use crate::ast::{IntRepr, PrimType, SentinelVal};
use crate::codegen::{CodeBuffer, CodeGenerator};
use crate::ir::*;

pub struct PythonGenerator;

impl CodeGenerator for PythonGenerator {
    fn generate(&self, schema: &SchemaIr) -> String {
        let mut b = CodeBuffer::new();
        b.raw(PYTHON_PREAMBLE);
        b.line("");

        for e in &schema.enums {
            emit_enum(&mut b, e);
            b.line("");
        }

        if let Some(frame) = &schema.frame {
            emit_frame(&mut b, frame);
            b.line("");
        }

        for s in &schema.structs {
            emit_record(&mut b, s);
            b.line("");
        }

        for c in &schema.choices {
            emit_choice(&mut b, c);
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
    b.line("def encode_frame(payload: bytes) -> bytes:");
    b.indent();
    b.line(&format!("if len(payload) < 1 or len(payload) > {max}:"));
    b.indent();
    b.line("raise WireError('frame length out of range')");
    b.dedent();
    b.line("w = WireWriter()");
    match f.length_repr {
        IntRepr::U16Be => b.line("w.write_u16be(len(payload))"),
        IntRepr::U32Be => b.line("w.write_u32be(len(payload))"),
        IntRepr::U32Le => b.line("w.write_u32le(len(payload))"),
        _ => b.line("w.write_u32be(len(payload))"),
    }
    b.line("w.write_raw(payload)");
    b.line("return w.to_bytes()");
    b.dedent();
    b.line("");

    b.line(&format!(
        "def decode_frame(buf: bytes) -> tuple[{tag_enum}, bytes]:"
    ));
    b.indent();
    b.line("r = WireReader(buf)");
    let len_expr = match f.length_repr {
        IntRepr::U16Be => "r.read_u16be()",
        IntRepr::U32Be => "r.read_u32be()",
        IntRepr::U32Le => "r.read_u32le()",
        _ => "r.read_u32be()",
    };
    b.line(&format!("length = {len_expr}"));
    b.line(&format!(
        "if length < 1 or length > {max} or r.remaining() < length:"
    ));
    b.indent();
    b.line("raise WireError('frame length out of range')");
    b.dedent();
    b.line("start = r.position()");
    b.line(&format!("tag = {tag_enum}.from_wire(buf[start])"));
    b.line("return tag, buf[start + 1 : start + length]");
    b.dedent();
}

fn emit_enum(b: &mut CodeBuffer, e: &EnumIr) {
    b.line(&format!("class {}(IntEnum):", e.name));
    b.indent();
    for (vname, disc) in &e.variants {
        b.line(&format!("{vname} = {disc}"));
    }
    b.line("");
    b.line("def to_wire(self) -> int:");
    b.indent();
    b.line("return int(self)");
    b.dedent();
    b.line("");
    b.line("@classmethod");
    b.line(&format!("def from_wire(cls, v: int) -> {}:", e.name));
    b.indent();
    b.line("try:");
    b.indent();
    b.line("return cls(v)");
    b.dedent();
    b.line("except ValueError as exc:");
    b.indent();
    b.line("raise WireError(f'invalid enum variant {v}') from exc");
    b.dedent();
    b.dedent();
    b.dedent();
}

fn emit_choice(b: &mut CodeBuffer, c: &ChoiceIr) {
    for v in &c.variants {
        emit_record(b, &v.record);
        b.line("");
    }

    let union_names: Vec<String> = c.variants.iter().map(|v| v.record.name.clone()).collect();
    b.line(&format!(
        "{}Variant = Union[{}]",
        c.name,
        union_names.join(", ")
    ));
    b.line("");
    b.line(&format!("class {}:", c.name));
    b.indent();
    for v in &c.variants {
        b.line(&format!("{} = {}", py_ident(&v.name), v.record.name));
    }
    b.line("");
    b.line("@staticmethod");
    b.line(&format!(
        "def encode_wire(val: {}Variant, w: WireWriter) -> None:",
        c.name
    ));
    b.indent();
    for (i, v) in c.variants.iter().enumerate() {
        let kw = if i == 0 { "if" } else { "elif" };
        b.line(&format!("{kw} isinstance(val, {}):", v.record.name));
        b.indent();
        b.line(&write_int_repr(c.tag_repr, &v.discriminant.to_string()));
        b.line("val.encode_wire(w)");
        b.dedent();
    }
    b.line("else:");
    b.indent();
    b.line("raise WireError('unknown choice variant')");
    b.dedent();
    b.dedent();
    b.line("");

    b.line("@staticmethod");
    b.line(&format!(
        "def decode_wire(r: WireReader) -> {}Variant:",
        c.name
    ));
    b.indent();
    b.line(&format!("tag = {}", read_int_repr(c.tag_repr)));
    for (i, v) in c.variants.iter().enumerate() {
        let kw = if i == 0 { "if" } else { "elif" };
        b.line(&format!("{kw} tag == {}:", v.discriminant));
        b.indent();
        b.line(&format!("return {}.decode_wire(r)", v.record.name));
        b.dedent();
    }
    b.line("raise WireError(f'invalid choice tag {tag}')");
    b.dedent();
    b.dedent();
}

fn emit_record(b: &mut CodeBuffer, r: &RecordIr) {
    b.line("@dataclass");
    b.line(&format!("class {}:", r.name));
    b.indent();
    if r.fields.is_empty() {
        b.line("pass");
    } else {
        for f in &r.fields {
            b.line(&format!("{}: {}", f.name, py_type(&f.ty)));
        }
    }
    b.line("");

    b.line("def encode_wire(self, w: WireWriter) -> None:");
    b.indent();
    if r.ops.is_empty() {
        b.line("pass");
    } else {
        for op in &r.ops {
            emit_encode_op(b, op);
        }
    }
    b.dedent();
    b.line("");

    if let Some(tag_byte) = r.tag_byte {
        let body_ops = &r.ops[1..];
        b.line("@classmethod");
        b.line(&format!(
            "def decode_body(cls, r: WireReader) -> {}:",
            r.name
        ));
        b.indent();
        for op in body_ops {
            emit_decode_op(b, op);
        }
        if r.fields.is_empty() {
            b.line("return cls()");
        } else {
            let args: Vec<String> = r
                .fields
                .iter()
                .map(|f| format!("{}={}", f.name, f.name))
                .collect();
            b.line(&format!("return cls({})", args.join(", ")));
        }
        b.dedent();
        b.line("");

        b.line("@classmethod");
        b.line(&format!(
            "def decode_wire(cls, r: WireReader) -> {}:",
            r.name
        ));
        b.indent();
        b.line(&format!(
            "if r.read_u8() != {tag_byte:#04x}: raise WireError('unexpected message tag')"
        ));
        b.line("return cls.decode_body(r)");
        b.dedent();
        b.line("");

        b.line("def to_bytes(self) -> bytes:");
        b.indent();
        b.line("w = WireWriter()");
        b.line("self.encode_wire(w)");
        b.line("return w.to_bytes()");
        b.dedent();
        b.line("");
        b.line("@classmethod");
        b.line(&format!("def from_bytes(cls, buf: bytes) -> {}:", r.name));
        b.indent();
        b.line("return cls.decode_wire(WireReader(buf))");
        b.dedent();
        b.line("");
        b.line("@classmethod");
        b.line(&format!("def from_body(cls, buf: bytes) -> {}:", r.name));
        b.indent();
        b.line("return cls.decode_body(WireReader(buf))");
        b.dedent();
    } else {
        b.line("@classmethod");
        b.line(&format!(
            "def decode_wire(cls, r: WireReader) -> {}:",
            r.name
        ));
        b.indent();
        for op in &r.ops {
            emit_decode_op(b, op);
        }
        if r.fields.is_empty() {
            b.line("return cls()");
        } else {
            let args: Vec<String> = r
                .fields
                .iter()
                .map(|f| format!("{}={}", f.name, f.name))
                .collect();
            b.line(&format!("return cls({})", args.join(", ")));
        }
        b.dedent();
    }
    b.dedent();
}

fn emit_encode_op(b: &mut CodeBuffer, op: &WireOp) {
    match op {
        WireOp::MessageTag { byte, .. } => {
            b.line(&format!("w.write_u8({byte:#04x})"));
        }
        WireOp::BitflagsHeader { var_name, links } => {
            b.line(&format!("{var_name} = 0"));
            for link in links {
                let cond = match link.kind {
                    BitflagBindingKind::InlinedBool => format!("self.{}", link.field_name),
                    BitflagBindingKind::OptionalValue => {
                        format!("self.{} is not None", link.field_name)
                    }
                    BitflagBindingKind::OptionalList => {
                        format!(
                            "self.{} is not None and len(self.{}) > 0",
                            link.field_name, link.field_name
                        )
                    }
                };
                b.line(&format!("if {cond}:"));
                b.indent();
                b.line(&format!("{var_name} |= {:#04x}", link.mask));
                b.dedent();
            }
            b.line(&format!("w.write_u8({var_name})"));
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
                b.line(&format!("if self.{field_name} is not None:"));
                b.indent();
                emit_encode_scalar(b, &format!("self.{field_name}"), scalar);
                b.dedent();
            }
            GuardedEncoding::List { prefix, item, .. } => {
                b.line(&format!(
                    "if self.{field_name} is not None and len(self.{field_name}) > 0:"
                ));
                b.indent();
                b.line(&write_int_repr(*prefix, &format!("len(self.{field_name})")));
                b.line(&format!("for _item in self.{field_name}:"));
                b.indent();
                emit_encode_scalar(b, "_item", item);
                b.dedent();
                b.dedent();
            }
        },
        WireOp::TailGroup { fields } => {
            let conds: Vec<String> = fields
                .iter()
                .map(|tf| tail_field_active_expr(&tf.field_name, &tf.encoding))
                .collect();
            b.line(&format!("if {}:", conds.join(" or ")));
            b.indent();
            for tf in fields {
                emit_encode_field(b, &format!("self.{}", tf.field_name), &tf.encoding);
            }
            b.dedent();
        }
    }
}

fn emit_encode_field(b: &mut CodeBuffer, access: &str, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => emit_encode_scalar(b, access, scalar),
        FieldEncoding::SentinelOptional { inner, sentinel } => match sentinel {
            SentinelVal::EmptyString => {
                b.line(&format!(
                    "w.write_str({access} if {access} is not None else '')"
                ));
            }
            SentinelVal::EmptyGuid => {
                b.line(&format!(
                    "w.write_str({access} if {access} is not None else EMPTY_GUID)"
                ));
            }
            SentinelVal::Zero => {
                let expr = format!("({access} if {access} is not None else 0)");
                let stmt = match inner {
                    ScalarKind::Prim(p) => write_prim(*p, &expr),
                    _ => format!("w.write_varint({expr})"),
                };
                b.line(&stmt);
            }
        },
        FieldEncoding::List { prefix, item, .. } => {
            b.line(&write_int_repr(*prefix, &format!("len({access})")));
            b.line(&format!("for _item in {access}:"));
            b.indent();
            emit_encode_scalar(b, "_item", item);
            b.dedent();
        }
    }
}

