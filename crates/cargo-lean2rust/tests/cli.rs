//! `cargo lean2rust` commands against the design example (`examples/session`), whose
//! `lean2rust.toml` mirrors its `build.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/session").canonicalize().unwrap()
}

/// Runs `cargo lean2rust <args>` in the example package; returns (success, stdout, stderr).
fn cli(args: &[&str]) -> (bool, String, String) {
    let out_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-out");
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-lean2rust"))
        .arg("lean2rust")
        .args(args)
        .arg("--package-dir")
        .arg(package())
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

#[track_caller]
fn ok(args: &[&str]) -> String {
    let (success, stdout, stderr) = cli(args);
    assert!(success, "cargo lean2rust {args:?} failed:\n{stdout}\n{stderr}");
    stdout
}

#[test]
fn prepare_reports_toolchain_and_worker() {
    let out = ok(&["prepare"]);
    assert!(out.contains("Lean 4.34.1 (5045d0056413266e57c625dcd7c365b10e377c52)"), "{out}");
    assert!(out.contains("worker "), "{out}");
}

#[test]
fn check_summarizes_the_program() {
    let out = ok(&["check"]);
    assert!(out.contains("Lean 4.34.1"), "{out}");
    assert!(out.contains("modules,") && out.contains("compiled declarations"), "{out}");
    assert!(out.contains("generation succeeded"), "{out}");
}

#[test]
fn build_generates_then_reuses() {
    let first = ok(&["build"]);
    assert!(first.starts_with("generated ") || first.starts_with("up to date: "), "{first}");
    let second = ok(&["build"]);
    assert!(second.starts_with("up to date: "), "{second}");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-out");
    for f in ["formal.rs", "names.json", "externs.json", "sources.json", "manifest.json", "build-info.json"] {
        assert!(out.join(f).is_file(), "{f} is published");
    }
}

#[test]
fn inspect_shows_type_source_and_trust() {
    let out = ok(&["inspect", "Formal.apply"]);
    assert!(out.contains("Formal.apply : Formal.Op → Formal.Sess → Option Formal.Sess"), "{out}");
    assert!(out.contains("module: Formal.Session"), "{out}");
    assert!(out.contains("source: lean/Formal/Session.lean"), "{out}");
    assert!(out.contains("depends on sorry: false"), "{out}");
    assert!(out.contains("compiled: ("), "{out}");
    let (success, _, stderr) = cli(&["inspect", "Formal.nope"]);
    assert!(!success);
    assert!(stderr.contains("Formal.nope is neither exported nor part of the compiled program"), "{stderr}");
}

#[test]
fn ir_prints_bridge_ir() {
    let out = ok(&["ir", "Formal.run"]);
    assert!(out.contains("def Formal.run"), "{out}");
    assert!(out.contains("case[tobj] x_1 : tobj of"), "{out}");
    assert!(out.contains("def Formal.run._boxed"), "compiler auxiliaries are included:\n{out}");
}

#[test]
fn rust_prints_generated_code() {
    let out = ok(&["rust", "Formal.run"]);
    assert!(out.contains("// Lean: Formal.run"), "{out}");
    assert!(out.contains("pub fn run("), "the facade function is shown:\n{out}");
}

#[test]
fn externs_and_mappings_are_machine_readable() {
    let externs: serde_json::Value = serde_json::from_str(&ok(&["externs"])).unwrap();
    let nat_add =
        externs.as_array().unwrap().iter().find(|e| e["declaration"] == "Nat.add").expect("Formal.run reaches Nat.add");
    assert_eq!(nat_add["key"], "lean_nat_add");
    assert_eq!(nat_add["resolution"], "runtime");
    assert_eq!(nat_add["implementation"], "lean2rust::__runtime::intrinsics::lean_nat_add");
    let names = ok(&["mappings"]);
    let records: serde_json::Value = serde_json::from_str(&names).unwrap();
    let field = records.as_array().unwrap().iter().find(|r| r["lean_name"] == "Formal.Sess.isOpen").unwrap();
    assert_eq!(
        (&field["kind"], &field["rust_path"], &field["renamed"]),
        (&"field".into(), &"Sess.is_open".into(), &true.into())
    );
    for needle in [
        "\"Formal.apply\"",
        "\"Formal.run\"",
        "\"Formal.Sess\"",
        "\"Formal.Sess.isOpen\"",
        "\"Sess.is_open\"",
        "\"Formal.Op.open\"",
        "\"Op::Open\"",
    ] {
        assert!(names.contains(needle), "names.json maps {needle}:\n{names}");
    }
}

#[test]
fn configuration_errors_are_reported() {
    let out = Command::new(env!("CARGO_BIN_EXE_cargo-lean2rust"))
        .args(["lean2rust", "check", "--package-dir"])
        .arg(Path::new(env!("CARGO_TARGET_TMPDIR")))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no lean2rust configuration"));
}
