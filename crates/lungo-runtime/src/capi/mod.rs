//! The C ABI of the runtime, for programs the lungo C backend generates.
//!
//! Every primitive of the [`registry`](crate::registry) is exported as `lungo_<symbol>`. This
//! module adds what generated C needs besides them: allocation and the slow paths of reference
//! counting (the fast paths are inline in `lungo.h`, over the same object layout), closure
//! application, boxing, literals, constants and initialization cells, program initialization
//! and `main`. Every function here has a `lungo_` symbol and the C calling convention; a Rust
//! panic reaching one of them aborts the process, since unwinding cannot cross into C.
//!
//! The generated C declares these functions through `lungo.h`, which `lungo-capi` publishes
//! and keeps in sync with this module.

#![allow(clippy::missing_safety_doc)]

pub mod boundary;
mod cells;
mod object;
mod program;
pub mod value;

pub use cells::{InitBits, InitObj, LazyBits, LazyObj};

/// Defined only by a runtime with C ABI version 1 (`LUNGO_ABI_VERSION` in `lungo.h`); generated
/// programs reference it, so linking one against a runtime of another ABI version fails.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static lungo_abi_v1: u32 = crate::ABI_VERSION;

const _: () = assert!(crate::ABI_VERSION == 1, "rename `lungo_abi_v1` and LUNGO_ABI_VERSION in lungo.h");
