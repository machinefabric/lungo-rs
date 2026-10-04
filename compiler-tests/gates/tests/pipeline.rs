//! Release gates of the build pipeline (design §49), exercised on untouched Lake projects.
//!
//! Each test copies a fixture from `fixtures/` into its own scratch directory and runs the same
//! library pipeline `build.rs` runs (`Builder::run`) against it.

use lungo_build::{BuildOutcome, Builder, Environment, Error, ErrorCode, configure};
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

/// The `imports` fixture's default configuration: its default target `Imports`, exported.
fn imports_config() -> Builder {
    configure()
}

impl Scratch {
    /// Builds the scratch project with `cfg`, publishing under `<package>/<out>`.
    fn build(&self, cfg: &Builder, out: &str) -> lungo_build::Result<BuildOutcome> {
        cfg.run(Path::new("lean"), &self.env(out))
    }

    /// The published module `name` under `<package>/<out>`.
    fn module(&self, out: &str, name: &str) -> PathBuf {
        self.package.join(out).join(name)
    }
}

/// TEST0029: changing an imported file rebuilds
#[test]
fn test0029_changing_an_imported_file_rebuilds() {
    let s = Scratch::new("imports", "rebuild");
    let cfg = imports_config();
    let first = s.build(&cfg, "out").unwrap();
    assert_eq!(first.name, "imports", "the module is named after the Lake package");
    assert!(!first.reused);
    let base = s.project().join("Imports/Base.lean").canonicalize().unwrap();
    assert!(
        first.inputs.iter().any(|p| p.canonicalize().ok().as_deref() == Some(&base)),
        "imported local modules are build inputs (for cargo::rerun-if-changed): {:?}",
        first.inputs
    );
    let module = s.module("out", "imports");
    let before = snapshot(&module, &[]);
    assert!(before.contains_key("imports.rs"), "the aggregate is `<name>/<name>.rs`: {:?}", before.keys());

    // Unchanged inputs reuse the published output.
    assert!(s.build(&cfg, "out").unwrap().reused);

    // Editing only the imported module regenerates the Rust.
    s.write(
        "Imports/Base.lean",
        "namespace Imports.Base\n\ndef factor : Nat := 7\n\ndef offset : Nat := 1\n\ndef salutation : String := \"Howdy\"\n\nend Imports.Base\n",
    );
    let second = s.build(&cfg, "out").unwrap();
    assert!(!second.reused, "a change to an imported file must rebuild");
    let after = snapshot(&module, &[]);
    let changed: Vec<&String> = after.keys().filter(|k| before.get(*k) != after.get(*k)).collect();
    assert!(
        changed.iter().any(|k| k.starts_with("modules/Imports-Base")),
        "the imported module's generated code changes: {changed:?}"
    );
    let base_rs = String::from_utf8(after["modules/Imports-Base.rs"].clone()).unwrap();
    assert!(base_rs.contains("Howdy"), "the new string literal is compiled");
}

/// TEST0030: invalid lean fails with source diagnostics
#[test]
fn test0030_invalid_lean_fails_with_source_diagnostics() {
    let s = Scratch::new("type-error", "type-error");
    let err = s.build(&configure(), "out").unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, Error::LeanElaboration { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::LeanRejected);
    assert!(text.starts_with("error[LNG0201]: "), "{text}");
    assert!(text.contains("TypeError.lean:5:"), "the diagnostic names the source position:\n{text}");
    assert!(text.contains("error"), "{text}");
    assert!(!s.package.join("out").exists(), "no output is published for a failed build");
}

/// TEST0031: proofs are checked during the build
#[test]
fn test0031_proofs_are_checked_during_the_build() {
    let s = Scratch::new("false-proof", "false-proof");
    let err = s.build(&configure(), "out").unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, Error::LeanElaboration { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::LeanRejected);
    assert!(text.contains("FalseProof.lean:7:"), "the failing proof is reported at its source:\n{text}");
    assert!(text.contains("decide"), "{text}");
}

