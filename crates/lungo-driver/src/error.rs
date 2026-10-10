use lungo_codegen::{CodegenError, ErrorCode};
use lungo_protocol::{Diagnostic, DiagnosticKind, Severity};
use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// Every way generation can fail. Lean errors, project errors, and backend errors are kept
/// distinct so that invalid Lean is never reported as a backend failure or vice versa. Each
/// error has a stable [`ErrorCode`] ([`Error::code`]), printed as `error[LNG0401]: …`.
///
/// `Debug` formats like `Display`, so a build script returning this error (`fn main() ->
/// Result<()>`) shows Cargo the readable message.
pub enum Error {
    Io {
        context: String,
        source: std::io::Error,
    },
    /// The Lake project is incomplete or inconsistent.
    Project(String),
    /// The project's `lean-toolchain` names a toolchain this bridge does not support.
    UnsupportedToolchain {
        found: String,
        supported: Vec<String>,
    },
    /// The pinned toolchain is not installed and installation was not permitted.
    ToolchainNotInstalled {
        toolchain: String,
        expected_at: PathBuf,
    },
    /// Lean rejected the program (elaboration, kernel checking, or Lean's compiler).
    LeanElaboration {
        output: String,
    },
    /// Structured diagnostics reported by the worker.
    Worker {
        toolchain: String,
        diagnostics: Vec<Diagnostic>,
    },
    /// The worker process failed without producing a response.
    WorkerCrashed {
        status: String,
        stderr: String,
        /// The resource limits in effect, which a failure may be due to.
        limits: Option<String>,
    },
    /// The worker exceeded a configured resource limit and was stopped by the operating system.
    WorkerResourceLimit {
        limit: String,
        stderr: String,
    },
    WorkerTimeout {
        seconds: u64,
        stderr: String,
    },
    Protocol(String),
    /// Code generation failed; every error found is reported.
    Codegen {
        toolchain: String,
        bir_version: u32,
        errors: Vec<CodegenError>,
    },
    /// An export, or the evidence of a claim, violates the configured trust policy.
    Trust(Vec<String>),
    /// The program's assurance records are invalid, or violate the assurance policy; every
    /// problem found is reported.
    Assurance(Vec<crate::AssuranceIssue>),
    /// A command the bridge runs failed.
    Command {
        program: String,
        status: String,
        output: String,
    },
    /// Environment the bridge requires is missing.
    Environment(String),
    /// The build configuration cannot be applied.
    Configuration(String),
    /// No lungo runtime is available for a language binding.
    RuntimeUnavailable(String),
    /// A runtime artifact does not have its recorded digest.
    RuntimeChecksum(String),
    /// An output directory is not what the project generates now.
    OutputDrift(String),
    /// A generator plugin failed.
    Plugin(String),
}

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io { context: context.into(), source }
    }

    /// The stable code of this error. For several code generation errors, the code of the
    /// first; each is printed with its own code.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Io { .. } => ErrorCode::Io,
            Error::Project(_) => ErrorCode::InvalidProject,
            Error::UnsupportedToolchain { .. } => ErrorCode::UnsupportedToolchain,
            Error::ToolchainNotInstalled { .. } => ErrorCode::ToolchainNotInstalled,
            Error::LeanElaboration { .. } => ErrorCode::LeanRejected,
            Error::Worker { diagnostics, .. } => worker_code(diagnostics),
            Error::WorkerCrashed { .. } => ErrorCode::WorkerCrashed,
            Error::WorkerResourceLimit { .. } => ErrorCode::WorkerResourceLimit,
            Error::WorkerTimeout { .. } => ErrorCode::WorkerTimeout,
            Error::Protocol(_) => ErrorCode::Protocol,
            Error::Codegen { errors, .. } => errors.first().map_or(ErrorCode::InternalGenerator, |e| e.code()),
            Error::Trust(_) => ErrorCode::TrustPolicy,
            Error::Assurance(issues) => issues.first().map_or(ErrorCode::InternalGenerator, |i| i.code),
            Error::Command { .. } => ErrorCode::CommandFailed,
            Error::Environment(_) => ErrorCode::Environment,
            Error::Configuration(_) => ErrorCode::InvalidConfiguration,
            Error::RuntimeUnavailable(_) => ErrorCode::RuntimeUnavailable,
            Error::RuntimeChecksum(_) => ErrorCode::RuntimeChecksum,
            Error::OutputDrift(_) => ErrorCode::OutputDrift,
            Error::Plugin(_) => ErrorCode::PluginFailed,
        }
    }
}

