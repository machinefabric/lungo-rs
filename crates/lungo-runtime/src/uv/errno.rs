//! libuv error codes and messages.
//!
//! Lean's `Std.Internal.UV` primitives report errors as libuv error codes, which Lean's runtime
//! decodes into `IO.Error` values (`lean_decode_uv_error`). On Unix platforms libuv error codes
//! are negated `errno` values; codes libuv defines itself (and all codes on Windows) have the
//! fixed values of libuv's `uv/errno.h`.

#![allow(dead_code)]

macro_rules! uv_codes {
    ($( $name:ident = $errno:ident / $fixed:literal, $msg:literal; )*) => {
        $(
            #[cfg(any(unix, target_os = "wasi"))]
            pub const $name: i32 = -(libc::$errno as i32);
            #[cfg(windows)]
            pub const $name: i32 = $fixed;
        )*

        /// libuv's message for `code` (`uv_strerror`).
        pub fn uv_strerror(code: i32) -> &'static str {
            $( if code == $name { return $msg; } )*
            fixed_message(code).unwrap_or_else(|| unknown_message(code))
        }
    };
}

// Codes that are `-errno` on every Unix platform the runtime supports, and on WASI.
uv_codes! {
    UV_E2BIG = E2BIG / -4093, "argument list too long";
    UV_EACCES = EACCES / -4092, "permission denied";
    UV_EADDRINUSE = EADDRINUSE / -4091, "address already in use";
    UV_EADDRNOTAVAIL = EADDRNOTAVAIL / -4090, "address not available";
    UV_EAFNOSUPPORT = EAFNOSUPPORT / -4089, "address family not supported";
    UV_EAGAIN = EAGAIN / -4088, "resource temporarily unavailable";
    UV_EALREADY = EALREADY / -4084, "connection already in progress";
    UV_EBADF = EBADF / -4083, "bad file descriptor";
    UV_EBUSY = EBUSY / -4082, "resource busy or locked";
    UV_ECANCELED = ECANCELED / -4081, "operation canceled";
    UV_ECONNABORTED = ECONNABORTED / -4079, "software caused connection abort";
    UV_ECONNREFUSED = ECONNREFUSED / -4078, "connection refused";
    UV_ECONNRESET = ECONNRESET / -4077, "connection reset by peer";
    UV_EDESTADDRREQ = EDESTADDRREQ / -4076, "destination address required";
    UV_EEXIST = EEXIST / -4075, "file already exists";
    UV_EFAULT = EFAULT / -4074, "bad address in system call argument";
    UV_EHOSTUNREACH = EHOSTUNREACH / -4073, "host is unreachable";
    UV_EINTR = EINTR / -4072, "interrupted system call";
    UV_EINVAL = EINVAL / -4071, "invalid argument";
    UV_EIO = EIO / -4070, "i/o error";
    UV_EISCONN = EISCONN / -4069, "socket is already connected";
    UV_EISDIR = EISDIR / -4068, "illegal operation on a directory";
    UV_ELOOP = ELOOP / -4067, "too many symbolic links encountered";
    UV_EMFILE = EMFILE / -4066, "too many open files";
    UV_EMSGSIZE = EMSGSIZE / -4065, "message too long";
    UV_ENAMETOOLONG = ENAMETOOLONG / -4064, "name too long";
    UV_ENETDOWN = ENETDOWN / -4063, "network is down";
    UV_ENETUNREACH = ENETUNREACH / -4062, "network is unreachable";
    UV_ENFILE = ENFILE / -4061, "file table overflow";
    UV_ENOBUFS = ENOBUFS / -4060, "no buffer space available";
    UV_ENODEV = ENODEV / -4059, "no such device";
    UV_ENOENT = ENOENT / -4058, "no such file or directory";
    UV_ENOMEM = ENOMEM / -4057, "not enough memory";
    UV_ENOSPC = ENOSPC / -4055, "no space left on device";
    UV_ENOSYS = ENOSYS / -4054, "function not implemented";
    UV_ENOTCONN = ENOTCONN / -4053, "socket is not connected";
    UV_ENOTDIR = ENOTDIR / -4052, "not a directory";
    UV_ENOTEMPTY = ENOTEMPTY / -4051, "directory not empty";
    UV_ENOTSOCK = ENOTSOCK / -4050, "socket operation on non-socket";
    UV_ENOTSUP = ENOTSUP / -4049, "operation not supported on socket";
    UV_EOVERFLOW = EOVERFLOW / -4026, "value too large for defined data type";
    UV_EPERM = EPERM / -4048, "operation not permitted";
    UV_EPIPE = EPIPE / -4047, "broken pipe";
    UV_EPROTO = EPROTO / -4046, "protocol error";
    UV_EPROTONOSUPPORT = EPROTONOSUPPORT / -4045, "protocol not supported";
    UV_EPROTOTYPE = EPROTOTYPE / -4044, "protocol wrong type for socket";
    UV_EROFS = EROFS / -4043, "read-only file system";
    UV_ESPIPE = ESPIPE / -4041, "invalid seek";
    UV_ESRCH = ESRCH / -4040, "no such process";
    UV_ETIMEDOUT = ETIMEDOUT / -4039, "connection timed out";
    UV_ETXTBSY = ETXTBSY / -4038, "text file is busy";
    UV_EXDEV = EXDEV / -4037, "cross-device link not permitted";
    UV_EFBIG = EFBIG / -4036, "file too large";
    UV_ENOPROTOOPT = ENOPROTOOPT / -4035, "protocol not available";
    UV_ERANGE = ERANGE / -4034, "result too large";
    UV_ENXIO = ENXIO / -4033, "no such device or address";
    UV_EMLINK = EMLINK / -4032, "too many links";
    UV_ENOTTY = ENOTTY / -4029, "inappropriate ioctl for device";
    UV_EILSEQ = EILSEQ / -4027, "illegal byte sequence";
    UV_ENOEXEC = ENOEXEC / -4022, "exec format error";
}

