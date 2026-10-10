//! Release gates of formal assurance: what a Lake project built with lungo's Lean library claims
//! and proves of its exports, checked against the environment, carried into every output as
//! `assurance.json`, and held to the project's policy.
//!
//! Each test copies the `assurance` fixture into its own scratch directory, with lungo's Lean
//! library (`lungo-rs/lean`) next to it as `lungo-lib`, so that each builds its own copy of the
//! library and no two tests build one concurrently.

use lungo_build::{Builder, Environment, Error, ErrorCode, configure};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Scratch {
    package: PathBuf,
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

impl Scratch {
    fn new(test: &str) -> Scratch {
        let package = Path::new(env!("CARGO_TARGET_TMPDIR")).join("assurance-gates").join(test);
        if package.exists() {
            std::fs::remove_dir_all(&package).unwrap();
        }
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        copy_tree(&manifest.join("fixtures/assurance"), &package.join("lean"));
        copy_tree(&manifest.join("../../lean"), &package.join("lungo-lib"));
        Scratch { package }
    }

    fn file(&self, rel: &str) -> PathBuf {
        self.package.join("lean").join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.file(rel), text).unwrap();
    }

    /// Appends `text` to the module `Assured`, inside its namespace.
    fn append(&self, text: &str) {
        let path = self.file("Assured.lean");
        let source = std::fs::read_to_string(&path).unwrap();
        let source = source.trim_end().strip_suffix("end Assured").expect("the module ends its namespace");
        std::fs::write(&path, format!("{source}{text}\nend Assured\n")).unwrap();
    }

    fn build(&self, cfg: &Builder, out: &str) -> lungo_build::Result<lungo_build::BuildOutcome> {
        let env = Environment::native(self.package.clone(), self.package.join(out), self.package.join(format!("{out}-work")));
        cfg.run(Path::new("lean"), &env)
    }

    /// The assurance document of the output `out`.
    fn assurance(&self, out: &str) -> Value {
        serde_json::from_str(&self.text(out, "assurance.json")).unwrap()
    }

    fn text(&self, out: &str, rel: &str) -> String {
        std::fs::read_to_string(self.package.join(out).join("assured").join(rel)).unwrap()
    }
}

/// The fixture's configuration: the host implements the clock in Rust.
fn config() -> Builder {
    configure().rust_extern("assured_now", "crate::host::now")
}

/// The codes of the errors of a failed build.
fn codes(err: &Error) -> Vec<ErrorCode> {
    match err {
        Error::Assurance(issues) => issues.iter().map(|i| i.code).collect(),
        other => vec![other.code()],
    }
}

#[track_caller]
fn fails_with(result: lungo_build::Result<lungo_build::BuildOutcome>, code: ErrorCode, needle: &str) -> String {
    let err = result.expect_err("the build is refused");
    let text = err.to_string();
    assert!(codes(&err).contains(&code), "{code:?} expected: {:?}\n{text}", codes(&err));
    assert!(text.contains(needle), "{needle:?} expected in:\n{text}");
    text
}

fn named<'v>(doc: &'v Value, section: &str, name: &str) -> &'v Value {
    doc[section]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {section}: {}", doc[section]))
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect()
}

/// A record written by hand, without its attribute: Lean code adding the definition the
/// attribute would add, in the canonical form lungo reads.
fn forged(decl: &str, kind: &str, structure: &str, value: &str) -> String {
    format!(
        "\nopen Lean Lungo.Attr in\nrun_meta Lungo.Attr.addRecord \"forged\" `{decl} \"{kind}\" ``Lungo.Registry.{structure} ({value})\n"
    )
}

fn forged_claim(evidence: &str, relation: &str, subjects: &[&str], specs: &[&str]) -> String {
    let names = |xs: &[&str]| format!("namesExpr [{}]", xs.iter().map(|x| format!("`{x}")).collect::<Vec<_>>().join(", "));
    forged(
        evidence,
        "claim",
        "Claim",
        &format!(
            "mkApp4 (mkConst ``Lungo.Registry.Claim.mk) (nameExpr `{evidence}) (mkStrLit \"{relation}\") ({}) ({})",
            names(subjects),
            names(specs)
        ),
    )
}

