//! `IO` primitives, ported from Lean's `runtime/io.cpp`.
//!
//! This module provides the decoding of operating-system errors into `IO.Error`s, file handles
//! with C `FILE`-like buffering, the standard streams and their per-thread redirection,
//! file-system and environment access, clocks, and process exit.
//!
//! Like Lean's C runtime, this module calls compiled Lean code (see [`crate::exports`]): errors
//! are built with the `IO.Error` constructors Lean exports (`lean_mk_io_error_*`), the standard
//! streams are `IO.FS.Stream.ofHandle` applied to the standard handles, and runtime diagnostics
//! (panic messages, `timeit`) are printed with Lean's `IO.eprintln`.
//!
//! `BaseIO` primitives return their value directly; `IO` primitives return an `IO` result
//! (`EStateM.Result` with the world token erased).

use crate::object::*;
use crate::registry::Unsupported;
use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

pub(crate) const UNSUPPORTED: &[Unsupported] = &[];

// =============================================================================================
// Operating-system error codes. The libuv error codes and messages (libuv 1.52.1, bundled with
// Lean 4.34.1) are defined once, in `crate::uv`.
// =============================================================================================

use crate::uv::errno as uve;

/// `errno` values of the C runtime Lean links against on Windows (MinGW-w64 / UCRT).
#[cfg(windows)]
#[allow(dead_code)]
mod crt {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ESRCH: i32 = 3;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const ENXIO: i32 = 6;
    pub const E2BIG: i32 = 7;
    pub const ENOEXEC: i32 = 8;
    pub const EBADF: i32 = 9;
    pub const ECHILD: i32 = 10;
    pub const EAGAIN: i32 = 11;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EBUSY: i32 = 16;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENODEV: i32 = 19;
    pub const ENOTDIR: i32 = 20;
    pub const EISDIR: i32 = 21;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const EFBIG: i32 = 27;
    pub const ENOSPC: i32 = 28;
    pub const ESPIPE: i32 = 29;
    pub const EROFS: i32 = 30;
    pub const EMLINK: i32 = 31;
    pub const EPIPE: i32 = 32;
    pub const EDOM: i32 = 33;
    pub const ERANGE: i32 = 34;
    pub const EDEADLK: i32 = 36;
    pub const ENAMETOOLONG: i32 = 38;
    pub const ENOLCK: i32 = 39;
    pub const ENOSYS: i32 = 40;
    pub const ENOTEMPTY: i32 = 41;
    pub const EILSEQ: i32 = 42;
    pub const EADDRINUSE: i32 = 100;
    pub const EADDRNOTAVAIL: i32 = 101;
    pub const EAFNOSUPPORT: i32 = 102;
    pub const EALREADY: i32 = 103;
    pub const EBADMSG: i32 = 104;
    pub const ECANCELED: i32 = 105;
    pub const ECONNABORTED: i32 = 106;
    pub const ECONNREFUSED: i32 = 107;
    pub const ECONNRESET: i32 = 108;
    pub const EDESTADDRREQ: i32 = 109;
    pub const EHOSTUNREACH: i32 = 110;
    pub const EIDRM: i32 = 111;
    pub const EINPROGRESS: i32 = 112;
    pub const EISCONN: i32 = 113;
    pub const ELOOP: i32 = 114;
    pub const EMSGSIZE: i32 = 115;
    pub const ENETDOWN: i32 = 116;
    pub const ENETRESET: i32 = 117;
    pub const ENETUNREACH: i32 = 118;
    pub const ENOBUFS: i32 = 119;
    pub const ENODATA: i32 = 120;
    pub const ENOLINK: i32 = 121;
    pub const ENOMSG: i32 = 122;
    pub const ENOPROTOOPT: i32 = 123;
    pub const ENOSR: i32 = 124;
    pub const ENOSTR: i32 = 125;
    pub const ENOTCONN: i32 = 126;
    pub const ENOTSOCK: i32 = 128;
    pub const ENOTSUP: i32 = 129;
    pub const EOPNOTSUPP: i32 = 130;
    pub const EOVERFLOW: i32 = 132;
    pub const EPROTO: i32 = 134;
    pub const EPROTONOSUPPORT: i32 = 135;
    pub const EPROTOTYPE: i32 = 136;
    pub const ETIME: i32 = 137;
    pub const ETIMEDOUT: i32 = 138;
    pub const ETXTBSY: i32 = 139;
    pub const EWOULDBLOCK: i32 = 140;
}

/// Platform `errno` constants under uniform names.
#[cfg(unix)]
mod errno {
    pub use libc::*;
}
#[cfg(windows)]
mod errno {
    pub use super::crt::*;
}

/// `lean_crt_to_uv_err`: translates a C-runtime `errno` value into the libuv code used to
/// classify it. Values libuv cannot represent are approximated as Lean's runtime does;
/// unrecognized values are negated.
pub(crate) fn uv_code_of_errno(err: i32) -> i32 {
    use errno as e;
    match err {
        e::E2BIG => uve::UV_E2BIG,
        e::EACCES => uve::UV_EACCES,
        e::EADDRINUSE => uve::UV_EADDRINUSE,
        e::EADDRNOTAVAIL => uve::UV_EADDRNOTAVAIL,
        e::EAFNOSUPPORT => uve::UV_EAFNOSUPPORT,
        e::EAGAIN => uve::UV_EAGAIN,
        e::EBADF => uve::UV_EBADF,
        e::EBUSY => uve::UV_EBUSY,
        e::ECONNABORTED => uve::UV_ECONNABORTED,
        e::ECONNREFUSED => uve::UV_ECONNREFUSED,
        e::ECONNRESET => uve::UV_ECONNRESET,
        e::EDESTADDRREQ => uve::UV_EDESTADDRREQ,
        e::EEXIST => uve::UV_EEXIST,
        e::EFAULT => uve::UV_EFAULT,
        e::EFBIG => uve::UV_EFBIG,
        e::EHOSTUNREACH => uve::UV_EHOSTUNREACH,
        e::EILSEQ => uve::UV_EILSEQ,
        e::EINTR => uve::UV_EINTR,
        e::EINVAL => uve::UV_EINVAL,
        e::EIO => uve::UV_EIO,
        e::EISCONN => uve::UV_EISCONN,
        e::EISDIR => uve::UV_EISDIR,
        e::ELOOP => uve::UV_ELOOP,
        e::EMFILE => uve::UV_EMFILE,
        e::EMLINK => uve::UV_EMLINK,
        e::EMSGSIZE => uve::UV_EMSGSIZE,
        e::ENAMETOOLONG => uve::UV_ENAMETOOLONG,
        e::ENETDOWN => uve::UV_ENETDOWN,
        e::ENETUNREACH => uve::UV_ENETUNREACH,
        e::ENFILE => uve::UV_ENFILE,
        e::ENOBUFS => uve::UV_ENOBUFS,
        e::ENODEV => uve::UV_ENODEV,
        e::ENOENT => uve::UV_ENOENT,
        e::ENOMEM => uve::UV_ENOMEM,
        e::ENOPROTOOPT => uve::UV_ENOPROTOOPT,
        e::ENOSPC => uve::UV_ENOSPC,
        e::ENOSYS => uve::UV_ENOSYS,
        e::ENOTCONN => uve::UV_ENOTCONN,
        e::ENOTDIR => uve::UV_ENOTDIR,
        e::ENOTEMPTY => uve::UV_ENOTEMPTY,
        e::ENOTSOCK => uve::UV_ENOTSOCK,
        e::ENOTTY => uve::UV_ENOTTY,
        e::ENXIO => uve::UV_ENXIO,
        e::EOPNOTSUPP => uve::UV_ENOTSUP,
        e::EPERM => uve::UV_EPERM,
        e::EPIPE => uve::UV_EPIPE,
        e::EPROTO => uve::UV_EPROTO,
        e::EPROTONOSUPPORT => uve::UV_EPROTONOSUPPORT,
        e::EPROTOTYPE => uve::UV_EPROTOTYPE,
        e::ERANGE => uve::UV_ERANGE,
        e::EROFS => uve::UV_EROFS,
        e::ESPIPE => uve::UV_ESPIPE,
        e::ESRCH => uve::UV_ESRCH,
        e::ETIMEDOUT => uve::UV_ETIMEDOUT,
        e::ETXTBSY => uve::UV_ETXTBSY,
        e::EXDEV => uve::UV_EXDEV,
        e::ENODATA => uve::UV_ENODATA,
        e::ENOMSG => uve::UV_ENODATA,
        e::ENOEXEC => uve::UV_ENOEXEC,
        e::EBADMSG => uve::UV_EPROTO,
        e::ECHILD => uve::UV_ESRCH,
        e::EDEADLK => uve::UV_EBUSY,
        e::EDOM => uve::UV_EINVAL,
        e::EIDRM => uve::UV_EPIPE,
        e::EINPROGRESS => uve::UV_EISCONN,
        e::ENETRESET => uve::UV_ECONNRESET,
        e::ENOLCK => uve::UV_EAGAIN,
        e::ENOLINK => uve::UV_ECONNRESET,
        e::ENOSR => uve::UV_ENOBUFS,
        e::ENOSTR => uve::UV_EINVAL,
        e::ETIME => uve::UV_ETIMEDOUT,
        _ => -err,
    }
}

// =============================================================================================
// IO.Error values
// =============================================================================================

/// The `IO.Error` constructors exported by `Init/System/IOError.lean` (`lean_mk_io_error_*`),
/// called through [`crate::exports`] exactly as Lean's C runtime calls them. Every object
/// argument is owned.
pub mod mk {
    use super::*;
    use crate::exports::{Export as E, call_code, call_file, call1};

    pub unsafe fn user_error(msg: Obj) -> Obj {
        unsafe { call1(E::UserError, msg) }
    }
    pub fn eof() -> Obj {
        unsafe { call1(E::Eof, lean_box(0)) }
    }
    pub unsafe fn already_exists(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::AlreadyExists, code, details) }
    }
    pub unsafe fn already_exists_file(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::AlreadyExistsFile, fname, code, details) }
    }
    pub unsafe fn other_error(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::OtherError, code, details) }
    }
    pub unsafe fn resource_busy(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::ResourceBusy, code, details) }
    }
    pub unsafe fn resource_vanished(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::ResourceVanished, code, details) }
    }
    pub unsafe fn unsupported_operation(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::UnsupportedOperation, code, details) }
    }
    pub unsafe fn hardware_fault(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::HardwareFault, code, details) }
    }
    pub unsafe fn unsatisfied_constraints(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::UnsatisfiedConstraints, code, details) }
    }
    pub unsafe fn illegal_operation(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::IllegalOperation, code, details) }
    }
    pub unsafe fn protocol_error(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::ProtocolError, code, details) }
    }
    pub unsafe fn time_expired(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::TimeExpired, code, details) }
    }
    pub unsafe fn interrupted(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::Interrupted, fname, code, details) }
    }
    pub unsafe fn no_file_or_directory(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::NoFileOrDirectory, fname, code, details) }
    }
    pub unsafe fn invalid_argument(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::InvalidArgument, code, details) }
    }
    pub unsafe fn invalid_argument_file(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::InvalidArgumentFile, fname, code, details) }
    }
    pub unsafe fn permission_denied(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::PermissionDenied, code, details) }
    }
    pub unsafe fn permission_denied_file(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::PermissionDeniedFile, fname, code, details) }
    }
    pub unsafe fn resource_exhausted(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::ResourceExhausted, code, details) }
    }
    pub unsafe fn resource_exhausted_file(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::ResourceExhaustedFile, fname, code, details) }
    }
    pub unsafe fn inappropriate_type(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::InappropriateType, code, details) }
    }
    pub unsafe fn inappropriate_type_file(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::InappropriateTypeFile, fname, code, details) }
    }
    pub unsafe fn no_such_thing(code: u32, details: Obj) -> Obj {
        unsafe { call_code(E::NoSuchThing, code, details) }
    }
    pub unsafe fn no_such_thing_file(fname: Obj, code: u32, details: Obj) -> Obj {
        unsafe { call_file(E::NoSuchThingFile, fname, code, details) }
    }
}

