use crate::ast::{IntRepr, PrimType, SentinelVal};
use crate::codegen::{CodeBuffer, CodeGenerator};
use crate::ir::*;

pub struct RustGenerator;

impl CodeGenerator for RustGenerator {
    fn generate(&self, schema: &SchemaIr) -> String {
        let mut b = CodeBuffer::new();
        b.raw(RUST_PREAMBLE);
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
    b.block(
        "pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, WireError>",
        |b| {
            b.line(&format!(
                "if payload.is_empty() || payload.len() > {max} {{ return Err(WireError::FrameLengthOutOfRange); }}"
            ));
            b.line("let mut w = WireWriter::with_capacity(4 + payload.len());");
            match f.length_repr {
                IntRepr::U16Be => b.line("w.write_u16be(payload.len() as u16);"),
                IntRepr::U32Be => b.line("w.write_u32be(payload.len() as u32);"),
                IntRepr::U32Le => b.line("w.write_u32le(payload.len() as u32);"),
                _ => b.line("w.write_u32be(payload.len() as u32);"),
            }
            b.line("w.write_raw(payload);");
            b.line("Ok(w.into_vec())");
        },
    );
    b.line("");
    b.block(
        &format!("pub fn decode_frame(buf: &[u8]) -> Result<({tag_enum}, &[u8]), WireError>"),
        |b| {
            b.line("let mut r = WireReader::new(buf);");
            let len_expr = match f.length_repr {
                IntRepr::U16Be => "r.read_u16be()? as usize",
                IntRepr::U32Be => "r.read_u32be()? as usize",
                IntRepr::U32Le => "r.read_u32le()? as usize",
                _ => "r.read_u32be()? as usize",
            };
            b.line(&format!("let len = {len_expr};"));
            b.line(&format!(
                "if !(1..={max}).contains(&len) || r.remaining() < len {{ return Err(WireError::FrameLengthOutOfRange); }}"
            ));
            b.line("let start = r.position();");
            b.line(&format!("let tag = {tag_enum}::from_wire(buf[start])?;"));
            b.line("Ok((tag, &buf[start + 1..start + len]))");
        },
    );
}

fn emit_enum(b: &mut CodeBuffer, e: &EnumIr) {
    let repr_ty = int_repr_type(e.repr);
    b.line("#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
    b.block(&format!("pub enum {}", e.name), |b| {
        for (vname, disc) in &e.variants {
            b.line(&format!("{vname} = {disc},"));
        }
    });
    b.line("");
    b.block(&format!("impl {}", e.name), |b| {
        b.block(&format!("pub fn to_wire(self) -> {repr_ty}"), |b| {
            b.block("match self", |b| {
                for (vname, disc) in &e.variants {
                    b.line(&format!("Self::{vname} => {disc},"));
                }
            });
        });
        b.line("");
        b.block(
            &format!("pub fn from_wire(v: {repr_ty}) -> Result<Self, WireError>"),
            |b| {
                b.block("match v", |b| {
                    for (vname, disc) in &e.variants {
                        b.line(&format!("{disc} => Ok(Self::{vname}),"));
                    }
                    b.line("_ => Err(WireError::InvalidEnumVariant),");
                });
            },
        );
    });
}

fn emit_choice(b: &mut CodeBuffer, c: &ChoiceIr) {
    for v in &c.variants {
        if !v.record.fields.is_empty() {
            emit_record(b, &v.record);
            b.line("");
        }
    }

    b.line("#[derive(Debug, Clone, PartialEq, Eq)]");
    b.block(&format!("pub enum {}", c.name), |b| {
        for v in &c.variants {
            if v.record.fields.is_empty() {
                b.line(&format!("{},", v.name));
            } else {
                b.line(&format!("{}({}),", v.name, v.record.name));
            }
        }
    });
    b.line("");

    b.block(&format!("impl {}", c.name), |b| {
        b.block("pub fn encode_wire(&self, w: &mut WireWriter)", |b| {
            b.block("match self", |b| {
                for v in &c.variants {
                    let write_disc = write_int_repr(c.tag_repr, &v.discriminant.to_string());
                    if v.record.fields.is_empty() {
                        b.line(&format!("Self::{} => {{ {write_disc}; }}", v.name));
                    } else {
                        b.block(&format!("Self::{}(val) =>", v.name), |b| {
                            b.line(&format!("{write_disc};"));
                            b.line("val.encode_wire(w);");
                        });
                    }
                }
            });
        });
        b.line("");
        b.block(
            "pub fn decode_wire(r: &mut WireReader<'_>) -> Result<Self, WireError>",
            |b| {
                let read_disc = read_int_repr(c.tag_repr);
                b.line(&format!("let tag = {read_disc};"));
                b.block("match tag", |b| {
                    for v in &c.variants {
                        if v.record.fields.is_empty() {
                            b.line(&format!("{} => Ok(Self::{}),", v.discriminant, v.name));
                        } else {
                            b.line(&format!(
                                "{} => Ok(Self::{}({}::decode_wire(r)?)),",
                                v.discriminant, v.name, v.record.name
                            ));
                        }
                    }
                    b.line("_ => Err(WireError::InvalidChoiceTag),");
                });
            },
        );
    });
}

fn emit_record(b: &mut CodeBuffer, r: &RecordIr) {
    b.line("#[derive(Debug, Clone, PartialEq, Eq)]");
    if r.fields.is_empty() {
        b.line(&format!("pub struct {};", r.name));
    } else {
        b.block(&format!("pub struct {}", r.name), |b| {
            for f in &r.fields {
                b.line(&format!("pub {}: {},", f.name, rust_type(&f.ty)));
            }
        });
    }
    b.line("");

    b.block(&format!("impl {}", r.name), |b| {
        let w_param = if r.ops.is_empty() { "_w" } else { "w" };
        b.block(
            &format!("pub fn encode_wire(&self, {w_param}: &mut WireWriter)"),
            |b| {
                for op in &r.ops {
                    emit_encode_op(b, op);
                }
            },
        );
        b.line("");

        if let Some(tag_byte) = r.tag_byte {
            let body_ops = &r.ops[1..];
            let r_body_param = if body_ops.is_empty() { "_r" } else { "r" };
            b.block(
                &format!(
                    "pub fn decode_body({r_body_param}: &mut WireReader<'_>) -> Result<Self, WireError>"
                ),
                |b| {
                    for op in body_ops {
                        emit_decode_op(b, op);
                    }
                    if r.fields.is_empty() {
                        b.line("Ok(Self)");
                    } else {
                        b.block("Ok(Self", |b| {
                            for f in &r.fields {
                                b.line(&format!("{},", f.name));
                            }
                        });
                        b.line(")");
                    }
                },
            );
            b.line("");
            b.block(
                "pub fn decode_wire(r: &mut WireReader<'_>) -> Result<Self, WireError>",
                |b| {
                    b.line(&format!(
                        "if r.read_u8()? != {tag_byte:#04x} {{ return Err(WireError::UnexpectedMessageTag); }}"
                    ));
                    b.line("Self::decode_body(r)");
                },
            );
            b.line("");
            b.block("pub fn to_bytes(&self) -> Vec<u8>", |b| {
                b.line("let mut w = WireWriter::with_capacity(64);");
                b.line("self.encode_wire(&mut w);");
                b.line("w.into_vec()");
            });
            b.line("");
            b.block(
                "pub fn from_bytes(buf: &[u8]) -> Result<Self, WireError>",
                |b| {
                    b.line("let mut r = WireReader::new(buf);");
                    b.line("Self::decode_wire(&mut r)");
                },
            );
            b.line("");
            b.block(
                "pub fn from_body(buf: &[u8]) -> Result<Self, WireError>",
                |b| {
                    b.line("let mut r = WireReader::new(buf);");
                    b.line("Self::decode_body(&mut r)");
                },
            );
        } else {
            let r_param = if r.ops.is_empty() { "_r" } else { "r" };
            b.block(
                &format!(
                    "pub fn decode_wire({r_param}: &mut WireReader<'_>) -> Result<Self, WireError>"
                ),
                |b| {
                    for op in &r.ops {
                        emit_decode_op(b, op);
                    }
                    if r.fields.is_empty() {
                        b.line("Ok(Self)");
                    } else {
                        b.block("Ok(Self", |b| {
                            for f in &r.fields {
                                b.line(&format!("{},", f.name));
                            }
                        });
                        b.line(")");
                    }
                },
            );
        }
    });
}

fn emit_encode_op(b: &mut CodeBuffer, op: &WireOp) {
    match op {
        WireOp::MessageTag { byte, .. } => {
            b.line(&format!("w.write_u8({byte:#04x});"));
        }
        WireOp::BitflagsHeader { var_name, links } => {
            let mut_kw = if links.is_empty() { "" } else { "mut " };
            b.line(&format!("let {mut_kw}{var_name}: u8 = 0;"));
            for link in links {
                let cond = match link.kind {
                    BitflagBindingKind::InlinedBool => format!("self.{}", link.field_name),
                    BitflagBindingKind::OptionalValue => {
                        format!("self.{}.is_some()", link.field_name)
                    }
                    BitflagBindingKind::OptionalList => {
                        format!(
                            "self.{}.as_ref().is_some_and(|v| !v.is_empty())",
                            link.field_name
                        )
                    }
                };
                b.line(&format!(
                    "if {cond} {{ {var_name} |= {:#04x}; }}",
                    link.mask
                ));
            }
            b.line(&format!("w.write_u8({var_name});"));
        }
        WireOp::Field {
            field_name,
            encoding,
        } => {
            emit_encode_field(b, &format!("self.{field_name}"), encoding);
        }
        WireOp::FlagGuarded {
            field_name,
            encoding,
            ..
        } => match encoding {
            GuardedEncoding::Scalar(scalar) => {
                b.block(&format!("if let Some(val) = &self.{field_name}"), |b| {
                    emit_encode_scalar(b, "val", scalar, true);
                });
            }
            GuardedEncoding::List { prefix, item, .. } => {
                b.block(&format!("if let Some(items) = &self.{field_name}"), |b| {
                    b.block("if !items.is_empty()", |b| {
                        b.line(&format!("{};", write_int_repr(*prefix, "items.len()")));
                        b.block("for item in items", |b| {
                            emit_encode_scalar(b, "item", item, true);
                        });
                    });
                });
            }
        },
        WireOp::TailGroup { fields } => {
            let conds: Vec<String> = fields
                .iter()
                .map(|tf| tail_field_active_expr(&tf.field_name, &tf.encoding))
                .collect();
            b.block(&format!("if {}", conds.join(" || ")), |b| {
                for tf in fields {
                    emit_encode_field(b, &format!("self.{}", tf.field_name), &tf.encoding);
                }
            });
        }
    }
}

fn emit_encode_field(b: &mut CodeBuffer, access: &str, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => emit_encode_scalar(b, access, scalar, false),
        FieldEncoding::SentinelOptional { inner, sentinel } => match sentinel {
            SentinelVal::EmptyString => {
                b.line(&format!(
                    "w.write_str({access}.as_deref().unwrap_or(\"\"));"
                ));
            }
            SentinelVal::EmptyGuid => {
                b.line(&format!(
                    "w.write_str({access}.as_deref().unwrap_or(EMPTY_GUID));"
                ));
            }
            SentinelVal::Zero => {
                let write_stmt = match inner {
                    ScalarKind::Prim(PrimType::Varint) => {
                        format!("w.write_varint({access}.unwrap_or(0))")
                    }
                    ScalarKind::Prim(p) => write_prim(*p, &format!("{access}.unwrap_or(0)"), false),
                    _ => format!("w.write_varint({access}.unwrap_or(0))"),
                };
                b.line(&format!("{write_stmt};"));
            }
        },
        FieldEncoding::List { prefix, item, .. } => {
            b.line(&format!(
                "{};",
                write_int_repr(*prefix, &format!("{access}.len()"))
            ));
            b.block(&format!("for item in &{access}"), |b| {
                emit_encode_scalar(b, "item", item, true);
            });
        }
    }
}

fn emit_encode_scalar(b: &mut CodeBuffer, expr: &str, scalar: &ScalarKind, is_ref: bool) {
    match scalar {
        ScalarKind::Prim(p) => {
            b.line(&format!("{};", write_prim(*p, expr, is_ref)));
        }
        ScalarKind::BytesWithPrefix { prefix, .. } => {
            b.line(&format!(
                "{};",
                write_int_repr(*prefix, &format!("{expr}.len()"))
            ));
            let slice = if is_ref {
                expr.to_string()
            } else {
                format!("&{expr}")
            };
            b.line(&format!("w.write_raw({slice});"));
        }
        ScalarKind::Enum { repr, .. } => {
            let val = if is_ref {
                format!("(*{expr}).to_wire()")
            } else {
                format!("{expr}.to_wire()")
            };
            b.line(&format!("{};", write_int_repr(*repr, &val)));
        }
        ScalarKind::Struct(_) | ScalarKind::Choice(_) => {
            b.line(&format!("{expr}.encode_wire(w);"));
        }
    }
}

fn emit_decode_op(b: &mut CodeBuffer, op: &WireOp) {
    match op {
        WireOp::MessageTag { byte, .. } => {
            b.line(&format!(
                "if r.read_u8()? != {byte:#04x} {{ return Err(WireError::UnexpectedMessageTag); }}"
            ));
        }
        WireOp::BitflagsHeader { var_name, links } => {
            let used_later = links
                .iter()
                .any(|l| l.kind != BitflagBindingKind::InlinedBool);
            let bind_name = if links.is_empty() {
                format!("_{var_name}")
            } else {
                var_name.clone()
            };
            b.line(&format!("let {bind_name} = r.read_u8()?;"));
            for link in links {
                if link.kind == BitflagBindingKind::InlinedBool {
                    b.line(&format!(
                        "let {} = ({bind_name} & {:#04x}) != 0;",
                        link.field_name, link.mask
                    ));
                }
            }
            if !used_later && !links.is_empty() {
                b.line(&format!("let _ = {bind_name};"));
            }
        }
        WireOp::Field {
            field_name,
            encoding,
        } => {
            emit_decode_field_binding(b, field_name, encoding);
        }
        WireOp::FlagGuarded {
            flags_var,
            mask,
            field_name,
            encoding,
        } => {
            b.line(&format!(
                "let {field_name} = if ({flags_var} & {mask:#04x}) != 0 {{"
            ));
            b.indent();
            match encoding {
                GuardedEncoding::Scalar(scalar) => {
                    b.line(&format!("Some({})", decode_scalar_expr(scalar)));
                }
                GuardedEncoding::List {
                    prefix,
                    max_count,
                    item,
                } => {
                    emit_decode_list_block(b, *prefix, *max_count, item);
                    b.line("Some(items)");
                }
            }
            b.dedent();
            b.line("} else {");
            b.indent();
            b.line("None");
            b.dedent();
            b.line("};");
        }
        WireOp::TailGroup { fields } => {
            for tf in fields {
                b.line(&format!("let {} = if !r.is_at_end() {{", tf.field_name));
                b.indent();
                emit_decode_field_value_expr(b, &tf.encoding);
                b.dedent();
                b.line("} else {");
                b.indent();
                b.line(tail_field_default_expr(&tf.encoding));
                b.dedent();
                b.line("};");
            }
        }
    }
}

fn emit_decode_field_binding(b: &mut CodeBuffer, field_name: &str, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => {
            b.line(&format!(
                "let {field_name} = {};",
                decode_scalar_expr(scalar)
            ));
        }
        FieldEncoding::SentinelOptional { inner, sentinel } => {
            let raw_expr = decode_scalar_expr(inner);
            let check = match sentinel {
                SentinelVal::EmptyString => "!raw.is_empty()",
                SentinelVal::EmptyGuid => "!raw.is_empty() && raw != EMPTY_GUID",
                SentinelVal::Zero => "raw != 0",
            };
            b.line(&format!(
                "let {field_name} = {{ let raw = {raw_expr}; if {check} {{ Some(raw) }} else {{ None }} }};"
            ));
        }
        FieldEncoding::List {
            prefix,
            max_count,
            item,
        } => {
            b.line(&format!("let {field_name} = {{"));
            b.indent();
            emit_decode_list_block(b, *prefix, *max_count, item);
            b.line("items");
            b.dedent();
            b.line("};");
        }
    }
}

