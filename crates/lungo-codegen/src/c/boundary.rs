//! The program's boundary: how the language bindings call exported Lean functions and how
//! Lean calls the host's implementations of externs.
//!
//! [`generate`] computes the [`Boundary`], the model every binding generator consumes (the
//! program's types as a wire-format type table, its callable functions, the externs the host
//! implements, the entry point), and emits its C: the type table, one call entry point per
//! exported function, and one adapter per host extern.

use super::program::Emitter;
use super::syntax::{c_type, comment, string};
use crate::core::externs::{ExternPlan, Resolution, implementation_params};
use crate::core::interface::reachable_types;
use crate::core::names::mangle;
use crate::core::writer::Writer;
use crate::{CodegenError, ErrorCode};
use lungo_bir::{Declaration, IrType, Program};
use lungo_protocol::{
    DeclSource, Export, ExternRequirement, FacadeParam, FacadeType, FieldKind, Success, Trust, TypeDecl,
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
    pub returns: Returns,
    pub source: Option<DeclSource>,
    pub trust: Trust,
}

/// An extern the host implements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostExtern {
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

impl HostExtern {
    /// The signature of the host's implementation.
    pub fn signature(&self) -> Signature {
        Signature { type_params: 0, params: self.params.iter().map(|p| p.ty.clone()).collect(), returns: self.returns.clone() }
    }
}

