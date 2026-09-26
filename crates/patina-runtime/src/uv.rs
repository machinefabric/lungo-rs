//! `Std.Internal.UV`: timers, signals, TCP and UDP sockets, DNS, and system information,
//! ported from Lean's `runtime/uv/*.cpp` and implemented without libuv on a process-wide event
//! loop (the private `reactor` module).
//!
//! Operations that complete asynchronously return `IO.Promise` values resolved from the event
//! loop with `Except IO.Error _`, exactly as Lean's runtime does; errors are libuv error codes
//! decoded by `lean_decode_uv_error`.

mod addr;
mod dns;
pub(crate) mod errno;
mod reactor;
mod signal;
mod sys;
mod system;
mod tcp;
mod timer;
mod udp;

pub(crate) use errno::{uv_code_of_os_error, uv_strerror};

use crate::object::*;
use crate::registry::Unsupported;
use errno::uv_code_of_io_error;

/// Every `lean_uv_*` symbol is implemented.
pub(crate) const UNSUPPORTED: &[Unsupported] = &[];

/// The data of external object `o` of class `class`, failing hard on a mismatch.
unsafe fn external_data<'a, T>(o: Obj, class: &'static ExternalClass, what: &str) -> &'a T {
    unsafe {
        if !std::ptr::eq(lean_get_external_class(o), class) {
            lean_internal_panic(&format!("expected a {what} object"));
        }
        &*(lean_get_external_data(o) as *const T)
    }
}

/// A new promise, shared with the event loop thread.
unsafe fn new_promise() -> Obj {
    unsafe {
        let p = crate::task::new_promise();
        lean_mark_mt(p);
        p
    }
}

unsafe fn promise_is_resolved(p: Obj) -> bool {
    unsafe { crate::task::promise_is_resolved(p) }
}

/// `Except.ok v`.
unsafe fn except_ok(v: Obj) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(1, 1, 0);
        lean_ctor_set(r, 0, v);
        r
    }
}

/// `Except.error e`.
unsafe fn except_err(e: Obj) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 1, 0);
        lean_ctor_set(r, 0, e);
        r
    }
}

unsafe fn some(v: Obj) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(1, 1, 0);
        lean_ctor_set(r, 0, v);
        r
    }
}

fn none() -> Obj {
    lean_box(0)
}

/// `lean_decode_uv_error`: the `IO.Error` for libuv error `code`. `fname` is borrowed.
unsafe fn uv_error(code: i32, fname: Option<Obj>) -> Obj {
    unsafe { crate::io::decode_uv_error(code, fname) }
}

/// An `IO` error result for libuv error `code`.
unsafe fn uv_io_error(code: i32) -> Obj {
    unsafe { lean_io_result_mk_error(uv_error(code, None)) }
}

/// `lean_promise_resolve_with_code`: resolves `promise` (borrowed) with `Except.ok ()` or the
/// decoded error.
unsafe fn resolve_with_code(status: i32, promise: Obj) {
    unsafe {
        let v = if status == 0 { except_ok(lean_box(0)) } else { except_err(uv_error(status, None)) };
        crate::task::promise_resolve(v, promise);
    }
}