fn emit_decode_field_value_expr(b: &mut CodeBuffer, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => {
            b.line(&decode_scalar_expr(scalar));
        }
        FieldEncoding::SentinelOptional { inner, sentinel } => {
            let raw_expr = decode_scalar_expr(inner);
            let check = match sentinel {
                SentinelVal::EmptyString => "!raw.is_empty()",
                SentinelVal::EmptyGuid => "!raw.is_empty() && raw != EMPTY_GUID",
                SentinelVal::Zero => "raw != 0",
            };
            b.line(&format!("let raw = {raw_expr};"));
            b.line(&format!("if {check} {{ Some(raw) }} else {{ None }}"));
        }
        FieldEncoding::List {
            prefix,
            max_count,
            item,
        } => {
            emit_decode_list_block(b, *prefix, *max_count, item);
            b.line("items");
        }
    }
}

fn emit_decode_list_block(
    b: &mut CodeBuffer,
    prefix: IntRepr,
    max_count: Option<usize>,
    item: &ScalarKind,
) {
    let raw_count = read_int_repr(prefix);
    b.line(&format!("let count = ({raw_count}) as usize;"));
    if let Some(max) = max_count {
        b.line(&format!(
            "if count > {max} || count > r.remaining() {{ return Err(WireError::CountExceedsMax); }}"
        ));
    } else {
        b.line("if count > r.remaining() { return Err(WireError::CountExceedsMax); }");
    }
    b.line("let mut items = Vec::with_capacity(count);");
    b.block("for _ in 0..count", |b| {
        b.line(&format!("items.push({});", decode_scalar_expr(item)));
    });
}

