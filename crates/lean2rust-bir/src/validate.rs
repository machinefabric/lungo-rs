//! An independent verifier for BIR invariants.
//!
//! Generated Rust is only accepted for programs that pass this verifier. It checks the
//! structural and typing invariants the Rust backend relies on, independently of the worker
//! that produced the program: scoping of variables and join points, arities of calls, jumps and
//! closures, representation compatibility of every argument, literal ranges, and closure
//! completeness of the declaration set.

use crate::{
    Alt, Arg, BIR_VERSION, Block, Body, Declaration, Expr, Initializer, IrType, JoinId, Literal, Param, Program, Stmt,
    Terminator, VarId,
};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub declaration: Option<String>,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.declaration {
            Some(d) => write!(f, "{d}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Verifies `program`, returning every violation found.
pub fn validate(program: &Program) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut global = |message: String| errors.push(ValidationError { declaration: None, message });
    if program.bir_version != BIR_VERSION {
        global(format!("BIR version {} is incompatible with {BIR_VERSION}", program.bir_version));
    }
    let mut module_names = HashSet::new();
    for module in &program.modules {
        if module.name.is_empty() || !module_names.insert(module.name.as_str()) {
            global(format!("module name {:?} is empty or duplicated", module.name));
        }
    }
    for pair in program.declarations.windows(2) {
        if pair[0].name >= pair[1].name {
            global(format!("declarations are not strictly sorted: {:?} precedes {:?}", pair[0].name, pair[1].name));
        }
    }
    let signatures: HashMap<&str, &Declaration> = program.declarations.iter().map(|d| (d.name.as_str(), d)).collect();
    for module in &program.modules {
        for init in &module.initializers {
            let names: Vec<&str> = match init {
                Initializer::Io(d) => vec![d],
                Initializer::Value { decl, init_fn } => vec![decl, init_fn],
            };
            for n in names {
                if !signatures.contains_key(n) {
                    global(format!("module {} initializer references missing declaration {n:?}", module.name));
                }
            }
        }
    }
    for decl in &program.declarations {
        let mut checker = Checker {
            signatures: &signatures,
            decl,
            errors: &mut errors,
            vars: HashMap::new(),
            joins: HashMap::new(),
            defined: HashSet::new(),
        };
        checker.check_declaration(&module_names);
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

struct Checker<'a> {
    signatures: &'a HashMap<&'a str, &'a Declaration>,
    decl: &'a Declaration,
    errors: &'a mut Vec<ValidationError>,
    vars: HashMap<VarId, IrType>,
    joins: HashMap<JoinId, &'a [Param]>,
    /// Every identifier bound anywhere in the declaration (variables and join points share one
    /// namespace in Lean's compiler output).
    defined: HashSet<u32>,
}

fn is_canonical_decimal(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

fn scalar_max(ty: IrType) -> Option<u128> {
    Some(match ty {
        IrType::Uint8 => u8::MAX as u128,
        IrType::Uint16 => u16::MAX as u128,
        IrType::Uint32 => u32::MAX as u128,
        IrType::Uint64 | IrType::Usize => u64::MAX as u128,
        _ => return None,
    })
}

impl<'a> Checker<'a> {
    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(ValidationError { declaration: Some(self.decl.name.clone()), message: message.into() });
    }

    fn check_declaration(&mut self, modules: &HashSet<&str>) {
        let decl = self.decl;
        if decl.name.is_empty() {
            self.error("declaration name is empty");
        }
        if !modules.contains(decl.module.as_str()) {
            self.error(format!("owning module {:?} is not part of the program", decl.module));
        }
        for p in &decl.params {
            self.bind(p.var, p.ty);
        }
        match &decl.body {
            Body::Function { block } => {
                let saved_vars = self.vars.clone();
                self.check_block(block);
                self.vars = saved_vars;
            }
            Body::Extern { entries, selected, exported_by } => {
                if !entries.contains(selected) {
                    self.error("the selected extern entry is not one of the declaration's entries");
                }
                if let Some(implementation) = exported_by {
                    match self.signatures.get(implementation.as_str()) {
                        Some(d) if matches!(d.body, Body::Function { .. }) => {}
                        _ => self.error(format!(
                            "the @[export] implementation {implementation:?} is not a compiled function in the program"
                        )),
                    }
                }
            }
        }
    }

    fn bind(&mut self, var: VarId, ty: IrType) {
        if !self.defined.insert(var) {
            self.error(format!("identifier x_{var} is bound more than once"));
        }
        self.vars.insert(var, ty);
    }

    fn var_type(&mut self, var: VarId) -> Option<IrType> {
        let ty = self.vars.get(&var).copied();
        if ty.is_none() {
            self.error(format!("variable x_{var} is used outside its scope"));
        }
        ty
    }

    fn require_object(&mut self, var: VarId, what: &str) {
        if let Some(ty) = self.var_type(var)
            && !ty.is_object()
        {
            self.error(format!("{what} requires an object, but x_{var} has type {}", ty.name()));
        }
    }

    /// Checks that `arg` can be passed where a value of type `expected` is required.
    fn check_arg(&mut self, arg: Arg, expected: IrType, what: &str) {
        match arg {
            Arg::Erased => {
                if expected.is_scalar() {
                    self.error(format!("{what}: an erased argument cannot be passed as {}", expected.name()));
                }
            }
            Arg::Var(v) => {
                if let Some(actual) = self.var_type(v) {
                    let compatible = if expected.is_scalar() || actual.is_scalar() { actual == expected } else { true };
                    if !compatible {
                        self.error(format!("{what}: x_{v} of type {} is passed as {}", actual.name(), expected.name()));
                    }
                }
            }
        }
    }

    fn check_block(&mut self, block: &'a Block) {
        let mut bound_vars = Vec::new();
        let mut bound_joins = Vec::new();
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let { var, ty, expr } => {
                    self.check_expr(*ty, expr);
                    self.bind(*var, *ty);
                    bound_vars.push(*var);
                }
                Stmt::Join { id, params, body } => {
                    if !self.defined.insert(*id) {
                        self.error(format!("identifier block_{id} is bound more than once"));
                    }
                    let saved = self.vars.clone();
                    for p in params {
                        self.bind(p.var, p.ty);
                    }
                    self.check_block(body);
                    self.vars = saved;
                    self.joins.insert(*id, params);
                    bound_joins.push(*id);
                }
                Stmt::Set { var, arg, .. } => {
                    self.require_object(*var, "set");
                    self.check_arg(*arg, IrType::Object, "set");
                }
                Stmt::SetTag { var, tag } => {
                    self.require_object(*var, "setTag");
                    if *tag > 243 {
                        self.error(format!("constructor tag {tag} exceeds the maximum 243"));
                    }
                }
                Stmt::Uset { var, value, .. } => {
                    self.require_object(*var, "uset");
                    self.check_arg(Arg::Var(*value), IrType::Usize, "uset");
                }
                Stmt::Sset { var, value, ty, .. } => {
                    self.require_object(*var, "sset");
                    if !ty.is_scalar() || *ty == IrType::Usize {
                        self.error(format!("sset stores a non-scalar type {}", ty.name()));
                    }
                    self.check_arg(Arg::Var(*value), *ty, "sset");
                }
                Stmt::Inc { var, count, .. } => {
                    self.require_object(*var, "inc");
                    if *count == 0 {
                        self.error("inc by zero");
                    }
                }
                Stmt::Dec { var, count, .. } => {
                    self.require_object(*var, "dec");
                    if *count != 1 {
                        self.error(format!("dec by {count}; Lean's compiler only decrements by one"));
                    }
                }
                Stmt::Del { var } => self.require_object(*var, "del"),
            }
        }
        self.check_terminator(&block.terminator);
        for v in bound_vars {
            self.vars.remove(&v);
        }
        for j in bound_joins {
            self.joins.remove(&j);
        }
    }

    fn check_terminator(&mut self, term: &'a Terminator) {
        match term {
            Terminator::Case { var, var_ty, alts, .. } => {
                if let Some(actual) = self.var_type(*var) {
                    let ok = if var_ty.is_scalar() || actual.is_scalar() { actual == *var_ty } else { true };
                    if !ok {
                        self.error(format!("case on x_{var} of type {} declared as {}", actual.name(), var_ty.name()));
                    }
                    if !(actual.is_object() || matches!(actual, IrType::Uint8 | IrType::Uint16 | IrType::Uint32)) {
                        self.error(format!("case on a value of type {}", actual.name()));
                    }
                }
                if alts.is_empty() {
                    self.error("case without alternatives");
                }
                let mut tags = HashSet::new();
                for (i, alt) in alts.iter().enumerate() {
                    match alt {
                        Alt::Ctor { info, .. } => {
                            if !tags.insert(info.tag) {
                                self.error(format!("case has two alternatives for tag {}", info.tag));
                            }
                        }
                        Alt::Default { .. } => {
                            if i + 1 != alts.len() {
                                self.error("default alternative is not the last alternative");
                            }
                        }
                    }
                    let saved = self.vars.clone();
                    self.check_block(alt.body());
                    self.vars = saved;
                }
            }
            Terminator::Ret { arg } => {
                let result = self.decl.result;
                self.check_arg(*arg, result, "return");
            }
            Terminator::Jmp { id, args } => match self.joins.get(id).copied() {
                None => self.error(format!("jump to block_{id}, which is not in scope")),
                Some(params) => {
                    if params.len() != args.len() {
                        self.error(format!(
                            "jump to block_{id} passes {} arguments for {} parameters",
                            args.len(),
                            params.len()
                        ));
                    }
                    for (p, a) in params.iter().zip(args) {
                        self.check_arg(*a, p.ty, "jump argument");
                    }
                }
            },
            Terminator::Unreachable => {}
        }
    }

    fn check_expr(&mut self, ty: IrType, expr: &Expr) {
        let object_result = |c: &mut Self, what: &str| {
            if !ty.is_object() {
                c.error(format!("{what} produces an object but is bound as {}", ty.name()));
            }
        };
        match expr {
            Expr::Ctor { info, args } => {
                object_result(self, "ctor");
                if info.tag > 243 {
                    self.error(format!("constructor tag {} exceeds the maximum 243", info.tag));
                }
                if args.len() != info.size as usize {
                    self.error(format!(
                        "ctor {} takes {} object fields but is given {}",
                        info.name,
                        info.size,
                        args.len()
                    ));
                }
                for a in args {
                    self.check_arg(*a, IrType::Object, "ctor field");
                }
            }
            Expr::Reset { var, .. } => {
                object_result(self, "reset");
                self.require_object(*var, "reset");
            }
            Expr::Reuse { var, info, args, .. } => {
                object_result(self, "reuse");
                self.require_object(*var, "reuse");
                if args.len() != info.size as usize {
                    self.error(format!("reuse for {} gives {} fields for {}", info.name, args.len(), info.size));
                }
                for a in args {
                    self.check_arg(*a, IrType::Object, "reuse field");
                }
            }
            Expr::Proj { var, .. } => {
                object_result(self, "proj");
                self.require_object(*var, "proj");
            }
            Expr::Uproj { var, .. } => {
                if ty != IrType::Usize {
                    self.error(format!("uproj bound as {}", ty.name()));
                }
                self.require_object(*var, "uproj");
            }
            Expr::Sproj { var, .. } => {
                if !ty.is_scalar() || ty == IrType::Usize {
                    self.error(format!("sproj bound as {}", ty.name()));
                }
                self.require_object(*var, "sproj");
            }
            Expr::Fap { function, args } => match self.signatures.get(function.as_str()).copied() {
                None => self.error(format!("call to unknown declaration {function:?}")),
                Some(callee) => {
                    if callee.params.len() != args.len() {
                        self.error(format!(
                            "{function} takes {} arguments but is applied to {}",
                            callee.params.len(),
                            args.len()
                        ));
                    }
                    for (p, a) in callee.params.iter().zip(args) {
                        self.check_arg(*a, p.ty, "call argument");
                    }
                    let ok = if ty.is_scalar() || callee.result.is_scalar() { ty == callee.result } else { true };
                    if !ok {
                        self.error(format!(
                            "{function} returns {} but its result is bound as {}",
                            callee.result.name(),
                            ty.name()
                        ));
                    }
                }
            },
            Expr::Pap { function, args } => {
                object_result(self, "pap");
                match self.signatures.get(function.as_str()).copied() {
                    None => self.error(format!("closure over unknown declaration {function:?}")),
                    Some(callee) => {
                        if args.len() >= callee.params.len() {
                            self.error(format!(
                                "closure over {function} fixes {} of {} arguments",
                                args.len(),
                                callee.params.len()
                            ));
                        }
                        if callee.params.len() > u16::MAX as usize {
                            self.error(format!("closure over {function} has too many parameters"));
                        }
                        if callee.params.iter().any(|p| p.ty.is_scalar()) {
                            self.error(format!("closure over {function}, which takes unboxed parameters"));
                        }
                        if callee.result.is_scalar() {
                            self.error(format!("closure over {function}, which returns an unboxed value"));
                        }
                        for a in args {
                            self.check_arg(*a, IrType::Object, "closure argument");
                        }
                    }
                }
            }
            Expr::Ap { var, args } => {
                object_result(self, "application");
                self.require_object(*var, "application");
                if args.is_empty() {
                    self.error("closure application without arguments");
                }
                for a in args {
                    self.check_arg(*a, IrType::Object, "application argument");
                }
            }
            Expr::Box { ty: boxed, var } => {
                object_result(self, "box");
                if !boxed.is_scalar() {
                    self.error(format!("box of non-scalar type {}", boxed.name()));
                }
                self.check_arg(Arg::Var(*var), *boxed, "box");
            }
            Expr::Unbox { var } => {
                if !ty.is_scalar() {
                    self.error(format!("unbox bound as {}", ty.name()));
                }
                self.require_object(*var, "unbox");
            }
            Expr::Lit(Literal::Num(n)) => {
                if !is_canonical_decimal(n) {
                    self.error(format!("numeric literal {n:?} is not a canonical decimal"));
                } else if let Some(max) = scalar_max(ty) {
                    match n.parse::<u128>() {
                        Ok(v) if v <= max => {}
                        _ => self.error(format!("literal {n} does not fit {}", ty.name())),
                    }
                } else if !ty.is_object() {
                    self.error(format!("numeric literal bound as {}", ty.name()));
                }
            }
            Expr::Lit(Literal::Str(_)) => object_result(self, "string literal"),
            Expr::IsShared { var } => {
                if ty != IrType::Uint8 {
                    self.error(format!("isShared bound as {}", ty.name()));
                }
                self.require_object(*var, "isShared");
            }
        }
    }
}
