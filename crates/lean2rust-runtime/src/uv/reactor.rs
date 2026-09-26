//! The event loop behind `Std.Internal.UV`, replacing libuv's.
//!
//! As in Lean's `runtime/uv/event_loop.cpp`, a single loop serves the whole process: one thread
//! waits for socket readiness and timer deadlines and runs completion callbacks, and every
//! operation on a loop-managed handle — from any thread — holds the loop's (recursive) lock.
//! Callbacks run with the lock held, exactly like libuv callbacks run inside `uv_run` while
//! Lean's loop mutex is held.

use super::sys::{self, PollEntry, RawSock};
use crate::object::lean_internal_panic;
use std::cell::UnsafeCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// A recursive mutex whose lock and unlock may be separated across calls.
pub struct LoopLock {
    state: Mutex<(Option<ThreadId>, usize)>,
    released: Condvar,
}

pub struct LoopGuard<'a> {
    lock: &'a LoopLock,
}

impl LoopLock {
    const fn new() -> Self {
        LoopLock { state: Mutex::new((None, 0)), released: Condvar::new() }
    }

    pub fn lock(&self) -> LoopGuard<'_> {
        let me = std::thread::current().id();
        let mut g = self.state.lock().unwrap_or_else(|_| lean_internal_panic("the event loop lock is poisoned"));
        loop {
            match g.0 {
                None => {
                    *g = (Some(me), 1);
                    break;
                }
                Some(owner) if owner == me => {
                    g.1 += 1;
                    break;
                }
                Some(_) => {
                    g = self
                        .released
                        .wait(g)
                        .unwrap_or_else(|_| lean_internal_panic("the event loop lock is poisoned"));
                }
            }
        }
        LoopGuard { lock: self }
    }
}

impl Drop for LoopGuard<'_> {
    fn drop(&mut self) {
        let mut g = self.lock.state.lock().unwrap_or_else(|_| lean_internal_panic("the event loop lock is poisoned"));
        g.1 -= 1;
        if g.1 == 0 {
            g.0 = None;
            drop(g);
            self.lock.released.notify_one();
        }
    }
}

/// A socket (or other pollable resource) managed by the loop.
pub trait IoSource {
    /// The socket to poll and whether readability / writability are of interest.
    fn interest(&self) -> Option<(RawSock, bool, bool)>;
    /// Handles readiness. Called on the loop thread with the loop lock held.
    unsafe fn on_ready(&self, readable: bool, writable: bool);
}

/// A timer target managed by the loop.
pub trait TimerSink {
    /// Called on the loop thread with the loop lock held when a deadline scheduled with `token`
    /// expires.
    unsafe fn fire(&self, token: u64);
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TimerEntry {
    deadline: Instant,
    seq: u64,
    id: u64,
    token: u64,
}

type Callback = Box<dyn FnOnce() + Send>;

struct LoopState {
    sources: BTreeMap<u64, *const dyn IoSource>,
    timer_sinks: BTreeMap<u64, *const dyn TimerSink>,
    timers: BinaryHeap<Reverse<TimerEntry>>,
    pending: VecDeque<Callback>,
    next_id: u64,
    next_seq: u64,
}

pub struct Reactor {
    lock: LoopLock,
    state: UnsafeCell<LoopState>,
    wake_tx: std::net::UdpSocket,
    wake_rx: std::net::UdpSocket,
}

// The loop state is only accessed while holding `lock`.
unsafe impl Sync for Reactor {}
unsafe impl Send for Reactor {}

static REACTOR: OnceLock<&'static Reactor> = OnceLock::new();

/// The process-wide event loop, started on first use.
pub fn reactor() -> &'static Reactor {
    REACTOR.get_or_init(|| {
        sys::init();
        let wake_rx = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot create the event loop wake-up socket: {e}")));
        let wake_tx = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot create the event loop wake-up socket: {e}")));
        let rx_addr = wake_rx
            .local_addr()
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot address the event loop wake-up socket: {e}")));
        wake_tx
            .connect(rx_addr)
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot connect the event loop wake-up socket: {e}")));
        wake_rx
            .set_nonblocking(true)
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot configure the event loop wake-up socket: {e}")));
        wake_tx
            .set_nonblocking(true)
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot configure the event loop wake-up socket: {e}")));
        let r: &'static Reactor = Box::leak(Box::new(Reactor {
            lock: LoopLock::new(),
            state: UnsafeCell::new(LoopState {
                sources: BTreeMap::new(),
                timer_sinks: BTreeMap::new(),
                timers: BinaryHeap::new(),
                pending: VecDeque::new(),
                next_id: 1,
                next_seq: 0,
            }),
            wake_tx,
            wake_rx,
        }));
        std::thread::Builder::new()
            .name("lean-event-loop".into())
            .spawn(move || r.run())
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot start the event loop thread: {e}")));
        r
    })
}

#[cfg(unix)]
fn raw_of(s: &std::net::UdpSocket) -> RawSock {
    use std::os::fd::AsRawFd;
    s.as_raw_fd()
}

