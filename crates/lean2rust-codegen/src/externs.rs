//! Resolution of Lean `@[extern]` declarations.
//!
//! Externs resolve, in order, to runtime primitives implemented by `lean2rust-runtime` and to
//! Rust functions the application maps with `Config::rust_extern`. An extern that resolves to
//! neither is a build error naming the Lean declaration, the symbol, its Lean type, the runtime
//! representation it must have, and its source location; it is never replaced or ignored.
//!
//! Runtime primitives are checked against the representation Lean's compiler expects. Their
//! ownership conventions are reconciled explicitly: when Lean passes an owned argument to a
//! primitive that borrows it, the caller releases it after the call, and when Lean passes a
//! borrowed argument to a primitive that consumes it, the caller retains it first.

use crate::CodegenError;
use crate::compiler::RT;
use crate::names::mangle;
use lean2rust_bir::{Body, Declaration, ExternEntry, IrType, Param};
use lean2rust_protocol::ExternRequirement;
use lean2rust_runtime::registry::{self, Intrinsic, Ty};
use std::collections::{BTreeMap, HashMap};

/// How an extern declaration is implemented.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// Compiled Lean code that provides the symbol with `@[export]`.
    Lean {
        implementation: String,
    },
    Intrinsic(&'static Intrinsic),
    /// A Rust function supplied by the application, reached through a generated adapter.
    User {
        key: String,
        rust_path: String,
    },
}

/// The key under which an extern declaration is resolved: its C symbol for `@[extern "sym"]`,
/// otherwise its Lean declaration name.
pub fn resolution_key(decl: &Declaration) -> Result<String, CodegenError> {
    let Body::Extern { selected, .. } = &decl.body else {
        return Err(CodegenError::internal(format!("{} is not an extern", decl.name)));
    };
    match selected {
        ExternEntry::Standard { symbol, .. } => Ok(symbol.clone()),
        ExternEntry::Adhoc { .. } | ExternEntry::Inline { .. } => Ok(decl.name.clone()),
        ExternEntry::Opaque => Err(CodegenError::adapter(&decl.name, "extern with an opaque entry")),
    }
}

pub struct ExternCall {
    pub pre: Vec<String>,
    pub call: String,
    pub post: Vec<String>,
}

#[derive(Default)]
pub struct ExternPlan {
    pub resolutions: BTreeMap<String, Resolution>,
}

fn ty_matches(ir: IrType, ty: Ty) -> bool {
    match ty {
        Ty::obj | Ty::b_obj => ir.is_object(),
        Ty::u8 => ir == IrType::Uint8,
        Ty::u16 => ir == IrType::Uint16,
        Ty::u32 => ir == IrType::Uint32,
        Ty::u64 => ir == IrType::Uint64,
        Ty::usize => ir == IrType::Usize,
        Ty::f64 => ir == IrType::Float,
        Ty::f32 => ir == IrType::Float32,
    }
}

/// Parameters passed to an extern's implementation: Lean's C emitter drops erased and `void`
/// parameters of `@[extern]` declarations.
pub fn implementation_params(decl: &Declaration) -> Vec<&Param> {
    decl.params.iter().filter(|p| !matches!(p.ty, IrType::Erased | IrType::Void)).collect()
}

fn describe(
    req: Option<&ExternRequirement>,
    decl: &Declaration,
    key: &str,
    source_prefix: &dyn Fn(&ExternRequirement) -> Option<String>,
) -> String {
    let mut s = format!("Declaration: {}\nSymbol: {key}\n", decl.name);
    if let Some(t) = req.and_then(|r| r.lean_type.as_deref()) {
        s.push_str(&format!("Lean type: {t}\n"));
    }
    let params: Vec<String> = implementation_params(decl)
        .iter()
        .map(|p| format!("{}{}", if p.borrow { "@& " } else { "" }, p.ty.name()))
        .collect();
    s.push_str(&format!("Expected runtime representation: ({}) -> {}\n", params.join(", "), decl.result.name()));
    if let Some(src) = req.and_then(source_prefix) {
        s.push_str(&format!("Source: {src}\n"));
    }
    s
}