// Codes whose `errno` exists only on some platforms.
#[cfg(unix)]
pub const UV_EHOSTDOWN: i32 = -libc::EHOSTDOWN;
#[cfg(not(unix))]
pub const UV_EHOSTDOWN: i32 = -4031;
#[cfg(unix)]
pub const UV_ESHUTDOWN: i32 = -libc::ESHUTDOWN;
#[cfg(not(unix))]
pub const UV_ESHUTDOWN: i32 = -4042;
#[cfg(unix)]
pub const UV_ESOCKTNOSUPPORT: i32 = -libc::ESOCKTNOSUPPORT;
#[cfg(not(unix))]
pub const UV_ESOCKTNOSUPPORT: i32 = -4025;
#[cfg(unix)]
pub const UV_ENODATA: i32 = -libc::ENODATA;
#[cfg(not(unix))]
pub const UV_ENODATA: i32 = -4024;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const UV_ENONET: i32 = -libc::ENONET;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub const UV_ENONET: i32 = -4056;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const UV_EREMOTEIO: i32 = -libc::EREMOTEIO;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub const UV_EREMOTEIO: i32 = -4030;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const UV_EUNATCH: i32 = -libc::EUNATCH;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub const UV_EUNATCH: i32 = -4023;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub const UV_EFTYPE: i32 = -libc::EFTYPE;
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
pub const UV_EFTYPE: i32 = -4028;

// Codes libuv defines itself on every platform.
pub const UV_ECHARSET: i32 = -4080;
pub const UV_EOF: i32 = -4095;
pub const UV_UNKNOWN: i32 = -4094;
pub const UV_EAI_ADDRFAMILY: i32 = -3000;
pub const UV_EAI_AGAIN: i32 = -3001;
pub const UV_EAI_BADFLAGS: i32 = -3002;
pub const UV_EAI_CANCELED: i32 = -3003;
pub const UV_EAI_FAIL: i32 = -3004;
pub const UV_EAI_FAMILY: i32 = -3005;
pub const UV_EAI_MEMORY: i32 = -3006;
pub const UV_EAI_NODATA: i32 = -3007;
pub const UV_EAI_NONAME: i32 = -3008;
pub const UV_EAI_OVERFLOW: i32 = -3009;
pub const UV_EAI_SERVICE: i32 = -3010;
pub const UV_EAI_SOCKTYPE: i32 = -3011;
pub const UV_EAI_BADHINTS: i32 = -3013;
pub const UV_EAI_PROTOCOL: i32 = -3014;

/// `Unknown system error <code>`, interned so that every distinct code is formatted once.
fn unknown_message(code: i32) -> &'static str {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static MESSAGES: Mutex<Option<HashMap<i32, &'static str>>> = Mutex::new(None);
    let mut g = MESSAGES.lock().unwrap_or_else(|p| p.into_inner());
    g.get_or_insert_with(HashMap::new)
        .entry(code)
        .or_insert_with(|| Box::leak(format!("Unknown system error {code}").into_boxed_str()))
}