fn decode_scalar_expr(scalar: &ScalarKind) -> String {
    match scalar {
        ScalarKind::Prim(p) => match p {
            PrimType::U8 => "r.read_u8()?".into(),
            PrimType::U16Be => "r.read_u16be()?".into(),
            PrimType::U16Le => "r.read_u16le()?".into(),
            PrimType::U32Be => "r.read_u32be()?".into(),
            PrimType::U32Le => "r.read_u32le()?".into(),
            PrimType::U64Be => "r.read_u64be()?".into(),
            PrimType::U64Le => "r.read_u64le()?".into(),
            PrimType::Varint => "r.read_varint()?".into(),
            PrimType::Bool => "r.read_bool()?".into(),
            PrimType::Bytes => "r.read_bytes()?".into(),
            PrimType::String | PrimType::Guid => "r.read_str()?".into(),
        },
        ScalarKind::BytesWithPrefix { prefix, max_len } => {
            let len_read = read_int_repr(*prefix);
            if let Some(max) = max_len {
                format!(
                    "{{ let len = ({len_read}) as usize; if len > {max} {{ return Err(WireError::CountExceedsMax); }} r.read_raw(len)? }}"
                )
            } else {
                format!("{{ let len = ({len_read}) as usize; r.read_raw(len)? }}")
            }
        }
        ScalarKind::Enum { name, repr } => {
            let raw = read_int_repr(*repr);
            format!("{name}::from_wire({raw})?")
        }
        ScalarKind::Struct(name) | ScalarKind::Choice(name) => {
            format!("{name}::decode_wire(r)?")
        }
    }
}

