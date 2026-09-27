//! The C compiler layer: a mechanical translation of every Bridge IR declaration into C on the
//! lungo runtime's C library (`lungo.h`).
//!
//! The translation follows Lean's own C emitter:
//!
//! * every variable of a function is declared at its top, and statements assign them;
//! * a join point is a label (`block_<id>`) that jumps reach with `goto` after assigning its
//!   parameters, and whose body follows the code that jumps to it;
//! * a self tail call assigns the parameters and jumps back to `_start`;
//! * `case` is a `switch` whose last alternative is the default, as Lean's emitter does;
//! * nullary declarations are constants, evaluated once and persistent;
//! * reference counting, constructor layout, boxing and closures use the inline functions of
//!   `lungo.h`, primitives are the runtime's `lungo_<symbol>` functions.
//!
//! Every symbol is prefixed with the program's prefix, so that several generated programs link
//! into one process.

use super::syntax::{c_type, comment, string, word_offset};
use crate::core::externs::{ExternPlan, Implementation};
use crate::core::names::mangle;
use crate::core::writer::Writer;
use crate::{CodegenError, SourceIndex};
use lungo_bir::{Alt, Arg, Block, Body, CtorInfo, Declaration, Expr, IrType, Literal, Param, Stmt, Terminator};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Parameters that appear in the C signature: `void` (the `IO` world token) is omitted, as in
/// Lean's C emitter.
pub fn c_params(params: &[Param]) -> Vec<&Param> {
    params.iter().filter(|p| p.ty != IrType::Void).collect()
}

/// Whether the declaration takes its arguments as an array, as Lean's boxed variants of
/// functions with more than 16 parameters do.
pub fn takes_arg_array(decl: &Declaration) -> bool {
    decl.params.len() > 16 && decl.name.ends_with("._boxed")
}

/// Emits the C of every declaration of a program.
pub struct Emitter<'a> {
    /// The program's symbol prefix, including its trailing `_`.
    pub prefix: &'a str,
    pub decls: HashMap<&'a str, &'a Declaration>,
    pub externs: &'a ExternPlan,
    pub sources: &'a SourceIndex,
    /// Declarations whose value is set by a module initializer.
    pub init_values: HashMap<&'a str, &'a str>,
}

/// How a scalar value is stored in the 64-bit cells of constants and initialization cells.
fn to_bits(ty: IrType, value: &str) -> String {
    match ty {
        IrType::Float => format!("lungo_bits_of_double({value})"),
        IrType::Float32 => format!("lungo_bits_of_float({value})"),
        _ => format!("(uint64_t)({value})"),
    }
}

fn from_bits(ty: IrType, bits: &str) -> String {
    match ty {
        IrType::Float => format!("lungo_double_of_bits({bits})"),
        IrType::Float32 => format!("lungo_float_of_bits({bits})"),
        other => format!("({})({bits})", c_type(other)),
    }
}

impl<'a> Emitter<'a> {
    /// The C symbol of the Lean declaration `name`.
    pub fn symbol(&self, name: &str) -> String {
        format!("{}{}", self.prefix, mangle(name))
    }

