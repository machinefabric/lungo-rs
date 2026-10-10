//! The program's boundary: how the language bindings call exported Lean functions and how
//! Lean calls the host's implementations of externs.
//!
//! [`generate`] computes the [`Boundary`], the model every binding generator consumes (the
//! program's types as a wire-format type table, its callable functions, the capabilities the
//! host provides, the entry point), and emits its C: the type table, one call entry point per
//! exported function, and one adapter per operation of a capability.
//!
//! A capability is either a group of `@[extern]` operations the host implements synchronously,
//! called through these adapters, or an async capability: the constructors of an operation type
//! an async program asks the host to perform, answered through the handler the caller passes to
//! each async export (see [`lungo_runtime::wire::program`]).

use super::program::Emitter;
use super::syntax::{c_type, comment, string};
use crate::core::externs::{ExternPlan, Resolution, implementation_params};
use crate::core::interface::reachable_types;
use crate::core::names::mangle;
use crate::core::writer::Writer;
use crate::{CodegenError, ErrorCode};
use lungo_bir::{Declaration, IrType, Program};
use lungo_protocol::{
    CapabilityKind, DeclSource, Export, ExternRequirement, FacadeParam, FacadeType, FieldKind, Success, TypeDecl,
};
use lungo_runtime::wire::{self, Returns, Signature, Type};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The C symbol of the adapter through which the program calls the host's implementation of the
/// extern declaration `decl_name`.
pub fn host_adapter_symbol(prefix: &str, decl_name: &str) -> String {
    format!("{prefix}host_{}", mangle(decl_name))
}

/// The prototype of the host adapter of the extern declaration `decl`: the representation of
/// the declaration's implementation parameters and result.
pub fn host_adapter_prototype(prefix: &str, decl: &Declaration) -> String {
    let params: Vec<String> =
        implementation_params(decl).iter().map(|p| format!("{} x_{}", c_type(p.ty), p.var)).collect();
    let params = if params.is_empty() { "void".to_owned() } else { params.join(", ") };
    format!("{} {}({params})", c_type(decl.result), host_adapter_symbol(prefix, &decl.name))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    /// The Lean binder name (may be empty).
    pub name: String,
    pub ty: Type,
}

/// An exported Lean function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub lean_name: String,
    pub module: String,
    pub lean_type: String,
    /// The C entry point: `int32_t symbol(const uint8_t *input, size_t len, lungo_buffer *out)`.
    pub symbol: String,
    /// Type parameters, instantiated by the caller's type arguments (`Type::Param`).
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    /// What the function returns; `Returns::Async` for an async program, whose call returns its
    /// first step.
    pub returns: Returns,
    pub source: Option<DeclSource>,
}

/// An operation of a capability: an `@[extern]` declaration the host implements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    /// Its index for `<prefix>set_host_extern`.
    pub index: usize,
    /// The extern's key: its C symbol, or the Lean declaration for other extern forms.
    pub key: String,
    pub declaration: String,
    pub lean_type: Option<String>,
    /// Type parameters of a polymorphic extern are passed as opaque values.
    pub params: Vec<Param>,
    pub returns: Returns,
}

/// A capability the host provides by implementing its operations, each called synchronously.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    /// The namespaced identifier, such as `time.clock`.
    pub id: String,
    /// The Lean declaration registered as the capability.
    pub lean_name: String,
    pub operations: Vec<Operation>,
}

/// An operation of an async capability: a constructor of its operation type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsyncOperation {
    /// The constructor's index in the operation type.
    pub ctor: u32,
    /// The constructor's Lean name.
    pub lean_name: String,
    /// The type of the host's answer.
    pub answer: Type,
}

/// A capability whose operations an async program asks the host to perform; the caller of an
/// async export passes a handler for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsyncCapability {
    pub id: String,
    /// The Lean declaration registered as the capability (its `Lungo.Async.Interface` instance).
    pub lean_name: String,
    /// The operation type: its index in the type table.
    pub op_type: u32,
    pub operations: Vec<AsyncOperation>,
}

impl Function {
    /// The signature `lungo_invoke` and the support libraries check calls against.
    pub fn signature(&self) -> Signature {
        Signature {
            type_params: self.type_params.len() as u32,
            params: self.params.iter().map(|p| p.ty.clone()).collect(),
            returns: self.returns.clone(),
        }
    }
}

impl Operation {
    /// The signature of the host's implementation.
    pub fn signature(&self) -> Signature {
        Signature {
            type_params: 0,
            params: self.params.iter().map(|p| p.ty.clone()).collect(),
            returns: self.returns.clone(),
        }
    }
}