fn emit_encode_scalar(b: &mut CodeBuffer, expr: &str, scalar: &ScalarKind) {
    match scalar {
        ScalarKind::Prim(p) => b.line(&write_prim(*p, expr)),
        ScalarKind::BytesWithPrefix { prefix, .. } => {
            b.line(&write_int_repr(*prefix, &format!("len({expr})")));
            b.line(&format!("w.write_raw({expr})"));
        }
        ScalarKind::Enum { repr, .. } => {
            b.line(&write_int_repr(*repr, &format!("{expr}.to_wire()")));
        }
        ScalarKind::Struct(_) => {
            b.line(&format!("{expr}.encode_wire(w)"));
        }
        ScalarKind::Choice(name) => {
            b.line(&format!("{name}.encode_wire({expr}, w)"));
        }
    }
}

fn emit_decode_op(b: &mut CodeBuffer, op: &WireOp) {
    match op {
        WireOp::MessageTag { byte, .. } => {
            b.line(&format!("if r.read_u8() != {byte:#04x}:"));
            b.indent();
            b.line("raise WireError('unexpected message tag')");
            b.dedent();
        }
        WireOp::BitflagsHeader { var_name, links } => {
            b.line(&format!("{var_name} = r.read_u8()"));
            for link in links {
                if link.kind == BitflagBindingKind::InlinedBool {
                    b.line(&format!(
                        "{} = ({var_name} & {:#04x}) != 0",
                        link.field_name, link.mask
                    ));
                }
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
            b.line(&format!("if ({flags_var} & {mask:#04x}) != 0:"));
            b.indent();
            match encoding {
                GuardedEncoding::Scalar(scalar) => {
                    b.line(&format!("{field_name} = {}", decode_scalar_expr(scalar)));
                }
                GuardedEncoding::List {
                    prefix,
                    max_count,
                    item,
                } => {
                    emit_decode_list_to_var(b, field_name, *prefix, *max_count, item);
                }
            }
            b.dedent();
            b.line("else:");
            b.indent();
            b.line(&format!("{field_name} = None"));
            b.dedent();
        }
        WireOp::TailGroup { fields } => {
            for tf in fields {
                b.line("if not r.is_at_end():");
                b.indent();
                emit_decode_field_binding(b, &tf.field_name, &tf.encoding);
                b.dedent();
                b.line("else:");
                b.indent();
                b.line(&format!(
                    "{} = {}",
                    tf.field_name,
                    tail_field_default_expr(&tf.encoding)
                ));
                b.dedent();
            }
        }
    }
}

fn emit_decode_field_binding(b: &mut CodeBuffer, field_name: &str, encoding: &FieldEncoding) {
    match encoding {
        FieldEncoding::Scalar(scalar) => {
            b.line(&format!("{field_name} = {}", decode_scalar_expr(scalar)));
        }
        FieldEncoding::SentinelOptional { inner, sentinel } => {
            b.line(&format!("_raw = {}", decode_scalar_expr(inner)));
            let cond = match sentinel {
                SentinelVal::EmptyString => "len(_raw) > 0",
                SentinelVal::EmptyGuid => "len(_raw) > 0 and _raw != EMPTY_GUID",
                SentinelVal::Zero => "_raw != 0",
            };
            b.line(&format!("{field_name} = _raw if ({cond}) else None"));
        }
        FieldEncoding::List {
            prefix,
            max_count,
            item,
        } => {
            emit_decode_list_to_var(b, field_name, *prefix, *max_count, item);
        }
    }
}

fn emit_decode_list_to_var(
    b: &mut CodeBuffer,
    var_name: &str,
    prefix: IntRepr,
    max_count: Option<usize>,
    item: &ScalarKind,
) {
    b.line(&format!("_count = {}", read_int_repr(prefix)));
    if let Some(max) = max_count {
        b.line(&format!("if _count > {max} or _count > r.remaining():"));
    } else {
        b.line("if _count > r.remaining():");
    }
    b.indent();
    b.line("raise WireError('count exceeds max')");
    b.dedent();
    b.line(&format!(
        "{var_name} = [{} for _ in range(_count)]",
        decode_scalar_expr(item)
    ));
}

fn decode_scalar_expr(scalar: &ScalarKind) -> String {
    match scalar {
        ScalarKind::Prim(p) => match p {
            PrimType::U8 => "r.read_u8()".into(),
            PrimType::U16Be => "r.read_u16be()".into(),
            PrimType::U16Le => "r.read_u16le()".into(),
            PrimType::U32Be => "r.read_u32be()".into(),
            PrimType::U32Le => "r.read_u32le()".into(),
            PrimType::U64Be => "r.read_u64be()".into(),
            PrimType::U64Le => "r.read_u64le()".into(),
            PrimType::Varint => "r.read_varint()".into(),
            PrimType::Bool => "r.read_bool()".into(),
            PrimType::Bytes => "r.read_bytes()".into(),
            PrimType::String | PrimType::Guid => "r.read_str()".into(),
        },
        ScalarKind::BytesWithPrefix { prefix, max_len } => {
            let len_expr = read_int_repr(*prefix);
            if let Some(max) = max_len {
                format!("r.read_raw_bounded({len_expr}, {max})")
            } else {
                format!("r.read_raw({len_expr})")
            }
        }
        ScalarKind::Enum { name, repr } => {
            format!("{name}.from_wire({})", read_int_repr(*repr))
        }
        ScalarKind::Struct(name) | ScalarKind::Choice(name) => {
            format!("{name}.decode_wire(r)")
        }
    }
}

fn tail_field_active_expr(field_name: &str, encoding: &FieldEncoding) -> String {
    match encoding {
        FieldEncoding::SentinelOptional { .. } => format!("self.{field_name} is not None"),
        FieldEncoding::List { .. } => format!("len(self.{field_name}) > 0"),
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bool)) => format!("self.{field_name}"),
        FieldEncoding::Scalar(ScalarKind::Prim(
            PrimType::String | PrimType::Guid | PrimType::Bytes,
        )) => format!("len(self.{field_name}) > 0"),
        FieldEncoding::Scalar(_) => format!("self.{field_name} != 0"),
    }
}

