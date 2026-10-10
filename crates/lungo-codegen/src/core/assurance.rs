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
//! - **assumptions**: propositions about the host's capabilities a claim's statement takes as
//!   hypotheses. A claim with assumptions holds of a running program only when the host's
//!   implementation satisfies them; registering an implementation proves nothing about it.
//!
//! Every record carries the Lake package declaring it and a fingerprint of its meaning (its
//! statement, and the definition of a specification), so documents of separately generated
//! packages can be checked to agree on what a shared name means (`lungo assurance --compose`).

use lungo_protocol::{
    Assurance, CapabilityKind, ClaimRecord, DeclSource, Export, FacadeType, Position, Success, Trust,
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
    pub capabilities: Vec<Capability>,
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
    pub package: Option<String>,
    pub fingerprint: String,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityForm {
    /// `@[extern]` operations the host implements, called synchronously.
    Extern,
    /// The constructors of an operation type, performed asynchronously by a handler the caller
    /// passes to each async export.
    Async,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub name: String,
    /// The namespaced identifier, such as `time.clock`.
    pub id: String,
    pub form: CapabilityForm,
    /// The operation type of an async capability.
    pub op_type: Option<String>,
    pub operations: Vec<Operation>,
    /// The assumptions registered for this capability.
    pub assumptions: Vec<String>,
    pub package: Option<String>,
    pub fingerprint: String,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    /// The `@[extern]` declaration, or the constructor of an async capability's operation type.
    pub name: String,
    /// The extern key of a synchronous operation.
    pub symbol: Option<String>,
    /// The fingerprint of a synchronous operation's declaration (an async capability's operations
    /// are covered by the capability's).
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub name: String,
    pub capability: String,
    pub statement: String,
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
    /// The capabilities the export needs the host to provide.
    pub capabilities: Vec<String>,
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

/// The fingerprint of a record: the lowercase hexadecimal SHA-256 of its canonical text, as the
/// worker writes it, with the Lean version whose elaborator produced it.
pub fn fingerprint(material: &str, lean_version: &str) -> String {
    let text = format!("{FINGERPRINT_FORMAT}\nlean {lean_version}\n{material}");
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
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
pub fn document(success: &Success, toolchain: &WorkerToolchain, program: &str) -> AssuranceDocument {
    let a: &Assurance = &success.assurance;
    let lean = &toolchain.lean_version;
    let specifications = a
        .specs
        .iter()
        .map(|s| Specification {
            name: s.name.clone(),
            kind: s.kind.clone(),
            statement: s.statement.clone(),
            package: s.origin.package.clone(),
            fingerprint: fingerprint(&s.fingerprint_material, lean),
            source: source(&s.origin.source),
        })
        .collect();
    let mut capability_assumptions: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for x in &a.assumptions {
        capability_assumptions.entry(x.capability.as_str()).or_default().insert(x.name.clone());
    }
    let capabilities: Vec<Capability> = a
        .capabilities
        .iter()
        .map(|c| {
            let (form, op_type, operations) = match &c.kind {
                CapabilityKind::Extern => (
                    CapabilityForm::Extern,
                    None,
                    a.operations
                        .iter()
                        .filter(|o| o.capability == c.name && o.reachable)
                        .map(|o| Operation {
                            name: o.name.clone(),
                            symbol: Some(o.symbol.clone()),
                            fingerprint: Some(fingerprint(&o.fingerprint_material, lean)),
                        })
                        .collect(),
                ),
                CapabilityKind::Async { op_type, operations } => (
                    CapabilityForm::Async,
                    Some(op_type.clone()),
                    operations.iter().map(|o| Operation { name: o.clone(), symbol: None, fingerprint: None }).collect(),
                ),
            };
            Capability {
                name: c.name.clone(),
                id: c.id.clone(),
                form,
                op_type,
                operations,
                assumptions: capability_assumptions.get(c.name.as_str()).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
                package: c.origin.package.clone(),
                fingerprint: fingerprint(&c.fingerprint_material, lean),
                source: source(&c.origin.source),
            }
        })
        .collect();
    let assumptions = a
        .assumptions
        .iter()
        .map(|x| Assumption {
            name: x.name.clone(),
            capability: x.capability.clone(),
            statement: x.statement.clone(),
            package: x.origin.package.clone(),
            fingerprint: fingerprint(&x.fingerprint_material, lean),
            source: source(&x.origin.source),
        })
        .collect();
    let claims: Vec<Claim> = a
        .claims
        .iter()
        .map(|c| Claim {
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
            fingerprint: fingerprint(&c.fingerprint_material, lean),
            source: source(&c.origin.source),
        })
        .collect();
    let roles = a.roles.iter().map(|r| Role { name: r.name.clone(), role: r.role.clone(), exported: r.exported }).collect();
    let symbol_capability: BTreeMap<&str, &str> =
        a.operations.iter().map(|o| (o.symbol.as_str(), o.capability.as_str())).collect();
    let async_capability: BTreeMap<&str, &str> = a
        .capabilities
        .iter()
        .filter_map(|c| match &c.kind {
            CapabilityKind::Async { op_type, .. } => Some((op_type.as_str(), c.name.as_str())),
            CapabilityKind::Extern => None,
        })
        .collect();
    let mut exports: Vec<ExportSummary> = success
        .interface
        .exports
        .iter()
        .map(|e| {
            let on: Vec<&Claim> = claims.iter().filter(|c| c.subjects.contains(&e.name)).collect();
            let assumptions: BTreeSet<String> = on.iter().flat_map(|c| c.assumptions.iter().cloned()).collect();
            let mut caps: BTreeSet<String> = e
                .trust
                .extern_dependencies
                .iter()
                .filter_map(|s| symbol_capability.get(s.as_str()).map(|c| (*c).to_owned()))
                .collect();
            if let Some(op) = async_op(e) {
                if let Some(c) = async_capability.get(op) {
                    caps.insert((*c).to_owned());
                }
            }
            ExportSummary {
                name: e.name.clone(),
                module: e.module.clone(),
                r#async: async_op(e).is_some(),
                trust: e.trust.clone(),
                claims: on.iter().map(|c| c.name.clone()).collect(),
                assumptions: assumptions.into_iter().collect(),
                capabilities: caps.into_iter().collect(),
                roles: a.roles.iter().filter(|r| r.name == e.name).map(|r| r.role.clone()).collect(),
                source: source(&e.source),
            }
        })
        .collect();
    exports.sort_by(|a, b| a.name.cmp(&b.name));
    AssuranceDocument {
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
        capabilities,
        assumptions,
        claims,
        roles,
        exports,
    }
}