/// A type of the table, with the Lean names generators need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedType {
    pub lean_name: String,
    pub params: Vec<String>,
    pub structure: bool,
    /// Values cross as handles; the table entry has no constructors (see
    /// `lungo_runtime::wire::TypeDecl::opaque`).
    pub opaque: bool,
    /// The type's layout fingerprint (`crate::core::fingerprint`): what a package using this
    /// type from another package checks it agrees on.
    pub fingerprint: String,
}

/// The program's boundary, the input of every binding generator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    /// The program's C identifier.
    pub id: String,
    /// The symbol prefix of the program (`<id>__`).
    pub prefix: String,
    /// The program's types; `Type::Inductive` indexes them.
    pub table: wire::TypeTable,
    /// The Lean names of the table's types, by index.
    pub types: Vec<NamedType>,
    pub functions: Vec<Function>,
    /// The capabilities whose operations the host implements, by identifier.
    pub capabilities: Vec<Capability>,
    /// The async capabilities the program's async exports ask operations of, by identifier.
    pub async_capabilities: Vec<AsyncCapability>,
    /// `int32_t <prefix>run_main(size_t argc, const char *const *argv)`, when a root module
    /// defines `main`.
    pub run_main: Option<String>,
    /// `void <prefix>set_host_extern(size_t index, uint64_t callback)`: registers the host's
    /// implementation of operation `index`.
    pub set_host_extern: String,
    /// `const lungo_types *<prefix>types(void)`.
    pub types_symbol: String,
    /// `void <prefix>initialize(void)`.
    pub initialize: String,
}

/// Translates facade types into wire types.
struct Types<'a> {
    index: HashMap<&'a str, u32>,
}

impl Types<'_> {
    fn index_of(&self, name: &str) -> Result<u32, CodegenError> {
        self.index.get(name).copied().ok_or_else(|| CodegenError::internal(format!("type {name} is not described")))
    }

    /// The wire type of `ft` at a value position. `IO` actions stored as values are functions
    /// the boundary cannot call; they cross as opaque values, as in the Rust facade. An async
    /// program is only ever a function's result: elsewhere it is an error.
    fn wire(&self, ft: &FacadeType) -> Result<Type, CodegenError> {
        Ok(match ft {
            FacadeType::Nat => Type::Nat,
            FacadeType::Int => Type::Int,
            FacadeType::Bool => Type::Bool,
            FacadeType::Uint8 => Type::UInt8,
            FacadeType::Uint16 => Type::UInt16,
            FacadeType::Uint32 => Type::UInt32,
            FacadeType::Uint64 => Type::UInt64,
            FacadeType::Usize => Type::USize,
            FacadeType::Int8 => Type::Int8,
            FacadeType::Int16 => Type::Int16,
            FacadeType::Int32 => Type::Int32,
            FacadeType::Int64 => Type::Int64,
            FacadeType::Isize => Type::ISize,
            FacadeType::Float => Type::Float,
            FacadeType::Float32 => Type::Float32,
            FacadeType::Char => Type::Char,
            FacadeType::String => Type::String,
            FacadeType::Unit => Type::Unit,
            FacadeType::ByteArray => Type::ByteArray,
            FacadeType::FloatArray => Type::FloatArray,
            FacadeType::Option(t) => Type::Option(Box::new(self.wire(t)?)),
            FacadeType::List(t) => Type::List(Box::new(self.wire(t)?)),
            FacadeType::Array(t) => Type::Array(Box::new(self.wire(t)?)),
            FacadeType::Prod(a, b) => Type::Prod(Box::new(self.wire(a)?), Box::new(self.wire(b)?)),
            FacadeType::Except { error, value } => {
                Type::Except { error: Box::new(self.wire(error)?), value: Box::new(self.wire(value)?) }
            }
            FacadeType::Io(_) | FacadeType::Eio { .. } | FacadeType::BaseIo(_) => Type::Opaque,
            FacadeType::Function { params, result } => {
                if params.is_empty() || params.len() > wire::MAX_FUNCTION_PARAMS {
                    Type::Opaque
                } else {
                    Type::Function {
                        params: params.iter().map(|p| self.wire(p)).collect::<Result<_, _>>()?,
                        result: Box::new(self.wire(result)?),
                    }
                }
            }
            FacadeType::Param(i) => Type::Param(*i),
            FacadeType::Inductive { name, args } => Type::Inductive {
                index: *self
                    .index
                    .get(name.as_str())
                    .ok_or_else(|| CodegenError::internal(format!("type {name} is not described")))?,
                args: args.iter().map(|a| self.wire(a)).collect::<Result<_, _>>()?,
            },
            // A named opaque type is an entry of the table; other opaque values have no name.
            FacadeType::Opaque { head: Some(h), .. } if self.index.contains_key(h.as_str()) => {
                Type::Inductive { index: self.index[h.as_str()], args: Vec::new() }
            }
            FacadeType::Opaque { .. } => Type::Opaque,
            FacadeType::Async { op, .. } => {
                return Err(CodegenError::external(
                    ErrorCode::AsyncInterface,
                    format!(
                        "an async program over {op} appears inside a value; an async program crosses to the host \
                         only as what an exported function returns"
                    ),
                ));
            }
        })
    }

    /// What a function with result type `ft` returns.
    fn returns(&self, ft: &FacadeType) -> Result<Returns, CodegenError> {
        Ok(match ft {
            FacadeType::Io(t) => Returns::Io(self.wire(t)?),
            FacadeType::Eio { error, value } => Returns::Eio { error: self.wire(error)?, value: self.wire(value)? },
            // `BaseIO α` functions return their value directly.
            FacadeType::BaseIo(t) => Returns::Value(self.wire(t)?),
            FacadeType::Async { op, rets, result } => Returns::Async {
                op: self.index_of(op)?,
                rets: rets.iter().map(|r| self.wire(r)).collect::<Result<_, _>>()?,
                value: self.wire(result)?,
            },
            other => Returns::Value(self.wire(other)?),
        })
    }
}

