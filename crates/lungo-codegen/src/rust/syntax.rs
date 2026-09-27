//! Rust source syntax: keyword escaping of identifiers and literal escaping.

use crate::core::names::{snake_case, upper_camel_case};
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

/// Lean component → `snake_case` Rust identifier (for functions, modules, fields).
pub fn snake(s: &str) -> String {
    identifier(&snake_case(s))
}

/// Lean component → `UpperCamelCase` Rust identifier (for types and variants).
pub fn camel(s: &str) -> String {
    identifier(&upper_camel_case(s))
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
