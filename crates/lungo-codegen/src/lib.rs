//! lungo's code generators: Bridge IR and the program's interface to source code.
//!
//! [`rust`] generates a Rust crate module on the `lungo` runtime; [`c`] generates the program as
//! C on the lungo runtime's C library, which the bindings of the other languages build on. The
//! pieces every backend shares (extern resolution, naming, the source writer, error codes) are
//! in [`core`].

pub mod c;
pub mod core;
pub mod plugin;
pub mod rust;

pub use crate::core::codes::ErrorCode;
pub use crate::core::externs::{Resolution, resolution_key};
pub use crate::core::names::{mangle, module_file_stem};
pub use rust::{Attribute, Shaping, selects};

use lungo_bir::{Body, Declaration, ValidationError};
use lungo_protocol::{DeclSource, PackageOrigin, SourceLocation};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;

/// Version of the generated-code format; part of the build fingerprint.
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub enum CodegenError {
    /// The program violates BIR invariants.
    Validation(Vec<ValidationError>),
    /// The compiler output contains a construct this backend does not implement.
    Adapter { declaration: String, message: String },
    /// An extern cannot be resolved or implemented; `code` identifies the condition.
    Extern { code: ErrorCode, message: String },
    /// A violated internal invariant of the generator.
    Internal(String),
    /// The application's configuration of the generated code cannot be applied.
    Configuration(String),
}

impl CodegenError {
    pub fn adapter(declaration: &str, message: impl Into<String>) -> Self {
        CodegenError::Adapter { declaration: declaration.to_owned(), message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        CodegenError::Internal(message.into())
    }

    pub fn external(code: ErrorCode, message: impl Into<String>) -> Self {
        CodegenError::Extern { code, message: message.into() }
    }

    /// The stable code of this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            CodegenError::Validation(_) => ErrorCode::InvalidBridgeIr,
            CodegenError::Adapter { .. } => ErrorCode::UnsupportedCompilerOutput,
            CodegenError::Extern { code, .. } => *code,
            CodegenError::Internal(_) => ErrorCode::InternalGenerator,
            CodegenError::Configuration(_) => ErrorCode::InvalidConfiguration,
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::Validation(errors) => {
                writeln!(f, "the Bridge IR produced by the Lean worker is invalid:")?;
                for e in errors.iter().take(50) {
                    writeln!(f, "  {e}")?;
                }
                if errors.len() > 50 {
                    writeln!(f, "  ... and {} more", errors.len() - 50)?;
                }
                Ok(())
            }
            CodegenError::Adapter { declaration, message } => write!(
                f,
                "lungo backend does not implement this compiler output: {message}\n\nLean declaration: {declaration}"
            ),
            CodegenError::Extern { message, .. } => f.write_str(message),
            CodegenError::Internal(message) => write!(f, "internal lungo code generator error: {message}"),
            CodegenError::Configuration(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CodegenError {}

/// A generated public item and the Lean declaration it represents.
#[derive(Debug, Clone, Serialize)]
pub struct NameRecord {
    pub lean_name: String,
    pub kind: String,
    pub rust_path: String,
    /// Whether sanitization changed the identifier.
    pub renamed: bool,
}

/// Human-readable locations of Lean declarations, for comments and metadata.
pub struct SourceIndex {
    entries: HashMap<String, (String, Option<(u32, u32)>)>,
}

impl SourceIndex {
    pub(crate) fn new(entries: &[lungo_protocol::SourceEntry], local_prefix: &str) -> Self {
        let entries = entries
            .iter()
            .map(|e| {
                (
                    e.name.clone(),
                    (
                        display_path(&e.source.location, local_prefix),
                        e.source.range.map(|r| (r.start.line, r.start.column)),
                    ),
                )
            })
            .collect();
        SourceIndex { entries }
    }

    /// `path:line:column` for `lean_name`.
    pub fn describe(&self, lean_name: &str) -> Option<String> {
        self.entries.get(lean_name).map(|(p, r)| match r {
            Some((l, c)) => format!("{p}:{l}:{c}"),
            None => p.clone(),
        })
    }
}

/// A source location without machine-specific absolute paths: local files relative to the Cargo
/// package, dependency and toolchain files under a symbolic root.
pub fn display_path(loc: &SourceLocation, local_prefix: &str) -> String {
    let join = |a: &str, b: &str| {
        if a.is_empty() { b.to_owned() } else { format!("{}/{b}", a.trim_end_matches('/')) }
    };
    match &loc.origin {
        PackageOrigin::Root => join(local_prefix, &loc.path),
        PackageOrigin::Path { dir } => join(&join(local_prefix, dir), &loc.path),
        PackageOrigin::Git { .. } => format!("<package:{}>/{}", loc.package, loc.path),
        PackageOrigin::Toolchain => format!("<lean>/{}", loc.path),
    }
}

pub(crate) fn describe_source(src: &Option<DeclSource>, local_prefix: &str) -> Option<String> {
    src.as_ref().map(|s| {
        let p = display_path(&s.location, local_prefix);
        match s.range {
            Some(r) => format!("{p}:{}:{}", r.start.line, r.start.column),
            None => p,
        }
    })
}

/// `v` as pretty-printed JSON, for the metadata files every backend writes.
pub(crate) fn to_json<T: Serialize>(v: &T) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("metadata serializes");
    s.push('\n');
    s
}

/// Whether `decl` is an extern declaration.
pub fn is_extern(decl: &Declaration) -> bool {
    matches!(decl.body, Body::Extern { .. })
}
