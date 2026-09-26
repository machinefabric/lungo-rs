//! The versioned protocol spoken between `patina-build` and the toolchain-specific worker.
//!
//! A frame is the magic `PTNF`, a one-byte [`FrameKind`], the protocol version as a
//! little-endian `u32`, the payload length as a little-endian `u64`, and a CBOR payload. The
//! protocol version and the Bridge IR version are versioned independently.

use patina_bir::{ExternEntry, IrType, Param, Program};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 1;
const MAGIC: &[u8; 4] = b"PTNF";
const HEADER_SIZE: usize = 17;
/// Nesting permitted in a response: bounded by the control-flow nesting of compiled code.
const RECURSION_LIMIT: usize = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Request = 1,
    Response = 2,
}

#[derive(Debug)]
pub enum FrameError {
    Truncated,
    InvalidMagic,
    UnexpectedKind { expected: FrameKind, found: u8 },
    ProtocolVersion(u32),
    InvalidLength { declared: u64, actual: usize },
    Decode(String),
    Encode(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated patina protocol frame"),
            Self::InvalidMagic => f.write_str("invalid patina protocol frame magic"),
            Self::UnexpectedKind { expected, found } => {
                write!(f, "expected a {expected:?} frame, found frame kind {found}")
            }
            Self::ProtocolVersion(v) => {
                write!(f, "worker speaks protocol version {v}, but patina implements {PROTOCOL_VERSION}")
            }
            Self::InvalidLength { declared, actual } => {
                write!(f, "frame declares {declared} payload bytes but carries {actual}")
            }
            Self::Decode(e) => write!(f, "invalid protocol payload: {e}"),
            Self::Encode(e) => write!(f, "cannot encode protocol payload: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

pub fn encode_frame<T: Serialize>(kind: FrameKind, value: &T) -> Result<Vec<u8>, FrameError> {
    let mut payload = Vec::new();
    ciborium::ser::into_writer(value, &mut payload).map_err(|e| FrameError::Encode(e.to_string()))?;
    let mut frame = Vec::with_capacity(HEADER_SIZE + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.push(kind as u8);
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: serde::de::DeserializeOwned>(kind: FrameKind, frame: &[u8]) -> Result<T, FrameError> {
    let header = frame.get(..HEADER_SIZE).ok_or(FrameError::Truncated)?;
    if &header[..4] != MAGIC {
        return Err(FrameError::InvalidMagic);
    }
    if header[4] != kind as u8 {
        return Err(FrameError::UnexpectedKind { expected: kind, found: header[4] });
    }
    let version = u32::from_le_bytes(header[5..9].try_into().expect("four bytes"));
    if version != PROTOCOL_VERSION {
        return Err(FrameError::ProtocolVersion(version));
    }
    let declared = u64::from_le_bytes(header[9..17].try_into().expect("eight bytes"));
    let payload = &frame[HEADER_SIZE..];
    if declared != payload.len() as u64 {
        return Err(FrameError::InvalidLength { declared, actual: payload.len() });
    }
    ciborium::de::from_reader_with_recursion_limit(payload, RECURSION_LIMIT)
        .map_err(|e| FrameError::Decode(e.to_string()))
}

// ---------------------------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    pub bridge_version: String,
    /// Absolute path of the Lake project root.
    pub project_root: String,
    pub root_modules: Vec<String>,
    pub export_policy: ExportPolicy,
    pub host_triple: String,
    pub target: Target,
    /// Lean options in effect while the worker runs Lean metaprograms over the environment.
    pub compiler_options: Vec<CompilerOption>,
    pub hermetic: bool,
    pub diagnostics: DiagnosticOptions,
    /// `@[export]` symbols of Lean definitions the target runtime calls; they are compiled into
    /// every program.
    pub runtime_exports: Vec<String>,
}

/// Which declarations receive public Rust facades. This never restricts what is compiled.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPolicy {
    pub declarations: Vec<String>,
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub triple: String,
    pub pointer_width: u32,
    pub endian: Endian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Endian {
    Little,
    Big,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerOption {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticOptions {
    /// Maximum number of error diagnostics to report; zero reports all.
    pub max_errors: u32,
}

// ---------------------------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol_version: u32,
    pub toolchain: WorkerToolchain,
    pub diagnostics: Vec<Diagnostic>,
    pub outcome: Outcome,
}

/// The identity of the Lean toolchain the worker was compiled against.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerToolchain {
    pub lean_version: String,
    pub lean_githash: String,
    pub adapter_version: u32,
    pub bir_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Success(Box<Success>),
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Success {
    /// Native symbols of Lean's own C backend for the program.
    pub oracle: OracleSymbols,
    pub module_graph: Vec<ModuleNode>,
    /// Project-relative, `/`-separated paths of every editable input of the build.
    pub input_files: Vec<String>,
    pub bir: Program,
    pub extern_requirements: Vec<ExternRequirement>,
    pub source_metadata: Vec<SourceEntry>,
    pub interface: Interface,
    /// Lean's `main`, when a root module defines it.
    pub entry_point: Option<EntryPoint>,
    /// The Lean definitions providing the requested runtime exports.
    pub runtime_exports: Vec<RuntimeExport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeExport {
    pub symbol: String,
    pub declaration: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPoint {
    pub declaration: String,
    /// `main : List String → IO _`.
    pub takes_args: bool,
    /// `main : … → IO UInt32`.
    pub returns_exit_code: bool,
}

/// The C symbols Lean's native backend gives the program's exports, a few runtime helpers, and
/// the root modules' initializers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleSymbols {
    pub symbols: Vec<NativeSymbol>,
    pub module_initializers: Vec<ModuleInitializer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSymbol {
    pub name: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleInitializer {
    pub module: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleNode {
    pub name: String,
    pub imports: Vec<String>,
    pub source: Option<SourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLocation {
    pub package: String,
    pub origin: PackageOrigin,
    /// `/`-separated path relative to the owning package's directory.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageOrigin {
    Root,
    Path { dir: String },
    Git { url: String, rev: String },
    Toolchain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclSource {
    pub location: SourceLocation,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// One-based line and column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEntry {
    pub name: String,
    pub source: DeclSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternRequirement {
    pub declaration: String,
    pub entry: ExternEntry,
    pub lean_type: Option<String>,
    pub params: Vec<Param>,
    pub result: IrType,
    pub source: Option<DeclSource>,
    /// The Rust-facing signature of an application-provided implementation, when the extern's
    /// Lean type determines one.
    pub facade: Option<FacadeSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacadeSignature {
    pub type_params: Vec<String>,
    /// One entry per parameter of the compiler declaration.
    pub params: Vec<FacadeParam>,
    pub result: FacadeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub exports: Vec<Export>,
    pub types: Vec<TypeDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub name: String,
    pub module: String,
    pub lean_type: String,
    pub type_params: Vec<String>,
    /// One entry per parameter of the compiler declaration.
    pub params: Vec<FacadeParam>,
    pub result: FacadeType,
    pub source: Option<DeclSource>,
    pub trust: Trust,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FacadeParam {
    Value {
        name: String,
        ty: FacadeType,
    },
    /// Erased by the compiler (a type, a proof) or the `IO` world token.
    Erased,
}

/// The Rust-facing shape of a Lean type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FacadeType {
    Nat,
    Int,
    Bool,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Usize,
    Int8,
    Int16,
    Int32,
    Int64,
    Isize,
    Float,
    Float32,
    Char,
    String,
    Unit,
    ByteArray,
    FloatArray,
    Option(Box<FacadeType>),
    List(Box<FacadeType>),
    Array(Box<FacadeType>),
    Prod(Box<FacadeType>, Box<FacadeType>),
    Except {
        error: Box<FacadeType>,
        value: Box<FacadeType>,
    },
    Io(Box<FacadeType>),
    Eio {
        error: Box<FacadeType>,
        value: Box<FacadeType>,
    },
    BaseIo(Box<FacadeType>),
    Function {
        params: Vec<FacadeType>,
        result: Box<FacadeType>,
    },
    /// A type parameter of the enclosing declaration or type.
    Param(u32),
    Inductive {
        name: String,
        args: Vec<FacadeType>,
    },
    /// A value exposed only as an opaque Lean value.
    Opaque {
        head: Option<String>,
        lean_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    /// Axioms the definition depends on, excluding `sorryAx`.
    pub axioms: Vec<String>,
    pub depends_on_sorry: bool,
    /// `unsafe` constants in the executable closure (the export itself, and those outside the
    /// Lean toolchain).
    pub unsafe_dependencies: Vec<String>,
    /// `partial` constants in the executable closure, with the same scope.
    pub partial_dependencies: Vec<String>,
    /// External symbols (or adhoc extern declarations) the executable closure calls.
    pub extern_dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDecl {
    pub name: String,
    pub params: Vec<String>,
    /// The runtime representation of values of this type.
    pub repr: IrType,
    /// Set for single-constructor types with one relevant field, which are represented by
    /// that field.
    pub trivial: Option<TrivialStructure>,
    pub structure: bool,
    pub ctors: Vec<CtorDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrivialStructure {
    pub ctor: String,
    pub field: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CtorDecl {
    pub name: String,
    pub tag: u32,
    pub size: u32,
    pub usize: u32,
    pub ssize: u32,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDecl {
    pub name: String,
    pub ty: FacadeType,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldKind {
    Object(u32),
    Usize(u32),
    Scalar { size: u32, offset: u32, ty: IrType },
    Erased,
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub file: Option<String>,
    pub position: Option<Position>,
    pub declaration: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Information,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// The Lean frontend, kernel, or compiler rejected the program.
    Lean,
    /// The project layout, manifest, or toolchain is invalid.
    Project,
    /// The request cannot be satisfied.
    Request,
    /// Lean produced compiler output the adapter does not accept.
    Adapter,
}

impl Response {
    pub fn from_frame(frame: &[u8]) -> Result<Self, FrameError> {
        let response: Response = decode_frame(FrameKind::Response, frame)?;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(FrameError::ProtocolVersion(response.protocol_version));
        }
        Ok(response)
    }
}

impl Request {
    pub fn to_frame(&self) -> Result<Vec<u8>, FrameError> {
        encode_frame(FrameKind::Request, self)
    }
}
