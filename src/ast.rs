#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaAst {
    pub package: String,
    pub decls: Vec<Decl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    Frame(FrameDecl),
    Enum(EnumDecl),
    Struct(StructDecl),
    Message(MessageDecl),
    Choice(ChoiceDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    pub repr: IntRepr,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntRepr {
    U8,
    U16Be,
    U16Le,
    U32Be,
    U32Le,
    U64Be,
    U64Le,
    Varint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    pub discriminant: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDecl {
    pub name: String,
    pub tag: TagRef,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagRef {
    Literal(u8),
    EnumVariant {
        enum_name: String,
        variant_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceDecl {
    pub name: String,
    pub tag_repr: IntRepr,
    pub variants: Vec<ChoiceVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceVariant {
    pub name: String,
    pub discriminant: u64,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    Primitive(PrimType),
    Bitflags(IntRepr),
    Optional(Box<TypeExpr>),
    List(Box<TypeExpr>),
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimType {
    U8,
    U16Be,
    U16Le,
    U32Be,
    U32Le,
    U64Be,
    U64Le,
    Varint,
    Bool,
    Bytes,
    String,
    Guid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    Prefix(IntRepr),
    Max(usize),
    Sentinel(SentinelVal),
    Flag { flags_field: String, mask: u64 },
    Tail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentinelVal {
    EmptyString,
    Zero,
    EmptyGuid,
}
