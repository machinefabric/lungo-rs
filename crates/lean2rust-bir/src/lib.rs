//! The versioned compiler boundary emitted by the toolchain-specific Lean worker.

use serde::Deserialize;
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 1;
pub const BIR_VERSION: u32 = 1;
const MAGIC: &[u8; 4] = b"L2RB";

#[derive(Debug)]
pub enum FrameError {
    Truncated,
    InvalidMagic,
    InvalidLength { declared: usize, actual: usize },
    InvalidPayload(serde_json::Error),
    ProtocolVersion(u32),
    BirVersion(u32),
    InvalidModule,
    InvalidDeclarationOrder,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated lean2rust BIR frame"),
            Self::InvalidMagic => f.write_str("invalid lean2rust BIR frame magic"),
            Self::InvalidLength { declared, actual } => write!(
                f,
                "lean2rust BIR frame declares {declared} payload bytes but contains {actual}"
            ),
            Self::InvalidPayload(error) => write!(f, "invalid lean2rust BIR payload: {error}"),
            Self::ProtocolVersion(version) => write!(
                f,
                "lean2rust worker protocol version {version} is incompatible with {PROTOCOL_VERSION}"
            ),
            Self::BirVersion(version) => write!(
                f,
                "lean2rust BIR version {version} is incompatible with {BIR_VERSION}"
            ),
            Self::InvalidModule => f.write_str("lean2rust BIR module name is empty"),
            Self::InvalidDeclarationOrder => {
                f.write_str("lean2rust BIR declarations are unsorted or contain duplicate names")
            }
        }
    }
}

impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidPayload(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Module {
    pub protocol_version: u32,
    pub bir_version: u32,
    pub module: String,
    pub declarations: Vec<Declaration>,
}

impl Module {
    pub fn from_frame(frame: &[u8]) -> Result<Self, FrameError> {
        let Some(header) = frame.get(..8) else {
            return Err(FrameError::Truncated);
        };
        if &header[..4] != MAGIC {
            return Err(FrameError::InvalidMagic);
        }
        let declared =
            u32::from_le_bytes(header[4..8].try_into().expect("four-byte header")) as usize;
        let actual = frame.len() - 8;
        if declared != actual {
            return Err(FrameError::InvalidLength { declared, actual });
        }
        let module: Self =
            serde_json::from_slice(&frame[8..]).map_err(FrameError::InvalidPayload)?;
        if module.protocol_version != PROTOCOL_VERSION {
            return Err(FrameError::ProtocolVersion(module.protocol_version));
        }
        if module.bir_version != BIR_VERSION {
            return Err(FrameError::BirVersion(module.bir_version));
        }
        if module.module.is_empty() {
            return Err(FrameError::InvalidModule);
        }
        if module
            .declarations
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(FrameError::InvalidDeclarationOrder);
        }
        Ok(module)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Declaration {
    pub name: String,
    pub params: Vec<Parameter>,
    pub result_type: Type,
    pub safe: bool,
    pub recursive: bool,
    pub value: DeclarationValue,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: Type,
    pub borrow: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeclarationValue {
    Code { body: Code },
    Extern { entries: Vec<ExternEntry> },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternEntry {
    Adhoc { backend: String },
    Inline { backend: String, pattern: String },
    Standard { backend: String, symbol: String },
    Opaque,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Type {
    Name(String),
    Expression(Box<TypeExpression>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum TypeExpression {
    BoundVariable {
        index: String,
    },
    FreeVariable {
        id: String,
    },
    Metavariable {
        id: String,
    },
    Sort {
        level: Level,
    },
    Constant {
        name: String,
        levels: Vec<Level>,
    },
    Application {
        function: Type,
        argument: Type,
    },
    Lambda {
        name: String,
        domain: Type,
        body: Type,
        binder: Binder,
    },
    Forall {
        name: String,
        domain: Type,
        body: Type,
        binder: Binder,
    },
    Let {
        name: String,
        #[serde(rename = "type")]
        ty: Type,
        value: Type,
        body: Type,
        nondependent: bool,
    },
    NaturalLiteral {
        value: String,
    },
    StringLiteral {
        value: String,
    },
    Projection {
        #[serde(rename = "typeName")]
        type_name: String,
        index: String,
        value: Type,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binder {
    Explicit,
    Implicit,
    StrictImplicit,
    Instance,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Level {
    Zero,
    Successor { value: Box<Level> },
    Maximum { left: Box<Level>, right: Box<Level> },
    DependentMaximum { left: Box<Level>, right: Box<Level> },
    Parameter { name: String },
    Metavariable { id: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Argument {
    Erased,
    Var { id: String },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstructorInfo {
    pub name: String,
    pub tag: String,
    #[serde(rename = "objectFields")]
    pub object_fields: String,
    #[serde(rename = "usizeFields")]
    pub usize_fields: String,
    #[serde(rename = "scalarBytes")]
    pub scalar_bytes: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Literal {
    Nat { value: String },
    String { value: String },
    Uint8 { value: String },
    Uint16 { value: String },
    Uint32 { value: String },
    Uint64 { value: String },
    Usize { value: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum LetValue {
    Literal {
        literal: Literal,
    },
    Erased,
    ApplyClosure {
        function: String,
        args: Vec<Argument>,
    },
    Constructor {
        info: ConstructorInfo,
        args: Vec<Argument>,
    },
    ObjectProjection {
        index: String,
        value: String,
    },
    UsizeProjection {
        index: String,
        value: String,
    },
    ScalarProjection {
        bytes: String,
        offset: String,
        value: String,
    },
    Call {
        function: String,
        args: Vec<Argument>,
    },
    PartialApplication {
        function: String,
        args: Vec<Argument>,
    },
    Reset {
        fields: String,
        value: String,
    },
    Reuse {
        value: String,
        info: ConstructorInfo,
        #[serde(rename = "updateHeader")]
        update_header: bool,
        args: Vec<Argument>,
    },
    Box {
        #[serde(rename = "type")]
        ty: Type,
        value: String,
    },
    Unbox {
        value: String,
    },
    IsShared {
        value: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Alternative {
    Constructor { info: ConstructorInfo, body: Code },
    Default { body: Code },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Code {
    Let {
        id: String,
        #[serde(rename = "type")]
        ty: Type,
        value: LetValue,
        next: Box<Code>,
    },
    Join {
        id: String,
        params: Vec<Parameter>,
        #[serde(rename = "type")]
        ty: Type,
        body: Box<Code>,
        next: Box<Code>,
    },
    Jump {
        target: String,
        args: Vec<Argument>,
    },
    Cases {
        #[serde(rename = "typeName")]
        type_name: String,
        #[serde(rename = "resultType")]
        result_type: Type,
        discriminator: String,
        alternatives: Vec<Alternative>,
    },
    Return {
        value: String,
    },
    Unreachable {
        #[serde(rename = "type")]
        ty: Type,
    },
    ObjectSet {
        value: String,
        index: String,
        field: Argument,
        next: Box<Code>,
    },
    UsizeSet {
        value: String,
        index: String,
        field: String,
        next: Box<Code>,
    },
    ScalarSet {
        value: String,
        index: String,
        offset: String,
        field: String,
        #[serde(rename = "type")]
        ty: Type,
        next: Box<Code>,
    },
    SetTag {
        value: String,
        tag: String,
        next: Box<Code>,
    },
    Increment {
        value: String,
        count: String,
        check: bool,
        persistent: bool,
        next: Box<Code>,
    },
    Decrement {
        value: String,
        count: String,
        check: bool,
        persistent: bool,
        objects: Option<String>,
        next: Box<Code>,
    },
    Delete {
        value: String,
        next: Box<Code>,
    },
}