/// Takes a new reference to the file name of an error that requires one. Lean's C runtime
/// dereferences the name unconditionally for these classes, so their absence is a violated
/// invariant of the caller.
unsafe fn required_fname(fname: Option<Obj>, class: &str) -> Obj {
    match fname {
        Some(f) => {
            unsafe { lean_inc(f) };
            f
        }
        None => lean_internal_panic(&format!("an `IO.Error.{class}` error requires a file name")),
    }
}

/// `decode_uv_error_impl`: classifies the libuv error `errnum` into an `IO.Error`, storing
/// `posix_errnum` as its OS code and libuv's message as its details. `fname` is borrowed.
pub(crate) unsafe fn decode_uv_error_impl(errnum: i32, posix_errnum: i32, fname: Option<Obj>) -> Obj {
    unsafe {
        let details = lean_mk_string(crate::uv::uv_strerror(errnum));
        let code = posix_errnum as u32;
        let file = |f: Option<Obj>| {
            f.inspect(|&f| {
                lean_inc(f);
            })
        };
        match errnum {
            uve::UV_EINTR => mk::interrupted(required_fname(fname, "interrupted"), code, details),
            uve::UV_ELOOP
            | uve::UV_ENAMETOOLONG
            | uve::UV_EDESTADDRREQ
            | uve::UV_EBADF
            | uve::UV_EINVAL
            | uve::UV_EILSEQ
            | uve::UV_ENOTCONN
            | uve::UV_ENOTSOCK
            | uve::UV_ENOEXEC => match file(fname) {
                None => mk::invalid_argument(code, details),
                Some(f) => mk::invalid_argument_file(f, code, details),
            },
            uve::UV_ENOENT => mk::no_file_or_directory(required_fname(fname, "noFileOrDirectory"), code, details),
            uve::UV_EACCES | uve::UV_EROFS | uve::UV_ECONNABORTED | uve::UV_EFBIG | uve::UV_EPERM => {
                match file(fname) {
                    None => mk::permission_denied(code, details),
                    Some(f) => mk::permission_denied_file(f, code, details),
                }
            }
            uve::UV_EMFILE
            | uve::UV_ENFILE
            | uve::UV_ENOSPC
            | uve::UV_E2BIG
            | uve::UV_EAGAIN
            | uve::UV_EMLINK
            | uve::UV_EMSGSIZE
            | uve::UV_ENOBUFS
            | uve::UV_ENOMEM => match file(fname) {
                None => mk::resource_exhausted(code, details),
                Some(f) => mk::resource_exhausted_file(f, code, details),
            },
            uve::UV_EISDIR | uve::UV_ENOTDIR => match file(fname) {
                None => mk::inappropriate_type(code, details),
                Some(f) => mk::inappropriate_type_file(f, code, details),
            },
            uve::UV_ENXIO
            | uve::UV_EHOSTUNREACH
            | uve::UV_ENETUNREACH
            | uve::UV_ECONNREFUSED
            | uve::UV_ENODATA
            | uve::UV_ESRCH => match file(fname) {
                None => mk::no_such_thing(code, details),
                Some(f) => mk::no_such_thing_file(f, code, details),
            },
            uve::UV_EEXIST | uve::UV_EISCONN => match file(fname) {
                None => mk::already_exists(code, details),
                Some(f) => mk::already_exists_file(f, code, details),
            },
            uve::UV_EIO => mk::hardware_fault(code, details),
            uve::UV_ENOTEMPTY => mk::unsatisfied_constraints(code, details),
            uve::UV_ENOTTY => mk::illegal_operation(code, details),
            uve::UV_ECONNRESET | uve::UV_ENETDOWN | uve::UV_EPIPE => mk::resource_vanished(code, details),
            uve::UV_EPROTO | uve::UV_EPROTONOSUPPORT | uve::UV_EPROTOTYPE => mk::protocol_error(code, details),
            uve::UV_ETIMEDOUT => mk::time_expired(code, details),
            uve::UV_EADDRINUSE | uve::UV_EBUSY | uve::UV_ETXTBSY => mk::resource_busy(code, details),
            uve::UV_EADDRNOTAVAIL
            | uve::UV_EAFNOSUPPORT
            | uve::UV_ENODEV
            | uve::UV_ENOPROTOOPT
            | uve::UV_ENOSYS
            | uve::UV_ENOTSUP
            | uve::UV_ERANGE
            | uve::UV_ESPIPE
            | uve::UV_EXDEV => mk::unsupported_operation(code, details),
            _ => mk::other_error(code, details),
        }
    }
}

/// `lean_decode_io_error`: an `IO.Error` for the C-runtime `errno` value `errnum`. `fname` is
/// borrowed.
pub(crate) unsafe fn decode_io_error(errnum: i32, fname: Option<Obj>) -> Obj {
    unsafe { decode_uv_error_impl(uv_code_of_errno(errnum), errnum, fname) }
}

/// `lean_decode_uv_error`: an `IO.Error` for the libuv error code `errnum`. `fname` is borrowed.
pub(crate) unsafe fn decode_uv_error(errnum: i32, fname: Option<Obj>) -> Obj {
    unsafe { decode_uv_error_impl(errnum, -errnum, fname) }
}

/// The C-runtime `errno` value corresponding to an operating-system error reported by the Rust
/// standard library: the `errno` itself on Unix, the C runtime's translation (`_dosmaperr`) of
/// the Win32 error code on Windows.
pub(crate) fn errno_of(e: &std::io::Error) -> i32 {
    #[cfg(unix)]
    {
        match e.raw_os_error() {
            Some(n) => n,
            None => lean_internal_panic(&format!("I/O error without an OS error code: {e}")),
        }
    }
    #[cfg(windows)]
    {
        match e.raw_os_error() {
            Some(n) => win::dosmaperr(n as u32),
            None => lean_internal_panic(&format!("I/O error without an OS error code: {e}")),
        }
    }
}

/// The libuv error code for an operating-system error reported by the Rust standard library,
/// as libuv computes it for its own system calls.
pub(crate) fn uv_code_of_io_error(e: &std::io::Error) -> i32 {
    match e.raw_os_error() {
        Some(n) => crate::uv::uv_code_of_os_error(n),
        None => lean_internal_panic(&format!("I/O error without an OS error code: {e}")),
    }
}

/// An `IO.Error` for an operating-system error of a C-runtime call, with an optional file
/// name.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn io_error_from_std(e: &std::io::Error, fname: Option<&str>) -> Obj {
    unsafe {
        let f = fname.map(lean_mk_string);
        let r = decode_io_error(errno_of(e), f);
        if let Some(f) = f {
            lean_dec(f);
        }
        r
    }
}

/// An `IO` error result with `IO.userError msg`.
pub(crate) unsafe fn io_result_mk_user_error(msg: &str) -> Obj {
    unsafe { lean_io_result_mk_error(mk::user_error(lean_mk_string(msg))) }
}

unsafe fn io_error_result(e: &std::io::Error, fname: Option<Obj>) -> Obj {
    unsafe { lean_io_result_mk_error(decode_io_error(errno_of(e), fname)) }
}

unsafe fn uv_error_result(e: &std::io::Error, fname: Option<Obj>) -> Obj {
    unsafe { lean_io_result_mk_error(decode_uv_error(uv_code_of_io_error(e), fname)) }
}

unsafe fn io_ok_unit() -> Obj {
    unsafe { lean_io_result_mk_ok(lean_box(0)) }
}

/// The `EINVAL` value of the C runtime.
fn einval() -> i32 {
    errno::EINVAL
}

/// `mk_embedded_nul_error`: file names containing NUL bytes cannot be passed to the OS.
unsafe fn embedded_nul_error(s: Obj) -> Obj {
    unsafe {
        lean_inc(s);
        lean_io_result_mk_error(mk::invalid_argument_file(
            s,
            einval() as u32,
            lean_mk_string("string contains NUL bytes"),
        ))
    }
}

/// The file name in the string object `s` (borrowed), or `None` if it contains NUL bytes.
unsafe fn path_arg<'a>(s: Obj) -> Option<&'a str> {
    let text = unsafe { lean_string_str(s) };
    if text.as_bytes().contains(&0) { None } else { Some(text) }
}

// =============================================================================================
// Windows support
// =============================================================================================

#[cfg(windows)]
mod win {
    use super::crt;
    use windows_sys::Win32::Foundation as f;

    /// The C runtime's `_dosmaperr`.
    pub fn dosmaperr(code: u32) -> i32 {
        match code {
            f::ERROR_INVALID_FUNCTION => crt::EINVAL,
            f::ERROR_FILE_NOT_FOUND | f::ERROR_PATH_NOT_FOUND => crt::ENOENT,
            f::ERROR_TOO_MANY_OPEN_FILES => crt::EMFILE,
            f::ERROR_ACCESS_DENIED => crt::EACCES,
            f::ERROR_INVALID_HANDLE => crt::EBADF,
            f::ERROR_ARENA_TRASHED | f::ERROR_NOT_ENOUGH_MEMORY | f::ERROR_INVALID_BLOCK => crt::ENOMEM,
            f::ERROR_BAD_ENVIRONMENT => crt::E2BIG,
            f::ERROR_BAD_FORMAT => crt::ENOEXEC,
            f::ERROR_INVALID_ACCESS | f::ERROR_INVALID_DATA => crt::EINVAL,
            f::ERROR_INVALID_DRIVE => crt::ENOENT,
            f::ERROR_CURRENT_DIRECTORY => crt::EACCES,
            f::ERROR_NOT_SAME_DEVICE => crt::EXDEV,
            f::ERROR_NO_MORE_FILES => crt::ENOENT,
            f::ERROR_LOCK_VIOLATION => crt::EACCES,
            f::ERROR_BAD_NETPATH => crt::ENOENT,
            f::ERROR_NETWORK_ACCESS_DENIED => crt::EACCES,
            f::ERROR_BAD_NET_NAME => crt::ENOENT,
            f::ERROR_FILE_EXISTS => crt::EEXIST,
            f::ERROR_CANNOT_MAKE | f::ERROR_FAIL_I24 => crt::EACCES,
            f::ERROR_INVALID_PARAMETER => crt::EINVAL,
            f::ERROR_NO_PROC_SLOTS => crt::EAGAIN,
            f::ERROR_DRIVE_LOCKED => crt::EACCES,
            f::ERROR_BROKEN_PIPE => crt::EPIPE,
            f::ERROR_DISK_FULL => crt::ENOSPC,
            f::ERROR_INVALID_TARGET_HANDLE => crt::EBADF,
            f::ERROR_WAIT_NO_CHILDREN | f::ERROR_CHILD_NOT_COMPLETE => crt::ECHILD,
            f::ERROR_DIRECT_ACCESS_HANDLE => crt::EBADF,
            f::ERROR_NEGATIVE_SEEK => crt::EINVAL,
            f::ERROR_SEEK_ON_DEVICE => crt::EACCES,
            f::ERROR_DIR_NOT_EMPTY => crt::ENOTEMPTY,
            f::ERROR_NOT_LOCKED => crt::EACCES,
            f::ERROR_BAD_PATHNAME => crt::ENOENT,
            f::ERROR_MAX_THRDS_REACHED => crt::EAGAIN,
            f::ERROR_LOCK_FAILED => crt::EACCES,
            f::ERROR_ALREADY_EXISTS => crt::EEXIST,
            f::ERROR_FILENAME_EXCED_RANGE => crt::ENOENT,
            f::ERROR_NESTING_NOT_ALLOWED => crt::EAGAIN,
            f::ERROR_NOT_ENOUGH_QUOTA => crt::ENOMEM,
            c if (f::ERROR_WRITE_PROTECT..=f::ERROR_SHARING_BUFFER_EXCEEDED).contains(&c) => crt::EACCES,
            c if (f::ERROR_INVALID_STARTING_CODESEG..=f::ERROR_INFLOOP_IN_RELOC_CHAIN).contains(&c) => crt::ENOEXEC,
            _ => crt::EINVAL,
        }
    }

