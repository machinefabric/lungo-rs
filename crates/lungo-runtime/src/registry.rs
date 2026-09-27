//! The registry of runtime primitives that implement Lean `@[extern]` symbols.
//!
//! Every primitive is declared with the crate's `lean_externs!` macro, which defines the Rust function and
//! records its C-level signature: parameter representations and whether each object parameter
//! is borrowed (`b_obj`) or owned (`obj`). The code generator resolves each extern symbol
//! reached by a program against this registry and rejects any mismatch between the signature
//! Lean's compiler expects and the one implemented here, so ownership conventions cannot drift.
//!
//! Symbols that exist in the Lean toolchain but that the PureRust runtime deliberately does not
//! implement are listed with the reason (see [`unsupported_symbols`]); reaching one is a build
//! error.

#![allow(non_camel_case_types)]

use crate::object::Obj;

/// An owned object argument or result.
pub type obj = Obj;
/// A borrowed object argument: the callee does not consume the reference.
pub type b_obj = Obj;
// Scalar representations, named so that `lean_externs!` can map each parameter type token to
// both a Rust type and a registry entry.
pub type u8 = ::core::primitive::u8;
pub type u16 = ::core::primitive::u16;
pub type u32 = ::core::primitive::u32;
pub type u64 = ::core::primitive::u64;
pub type usize = ::core::primitive::usize;
pub type f64 = ::core::primitive::f64;
pub type f32 = ::core::primitive::f32;

/// The representation of one parameter or result of a primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    obj,
    b_obj,
    u8,
    u16,
    u32,
    u64,
    usize,
    f64,
    f32,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::obj => "obj",
            Ty::b_obj => "@& obj",
            Ty::u8 => "u8",
            Ty::u16 => "u16",
            Ty::u32 => "u32",
            Ty::u64 => "u64",
            Ty::usize => "usize",
            Ty::f64 => "f64",
            Ty::f32 => "f32",
        }
    }
}

#[derive(Debug)]
pub struct Intrinsic {
    /// The C symbol Lean's `@[extern]` attribute names.
    pub symbol: &'static str,
    pub params: &'static [Ty],
    pub result: Ty,
}

#[derive(Debug)]
pub struct Unsupported {
    pub symbol: &'static str,
    pub reason: &'static str,
}

/// Defines runtime primitives and their registry entries.
///
/// ```ignore
/// lean_externs! {
///     fn lean_nat_add(a: b_obj, b: b_obj) -> obj { ... }
/// }
/// ```
macro_rules! lean_externs {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ident),* $(,)?) -> $ret:ident $body:block
    )*) => {
        $(
            $(#[$meta])*
            #[allow(non_snake_case, clippy::missing_safety_doc, unused_unsafe)]
            #[inline]
            pub unsafe fn $name($($arg: $crate::registry::$ty),*) -> $crate::registry::$ret {
                unsafe { $body }
            }
        )*
        /// The primitives under their C symbols (`lungo_<symbol>`), for generated C programs.
        /// Exported symbols are part of the library's interface whatever the module's visibility.
        #[cfg(feature = "capi")]
        mod capi {
            $(
                #[allow(non_snake_case, clippy::missing_safety_doc, clippy::too_many_arguments)]
                #[unsafe(export_name = concat!("lungo_", stringify!($name)))]
                pub unsafe extern "C" fn $name($($arg: $crate::registry::$ty),*) -> $crate::registry::$ret {
                    unsafe { super::$name($($arg),*) }
                }
            )*
        }
        #[allow(dead_code)]
        pub(crate) const INTRINSICS: &[$crate::registry::Intrinsic] = &[
            $(
                $crate::registry::Intrinsic {
                    symbol: stringify!($name),
                    params: &[$($crate::registry::Ty::$ty),*],
                    result: $crate::registry::Ty::$ret,
                },
            )*
        ];
    };
}

pub(crate) use lean_externs;

/// Every primitive the runtime implements.
pub fn intrinsics() -> impl Iterator<Item = &'static Intrinsic> {
    crate::intrinsics::TABLES.iter().flat_map(|t| t.iter())
}

/// Looks up the primitive implementing `symbol`.
pub fn lookup(symbol: &str) -> Option<&'static Intrinsic> {
    intrinsics().find(|i| i.symbol == symbol)
}

/// Toolchain symbols deliberately not provided by the PureRust runtime, with the reason.
pub fn unsupported_symbols() -> impl Iterator<Item = &'static Unsupported> {
    crate::intrinsics::UNSUPPORTED_TABLES.iter().flat_map(|t| t.iter())
}

/// Looks up whether `symbol` is a known toolchain symbol the PureRust runtime does not provide.
pub fn unsupported(symbol: &str) -> Option<&'static Unsupported> {
    unsupported_symbols().find(|u| u.symbol == symbol)
}

/// Why the primitive `symbol` is unavailable on the target `target` (a target triple), if it is.
/// WebAssembly (`wasm32-wasip1`) runs a program on one thread, without child processes, sockets
/// or an event loop; every other target has every primitive.
pub fn unavailable_on(symbol: &str, target: &str) -> Option<&'static str> {
    if !target.starts_with("wasm32-") {
        return None;
    }
    crate::intrinsics::wasi_unavailable(symbol)
}