fn repr_of(ir: IrType) -> Result<wire::Repr, CodegenError> {
    Ok(match ir {
        IrType::Float => wire::Repr::Float,
        IrType::Float32 => wire::Repr::Float32,
        IrType::Uint8 => wire::Repr::UInt8,
        IrType::Uint16 => wire::Repr::UInt16,
        IrType::Uint32 => wire::Repr::UInt32,
        IrType::Uint64 => wire::Repr::UInt64,
        IrType::Usize => wire::Repr::USize,
        IrType::Object | IrType::Tobject | IrType::Tagged => wire::Repr::Object,
        IrType::Erased | IrType::Void => {
            return Err(CodegenError::internal(format!("a type represented as {}", ir.name())));
        }
    })
}

/// The type table of `types`.
fn type_table(types: &[&TypeDecl], t: &Types) -> Result<wire::TypeTable, CodegenError> {
    let mut out = Vec::new();
    for decl in types {
        if decl.opaque {
            out.push(wire::TypeDecl {
                name: decl.name.clone(),
                opaque: true,
                params: 0,
                repr: wire::Repr::Object,
                trivial: None,
                ctors: Vec::new(),
            });
            continue;
        }
        let mut ctors = Vec::new();
        for c in &decl.ctors {
            let mut fields = Vec::new();
            for f in &c.fields {
                let kind = match f.kind {
                    FieldKind::Object(i) => wire::FieldKind::Object(i),
                    FieldKind::Usize(i) => wire::FieldKind::USize(i),
                    FieldKind::Scalar { offset, ty, .. } => wire::FieldKind::Scalar { offset, repr: repr_of(ty)? },
                    FieldKind::Erased | FieldKind::Void => {
                        return Err(CodegenError::internal(format!(
                            "type {} has a field without a runtime representation",
                            decl.name
                        )));
                    }
                };
                fields.push(wire::Field { name: f.name.clone(), kind, ty: t.wire(&f.ty)? });
            }
            ctors.push(wire::Ctor {
                name: c.name.clone(),
                tag: c.tag,
                size: c.size,
                usize: c.usize,
                ssize: c.ssize,
                fields,
            });
        }
        let trivial = match &decl.trivial {
            Some(triv) => {
                let c = decl.ctors.iter().position(|c| c.name == triv.ctor).ok_or_else(|| {
                    CodegenError::internal(format!("trivial structure constructor of {} missing", decl.name))
                })?;
                Some((c as u32, triv.field))
            }
            None => None,
        };
        out.push(wire::TypeDecl {
            name: decl.name.clone(),
            opaque: false,
            params: decl.params.len() as u32,
            repr: repr_of(decl.repr)?,
            trivial,
            ctors,
        });
    }
    Ok(wire::TypeTable { types: out })
}

/// A C array initializer of `bytes`.
pub(crate) fn byte_array(name: &str, bytes: &[u8]) -> Vec<String> {
    let mut lines = vec![format!("static const uint8_t {name}[] = {{")];
    if bytes.is_empty() {
        // An empty array is not valid C; the length is carried separately as zero.
        lines = vec![format!("static const uint8_t {name}[1] = {{0}};")];
        return lines;
    }
    for chunk in bytes.chunks(16) {
        let items: Vec<String> = chunk.iter().map(|b| format!("0x{b:02x}")).collect();
        lines.push(format!("    {},", items.join(", ")));
    }
    lines.push("};".to_owned());
    lines
}