    /// Lowercases the drive letter of `C:\...` paths, as Lean's runtime does.
    pub fn lowercase_drive(mut s: String) -> String {
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii() {
            let lower = (b[0] as char).to_ascii_lowercase();
            s.replace_range(0..1, &lower.to_string());
        }
        s
    }
}

// =============================================================================================
// File handles
// =============================================================================================

/// Bytes buffered before a fully buffered stream writes to its device.
const BUFFER_SIZE: usize = 8192;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Buffering {
    Full,
    Line,
    Unbuffered,
}

/// The underlying device of a handle.
enum Device {
    /// A file owned by the handle and closed with it.
    Owned(File),
    /// A standard stream of the process, never closed.
    Std(ManuallyDrop<File>),
}

impl Device {
    fn file(&mut self) -> &mut File {
        match self {
            Device::Owned(f) => f,
            Device::Std(f) => f,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StdKind {
    Stdin,
    Stdout,
    Stderr,
}

struct HandleState {
    device: Device,
    readable: bool,
    writable: bool,
    buffering: Buffering,
    std: Option<StdKind>,
    read_buf: Vec<u8>,
    read_pos: usize,
    write_buf: Vec<u8>,
    /// The `errno` of a failed device operation, sticky until cleared like the `FILE` error flag.
    error: Option<i32>,
}

/// A buffered file handle with the observable behaviour of a C `FILE`.
pub(crate) struct Handle {
    state: Mutex<HandleState>,
}

/// Every open handle, so that process exit flushes buffered output as C's `exit` does.
static OPEN_HANDLES: Mutex<Vec<Weak<Handle>>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

impl Handle {
    fn new(device: Device, readable: bool, writable: bool, buffering: Buffering, std: Option<StdKind>) -> Arc<Handle> {
        let h = Arc::new(Handle {
            state: Mutex::new(HandleState {
                device,
                readable,
                writable,
                buffering,
                std,
                read_buf: Vec::new(),
                read_pos: 0,
                write_buf: Vec::new(),
                error: None,
            }),
        });
        let mut open = lock(&OPEN_HANDLES);
        open.retain(|w| w.strong_count() > 0);
        open.push(Arc::downgrade(&h));
        h
    }

    fn lock(&self) -> MutexGuard<'_, HandleState> {
        lock(&self.state)
    }
}

fn ebadf() -> std::io::Error {
    #[cfg(unix)]
    {
        std::io::Error::from_raw_os_error(libc::EBADF)
    }
    #[cfg(windows)]
    {
        std::io::Error::from_raw_os_error(windows_sys::Win32::Foundation::ERROR_INVALID_HANDLE as i32)
    }
}

impl HandleState {
    /// Writes all buffered output to the device.
    fn flush_writes(&mut self) -> std::io::Result<()> {
        if self.write_buf.is_empty() {
            return Ok(());
        }
        let buf = std::mem::take(&mut self.write_buf);
        let r = self.device.file().write_all(&buf);
        if let Err(e) = &r {
            self.error = Some(errno_of(e));
        }
        r
    }

    /// Discards buffered input, moving the device position back to the logical position.
    fn drop_reads(&mut self) -> std::io::Result<()> {
        let unread = self.read_buf.len() - self.read_pos;
        self.read_buf.clear();
        self.read_pos = 0;
        if unread > 0 {
            self.device.file().seek(SeekFrom::Current(-(unread as i64)))?;
        }
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        if !self.writable {
            let e = ebadf();
            self.error = Some(errno_of(&e));
            return Err(e);
        }
        if self.read_pos < self.read_buf.len() {
            self.drop_reads()?;
        } else {
            self.read_buf.clear();
            self.read_pos = 0;
        }
        match self.buffering {
            Buffering::Unbuffered => {
                self.flush_writes()?;
                let r = self.device.file().write_all(data);
                if let Err(e) = &r {
                    self.error = Some(errno_of(e));
                }
                r
            }
            Buffering::Line => {
                self.write_buf.extend_from_slice(data);
                if data.contains(&b'\n') || self.write_buf.len() >= BUFFER_SIZE {
                    self.flush_writes()?;
                }
                Ok(())
            }
            Buffering::Full => {
                self.write_buf.extend_from_slice(data);
                if self.write_buf.len() >= BUFFER_SIZE {
                    self.flush_writes()?;
                }
                Ok(())
            }
        }
    }

    /// Reads more input into the buffer. Returns `Ok(false)` at end of input.
    fn fill(&mut self) -> std::io::Result<bool> {
        if !self.readable {
            let e = ebadf();
            self.error = Some(errno_of(&e));
            return Err(e);
        }
        self.flush_writes()?;
        if self.std == Some(StdKind::Stdin) {
            // Like the C library, reading from an interactive input stream first flushes
            // line-buffered output.
            flush_line_buffered_stdout();
        }
        self.read_buf.resize(BUFFER_SIZE, 0);
        self.read_pos = 0;
        loop {
            match self.device.file().read(&mut self.read_buf) {
                Ok(n) => {
                    self.read_buf.truncate(n);
                    return Ok(n > 0);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.read_buf.clear();
                    self.error = Some(errno_of(&e));
                    return Err(e);
                }
            }
        }
    }

    /// Reads up to `out.len()` bytes, stopping early only at end of input or on an error, as
    /// `fread` does. Returns the number of bytes read and the error, if one occurred.
    fn read(&mut self, out: &mut [u8]) -> (usize, Option<std::io::Error>) {
        let mut n = 0;
        while n < out.len() {
            if self.read_pos == self.read_buf.len() {
                match self.fill() {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => return (n, Some(e)),
                }
            }
            let available = &self.read_buf[self.read_pos..];
            let k = available.len().min(out.len() - n);
            out[n..n + k].copy_from_slice(&available[..k]);
            self.read_pos += k;
            n += k;
        }
        (n, None)
    }

    /// Reads a line including its terminating newline, as Lean's `getLine` does.
    fn get_line(&mut self) -> std::io::Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            if self.read_pos == self.read_buf.len() {
                match self.fill() {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => return Err(e),
                }
            }
            let available = &self.read_buf[self.read_pos..];
            match available.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    line.extend_from_slice(&available[..=i]);
                    self.read_pos += i + 1;
                    return Ok(line);
                }
                None => {
                    line.extend_from_slice(available);
                    self.read_pos = self.read_buf.len();
                }
            }
        }
        Ok(line)
    }

    /// The logical stream position, accounting for buffered input and output.
    fn position(&mut self) -> std::io::Result<u64> {
        self.flush_writes()?;
        let pos = self.device.file().stream_position()?;
        Ok(pos - (self.read_buf.len() - self.read_pos) as u64)
    }

    fn seek_start(&mut self) -> std::io::Result<()> {
        self.flush_writes()?;
        self.read_buf.clear();
        self.read_pos = 0;
        self.device.file().seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

/// Handles are external objects holding an `Arc<Handle>`; finalizing one flushes and closes the
/// handle, ignoring errors as `fclose` in a finalizer does.
static HANDLE_CLASS: ExternalClass = ExternalClass { finalize: finalize_handle, for_each: for_each_handle };

unsafe fn finalize_handle(data: *mut ()) {
    let h = unsafe { Box::from_raw(data as *mut Arc<Handle>) };
    let _ = h.lock().flush_writes();
    drop(h);
}

unsafe fn for_each_handle(_data: *mut (), _f: &mut dyn FnMut(Obj)) {}

pub(crate) fn wrap_handle(h: Arc<Handle>) -> Obj {
    unsafe { lean_alloc_external(&HANDLE_CLASS, Box::into_raw(Box::new(h)) as *mut ()) }
}

/// Wraps a file opened by the runtime (for example a pipe to a child process) as a Lean handle.
pub(crate) fn wrap_file(file: File, readable: bool, writable: bool) -> Obj {
    wrap_handle(Handle::new(Device::Owned(file), readable, writable, Buffering::Full, None))
}

/// The handle stored in the Lean handle object `h` (borrowed).
unsafe fn handle_of<'a>(h: Obj) -> &'a Handle {
    unsafe {
        if lean_ptr_tag(h) != LEAN_EXTERNAL || !std::ptr::eq(lean_get_external_class(h), &HANDLE_CLASS) {
            lean_internal_panic("expected an IO.FS.Handle object");
        }
        &*(lean_get_external_data(h) as *const Arc<Handle>)
    }
}

// =============================================================================================
// Standard streams
// =============================================================================================

static STD_HANDLES: OnceLock<[Arc<Handle>; 3]> = OnceLock::new();

fn std_handles() -> &'static [Arc<Handle>; 3] {
    STD_HANDLES.get_or_init(|| {
        #[cfg(unix)]
        let open = |fd: i32| {
            use std::os::fd::FromRawFd;
            // The process's standard descriptors are borrowed for the lifetime of the process.
            ManuallyDrop::new(unsafe { File::from_raw_fd(fd) })
        };
        #[cfg(windows)]
        let open = |which: u32| {
            use std::os::windows::io::FromRawHandle;
            let h = unsafe { windows_sys::Win32::System::Console::GetStdHandle(which) };
            ManuallyDrop::new(unsafe { File::from_raw_handle(h as _) })
        };
        #[cfg(unix)]
        let (fin, fout, ferr) = (open(0), open(1), open(2));
        #[cfg(windows)]
        let (fin, fout, ferr) = {
            use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
            (open(STD_INPUT_HANDLE), open(STD_OUTPUT_HANDLE), open(STD_ERROR_HANDLE))
        };
        let stdout_buffering = if is_tty_file(&fout) { Buffering::Line } else { Buffering::Full };
        [
            Handle::new(Device::Std(fin), true, false, Buffering::Full, Some(StdKind::Stdin)),
            Handle::new(Device::Std(fout), false, true, stdout_buffering, Some(StdKind::Stdout)),
            Handle::new(Device::Std(ferr), false, true, Buffering::Unbuffered, Some(StdKind::Stderr)),
        ]
    })
}

fn flush_line_buffered_stdout() {
    if let Some(handles) = STD_HANDLES.get() {
        let mut s = handles[1].lock();
        if s.buffering == Buffering::Line {
            let _ = s.flush_writes();
        }
    }
}

