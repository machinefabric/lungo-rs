//! Lean definitions the runtime itself calls.
//!
//! Lean's C runtime calls a few functions written in Lean and published with `@[export]`: it
//! renders `IO.Error`s with `lean_io_error_to_string`, builds `IO.Error` values with the
//! `lean_mk_io_error_*` constructors, wraps handles into streams with `lean_stream_of_handle`,
//! and prints panic messages with `lean_io_eprintln`. The lungo runtime calls the same
//! compiled Lean definitions: every program includes them, and its initialization registers
//! them here before any Lean code runs. Their calling convention is the C signature Lean's
//! runtime uses; generated wrappers reconcile it with the compiled definitions.

use crate::object::{Obj, lean_internal_panic};
use crate::registry::Ty;
use std::sync::atomic::{AtomicPtr, Ordering};

/// A Lean `@[export]` symbol the runtime calls, with the C signature the runtime uses.
#[derive(Debug)]
pub struct RequiredExport {
    pub symbol: &'static str,
    pub params: &'static [Ty],
    pub result: Ty,
}

macro_rules! required {
    ($($id:ident = $sym:literal ($($p:ident),*) -> $r:ident;)*) => {
        /// Indices into [`REQUIRED`].
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Clone, Copy)]
        #[repr(usize)]
        pub enum Export { $($id),* }

        /// Every export the runtime calls.
        pub static REQUIRED: &[RequiredExport] = &[
            $(RequiredExport { symbol: $sym, params: &[$(Ty::$p),*], result: Ty::$r },)*
        ];

        const COUNT: usize = [$($sym),*].len();

        static SLOTS: [AtomicPtr<()>; COUNT] = [const { AtomicPtr::new(std::ptr::null_mut()) }; COUNT];
    };
}

required! {
    IoErrorToString = "lean_io_error_to_string"(obj) -> obj;
    StreamOfHandle = "lean_stream_of_handle"(obj) -> obj;
    IoEprintln = "lean_io_eprintln"(obj) -> obj;
    UserError = "lean_mk_io_user_error"(obj) -> obj;
    Eof = "lean_mk_io_error_eof"(obj) -> obj;
    AlreadyExists = "lean_mk_io_error_already_exists"(u32, obj) -> obj;
    AlreadyExistsFile = "lean_mk_io_error_already_exists_file"(obj, u32, obj) -> obj;
    HardwareFault = "lean_mk_io_error_hardware_fault"(u32, obj) -> obj;
    IllegalOperation = "lean_mk_io_error_illegal_operation"(u32, obj) -> obj;
    InappropriateType = "lean_mk_io_error_inappropriate_type"(u32, obj) -> obj;
    InappropriateTypeFile = "lean_mk_io_error_inappropriate_type_file"(obj, u32, obj) -> obj;
    Interrupted = "lean_mk_io_error_interrupted"(obj, u32, obj) -> obj;
    InvalidArgument = "lean_mk_io_error_invalid_argument"(u32, obj) -> obj;
    InvalidArgumentFile = "lean_mk_io_error_invalid_argument_file"(obj, u32, obj) -> obj;
    NoFileOrDirectory = "lean_mk_io_error_no_file_or_directory"(obj, u32, obj) -> obj;
    NoSuchThing = "lean_mk_io_error_no_such_thing"(u32, obj) -> obj;
    NoSuchThingFile = "lean_mk_io_error_no_such_thing_file"(obj, u32, obj) -> obj;
    OtherError = "lean_mk_io_error_other_error"(u32, obj) -> obj;
    PermissionDenied = "lean_mk_io_error_permission_denied"(u32, obj) -> obj;
    PermissionDeniedFile = "lean_mk_io_error_permission_denied_file"(obj, u32, obj) -> obj;
    ProtocolError = "lean_mk_io_error_protocol_error"(u32, obj) -> obj;
    ResourceBusy = "lean_mk_io_error_resource_busy"(u32, obj) -> obj;
    ResourceExhausted = "lean_mk_io_error_resource_exhausted"(u32, obj) -> obj;
    ResourceExhaustedFile = "lean_mk_io_error_resource_exhausted_file"(obj, u32, obj) -> obj;
    ResourceVanished = "lean_mk_io_error_resource_vanished"(u32, obj) -> obj;
    TimeExpired = "lean_mk_io_error_time_expired"(u32, obj) -> obj;
    UnsatisfiedConstraints = "lean_mk_io_error_unsatisfied_constraints"(u32, obj) -> obj;
    UnsupportedOperation = "lean_mk_io_error_unsupported_operation"(u32, obj) -> obj;
}

/// Registers the compiled implementation of the required export `symbol`.
///
/// `f` must be an `unsafe extern "C"` function with the export's C signature.
pub fn register(symbol: &str, f: *const ()) {
    let Some(i) = REQUIRED.iter().position(|r| r.symbol == symbol) else {
        lean_internal_panic(&format!("`{symbol}` is not an export the lungo runtime calls"));
    };
    SLOTS[i].store(f as *mut (), Ordering::Release);
}

/// The registered implementation of `export`.
#[inline]
pub fn get(export: Export) -> *const () {
    let f = SLOTS[export as usize].load(Ordering::Acquire);
    if f.is_null() {
        lean_internal_panic(&format!(
            "the Lean definition exported as `{}` is not part of the compiled program",
            REQUIRED[export as usize].symbol
        ));
    }
    f
}

