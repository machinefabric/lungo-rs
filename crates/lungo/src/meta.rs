/// A one-based line and column in a Lean source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

/// The trust assumptions an exported declaration's compiled code depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportTrust {
    /// Axioms the definition depends on, excluding `sorryAx`.
    pub axioms: &'static [&'static str],
    pub depends_on_sorry: bool,
    /// `unsafe` constants in the executable closure: the declaration itself, and those outside
    /// the Lean toolchain.
    pub unsafe_dependencies: &'static [&'static str],
    /// `partial` constants in the executable closure, with the same scope.
    pub partial_dependencies: &'static [&'static str],
    /// External symbols the compiled code calls.
    pub extern_dependencies: &'static [&'static str],
}

/// Static metadata about a Lean declaration with a generated Rust facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarationInfo {
    /// The fully qualified Lean name: the canonical identity of the declaration.
    pub lean_name: &'static str,
    pub module: &'static str,
    /// The Lean source file, relative to the Cargo package (or `<package>/…` for dependencies).
    pub source_file: Option<&'static str>,
    pub range: Option<SourceRange>,
    /// The declaration's Lean type.
    pub lean_type: &'static str,
    /// The signature of the compiled declaration in runtime representation types.
    pub compiled_signature: &'static str,
    /// The path of the generated Rust item, relative to the generated module.
    pub rust_path: &'static str,
    pub trust: ExportTrust,
    pub assurance: ExportAssurance,
}

/// Whether a claim is proved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimStatus {
    /// Its evidence is a theorem Lean's kernel checked, depending on no `sorry`.
    Proved,
    /// Its evidence depends on `sorry`: stated, not proved.
    Incomplete,
}

/// A claim: a theorem (the evidence) proving that its subjects, executable definitions, stand in a
/// relation to specifications. It holds of a running program only when the host satisfies its
/// assumptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    /// The evidence theorem; also the claim's identity.
    pub name: &'static str,
    /// The relation, a namespaced kind such as `lungo.decides`.
    pub relation: &'static str,
    pub subjects: &'static [&'static str],
    pub specifications: &'static [&'static str],
    /// The evidence's statement.
    pub statement: &'static str,
    pub status: ClaimStatus,
    /// Axioms the evidence depends on, excluding `sorryAx`.
    pub evidence_axioms: &'static [&'static str],
    /// The assumptions about the host the claim is conditional on.
    pub assumptions: &'static [&'static str],
    /// The Lake package declaring the evidence.
    pub package: Option<&'static str>,
    pub fingerprint: &'static str,
}

/// A specification the program's claims cite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Specification {
    pub name: &'static str,
    pub kind: &'static str,
    pub statement: &'static str,
    pub package: Option<&'static str>,
    pub fingerprint: &'static str,
}

/// A proposition assumed, never proved, of the host's implementation of a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assumption {
    pub name: &'static str,
    /// The capability it is assumed of.
    pub capability: &'static str,
    pub statement: &'static str,
    pub package: Option<&'static str>,
    pub fingerprint: &'static str,
}

/// A capability the host provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// The Lean declaration registered as the capability.
    pub name: &'static str,
    /// The namespaced identifier, such as `time.clock`.
    pub id: &'static str,
    /// Whether its operations are performed asynchronously, by a handler async functions take.
    pub asynchronous: bool,
    pub operations: &'static [&'static str],
    /// The assumptions registered for it.
    pub assumptions: &'static [&'static str],
    pub package: Option<&'static str>,
    pub fingerprint: &'static str,
}

/// What a declaration is for: an implementation, an oracle, a monitor, a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role {
    pub name: &'static str,
    pub role: &'static str,
    pub exported: bool,
}

/// The program's assurance: every record of `assurance.json`, each array sorted by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assurance {
    pub schema_version: u32,
    pub program: &'static str,
    pub specifications: &'static [Specification],
    pub capabilities: &'static [Capability],
    pub assumptions: &'static [Assumption],
    pub claims: &'static [Claim],
    pub roles: &'static [Role],
}

impl Assurance {
    /// The claim whose evidence is `name`.
    pub fn claim(&self, name: &str) -> Option<&'static Claim> {
        self.claims.binary_search_by(|c| c.name.cmp(name)).ok().map(|i| &self.claims[i])
    }

    /// The claims about the declaration `lean_name`.
    pub fn claims_about<'n>(&self, lean_name: &'n str) -> impl Iterator<Item = &'static Claim> + 'n {
        let claims: &'static [Claim] = self.claims;
        claims.iter().filter(move |c| c.subjects.contains(&lean_name))
    }

    /// The capability declared as `name`.
    pub fn capability(&self, name: &str) -> Option<&'static Capability> {
        self.capabilities.binary_search_by(|c| c.name.cmp(name)).ok().map(|i| &self.capabilities[i])
    }

    /// The assumption declared as `name`.
    pub fn assumption(&self, name: &str) -> Option<&'static Assumption> {
        self.assumptions.binary_search_by(|a| a.name.cmp(name)).ok().map(|i| &self.assumptions[i])
    }

    /// The specification declared as `name`.
    pub fn specification(&self, name: &str) -> Option<&'static Specification> {
        self.specifications.binary_search_by(|s| s.name.cmp(name)).ok().map(|i| &self.specifications[i])
    }
}

/// What an export's assurance is: the claims about it, the assumptions they are conditional on,
/// the capabilities it needs, and its roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportAssurance {
    pub claims: &'static [&'static str],
    pub assumptions: &'static [&'static str],
    pub capabilities: &'static [&'static str],
    pub roles: &'static [&'static str],
    /// Whether the export returns an async program.
    pub asynchronous: bool,
}
