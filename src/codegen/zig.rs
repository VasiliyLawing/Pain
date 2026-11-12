use crate::ast::{IntRepr, PrimType, SentinelVal};
use crate::codegen::{CodeBuffer, CodeGenerator};
use crate::ir::*;

pub struct ZigGenerator;

impl CodeGenerator for ZigGenerator {
    fn generate(&self, schema: &SchemaIr) -> String {
        let mut b = CodeBuffer::new();
        b.raw(ZIG_PREAMBLE);
        b.line("");

        if let Some(frame) = &schema.frame {
            emit_frame(&mut b, frame);
            b.line("");
        }

        for e in &schema.enums {
            emit_enum(&mut b, e);
            b.line("");
        }

        for c in &schema.choices {
            emit_choice(&mut b, c);
            b.line("");
        }

        for s in &schema.structs {
            emit_record(&mut b, s);
            b.line("");
        }

        for m in &schema.messages {
            emit_record(&mut b, m);
            b.line("");
        }

        b.finish()
    }
}

fn emit_frame(b: &mut CodeBuffer, f: &FrameIr) {
    let tag_enum = &f.tag_enum;
    let max = f.max_frame_bytes;
    b.block(&format!("pub const {} = struct", f.name), |b| {
        b.line(&format!("pub const Decoded = struct {{ tag: {tag_enum}, body: []const u8 }};"));
        b.line("");
        b.block(
            "pub fn encodeFrame(allocator: std.mem.Allocator, payload: []const u8) WireError![]u8",
            |b| {
                b.line(&format!(
                    "if (payload.len < 1 or payload.len > {max}) return error.FrameLengthOutOfRange;"
                ));
                b.line("var w = WireWriter.init(allocator);");
                b.line("errdefer w.deinit();");
                match f.length_repr {
                    IntRepr::U16Be => b.line("try w.writeU16Be(@intCast(payload.len));"),
                    IntRepr::U32Be => b.line("try w.writeU32Be(@intCast(payload.len));"),
                    IntRepr::U32Le => b.line("try w.writeU32Le(@intCast(payload.len));"),
                    _ => b.line("try w.writeU32Be(@intCast(payload.len));"),
                }
                b.line("try w.writeRaw(payload);");
                b.line("return w.toOwnedSlice();");
            },
        );
        b.line("");
        b.block(
            "pub fn decodeFrame(buf: []const u8) WireError!Decoded",
            |b| {
                b.line("var r = WireReader.init(buf);");
                let len_expr = match f.length_repr {
                    IntRepr::U16Be => "@as(usize, try r.readU16Be())",
                    IntRepr::U32Be => "@as(usize, try r.readU32Be())",
                    IntRepr::U32Le => "@as(usize, try r.readU32Le())",
                    _ => "@as(usize, try r.readU32Be())",
                };
                b.line(&format!("const length = {len_expr};"));
                b.line(&format!(
                    "if (length < 1 or length > {max} or r.remaining() < length) return error.FrameLengthOutOfRange;"
                ));
                b.line("const start = r.pos;");
                b.line(&format!("const tag = try {tag_enum}.fromWire(buf[start]);"));
                b.line("return Decoded{ .tag = tag, .body = buf[start + 1 .. start + length] };");
            },
        );
    });
    b.line(";");
}

fn emit_enum(b: &mut CodeBuffer, e: &EnumIr) {
    let repr_ty = zig_int_repr_type(e.repr);
    b.block(&format!("pub const {} = enum({repr_ty})", e.name), |b| {
        for (vname, disc) in &e.variants {
            b.line(&format!("{} = {disc},", zig_ident(vname)));
        }
        b.line("");
        b.block(&format!("pub fn toWire(self: {}) {repr_ty}", e.name), |b| {
            b.line("return @intFromEnum(self);");
        });
        b.line("");
        b.block(
            &format!("pub fn fromWire(v: {repr_ty}) WireError!{}", e.name),
            |b| {
                b.block("return switch (v)", |b| {
                    for (vname, disc) in &e.variants {
                        b.line(&format!("{disc} => .{},", zig_ident(vname)));
                    }
                    b.line("else => error.InvalidEnumVariant,");
                });
                b.line(";");
            },
        );
    });
    b.line(";");
}

