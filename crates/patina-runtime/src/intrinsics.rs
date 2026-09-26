//! The flat namespace of runtime primitives, addressed by generated code as
//! `::patina::__runtime::intrinsics::<symbol>`.
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