fn is_tty_file(f: &File) -> bool {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        unsafe { libc::isatty(f.as_raw_fd()) == 1 }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let mut mode = 0u32;
        unsafe { windows_sys::Win32::System::Console::GetConsoleMode(f.as_raw_handle() as _, &mut mode) != 0 }
    }
}

/// The initial standard streams: `IO.FS.Stream.ofHandle` of the standard handles, persistent.
static DEFAULT_STREAMS: OnceLock<[SendObj; 3]> = OnceLock::new();

fn default_streams() -> &'static [SendObj; 3] {
    DEFAULT_STREAMS.get_or_init(|| {
        let handles = std_handles();
        let mk = |i: usize| unsafe {
            let s = crate::exports::call1(crate::exports::Export::StreamOfHandle, wrap_handle(handles[i].clone()));
            lean_mark_persistent(s);
            SendObj(s)
        };
        [mk(0), mk(1), mk(2)]
    })
}

/// The current thread's standard streams; each slot owns a reference.
struct CurrentStreams([Option<Obj>; 3]);

impl Drop for CurrentStreams {
    fn drop(&mut self) {
        for s in self.0.iter().flatten() {
            unsafe { lean_dec(*s) };
        }
    }
}

thread_local! {
    static CURRENT_STREAMS: RefCell<CurrentStreams> = const { RefCell::new(CurrentStreams([None, None, None])) };
}

/// A new reference to the current thread's stream `i` (0: stdin, 1: stdout, 2: stderr).
fn get_stream(i: usize) -> Obj {
    CURRENT_STREAMS.with(|c| {
        let mut c = c.borrow_mut();
        let s = *c.0[i].get_or_insert_with(|| default_streams()[i].0);
        unsafe { lean_inc(s) };
        s
    })
}

/// Replaces the current thread's stream `i` with `h` (owned), returning the previous one
/// (owned).
fn set_stream(i: usize, h: Obj) -> Obj {
    CURRENT_STREAMS.with(|c| {
        let mut c = c.borrow_mut();
        let old = c.0[i].unwrap_or_else(|| default_streams()[i].0);
        c.0[i] = Some(h);
        old
    })
}

/// Prints `s` and a newline to the current Lean standard error stream (`IO.eprintln`), as the C
/// runtime's `io_eprintln` does.
pub fn io_eprintln(s: &str) {
    unsafe {
        let r = crate::exports::call1(crate::exports::Export::IoEprintln, lean_mk_string(s));
        lean_dec(r);
    }
}

/// Flushes the buffered output of every open handle, as C's `exit` flushes every `FILE`.
pub fn flush_stdio() {
    let handles: Vec<Arc<Handle>> = lock(&OPEN_HANDLES).iter().filter_map(Weak::upgrade).collect();
    for h in handles {
        let _ = h.lock().flush_writes();
    }
}

/// Flushes the buffered output of every open handle in a child process between `fork` and
/// `exec`. Lean's runtime ends a child that cannot run its program with C's `exit`, which
/// flushes the `FILE` buffers the child inherited, so output the parent had buffered is written
/// again, to the child's streams. Only async-signal-safe operations are used: nothing is
/// allocated or waited for, and a handle that was locked when the process forked is skipped.
///
/// # Safety
///
/// Must only be called in a forked child that will exit without returning to Rust code.
#[cfg(unix)]
pub(crate) unsafe fn flush_stdio_in_forked_child() {
    use std::os::fd::AsRawFd;
    use std::sync::TryLockError;
    let open = match OPEN_HANDLES.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    for weak in open.iter() {
        let Some(handle) = weak.upgrade() else { continue };
        {
            let state = match handle.state.try_lock() {
                Ok(g) => Some(g),
                Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
                Err(TryLockError::WouldBlock) => None,
            };
            if let Some(mut state) = state {
                let fd = state.device.file().as_raw_fd();
                let mut rest: &[u8] = &state.write_buf;
                while !rest.is_empty() {
                    let n = unsafe { libc::write(fd, rest.as_ptr() as *const libc::c_void, rest.len()) };
                    if n <= 0 {
                        break;
                    }
                    rest = &rest[n as usize..];
                }
            }
        }
        // The child exits without returning; releasing the reference could only deallocate.
        std::mem::forget(handle);
    }
}

static INITIALIZING: AtomicBool = AtomicBool::new(true);

/// Marks the end of module initialization (`lean_io_mark_end_initialization`).
pub fn mark_end_initialization() {
    INITIALIZING.store(false, Ordering::Relaxed);
}

// =============================================================================================
// Heartbeats
// =============================================================================================

thread_local! {
    static HEARTBEATS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Counts one heartbeat (`lean_inc_heartbeat`): Lean's runtime counts one per small-object
/// allocation.
#[inline]
pub fn inc_heartbeat() {
    HEARTBEATS.with(|h| h.set(h.get().wrapping_add(1)));
}

// =============================================================================================
// Clocks
// =============================================================================================

/// Nanoseconds of the monotonic clock C++'s `std::chrono::steady_clock` uses.
fn steady_nanos() -> u64 {
    #[cfg(target_vendor = "apple")]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut ts) };
        ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
    }
    #[cfg(all(unix, not(target_vendor = "apple")))]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
    }
    #[cfg(windows)]
    {
        // The absolute origin of `steady_clock` is unspecified; anchor a high-resolution
        // monotonic clock at the system uptime when first used.
        static ORIGIN: OnceLock<(std::time::Instant, u64)> = OnceLock::new();
        let (start, base) = ORIGIN.get_or_init(|| {
            let ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
            (std::time::Instant::now(), ms * 1_000_000)
        });
        base + start.elapsed().as_nanos() as u64
    }
}

/// Formats `v` as C++ iostreams do with `std::setprecision(prec)` and the default float field
/// (`%g`).
fn format_g(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    let exp = format!("{:.*e}", prec - 1, v);
    let (mantissa, e) = exp.split_once('e').expect("exponent notation");
    let e: i32 = e.parse().expect("exponent");
    let trim = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s.to_owned() }
    };
    if e < -4 || e >= prec as i32 {
        let sign = if e < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", trim(mantissa), sign, e.abs())
    } else {
        let decimals = (prec as i32 - 1 - e).max(0) as usize;
        trim(&format!("{:.*}", decimals, v))
    }
}

// =============================================================================================
// Primitives
// =============================================================================================

#[cfg(unix)]
fn os_str_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_str_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

/// The system temporary directory as libuv's `uv_os_tmpdir` computes it.
fn uv_os_tmpdir() -> Result<String, i32> {
    #[cfg(unix)]
    {
        for var in ["TMPDIR", "TMP", "TEMP", "TEMPDIR"] {
            if let Some(v) = std::env::var_os(var)
                && !v.is_empty()
            {
                let mut s = String::from_utf8_lossy(&os_str_bytes(&v)).into_owned();
                if s.len() > 1 && s.ends_with('/') {
                    s.pop();
                }
                return Ok(s);
            }
        }
        Ok("/tmp".to_owned())
    }
    #[cfg(windows)]
    {
        let mut s = std::env::temp_dir().to_string_lossy().into_owned();
        // libuv strips the trailing separator unless the path is a drive root such as `C:\`.
        if s.ends_with('\\') && !(s.len() == 3 && s.as_bytes()[1] == b':') {
            s.pop();
        }
        if s.is_empty() {
            return Err(uve::UV_ENOENT);
        }
        Ok(s)
    }
}

/// The temporary-file template Lean's runtime builds: `<tmpdir>/tmp.XXXXXXXX`.
unsafe fn temp_template() -> Result<String, Obj> {
    match uv_os_tmpdir() {
        Err(code) => Err(unsafe { lean_io_result_mk_error(decode_uv_error(code, None)) }),
        Ok(dir) if dir.is_empty() => unsafe {
            let empty = lean_mk_string("");
            let e = decode_uv_error(uve::UV_ENOENT, Some(empty));
            lean_dec(empty);
            Err(lean_io_result_mk_error(e))
        },
        Ok(mut dir) => {
            let sep = if cfg!(windows) { '\\' } else { '/' };
            if !dir.ends_with(sep) {
                dir.push(sep);
            }
            dir.push_str("tmp.XXXXXXXX");
            Ok(dir)
        }
    }
}

/// Replaces the trailing six `X` characters with random ones, as libuv's Windows `mkstemp`
/// and `mkdtemp` do.
#[cfg(windows)]
fn fill_template(template: &str) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut bytes = [0u8; 6];
    if getrandom::fill(&mut bytes).is_err() {
        lean_internal_panic("the system random number generator failed");
    }
    let mut s = template[..template.len() - 6].to_owned();
    for b in bytes {
        s.push(CHARS[b as usize % CHARS.len()] as char);
    }
    s
}

/// `uv_fs_mkstemp`: creates a unique file from `template`, returning its path and file.
fn mkstemp(template: &str) -> Result<(String, File), i32> {
    #[cfg(unix)]
    {
        let mut buf = std::ffi::CString::new(template).expect("template without NUL").into_bytes_with_nul();
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let fd = unsafe { libc::mkostemp(buf.as_mut_ptr() as *mut libc::c_char, libc::O_CLOEXEC) };
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let fd = unsafe {
            let fd = libc::mkstemp(buf.as_mut_ptr() as *mut libc::c_char);
            if fd >= 0 {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
            fd
        };
        if fd < 0 {
            return Err(-std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO));
        }
        buf.pop();
        let path = String::from_utf8(buf).expect("mkstemp keeps the template's UTF-8");
        use std::os::fd::FromRawFd;
        Ok((path, unsafe { File::from_raw_fd(fd) }))
    }
    #[cfg(windows)]
    {
        const TMP_MAX: usize = 32767;
        for _ in 0..TMP_MAX {
            let path = fill_template(template);
            match std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
                Ok(f) => return Ok((path, f)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(uv_code_of_io_error(&e)),
            }
        }
        Err(uve::UV_EEXIST)
    }
}

/// `uv_fs_mkdtemp`: creates a unique directory from `template`.
fn mkdtemp(template: &str) -> Result<String, i32> {
    #[cfg(unix)]
    {
        let mut buf = std::ffi::CString::new(template).expect("template without NUL").into_bytes_with_nul();
        let r = unsafe { libc::mkdtemp(buf.as_mut_ptr() as *mut libc::c_char) };
        if r.is_null() {
            return Err(-std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO));
        }
        buf.pop();
        Ok(String::from_utf8(buf).expect("mkdtemp keeps the template's UTF-8"))
    }
    #[cfg(windows)]
    {
        const TMP_MAX: usize = 32767;
        for _ in 0..TMP_MAX {
            let path = fill_template(template);
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(uv_code_of_io_error(&e)),
            }
        }
        Err(uve::UV_EEXIST)
    }
}

/// `IO.FS.SystemTime` from seconds and nanoseconds.
unsafe fn system_time(sec: i64, nsec: u32) -> Obj {
    unsafe {
        let o = lean_alloc_ctor(0, 1, 4);
        lean_ctor_set(o, 0, crate::int::lean_int64_to_int(sec));
        lean_ctor_set_uint32(o, size_of::<Obj>(), nsec);
        o
    }
}

struct Stat {
    atime: (i64, u32),
    mtime: (i64, u32),
    size: u64,
    nlink: u64,
    /// `IO.FS.FileType` constructor index.
    kind: u8,
}

