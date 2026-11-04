use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Number(u64),
    StrLit(String),
    Semi,
    Colon,
    Comma,
    Dot,
    Eq,
    At,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LAngle,
    RAngle,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, col {}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0;
    let mut line = 1;
    let mut col = 1;

    while pos < chars.len() {
        let c = chars[pos];

        if c == '\n' {
            pos += 1;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_whitespace() {
            pos += 1;
            col += 1;
            continue;
        }
        if c == '/' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
            while pos < chars.len() && chars[pos] != '\n' {
                pos += 1;
            }
            continue;
        }

        let start_line = line;
        let start_col = col;

        let single = match c {
            ';' => Some(TokenKind::Semi),
            ':' => Some(TokenKind::Colon),
            ',' => Some(TokenKind::Comma),
            '.' => Some(TokenKind::Dot),
            '=' => Some(TokenKind::Eq),
            '@' => Some(TokenKind::At),
            '{' => Some(TokenKind::LBrace),
            '}' => Some(TokenKind::RBrace),
            '(' => Some(TokenKind::LParen),
            ')' => Some(TokenKind::RParen),
            '<' => Some(TokenKind::LAngle),
            '>' => Some(TokenKind::RAngle),
            _ => None,
        };

        if let Some(kind) = single {
            tokens.push(Token {
                kind,
                line: start_line,
                col: start_col,
            });
            pos += 1;
            col += 1;
            continue;
        }

        if c == '"' {
            pos += 1;
            col += 1;
            let mut s = String::new();
            while pos < chars.len() && chars[pos] != '"' {
                s.push(chars[pos]);
                pos += 1;
                col += 1;
            }
            if pos >= chars.len() {
                return Err(ParseError {
                    message: "unterminated string literal".into(),
                    line: start_line,
                    col: start_col,
                });
            }
            pos += 1;
            col += 1;
            tokens.push(Token {
                kind: TokenKind::StrLit(s),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        if c.is_ascii_digit() {
            let mut num_str = String::new();
            if c == '0' && pos + 1 < chars.len() && (chars[pos + 1] == 'x' || chars[pos + 1] == 'X')
            {
                pos += 2;
                col += 2;
                while pos < chars.len() && (chars[pos].is_ascii_hexdigit() || chars[pos] == '_') {
                    if chars[pos] != '_' {
                        num_str.push(chars[pos]);
                    }
                    pos += 1;
                    col += 1;
                }
                let val = u64::from_str_radix(&num_str, 16).map_err(|_| ParseError {
                    message: format!("invalid hex literal '0x{num_str}'"),
                    line: start_line,
                    col: start_col,
                })?;
                tokens.push(Token {
                    kind: TokenKind::Number(val),
                    line: start_line,
                    col: start_col,
                });
            } else {
                while pos < chars.len() && (chars[pos].is_ascii_digit() || chars[pos] == '_') {
                    if chars[pos] != '_' {
                        num_str.push(chars[pos]);
                    }
                    pos += 1;
                    col += 1;
                }
                let val = num_str.parse::<u64>().map_err(|_| ParseError {
                    message: format!("invalid integer literal '{num_str}'"),
                    line: start_line,
                    col: start_col,
                })?;
                tokens.push(Token {
                    kind: TokenKind::Number(val),
                    line: start_line,
                    col: start_col,
                });
            }
            continue;
        }

        if c.is_ascii_alphabetic() || c == '_' {
            let mut ident = String::new();
            while pos < chars.len() && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_') {
                ident.push(chars[pos]);
                pos += 1;
                col += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident(ident),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        return Err(ParseError {
            message: format!("unexpected character '{c}'"),
            line: start_line,
            col: start_col,
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        line,
        col,
    });
    Ok(tokens)
}