/// A type of the table, with the Lean names generators need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedType {
    pub lean_name: String,
    pub params: Vec<String>,
    pub structure: bool,
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
    pub host_externs: Vec<HostExtern>,
    /// `int32_t <prefix>run_main(size_t argc, const char *const *argv)`, when a root module
    /// defines `main`.
    pub run_main: Option<String>,
    /// `void <prefix>set_host_extern(size_t index, uint64_t callback)`.
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
    /// The wire type of `ft` at a value position. `IO` actions stored as values are functions
    /// the boundary cannot call; they cross as opaque values, as in the Rust facade.
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
            FacadeType::Opaque { .. } => Type::Opaque,
        })
    }

    /// What a function with result type `ft` returns.
    fn returns(&self, ft: &FacadeType) -> Result<Returns, CodegenError> {
        Ok(match ft {
            FacadeType::Io(t) => Returns::Io(self.wire(t)?),
            FacadeType::Eio { error, value } => Returns::Eio { error: self.wire(error)?, value: self.wire(value)? },
            // `BaseIO α` functions return their value directly.
            FacadeType::BaseIo(t) => Returns::Value(self.wire(t)?),
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
        let name = format!("{prefix}t_{}", self.count);
        self.count += 1;
        self.lines.extend(byte_array(&name, &bytes));
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
    pub success: &'a Success,
    pub program: &'a Program,
    pub externs: &'a ExternPlan,
    pub run_main: Option<String>,
}

/// Computes the boundary and emits its C.
pub fn generate(input: &BoundaryInput, e: &Emitter) -> Result<(Boundary, String), Vec<CodegenError>> {
    let prefix = e.prefix;
    let interface = &input.success.interface;
    let requirements: HashMap<&str, &ExternRequirement> =
        input.success.extern_requirements.iter().map(|r| (r.declaration.as_str(), r)).collect();
    let host: Vec<(&Declaration, &ExternRequirement, String)> = input
        .externs
        .resolutions
        .iter()
        .filter_map(|(d, r)| match r {
            Resolution::Application { key, .. } => Some((d.as_str(), key.clone())),
            _ => None,
        })
        .map(|(d, key)| {
            let decl = input.program.declaration(d).ok_or_else(|| CodegenError::internal(format!("extern {d} missing")))?;
            let req =
                requirements.get(d).copied().ok_or_else(|| CodegenError::internal(format!("no extern requirement for {d}")))?;
            Ok((decl, req, key))
        })
        .collect::<Result<_, CodegenError>>()
        .map_err(|e| vec![e])?;
    let host_reqs: Vec<&ExternRequirement> = host.iter().map(|(_, r, _)| *r).collect();
    let reachable = reachable_types(&interface.types, &interface.exports, &host_reqs, &|_| false);
    let types: Vec<&TypeDecl> = interface.types.iter().filter(|t| reachable.contains(t.name.as_str())).collect();
    let t = Types { index: types.iter().enumerate().map(|(i, d)| (d.name.as_str(), i as u32)).collect() };
    let table = type_table(&types, &t).map_err(|e| vec![e])?;
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

    // Host externs.
    let mut host_externs = Vec::new();
    for (index, (decl, req, key)) in host.iter().enumerate() {
        match emit_host_adapter(&mut body, &mut consts, e, &t, index, decl, req, key) {
            Ok(h) => host_externs.push(h),
            Err(err) => errors.push(err),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // The type table, the host callbacks, and the check that every host extern is implemented.
    w.line(format!("#include \"{}_program.h\"", input.id));
    w.line("");
    let table_bytes = table.encode();
    for l in byte_array(&format!("{prefix}type_table"), &table_bytes) {
        w.line(l);
    }
    w.line(format!("static lungo_lazy_bits {prefix}types_cell;"));
    w.open(format!("static uint64_t {prefix}types_load(void) {{"));
    w.line(format!(
        "return (uint64_t)(uintptr_t)lungo_types_load({prefix}type_table, {});",
        table_bytes.len()
    ));
    w.close("}");
    w.open(format!("const lungo_types *{prefix}types(void) {{"));
    w.line(format!("return (const lungo_types *)(uintptr_t)lungo_lazy_bits_get(&{prefix}types_cell, {prefix}types_load);"));
    w.close("}");
    w.line("");
    let n = host_externs.len();
    // A zero-length array is not valid C; the table always has a slot.
    w.line(format!("static uint64_t {prefix}host_callbacks[{}];", n.max(1)));
    w.line(comment("Registers the host's implementation of host extern `index`, before the program is initialized."));
    w.open(format!("void {prefix}set_host_extern(size_t index, uint64_t callback) {{"));
    w.line(format!("if (index >= {n} || callback == 0) lungo_panic_unreachable();"));
    w.line(format!("{prefix}host_callbacks[index] = callback;"));
    w.close("}");
    w.open(format!("void {prefix}check_host_externs(void) {{"));
    for h in &host_externs {
        w.line(format!(
            "if ({prefix}host_callbacks[{}] == 0) lungo_panic_host_extern_missing({});",
            h.index,
            string(h.declaration.as_bytes())
        ));
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
            .map(|d| NamedType { lean_name: d.name.clone(), params: d.params.clone(), structure: d.structure })
            .collect(),
        functions,
        host_externs,
        run_main: input.run_main.clone(),
        set_host_extern: format!("{prefix}set_host_extern"),
        types_symbol: format!("{prefix}types"),
        initialize: format!("{prefix}initialize"),
    };
    Ok((boundary, text))
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
        trust: export.trust.clone(),
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
        Type::Inductive { index, args } => Type::Inductive { index, args: args.into_iter().map(opaque_params).collect() },
        other => other,
    }
}

fn opaque_returns(r: Returns) -> Returns {
    match r {
        Returns::Value(t) => Returns::Value(opaque_params(t)),
        Returns::Io(t) => Returns::Io(opaque_params(t)),
        Returns::Eio { error, value } => Returns::Eio { error: opaque_params(error), value: opaque_params(value) },
    }
}

/// Emits the adapter of host extern `decl`.
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
) -> Result<HostExtern, CodegenError> {
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
    let returns = opaque_returns(t.returns(&sig.result)?);
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
    }
    w.close("}");
    w.line("");
    Ok(HostExtern {
        index,
        key: key.to_owned(),
        declaration: decl.name.clone(),
        lean_type: req.lean_type.clone(),
        params,
        returns,
    })
}