    fn decl(&self, name: &str) -> Result<&'a Declaration, CodegenError> {
        self.decls
            .get(name)
            .copied()
            .ok_or_else(|| CodegenError::internal(format!("reference to unknown declaration {name:?}")))
    }

    /// The prototype of the C function of `decl` (without the trailing `;` or body).
    pub fn prototype(&self, decl: &Declaration) -> String {
        let m = self.symbol(&decl.name);
        let ret = c_type(decl.result);
        if self.init_values.contains_key(decl.name.as_str()) || (decl.params.is_empty() && !is_extern(decl)) {
            return format!("{ret} {m}(void)");
        }
        if takes_arg_array(decl) {
            return format!("{ret} {m}(lungo_obj *args)");
        }
        let params: Vec<String> = c_params(&decl.params).iter().map(|p| format!("{} x_{}", c_type(p.ty), p.var)).collect();
        if params.is_empty() { format!("{ret} {m}(void)") } else { format!("{ret} {m}({})", params.join(", ")) }
    }

    /// The declarations other files need: the prototype of every function and the
    /// initialization cells the program's initializer sets.
    pub fn declarations(&self, w: &mut Writer, decls: &[&'a Declaration]) {
        for decl in decls {
            if let Some(_init) = self.init_values.get(decl.name.as_str()) {
                let cell = if decl.result.is_scalar() { "lungo_init_bits" } else { "lungo_init_obj" };
                w.line(format!("extern {cell} {}I_{};", self.prefix, mangle(&decl.name)));
            }
            w.line(format!("{};", self.prototype(decl)));
        }
    }

    /// Emits the C items for `decl`.
    pub fn declaration(&self, w: &mut Writer, decl: &'a Declaration) -> Result<(), CodegenError> {
        let origin = decl.origin.as_deref().unwrap_or(&decl.name);
        w.line(comment(&format!("Lean: {}", decl.name)));
        if origin != decl.name {
            w.line(comment(&format!("Compiled from: {origin}")));
        }
        if let Some(src) = self.sources.describe(origin) {
            w.line(comment(&format!("Source: {src}")));
        }
        if let Some(init_fn) = self.init_values.get(decl.name.as_str()) {
            return self.init_value(w, decl, init_fn);
        }
        match &decl.body {
            Body::Extern { .. } => self.extern_wrapper(w, decl),
            Body::Function { block } => {
                if decl.params.is_empty() {
                    self.constant(w, decl, block)
                } else {
                    self.function(w, decl, block)
                }
            }
        }
    }

    fn init_value(&self, w: &mut Writer, decl: &Declaration, init_fn: &str) -> Result<(), CodegenError> {
        if !decl.params.is_empty() {
            return Err(CodegenError::adapter(&decl.name, format!("`[init {init_fn}]` declaration has parameters")));
        }
        let cell = format!("{}I_{}", self.prefix, mangle(&decl.name));
        let name = string(decl.name.as_bytes());
        w.open(format!("{} {{", self.prototype(decl)));
        if decl.result.is_scalar() {
            w.line(format!("return {};", from_bits(decl.result, &format!("lungo_init_bits_get(&{cell})"))));
        } else {
            w.line(format!("return lungo_init_obj_get(&{cell});"));
        }
        w.close("}");
        let kind = if decl.result.is_scalar() { "lungo_init_bits" } else { "lungo_init_obj" };
        w.line(format!("{kind} {cell} = {{0, 0, {name}}};"));
        w.line("");
        Ok(())
    }

    fn extern_wrapper(&self, w: &mut Writer, decl: &Declaration) -> Result<(), CodegenError> {
        let call = self.externs.call(decl, &|n| self.decls.get(n).copied())?;
        let args: Vec<String> = call.args.iter().map(|v| format!("x_{v}")).collect();
        let callee = match &call.implementation {
            Implementation::Lean(implementation) => self.symbol(implementation),
            Implementation::Intrinsic(intrinsic) => format!("lungo_{}", intrinsic.symbol),
            Implementation::Application => super::boundary::host_adapter_symbol(self.prefix, &decl.name),
        };
        w.open(format!("{} {{", self.prototype(decl)));
        // Erased parameters (proofs, types) are not passed to the implementation.
        for p in c_params(&decl.params) {
            if !call.args.contains(&p.var) {
                w.line(format!("(void)x_{};", p.var));
            }
        }
        for v in &call.retain {
            w.line(format!("lungo_inc(x_{v});"));
        }
        w.line(format!("{} r = {callee}({});", c_type(decl.result), args.join(", ")));
        for v in &call.release {
            w.line(format!("lungo_dec(x_{v});"));
        }
        w.line("return r;");
        w.close("}");
        w.line("");
        Ok(())
    }

    fn constant(&self, w: &mut Writer, decl: &'a Declaration, block: &'a Block) -> Result<(), CodegenError> {
        let m = self.symbol(&decl.name);
        let t = c_type(decl.result);
        w.open(format!("static {t} {m}__init(void) {{"));
        let mut f = FnCtx::new(self, decl, block, false)?;
        f.declare(w);
        f.block(w, block)?;
        w.close("}");
        if decl.result.is_scalar() {
            w.line(format!("static lungo_lazy_bits {m}__cell;"));
            w.open(format!("static uint64_t {m}__init_bits(void) {{"));
            w.line(format!("return {};", to_bits(decl.result, &format!("{m}__init()"))));
            w.close("}");
            w.open(format!("{} {{", self.prototype(decl)));
            w.line(format!(
                "return {};",
                from_bits(decl.result, &format!("lungo_lazy_bits_get(&{m}__cell, {m}__init_bits)"))
            ));
        } else {
            w.line(format!("static lungo_lazy_obj {m}__cell;"));
            w.open(format!("{} {{", self.prototype(decl)));
            w.line(format!("return lungo_lazy_obj_get(&{m}__cell, {m}__init);"));
        }
        w.close("}");
        w.line("");
        Ok(())
    }

    fn function(&self, w: &mut Writer, decl: &'a Declaration, block: &'a Block) -> Result<(), CodegenError> {
        let params = c_params(&decl.params);
        let array = takes_arg_array(decl);
        if array && params.len() != decl.params.len() {
            return Err(CodegenError::adapter(&decl.name, "boxed declaration with `void` parameters"));
        }
        let tail = has_self_tail_call(&decl.name, block);
        w.open(format!("{} {{", self.prototype(decl)));
        let mut f = FnCtx::new(self, decl, block, tail)?;
        let used = f.read.clone();
        if array {
            for (i, p) in params.iter().enumerate() {
                w.line(format!("lungo_obj x_{} = args[{i}];", p.var));
            }
        }
        // Lean's compiled code does not read every parameter (erased arguments, the extra
        // arguments of `_boxed` variants).
        for p in &params {
            if !used.contains(&p.var) {
                w.line(format!("(void)x_{};", p.var));
            }
        }
        f.declare(w);
        if tail {
            w.line("_start:");
        }
        f.block(w, block)?;
        w.close("}");
        w.line("");
        Ok(())
    }

    /// The C expression calling declaration `callee` with IR arguments `args`, or the
    /// statements doing so when the callee takes its arguments as an array.
    fn call(&self, callee: &Declaration, args: &[Arg], arg: &dyn Fn(&Arg) -> String) -> Result<Call, CodegenError> {
        if callee.params.len() != args.len() {
            return Err(CodegenError::internal(format!("{} applied to {} arguments", callee.name, args.len())));
        }
        let m = self.symbol(&callee.name);
        if takes_arg_array(callee) {
            let items: Vec<String> = args.iter().map(arg).collect();
            return Ok(Call::Array { function: m, args: items });
        }
        let items: Vec<String> =
            callee.params.iter().zip(args).filter(|(p, _)| p.ty != IrType::Void).map(|(_, a)| arg(a)).collect();
        Ok(Call::Direct(format!("{m}({})", items.join(", "))))
    }
}