fn is_would_block(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::WouldBlock || uv_code_of_io_error(e) == errno::UV_EAGAIN
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_uv_event_loop_configure(_options: b_obj) -> obj {
            // Idle-time metrics and `SIGPROF` blocking configure libuv's loop; neither affects
            // the observable behaviour of this event loop.
            reactor::reactor();
            lean_box(0)
        }

        fn lean_uv_event_loop_alive() -> u8 {
            // The loop always holds its wake-up handle, so it is always alive, as libuv's is
            // with its `uv_async_t`.
            reactor::reactor();
            1
        }

        fn lean_uv_timer_mk(timeout: u64, repeating: u8) -> obj { timer::mk(timeout, repeating != 0) }
        fn lean_uv_timer_next(t: b_obj) -> obj { timer::next(t) }
        fn lean_uv_timer_reset(t: b_obj) -> obj { timer::reset(t) }
        fn lean_uv_timer_stop(t: b_obj) -> obj { timer::stop(t) }
        fn lean_uv_timer_cancel(t: b_obj) -> obj { timer::cancel(t) }

        fn lean_uv_signal_mk(signum: u32, repeating: u8) -> obj { signal::mk(signum, repeating != 0) }
        fn lean_uv_signal_next(s: b_obj) -> obj { signal::next(s) }
        fn lean_uv_signal_stop(s: b_obj) -> obj { signal::stop(s) }
        fn lean_uv_signal_cancel(s: b_obj) -> obj { signal::cancel(s) }

        fn lean_uv_tcp_new() -> obj { tcp::new() }
        fn lean_uv_tcp_connect(s: b_obj, addr: b_obj) -> obj { tcp::connect(s, addr) }
        fn lean_uv_tcp_send(s: b_obj, data: obj) -> obj { tcp::send(s, data) }
        fn lean_uv_tcp_recv(s: b_obj, size: u64) -> obj { tcp::recv(s, size) }
        fn lean_uv_tcp_wait_readable(s: b_obj) -> obj { tcp::wait_readable(s) }
        fn lean_uv_tcp_cancel_recv(s: b_obj) -> obj { tcp::cancel_recv(s) }
        fn lean_uv_tcp_bind(s: b_obj, addr: b_obj) -> obj { tcp::bind(s, addr) }
        fn lean_uv_tcp_listen(s: b_obj, backlog: u32) -> obj { tcp::listen(s, backlog) }
        fn lean_uv_tcp_accept(s: b_obj) -> obj { tcp::accept(s) }
        fn lean_uv_tcp_try_accept(s: b_obj) -> obj { tcp::try_accept(s) }
        fn lean_uv_tcp_cancel_accept(s: b_obj) -> obj { tcp::cancel_accept(s) }
        fn lean_uv_tcp_shutdown(s: b_obj) -> obj { tcp::shutdown(s) }
        fn lean_uv_tcp_getpeername(s: b_obj) -> obj { tcp::getpeername(s) }
        fn lean_uv_tcp_getsockname(s: b_obj) -> obj { tcp::getsockname(s) }
        fn lean_uv_tcp_nodelay(s: b_obj) -> obj { tcp::nodelay(s) }
        fn lean_uv_tcp_keepalive(s: b_obj, enable: u8, delay: u32) -> obj { tcp::keepalive(s, enable, delay) }

        fn lean_uv_udp_new() -> obj { udp::new() }
        fn lean_uv_udp_bind(s: b_obj, addr: b_obj) -> obj { udp::bind(s, addr) }
        fn lean_uv_udp_connect(s: b_obj, addr: b_obj) -> obj { udp::connect(s, addr) }
        fn lean_uv_udp_send(s: b_obj, data: obj, addr: b_obj) -> obj { udp::send(s, data, addr) }
        fn lean_uv_udp_recv(s: b_obj, size: u64) -> obj { udp::recv(s, size) }
        fn lean_uv_udp_wait_readable(s: b_obj) -> obj { udp::wait_readable(s) }
        fn lean_uv_udp_cancel_recv(s: b_obj) -> obj { udp::cancel_recv(s) }
        fn lean_uv_udp_getpeername(s: b_obj) -> obj { udp::getpeername(s) }
        fn lean_uv_udp_getsockname(s: b_obj) -> obj { udp::getsockname(s) }
        fn lean_uv_udp_set_broadcast(s: b_obj, on: u8) -> obj { udp::set_broadcast(s, on) }
        fn lean_uv_udp_set_multicast_loop(s: b_obj, on: u8) -> obj { udp::set_multicast_loop(s, on) }
        fn lean_uv_udp_set_multicast_ttl(s: b_obj, ttl: u32) -> obj { udp::set_multicast_ttl(s, ttl) }
        fn lean_uv_udp_set_membership(s: b_obj, group: b_obj, iface: b_obj, membership: u8) -> obj {
            udp::set_membership(s, group, iface, membership)
        }
        fn lean_uv_udp_set_multicast_interface(s: b_obj, iface: b_obj) -> obj {
            udp::set_multicast_interface(s, iface)
        }
        fn lean_uv_udp_set_ttl(s: b_obj, ttl: u32) -> obj { udp::set_ttl(s, ttl) }

        fn lean_uv_dns_get_info(name: b_obj, service: b_obj, family: u8) -> obj { dns::get_info(name, service, family) }
        fn lean_uv_dns_get_name(addr: b_obj) -> obj { dns::get_name(addr) }

        fn lean_uv_pton_v4(s: b_obj) -> obj {
            match addr::c_str_bytes(s).and_then(addr::pton4) {
                Some(o) => some(addr::lean_of_ipv4(&std::net::Ipv4Addr::from(o))),
                None => none(),
            }
        }

        fn lean_uv_ntop_v4(a: b_obj) -> obj {
            lean_mk_string(&addr::ntop4(&addr::ipv4_of_lean(a).octets()))
        }

        fn lean_uv_pton_v6(s: b_obj) -> obj {
            match addr::c_str_bytes(s).and_then(addr::uv_pton6) {
                Some(o) => some(addr::lean_of_ipv6(&std::net::Ipv6Addr::from(o))),
                None => none(),
            }
        }

        fn lean_uv_ntop_v6(a: b_obj) -> obj {
            lean_mk_string(&addr::ntop6(&addr::ipv6_of_lean(a).octets()))
        }

        fn lean_uv_interface_addresses() -> obj {
            match addr::interface_addresses() {
                Err(_) => lean_io_result_mk_error(crate::io::mk::invalid_argument(
                    einval_errno(),
                    lean_mk_string("failed to get interface addresses"),
                )),
                Ok(list) => {
                    let arr = lean_alloc_array(list.len(), list.len());
                    for (i, iface) in list.iter().enumerate() {
                        let o = lean_alloc_ctor(0, 4, 1);
                        lean_ctor_set(o, 0, lean_mk_string(&iface.name));
                        lean_ctor_set(o, 1, addr::lean_of_mac(&iface.mac));
                        lean_ctor_set(o, 2, addr::lean_of_ip(&iface.address));
                        lean_ctor_set(o, 3, addr::lean_of_ip(&iface.netmask));
                        lean_ctor_set_uint8(o, 4 * size_of::<Obj>(), iface.internal as u8);
                        lean_array_set_core(arr, i, o);
                    }
                    lean_io_result_mk_ok(arr)
                }
            }
        }

        fn lean_uv_get_process_title() -> obj { system::get_process_title() }
        fn lean_uv_set_process_title(t: b_obj) -> obj { system::set_process_title(t) }
        fn lean_uv_uptime() -> obj { system::uptime_result() }
        fn lean_uv_os_getpid() -> obj { system::getpid() }
        fn lean_uv_os_getppid() -> obj { system::getppid() }
        fn lean_uv_cpu_info() -> obj { system::cpu_info_result() }
        fn lean_uv_cwd() -> obj { system::cwd() }
        fn lean_uv_chdir(p: b_obj) -> obj { system::chdir(p) }
        fn lean_uv_os_homedir() -> obj { system::os_homedir() }
        fn lean_uv_os_tmpdir() -> obj { system::os_tmpdir() }
        fn lean_uv_os_get_passwd() -> obj { system::os_get_passwd() }
        fn lean_uv_os_get_group(gid: u64) -> obj { system::os_get_group(gid) }
        fn lean_uv_os_environ() -> obj { system::os_environ() }
        fn lean_uv_os_getenv(name: b_obj) -> obj { system::os_getenv(name) }
        fn lean_uv_os_setenv(name: b_obj, value: b_obj) -> obj { system::os_setenv(name, value) }
        fn lean_uv_os_unsetenv(name: b_obj) -> obj { system::os_unsetenv(name) }
        fn lean_uv_os_gethostname() -> obj { system::os_gethostname() }
        fn lean_uv_os_getpriority(pid: u64) -> obj { system::os_getpriority(pid) }
        fn lean_uv_os_setpriority(pid: u64, priority: u64) -> obj { system::os_setpriority(pid, priority) }
        fn lean_uv_os_uname() -> obj { system::os_uname() }
        fn lean_uv_hrtime() -> obj { system::hrtime() }
        fn lean_uv_random(size: u64) -> obj { system::random(size) }
        fn lean_uv_getrusage() -> obj { system::getrusage() }
        fn lean_uv_exepath() -> obj { system::exepath() }
        fn lean_uv_get_free_memory() -> obj { system::get_free_memory() }
        fn lean_uv_get_total_memory() -> obj { system::get_total_memory() }
        fn lean_uv_get_constrained_memory() -> obj { system::get_constrained_memory() }
        fn lean_uv_get_available_memory() -> obj { system::get_available_memory() }
    }
}

