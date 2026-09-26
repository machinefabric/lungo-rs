//! `Std.Internal.UV.DNS`, ported from `runtime/uv/dns.cpp`. Lookups run on a worker thread, as
//! libuv's thread pool runs them, and complete on the event loop.

use super::errno::*;
use super::reactor::reactor;
use super::*;
use std::net::{IpAddr, SocketAddr};

fn is_safe_ascii(s: &[u8]) -> bool {
    s.iter().all(|&c| {
        c.is_ascii_alphanumeric()
            || matches!(c, b'-' | b'_' | b'.' | b':' | b'/' | b'+' | b'~' | b'@' | b'=' | b',' | b'%')
    })
}

#[cfg(unix)]
fn getaddrinfo(host: &[u8], service: &[u8], family: u8) -> Result<Vec<IpAddr>, i32> {
    use std::ffi::CString;
    let host = CString::new(host).map_err(|_| UV_EINVAL)?;
    let service = CString::new(service).map_err(|_| UV_EINVAL)?;
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = match family {
            1 => libc::PF_INET,
            2 => libc::PF_INET6,
            _ => libc::PF_UNSPEC,
        };
        let mut res: *mut libc::addrinfo = std::ptr::null_mut();
        let r = libc::getaddrinfo(host.as_ptr(), service.as_ptr(), &hints, &mut res);
        if r != 0 {
            return Err(uv_code_of_gai_error(r));
        }
        let mut out = Vec::new();
        let mut ai = res;
        while !ai.is_null() {
            if let Some(a) = super::sys::sockaddr_from_raw((*ai).ai_addr as *const _) {
                out.push(a.ip());
            }
            ai = (*ai).ai_next;
        }
        libc::freeaddrinfo(res);
        Ok(out)
    }
}

#[cfg(windows)]
fn getaddrinfo(host: &[u8], service: &[u8], family: u8) -> Result<Vec<IpAddr>, i32> {
    use windows_sys::Win32::Networking::WinSock::*;
    super::sys::init();
    let wide = |b: &[u8]| -> Vec<u16> { String::from_utf8_lossy(b).encode_utf16().chain(std::iter::once(0)).collect() };
    let host = wide(host);
    let service = wide(service);
    unsafe {
        let mut hints: ADDRINFOW = std::mem::zeroed();
        hints.ai_family = match family {
            1 => AF_INET as i32,
            2 => AF_INET6 as i32,
            _ => AF_UNSPEC as i32,
        };
        let mut res: *mut ADDRINFOW = std::ptr::null_mut();
        let r = GetAddrInfoW(host.as_ptr(), service.as_ptr(), &hints, &mut res);
        if r != 0 {
            return Err(uv_code_of_gai_error(WSAGetLastError()));
        }
        let mut out = Vec::new();
        let mut ai = res;
        while !ai.is_null() {
            if let Some(a) = super::sys::sockaddr_from_raw((*ai).ai_addr as *const _) {
                out.push(a.ip());
            }
            ai = (*ai).ai_next;
        }
        FreeAddrInfoW(res);
        Ok(out)
    }
}

#[cfg(unix)]
fn getnameinfo(addr: &SocketAddr) -> Result<(String, String), i32> {
    let (storage, len) = super::sys::sockaddr_storage_of(addr);
    let mut host = [0 as libc::c_char; 1025];
    let mut service = [0 as libc::c_char; 32];
    unsafe {
        let r = libc::getnameinfo(
            &storage as *const _ as *const libc::sockaddr,
            len,
            host.as_mut_ptr(),
            host.len() as libc::socklen_t,
            service.as_mut_ptr(),
            service.len() as libc::socklen_t,
            0,
        );
        if r != 0 {
            return Err(uv_code_of_gai_error(r));
        }
        Ok((
            std::ffi::CStr::from_ptr(host.as_ptr()).to_string_lossy().into_owned(),
            std::ffi::CStr::from_ptr(service.as_ptr()).to_string_lossy().into_owned(),
        ))
    }
}

