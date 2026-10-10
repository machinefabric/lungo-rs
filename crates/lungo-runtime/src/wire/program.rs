//! Async programs at the boundary: the steps of a `Lungo.Async.Program` and their resumption.
//!
//! An export returning `Lungo.Async.Program op α` is called like any other function, and its
//! result is the program's first *step*:
//!
//! - `0` and a value of `α`: the program is done;
//! - `1`, a value of `op` (the operation the host is asked to perform) and a `u64` *resumption*:
//!   the program waits for the host's answer.
//!
//! The host performs the operation (asynchronously, in its own terms) and resumes the program with
//! the answer, a value of the type the operation's constructor is answered with
//! ([`resume`]); the result is the next step. A resumption is used exactly once: resuming
//! consumes it, whether the answer is accepted or not, so the continuation can never run twice.
//! [`cancel`] gives one up without resuming it. A resumption that was resumed, cancelled, or never
//! issued is *stale*: resuming or cancelling it again is reported, never undefined.
//!
//! Lean represents the program as an inductive value: `done` (constructor 0) holds the value in
//! object field 0, and `call` (constructor 1) holds the operation in object field 0 and the
//! continuation, a closure from the answer to the next program, in object field 1. Both fields
//! are boxed, the parameters of the type not being stored. The worker checks this layout against
//! Lean's compiler for the toolchain it supports.

use super::{Handles, Reader, Returns, Type, TypeTable, WireError, decode, encode};
use crate::object::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Kinds of steps.
pub mod step {
    pub const DONE: u8 = 0;
    pub const CALL: u8 = 1;
}

const DONE_TAG: u32 = 0;
const DONE_VALUE: u32 = 0;
const CALL_TAG: u32 = 1;
const CALL_OP: u32 = 0;
const CALL_RESUME: u32 = 1;

/// A program waiting for the host's answer to an operation.
struct Resumption {
    table: &'static TypeTable,
    /// The operation type, the answer type of each of its constructors, and the program's result
    /// type.
    op: u32,
    rets: Vec<Type>,
    value: Type,
    /// The constructor of the operation asked: its answer is a value of `rets[ctor]`.
    ctor: u32,
    /// The continuation, owned.
    resume: SendObj,
}

struct Resumptions {
    /// Identifiers are never reused: an identifier not in `live` is stale forever.
    next: AtomicU64,
    live: Mutex<HashMap<u64, Resumption>>,
}

fn resumptions() -> &'static Resumptions {
    static R: OnceLock<Resumptions> = OnceLock::new();
    R.get_or_init(|| Resumptions { next: AtomicU64::new(1), live: Mutex::new(HashMap::new()) })
}

/// Why a resumption was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeError {
    /// The resumption was resumed or cancelled already, or never issued.
    Stale(u64),
    /// The answer is not a value of the type the operation is answered with. The resumption is
    /// consumed: the program cannot continue.
    Malformed(String),
}

impl std::fmt::Display for ResumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResumeError::Stale(id) => write!(f, "{id} is not a live resumption: it was resumed or cancelled already"),
            ResumeError::Malformed(m) => write!(f, "the answer is malformed: {m}"),
        }
    }
}

/// The constructor index of the operation `o` (borrowed), a value of table type `op`.
unsafe fn ctor_index(table: &TypeTable, op: u32, o: Obj) -> u32 {
    let t = &table.types[op as usize];
    if t.trivial.is_some() || t.ctors.len() == 1 {
        return 0;
    }
    let tag = unsafe { lean_obj_tag(o) };
    match t.ctors.iter().position(|c| c.tag == tag) {
        Some(i) => i as u32,
        None => lean_internal_panic(&format!("invalid constructor tag {tag} for the operations {}", t.name)),
    }
}

