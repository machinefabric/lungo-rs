use lean2rust_protocol::{Diagnostic, DiagnosticKind, Severity};
use std::fmt;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// Every way generation can fail. Lean errors, project errors, and backend errors are kept
/// distinct so that invalid Lean is never reported as a backend failure or vice versa.
#[derive(Debug)]
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
    /// Code generation failed.
    Codegen {
        toolchain: String,
        bir_version: u32,
        messages: Vec<String>,
    },
    /// An export violates the configured trust policy.
    Trust(Vec<String>),
    /// A command the bridge runs failed.
    Command {
        program: String,
        status: String,
        output: String,
    },
    /// Environment the bridge requires is missing.
    Environment(String),
}

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io { context: context.into(), source }
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
        match self {
            Error::Io { context, source } => write!(f, "lean2rust: {context}: {source}"),
            Error::Project(msg) => write!(f, "lean2rust: invalid Lean project: {msg}"),
            Error::UnsupportedToolchain { found, supported } => write!(
                f,
                "lean2rust: the Lean project pins toolchain {found:?}, which this bridge does not support.\nSupported toolchains: {}\nPin one of them in lean-toolchain.",
                supported.join(", ")
            ),
            Error::ToolchainNotInstalled { toolchain, expected_at } => write!(
                f,
                "lean2rust: Lean toolchain {toolchain} is not installed (expected at {}).\nInstall it explicitly with `elan toolchain install {toolchain}` or `cargo lean2rust setup`; builds never install toolchains implicitly.",
                expected_at.display()
            ),
            Error::LeanElaboration { output } => {
                writeln!(f, "lean2rust: Lean elaboration failed\n")?;
                f.write_str(output.trim_end())
            }
            Error::Worker { toolchain, diagnostics } => {
                let lean = diagnostics.iter().any(|d| d.kind == DiagnosticKind::Lean);
                let adapter = diagnostics.iter().any(|d| d.kind == DiagnosticKind::Adapter);
                if lean {
                    writeln!(f, "lean2rust: Lean elaboration failed\n")?;
                } else if adapter {
                    writeln!(f, "lean2rust: the Lean {toolchain} adapter cannot process the compiler output\n")?;
                } else {
                    writeln!(f, "lean2rust: the request cannot be satisfied\n")?;
                }
                for d in diagnostics {
                    render_diagnostic(f, d)?;
                }
                Ok(())
            }
            Error::WorkerCrashed { status, stderr, limits } => {
                write!(f, "lean2rust: the Lean worker process failed ({status}) without producing a response")?;
                if let Some(l) = limits {
                    write!(f, " (worker resource limits: {l})")?;
                }
                write!(f, "\n{stderr}")
            }
            Error::WorkerResourceLimit { limit, stderr } => {
                write!(f, "lean2rust: the Lean worker exceeded its {limit} limit and was stopped\n{stderr}")
            }
            Error::WorkerTimeout { seconds, stderr } => {
                write!(f, "lean2rust: the Lean worker did not finish within {seconds} s and was terminated\n{stderr}")
            }
            Error::Protocol(msg) => write!(f, "lean2rust: worker protocol error: {msg}"),
            Error::Codegen { toolchain, bir_version, messages } => {
                for m in messages {
                    writeln!(f, "error: {m}\n")?;
                }
                write!(f, "Lean toolchain: {toolchain}\nBridge IR: {bir_version}")
            }
            Error::Trust(violations) => {
                writeln!(f, "lean2rust: exported declarations violate the configured trust policy:")?;
                for v in violations {
                    writeln!(f, "  {v}")?;
                }
                Ok(())
            }
            Error::Command { program, status, output } => {
                write!(f, "lean2rust: `{program}` failed ({status})\n{output}")
            }
            Error::Environment(msg) => write!(f, "lean2rust: {msg}"),
        }
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