/// `metadata_core`: builds `IO.FS.Metadata`.
unsafe fn metadata_obj(st: &Stat) -> Obj {
    unsafe {
        let p = size_of::<Obj>();
        let o = lean_alloc_ctor(0, 2, 2 * 8 + 1);
        lean_ctor_set(o, 0, system_time(st.atime.0, st.atime.1));
        lean_ctor_set(o, 1, system_time(st.mtime.0, st.mtime.1));
        lean_ctor_set_uint64(o, 2 * p, st.size);
        lean_ctor_set_uint64(o, 2 * p + 8, st.nlink);
        lean_ctor_set_uint8(o, 2 * p + 16, st.kind);
        lean_io_result_mk_ok(o)
    }
}

#[cfg(unix)]
fn stat_of(m: &std::fs::Metadata) -> Stat {
    use std::os::unix::fs::MetadataExt;
    let ft = m.file_type();
    Stat {
        atime: (m.atime(), m.atime_nsec() as u32),
        mtime: (m.mtime(), m.mtime_nsec() as u32),
        size: m.size(),
        nlink: m.nlink(),
        kind: if ft.is_dir() {
            0
        } else if ft.is_file() {
            1
        } else if ft.is_symlink() {
            2
        } else {
            3
        },
    }
}

#[cfg(windows)]
fn stat_path(path: &str) -> std::io::Result<Stat> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem as fs;
    // libuv opens the file for attribute access only, following reparse points.
    let file = std::fs::OpenOptions::new()
        .access_mode(fs::FILE_READ_ATTRIBUTES)
        .share_mode(fs::FILE_SHARE_READ | fs::FILE_SHARE_WRITE | fs::FILE_SHARE_DELETE)
        .custom_flags(fs::FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info: fs::BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { fs::GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let ts = |ft: windows_sys::Win32::Foundation::FILETIME| -> (i64, u32) {
        // `uv__filetime_to_timespec`: 100ns intervals since 1601-01-01.
        let t = ((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64) as i64 - 116_444_736_000_000_000;
        let sec = t.div_euclid(10_000_000);
        let nsec = t.rem_euclid(10_000_000) * 100;
        (sec, nsec as u32)
    };
    Ok(Stat {
        atime: ts(info.ftLastAccessTime),
        mtime: ts(info.ftLastWriteTime),
        size: (info.nFileSizeHigh as u64) << 32 | info.nFileSizeLow as u64,
        nlink: info.nNumberOfLinks as u64,
        kind: if info.dwFileAttributes & fs::FILE_ATTRIBUTE_DIRECTORY != 0 { 0 } else { 1 },
    })
}

unsafe fn metadata_impl(filename: Obj, follow: bool) -> Obj {
    unsafe {
        let Some(path) = path_arg(filename) else { return embedded_nul_error(filename) };
        #[cfg(unix)]
        let r = if follow { std::fs::metadata(path) } else { std::fs::symlink_metadata(path) }.map(|m| stat_of(&m));
        #[cfg(windows)]
        let r = {
            let _ = follow;
            stat_path(path)
        };
        match r {
            Ok(st) => metadata_obj(&st),
            Err(e) => uv_error_result(&e, Some(filename)),
        }
    }
}

/// The user-visible failure message of the Windows API calls whose errors Lean's runtime
/// reports as the numeric `GetLastError` value.
#[cfg(windows)]
fn last_error_code(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => (n as u32).to_string(),
        None => lean_internal_panic(&format!("I/O error without an OS error code: {e}")),
    }
}

/// Reports an error of a handle operation that Lean's runtime reports via `decode_io_error` on
/// Unix and as the numeric Windows error code on Windows.
unsafe fn lock_error(e: &std::io::Error) -> Obj {
    #[cfg(unix)]
    {
        unsafe { io_error_result(e, None) }
    }
    #[cfg(windows)]
    {
        unsafe { io_result_mk_user_error(&last_error_code(e)) }
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_io_initializing() -> u8 {
            INITIALIZING.load(Ordering::Relaxed) as u8
        }

        /* IO.setAccessRights (filename : @& String) (mode : UInt32) : IO Unit */
        fn lean_chmod(filename: b_obj, mode: u32) -> obj {
            let Some(path) = path_arg(filename) else { return embedded_nul_error(filename) };
            #[cfg(unix)]
            let r = {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            };
            #[cfg(windows)]
            let r = {
                // The C runtime's `chmod` only controls the read-only attribute via `_S_IWRITE`.
                std::fs::metadata(path).and_then(|m| {
                    let mut p = m.permissions();
                    p.set_readonly(mode & 0o200 == 0);
                    std::fs::set_permissions(path, p)
                })
            };
            match r {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, Some(filename)),
            }
        }

        /* Handle.mk (filename : @& String) (mode : FS.Mode) : IO Handle */
        fn lean_io_prim_handle_mk(filename: b_obj, mode: u8) -> obj {
            let mut opts = std::fs::OpenOptions::new();
            let (readable, writable) = match mode {
                0 => { opts.read(true); (true, false) }
                1 => { opts.write(true).create(true).truncate(true); (false, true) }
                2 => { opts.write(true).create_new(true); (false, true) }
                3 => { opts.read(true).write(true); (true, true) }
                4 => { opts.append(true).create(true); (false, true) }
                _ => lean_internal_panic(&format!("invalid IO.FS.Mode {mode}")),
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o666);
            }
            let Some(path) = path_arg(filename) else { return embedded_nul_error(filename) };
            match opts.open(path) {
                Ok(f) => lean_io_result_mk_ok(wrap_handle(Handle::new(
                    Device::Owned(f), readable, writable, Buffering::Full, None,
                ))),
                Err(e) => io_error_result(&e, Some(filename)),
            }
        }

        /* Handle.lock : (@& Handle) → (exclusive : Bool) → IO Unit */
        fn lean_io_prim_handle_lock(h: b_obj, exclusive: u8) -> obj {
            let mut s = handle_of(h).lock();
            let f = s.device.file();
            let r = if exclusive != 0 { f.lock() } else { f.lock_shared() };
            match r {
                Ok(()) => io_ok_unit(),
                Err(e) => lock_error(&e),
            }
        }

        /* Handle.tryLock : (@& Handle) → (exclusive : Bool) → IO Bool */
        fn lean_io_prim_handle_try_lock(h: b_obj, exclusive: u8) -> obj {
            let mut s = handle_of(h).lock();
            let f = s.device.file();
            let r = if exclusive != 0 { f.try_lock() } else { f.try_lock_shared() };
            match r {
                Ok(()) => lean_io_result_mk_ok(lean_box(1)),
                Err(std::fs::TryLockError::WouldBlock) => lean_io_result_mk_ok(lean_box(0)),
                Err(std::fs::TryLockError::Error(e)) => lock_error(&e),
            }
        }

        /* Handle.unlock : (@& Handle) → IO Unit */
        fn lean_io_prim_handle_unlock(h: b_obj) -> obj {
            let mut s = handle_of(h).lock();
            match s.device.file().unlock() {
                Ok(()) => io_ok_unit(),
                #[cfg(windows)]
                Err(e) if e.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_NOT_LOCKED as i32) => {
                    // For consistency with Unix.
                    io_ok_unit()
                }
                Err(e) => lock_error(&e),
            }
        }

        /* Handle.isTty : (@& Handle) → BaseIO Bool */
        fn lean_io_prim_handle_is_tty(h: b_obj) -> u8 {
            let mut s = handle_of(h).lock();
            is_tty_file(s.device.file()) as u8
        }

        /* Handle.flush : (@& Handle) → IO Unit */
        fn lean_io_prim_handle_flush(h: b_obj) -> obj {
            let mut s = handle_of(h).lock();
            match s.flush_writes().and_then(|()| s.device.file().flush()) {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, None),
            }
        }

        /* Handle.rewind : (@& Handle) → IO Unit */
        fn lean_io_prim_handle_rewind(h: b_obj) -> obj {
            let mut s = handle_of(h).lock();
            match s.seek_start() {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, None),
            }
        }

        /* Handle.truncate : (@& Handle) → IO Unit */
        fn lean_io_prim_handle_truncate(h: b_obj) -> obj {
            let mut s = handle_of(h).lock();
            let r = s.position().and_then(|pos| s.device.file().set_len(pos));
            match r {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, None),
            }
        }

        /* Handle.read : (@& Handle) → USize → IO ByteArray */
        fn lean_io_prim_handle_read(h: b_obj, nbytes: usize) -> obj {
            if nbytes > isize::MAX as usize - 64 {
                return lean_io_result_mk_error(decode_io_error(errno::ENOMEM, None));
            }
            let res = lean_alloc_sarray(1, 0, nbytes);
            if nbytes == 0 {
                return lean_io_result_mk_ok(res);
            }
            let out = std::slice::from_raw_parts_mut(lean_sarray_cptr(res), nbytes);
            let mut s = handle_of(h).lock();
            let (n, err) = s.read(out);
            if n > 0 || err.is_none() {
                if err.is_none() {
                    // End of input: `clearerr`.
                    s.error = None;
                }
                lean_sarray_set_size(res, n);
                lean_io_result_mk_ok(res)
            } else {
                lean_dec(res);
                io_error_result(&err.expect("read error"), None)
            }
        }

        /* Handle.write : (@& Handle) → (@& ByteArray) → IO Unit */
        fn lean_io_prim_handle_write(h: b_obj, buf: b_obj) -> obj {
            let data = std::slice::from_raw_parts(lean_sarray_cptr(buf), lean_sarray_size(buf));
            match handle_of(h).lock().write(data) {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, None),
            }
        }

        /* Handle.getLine : (@& Handle) → IO String */
        fn lean_io_prim_handle_get_line(h: b_obj) -> obj {
            let mut s = handle_of(h).lock();
            match s.get_line() {
                Ok(line) => {
                    if let Some(errnum) = s.error {
                        // The stream's error indicator was set by an earlier failed operation.
                        return lean_io_result_mk_error(decode_io_error(errnum, None));
                    }
                    lean_io_result_mk_ok(lean_mk_string_from_bytes(&line))
                }
                Err(e) => io_error_result(&e, None),
            }
        }

        /* Handle.putStr : (@& Handle) → (@& String) → IO Unit */
        fn lean_io_prim_handle_put_str(h: b_obj, s: b_obj) -> obj {
            match handle_of(h).lock().write(lean_string_bytes(s)) {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, None),
            }
        }

        /* getStdin : BaseIO FS.Stream */
        fn lean_get_stdin() -> obj {
            get_stream(0)
        }

        /* getStdout : BaseIO FS.Stream */
        fn lean_get_stdout() -> obj {
            get_stream(1)
        }

        /* getStderr : BaseIO FS.Stream */
        fn lean_get_stderr() -> obj {
            get_stream(2)
        }

        /* setStdin : FS.Stream → BaseIO FS.Stream */
        fn lean_get_set_stdin(h: obj) -> obj {
            set_stream(0, h)
        }

        /* setStdout : FS.Stream → BaseIO FS.Stream */
        fn lean_get_set_stdout(h: obj) -> obj {
            set_stream(1, h)
        }

        /* setStderr : FS.Stream → BaseIO FS.Stream */
        fn lean_get_set_stderr(h: obj) -> obj {
            set_stream(2, h)
        }

        /* Std.Time.Timestamp.now : IO Timestamp */
        fn lean_get_current_time() -> obj {
            let nanos: i128 = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_nanos() as i128,
                Err(e) => -(e.duration().as_nanos() as i128),
            };
            let secs = (nanos / 1_000_000_000) as i64;
            let nano = (nanos % 1_000_000_000) as i64;
            let ts = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(ts, 0, crate::int::lean_int64_to_int(secs));
            lean_ctor_set(ts, 1, crate::int::lean_int64_to_int(nano));
            lean_io_result_mk_ok(ts)
        }

        /* Std.Time.Database.Windows.getNextTransition : @&String → Int64 → Bool → IO (Option (Int64 × TimeZone)) */
        fn lean_windows_get_next_transition(timezone: b_obj, tm: u64, default_time: u8) -> obj {
            #[cfg(windows)]
            {
                icu::next_transition(lean_string_bytes(timezone), tm as i64, default_time != 0)
            }
            #[cfg(not(windows))]
            {
                let _ = (timezone, tm, default_time);
                lean_io_result_mk_error(mk::invalid_argument(
                    einval() as u32,
                    lean_mk_string("failed to get timezone, its windows only."),
                ))
            }
        }

        /* Std.Time.Database.Windows.getLocalTimeZoneIdentifierAt : Int64 → IO String */
        fn lean_get_windows_local_timezone_id_at(tm: u64) -> obj {
            #[cfg(windows)]
            {
                icu::local_timezone_id_at(tm as i64)
            }
            #[cfg(not(windows))]
            {
                let _ = tm;
                lean_io_result_mk_error(mk::invalid_argument(
                    einval() as u32,
                    lean_mk_string("timezone retrieval is Windows-only"),
                ))
            }
        }

        /* monoMsNow : BaseIO Nat */
        fn lean_io_mono_ms_now() -> obj {
            crate::nat::lean_uint64_to_nat(steady_nanos() / 1_000_000)
        }

        /* monoNanosNow : BaseIO Nat */
        fn lean_io_mono_nanos_now() -> obj {
            crate::nat::lean_uint64_to_nat(steady_nanos())
        }

        /* getRandomBytes (nBytes : USize) : IO ByteArray */
        fn lean_io_get_random_bytes(nbytes: usize) -> obj {
            if nbytes == 0 {
                return lean_io_result_mk_ok(lean_alloc_sarray(1, 0, 0));
            }
            #[cfg(unix)]
            let mut urandom = match File::open("/dev/urandom") {
                Ok(f) => f,
                Err(e) => {
                    let name = lean_mk_string("/dev/urandom");
                    let r = io_error_result(&e, Some(name));
                    lean_dec(name);
                    return r;
                }
            };
            if nbytes > isize::MAX as usize - 64 {
                return lean_io_result_mk_error(decode_io_error(errno::ENOMEM, None));
            }
            let res = lean_alloc_sarray(1, 0, nbytes);
            let out = std::slice::from_raw_parts_mut(lean_sarray_cptr(res), nbytes);
            #[cfg(unix)]
            {
                let mut n = 0;
                while n < nbytes {
                    match urandom.read(&mut out[n..]) {
                        Ok(k) => n += k,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(e) => {
                            lean_dec(res);
                            return io_error_result(&e, None);
                        }
                    }
                }
            }
            #[cfg(windows)]
            {
                if getrandom::fill(out).is_err() {
                    lean_dec(res);
                    return io_result_mk_user_error("BCryptGenRandom failed");
                }
            }
            lean_sarray_set_size(res, nbytes);
            lean_io_result_mk_ok(res)
        }

        /* timeit {α : Type} (msg : @& String) (fn : IO α) : IO α */
        fn lean_io_timeit(msg: b_obj, f: obj) -> obj {
            let start = std::time::Instant::now();
            let r = crate::apply::lean_apply_1(f, lean_box(0));
            let secs = start.elapsed().as_secs_f64();
            let text = if secs < 1.0 {
                format!("{} {}ms", lean_string_str(msg), format_g(secs * 1000.0, 3))
            } else {
                format!("{} {}s", lean_string_str(msg), format_g(secs, 3))
            };
            io_eprintln(&text);
            r
        }

        /* allocprof {α : Type} (msg : @& String) (fn : IO α) : IO α */
        fn lean_io_allocprof(msg: b_obj, f: obj) -> obj {
            let r = crate::apply::lean_apply_1(f, lean_box(0));
            // Lean release builds are compiled without `LEAN_RUNTIME_STATS`.
            io_eprintln(&format!(
                "{}\nAllocation profiling data is not available, compile lean using `-D RUNTIME_STATS=ON`\n",
                lean_string_str(msg)
            ));
            r
        }

        /* getNumHeartbeats : BaseIO Nat */
        fn lean_io_get_num_heartbeats() -> obj {
            crate::nat::lean_uint64_to_nat(HEARTBEATS.with(|h| h.get()))
        }

        /* setHeartbeats (count : Nat) : BaseIO Unit */
        fn lean_io_set_heartbeats(count: obj) -> obj {
            let n = crate::uint::externs::lean_uint64_of_nat(count);
            lean_dec(count);
            HEARTBEATS.with(|h| h.set(n));
            lean_box(0)
        }

        /* getEnv (var : @& String) : BaseIO (Option String) */
        fn lean_io_getenv(var: b_obj) -> obj {
            let name = lean_string_str(var);
            if name.as_bytes().contains(&0) || name.is_empty() || name.contains('=') {
                // The C library's `getenv` finds no variable with such a name.
                return lean_mk_option_none();
            }
            match std::env::var_os(name) {
                Some(v) => lean_mk_option_some(lean_mk_string_from_bytes(&os_str_bytes(&v))),
                None => lean_mk_option_none(),
            }
        }

        /* realPath (fname : FilePath) : IO FilePath */
        fn lean_io_realpath(filename: obj) -> obj {
            let Some(path) = path_arg(filename) else {
                let r = embedded_nul_error(filename);
                lean_dec(filename);
                return r;
            };
            #[cfg(unix)]
            {
                match std::fs::canonicalize(path) {
                    Ok(p) => {
                        let s = lean_mk_string_from_bytes(&os_str_bytes(p.as_os_str()));
                        lean_dec(filename);
                        lean_io_result_mk_ok(s)
                    }
                    Err(_) => {

                        lean_io_result_mk_error(mk::no_file_or_directory(
                            filename,
                            errno::ENOENT as u32,
                            lean_mk_string(""),
                        ))
                    }
                }
            }
            #[cfg(windows)]
            {
                match std::fs::canonicalize(path) {
                    Ok(p) => {
                        let mut res = p.to_string_lossy().into_owned();
                        if let Some(rest) = res.strip_prefix(r"\\?\") {
                            res = if let Some(unc) = rest.strip_prefix(r"UNC\") {
                                format!(r"\\{unc}")
                            } else {
                                rest.to_owned()
                            };
                        }
                        lean_dec(filename);
                        lean_io_result_mk_ok(lean_mk_string(&win::lowercase_drive(res)))
                    }
                    Err(_) => lean_io_result_mk_error(mk::no_file_or_directory(
                        filename,
                        errno::ENOENT as u32,
                        lean_mk_string(""),
                    )),
                }
            }
        }

        /* readDir : @& FilePath → IO (Array DirEntry) */
        fn lean_io_read_dir(dirname: b_obj) -> obj {
            let Some(path) = path_arg(dirname) else { return embedded_nul_error(dirname) };
            let entries = match std::fs::read_dir(path) {
                Ok(it) => it,
                Err(e) => return io_error_result(&e, Some(dirname)),
            };
            let mut items: Vec<Obj> = Vec::new();
            for entry in entries {
                // `readdir` ends the listing on an error.
                let Ok(entry) = entry else { break };
                let name = entry.file_name();
                let bytes = os_str_bytes(&name);
                if bytes == b"." || bytes == b".." {
                    continue;
                }
                let o = lean_alloc_ctor(0, 2, 0);
                lean_inc(dirname);
                lean_ctor_set(o, 0, dirname);
                lean_ctor_set(o, 1, lean_mk_string_from_bytes(&bytes));
                items.push(o);
            }
            let arr = lean_alloc_array(items.len(), items.len());
            for (i, o) in items.into_iter().enumerate() {
                lean_array_set_core(arr, i, o);
            }
            lean_io_result_mk_ok(arr)
        }

        /* metadata : @& FilePath → IO IO.FS.Metadata */
        fn lean_io_metadata(filename: b_obj) -> obj {
            metadata_impl(filename, true)
        }

        /* symlinkMetadata : @& FilePath → IO IO.FS.Metadata */
        fn lean_io_symlink_metadata(filename: b_obj) -> obj {
            metadata_impl(filename, false)
        }

        /* createDir : @& FilePath → IO Unit */
        fn lean_io_create_dir(p: b_obj) -> obj {
            let Some(path) = path_arg(p) else { return embedded_nul_error(p) };
            #[cfg_attr(windows, allow(unused_mut))]
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o777);
            }
            match b.create(path) {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, Some(p)),
            }
        }

        /* removeDir : @& FilePath → IO Unit */
        fn lean_io_remove_dir(p: b_obj) -> obj {
            let Some(path) = path_arg(p) else { return embedded_nul_error(p) };
            match std::fs::remove_dir(path) {
                Ok(()) => io_ok_unit(),
                Err(e) => io_error_result(&e, Some(p)),
            }
        }

        /* rename (old new : @& FilePath) : IO Unit */
        fn lean_io_rename(from: b_obj, to: b_obj) -> obj {
            let Some(from_path) = path_arg(from) else { return embedded_nul_error(from) };
            let Some(to_path) = path_arg(to) else { return embedded_nul_error(to) };
            match std::fs::rename(from_path, to_path) {
                Ok(()) => io_ok_unit(),
                #[cfg(unix)]
                Err(e) => {
                    let both = lean_mk_string(&format!("{from_path} and/or {to_path}"));
                    let r = io_error_result(&e, Some(both));
                    lean_dec(both);
                    r
                }
                #[cfg(windows)]
                Err(e) => io_result_mk_user_error(&format!(
                    "failed to rename '{from_path}' to '{to_path}': {}",
                    last_error_code(&e)
                )),
            }
        }

        /* hardLink (orig link : @& FilePath) : IO Unit */
        fn lean_io_hard_link(orig: b_obj, link: b_obj) -> obj {
            let Some(orig_path) = path_arg(orig) else { return embedded_nul_error(orig) };
            let Some(link_path) = path_arg(link) else { return embedded_nul_error(link) };
            match std::fs::hard_link(orig_path, link_path) {
                Ok(()) => io_ok_unit(),
                Err(e) => uv_error_result(&e, Some(orig)),
            }
        }

        /* createTempFile : IO (Handle × FilePath) */
        fn lean_io_create_tempfile() -> obj {
            let template = match temp_template() {
                Ok(t) => t,
                Err(r) => return r,
            };
            match mkstemp(&template) {
                Err(code) => lean_io_result_mk_error(decode_uv_error(code, None)),
                Ok((path, file)) => {
                    let h = wrap_handle(Handle::new(Device::Owned(file), true, true, Buffering::Full, None));
                    let pair = lean_alloc_ctor(0, 2, 0);
                    lean_ctor_set(pair, 0, h);
                    lean_ctor_set(pair, 1, lean_mk_string(&path));
                    lean_io_result_mk_ok(pair)
                }
            }
        }

        /* createTempDir : IO FilePath */
        fn lean_io_create_tempdir() -> obj {
            let template = match temp_template() {
                Ok(t) => t,
                Err(r) => return r,
            };
            match mkdtemp(&template) {
                Err(code) => lean_io_result_mk_error(decode_uv_error(code, None)),
                Ok(path) => lean_io_result_mk_ok(lean_mk_string(&path)),
            }
        }

        /* removeFile : @& FilePath → IO Unit */
        fn lean_io_remove_file(filename: b_obj) -> obj {
            let Some(path) = path_arg(filename) else { return embedded_nul_error(filename) };
            let r = std::fs::remove_file(path);
            #[cfg(windows)]
            let r = match r {
                // libuv's Windows `unlink` clears the read-only attribute before deleting.
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    match std::fs::symlink_metadata(path) {
                        Ok(m) if m.permissions().readonly() && !m.is_dir() => {
                            let mut p = m.permissions();
                            // On Windows this clears `FILE_ATTRIBUTE_READONLY` only.
                            #[allow(clippy::permissions_set_readonly_false)]
                            p.set_readonly(false);
                            std::fs::set_permissions(path, p).and_then(|()| std::fs::remove_file(path))
                        }
                        _ => Err(e),
                    }
                }
                r => r,
            };
            match r {
                Ok(()) => io_ok_unit(),
                Err(e) => uv_error_result(&e, Some(filename)),
            }
        }

        /* appPath : IO FilePath */
        fn lean_io_app_path() -> obj {
            #[cfg(target_vendor = "apple")]
            {
                let exe = match std::env::current_exe() {
                    Ok(p) => p,
                    Err(_) => return io_result_mk_user_error("failed to locate application"),
                };
                match std::fs::canonicalize(exe) {
                    Ok(p) => lean_io_result_mk_ok(lean_mk_string_from_bytes(&os_str_bytes(p.as_os_str()))),
                    Err(_) => io_result_mk_user_error("failed to resolve symbolic links when locating application"),
                }
            }
            #[cfg(all(unix, not(target_vendor = "apple")))]
            {
                match std::fs::read_link(format!("/proc/{}/exe", std::process::id())) {
                    Ok(p) => lean_io_result_mk_ok(lean_mk_string_from_bytes(&os_str_bytes(p.as_os_str()))),
                    Err(_) => io_result_mk_user_error("failed to locate application"),
                }
            }
            #[cfg(windows)]
            {
                match std::env::current_exe() {
                    Ok(p) => lean_io_result_mk_ok(lean_mk_string(&win::lowercase_drive(p.to_string_lossy().into_owned()))),
                    Err(_) => io_result_mk_user_error("failed to locate application"),
                }
            }
        }

        /* currentDir : IO FilePath */
        fn lean_io_current_dir() -> obj {
            match std::env::current_dir() {
                Ok(p) => lean_io_result_mk_ok(lean_mk_string_from_bytes(&os_str_bytes(p.as_os_str()))),
                Err(_) => io_result_mk_user_error("failed to retrieve current working directory"),
            }
        }

        /* IO.Process.exit : UInt8 → IO α */
        fn lean_io_exit(code: u8) -> obj {
            flush_stdio();
            std::process::exit(code as i32)
        }

        /* IO.Process.forceExit : UInt8 → IO α */
        fn lean_io_force_exit(code: u8) -> obj {
            #[cfg(unix)]
            {
                libc::_exit(code as i32)
            }
            #[cfg(windows)]
            {
                windows_sys::Win32::System::Threading::ExitProcess(code as u32)
            }
        }

        /* getTID : BaseIO UInt64 */
        fn lean_io_get_tid() -> u64 {
            crate::process::current_thread_id()
        }
    }
}