fn emit_choice(b: &mut CodeBuffer, c: &ChoiceIr) {
    for v in &c.variants {
        emit_record(b, &v.record);
        b.line("");
    }

    b.block(&format!("pub const {} = union(enum)", c.name), |b| {
        for v in &c.variants {
            b.line(&format!("{}: {},", zig_ident(&v.name), v.record.name));
        }
        b.line("");
        b.block(
            &format!(
                "pub fn encodeWire(self: {}, w: *WireWriter) WireError!void",
                c.name
            ),
            |b| {
                b.block("switch (self)", |b| {
                    for v in &c.variants {
                        let write_tag = write_int_repr(c.tag_repr, &v.discriminant.to_string());
                        b.line(&format!(".{} => |val| {{", zig_ident(&v.name)));
                        b.indent();
                        b.line(&format!("try {write_tag};"));
                        b.line("try val.encodeWire(w);");
                        b.dedent();
                        b.line("},");
                    }
                });
            },
        );
        b.line("");
        b.block(
            &format!(
                "pub fn decodeWire(r: *WireReader, allocator: std.mem.Allocator) WireError!{}",
                c.name
            ),
            |b| {
                b.line(&format!("const tag = try {};", read_int_repr(c.tag_repr)));
                b.block("return switch (tag)", |b| {
                    for v in &c.variants {
                        b.line(&format!(
                            "{} => .{{ .{} = try {}.decodeWire(r, allocator) }},",
                            v.discriminant,
                            zig_ident(&v.name),
                            v.record.name
                        ));
                    }
                    b.line("else => error.InvalidChoiceTag,");
                });
                b.line(";");
            },
        );
    });
    b.line(";");
}

fn emit_record(b: &mut CodeBuffer, r: &RecordIr) {
    b.block(&format!("pub const {} = struct", r.name), |b| {
        for f in &r.fields {
            b.line(&format!("{}: {},", zig_ident(&f.name), zig_type(&f.ty)));
        }
        if !r.fields.is_empty() {
            b.line("");
        }

        b.block(
            &format!(
                "pub fn encodeWire(self: {}, w: *WireWriter) WireError!void",
                r.name
            ),
            |b| {
                if r.fields.is_empty() {
                    b.line("_ = self;");
                }
                if r.ops.is_empty() {
                    b.line("_ = w;");
                }
                for op in &r.ops {
                    emit_encode_op(b, op);
                }
            },
        );
        b.line("");

        if let Some(tag_byte) = r.tag_byte {
            let body_ops = &r.ops[1..];
            b.block(
                &format!(
                    "pub fn decodeBody(r: *WireReader, allocator: std.mem.Allocator) WireError!{}",
                    r.name
                ),
                |b| {
                    if body_ops.is_empty() {
                        b.line("_ = r;");
                    }
                    if !ops_use_allocator(body_ops) {
                        b.line("_ = allocator;");
                    }
                    for op in body_ops {
                        emit_decode_op(b, op);
                    }
                    if r.fields.is_empty() {
                        b.line(&format!("return {}{{}};", r.name));
                    } else {
                        b.block(&format!("return {}", r.name), |b| {
                            for f in &r.fields {
                                let id = zig_ident(&f.name);
                                b.line(&format!(".{id} = f_{},", f.name));
                            }
                        });
                        b.line(";");
                    }
                },
            );
            b.line("");
            b.block(
                &format!(
                    "pub fn decodeWire(r: *WireReader, allocator: std.mem.Allocator) WireError!{}",
                    r.name
                ),
                |b| {
                    b.line(&format!(
                        "if ((try r.readU8()) != {tag_byte:#04x}) return error.UnexpectedMessageTag;"
                    ));
                    b.line("return decodeBody(r, allocator);");
                },
            );
            b.line("");
            b.block(
                &format!(
                    "pub fn toBytes(self: {}, allocator: std.mem.Allocator) WireError![]u8",
                    r.name
                ),
                |b| {
                    b.line("var w = WireWriter.init(allocator);");
                    b.line("errdefer w.deinit();");
                    b.line("try self.encodeWire(&w);");
                    b.line("return w.toOwnedSlice();");
                },
            );
            b.line("");
            b.block(
                &format!(
                    "pub fn fromBytes(buf: []const u8, allocator: std.mem.Allocator) WireError!{}",
                    r.name
                ),
                |b| {
                    b.line("var r = WireReader.init(buf);");
                    b.line("return decodeWire(&r, allocator);");
                },
            );
            b.line("");
            b.block(
                &format!(
                    "pub fn fromBody(buf: []const u8, allocator: std.mem.Allocator) WireError!{}",
                    r.name
                ),
                |b| {
                    b.line("var r = WireReader.init(buf);");
                    b.line("return decodeBody(&r, allocator);");
                },
            );
        } else {
            b.block(
                &format!(
                    "pub fn decodeWire(r: *WireReader, allocator: std.mem.Allocator) WireError!{}",
                    r.name
                ),
                |b| {
                    if r.ops.is_empty() {
                        b.line("_ = r;");
                    }
                    if !ops_use_allocator(&r.ops) {
                        b.line("_ = allocator;");
                    }
                    for op in &r.ops {
                        emit_decode_op(b, op);
                    }
                    if r.fields.is_empty() {
                        b.line(&format!("return {}{{}};", r.name));
                    } else {
                        b.block(&format!("return {}", r.name), |b| {
                            for f in &r.fields {
                                let id = zig_ident(&f.name);
                                b.line(&format!(".{id} = f_{},", f.name));
                            }
                        });
                        b.line(";");
                    }
                },
            );
        }
    });
    b.line(";");
}