/// Emits a static type expression and returns the C expressions of its bytes and length.
struct TypeConsts {
    lines: Vec<String>,
    count: usize,
}

impl TypeConsts {
    fn add(&mut self, prefix: &str, ty: &Type) -> (String, String) {
        let mut bytes = Vec::new();
        ty.encode(&mut bytes);
        self.bytes(prefix, &bytes)
    }

    /// What a function returns, encoded.
    fn returns(&mut self, prefix: &str, returns: &Returns) -> (String, String) {
        let mut bytes = Vec::new();
        returns.encode(&mut bytes);
        self.bytes(prefix, &bytes)
    }

    fn bytes(&mut self, prefix: &str, bytes: &[u8]) -> (String, String) {
        let name = format!("{prefix}t_{}", self.count);
        self.count += 1;
        self.lines.extend(byte_array(&name, bytes));
        let len = bytes.len();
        (name, len.to_string())
    }
}

/// Boxes an IR scalar `x` of type `ir` into an owned object (objects are returned as they are).
fn boxed(ir: IrType, x: &str) -> String {
    match ir {
        IrType::Uint8 | IrType::Uint16 => format!("lungo_box((size_t){x})"),
        IrType::Uint32 => format!("lungo_box_uint32({x})"),
        IrType::Uint64 => format!("lungo_box_uint64({x})"),
        IrType::Usize => format!("lungo_box_usize({x})"),
        IrType::Float => format!("lungo_box_float({x})"),
        IrType::Float32 => format!("lungo_box_float32({x})"),
        _ => x.to_owned(),
    }
}

/// The IR scalar of type `ir` held by the boxed object `o`, and whether `o` must be released
/// once the scalar is read.
fn unboxed(ir: IrType, o: &str) -> String {
    match ir {
        IrType::Uint8 | IrType::Uint16 => format!("({})lungo_unbox({o})", c_type(ir)),
        IrType::Uint32 => format!("lungo_unbox_uint32({o})"),
        IrType::Uint64 => format!("lungo_unbox_uint64({o})"),
        IrType::Usize => format!("lungo_unbox_usize({o})"),
        IrType::Float => format!("lungo_unbox_float({o})"),
        IrType::Float32 => format!("lungo_unbox_float32({o})"),
        _ => o.to_owned(),
    }
}

pub struct BoundaryInput<'a> {
    /// The program's C identifier.
    pub id: &'a str,
    /// The Lean version the program was compiled with (part of every layout fingerprint).
    pub lean_version: &'a str,
    pub success: &'a Success,
    pub program: &'a Program,
    pub externs: &'a ExternPlan,
    pub run_main: Option<String>,
}