/// Calls a registered `obj → obj` export, consuming the argument.
#[inline]
pub unsafe fn call1(export: Export, a: Obj) -> Obj {
    unsafe { std::mem::transmute::<*const (), unsafe extern "C" fn(Obj) -> Obj>(get(export))(a) }
}

/// Calls a registered `(u32, obj) → obj` export, consuming the object.
#[inline]
pub unsafe fn call_code(export: Export, code: u32, details: Obj) -> Obj {
    unsafe { std::mem::transmute::<*const (), unsafe extern "C" fn(u32, Obj) -> Obj>(get(export))(code, details) }
}

/// Calls a registered `(obj, u32, obj) → obj` export, consuming the objects.
#[inline]
pub unsafe fn call_file(export: Export, file: Obj, code: u32, details: Obj) -> Obj {
    unsafe {
        std::mem::transmute::<*const (), unsafe extern "C" fn(Obj, u32, Obj) -> Obj>(get(export))(file, code, details)
    }
}

/// Test support: stand-ins for the compiled Lean exports, for unit tests of the runtime that
/// run without a Lean program.
///
/// The `IO.Error` constructors build a record of which constructor the runtime chose and the
/// arguments it passed, which is exactly the runtime's responsibility (the layout of `IO.Error`
/// belongs to the compiled Lean code). `lean_io_eprintln` writes to the process's stderr.
#[cfg(test)]
pub(crate) mod recording {
    use super::{Export, REQUIRED, register};
    use crate::object::*;

    /// An `IO.Error` built by a recording constructor.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Recorded {
        /// The constructor export without its `lean_mk_io_error_` prefix, e.g. `no_file_or_directory`.
        pub constructor: &'static str,
        pub file: Option<String>,
        pub code: u32,
        pub details: String,
    }

    fn make(export: Export, file: Obj, code: u32, details: Obj) -> Obj {
        unsafe {
            let o = lean_alloc_ctor(export as u32, 2, 4);
            lean_ctor_set(o, 0, file);
            lean_ctor_set(o, 1, details);
            lean_ctor_set_uint32(o, 2 * size_of::<Obj>(), code);
            o
        }
    }

    fn export_at(i: usize) -> Export {
        // `Export` is a fieldless `repr(usize)` enum indexing `REQUIRED`.
        assert!(i < REQUIRED.len());
        unsafe { std::mem::transmute::<usize, Export>(i) }
    }

    unsafe extern "C" fn message<const I: usize>(details: Obj) -> Obj {
        make(export_at(I), lean_box(0), 0, details)
    }

    unsafe extern "C" fn with_code<const I: usize>(code: u32, details: Obj) -> Obj {
        make(export_at(I), lean_box(0), code, details)
    }

    unsafe extern "C" fn with_file<const I: usize>(file: Obj, code: u32, details: Obj) -> Obj {
        make(export_at(I), file, code, details)
    }

    unsafe extern "C" fn eprintln(s: Obj) -> Obj {
        unsafe {
            eprintln!("{}", lean_string_str(s));
            lean_dec(s);
            lean_io_result_mk_ok(lean_box(0))
        }
    }

    unsafe extern "C" fn to_string(e: Obj) -> Obj {
        let r = unsafe { read(e) };
        unsafe { lean_dec(e) };
        lean_mk_string(&format!("{r:?}"))
    }

    macro_rules! install_all {
        ($($e:ident => $f:ident),* $(,)?) => {
            $(register(REQUIRED[Export::$e as usize].symbol, $f::<{ Export::$e as usize }> as *const ());)*
        };
    }

    /// Registers the stand-ins (idempotent). `lean_stream_of_handle` stays unregistered.
    pub fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            register(REQUIRED[Export::IoEprintln as usize].symbol, eprintln as *const ());
            register(REQUIRED[Export::IoErrorToString as usize].symbol, to_string as *const ());
            install_all! {
                UserError => message, Eof => message,
                AlreadyExists => with_code, AlreadyExistsFile => with_file,
                HardwareFault => with_code, IllegalOperation => with_code,
                InappropriateType => with_code, InappropriateTypeFile => with_file,
                Interrupted => with_file, InvalidArgument => with_code,
                InvalidArgumentFile => with_file, NoFileOrDirectory => with_file,
                NoSuchThing => with_code, NoSuchThingFile => with_file,
                OtherError => with_code, PermissionDenied => with_code,
                PermissionDeniedFile => with_file, ProtocolError => with_code,
                ResourceBusy => with_code, ResourceExhausted => with_code,
                ResourceExhaustedFile => with_file, ResourceVanished => with_code,
                TimeExpired => with_code, UnsatisfiedConstraints => with_code,
                UnsupportedOperation => with_code,
            }
        });
    }

    /// Reads a recorded error (borrowed).
    pub unsafe fn read(e: Obj) -> Recorded {
        unsafe {
            let symbol = REQUIRED[lean_obj_tag(e) as usize].symbol;
            let constructor = symbol
                .strip_prefix("lean_mk_io_error_")
                .or_else(|| symbol.strip_prefix("lean_mk_io_"))
                .unwrap_or_else(|| panic!("{symbol} is not an IO.Error constructor"));
            let f = lean_ctor_get(e, 0);
            Recorded {
                constructor,
                file: (!f.is_scalar()).then(|| lean_string_str(f).to_owned()),
                code: lean_ctor_get_uint32(e, 2 * size_of::<Obj>()),
                details: lean_string_str(lean_ctor_get(e, 1)).to_owned(),
            }
        }
    }
}
