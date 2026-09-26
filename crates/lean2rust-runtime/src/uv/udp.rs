//! `Std.Internal.UV.UDP`, ported from `runtime/uv/udp.cpp` together with the libuv UDP
//! semantics it relies on (deferred binding to the wildcard address, connected sockets, and
//! queued sends completed from the event loop).

use super::errno::*;
use super::reactor::{IoSource, reactor};
use super::sys::{Family, Kind, Opt, RawSock, Sock};
use super::*;
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadMode {
    None,
    Bytes,
    Readable,
}

struct Send {
    promise: Obj,
    data: Obj,
    to: Option<SocketAddr>,
}

struct Inner {
    sock: Option<Sock>,
    connected: bool,
    read: ReadMode,
    promise_read: Obj,
    byte_array: Obj,
    sends: VecDeque<Send>,
}

pub(crate) struct Udp {
    obj: Obj,
    source: u64,
    inner: UnsafeCell<Inner>,
}

unsafe fn finalize(p: *mut ()) {
    unsafe {
        let r = reactor();
        let guard = r.lock();
        let udp = Box::from_raw(p as *mut Udp);
        r.unregister_source(&guard, udp.source);
        let inner = &*udp.inner.get();
        if !inner.promise_read.is_null() || !inner.byte_array.is_null() || !inner.sends.is_empty() {
            lean_internal_panic("a UDP socket with pending operations was finalized");
        }
        drop(udp);
        drop(guard);
    }
}

unsafe fn for_each(p: *mut (), f: &mut dyn FnMut(Obj)) {
    unsafe {
        let inner = &*(*(p as *mut Udp)).inner.get();
        for o in [inner.promise_read, inner.byte_array] {
            if !o.is_null() {
                f(o);
            }
        }
    }
}

static CLASS: ExternalClass = ExternalClass { finalize, for_each };

unsafe fn get<'a>(o: Obj) -> &'a Udp {
    unsafe { external_data::<Udp>(o, &CLASS, "UDP socket") }
}

impl Udp {
    #[allow(clippy::mut_from_ref)]
    unsafe fn inner(&self) -> &mut Inner {
        unsafe { &mut *self.inner.get() }
    }
}

/// `uv__udp_bind`.
fn udp_bind(inner: &mut Inner, addr: &SocketAddr, reuse: bool) -> Result<(), i32> {
    let family = Family::of(addr);
    if inner.sock.is_none() {
        inner.sock = Some(Sock::new(family, Kind::Datagram).map_err(|e| uv_code_of_io_error(&e))?);
    }
    let sock = inner.sock.as_ref().expect("socket created");
    if reuse {
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        ))]
        sock.set_opt(Opt::ReusePort, 1).map_err(|e| uv_code_of_io_error(&e))?;
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        )))]
        sock.set_opt(Opt::ReuseAddr, 1).map_err(|e| uv_code_of_io_error(&e))?;
    }
    if family == Family::V6 {
        sock.set_opt(Opt::Ipv6Only, 0).map_err(|e| uv_code_of_io_error(&e))?;
    }
    sock.bind(addr).map_err(|e| {
        let code = uv_code_of_io_error(&e);
        if code == UV_EAFNOSUPPORT { UV_EINVAL } else { code }
    })
}

