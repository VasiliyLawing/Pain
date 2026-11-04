use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ast::*;
use crate::ir::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub message: String,
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LowerError {}

struct TypeCatalog {
    enums: HashMap<String, EnumDecl>,
    structs: HashSet<String>,
    choices: HashSet<String>,
}

impl TypeCatalog {
    fn build(ast: &SchemaAst) -> Result<Self, LowerError> {
        let mut enums = HashMap::new();
        let mut structs = HashSet::new();
        let mut choices = HashSet::new();

        for decl in &ast.decls {
            match decl {
                Decl::Enum(e) => {
                    if enums.insert(e.name.clone(), e.clone()).is_some() {
                        return Err(err(format!("duplicate enum '{}'", e.name)));
                    }
                }
                Decl::Struct(s) => {
                    if !structs.insert(s.name.clone()) {
                        return Err(err(format!("duplicate struct '{}'", s.name)));
                    }
                }
                Decl::Choice(c) => {
                    if !choices.insert(c.name.clone()) {
                        return Err(err(format!("duplicate choice '{}'", c.name)));
                    }
                }
                Decl::Frame(_) | Decl::Message(_) => {}
            }
        }

        Ok(Self {
            enums,
            structs,
            choices,
        })
    }

    fn resolve_scalar(&self, ty: &TypeExpr) -> Result<ScalarKind, LowerError> {
        match ty {
            TypeExpr::Primitive(p) => Ok(ScalarKind::Prim(*p)),
            TypeExpr::Named(name) => {
                if let Some(e) = self.enums.get(name) {
                    Ok(ScalarKind::Enum {
                        name: name.clone(),
                        repr: e.repr,
                    })
                } else if self.structs.contains(name) {
                    Ok(ScalarKind::Struct(name.clone()))
                } else if self.choices.contains(name) {
                    Ok(ScalarKind::Choice(name.clone()))
                } else {
                    Err(err(format!("unknown type '{name}'")))
                }
            }
            other => Err(err(format!("expected scalar type, found {other:?}"))),
        }
    }

    fn resolve_tag(&self, tag: &TagRef) -> Result<(u8, Option<(String, String)>), LowerError> {
        match tag {
            TagRef::Literal(b) => Ok((*b, None)),
            TagRef::EnumVariant {
                enum_name,
                variant_name,
            } => {
                let e = self
                    .enums
                    .get(enum_name)
                    .ok_or_else(|| err(format!("unknown tag enum '{enum_name}'")))?;
                let v = e
                    .variants
                    .iter()
                    .find(|v| &v.name == variant_name)
                    .ok_or_else(|| {
                        err(format!(
                            "unknown variant '{variant_name}' in enum '{enum_name}'"
                        ))
                    })?;
                let byte = u8::try_from(v.discriminant).map_err(|_| {
                    err(format!(
                        "tag '{enum_name}.{variant_name}' value {} exceeds u8",
                        v.discriminant
                    ))
                })?;
                Ok((byte, Some((enum_name.clone(), variant_name.clone()))))
            }
        }
    }
}

pub fn lower_schema(ast: &SchemaAst) -> Result<SchemaIr, LowerError> {
    let catalog = TypeCatalog::build(ast)?;

    let mut frame = None;
    let mut enums = Vec::new();
    let mut structs = Vec::new();
    let mut choices = Vec::new();
    let mut messages = Vec::new();

    for decl in &ast.decls {
        match decl {
            Decl::Frame(f) => {
                if frame.is_some() {
                    return Err(err("multiple frame declarations in schema"));
                }
                frame = Some(lower_frame(f, &catalog)?);
            }
            Decl::Enum(e) => {
                enums.push(EnumIr {
                    name: e.name.clone(),
                    repr: e.repr,
                    variants: e
                        .variants
                        .iter()
                        .map(|v| (v.name.clone(), v.discriminant))
                        .collect(),
                });
            }
            Decl::Struct(s) => {
                structs.push(lower_record(&s.name, None, None, &s.fields, &catalog)?);
            }
            Decl::Choice(c) => {
                let mut variants = Vec::new();
                for v in &c.variants {
                    let rec_name = format!("{}{}", c.name, v.name);
                    let record = lower_record(&rec_name, None, None, &v.fields, &catalog)?;
                    variants.push(ChoiceVariantIr {
                        name: v.name.clone(),
                        discriminant: v.discriminant,
                        record,
                    });
                }
                choices.push(ChoiceIr {
                    name: c.name.clone(),
                    tag_repr: c.tag_repr,
                    variants,
                });
            }
            Decl::Message(m) => {
                let (tag_byte, tag_enum_variant) = catalog.resolve_tag(&m.tag)?;
                messages.push(lower_record(
                    &m.name,
                    Some(tag_byte),
                    tag_enum_variant,
                    &m.fields,
                    &catalog,
                )?);
            }
        }
    }

    Ok(SchemaIr {
        package: ast.package.clone(),
        frame,
        enums,
        structs,
        choices,
        messages,
    })
}