/// TEST0032: toolchain mismatch is detected before compilation
#[test]
fn test0032_toolchain_mismatch_is_detected_before_compilation() {
    let s = Scratch::new("imports", "toolchain-mismatch");
    for pin in ["leanprover/lean4:v4.35.0", "leanprover/lean4:stable", "leanprover/lean4:nightly-2026-01-01"] {
        s.write("lean-toolchain", &format!("{pin}\n"));
        let err = s.build(&imports_config(), "out").unwrap_err();
        assert!(matches!(&err, Error::UnsupportedToolchain { found, .. } if found == pin), "{err:?}");
        assert!(err.to_string().contains("v4.34.1"), "the error names the supported toolchains: {err}");
        assert!(err.to_string().starts_with("error[LNG0102]: "), "{err}");
    }
    assert!(!s.project().join(".lake").exists(), "nothing was compiled");
}

/// TEST0033: unknown extern symbols are hard errors
#[test]
fn test0033_unknown_extern_symbols_are_hard_errors() {
    let s = Scratch::new("unknown-extern", "unknown-extern");
    let err = s.build(&configure(), "out").unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, Error::Codegen { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::UnresolvedExtern);
    assert!(text.starts_with("error[LNG0401]: unresolved Lean external symbol"), "{text}");
    for needle in ["unresolved Lean external symbol", "provider_send", "Unknown.providerSend", "Unknown.lean"] {
        assert!(text.contains(needle), "the error mentions {needle}:\n{text}");
    }
    assert!(!s.package.join("out").exists(), "no output is published for a failed build");
}

/// TEST0034: unused rust extern mappings are rejected
#[test]
fn test0034_unused_rust_extern_mappings_are_rejected() {
    let s = Scratch::new("imports", "unused-mapping");
    let err = s.build(&imports_config().rust_extern("never_declared", "crate::f"), "out").unwrap_err();
    assert!(err.to_string().contains("never_declared"), "{err}");
    assert_eq!(err.code(), ErrorCode::UnusedExternMapping);
}

/// TEST0035: builds are deterministic and location independent
#[test]
fn test0035_builds_are_deterministic_and_location_independent() {
    // Two copies of the same project at different locations, built independently.
    let a = Scratch::new("imports", "determinism-a");
    let b = Scratch::new("imports", "determinism-b");
    a.build(&imports_config(), "out").unwrap();
    b.build(&imports_config(), "out").unwrap();
    let sa = snapshot(&a.module("out", "imports"), &[]);
    let sb = snapshot(&b.module("out", "imports"), &[]);
    assert_eq!(sa.keys().collect::<Vec<_>>(), sb.keys().collect::<Vec<_>>());
    for (k, v) in &sa {
        if k == "build-info.json" {
            continue; // Records the build key, which identifies the project's location.
        }
        assert!(v == &sb[k], "{k} differs between identical builds");
    }
    // A forced regeneration in place (the recorded build key no longer matches) reproduces the
    // same bytes.
    let info_path = a.module("out", "imports").join("build-info.json");
    let mut info: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&info_path).unwrap()).unwrap();
    info["build_key"] = "stale".into();
    std::fs::write(&info_path, serde_json::to_string(&info).unwrap()).unwrap();
    let outcome = a.build(&imports_config(), "out").unwrap();
    assert!(!outcome.reused, "a stale build key forces regeneration");
    assert_eq!(snapshot(&a.module("out", "imports"), &["build-info.json"]), {
        let mut s = sa.clone();
        s.remove("build-info.json");
        s
    });
}

