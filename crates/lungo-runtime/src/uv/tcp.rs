//! `Std.Internal.UV.TCP`, ported from `runtime/uv/tcp.cpp` together with the libuv stream and
//! TCP semantics it relies on (deferred bind errors, lazily created sockets, accept queues of
//! depth one, read/write/shutdown requests).
//!
//! While an operation is pending, the event loop holds a reference to the socket object and to
//! the operation's promise, released when the operation completes or is cancelled.

use super::errno::*;
use super::reactor::{IoSource, LoopGuard, reactor};
use super::sys::{Family, Kind, Opt, RawSock, Sock};
use super::*;
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::net::SocketAddr;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadMode {
    None,
    /// `recv?`: read into the pending byte array.
    Bytes,
    /// `waitReadable`: report readability without reading.
    Readable,
}

struct Write {
    promise: Obj,
    data: Obj,
    index: usize,
    offset: usize,
}

struct Inner {
    sock: Option<Sock>,
    readable: bool,
    writable: bool,
    listening: bool,
    delayed_error: i32,
    nodelay: bool,
    keepalive: bool,
    connect: Obj,
    accepted: Option<Sock>,
    promise_accept: Obj,
    client: Obj,
    read: ReadMode,
    promise_read: Obj,
    byte_array: Obj,
    writes: VecDeque<Write>,
    promise_shutdown: Obj,
    shutting: bool,
    shut: bool,
}

pub(crate) struct Tcp {
    obj: Obj,
    source: u64,
    inner: UnsafeCell<Inner>,
}

unsafe fn finalize(p: *mut ()) {
    unsafe {
        let r = reactor();
        let guard = r.lock();
        let tcp = Box::from_raw(p as *mut Tcp);
        r.unregister_source(&guard, tcp.source);
        let inner = &*tcp.inner.get();
        if !inner.promise_shutdown.is_null()
            || !inner.promise_accept.is_null()
            || !inner.promise_read.is_null()
            || !inner.byte_array.is_null()
            || !inner.connect.is_null()
            || !inner.writes.is_empty()
        {
            lean_internal_panic("a TCP socket with pending operations was finalized");
        }
        drop(tcp);
        drop(guard);
    }
}

unsafe fn for_each(p: *mut (), f: &mut dyn FnMut(Obj)) {
    unsafe {
        let inner = &*(*(p as *mut Tcp)).inner.get();
        for o in [inner.promise_accept, inner.promise_shutdown, inner.promise_read, inner.byte_array] {
            if !o.is_null() {
                f(o);
            }
        }
    }
}

static CLASS: ExternalClass = ExternalClass { finalize, for_each };

unsafe fn get<'a>(o: Obj) -> &'a Tcp {
    unsafe { external_data::<Tcp>(o, &CLASS, "TCP socket") }
}

impl Tcp {
    #[allow(clippy::mut_from_ref)]
    unsafe fn inner(&self) -> &mut Inner {
        unsafe { &mut *self.inner.get() }
    }
}

fn keepalive_opts(sock: &Sock, on: bool, delay: u32) -> Result<(), i32> {
    sock.set_opt(Opt::KeepAlive, on as i32).map_err(|e| uv_code_of_io_error(&e))?;
    if on {
        sock.set_opt(Opt::KeepIdle, delay as i32).map_err(|e| uv_code_of_io_error(&e))?;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            sock.set_int(libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 1).map_err(|e| uv_code_of_io_error(&e))?;
            sock.set_int(libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 10).map_err(|e| uv_code_of_io_error(&e))?;
        }
    }
    Ok(())
}

/// `uv__stream_open` for a socket: applies the handle's pending options.
fn open(inner: &mut Inner, sock: Sock, readable: bool, writable: bool) -> Result<(), i32> {
    if inner.nodelay {
        sock.set_opt(Opt::NoDelay, 1).map_err(|e| uv_code_of_io_error(&e))?;
    }
    if inner.keepalive {
        keepalive_opts(&sock, true, 60)?;
    }
    inner.sock = Some(sock);
    inner.readable |= readable;
    inner.writable |= writable;
    Ok(())
}

