//! The assurance policy: what a project requires of the claims about its exports and of the
//! assumptions they rest on, checked against the records the worker read.
//!
//! The records themselves are checked first: every problem the worker found in them (a record not
//! in the library's form, a claim whose evidence is not a theorem or does not mention its subject,
//! an operation that is not an extern, …) is an error with its own code, whatever the policy.
//! Then the policy (`[assurance]` in `lungo.toml`, `Builder::require_claims` and
//! `Builder::forbid_assumption`): exports that must carry a proved claim, and assumptions or
//! facilities nothing may rest on. The trust policy's `deny_sorry` and `deny_axioms` apply to
//! the evidence of claims through [`crate::LeanOptions::check_trust`].

use crate::{Error, Result};
use lungo_codegen::ErrorCode;
use lungo_codegen::core::assurance::{ClaimStatus, claim_status};
use lungo_protocol::{Success, ViolationKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What a project requires of its assurance. Loadable from the `[assurance]` table of a
/// `lungo.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AssurancePolicy {
    /// Exports that must each be the subject of at least one proved claim, selected by path:
    /// `.` selects every export; any other path selects the export it names and every export in
    /// it as a namespace.
    pub require_claims: Vec<String>,
    /// Assumptions, or facilities (all of whose assumptions and operations), that no proved
    /// claim and no export may rest on, by Lean name.
    pub forbid_assumptions: Vec<String>,
}

impl AssurancePolicy {
    pub fn require_claims(mut self, path: impl Into<String>) -> Self {
        self.require_claims.push(path.into());
        self
    }

    pub fn forbid_assumption(mut self, name: impl Into<String>) -> Self {
        self.forbid_assumptions.push(name.into());
        self
    }

    /// Checks the program's records, then this policy.
    pub fn check(&self, success: &Success) -> Result<()> {
        let mut issues = record_issues(success);
        issues.extend(self.policy_issues(success)?);
        if issues.is_empty() { Ok(()) } else { Err(Error::Assurance(issues)) }
    }

    fn policy_issues(&self, success: &Success) -> Result<Vec<AssuranceIssue>> {
        let a = &success.assurance;
        let exports: Vec<&str> = success.interface.exports.iter().map(|e| e.name.as_str()).collect();
        let mut issues = Vec::new();
        // Claims that are proved, by subject.
        let mut proved: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for c in &a.claims {
            if claim_status(c) == ClaimStatus::Proved {
                for s in &c.subjects {
                    proved.entry(s.as_str()).or_default().push(c.evidence.as_str());
                }
            }
        }
        for path in &self.require_claims {
            let selected: Vec<&str> = exports.iter().copied().filter(|e| selects(path, e)).collect();
            if selected.is_empty() && path != "." {
                return Err(Error::Configuration(format!(
                    "`require-claims` names {path:?}, which selects no export of the program"
                )));
            }
            for e in selected {
                if !proved.contains_key(e) {
                    issues.push(AssuranceIssue {
                        code: ErrorCode::ExportWithoutClaim,
                        message: format!(
                            "the export {e} has no proved claim (`require-claims` selects it with {path:?}); state \
                             what it does with `@[lungo_claim]` on a theorem about it"
                        ),
                    });
                }
            }
        }
        let assumptions: BTreeMap<&str, &str> =
            a.assumptions.iter().map(|x| (x.name.as_str(), x.facility.as_str())).collect();
        let facilities: BTreeSet<&str> = a.facilities.iter().map(|c| c.name.as_str()).collect();
        for name in &self.forbid_assumptions {
            if !assumptions.contains_key(name.as_str()) && !facilities.contains(name.as_str()) {
                return Err(Error::Configuration(format!(
                    "`forbid-assumptions` names {name:?}, which is neither an assumption nor a facility of the program"
                )));
            }
        }
        let forbidden = |assumption: &str| {
            self.forbid_assumptions
                .iter()
                .any(|f| f == assumption || assumptions.get(assumption).is_some_and(|facility| f == facility))
        };
        for c in &a.claims {
            if claim_status(c) != ClaimStatus::Proved {
                continue;
            }
            for x in &c.assumptions {
                if forbidden(x) {
                    issues.push(AssuranceIssue {
                        code: ErrorCode::ForbiddenAssumption,
                        message: format!("the claim {} assumes {x}, which `forbid-assumptions` forbids", c.evidence),
                    });
                }
            }
        }
        let forbidden_symbols: BTreeMap<&str, &str> = a
            .operations
            .iter()
            .filter(|o| self.forbid_assumptions.iter().any(|f| f == &o.facility))
            .map(|o| (o.symbol.as_str(), o.facility.as_str()))
            .collect();
        for e in &success.interface.exports {
            for s in &e.trust.extern_dependencies {
                if let Some(facility) = forbidden_symbols.get(s.as_str()) {
                    issues.push(AssuranceIssue {
                        code: ErrorCode::ForbiddenAssumption,
                        message: format!(
                            "the export {} needs the facility {facility} (its operation `{s}`), which `forbid-assumptions` forbids",
                            e.name
                        ),
                    });
                }
            }
        }
        Ok(issues)
    }
}

/// Whether the policy path `path` selects the export `name`.
pub fn selects(path: &str, name: &str) -> bool {
    path == "." || name == path || name.strip_prefix(path).is_some_and(|rest| rest.starts_with('.'))
}

/// A problem with the program's assurance, with its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssuranceIssue {
    pub code: ErrorCode,
    pub message: String,
}

/// The problems the worker found in the program's records.
pub fn record_issues(success: &Success) -> Vec<AssuranceIssue> {
    success
        .assurance
        .violations
        .iter()
        .map(|v| AssuranceIssue {
            code: match v.kind {
                ViolationKind::MalformedRecord => ErrorCode::MalformedAssuranceRecord,
                ViolationKind::DanglingReference => ErrorCode::DanglingAssuranceReference,
                ViolationKind::InvalidClaim => ErrorCode::InvalidClaim,
                ViolationKind::NotInStatement => ErrorCode::ClaimNotInStatement,
                ViolationKind::DuplicateId => ErrorCode::DuplicateAssuranceId,
                ViolationKind::FacilityMismatch => ErrorCode::FacilityMismatch,
                ViolationKind::AsyncInterface => ErrorCode::AsyncInterface,
                ViolationKind::LibraryVersion => ErrorCode::AssuranceLibraryVersion,
                ViolationKind::MetadataOnlyDependency => ErrorCode::MetadataOnlyDependency,
            },
            message: match &v.declaration {
                Some(d) => format!("{}\n  Lean declaration: {d}", v.message),
                None => v.message.clone(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0270: policy paths select an export, a namespace, or everything
    #[test]
    fn test0270_policy_paths_select_an_export_a_namespace_or_everything() {
        assert!(selects(".", "A.f"));
        assert!(selects("A", "A.f"));
        assert!(selects("A.f", "A.f"));
        assert!(!selects("A.f", "A.fg"), "a path is a namespace, not a string prefix");
        assert!(!selects("A.fg", "A.f"));
        assert!(!selects("B", "A.f"));
    }

    /// TEST0271: the policy reads from toml and rejects unknown keys
    #[test]
    fn test0271_the_policy_reads_from_toml_and_rejects_unknown_keys() {
        let p: AssurancePolicy =
            toml::from_str("require-claims = [\".\"]\nforbid-assumptions = [\"Clock.Monotone\"]\n").unwrap();
        assert_eq!(p, AssurancePolicy::default().require_claims(".").forbid_assumption("Clock.Monotone"));
        assert!(toml::from_str::<AssurancePolicy>("require-claim = []\n").is_err());
    }
}
