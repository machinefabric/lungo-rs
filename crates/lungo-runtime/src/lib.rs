//! The lungo runtime: a Rust implementation of the runtime semantics Lean's compiler output
//! assumes.
//!
//! Generated code refers to this crate through `::lungo::__runtime`. The object model,
//! reference counting, closures, big numbers, strings, arrays, thunks, tasks, and `IO`
//! primitives reproduce the behaviour of Lean's C runtime (`lean.h` and `src/runtime`) for the
//! Lean release the bridge supports; the primitives implementing `@[extern]` symbols are listed
//! in [`registry`].

#![allow(clippy::missing_safety_doc)]

pub mod apply;
pub mod exports;
pub mod header;
pub mod init;
pub mod object;
pub mod panic;
pub mod registry;
pub(crate) use registry::lean_externs;
pub mod utf8;

pub mod array;
pub mod float;
pub mod hash;
pub mod int;
pub mod io;
pub mod misc;
pub mod nat;
pub mod platform;
#[cfg(not(target_os = "wasi"))]
pub mod process;
pub mod sharecommon;
pub mod st;
pub mod string;
pub mod task;
pub mod uint;
pub mod uv;

pub mod intrinsics;
pub mod wire;

#[cfg(feature = "capi")]
pub mod capi;

pub use apply::*;
pub use init::*;
pub use object::*;

/// Version of the interface between generated code and this runtime. Generated code asserts it
/// at compile time, so code generated for a different runtime fails to compile.
pub const ABI_VERSION: u32 = 1;

/// Compile-time check that generated code targets this runtime's ABI.
pub const fn assert_abi<const VERSION: u32>() {
    assert!(
        VERSION == ABI_VERSION,
        "generated Lean code targets a different lungo runtime ABI; regenerate it with the matching lungo-build"
    );
}
