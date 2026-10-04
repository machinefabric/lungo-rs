//! Deterministic naming.
//!
//! Compiler-layer items use an injective mangling of the canonical Lean name, so distinct Lean
//! declarations can never collide. Public facade items use idiomatic Rust names derived from the
//! Lean name; sanitization is lossy, so collisions are resolved deterministically and every
//! mapping is recorded in `names.json`, keyed by the canonical Lean name.

use std::fmt::Write as _;

/// Lean's fixed-width integer type names, which read as single words.
const INTEGER_TYPE_WORDS: &[(&str, &str)] = &[("UInt", "Uint"), ("USize", "Usize"), ("ISize", "Isize")];

/// Splits a Lean identifier component into words for case conversion.
pub fn words(s: &str) -> Vec<String> {
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

/// Transliterates characters outside the ASCII identifier alphabet shared by every generated
/// language (Lean allows Unicode letters,
/// subscripts, `!`, `?`, `'`, ...) into ASCII.
pub fn ascii_word(w: &str) -> String {
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

pub fn symbol_suffix(s: &str) -> String {
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

/// Lean component → `snake_case` words, before any language's keyword escaping.
pub fn snake_case(s: &str) -> String {
    let base: Vec<String> = words(s).iter().map(|w| ascii_word(&w.to_lowercase())).collect();
    let mut id = base.join("_");
    id.push_str(&symbol_suffix(s));
    if id.is_empty() {
        id.push('_');
    }
    if id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, '_');
    }
    id
}

/// Lean component → `UpperCamelCase` words, before any language's keyword escaping.
pub fn upper_camel_case(s: &str) -> String {
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
    id
}

/// Lean component → `lowerCamelCase` words, before any language's keyword escaping.
pub fn lower_camel_case(s: &str) -> String {
    let upper = upper_camel_case(s);
    let mut cs = upper.chars();
    match cs.next() {
        Some(first) if first != '_' => first.to_lowercase().chain(cs).collect(),
        _ => upper,
    }
}

/// Injective mangling of a canonical Lean name into a Rust identifier.
///
/// ASCII letters and digits are kept; `_` becomes `__`; `.` becomes `_d`; any other character
/// becomes `_x<hex>_`. Each escape starts with `_` followed by a distinct marker, so the
/// encoding is prefix-free and injective.
pub fn mangle(lean_name: &str) -> String {
    let mut out = String::with_capacity(lean_name.len() + 2);
    out.push_str("l_");
    for c in lean_name.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => out.push(c),
            '_' => out.push_str("__"),
            '.' => out.push_str("_d"),
            _ => {
                out.push_str("_x");
                out.push_str(&format!("{:x}", c as u32));
                out.push('_');
            }
        }
    }
    out
}

/// Splits a canonical (escaped) Lean name into its components.
///
/// Escaped components are enclosed in `«»`; numeric components are digits.
pub fn components(lean_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_escape = false;
    for c in lean_name.chars() {
        match c {
            '«' if !in_escape => in_escape = true,
            '»' if in_escape => in_escape = false,
            '.' if !in_escape => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The file stem of a Lean module's compiler-layer file: an injective, readable encoding of
/// the module name (`.` becomes `-`; characters other than ASCII letters, digits and `_` become
/// `+<hex>+`).
pub fn module_file_stem(module: &str) -> String {
    let mut out = String::new();
    for c in module.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => out.push(c),
            '.' => out.push('-'),
            _ => out.push_str(&format!("+{:x}+", c as u32)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0123: mangling is injective on ambiguous spellings
    #[test]
    fn test0123_mangling_is_injective_on_ambiguous_spellings() {
        let names = ["a.b", "a_b", "a__b", "a.«b.c»", "a._d", "a_d", "«a.b»", "x'", "x_x27_"];
        let mangled: std::collections::HashSet<_> = names.iter().map(|n| mangle(n)).collect();
        assert_eq!(mangled.len(), names.len());
    }

    /// TEST0124: module file stems are injective
    #[test]
    fn test0124_module_file_stems_are_injective() {
        let modules = ["A.B_C", "A_B.C", "A._B", "A_.B", "A.«b-c»", "A.b.c", "A.«b.c»"];
        let stems: std::collections::HashSet<_> = modules.iter().map(|m| module_file_stem(m)).collect();
        assert_eq!(stems.len(), modules.len());
        assert_eq!(module_file_stem("Init.Data.List.Basic"), "Init-Data-List-Basic");
    }

    /// TEST0125: components respect escapes
    #[test]
    fn test0125_components_respect_escapes() {
        assert_eq!(components("Formal.Op.«open»"), vec!["Formal", "Op", "open"]);
        assert_eq!(components("a.«b.c».d"), vec!["a", "b.c", "d"]);
    }
}
