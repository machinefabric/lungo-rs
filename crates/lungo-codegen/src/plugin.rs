//! Language binding generators and the plugin protocol.
//!
//! A binding generator turns a program's [`Boundary`] and its C sources into a package in one
//! language. The built-in generators ([`builtin`]) run in-process; any other generator is a
//! plugin, a program named `lungo-gen-<language>`, which reads one [`GenerateRequest`] as JSON on
//! its standard input and writes one [`GenerateResponse`] as JSON on its standard output, as
//! `protoc` plugins do. Both kinds receive exactly the same request.

use crate::CodegenError;
use crate::c::boundary::Boundary;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The version of the request and response formats.
pub const PROTOCOL_VERSION: u32 = 1;

/// The program the package is generated for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramInfo {
    /// The program's name (the Lake package's, unless configured).
    pub name: String,
    pub lean_version: String,
    pub lean_githash: String,
    pub bir_version: u32,
    pub root_modules: Vec<String>,
}

/// A release artifact of the lungo runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    /// Lowercase hexadecimal SHA-256 of the artifact.
    pub sha256: String,
}

/// The lungo runtime the package links against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeInfo {
    /// The lungo version: the runtime, `lungo.h` and the support libraries of every language
    /// are released together under it.
    pub version: String,
    /// The C ABI version (`LUNGO_ABI_VERSION`).
    pub abi_version: u32,
    pub distribution: Distribution,
}

/// Where a generated package gets the runtime and its language's support library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Distribution {
    /// The lungo release: runtime archives by target triple, and the Apple XCFramework
    /// (`xcframework`); the support libraries from their registries (PyPI `lungo-py`, npm
    /// `lungo-ts`, the Go module and Swift package in their distribution repositories).
    Release { artifacts: BTreeMap<String, Artifact> },
    /// A local distribution (for an unreleased lungo): an absolute directory (with `/`
    /// separators, also on Windows) laid out as the release is (`runtime/` for the host, `wasm/`, `go/`, `python/`, `swift/`,
    /// `typescript/`), so packages are machine-specific.
    Local { dir: String },
}

/// What a generator is asked to generate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateRequest {
    pub protocol_version: u32,
    pub program: ProgramInfo,
    pub boundary: Boundary,
    /// The program as C, by path relative to the package (under `program/`), including
    /// `lungo.h`.
    pub program_files: BTreeMap<String, String>,
    pub runtime: RuntimeInfo,
    /// The generator's options (`--<language>_opt=key=value`, or the language's table of
    /// `lungo.toml`). A generator rejects keys it does not know.
    pub options: BTreeMap<String, String>,
}

/// What a generator generated: the package's files, or why it could not.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateResponse {
    /// The package's files, by `/`-separated path relative to the package directory.
    pub files: BTreeMap<String, String>,
    /// Why generation failed; the files are then ignored.
    pub errors: Vec<String>,
}

/// A binding generator.
pub trait Generator: Sync {
    /// The language, as named on the command line (`--<language>_out`).
    fn language(&self) -> &'static str;

    /// The package's files, by `/`-separated path relative to the output directory.
    fn generate(&self, request: &GenerateRequest) -> Result<BTreeMap<String, String>, Vec<CodegenError>>;
}

/// The built-in generators.
pub fn builtins() -> &'static [&'static dyn Generator] {
    &[
        &crate::c::api::CGenerator,
        &crate::go::GoGenerator,
        &crate::python::PythonGenerator,
        &crate::swift::SwiftGenerator,
        &crate::ts::TsGenerator,
    ]
}

/// The built-in generator of `language`.
pub fn builtin(language: &str) -> Option<&'static dyn Generator> {
    builtins().iter().copied().find(|g| g.language() == language)
}

/// The options of `request` for keys `known`, rejecting others.
pub fn options<'r>(
    request: &'r GenerateRequest,
    language: &str,
    known: &[&str],
) -> Result<BTreeMap<&'r str, &'r str>, CodegenError> {
    let mut out = BTreeMap::new();
    for (k, v) in &request.options {
        if !known.contains(&k.as_str()) {
            return Err(CodegenError::Configuration(format!(
                "the {language} generator has no option `{k}` (options: {})",
                known.join(", ")
            )));
        }
        out.insert(k.as_str(), v.as_str());
    }
    Ok(out)
}
