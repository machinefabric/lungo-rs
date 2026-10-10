//! `lungo assurance`: the program's assurance, export by export, for people and for CI.
//!
//! The report keeps apart what must not be confused: the claims Lean proves of an export, the
//! code the export's proofs do not cover (its trust), and the assumptions about the host its
//! claims are conditional on. `--policy` checks the trust and assurance policies; `--compose`
//! checks that separately generated packages describe every record they share the same way.

use lungo_build::codegen::core::assurance::{AssuranceDocument, ClaimStatus, ExportSummary};
use lungo_build::{Error, ErrorCode};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The standard axioms of Lean's logic: not a trust issue.
const STANDARD_AXIOMS: &[&str] = &["propext", "Classical.choice", "Quot.sound"];

/// The exit status of a policy violation.
pub const POLICY_FAILURE: u8 = 2;
/// The exit status of a composition mismatch.
pub const COMPOSITION_FAILURE: u8 = 3;

/// The kinds of trust issue `--trust-issue` selects exports by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TrustIssue {
    /// The export depends on `sorry`.
    Sorry,
    /// The export depends on an axiom beyond Lean's standard three.
    Axioms,
    /// The export runs `unsafe` code.
    Unsafe,
    /// The export runs `partial` definitions.
    Partial,
    /// The export needs a capability of the host.
    Capability,
    /// The export is the subject of no claim.
    NoClaim,
    /// A claim about the export rests on `sorry`.
    Incomplete,
    /// A claim about the export is conditional on an assumption.
    Assumption,
}

/// Which exports the report shows: every filter kind given must match (any of its values).
#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub declaration: Option<String>,
    pub claim_kinds: Vec<String>,
    pub capabilities: Vec<String>,
    pub assumptions: Vec<String>,
    pub trust_issues: Vec<TrustIssue>,
}

fn has_issue(doc: &AssuranceDocument, e: &ExportSummary, issue: TrustIssue) -> bool {
    match issue {
        TrustIssue::Sorry => e.trust.depends_on_sorry,
        TrustIssue::Axioms => e.trust.axioms.iter().any(|a| !STANDARD_AXIOMS.contains(&a.as_str())),
        TrustIssue::Unsafe => !e.trust.unsafe_dependencies.is_empty(),
        TrustIssue::Partial => !e.trust.partial_dependencies.is_empty(),
        TrustIssue::Capability => !e.capabilities.is_empty(),
        TrustIssue::NoClaim => e.claims.is_empty(),
        TrustIssue::Incomplete => {
            e.claims.iter().any(|c| doc.claim(c).is_some_and(|c| c.status == ClaimStatus::Incomplete))
        }
        TrustIssue::Assumption => !e.assumptions.is_empty(),
    }
}

/// The capability `name` (a Lean name or an identifier) names, if any.
fn capability_matches(doc: &AssuranceDocument, export_capability: &str, wanted: &str) -> bool {
    export_capability == wanted || doc.capabilities.iter().any(|c| c.name == export_capability && c.id == wanted)
}

impl Filters {
    fn selects(&self, doc: &AssuranceDocument, e: &ExportSummary) -> bool {
        let any = |values: &[String], test: &dyn Fn(&str) -> bool| values.is_empty() || values.iter().any(|v| test(v));
        self.declaration.as_ref().is_none_or(|d| &e.name == d)
            && any(&self.claim_kinds, &|k| {
                e.claims.iter().any(|c| doc.claim(c).is_some_and(|c| c.relation == k))
            })
            && any(&self.capabilities, &|c| e.capabilities.iter().any(|x| capability_matches(doc, x, c)))
            && any(&self.assumptions, &|a| e.assumptions.iter().any(|x| x == a))
            && (self.trust_issues.is_empty() || self.trust_issues.iter().any(|i| has_issue(doc, e, *i)))
    }
}