fn lower_frame(f: &FrameDecl, catalog: &TypeCatalog) -> Result<FrameIr, LowerError> {
    let len_field = f
        .fields
        .iter()
        .find(|fd| fd.name == "length")
        .ok_or_else(|| err(format!("frame '{}' missing 'length' field", f.name)))?;
    let length_repr = match len_field.ty {
        TypeExpr::Primitive(PrimType::U16Be) => IntRepr::U16Be,
        TypeExpr::Primitive(PrimType::U32Be) => IntRepr::U32Be,
        TypeExpr::Primitive(PrimType::U32Le) => IntRepr::U32Le,
        _ => {
            return Err(err(format!(
                "frame '{}' length field must be u16be, u32be, or u32le",
                f.name
            )));
        }
    };
    let max_frame_bytes = find_max(&len_field.annotations).unwrap_or(16 * 1024 * 1024);

    let tag_field = f
        .fields
        .iter()
        .find(|fd| fd.name == "tag")
        .ok_or_else(|| err(format!("frame '{}' missing 'tag' field", f.name)))?;
    let tag_enum = match &tag_field.ty {
        TypeExpr::Named(name) if catalog.enums.contains_key(name) => name.clone(),
        _ => {
            return Err(err(format!(
                "frame '{}' tag field must reference a declared enum",
                f.name
            )));
        }
    };

    Ok(FrameIr {
        name: f.name.clone(),
        length_repr,
        max_frame_bytes,
        tag_enum,
    })
}

fn lower_record(
    name: &str,
    tag_byte: Option<u8>,
    tag_enum_variant: Option<(String, String)>,
    raw_fields: &[FieldDecl],
    catalog: &TypeCatalog,
) -> Result<RecordIr, LowerError> {
    let mut bitflags_links: HashMap<String, Vec<BitflagLink>> = HashMap::new();
    for fd in raw_fields {
        if let TypeExpr::Bitflags(repr) = fd.ty {
            if repr != IntRepr::U8 {
                return Err(err(format!(
                    "record '{name}' field '{}': only bitflags<u8> is supported",
                    fd.name
                )));
            }
            bitflags_links.insert(fd.name.clone(), Vec::new());
        }
    }

    for fd in raw_fields {
        if let Some((flags_field, mask)) = find_flag(&fd.annotations) {
            let mask_u8 = u8::try_from(mask).map_err(|_| {
                err(format!(
                    "record '{name}' field '{}': flag mask {mask:#x} exceeds u8",
                    fd.name
                ))
            })?;
            let kind = match &fd.ty {
                TypeExpr::Primitive(PrimType::Bool) => BitflagBindingKind::InlinedBool,
                TypeExpr::Optional(inner) => match inner.as_ref() {
                    TypeExpr::List(_) => BitflagBindingKind::OptionalList,
                    _ => BitflagBindingKind::OptionalValue,
                },
                _ => {
                    return Err(err(format!(
                        "record '{name}' field '{}': @flag is only valid on bool or optional<...>",
                        fd.name
                    )));
                }
            };
            let links = bitflags_links.get_mut(&flags_field).ok_or_else(|| {
                err(format!(
                    "record '{name}' field '{}' references undeclared bitflags field '{flags_field}'",
                    fd.name
                ))
            })?;
            links.push(BitflagLink {
                field_name: fd.name.clone(),
                mask: mask_u8,
                kind,
            });
        }
    }

    let mut fields = Vec::new();
    let mut ops = Vec::new();
    let mut tail_fields = Vec::new();
    let mut seen_tail = false;

    if let Some(byte) = tag_byte {
        ops.push(WireOp::MessageTag {
            byte,
            enum_variant: tag_enum_variant.clone(),
        });
    }

    for fd in raw_fields {
        let is_tail = fd.annotations.iter().any(|a| matches!(a, Annotation::Tail));
        if seen_tail && !is_tail {
            return Err(err(format!(
                "record '{name}' field '{}': non-@tail field cannot follow a @tail field",
                fd.name
            )));
        }
        if is_tail {
            seen_tail = true;
            let (ir_ty, encoding) = lower_unconditional_field(name, fd, catalog)?;
            fields.push(FieldIr {
                name: fd.name.clone(),
                ty: ir_ty,
            });
            tail_fields.push(TailFieldIr {
                field_name: fd.name.clone(),
                encoding,
            });
            continue;
        }

        if let TypeExpr::Bitflags(_) = fd.ty {
            let links = bitflags_links.remove(&fd.name).unwrap_or_default();
            ops.push(WireOp::BitflagsHeader {
                var_name: fd.name.clone(),
                links,
            });
            continue;
        }

        if let Some((flags_var, mask)) = find_flag(&fd.annotations) {
            let mask_u8 = mask as u8;
            match &fd.ty {
                TypeExpr::Primitive(PrimType::Bool) => {
                    fields.push(FieldIr {
                        name: fd.name.clone(),
                        ty: IrType::Scalar(ScalarKind::Prim(PrimType::Bool)),
                    });
                }
                TypeExpr::Optional(inner) => match inner.as_ref() {
                    TypeExpr::List(item_expr) => {
                        let item_scalar = catalog.resolve_scalar(item_expr)?;
                        let prefix = find_prefix(&fd.annotations).unwrap_or(IntRepr::Varint);
                        let max_count = find_max(&fd.annotations);
                        fields.push(FieldIr {
                            name: fd.name.clone(),
                            ty: IrType::Optional(Box::new(IrType::List(Box::new(
                                item_scalar.clone(),
                            )))),
                        });
                        ops.push(WireOp::FlagGuarded {
                            flags_var,
                            mask: mask_u8,
                            field_name: fd.name.clone(),
                            encoding: GuardedEncoding::List {
                                prefix,
                                max_count,
                                item: item_scalar,
                            },
                        });
                    }
                    other => {
                        let scalar = catalog.resolve_scalar(other)?;
                        fields.push(FieldIr {
                            name: fd.name.clone(),
                            ty: IrType::Optional(Box::new(IrType::Scalar(scalar.clone()))),
                        });
                        ops.push(WireOp::FlagGuarded {
                            flags_var,
                            mask: mask_u8,
                            field_name: fd.name.clone(),
                            encoding: GuardedEncoding::Scalar(scalar),
                        });
                    }
                },
                _ => unreachable!(),
            }
            continue;
        }

        let (ir_ty, encoding) = lower_unconditional_field(name, fd, catalog)?;
        fields.push(FieldIr {
            name: fd.name.clone(),
            ty: ir_ty,
        });
        ops.push(WireOp::Field {
            field_name: fd.name.clone(),
            encoding,
        });
    }

    if !tail_fields.is_empty() {
        ops.push(WireOp::TailGroup {
            fields: tail_fields,
        });
    }

    Ok(RecordIr {
        name: name.to_string(),
        tag_byte,
        tag_enum_variant,
        fields,
        ops,
    })
}