fn emit_encode_op(b: &mut CodeBuffer, op: &WireOp) {
    match op {
        WireOp::MessageTag { byte, .. } => {
            b.line(&format!("try w.writeU8({byte:#04x});"));
        }
        WireOp::BitflagsHeader { var_name, links } => {
            let kw = if links.is_empty() { "const" } else { "var" };
            b.line(&format!("{kw} flag_{var_name}: u8 = 0;"));
            for link in links {
                let id = zig_ident(&link.field_name);
                let cond = match link.kind {
                    BitflagBindingKind::InlinedBool => format!("self.{id}"),
                    BitflagBindingKind::OptionalValue => format!("self.{id} != null"),
                    BitflagBindingKind::OptionalList => {
                        format!("if (self.{id}) | s | s.len > 0 else false")
                    }
                };
                b.line(&format!(
                    "if ({cond}) flag_{var_name} |= {:#04x};",
                    link.mask
                ));
            }
            b.line(&format!("try w.writeU8(flag_{var_name});"));
        }
        WireOp::Field {
            field_name,
            encoding,
        } => {
            let id = zig_ident(field_name);
            emit_encode_field(b, &format!("self.{id}"), encoding);
        }
        WireOp::FlagGuarded {
            field_name,
            encoding,
            ..
        } => {
            let id = zig_ident(field_name);
            match encoding {
                GuardedEncoding::Scalar(scalar) => {
                    b.block(&format!("if (self.{id}) |val|"), |b| {
                        emit_encode_scalar(b, "val", scalar);
                    });
                }
                GuardedEncoding::List { prefix, item, .. } => {
                    b.block(&format!("if (self.{id}) |items|"), |b| {
                        b.block("if (items.len > 0)", |b| {
                            b.line(&format!("try {};", write_int_repr(*prefix, "items.len")));
                            b.block("for (items) |item|", |b| {
                                emit_encode_scalar(b, "item", item);
                            });
                        });
                    });
                }
            }
        }
        WireOp::TailGroup { fields } => {
            let conds: Vec<String> = fields
                .iter()
                .map(|tf| tail_field_active_expr(&zig_ident(&tf.field_name), &tf.encoding))
                .collect();
            b.block(&format!("if ({})", conds.join(" or ")), |b| {
                for tf in fields {
                    let id = zig_ident(&tf.field_name);
                    emit_encode_field(b, &format!("self.{id}"), &tf.encoding);
                }
            });
        }
    }
}

fn emit_encode_field(b: &mut CodeBuffer, access: &str, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => emit_encode_scalar(b, access, scalar),
        FieldEncoding::SentinelOptional { inner, sentinel } => match sentinel {
            SentinelVal::EmptyString => {
                b.line(&format!("try w.writeStr({access} orelse \"\");"));
            }
            SentinelVal::EmptyGuid => {
                b.line(&format!("try w.writeStr({access} orelse EMPTY_GUID);"));
            }
            SentinelVal::Zero => {
                let expr = format!("({access} orelse 0)");
                let stmt = match inner {
                    ScalarKind::Prim(p) => write_prim(*p, &expr),
                    _ => format!("w.writeVarint({expr})"),
                };
                b.line(&format!("try {stmt};"));
            }
        },
        FieldEncoding::List { prefix, item, .. } => {
            b.line(&format!(
                "try {};",
                write_int_repr(*prefix, &format!("{access}.len"))
            ));
            b.block(&format!("for ({access}) |item|"), |b| {
                emit_encode_scalar(b, "item", item);
            });
        }
    }
}

