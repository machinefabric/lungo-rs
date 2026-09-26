//! The compiler layer: a mechanical translation of every BIR declaration into Rust.
//!
//! The translation mirrors Lean's own C emitter instruction by instruction:
//!
//! * variables become `let` bindings of their runtime representation (`Obj` or a scalar);
//! * a join point becomes a labeled block: the code that may jump to it runs inside the block,
//!   a jump assigns the join point's parameters and breaks out, and the join point's body
//!   follows the block;
//! * a self tail call reassigns the parameters and restarts the function's loop, as Lean's
//!   `goto _start` does;
//! * reference counting, constructor layout, boxing, and closure operations call the runtime
//!   primitives with the same names as `lean.h`;
//! * nullary declarations are lazily initialized persistent constants.

use crate::externs::ExternPlan;
use crate::names::mangle;
use crate::rust::{Writer, byte_string, string};
use crate::{CodegenError, SourceIndex};
use patina_bir::{Alt, Arg, Block, Body, CtorInfo, Declaration, Expr, IrType, Literal, Param, Stmt, Terminator};
use std::collections::HashMap;

pub const RT: &str = "rt";

/// The Rust type representing values of IR type `ty`.
pub fn rust_type(ty: IrType) -> &'static str {
    match ty {
        IrType::Float => "f64",
        IrType::Float32 => "f32",
        IrType::Uint8 => "u8",
        IrType::Uint16 => "u16",
        IrType::Uint32 => "u32",
        IrType::Uint64 => "u64",
        IrType::Usize => "usize",
        IrType::Erased | IrType::Object | IrType::Tobject | IrType::Tagged | IrType::Void => "Obj",
    }
}

/// Parameters that appear in the Rust signature: `void` (the `IO` world token) is omitted, as
/// in Lean's C emitter.
pub fn rust_params(params: &[Param]) -> Vec<&Param> {
    params.iter().filter(|p| p.ty != IrType::Void).collect()
}

/// Whether the declaration takes its arguments as an array, as Lean's boxed variants of
/// functions with more than 16 parameters do.
pub fn takes_arg_array(decl: &Declaration) -> bool {
    decl.params.len() > 16 && decl.name.ends_with("._boxed")
}

pub struct Emitter<'a> {
    pub decls: HashMap<&'a str, &'a Declaration>,
    pub externs: &'a ExternPlan,
    pub sources: &'a SourceIndex,
    /// Declarations whose value is set by a module initializer.
    pub init_values: HashMap<&'a str, &'a str>,
}