/// `maybe_new_socket`.
fn maybe_new_socket(inner: &mut Inner, family: Family, readable: bool, writable: bool) -> Result<(), i32> {
    if inner.sock.is_some() {
        inner.readable |= readable;
        inner.writable |= writable;
        return Ok(());
    }
    let sock = Sock::new(family, Kind::Stream).map_err(|e| uv_code_of_io_error(&e))?;
    open(inner, sock, readable, writable)
}

impl IoSource for Tcp {
    fn interest(&self) -> Option<(RawSock, bool, bool)> {
        let inner = unsafe { &*self.inner.get() };
        let sock = inner.sock.as_ref()?.raw();
        let mut read = false;
        let mut write = false;
        // Writable interest: a pending connect, queued writes, or a pending shutdown.
        if !inner.connect.is_null() || !inner.writes.is_empty() || (inner.shutting && !inner.shut) {
            write = true;
        }
        if inner.listening && inner.accepted.is_none() {
            read = true;
        }
        if inner.read != ReadMode::None {
            read = true;
        }
        Some((sock, read, write))
    }

    unsafe fn on_ready(&self, readable: bool, writable: bool) {
        unsafe {
            let obj = self.obj;
            // Keep the socket alive while its completions release the loop's references.
            lean_inc(obj);
            if writable && !self.inner().connect.is_null() {
                complete_connect(self, None);
            }
            if readable && self.inner().listening {
                server_io(self);
            }
            if readable && self.inner().read != ReadMode::None {
                do_read(self);
            }
            if writable && self.inner().connect.is_null() {
                flush_writes(self);
            }
            lean_dec(obj);
        }
    }
}

unsafe fn complete_connect(tcp: &Tcp, fed: Option<()>) {
    unsafe {
        let inner = tcp.inner();
        if inner.connect.is_null() {
            return;
        }
        let status = if inner.delayed_error != 0 {
            std::mem::replace(&mut inner.delayed_error, 0)
        } else if fed.is_some() {
            return;
        } else {
            let sock = match inner.sock.as_ref() {
                Some(s) => s,
                None => lean_internal_panic("pending TCP connect without a socket"),
            };
            match sock.take_error() {
                Ok(0) => 0,
                Ok(raw) => {
                    let code = uv_code_of_os_error(raw);
                    if code == in_progress_code() {
                        return;
                    }
                    code
                }
                Err(e) => uv_code_of_io_error(&e),
            }
        };
        let promise = std::mem::replace(&mut inner.connect, Obj::null());
        let cancelled: Vec<Write> = if status < 0 { inner.writes.drain(..).collect() } else { Vec::new() };
        resolve_with_code(status, promise);
        lean_dec(promise);
        for w in cancelled {
            resolve_with_code(UV_ECANCELED, w.promise);
            lean_dec(w.promise);
            lean_dec(w.data);
            lean_dec(tcp.obj);
        }
        // The event loop does not own the socket anymore.
        lean_dec(tcp.obj);
    }
}

#[cfg(unix)]
fn in_progress_code() -> i32 {
    -libc::EINPROGRESS
}

#[cfg(windows)]
fn in_progress_code() -> i32 {
    uv_code_of_os_error(windows_sys::Win32::Networking::WinSock::WSAEWOULDBLOCK)
}

/// `uv__server_io`: accepts one pending connection at a time.
unsafe fn server_io(tcp: &Tcp) {
    unsafe {
        loop {
            let inner = tcp.inner();
            if inner.accepted.is_some() {
                return;
            }
            let result = match inner.sock.as_ref() {
                Some(s) => s.accept(),
                None => return,
            };
            match result {
                Ok(s) => {
                    inner.accepted = Some(s);
                    connection_cb(tcp, 0);
                    if tcp.inner().accepted.is_some() {
                        // Nobody is accepting: stop polling until `uv_accept` is called.
                        return;
                    }
                }
                Err(e) => {
                    let code = uv_code_of_io_error(&e);
                    if code == UV_EAGAIN || is_would_block(&e) {
                        return;
                    }
                    if code == UV_ECONNABORTED {
                        continue;
                    }
                    connection_cb(tcp, code);
                    return;
                }
            }
        }
    }
}

