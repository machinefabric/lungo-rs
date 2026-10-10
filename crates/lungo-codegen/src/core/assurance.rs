//! The program's assurance document: what its Lean code claims, proves and assumes, and what
//! every export depends on, as one versioned JSON document (`assurance.json`).
//!
//! The document is built once from the worker's analysis and written, byte for byte the same,
//! into every output: the Rust module, each language's package, and each plugin's request. Every
//! language's accessor reads this one document, so no package can present a different account of
//! the same program.
//!
//! It distinguishes three things a reader must not confuse:
//!
//! - **claims**, proved by a Lean theorem the kernel checked (`proved`), or resting on `sorry`
//!   (`incomplete`, only when the trust policy allows it);
//! - **trust**: the axioms, `sorry`, `unsafe`, `partial` and extern code an export's executable
//!   closure depends on, as before;
//! - **assumptions**: propositions about the host's facilities a claim's statement takes as
//!   hypotheses. A claim with assumptions holds of a running program only when the host's
//!   implementation satisfies them; registering an implementation proves nothing about it.
//!
//! Every record carries the Lake package declaring it and a fingerprint of its meaning (its
//! statement, and the definition of a specification, and the meaning of every definition of the
//! workspace's packages it uses, transitively), so documents of separately generated packages can
//! be checked to agree on what a shared name means (`lungo assurance --compose`).

use crate::CodegenError;
use lungo_protocol::{
    Assurance, ClaimRecord, DeclSource, DefinitionRecord, Export, FacadeType, FacilityKind, Position, Success, Trust,
    WorkerToolchain,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The version of the document format. A reader refuses a version it does not know.
pub const SCHEMA_VERSION: u32 = 1;

/// The version of the canonical text a fingerprint is computed from.
const FINGERPRINT_FORMAT: &str = "lungo-assurance-1";

/// The file every output carries the document in.
pub const FILE_NAME: &str = "assurance.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssuranceDocument {
    pub schema_version: u32,
    /// The program's name.
    pub program: String,
    pub provenance: Provenance,
    /// lungo's Lean library, when the program uses it.
    pub library: Option<Library>,
    pub specifications: Vec<Specification>,
    pub facilities: Vec<Facility>,
    pub assumptions: Vec<Assumption>,
    pub claims: Vec<Claim>,
    pub roles: Vec<Role>,
    pub exports: Vec<ExportSummary>,
}

/// What produced the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub lean_version: String,
    pub lean_githash: String,
    pub lungo_version: String,
    pub bir_version: u32,
    pub runtime_abi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub package: String,
    pub schema_version: u32,
}

/// Where a declaration is, relative to the Lake package declaring it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub package: String,
    /// `/`-separated, relative to the package's directory.
    pub file: String,
    pub start: Option<Position>,
    pub end: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Specification {
    pub name: String,
    pub kind: String,
    pub statement: String,
    /// The body, pretty-printed, when the declaration is a definition.
    pub definition: Option<String>,
    pub package: Option<String>,
    pub fingerprint: String,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacilityForm {
    /// `@[extern]` operations the host implements, called synchronously.
    Extern,
    /// The constructors of an operation type, performed asynchronously by a handler the caller
    /// passes to each async export.
    Async,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facility {
    pub name: String,
    /// The namespaced identifier, such as `time.clock`.
    pub id: String,
    pub form: FacilityForm,
    /// The operation type of an async facility.
    pub op_type: Option<String>,
    pub operations: Vec<Operation>,
    /// The assumptions registered for this facility.
    pub assumptions: Vec<String>,
    pub package: Option<String>,
    pub fingerprint: String,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    /// The `@[extern]` declaration, or the constructor of an async facility's operation type.
    pub name: String,
    /// The extern key of a synchronous operation.
    pub symbol: Option<String>,
    /// The fingerprint of a synchronous operation's declaration (an async facility's operations
    /// are covered by the facility's).
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub name: String,
    pub facility: String,
    pub statement: String,
    /// The body, pretty-printed, when the declaration is a definition.
    pub definition: Option<String>,
    pub package: Option<String>,
    pub fingerprint: String,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    /// The evidence is a theorem the kernel checked, depending on no `sorry`.
    Proved,
    /// The evidence depends on `sorry`: the claim is stated, not proved. Present only when the
    /// trust policy allows `sorry`.
    Incomplete,
}

impl ClaimStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimStatus::Proved => "proved",
            ClaimStatus::Incomplete => "incomplete",
        }
    }
}