/// `uv__udp_maybe_deferred_bind`: binds to the wildcard address of `family` if unbound.
fn maybe_deferred_bind(inner: &mut Inner, family: Family, reuse: bool) -> Result<(), i32> {
    if inner.sock.is_some() {
        return Ok(());
    }
    let any = match family {
        Family::V4 => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        Family::V6 => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    udp_bind(inner, &any, reuse)
}

impl IoSource for Udp {
    fn interest(&self) -> Option<(RawSock, bool, bool)> {
        let inner = unsafe { &*self.inner.get() };
        let sock = inner.sock.as_ref()?.raw();
        Some((sock, inner.read != ReadMode::None, !inner.sends.is_empty()))
    }

    unsafe fn on_ready(&self, readable: bool, writable: bool) {
        unsafe {
            let obj = self.obj;
            lean_inc(obj);
            if readable && self.inner().read != ReadMode::None {
                do_read(self);
            }
            if writable {
                flush_sends(self);
            }
            lean_dec(obj);
        }
    }
}

unsafe fn do_read(udp: &Udp) {
    unsafe {
        let inner = udp.inner();
        let value = match inner.read {
            ReadMode::None => return,
            ReadMode::Readable => except_ok(lean_box(0)),
            ReadMode::Bytes => {
                let ba = inner.byte_array;
                let cap = lean_sarray_capacity(ba);
                if cap == 0 {
                    inner.byte_array = Obj::null();
                    lean_dec(ba);
                    except_err(uv_error(UV_ENOBUFS, None))
                } else {
                    let buf = std::slice::from_raw_parts_mut(lean_sarray_cptr(ba), cap);
                    let result = match inner.sock.as_ref() {
                        Some(s) => s.recv_from(buf),
                        None => lean_internal_panic("pending UDP read without a socket"),
                    };
                    inner.byte_array = Obj::null();
                    match result {
                        Ok((n, from)) => {
                            lean_sarray_set_size(ba, n);
                            pair(ba, from)
                        }
                        // libuv reports a spurious wake-up as an empty datagram without sender.
                        Err(e) if is_would_block(&e) => {
                            lean_sarray_set_size(ba, 0);
                            pair(ba, None)
                        }
                        Err(e) => {
                            lean_dec(ba);
                            except_err(uv_error(uv_code_of_io_error(&e), None))
                        }
                    }
                }
            }
        };
        inner.read = ReadMode::None;
        let promise = std::mem::replace(&mut inner.promise_read, Obj::null());
        crate::task::promise_resolve(value, promise);
        lean_dec(promise);
        // The event loop does not own the socket anymore.
        lean_dec(udp.obj);
    }
}

unsafe fn pair(ba: Obj, from: Option<SocketAddr>) -> Obj {
    unsafe {
        let addr = match from {
            Some(a) => some(addr::lean_of_socket_addr(&a)),
            None => none(),
        };
        let p = lean_alloc_ctor(0, 2, 0);
        lean_ctor_set(p, 0, ba);
        lean_ctor_set(p, 1, addr);
        except_ok(p)
    }
}

unsafe fn flush_sends(udp: &Udp) {
    unsafe {
        loop {
            let inner = udp.inner();
            let Some(front) = inner.sends.front() else { return };
            let sock = match inner.sock.as_ref() {
                Some(s) => s,
                None => lean_internal_panic("pending UDP send without a socket"),
            };
            // A send is one datagram made of all buffers.
            let mut datagram = Vec::new();
            for i in 0..lean_array_size(front.data) {
                let ba = lean_array_get_core(front.data, i);
                datagram.extend_from_slice(std::slice::from_raw_parts(lean_sarray_cptr(ba), lean_sarray_size(ba)));
            }
            let status = match sock.send_to(&datagram, front.to.as_ref()) {
                Ok(_) => 0,
                Err(e) if is_would_block(&e) => return,
                Err(e) => uv_code_of_io_error(&e),
            };
            let s = inner.sends.pop_front().expect("front send");
            resolve_with_code(status, s.promise);
            lean_dec(s.promise);
            lean_dec(s.data);
            lean_dec(udp.obj);
        }
    }
}

pub(crate) unsafe fn new() -> Obj {
    unsafe {
        let guard = reactor().lock();
        let udp = Box::into_raw(Box::new(Udp {
            obj: Obj::null(),
            source: 0,
            inner: UnsafeCell::new(Inner {
                sock: None,
                connected: false,
                read: ReadMode::None,
                promise_read: Obj::null(),
                byte_array: Obj::null(),
                sends: VecDeque::new(),
            }),
        }));
        let obj = lean_alloc_external(&CLASS, udp as *mut ());
        lean_mark_mt(obj);
        (*udp).obj = obj;
        (*udp).source = reactor().register_source(&guard, udp as *const Udp as *const dyn IoSource);
        drop(guard);
        lean_io_result_mk_ok(obj)
    }
}

unsafe fn unit_result(r: Result<(), i32>) -> Obj {
    unsafe {
        match r {
            Ok(()) => lean_io_result_mk_ok(lean_box(0)),
            Err(code) => uv_io_error(code),
        }
    }
}

pub(crate) unsafe fn bind(socket: Obj, addr: Obj) -> Obj {
    unsafe {
        let udp = get(socket);
        let addr = addr::socket_addr_of_lean(addr);
        let guard = reactor().lock();
        let r = udp_bind(udp.inner(), &addr, true);
        drop(guard);
        unit_result(r)
    }
}

pub(crate) unsafe fn connect(socket: Obj, addr: Obj) -> Obj {
    unsafe {
        let udp = get(socket);
        let addr = addr::socket_addr_of_lean(addr);
        let guard = reactor().lock();
        let inner = udp.inner();
        let r = (|| {
            if inner.connected {
                return Err(UV_EISCONN);
            }
            maybe_deferred_bind(inner, Family::of(&addr), false)?;
            inner.sock.as_ref().expect("socket bound").connect(&addr).map_err(|e| uv_code_of_io_error(&e))?;
            inner.connected = true;
            Ok(())
        })();
        drop(guard);
        unit_result(r)
    }
}

pub(crate) unsafe fn send(socket: Obj, data: Obj, opt_addr: Obj) -> Obj {
    unsafe {
        let udp = get(socket);
        if lean_array_size(data) == 0 {
            lean_dec(data);
            let promise = new_promise();
            resolve_with_code(0, promise);
            return lean_io_result_mk_ok(promise);
        }
        lean_mark_mt(data);
        let to = if lean_obj_tag(opt_addr) == 1 {
            Some(addr::socket_addr_of_lean(lean_ctor_get(opt_addr, 0)))
        } else {
            None
        };
        let promise = new_promise();
        lean_inc(promise);
        lean_inc(socket);
        let guard = reactor().lock();
        let inner = udp.inner();
        let r = (|| {
            match &to {
                None if !inner.connected => return Err(UV_EDESTADDRREQ),
                Some(_) if inner.connected => return Err(UV_EISCONN),
                Some(a) => maybe_deferred_bind(inner, Family::of(a), false)?,
                None => {}
            }
            inner.sends.push_back(Send { promise, data, to });
            Ok(())
        })();
        drop(guard);
        match r {
            Ok(()) => {
                reactor().wake();
                lean_io_result_mk_ok(promise)
            }
            Err(code) => {
                lean_dec(promise);
                lean_dec(promise);
                lean_dec(socket);
                lean_dec(data);
                uv_io_error(code)
            }
        }
    }
}

unsafe fn start_read(socket: Obj, mode: ReadMode, size: u64) -> Obj {
    unsafe {
        let udp = get(socket);
        let guard = reactor().lock();
        let inner = udp.inner();
        if !inner.promise_read.is_null() {
            drop(guard);
            return uv_io_error(UV_EALREADY);
        }
        if let Err(code) = maybe_deferred_bind(inner, Family::V4, false) {
            drop(guard);
            return uv_io_error(code);
        }
        let byte_array = if mode == ReadMode::Bytes {
            let cap = usize::try_from(size).unwrap_or_else(|_| lean_internal_panic_out_of_memory());
            lean_alloc_sarray(1, 0, cap)
        } else {
            Obj::null()
        };
        let promise = new_promise();
        inner.byte_array = byte_array;
        inner.promise_read = promise;
        inner.read = mode;
        lean_inc(promise);
        lean_inc(socket);
        drop(guard);
        reactor().wake();
        lean_io_result_mk_ok(promise)
    }
}

pub(crate) unsafe fn recv(socket: Obj, size: u64) -> Obj {
    unsafe { start_read(socket, ReadMode::Bytes, size) }
}

pub(crate) unsafe fn wait_readable(socket: Obj) -> Obj {
    unsafe { start_read(socket, ReadMode::Readable, 0) }
}

pub(crate) unsafe fn cancel_recv(socket: Obj) -> Obj {
    unsafe {
        let udp = get(socket);
        let guard = reactor().lock();
        let inner = udp.inner();
        if inner.promise_read.is_null() {
            return lean_io_result_mk_ok(lean_box(0));
        }
        inner.read = ReadMode::None;
        let promise = std::mem::replace(&mut inner.promise_read, Obj::null());
        let byte_array = std::mem::replace(&mut inner.byte_array, Obj::null());
        drop(guard);
        lean_dec(promise);
        if !byte_array.is_null() {
            lean_dec(byte_array);
        }
        // The loop's reference taken when the read started.
        lean_dec(socket);
        lean_io_result_mk_ok(lean_box(0))
    }
}

unsafe fn sock_name(socket: Obj, peer: bool) -> Obj {
    unsafe {
        let udp = get(socket);
        let guard = reactor().lock();
        let r = match udp.inner().sock.as_ref() {
            None => Err(UV_EBADF),
            Some(s) => {
                let a = if peer { s.peer_addr() } else { s.local_addr() };
                a.map_err(|e| uv_code_of_io_error(&e))
            }
        };
        drop(guard);
        match r {
            Ok(a) => lean_io_result_mk_ok(addr::lean_of_socket_addr(&a)),
            Err(code) => uv_io_error(code),
        }
    }
}

pub(crate) unsafe fn getpeername(socket: Obj) -> Obj {
    unsafe { sock_name(socket, true) }
}

pub(crate) unsafe fn getsockname(socket: Obj) -> Obj {
    unsafe { sock_name(socket, false) }
}

/// `uv__setsockopt`: the IPv4 or IPv6 variant of an option, by the socket's family.
unsafe fn with_socket(socket: Obj, f: impl FnOnce(&Sock) -> Result<(), i32>) -> Obj {
    unsafe {
        let udp = get(socket);
        let guard = reactor().lock();
        let r = match udp.inner().sock.as_ref() {
            None => Err(UV_EBADF),
            Some(s) => f(s),
        };
        drop(guard);
        unit_result(r)
    }
}

fn set(sock: &Sock, v4: Opt, v6: Opt, value: i32) -> Result<(), i32> {
    let opt = if sock.family == Family::V6 { v6 } else { v4 };
    sock.set_opt(opt, value).map_err(|e| uv_code_of_io_error(&e))
}

pub(crate) unsafe fn set_broadcast(socket: Obj, on: u8) -> Obj {
    unsafe { with_socket(socket, |s| s.set_opt(Opt::Broadcast, (on != 0) as i32).map_err(|e| uv_code_of_io_error(&e))) }
}

pub(crate) unsafe fn set_multicast_loop(socket: Obj, on: u8) -> Obj {
    unsafe { with_socket(socket, |s| set(s, Opt::MulticastLoop, Opt::Ipv6MulticastLoop, (on != 0) as i32)) }
}

pub(crate) unsafe fn set_multicast_ttl(socket: Obj, ttl: u32) -> Obj {
    unsafe {
        if ttl > 255 {
            return uv_io_error(UV_EINVAL);
        }
        with_socket(socket, |s| set(s, Opt::MulticastTtl, Opt::Ipv6MulticastHops, ttl as i32))
    }
}

pub(crate) unsafe fn set_ttl(socket: Obj, ttl: u32) -> Obj {
    unsafe {
        if !(1..=255).contains(&ttl) {
            return uv_io_error(UV_EINVAL);
        }
        with_socket(socket, |s| set(s, Opt::Ttl, Opt::Ipv6UnicastHops, ttl as i32))
    }
}

#[cfg(unix)]
mod mcast {
    pub const IPPROTO_IP: i32 = libc::IPPROTO_IP;
    pub const IPPROTO_IPV6: i32 = libc::IPPROTO_IPV6;
    pub const IP_ADD: i32 = libc::IP_ADD_MEMBERSHIP;
    pub const IP_DROP: i32 = libc::IP_DROP_MEMBERSHIP;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub const IPV6_JOIN: i32 = libc::IPV6_ADD_MEMBERSHIP;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub const IPV6_LEAVE: i32 = libc::IPV6_DROP_MEMBERSHIP;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub const IPV6_JOIN: i32 = libc::IPV6_JOIN_GROUP;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub const IPV6_LEAVE: i32 = libc::IPV6_LEAVE_GROUP;
    pub const IP_MULTICAST_IF: i32 = libc::IP_MULTICAST_IF;
    pub const IPV6_MULTICAST_IF: i32 = libc::IPV6_MULTICAST_IF;
}

#[cfg(windows)]
mod mcast {
    use windows_sys::Win32::Networking::WinSock as ws;
    pub const IPPROTO_IP: i32 = ws::IPPROTO_IP;
    pub const IPPROTO_IPV6: i32 = ws::IPPROTO_IPV6;
    pub const IP_ADD: i32 = ws::IP_ADD_MEMBERSHIP;
    pub const IP_DROP: i32 = ws::IP_DROP_MEMBERSHIP;
    pub const IPV6_JOIN: i32 = ws::IPV6_ADD_MEMBERSHIP;
    pub const IPV6_LEAVE: i32 = ws::IPV6_DROP_MEMBERSHIP;
    pub const IP_MULTICAST_IF: i32 = ws::IP_MULTICAST_IF;
    pub const IPV6_MULTICAST_IF: i32 = ws::IPV6_MULTICAST_IF;
}

/// `uv_udp_set_membership`: `membership` is `0` (leave) or `1` (join).
pub(crate) unsafe fn set_membership(socket: Obj, multicast: Obj, interface: Obj, membership: u8) -> Obj {
    unsafe {
        let group = addr::ip_of_lean(multicast);
        let iface = if interface.is_scalar() { None } else { Some(addr::ip_of_lean(lean_ctor_get(interface, 0))) };
        let join = match membership {
            0 => false,
            1 => true,
            _ => return uv_io_error(UV_EINVAL),
        };
        let udp = get(socket);
        let guard = reactor().lock();
        let inner = udp.inner();
        let r = (|| {
            let family = if group.is_ipv4() { Family::V4 } else { Family::V6 };
            maybe_deferred_bind(inner, family, true)?;
            let sock = inner.sock.as_ref().expect("socket bound");
            let result = match group {
                IpAddr::V4(g) => {
                    let local = match iface {
                        Some(IpAddr::V4(a)) => a,
                        Some(IpAddr::V6(_)) => return Err(UV_EINVAL),
                        None => Ipv4Addr::UNSPECIFIED,
                    };
                    let mut mreq = [0u8; 8];
                    mreq[..4].copy_from_slice(&g.octets());
                    mreq[4..].copy_from_slice(&local.octets());
                    sock.set_raw(mcast::IPPROTO_IP, if join { mcast::IP_ADD } else { mcast::IP_DROP }, &mreq)
                }
                IpAddr::V6(g) => {
                    // A Lean address has no zone, so the interface index is unspecified.
                    let mut mreq = [0u8; 20];
                    mreq[..16].copy_from_slice(&g.octets());
                    sock.set_raw(mcast::IPPROTO_IPV6, if join { mcast::IPV6_JOIN } else { mcast::IPV6_LEAVE }, &mreq)
                }
            };
            result.map_err(|e| uv_code_of_io_error(&e))
        })();
        drop(guard);
        unit_result(r)
    }
}

/// `uv_udp_set_multicast_interface`.
pub(crate) unsafe fn set_multicast_interface(socket: Obj, interface: Obj) -> Obj {
    unsafe {
        let iface = addr::ip_of_lean(interface);
        with_socket(socket, |s| {
            let r = match iface {
                IpAddr::V4(a) => s.set_raw(mcast::IPPROTO_IP, mcast::IP_MULTICAST_IF, &a.octets()),
                // A Lean address has no zone: interface index 0 selects the default.
                IpAddr::V6(_) => s.set_raw(mcast::IPPROTO_IPV6, mcast::IPV6_MULTICAST_IF, &0u32.to_ne_bytes()),
            };
            r.map_err(|e| uv_code_of_io_error(&e))
        })
    }
}
