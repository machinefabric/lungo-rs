//! `lungo assurance` and the assurance document of generated packages, on the polyglot fixture
//! (`compiler-tests/polyglot`): claims with and without assumptions, two facilities and an async
//! one.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("assurance-cli").join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `lungo <args>`; its exit status, standard output and standard error.
fn lungo(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lungo")).args(args).current_dir(repo()).output().unwrap();
    (
        out.status.code().expect("lungo exits"),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

const CONFIG: &str = "compiler-tests/polyglot/lungo.toml";

fn assurance(args: &[&str]) -> (i32, String, String) {
    lungo(&[&["--config", CONFIG, "assurance"], args].concat())
}

fn json(args: &[&str]) -> Value {
    let (status, out, err) = assurance(&[&["--format", "json"], args].concat());
    assert_eq!(status, 0, "{err}");
    serde_json::from_str(&out).unwrap()
}

fn export_names(doc: &Value) -> Vec<&str> {
    doc["exports"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect()
}

/// A local distribution: a development build of lungo needs one for any binding; generating a
/// package uses no part of it but its version.
fn distribution() -> PathBuf {
    let dir = scratch("dist");
    std::fs::write(dir.join("VERSION"), format!("{}\n", env!("CARGO_PKG_VERSION"))).unwrap();
    dir
}

/// TEST0295: every output carries the same assurance document
#[test]
fn test0295_every_output_carries_the_same_assurance_document() {
    let out = scratch("outputs");
    let dist = distribution();
    let (status, _, err) = lungo(&[
        "--config",
        CONFIG,
        "generate",
        &format!("--c_out={}", out.join("c").display()),
        &format!("--go_out={}", out.join("go").display()),
        &format!("--python_out={}", out.join("python").display()),
        &format!("--swift_out={}", out.join("swift").display()),
        "--runtime-dir",
        dist.to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{err}");
    let c = std::fs::read(out.join("c/assurance.json")).unwrap();
    for language in ["go", "python", "swift"] {
        assert!(std::fs::read(out.join(language).join("assurance.json")).unwrap() == c, "{language} differs from c");
    }
    let doc: Value = serde_json::from_slice(&c).unwrap();
    assert_eq!(doc["program"], "polyglot");
    // Each language embeds the document's text exactly: as a raw string, or the file itself.
    let text = String::from_utf8(c).unwrap();
    let swift = std::fs::read_to_string(out.join("swift/Sources/Polyglot/Polyglot.swift")).unwrap();
    assert!(swift.contains(&format!("#\"\"\"\n{text}\n\"\"\"#")), "Swift embeds another text");
    let go = std::fs::read_to_string(out.join("go/polyglot.go")).unwrap();
    assert!(go.contains("//go:embed assurance.json"), "Go embeds the file");
}

/// TEST0296: lungo assurance reports exports by what they rest on
#[test]
fn test0296_lungo_assurance_reports_exports_by_what_they_rest_on() {
    let (status, out, err) = assurance(&["Polyglot.scaledSum"]);
    assert_eq!(status, 0, "{err}");
    assert!(
        out.contains("[proved] lungo.law Polyglot.scaledSum_singleton_mono — assuming Polyglot.ScalesMonotonically"),
        "{out}"
    );
    assert!(out.contains("[assumed] Polyglot.ScalesMonotonically"), "{out}");
    assert!(out.contains("polyglot.scaler (Polyglot.Scaler)"), "{out}");
    // Filters select exports by facility (by Lean name or identifier), relation, assumption and
    // trust issue.
    assert_eq!(
        export_names(&json(&["--facility", "polyglot.fetch"])),
        ["Polyglot.applyScaler", "Polyglot.fetchAll", "Polyglot.stampToken"]
    );
    assert_eq!(export_names(&json(&["--facility", "Polyglot.Scaler"])), ["Polyglot.hostScale", "Polyglot.scaledSum"]);
    assert_eq!(export_names(&json(&["--claim-kind", "lungo.law"])), ["Polyglot.factorial", "Polyglot.scaledSum"]);
    assert_eq!(export_names(&json(&["--assumption", "Polyglot.ScalesMonotonically"])), ["Polyglot.scaledSum"]);
    let doc = json(&["--trust-issue", "assumption"]);
    assert_eq!(export_names(&doc), ["Polyglot.scaledSum"]);
    // The claims reported are those about the exports reported.
    let claims: Vec<&str> = doc["claims"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(claims, ["Polyglot.scaledSum_singleton_mono"]);
    let doc = json(&["--trust-issue", "no-claim"]);
    let unclaimed = export_names(&doc);
    assert!(unclaimed.contains(&"Polyglot.negate") && !unclaimed.contains(&"Polyglot.factorial"), "{unclaimed:?}");
    let (status, _, err) = assurance(&["Polyglot.nothing"]);
    assert_ne!(status, 0);
    assert!(err.contains("Polyglot.nothing"), "{err}");
}

/// TEST0304: lungo assurance checks the policy and composes documents
#[test]
fn test0304_lungo_assurance_checks_the_policy_and_composes_documents() {
    let dir = scratch("policy");
    // The fixture's policy holds.
    let (status, out, err) = assurance(&["--policy"]);
    assert_eq!(status, 0, "{err}");
    assert!(out.contains("policy: satisfied"), "{out}");
    // A stricter one does not: exit status 2, each violation with its code.
    let strict = dir.join("strict.toml");
    std::fs::write(
        &strict,
        format!(
            "project = {:?}\n\n[assurance]\nrequire-claims = [\".\"]\nforbid-assumptions = [\"Polyglot.ScalesMonotonically\"]\n",
            repo().join("compiler-tests/polyglot/lean").to_str().unwrap()
        ),
    )
    .unwrap();
    let (status, _, err) = lungo(&["--config", strict.to_str().unwrap(), "assurance", "--policy"]);
    assert_eq!(status, 2, "{err}");
    assert!(err.contains("error[LNG0602]") && err.contains("Polyglot.negate"), "{err}");
    assert!(err.contains("error[LNG0603]") && err.contains("Polyglot.scaledSum_singleton_mono"), "{err}");
    // Composition: documents agreeing on every record they share compose; a record described
    // differently is a mismatch, with exit status 3.
    let out = scratch("compose");
    let (status, _, err) = lungo(&[
        "--config",
        CONFIG,
        "generate",
        &format!("--c_out={}", out.join("c").display()),
        "--runtime-dir",
        distribution().to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{err}");
    let ours = out.join("c/assurance.json");
    let mut theirs: Value = serde_json::from_str(&std::fs::read_to_string(&ours).unwrap()).unwrap();
    let ours = ours.to_str().unwrap();
    let (status, out_text, err) = lungo(&["assurance", "--compose", ours, "--compose", ours]);
    assert_eq!(status, 0, "{err}");
    assert!(out_text.contains("2 documents agree"), "{out_text}");
    theirs["specifications"][0]["fingerprint"] = Value::String("0".repeat(64));
    let changed = dir.join("changed.json");
    std::fs::write(&changed, serde_json::to_string_pretty(&theirs).unwrap()).unwrap();
    let changed = changed.to_str().unwrap();
    let (status, _, err) = lungo(&["assurance", "--compose", ours, "--compose", changed]);
    assert_eq!(status, 3, "{err}");
    assert!(err.contains("error[LNG0708]") && err.contains("Polyglot.natDiv"), "{err}");
    // Against the project itself.
    let (status, _, err) = assurance(&["--compose", changed]);
    assert_eq!(status, 3, "{err}");
    // A document lungo cannot read is an error of its own.
    let unreadable = dir.join("unreadable.json");
    std::fs::write(&unreadable, "{\"schema_version\": 99}").unwrap();
    let (status, _, err) = lungo(&["assurance", "--compose", ours, "--compose", unreadable.to_str().unwrap()]);
    assert_eq!(status, 1, "{err}");
    assert!(err.contains("error[LNG0711]"), "{err}");
}
