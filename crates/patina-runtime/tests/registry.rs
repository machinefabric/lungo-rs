//! The runtime's native interface against the Lean toolchain it supports.
//!
//! `tests/data/toolchain-externs.txt` and `tests/data/toolchain-exports.txt` are generated from
//! the toolchain by `runtime-tests/ExternInventory.lean`. Every `@[extern]` symbol of `Init` and
//! `Std` must be accounted for: implemented by a Lean `@[export]` definition (compiled like any
//! other Lean code), implemented by a runtime primitive with the representation Lean's compiler
//! uses, or deliberately unsupported with a reason. Every Lean export the runtime calls must
//! exist with the representation the runtime calls it with.
//!
//! Borrowing is not compared: code generation reconciles borrowed and owned object parameters
//! with explicit reference-count operations. Representations (object or a scalar width) are.

use patina_runtime::exports::REQUIRED;
use patina_runtime::registry::{self, Ty};
use std::collections::{BTreeMap, BTreeSet};

struct Entry {
    declaration: String,
    params: Vec<Ty>,
    result: Ty,
    implemented_by: Option<String>,
}

fn ty(s: &str) -> Ty {
    match s {
        "obj" => Ty::obj,
        "b_obj" => Ty::b_obj,
        "u8" => Ty::u8,
        "u16" => Ty::u16,
        "u32" => Ty::u32,
        "u64" => Ty::u64,
        "usize" => Ty::usize,
        "f64" => Ty::f64,
        "f32" => Ty::f32,
        other => panic!("unknown representation {other:?} in the inventory"),
    }
}

/// Parses `symbol<TAB>declaration<TAB>params<TAB>result[<TAB>implementation]` lines.
fn inventory(text: &str, columns: usize) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), columns, "malformed inventory line {line:?}");
        let params = if cols[2].is_empty() { Vec::new() } else { cols[2].split(',').map(ty).collect() };
        let implemented_by = cols.get(4).filter(|c| **c != "-").map(|c| c.to_string());
        let entry = Entry { declaration: cols[1].to_owned(), params, result: ty(cols[3]), implemented_by };
        // A symbol can be declared by several declarations; they must agree.
        if let Some(prev) = out.get(cols[0]) {
            let prev: &Entry = prev;
            assert!(
                same_repr(&prev.params, &entry.params) && same_repr(&[prev.result], &[entry.result]),
                "{} is declared with different representations by {} and {}",
                cols[0],
                prev.declaration,
                entry.declaration
            );
        }
        out.insert(cols[0].to_owned(), entry);
    }
    out
}

fn is_object(t: Ty) -> bool {
    matches!(t, Ty::obj | Ty::b_obj)
}

fn same_repr(a: &[Ty], b: &[Ty]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y || (is_object(*x) && is_object(*y)))
}

fn render(params: &[Ty], result: Ty) -> String {
    let ps: Vec<&str> = params.iter().map(|t| t.name()).collect();
    format!("({}) -> {}", ps.join(", "), result.name())
}

fn externs() -> BTreeMap<String, Entry> {
    inventory(include_str!("data/toolchain-externs.txt"), 5)
}

fn exports() -> BTreeMap<String, Entry> {
    inventory(include_str!("data/toolchain-exports.txt"), 4)
}

#[test]
fn every_toolchain_extern_is_classified() {
    let mut problems = Vec::new();
    for (symbol, e) in &externs() {
        if e.implemented_by.is_some() {
            continue;
        }
        match (registry::lookup(symbol), registry::unsupported(symbol)) {
            (Some(_), Some(_)) => problems.push(format!("{symbol} is both implemented and listed as unsupported")),
            (Some(i), None) => {
                if !same_repr(i.params, &e.params) || !same_repr(&[i.result], &[e.result]) {
                    problems.push(format!(
                        "{symbol} ({}): Lean uses {}, the runtime implements {}",
                        e.declaration,
                        render(&e.params, e.result),
                        render(i.params, i.result)
                    ));
                }
            }
            (None, Some(u)) => assert!(!u.reason.trim().is_empty(), "{symbol} is unsupported without a reason"),
            (None, None) => {
                problems.push(format!("{symbol} ({}) is neither implemented nor classified", e.declaration))
            }
        }
    }
    assert!(problems.is_empty(), "{} unclassified externs:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn externs_implemented_in_lean_are_not_shadowed_by_primitives() {
    // Resolution prefers the compiled `@[export]` definition; a primitive for the same symbol
    // would be dead code that silently diverges from the definition Lean actually runs.
    let shadowed: Vec<String> = externs()
        .iter()
        .filter(|(s, e)| e.implemented_by.is_some() && registry::lookup(s).is_some())
        .map(|(s, e)| format!("{s} (implemented by {})", e.implemented_by.as_deref().unwrap_or_default()))
        .collect();
    assert!(shadowed.is_empty(), "primitives shadowing Lean implementations:\n{}", shadowed.join("\n"));
}

#[test]
fn registry_has_no_symbols_unknown_to_the_toolchain() {
    let known: BTreeSet<String> = externs().into_keys().collect();
    let unknown: Vec<&str> = registry::intrinsics()
        .map(|i| i.symbol)
        .chain(registry::unsupported_symbols().map(|u| u.symbol))
        .filter(|s| !known.contains(*s))
        .collect();
    assert!(unknown.is_empty(), "registry entries that are not toolchain externs: {unknown:?}");
}

#[test]
fn registry_symbols_are_unique() {
    let mut seen = BTreeSet::new();
    let dups: Vec<&str> = registry::intrinsics().map(|i| i.symbol).filter(|s| !seen.insert(*s)).collect();
    assert!(dups.is_empty(), "primitives registered twice: {dups:?}");
}

#[test]
fn required_exports_exist_with_the_called_representation() {
    let exports = exports();
    let mut problems = Vec::new();
    for r in REQUIRED {
        match exports.get(r.symbol) {
            None => problems.push(format!("{} is not exported by the toolchain", r.symbol)),
            Some(e) if !same_repr(r.params, &e.params) || !same_repr(&[r.result], &[e.result]) => {
                problems.push(format!(
                    "{} ({}): compiled as {}, the runtime calls {}",
                    r.symbol,
                    e.declaration,
                    render(&e.params, e.result),
                    render(r.params, r.result)
                ))
            }
            Some(_) => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
