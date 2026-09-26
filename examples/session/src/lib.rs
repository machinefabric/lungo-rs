//! A Lean specification of a session state machine, compiled to Rust.

pub mod formal {
    include!(concat!(env!("OUT_DIR"), "/lean2rust/formal.rs"));
}

/// Advances `session` by `op`, as specified by `Formal.apply`.
pub fn next(op: formal::Op, session: formal::Sess) -> Option<formal::Sess> {
    formal::apply(op, session)
}