fn fixed_message(code: i32) -> Option<&'static str> {
    Some(match code {
        UV_EHOSTDOWN => "host is down",
        UV_ESHUTDOWN => "cannot send after transport endpoint shutdown",
        UV_ESOCKTNOSUPPORT => "socket type not supported",
        UV_ENODATA => "no data available",
        UV_ENONET => "machine is not on the network",
        UV_EREMOTEIO => "remote I/O error",
        UV_EUNATCH => "protocol driver not attached",
        UV_EFTYPE => "inappropriate file type or format",
        UV_ECHARSET => "invalid Unicode character",
        UV_EOF => "end of file",
        UV_UNKNOWN => "unknown error",
        UV_EAI_ADDRFAMILY => "address family not supported",
        UV_EAI_AGAIN => "temporary failure",
        UV_EAI_BADFLAGS => "bad ai_flags value",
        UV_EAI_CANCELED => "request canceled",
        UV_EAI_FAIL => "permanent failure",
        UV_EAI_FAMILY => "ai_family not supported",
        UV_EAI_MEMORY => "out of memory",
        UV_EAI_NODATA => "no address",
        UV_EAI_NONAME => "unknown node or service",
        UV_EAI_OVERFLOW => "argument buffer overflow",
        UV_EAI_SERVICE => "service not available for socket type",
        UV_EAI_SOCKTYPE => "socket type not supported",
        UV_EAI_BADHINTS => "invalid value for hints",
        UV_EAI_PROTOCOL => "resolved protocol is unknown",
        _ => return None,
    })
}

/// The libuv error code for an operating-system error (`UV__ERR(errno)` on Unix and WASI,
/// `uv_translate_sys_error` on Windows).
#[cfg(any(unix, target_os = "wasi"))]
pub fn uv_code_of_os_error(raw: i32) -> i32 {
    if raw <= 0 { raw } else { -raw }
}