/// The C runtime's `EINVAL`.
fn einval_errno() -> u32 {
    #[cfg(unix)]
    {
        libc::EINVAL as u32
    }
    #[cfg(windows)]
    {
        22
    }
}

#[cfg(test)]
mod tests {
    //! Expected outcomes were observed from Lean 4.34.1 (`lean --run`) running the same
    //! sequence of `Std.Internal.UV` operations over loopback.
    use super::externs::*;
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    // The `IO.Error` constructors the runtime chooses.
    const OTHER_ERROR: &str = "other_error";
    const INVALID_ARGUMENT: &str = "invalid_argument";
    const NO_SUCH_THING: &str = "no_such_thing";

    fn setup() {
        crate::exports::recording::install();
        crate::task::ensure_task_manager();
    }

    unsafe fn ok(r: Obj) -> Obj {
        unsafe {
            assert!(lean_io_result_is_ok(r), "expected an ok IO result");
            lean_io_result_take_value(r)
        }
    }

    /// The constructor and OS code of the `IO.Error` of a failed IO result (consumed).
    unsafe fn io_err(r: Obj) -> (&'static str, u32) {
        unsafe {
            assert!(lean_io_result_is_error(r), "expected an IO error");
            let e = lean_io_result_get_error(r);
            let info = error_info(e);
            lean_dec(r);
            info
        }
    }