/// `uv_accept`: moves the pending connection of `server` into `client`.
unsafe fn uv_accept(server: &Tcp, client: Obj) -> i32 {
    unsafe {
        let Some(s) = server.inner().accepted.take() else { return UV_EAGAIN };
        match open(get(client).inner(), s, true, true) {
            Ok(()) => 0,
            Err(code) => code,
        }
    }
}

/// The connection callback installed by `listen`.
unsafe fn connection_cb(tcp: &Tcp, status: i32) {
    unsafe {
        let inner = tcp.inner();
        if inner.promise_accept.is_null() {
            return;
        }
        let promise = std::mem::replace(&mut inner.promise_accept, Obj::null());
        let client = std::mem::replace(&mut inner.client, Obj::null());
        if status < 0 {
            resolve_with_code(status, promise);
            lean_dec(promise);
            lean_dec(client);
            lean_dec(tcp.obj);
            return;
        }
        let r = uv_accept(tcp, client);
        if r < 0 {
            lean_dec(client);
            resolve_with_code(r, promise);
            lean_dec(promise);
            lean_dec(tcp.obj);
            return;
        }
        crate::task::promise_resolve(except_ok(client), promise);
        lean_dec(promise);
        // The accept took a reference to the server that the connection releases.
        lean_dec(tcp.obj);
    }
}