/// TEST0278: claims about exports are reported with their status and assumptions
#[test]
fn test0278_claims_about_exports_are_reported_with_their_status_and_assumptions() {
    let s = Scratch::new("claims");
    s.build(&config(), "out").unwrap();
    let doc = s.assurance("out");
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["program"], "assured");
    assert_eq!(doc["library"]["package"], "lungo");
    let c = named(&doc, "claims", "Assured.double_eq");
    assert_eq!((c["relation"].as_str(), c["status"].as_str()), (Some("lungo.equals"), Some("proved")));
    assert_eq!((strings(&c["subjects"]), strings(&c["specifications"])), (vec!["Assured.double"], vec!["Assured.twice"]));
    assert!(strings(&c["assumptions"]).is_empty());
    // An assumption is a hypothesis of the claim's statement, found where it is bound.
    let c = named(&doc, "claims", "Assured.elapsed_le");
    assert_eq!(strings(&c["assumptions"]), ["Assured.Monotone"]);
    assert_eq!(c["status"], "proved", "a conditional claim is proved, under its assumption");
    let spec = named(&doc, "specifications", "Assured.twice");
    assert_eq!(spec["kind"], "lungo.model");
    let clock = named(&doc, "capabilities", "Assured.Clock");
    assert_eq!((clock["id"].as_str(), clock["form"].as_str()), (Some("assured.clock"), Some("extern")));
    assert_eq!(strings(&clock["assumptions"]), ["Assured.Monotone"]);
    assert_eq!(clock["operations"][0]["name"], "Assured.now");
    let e = named(&doc, "exports", "Assured.elapsed");
    assert_eq!(strings(&e["claims"]), ["Assured.elapsed_le"]);
    assert_eq!(strings(&e["capabilities"]), ["Assured.Clock"]);
    assert_eq!(strings(&e["assumptions"]), ["Assured.Monotone"]);
    let e = named(&doc, "exports", "Assured.double");
    assert_eq!(strings(&e["claims"]), ["Assured.double_eq"]);
    assert!(strings(&e["capabilities"]).is_empty() && strings(&e["assumptions"]).is_empty());
    // Every record is fingerprinted, and no path of this machine is in the document.
    for section in ["specifications", "capabilities", "assumptions", "claims"] {
        for r in doc[section].as_array().unwrap() {
            let fp = r["fingerprint"].as_str().unwrap();
            assert!(fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit()), "{r}");
        }
    }
    let text = s.text("out", "assurance.json");
    assert!(!text.contains(s.package.to_str().unwrap()), "a machine path in the document");
    // The Rust module carries the same document.
    let aggregate = s.text("out", "assured.rs");
    assert!(aggregate.contains("ASSURANCE_JSON"), "the module embeds the document");
}

/// TEST0279: a claim resting on sorry is refused, or reported incomplete when sorry is allowed
#[test]
fn test0279_a_claim_resting_on_sorry_is_refused_or_reported_incomplete_when_sorry_is_allowed() {
    let s = Scratch::new("sorry");
    s.append(
        "\n@[lungo_claim \"lungo.law\" subject double]\ntheorem double_big (n : Nat) : n ≤ double n := by\n  sorry\n",
    );
    fails_with(s.build(&config(), "out"), ErrorCode::TrustPolicy, "the claim Assured.double_big depends on `sorry`");
    s.build(&config().deny_sorry(false), "out").unwrap();
    let doc = s.assurance("out");
    let c = named(&doc, "claims", "Assured.double_big");
    assert_eq!(c["status"], "incomplete");
    assert_eq!(c["evidence_trust"]["depends_on_sorry"], true);
    // An incomplete claim is not a proof: an export required to carry a proved claim has none.
    s.write(
        "Assured.lean",
        &std::fs::read_to_string(s.file("Assured.lean")).unwrap().replace("@[lungo_claim \"lungo.equals\" subject double spec twice]\n", ""),
    );
    fails_with(
        s.build(&config().deny_sorry(false).require_claims("Assured.double"), "out"),
        ErrorCode::ExportWithoutClaim,
        "the export Assured.double has no proved claim",
    );
}

