//! Support crate for Rust code generated from Lean by `lungo-build`.
//!
//! Generated code consists of a compiler layer, which mechanically reproduces Lean's compiled
//! program on top of [`__runtime`], and a public facade of ordinary Rust types and functions.
//! This crate provides the Rust types the facade exposes for Lean's builtin types ([`Nat`],
//! [`Int`], [`List`], [`ByteArray`], [`FloatArray`]), opaque handles for Lean values without a
//! plain Rust representation ([`LeanValue`], [`LeanClosure`]), `IO` errors ([`IoError`]), and
//! the [`LeanType`] conversion trait. [`include_lean!`] includes a generated module.
//!
//! With the `serde` feature, the facade types implement `serde::Serialize` and
//! `serde::Deserialize`, so that generated types can derive them.

#![cfg_attr(docsrs, feature(doc_cfg))]

mod backend;
mod collections;
mod convert;
mod int;
mod io;
mod layout;
mod meta;
mod nat;
#[cfg(feature = "serde")]
mod serde_impls;
mod value;

pub use backend::{Backend, RustBackend};
pub use collections::{ByteArray, FloatArray, List};
pub use convert::LeanType;
pub use int::Int;
pub use io::{IoError, IoErrorType};
#[doc(hidden)]
pub use layout::assert_layout;
pub use layout::LeanLayout;
pub use meta::{DeclarationInfo, ExportTrust, SourcePosition, SourceRange};
pub use nat::Nat;
pub use value::{LeanClosure, LeanValue, RustClosure};

/// Includes the Rust module lungo generated for a Lean project, by the module's name: the Lake
/// package's name, or the name given to `lungo_build::Builder::name`.
///
/// ```ignore
/// // build.rs: lungo_build::compile_lean("lean")   (Lake package `formal`)
/// pub mod formal {
///     lungo::include_lean!("formal");
/// }
/// ```
///
/// This finds modules in the default output directory, `$OUT_DIR/lungo`. A module generated
/// with `lungo_build::Builder::out_dir` is included with `include!` from that directory:
/// `include!("<out_dir>/<name>/<name>.rs")`.
#[macro_export]
macro_rules! include_lean {
    ($name:tt) => {
        include!(concat!(env!("OUT_DIR"), "/lungo/", $name, "/", $name, ".rs"));
    };
}

/// The runtime used by generated code. Not a stable public interface.
#[doc(hidden)]
pub use lungo_runtime as __runtime;

/// Big-number types used by generated code. Not a stable public interface.
#[doc(hidden)]
pub use num_bigint as __num_bigint;

/// Conversion helpers used by generated facades. Not a stable public interface.
#[doc(hidden)]
pub mod __facade {
    pub use crate::convert::facade::*;
}

/// Runs `f` on a thread with the stack size Lean programs expect (1 GiB reserved on 64-bit
/// targets, or `LEAN_STACK_SIZE_KB`), for calls into deeply recursive Lean code.
pub fn with_lean_stack<R: Send>(f: impl FnOnce() -> R + Send) -> R {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("lean".into())
            .stack_size(lungo_runtime::task::thread_stack_size())
            .spawn_scoped(s, f)
            .expect("cannot start a thread for Lean code")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}