unsafe fn do_read(tcp: &Tcp) {
    unsafe {
        let inner = tcp.inner();
        let value = match inner.read {
            ReadMode::None => return,
            ReadMode::Readable => except_ok(lean_box(1)),
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
                        Some(s) => s.recv(buf),
                        None => lean_internal_panic("pending TCP read without a socket"),
                    };
                    match result {
                        Ok(0) => {
                            inner.byte_array = Obj::null();
                            lean_dec(ba);
                            except_ok(none())
                        }
                        Ok(n) => {
                            inner.byte_array = Obj::null();
                            lean_sarray_set_size(ba, n);
                            except_ok(some(ba))
                        }
                        Err(e) if is_would_block(&e) => return,
                        Err(e) => {
                            inner.byte_array = Obj::null();
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
        lean_dec(tcp.obj);
    }
}

unsafe fn flush_writes(tcp: &Tcp) {
    unsafe {
        loop {
            let inner = tcp.inner();
            let Some(front) = inner.writes.front_mut() else { break };
            let sock = match inner.sock.as_ref() {
                Some(s) => s,
                None => lean_internal_panic("pending TCP write without a socket"),
            };
            let mut status = 0;
            loop {
                // The unsent remainder of the request, written with one vectored call as
                // libuv's `uv__write` does.
                let n_bufs = lean_array_size(front.data);
                let mut pending: Vec<&[u8]> = Vec::new();
                for i in front.index..n_bufs {
                    let ba = lean_array_get_core(front.data, i);
                    let bytes = std::slice::from_raw_parts(lean_sarray_cptr(ba), lean_sarray_size(ba));
                    let start = if i == front.index { front.offset } else { 0 };
                    if start < bytes.len() {
                        pending.push(&bytes[start..]);
                    }
                }
                if pending.is_empty() {
                    break;
                }
                match sock.send_vectored(&pending) {
                    Ok(mut n) => {
                        // Advance past the bytes written.
                        while n > 0 && front.index < n_bufs {
                            let ba = lean_array_get_core(front.data, front.index);
                            let left = lean_sarray_size(ba) - front.offset;
                            if n >= left {
                                n -= left;
                                front.index += 1;
                                front.offset = 0;
                            } else {
                                front.offset += n;
                                n = 0;
                            }
                        }
                        while front.index < n_bufs
                            && lean_sarray_size(lean_array_get_core(front.data, front.index)) == front.offset
                        {
                            front.index += 1;
                            front.offset = 0;
                        }
                    }
                    Err(e) if is_would_block(&e) => return,
                    Err(e) => {
                        status = uv_code_of_io_error(&e);
                        break;
                    }
                }
            }
            let w = inner.writes.pop_front().expect("front write");
            resolve_with_code(status, w.promise);
            lean_dec(w.promise);
            lean_dec(w.data);
            lean_dec(tcp.obj);
        }
        let inner = tcp.inner();
        if inner.shutting && !inner.shut && inner.writes.is_empty() {
            inner.shut = true;
            let status = match inner.sock.as_ref() {
                Some(s) => match s.shutdown_write() {
                    Ok(()) => 0,
                    Err(e) => uv_code_of_io_error(&e),
                },
                None => UV_ENOTCONN,
            };
            let promise = std::mem::replace(&mut inner.promise_shutdown, Obj::null());
            if status < 0 {
                resolve_with_code(status, promise);
            } else {
                crate::task::promise_resolve(except_ok(lean_box(0)), promise);
            }
            lean_dec(promise);
            lean_dec(tcp.obj);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------------------------

unsafe fn new_socket(guard: &LoopGuard<'_>) -> Obj {
    unsafe {
        let tcp = Box::into_raw(Box::new(Tcp {
            obj: Obj::null(),
            source: 0,
            inner: UnsafeCell::new(Inner {
                sock: None,
                readable: false,
                writable: false,
                listening: false,
                delayed_error: 0,
                nodelay: false,
                keepalive: false,
                connect: Obj::null(),
                accepted: None,
                promise_accept: Obj::null(),
                client: Obj::null(),
                read: ReadMode::None,
                promise_read: Obj::null(),
                byte_array: Obj::null(),
                writes: VecDeque::new(),
                promise_shutdown: Obj::null(),
                shutting: false,
                shut: false,
            }),
        }));
        let obj = lean_alloc_external(&CLASS, tcp as *mut ());
        lean_mark_mt(obj);
        (*tcp).obj = obj;
        (*tcp).source = reactor().register_source(guard, tcp as *const Tcp as *const dyn IoSource);
        obj
    }
}

pub(crate) unsafe fn new() -> Obj {
    unsafe {
        let guard = reactor().lock();
        let obj = new_socket(&guard);
        drop(guard);
        lean_io_result_mk_ok(obj)
    }
}

pub(crate) unsafe fn bind(socket: Obj, addr: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let addr = addr::socket_addr_of_lean(addr);
        let guard = reactor().lock();
        let r = tcp_bind(tcp.inner(), &addr);
        drop(guard);
        match r {
            Ok(()) => lean_io_result_mk_ok(lean_box(0)),
            Err(code) => uv_io_error(code),
        }
    }
}

fn tcp_bind(inner: &mut Inner, addr: &SocketAddr) -> Result<(), i32> {
    let family = Family::of(addr);
    maybe_new_socket(inner, family, false, false)?;
    let sock = inner.sock.as_ref().expect("socket created");
    #[cfg(unix)]
    sock.set_opt(Opt::ReuseAddr, 1).map_err(|e| uv_code_of_io_error(&e))?;
    if family == Family::V6 {
        sock.set_opt(Opt::Ipv6Only, 0).map_err(|e| uv_code_of_io_error(&e))?;
    }
    match sock.bind(addr) {
        Ok(()) => inner.delayed_error = 0,
        Err(e) => {
            let code = uv_code_of_io_error(&e);
            if code == UV_EADDRINUSE {
                inner.delayed_error = code;
            } else if code == UV_EAFNOSUPPORT {
                return Err(UV_EINVAL);
            } else {
                return Err(code);
            }
        }
    }
    Ok(())
}

pub(crate) unsafe fn listen(socket: Obj, backlog: u32) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        let r = (|| {
            if inner.delayed_error != 0 {
                return Err(inner.delayed_error);
            }
            maybe_new_socket(inner, Family::V4, false, false)?;
            inner.sock.as_ref().expect("socket created").listen(backlog as i32).map_err(|e| uv_code_of_io_error(&e))?;
            inner.listening = true;
            Ok(())
        })();
        drop(guard);
        reactor().wake();
        match r {
            Ok(()) => lean_io_result_mk_ok(lean_box(0)),
            Err(code) => uv_io_error(code),
        }
    }
}

pub(crate) unsafe fn connect(socket: Obj, addr: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let addr = addr::socket_addr_of_lean(addr);
        let promise = new_promise();
        // The event loop owns the socket and the promise while the connection is pending.
        lean_inc(socket);
        lean_inc(promise);
        let guard = reactor().lock();
        let inner = tcp.inner();
        let r = (|| {
            if !inner.connect.is_null() {
                return Err(UV_EALREADY);
            }
            if inner.delayed_error == 0 {
                maybe_new_socket(inner, Family::of(&addr), true, true)?;
                match inner.sock.as_ref().expect("socket created").connect(&addr) {
                    Ok(_) => {}
                    Err(e) => {
                        let code = uv_code_of_io_error(&e);
                        if code == UV_ECONNREFUSED {
                            inner.delayed_error = code;
                        } else {
                            return Err(code);
                        }
                    }
                }
            }
            inner.connect = promise;
            Ok(inner.delayed_error != 0)
        })();
        drop(guard);
        match r {
            Ok(feed) => {
                if feed {
                    // Report the delayed error from the loop, as `uv__io_feed` does.
                    let s = SendObj(socket);
                    reactor().defer(Box::new(move || {
                        let s = s;
                        let tcp = get(s.0);
                        lean_inc(s.0);
                        complete_connect(tcp, Some(()));
                        lean_dec(s.0);
                    }));
                } else {
                    reactor().wake();
                }
                lean_io_result_mk_ok(promise)
            }
            Err(code) => {
                lean_dec(promise);
                lean_dec(promise);
                lean_dec(socket);
                uv_io_error(code)
            }
        }
    }
}

pub(crate) unsafe fn send(socket: Obj, data: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        if lean_array_size(data) == 0 {
            lean_dec(data);
            let promise = new_promise();
            resolve_with_code(0, promise);
            return lean_io_result_mk_ok(promise);
        }
        // The loop thread reads the buffers and releases them.
        lean_mark_mt(data);
        let promise = new_promise();
        lean_inc(promise);
        lean_inc(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        let r = if inner.sock.is_none() {
            Err(UV_EBADF)
        } else if !inner.writable {
            Err(UV_EPIPE)
        } else {
            inner.writes.push_back(Write { promise, data, index: 0, offset: 0 });
            Ok(())
        };
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

unsafe fn start_read(socket: Obj, mode: ReadMode, buffer_size: u64) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        if !inner.promise_read.is_null() {
            drop(guard);
            return uv_io_error(UV_EALREADY);
        }
        let byte_array = if mode == ReadMode::Bytes {
            let cap = usize::try_from(buffer_size).unwrap_or_else(|_| lean_internal_panic_out_of_memory());
            lean_alloc_sarray(1, 0, cap)
        } else {
            Obj::null()
        };
        let promise = new_promise();
        if !inner.readable || inner.sock.is_none() {
            drop(guard);
            if !byte_array.is_null() {
                lean_dec(byte_array);
            }
            lean_dec(promise);
            return uv_io_error(UV_ENOTCONN);
        }
        inner.byte_array = byte_array;
        inner.promise_read = promise;
        inner.read = mode;
        // The event loop owns the socket while the read is pending.
        lean_inc(socket);
        lean_inc(promise);
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
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
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
        lean_dec(socket);
        lean_io_result_mk_ok(lean_box(0))
    }
}

unsafe fn parallel_accept_error() -> Obj {
    unsafe {
        let msg = lean_mk_string(
            "parallel accept is not allowed! consider binding multiple sockets to the same address and accepting on them instead",
        );
        let e = uv_error(UV_EALREADY, Some(msg));
        lean_dec(msg);
        lean_io_result_mk_error(e)
    }
}

pub(crate) unsafe fn accept(socket: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        if !tcp.inner().promise_accept.is_null() {
            drop(guard);
            return parallel_accept_error();
        }
        let promise = new_promise();
        let client = new_socket(&guard);
        let r = uv_accept(tcp, client);
        if r < 0 && r != UV_EAGAIN {
            drop(guard);
            lean_dec(client);
            resolve_with_code(r, promise);
        } else if r >= 0 {
            drop(guard);
            crate::task::promise_resolve(except_ok(client), promise);
        } else {
            // The event loop owns the server until a connection arrives.
            lean_inc(socket);
            lean_inc(promise);
            let inner = tcp.inner();
            inner.promise_accept = promise;
            inner.client = client;
            drop(guard);
            reactor().wake();
        }
        lean_io_result_mk_ok(promise)
    }
}

