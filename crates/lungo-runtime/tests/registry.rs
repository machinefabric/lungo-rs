//! The runtime's native interface against every Lean release it supports.
//!
//! `tests/data/<version>/toolchain-externs.txt` and `tests/data/<version>/toolchain-exports.txt`
//! are generated from each release by `runtime-tests/ExternInventory.lean`. Every `@[extern]`
//! symbol of a release's `Init` and `Std` must be accounted for, for that release: implemented by
//! a Lean `@[export]` definition (compiled like any other Lean code), by the runtime primitive
//! serving the release with the representation Lean's compiler uses, by a value of the release
//! compiled into the program, or deliberately unsupported with a reason. Every Lean export the
//! runtime calls must exist in every release with the representation the runtime calls it with.
//!
//! Borrowing is not compared: code generation reconciles borrowed and owned object parameters
//! with explicit reference-count operations. Representations (object or a scalar width) are.

use lungo_runtime::exports::REQUIRED;
use lungo_runtime::registry::{self, Intrinsic, Ty};
use lungo_runtime::releases::{LEAN_RELEASES, LeanRelease, TOOLCHAIN_CONSTANT_SYMBOLS, toolchain_constant};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

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

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

fn read(release: &LeanRelease, file: &str) -> String {
    let path = data_dir().join(release.version.to_string()).join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn externs(release: &LeanRelease) -> BTreeMap<String, Entry> {
    inventory(&read(release, "toolchain-externs.txt"), 5)
}

fn exports(release: &LeanRelease) -> BTreeMap<String, Entry> {
    inventory(&read(release, "toolchain-exports.txt"), 4)
}

/// Whether `a` and `b` serve a supported release in common.
fn overlap(a: &Intrinsic, b: &Intrinsic) -> bool {
    LEAN_RELEASES.iter().any(|r| a.releases.contains(r.version) && b.releases.contains(r.version))
}

/// TEST0243: every extern of every supported release is classified for that release
#[test]
fn test0243_every_extern_of_every_release_is_classified() {
    let mut problems = Vec::new();
    for release in LEAN_RELEASES {
        let v = release.version;
        for (symbol, e) in &externs(release) {
            if e.implemented_by.is_some() {
                continue;
            }
            let constant = toolchain_constant(symbol, release);
            match (registry::lookup(symbol, v), constant, registry::unsupported(symbol)) {
                (Some(i), None, None) => {
                    if !same_repr(i.params, &e.params) || !same_repr(&[i.result], &[e.result]) {
                        problems.push(format!(
                            "{v}: {symbol} ({}): Lean uses {}, the runtime implements {}",
                            e.declaration,
                            render(&e.params, e.result),
                            render(i.params, i.result)
                        ));
                    }
                }
                (None, Some(c), None) => {
                    let (params, result) = c.representation();
                    if !same_repr(params, &e.params) || !same_repr(&[result], &[e.result]) {
                        problems.push(format!(
                            "{v}: {symbol} ({}): Lean uses {}, the program's constant is {}",
                            e.declaration,
                            render(&e.params, e.result),
                            render(params, result)
                        ));
                    }
                }
                (None, None, Some(u)) => {
                    assert!(!u.reason.trim().is_empty(), "{symbol} is unsupported without a reason")
                }
                (None, None, None) => {
                    problems.push(format!("{v}: {symbol} ({}) is neither implemented nor classified", e.declaration))
                }
                _ => problems.push(format!(
                    "{v}: {symbol} is classified more than once (primitive, program constant, unsupported)"
                )),
            }
        }
    }
    assert!(problems.is_empty(), "{} unclassified externs:\n{}", problems.len(), problems.join("\n"));
}

/// TEST0244: externs implemented in lean are not shadowed by primitives
#[test]
fn test0244_externs_implemented_in_lean_are_not_shadowed_by_primitives() {
    // Resolution prefers the compiled `@[export]` definition; a primitive for the same symbol
    // would be dead code that silently diverges from the definition Lean actually runs.
    let mut shadowed = Vec::new();
    for release in LEAN_RELEASES {
        for (s, e) in externs(release) {
            if let Some(by) = e.implemented_by
                && (registry::lookup(&s, release.version).is_some() || toolchain_constant(&s, release).is_some())
            {
                shadowed.push(format!("{}: {s} (implemented by {by})", release.version));
            }
        }
    }
    assert!(shadowed.is_empty(), "primitives shadowing Lean implementations:\n{}", shadowed.join("\n"));
}

/// TEST0245: every primitive implements an extern of each release it serves, and serves one
#[test]
fn test0245_every_primitive_implements_an_extern_of_each_release_it_serves() {
    let known: Vec<(&LeanRelease, BTreeSet<String>)> =
        LEAN_RELEASES.iter().map(|r| (r, externs(r).into_keys().collect())).collect();
    let mut problems = Vec::new();
    for i in registry::intrinsics() {
        let served: Vec<_> = known.iter().filter(|(r, _)| i.releases.contains(r.version)).collect();
        if served.is_empty() {
            problems.push(format!("{} serves no supported release", i.function));
        }
        for (r, symbols) in served {
            if !symbols.contains(i.symbol) {
                problems
                    .push(format!("{} serves {} but {} is not one of its externs", i.function, r.version, i.symbol));
            }
        }
    }
    for u in registry::unsupported_symbols() {
        if !known.iter().any(|(_, symbols)| symbols.contains(u.symbol)) {
            problems.push(format!("{} is listed as unsupported but no release declares it", u.symbol));
        }
    }
    for s in TOOLCHAIN_CONSTANT_SYMBOLS {
        for (r, symbols) in &known {
            if !symbols.contains(*s) {
                problems.push(format!("{s} is a program constant but not an extern of {}", r.version));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// TEST0246: one primitive per symbol and release, and one per function
#[test]
fn test0246_one_primitive_per_symbol_and_release_and_one_per_function() {
    let all: Vec<&Intrinsic> = registry::intrinsics().collect();
    let mut problems = Vec::new();
    for (n, a) in all.iter().enumerate() {
        for b in &all[n + 1..] {
            if a.function == b.function {
                problems.push(format!("the function {} is registered twice", a.function));
            } else if a.symbol == b.symbol && overlap(a, b) {
                problems.push(format!("{} and {} both implement {} for a release", a.function, b.function, a.symbol));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// TEST0247: required exports exist in every release with the called representation
#[test]
fn test0247_required_exports_exist_in_every_release_with_the_called_representation() {
    let mut problems = Vec::new();
    for release in LEAN_RELEASES {
        let exports = exports(release);
        for r in REQUIRED {
            match exports.get(r.symbol) {
                None => problems.push(format!("{}: {} is not exported", release.version, r.symbol)),
                Some(e) if !same_repr(r.params, &e.params) || !same_repr(&[r.result], &[e.result]) => {
                    problems.push(format!(
                        "{}: {} ({}): compiled as {}, the runtime calls {}",
                        release.version,
                        r.symbol,
                        e.declaration,
                        render(&e.params, e.result),
                        render(r.params, r.result)
                    ))
                }
                Some(_) => {}
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// TEST0386: there is an inventory for each supported release, and for nothing else
#[test]
fn test0386_there_is_an_inventory_for_each_supported_release_and_nothing_else() {
    let present: BTreeSet<String> =
        std::fs::read_dir(data_dir()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    let supported: BTreeSet<String> = LEAN_RELEASES.iter().map(|r| r.version.to_string()).collect();
    assert_eq!(present, supported, "tests/data holds one directory per supported release");
    for release in LEAN_RELEASES {
        assert!(!externs(release).is_empty() && !exports(release).is_empty(), "{}", release.version);
    }
}
