//! C source syntax: types of IR values, literals, and comments.

use lungo_bir::IrType;
use std::fmt::Write as _;

/// The C type representing values of IR type `ty`.
pub fn c_type(ty: IrType) -> &'static str {
    match ty {
        IrType::Float => "double",
        IrType::Float32 => "float",
        IrType::Uint8 => "uint8_t",
        IrType::Uint16 => "uint16_t",
        IrType::Uint32 => "uint32_t",
        IrType::Uint64 => "uint64_t",
        IrType::Usize => "size_t",
        IrType::Erased | IrType::Object | IrType::Tobject | IrType::Tagged | IrType::Void => "lungo_obj",
    }
}

/// A C string literal with exactly the bytes of `bytes`. Every byte outside printable ASCII is
/// an octal escape (three digits, so a following digit can never extend it), and `?` is escaped
/// so no trigraph can form.
pub fn string(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'?' => out.push_str("\\?"),
            0x20..=0x7e => out.push(b as char),
            _ => write!(out, "\\{b:03o}").unwrap(),
        }
    }
    out.push('"');
    out
}

/// A `/* … */` comment with `text`, which cannot end the comment early.
pub fn comment(text: &str) -> String {
    format!("/* {} */", text.replace("*/", "* /"))
}

/// The byte offset `words * sizeof(size_t) + bytes` as a C expression.
pub fn word_offset(words: u32, bytes: u32) -> String {
    match (words, bytes) {
        (0, b) => b.to_string(),
        (w, 0) => format!("{w} * sizeof(size_t)"),
        (w, b) => format!("{w} * sizeof(size_t) + {b}"),
    }
}

/// A C identifier from an arbitrary name: ASCII letters and digits are kept, everything else
/// becomes `_`, and a leading digit is prefixed. Used for per-program symbol prefixes, which
/// the configuration validates to be identifiers already.
pub fn identifier(name: &str) -> String {
    let mut id: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if id.is_empty() || id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_keep_every_byte_and_cannot_form_trigraphs_or_extended_escapes() {
        // An octal escape followed by a digit, a trigraph, quotes and non-ASCII bytes.
        assert_eq!(string(b"\x001??=\"\\\n\xff"), "\"\\0001\\?\\?=\\\"\\\\\\012\\377\"");
    }

    #[test]
    fn comments_cannot_be_closed_by_their_text() {
        assert_eq!(comment("a */ b"), "/* a * / b */");
    }
}