/// TEST0280: a record lungo cannot read as written is refused
#[test]
fn test0280_a_record_lungo_cannot_read_as_written_is_refused() {
    let s = Scratch::new("malformed");
    // A computed field: the worker reads records without evaluating them.
    s.append(
        "\ndef notes : Nat := 1\n\nnoncomputable def notes._lungo_spec : Lungo.Registry.Spec :=\n  ⟨`Assured.notes, \"lungo.\" ++ \"model\"⟩\n",
    );
    fails_with(s.build(&config(), "out"), ErrorCode::MalformedAssuranceRecord, "Assured.notes._lungo_spec");
}

/// TEST0281: a record naming a declaration that does not exist is refused
#[test]
fn test0281_a_record_naming_a_declaration_that_does_not_exist_is_refused() {
    let s = Scratch::new("dangling");
    s.append("\ntheorem double_pos (n : Nat) : 0 ≤ double n := Nat.zero_le _\n");
    s.append(&forged_claim("Assured.double_pos", "lungo.law", &["Assured.gone"], &[]));
    fails_with(s.build(&config(), "out"), ErrorCode::DanglingAssuranceReference, "Assured.gone");
}

/// TEST0282: a claim whose evidence is not a theorem is refused
#[test]
fn test0282_a_claim_whose_evidence_is_not_a_theorem_is_refused() {
    let s = Scratch::new("invalid-claim");
    s.append("\ndef tripled (n : Nat) : Nat := double n + n\n");
    s.append(&forged_claim("Assured.tripled", "lungo.law", &["Assured.double"], &[]));
    fails_with(s.build(&config(), "out"), ErrorCode::InvalidClaim, "Assured.tripled");
}

/// TEST0283: a claim about what its statement does not mention is refused
#[test]
fn test0283_a_claim_about_what_its_statement_does_not_mention_is_refused() {
    let s = Scratch::new("not-in-statement");
    s.append("\ntheorem double_pos (n : Nat) : 0 ≤ double n := Nat.zero_le _\n");
    s.append(&forged_claim("Assured.double_pos", "lungo.law", &["Assured.double"], &["Assured.twice"]));
    fails_with(s.build(&config(), "out"), ErrorCode::ClaimNotInStatement, "Assured.twice");
}

/// TEST0284: two capabilities with one identifier are refused
#[test]
fn test0284_two_capabilities_with_one_identifier_are_refused() {
    let s = Scratch::new("duplicate");
    s.append("\n@[lungo_capability \"assured.clock\"]\nstructure OtherClock\n");
    fails_with(s.build(&config(), "out"), ErrorCode::DuplicateAssuranceId, "assured.clock");
}

/// TEST0285: externs and capabilities must agree
#[test]
fn test0285_externs_and_capabilities_must_agree() {
    // A reachable operation the host does not implement.
    let s = Scratch::new("capabilities");
    fails_with(s.build(&configure(), "out"), ErrorCode::UnresolvedExtern, "assured_now");
    // An extern that is no capability's operation: the host cannot implement it.
    s.append("\n@[extern \"assured_raw\"]\nopaque raw (n : Nat) : Nat\n\ndef viaRaw (n : Nat) : Nat := raw n\n");
    fails_with(s.build(&config(), "out"), ErrorCode::UnresolvedExtern, "@[lungo_operation");
    fails_with(
        s.build(&config().rust_extern("assured_raw", "crate::host::raw"), "out"),
        ErrorCode::CapabilityMismatch,
        "assured_raw",
    );
    // An operation the runtime implements itself.
    let s = Scratch::new("intrinsic-operation");
    s.append("\n@[extern \"lean_nat_add\", lungo_operation Clock]\nopaque plus (a b : Nat) : Nat\n");
    fails_with(s.build(&config(), "out"), ErrorCode::CapabilityMismatch, "lean_nat_add");
}

