//! `lungo` commands against the design example (`examples/session`), whose `lungo.toml`
//! mirrors its `build.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("examples/session")
}

/// Runs `lungo <args>` in the example package; returns (success, stdout, stderr).
fn cli(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lungo")).args(args).current_dir(package()).output().unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

#[track_caller]
fn ok(args: &[&str]) -> String {
    let (success, stdout, stderr) = cli(args);
    assert!(success, "lungo {args:?} failed:\n{stdout}\n{stderr}");
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
fn generate_writes_rust_then_reuses_it() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-out");
    let flag = format!("--rust_out={}", out.display());
    let first = ok(&["generate", &flag]);
    assert!(first.starts_with("generated rust into ") || first.starts_with("up to date: rust "), "{first}");
    let second = ok(&["generate", &flag]);
    let module = out.join("formal");
    assert_eq!(second.trim_end(), format!("up to date: rust {}", module.display()));
    for f in ["formal.rs", "names.json", "externs.json", "sources.json", "manifest.json", "build-info.json"] {
        assert!(module.join(f).is_file(), "{f} is published");
    }
}

#[test]
fn generate_writes_the_outputs_the_configuration_names() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let out = tmp.join("configured-out");
    let config = tmp.join("out-dir.toml");
    let project = package().join("lean");
    std::fs::write(
        &config,
        format!("project = {:?}\n\n[rust]\nout-dir = {:?}\n", project.to_str().unwrap(), out.to_str().unwrap()),
    )
    .unwrap();
    let stdout = ok(&["generate", "--config", config.to_str().unwrap()]);
    assert!(stdout.trim_end().ends_with(&*out.join("formal").to_string_lossy()), "{stdout}");
    assert!(out.join("formal").join("formal.rs").is_file());
}

#[test]
fn generate_refuses_to_replace_a_directory_it_did_not_create() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("foreign-out");
    let dir = tmp.join("formal");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mine.txt"), "user data").unwrap();
    let (success, _, stderr) = cli(&["generate", &format!("--rust_out={}", tmp.display())]);
    assert!(!success);
    assert!(stderr.contains("error[LNG0107]") && stderr.contains("not a directory lungo generated"), "{stderr}");
    assert_eq!(std::fs::read_to_string(dir.join("mine.txt")).unwrap(), "user data");
}

#[test]
fn a_development_build_requires_a_local_distribution_for_bindings() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-runtime");
    let (success, _, stderr) = cli(&["generate", &format!("--c_out={}", out.display())]);
    assert!(!success);
    assert!(stderr.contains("error[LNG0108]") && stderr.contains("--runtime-dir"), "{stderr}");
}

#[test]
fn unknown_generators_are_plugins_that_must_exist() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-plugin");
    let (success, _, stderr) = cli(&["generate", &format!("--nonexistent_out={}", out.display())]);
    assert!(!success);
    assert!(stderr.contains("error[LNG0504]") && stderr.contains("lungo-gen-nonexistent"), "{stderr}");
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
fn rust_finds_facade_functions_without_documentation() {
    let config = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-comments.toml");
    std::fs::write(
        &config,
        format!(
            "project = {:?}\n\n[rust]\ndisable-comments = [\"Formal.run\"]\n",
            package().join("lean").to_str().unwrap()
        ),
    )
    .unwrap();
    let out = ok(&["rust", "Formal.run", "--config", config.to_str().unwrap()]);
    assert!(out.contains("pub fn run("), "the facade function is shown:\n{out}");
    assert!(!out.contains("/// Lean: `Formal.run"), "its documentation is disabled:\n{out}");
}

#[test]
fn externs_and_mappings_are_machine_readable() {
    let externs: serde_json::Value = serde_json::from_str(&ok(&["externs"])).unwrap();
    let nat_add =
        externs.as_array().unwrap().iter().find(|e| e["declaration"] == "Nat.add").expect("Formal.run reaches Nat.add");
    assert_eq!(nat_add["key"], "lean_nat_add");
    assert_eq!(nat_add["resolution"], "runtime");
    assert_eq!(nat_add["implementation"], "lungo::__runtime::intrinsics::lean_nat_add");
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
    let out = Command::new(env!("CARGO_BIN_EXE_lungo"))
        .arg("check")
        .current_dir(Path::new(env!("CARGO_TARGET_TMPDIR")))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no lungo configuration"));
}