fn tail_field_active_expr(field_name: &str, encoding: &FieldEncoding) -> String {
    match encoding {
        FieldEncoding::SentinelOptional { .. } => format!("self.{field_name}.is_some()"),
        FieldEncoding::List { .. } => format!("!self.{field_name}.is_empty()"),
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bool)) => format!("self.{field_name}"),
        FieldEncoding::Scalar(ScalarKind::Prim(
            PrimType::String | PrimType::Guid | PrimType::Bytes,
        )) => format!("!self.{field_name}.is_empty()"),
        FieldEncoding::Scalar(_) => format!("self.{field_name} != 0"),
    }
}

fn tail_field_default_expr(encoding: &FieldEncoding) -> &'static str {
    match encoding {
        FieldEncoding::SentinelOptional { .. } => "None",
        FieldEncoding::List { .. } => "Vec::new()",
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bool)) => "false",
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::String | PrimType::Guid)) => {
            "String::new()"
        }
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bytes)) => "Vec::new()",
        FieldEncoding::Scalar(_) => "0",
    }
}

fn rust_type(ty: &IrType) -> String {
    match ty {
        IrType::Scalar(s) => rust_scalar_type(s),
        IrType::Optional(inner) => format!("Option<{}>", rust_type(inner)),
        IrType::List(item) => format!("Vec<{}>", rust_scalar_type(item)),
    }
}

