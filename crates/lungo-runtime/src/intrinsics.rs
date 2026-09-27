//! The flat namespace of runtime primitives, addressed by generated code as
//! `::lungo::__runtime::intrinsics::<symbol>`.
//!
//! Each runtime module defines its primitives in an `externs` submodule with
//! the crate's `lean_externs!` macro, which also produces the module's registry table.

use crate::registry::{Intrinsic, Unsupported};

pub use crate::array::externs::*;
pub use crate::float::externs::*;
pub use crate::int::externs::*;
pub use crate::io::externs::*;
pub use crate::misc::externs::*;
pub use crate::nat::externs::*;
pub use crate::panic::externs::*;
pub use crate::platform::externs::*;
pub use crate::process::externs::*;
pub use crate::sharecommon::externs::*;
pub use crate::st::externs::*;
pub use crate::string::externs::*;
pub use crate::task::externs::*;
pub use crate::uint::externs::*;
pub use crate::uv::externs::*;

pub(crate) static TABLES: &[&[Intrinsic]] = &[
    crate::array::externs::INTRINSICS,
    crate::float::externs::INTRINSICS,
    crate::int::externs::INTRINSICS,
    crate::io::externs::INTRINSICS,
    crate::misc::externs::INTRINSICS,
    crate::nat::externs::INTRINSICS,
    crate::panic::externs::INTRINSICS,
    crate::platform::externs::INTRINSICS,
    crate::process::externs::INTRINSICS,
    crate::sharecommon::externs::INTRINSICS,
    crate::st::externs::INTRINSICS,
    crate::string::externs::INTRINSICS,
    crate::task::externs::INTRINSICS,
    crate::uint::externs::INTRINSICS,
    crate::uv::externs::INTRINSICS,
];

pub(crate) static UNSUPPORTED_TABLES: &[&[Unsupported]] =
    &[crate::io::UNSUPPORTED, crate::platform::UNSUPPORTED, crate::uv::UNSUPPORTED];

/// Why a `wasm32-wasip1` runtime lacks the primitive `symbol`, if it does: that runtime is built
/// without the modules for child processes (`process`) and for sockets, signals, timers and the
/// event loop (`uv`), and without the primitives listed in [`WASI_UNAVAILABLE`].
pub(crate) fn wasi_unavailable(symbol: &str) -> Option<&'static str> {
    let in_table = |t: &[Intrinsic]| t.iter().any(|i| i.symbol == symbol);
    if in_table(crate::process::externs::INTRINSICS) {
        return Some("WebAssembly has no child processes");
    }
    if in_table(crate::uv::externs::INTRINSICS) {
        return Some("WebAssembly has no sockets, signals, timers or event loop");
    }
    WASI_UNAVAILABLE.iter().find(|u| u.symbol == symbol).map(|u| u.reason)
}

/// Primitives of otherwise available modules that a `wasm32-wasip1` runtime lacks.
pub(crate) static WASI_UNAVAILABLE: &[Unsupported] = &[];