pub(crate) unsafe fn try_accept(socket: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        if !tcp.inner().promise_accept.is_null() {
            drop(guard);
            return parallel_accept_error();
        }
        let client = new_socket(&guard);
        let r = uv_accept(tcp, client);
        drop(guard);
        reactor().wake();
        if r < 0 && r != UV_EAGAIN {
            lean_dec(client);
            uv_io_error(r)
        } else if r >= 0 {
            lean_io_result_mk_ok(except_ok(some(client)))
        } else {
            lean_dec(client);
            lean_io_result_mk_ok(except_ok(none()))
        }
    }
}

pub(crate) unsafe fn cancel_accept(socket: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        if inner.promise_accept.is_null() {
            return lean_io_result_mk_ok(lean_box(0));
        }
        let promise = std::mem::replace(&mut inner.promise_accept, Obj::null());
        let client = std::mem::replace(&mut inner.client, Obj::null());
        drop(guard);
        lean_dec(promise);
        if !client.is_null() {
            lean_dec(client);
        }
        lean_dec(socket);
        lean_io_result_mk_ok(lean_box(0))
    }
}

pub(crate) unsafe fn shutdown(socket: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        if !inner.promise_shutdown.is_null() {
            drop(guard);
            let msg = lean_mk_string("shutdown already in progress");
            let e = uv_error(UV_EALREADY, Some(msg));
            lean_dec(msg);
            return lean_io_result_mk_error(e);
        }
        if !inner.writable || inner.shut || inner.shutting {
            drop(guard);
            return uv_io_error(UV_ENOTCONN);
        }
        let promise = new_promise();
        inner.promise_shutdown = promise;
        inner.shutting = true;
        inner.writable = false;
        lean_inc(promise);
        lean_inc(socket);
        drop(guard);
        reactor().wake();
        lean_io_result_mk_ok(promise)
    }
}