/// TEST0036: generated artifacts contain no machine specific paths
#[test]
fn test0036_generated_artifacts_contain_no_machine_specific_paths() {
    let s = Scratch::new("imports", "no-abs-paths");
    s.build(&imports_config().embed_sources(true), "out").unwrap();
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

/// TEST0037: builds never rewrite the lean project
#[test]
fn test0037_builds_never_rewrite_the_lean_project() {
    let s = Scratch::new("imports", "no-rewrite");
    let before = snapshot(&s.project(), &[".lake"]);
    s.build(&imports_config(), "out").unwrap();
    let after = snapshot(&s.project(), &[".lake"]);
    assert_eq!(before, after, "the build modified files of the Lean project");
}

/// The `lean_name`s of the records in a published `names.json`.
fn lean_names(module: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(module.join("names.json")).unwrap();
    let records: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    records.iter().map(|r| r["lean_name"].as_str().unwrap().to_owned()).collect()
}

/// TEST0038: default targets of a lakefile lean are the roots
#[test]
fn test0038_default_targets_of_a_lakefile_lean_are_the_roots() {
    let s = Scratch::new("targets", "default-targets");
    let outcome = s.build(&configure(), "out").unwrap();
    assert_eq!(outcome.name, "targets");
    let module = s.module("out", "targets");
    let names = lean_names(&module);
    for exported in ["Shapes.Circle.diameter", "Shapes.Square.area", "Tool.report", "main"] {
        assert!(names.iter().any(|n| n == exported), "every root of every default target is exported: {names:?}");
    }
    let aggregate = std::fs::read_to_string(module.join("targets.rs")).unwrap();
    assert!(aggregate.contains("pub fn __lean_main()"), "the executable's root defines `main`");
}

/// TEST0039: a package without default targets needs explicit roots
#[test]
fn test0039_a_package_without_default_targets_needs_explicit_roots() {
    let s = Scratch::new("imports", "no-default-targets");
    s.write("lakefile.toml", "name = \"imports\"\nversion = \"0.1.0\"\n\n[[lean_lib]]\nname = \"Imports\"\n");
    let err = s.build(&configure(), "out").unwrap_err();
    assert_eq!(err.code(), ErrorCode::UnsatisfiableRequest, "{err}");
    assert!(err.to_string().contains("no default targets"), "{err}");
    let explicit = s.build(&configure().root_module("Imports"), "out").unwrap();
    assert!(lean_names(&s.module("out", explicit.name.as_str())).iter().any(|n| n == "Imports.scaled"));
}

/// TEST0040: two projects cannot generate the same module
#[test]
fn test0040_two_projects_cannot_generate_the_same_module() {
    let a = Scratch::new("imports", "same-name-a");
    let b = Scratch::new("imports", "same-name-b");
    let env = a.env("out");
    configure().run(&a.project(), &env).unwrap();
    let err = configure().run(&b.project(), &env).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidConfiguration, "{err}");
    assert!(err.to_string().contains("would both generate the program `imports`"), "{err}");
    let renamed = configure().name("imports_b").run(&b.project(), &env).unwrap();
    assert_eq!(renamed.name, "imports_b");
    assert!(a.module("out", "imports_b").join("imports_b.rs").is_file());
}

/// TEST0041: shaping paths that select nothing are rejected
#[test]
fn test0041_shaping_paths_that_select_nothing_are_rejected() {
    let s = Scratch::new("imports", "unmatched-paths");
    let cfg = configure()
        .type_attribute("Imports.Missing", "#[derive(Default)]")
        .field_attribute("Imports.scaled", "#[doc(hidden)]")
        .skip_debug(["Imports.Scaled"]);
    let Error::Codegen { errors, .. } = s.build(&cfg, "out").unwrap_err() else {
        panic!("shaping is checked by the code generator");
    };
    let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    assert!(errors.iter().all(|e| e.code() == ErrorCode::InvalidConfiguration), "{messages:?}");
    assert_eq!(messages.len(), 3, "{messages:?}");
    for path in ["`Imports.Missing`", "`Imports.scaled`", "`Imports.Scaled`"] {
        assert!(messages.iter().any(|m| m.contains(path)), "{path} is reported: {messages:?}");
    }
}