#[cfg(windows)]
fn raw_of(s: &std::net::UdpSocket) -> RawSock {
    use std::os::windows::io::AsRawSocket;
    s.as_raw_socket() as RawSock
}

impl Reactor {
    /// Acquires the loop lock (recursively).
    pub fn lock(&self) -> LoopGuard<'_> {
        self.lock.lock()
    }

    /// The loop state. The caller must hold the loop lock and must not keep the reference
    /// across calls that may re-enter the loop.
    #[allow(clippy::mut_from_ref)]
    fn state(&self, _guard: &LoopGuard<'_>) -> &mut LoopState {
        unsafe { &mut *self.state.get() }
    }

    /// Interrupts the loop's wait so that it re-examines interests, timers, and callbacks.
    pub fn wake(&self) {
        // A full socket buffer already guarantees a pending wake-up.
        let _ = self.wake_tx.send(&[0]);
    }

    pub fn register_source(&self, guard: &LoopGuard<'_>, source: *const dyn IoSource) -> u64 {
        let st = self.state(guard);
        let id = st.next_id;
        st.next_id += 1;
        st.sources.insert(id, source);
        id
    }

    pub fn unregister_source(&self, guard: &LoopGuard<'_>, id: u64) {
        self.state(guard).sources.remove(&id);
    }

    pub fn register_timer(&self, guard: &LoopGuard<'_>, sink: *const dyn TimerSink) -> u64 {
        let st = self.state(guard);
        let id = st.next_id;
        st.next_id += 1;
        st.timer_sinks.insert(id, sink);
        id
    }

    pub fn unregister_timer(&self, guard: &LoopGuard<'_>, id: u64) {
        self.state(guard).timer_sinks.remove(&id);
    }

    /// Schedules `TimerSink::fire(token)` of timer `id` after `delay`.
    pub fn schedule(&self, guard: &LoopGuard<'_>, id: u64, token: u64, delay: Duration) {
        let st = self.state(guard);
        let seq = st.next_seq;
        st.next_seq += 1;
        st.timers.push(Reverse(TimerEntry { deadline: Instant::now() + delay, seq, id, token }));
        self.wake();
    }

    /// Runs `f` on the loop thread, with the loop lock held, during the next iteration.
    pub fn defer(&self, f: Callback) {
        let guard = self.lock();
        self.state(&guard).pending.push_back(f);
        drop(guard);
        self.wake();
    }

    fn run(&'static self) {
        loop {
            let mut entries = vec![PollEntry {
                sock: raw_of(&self.wake_rx),
                read: true,
                write: false,
                readable: false,
                writable: false,
            }];
            let mut ids = vec![0u64];
            let timeout_ms;
            {
                let guard = self.lock();
                let st = self.state(&guard);
                let sources: Vec<(u64, *const dyn IoSource)> = st.sources.iter().map(|(k, v)| (*k, *v)).collect();
                timeout_ms = if !st.pending.is_empty() {
                    0
                } else {
                    match st.timers.peek() {
                        Some(Reverse(t)) => {
                            let now = Instant::now();
                            if t.deadline <= now {
                                0
                            } else {
                                let d = t.deadline - now;
                                // Round up so that the deadline has passed when the wait ends.
                                let ms = d.as_millis() + u128::from(!d.subsec_nanos().is_multiple_of(1_000_000));
                                ms.min(i32::MAX as u128) as i32
                            }
                        }
                        None => -1,
                    }
                };
                for (id, src) in sources {
                    if let Some((sock, read, write)) = unsafe { (*src).interest() }
                        && (read || write)
                    {
                        entries.push(PollEntry { sock, read, write, readable: false, writable: false });
                        ids.push(id);
                    }
                }
            }
            if let Err(e) = sys::poll(&mut entries, timeout_ms) {
                lean_internal_panic(&format!("event loop poll failed: {e}"));
            }
            let guard = self.lock();
            if entries[0].readable {
                let mut buf = [0u8; 64];
                while self.wake_rx.recv(&mut buf).is_ok() {}
            }
            // Deferred callbacks.
            loop {
                let next = self.state(&guard).pending.pop_front();
                match next {
                    Some(f) => f(),
                    None => break,
                }
            }
            // Expired timers.
            let now = Instant::now();
            loop {
                let due = {
                    let st = self.state(&guard);
                    match st.timers.peek() {
                        Some(Reverse(t)) if t.deadline <= now => {
                            let Reverse(t) = st.timers.pop().expect("peeked timer");
                            st.timer_sinks.get(&t.id).map(|s| (*s, t.token))
                        }
                        _ => break,
                    }
                };
                if let Some((sink, token)) = due {
                    unsafe { (*sink).fire(token) };
                }
            }
            // Ready sources, if still registered.
            for (entry, id) in entries.iter().zip(&ids).skip(1) {
                if !(entry.readable || entry.writable) {
                    continue;
                }
                let src = self.state(&guard).sources.get(id).copied();
                if let Some(src) = src {
                    unsafe { (*src).on_ready(entry.readable, entry.writable) };
                }
            }
            drop(guard);
        }
    }
}