// =============================================================================================
// ICU time-zone queries (Windows)
// =============================================================================================

#[cfg(windows)]
mod icu {
    use super::*;

    type UChar = u16;
    type UErrorCode = i32;
    type UCalendar = std::ffi::c_void;
    const U_ZERO_ERROR: UErrorCode = 0;
    const UCAL_GREGORIAN: i32 = 1;
    const UCAL_TZ_TRANSITION_NEXT: i32 = 0;
    const UCAL_ZONE_OFFSET: i32 = 15;
    const UCAL_DST_OFFSET: i32 = 16;
    const UCAL_STANDARD: i32 = 0;
    const UCAL_SHORT_STANDARD: i32 = 1;
    const UCAL_DST: i32 = 2;
    const UCAL_SHORT_DST: i32 = 3;

    #[link(name = "icu")]
    unsafe extern "C" {
        fn u_strFromUTF8(
            dest: *mut UChar,
            cap: i32,
            len: *mut i32,
            src: *const u8,
            src_len: i32,
            status: *mut UErrorCode,
        ) -> *mut UChar;
        fn u_strToUTF8(
            dest: *mut u8,
            cap: i32,
            len: *mut i32,
            src: *const UChar,
            src_len: i32,
            status: *mut UErrorCode,
        ) -> *mut u8;
        fn ucal_open(
            zone: *const UChar,
            len: i32,
            locale: *const u8,
            kind: i32,
            status: *mut UErrorCode,
        ) -> *mut UCalendar;
        fn ucal_close(cal: *mut UCalendar);
        fn ucal_setMillis(cal: *mut UCalendar, date: f64, status: *mut UErrorCode);
        fn ucal_getTimeZoneTransitionDate(
            cal: *const UCalendar,
            kind: i32,
            date: *mut f64,
            status: *mut UErrorCode,
        ) -> i8;
        fn ucal_get(cal: *const UCalendar, field: i32, status: *mut UErrorCode) -> i32;
        fn ucal_getTimeZoneDisplayName(
            cal: *const UCalendar,
            kind: i32,
            locale: *const u8,
            result: *mut UChar,
            len: i32,
            status: *mut UErrorCode,
        ) -> i32;
        fn ucal_getTimeZoneID(cal: *const UCalendar, result: *mut UChar, len: i32, status: *mut UErrorCode) -> i32;
    }