    unsafe fn error_info(e: Obj) -> (&'static str, u32) {
        let r = unsafe { crate::exports::recording::read(e) };
        (r.constructor, r.code)
    }

    /// Waits for `promise` (consumed) and returns the `Except` it was resolved with.
    unsafe fn await_promise(promise: Obj) -> Obj {
        unsafe {
            let t = crate::task::promise_result_task(promise);
            lean_inc(t);
            lean_dec(promise);
            let opt = crate::task::task_get_own(t);
            assert_eq!(lean_obj_tag(opt), 1, "promise dropped without resolution");
            let v = lean_ctor_get(opt, 0);
            lean_inc(v);
            lean_dec(opt);
            v
        }
    }

    unsafe fn expect_ok(except: Obj) -> Obj {
        unsafe {
            assert_eq!(
                lean_obj_tag(except),
                1,
                "expected Except.ok, got error {:?}",
                error_info(lean_ctor_get(except, 0))
            );
            let v = lean_ctor_get(except, 0);
            lean_inc(v);
            lean_dec(except);
            v
        }
    }

    unsafe fn expect_err(except: Obj) -> (&'static str, u32) {
        unsafe {
            assert_eq!(lean_obj_tag(except), 0, "expected Except.error");
            let info = error_info(lean_ctor_get(except, 0));
            lean_dec(except);
            info
        }
    }

    unsafe fn bytes(s: &[u8]) -> Obj {
        unsafe {
            let ba = lean_alloc_sarray(1, s.len(), s.len());
            std::ptr::copy_nonoverlapping(s.as_ptr(), lean_sarray_cptr(ba), s.len());
            ba
        }
    }

    unsafe fn array(items: &[Obj]) -> Obj {
        unsafe {
            let a = lean_alloc_array(items.len(), items.len());
            for (i, x) in items.iter().enumerate() {
                lean_array_set_core(a, i, *x);
            }
            a
        }
    }

    unsafe fn contents(ba: Obj) -> Vec<u8> {
        unsafe { std::slice::from_raw_parts(lean_sarray_cptr(ba), lean_sarray_size(ba)).to_vec() }
    }

