//! Stable error codes.
//!
//! Every error lungo reports carries one code, printed as `error[LNG0401]: …`. Codes are
//! stable identifiers: a code is never reused for a different condition. They are grouped by
//! the stage that detects the error, and each one is described in `docs/reference/errors.md` of lungo-rs.

use std::fmt;

macro_rules! codes {
    ($($(#[$doc:meta])* $variant:ident = $code:literal, $title:literal;)*) => {
        /// A stable lungo error code.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ErrorCode {
            $($(#[$doc])* $variant,)*
        }

        impl ErrorCode {
            /// Every code, in numeric order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),*];

            /// The code as printed, such as `LNG0401`.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(ErrorCode::$variant => $code,)*
                }
            }

            /// A one-line description of the condition.
            pub fn title(self) -> &'static str {
                match self {
                    $(ErrorCode::$variant => $title,)*
                }
            }
        }
    };
}

codes! {
    // Project and toolchain (01xx).
    /// The Lake project is incomplete or inconsistent.
    InvalidProject = "LNG0101", "invalid Lean project";
    /// `lean-toolchain` names a toolchain lungo does not support.
    UnsupportedToolchain = "LNG0102", "unsupported Lean toolchain";
    /// The pinned toolchain is not installed.
    ToolchainNotInstalled = "LNG0103", "Lean toolchain not installed";
    /// The build environment lacks something lungo requires, or cannot provide a requested
    /// capability.
    Environment = "LNG0104", "unsupported build environment";
    /// A file-system operation failed.
    Io = "LNG0105", "input/output error";
    /// A tool lungo runs (Lake, `leanc`, `llvm-ar`) failed.
    CommandFailed = "LNG0106", "external command failed";
    /// The build configuration cannot be applied: an invalid or conflicting output name, or a
    /// setting that selects nothing.
    InvalidConfiguration = "LNG0107", "invalid configuration";
    /// No lungo runtime is available for a language binding: this lungo is not a release and
    /// no local runtime distribution was given, or the release has no runtime for the target.
    RuntimeUnavailable = "LNG0108", "lungo runtime unavailable";
    /// A downloaded runtime artifact does not have the SHA-256 digest the release lists.
    RuntimeChecksum = "LNG0109", "runtime artifact checksum mismatch";
    /// `lungo generate --verify`: an output directory holds something other than what the
    /// project generates now.
    OutputDrift = "LNG0110", "generated output is out of date";

    // Lean (02xx).
    /// Lean rejected the program: elaboration, kernel checking, or Lean's compiler.
    LeanRejected = "LNG0201", "Lean rejected the program";

    // Worker (03xx).
    /// The configuration asks for something the program does not provide.
    UnsatisfiableRequest = "LNG0301", "request cannot be satisfied";
    /// The worker's adapter cannot represent Lean's compiler output.
    AdapterRejected = "LNG0302", "compiler output not representable";
    /// The worker ended without producing a response.
    WorkerCrashed = "LNG0303", "worker crashed";
    /// The worker did not finish within `worker_timeout`.
    WorkerTimeout = "LNG0304", "worker timed out";
    /// The worker exceeded a configured resource limit.
    WorkerResourceLimit = "LNG0305", "worker resource limit exceeded";
    /// The worker's response is malformed or of an incompatible version.
    Protocol = "LNG0306", "worker protocol error";

    // Externs (04xx).
    /// An `@[extern]` symbol has no implementation.
    UnresolvedExtern = "LNG0401", "unresolved extern symbol";
    /// An `@[extern]` symbol has more than one implementation.
    ConflictingExtern = "LNG0402", "extern symbol implemented twice";
    /// A `rust_extern` mapping names a symbol the runtime implements.
    RuntimeExternRemapped = "LNG0403", "runtime symbol remapped";
    /// A runtime primitive's representation differs from Lean's declaration.
    ExternRepresentation = "LNG0404", "extern representation mismatch";
    /// A `rust_extern` mapping names a symbol no reachable declaration uses.
    UnusedExternMapping = "LNG0405", "unused rust_extern mapping";
    /// An extern's Lean type does not determine a Rust signature for a `rust_extern` function.
    ExternSignature = "LNG0406", "extern has no Rust signature";
    /// LeanOracle mode cannot provide an application extern of this form.
    OracleExternForm = "LNG0407", "extern form unsupported in LeanOracle mode";
    /// The program needs a runtime primitive the target does not provide (WebAssembly has no
    /// threads, processes or sockets).
    UnsupportedOnTarget = "LNG0408", "primitive unsupported on the target";

    // Code generation (05xx).
    /// The Bridge IR violates an invariant the backend relies on.
    InvalidBridgeIr = "LNG0501", "invalid Bridge IR";
    /// The compiler output contains a construct the backend does not implement.
    UnsupportedCompilerOutput = "LNG0502", "unsupported compiler output";
    /// An internal invariant of the code generator was violated.
    InternalGenerator = "LNG0503", "internal code generator error";
    /// A generator plugin (`lungo-gen-<language>`) failed, or its response is malformed.
    PluginFailed = "LNG0504", "generator plugin failed";

    // Trust and assurance policy (06xx).
    /// An export, or the evidence of a claim, violates `deny_sorry`, `deny_axioms`, or
    /// `deny_unsafe`.
    TrustPolicy = "LNG0601", "trust policy violation";
    /// An export `require-claims` selects has no proved claim.
    ExportWithoutClaim = "LNG0602", "export without a proved claim";
    /// A proved claim, or an export, depends on an assumption or facility
    /// `forbid-assumptions` names.
    ForbiddenAssumption = "LNG0603", "forbidden assumption";

    // Assurance records (07xx).
    /// A record is not in the form lungo's Lean library writes, or names an invalid kind,
    /// relation, role or identifier.
    MalformedAssuranceRecord = "LNG0701", "malformed assurance record";
    /// A record names a declaration that does not exist or is not registered as it must be.
    DanglingAssuranceReference = "LNG0702", "dangling assurance reference";
    /// A claim's evidence is not a theorem, its subject is a theorem, or its statement does not
    /// have the shape of its relation.
    InvalidClaim = "LNG0703", "invalid claim";
    /// The statement of a claim's evidence does not mention one of its subjects or
    /// specifications.
    ClaimNotInStatement = "LNG0704", "claim subject not in its statement";
    /// Two records carry the same identifier.
    DuplicateAssuranceId = "LNG0705", "duplicate assurance identifier";
    /// An operation of a facility is not an extern the host can implement, or a `rust_extern`
    /// mapping names an extern that is not an operation.
    FacilityMismatch = "LNG0706", "facility mismatch";
    /// An async facility's interface, or an async export, cannot cross to the host.
    AsyncInterface = "LNG0707", "invalid async interface";
    /// `lungo assurance --compose`: two packages describe the same record differently.
    FingerprintMismatch = "LNG0708", "assurance fingerprint mismatch";
    /// lungo's Lean library writes records in a format this lungo does not read.
    AssuranceLibraryVersion = "LNG0709", "incompatible assurance library";
    /// The program uses a value computed by the initializer of a module it does not link: one
    /// loaded only for its assurance records, or reached only through lungo's compile-time
    /// attributes.
    MetadataOnlyDependency = "LNG0710", "program depends on an unlinked module's initialization";
    /// An `assurance.json` cannot be read: it is malformed or of an unknown schema version.
    InvalidAssuranceDocument = "LNG0711", "invalid assurance document";
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// TEST0119: codes are unique well formed and ordered
    #[test]
    fn test0119_codes_are_unique_well_formed_and_ordered() {
        let mut seen = BTreeSet::new();
        let mut previous = "";
        for c in ErrorCode::ALL {
            let s = c.as_str();
            assert!(s.len() == 7 && s.starts_with("LNG") && s[3..].bytes().all(|b| b.is_ascii_digit()), "{s}");
            assert!(seen.insert(s), "{s} is used twice");
            assert!(previous < s, "{s} is out of order");
            previous = s;
            assert!(!c.title().is_empty());
        }
    }
}