impl<'a> Emitter<'a> {
    fn decl(&self, name: &str) -> Result<&'a Declaration, CodegenError> {
        self.decls
            .get(name)
            .copied()
            .ok_or_else(|| CodegenError::internal(format!("reference to unknown declaration {name:?}")))
    }

    /// Emits the Rust items for `decl`.
    pub fn declaration(&self, w: &mut Writer, decl: &'a Declaration) -> Result<(), CodegenError> {
        let m = mangle(&decl.name);
        let origin = decl.origin.as_deref().unwrap_or(&decl.name);
        w.line(format!("// Lean: {}", decl.name));
        if origin != decl.name {
            w.line(format!("// Compiled from: {origin}"));
        }
        if let Some(src) = self.sources.describe(origin) {
            w.line(format!("// Source: {src}"));
        }
        if let Some(init_fn) = self.init_values.get(decl.name.as_str()) {
            return self.init_value(w, decl, &m, init_fn);
        }
        match &decl.body {
            Body::Extern { .. } => self.extern_wrapper(w, decl, &m),
            Body::Function { block } => {
                if decl.params.is_empty() {
                    self.constant(w, decl, &m, block)
                } else {
                    self.function(w, decl, &m, block)
                }
            }
        }
    }

    fn init_value(&self, w: &mut Writer, decl: &Declaration, m: &str, init_fn: &str) -> Result<(), CodegenError> {
        if !decl.params.is_empty() {
            return Err(CodegenError::adapter(&decl.name, format!("`[init {init_fn}]` declaration has parameters")));
        }
        let lean = string(&decl.name);
        if decl.result.is_scalar() {
            let t = rust_type(decl.result);
            w.line(format!("pub(crate) static I_{m}: {RT}::InitCell<{t}> = {RT}::InitCell::new({lean});"));
            w.line("#[inline]");
            w.open(format!("pub(crate) unsafe fn {m}() -> {t} {{"));
            w.line(format!("I_{m}.get()"));
        } else {
            w.line(format!("pub(crate) static I_{m}: {RT}::InitObj = {RT}::InitObj::new({lean});"));
            w.line("#[inline]");
            w.open(format!("pub(crate) unsafe fn {m}() -> Obj {{"));
            w.line(format!("I_{m}.get_obj()"));
        }
        w.close("}");
        w.line("");
        Ok(())
    }

    fn extern_wrapper(&self, w: &mut Writer, decl: &Declaration, m: &str) -> Result<(), CodegenError> {
        let call = self.externs.call(decl, &|n| self.decls.get(n).copied())?;
        let params = rust_params(&decl.params);
        let sig: Vec<String> = params.iter().map(|p| format!("x_{}: {}", p.var, rust_type(p.ty))).collect();
        w.line("#[inline(always)]");
        w.open(format!("pub(crate) unsafe extern \"C\" fn {m}({}) -> {} {{", sig.join(", "), rust_type(decl.result)));
        for line in call.pre {
            w.line(line);
        }
        w.line(format!("let r: {} = {};", rust_type(decl.result), call.call));
        for line in call.post {
            w.line(line);
        }
        w.line("r");
        w.close("}");
        w.line("");
        Ok(())
    }

    fn constant(&self, w: &mut Writer, decl: &'a Declaration, m: &str, block: &'a Block) -> Result<(), CodegenError> {
        let t = rust_type(decl.result);
        if decl.result.is_scalar() {
            w.line(format!("static C_{m}: {RT}::LazyScalar<{t}> = {RT}::LazyScalar::new();"));
        } else {
            w.line(format!("static C_{m}: {RT}::LazyObj = {RT}::LazyObj::new();"));
        }
        w.open(format!("unsafe fn {m}__init() -> {t} {{"));
        let mut f = FnCtx::new(self, decl, false);
        f.block(w, block)?;
        w.close("}");
        w.line("#[inline]");
        w.open(format!("pub(crate) unsafe fn {m}() -> {t} {{"));
        w.line(format!("C_{m}.get({m}__init)"));
        w.close("}");
        w.line("");
        Ok(())
    }

    fn function(&self, w: &mut Writer, decl: &'a Declaration, m: &str, block: &'a Block) -> Result<(), CodegenError> {
        let params = rust_params(&decl.params);
        let array = takes_arg_array(decl);
        if array && params.len() != decl.params.len() {
            return Err(CodegenError::adapter(&decl.name, "boxed declaration with `void` parameters"));
        }
        let tail = has_self_tail_call(&decl.name, block);
        let ret = rust_type(decl.result);
        if array {
            w.open(format!("pub(crate) unsafe extern \"C\" fn {m}(args: *mut Obj) -> {ret} {{"));
            for (i, p) in params.iter().enumerate() {
                let kw = if tail { "let mut" } else { "let" };
                w.line(format!("{kw} x_{}: Obj = *args.add({i});", p.var));
            }
        } else {
            let sig: Vec<String> = params
                .iter()
                .map(|p| {
                    let kw = if tail { "mut " } else { "" };
                    format!("{kw}x_{}: {}", p.var, rust_type(p.ty))
                })
                .collect();
            w.open(format!("pub(crate) unsafe extern \"C\" fn {m}({}) -> {ret} {{", sig.join(", ")));
        }
        let mut f = FnCtx::new(self, decl, tail);
        if tail {
            w.open("'tail: loop {");
            f.block(w, block)?;
            w.close("}");
        } else {
            f.block(w, block)?;
        }
        w.close("}");
        w.line("");
        Ok(())
    }

    /// The Rust expression calling declaration `callee` with IR arguments `args`.
    pub fn call(
        &self,
        callee: &Declaration,
        args: &[Arg],
        arg: &dyn Fn(&Arg) -> String,
    ) -> Result<String, CodegenError> {
        if callee.params.len() != args.len() {
            return Err(CodegenError::internal(format!("{} applied to {} arguments", callee.name, args.len())));
        }
        let m = mangle(&callee.name);
        if takes_arg_array(callee) {
            let items: Vec<String> = args.iter().map(arg).collect();
            return Ok(format!("{m}([{}].as_mut_ptr())", items.join(", ")));
        }
        let items: Vec<String> =
            callee.params.iter().zip(args).filter(|(p, _)| p.ty != IrType::Void).map(|(_, a)| arg(a)).collect();
        Ok(format!("{m}({})", items.join(", ")))
    }
}