/// TEST0286: an async capability whose answers depend on the operation's fields is refused
#[test]
fn test0286_an_async_capability_whose_answers_depend_on_the_operations_fields_is_refused() {
    let s = Scratch::new("async-interface");
    s.append(
        "\ninductive Pick where\n  | below (n : Nat)\n\ninstance pickInterface : Lungo.Async.Interface Pick where\n  Ret\n    | .below n => Fin (n + 1)\n",
    );
    s.append(&forged(
        "Assured.pickInterface",
        "capability",
        "Capability",
        "mkApp3 (mkConst ``Lungo.Registry.Capability.mk) (nameExpr `Assured.pickInterface) (mkStrLit \"assured.pick\") \
         (optionNameExpr (some `Assured.Pick))",
    ));
    fails_with(s.build(&config(), "out"), ErrorCode::AsyncInterface, "Assured.Pick");
}

/// TEST0287: a library of another schema version is refused
#[test]
fn test0287_a_library_of_another_schema_version_is_refused() {
    let s = Scratch::new("library-version");
    let registry = s.package.join("lungo-lib/Lungo/Registry.lean");
    let source = std::fs::read_to_string(&registry).unwrap();
    assert!(source.contains("def schemaVersion : Nat := nat_lit 1"));
    std::fs::write(&registry, source.replace("def schemaVersion : Nat := nat_lit 1", "def schemaVersion : Nat := nat_lit 2"))
        .unwrap();
    fails_with(s.build(&config(), "out"), ErrorCode::AssuranceLibraryVersion, "2");
}

/// TEST0288: a program using a value only lungo's compile-time modules initialize is refused
#[test]
fn test0288_a_program_using_a_value_only_lungos_compile_time_modules_initialize_is_refused() {
    let s = Scratch::new("metadata-only");
    // Lean is imported only through lungo's attributes, which are never linked.
    s.append("\ndef depth : Nat := Lean.maxRecDepth.defValue\n");
    fails_with(s.build(&config(), "out"), ErrorCode::MetadataOnlyDependency, "Lean.maxRecDepth");
}

/// TEST0289: assurance modules add their claims without changing the program
#[test]
fn test0289_assurance_modules_add_their_claims_without_changing_the_program() {
    let s = Scratch::new("assurance-modules");
    s.build(&config(), "plain").unwrap();
    s.build(&config().assurance_module("Assured.Laws"), "laws").unwrap();
    let plain = s.assurance("plain");
    let laws = s.assurance("laws");
    assert!(plain["claims"].as_array().unwrap().iter().all(|c| c["name"] != "Assured.double_even"));
    let c = named(&laws, "claims", "Assured.double_even");
    assert_eq!(c["status"], "proved");
    assert_eq!(strings(&named(&laws, "exports", "Assured.double")["claims"]), ["Assured.double_eq", "Assured.double_even"]);
    // The program is the same: every generated Rust file but the assurance it embeds.
    let rust = |out: &str| -> BTreeMap<String, String> {
        let dir = s.package.join(out).join("assured/modules");
        std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap())
            })
            .collect()
    };
    assert_eq!(rust("plain"), rust("laws"), "an assurance module changed the program's code");
    assert!(!rust("laws").keys().any(|m| m.contains("Laws")), "the assurance module is not linked");
    // A root module's records are read anyway: it cannot also be an assurance module.
    let err = s.build(&config().assurance_module("Assured"), "both").unwrap_err();
    assert!(err.to_string().contains("both a root module and an assurance module"), "{err}");
}

/// TEST0290: the policy requires claims and forbids assumptions
#[test]
fn test0290_the_policy_requires_claims_and_forbids_assumptions() {
    let s = Scratch::new("policy");
    s.append("\ndef unclaimed (n : Nat) : Nat := n\n");
    s.build(&config().require_claims("Assured.double").require_claims("Assured.elapsed"), "out").unwrap();
    let text = fails_with(s.build(&config().require_claims("Assured"), "out"), ErrorCode::ExportWithoutClaim, "Assured.unclaimed");
    assert!(!text.contains("export Assured.double has"), "{text}");
    fails_with(s.build(&config().require_claims("."), "out"), ErrorCode::ExportWithoutClaim, "Assured.unclaimed");
    // Forbidding the assumption refuses the claim resting on it; forbidding the capability also
    // refuses the export using it.
    fails_with(
        s.build(&config().forbid_assumption("Assured.Monotone"), "out"),
        ErrorCode::ForbiddenAssumption,
        "the claim Assured.elapsed_le assumes Assured.Monotone",
    );
    fails_with(
        s.build(&config().forbid_assumption("Assured.Clock"), "out"),
        ErrorCode::ForbiddenAssumption,
        "the export Assured.elapsed needs the capability Assured.Clock",
    );
    // A policy naming nothing of the program is a configuration error, not an empty policy.
    for cfg in [config().require_claims("Assured.missing"), config().forbid_assumption("Assured.Missing")] {
        assert_eq!(s.build(&cfg, "out").unwrap_err().code(), ErrorCode::InvalidConfiguration);
    }
}

