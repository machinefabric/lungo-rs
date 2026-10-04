//! Every BIR instruction the supported toolchain's compiler emits occurs in the compiled code of
//! the conformance programs themselves (not only in the libraries they use), so the
//! differential conformance test compares every such instruction's Rust implementation against
//! Lean's native backend.

use lungo_build::bir::{Block, Body, Expr, Literal, Stmt, Terminator};
use lungo_build::{Environment, configure};
use std::collections::BTreeSet;
use std::path::Path;

const ALL: &[&str] = &[
    "stmt:let",
    "stmt:join",
    "stmt:set",
    "stmt:set_tag",
    "stmt:uset",
    "stmt:sset",
    "stmt:inc",
    "stmt:dec",
    "stmt:del",
    "term:case",
    "term:ret",
    "term:jmp",
    "term:unreachable",
    "expr:ctor",
    "expr:reset",
    "expr:reuse",
    "expr:proj",
    "expr:uproj",
    "expr:sproj",
    "expr:fap",
    "expr:pap",
    "expr:ap",
    "expr:box",
    "expr:unbox",
    "expr:lit_num",
    "expr:lit_str",
    "expr:is_shared",
];

/// Instructions Lean 4.34.1 never leaves in final IR: its reset/reuse expansion rewrites every
/// `reset`/`reuse` pair into `isShared`, `set`, `setTag` and `del` before code generation. BIR
/// still represents them (they are part of Lean's IR), and the backend implements them.
const NOT_EMITTED_BY_TOOLCHAIN: &[&str] = &["expr:reset", "expr:reuse"];

fn expr_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Ctor { .. } => "expr:ctor",
        Expr::Reset { .. } => "expr:reset",
        Expr::Reuse { .. } => "expr:reuse",
        Expr::Proj { .. } => "expr:proj",
        Expr::Uproj { .. } => "expr:uproj",
        Expr::Sproj { .. } => "expr:sproj",
        Expr::Fap { .. } => "expr:fap",
        Expr::Pap { .. } => "expr:pap",
        Expr::Ap { .. } => "expr:ap",
        Expr::Box { .. } => "expr:box",
        Expr::Unbox { .. } => "expr:unbox",
        Expr::Lit(Literal::Num(_)) => "expr:lit_num",
        Expr::Lit(Literal::Str(_)) => "expr:lit_str",
        Expr::IsShared { .. } => "expr:is_shared",
    }
}

fn stmt_kind(s: &Stmt) -> &'static str {
    match s {
        Stmt::Let { .. } => "stmt:let",
        Stmt::Join { .. } => "stmt:join",
        Stmt::Set { .. } => "stmt:set",
        Stmt::SetTag { .. } => "stmt:set_tag",
        Stmt::Uset { .. } => "stmt:uset",
        Stmt::Sset { .. } => "stmt:sset",
        Stmt::Inc { .. } => "stmt:inc",
        Stmt::Dec { .. } => "stmt:dec",
        Stmt::Del { .. } => "stmt:del",
    }
}

fn walk(b: &Block, seen: &mut BTreeSet<&'static str>) {
    for s in &b.stmts {
        seen.insert(stmt_kind(s));
        match s {
            Stmt::Let { expr, .. } => {
                seen.insert(expr_kind(expr));
            }
            Stmt::Join { body, .. } => walk(body, seen),
            _ => {}
        }
    }
    let kind = match &b.terminator {
        Terminator::Case { alts, .. } => {
            for a in alts {
                walk(a.body(), seen);
            }
            "term:case"
        }
        Terminator::Ret { .. } => "term:ret",
        Terminator::Jmp { .. } => "term:jmp",
        Terminator::Unreachable => "term:unreachable",
    };
    seen.insert(kind);
}

/// TEST0028: conformance corpus covers every instruction
#[test]
fn test0028_conformance_corpus_covers_every_instruction() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance").canonicalize().unwrap();
    let lakefile = std::fs::read_to_string(project.join("lakefile.toml")).unwrap();
    let roots: Vec<&str> =
        lakefile.lines().filter_map(|l| l.trim().strip_prefix("root = \"")).map(|r| r.trim_end_matches('"')).collect();
    assert!(roots.len() >= 10, "conformance roots: {roots:?}");
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("coverage");
    let mut local = BTreeSet::new();
    let mut emitted = BTreeSet::new();
    for root in &roots {
        let env = Environment::native(project.clone(), scratch.join(root), scratch.join(format!("{root}-work")));
        let analysis = configure().root_module(*root).analyze(&project, &env).unwrap();
        for d in &analysis.success.bir.declarations {
            if let Body::Function { block } = &d.body {
                walk(block, &mut emitted);
                if d.module.starts_with("Conformance") {
                    walk(block, &mut local);
                }
            }
        }
    }
    let untested: Vec<&&str> = emitted.iter().filter(|k| !local.contains(**k)).collect();
    assert!(untested.is_empty(), "instructions the toolchain emits but the conformance programs lack: {untested:?}");
    let never: Vec<&str> = ALL.iter().copied().filter(|k| !emitted.contains(k)).collect();
    assert_eq!(never, NOT_EMITTED_BY_TOOLCHAIN, "the set of instructions the toolchain never emits changed");
}