/// Encodes the step of program `p` (consumed): `DONE` and its value, or `CALL`, its operation and
/// a new resumption holding its continuation.
///
/// # Safety
///
/// `p` must be a live `Lungo.Async.Program` whose operations are table type `op` and whose value
/// is of type `value`, and `rets` the answer types of `op`'s constructors (checked by
/// [`Returns::check`]).
pub unsafe fn step(table: &'static TypeTable, op: u32, rets: &[Type], value: &Type, p: Obj, out: &mut Vec<u8>) {
    unsafe {
        if lean_is_scalar(p) {
            lean_internal_panic("an async program is a scalar: its layout is not the one lungo reads");
        }
        match lean_ptr_tag(p) as u32 {
            DONE_TAG => {
                out.push(step::DONE);
                encode(table, value, lean_ctor_get(p, DONE_VALUE), out);
            }
            CALL_TAG => {
                let operation = lean_ctor_get(p, CALL_OP);
                let ctor = ctor_index(table, op, operation);
                out.push(step::CALL);
                encode(table, &Type::Inductive { index: op, args: Vec::new() }, operation, out);
                let resume = lean_ctor_get(p, CALL_RESUME);
                lean_inc(resume);
                // The host may resume on any thread.
                lean_mark_mt(resume);
                let r = resumptions();
                let id = r.next.fetch_add(1, Ordering::Relaxed);
                r.live.lock().unwrap_or_else(|e| e.into_inner()).insert(
                    id,
                    Resumption { table, op, rets: rets.to_vec(), value: value.clone(), ctor, resume: SendObj(resume) },
                );
                out.extend_from_slice(&id.to_le_bytes());
            }
            tag => lean_internal_panic(&format!(
                "an async program has constructor tag {tag}: its layout is not the one lungo reads"
            )),
        }
        lean_dec(p);
    }
}

/// Encodes the step a function returning `returns` produced as its result `p` (consumed).
///
/// # Safety
///
/// As [`step`]; `returns` must be [`Returns::Async`].
pub unsafe fn step_returns(table: &'static TypeTable, returns: &Returns, p: Obj, out: &mut Vec<u8>) {
    match returns {
        Returns::Async { op, rets, value } => unsafe { step(table, *op, rets, value, p, out) },
        _ => lean_internal_panic("an async step of a function that does not return an async program"),
    }
}

fn take(id: u64) -> Result<Resumption, ResumeError> {
    resumptions().live.lock().unwrap_or_else(|e| e.into_inner()).remove(&id).ok_or(ResumeError::Stale(id))
}

/// Resumes the program waiting on resumption `id` with the answer `input` (its handles are
/// given to the runtime, as a host function's result's are), and encodes its next step. The
/// resumption is consumed.
pub fn resume(id: u64, input: &[u8]) -> Result<Vec<u8>, ResumeError> {
    let res = take(id)?;
    let ret = &res.rets[res.ctor as usize];
    let mut r = Reader::new(input);
    let answer = match decode(res.table, ret, &mut r, Handles::Take).and_then(|v| r.finish().map(|_| v)) {
        Ok(v) => v,
        Err(WireError(m)) => {
            unsafe { lean_dec(res.resume.0) };
            return Err(ResumeError::Malformed(m));
        }
    };
    let mut out = Vec::new();
    unsafe {
        let next = crate::apply::lean_apply_n(res.resume.0, &[answer]);
        step(res.table, res.op, &res.rets, &res.value, next, &mut out);
    }
    Ok(out)
}

/// Gives up the program waiting on resumption `id`, releasing it. False when `id` is stale.
pub fn cancel(id: u64) -> bool {
    match take(id) {
        Ok(res) => {
            unsafe { lean_dec(res.resume.0) };
            true
        }
        Err(_) => false,
    }
}

