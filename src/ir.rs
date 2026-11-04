use crate::ast::{IntRepr, PrimType, SentinelVal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIr {
    pub package: String,
    pub frame: Option<FrameIr>,
    pub enums: Vec<EnumIr>,
    pub structs: Vec<RecordIr>,
    pub choices: Vec<ChoiceIr>,
    pub messages: Vec<RecordIr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameIr {
    pub name: String,
    pub length_repr: IntRepr,
    pub max_frame_bytes: usize,
    pub tag_enum: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumIr {
    pub name: String,
    pub repr: IntRepr,
    pub variants: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceIr {
    pub name: String,
    pub tag_repr: IntRepr,
    pub variants: Vec<ChoiceVariantIr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceVariantIr {
    pub name: String,
    pub discriminant: u64,
    pub record: RecordIr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordIr {
    pub name: String,
    pub tag_byte: Option<u8>,
    pub tag_enum_variant: Option<(String, String)>,
    pub fields: Vec<FieldIr>,
    pub ops: Vec<WireOp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldIr {
    pub name: String,
    pub ty: IrType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrType {
    Scalar(ScalarKind),
    Optional(Box<IrType>),
    List(Box<ScalarKind>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalarKind {
    Prim(PrimType),
    BytesWithPrefix {
        prefix: IntRepr,
        max_len: Option<usize>,
    },
    Enum {
        name: String,
        repr: IntRepr,
    },
    Struct(String),
    Choice(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireOp {
    MessageTag {
        byte: u8,
        enum_variant: Option<(String, String)>,
    },
    BitflagsHeader {
        var_name: String,
        links: Vec<BitflagLink>,
    },
    Field {
        field_name: String,
        encoding: FieldEncoding,
    },
    FlagGuarded {
        flags_var: String,
        mask: u8,
        field_name: String,
        encoding: GuardedEncoding,
    },
    TailGroup {
        fields: Vec<TailFieldIr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitflagLink {
    pub field_name: String,
    pub mask: u8,
    pub kind: BitflagBindingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitflagBindingKind {
    InlinedBool,
    OptionalValue,
    OptionalList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldEncoding {
    Scalar(ScalarKind),
    SentinelOptional {
        inner: ScalarKind,
        sentinel: SentinelVal,
    },
    List {
        prefix: IntRepr,
        max_count: Option<usize>,
        item: ScalarKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedEncoding {
    Scalar(ScalarKind),
    List {
        prefix: IntRepr,
        max_count: Option<usize>,
        item: ScalarKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailFieldIr {
    pub field_name: String,
    pub encoding: FieldEncoding,
}