/// The exports `filters` select, or an error when a declaration was asked for and is no export.
pub fn select<'d>(doc: &'d AssuranceDocument, filters: &Filters) -> Result<Vec<&'d ExportSummary>, Error> {
    if let Some(d) = &filters.declaration
        && doc.export(d).is_none()
    {
        return Err(Error::Configuration(format!("{d} is not an export of the program")));
    }
    Ok(doc.exports.iter().filter(|e| filters.selects(doc, e)).collect())
}

/// The human-readable report of `exports`: one block per export, its sections plain text.
pub fn human(doc: &AssuranceDocument, exports: &[&ExportSummary]) -> String {
    let mut out = String::new();
    let p = &doc.provenance;
    out.push_str(&format!(
        "program {}: Lean {} ({}), lungo {}, assurance schema {}\n",
        doc.program, p.lean_version, p.lean_githash, p.lungo_version, doc.schema_version
    ));
    if exports.is_empty() {
        out.push_str("no matching exports\n");
        return out;
    }
    for e in exports {
        out.push('\n');
        out.push_str(&export_block(doc, e));
    }
    out
}

/// The sections of one export: claims, trust, assumptions, capabilities.
pub fn export_block(doc: &AssuranceDocument, e: &ExportSummary) -> String {
    let mut out = format!("{}{}\n", e.name, if e.r#async { " (async)" } else { "" });
    section(&mut out, "Claims", e.claims.iter().map(|name| match doc.claim(name) {
        Some(c) => {
            let mut line = format!("[{}] {} {}", c.status.as_str(), c.relation, c.name);
            if !c.specifications.is_empty() {
                line.push_str(&format!(" (of {})", c.specifications.join(", ")));
            }
            if !c.assumptions.is_empty() {
                line.push_str(&format!(" — assuming {}", c.assumptions.join(", ")));
            }
            line
        }
        None => format!("[missing] {name}"),
    }));
    let t = &e.trust;
    let mut trust = Vec::new();
    let extra: Vec<&str> =
        t.axioms.iter().map(String::as_str).filter(|a| !STANDARD_AXIOMS.contains(a)).collect();
    if !extra.is_empty() {
        trust.push(format!("non-standard axioms: {}", extra.join(", ")));
    }
    if t.depends_on_sorry {
        trust.push("depends on sorry".to_owned());
    }
    if !t.unsafe_dependencies.is_empty() {
        trust.push(format!("unsafe: {}", t.unsafe_dependencies.join(", ")));
    }
    if !t.partial_dependencies.is_empty() {
        trust.push(format!("partial: {}", t.partial_dependencies.join(", ")));
    }
    section(&mut out, "Trust", trust.into_iter());
    section(
        &mut out,
        "Assumptions",
        e.assumptions.iter().map(|a| match doc.assumptions.iter().find(|x| &x.name == a) {
            Some(x) => format!("[assumed] {a} : {} (of {})", x.statement.replace('\n', " "), x.capability),
            None => format!("[assumed] {a}"),
        }),
    );
    section(
        &mut out,
        "Capabilities",
        e.capabilities.iter().map(|c| match doc.capabilities.iter().find(|x| &x.name == c) {
            Some(x) => format!("{} ({c})", x.id),
            None => c.clone(),
        }),
    );
    out
}

fn section(out: &mut String, title: &str, lines: impl Iterator<Item = String>) {
    out.push_str(&format!("  == {title} ==\n"));
    let mut any = false;
    for l in lines {
        any = true;
        out.push_str(&format!("    {l}\n"));
    }
    if !any {
        out.push_str("    (none)\n");
    }
}

/// A record two documents describe differently.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Mismatch {
    pub kind: &'static str,
    pub name: String,
    /// Per document (by its label): the package and fingerprint it gives.
    pub descriptions: Vec<(String, Option<String>, String)>,
}