fn rust_scalar_type(s: &ScalarKind) -> String {
    match s {
        ScalarKind::Prim(p) => match p {
            PrimType::U8 => "u8".into(),
            PrimType::U16Be | PrimType::U16Le => "u16".into(),
            PrimType::U32Be | PrimType::U32Le => "u32".into(),
            PrimType::U64Be | PrimType::U64Le | PrimType::Varint => "u64".into(),
            PrimType::Bool => "bool".into(),
            PrimType::Bytes => "Vec<u8>".into(),
            PrimType::String | PrimType::Guid => "String".into(),
        },
        ScalarKind::BytesWithPrefix { .. } => "Vec<u8>".into(),
        ScalarKind::Enum { name, .. } | ScalarKind::Struct(name) | ScalarKind::Choice(name) => {
            name.clone()
        }
    }
}

fn int_repr_type(repr: IntRepr) -> &'static str {
    match repr {
        IntRepr::U8 => "u8",
        IntRepr::U16Be | IntRepr::U16Le => "u16",
        IntRepr::U32Be | IntRepr::U32Le => "u32",
        IntRepr::U64Be | IntRepr::U64Le | IntRepr::Varint => "u64",
    }
}

fn write_int_repr(repr: IntRepr, expr: &str) -> String {
    match repr {
        IntRepr::U8 => format!("w.write_u8(({expr}) as u8)"),
        IntRepr::U16Be => format!("w.write_u16be(({expr}) as u16)"),
        IntRepr::U16Le => format!("w.write_u16le(({expr}) as u16)"),
        IntRepr::U32Be => format!("w.write_u32be(({expr}) as u32)"),
        IntRepr::U32Le => format!("w.write_u32le(({expr}) as u32)"),
        IntRepr::U64Be => format!("w.write_u64be(({expr}) as u64)"),
        IntRepr::U64Le => format!("w.write_u64le(({expr}) as u64)"),
        IntRepr::Varint => format!("w.write_varint(({expr}) as u64)"),
    }
}