fn tail_field_default_expr(encoding: &FieldEncoding) -> &'static str {
    match encoding {
        FieldEncoding::SentinelOptional { .. } => "None",
        FieldEncoding::List { .. } => "[]",
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bool)) => "False",
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::String | PrimType::Guid)) => "''",
        FieldEncoding::Scalar(ScalarKind::Prim(PrimType::Bytes)) => "b''",
        FieldEncoding::Scalar(_) => "0",
    }
}

fn py_ident(name: &str) -> String {
    match name {
        "None" | "True" | "False" | "from" | "pass" | "class" | "def" | "return" | "in" | "is" => {
            format!("{name}_")
        }
        other => other.to_string(),
    }
}

fn py_type(ty: &IrType) -> String {
    match ty {
        IrType::Scalar(s) => py_scalar_type(s),
        IrType::Optional(inner) => format!("Optional[{}]", py_type(inner)),
        IrType::List(item) => format!("list[{}]", py_scalar_type(item)),
    }
}

fn py_scalar_type(s: &ScalarKind) -> String {
    match s {
        ScalarKind::Prim(p) => match p {
            PrimType::U8
            | PrimType::U16Be
            | PrimType::U16Le
            | PrimType::U32Be
            | PrimType::U32Le
            | PrimType::U64Be
            | PrimType::U64Le
            | PrimType::Varint => "int".into(),
            PrimType::Bool => "bool".into(),
            PrimType::Bytes => "bytes".into(),
            PrimType::String | PrimType::Guid => "str".into(),
        },
        ScalarKind::BytesWithPrefix { .. } => "bytes".into(),
        ScalarKind::Enum { name, .. } | ScalarKind::Struct(name) => name.clone(),
        ScalarKind::Choice(name) => format!("{name}Variant"),
    }
}

