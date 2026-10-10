//! Resolution of Lean `@[extern]` declarations, shared by every backend.
//!
//! Externs resolve, in order, to Lean definitions exported with `@[export]`, to runtime
//! primitives implemented by `lungo-runtime`, and — for the operations of facilities
//! (`@[lungo_operation]`), and only for them — to implementations the application provides (a
//! Rust function for the Rust backend, a host function of the target language otherwise). An
//! extern that resolves to none of them is a build error naming the Lean declaration, the symbol,
//! its Lean type, the runtime representation it must have, and its source location; it is never
//! replaced or ignored. An operation is the host's by declaration: one that Lean or the runtime
//! also implements is an error, as is an application implementation of an extern that is not an
//! operation.
//!
//! Runtime primitives are checked against the representation Lean's compiler expects. Their
//! ownership conventions are reconciled explicitly: when Lean passes an owned argument to a
//! primitive that borrows it, the caller releases it after the call, and when Lean passes a
//! borrowed argument to a primitive that consumes it, the caller retains it first.

use crate::CodegenError;
use crate::core::codes::ErrorCode;
use lungo_bir::{Body, Declaration, ExternEntry, IrType, Param};
use lungo_protocol::ExternRequirement;
use lungo_runtime::registry::{self, Intrinsic, Ty};
use std::collections::{BTreeMap, HashMap};

/// How an extern declaration is implemented.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// Compiled Lean code that provides the symbol with `@[export]`.
    Lean {
        implementation: String,
    },
    Intrinsic(&'static Intrinsic),
    /// An implementation the application provides, reached through a generated adapter.
    Application {
        key: String,
        /// How the application provides it: a Rust path for the Rust backend, the host symbol
        /// otherwise.
        implementation: String,
    },
}

/// How the application provides the operations of facilities to a backend, for resolution and
/// its diagnostics.
pub struct ApplicationExterns<'a> {
    /// Operation key → the application's implementation.
    pub implementations: &'a BTreeMap<String, String>,
    /// The setting that provides them, named in diagnostics (`Builder::rust_extern`).
    pub setting: &'a str,
    /// How to provide an operation with key `key` that has no implementation, appended to its
    /// diagnostic.
    pub hint: &'a dyn Fn(&str) -> String,
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

/// The extern keys of the operations of facilities among `decls` (every extern declaration
/// whose requirement names a facility), each mapped to itself: what the host implements, by the
/// key it is registered under.
pub fn operation_keys(
    decls: &[Declaration],
    requirements: &[ExternRequirement],
) -> Result<BTreeMap<String, String>, CodegenError> {
    let operations: std::collections::BTreeSet<&str> =
        requirements.iter().filter(|r| r.operation.is_some()).map(|r| r.declaration.as_str()).collect();
    let mut keys = BTreeMap::new();
    for d in decls.iter().filter(|d| operations.contains(d.name.as_str())) {
        let key = resolution_key(d)?;
        keys.insert(key.clone(), key);
    }
    Ok(keys)
}

