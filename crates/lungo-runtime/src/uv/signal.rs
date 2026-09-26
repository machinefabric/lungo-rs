//! `Std.Internal.UV.Signal`, ported from `runtime/uv/signal.cpp` together with libuv's signal
//! handling: a process-wide handler forwards delivered signals to the event loop, which
//! dispatches them to every running signal handle watching that signal. The default disposition
//! is restored when the last handle for a signal stops, as libuv does.

use super::errno::*;
use super::reactor::{LoopGuard, reactor};
use super::*;
use std::cell::UnsafeCell;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    Running,
    Finished,
}

struct Inner {
    promise: Obj,
    signum: i32,
    repeating: bool,
    state: State,
    /// Whether the handle is registered with the dispatcher (`uv_signal_start`).
    active: bool,
}

pub(crate) struct Signal {
    obj: Obj,
    inner: UnsafeCell<Inner>,
}

/// Running signal handles by signal number. Accessed with the loop lock held.
struct Watchers(UnsafeCell<BTreeMap<i32, Vec<*const Signal>>>);
unsafe impl Sync for Watchers {}

static WATCHERS: Watchers = Watchers(UnsafeCell::new(BTreeMap::new()));

#[allow(clippy::mut_from_ref)]
fn watchers<'a>(_guard: &'a LoopGuard<'_>) -> &'a mut BTreeMap<i32, Vec<*const Signal>> {
    unsafe { &mut *WATCHERS.0.get() }
}

unsafe fn finalize(p: *mut ()) {
    unsafe {
        let guard = reactor().lock();
        let signal = Box::from_raw(p as *mut Signal);
        if (*signal.inner.get()).active {
            unregister(&guard, &signal);
        }
        let promise = (*signal.inner.get()).promise;
        drop(guard);
        if !promise.is_null() {
            lean_dec(promise);
        }
    }
}

unsafe fn for_each(p: *mut (), f: &mut dyn FnMut(Obj)) {
    unsafe {
        let promise = (*(*(p as *mut Signal)).inner.get()).promise;
        if !promise.is_null() {
            f(promise);
        }
    }
}

static CLASS: ExternalClass = ExternalClass { finalize, for_each };

unsafe fn get<'a>(o: Obj) -> &'a Signal {
    unsafe { external_data::<Signal>(o, &CLASS, "signal") }
}

impl Signal {
    #[allow(clippy::mut_from_ref)]
    unsafe fn inner(&self) -> &mut Inner {
        unsafe { &mut *self.inner.get() }
    }
}

/// `uv_signal_start`.
unsafe fn register(guard: &LoopGuard<'_>, signal: &Signal) -> Result<(), i32> {
    unsafe {
        let signum = signal.inner().signum;
        if signum <= 0 {
            return Err(UV_EINVAL);
        }
        let w = watchers(guard);
        let list = w.entry(signum).or_default();
        if list.is_empty()
            && let Err(code) = platform::install(signum)
        {
            w.remove(&signum);
            return Err(code);
        }
        w.get_mut(&signum).expect("entry").push(signal as *const Signal);
        signal.inner().active = true;
        Ok(())
    }
}

/// `uv_signal_stop`.
unsafe fn unregister(guard: &LoopGuard<'_>, signal: &Signal) {
    unsafe {
        let signum = signal.inner().signum;
        signal.inner().active = false;
        let w = watchers(guard);
        if let Some(list) = w.get_mut(&signum) {
            list.retain(|p| !std::ptr::eq(*p, signal));
            if list.is_empty() {
                w.remove(&signum);
                platform::uninstall(signum);
            }
        }
    }
}

/// Delivers `signum` to every handle watching it. Runs on the loop thread.
fn dispatch(signum: i32) {
    let guard = reactor().lock();
    let targets: Vec<*const Signal> = watchers(&guard).get(&signum).cloned().unwrap_or_default();
    for t in targets {
        // A handle may have been stopped by an earlier delivery in this batch.
        let still = watchers(&guard).get(&signum).is_some_and(|l| l.iter().any(|p| std::ptr::eq(*p, t)));
        if still {
            unsafe { handle_signal_event(&guard, &*t, signum) };
        }
    }
}

unsafe fn handle_signal_event(guard: &LoopGuard<'_>, signal: &Signal, signum: i32) {
    unsafe {
        let obj = signal.obj;
        let (repeating, promise, state) = {
            let inner = signal.inner();
            (inner.repeating, inner.promise, inner.state)
        };
        if state != State::Running {
            lean_internal_panic("signal event for a signal handle that is not running");
        }
        if repeating {
            if !promise.is_null() && !promise_is_resolved(promise) {
                crate::task::promise_resolve(lean_box(signum as usize), promise);
            }
        } else {
            if !promise.is_null() {
                crate::task::promise_resolve(lean_box(signum as usize), promise);
            }
            unregister(guard, signal);
            signal.inner().state = State::Finished;
            lean_dec(obj);
        }
    }
}

