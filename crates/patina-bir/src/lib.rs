//! Bridge IR (BIR): patina's versioned compiler IR.
//!
//! BIR represents Lean's final compiler representation — the lowering of final impure LCNF —
//! instruction for instruction. It is produced by the toolchain-specific worker and consumed by
//! the Rust backend. It is not a restricted Lean fragment: every construct the Lean compiler
//! hands to a backend has a representation here.
//!
//! Straight-line code is flattened into [`Block`]s so that consumers recurse only on genuinely
//! nested control flow (join points and case alternatives).

mod pretty;
mod validate;

pub use pretty::{pretty_declaration, pretty_program};
pub use validate::{ValidationError, validate};

use serde::{Deserialize, Serialize};

/// Version of the BIR data model. Independent of the worker protocol version.
pub const BIR_VERSION: u32 = 3;

/// A variable identifier, unique within one declaration.
pub type VarId = u32;
/// A join point identifier, unique within one declaration.
pub type JoinId = u32;

/// The complete executable closure of the requested Lean program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub bir_version: u32,
    /// Every loaded module, in initialization order (dependencies first).
    pub modules: Vec<Module>,
    /// Every reachable compiler declaration, sorted by name.
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub name: String,
    pub imports: Vec<String>,
    /// Initialization actions of this module, in declaration order.
    pub initializers: Vec<Initializer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Initializer {
    /// An `IO Unit` action run once, for its effects, during initialization.
    Io(String),
    /// `decl` holds the value produced by running the `IO` action `init_fn` during initialization.
    Value { decl: String, init_fn: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// Canonical (escaped) fully qualified Lean name.
    pub name: String,
    /// The module whose compiler output contains this declaration.
    pub module: String,
    /// The source-level constant this declaration was compiled from, if any.
    pub origin: Option<String>,
    pub params: Vec<Param>,
    pub result: IrType,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Body {
    Function {
        block: Block,
    },
    /// An implementation supplied outside Lean: a runtime primitive or host function.
    Extern {
        entries: Vec<ExternEntry>,
        /// The entry Lean's own C backend selects.
        selected: ExternEntry,
        /// The Lean definition that provides the selected symbol with `@[export]`, when the
        /// symbol is implemented by compiled Lean code rather than by the runtime.
        exported_by: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternEntry {
    /// Implemented under the declaration's own (mangled) name.
    Adhoc {
        backend: String,
    },
    /// A backend-specific code pattern.
    Inline {
        backend: String,
        pattern: String,
    },
    /// A named external symbol.
    Standard {
        backend: String,
        symbol: String,
    },
    Opaque,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub var: VarId,
    pub ty: IrType,
    /// Borrowed parameters are not consumed by the callee.
    pub borrow: bool,
}

/// Runtime representation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IrType {
    Float,
    Float32,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Usize,
    /// Types, propositions and proofs: represented by the scalar `box(0)`.
    Erased,
    /// A pointer to a heap object.
    Object,
    /// A heap object or a tagged scalar.
    Tobject,
    /// A tagged scalar.
    Tagged,
    /// The `IO` world token; it has no runtime representation.
    Void,
}

impl IrType {
    /// Unboxed scalar types.
    pub fn is_scalar(self) -> bool {
        matches!(
            self,
            IrType::Float
                | IrType::Float32
                | IrType::Uint8
                | IrType::Uint16
                | IrType::Uint32
                | IrType::Uint64
                | IrType::Usize
        )
    }

    /// Types represented by an object pointer or tagged scalar.
    pub fn is_object(self) -> bool {
        matches!(self, IrType::Object | IrType::Tobject | IrType::Tagged | IrType::Erased | IrType::Void)
    }

    pub fn name(self) -> &'static str {
        match self {
            IrType::Float => "float",
            IrType::Float32 => "float32",
            IrType::Uint8 => "u8",
            IrType::Uint16 => "u16",
            IrType::Uint32 => "u32",
            IrType::Uint64 => "u64",
            IrType::Usize => "usize",
            IrType::Erased => "◾",
            IrType::Object => "obj",
            IrType::Tobject => "tobj",
            IrType::Tagged => "tagged",
            IrType::Void => "void",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Stmt {
    Let {
        var: VarId,
        ty: IrType,
        expr: Expr,
    },
    /// A join point, in scope for the remainder of the enclosing block.
    Join {
        id: JoinId,
        params: Vec<Param>,
        body: Block,
    },
    /// Store an object field of an exclusive constructor object.
    Set {
        var: VarId,
        index: u32,
        arg: Arg,
    },
    SetTag {
        var: VarId,
        tag: u32,
    },
    /// Store a `usize` field (index counted in words, after the object fields).
    Uset {
        var: VarId,
        index: u32,
        value: VarId,
    },
    /// Store a scalar at `index` words plus `offset` bytes.
    Sset {
        var: VarId,
        index: u32,
        offset: u32,
        value: VarId,
        ty: IrType,
    },
    Inc {
        var: VarId,
        count: u32,
        /// Whether the value may be a tagged scalar.
        checked: bool,
        /// Statically known to be persistent: no reference counting is performed.
        persistent: bool,
    },
    Dec {
        var: VarId,
        count: u32,
        checked: bool,
        persistent: bool,
    },
    /// Free an object without touching its fields.
    Del {
        var: VarId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Terminator {
    Case { type_name: String, var: VarId, var_ty: IrType, alts: Vec<Alt> },
    Ret { arg: Arg },
    Jmp { id: JoinId, args: Vec<Arg> },
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Alt {
    Ctor { info: CtorInfo, body: Block },
    Default { body: Block },
}

impl Alt {
    pub fn body(&self) -> &Block {
        match self {
            Alt::Ctor { body, .. } | Alt::Default { body } => body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    /// Allocate a constructor object (or a tagged scalar when it has no fields).
    Ctor {
        info: CtorInfo,
        args: Vec<Arg>,
    },
    /// Prepare an exclusive object for reuse, or release a shared one.
    Reset {
        fields: u32,
        var: VarId,
    },
    /// Reuse a reset object's memory for a constructor.
    Reuse {
        var: VarId,
        info: CtorInfo,
        update_header: bool,
        args: Vec<Arg>,
    },
    /// Borrow an object field.
    Proj {
        index: u32,
        var: VarId,
    },
    Uproj {
        index: u32,
        var: VarId,
    },
    Sproj {
        fields: u32,
        offset: u32,
        var: VarId,
    },
    /// Full application of a declaration.
    Fap {
        function: String,
        args: Vec<Arg>,
    },
    /// Partial application creating a closure.
    Pap {
        function: String,
        args: Vec<Arg>,
    },
    /// Application of a closure.
    Ap {
        var: VarId,
        args: Vec<Arg>,
    },
    Box {
        ty: IrType,
        var: VarId,
    },
    Unbox {
        var: VarId,
    },
    Lit(Literal),
    /// `1 : u8` iff the object is shared.
    IsShared {
        var: VarId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Literal {
    /// A natural number literal in canonical decimal notation. Its representation follows the
    /// type of the binding variable.
    Num(String),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Arg {
    Var(VarId),
    /// An erased value, represented by `box(0)`.
    Erased,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CtorInfo {
    pub name: String,
    pub tag: u32,
    /// Number of object fields.
    pub size: u32,
    /// Number of `usize` fields.
    pub usize: u32,
    /// Bytes of other scalar fields.
    pub ssize: u32,
}

impl CtorInfo {
    /// Constructors without fields are represented as tagged scalars.
    pub fn is_scalar(&self) -> bool {
        self.size == 0 && self.usize == 0 && self.ssize == 0
    }
}

impl Program {
    pub fn declaration(&self, name: &str) -> Option<&Declaration> {
        self.declarations.binary_search_by(|d| d.name.as_str().cmp(name)).ok().map(|i| &self.declarations[i])
    }
}
