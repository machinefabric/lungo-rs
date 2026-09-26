//! A human-readable rendering of BIR, modelled on Lean's own IR formatter.

use crate::{
    Alt, Arg, Block, Body, CtorInfo, Declaration, Expr, ExternEntry, Initializer, Literal, Param, Program, Stmt,
    Terminator,
};
use std::fmt::Write;

pub fn pretty_program(program: &Program) -> String {
    let mut out = String::new();
    writeln!(out, "-- Bridge IR version {}", program.bir_version).unwrap();
    for module in &program.modules {
        if module.initializers.is_empty() {
            continue;
        }
        writeln!(out, "-- module {} initializers:", module.name).unwrap();
        for init in &module.initializers {
            match init {
                Initializer::Io(d) => writeln!(out, "--   run {d}").unwrap(),
                Initializer::Value { decl, init_fn } => writeln!(out, "--   {decl} := run {init_fn}").unwrap(),
            }
        }
    }
    for decl in &program.declarations {
        out.push_str(&pretty_declaration(decl));
    }
    out
}

pub fn pretty_declaration(decl: &Declaration) -> String {
    let mut out = String::new();
    let params = params(&decl.params);
    match &decl.body {
        Body::Function { block } => {
            writeln!(out, "def {}{} : {} :=", decl.name, params, decl.result.name()).unwrap();
            write_block(&mut out, block, 1);
        }
        Body::Extern { selected, .. } => {
            writeln!(out, "extern {}{} : {} := {}", decl.name, params, decl.result.name(), extern_entry(selected))
                .unwrap();
        }
    }
    out
}

fn extern_entry(e: &ExternEntry) -> String {
    match e {
        ExternEntry::Adhoc { backend } => format!("adhoc {backend}"),
        ExternEntry::Inline { backend, pattern } => format!("{backend} inline {pattern:?}"),
        ExternEntry::Standard { backend, symbol } => format!("{backend} {symbol:?}"),
        ExternEntry::Opaque => "opaque".to_owned(),
    }
}

fn params(ps: &[Param]) -> String {
    let mut s = String::new();
    for p in ps {
        let borrow = if p.borrow { "@& " } else { "" };
        write!(s, " (x_{} : {borrow}{})", p.var, p.ty.name()).unwrap();
    }
    s
}

fn arg(a: &Arg) -> String {
    match a {
        Arg::Var(v) => format!("x_{v}"),
        Arg::Erased => "◾".to_owned(),
    }
}

fn args(xs: &[Arg]) -> String {
    xs.iter().map(|a| format!(" {}", arg(a))).collect()
}

fn ctor(info: &CtorInfo) -> String {
    if info.usize > 0 || info.ssize > 0 {
        format!("ctor_{}.{}.{}[{}]", info.tag, info.usize, info.ssize, info.name)
    } else {
        format!("ctor_{}[{}]", info.tag, info.name)
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Ctor { info, args: ys } => format!("{}{}", ctor(info), args(ys)),
        Expr::Reset { fields, var } => format!("reset[{fields}] x_{var}"),
        Expr::Reuse { var, info, update_header, args: ys } => {
            let upd = if *update_header { "!" } else { "" };
            format!("reuse{upd} x_{var} in {}{}", ctor(info), args(ys))
        }
        Expr::Proj { index, var } => format!("proj[{index}] x_{var}"),
        Expr::Uproj { index, var } => format!("uproj[{index}] x_{var}"),
        Expr::Sproj { fields, offset, var } => format!("sproj[{fields}, {offset}] x_{var}"),
        Expr::Fap { function, args: ys } => format!("{function}{}", args(ys)),
        Expr::Pap { function, args: ys } => format!("pap {function}{}", args(ys)),
        Expr::Ap { var, args: ys } => format!("app x_{var}{}", args(ys)),
        Expr::Box { var, .. } => format!("box x_{var}"),
        Expr::Unbox { var } => format!("unbox x_{var}"),
        Expr::Lit(Literal::Num(n)) => n.clone(),
        Expr::Lit(Literal::Str(s)) => format!("{s:?}"),
        Expr::IsShared { var } => format!("isShared x_{var}"),
    }
}

fn write_block(out: &mut String, block: &Block, depth: usize) {
    for stmt in &block.stmts {
        indent(out, depth);
        match stmt {
            Stmt::Let { var, ty, expr: e } => writeln!(out, "let x_{var} : {} := {};", ty.name(), expr(e)).unwrap(),
            Stmt::Join { id, params: ps, body } => {
                writeln!(out, "block_{id}{} :=", params(ps)).unwrap();
                write_block(out, body, depth + 1);
            }
            Stmt::Set { var, index, arg: a } => writeln!(out, "set x_{var}[{index}] := {};", arg(a)).unwrap(),
            Stmt::SetTag { var, tag } => writeln!(out, "setTag x_{var} := {tag};").unwrap(),
            Stmt::Uset { var, index, value } => writeln!(out, "uset x_{var}[{index}] := x_{value};").unwrap(),
            Stmt::Sset { var, index, offset, value, ty } => {
                writeln!(out, "sset x_{var}[{index}, {offset}] : {} := x_{value};", ty.name()).unwrap()
            }
            Stmt::Inc { var, count, checked, persistent } => {
                let n = if *count != 1 { format!(" {count}") } else { String::new() };
                let flags = match (checked, persistent) {
                    (false, _) => "[ref]",
                    (true, true) => "[persistent]",
                    (true, false) => "",
                };
                writeln!(out, "inc{flags} x_{var}{n};").unwrap()
            }
            Stmt::Dec { var, checked, persistent, .. } => {
                let flags = match (checked, persistent) {
                    (false, _) => "[ref]",
                    (true, true) => "[persistent]",
                    (true, false) => "",
                };
                writeln!(out, "dec{flags} x_{var};").unwrap()
            }
            Stmt::Del { var } => writeln!(out, "del x_{var};").unwrap(),
        }
    }
    indent(out, depth);
    match &block.terminator {
        Terminator::Case { type_name, var, var_ty, alts } => {
            writeln!(out, "case[{type_name}] x_{var} : {} of", var_ty.name()).unwrap();
            for alt in alts {
                indent(out, depth);
                match alt {
                    Alt::Ctor { info, body } => {
                        writeln!(out, "{} →", info.name).unwrap();
                        write_block(out, body, depth + 1);
                    }
                    Alt::Default { body } => {
                        writeln!(out, "default →").unwrap();
                        write_block(out, body, depth + 1);
                    }
                }
            }
        }
        Terminator::Ret { arg: a } => writeln!(out, "ret {}", arg(a)).unwrap(),
        Terminator::Jmp { id, args: ys } => writeln!(out, "jmp block_{id}{}", args(ys)).unwrap(),
        Terminator::Unreachable => writeln!(out, "⊥").unwrap(),
    }
}