enum Call {
    Direct(String),
    Array { function: String, args: Vec<String> },
}

fn is_extern(decl: &Declaration) -> bool {
    matches!(decl.body, Body::Extern { .. })
}

/// Whether `stmts[i]` is a call of the function `name` itself whose result the block returns:
/// a self tail call, compiled as a jump.
fn is_self_tail_call(name: &str, stmts: &[Stmt], i: usize, term: &Terminator) -> bool {
    matches!(
        &stmts[i],
        Stmt::Let { var, expr: Expr::Fap { function, .. }, .. }
            if function == name && i + 1 == stmts.len() && *term == (Terminator::Ret { arg: Arg::Var(*var) })
    )
}

fn has_self_tail_call(name: &str, block: &Block) -> bool {
    fn visit(name: &str, block: &Block) -> bool {
        for (i, s) in block.stmts.iter().enumerate() {
            if let Stmt::Join { body, .. } = s
                && visit(name, body)
            {
                return true;
            }
            if is_self_tail_call(name, &block.stmts, i, &block.terminator) {
                return true;
            }
        }
        if let Terminator::Case { alts, .. } = &block.terminator {
            return alts.iter().any(|a| visit(name, a.body()));
        }
        false
    }
    visit(name, block)
}

/// The variables the emitted code of `decl`'s `block` reads: arguments for `void` parameters
/// are not passed, and reference-count operations on persistent values are not emitted.
fn referenced_vars(decls: &HashMap<&str, &Declaration>, decl: &Declaration, block: &Block) -> BTreeSet<u32> {
    fn arg(out: &mut BTreeSet<u32>, a: &Arg) {
        if let Arg::Var(v) = a {
            out.insert(*v);
        }
    }
    /// The arguments of a call of `function` that the emitted call passes.
    fn passed<'x>(decls: &HashMap<&str, &Declaration>, function: &str, args: &'x [Arg]) -> Vec<&'x Arg> {
        match decls.get(function) {
            Some(callee) if !takes_arg_array(callee) => {
                callee.params.iter().zip(args).filter(|(p, _)| p.ty != IrType::Void).map(|(_, a)| a).collect()
            }
            _ => args.iter().collect(),
        }
    }
    fn visit(decls: &HashMap<&str, &Declaration>, decl: &Declaration, block: &Block, out: &mut BTreeSet<u32>) {
        for (i, s) in block.stmts.iter().enumerate() {
            match s {
                Stmt::Let { expr: Expr::Fap { args, .. }, .. }
                    if is_self_tail_call(&decl.name, &block.stmts, i, &block.terminator) =>
                {
                    passed(decls, &decl.name, args).into_iter().for_each(|a| arg(out, a))
                }
                Stmt::Let { expr, .. } => match expr {
                    Expr::Fap { function, args } => passed(decls, function, args).into_iter().for_each(|a| arg(out, a)),
                    Expr::Ctor { args, .. } | Expr::Pap { args, .. } => args.iter().for_each(|a| arg(out, a)),
                    Expr::Reuse { var, args, .. } | Expr::Ap { var, args } => {
                        out.insert(*var);
                        args.iter().for_each(|a| arg(out, a));
                    }
                    Expr::Reset { var, .. }
                    | Expr::Proj { var, .. }
                    | Expr::Uproj { var, .. }
                    | Expr::Sproj { var, .. }
                    | Expr::Box { var, .. }
                    | Expr::Unbox { var }
                    | Expr::IsShared { var } => {
                        out.insert(*var);
                    }
                    Expr::Lit(_) => {}
                },
                Stmt::Join { body, .. } => visit(decls, decl, body, out),
                Stmt::Set { var, arg: a, .. } => {
                    out.insert(*var);
                    arg(out, a);
                }
                Stmt::SetTag { var, .. } | Stmt::Del { var } => {
                    out.insert(*var);
                }
                Stmt::Uset { var, value, .. } | Stmt::Sset { var, value, .. } => {
                    out.insert(*var);
                    out.insert(*value);
                }
                Stmt::Inc { var, persistent, .. } | Stmt::Dec { var, persistent, .. } => {
                    if !persistent {
                        out.insert(*var);
                    }
                }
            }
        }
        match &block.terminator {
            Terminator::Case { var, alts, .. } => {
                out.insert(*var);
                for a in alts {
                    visit(decls, decl, a.body(), out);
                }
            }
            Terminator::Ret { arg: a } => arg(out, a),
            Terminator::Jmp { args, .. } => args.iter().for_each(|a| arg(out, a)),
            Terminator::Unreachable => {}
        }
    }
    let mut out = BTreeSet::new();
    visit(decls, decl, block, &mut out);
    out
}