/// Every record (specification, claim, capability, assumption) at least two of `docs` describe
/// differently: from another package, or with another meaning.
pub fn compose(docs: &[(String, AssuranceDocument)]) -> Vec<Mismatch> {
    type Key = (&'static str, String);
    let mut seen: BTreeMap<Key, Vec<(String, Option<String>, String)>> = BTreeMap::new();
    for (label, d) in docs {
        let mut add = |kind: &'static str, name: &str, package: &Option<String>, fingerprint: &str| {
            seen.entry((kind, name.to_owned())).or_default().push((label.clone(), package.clone(), fingerprint.to_owned()));
        };
        for s in &d.specifications {
            add("specification", &s.name, &s.package, &s.fingerprint);
        }
        for c in &d.claims {
            add("claim", &c.name, &c.package, &c.fingerprint);
        }
        for c in &d.capabilities {
            add("capability", &c.name, &c.package, &c.fingerprint);
        }
        for a in &d.assumptions {
            add("assumption", &a.name, &a.package, &a.fingerprint);
        }
    }
    seen.into_iter()
        .filter(|(_, ds)| ds.len() > 1 && ds.iter().any(|(_, p, f)| (p, f) != (&ds[0].1, &ds[0].2)))
        .map(|((kind, name), descriptions)| Mismatch { kind, name, descriptions })
        .collect()
}

/// Reads an assurance document a package carries.
pub fn read_document(path: &PathBuf) -> Result<AssuranceDocument, Error> {
    let text =
        std::fs::read_to_string(path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
    AssuranceDocument::from_json(&text).map_err(|e| {
        Error::Assurance(vec![lungo_build::AssuranceIssue {
            code: ErrorCode::InvalidAssuranceDocument,
            message: format!("{} is not an assurance document lungo reads: {e}", path.display()),
        }])
    })
}

/// The mismatches as issues, for display.
pub fn mismatch_issues(mismatches: &[Mismatch]) -> Vec<lungo_build::AssuranceIssue> {
    mismatches
        .iter()
        .map(|m| lungo_build::AssuranceIssue {
            code: ErrorCode::FingerprintMismatch,
            message: format!(
                "the {} {} is described differently:\n{}",
                m.kind,
                m.name,
                m.descriptions
                    .iter()
                    .map(|(label, package, fp)| format!(
                        "  {label}: package {}, fingerprint {fp}",
                        package.as_deref().unwrap_or("(none)")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lungo_build::codegen::core::assurance::{Claim, EvidenceTrust, Provenance, SCHEMA_VERSION, Specification};

    fn doc(program: &str, spec_fp: &str) -> AssuranceDocument {
        AssuranceDocument {
            schema_version: SCHEMA_VERSION,
            program: program.into(),
            provenance: Provenance {
                lean_version: "4.34.1".into(),
                lean_githash: "x".into(),
                lungo_version: "0".into(),
                bir_version: 3,
                runtime_abi: 2,
            },
            library: None,
            specifications: vec![Specification {
                name: "Shared.refines".into(),
                kind: "lungo.relation".into(),
                statement: "Prop".into(),
                package: Some("shared".into()),
                fingerprint: spec_fp.into(),
                source: None,
            }],
            capabilities: vec![],
            assumptions: vec![],
            claims: vec![Claim {
                name: format!("{program}.thm"),
                relation: "lungo.law".into(),
                subjects: vec![],
                specifications: vec![],
                statement: "True".into(),
                status: ClaimStatus::Proved,
                evidence_trust: EvidenceTrust { axioms: vec![], depends_on_sorry: false },
                assumptions: vec![],
                package: Some(program.into()),
                fingerprint: "c".into(),
                source: None,
            }],
            roles: vec![],
            exports: vec![],
        }
    }

    /// TEST0272: composition finds a shared record two packages describe differently, and only it
    #[test]
    fn test0272_composition_finds_a_shared_record_described_differently() {
        let same = compose(&[("a".into(), doc("a", "f1")), ("b".into(), doc("b", "f1"))]);
        assert!(same.is_empty(), "the same specification, and claims of their own: {same:?}");
        let differ = compose(&[("a".into(), doc("a", "f1")), ("b".into(), doc("b", "f2"))]);
        assert_eq!(differ.len(), 1);
        assert_eq!((differ[0].kind, differ[0].name.as_str()), ("specification", "Shared.refines"));
    }
}