#[cfg(windows)]
fn getnameinfo(addr: &SocketAddr) -> Result<(String, String), i32> {
    use windows_sys::Win32::Networking::WinSock::*;
    super::sys::init();
    let (storage, len) = super::sys::sockaddr_storage_of(addr);
    let mut host = [0u16; 1025];
    let mut service = [0u16; 32];
    unsafe {
        let r = GetNameInfoW(
            &storage as *const _ as *const SOCKADDR,
            len,
            host.as_mut_ptr(),
            host.len() as u32,
            service.as_mut_ptr(),
            service.len() as u32,
            0,
        );
        if r != 0 {
            return Err(uv_code_of_gai_error(WSAGetLastError()));
        }
        let s = |b: &[u16]| {
            let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            String::from_utf16_lossy(&b[..n])
        };
        Ok((s(&host), s(&service)))
    }
}

/// Runs `lookup` on a worker thread and resolves `promise` from the event loop with the value
/// `complete` builds from its result.
fn run_lookup<T: Send + 'static>(
    promise: Obj,
    lookup: impl FnOnce() -> Result<T, i32> + Send + 'static,
    complete: unsafe fn(T) -> Obj,
) {
    let p = SendObj(promise);
    std::thread::Builder::new()
        .name("lean-dns".into())
        .spawn(move || {
            let result = lookup();
            reactor().defer(Box::new(move || {
                let p = p;
                unsafe {
                    match result {
                        Ok(v) => crate::task::promise_resolve(except_ok(complete(v)), p.0),
                        Err(code) => resolve_with_code(code, p.0),
                    }
                    lean_dec(p.0);
                }
            }));
        })
        .unwrap_or_else(|e| lean_internal_panic(&format!("cannot start a DNS lookup thread: {e}")));
}

unsafe fn addresses(addrs: Vec<IpAddr>) -> Obj {
    unsafe {
        let arr = lean_alloc_array(addrs.len(), addrs.len());
        for (i, a) in addrs.iter().enumerate() {
            lean_array_set_core(arr, i, addr::lean_of_ip(a));
        }
        arr
    }
}

unsafe fn name_pair(names: (String, String)) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 2, 0);
        lean_ctor_set(r, 0, lean_mk_string(&names.0));
        lean_ctor_set(r, 1, lean_mk_string(&names.1));
        r
    }
}

pub(crate) unsafe fn get_info(name: Obj, service: Obj, family: u8) -> Obj {
    unsafe {
        let host = lean_string_bytes(name).to_vec();
        let serv = lean_string_bytes(service).to_vec();
        if !is_safe_ascii(&host) {
            return lean_io_result_mk_error(crate::io::mk::invalid_argument(
                invalid_argument_errno(),
                lean_mk_string("name is not ASCII"),
            ));
        }
        if !is_safe_ascii(&serv) {
            return lean_io_result_mk_error(crate::io::mk::invalid_argument(
                invalid_argument_errno(),
                lean_mk_string("service is not ASCII"),
            ));
        }
        reactor();
        let promise = new_promise();
        // The lookup owns a reference until it completes.
        lean_inc(promise);
        run_lookup(promise, move || getaddrinfo(&host, &serv, family), addresses);
        lean_io_result_mk_ok(promise)
    }
}

pub(crate) unsafe fn get_name(addr: Obj) -> Obj {
    unsafe {
        let a = addr::socket_addr_of_lean(addr);
        reactor();
        let promise = new_promise();
        lean_inc(promise);
        run_lookup(promise, move || getnameinfo(&a), name_pair);
        lean_io_result_mk_ok(promise)
    }
}

/// The C runtime's `EINVAL`, stored in `IO.Error.invalidArgument` by the C implementation.
fn invalid_argument_errno() -> u32 {
    #[cfg(unix)]
    {
        libc::EINVAL as u32
    }
    #[cfg(windows)]
    {
        22
    }
}
