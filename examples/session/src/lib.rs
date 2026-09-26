//! A Lean specification of a session state machine, compiled to Rust.

pub mod formal {
    patina::include_lean!("formal");
}

/// Advances `session` by `op`, as specified by `Formal.apply`.
pub fn next(op: formal::Op, session: formal::Sess) -> Option<formal::Sess> {
    formal::apply(op, session)
}