unsafe fn sock_name(socket: Obj, peer: bool) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        let r = if inner.delayed_error != 0 {
            Err(inner.delayed_error)
        } else {
            match inner.sock.as_ref() {
                None => Err(UV_EBADF),
                Some(s) => {
                    let a = if peer { s.peer_addr() } else { s.local_addr() };
                    a.map_err(|e| uv_code_of_io_error(&e))
                }
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

pub(crate) unsafe fn nodelay(socket: Obj) -> Obj {
    unsafe {
        let tcp = get(socket);
        let guard = reactor().lock();
        let inner = tcp.inner();
        let r = match inner.sock.as_ref() {
            Some(s) => s.set_opt(Opt::NoDelay, 1).map_err(|e| uv_code_of_io_error(&e)),
            None => Ok(()),
        };
        if r.is_ok() {
            inner.nodelay = true;
        }
        drop(guard);
        match r {
            Ok(()) => lean_io_result_mk_ok(lean_box(0)),
            Err(code) => uv_io_error(code),
        }
    }
}

pub(crate) unsafe fn keepalive(socket: Obj, enable: u8, delay: u32) -> Obj {
    unsafe {
        let tcp = get(socket);
        let on = enable != 0;
        let guard = reactor().lock();
        let inner = tcp.inner();
        let r = match inner.sock.as_ref() {
            Some(s) => keepalive_opts(s, on, delay),
            None => Ok(()),
        };
        if r.is_ok() {
            inner.keepalive = on;
        }
        drop(guard);
        match r {
            Ok(()) => lean_io_result_mk_ok(lean_box(0)),
            Err(code) => uv_io_error(code),
        }
    }
}
