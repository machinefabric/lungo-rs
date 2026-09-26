//! Stable error codes.
//!
//! Every error lean2rust reports carries one code, printed as `error[L2R0401]: …`. Codes are
//! stable identifiers: a code is never reused for a different condition. They are grouped by
//! the stage that detects the error, and each one is described in `docs/reference/errors.md`.

use std::fmt;

macro_rules! codes {
    ($($(#[$doc:meta])* $variant:ident = $code:literal, $title:literal;)*) => {
        /// A stable lean2rust error code.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ErrorCode {
            $($(#[$doc])* $variant,)*
        }

        impl ErrorCode {
            /// Every code, in numeric order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),*];

            /// The code as printed, such as `L2R0401`.
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
    InvalidProject = "L2R0101", "invalid Lean project";
    /// `lean-toolchain` names a toolchain lean2rust does not support.
    UnsupportedToolchain = "L2R0102", "unsupported Lean toolchain";
    /// The pinned toolchain is not installed.
    ToolchainNotInstalled = "L2R0103", "Lean toolchain not installed";
    /// The build environment lacks something lean2rust requires, or cannot provide a requested
    /// capability.
    Environment = "L2R0104", "unsupported build environment";
    /// A file-system operation failed.
    Io = "L2R0105", "input/output error";
    /// A tool lean2rust runs (Lake, `leanc`, `llvm-ar`) failed.
    CommandFailed = "L2R0106", "external command failed";

    // Lean (02xx).
    /// Lean rejected the program: elaboration, kernel checking, or Lean's compiler.
    LeanRejected = "L2R0201", "Lean rejected the program";

    // Worker (03xx).
    /// The configuration asks for something the program does not provide.
    UnsatisfiableRequest = "L2R0301", "request cannot be satisfied";
    /// The worker's adapter cannot represent Lean's compiler output.
    AdapterRejected = "L2R0302", "compiler output not representable";
    /// The worker ended without producing a response.
    WorkerCrashed = "L2R0303", "worker crashed";
    /// The worker did not finish within `worker_timeout`.
    WorkerTimeout = "L2R0304", "worker timed out";
    /// The worker exceeded a configured resource limit.
    WorkerResourceLimit = "L2R0305", "worker resource limit exceeded";
    /// The worker's response is malformed or of an incompatible version.
    Protocol = "L2R0306", "worker protocol error";

    // Externs (04xx).
    /// An `@[extern]` symbol has no implementation.
    UnresolvedExtern = "L2R0401", "unresolved extern symbol";
    /// An `@[extern]` symbol has more than one implementation.
    ConflictingExtern = "L2R0402", "extern symbol implemented twice";
    /// A `rust_extern` mapping names a symbol the runtime implements.
    RuntimeExternRemapped = "L2R0403", "runtime symbol remapped";
    /// A runtime primitive's representation differs from Lean's declaration.
    ExternRepresentation = "L2R0404", "extern representation mismatch";
    /// A `rust_extern` mapping names a symbol no reachable declaration uses.
    UnusedExternMapping = "L2R0405", "unused rust_extern mapping";
    /// An extern's Lean type does not determine a Rust signature for a `rust_extern` function.
    ExternSignature = "L2R0406", "extern has no Rust signature";
    /// LeanOracle mode cannot provide an application extern of this form.
    OracleExternForm = "L2R0407", "extern form unsupported in LeanOracle mode";

    // Code generation (05xx).
    /// The Bridge IR violates an invariant the backend relies on.
    InvalidBridgeIr = "L2R0501", "invalid Bridge IR";
    /// The compiler output contains a construct the backend does not implement.
    UnsupportedCompilerOutput = "L2R0502", "unsupported compiler output";
    /// An internal invariant of the code generator was violated.
    InternalGenerator = "L2R0503", "internal code generator error";

    // Trust policy (06xx).
    /// An export violates `deny_sorry`, `deny_axioms`, or `deny_unsafe`.
    TrustPolicy = "L2R0601", "trust policy violation";
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
            assert!(s.len() == 7 && s.starts_with("L2R") && s[3..].bytes().all(|b| b.is_ascii_digit()), "{s}");
            assert!(seen.insert(s), "{s} is used twice");
            assert!(previous < s, "{s} is out of order");
            previous = s;
            assert!(!c.title().is_empty());
        }
    }
}