/// Computes the boundary and emits its C.
pub fn generate(input: &BoundaryInput, e: &Emitter) -> Result<(Boundary, String), Vec<CodegenError>> {
    let prefix = e.prefix;
    let interface = &input.success.interface;
    let assurance = &input.success.assurance;
    let requirements: HashMap<&str, &ExternRequirement> =
        input.success.extern_requirements.iter().map(|r| (r.declaration.as_str(), r)).collect();
    let capability_ids: HashMap<&str, &str> =
        assurance.capabilities.iter().map(|c| (c.name.as_str(), c.id.as_str())).collect();
    // The operations the host implements, grouped by capability: capabilities by identifier,
    // operations by declaration. Their order is the order of their indices.
    let mut grouped: std::collections::BTreeMap<&str, Vec<(&Declaration, &ExternRequirement, String, &str)>> =
        std::collections::BTreeMap::new();
    for (d, r) in &input.externs.resolutions {
        let Resolution::Application { key, .. } = r else { continue };
        let decl = input
            .program
            .declaration(d)
            .ok_or_else(|| vec![CodegenError::internal(format!("extern {d} missing"))])?;
        let req = requirements
            .get(d.as_str())
            .copied()
            .ok_or_else(|| vec![CodegenError::internal(format!("no extern requirement for {d}"))])?;
        let capability = &req
            .operation
            .as_ref()
            .ok_or_else(|| vec![CodegenError::internal(format!("{d} is implemented by the host but is no operation"))])?
            .capability;
        let id = capability_ids
            .get(capability.as_str())
            .copied()
            .ok_or_else(|| vec![CodegenError::internal(format!("the capability {capability} has no record"))])?;
        grouped.entry(id).or_default().push((decl, req, key.clone(), capability.as_str()));
    }
    let host_reqs: Vec<&ExternRequirement> = grouped.values().flatten().map(|(_, r, _, _)| *r).collect();
    let reachable = reachable_types(&interface.types, &interface.exports, &host_reqs, &|_| false);
    let types: Vec<&TypeDecl> = interface.types.iter().filter(|t| reachable.contains(t.name.as_str())).collect();
    let t = Types { index: types.iter().enumerate().map(|(i, d)| (d.name.as_str(), i as u32)).collect() };
    let table = type_table(&types, &t).map_err(|e| vec![e])?;
    let fingerprints = crate::core::fingerprint::fingerprints(&interface.types, input.lean_version);
    let mut errors = Vec::new();
    let mut w = Writer::new();
    let mut consts = TypeConsts { lines: Vec::new(), count: 0 };
    let mut body = Writer::new();

    // Exported functions.
    let mut functions = Vec::new();
    let mut exports: Vec<&Export> = interface.exports.iter().collect();
    exports.sort_by(|a, b| a.name.cmp(&b.name));
    for export in exports {
        match emit_call(&mut body, &mut consts, e, &t, input.program, export) {
            Ok(f) => functions.push(f),
            Err(err) => errors.push(err),
        }
    }

    // Async capabilities, as the async exports ask their operations.
    let mut async_capabilities: Vec<AsyncCapability> = Vec::new();
    for f in &functions {
        let Returns::Async { op, rets, .. } = &f.returns else { continue };
        if async_capabilities.iter().any(|c| c.op_type == *op) {
            continue;
        }
        let op_name = &table.types[*op as usize].name;
        let record = assurance.capabilities.iter().find(|c| {
            matches!(&c.kind, CapabilityKind::Async { op_type, .. } if op_type == op_name)
        });
        let Some(record) = record else {
            errors.push(CodegenError::external(
                ErrorCode::AsyncInterface,
                format!(
                    "{} returns an async program over {op_name}, which is not the operation type of an async \
                     capability: give its `Lungo.Async.Interface {op_name}` instance `@[lungo_capability \"ns.name\"]`",
                    f.lean_name
                ),
            ));
            continue;
        };
        let operations = table.types[*op as usize]
            .ctors
            .iter()
            .zip(rets)
            .enumerate()
            .map(|(i, (c, answer))| AsyncOperation { ctor: i as u32, lean_name: c.name.clone(), answer: answer.clone() })
            .collect();
        async_capabilities.push(AsyncCapability {
            id: record.id.clone(),
            lean_name: record.name.clone(),
            op_type: *op,
            operations,
        });
    }
    async_capabilities.sort_by(|a, b| a.id.cmp(&b.id));

    // Capabilities and their operations.
    let mut capabilities = Vec::new();
    let mut index = 0;
    for (id, ops) in &grouped {
        let mut operations = Vec::new();
        for (decl, req, key, _) in ops {
            match emit_host_adapter(&mut body, &mut consts, e, &t, index, decl, req, key) {
                Ok(o) => operations.push(o),
                Err(err) => errors.push(err),
            }
            index += 1;
        }
        capabilities.push(Capability { id: (*id).to_owned(), lean_name: ops[0].3.to_owned(), operations });
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // The type table, the host callbacks, and the check that every capability is provided.
    w.line(format!("#include \"{}_program.h\"", input.id));
    w.line("");
    let table_bytes = table.encode();
    for l in byte_array(&format!("{prefix}type_table"), &table_bytes) {
        w.line(l);
    }
    w.line(format!("static lungo_lazy_bits {prefix}types_cell;"));
    w.open(format!("static uint64_t {prefix}types_load(void) {{"));
    w.line(format!("return (uint64_t)(uintptr_t)lungo_types_load({prefix}type_table, {});", table_bytes.len()));
    w.close("}");
    w.open(format!("const lungo_types *{prefix}types(void) {{"));
    w.line(format!(
        "return (const lungo_types *)(uintptr_t)lungo_lazy_bits_get(&{prefix}types_cell, {prefix}types_load);"
    ));
    w.close("}");
    w.line("");
    let n = index;
    // A zero-length array is not valid C; the table always has a slot.
    w.line(format!("static uint64_t {prefix}host_callbacks[{}];", n.max(1)));
    w.line(comment("Registers the host's implementation of operation `index`, before the program is initialized."));
    w.open(format!("void {prefix}set_host_extern(size_t index, uint64_t callback) {{"));
    if n == 0 {
        // A program without operations registers nothing.
        w.line("(void)index;");
        w.line("(void)callback;");
        w.line("lungo_panic_unreachable();");
    } else {
        w.line(format!("if (index >= {n} || callback == 0) lungo_panic_unreachable();"));
        w.line(format!("{prefix}host_callbacks[index] = callback;"));
    }
    w.close("}");
    w.open(format!("void {prefix}check_capabilities(void) {{"));
    for c in &capabilities {
        for o in &c.operations {
            w.line(format!(
                "if ({prefix}host_callbacks[{}] == 0) lungo_panic_capability_missing({}, {}, {});",
                o.index,
                string(c.id.as_bytes()),
                string(operation_name(&o.declaration).as_bytes()),
                string(o.declaration.as_bytes())
            ));
        }
    }
    w.close("}");
    w.line("");
    for l in &consts.lines {
        w.line(l);
    }
    w.line("");
    let text = format!("{}{}", w.finish(), body.finish());
    let boundary = Boundary {
        id: input.id.to_owned(),
        prefix: prefix.to_owned(),
        table,
        types: types
            .iter()
            .map(|d| NamedType {
                lean_name: d.name.clone(),
                params: if d.opaque { Vec::new() } else { d.params.clone() },
                structure: d.structure && !d.opaque,
                opaque: d.opaque,
                fingerprint: fingerprints[&d.name].clone(),
            })
            .collect(),
        functions,
        capabilities,
        async_capabilities,
        run_main: input.run_main.clone(),
        set_host_extern: format!("{prefix}set_host_extern"),
        types_symbol: format!("{prefix}types"),
        initialize: format!("{prefix}initialize"),
    };
    Ok((boundary, text))
}

/// The name of an operation within its capability: the last component of its declaration.
pub fn operation_name(declaration: &str) -> &str {
    declaration.rsplit('.').next().unwrap_or(declaration)
}

/// The name of a capability in a package: the last segment of its identifier (`clock` for
/// `time.clock`), with `-` as `_`.
pub fn capability_name(id: &str) -> String {
    id.rsplit('.').next().unwrap_or(id).replace('-', "_")
}

/// A readable Lean form of wire type `ty`, for documentation.
pub fn describe_type(types: &[NamedType], ty: &Type) -> String {
    let paren = |t: &Type| {
        let s = describe_type(types, t);
        if s.contains(' ') { format!("({s})") } else { s }
    };
    match ty {
        Type::Nat => "Nat".into(),
        Type::Int => "Int".into(),
        Type::Bool => "Bool".into(),
        Type::UInt8 => "UInt8".into(),
        Type::UInt16 => "UInt16".into(),
        Type::UInt32 => "UInt32".into(),
        Type::UInt64 => "UInt64".into(),
        Type::USize => "USize".into(),
        Type::Int8 => "Int8".into(),
        Type::Int16 => "Int16".into(),
        Type::Int32 => "Int32".into(),
        Type::Int64 => "Int64".into(),
        Type::ISize => "ISize".into(),
        Type::Float => "Float".into(),
        Type::Float32 => "Float32".into(),
        Type::Char => "Char".into(),
        Type::String => "String".into(),
        Type::Unit => "Unit".into(),
        Type::ByteArray => "ByteArray".into(),
        Type::FloatArray => "FloatArray".into(),
        Type::Option(t) => format!("Option {}", paren(t)),
        Type::List(t) => format!("List {}", paren(t)),
        Type::Array(t) => format!("Array {}", paren(t)),
        Type::Prod(a, b) => format!("{} × {}", paren(a), paren(b)),
        Type::Except { error, value } => format!("Except {} {}", paren(error), paren(value)),
        Type::Function { params, result } => {
            let mut parts: Vec<String> = params.iter().map(paren).collect();
            parts.push(paren(result));
            parts.join(" → ")
        }
        Type::Param(i) => format!("α{i}"),
        Type::Inductive { index, args } => {
            let name = types.get(*index as usize).map_or("?", |t| t.lean_name.as_str());
            std::iter::once(name.to_owned()).chain(args.iter().map(paren)).collect::<Vec<_>>().join(" ")
        }
        Type::Opaque => "(opaque)".into(),
    }
}

/// Emits `<prefix>call_<name>` for `export`.
fn emit_call(
    w: &mut Writer,
    consts: &mut TypeConsts,
    e: &Emitter,
    t: &Types,
    program: &Program,
    export: &Export,
) -> Result<Function, CodegenError> {
    let prefix = e.prefix;
    let decl = program
        .declaration(&export.name)
        .ok_or_else(|| CodegenError::internal(format!("export {} has no compiled declaration", export.name)))?;
    if export.params.len() != decl.params.len() {
        return Err(CodegenError::internal(format!(
            "export {} has {} parameters for {} compiled parameters",
            export.name,
            export.params.len(),
            decl.params.len()
        )));
    }
    let symbol = format!("{prefix}call_{}", mangle(&export.name));
    let returns = t.returns(&export.result)?;
    w.line(comment(&format!("{} : {}", export.name, export.lean_type)));
    w.open(format!("int32_t {symbol}(const uint8_t *input, size_t len, lungo_buffer *out) {{"));
    w.line(format!("{prefix}initialize();"));
    w.line(format!("lungo_call *call = lungo_call_begin({prefix}types(), input, len);"));
    let mut params = Vec::new();
    let mut decoded = Vec::new();
    let mut args = Vec::new();
    let mut release = Vec::new();
    let mut prepare = Vec::new();
    for (i, (fp, p)) in export.params.iter().zip(&decl.params).enumerate() {
        match fp {
            FacadeParam::Erased => {
                if p.ty != IrType::Void {
                    args.push("lungo_box(0)".to_owned());
                }
            }
            FacadeParam::Value { name, ty } => {
                if matches!(p.ty, IrType::Void | IrType::Erased) {
                    return Err(CodegenError::internal(format!(
                        "{}: a runtime parameter is compiled as {}",
                        export.name,
                        p.ty.name()
                    )));
                }
                let wty = t.wire(ty)?;
                let (bytes, len) = consts.add(prefix, &wty);
                w.line(format!("lungo_obj a_{i} = lungo_call_read(call, {bytes}, {len});"));
                decoded.push(format!("a_{i}"));
                if p.ty.is_scalar() {
                    prepare.push(format!("{} p_{i} = {};", c_type(p.ty), unboxed(p.ty, &format!("a_{i}"))));
                    prepare.push(format!("lungo_dec(a_{i});"));
                    args.push(format!("p_{i}"));
                } else {
                    args.push(format!("a_{i}"));
                    if p.borrow {
                        release.push(format!("lungo_dec(a_{i});"));
                    }
                }
                params.push(Param { name: name.clone(), ty: wty });
            }
        }
    }
    w.open("if (!lungo_call_complete(call)) {");
    for d in &decoded {
        w.line(format!("lungo_dec({d});"));
    }
    w.line("return lungo_call_end(call, out);");
    w.close("}");
    for l in prepare {
        w.line(l);
    }
    w.line(format!("{} r = {}({});", c_type(decl.result), e.symbol(&decl.name), args.join(", ")));
    for l in release {
        w.line(l);
    }
    match &returns {
        Returns::Value(ty) => {
            let (bytes, len) = consts.add(prefix, ty);
            w.line(format!("lungo_call_write(call, {}, {bytes}, {len});", boxed(decl.result, "r")));
        }
        Returns::Io(ty) => {
            if decl.result.is_scalar() {
                return Err(CodegenError::internal(format!("{}: IO result compiled as a scalar", export.name)));
            }
            let (bytes, len) = consts.add(prefix, ty);
            w.line(format!("lungo_call_write_io(call, r, {bytes}, {len});"));
        }
        Returns::Eio { error, value } => {
            if decl.result.is_scalar() {
                return Err(CodegenError::internal(format!("{}: EIO result compiled as a scalar", export.name)));
            }
            let (eb, el) = consts.add(prefix, error);
            let (vb, vl) = consts.add(prefix, value);
            w.line(format!("lungo_call_write_eio(call, r, {eb}, {el}, {vb}, {vl});"));
        }
        Returns::Async { .. } => {
            if decl.result.is_scalar() {
                return Err(CodegenError::internal(format!("{}: async program compiled as a scalar", export.name)));
            }
            let (rb, rl) = consts.returns(prefix, &returns);
            w.line(format!("lungo_call_write_async(call, r, {rb}, {rl});"));
        }
    }
    w.line("return lungo_call_end(call, out);");
    w.close("}");
    w.line("");
    Ok(Function {
        lean_name: export.name.clone(),
        module: export.module.clone(),
        lean_type: export.lean_type.clone(),
        symbol,
        type_params: export.type_params.clone(),
        params,
        returns,
        source: export.source.clone(),
    })
}

/// Replaces the type parameters of a polymorphic extern by opaque values: the host receives and
/// returns them unchanged.
fn opaque_params(ty: Type) -> Type {
    match ty {
        Type::Param(_) => Type::Opaque,
        Type::Option(t) => Type::Option(Box::new(opaque_params(*t))),
        Type::List(t) => Type::List(Box::new(opaque_params(*t))),
        Type::Array(t) => Type::Array(Box::new(opaque_params(*t))),
        Type::Prod(a, b) => Type::Prod(Box::new(opaque_params(*a)), Box::new(opaque_params(*b))),
        Type::Except { error, value } => {
            Type::Except { error: Box::new(opaque_params(*error)), value: Box::new(opaque_params(*value)) }
        }
        Type::Function { params, result } => Type::Function {
            params: params.into_iter().map(opaque_params).collect(),
            result: Box::new(opaque_params(*result)),
        },
        Type::Inductive { index, args } => {
            Type::Inductive { index, args: args.into_iter().map(opaque_params).collect() }
        }
        other => other,
    }
}

fn opaque_returns(r: Returns) -> Result<Returns, CodegenError> {
    Ok(match r {
        Returns::Value(t) => Returns::Value(opaque_params(t)),
        Returns::Io(t) => Returns::Io(opaque_params(t)),
        Returns::Eio { error, value } => Returns::Eio { error: opaque_params(error), value: opaque_params(value) },
        Returns::Async { .. } => {
            return Err(CodegenError::external(
                ErrorCode::AsyncInterface,
                "an operation of a capability returns an async program; an operation the host performs \
                 asynchronously is a constructor of an async capability's operation type",
            ));
        }
    })
}

/// Emits the adapter of operation `decl`.
#[allow(clippy::too_many_arguments)]
fn emit_host_adapter(
    w: &mut Writer,
    consts: &mut TypeConsts,
    e: &Emitter,
    t: &Types,
    index: usize,
    decl: &Declaration,
    req: &ExternRequirement,
    key: &str,
) -> Result<Operation, CodegenError> {
    let prefix = e.prefix;
    let sig = req.facade.as_ref().ok_or_else(|| {
        CodegenError::external(
            ErrorCode::ExternSignature,
            format!(
                "cannot generate a host adapter for extern {} (`{}`): its Lean type does not determine a signature",
                decl.name,
                req.lean_type.as_deref().unwrap_or("unknown type")
            ),
        )
    })?;
    if sig.params.len() != decl.params.len() {
        return Err(CodegenError::internal(format!("extern {} signature arity mismatch", decl.name)));
    }
    let returns = opaque_returns(t.returns(&sig.result)?)?;
    w.line(comment(&format!("Lean calls the host's {key} for {}.", decl.name)));
    w.open(format!("{} {{", host_adapter_prototype(prefix, decl)));
    w.line(format!(
        "lungo_hostcall *call = lungo_hostcall_begin({prefix}types(), {prefix}host_callbacks[{index}], {});",
        string(decl.name.as_bytes())
    ));
    let mut params = Vec::new();
    for (fp, p) in sig.params.iter().zip(&decl.params) {
        if matches!(p.ty, IrType::Erased | IrType::Void) {
            continue;
        }
        let x = format!("x_{}", p.var);
        match fp {
            FacadeParam::Value { name, ty } => {
                let wty = opaque_params(t.wire(ty)?);
                let (bytes, len) = consts.add(prefix, &wty);
                if p.ty.is_scalar() {
                    w.open("{");
                    w.line(format!("lungo_obj b = {};", boxed(p.ty, &x)));
                    w.line(format!("lungo_hostcall_write(call, b, {bytes}, {len});"));
                    w.line("lungo_dec(b);");
                    w.close("}");
                } else {
                    w.line(format!("lungo_hostcall_write(call, {x}, {bytes}, {len});"));
                    if !p.borrow {
                        w.line(format!("lungo_dec({x});"));
                    }
                }
                params.push(Param { name: name.clone(), ty: wty });
            }
            FacadeParam::Erased => {
                if p.ty.is_object() && !p.borrow {
                    w.line(format!("lungo_dec({x});"));
                } else {
                    w.line(format!("(void){x};"));
                }
            }
        }
    }
    match &returns {
        Returns::Value(ty) => {
            let (bytes, len) = consts.add(prefix, ty);
            w.line(format!("lungo_obj v = lungo_hostcall_finish(call, {bytes}, {len});"));
            if decl.result.is_scalar() {
                w.line(format!("{} r = {};", c_type(decl.result), unboxed(decl.result, "v")));
                w.line("lungo_dec(v);");
                w.line("return r;");
            } else {
                w.line("return v;");
            }
        }
        Returns::Io(ty) => {
            let (bytes, len) = consts.add(prefix, ty);
            w.line(format!("return lungo_hostcall_finish_io(call, {bytes}, {len});"));
        }
        Returns::Eio { error, value } => {
            let (eb, el) = consts.add(prefix, error);
            let (vb, vl) = consts.add(prefix, value);
            w.line(format!("return lungo_hostcall_finish_eio(call, {eb}, {el}, {vb}, {vl});"));
        }
        Returns::Async { .. } => unreachable!("refused by opaque_returns"),
    }
    w.close("}");
    w.line("");
    Ok(Operation {
        index,
        key: key.to_owned(),
        declaration: decl.name.clone(),
        lean_type: req.lean_type.clone(),
        params,
        returns,
    })
}
