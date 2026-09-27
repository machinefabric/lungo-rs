//! Layout fingerprints of the program's named types.
//!
//! Two generated packages can share a Lean type: a program built on another's model passes the
//! other program's values through its own compiled code, which reads their fields at the layout
//! it was compiled for. That is sound only when both were generated from the same definition of
//! the type — and of every type its fields reach. A package that uses another package's type
//! (an *extern type*) therefore checks that the two agree on the type's fingerprint before it
//! runs: at compile time in Rust, when the package is loaded in every other language.
//!
//! A fingerprint is the SHA-256 of a canonical description of the type's layout: its
//! constructors, their field layouts and the types of their fields, with every named type a
//! field reaches described in place (so a change anywhere in the closure changes it), and the
//! Lean version, whose compiler decides the layout of the types the program does not describe.

use lungo_protocol::{FacadeType, FieldKind, TypeDecl};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// The version of the canonical description; part of every fingerprint.
const FORMAT: &str = "lungo-layout-1";

/// The fingerprint (lowercase hexadecimal SHA-256) of every type in `types`, by Lean name.
pub fn fingerprints(types: &[TypeDecl], lean_version: &str) -> BTreeMap<String, String> {
    let by_name: HashMap<&str, &TypeDecl> = types.iter().map(|t| (t.name.as_str(), t)).collect();
    types
        .iter()
        .map(|t| {
            let mut text = format!("{FORMAT}\nlean {lean_version}\n");
            describe(&by_name, t, &mut Vec::new(), &mut text);
            let digest = Sha256::digest(text.as_bytes());
            (t.name.clone(), digest.iter().map(|b| format!("{b:02x}")).collect())
        })
        .collect()
}

/// Appends the description of `t`. `open` holds the types being described around it: a type
/// reached again from inside its own description is recursive, and is named rather than
/// described again.
fn describe<'t>(by_name: &HashMap<&str, &'t TypeDecl>, t: &'t TypeDecl, open: &mut Vec<&'t str>, out: &mut String) {
    open.push(&t.name);
    out.push_str(&format!(
        "type {} opaque={} params={} repr={} trivial={}\n",
        t.name,
        t.opaque,
        t.params.len(),
        t.repr.name(),
        t.trivial.as_ref().map(|tr| format!("{}.{}", tr.ctor, tr.field)).unwrap_or_else(|| "-".to_owned())
    ));
    for c in &t.ctors {
        out.push_str(&format!("ctor {} tag={} size={} usize={} ssize={}\n", c.name, c.tag, c.size, c.usize, c.ssize));
        for f in &c.fields {
            let kind = match f.kind {
                FieldKind::Object(i) => format!("object {i}"),
                FieldKind::Usize(i) => format!("usize {i}"),
                FieldKind::Scalar { size, offset, ty } => format!("scalar {size} {offset} {}", ty.name()),
                FieldKind::Erased => "erased".to_owned(),
                FieldKind::Void => "void".to_owned(),
            };
            out.push_str(&format!("field {} {kind} : ", f.name));
            ty(by_name, &f.ty, open, out);
            out.push('\n');
        }
    }
    out.push_str("end\n");
    open.pop();
}

