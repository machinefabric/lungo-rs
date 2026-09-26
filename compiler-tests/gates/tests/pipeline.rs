//! Release gates of the build pipeline (design §49), exercised on untouched Lake projects.
//!
//! Each test copies a fixture from `fixtures/` into its own scratch directory and runs the same
//! library pipeline `build.rs` runs (`Config::run`) against it.

use patina_build::{Config, Environment, Error, ErrorCode};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A scratch copy of a fixture project inside a scratch "Cargo package".
struct Scratch {
    /// The package directory (`CARGO_MANIFEST_DIR` of the simulated build).
    package: PathBuf,
}

impl Scratch {
    fn new(fixture: &str, test: &str) -> Scratch {
        let package = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gates").join(test);
        if package.exists() {
            std::fs::remove_dir_all(&package).unwrap();
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(fixture);
        copy_tree(&src, &package.join("lean"));
        Scratch { package }
    }

    fn project(&self) -> PathBuf {
        self.package.join("lean")
    }

    fn env(&self, out: &str) -> Environment {
        Environment::native(self.package.clone(), self.package.join(out), self.package.join(format!("{out}-work")))
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.project().join(rel), text).unwrap();
    }
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".lake" {
            continue;
        }
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}

/// Every file under `dir` (relative path → contents), skipping `skip` directory names.
fn snapshot(dir: &Path, skip: &[&str]) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, skip: &[&str], out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if skip.contains(&name.as_str()) {
                continue;
            }
            let p = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &p, skip, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, skip, &mut out);
    out
}

fn imports_config() -> Config {
    Config::new("lean").root_module("Imports").export_module("Imports")
}

#[test]
fn changing_an_imported_file_rebuilds() {
    let s = Scratch::new("imports", "rebuild");
    let env = s.env("out");
    let cfg = imports_config();
    let first = cfg.run(&env).unwrap();
    assert!(!first.reused);
    let base = s.project().join("Imports/Base.lean").canonicalize().unwrap();
    assert!(
        first.inputs.iter().any(|p| p.canonicalize().ok().as_deref() == Some(&base)),
        "imported local modules are build inputs (for cargo::rerun-if-changed): {:?}",
        first.inputs
    );
    let before = snapshot(&env.out_dir, &[]);

    // Unchanged inputs reuse the published output.
    assert!(cfg.run(&env).unwrap().reused);

    // Editing only the imported module regenerates the Rust.
    s.write(
        "Imports/Base.lean",
        "namespace Imports.Base\n\ndef factor : Nat := 7\n\ndef offset : Nat := 1\n\ndef salutation : String := \"Howdy\"\n\nend Imports.Base\n",
    );
    let second = cfg.run(&env).unwrap();
    assert!(!second.reused, "a change to an imported file must rebuild");
    let after = snapshot(&env.out_dir, &[]);
    let changed: Vec<&String> = after.keys().filter(|k| before.get(*k) != after.get(*k)).collect();
    assert!(
        changed.iter().any(|k| k.starts_with("modules/Imports-Base")),
        "the imported module's generated code changes: {changed:?}"
    );
    let base_rs = String::from_utf8(after["modules/Imports-Base.rs"].clone()).unwrap();
    assert!(base_rs.contains("Howdy"), "the new string literal is compiled");
}

#[test]
fn invalid_lean_fails_with_source_diagnostics() {
    let s = Scratch::new("type-error", "type-error");
    let err = Config::new("lean").root_module("TypeError").run(&s.env("out")).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, Error::LeanElaboration { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::LeanRejected);
    assert!(text.starts_with("error[PTN0201]: "), "{text}");
    assert!(text.contains("TypeError.lean:5:"), "the diagnostic names the source position:\n{text}");
    assert!(text.contains("error"), "{text}");
    assert!(!s.package.join("out").exists(), "no output is published for a failed build");
}

#[test]
fn proofs_are_checked_during_the_build() {
    let s = Scratch::new("false-proof", "false-proof");
    let err = Config::new("lean").root_module("FalseProof").run(&s.env("out")).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, Error::LeanElaboration { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::LeanRejected);
    assert!(text.contains("FalseProof.lean:7:"), "the failing proof is reported at its source:\n{text}");
    assert!(text.contains("decide"), "{text}");
}