/// TEST0291: claims and specifications do not change the program
#[test]
fn test0291_claims_and_specifications_do_not_change_the_program() {
    let s = Scratch::new("annotations");
    s.build(&config(), "annotated").unwrap();
    let source = std::fs::read_to_string(s.file("Assured.lean")).unwrap();
    // Blank, not removed: the generated code cites its source lines.
    let bare: String =
        source.lines().map(|l| if l.starts_with("@[lungo_") { "\n".to_owned() } else { format!("{l}\n") }).collect();
    assert_ne!(bare, source);
    s.write("Assured.lean", &bare);
    s.build(&config(), "bare").unwrap();
    assert!(s.assurance("bare")["claims"].as_array().unwrap().is_empty());
    // The program's code: every generated Rust file but the aggregate, which embeds the assurance
    // document (and differs, as it should).
    let generated = |out: &str| -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        let dir = s.package.join(out).join("assured");
        for sub in ["modules", "."] {
            for e in std::fs::read_dir(dir.join(sub)).unwrap() {
                let p = e.unwrap().path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if p.is_file() && name.ends_with(".rs") && name != "assured.rs" {
                    files.insert(format!("{sub}/{name}"), std::fs::read_to_string(&p).unwrap());
                }
            }
        }
        files
    };
    let (annotated, bare) = (generated("annotated"), generated("bare"));
    assert!(annotated.keys().any(|k| k.starts_with("modules/")), "{:?}", annotated.keys());
    assert_eq!(annotated, bare, "annotations changed the generated program");
}

/// TEST0292: the assurance document is deterministic and independent of the project's location
#[test]
fn test0292_the_assurance_document_is_deterministic_and_independent_of_the_projects_location() {
    let a = Scratch::new("location-a");
    let b = Scratch::new("location-b-elsewhere");
    a.build(&config(), "out").unwrap();
    b.build(&config(), "out").unwrap();
    let doc = a.text("out", "assurance.json");
    assert_eq!(doc, b.text("out", "assurance.json"));
    a.build(&config(), "again").unwrap();
    assert_eq!(doc, a.text("again", "assurance.json"));
}

/// TEST0293: thousands of claims are read within the worker's limits
#[test]
fn test0293_thousands_of_claims_are_read_within_the_workers_limits() {
    let s = Scratch::new("scale");
    let mut text = String::from("import Assured\n\nnamespace Assured\n");
    for i in 0..2000 {
        text.push_str(&format!(
            "\n@[lungo_claim \"lungo.equals\" subject double spec twice]\ntheorem double_{i} : double {i} = twice {i} := rfl\n"
        ));
    }
    text.push_str("\nend Assured\n");
    s.write("Assured/Many.lean", &text);
    s.build(&config().assurance_module("Assured.Many"), "out").unwrap();
    let doc = s.assurance("out");
    assert_eq!(doc["claims"].as_array().unwrap().len(), 2002);
    assert_eq!(strings(&named(&doc, "exports", "Assured.double")["claims"]).len(), 2001);
}

/// TEST0294: lungo's Lean library checks every attribute it declares
#[test]
fn test0294_lungos_lean_library_checks_every_attribute_it_declares() {
    let s = Scratch::new("library");
    let lib = s.package.join("lungo-lib");
    let out = std::process::Command::new("lake").args(["build", "Lungo", "LungoTest"]).current_dir(&lib).output().unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The library is released with lungo, under its version.
    let lakefile = std::fs::read_to_string(lib.join("lakefile.toml")).unwrap();
    let version = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../version.txt")).unwrap();
    assert!(lakefile.contains(&format!("version = \"{}\"", version.trim())), "{lakefile}");
}