fn read_int_repr(repr: IntRepr) -> &'static str {
    match repr {
        IntRepr::U8 => "r.read_u8()?",
        IntRepr::U16Be => "r.read_u16be()?",
        IntRepr::U16Le => "r.read_u16le()?",
        IntRepr::U32Be => "r.read_u32be()?",
        IntRepr::U32Le => "r.read_u32le()?",
        IntRepr::U64Be => "r.read_u64be()?",
        IntRepr::U64Le => "r.read_u64le()?",
        IntRepr::Varint => "r.read_varint()?",
    }
}

fn write_prim(p: PrimType, expr: &str, is_ref: bool) -> String {
    let val = if is_ref {
        format!("*{expr}")
    } else {
        expr.to_string()
    };
    let borrow = if is_ref {
        expr.to_string()
    } else {
        format!("&{expr}")
    };
    match p {
        PrimType::U8 => format!("w.write_u8({val})"),
        PrimType::U16Be => format!("w.write_u16be({val})"),
        PrimType::U16Le => format!("w.write_u16le({val})"),
        PrimType::U32Be => format!("w.write_u32be({val})"),
        PrimType::U32Le => format!("w.write_u32le({val})"),
        PrimType::U64Be => format!("w.write_u64be({val})"),
        PrimType::U64Le => format!("w.write_u64le({val})"),
        PrimType::Varint => format!("w.write_varint({val})"),
        PrimType::Bool => format!("w.write_bool({val})"),
        PrimType::Bytes => format!("w.write_bytes({borrow})"),
        PrimType::String | PrimType::Guid => format!("w.write_str({borrow})"),
    }
}

