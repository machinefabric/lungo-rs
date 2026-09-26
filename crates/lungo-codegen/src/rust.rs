//! Rust source emission utilities: identifier sanitization, literal escaping, and an indenting
//! writer that produces stable formatting.

use std::fmt::Write as _;

/// Rust keywords (strict, reserved, and weak keywords that cannot be used as identifiers).
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
    "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract",
    "become", "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// Keywords that cannot be used even as raw identifiers.
const NON_RAW: &[&str] = &["crate", "self", "Self", "super", "_"];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

/// Makes `s` a valid Rust identifier: keywords become raw identifiers (or get a trailing `_`
/// when they cannot be raw).
pub fn identifier(s: &str) -> String {
    if NON_RAW.contains(&s) {
        format!("{s}_")
    } else if is_keyword(s) {
        format!("r#{s}")
    } else {
        s.to_owned()
    }
}

/// Lean's fixed-width integer type names, which read as single words.
const INTEGER_TYPE_WORDS: &[(&str, &str)] = &[("UInt", "Uint"), ("USize", "Usize"), ("ISize", "Isize")];

/// Splits a Lean identifier component into words for case conversion.
fn words(s: &str) -> Vec<String> {
    let mut s = s.to_owned();
    for (from, to) in INTEGER_TYPE_WORDS {
        s = s.replace(from, to);
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let boundary = c.is_uppercase()
            && !cur.is_empty()
            && (chars[i - 1].is_lowercase()
                || chars[i - 1].is_ascii_digit()
                || chars.get(i + 1).is_some_and(|n| n.is_lowercase()));
        if boundary {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Transliterates characters outside Rust's identifier alphabet (Lean allows Unicode letters,
/// subscripts, `!`, `?`, `'`, ...) into ASCII.
fn ascii_word(w: &str) -> String {
    let mut out = String::new();
    for c in w.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            write!(out, "u{:x}", c as u32).unwrap();
        }
    }
    out
}

fn symbol_suffix(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '!' => out.push_str("_bang"),
            '?' => out.push_str("_opt"),
            '\'' => out.push_str("_prime"),
            _ => {}
        }
    }
    out
}

/// Lean component → `snake_case` Rust identifier (for functions, modules, fields).
pub fn snake(s: &str) -> String {
    let base: Vec<String> = words(s).iter().map(|w| ascii_word(&w.to_lowercase())).collect();
    let mut id = base.join("_");
    id.push_str(&symbol_suffix(s));
    if id.is_empty() {
        id.push('_');
    }
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    identifier(&id)
}

/// Lean component → `UpperCamelCase` Rust identifier (for types and variants).
pub fn camel(s: &str) -> String {
    let mut id = String::new();
    for w in words(s) {
        let mut cs = w.chars();
        if let Some(first) = cs.next() {
            id.extend(first.to_uppercase());
            id.push_str(cs.as_str());
        }
    }
    let mut id = ascii_word(&id);
    id.push_str(&symbol_suffix(s).replace('_', ""));
    if id.is_empty() {
        id.push('_');
    }
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    identifier(&id)
}

/// A Rust byte-string literal for `bytes`.
pub fn byte_string(bytes: &[u8]) -> String {
    let mut out = String::from("b\"");
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => write!(out, "\\x{b:02x}").unwrap(),
        }
    }
    out.push('"');
    out
}

/// A Rust string literal for `s`.
pub fn string(s: &str) -> String {
    format!("{s:?}")
}

/// An indenting source writer.
#[derive(Default)]
pub struct Writer {
    out: String,
    indent: usize,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn line(&mut self, s: impl AsRef<str>) {
        let s = s.as_ref();
        if s.is_empty() {
            self.out.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    pub fn open(&mut self, s: impl AsRef<str>) {
        self.line(s);
        self.indent += 1;
    }

    pub fn close(&mut self, s: impl AsRef<str>) {
        self.indent -= 1;
        self.line(s);
    }

    pub fn dedent(&mut self) {
        self.indent -= 1;
    }

    pub fn finish(self) -> String {
        self.out
    }
}

/// The byte offset `words * size_of::<usize>() + bytes` as a Rust expression, written without
/// operations that have no effect (which lints reject in the including crate).
pub fn word_offset(words: u32, bytes: u32) -> String {
    let word = "::core::mem::size_of::<usize>()";
    match (words, bytes) {
        (0, b) => b.to_string(),
        (1, 0) => word.to_owned(),
        (1, b) => format!("{word} + {b}"),
        (w, 0) => format!("{w} * {word}"),
        (w, b) => format!("{w} * {word} + {b}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_conversion_follows_rust_conventions() {
        assert_eq!(snake("isOpen"), "is_open");
        assert_eq!(snake("toUInt8"), "to_uint8");
        assert_eq!(snake("ofUSize"), "of_usize");
        assert_eq!(camel("UInt64"), "Uint64");
        assert_eq!(snake("HTTPServer"), "http_server");
        assert_eq!(snake("get!"), "get_bang");
        assert_eq!(snake("find?"), "find_opt");
        assert_eq!(snake("match"), "r#match");
        assert_eq!(snake("self"), "self_");
        assert_eq!(camel("open"), "Open");
        assert_eq!(camel("sess"), "Sess");
        assert_eq!(camel("Self"), "Self_");
        assert_eq!(snake("α"), "u3b1");
    }

    #[test]
    fn byte_strings_escape_every_non_printable_byte() {
        assert_eq!(byte_string(b"a\"\\\n\x00\xff"), "b\"a\\\"\\\\\\n\\x00\\xff\"");
    }
}