/// The variables a function assigns (its `let` bindings, except the results of self tail calls,
/// and its join-point parameters) and their types. A variable bound twice with different types is invalid Bridge IR for this backend.
fn collect_vars(decl: &Declaration, block: &Block) -> Result<BTreeMap<u32, IrType>, CodegenError> {
    fn bind(decl: &Declaration, vars: &mut BTreeMap<u32, IrType>, var: u32, ty: IrType) -> Result<(), CodegenError> {
        match vars.insert(var, ty) {
            Some(prev) if prev != ty => Err(CodegenError::adapter(
                &decl.name,
                format!("variable x_{var} is bound at types {} and {}", prev.name(), ty.name()),
            )),
            _ => Ok(()),
        }
    }
    fn visit(decl: &Declaration, block: &Block, vars: &mut BTreeMap<u32, IrType>) -> Result<(), CodegenError> {
        for (i, s) in block.stmts.iter().enumerate() {
            match s {
                // The result of a self tail call is never assigned: the call is a jump.
                Stmt::Let { .. } if is_self_tail_call(&decl.name, &block.stmts, i, &block.terminator) => {}
                Stmt::Let { var, ty, .. } => bind(decl, vars, *var, *ty)?,
                Stmt::Join { params, body, .. } => {
                    for p in params {
                        bind(decl, vars, p.var, p.ty)?;
                    }
                    visit(decl, body, vars)?;
                }
                _ => {}
            }
        }
        if let Terminator::Case { alts, .. } = &block.terminator {
            for a in alts {
                visit(decl, a.body(), vars)?;
            }
        }
        Ok(())
    }
    let mut vars = BTreeMap::new();
    visit(decl, block, &mut vars)?;
    for p in &decl.params {
        if vars.contains_key(&p.var) {
            return Err(CodegenError::adapter(&decl.name, format!("parameter x_{} is rebound in the body", p.var)));
        }
    }
    Ok(vars)
}