fn emit_encode_scalar(b: &mut CodeBuffer, expr: &str, scalar: &ScalarKind) {
    match scalar {
        ScalarKind::Prim(p) => b.line(&format!("try {};", write_prim(*p, expr))),
        ScalarKind::BytesWithPrefix { prefix, .. } => {
            b.line(&format!(
                "try {};",
                write_int_repr(*prefix, &format!("{expr}.len"))
            ));
            b.line(&format!("try w.writeRaw({expr});"));
        }
        ScalarKind::Enum { repr, .. } => {
            b.line(&format!(
                "try {};",
                write_int_repr(*repr, &format!("{expr}.toWire()"))
            ));
        }
        ScalarKind::Struct(_) | ScalarKind::Choice(_) => {
            b.line(&format!("try {expr}.encodeWire(w);"));
        }
    }
}

fn emit_decode_op(b: &mut CodeBuffer, op: &WireOp) {
    match op {
        WireOp::MessageTag { byte, .. } => {
            b.line(&format!(
                "if ((try r.readU8()) != {byte:#04x}) return error.UnexpectedMessageTag;"
            ));
        }
        WireOp::BitflagsHeader { var_name, links } => {
            if links.is_empty() {
                b.line("_ = try r.readU8();");
            } else {
                b.line(&format!("const flag_{var_name} = try r.readU8();"));
                for link in links {
                    if link.kind == BitflagBindingKind::InlinedBool {
                        b.line(&format!(
                            "const f_{} = (flag_{var_name} & {:#04x}) != 0;",
                            link.field_name, link.mask
                        ));
                    }
                }
            }
        }
        WireOp::Field {
            field_name,
            encoding,
        } => {
            emit_decode_field_binding(b, &format!("f_{field_name}"), encoding);
        }
        WireOp::FlagGuarded {
            flags_var,
            mask,
            field_name,
            encoding,
        } => {
            let var_name = format!("f_{field_name}");
            let ty = match encoding {
                GuardedEncoding::Scalar(s) => format!("?{}", zig_scalar_type(s)),
                GuardedEncoding::List { item, .. } => format!("?[]const {}", zig_scalar_type(item)),
            };
            b.line(&format!(
                "const {var_name}: {ty} = if ((flag_{flags_var} & {mask:#04x}) != 0) blk: {{"
            ));
            b.indent();
            match encoding {
                GuardedEncoding::Scalar(scalar) => {
                    b.line(&format!("break :blk {};", decode_scalar_expr(scalar)));
                }
                GuardedEncoding::List {
                    prefix,
                    max_count,
                    item,
                } => {
                    emit_decode_list_block(b, *prefix, *max_count, item, "blk");
                }
            }
            b.dedent();
            b.line("} else null;");
        }
        WireOp::TailGroup { fields } => {
            for tf in fields {
                let var_name = format!("f_{}", tf.field_name);
                let ty = zig_field_encoding_type(&tf.encoding);
                let def = tail_field_default_expr(&tf.encoding);
                b.line(&format!(
                    "const {var_name}: {ty} = if (!r.isAtEnd()) blk: {{"
                ));
                b.indent();
                emit_decode_field_blk(b, &tf.encoding, "blk");
                b.dedent();
                b.line(&format!("}} else {def};"));
            }
        }
    }
}

fn emit_decode_field_binding(b: &mut CodeBuffer, var_name: &str, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => {
            b.line(&format!(
                "const {var_name} = {};",
                decode_scalar_expr(scalar)
            ));
        }
        FieldEncoding::SentinelOptional { inner, sentinel } => {
            let ty = zig_scalar_type(inner);
            b.line(&format!("const {var_name}: ?{ty} = blk: {{"));
            b.indent();
            b.line(&format!("const raw = {};", decode_scalar_expr(inner)));
            let cond = match sentinel {
                SentinelVal::EmptyString => "raw.len > 0",
                SentinelVal::EmptyGuid => "raw.len > 0 and !std.mem.eql(u8, raw, EMPTY_GUID)",
                SentinelVal::Zero => "raw != 0",
            };
            b.line(&format!("break :blk if ({cond}) raw else null;"));
            b.dedent();
            b.line("};");
        }
        FieldEncoding::List {
            prefix,
            max_count,
            item,
        } => {
            let item_ty = zig_scalar_type(item);
            b.line(&format!("const {var_name}: []const {item_ty} = blk: {{"));
            b.indent();
            emit_decode_list_block(b, *prefix, *max_count, item, "blk");
            b.dedent();
            b.line("};");
        }
    }
}

