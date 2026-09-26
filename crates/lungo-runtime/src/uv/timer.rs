//! `Std.Internal.UV.Timer`, ported from `runtime/uv/timer.cpp`.
//!
//! A timer handle is an external object. While a timer is running, the event loop holds a
//! reference to it. One-shot timers resolve their promise once and finish; repeating timers
//! resolve the current promise on each tick if nobody has observed its resolution yet.

use super::reactor::{TimerSink, reactor};
use super::*;
use std::cell::UnsafeCell;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    Running,
    Finished,
}

struct Inner {
    promise: Obj,
    timeout: u64,
    repeating: bool,
    state: State,
    /// Identifies the current schedule; earlier schedules are ignored when they expire.
    token: u64,
    /// Whether a schedule is active (`uv_timer_start` without a matching `uv_timer_stop`).
    active: bool,
    /// The repeat interval of the active schedule (`0`: one-shot).
    repeat: u64,
}

pub(crate) struct Timer {
    obj: Obj,
    id: u64,
    inner: UnsafeCell<Inner>,
}

unsafe fn finalize(p: *mut ()) {
    unsafe {
        let timer = Box::from_raw(p as *mut Timer);
        let r = reactor();
        let guard = r.lock();
        r.unregister_timer(&guard, timer.id);
        let promise = (*timer.inner.get()).promise;
        drop(guard);
        if !promise.is_null() {
            lean_dec(promise);
        }
    }
}

unsafe fn for_each(p: *mut (), f: &mut dyn FnMut(Obj)) {
    unsafe {
        let promise = (*(*(p as *mut Timer)).inner.get()).promise;
        if !promise.is_null() {
            f(promise);
        }
    }
}

static CLASS: ExternalClass = ExternalClass { finalize, for_each };

unsafe fn get<'a>(o: Obj) -> &'a Timer {
    unsafe { external_data::<Timer>(o, &CLASS, "timer") }
}

impl Timer {
    #[allow(clippy::mut_from_ref)]
    unsafe fn inner(&self) -> &mut Inner {
        unsafe { &mut *self.inner.get() }
    }

    /// `uv_timer_start(timeout, repeat)`.
    unsafe fn start(&self, guard: &super::reactor::LoopGuard<'_>, timeout: u64, repeat: u64) {
        unsafe {
            let inner = self.inner();
            inner.token += 1;
            inner.active = true;
            inner.repeat = repeat;
            reactor().schedule(guard, self.id, inner.token, Duration::from_millis(timeout));
        }
    }

    /// `uv_timer_stop`.
    unsafe fn stop(&self) {
        unsafe {
            let inner = self.inner();
            inner.token += 1;
            inner.active = false;
        }
    }
}

impl TimerSink for Timer {
    unsafe fn fire(&self, token: u64) {
        unsafe {
            let (repeat, obj) = {
                let inner = self.inner();
                if !inner.active || inner.token != token {
                    return;
                }
                (inner.repeat, self.obj)
            };
            // As libuv's `uv__run_timers`: the schedule is renewed before the callback runs.
            let guard = reactor().lock();
            if repeat > 0 {
                self.start(&guard, repeat, repeat);
            } else {
                self.inner().active = false;
            }
            handle_timer_event(self, obj);
            drop(guard);
        }
    }
}

unsafe fn handle_timer_event(timer: &Timer, obj: Obj) {
    unsafe {
        let (repeating, promise, state) = {
            let inner = timer.inner();
            (inner.repeating, inner.promise, inner.state)
        };
        if state != State::Running {
            lean_internal_panic("timer event for a timer that is not running");
        }
        if repeating {
            if !promise.is_null() && !promise_is_resolved(promise) {
                crate::task::promise_resolve(lean_box(0), promise);
            }
        } else {
            if !promise.is_null() {
                if promise_is_resolved(promise) {
                    lean_internal_panic("one-shot timer promise resolved twice");
                }
                crate::task::promise_resolve(lean_box(0), promise);
            }
            timer.stop();
            timer.inner().state = State::Finished;
            // The loop does not need to keep the timer alive anymore.
            lean_dec(obj);
        }
    }
}

pub(crate) unsafe fn mk(timeout: u64, repeating: bool) -> Obj {
    unsafe {
        let r = reactor();
        let guard = r.lock();
        let timer = Box::into_raw(Box::new(Timer {
            obj: Obj::null(),
            id: 0,
            inner: UnsafeCell::new(Inner {
                promise: Obj::null(),
                timeout,
                repeating,
                state: State::Initial,
                token: 0,
                active: false,
                repeat: 0,
            }),
        }));
        let obj = lean_alloc_external(&CLASS, timer as *mut ());
        lean_mark_mt(obj);
        (*timer).obj = obj;
        (*timer).id = r.register_timer(&guard, timer as *const Timer as *const dyn TimerSink);
        drop(guard);
        lean_io_result_mk_ok(obj)
    }
}

pub(crate) unsafe fn next(obj: Obj) -> Obj {
    unsafe {
        let timer = get(obj);
        let guard = reactor().lock();
        let inner = timer.inner();
        let setup = |inner: &mut Inner| -> Obj {
            let promise = new_promise();
            inner.promise = promise;
            inner.state = State::Running;
            // The event loop keeps the timer alive while it runs.
            lean_inc(obj);
            lean_inc(promise);
            let (timeout, repeat) = if inner.repeating { (0, inner.timeout) } else { (inner.timeout, 0) };
            timer.start(&guard, timeout, repeat);
            lean_io_result_mk_ok(promise)
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
                    if !inner.promise.is_null() {
                        lean_inc(inner.promise);
                        lean_io_result_mk_ok(inner.promise)
                    } else {
                        lean_io_result_mk_ok(new_promise())
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

pub(crate) unsafe fn reset(obj: Obj) -> Obj {
    unsafe {
        let timer = get(obj);
        let guard = reactor().lock();
        let inner = timer.inner();
        if inner.state == State::Running {
            let (timeout, repeating) = (inner.timeout, inner.repeating);
            timer.stop();
            timer.start(&guard, timeout, if repeating { timeout } else { 0 });
        }
        lean_io_result_mk_ok(lean_box(0))
    }
}

pub(crate) unsafe fn stop(obj: Obj) -> Obj {
    unsafe {
        let timer = get(obj);
        let guard = reactor().lock();
        let inner = timer.inner();
        let promise = std::mem::replace(&mut inner.promise, Obj::null());
        let was_running = inner.state == State::Running;
        if was_running {
            timer.stop();
            inner.state = State::Finished;
        }
        drop(guard);
        if !promise.is_null() {
            lean_dec(promise);
        }
        if was_running {
            // The loop does not need to keep the timer alive anymore.
            lean_dec(obj);
        }
        lean_io_result_mk_ok(lean_box(0))
    }
}

pub(crate) unsafe fn cancel(obj: Obj) -> Obj {
    unsafe {
        let timer = get(obj);
        let guard = reactor().lock();
        let inner = timer.inner();
        let mut release_promise = Obj::null();
        let mut release_timer = false;
        if inner.state == State::Running && !inner.promise.is_null() {
            release_promise = std::mem::replace(&mut inner.promise, Obj::null());
            if !inner.repeating {
                timer.stop();
                inner.state = State::Initial;
                release_timer = true;
            }
        }
        drop(guard);
        if !release_promise.is_null() {
            lean_dec(release_promise);
        }
        if release_timer {
            lean_dec(obj);
        }
        lean_io_result_mk_ok(lean_box(0))
    }
}