fn has_self_tail_call(name: &str, block: &Block) -> bool {
    fn visit(name: &str, block: &Block) -> bool {
        for (i, s) in block.stmts.iter().enumerate() {
            match s {
                Stmt::Join { body, .. } => {
                    if visit(name, body) {
                        return true;
                    }
                }
                Stmt::Let { var, expr: Expr::Fap { function, .. }, .. }
                    if function == name
                        && i + 1 == block.stmts.len()
                        && block.terminator == (Terminator::Ret { arg: Arg::Var(*var) }) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        if let Terminator::Case { alts, .. } = &block.terminator {
            return alts.iter().any(|a| visit(name, a.body()));
        }
        false
    }
    visit(name, block)
}

struct FnCtx<'e, 'a> {
    e: &'e Emitter<'a>,
    decl: &'a Declaration,
    tail: bool,
    /// Parameters of each join point in scope.
    joins: HashMap<u32, &'a [Param]>,
    vars: HashMap<u32, IrType>,
}

impl<'e, 'a> FnCtx<'e, 'a> {
    fn new(e: &'e Emitter<'a>, decl: &'a Declaration, tail: bool) -> Self {
        let vars = decl.params.iter().map(|p| (p.var, p.ty)).collect();
        FnCtx { e, decl, tail, joins: HashMap::new(), vars }
    }

    fn arg(&self, a: &Arg) -> String {
        match a {
            Arg::Var(v) => format!("x_{v}"),
            Arg::Erased => format!("{RT}::lean_box(0)"),
        }
    }

    fn block(&mut self, w: &mut Writer, block: &'a Block) -> Result<(), CodegenError> {
        self.stmts(w, &block.stmts, &block.terminator)
    }

    fn stmts(&mut self, w: &mut Writer, stmts: &'a [Stmt], term: &'a Terminator) -> Result<(), CodegenError> {
        for (i, stmt) in stmts.iter().enumerate() {
            match stmt {
                Stmt::Join { id, params, body } => {
                    for p in params {
                        w.line(format!("let mut x_{}: {};", p.var, rust_type(p.ty)));
                        self.vars.insert(p.var, p.ty);
                    }
                    w.open(format!("'b{id}: {{"));
                    self.joins.insert(*id, params);
                    self.stmts(w, &stmts[i + 1..], term)?;
                    w.close("}");
                    self.joins.remove(id);
                    return self.block(w, body);
                }
                Stmt::Let { var, expr: Expr::Fap { function, args }, .. }
                    if self.tail
                        && *function == self.decl.name
                        && i + 1 == stmts.len()
                        && *term == (Terminator::Ret { arg: Arg::Var(*var) }) =>
                {
                    return self.tail_call(w, args);
                }
                Stmt::Let { var, ty, expr } => {
                    self.let_(w, *var, *ty, expr)?;
                    self.vars.insert(*var, *ty);
                }
                Stmt::Set { var, index, arg } => {
                    w.line(format!("{RT}::lean_ctor_set(x_{var}, {index}, {});", self.arg(arg)))
                }
                Stmt::SetTag { var, tag } => w.line(format!("{RT}::lean_ctor_set_tag(x_{var}, {tag});")),
                Stmt::Uset { var, index, value } => {
                    w.line(format!("{RT}::lean_ctor_set_usize(x_{var}, {index}, x_{value});"))
                }
                Stmt::Sset { var, index, offset, value, ty } => {
                    let setter = scalar_accessor(*ty, "set")?;
                    w.line(format!("{RT}::{setter}(x_{var}, {}, x_{value});", scalar_offset(*index, *offset)));
                }
                Stmt::Inc { var, count, checked, persistent } => {
                    if !persistent {
                        let f = match (*count == 1, *checked) {
                            (true, true) => format!("lean_inc(x_{var})"),
                            (true, false) => format!("lean_inc_ref(x_{var})"),
                            (false, true) => format!("lean_inc_n(x_{var}, {count})"),
                            (false, false) => format!("lean_inc_ref_n(x_{var}, {count})"),
                        };
                        w.line(format!("{RT}::{f};"));
                    }
                }
                Stmt::Dec { var, count, checked, persistent } => {
                    if *count != 1 {
                        return Err(CodegenError::adapter(&self.decl.name, "decrement by more than one"));
                    }
                    if !persistent {
                        let f = if *checked { "lean_dec" } else { "lean_dec_ref" };
                        w.line(format!("{RT}::{f}(x_{var});"));
                    }
                }
                Stmt::Del { var } => w.line(format!("{RT}::lean_del_object(x_{var});")),
            }
        }
        self.terminator(w, term)
    }

    fn tail_call(&mut self, w: &mut Writer, args: &[Arg]) -> Result<(), CodegenError> {
        let params: Vec<&Param> = rust_params(&self.decl.params);
        let args: Vec<&Arg> =
            self.decl.params.iter().zip(args).filter(|(p, _)| p.ty != IrType::Void).map(|(_, a)| a).collect();
        // Evaluate every argument before assigning any parameter.
        for (i, (p, a)) in params.iter().zip(&args).enumerate() {
            w.line(format!("let t_{i}: {} = {};", rust_type(p.ty), self.arg(a)));
        }
        for (i, p) in params.iter().enumerate() {
            w.line(format!("x_{} = t_{i};", p.var));
        }
        w.line("continue 'tail;");
        Ok(())
    }

    fn let_(&mut self, w: &mut Writer, var: u32, ty: IrType, expr: &'a Expr) -> Result<(), CodegenError> {
        let t = rust_type(ty);
        let x = format!("x_{var}");
        match expr {
            Expr::Ctor { info, args } => {
                if info.is_scalar() {
                    w.line(format!("let {x}: Obj = {RT}::lean_box({});", info.tag));
                } else {
                    w.line(format!("let {x}: Obj = {};", alloc_ctor(info)));
                    for (i, a) in args.iter().enumerate() {
                        w.line(format!("{RT}::lean_ctor_set({x}, {i}, {});", self.arg(a)));
                    }
                }
            }
            Expr::Reset { fields, var: y } => {
                w.open(format!("let {x}: Obj = if {RT}::lean_is_exclusive(x_{y}) {{"));
                for i in 0..*fields {
                    w.line(format!("{RT}::lean_ctor_release(x_{y}, {i});"));
                }
                w.line(format!("x_{y}"));
                w.dedent();
                w.open("} else {");
                w.line(format!("{RT}::lean_dec_ref(x_{y});"));
                w.line(format!("{RT}::lean_box(0)"));
                w.close("};");
            }
            Expr::Reuse { var: y, info, update_header, args } => {
                w.open(format!("let {x}: Obj = if {RT}::lean_is_scalar(x_{y}) {{"));
                w.line(alloc_ctor(info));
                w.dedent();
                w.open("} else {");
                if *update_header {
                    w.line(format!("{RT}::lean_ctor_set_tag(x_{y}, {});", info.tag));
                }
                w.line(format!("x_{y}"));
                w.close("};");
                for (i, a) in args.iter().enumerate() {
                    w.line(format!("{RT}::lean_ctor_set({x}, {i}, {});", self.arg(a)));
                }
            }
            Expr::Proj { index, var: y } => w.line(format!("let {x}: Obj = {RT}::lean_ctor_get(x_{y}, {index});")),
            Expr::Uproj { index, var: y } => {
                w.line(format!("let {x}: usize = {RT}::lean_ctor_get_usize(x_{y}, {index});"))
            }
            Expr::Sproj { fields, offset, var: y } => {
                let getter = scalar_accessor(ty, "get")?;
                w.line(format!("let {x}: {t} = {RT}::{getter}(x_{y}, {});", scalar_offset(*fields, *offset)));
            }
            Expr::Fap { function, args } => {
                let callee = self.e.decl(function)?;
                let call = self.e.call(callee, args, &|a| self.arg(a))?;
                w.line(format!("let {x}: {t} = {call};"));
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
                    "let {x}: Obj = {RT}::lean_alloc_closure({} as *const (), {}, {});",
                    mangle(function),
                    callee.params.len(),
                    args.len()
                ));
                for (i, a) in args.iter().enumerate() {
                    w.line(format!("{RT}::lean_closure_set({x}, {i}, {});", self.arg(a)));
                }
            }
            Expr::Ap { var: f, args } => {
                let items: Vec<String> = args.iter().map(|a| self.arg(a)).collect();
                if args.len() <= 16 {
                    w.line(format!("let {x}: Obj = {RT}::lean_apply_{}(x_{f}, {});", args.len(), items.join(", ")));
                } else {
                    w.line(format!("let {x}: Obj = {RT}::lean_apply_m(x_{f}, &[{}]);", items.join(", ")));
                }
            }
            Expr::Box { ty: boxed, var: y } => {
                let e = match boxed {
                    IrType::Usize => format!("{RT}::lean_box_usize(x_{y})"),
                    IrType::Uint32 => format!("{RT}::lean_box_uint32(x_{y})"),
                    IrType::Uint64 => format!("{RT}::lean_box_uint64(x_{y})"),
                    IrType::Float => format!("{RT}::lean_box_float(x_{y})"),
                    IrType::Float32 => format!("{RT}::lean_box_float32(x_{y})"),
                    IrType::Uint8 | IrType::Uint16 => format!("{RT}::lean_box(x_{y} as usize)"),
                    other => {
                        return Err(CodegenError::adapter(
                            &self.decl.name,
                            format!("box of non-scalar type {}", other.name()),
                        ));
                    }
                };
                w.line(format!("let {x}: Obj = {e};"));
            }
            Expr::Unbox { var: y } => {
                let e = match ty {
                    IrType::Usize => format!("{RT}::lean_unbox_usize(x_{y})"),
                    IrType::Uint32 => format!("{RT}::lean_unbox_uint32(x_{y})"),
                    IrType::Uint64 => format!("{RT}::lean_unbox_uint64(x_{y})"),
                    IrType::Float => format!("{RT}::lean_unbox_float(x_{y})"),
                    IrType::Float32 => format!("{RT}::lean_unbox_float32(x_{y})"),
                    IrType::Uint8 | IrType::Uint16 => format!("{RT}::lean_unbox(x_{y}) as {t}"),
                    other => {
                        return Err(CodegenError::adapter(
                            &self.decl.name,
                            format!("unbox into non-scalar type {}", other.name()),
                        ));
                    }
                };
                w.line(format!("let {x}: {t} = {e};"));
            }
            Expr::Lit(Literal::Num(n)) => {
                if ty.is_scalar() {
                    let suffix = rust_type(ty);
                    if ty == IrType::Float || ty == IrType::Float32 {
                        return Err(CodegenError::adapter(&self.decl.name, "numeric literal of floating-point type"));
                    }
                    w.line(format!("let {x}: {t} = {n}{suffix};"));
                } else {
                    let small = n.len() < 10 || n.parse::<u64>().is_ok_and(|v| v < (1u64 << 32));
                    if small {
                        w.line(format!("let {x}: Obj = {RT}::nat::lean_usize_to_nat({n});"));
                    } else {
                        w.line(format!("let {x}: Obj = {RT}::nat::lean_cstr_to_nat({});", string(n)));
                    }
                }
            }
            Expr::Lit(Literal::Str(s)) => {
                w.line(format!(
                    "let {x}: Obj = {RT}::lean_mk_string_unchecked({}, {});",
                    byte_string(s.as_bytes()),
                    s.chars().count()
                ));
            }
            Expr::IsShared { var: y } => {
                w.line(format!("let {x}: u8 = (!{RT}::lean_is_exclusive(x_{y})) as u8;"));
            }
        }
        Ok(())
    }

    fn terminator(&mut self, w: &mut Writer, term: &'a Terminator) -> Result<(), CodegenError> {
        match term {
            Terminator::Ret { arg } => {
                let _ = self.decl;
                w.line(format!("return {};", self.arg(arg)));
            }
            Terminator::Jmp { id, args } => {
                let params = self.joins.get(id).copied().ok_or_else(|| {
                    CodegenError::internal(format!("jump to block_{id} outside its scope in {}", self.decl.name))
                })?;
                for (p, a) in params.iter().zip(args) {
                    w.line(format!("x_{} = {};", p.var, self.arg(a)));
                }
                w.line(format!("break 'b{id};"));
            }
            Terminator::Unreachable => w.line(format!("{RT}::lean_internal_panic_unreachable();")),
            Terminator::Case { var, alts, .. } => {
                let var_ty = self.vars.get(var).copied().ok_or_else(|| {
                    CodegenError::internal(format!("case on unknown variable x_{var} in {}", self.decl.name))
                })?;
                let scrutinee =
                    if var_ty.is_scalar() { format!("x_{var}") } else { format!("{RT}::lean_obj_tag(x_{var})") };
                w.open(format!("match {scrutinee} {{"));
                // As Lean's emitter does, the last alternative covers every remaining tag when no
                // default alternative is present.
                let last = alts.len() - 1;
                for (i, alt) in alts.iter().enumerate() {
                    let pattern = match alt {
                        Alt::Ctor { info, .. } if i != last => info.tag.to_string(),
                        _ => "_".to_owned(),
                    };
                    w.open(format!("{pattern} => {{"));
                    let saved_vars = self.vars.clone();
                    self.block(w, alt.body())?;
                    self.vars = saved_vars;
                    w.close("}");
                }
                w.close("}");
            }
        }
        Ok(())
    }
}

fn alloc_ctor(info: &CtorInfo) -> String {
    format!("{RT}::lean_alloc_ctor({}, {}, {})", info.tag, info.size, crate::rust::word_offset(info.usize, info.ssize))
}

fn scalar_offset(fields: u32, offset: u32) -> String {
    crate::rust::word_offset(fields, offset)
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
    Ok(format!("lean_ctor_{op}_{kind}"))
}