/// The axioms the evidence of a claim depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceTrust {
    pub axioms: Vec<String>,
    pub depends_on_sorry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    /// The evidence theorem; also the claim's identity.
    pub name: String,
    pub relation: String,
    pub subjects: Vec<String>,
    pub specifications: Vec<String>,
    pub statement: String,
    pub status: ClaimStatus,
    pub evidence_trust: EvidenceTrust,
    /// The assumptions the claim is conditional on.
    pub assumptions: Vec<String>,
    pub package: Option<String>,
    pub fingerprint: String,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub name: String,
    pub role: String,
    pub exported: bool,
}

/// What one export is, does and depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSummary {
    pub name: String,
    pub module: String,
    /// The export returns an async program (`Lungo.Async.Program`).
    pub r#async: bool,
    pub trust: Trust,
    /// The claims whose subjects include the export.
    pub claims: Vec<String>,
    /// The assumptions those claims are conditional on.
    pub assumptions: Vec<String>,
    /// The facilities the export needs the host to provide.
    pub facilities: Vec<String>,
    /// The roles registered for the export.
    pub roles: Vec<String>,
    pub source: Option<Source>,
}

impl AssuranceDocument {
    /// The document as written to `assurance.json`: pretty-printed, with a final newline.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("an assurance document serializes");
        s.push('\n');
        s
    }

    /// Reads a document, refusing one of another schema version.
    pub fn from_json(text: &str) -> Result<AssuranceDocument, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        match value.get("schema_version").and_then(|v| v.as_u64()) {
            Some(v) if v == u64::from(SCHEMA_VERSION) => {}
            Some(v) => return Err(format!("schema version {v}; this lungo reads version {SCHEMA_VERSION}")),
            None => return Err("no schema_version".into()),
        }
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    pub fn claim(&self, name: &str) -> Option<&Claim> {
        self.claims.iter().find(|c| c.name == name)
    }

    pub fn export(&self, name: &str) -> Option<&ExportSummary> {
        self.exports.iter().find(|e| e.name == name)
    }
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest of every definition in `definitions`: its canonical text, and the digests of the
/// definitions it uses, so a change anywhere below a definition changes it. Definitions using
/// each other (a strongly connected component of the graph) are digested together.
pub fn definition_digests(
    definitions: &[DefinitionRecord],
    lean_version: &str,
) -> Result<BTreeMap<String, String>, CodegenError> {
    let index: BTreeMap<&str, usize> = definitions.iter().enumerate().map(|(i, d)| (d.name.as_str(), i)).collect();
    let edges: Vec<Vec<usize>> = definitions
        .iter()
        .map(|d| {
            d.dependencies
                .iter()
                .map(|x| {
                    index.get(x.as_str()).copied().ok_or_else(|| {
                        CodegenError::internal(format!("the definition {} uses {x}, which the worker did not describe", d.name))
                    })
                })
                .collect()
        })
        .collect::<Result<_, _>>()?;
    let mut digests: Vec<Option<String>> = vec![None; definitions.len()];
    // Tarjan's algorithm, iteratively; it completes each component after every component it
    // uses, so their digests are known when it is digested.
    let n = definitions.len();
    let (mut order, mut low, mut on_stack) = (vec![usize::MAX; n], vec![0; n], vec![false; n]);
    let (mut stack, mut next) = (Vec::new(), 0);
    for root in 0..n {
        if order[root] != usize::MAX {
            continue;
        }
        let mut frames: Vec<(usize, usize)> = vec![(root, 0)];
        order[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&mut (v, ref mut k)) = frames.last_mut() {
            if *k < edges[v].len() {
                let w = edges[v][*k];
                *k += 1;
                if order[w] == usize::MAX {
                    order[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    frames.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(order[w]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(u, _)) = frames.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == order[v] {
                let mut component = Vec::new();
                loop {
                    let w = stack.pop().expect("v is on the stack");
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_by(|a, b| definitions[*a].name.cmp(&definitions[*b].name));
                let mut text = format!("{FINGERPRINT_FORMAT} definitions\nlean {lean_version}\n");
                for &m in &component {
                    let d = &definitions[m];
                    text.push_str(&format!("member {}\n{}\n", d.name, d.material));
                    let mut uses: Vec<usize> = edges[m].iter().copied().filter(|w| !component.contains(w)).collect();
                    uses.sort_by(|a, b| definitions[*a].name.cmp(&definitions[*b].name));
                    uses.dedup();
                    for w in uses {
                        let digest = digests[w].as_ref().expect("a component is digested after those it uses");
                        text.push_str(&format!("uses {} {digest}\n", definitions[w].name));
                    }
                }
                let combined = sha256(&text);
                for &m in &component {
                    digests[m] = Some(sha256(&format!("{combined}\n{}", definitions[m].name)));
                }
            }
        }
    }
    Ok(definitions.iter().zip(digests).map(|(d, x)| (d.name.clone(), x.expect("every definition is digested"))).collect())
}

/// The fingerprint of a record: the lowercase hexadecimal SHA-256 of its canonical text, as the
/// worker writes it, with the Lean version whose elaborator produced it and the digest of each
/// workspace definition the record uses.
pub fn fingerprint(
    material: &str,
    dependencies: &[String],
    digests: &BTreeMap<String, String>,
    lean_version: &str,
) -> Result<String, CodegenError> {
    let mut text = format!("{FINGERPRINT_FORMAT}\nlean {lean_version}\n{material}");
    for d in dependencies {
        let digest = digests
            .get(d)
            .ok_or_else(|| CodegenError::internal(format!("a record uses {d}, which the worker did not describe")))?;
        text.push_str(&format!("\nuses {d} {digest}"));
    }
    Ok(sha256(&text))
}

/// The status of a claim.
pub fn claim_status(c: &ClaimRecord) -> ClaimStatus {
    if c.evidence_trust.depends_on_sorry { ClaimStatus::Incomplete } else { ClaimStatus::Proved }
}

fn source(s: &Option<DeclSource>) -> Option<Source> {
    s.as_ref().map(|s| Source {
        package: s.location.package.clone(),
        file: s.location.path.clone(),
        start: s.range.map(|r| r.start),
        end: s.range.map(|r| r.end),
    })
}

/// The async operation type an export's result names, if it returns an async program.
pub fn async_op(e: &Export) -> Option<&str> {
    match &e.result {
        FacadeType::Async { op, .. } => Some(op),
        _ => None,
    }
}

/// Builds the document of `success`, for the program `program`.
pub fn document(
    success: &Success,
    toolchain: &WorkerToolchain,
    program: &str,
) -> Result<AssuranceDocument, CodegenError> {
    let a: &Assurance = &success.assurance;
    let lean = &toolchain.lean_version;
    let digests = definition_digests(&a.definitions, lean)?;
    let fp = |material: &str, dependencies: &[String]| fingerprint(material, dependencies, &digests, lean);
    let specifications = a
        .specs
        .iter()
        .map(|s| {
            Ok(Specification {
                name: s.name.clone(),
                kind: s.kind.clone(),
                statement: s.statement.clone(),
                definition: s.definition.clone(),
                package: s.origin.package.clone(),
                fingerprint: fp(&s.fingerprint_material, &s.dependencies)?,
                source: source(&s.origin.source),
            })
        })
        .collect::<Result<_, CodegenError>>()?;
    let mut facility_assumptions: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for x in &a.assumptions {
        facility_assumptions.entry(x.facility.as_str()).or_default().insert(x.name.clone());
    }
    let facilities: Vec<Facility> = a
        .facilities
        .iter()
        .map(|c| -> Result<Facility, CodegenError> {
            let (form, op_type, operations) = match &c.kind {
                FacilityKind::Extern => (
                    FacilityForm::Extern,
                    None,
                    a.operations
                        .iter()
                        .filter(|o| o.facility == c.name && o.reachable)
                        .map(|o| {
                            Ok(Operation {
                                name: o.name.clone(),
                                symbol: Some(o.symbol.clone()),
                                fingerprint: Some(fp(&o.fingerprint_material, &o.dependencies)?),
                            })
                        })
                        .collect::<Result<_, CodegenError>>()?,
                ),
                FacilityKind::Async { op_type, operations } => (
                    FacilityForm::Async,
                    Some(op_type.clone()),
                    operations.iter().map(|o| Operation { name: o.clone(), symbol: None, fingerprint: None }).collect(),
                ),
            };
            Ok(Facility {
                name: c.name.clone(),
                id: c.id.clone(),
                form,
                op_type,
                operations,
                assumptions: facility_assumptions
                    .get(c.name.as_str())
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default(),
                package: c.origin.package.clone(),
                fingerprint: fp(&c.fingerprint_material, &c.dependencies)?,
                source: source(&c.origin.source),
            })
        })
        .collect::<Result<_, _>>()?;
    let assumptions = a
        .assumptions
        .iter()
        .map(|x| {
            Ok(Assumption {
                name: x.name.clone(),
                facility: x.facility.clone(),
                statement: x.statement.clone(),
                definition: x.definition.clone(),
                package: x.origin.package.clone(),
                fingerprint: fp(&x.fingerprint_material, &x.dependencies)?,
                source: source(&x.origin.source),
            })
        })
        .collect::<Result<_, CodegenError>>()?;
    let claims: Vec<Claim> = a
        .claims
        .iter()
        .map(|c| {
            Ok(Claim {
            name: c.evidence.clone(),
            relation: c.relation.clone(),
            subjects: c.subjects.clone(),
            specifications: c.specs.clone(),
            statement: c.statement.clone(),
            status: claim_status(c),
            evidence_trust: EvidenceTrust {
                axioms: c.evidence_trust.axioms.clone(),
                depends_on_sorry: c.evidence_trust.depends_on_sorry,
            },
            assumptions: c.assumptions.clone(),
            package: c.origin.package.clone(),
            fingerprint: fp(&c.fingerprint_material, &c.dependencies)?,
            source: source(&c.origin.source),
            })
        })
        .collect::<Result<_, CodegenError>>()?;
    let roles =
        a.roles.iter().map(|r| Role { name: r.name.clone(), role: r.role.clone(), exported: r.exported }).collect();
    let symbol_facility: BTreeMap<&str, &str> =
        a.operations.iter().map(|o| (o.symbol.as_str(), o.facility.as_str())).collect();
    let async_facility: BTreeMap<&str, &str> = a
        .facilities
        .iter()
        .filter_map(|c| match &c.kind {
            FacilityKind::Async { op_type, .. } => Some((op_type.as_str(), c.name.as_str())),
            FacilityKind::Extern => None,
        })
        .collect();
    let mut exports: Vec<ExportSummary> = success
        .interface
        .exports
        .iter()
        .map(|e| {
            let on: Vec<&Claim> = claims.iter().filter(|c| c.subjects.contains(&e.name)).collect();
            let assumptions: BTreeSet<String> = on.iter().flat_map(|c| c.assumptions.iter().cloned()).collect();
            let mut facilities: BTreeSet<String> = e
                .trust
                .extern_dependencies
                .iter()
                .filter_map(|s| symbol_facility.get(s.as_str()).map(|c| (*c).to_owned()))
                .collect();
            if let Some(op) = async_op(e) {
                if let Some(c) = async_facility.get(op) {
                    facilities.insert((*c).to_owned());
                }
            }
            ExportSummary {
                name: e.name.clone(),
                module: e.module.clone(),
                r#async: async_op(e).is_some(),
                trust: e.trust.clone(),
                claims: on.iter().map(|c| c.name.clone()).collect(),
                assumptions: assumptions.into_iter().collect(),
                facilities: facilities.into_iter().collect(),
                roles: a.roles.iter().filter(|r| r.name == e.name).map(|r| r.role.clone()).collect(),
                source: source(&e.source),
            }
        })
        .collect();
    exports.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(AssuranceDocument {
        schema_version: SCHEMA_VERSION,
        program: program.to_owned(),
        provenance: Provenance {
            lean_version: toolchain.lean_version.clone(),
            lean_githash: toolchain.lean_githash.clone(),
            lungo_version: crate::GENERATOR_VERSION.to_owned(),
            bir_version: toolchain.bir_version,
            runtime_abi: lungo_runtime::ABI_VERSION,
        },
        library: a.library.as_ref().map(|l| Library { package: l.package.clone(), schema_version: l.schema_version }),
        specifications,
        facilities,
        assumptions,
        claims,
        roles,
        exports,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, material: &str, deps: &[&str]) -> DefinitionRecord {
        DefinitionRecord {
            name: name.into(),
            material: material.into(),
            dependencies: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    /// `spec` uses `helper`, which uses `base`; `even` and `odd` use each other.
    fn graph(base: &str) -> Vec<DefinitionRecord> {
        vec![
            def("base", base, &[]),
            def("helper", "helper's text", &["base"]),
            def("spec", "spec's text", &["helper"]),
            def("even", "even's text", &["odd", "base"]),
            def("odd", "odd's text", &["even"]),
        ]
    }

    fn record(defs: &[DefinitionRecord], uses: &[&str]) -> String {
        let digests = definition_digests(defs, "4.34.1").unwrap();
        let uses: Vec<String> = uses.iter().map(|u| u.to_string()).collect();
        fingerprint("the record's text", &uses, &digests, "4.34.1").unwrap()
    }

    /// TEST0347: a fingerprint covers what its record uses, transitively
    #[test]
    fn test0347_a_fingerprint_covers_what_its_record_uses_transitively() {
        let before = graph("base's text");
        let after = graph("base's text, changed");
        // A change two definitions down changes the record using the top one.
        assert_ne!(record(&before, &["spec"]), record(&after, &["spec"]));
        // And through mutual recursion.
        assert_ne!(record(&before, &["odd"]), record(&after, &["odd"]));
        // A record using nothing that changed keeps its fingerprint; the order the worker lists
        // definitions in does not matter.
        let mut reordered = before.clone();
        reordered.reverse();
        assert_eq!(record(&before, &["spec"]), record(&reordered, &["spec"]));
        let unrelated = vec![def("base", "base's text, changed", &[]), def("other", "other's text", &[])];
        let same = vec![def("base", "base's text", &[]), def("other", "other's text", &[])];
        assert_eq!(record(&same, &["other"]), record(&unrelated, &["other"]));
        // Members of one component are digested apart from each other.
        let digests = definition_digests(&before, "4.34.1").unwrap();
        assert_ne!(digests["even"], digests["odd"]);
        // The Lean version is part of every digest.
        assert_ne!(definition_digests(&before, "4.35.0").unwrap()["base"], digests["base"]);
        // A definition the worker did not describe is an error, not a silent omission.
        assert!(definition_digests(&[def("a", "", &["missing"])], "4.34.1").is_err());
        assert!(fingerprint("x", &["missing".into()], &digests, "4.34.1").is_err());
    }
}