/// The libuv error code for an operating-system error (`UV__ERR(errno)` on Unix,
/// `uv_translate_sys_error` on Windows).
#[cfg(windows)]
pub fn uv_code_of_os_error(raw: i32) -> i32 {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Networking::WinSock::*;
    if raw <= 0 {
        return raw;
    }
    let code = raw as u32;
    let c = code as i32;
    match code {
        ERROR_NOACCESS => UV_EACCES,
        ERROR_ELEVATION_REQUIRED => UV_EACCES,
        ERROR_CANT_ACCESS_FILE => UV_EACCES,
        ERROR_ADDRESS_ALREADY_ASSOCIATED => UV_EADDRINUSE,
        ERROR_NO_DATA => UV_EAGAIN,
        ERROR_INVALID_FLAGS => UV_EBADF,
        ERROR_INVALID_HANDLE => UV_EBADF,
        ERROR_LOCK_VIOLATION => UV_EBUSY,
        ERROR_PIPE_BUSY => UV_EBUSY,
        ERROR_SHARING_VIOLATION => UV_EBUSY,
        ERROR_OPERATION_ABORTED => UV_ECANCELED,
        ERROR_NO_UNICODE_TRANSLATION => UV_ECHARSET,
        ERROR_CONNECTION_ABORTED => UV_ECONNABORTED,
        ERROR_CONNECTION_REFUSED => UV_ECONNREFUSED,
        ERROR_NETNAME_DELETED => UV_ECONNRESET,
        ERROR_ALREADY_EXISTS => UV_EEXIST,
        ERROR_FILE_EXISTS => UV_EEXIST,
        ERROR_BUFFER_OVERFLOW => UV_EFAULT,
        ERROR_HOST_UNREACHABLE => UV_EHOSTUNREACH,
        ERROR_INSUFFICIENT_BUFFER => UV_EINVAL,
        ERROR_INVALID_DATA => UV_EINVAL,
        ERROR_INVALID_PARAMETER => UV_EINVAL,
        ERROR_SYMLINK_NOT_SUPPORTED => UV_EINVAL,
        ERROR_BEGINNING_OF_MEDIA => UV_EIO,
        ERROR_BUS_RESET => UV_EIO,
        ERROR_CRC => UV_EIO,
        ERROR_DEVICE_DOOR_OPEN => UV_EIO,
        ERROR_DEVICE_REQUIRES_CLEANING => UV_EIO,
        ERROR_DISK_CORRUPT => UV_EIO,
        ERROR_EOM_OVERFLOW => UV_EIO,
        ERROR_FILEMARK_DETECTED => UV_EIO,
        ERROR_GEN_FAILURE => UV_EIO,
        ERROR_INVALID_BLOCK_LENGTH => UV_EIO,
        ERROR_IO_DEVICE => UV_EIO,
        ERROR_NO_DATA_DETECTED => UV_EIO,
        ERROR_NO_SIGNAL_SENT => UV_EIO,
        ERROR_OPEN_FAILED => UV_EIO,
        ERROR_SETMARK_DETECTED => UV_EIO,
        ERROR_SIGNAL_REFUSED => UV_EIO,
        ERROR_CANT_RESOLVE_FILENAME => UV_ELOOP,
        ERROR_TOO_MANY_OPEN_FILES => UV_EMFILE,
        ERROR_BAD_PATHNAME => UV_ENOENT,
        ERROR_DIRECTORY => UV_ENOENT,
        ERROR_ENVVAR_NOT_FOUND => UV_ENOENT,
        ERROR_FILE_NOT_FOUND => UV_ENOENT,
        ERROR_INVALID_NAME => UV_ENOENT,
        ERROR_INVALID_DRIVE => UV_ENOENT,
        ERROR_INVALID_REPARSE_DATA => UV_ENOENT,
        ERROR_MOD_NOT_FOUND => UV_ENOENT,
        ERROR_PATH_NOT_FOUND => UV_ENOENT,
        ERROR_FILENAME_EXCED_RANGE => UV_ENAMETOOLONG,
        ERROR_NETWORK_UNREACHABLE => UV_ENETUNREACH,
        ERROR_NOT_ENOUGH_MEMORY => UV_ENOMEM,
        ERROR_OUTOFMEMORY => UV_ENOMEM,
        ERROR_CANNOT_MAKE => UV_ENOSPC,
        ERROR_DISK_FULL => UV_ENOSPC,
        ERROR_EA_TABLE_FULL => UV_ENOSPC,
        ERROR_END_OF_MEDIA => UV_ENOSPC,
        ERROR_HANDLE_DISK_FULL => UV_ENOSPC,
        ERROR_NOT_CONNECTED => UV_ENOTCONN,
        ERROR_DIR_NOT_EMPTY => UV_ENOTEMPTY,
        ERROR_NOT_SUPPORTED => UV_ENOTSUP,
        ERROR_BROKEN_PIPE => UV_EOF,
        ERROR_ACCESS_DENIED => UV_EPERM,
        ERROR_PRIVILEGE_NOT_HELD => UV_EPERM,
        ERROR_BAD_PIPE => UV_EPIPE,
        ERROR_PIPE_NOT_CONNECTED => UV_EPIPE,
        ERROR_SEM_TIMEOUT => UV_ETIMEDOUT,
        ERROR_NOT_SAME_DEVICE => UV_EXDEV,
        ERROR_INVALID_FUNCTION => UV_EISDIR,
        ERROR_META_EXPANSION_TOO_LONG => UV_E2BIG,
        _ => match c {
            WSAEACCES => UV_EACCES,
            WSAEADDRINUSE => UV_EADDRINUSE,
            WSAEADDRNOTAVAIL => UV_EADDRNOTAVAIL,
            WSAEAFNOSUPPORT => UV_EAFNOSUPPORT,
            WSAEWOULDBLOCK => UV_EAGAIN,
            WSAEALREADY => UV_EALREADY,
            WSAEINVAL => UV_EINVAL,
            WSAEBADF => UV_EBADF,
            WSAECONNABORTED => UV_ECONNABORTED,
            WSAECONNREFUSED => UV_ECONNREFUSED,
            WSAECONNRESET => UV_ECONNRESET,
            WSAEFAULT => UV_EFAULT,
            WSAEHOSTUNREACH => UV_EHOSTUNREACH,
            WSAEINTR => UV_EINTR,
            WSAEISCONN => UV_EISCONN,
            WSAEMFILE => UV_EMFILE,
            WSAEMSGSIZE => UV_EMSGSIZE,
            WSAENETDOWN => UV_ENETDOWN,
            WSAENETUNREACH => UV_ENETUNREACH,
            WSAENOBUFS => UV_ENOBUFS,
            WSAENOTCONN => UV_ENOTCONN,
            WSAENOTSOCK => UV_ENOTSOCK,
            WSAEOPNOTSUPP => UV_ENOTSUP,
            WSAEPFNOSUPPORT => UV_EAFNOSUPPORT,
            WSAEPROTONOSUPPORT => UV_EPROTONOSUPPORT,
            WSAEPROTOTYPE => UV_EPROTOTYPE,
            WSAESHUTDOWN => UV_EPIPE,
            WSAESOCKTNOSUPPORT => UV_ESOCKTNOSUPPORT,
            WSAETIMEDOUT => UV_ETIMEDOUT,
            WSAENOPROTOOPT => UV_ENOPROTOOPT,
            WSAEHOSTDOWN => UV_EHOSTDOWN,
            WSAEDESTADDRREQ => UV_EDESTADDRREQ,
            WSAENAMETOOLONG => UV_ENAMETOOLONG,
            WSAEINPROGRESS => UV_EBUSY,
            _ => UV_UNKNOWN,
        },
    }
}

