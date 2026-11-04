use crate::ast::*;
use crate::lexer::{ParseError, Token, TokenKind, tokenize};

pub fn parse_schema(source: &str) -> Result<SchemaAst, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_schema()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn next(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ParseError> {
        let tok = self.peek();
        Err(ParseError {
            message: msg.into(),
            line: tok.line,
            col: tok.col,
        })
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match &self.peek().kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.next();
                Ok(s)
            }
            other => self.err(format!("expected identifier, found {other:?}")),
        }
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), ParseError> {
        let id = self.expect_ident()?;
        if id != kw {
            return self.err(format!("expected keyword '{kw}', found '{id}'"));
        }
        Ok(())
    }

    fn expect_number(&mut self) -> Result<u64, ParseError> {
        match self.peek().kind {
            TokenKind::Number(n) => {
                self.next();
                Ok(n)
            }
            ref other => self.err(format!("expected number, found {other:?}")),
        }
    }

    fn expect_token(&mut self, expected: &TokenKind) -> Result<(), ParseError> {
        if &self.peek().kind == expected {
            self.next();
            Ok(())
        } else {
            self.err(format!(
                "expected {expected:?}, found {:?}",
                self.peek().kind
            ))
        }
    }

    fn parse_schema(&mut self) -> Result<SchemaAst, ParseError> {
        self.expect_keyword("schema")?;
        let mut pkg = self.expect_ident()?;
        while self.peek().kind == TokenKind::Dot {
            self.next();
            let part = self.expect_ident()?;
            pkg.push('.');
            pkg.push_str(&part);
        }
        self.expect_token(&TokenKind::Semi)?;

        let mut decls = Vec::new();
        while self.peek().kind != TokenKind::Eof {
            let kw = self.expect_ident()?;
            match kw.as_str() {
                "frame" => decls.push(Decl::Frame(self.parse_frame()?)),
                "enum" => decls.push(Decl::Enum(self.parse_enum()?)),
                "struct" => decls.push(Decl::Struct(self.parse_struct()?)),
                "message" => decls.push(Decl::Message(self.parse_message()?)),
                "choice" => decls.push(Decl::Choice(self.parse_choice()?)),
                other => return self.err(format!("unknown top-level declaration '{other}'")),
            }
        }

        Ok(SchemaAst {
            package: pkg,
            decls,
        })
    }

    fn parse_frame(&mut self) -> Result<FrameDecl, ParseError> {
        let name = self.expect_ident()?;
        let fields = self.parse_field_block()?;
        Ok(FrameDecl { name, fields })
    }

    fn parse_enum(&mut self) -> Result<EnumDecl, ParseError> {
        let name = self.expect_ident()?;
        self.expect_token(&TokenKind::Colon)?;
        let repr = self.parse_int_repr()?;
        self.expect_token(&TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            let vname = self.expect_ident()?;
            self.expect_token(&TokenKind::Eq)?;
            let disc = self.expect_number()?;
            variants.push(EnumVariant {
                name: vname,
                discriminant: disc,
            });
            if self.peek().kind == TokenKind::Comma || self.peek().kind == TokenKind::Semi {
                self.next();
            }
        }
        self.expect_token(&TokenKind::RBrace)?;
        Ok(EnumDecl {
            name,
            repr,
            variants,
        })
    }

    fn parse_struct(&mut self) -> Result<StructDecl, ParseError> {
        let name = self.expect_ident()?;
        let fields = self.parse_field_block()?;
        Ok(StructDecl { name, fields })
    }

    fn parse_message(&mut self) -> Result<MessageDecl, ParseError> {
        let name = self.expect_ident()?;
        self.expect_token(&TokenKind::At)?;
        self.expect_keyword("tag")?;
        self.expect_token(&TokenKind::LParen)?;
        let tag = match &self.peek().kind {
            TokenKind::Number(n) => {
                let b = u8::try_from(*n).map_err(|_| ParseError {
                    message: format!("message tag {n} out of u8 range"),
                    line: self.peek().line,
                    col: self.peek().col,
                })?;
                self.next();
                TagRef::Literal(b)
            }
            TokenKind::Ident(_) => {
                let enum_name = self.expect_ident()?;
                self.expect_token(&TokenKind::Dot)?;
                let variant_name = self.expect_ident()?;
                TagRef::EnumVariant {
                    enum_name,
                    variant_name,
                }
            }
            other => {
                return self.err(format!(
                    "expected tag literal or Enum.Variant, found {other:?}"
                ));
            }
        };
        self.expect_token(&TokenKind::RParen)?;
        let fields = self.parse_field_block()?;
        Ok(MessageDecl { name, tag, fields })
    }

    fn parse_choice(&mut self) -> Result<ChoiceDecl, ParseError> {
        let name = self.expect_ident()?;
        self.expect_token(&TokenKind::Colon)?;
        let tag_repr = self.parse_int_repr()?;
        self.expect_token(&TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            let vname = self.expect_ident()?;
            self.expect_token(&TokenKind::Eq)?;
            let disc = self.expect_number()?;
            let fields = self.parse_field_block()?;
            variants.push(ChoiceVariant {
                name: vname,
                discriminant: disc,
                fields,
            });
            if self.peek().kind == TokenKind::Comma || self.peek().kind == TokenKind::Semi {
                self.next();
            }
        }
        self.expect_token(&TokenKind::RBrace)?;
        Ok(ChoiceDecl {
            name,
            tag_repr,
            variants,
        })
    }

    fn parse_field_block(&mut self) -> Result<Vec<FieldDecl>, ParseError> {
        self.expect_token(&TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            let name = self.expect_ident()?;
            self.expect_token(&TokenKind::Colon)?;
            let ty = self.parse_type_expr()?;
            let mut annotations = Vec::new();
            while self.peek().kind == TokenKind::At {
                self.next();
                annotations.push(self.parse_annotation()?);
            }
            self.expect_token(&TokenKind::Semi)?;
            fields.push(FieldDecl {
                name,
                ty,
                annotations,
            });
        }
        self.expect_token(&TokenKind::RBrace)?;
        Ok(fields)
    }

    fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let ident = self.expect_ident()?;
        match ident.as_str() {
            "u8" => Ok(TypeExpr::Primitive(PrimType::U8)),
            "u16be" => Ok(TypeExpr::Primitive(PrimType::U16Be)),
            "u16le" => Ok(TypeExpr::Primitive(PrimType::U16Le)),
            "u32be" => Ok(TypeExpr::Primitive(PrimType::U32Be)),
            "u32le" => Ok(TypeExpr::Primitive(PrimType::U32Le)),
            "u64be" => Ok(TypeExpr::Primitive(PrimType::U64Be)),
            "u64le" => Ok(TypeExpr::Primitive(PrimType::U64Le)),
            "varint" => Ok(TypeExpr::Primitive(PrimType::Varint)),
            "bool" => Ok(TypeExpr::Primitive(PrimType::Bool)),
            "bytes" => Ok(TypeExpr::Primitive(PrimType::Bytes)),
            "string" => Ok(TypeExpr::Primitive(PrimType::String)),
            "guid" => Ok(TypeExpr::Primitive(PrimType::Guid)),
            "bitflags" => {
                self.expect_token(&TokenKind::LAngle)?;
                let repr = self.parse_int_repr()?;
                self.expect_token(&TokenKind::RAngle)?;
                Ok(TypeExpr::Bitflags(repr))
            }
            "optional" => {
                self.expect_token(&TokenKind::LAngle)?;
                let inner = self.parse_type_expr()?;
                self.expect_token(&TokenKind::RAngle)?;
                Ok(TypeExpr::Optional(Box::new(inner)))
            }
            "list" => {
                self.expect_token(&TokenKind::LAngle)?;
                let inner = self.parse_type_expr()?;
                self.expect_token(&TokenKind::RAngle)?;
                Ok(TypeExpr::List(Box::new(inner)))
            }
            other => Ok(TypeExpr::Named(other.to_string())),
        }
    }

    fn parse_int_repr(&mut self) -> Result<IntRepr, ParseError> {
        let id = self.expect_ident()?;
        match id.as_str() {
            "u8" => Ok(IntRepr::U8),
            "u16be" => Ok(IntRepr::U16Be),
            "u16le" => Ok(IntRepr::U16Le),
            "u32be" => Ok(IntRepr::U32Be),
            "u32le" => Ok(IntRepr::U32Le),
            "u64be" => Ok(IntRepr::U64Be),
            "u64le" => Ok(IntRepr::U64Le),
            "varint" => Ok(IntRepr::Varint),
            other => self.err(format!(
                "expected integer wire representation, found '{other}'"
            )),
        }
    }

    fn parse_annotation(&mut self) -> Result<Annotation, ParseError> {
        let name = self.expect_ident()?;
        match name.as_str() {
            "prefix" => {
                self.expect_token(&TokenKind::LParen)?;
                let repr = self.parse_int_repr()?;
                self.expect_token(&TokenKind::RParen)?;
                Ok(Annotation::Prefix(repr))
            }
            "max" => {
                self.expect_token(&TokenKind::LParen)?;
                let n = self.expect_number()? as usize;
                self.expect_token(&TokenKind::RParen)?;
                Ok(Annotation::Max(n))
            }
            "sentinel" => {
                self.expect_token(&TokenKind::LParen)?;
                let sentinel = match &self.peek().kind {
                    TokenKind::StrLit(s) if s.is_empty() => {
                        self.next();
                        SentinelVal::EmptyString
                    }
                    TokenKind::Number(0) => {
                        self.next();
                        SentinelVal::Zero
                    }
                    TokenKind::Ident(id) if id == "empty" => {
                        self.next();
                        SentinelVal::EmptyGuid
                    }
                    other => {
                        return self.err(format!(
                            "unsupported sentinel {other:?}; expected \"\", 0, or empty"
                        ));
                    }
                };
                self.expect_token(&TokenKind::RParen)?;
                Ok(Annotation::Sentinel(sentinel))
            }
            "flag" => {
                self.expect_token(&TokenKind::LParen)?;
                let flags_field = self.expect_ident()?;
                self.expect_token(&TokenKind::Comma)?;
                let mask = self.expect_number()?;
                self.expect_token(&TokenKind::RParen)?;
                Ok(Annotation::Flag { flags_field, mask })
            }
            "tail" => Ok(Annotation::Tail),
            other => self.err(format!("unknown annotation '@{other}'")),
        }
    }
}