    fn failed(s: UErrorCode) -> bool {
        s > U_ZERO_ERROR
    }

    unsafe fn invalid(msg: &str) -> Obj {
        unsafe { lean_io_result_mk_error(mk::invalid_argument(einval() as u32, lean_mk_string(msg))) }
    }

    pub unsafe fn next_transition(zone: &[u8], tm: i64, default_time: bool) -> Obj {
        unsafe {
            let mut status = U_ZERO_ERROR;
            let mut tz_id = [0u16; 256];
            u_strFromUTF8(tz_id.as_mut_ptr(), 256, std::ptr::null_mut(), zone.as_ptr(), zone.len() as i32, &mut status);
            if failed(status) {
                return invalid("failed to read identifier");
            }
            let cal = ucal_open(tz_id.as_ptr(), -1, std::ptr::null(), UCAL_GREGORIAN, &mut status);
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to open calendar");
            }
            let mut t: i64 = 0;
            if !default_time {
                ucal_setMillis(cal, (tm * 1000) as f64, &mut status);
                if failed(status) {
                    ucal_close(cal);
                    return invalid("failed to set calendar time");
                }
                let mut next = 0f64;
                if ucal_getTimeZoneTransitionDate(cal, UCAL_TZ_TRANSITION_NEXT, &mut next, &mut status) == 0 {
                    ucal_close(cal);
                    return lean_io_result_mk_ok(lean_mk_option_none());
                }
                if failed(status) {
                    ucal_close(cal);
                    return invalid("failed to get next transition");
                }
                // Round up to whole seconds, as Lean's runtime does.
                t = (next / 1000.0).ceil() as i64;
            }
            let dst_offset = ucal_get(cal, UCAL_DST_OFFSET, &mut status);
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to get dst_offset");
            }
            let is_dst = dst_offset != 0;
            let id_len = ucal_getTimeZoneDisplayName(
                cal,
                if is_dst { UCAL_DST } else { UCAL_STANDARD },
                c"en_US".as_ptr() as *const u8,
                tz_id.as_mut_ptr(),
                32,
                &mut status,
            );
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to timezone identifier");
            }
            let mut dst_name = [0u8; 256];
            let mut dst_name_len = 0i32;
            u_strToUTF8(dst_name.as_mut_ptr(), 256, &mut dst_name_len, tz_id.as_ptr(), id_len, &mut status);
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to convert DST name to UTF-8");
            }
            let mut display = [0u16; 32];
            let display_len = ucal_getTimeZoneDisplayName(
                cal,
                if is_dst { UCAL_SHORT_DST } else { UCAL_SHORT_STANDARD },
                c"en_US".as_ptr() as *const u8,
                display.as_mut_ptr(),
                32,
                &mut status,
            );
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to read abbreaviation");
            }
            let mut abbrev = [0u8; 256];
            let mut abbrev_len = 0i32;
            u_strToUTF8(abbrev.as_mut_ptr(), 256, &mut abbrev_len, display.as_ptr(), display_len, &mut status);
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to get abbreviation to cstr");
            }
            let zone_offset = ucal_get(cal, UCAL_ZONE_OFFSET, &mut status) + dst_offset;
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to get zone_offset");
            }
            ucal_close(cal);
            let p = size_of::<Obj>();
            let tz = lean_alloc_ctor(0, 3, 1);
            lean_ctor_set(tz, 0, crate::int::lean_int_to_int(zone_offset / 1000));
            lean_ctor_set(tz, 1, lean_mk_string_from_bytes(&dst_name[..dst_name_len as usize]));
            lean_ctor_set(tz, 2, lean_mk_string_from_bytes(&abbrev[..abbrev_len as usize]));
            lean_ctor_set_uint8(tz, 3 * p, is_dst as u8);
            let pair = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(pair, 0, lean_box_uint64(t as u64));
            lean_ctor_set(pair, 1, tz);
            lean_io_result_mk_ok(lean_mk_option_some(pair))
        }
    }

    pub unsafe fn local_timezone_id_at(tm: i64) -> Obj {
        unsafe {
            let mut status = U_ZERO_ERROR;
            let cal = ucal_open(std::ptr::null(), -1, std::ptr::null(), UCAL_GREGORIAN, &mut status);
            if failed(status) {
                return invalid("failed to open calendar");
            }
            ucal_setMillis(cal, (tm * 1000) as f64, &mut status);
            if failed(status) {
                ucal_close(cal);
                return invalid("failed to set calendar time");
            }
            let mut id = [0u16; 256];
            let len = ucal_getTimeZoneID(cal, id.as_mut_ptr(), 256, &mut status);
            ucal_close(cal);
            if failed(status) {
                return invalid("failed to get timezone ID");
            }
            let mut out = [0u8; 256];
            let mut out_len = 0i32;
            u_strToUTF8(out.as_mut_ptr(), 256, &mut out_len, id.as_ptr(), len, &mut status);
            if failed(status) {
                return invalid("failed to convert timezone ID to UTF-8");
            }
            lean_io_result_mk_ok(lean_mk_string_from_bytes(&out[..out_len as usize]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    /// A scratch directory removed when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            crate::exports::recording::install();
            let dir = std::env::temp_dir().join(format!("patina-io-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        fn path(&self, name: &str) -> String {
            self.0.join(name).to_str().unwrap().to_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    unsafe fn s(text: &str) -> Obj {
        lean_mk_string(text)
    }

    /// Unwraps an `IO` result, returning the value (owned).
    unsafe fn ok(r: Obj) -> Obj {
        unsafe {
            assert!(lean_io_result_is_ok(r), "expected success, got {}", describe_error(lean_io_result_get_error(r)));
            lean_io_result_take_value(r)
        }
    }

    /// The constructor the runtime chose for an `IO.Error`, with its filename, code and details.
    unsafe fn error_parts(e: Obj) -> (&'static str, Option<String>, u32, String) {
        let r = unsafe { crate::exports::recording::read(e) };
        (r.constructor, r.file, r.code, r.details)
    }

    unsafe fn describe_error(e: Obj) -> String {
        format!("{:?}", unsafe { error_parts(e) })
    }

    unsafe fn err(r: Obj) -> (&'static str, Option<String>, u32, String) {
        unsafe {
            assert!(lean_io_result_is_error(r), "expected an error");
            let parts = error_parts(lean_io_result_get_error(r));
            lean_dec(r);
            parts
        }
    }

    // Expected values below were observed from Lean 4.34.1 (`lean --run`) on the same inputs.

    #[cfg(unix)]
    #[test]
    fn open_errors_classify_like_lean() {
        unsafe {
            let d = Scratch::new("open-errors");
            let missing = s(&d.path("missing.txt"));
            assert_eq!(
                err(lean_io_prim_handle_mk(missing, 0)),
                (
                    "no_file_or_directory",
                    Some(d.path("missing.txt")),
                    libc::ENOENT as u32,
                    "no such file or directory".into()
                )
            );
            std::fs::write(d.path("a.txt"), "x").unwrap();
            let a = s(&d.path("a.txt"));
            assert_eq!(
                err(lean_io_prim_handle_mk(a, 2)),
                ("already_exists_file", Some(d.path("a.txt")), libc::EEXIST as u32, "file already exists".into())
            );
            let dir = s(&d.0.to_string_lossy());
            assert_eq!(
                err(lean_io_prim_handle_mk(dir, 1)),
                (
                    "inappropriate_type_file",
                    Some(d.0.to_string_lossy().into_owned()),
                    libc::EISDIR as u32,
                    "illegal operation on a directory".into()
                )
            );
            let nul = s("t/a\u{0}b");
            assert_eq!(
                err(lean_io_prim_handle_mk(nul, 0)),
                (
                    "invalid_argument_file",
                    Some("t/a\u{0}b".into()),
                    libc::EINVAL as u32,
                    "string contains NUL bytes".into()
                )
            );
            for o in [missing, a, dir, nul] {
                lean_dec(o);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn permission_denied_on_read_only_file() {
        unsafe {
            if libc::geteuid() == 0 {
                // Root bypasses permission checks.
                return;
            }
            let d = Scratch::new("ro");
            let p = d.path("ro.txt");
            std::fs::write(&p, "x").unwrap();
            let f = s(&p);
            ok(lean_chmod(f, 0o400));
            assert_eq!(
                err(lean_io_prim_handle_mk(f, 1)),
                ("permission_denied_file", Some(p.clone()), libc::EACCES as u32, "permission denied".into())
            );
            lean_dec(f);
        }
    }

    #[test]
    fn directory_errors_classify_like_lean() {
        unsafe {
            let d = Scratch::new("dir-errors");
            let root = s(&d.0.to_string_lossy());
            std::fs::write(d.path("f"), "").unwrap();
            let (t, f, _, details) = err(lean_io_create_dir(root));
            assert_eq!(
                (t, f, details.as_str()),
                ("already_exists_file", Some(d.0.to_string_lossy().into_owned()), "file already exists")
            );
            // A non-empty directory drops the file name: `unsatisfiedConstraints`.
            let (t, f, _, details) = err(lean_io_remove_dir(root));
            assert_eq!((t, f, details.as_str()), ("unsatisfied_constraints", None, "directory not empty"));
            let nope = s(&d.path("nope"));
            assert_eq!(err(lean_io_remove_file(nope)).0, "no_file_or_directory");
            assert_eq!(err(lean_io_metadata(nope)).0, "no_file_or_directory");
            assert_eq!(err(lean_io_read_dir(nope)).0, "no_file_or_directory");
            let nope2 = s(&d.path("nope2"));
            #[cfg(unix)]
            assert_eq!(
                err(lean_io_rename(nope, nope2)).1,
                Some(format!("{} and/or {}", d.path("nope"), d.path("nope2")))
            );
            lean_inc(nope);
            let (t, f, code, details) = err(lean_io_realpath(nope));
            assert_eq!((t, f, details.as_str()), ("no_file_or_directory", Some(d.path("nope")), ""));
            assert_eq!(code as i32, errno::ENOENT);
            for o in [root, nope, nope2] {
                lean_dec(o);
            }
        }
    }

    #[test]
    fn get_line_returns_final_line_without_newline_then_empty() {
        unsafe {
            let d = Scratch::new("getline");
            std::fs::write(d.path("a.txt"), "hello\nworld").unwrap();
            let f = s(&d.path("a.txt"));
            let h = ok(lean_io_prim_handle_mk(f, 0));
            let mut lines = Vec::new();
            for _ in 0..3 {
                let l = ok(lean_io_prim_handle_get_line(h));
                lines.push(lean_string_str(l).to_owned());
                lean_dec(l);
            }
            assert_eq!(lines, ["hello\n", "world", ""]);
            lean_dec(h);
            lean_dec(f);
        }
    }

    unsafe fn read_file(p: &str) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn open_modes_behave_like_lean() {
        unsafe {
            let d = Scratch::new("modes");
            let p = d.path("a.txt");
            std::fs::write(&p, "hello\nworld").unwrap();
            let f = s(&p);
            let put = |mode: u8, text: &str| {
                let h = ok(lean_io_prim_handle_mk(f, mode));
                let t = s(text);
                ok(lean_io_prim_handle_put_str(h, t));
                ok(lean_io_prim_handle_flush(h));
                lean_dec(t);
                lean_dec(h);
            };
            put(4, "!");
            assert_eq!(read_file(&p), "hello\nworld!");
            put(3, "HE");
            assert_eq!(read_file(&p), "HEllo\nworld!");
            put(1, "new");
            assert_eq!(read_file(&p), "new");
            // Reads stop at end of input; a read at end of input returns no bytes.
            let h = ok(lean_io_prim_handle_mk(f, 0));
            let sizes: Vec<usize> = [2, 100, 5]
                .iter()
                .map(|&n| {
                    let b = ok(lean_io_prim_handle_read(h, n));
                    let k = lean_sarray_size(b);
                    lean_dec(b);
                    k
                })
                .collect();
            assert_eq!(sizes, [2, 1, 0]);
            // Writing to a read-only handle fails immediately with EBADF (`invalidArgument`).
            let x = s("x");
            let (t, fname, code, details) = err(lean_io_prim_handle_put_str(h, x));
            assert_eq!((t, fname, details.as_str()), ("invalid_argument", None, "bad file descriptor"));
            #[cfg(unix)]
            assert_eq!(code as i32, libc::EBADF);
            let _ = code;
            lean_dec(h);
            // Truncation happens at the logical position, after buffered reads.
            let h = ok(lean_io_prim_handle_mk(f, 3));
            lean_dec(ok(lean_io_prim_handle_read(h, 1)));
            ok(lean_io_prim_handle_truncate(h));
            lean_dec(h);
            assert_eq!(read_file(&p), "n");
            lean_dec(x);
            lean_dec(f);
        }
    }

    #[test]
    fn metadata_and_directory_listing() {
        unsafe {
            let d = Scratch::new("meta");
            std::fs::write(d.path("a.txt"), "n").unwrap();
            std::fs::write(d.path("sub.txt"), "").unwrap();
            let f = s(&d.path("a.txt"));
            let m = ok(lean_io_metadata(f));
            let p = size_of::<Obj>();
            assert_eq!(lean_ctor_get_uint64(m, 2 * p), 1);
            assert_eq!(lean_ctor_get_uint64(m, 2 * p + 8), 1);
            assert_eq!(lean_ctor_get_uint8(m, 2 * p + 16), 1);
            lean_dec(m);
            let root = s(&d.0.to_string_lossy());
            let m = ok(lean_io_metadata(root));
            assert_eq!(lean_ctor_get_uint8(m, 2 * p + 16), 0);
            lean_dec(m);
            let es = ok(lean_io_read_dir(root));
            let mut names: Vec<String> = (0..lean_array_size(es))
                .map(|i| {
                    let e = lean_array_get_core(es, i);
                    assert_eq!(lean_string_str(lean_ctor_get(e, 0)), d.0.to_string_lossy());
                    lean_string_str(lean_ctor_get(e, 1)).to_owned()
                })
                .collect();
            names.sort();
            assert_eq!(names, ["a.txt", "sub.txt"]);
            lean_dec(es);
            lean_dec(root);
            lean_dec(f);
        }
    }

    #[test]
    fn getenv_of_unset_and_invalid_names_is_none() {
        unsafe {
            for name in ["PATINA_SURELY_UNSET_VARIABLE", "A\u{0}B"] {
                let n = s(name);
                assert!(lean_io_getenv(n).is_scalar());
                lean_dec(n);
            }
            let n = s("PATH");
            let v = lean_io_getenv(n);
            assert_eq!(lean_obj_tag(v), 1);
            lean_dec(v);
            lean_dec(n);
        }
    }

    #[test]
    fn temp_files_and_directories() {
        unsafe {
            let r = ok(lean_io_create_tempfile());
            let h = lean_ctor_get(r, 0);
            let path = lean_string_str(lean_ctor_get(r, 1)).to_owned();
            assert!(std::path::Path::new(&path).file_name().unwrap().to_str().unwrap().starts_with("tmp."));
            let t = s("data");
            ok(lean_io_prim_handle_put_str(h, t));
            ok(lean_io_prim_handle_flush(h));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "data");
            lean_dec(t);
            lean_dec(r);
            std::fs::remove_file(&path).unwrap();
            let dir = ok(lean_io_create_tempdir());
            let dir_path = lean_string_str(dir).to_owned();
            assert!(std::path::Path::new(&dir_path).is_dir());
            std::fs::remove_dir(&dir_path).unwrap();
            lean_dec(dir);
        }
    }

    #[test]
    fn random_bytes_have_requested_length() {
        unsafe {
            for n in [0usize, 1, 33, 4096] {
                let b = ok(lean_io_get_random_bytes(n));
                assert_eq!(lean_sarray_size(b), n);
                lean_dec(b);
            }
        }
    }

    #[test]
    fn heartbeats_are_per_thread_counters() {
        unsafe {
            lean_io_set_heartbeats(lean_box(41));
            inc_heartbeat();
            let n = lean_io_get_num_heartbeats();
            assert_eq!(lean_unbox(n), 42);
        }
    }

    #[test]
    fn g_format_matches_iostreams() {
        assert_eq!(format_g(12.3456, 3), "12.3");
        assert_eq!(format_g(0.001234, 3), "0.00123");
        assert_eq!(format_g(1234.5, 3), "1.23e+03");
        assert_eq!(format_g(1.0, 3), "1");
        assert_eq!(format_g(0.5, 3), "0.5");
    }

    #[test]
    fn uv_messages_and_classification() {
        crate::exports::recording::install();
        unsafe {
            let e = decode_uv_error(uve::UV_ETIMEDOUT, None);
            assert_eq!(error_parts(e).0, "time_expired");
            lean_dec(e);
            let e = decode_io_error(errno::ENOTEMPTY, None);
            assert_eq!(error_parts(e).0, "unsatisfied_constraints");
            lean_dec(e);
        }
    }
}