/// The libuv error code of `e`.
pub fn uv_code_of_io_error(e: &std::io::Error) -> i32 {
    match e.raw_os_error() {
        Some(raw) => uv_code_of_os_error(raw),
        None => match e.kind() {
            std::io::ErrorKind::NotFound => UV_ENOENT,
            std::io::ErrorKind::PermissionDenied => UV_EACCES,
            std::io::ErrorKind::InvalidInput => UV_EINVAL,
            std::io::ErrorKind::OutOfMemory => UV_ENOMEM,
            std::io::ErrorKind::Unsupported => UV_ENOTSUP,
            std::io::ErrorKind::WouldBlock => UV_EAGAIN,
            std::io::ErrorKind::Interrupted => UV_EINTR,
            std::io::ErrorKind::TimedOut => UV_ETIMEDOUT,
            std::io::ErrorKind::UnexpectedEof => UV_EOF,
            _ => UV_UNKNOWN,
        },
    }
}

/// The libuv error code for a `getaddrinfo`/`getnameinfo` result
/// (`uv__getaddrinfo_translate_error`).
#[cfg(unix)]
pub fn uv_code_of_gai_error(code: i32) -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const EAI_ADDRFAMILY: i32 = -9;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const EAI_CANCELED: i32 = -101;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const EAI_ADDRFAMILY: i32 = 1;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const EAI_BADHINTS: i32 = 12;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const EAI_PROTOCOL: i32 = 13;
    match code {
        0 => 0,
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios"))]
        EAI_ADDRFAMILY => UV_EAI_ADDRFAMILY,
        libc::EAI_AGAIN => UV_EAI_AGAIN,
        libc::EAI_BADFLAGS => UV_EAI_BADFLAGS,
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        EAI_BADHINTS => UV_EAI_BADHINTS,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        EAI_CANCELED => UV_EAI_CANCELED,
        libc::EAI_FAIL => UV_EAI_FAIL,
        libc::EAI_FAMILY => UV_EAI_FAMILY,
        libc::EAI_MEMORY => UV_EAI_MEMORY,
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios"))]
        libc::EAI_NODATA if libc::EAI_NODATA != libc::EAI_NONAME => UV_EAI_NODATA,
        libc::EAI_NONAME => UV_EAI_NONAME,
        libc::EAI_OVERFLOW => UV_EAI_OVERFLOW,
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        EAI_PROTOCOL => UV_EAI_PROTOCOL,
        libc::EAI_SERVICE => UV_EAI_SERVICE,
        libc::EAI_SOCKTYPE => UV_EAI_SOCKTYPE,
        libc::EAI_SYSTEM => uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)),
        _ => crate::object::lean_internal_panic(&format!("unknown EAI_* error code {code}")),
    }
}

/// The libuv error code for a `GetAddrInfoW`/`GetNameInfoW` result
/// (`uv__getaddrinfo_translate_error` on Windows).
#[cfg(windows)]
pub fn uv_code_of_gai_error(code: i32) -> i32 {
    use windows_sys::Win32::Networking::WinSock::*;
    match code {
        0 => 0,
        WSATRY_AGAIN => UV_EAI_AGAIN,
        WSAEINVAL => UV_EAI_BADFLAGS,
        WSANO_RECOVERY => UV_EAI_FAIL,
        WSAEAFNOSUPPORT => UV_EAI_FAMILY,
        WSA_NOT_ENOUGH_MEMORY => UV_EAI_MEMORY,
        WSAHOST_NOT_FOUND => UV_EAI_NONAME,
        WSATYPE_NOT_FOUND => UV_EAI_SERVICE,
        WSAESOCKTNOSUPPORT => UV_EAI_SOCKTYPE,
        _ => uv_code_of_os_error(code),
    }
}