fn write_int_repr(repr: IntRepr, expr: &str) -> String {
    match repr {
        IntRepr::U8 => format!("w.write_u8({expr})"),
        IntRepr::U16Be => format!("w.write_u16be({expr})"),
        IntRepr::U16Le => format!("w.write_u16le({expr})"),
        IntRepr::U32Be => format!("w.write_u32be({expr})"),
        IntRepr::U32Le => format!("w.write_u32le({expr})"),
        IntRepr::U64Be => format!("w.write_u64be({expr})"),
        IntRepr::U64Le => format!("w.write_u64le({expr})"),
        IntRepr::Varint => format!("w.write_varint({expr})"),
    }
}

fn read_int_repr(repr: IntRepr) -> &'static str {
    match repr {
        IntRepr::U8 => "r.read_u8()",
        IntRepr::U16Be => "r.read_u16be()",
        IntRepr::U16Le => "r.read_u16le()",
        IntRepr::U32Be => "r.read_u32be()",
        IntRepr::U32Le => "r.read_u32le()",
        IntRepr::U64Be => "r.read_u64be()",
        IntRepr::U64Le => "r.read_u64le()",
        IntRepr::Varint => "r.read_varint()",
    }
}

fn write_prim(p: PrimType, expr: &str) -> String {
    match p {
        PrimType::U8 => format!("w.write_u8({expr})"),
        PrimType::U16Be => format!("w.write_u16be({expr})"),
        PrimType::U16Le => format!("w.write_u16le({expr})"),
        PrimType::U32Be => format!("w.write_u32be({expr})"),
        PrimType::U32Le => format!("w.write_u32le({expr})"),
        PrimType::U64Be => format!("w.write_u64be({expr})"),
        PrimType::U64Le => format!("w.write_u64le({expr})"),
        PrimType::Varint => format!("w.write_varint({expr})"),
        PrimType::Bool => format!("w.write_bool({expr})"),
        PrimType::Bytes => format!("w.write_bytes({expr})"),
        PrimType::String | PrimType::Guid => format!("w.write_str({expr})"),
    }
}