/// What implements an extern call.
#[derive(Debug, Clone)]
pub enum Implementation {
    /// The compiled Lean definition with this name.
    Lean(String),
    Intrinsic(&'static Intrinsic),
    /// The application's implementation, through the backend's adapter for the extern
    /// declaration.
    Application,
}

/// How the wrapper of an extern declaration calls its implementation. Arguments are the IR
/// variables of the wrapper's parameters; ownership differences between Lean's convention and
/// the implementation's are reconciled by retaining and releasing object arguments.
#[derive(Debug, Clone)]
pub struct ExternCall {
    pub implementation: Implementation,
    pub args: Vec<u32>,
    /// Object arguments Lean lends that the implementation consumes: retained before the call.
    pub retain: Vec<u32>,
    /// Object arguments Lean passes owned that the implementation borrows: released after it.
    pub release: Vec<u32>,
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
        application: &ApplicationExterns,
        source: &dyn Fn(&ExternRequirement) -> Option<String>,
    ) -> Result<ExternPlan, Vec<CodegenError>> {
        let user = application.implementations;
        let reqs: HashMap<&str, &ExternRequirement> =
            requirements.iter().map(|r| (r.declaration.as_str(), r)).collect();
        let mut plan = ExternPlan::default();
        let mut errors = Vec::new();
        // Mappings already reported as naming an extern that is not an operation.
        let mut misplaced: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
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
            if let Some(facility) = req.and_then(|r| r.operation.as_ref()).map(|o| &o.facility) {
                // An operation of a facility: the host's, and only the host's.
                let provider = match (&decl.body, intrinsic) {
                    (Body::Extern { exported_by: Some(implementation), .. }, _) => {
                        Some(format!("the Lean definition {implementation} (via @[export])"))
                    }
                    (_, Some(_)) => Some("the lungo runtime".to_owned()),
                    _ => None,
                };
                if let Some(provider) = provider {
                    errors.push(CodegenError::external(ErrorCode::FacilityMismatch, format!(
                        "`{}` is an operation of the facility {facility}, which the host implements, but \
                         {provider} already implements the symbol `{key}`; give the operation a symbol of its own\n\n{}",
                        decl.name,
                        describe(req, decl, &key, source)
                    )));
                    continue;
                }
                match user.get(&key) {
                    Some(implementation) => {
                        plan.resolutions.insert(
                            decl.name.clone(),
                            Resolution::Application { key: key.clone(), implementation: implementation.clone() },
                        );
                    }
                    None => errors.push(CodegenError::external(
                        ErrorCode::UnresolvedExtern,
                        format!(
                            "the operation `{}` of the facility {facility} has no implementation\n\n{}\n{}",
                            decl.name,
                            describe(req, decl, &key, source),
                            (application.hint)(&key)
                        ),
                    )),
                }
                continue;
            }
            if let Body::Extern { exported_by: Some(implementation), .. } = &decl.body {
                if intrinsic.is_some() || user.contains_key(&key) {
                    errors.push(CodegenError::external(ErrorCode::ConflictingExtern, format!(
                        "the symbol `{key}` is provided both by the Lean definition {implementation} (via @[export]) and by {}\n\n{}",
                        if intrinsic.is_some() { "the lungo runtime".to_owned() } else { format!("a {} mapping", application.setting) },
                        describe(req, decl, &key, source)
                    )));
                } else {
                    plan.resolutions
                        .insert(decl.name.clone(), Resolution::Lean { implementation: implementation.clone() });
                }
                continue;
            }
            match (intrinsic, user.get(&key)) {
                (Some(_), Some(path)) => errors.push(CodegenError::external(ErrorCode::RuntimeExternRemapped, format!(
                    "the Lean runtime symbol `{key}` is implemented by lungo and cannot be remapped to `{path}`\n\n{}",
                    describe(req, decl, &key, source)
                ))),
                (Some(intrinsic), None) => match check_signature(decl, intrinsic) {
                    Ok(()) => {
                        plan.resolutions.insert(decl.name.clone(), Resolution::Intrinsic(intrinsic));
                    }
                    Err(msg) => errors.push(CodegenError::external(ErrorCode::ExternRepresentation, format!(
                        "lungo runtime primitive `{key}` does not match the representation Lean's compiler expects: {msg}\n\n{}",
                        describe(req, decl, &key, source)
                    ))),
                },
                (None, Some(path)) => {
                    misplaced.insert(key.clone());
                    errors.push(CodegenError::external(ErrorCode::FacilityMismatch, format!(
                    "{} maps `{key}` to `{path}`, but `{}` is not an operation of a facility; the host implements \
                     only operations: mark the declaration `@[lungo_operation C]` with a facility `C`\n\n{}",
                    application.setting,
                    decl.name,
                    describe(req, decl, &key, source)
                    )));
                }
                (None, None) => {
                    let reason = match registry::unsupported(&key) {
                        Some(u) => format!(
                            "\nThe lungo PureRust runtime does not provide this Lean runtime primitive: {}\n",
                            u.reason
                        ),
                        None => String::new(),
                    };
                    errors.push(CodegenError::external(ErrorCode::UnresolvedExtern, format!(
                        "unresolved Lean external symbol\n\n{}{reason}\nNothing implements it. If the host is to \
                         implement it, make it an operation of a facility: give it `@[lungo_operation C]`, `C` \
                         being a declaration with `@[lungo_facility \"ns.name\"]` (lungo's Lean library).",
                        describe(req, decl, &key, source),
                    )));
                }
            }
        }
        for key in user.keys() {
            if misplaced.contains(key) {
                continue;
            }
            let used =
                plan.resolutions.values().any(|r| matches!(r, Resolution::Application { key: k, .. } if k == key));
            if !used {
                errors.push(CodegenError::external(ErrorCode::UnusedExternMapping, format!(
                    "{} provides `{key}`, but no extern declaration reachable from the root modules uses that symbol",
                    application.setting
                )));
            }
        }
        if errors.is_empty() { Ok(plan) } else { Err(errors) }
    }

    /// How the wrapper of extern `decl`, which receives the declaration's implementation
    /// parameters, calls its implementation.
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
        let args: Vec<u32> = params.iter().map(|p| p.var).collect();
        let mut retain = Vec::new();
        let mut release = Vec::new();
        let implementation = match resolution {
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
                for (p, q) in params.iter().zip(&callee_params) {
                    if p.ty.is_object() {
                        match (p.borrow, q.borrow) {
                            (false, true) => release.push(p.var),
                            (true, false) => retain.push(p.var),
                            _ => {}
                        }
                    }
                }
                Implementation::Lean(implementation.clone())
            }
            Resolution::Intrinsic(intrinsic) => {
                for (p, ty) in params.iter().zip(intrinsic.params) {
                    if p.ty.is_object() {
                        match (p.borrow, ty) {
                            (false, Ty::b_obj) => release.push(p.var),
                            (true, Ty::obj) => retain.push(p.var),
                            _ => {}
                        }
                    }
                }
                Implementation::Intrinsic(intrinsic)
            }
            Resolution::Application { .. } => Implementation::Application,
        };
        Ok(ExternCall { implementation, args, retain, release })
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