fn emit_decode_field_blk(b: &mut CodeBuffer, encoding: &FieldEncoding, label: &str) {
    match encoding {
        FieldEncoding::Scalar(scalar) => {
            b.line(&format!("break :{label} {};", decode_scalar_expr(scalar)));
        }
        FieldEncoding::SentinelOptional { inner, sentinel } => {
            b.line(&format!("const raw = {};", decode_scalar_expr(inner)));
            let cond = match sentinel {
                SentinelVal::EmptyString => "raw.len > 0",
                SentinelVal::EmptyGuid => "raw.len > 0 and !std.mem.eql(u8, raw, EMPTY_GUID)",
                SentinelVal::Zero => "raw != 0",
            };
            b.line(&format!("break :{label} if ({cond}) raw else null;"));
        }
        FieldEncoding::List {
            prefix,
            max_count,
            item,
        } => {
            emit_decode_list_block(b, *prefix, *max_count, item, label);
        }
    }
}

fn emit_decode_list_block(
    b: &mut CodeBuffer,
    prefix: IntRepr,
    max_count: Option<usize>,
    item: &ScalarKind,
    label: &str,
) {
    let raw_count = read_int_repr(prefix);
    b.line(&format!("const count: usize = @intCast(try {raw_count});"));
    if let Some(max) = max_count {
        b.line(&format!(
            "if (count > {max} or count > r.remaining()) return error.CountExceedsMax;"
        ));
    } else {
        b.line("if (count > r.remaining()) return error.CountExceedsMax;");
    }
    let item_ty = zig_scalar_type(item);
    b.line(&format!(
        "const items = try allocator.alloc({item_ty}, count);"
    ));
    b.block("for (0..count) |i|", |b| {
        b.line(&format!("items[i] = {};", decode_scalar_expr(item)));
    });
    b.line(&format!("break :{label} items;"));
}

fn decode_scalar_expr(scalar: &ScalarKind) -> String {
    match scalar {
        ScalarKind::Prim(p) => match p {
            PrimType::U8 => "try r.readU8()".into(),
            PrimType::U16Be => "try r.readU16Be()".into(),
            PrimType::U16Le => "try r.readU16Le()".into(),
            PrimType::U32Be => "try r.readU32Be()".into(),
            PrimType::U32Le => "try r.readU32Le()".into(),
            PrimType::U64Be => "try r.readU64Be()".into(),
            PrimType::U64Le => "try r.readU64Le()".into(),
            PrimType::Varint => "try r.readVarint()".into(),
            PrimType::Bool => "try r.readBool()".into(),
            PrimType::Bytes => "try r.readBytes()".into(),
            PrimType::String | PrimType::Guid => "try r.readStr()".into(),
        },
        ScalarKind::BytesWithPrefix { prefix, max_len } => {
            let len_expr = format!("@as(usize, @intCast(try {}))", read_int_repr(*prefix));
            if let Some(max) = max_len {
                format!("try r.readRawBounded({len_expr}, {max})")
            } else {
                format!("try r.readRaw({len_expr})")
            }
        }
        ScalarKind::Enum { name, repr } => {
            format!("try {name}.fromWire(try {})", read_int_repr(*repr))
        }
        ScalarKind::Struct(name) | ScalarKind::Choice(name) => {
            format!("try {name}.decodeWire(r, allocator)")
        }
    }
}

fn tail_field_active_expr(field_id: &str, encoding: &FieldEncoding) -> String {
    match encoding {
        FieldEncoding::SentinelOptional { .. } => format!("self.{field_id} != null"),
        FieldEncoding::List { .. } => format!("self.{field_id}.len > 0"),
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bool)) => format!("self.{field_id}"),
        FieldEncoding::Scalar(ScalarKind::Prim(
            PrimType::String | PrimType::Guid | PrimType::Bytes,
        )) => format!("self.{field_id}.len > 0"),
        FieldEncoding::Scalar(_) => format!("self.{field_id} != 0"),
    }
}

