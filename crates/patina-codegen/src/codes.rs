//! Stable error codes.
//!
//! Every error patina reports carries one code, printed as `error[PTN0401]: …`. Codes are
//! stable identifiers: a code is never reused for a different condition. They are grouped by
//! the stage that detects the error, and each one is described in `docs/reference/errors.md`.

use std::fmt;

macro_rules! codes {
    ($($(#[$doc:meta])* $variant:ident = $code:literal, $title:literal;)*) => {
        /// A stable patina error code.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ErrorCode {
            $($(#[$doc])* $variant,)*
        }

        impl ErrorCode {
            /// Every code, in numeric order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),*];

            /// The code as printed, such as `PTN0401`.
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
    InvalidProject = "PTN0101", "invalid Lean project";
    /// `lean-toolchain` names a toolchain patina does not support.
    UnsupportedToolchain = "PTN0102", "unsupported Lean toolchain";
    /// The pinned toolchain is not installed.
    ToolchainNotInstalled = "PTN0103", "Lean toolchain not installed";
    /// The build environment lacks something patina requires, or cannot provide a requested
    /// capability.
    Environment = "PTN0104", "unsupported build environment";
    /// A file-system operation failed.
    Io = "PTN0105", "input/output error";
    /// A tool patina runs (Lake, `leanc`, `llvm-ar`) failed.
    CommandFailed = "PTN0106", "external command failed";

    // Lean (02xx).
    /// Lean rejected the program: elaboration, kernel checking, or Lean's compiler.
    LeanRejected = "PTN0201", "Lean rejected the program";

    // Worker (03xx).
    /// The configuration asks for something the program does not provide.
    UnsatisfiableRequest = "PTN0301", "request cannot be satisfied";
    /// The worker's adapter cannot represent Lean's compiler output.
    AdapterRejected = "PTN0302", "compiler output not representable";
    /// The worker ended without producing a response.
    WorkerCrashed = "PTN0303", "worker crashed";
    /// The worker did not finish within `worker_timeout`.
    WorkerTimeout = "PTN0304", "worker timed out";
    /// The worker exceeded a configured resource limit.
    WorkerResourceLimit = "PTN0305", "worker resource limit exceeded";
    /// The worker's response is malformed or of an incompatible version.
    Protocol = "PTN0306", "worker protocol error";

    // Externs (04xx).
    /// An `@[extern]` symbol has no implementation.
    UnresolvedExtern = "PTN0401", "unresolved extern symbol";
    /// An `@[extern]` symbol has more than one implementation.
    ConflictingExtern = "PTN0402", "extern symbol implemented twice";
    /// A `rust_extern` mapping names a symbol the runtime implements.
    RuntimeExternRemapped = "PTN0403", "runtime symbol remapped";
    /// A runtime primitive's representation differs from Lean's declaration.
    ExternRepresentation = "PTN0404", "extern representation mismatch";
    /// A `rust_extern` mapping names a symbol no reachable declaration uses.
    UnusedExternMapping = "PTN0405", "unused rust_extern mapping";
    /// An extern's Lean type does not determine a Rust signature for a `rust_extern` function.
    ExternSignature = "PTN0406", "extern has no Rust signature";
    /// LeanOracle mode cannot provide an application extern of this form.
    OracleExternForm = "PTN0407", "extern form unsupported in LeanOracle mode";

    // Code generation (05xx).
    /// The Bridge IR violates an invariant the backend relies on.
    InvalidBridgeIr = "PTN0501", "invalid Bridge IR";
    /// The compiler output contains a construct the backend does not implement.
    UnsupportedCompilerOutput = "PTN0502", "unsupported compiler output";
    /// An internal invariant of the code generator was violated.
    InternalGenerator = "PTN0503", "internal code generator error";

    // Trust policy (06xx).
    /// An export violates `deny_sorry`, `deny_axioms`, or `deny_unsafe`.
    TrustPolicy = "PTN0601", "trust policy violation";
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

    #[test]
    fn codes_are_unique_well_formed_and_ordered() {
        let mut seen = BTreeSet::new();
        let mut previous = "";
        for c in ErrorCode::ALL {
            let s = c.as_str();
            assert!(s.len() == 7 && s.starts_with("PTN") && s[3..].bytes().all(|b| b.is_ascii_digit()), "{s}");
            assert!(seen.insert(s), "{s} is used twice");
            assert!(previous < s, "{s} is out of order");
            previous = s;
            assert!(!c.title().is_empty());
        }
    }
}