fn lower_unconditional_field(
    record_name: &str,
    fd: &FieldDecl,
    catalog: &TypeCatalog,
) -> Result<(IrType, FieldEncoding), LowerError> {
    match &fd.ty {
        TypeExpr::Optional(inner) => {
            let sentinel = find_sentinel(&fd.annotations).ok_or_else(|| {
                err(format!(
                    "record '{record_name}' field '{}': optional field without @flag must specify @sentinel(...)",
                    fd.name
                ))
            })?;
            let scalar = catalog.resolve_scalar(inner)?;
            Ok((
                IrType::Optional(Box::new(IrType::Scalar(scalar.clone()))),
                FieldEncoding::SentinelOptional {
                    inner: scalar,
                    sentinel,
                },
            ))
        }
        TypeExpr::List(item_expr) => {
            let item = catalog.resolve_scalar(item_expr)?;
            let prefix = find_prefix(&fd.annotations).unwrap_or(IntRepr::Varint);
            let max_count = find_max(&fd.annotations);
            Ok((
                IrType::List(Box::new(item.clone())),
                FieldEncoding::List {
                    prefix,
                    max_count,
                    item,
                },
            ))
        }
        TypeExpr::Primitive(PrimType::Bytes) => {
            let prefix = find_prefix(&fd.annotations).unwrap_or(IntRepr::Varint);
            let max_len = find_max(&fd.annotations);
            let scalar = if prefix == IntRepr::Varint && max_len.is_none() {
                ScalarKind::Prim(PrimType::Bytes)
            } else {
                ScalarKind::BytesWithPrefix { prefix, max_len }
            };
            Ok((
                IrType::Scalar(ScalarKind::Prim(PrimType::Bytes)),
                FieldEncoding::Scalar(scalar),
            ))
        }
        other => {
            let scalar = catalog.resolve_scalar(other)?;
            Ok((
                IrType::Scalar(scalar.clone()),
                FieldEncoding::Scalar(scalar),
            ))
        }
    }
}

fn find_prefix(anns: &[Annotation]) -> Option<IntRepr> {
    anns.iter().find_map(|a| match a {
        Annotation::Prefix(p) => Some(*p),
        _ => None,
    })
}

fn find_max(anns: &[Annotation]) -> Option<usize> {
    anns.iter().find_map(|a| match a {
        Annotation::Max(m) => Some(*m),
        _ => None,
    })
}

fn find_sentinel(anns: &[Annotation]) -> Option<SentinelVal> {
    anns.iter().find_map(|a| match a {
        Annotation::Sentinel(s) => Some(s.clone()),
        _ => None,
    })
}

fn find_flag(anns: &[Annotation]) -> Option<(String, u64)> {
    anns.iter().find_map(|a| match a {
        Annotation::Flag { flags_field, mask } => Some((flags_field.clone(), *mask)),
        _ => None,
    })
}

fn err(msg: impl Into<String>) -> LowerError {
    LowerError {
        message: msg.into(),
    }
}