fn tail_field_default_expr(encoding: &FieldEncoding) -> &'static str {
    match encoding {
        FieldEncoding::SentinelOptional { .. } => "null",
        FieldEncoding::List { .. } => "&.{}",
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bool)) => "false",
        FieldEncoding::Scalar(ScalarKind::Prim(
            PrimType::String | PrimType::Guid | PrimType::Bytes,
        )) => "\"\"",
        FieldEncoding::Scalar(_) => "0",
    }
}

fn zig_field_encoding_type(encoding: &FieldEncoding) -> String {
    match encoding {
        FieldEncoding::Scalar(s) => zig_scalar_type(s),
        FieldEncoding::SentinelOptional { inner, .. } => format!("?{}", zig_scalar_type(inner)),
        FieldEncoding::List { item, .. } => format!("[]const {}", zig_scalar_type(item)),
    }
}

fn zig_type(ty: &IrType) -> String {
    match ty {
        IrType::Scalar(s) => zig_scalar_type(s),
        IrType::Optional(inner) => format!("?{}", zig_type(inner)),
        IrType::List(item) => format!("[]const {}", zig_scalar_type(item)),
    }
}

fn zig_scalar_type(s: &ScalarKind) -> String {
    match s {
        ScalarKind::Prim(p) => match p {
            PrimType::U8 => "u8".into(),
            PrimType::U16Be | PrimType::U16Le => "u16".into(),
            PrimType::U32Be | PrimType::U32Le => "u32".into(),
            PrimType::U64Be | PrimType::U64Le | PrimType::Varint => "u64".into(),
            PrimType::Bool => "bool".into(),
            PrimType::Bytes | PrimType::String | PrimType::Guid => "[]const u8".into(),
        },
        ScalarKind::BytesWithPrefix { .. } => "[]const u8".into(),
        ScalarKind::Enum { name, .. } | ScalarKind::Struct(name) | ScalarKind::Choice(name) => {
            name.clone()
        }
    }
}

fn zig_int_repr_type(repr: IntRepr) -> &'static str {
    match repr {
        IntRepr::U8 => "u8",
        IntRepr::U16Be | IntRepr::U16Le => "u16",
        IntRepr::U32Be | IntRepr::U32Le => "u32",
        IntRepr::U64Be | IntRepr::U64Le | IntRepr::Varint => "u64",
    }
}

fn write_int_repr(repr: IntRepr, expr: &str) -> String {
    match repr {
        IntRepr::U8 => format!("w.writeU8(@intCast({expr}))"),
        IntRepr::U16Be => format!("w.writeU16Be(@intCast({expr}))"),
        IntRepr::U16Le => format!("w.writeU16Le(@intCast({expr}))"),
        IntRepr::U32Be => format!("w.writeU32Be(@intCast({expr}))"),
        IntRepr::U32Le => format!("w.writeU32Le(@intCast({expr}))"),
        IntRepr::U64Be => format!("w.writeU64Be(@intCast({expr}))"),
        IntRepr::U64Le => format!("w.writeU64Le(@intCast({expr}))"),
        IntRepr::Varint => format!("w.writeVarint(@intCast({expr}))"),
    }
}

fn read_int_repr(repr: IntRepr) -> &'static str {
    match repr {
        IntRepr::U8 => "r.readU8()",
        IntRepr::U16Be => "r.readU16Be()",
        IntRepr::U16Le => "r.readU16Le()",
        IntRepr::U32Be => "r.readU32Be()",
        IntRepr::U32Le => "r.readU32Le()",
        IntRepr::U64Be => "r.readU64Be()",
        IntRepr::U64Le => "r.readU64Le()",
        IntRepr::Varint => "r.readVarint()",
    }
}

fn write_prim(p: PrimType, expr: &str) -> String {
    match p {
        PrimType::U8 => format!("w.writeU8({expr})"),
        PrimType::U16Be => format!("w.writeU16Be({expr})"),
        PrimType::U16Le => format!("w.writeU16Le({expr})"),
        PrimType::U32Be => format!("w.writeU32Be({expr})"),
        PrimType::U32Le => format!("w.writeU32Le({expr})"),
        PrimType::U64Be => format!("w.writeU64Be({expr})"),
        PrimType::U64Le => format!("w.writeU64Le({expr})"),
        PrimType::Varint => format!("w.writeVarint({expr})"),
        PrimType::Bool => format!("w.writeBool({expr})"),
        PrimType::Bytes => format!("w.writeBytes({expr})"),
        PrimType::String | PrimType::Guid => format!("w.writeStr({expr})"),
    }
}