const PYTHON_PREAMBLE: &str = r#"from __future__ import annotations
from dataclasses import dataclass
from enum import IntEnum
import struct
from typing import Optional, Union

EMPTY_GUID = "00000000-0000-0000-0000-000000000000"


class WireError(ValueError):
    pass


class WireWriter:
    def __init__(self) -> None:
        self._buf = bytearray()

    def to_bytes(self) -> bytes:
        return bytes(self._buf)

    def write_u8(self, v: int) -> None:
        self._buf.append(v & 0xFF)

    def write_u16be(self, v: int) -> None:
        self._buf.extend(struct.pack(">H", v))

    def write_u16le(self, v: int) -> None:
        self._buf.extend(struct.pack("<H", v))

    def write_u32be(self, v: int) -> None:
        self._buf.extend(struct.pack(">I", v))

    def write_u32le(self, v: int) -> None:
        self._buf.extend(struct.pack("<I", v))

    def write_u64be(self, v: int) -> None:
        self._buf.extend(struct.pack(">Q", v))

    def write_u64le(self, v: int) -> None:
        self._buf.extend(struct.pack("<Q", v))

    def write_varint(self, v: int) -> None:
        if v < 0:
            raise WireError("negative varint")
        while v > 0x7F:
            self._buf.append((v & 0x7F) | 0x80)
            v >>= 7
        self._buf.append(v & 0x7F)

    def write_bool(self, v: bool) -> None:
        self._buf.append(1 if v else 0)

    def write_raw(self, data: bytes) -> None:
        self._buf.extend(data)

    def write_bytes(self, data: bytes) -> None:
        self.write_varint(len(data))
        self._buf.extend(data)

    def write_str(self, s: str) -> None:
        self.write_bytes(s.encode("utf-8"))