#[test]
fn toolchain_mismatch_is_detected_before_compilation() {
    let s = Scratch::new("imports", "toolchain-mismatch");
    for pin in ["leanprover/lean4:v4.35.0", "leanprover/lean4:stable", "leanprover/lean4:nightly-2026-01-01"] {
        s.write("lean-toolchain", &format!("{pin}\n"));
        let err = imports_config().run(&s.env("out")).unwrap_err();
        assert!(matches!(&err, Error::UnsupportedToolchain { found, .. } if found == pin), "{err:?}");
        assert!(err.to_string().contains("v4.34.1"), "the error names the supported toolchains: {err}");
        assert!(err.to_string().starts_with("error[PTN0102]: "), "{err}");
    }
    assert!(!s.project().join(".lake").exists(), "nothing was compiled");
}

#[test]
fn unknown_extern_symbols_are_hard_errors() {
    let s = Scratch::new("unknown-extern", "unknown-extern");
    let err = Config::new("lean").root_module("Unknown").export_module("Unknown").run(&s.env("out")).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, Error::Codegen { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::UnresolvedExtern);
    assert!(text.starts_with("error[PTN0401]: unresolved Lean external symbol"), "{text}");
    for needle in ["unresolved Lean external symbol", "provider_send", "Unknown.providerSend", "Unknown.lean"] {
        assert!(text.contains(needle), "the error mentions {needle}:\n{text}");
    }
    assert!(!s.package.join("out").exists(), "no output is published for a failed build");
}

#[test]
fn unused_rust_extern_mappings_are_rejected() {
    let s = Scratch::new("imports", "unused-mapping");
    let err = imports_config().rust_extern("never_declared", "crate::f").run(&s.env("out")).unwrap_err();
    assert!(err.to_string().contains("never_declared"), "{err}");
    assert_eq!(err.code(), ErrorCode::UnusedExternMapping);
}

#[test]
fn builds_are_deterministic_and_location_independent() {
    // Two copies of the same project at different locations, built independently.
    let a = Scratch::new("imports", "determinism-a");
    let b = Scratch::new("imports", "determinism-b");
    imports_config().run(&a.env("out")).unwrap();
    imports_config().run(&b.env("out")).unwrap();
    let sa = snapshot(&a.package.join("out"), &[]);
    let sb = snapshot(&b.package.join("out"), &[]);
    assert_eq!(sa.keys().collect::<Vec<_>>(), sb.keys().collect::<Vec<_>>());
    for (k, v) in &sa {
        if k == "build-info.json" {
            continue; // Records the build key, which identifies the project's location.
        }
        assert!(v == &sb[k], "{k} differs between identical builds");
    }
    // A forced regeneration in place reproduces the same bytes.
    std::fs::remove_file(a.package.join("out/build-info.json")).unwrap();
    imports_config().run(&a.env("out")).unwrap();
    assert_eq!(snapshot(&a.package.join("out"), &["build-info.json"]), {
        let mut s = sa.clone();
        s.remove("build-info.json");
        s
    });
}

#[test]
fn generated_artifacts_contain_no_machine_specific_paths() {
    let s = Scratch::new("imports", "no-abs-paths");
    imports_config().embed_sources(true).run(&s.env("out")).unwrap();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap();
    let forbidden = [
        s.package.to_string_lossy().into_owned(),
        s.package.canonicalize().unwrap().to_string_lossy().into_owned(),
        home,
        env!("CARGO_TARGET_TMPDIR").to_owned(),
    ];
    for (file, bytes) in snapshot(&s.package.join("out"), &[]) {
        if file == "build-info.json" {
            continue; // Cargo-side bookkeeping, never compiled into the crate.
        }
        let text = String::from_utf8_lossy(&bytes);
        for f in &forbidden {
            assert!(!text.contains(f.as_str()), "{file} contains the machine-specific path {f}");
        }
    }
}

#[test]
fn builds_never_rewrite_the_lean_project() {
    let s = Scratch::new("imports", "no-rewrite");
    let before = snapshot(&s.project(), &[".lake"]);
    imports_config().run(&s.env("out")).unwrap();
    let after = snapshot(&s.project(), &[".lake"]);
    assert_eq!(before, after, "the build modified files of the Lean project");
}