struct FnCtx<'e, 'a> {
    e: &'e Emitter<'a>,
    decl: &'a Declaration,
    tail: bool,
    /// Variables the emitted code reads; a variable Lean's IR binds without reading (the result
    /// of an effect run for its effect) is marked used after its assignment.
    read: BTreeSet<u32>,
    /// Parameters of each join point in scope.
    joins: HashMap<u32, &'a [Param]>,
    vars: BTreeMap<u32, IrType>,
}

impl<'e, 'a> FnCtx<'e, 'a> {
    fn new(e: &'e Emitter<'a>, decl: &'a Declaration, block: &'a Block, tail: bool) -> Result<Self, CodegenError> {
        let read = referenced_vars(&e.decls, decl, block);
        Ok(FnCtx { e, decl, tail, read, joins: HashMap::new(), vars: collect_vars(decl, block)? })
    }

    /// Declares every variable the body assigns.
    fn declare(&self, w: &mut Writer) {
        for (var, ty) in &self.vars {
            w.line(format!("{} x_{var};", c_type(*ty)));
        }
    }

    fn arg(&self, a: &Arg) -> String {
        match a {
            Arg::Var(v) => format!("x_{v}"),
            Arg::Erased => "lungo_box(0)".to_owned(),
        }
    }

    fn block(&mut self, w: &mut Writer, block: &'a Block) -> Result<(), CodegenError> {
        self.stmts(w, &block.stmts, &block.terminator)
    }

