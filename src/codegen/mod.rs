pub mod csharp;
pub mod go;
pub mod python;
pub mod rust;
pub mod zig;

use crate::ir::SchemaIr;

pub trait CodeGenerator {
    fn generate(&self, schema: &SchemaIr) -> String;
}

#[derive(Default)]
pub struct CodeBuffer {
    out: String,
    indent: usize,
}

impl CodeBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn line(&mut self, text: &str) {
        if text.is_empty() {
            self.out.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    pub fn raw(&mut self, text: &str) {
        self.out.push_str(text);
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    pub fn block<F: FnOnce(&mut Self)>(&mut self, header: &str, f: F) {
        self.line(&format!("{header} {{"));
        self.indent();
        f(self);
        self.dedent();
        self.line("}");
    }

    pub fn finish(self) -> String {
        self.out
    }
}

pub fn to_pascal_case(s: &str) -> String {
    let mut out = String::new();
    for part in s.split('_').filter(|p| !p.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

pub fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