/// The code of a worker failure, by the most fundamental kind of error it reports: a Lean error
/// explains any consequent failure, then project, adapter, and request errors.
fn worker_code(diagnostics: &[Diagnostic]) -> ErrorCode {
    let has = |k: DiagnosticKind| diagnostics.iter().any(|d| d.kind == k && d.severity == Severity::Error);
    if has(DiagnosticKind::Lean) {
        ErrorCode::LeanRejected
    } else if has(DiagnosticKind::Project) {
        ErrorCode::InvalidProject
    } else if has(DiagnosticKind::Adapter) {
        ErrorCode::AdapterRejected
    } else {
        ErrorCode::UnsatisfiableRequest
    }
}

fn render_diagnostic(f: &mut fmt::Formatter<'_>, d: &Diagnostic) -> fmt::Result {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Information => "info",
    };
    match (&d.file, &d.position) {
        (Some(file), Some(p)) => writeln!(f, "{file}:{}:{}: {severity}: {}", p.line, p.column, d.message)?,
        (Some(file), None) => writeln!(f, "{file}: {severity}: {}", d.message)?,
        _ => writeln!(f, "{severity}: {}", d.message)?,
    }
    if let Some(decl) = &d.declaration {
        writeln!(f, "  Lean declaration: {decl}")?;
    }
    Ok(())
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Error::Codegen { toolchain, bir_version, errors } = self {
            for e in errors {
                writeln!(f, "error[{}]: {e}\n", e.code())?;
            }
            return write!(f, "Lean toolchain: {toolchain}\nBridge IR: {bir_version}");
        }
        if let Error::Assurance(issues) = self {
            for (i, issue) in issues.iter().enumerate() {
                if i > 0 {
                    f.write_str("\n\n")?;
                }
                write!(f, "error[{}]: {}", issue.code, issue.message)?;
            }
            return Ok(());
        }
        write!(f, "error[{}]: ", self.code())?;
        match self {
            Error::Io { context, source } => write!(f, "{context}: {source}"),
            Error::Project(msg) => write!(f, "invalid Lean project: {msg}"),
            Error::UnsupportedToolchain { found, supported } => write!(
                f,
                "the Lean project pins toolchain {found:?}, which this bridge does not support.\nSupported toolchains: {}\nPin one of them in lean-toolchain.",
                supported.join(", ")
            ),
            Error::ToolchainNotInstalled { toolchain, expected_at } => write!(
                f,
                "Lean toolchain {toolchain} is not installed (expected at {}).\nInstall it explicitly with `elan toolchain install {toolchain}` or `lungo setup`; builds never install toolchains implicitly.",
                expected_at.display()
            ),
            Error::LeanElaboration { output } => {
                writeln!(f, "Lean elaboration failed\n")?;
                f.write_str(output.trim_end())
            }
            Error::Worker { toolchain, diagnostics } => {
                match worker_code(diagnostics) {
                    ErrorCode::LeanRejected => writeln!(f, "Lean elaboration failed\n")?,
                    ErrorCode::InvalidProject => writeln!(f, "invalid Lean project\n")?,
                    ErrorCode::AdapterRejected => {
                        writeln!(f, "the Lean {toolchain} adapter cannot process the compiler output\n")?
                    }
                    _ => writeln!(f, "the request cannot be satisfied\n")?,
                }
                for d in diagnostics {
                    render_diagnostic(f, d)?;
                }
                Ok(())
            }
            Error::WorkerCrashed { status, stderr, limits } => {
                write!(f, "the Lean worker process failed ({status}) without producing a response")?;
                if let Some(l) = limits {
                    write!(f, " (worker resource limits: {l})")?;
                }
                write!(f, "\n{stderr}")
            }
            Error::WorkerResourceLimit { limit, stderr } => {
                write!(f, "the Lean worker exceeded its {limit} limit and was stopped\n{stderr}")
            }
            Error::WorkerTimeout { seconds, stderr } => {
                write!(f, "the Lean worker did not finish within {seconds} s and was terminated\n{stderr}")
            }
            Error::Protocol(msg) => write!(f, "worker protocol error: {msg}"),
            Error::Codegen { .. } | Error::Assurance(_) => unreachable!("rendered above"),
            Error::Trust(violations) => {
                writeln!(f, "exported declarations violate the configured trust policy:")?;
                for v in violations {
                    writeln!(f, "  {v}")?;
                }
                Ok(())
            }
            Error::Command { program, status, output } => write!(f, "`{program}` failed ({status})\n{output}"),
            Error::Environment(msg)
            | Error::Configuration(msg)
            | Error::RuntimeUnavailable(msg)
            | Error::RuntimeChecksum(msg)
            | Error::OutputDrift(msg)
            | Error::Plugin(msg) => f.write_str(msg),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