const RUST_PREAMBLE: &str = r#"#![allow(dead_code, clippy::all)]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

pub const EMPTY_GUID: &str = "00000000-0000-0000-0000-000000000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    UnexpectedEof,
    VarintOverflow,
    InvalidUtf8,
    InvalidEnumVariant,
    InvalidChoiceTag,
    UnexpectedMessageTag,
    CountExceedsMax,
    FrameLengthOutOfRange,
}

#[derive(Default)]
pub struct WireWriter {
    buf: Vec<u8>,
}

impl WireWriter {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    pub fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn write_u16be(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_u16le(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_u32be(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_u32le(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_u64be(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_u64le(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_varint(&mut self, mut v: u64) {
        while v > 0x7F {
            self.buf.push(((v & 0x7F) as u8) | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }

    pub fn write_bool(&mut self, v: bool) {
        self.buf.push(if v { 1 } else { 0 });
    }

    pub fn write_raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_varint(bytes.len() as u64);
        self.buf.extend_from_slice(bytes);
    }

    pub fn write_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }
}

pub struct WireReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> WireReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn is_at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub fn read_u8(&mut self) -> Result<u8, WireError> {
        if self.pos >= self.buf.len() {
            return Err(WireError::UnexpectedEof);
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_u16be(&mut self) -> Result<u16, WireError> {
        let s = self.read_slice(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }

    pub fn read_u16le(&mut self) -> Result<u16, WireError> {
        let s = self.read_slice(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    pub fn read_u32be(&mut self) -> Result<u32, WireError> {
        let s = self.read_slice(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn read_u32le(&mut self) -> Result<u32, WireError> {
        let s = self.read_slice(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn read_u64be(&mut self) -> Result<u64, WireError> {
        let s = self.read_slice(8)?;
        Ok(u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
    }

    pub fn read_u64le(&mut self) -> Result<u64, WireError> {
        let s = self.read_slice(8)?;
        Ok(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
    }

    pub fn read_varint(&mut self) -> Result<u64, WireError> {
        let mut result = 0u64;
        let mut shift = 0u32;
        for _ in 0..10 {
            let b = self.read_u8()?;
            result |= ((b & 0x7F) as u64) << shift;
            if (b & 0x80) == 0 {
                return Ok(result);
            }
            shift += 7;
        }
        Err(WireError::VarintOverflow)
    }

    pub fn read_bool(&mut self) -> Result<bool, WireError> {
        Ok(self.read_u8()? == 1)
    }

    pub fn read_raw(&mut self, len: usize) -> Result<Vec<u8>, WireError> {
        Ok(self.read_slice(len)?.to_vec())
    }

    pub fn read_bytes(&mut self) -> Result<Vec<u8>, WireError> {
        let len = self.read_varint()? as usize;
        self.read_raw(len)
    }

    pub fn read_str(&mut self) -> Result<String, WireError> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).map_err(|_| WireError::InvalidUtf8)
    }

    fn read_slice(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        if len > self.remaining() {
            return Err(WireError::UnexpectedEof);
        }
        let s = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Ok(s)
    }
}
"#;