    fn loopback(port: u16) -> Obj {
        unsafe { addr::lean_of_socket_addr(&SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)) }
    }

    unsafe fn port_of(sa: Obj) -> u16 {
        unsafe { addr::socket_addr_of_lean(sa).port() }
    }

    fn errno(code: i32) -> u32 {
        (-code) as u32
    }

    #[test]
    fn tcp_echo_over_loopback() {
        setup();
        unsafe {
            let server = ok(lean_uv_tcp_new());
            assert_eq!(io_err(lean_uv_tcp_getsockname(server)), (INVALID_ARGUMENT, errno(errno::UV_EBADF)));
            let any = loopback(0);
            ok(lean_uv_tcp_bind(server, any));
            lean_dec(any);
            let addr = ok(lean_uv_tcp_getsockname(server));
            assert_ne!(port_of(addr), 0);
            ok(lean_uv_tcp_listen(server, 16));
            let none_yet = expect_ok(ok(lean_uv_tcp_try_accept(server)));
            assert!(none_yet.is_scalar(), "tryAccept without a pending connection is none");
            let acc = ok(lean_uv_tcp_accept(server));

            let client = ok(lean_uv_tcp_new());
            let data = array(&[bytes(&[1])]);
            assert_eq!(io_err(lean_uv_tcp_send(client, data)), (INVALID_ARGUMENT, errno(errno::UV_EBADF)));
            let conn = ok(lean_uv_tcp_connect(client, addr));
            lean_dec(expect_ok(await_promise(conn)));
            let peer = expect_ok(await_promise(acc));

            let sent = ok(lean_uv_tcp_send(client, array(&[bytes(b"hello "), bytes(b"world")])));
            lean_dec(expect_ok(await_promise(sent)));
            let r = ok(lean_uv_tcp_recv(peer, 64));
            assert_eq!(io_err(lean_uv_tcp_recv(peer, 64)), (OTHER_ERROR, errno(errno::UV_EALREADY)));
            let got = expect_ok(await_promise(r));
            assert_eq!(lean_obj_tag(got), 1);
            assert_eq!(contents(lean_ctor_get(got, 0)), b"hello world");
            lean_dec(got);

            let sh = ok(lean_uv_tcp_shutdown(client));
            lean_dec(expect_ok(await_promise(sh)));
            assert_eq!(io_err(lean_uv_tcp_shutdown(client)), (INVALID_ARGUMENT, errno(errno::UV_ENOTCONN)));
            let eof = ok(lean_uv_tcp_recv(peer, 64));
            let got = expect_ok(await_promise(eof));
            assert!(got.is_scalar(), "end of stream is reported as none");

            // A connection to a closed port is refused.
            let dead = ok(lean_uv_tcp_new());
            let any = loopback(0);
            ok(lean_uv_tcp_bind(dead, any));
            lean_dec(any);
            let dead_addr = ok(lean_uv_tcp_getsockname(dead));
            // Releasing the only reference closes the socket, as in the Lean program, where
            // `dead` is dead after `getSockName`.
            lean_dec(dead);
            let c2 = ok(lean_uv_tcp_new());
            let cp = ok(lean_uv_tcp_connect(c2, dead_addr));
            assert_eq!(expect_err(await_promise(cp)), (NO_SUCH_THING, errno(errno::UV_ECONNREFUSED)));

            for o in [server, addr, client, peer, dead_addr, c2] {
                lean_dec(o);
            }
        }
    }

    #[test]
    fn udp_datagrams_over_loopback() {
        setup();
        unsafe {
            let u1 = ok(lean_uv_udp_new());
            let any = loopback(0);
            ok(lean_uv_udp_bind(u1, any));
            lean_dec(any);
            let a1 = ok(lean_uv_udp_getsockname(u1));
            let u2 = ok(lean_uv_udp_new());
            assert_eq!(
                io_err(lean_uv_udp_send(u2, array(&[bytes(&[1])]), lean_box(0))),
                (INVALID_ARGUMENT, errno(errno::UV_EDESTADDRREQ))
            );
            let rp = ok(lean_uv_udp_recv(u1, 100));
            lean_inc(a1);
            let to = some(a1);
            let sp = ok(lean_uv_udp_send(u2, array(&[bytes(b"ping")]), to));
            lean_dec(to);
            lean_dec(expect_ok(await_promise(sp)));
            let got = expect_ok(await_promise(rp));
            assert_eq!(contents(lean_ctor_get(got, 0)), b"ping");
            assert_eq!(lean_obj_tag(lean_ctor_get(got, 1)), 1, "the sender address is reported");
            lean_dec(got);
            assert_eq!(io_err(lean_uv_udp_set_ttl(u1, 0)), (INVALID_ARGUMENT, errno(errno::UV_EINVAL)));
            assert_eq!(io_err(lean_uv_udp_getpeername(u1)), (INVALID_ARGUMENT, errno(errno::UV_ENOTCONN)));
            for o in [u1, a1, u2] {
                lean_dec(o);
            }
        }
    }

    #[test]
    fn dns_resolves_localhost_and_rejects_unknown_names() {
        setup();
        unsafe {
            let host = lean_mk_string("localhost");
            let service = lean_mk_string("");
            let p = ok(lean_uv_dns_get_info(host, service, 1));
            let arr = expect_ok(await_promise(p));
            let found: Vec<std::net::IpAddr> =
                (0..lean_array_size(arr)).map(|i| addr::ip_of_lean(lean_array_get_core(arr, i))).collect();
            assert!(found.contains(&std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)), "{found:?}");
            lean_dec(arr);
            let bad = lean_mk_string("no-such-host.invalid");
            let p = ok(lean_uv_dns_get_info(bad, service, 0));
            assert_eq!(expect_err(await_promise(p)), (OTHER_ERROR, errno(errno::UV_EAI_NONAME)));
            for o in [host, service, bad] {
                lean_dec(o);
            }
        }
    }

    #[test]
    fn timers_fire_once_and_repeat() {
        setup();
        unsafe {
            let t = ok(lean_uv_timer_mk(50, 0));
            let start = std::time::Instant::now();
            let p = ok(lean_uv_timer_next(t));
            let v = crate::task::promise_result_task(p);
            lean_inc(v);
            lean_dec(p);
            lean_dec(crate::task::task_get_own(v));
            assert!(start.elapsed() >= std::time::Duration::from_millis(45));
            let p2 = ok(lean_uv_timer_next(t));
            assert!(promise_is_resolved(p2), "next on a fired one-shot timer is already resolved");
            lean_dec(p2);
            lean_dec(t);

            let rt = ok(lean_uv_timer_mk(20, 1));
            for _ in 0..2 {
                let p = ok(lean_uv_timer_next(rt));
                let task = crate::task::promise_result_task(p);
                lean_inc(task);
                lean_dec(p);
                let r = crate::task::task_get_own(task);
                assert_eq!(lean_obj_tag(r), 1, "a tick resolves the promise");
                lean_dec(r);
            }
            ok(lean_uv_timer_stop(rt));
            lean_dec(rt);
        }
    }

    #[test]
    fn address_strings() {
        unsafe {
            let s = lean_mk_string("1.2.3");
            assert!(lean_uv_pton_v4(s).is_scalar());
            lean_dec(s);
            let s = lean_mk_string("10.0.0.255");
            let a = lean_uv_pton_v4(s);
            let text = lean_uv_ntop_v4(lean_ctor_get(a, 0));
            assert_eq!(lean_string_str(text), "10.0.0.255");
            for o in [s, a, text] {
                lean_dec(o);
            }
        }
    }

    #[test]
    fn system_queries_succeed() {
        unsafe {
            let cwd = ok(lean_uv_cwd());
            assert_eq!(std::path::Path::new(lean_string_str(cwd)), std::env::current_dir().unwrap());
            lean_dec(cwd);
            let pid = ok(lean_uv_os_getpid());
            assert_eq!(lean_unbox_uint64(pid), std::process::id() as u64);
            lean_dec(pid);
            let cpus = ok(lean_uv_cpu_info());
            assert!(lean_array_size(cpus) > 0);
            lean_dec(cpus);
            let u = ok(lean_uv_os_uname());
            assert!(!lean_string_str(lean_ctor_get(u, 0)).is_empty());
            lean_dec(u);
            let total = ok(lean_uv_get_total_memory());
            assert!(lean_unbox_uint64(total) > 0);
            lean_dec(total);
            let h1 = ok(lean_uv_hrtime());
            let h2 = ok(lean_uv_hrtime());
            assert!(lean_unbox_uint64(h2) >= lean_unbox_uint64(h1));
            lean_dec(h1);
            lean_dec(h2);
        }
    }

    #[cfg(unix)]
    #[test]
    fn signals_are_delivered_to_watchers() {
        setup();
        unsafe {
            // Portable number 10 is `SIGUSR1`.
            let sig = ok(lean_uv_signal_mk(10, 0));
            let p = ok(lean_uv_signal_next(sig));
            libc::raise(libc::SIGUSR1);
            let task = crate::task::promise_result_task(p);
            lean_inc(task);
            lean_dec(p);
            let r = crate::task::task_get_own(task);
            assert_eq!(lean_obj_tag(r), 1);
            assert_eq!(lean_unbox(lean_ctor_get(r, 0)), libc::SIGUSR1 as usize);
            lean_dec(r);
            lean_dec(sig);
        }
    }
}