    fn stmts(&mut self, w: &mut Writer, stmts: &'a [Stmt], term: &'a Terminator) -> Result<(), CodegenError> {
        for (i, stmt) in stmts.iter().enumerate() {
            match stmt {
                Stmt::Join { id, params, body } => {
                    // The code that may jump to the join point comes first; the join point's body
                    // follows its label. The code before the label always ends in a terminator,
                    // so control reaches the label only by a jump.
                    self.joins.insert(*id, params);
                    self.stmts(w, &stmts[i + 1..], term)?;
                    self.joins.remove(id);
                    w.line(format!("block_{id}:;"));
                    return self.block(w, body);
                }
                Stmt::Let { expr: Expr::Fap { args, .. }, .. }
                    if self.tail && is_self_tail_call(&self.decl.name, stmts, i, term) =>
                {
                    return self.tail_call(w, args);
                }
                Stmt::Let { var, ty, expr } => {
                    self.let_(w, *var, *ty, expr)?;
                    if !self.read.contains(var) {
                        w.line(format!("(void)x_{var};"));
                    }
                }
                Stmt::Set { var, index, arg } => w.line(format!("lungo_ctor_set(x_{var}, {index}, {});", self.arg(arg))),
                Stmt::SetTag { var, tag } => w.line(format!("lungo_ctor_set_tag(x_{var}, {tag});")),
                Stmt::Uset { var, index, value } => w.line(format!("lungo_ctor_set_usize(x_{var}, {index}, x_{value});")),
                Stmt::Sset { var, index, offset, value, ty } => {
                    let setter = scalar_accessor(*ty, "set")?;
                    w.line(format!("{setter}(x_{var}, {}, x_{value});", word_offset(*index, *offset)));
                }
                Stmt::Inc { var, count, checked, persistent } => {
                    if !persistent {
                        w.line(match (*count == 1, *checked) {
                            (true, true) => format!("lungo_inc(x_{var});"),
                            (true, false) => format!("lungo_inc_ref(x_{var});"),
                            (false, true) => format!("lungo_inc_n(x_{var}, {count});"),
                            (false, false) => format!("lungo_inc_ref_n(x_{var}, {count});"),
                        });
                    }
                }
                Stmt::Dec { var, count, checked, persistent } => {
                    if *count != 1 {
                        return Err(CodegenError::adapter(&self.decl.name, "decrement by more than one"));
                    }
                    if !persistent {
                        let f = if *checked { "lungo_dec" } else { "lungo_dec_ref" };
                        w.line(format!("{f}(x_{var});"));
                    }
                }
                Stmt::Del { var } => w.line(format!("lungo_del_object(x_{var});")),
            }
        }
        self.terminator(w, term)
    }

    fn tail_call(&mut self, w: &mut Writer, args: &[Arg]) -> Result<(), CodegenError> {
        let params = c_params(&self.decl.params);
        let args: Vec<&Arg> =
            self.decl.params.iter().zip(args).filter(|(p, _)| p.ty != IrType::Void).map(|(_, a)| a).collect();
        // Every argument is evaluated before any parameter is assigned.
        w.open("{");
        for (i, (p, a)) in params.iter().zip(&args).enumerate() {
            w.line(format!("{} t_{i} = {};", c_type(p.ty), self.arg(a)));
        }
        for (i, p) in params.iter().enumerate() {
            w.line(format!("x_{} = t_{i};", p.var));
        }
        w.line("goto _start;");
        w.close("}");
        Ok(())
    }