/// Appends the description of type expression `ft`.
fn ty<'t>(by_name: &HashMap<&str, &'t TypeDecl>, ft: &FacadeType, open: &mut Vec<&'t str>, out: &mut String) {
    let named = |name: &str, open: &mut Vec<&'t str>, out: &mut String| match by_name.get(name) {
        Some(_) if open.contains(&name) => out.push_str(&format!("(rec {name})")),
        Some(t) => {
            out.push('(');
            describe(by_name, t, open, out);
            out.push(')');
        }
        // Not described: a type the program's compiler lays out (the Lean version covers it).
        None => out.push_str(&format!("(external {name})")),
    };
    let apply = |head: &str, args: &[&FacadeType], open: &mut Vec<&'t str>, out: &mut String| {
        out.push('(');
        out.push_str(head);
        for a in args {
            out.push(' ');
            ty(by_name, a, open, out);
        }
        out.push(')');
    };
    match ft {
        FacadeType::Nat => out.push_str("nat"),
        FacadeType::Int => out.push_str("int"),
        FacadeType::Bool => out.push_str("bool"),
        FacadeType::Uint8 => out.push_str("uint8"),
        FacadeType::Uint16 => out.push_str("uint16"),
        FacadeType::Uint32 => out.push_str("uint32"),
        FacadeType::Uint64 => out.push_str("uint64"),
        FacadeType::Usize => out.push_str("usize"),
        FacadeType::Int8 => out.push_str("int8"),
        FacadeType::Int16 => out.push_str("int16"),
        FacadeType::Int32 => out.push_str("int32"),
        FacadeType::Int64 => out.push_str("int64"),
        FacadeType::Isize => out.push_str("isize"),
        FacadeType::Float => out.push_str("float"),
        FacadeType::Float32 => out.push_str("float32"),
        FacadeType::Char => out.push_str("char"),
        FacadeType::String => out.push_str("string"),
        FacadeType::Unit => out.push_str("unit"),
        FacadeType::ByteArray => out.push_str("byte_array"),
        FacadeType::FloatArray => out.push_str("float_array"),
        FacadeType::Option(t) => apply("option", &[t], open, out),
        FacadeType::List(t) => apply("list", &[t], open, out),
        FacadeType::Array(t) => apply("array", &[t], open, out),
        FacadeType::Io(t) => apply("io", &[t], open, out),
        FacadeType::BaseIo(t) => apply("base_io", &[t], open, out),
        FacadeType::Prod(a, b) => apply("prod", &[a, b], open, out),
        FacadeType::Except { error, value } => apply("except", &[error, value], open, out),
        FacadeType::Eio { error, value } => apply("eio", &[error, value], open, out),
        FacadeType::Function { params, result } => {
            let mut all: Vec<&FacadeType> = params.iter().collect();
            all.push(result);
            apply("function", &all, open, out)
        }
        FacadeType::Param(i) => out.push_str(&format!("(param {i})")),
        FacadeType::Inductive { name, args } => {
            out.push_str("(inductive ");
            named(name, open, out);
            for a in args {
                out.push(' ');
                ty(by_name, a, open, out);
            }
            out.push(')');
        }
        FacadeType::Opaque { head: Some(h), lean_type } if by_name.contains_key(h.as_str()) => {
            out.push_str("(opaque ");
            named(h, open, out);
            out.push_str(&format!(" {lean_type:?})"));
        }
        // A value only the program knows how to read (a proof's statement, a function, an
        // undescribed type): its Lean type is all there is to compare.
        FacadeType::Opaque { lean_type, .. } => out.push_str(&format!("(opaque {lean_type:?})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lungo_bir::IrType;
    use lungo_protocol::{CtorDecl, FieldDecl};

    fn field(name: &str, ty: FacadeType, kind: FieldKind) -> FieldDecl {
        FieldDecl { name: name.into(), ty, kind }
    }

    /// `structure Urn where tags : List Tag`, `structure Tag where key : String`, and an opaque
    /// `Wf` holding an `Urn` and a proof.
    fn model(tag_key: FacadeType) -> Vec<TypeDecl> {
        let decl = |name: &str, opaque: bool, fields: Vec<FieldDecl>| TypeDecl {
            name: name.into(),
            opaque,
            params: vec![],
            repr: IrType::Object,
            trivial: None,
            structure: true,
            ctors: vec![CtorDecl {
                name: format!("{name}.mk"),
                tag: 0,
                size: fields.iter().filter(|f| matches!(f.kind, FieldKind::Object(_))).count() as u32,
                usize: 0,
                ssize: 0,
                fields,
            }],
        };
        let inductive = |n: &str| FacadeType::Inductive { name: n.into(), args: vec![] };
        vec![
            decl("Wf", true, vec![
                field("urn", inductive("Urn"), FieldKind::Object(0)),
                field("sorted", FacadeType::Opaque { head: None, lean_type: "Sorted urn".into() }, FieldKind::Erased),
            ]),
            decl("Urn", false, vec![field("tags", FacadeType::List(Box::new(inductive("Tag"))), FieldKind::Object(0))]),
            decl("Tag", false, vec![field("key", tag_key, FieldKind::Object(0))]),
        ]
    }

    #[test]
    fn a_change_anywhere_in_the_closure_changes_the_fingerprint() {
        let before = fingerprints(&model(FacadeType::String), "4.34.1");
        assert_eq!(before, fingerprints(&model(FacadeType::String), "4.34.1"), "deterministic");
        // `Tag` is reached from `Wf` only through `Urn`'s list: changing it changes all three.
        let after = fingerprints(&model(FacadeType::Nat), "4.34.1");
        for name in ["Wf", "Urn", "Tag"] {
            assert_ne!(before[name], after[name], "{name}");
        }
        // Another Lean version may lay out what the program does not describe differently.
        assert_ne!(before["Wf"], fingerprints(&model(FacadeType::String), "4.35.0")["Wf"]);
        // Distinct types have distinct fingerprints even where their layouts coincide.
        assert_ne!(before["Urn"], before["Tag"]);
    }

    #[test]
    fn recursive_types_are_named_where_they_recur() {
        let t = TypeDecl {
            name: "Tree".into(),
            opaque: false,
            params: vec![],
            repr: IrType::Object,
            trivial: None,
            structure: false,
            ctors: vec![CtorDecl {
                name: "Tree.node".into(),
                tag: 0,
                size: 1,
                usize: 0,
                ssize: 0,
                fields: vec![field(
                    "children",
                    FacadeType::List(Box::new(FacadeType::Inductive { name: "Tree".into(), args: vec![] })),
                    FieldKind::Object(0),
                )],
            }],
        };
        let one = fingerprints(std::slice::from_ref(&t), "4.34.1");
        assert_eq!(one["Tree"].len(), 64);
        // A different recursive structure (a field more) is a different layout.
        let mut wider = t.clone();
        wider.ctors[0].fields.push(field("label", FacadeType::String, FieldKind::Object(1)));
        wider.ctors[0].size = 2;
        assert_ne!(one["Tree"], fingerprints(&[wider], "4.34.1")["Tree"]);
    }
}