fn ops_use_allocator(ops: &[WireOp]) -> bool {
    ops.iter().any(|op| match op {
        WireOp::MessageTag { .. } | WireOp::BitflagsHeader { .. } => false,
        WireOp::Field { encoding, .. } => field_encoding_uses_allocator(encoding),
        WireOp::FlagGuarded { encoding, .. } => match encoding {
            GuardedEncoding::Scalar(s) => scalar_uses_allocator(s),
            GuardedEncoding::List { .. } => true,
        },
        WireOp::TailGroup { fields } => fields
            .iter()
            .any(|tf| field_encoding_uses_allocator(&tf.encoding)),
    })
}

fn field_encoding_uses_allocator(encoding: &FieldEncoding) -> bool {
    match encoding {
        FieldEncoding::Scalar(s) | FieldEncoding::SentinelOptional { inner: s, .. } => {
            scalar_uses_allocator(s)
        }
        FieldEncoding::List { .. } => true,
    }
}

fn scalar_uses_allocator(scalar: &ScalarKind) -> bool {
    matches!(scalar, ScalarKind::Struct(_) | ScalarKind::Choice(_))
}

fn zig_ident(name: &str) -> String {
    match name {
        "type" | "error" | "test" | "fn" | "var" | "const" | "pub" | "return" | "struct"
        | "enum" | "union" | "switch" | "align" | "opaque" => format!("@\"{name}\""),
        other => other.to_string(),
    }
}

const ZIG_PREAMBLE: &str = r#"const std = @import("std");

pub const EMPTY_GUID: []const u8 = "00000000-0000-0000-0000-000000000000";

pub const WireError = error{
    OutOfMemory,
    BufferOverflow,
    UnexpectedEof,
    VarintOverflow,
    InvalidEnumVariant,
    InvalidChoiceTag,
    UnexpectedMessageTag,
    CountExceedsMax,
    FrameLengthOutOfRange,
};

pub const WireWriter = struct {
    buf: ?std.ArrayList(u8) = null,
    fixed: ?[]u8 = null,
    position: usize = 0,
    allocator: std.mem.Allocator = undefined,

    pub fn init(allocator: std.mem.Allocator) WireWriter {
        return .{ .buf = std.ArrayList(u8).empty, .fixed = null, .position = 0, .allocator = allocator };
    }

    pub fn initBuffer(buffer: []u8) WireWriter {
        return .{ .buf = null, .fixed = buffer, .position = 0 };
    }

    pub fn deinit(self: *WireWriter) void {
        if (self.buf) |*b| b.deinit(self.allocator);
    }

    pub fn toOwnedSlice(self: *WireWriter) WireError![]u8 {
        if (self.buf) |*b| return b.toOwnedSlice(self.allocator);
        return error.OutOfMemory;
    }

    pub fn written(self: *const WireWriter) []const u8 {
        if (self.buf) |*b| return b.items;
        if (self.fixed) |f| return f[0..self.position];
        return &.{};
    }

    pub fn writeU8(self: *WireWriter, v: u8) WireError!void {
        if (self.buf) |*b| {
            try b.append(self.allocator, v);
            self.position = b.items.len;
        } else if (self.fixed) |f| {
            if (self.position >= f.len) return error.BufferOverflow;
            f[self.position] = v;
            self.position += 1;
        }
    }

    pub fn writeU16Be(self: *WireWriter, v: u16) WireError!void {
        var b: [2]u8 = undefined;
        std.mem.writeInt(u16, &b, v, .big);
        try self.writeRaw(&b);
    }

    pub fn writeU16Le(self: *WireWriter, v: u16) WireError!void {
        var b: [2]u8 = undefined;
        std.mem.writeInt(u16, &b, v, .little);
        try self.writeRaw(&b);
    }

    pub fn writeU32Be(self: *WireWriter, v: u32) WireError!void {
        var b: [4]u8 = undefined;
        std.mem.writeInt(u32, &b, v, .big);
        try self.writeRaw(&b);
    }

    pub fn writeU32Le(self: *WireWriter, v: u32) WireError!void {
        var b: [4]u8 = undefined;
        std.mem.writeInt(u32, &b, v, .little);
        try self.writeRaw(&b);
    }

    pub fn writeU64Be(self: *WireWriter, v: u64) WireError!void {
        var b: [8]u8 = undefined;
        std.mem.writeInt(u64, &b, v, .big);
        try self.writeRaw(&b);
    }

    pub fn writeU64Le(self: *WireWriter, v: u64) WireError!void {
        var b: [8]u8 = undefined;
        std.mem.writeInt(u64, &b, v, .little);
        try self.writeRaw(&b);
    }

    pub fn writeVarint(self: *WireWriter, v_in: u64) WireError!void {
        var v = v_in;
        while (v > 0x7F) {
            try self.writeU8(@as(u8, @intCast(v & 0x7F)) | 0x80);
            v >>= 7;
        }
        try self.writeU8(@as(u8, @intCast(v)));
    }

    pub fn writeBool(self: *WireWriter, v: bool) WireError!void {
        try self.writeU8(if (v) 1 else 0);
    }

    pub fn writeRaw(self: *WireWriter, data: []const u8) WireError!void {
        if (self.buf) |*b| {
            try b.appendSlice(self.allocator, data);
            self.position = b.items.len;
        } else if (self.fixed) |f| {
            if (self.position + data.len > f.len) return error.BufferOverflow;
            @memcpy(f[self.position .. self.position + data.len], data);
            self.position += data.len;
        }
    }

    pub fn writeBytes(self: *WireWriter, data: []const u8) WireError!void {
        try self.writeVarint(@intCast(data.len));
        try self.writeRaw(data);
    }

    pub fn writeStr(self: *WireWriter, s: []const u8) WireError!void {
        try self.writeBytes(s);
    }
};