impl ExternPlan {
    /// Resolves every extern declaration of the program.
    pub fn resolve(
        decls: &[Declaration],
        requirements: &[ExternRequirement],
        user: &BTreeMap<String, String>,
        source: &dyn Fn(&ExternRequirement) -> Option<String>,
    ) -> Result<ExternPlan, Vec<CodegenError>> {
        let reqs: HashMap<&str, &ExternRequirement> =
            requirements.iter().map(|r| (r.declaration.as_str(), r)).collect();
        let mut plan = ExternPlan::default();
        let mut errors = Vec::new();
        for decl in decls.iter().filter(|d| matches!(d.body, Body::Extern { .. })) {
            let key = match resolution_key(decl) {
                Ok(k) => k,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let req = reqs.get(decl.name.as_str()).copied();
            let intrinsic = registry::lookup(&key);
            if let Body::Extern { exported_by: Some(implementation), .. } = &decl.body {
                if intrinsic.is_some() || user.contains_key(&key) {
                    errors.push(CodegenError::Extern(format!(
                        "the symbol `{key}` is provided both by the Lean definition {implementation} (via @[export]) and by {}\n\n{}",
                        if intrinsic.is_some() { "the lean2rust runtime" } else { "a Config::rust_extern mapping" },
                        describe(req, decl, &key, source)
                    )));
                } else {
                    plan.resolutions
                        .insert(decl.name.clone(), Resolution::Lean { implementation: implementation.clone() });
                }
                continue;
            }
            match (intrinsic, user.get(&key)) {
                (Some(_), Some(path)) => errors.push(CodegenError::Extern(format!(
                    "the Lean runtime symbol `{key}` is implemented by lean2rust and cannot be remapped to `{path}`\n\n{}",
                    describe(req, decl, &key, source)
                ))),
                (Some(intrinsic), None) => match check_signature(decl, intrinsic) {
                    Ok(()) => {
                        plan.resolutions.insert(decl.name.clone(), Resolution::Intrinsic(intrinsic));
                    }
                    Err(msg) => errors.push(CodegenError::Extern(format!(
                        "lean2rust runtime primitive `{key}` does not match the representation Lean's compiler expects: {msg}\n\n{}",
                        describe(req, decl, &key, source)
                    ))),
                },
                (None, Some(path)) => {
                    plan.resolutions.insert(
                        decl.name.clone(),
                        Resolution::User {
                            key: key.clone(),
                            rust_path: path.clone(),
                        },
                    );
                }
                (None, None) => {
                    let reason = match registry::unsupported(&key) {
                        Some(u) => format!(
                            "\nThe lean2rust PureRust runtime does not provide this Lean runtime primitive: {}\n",
                            u.reason
                        ),
                        None => String::new(),
                    };
                    errors.push(CodegenError::Extern(format!(
                        "unresolved Lean external symbol\n\n{}{reason}\nProvide a Rust mapping with Config::rust_extern({:?}, \"crate::path::to::function\")",
                        describe(req, decl, &key, source),
                        key
                    )));
                }
            }
        }
        for key in user.keys() {
            let used = plan.resolutions.values().any(|r| matches!(r, Resolution::User { key: k, .. } if k == key));
            if !used {
                errors.push(CodegenError::Extern(format!(
                    "Config::rust_extern maps `{key}`, but no extern declaration reachable from the root modules uses that symbol"
                )));
            }
        }
        if errors.is_empty() { Ok(plan) } else { Err(errors) }
    }

    /// The call of the implementation of extern `decl`, whose Rust wrapper receives the
    /// declaration's non-`void` parameters as `x_<var>`.
    pub fn call<'d>(
        &self,
        decl: &Declaration,
        lookup: &dyn Fn(&str) -> Option<&'d Declaration>,
    ) -> Result<ExternCall, CodegenError> {
        let resolution = self
            .resolutions
            .get(&decl.name)
            .ok_or_else(|| CodegenError::internal(format!("extern {} was not resolved", decl.name)))?;
        let params = implementation_params(decl);
        match resolution {
            Resolution::Lean { implementation } => {
                let callee = lookup(implementation).ok_or_else(|| {
                    CodegenError::internal(format!("@[export] implementation {implementation} is not in the program"))
                })?;
                let callee_params: Vec<&Param> = callee.params.iter().filter(|p| p.ty != IrType::Void).collect();
                let representable = callee_params.len() == params.len()
                    && callee_params
                        .iter()
                        .zip(&params)
                        .all(|(q, p)| if q.ty.is_scalar() || p.ty.is_scalar() { q.ty == p.ty } else { true })
                    && (if decl.result.is_scalar() || callee.result.is_scalar() {
                        decl.result == callee.result
                    } else {
                        true
                    });
                if !representable {
                    return Err(CodegenError::adapter(
                        &decl.name,
                        format!(
                            "the @[export] implementation {implementation} does not have the extern's runtime representation"
                        ),
                    ));
                }
                let mut pre = Vec::new();
                let mut post = Vec::new();
                let mut args = Vec::new();
                for (p, q) in params.iter().zip(&callee_params) {
                    let x = format!("x_{}", p.var);
                    if p.ty.is_object() {
                        match (p.borrow, q.borrow) {
                            (false, true) => post.push(format!("{RT}::lean_dec({x});")),
                            (true, false) => pre.push(format!("{RT}::lean_inc({x});")),
                            _ => {}
                        }
                    }
                    args.push(x);
                }
                Ok(ExternCall { pre, call: format!("{}({})", mangle(implementation), args.join(", ")), post })
            }
            Resolution::Intrinsic(intrinsic) => {
                let mut pre = Vec::new();
                let mut post = Vec::new();
                let mut args = Vec::new();
                for (p, ty) in params.iter().zip(intrinsic.params) {
                    let x = format!("x_{}", p.var);
                    if p.ty.is_object() {
                        match (p.borrow, ty) {
                            (false, Ty::b_obj) => post.push(format!("{RT}::lean_dec({x});")),
                            (true, Ty::obj) => pre.push(format!("{RT}::lean_inc({x});")),
                            _ => {}
                        }
                    }
                    args.push(x);
                }
                Ok(ExternCall {
                    pre,
                    call: format!("{RT}::intrinsics::{}({})", intrinsic.symbol, args.join(", ")),
                    post,
                })
            }
            Resolution::User { .. } => {
                let args: Vec<String> = params.iter().map(|p| format!("x_{}", p.var)).collect();
                Ok(ExternCall {
                    pre: Vec::new(),
                    call: format!("a_{}({})", mangle(&decl.name), args.join(", ")),
                    post: Vec::new(),
                })
            }
        }
    }
}

fn check_signature(decl: &Declaration, intrinsic: &Intrinsic) -> Result<(), String> {
    let params = implementation_params(decl);
    let expected: Vec<String> = intrinsic.params.iter().map(|t| t.name().to_owned()).collect();
    let actual: Vec<String> =
        params.iter().map(|p| format!("{}{}", if p.borrow { "@& " } else { "" }, p.ty.name())).collect();
    let mismatch = || {
        format!(
            "the runtime implements ({}) -> {}, Lean passes ({}) -> {}",
            expected.join(", "),
            intrinsic.result.name(),
            actual.join(", "),
            decl.result.name()
        )
    };
    if params.len() != intrinsic.params.len() {
        return Err(mismatch());
    }
    for (p, ty) in params.iter().zip(intrinsic.params) {
        if !ty_matches(p.ty, *ty) {
            return Err(mismatch());
        }
    }
    let result_ok = match intrinsic.result {
        Ty::b_obj => false,
        t => ty_matches(decl.result, t),
    };
    if !result_ok {
        return Err(mismatch());
    }
    Ok(())
}