/// The number of programs waiting for an answer.
pub fn outstanding() -> usize {
    resumptions().live.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// What resumption `id` waits for, without consuming it: the program's table, the type of the
/// answer, and what the program returns (to read its next step with).
pub fn awaiting(id: u64) -> Result<(&'static TypeTable, Type, Returns), ResumeError> {
    let live = resumptions().live.lock().unwrap_or_else(|e| e.into_inner());
    match live.get(&id) {
        Some(r) => Ok((
            r.table,
            r.rets[r.ctor as usize].clone(),
            Returns::Async { op: r.op, rets: r.rets.clone(), value: r.value.clone() },
        )),
        None => Err(ResumeError::Stale(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Ctor, Field, FieldKind, Repr, Signature, TypeDecl};
    use super::*;

    /// `inductive Op | ask (q : String) | tick`, answered with a `Nat` and a `Unit`, and
    /// `structure Only where q : String`, a single operation.
    fn table() -> &'static TypeTable {
        static T: OnceLock<TypeTable> = OnceLock::new();
        let ask = |name: &str, tag| Ctor {
            name: name.into(),
            tag,
            size: 1,
            usize: 0,
            ssize: 0,
            fields: vec![Field { name: "q".into(), kind: FieldKind::Object(0), ty: Type::String }],
        };
        T.get_or_init(|| TypeTable {
            types: vec![
                TypeDecl {
                    name: "Op".into(),
                    opaque: false,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![
                        ask("Op.ask", 0),
                        Ctor { name: "Op.tick".into(), tag: 1, size: 0, usize: 0, ssize: 0, fields: vec![] },
                    ],
                },
                TypeDecl {
                    name: "Only".into(),
                    opaque: false,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![ask("Only.mk", 0)],
                },
                TypeDecl {
                    name: "W".into(),
                    opaque: true,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![],
                },
            ],
        })
    }

    const OP: u32 = 0;
    const ONLY: u32 = 1;

    fn rets() -> Vec<Type> {
        vec![Type::Nat, Type::Unit]
    }

    /// The counters below are global: the tests that read them run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn done(v: Obj) -> Obj {
        unsafe {
            let p = lean_alloc_ctor(DONE_TAG, 1, 0);
            lean_ctor_set(p, DONE_VALUE, v);
            p
        }
    }

    fn call(op: Obj, resume: Obj) -> Obj {
        unsafe {
            let p = lean_alloc_ctor(CALL_TAG, 2, 0);
            lean_ctor_set(p, CALL_OP, op);
            lean_ctor_set(p, CALL_RESUME, resume);
            p
        }
    }

    fn ask(q: &str) -> Obj {
        unsafe {
            let o = lean_alloc_ctor(0, 1, 0);
            lean_ctor_set(o, 0, lean_mk_string(q));
            o
        }
    }

    /// After a tick: done with the number asked before it.
    unsafe extern "C" fn after_tick(n: Obj, unit: Obj) -> Obj {
        unsafe { lean_dec(unit) };
        done(n)
    }

    /// After the answer to `ask`: tick, then done with the answer.
    unsafe extern "C" fn after_ask(n: Obj) -> Obj {
        unsafe {
            let k = lean_alloc_closure(after_tick as *const (), 2, 1);
            lean_closure_set(k, 0, n);
            call(lean_box(1), k)
        }
    }

    /// `do let n ← ask q; tick; pure n`.
    fn ask_then_tick(q: &str) -> Obj {
        unsafe { call(ask(q), lean_alloc_closure(after_ask as *const (), 1, 0)) }
    }

    fn first_step(p: Obj) -> Vec<u8> {
        let mut out = Vec::new();
        unsafe { step(table(), OP, &rets(), &Type::Nat, p, &mut out) };
        out
    }

    /// The resumption of a `CALL` step.
    fn resumption(step: &[u8]) -> u64 {
        assert_eq!(step[0], step::CALL, "{step:?}");
        u64::from_le_bytes(step[step.len() - 8..].try_into().unwrap())
    }

    const NAT_5: [u8; 5] = [1, 0, 0, 0, 5];

    /// TEST0273: async programs step, wait and resume through the wire format
    #[test]
    fn test0273_async_programs_step_wait_and_resume_through_the_wire_format() {
        let _s = serial();
        let before = outstanding();
        // done: its value.
        assert_eq!(first_step(done(lean_box(5))), [&[step::DONE][..], &NAT_5[..]].concat());
        // ask "q": the operation (constructor 0 and its field) and a resumption.
        let s = first_step(ask_then_tick("q"));
        assert_eq!(&s[..s.len() - 8], [step::CALL, 0, 0, 0, 0, 1, 0, 0, 0, b'q']);
        let id = resumption(&s);
        assert_eq!(outstanding(), before + 1);
        assert!(awaiting(id).is_ok_and(|(_, ret, _)| ret == Type::Nat), "ask is answered with a Nat");
        // tick, a constructor without fields (a scalar in Lean), answered with a Unit.
        let s = resume(id, &NAT_5).unwrap();
        assert_eq!(&s[..s.len() - 8], [step::CALL, 1, 0, 0, 0]);
        let tick = resumption(&s);
        assert_ne!(tick, id, "a resumption is never reused");
        assert!(awaiting(tick).is_ok_and(|(_, ret, _)| ret == Type::Unit), "tick is answered with a Unit");
        assert_eq!(resume(tick, &[]).unwrap(), [&[step::DONE][..], &NAT_5[..]].concat());
        assert_eq!(outstanding(), before);
        // An operation type with a single constructor: its operations are constructor 0.
        let mut out = Vec::new();
        let only = unsafe { call(ask("x"), lean_alloc_closure(after_ask as *const (), 1, 0)) };
        unsafe { step(table(), ONLY, &[Type::Nat], &Type::Nat, only, &mut out) };
        assert_eq!(&out[..out.len() - 8], [step::CALL, 0, 0, 0, 0, 1, 0, 0, 0, b'x']);
        assert!(cancel(resumption(&out)));
        assert_eq!(outstanding(), before);
    }

    /// TEST0274: a resumption is used once, and a stale one is reported
    #[test]
    fn test0274_a_resumption_is_used_once_and_a_stale_one_is_reported() {
        let _s = serial();
        let before = outstanding();
        let id = resumption(&first_step(ask_then_tick("q")));
        let next = resume(id, &NAT_5).unwrap();
        assert_eq!(resume(id, &NAT_5), Err(ResumeError::Stale(id)), "resumed twice");
        assert!(!cancel(id), "cancelled after resuming");
        let tick = resumption(&next);
        assert!(cancel(tick));
        assert!(!cancel(tick), "cancelled twice");
        assert_eq!(resume(tick, &[]), Err(ResumeError::Stale(tick)), "resumed after cancelling");
        assert!(awaiting(tick).is_err());
        assert_eq!(resume(u64::MAX, &[]), Err(ResumeError::Stale(u64::MAX)), "never issued");
        // A malformed answer consumes the resumption: the program cannot continue.
        let id = resumption(&first_step(ask_then_tick("q")));
        assert!(matches!(resume(id, &[1, 0, 0]), Err(ResumeError::Malformed(_))), "a truncated Nat");
        assert_eq!(resume(id, &NAT_5), Err(ResumeError::Stale(id)));
        let id = resumption(&first_step(ask_then_tick("q")));
        assert!(matches!(resume(id, &[1, 0, 0, 0, 5, 0]), Err(ResumeError::Malformed(_))), "trailing bytes");
        assert_eq!(outstanding(), before);
    }

    /// TEST0275: threads racing to resume one resumption resume it once
    #[test]
    fn test0275_threads_racing_to_resume_one_resumption_resume_it_once() {
        let _s = serial();
        let before = outstanding();
        for _ in 0..50 {
            let id = resumption(&first_step(ask_then_tick("q")));
            let barrier = std::sync::Barrier::new(8);
            let results: Vec<Result<Vec<u8>, ResumeError>> = std::thread::scope(|s| {
                let handles: Vec<_> = (0..8)
                    .map(|_| {
                        s.spawn(|| {
                            barrier.wait();
                            resume(id, &NAT_5)
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let won: Vec<&Vec<u8>> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
            assert_eq!(won.len(), 1, "{results:?}");
            assert!(results.iter().all(|r| r.is_ok() || *r == Err(ResumeError::Stale(id))));
            assert!(cancel(resumption(won[0])));
        }
        assert_eq!(outstanding(), before);
    }

    /// TEST0276: async return kinds round trip and are validated
    #[test]
    fn test0276_async_return_kinds_round_trip_and_are_validated() {
        let sig = Signature {
            type_params: 1,
            params: vec![Type::Param(0)],
            returns: Returns::Async { op: OP, rets: rets(), value: Type::List(Box::new(Type::Param(0))) },
        };
        assert_eq!(Signature::decode(&sig.encode()).unwrap(), sig);
        sig.check(table()).unwrap();
        let inst = sig.instantiate(&[Type::String]).unwrap();
        assert_eq!(inst.returns, Returns::Async { op: OP, rets: rets(), value: Type::List(Box::new(Type::String)) });
        let rejected = |returns: Returns| Signature { type_params: 1, params: vec![], returns }.check(table()).is_err();
        assert!(rejected(Returns::Async { op: 2, rets: vec![], value: Type::Nat }), "opaque operations");
        assert!(rejected(Returns::Async { op: 9, rets: vec![], value: Type::Nat }), "outside the table");
        assert!(rejected(Returns::Async { op: OP, rets: vec![Type::Nat], value: Type::Nat }), "an answer missing");
        let open = vec![Type::Param(0), Type::Unit];
        assert!(rejected(Returns::Async { op: OP, rets: open, value: Type::Nat }), "an answer with a type parameter");
    }
}