    fn let_(&mut self, w: &mut Writer, var: u32, ty: IrType, expr: &'a Expr) -> Result<(), CodegenError> {
        let x = format!("x_{var}");
        match expr {
            Expr::Ctor { info, args } => {
                if info.is_scalar() {
                    w.line(format!("{x} = lungo_box({});", info.tag));
                } else {
                    w.line(format!("{x} = {};", alloc_ctor(info)));
                    for (i, a) in args.iter().enumerate() {
                        w.line(format!("lungo_ctor_set({x}, {i}, {});", self.arg(a)));
                    }
                }
            }
            Expr::Reset { fields, var: y } => {
                w.open(format!("if (lungo_is_exclusive(x_{y})) {{"));
                for i in 0..*fields {
                    w.line(format!("lungo_ctor_release(x_{y}, {i});"));
                }
                w.line(format!("{x} = x_{y};"));
                w.dedent();
                w.open("} else {");
                w.line(format!("lungo_dec_ref(x_{y});"));
                w.line(format!("{x} = lungo_box(0);"));
                w.close("}");
            }
            Expr::Reuse { var: y, info, update_header, args } => {
                w.open(format!("if (lungo_is_scalar(x_{y})) {{"));
                w.line(format!("{x} = {};", alloc_ctor(info)));
                w.dedent();
                w.open("} else {");
                if *update_header {
                    w.line(format!("lungo_ctor_set_tag(x_{y}, {});", info.tag));
                }
                w.line(format!("{x} = x_{y};"));
                w.close("}");
                for (i, a) in args.iter().enumerate() {
                    w.line(format!("lungo_ctor_set({x}, {i}, {});", self.arg(a)));
                }
            }
            Expr::Proj { index, var: y } => w.line(format!("{x} = lungo_ctor_get(x_{y}, {index});")),
            Expr::Uproj { index, var: y } => w.line(format!("{x} = lungo_ctor_get_usize(x_{y}, {index});")),
            Expr::Sproj { fields, offset, var: y } => {
                let getter = scalar_accessor(ty, "get")?;
                w.line(format!("{x} = {getter}(x_{y}, {});", word_offset(*fields, *offset)));
            }
            Expr::Fap { function, args } => {
                let callee = self.e.decl(function)?;
                match self.e.call(callee, args, &|a| self.arg(a))? {
                    Call::Direct(call) => w.line(format!("{x} = {call};")),
                    Call::Array { function, args } => {
                        w.open("{");
                        w.line(format!("lungo_obj args[] = {{{}}};", args.join(", ")));
                        w.line(format!("{x} = {function}(args);"));
                        w.close("}");
                    }
                }
            }
            Expr::Pap { function, args } => {
                let callee = self.e.decl(function)?;
                if callee.params.len() > 16 && !takes_arg_array(callee) {
                    return Err(CodegenError::adapter(
                        &self.decl.name,
                        format!(
                            "closure over {function}, which has more than 16 parameters but is not a boxed variant"
                        ),
                    ));
                }
                if callee.params.iter().any(|p| p.ty == IrType::Void || p.ty.is_scalar()) {
                    return Err(CodegenError::adapter(
                        &self.decl.name,
                        format!("closure over {function}, whose parameters are not all boxed"),
                    ));
                }
                w.line(format!(
                    "{x} = lungo_alloc_closure((void *){}, {}, {});",
                    self.e.symbol(function),
                    callee.params.len(),
                    args.len()
                ));
                for (i, a) in args.iter().enumerate() {
                    w.line(format!("lungo_closure_set({x}, {i}, {});", self.arg(a)));
                }
            }
            Expr::Ap { var: f, args } => {
                let items: Vec<String> = args.iter().map(|a| self.arg(a)).collect();
                if args.len() <= 16 {
                    w.line(format!("{x} = lungo_apply_{}(x_{f}, {});", args.len(), items.join(", ")));
                } else {
                    w.open("{");
                    w.line(format!("lungo_obj args[] = {{{}}};", items.join(", ")));
                    w.line(format!("{x} = lungo_apply_m(x_{f}, {}, args);", args.len()));
                    w.close("}");
                }
            }
            Expr::Box { ty: boxed, var: y } => {
                let e = match boxed {
                    IrType::Usize => format!("lungo_box_usize(x_{y})"),
                    IrType::Uint32 => format!("lungo_box_uint32(x_{y})"),
                    IrType::Uint64 => format!("lungo_box_uint64(x_{y})"),
                    IrType::Float => format!("lungo_box_float(x_{y})"),
                    IrType::Float32 => format!("lungo_box_float32(x_{y})"),
                    IrType::Uint8 | IrType::Uint16 => format!("lungo_box((size_t)x_{y})"),
                    other => {
                        return Err(CodegenError::adapter(
                            &self.decl.name,
                            format!("box of non-scalar type {}", other.name()),
                        ));
                    }
                };
                w.line(format!("{x} = {e};"));
            }
            Expr::Unbox { var: y } => {
                let e = match ty {
                    IrType::Usize => format!("lungo_unbox_usize(x_{y})"),
                    IrType::Uint32 => format!("lungo_unbox_uint32(x_{y})"),
                    IrType::Uint64 => format!("lungo_unbox_uint64(x_{y})"),
                    IrType::Float => format!("lungo_unbox_float(x_{y})"),
                    IrType::Float32 => format!("lungo_unbox_float32(x_{y})"),
                    IrType::Uint8 | IrType::Uint16 => format!("({})lungo_unbox(x_{y})", c_type(ty)),
                    other => {
                        return Err(CodegenError::adapter(
                            &self.decl.name,
                            format!("unbox into non-scalar type {}", other.name()),
                        ));
                    }
                };
                w.line(format!("{x} = {e};"));
            }
            Expr::Lit(Literal::Num(n)) => {
                if !n.bytes().all(|b| b.is_ascii_digit()) || n.is_empty() {
                    return Err(CodegenError::adapter(&self.decl.name, format!("numeric literal {n:?} is not a numeral")));
                }
                if ty.is_scalar() {
                    let value = match ty {
                        IrType::Float | IrType::Float32 => {
                            return Err(CodegenError::adapter(
                                &self.decl.name,
                                "numeric literal of floating-point type",
                            ));
                        }
                        IrType::Uint64 | IrType::Usize => format!("({})UINT64_C({n})", c_type(ty)),
                        _ => format!("({}){n}u", c_type(ty)),
                    };
                    w.line(format!("{x} = {value};"));
                } else {
                    let small = n.len() < 10 || n.parse::<u64>().is_ok_and(|v| v < (1u64 << 32));
                    if small {
                        w.line(format!("{x} = lungo_usize_to_nat({n}u);"));
                    } else {
                        w.line(format!("{x} = lungo_cstr_to_nat({});", string(n.as_bytes())));
                    }
                }
            }
            Expr::Lit(Literal::Str(s)) => {
                w.line(format!(
                    "{x} = lungo_mk_string_unchecked((const uint8_t *){}, {}, {});",
                    string(s.as_bytes()),
                    s.len(),
                    s.chars().count()
                ));
            }
            Expr::IsShared { var: y } => w.line(format!("{x} = !lungo_is_exclusive(x_{y});")),
        }
        Ok(())
    }