class WireReader:
    def __init__(self, buf: bytes) -> None:
        self._buf = buf
        self._pos = 0

    def position(self) -> int:
        return self._pos

    def remaining(self) -> int:
        return len(self._buf) - self._pos

    def is_at_end(self) -> bool:
        return self._pos >= len(self._buf)

    def read_u8(self) -> int:
        if self._pos >= len(self._buf):
            raise WireError("unexpected eof")
        b = self._buf[self._pos]
        self._pos += 1
        return b

    def read_u16be(self) -> int:
        return struct.unpack(">H", self.read_raw(2))[0]

    def read_u16le(self) -> int:
        return struct.unpack("<H", self.read_raw(2))[0]

    def read_u32be(self) -> int:
        return struct.unpack(">I", self.read_raw(4))[0]

    def read_u32le(self) -> int:
        return struct.unpack("<I", self.read_raw(4))[0]

    def read_u64be(self) -> int:
        return struct.unpack(">Q", self.read_raw(8))[0]

    def read_u64le(self) -> int:
        return struct.unpack("<Q", self.read_raw(8))[0]

    def read_varint(self) -> int:
        result = 0
        shift = 0
        for _ in range(10):
            b = self.read_u8()
            result |= (b & 0x7F) << shift
            if (b & 0x80) == 0:
                return result
            shift += 7
        raise WireError("varint overflow")

    def read_bool(self) -> bool:
        return self.read_u8() == 1

    def read_raw(self, length: int) -> bytes:
        if length < 0 or length > self.remaining():
            raise WireError("unexpected eof")
        out = self._buf[self._pos : self._pos + length]
        self._pos += length
        return out

    def read_raw_bounded(self, length: int, max_len: int) -> bytes:
        if length > max_len:
            raise WireError("length exceeds max")
        return self.read_raw(length)

    def read_bytes(self) -> bytes:
        length = self.read_varint()
        return self.read_raw(length)

    def read_str(self) -> str:
        raw = self.read_bytes()
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise WireError("invalid utf-8") from exc
"#;