pub const WireReader = struct {
    buf: []const u8,
    pos: usize = 0,

    pub fn init(buf: []const u8) WireReader {
        return .{ .buf = buf, .pos = 0 };
    }

    pub fn remaining(self: WireReader) usize {
        return self.buf.len -| self.pos;
    }

    pub fn isAtEnd(self: WireReader) bool {
        return self.pos >= self.buf.len;
    }

    pub fn readU8(self: *WireReader) WireError!u8 {
        if (self.pos >= self.buf.len) return error.UnexpectedEof;
        const b = self.buf[self.pos];
        self.pos += 1;
        return b;
    }

    pub fn readU16Be(self: *WireReader) WireError!u16 {
        const s = try self.readRaw(2);
        return std.mem.readInt(u16, s[0..2], .big);
    }

    pub fn readU16Le(self: *WireReader) WireError!u16 {
        const s = try self.readRaw(2);
        return std.mem.readInt(u16, s[0..2], .little);
    }

    pub fn readU32Be(self: *WireReader) WireError!u32 {
        const s = try self.readRaw(4);
        return std.mem.readInt(u32, s[0..4], .big);
    }

    pub fn readU32Le(self: *WireReader) WireError!u32 {
        const s = try self.readRaw(4);
        return std.mem.readInt(u32, s[0..4], .little);
    }

    pub fn readU64Be(self: *WireReader) WireError!u64 {
        const s = try self.readRaw(8);
        return std.mem.readInt(u64, s[0..8], .big);
    }

    pub fn readU64Le(self: *WireReader) WireError!u64 {
        const s = try self.readRaw(8);
        return std.mem.readInt(u64, s[0..8], .little);
    }

    pub fn readVarint(self: *WireReader) WireError!u64 {
        var result: u64 = 0;
        var shift: u6 = 0;
        var i: usize = 0;
        while (i < 10) : (i += 1) {
            const b = try self.readU8();
            result |= (@as(u64, b & 0x7F) << shift);
            if ((b & 0x80) == 0) return result;
            if (i < 9) shift += 7;
        }
        return error.VarintOverflow;
    }

    pub fn readBool(self: *WireReader) WireError!bool {
        return (try self.readU8()) == 1;
    }

    pub fn readRaw(self: *WireReader, len: usize) WireError![]const u8 {
        if (len > self.remaining()) return error.UnexpectedEof;
        const slice = self.buf[self.pos .. self.pos + len];
        self.pos += len;
        return slice;
    }

    pub fn readRawBounded(self: *WireReader, len: usize, max: usize) WireError![]const u8 {
        if (len > max) return error.CountExceedsMax;
        return self.readRaw(len);
    }

    pub fn readBytes(self: *WireReader) WireError![]const u8 {
        const len: usize = @intCast(try self.readVarint());
        return self.readRaw(len);
    }

    pub fn readStr(self: *WireReader) WireError![]const u8 {
        return self.readBytes();
    }
};
"#;