/// Maps the portable signal numbers of `Std.Internal.IO.Async.Signal` to the platform's.
fn platform_signum(n: i32) -> i32 {
    #[cfg(unix)]
    {
        match n {
            1 => libc::SIGHUP,
            2 => libc::SIGINT,
            3 => libc::SIGQUIT,
            6 => libc::SIGABRT,
            15 => libc::SIGTERM,
            28 => libc::SIGWINCH,
            5 => libc::SIGTRAP,
            10 => libc::SIGUSR1,
            12 => libc::SIGUSR2,
            14 => libc::SIGALRM,
            17 => libc::SIGCHLD,
            18 => libc::SIGCONT,
            20 => libc::SIGTSTP,
            21 => libc::SIGTTIN,
            22 => libc::SIGTTOU,
            23 => libc::SIGURG,
            24 => libc::SIGXCPU,
            25 => libc::SIGXFSZ,
            26 => libc::SIGVTALRM,
            27 => libc::SIGPROF,
            29 => libc::SIGIO,
            31 => libc::SIGSYS,
            _ => 0,
        }
    }
    #[cfg(windows)]
    {
        // libuv's `uv/win.h` numbering; `SIGABRT` is the C runtime's.
        match n {
            1 => 1,
            2 => 2,
            3 => 3,
            6 => 22,
            15 => 15,
            28 => 28,
            _ => 0,
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::super::errno::uv_code_of_os_error;
    use super::super::reactor::{IoSource, reactor};
    use super::super::sys::RawSock;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicI32, Ordering};

    static WRITE_FD: AtomicI32 = AtomicI32::new(-1);

    struct Pipe {
        read_fd: i32,
    }

    impl IoSource for Pipe {
        fn interest(&self) -> Option<(RawSock, bool, bool)> {
            Some((self.read_fd, true, false))
        }

        unsafe fn on_ready(&self, readable: bool, _writable: bool) {
            if !readable {
                return;
            }
            let mut buf = [0u8; 128];
            loop {
                let n = unsafe { libc::read(self.read_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n <= 0 {
                    break;
                }
                for &s in &buf[..n as usize] {
                    super::dispatch(s as i32);
                }
            }
        }
    }

    fn pipe() -> &'static Pipe {
        static PIPE: OnceLock<&'static Pipe> = OnceLock::new();
        PIPE.get_or_init(|| {
            let mut fds = [0i32; 2];
            unsafe {
                if libc::pipe(fds.as_mut_ptr()) != 0 {
                    crate::object::lean_internal_panic(&format!(
                        "cannot create the signal pipe: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                for fd in fds {
                    let fl = libc::fcntl(fd, libc::F_GETFL);
                    libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
                    let fdfl = libc::fcntl(fd, libc::F_GETFD);
                    libc::fcntl(fd, libc::F_SETFD, fdfl | libc::FD_CLOEXEC);
                }
            }
            WRITE_FD.store(fds[1], Ordering::Release);
            let p: &'static Pipe = Box::leak(Box::new(Pipe { read_fd: fds[0] }));
            let r = reactor();
            let guard = r.lock();
            r.register_source(&guard, p as *const Pipe as *const dyn IoSource);
            drop(guard);
            r.wake();
            p
        })
    }

    extern "C" fn handler(signum: libc::c_int) {
        unsafe {
            let saved = *errno_location();
            let fd = WRITE_FD.load(Ordering::Acquire);
            let b = signum as u8;
            libc::write(fd, &b as *const u8 as *const libc::c_void, 1);
            *errno_location() = saved;
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe fn errno_location() -> *mut libc::c_int {
        unsafe { libc::__errno_location() }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    unsafe fn errno_location() -> *mut libc::c_int {
        unsafe { libc::__error() }
    }

    pub fn install(signum: i32) -> Result<(), i32> {
        pipe();
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            if libc::sigfillset(&mut sa.sa_mask) != 0 {
                crate::object::lean_internal_panic("sigfillset failed");
            }
            sa.sa_sigaction = handler as *const () as libc::sighandler_t;
            sa.sa_flags = libc::SA_RESTART;
            if libc::sigaction(signum, &sa, std::ptr::null_mut()) != 0 {
                return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
            }
        }
        Ok(())
    }

    pub fn uninstall(signum: i32) {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            libc::sigemptyset(&mut sa.sa_mask);
            sa.sa_sigaction = libc::SIG_DFL;
            if libc::sigaction(signum, &sa, std::ptr::null_mut()) != 0 {
                crate::object::lean_internal_panic(&format!(
                    "cannot restore the default action of signal {signum}: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::super::reactor::reactor;
    use std::sync::Once;
    use windows_sys::Win32::Foundation::{BOOL, FALSE, TRUE};
    use windows_sys::Win32::System::Console::*;

    const SIGHUP: i32 = 1;
    const SIGINT: i32 = 2;
    const SIGBREAK: i32 = 21;

    unsafe extern "system" fn console_handler(ctrl: u32) -> BOOL {
        let signum = match ctrl {
            CTRL_C_EVENT => SIGINT,
            CTRL_BREAK_EVENT => SIGBREAK,
            CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => SIGHUP,
            _ => return FALSE,
        };
        let guard = reactor().lock();
        let watched = super::watchers(&guard).contains_key(&signum);
        drop(guard);
        if !watched {
            return FALSE;
        }
        reactor().defer(Box::new(move || super::dispatch(signum)));
        TRUE
    }

    pub fn install(_signum: i32) -> Result<(), i32> {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| unsafe {
            if SetConsoleCtrlHandler(Some(console_handler), TRUE) == 0 {
                crate::object::lean_internal_panic("cannot install the console control handler");
            }
        });
        Ok(())
    }

    pub fn uninstall(_signum: i32) {}
}

pub(crate) unsafe fn mk(signum: u32, repeating: bool) -> Obj {
    unsafe {
        let signum = platform_signum(signum as i32);
        let signal = Box::into_raw(Box::new(Signal {
            obj: Obj::null(),
            inner: UnsafeCell::new(Inner {
                promise: Obj::null(),
                signum,
                repeating,
                state: State::Initial,
                active: false,
            }),
        }));
        let obj = lean_alloc_external(&CLASS, signal as *mut ());
        lean_mark_mt(obj);
        (*signal).obj = obj;
        lean_io_result_mk_ok(obj)
    }
}

pub(crate) unsafe fn next(obj: Obj) -> Obj {
    unsafe {
        let signal = get(obj);
        let guard = reactor().lock();
        let inner = signal.inner();
        let setup = |inner: &mut Inner| -> Obj {
            let promise = new_promise();
            inner.promise = promise;
            inner.state = State::Running;
            lean_inc(obj);
            lean_inc(promise);
            match register(&guard, signal) {
                Ok(()) => lean_io_result_mk_ok(promise),
                Err(code) => {
                    lean_dec(obj);
                    lean_dec(promise);
                    uv_io_error(code)
                }
            }
        };
        if inner.repeating {
            match inner.state {
                State::Initial => setup(inner),
                State::Running => {
                    if inner.promise.is_null() || promise_is_resolved(inner.promise) {
                        if !inner.promise.is_null() {
                            lean_dec(inner.promise);
                        }
                        inner.promise = new_promise();
                    }
                    lean_inc(inner.promise);
                    lean_io_result_mk_ok(inner.promise)
                }
                State::Finished => {
                    if inner.promise.is_null() {
                        lean_io_result_mk_ok(new_promise())
                    } else {
                        lean_inc(inner.promise);
                        lean_io_result_mk_ok(inner.promise)
                    }
                }
            }
        } else if inner.state == State::Initial {
            setup(inner)
        } else if !inner.promise.is_null() {
            lean_inc(inner.promise);
            lean_io_result_mk_ok(inner.promise)
        } else {
            lean_io_result_mk_ok(new_promise())
        }
    }
}

pub(crate) unsafe fn stop(obj: Obj) -> Obj {
    unsafe {
        let signal = get(obj);
        let guard = reactor().lock();
        let inner = signal.inner();
        if inner.state != State::Running {
            return lean_io_result_mk_ok(lean_box(0));
        }
        if inner.active {
            unregister(&guard, signal);
        }
        let promise = std::mem::replace(&mut signal.inner().promise, Obj::null());
        signal.inner().state = State::Finished;
        drop(guard);
        if !promise.is_null() {
            lean_dec(promise);
        }
        lean_dec(obj);
        lean_io_result_mk_ok(lean_box(0))
    }
}

pub(crate) unsafe fn cancel(obj: Obj) -> Obj {
    unsafe {
        let signal = get(obj);
        let guard = reactor().lock();
        let inner = signal.inner();
        let mut release_promise = Obj::null();
        let mut release_signal = false;
        if inner.state == State::Running && !inner.promise.is_null() {
            release_promise = std::mem::replace(&mut inner.promise, Obj::null());
            if !inner.repeating {
                if inner.active {
                    unregister(&guard, signal);
                }
                signal.inner().state = State::Initial;
                release_signal = true;
            }
        }
        drop(guard);
        if !release_promise.is_null() {
            lean_dec(release_promise);
        }
        if release_signal {
            lean_dec(obj);
        }
        lean_io_result_mk_ok(lean_box(0))
    }
}