    fn terminator(&mut self, w: &mut Writer, term: &'a Terminator) -> Result<(), CodegenError> {
        match term {
            Terminator::Ret { arg } => w.line(format!("return {};", self.arg(arg))),
            Terminator::Jmp { id, args } => {
                let params = self.joins.get(id).copied().ok_or_else(|| {
                    CodegenError::internal(format!("jump to block_{id} outside its scope in {}", self.decl.name))
                })?;
                if params.len() != args.len() {
                    return Err(CodegenError::internal(format!(
                        "jump to block_{id} with {} arguments for {} parameters in {}",
                        args.len(),
                        params.len(),
                        self.decl.name
                    )));
                }
                for (p, a) in params.iter().zip(args) {
                    w.line(format!("x_{} = {};", p.var, self.arg(a)));
                }
                w.line(format!("goto block_{id};"));
            }
            Terminator::Unreachable => w.line("lungo_panic_unreachable();"),
            Terminator::Case { var, var_ty, alts, .. } => {
                if alts.is_empty() {
                    return Err(CodegenError::internal(format!("case without alternatives in {}", self.decl.name)));
                }
                let scrutinee = if var_ty.is_scalar() { format!("x_{var}") } else { format!("lungo_obj_tag(x_{var})") };
                w.open(format!("switch ({scrutinee}) {{"));
                // As Lean's emitter does, the last alternative covers every remaining tag when no
                // default alternative is present.
                let last = alts.len() - 1;
                for (i, alt) in alts.iter().enumerate() {
                    match alt {
                        Alt::Ctor { info, .. } if i != last => w.open(format!("case {}: {{", info.tag)),
                        _ => w.open("default: {"),
                    }
                    self.block(w, alt.body())?;
                    w.close("}");
                }
                w.close("}");
            }
        }
        Ok(())
    }
}

fn alloc_ctor(info: &CtorInfo) -> String {
    format!("lungo_alloc_ctor({}, {}, {})", info.tag, info.size, word_offset(info.usize, info.ssize))
}

fn scalar_accessor(ty: IrType, op: &str) -> Result<String, CodegenError> {
    let kind = match ty {
        IrType::Uint8 => "uint8",
        IrType::Uint16 => "uint16",
        IrType::Uint32 => "uint32",
        IrType::Uint64 => "uint64",
        IrType::Float => "float",
        IrType::Float32 => "float32",
        other => {
            return Err(CodegenError::internal(format!("scalar field access at type {}", other.name())));
        }
    };
    Ok(format!("lungo_ctor_{op}_{kind}"))
}
