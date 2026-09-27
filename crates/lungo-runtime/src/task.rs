//! Thunks, tasks, promises, and the synchronization primitives of `Std.Sync`, ported from the
//! task manager in Lean's `runtime/object.cpp`, `runtime/io.cpp`, and `runtime/mutex.cpp`.
//!
//! Tasks run on a pool of worker threads, one queue per priority (`0..=LEAN_MAX_PRIO`);
//! priorities above `LEAN_MAX_PRIO` get a dedicated thread and `sync` continuations run inline
//! on the thread that resolves their dependency. A task is multi-threaded from its creation;
//! its closure and value are marked multi-threaded before they cross threads. The lifetime of a
//! task object follows the state machine documented in `lean.h` (queued, waiting, promised,
//! running, deactivated, finished). Without a task manager (before `init_task_manager`, or with
//! `LEAN_NUM_THREADS=0`), tasks are evaluated eagerly on the calling thread, as in Lean.

use crate::apply::{lean_apply_1, lean_apply_2};
use crate::object::*;
use std::cell::Cell;
use std::collections::VecDeque;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

/// See `Task.Priority.max`.
const LEAN_MAX_PRIO: u32 = 8;
/// Priority of `sync` continuations: run on the thread that finishes their dependency.
const LEAN_SYNC_PRIO: u32 = u32::MAX;
const LEAN_STACK_BUFFER_SPACE: usize = 128 * 1024;

// ---------------------------------------------------------------------------------------------
// Thunks
// ---------------------------------------------------------------------------------------------

/// Evaluates the thunk `t` (borrowed) if necessary and returns its value (borrowed).
pub unsafe fn lean_thunk_get(t: Obj) -> Obj {
    unsafe {
        let v = (*lean_thunk_ptr(t)).value.load(Ordering::Acquire);
        if !v.is_null() {
            return Obj::from_raw(v);
        }
        thunk_get_core(t)
    }
}

unsafe fn thunk_get_core(t: Obj) -> Obj {
    unsafe {
        let th = lean_thunk_ptr(t);
        let c = (*th).closure.swap(ptr::null_mut(), Ordering::AcqRel);
        if !c.is_null() {
            // `lean_apply_1` consumes the closure; the thunk takes ownership of the result.
            let r = lean_apply_1(Obj::from_raw(c), lean_box(0));
            if r.is_null() {
                lean_internal_panic("thunk closure returned no value");
            }
            lean_mark_mt(r);
            (*th).value.store(r.ptr(), Ordering::Release);
            r
        } else {
            // Another thread is evaluating the closure; wait for it to publish the value.
            loop {
                let v = (*th).value.load(Ordering::Acquire);
                if !v.is_null() {
                    return Obj::from_raw(v);
                }
                std::thread::yield_now();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Task objects
// ---------------------------------------------------------------------------------------------

/// Layout of a task object: the object header followed by the value (null until finished)
/// and the execution data (null once finished).
#[repr(C)]
struct TaskObject {
    header: u64,
    value: AtomicPtr<Object>,
    imp: AtomicPtr<TaskImp>,
}

/// Execution data of an unfinished task. Fields other than `canceled` are only accessed while
/// holding the task manager's mutex, except where the C runtime accesses them without it
/// (the running task's own `closure`).
struct TaskImp {
    closure: Obj,
    head_dep: *mut TaskObject,
    next_dep: *mut TaskObject,
    prio: u32,
    canceled: AtomicBool,
    keep_alive: bool,
    deleted: bool,
}

/// Layout of a promise object: the header and the task it resolves.
#[repr(C)]
struct PromiseObject {
    header: u64,
    result: *mut TaskObject,
}

#[inline(always)]
fn to_task(o: Obj) -> *mut TaskObject {
    o.ptr() as *mut TaskObject
}

#[inline(always)]
fn task_obj(t: *mut TaskObject) -> Obj {
    Obj::from_raw(t as *mut Object)
}

#[inline(always)]
unsafe fn task_value(t: *mut TaskObject) -> Obj {
    unsafe { Obj::from_raw((*t).value.load(Ordering::Acquire)) }
}

#[inline(always)]
unsafe fn task_imp(t: *mut TaskObject) -> *mut TaskImp {
    unsafe { (*t).imp.load(Ordering::Relaxed) }
}

fn alloc_task_imp(closure: Obj, prio: u32, keep_alive: bool) -> *mut TaskImp {
    Box::into_raw(Box::new(TaskImp {
        closure,
        head_dep: ptr::null_mut(),
        next_dep: ptr::null_mut(),
        prio,
        canceled: AtomicBool::new(false),
        keep_alive,
        deleted: false,
    }))
}

unsafe fn free_task(t: *mut TaskObject) {
    unsafe {
        let imp = task_imp(t);
        if !imp.is_null() {
            drop(Box::from_raw(imp));
        }
        dealloc_raw(task_obj(t));
    }
}

/// A new multi-threaded task that will run `c` (consumed).
unsafe fn alloc_task(c: Obj, prio: u32, keep_alive: bool) -> *mut TaskObject {
    unsafe {
        lean_mark_mt(c);
        let o = lean_alloc_small_object(size_of::<TaskObject>());
        lean_set_mt_header(o, LEAN_TASK, 0);
        let t = to_task(o);
        ptr::addr_of_mut!((*t).value).write(AtomicPtr::new(ptr::null_mut()));
        ptr::addr_of_mut!((*t).imp).write(AtomicPtr::new(alloc_task_imp(c, prio, keep_alive)));
        if keep_alive {
            lean_inc_ref(o);
        }
        t
    }
}

/// A finished task holding `v` (consumed).
unsafe fn alloc_task_value(v: Obj) -> *mut TaskObject {
    unsafe {
        let o = alloc_raw(size_of::<TaskObject>(), LEAN_TASK);
        let t = to_task(o);
        ptr::addr_of_mut!((*t).value).write(AtomicPtr::new(v.ptr()));
        ptr::addr_of_mut!((*t).imp).write(AtomicPtr::new(ptr::null_mut()));
        t
    }
}

#[derive(Clone, Copy)]
struct TaskPtr(*mut TaskObject);
unsafe impl Send for TaskPtr {}

thread_local! {
    static CURRENT_TASK: Cell<*mut TaskObject> = const { Cell::new(ptr::null_mut()) };
}

fn current_task() -> *mut TaskObject {
    CURRENT_TASK.with(|c| c.get())
}

// ---------------------------------------------------------------------------------------------
// Task manager
// ---------------------------------------------------------------------------------------------

struct State {
    std_workers: Vec<JoinHandle<()>>,
    idle_std_workers: u32,
    max_std_workers: u32,
    num_dedicated_workers: u32,
    queues: [VecDeque<TaskPtr>; LEAN_MAX_PRIO as usize + 1],
    queues_size: u32,
    max_prio: u32,
    shutting_down: bool,
}

struct TaskManager {
    state: Mutex<State>,
    queue_cv: Condvar,
    task_finished_cv: Condvar,
    dedicated_finished_cv: Condvar,
}

type Guard = MutexGuard<'static, State>;

static MANAGER: AtomicPtr<TaskManager> = AtomicPtr::new(ptr::null_mut());

fn manager() -> Option<&'static TaskManager> {
    let p = MANAGER.load(Ordering::Acquire);
    if p.is_null() { None } else { Some(unsafe { &*p }) }
}

fn manager_required() -> &'static TaskManager {
    manager().unwrap_or_else(|| lean_internal_panic("the Lean task manager is not running"))
}

impl TaskManager {
    fn lock(&'static self) -> Guard {
        self.state.lock().unwrap_or_else(|_| lean_internal_panic("the Lean task manager's state is poisoned"))
    }

    fn wait(&'static self, cv: &'static Condvar, g: Guard) -> Guard {
        cv.wait(g).unwrap_or_else(|_| lean_internal_panic("the Lean task manager's state is poisoned"))
    }

    fn dequeue(g: &mut Guard) -> *mut TaskObject {
        let max = g.max_prio as usize;
        let t = g.queues[max].pop_front().unwrap_or_else(|| lean_internal_panic("task queue is empty")).0;
        g.queues_size -= 1;
        if g.queues[max].is_empty() {
            while g.max_prio > 0 {
                g.max_prio -= 1;
                if !g.queues[g.max_prio as usize].is_empty() {
                    break;
                }
            }
        }
        t
    }

    unsafe fn enqueue_core(&'static self, mut g: Guard, t: *mut TaskObject) -> Guard {
        unsafe {
            let imp = task_imp(t);
            if imp.is_null() {
                lean_internal_panic("enqueued task has no execution data");
            }
            let prio = (*imp).prio;
            if prio == LEAN_SYNC_PRIO {
                return self.run_task(g, t);
            }
            if prio > LEAN_MAX_PRIO {
                self.spawn_dedicated_worker(&mut g, t);
                return g;
            }
            if prio > g.max_prio {
                g.max_prio = prio;
            }
            g.queues[prio as usize].push_back(TaskPtr(t));
            g.queues_size += 1;
            if g.idle_std_workers == 0 && (g.std_workers.len() as u32) < g.max_std_workers {
                self.spawn_worker(&mut g);
            } else {
                self.queue_cv.notify_one();
            }
            g
        }
    }

    unsafe fn deactivate_task_core(&'static self, g: Guard, t: *mut TaskObject) -> Guard {
        unsafe {
            let imp = task_imp(t);
            let c = (*imp).closure;
            let it = (*imp).head_dep;
            (*imp).closure = Obj::null();
            (*imp).head_dep = ptr::null_mut();
            (*imp).deleted = true;
            (*imp).canceled.store(true, Ordering::Relaxed);
            drop(g);
            free_dependents(it);
            if !c.is_null() {
                lean_dec_ref(c);
            }
            self.lock()
        }
    }

    fn spawn_worker(&'static self, g: &mut Guard) {
        if g.shutting_down {
            return;
        }
        let handle = std::thread::Builder::new()
            .name("lean-task-worker".into())
            .stack_size(thread_stack_size())
            .spawn(move || self.worker_loop())
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot start a task worker thread: {e}")));
        g.std_workers.push(handle);
    }

    fn worker_loop(&'static self) {
        let mut g = self.lock();
        g.idle_std_workers += 1;
        loop {
            if g.queues_size == 0 {
                if g.shutting_down {
                    break;
                }
                g = self.wait(&self.queue_cv, g);
                continue;
            }
            // Throttle when `Task.get` lowered the number of runnable workers, except during
            // shutdown, where the last notification may already have been sent.
            if !g.shutting_down && g.std_workers.len() as u32 - g.idle_std_workers >= g.max_std_workers {
                g = self.wait(&self.queue_cv, g);
                continue;
            }
            let t = Self::dequeue(&mut g);
            g.idle_std_workers -= 1;
            g = unsafe { self.run_task(g, t) };
            g.idle_std_workers += 1;
        }
        g.idle_std_workers -= 1;
    }

    fn spawn_dedicated_worker(&'static self, g: &mut Guard, t: *mut TaskObject) {
        g.num_dedicated_workers += 1;
        let task = TaskPtr(t);
        std::thread::Builder::new()
            .name("lean-dedicated-task".into())
            .stack_size(thread_stack_size())
            .spawn(move || {
                let task = task;
                let g = self.lock();
                let mut g = unsafe { self.run_task(g, task.0) };
                g.num_dedicated_workers -= 1;
                self.dedicated_finished_cv.notify_all();
            })
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot start a dedicated task thread: {e}")));
    }

    unsafe fn run_task(&'static self, g: Guard, t: *mut TaskObject) -> Guard {
        unsafe {
            let imp = task_imp(t);
            if imp.is_null() {
                lean_internal_panic("running a task without execution data");
            }
            if (*imp).deleted {
                free_task(t);
                return g;
            }
            let c = (*imp).closure;
            (*imp).closure = Obj::null();
            drop(g);
            let previous = CURRENT_TASK.with(|cur| cur.replace(t));
            let v = lean_apply_1(c, lean_box(0));
            // Deactivation delayed by `keep_alive` happens after the final execution.
            if !v.is_null() && (*imp).keep_alive {
                lean_dec_ref(task_obj(t));
            }
            let mut g = self.lock();
            CURRENT_TASK.with(|cur| cur.set(previous));
            if (*imp).deleted {
                drop(g);
                if !v.is_null() {
                    lean_dec(v);
                }
                free_task(t);
                g = self.lock();
            } else if !v.is_null() {
                g = self.resolve_core(g, t, v);
            } else {
                // A `bind` task whose inner task has not finished: it becomes a dependent of
                // the inner task. The closure is read before unlocking, as deactivation may
                // clear it concurrently.
                let c = (*imp).closure;
                drop(g);
                let inner = to_task(lean_closure_get(c, 0));
                self.add_dep(inner, t);
                g = self.lock();
            }
            g
        }
    }

    unsafe fn resolve_core(&'static self, g: Guard, t: *mut TaskObject, v: Obj) -> Guard {
        unsafe {
            lean_mark_mt(v);
            (*t).value.store(v.ptr(), Ordering::Release);
            let imp = (*t).imp.swap(ptr::null_mut(), Ordering::Relaxed);
            let g = self.handle_finished(g, imp);
            drop(Box::from_raw(imp));
            self.task_finished_cv.notify_all();
            g
        }
    }

    unsafe fn handle_finished(&'static self, mut g: Guard, imp: *mut TaskImp) -> Guard {
        unsafe {
            let mut it = (*imp).head_dep;
            (*imp).head_dep = ptr::null_mut();
            while !it.is_null() {
                let it_imp = task_imp(it);
                if (*imp).canceled.load(Ordering::Relaxed) {
                    (*it_imp).canceled.store(true, Ordering::Relaxed);
                }
                let next = (*it_imp).next_dep;
                (*it_imp).next_dep = ptr::null_mut();
                if (*it_imp).deleted {
                    free_task(it);
                } else {
                    g = self.enqueue_core(g, it);
                }
                it = next;
            }
            g
        }
    }

    unsafe fn enqueue(&'static self, t: *mut TaskObject) {
        unsafe {
            let g = self.lock();
            drop(self.enqueue_core(g, t));
        }
    }

    unsafe fn resolve(&'static self, t: *mut TaskObject, v: Obj) {
        unsafe {
            if !task_value(t).is_null() {
                lean_dec(v);
                return;
            }
            let g = self.lock();
            if !task_value(t).is_null() {
                // `lean_dec(v)` may deactivate a task, which takes the lock.
                drop(g);
                lean_dec(v);
                return;
            }
            drop(self.resolve_core(g, t, v));
        }
    }

    unsafe fn add_dep(&'static self, t1: *mut TaskObject, t2: *mut TaskObject) {
        unsafe {
            if !task_value(t1).is_null() {
                self.enqueue(t2);
                return;
            }
            let g = self.lock();
            if !task_value(t1).is_null() {
                drop(self.enqueue_core(g, t2));
                return;
            }
            let imp1 = task_imp(t1);
            let imp2 = task_imp(t2);
            (*imp2).next_dep = (*imp1).head_dep;
            (*imp1).head_dep = t2;
        }
    }

    unsafe fn wait_for(&'static self, t: *mut TaskObject) {
        unsafe {
            if !task_value(t).is_null() {
                return;
            }
            let mut g = self.lock();
            if !task_value(t).is_null() {
                return;
            }
            let cur = current_task();
            let cur_prio = if cur.is_null() { None } else { Some((*task_imp(cur)).prio) };
            if cur_prio == Some(LEAN_SYNC_PRIO) {
                crate::panic::lean_panic("`Task.get` called from a `(sync := true)` task");
            }
            let in_pool = matches!(cur_prio, Some(p) if p <= LEAN_MAX_PRIO);
            if in_pool {
                // Keep the number of runnable workers constant while this one blocks.
                g.max_std_workers += 1;
                if g.idle_std_workers == 0 {
                    self.spawn_worker(&mut g);
                } else {
                    self.queue_cv.notify_one();
                }
            }
            while task_value(t).is_null() {
                g = self.wait(&self.task_finished_cv, g);
            }
            if in_pool {
                g.max_std_workers -= 1;
            }
        }
    }

    unsafe fn wait_any(&'static self, task_list: Obj) -> Obj {
        unsafe {
            if let Some(t) = wait_any_check(task_list) {
                return t;
            }
            let mut g = self.lock();
            loop {
                if let Some(t) = wait_any_check(task_list) {
                    return t;
                }
                g = self.wait(&self.task_finished_cv, g);
            }
        }
    }

    unsafe fn deactivate_task(&'static self, t: *mut TaskObject) {
        unsafe {
            let g = self.lock();
            let v = task_value(t);
            if !v.is_null() {
                drop(g);
                lean_dec(v);
                free_task(t);
            } else {
                drop(self.deactivate_task_core(g, t));
            }
        }
    }

    unsafe fn cancel(&'static self, t: *mut TaskObject) {
        unsafe {
            let _g = self.lock();
            let imp = task_imp(t);
            if !imp.is_null() {
                (*imp).canceled.store(true, Ordering::Relaxed);
            }
        }
    }

    unsafe fn get_task_state(&'static self, t: *mut TaskObject) -> u8 {
        unsafe {
            let _g = self.lock();
            let imp = task_imp(t);
            if imp.is_null() {
                2
            } else if !(*imp).closure.is_null() {
                0
            } else {
                1
            }
        }
    }

    fn shutdown(&'static self) {
        let workers = {
            let mut g = self.lock();
            g.shutting_down = true;
            std::mem::take(&mut g.std_workers)
        };
        self.queue_cv.notify_all();
        for w in workers {
            if w.join().is_err() {
                lean_internal_panic("a Lean task worker thread panicked");
            }
        }
        let mut g = self.lock();
        while g.num_dedicated_workers != 0 {
            g = self.wait(&self.dedicated_finished_cv, g);
        }
    }
}

/// Frees the (deleted) dependents chained from `it`.
unsafe fn free_dependents(mut it: *mut TaskObject) {
    unsafe {
        while !it.is_null() {
            let imp = task_imp(it);
            if !(*imp).deleted {
                lean_internal_panic("a dependent of a deactivated task is still alive");
            }
            let next = (*imp).next_dep;
            free_task(it);
            it = next;
        }
    }
}

unsafe fn wait_any_check(task_list: Obj) -> Option<Obj> {
    unsafe {
        let mut it = task_list;
        while !it.is_scalar() {
            let head = lean_ctor_get(it, 0);
            if !task_value(to_task(head)).is_null() {
                return Some(head);
            }
            it = lean_ctor_get(it, 1);
        }
        None
    }
}

fn hardware_concurrency() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
}

/// `atoi`: leading whitespace, optional sign, decimal digits; anything else yields 0.
fn atoi(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut n: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    if neg { -n } else { n }
}

/// The number of task workers: `LEAN_NUM_THREADS`, else the hardware concurrency. WebAssembly
/// has no threads: tasks run eagerly on the thread that spawns them, as Lean's runtime runs them
/// without a task manager.
fn lean_num_threads() -> u32 {
    if cfg!(target_os = "wasi") {
        return 0;
    }
    match std::env::var("LEAN_NUM_THREADS") {
        Ok(v) => atoi(&v) as u32,
        Err(_) => hardware_concurrency(),
    }
}

/// Stack size of threads running Lean code: 1 GiB (reserved, not committed) on 64-bit
/// platforms and 8 MiB on 32-bit ones, overridable with `LEAN_STACK_SIZE_KB`.
pub fn thread_stack_size() -> usize {
    if let Ok(v) = std::env::var("LEAN_STACK_SIZE_KB") {
        let kb = atoi(&v).max(0) as usize;
        let sz = kb / 4 * 4 * 1024;
        if sz > 0 {
            return sz + LEAN_STACK_BUFFER_SPACE;
        }
    }
    if usize::BITS == 64 { 1024 * 1024 * 1024 } else { 8 * 1024 * 1024 }
}

/// Starts the task manager with `LEAN_NUM_THREADS` (default: hardware concurrency) workers.
/// Must be called at most once before `finalize_task_manager`.
pub fn init_task_manager() {
    init_task_manager_using(lean_num_threads());
}

/// Starts the task manager with `num_workers` workers; zero disables it (tasks then run
/// eagerly on the spawning thread).
pub fn init_task_manager_using(num_workers: u32) {
    if !MANAGER.load(Ordering::Acquire).is_null() {
        lean_internal_panic("the Lean task manager is already running");
    }
    if num_workers == 0 {
        return;
    }
    let m = Box::into_raw(Box::new(TaskManager {
        state: Mutex::new(State {
            std_workers: Vec::new(),
            idle_std_workers: 0,
            max_std_workers: num_workers,
            num_dedicated_workers: 0,
            queues: Default::default(),
            queues_size: 0,
            max_prio: 0,
            shutting_down: false,
        }),
        queue_cv: Condvar::new(),
        task_finished_cv: Condvar::new(),
        dedicated_finished_cv: Condvar::new(),
    }));
    MANAGER.store(m, Ordering::Release);
}

/// Starts the task manager unless it is already running. For embedders of generated code that
/// do not go through a Lean `main`.
pub fn ensure_task_manager() {
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        if MANAGER.load(Ordering::Acquire).is_null() {
            init_task_manager();
        }
    });
}

/// Shuts the task manager down: queued tasks are still run, then all worker threads (and
/// dedicated threads) are joined. Tasks spawned afterwards run eagerly.
pub fn finalize_task_manager() {
    let Some(m) = manager() else { return };
    m.shutdown();
    // Threads that finished tasks during shutdown have exited; the manager is left allocated
    // because task objects may still reach it transiently while they are being released.
    MANAGER.store(ptr::null_mut(), Ordering::Release);
}

/// Releases a task whose reference count reached zero.
pub unsafe fn deactivate_task(o: Obj) {
    unsafe {
        let t = to_task(o);
        match manager() {
            Some(m) => m.deactivate_task(t),
            None => {
                let v = task_value(t);
                if !v.is_null() {
                    lean_dec(v);
                } else {
                    // An unfinished task outliving the manager: release its closure and
                    // (necessarily deleted) dependents.
                    let imp = task_imp(t);
                    let c = (*imp).closure;
                    (*imp).closure = Obj::null();
                    free_dependents((*imp).head_dep);
                    (*imp).head_dep = ptr::null_mut();
                    if !c.is_null() {
                        lean_dec_ref(c);
                    }
                }
                free_task(t);
            }
        }
    }
}

/// Releases a promise whose reference count reached zero, resolving its task to `none`.
pub unsafe fn deactivate_promise(o: Obj) {
    unsafe {
        let p = o.ptr() as *mut PromiseObject;
        let t = (*p).result;
        resolve_task(t, lean_box(0));
        lean_dec_ref(task_obj(t));
        dealloc_raw(o);
    }
}

/// The value of the task `o` (borrowed), waiting for it to finish.
pub unsafe fn lean_task_get(o: Obj) -> Obj {
    unsafe {
        let t = to_task(o);
        let v = task_value(t);
        if !v.is_null() {
            return v;
        }
        manager_required().wait_for(t);
        let v = task_value(t);
        if v.is_null() {
            lean_internal_panic("waited task has no value");
        }
        v
    }
}

/// The task of the promise `o` (borrowed).
pub unsafe fn promise_result_task(o: Obj) -> Obj {
    unsafe { task_obj((*(o.ptr() as *mut PromiseObject)).result) }
}

unsafe fn resolve_task(t: *mut TaskObject, v: Obj) {
    unsafe {
        match manager() {
            Some(m) => m.resolve(t, v),
            None => {
                // Without a manager nobody can be waiting on the task.
                if !task_value(t).is_null() {
                    lean_dec(v);
                    return;
                }
                lean_mark_mt(v);
                (*t).value.store(v.ptr(), Ordering::Release);
                let imp = (*t).imp.swap(ptr::null_mut(), Ordering::Relaxed);
                free_dependents((*imp).head_dep);
                drop(Box::from_raw(imp));
            }
        }
    }
}

unsafe fn mk_closure(f: *const (), arity: u32, fixed: &[Obj]) -> Obj {
    unsafe {
        let c = lean_alloc_closure(f, arity, fixed.len() as u32);
        for (i, a) in fixed.iter().enumerate() {
            lean_closure_set(c, i as u32, *a);
        }
        c
    }
}

/// `Task.spawn` with explicit `keep_alive`.
pub unsafe fn task_spawn_core(c: Obj, prio: u32, keep_alive: bool) -> Obj {
    unsafe {
        match manager() {
            None => task_obj(alloc_task_value(lean_apply_1(c, lean_box(0)))),
            Some(m) => {
                let t = alloc_task(c, prio, keep_alive);
                m.enqueue(t);
                task_obj(t)
            }
        }
    }
}

unsafe extern "C" fn task_map_fn(f: Obj, t: Obj, _w: Obj) -> Obj {
    unsafe {
        let v = task_value(to_task(t));
        lean_inc(v);
        lean_dec_ref(t);
        lean_apply_1(f, v)
    }
}

pub unsafe fn task_map_core(f: Obj, t: Obj, prio: u32, sync: bool, keep_alive: bool) -> Obj {
    unsafe {
        let m = manager();
        if m.is_none() || (sync && !task_value(to_task(t)).is_null()) {
            return task_obj(alloc_task_value(lean_apply_1(f, task_get_own(t))));
        }
        let m = m.unwrap_or_else(|| lean_internal_panic("unreachable"));
        let c = mk_closure(task_map_fn as *const (), 3, &[f, t]);
        let new_task = alloc_task(c, if sync { LEAN_SYNC_PRIO } else { prio }, keep_alive);
        m.add_dep(to_task(t), new_task);
        task_obj(new_task)
    }
}

unsafe extern "C" fn task_bind_fn2(t: Obj, _w: Obj) -> Obj {
    unsafe {
        let v = task_value(to_task(t));
        lean_inc(v);
        lean_dec_ref(t);
        v
    }
}

unsafe extern "C" fn task_bind_fn1(x: Obj, f: Obj, _w: Obj) -> Obj {
    unsafe {
        let v = task_value(to_task(x));
        lean_inc(v);
        lean_dec_ref(x);
        let new_task = lean_apply_1(f, v);
        if new_task.is_scalar() || lean_ptr_tag(new_task) != LEAN_TASK {
            lean_internal_panic("Task.bind continuation did not return a task");
        }
        let v = task_value(to_task(new_task));
        if !v.is_null() {
            lean_inc(v);
            lean_dec_ref(new_task);
            v
        } else {
            let cur = current_task();
            if cur.is_null() {
                lean_internal_panic("Task.bind continuation ran outside a task");
            }
            let imp = task_imp(cur);
            let c = mk_closure(task_bind_fn2 as *const (), 2, &[new_task]);
            lean_mark_mt(c);
            (*imp).closure = c;
            // Tells the task manager that the task has not finished yet.
            Obj::null()
        }
    }
}

pub unsafe fn task_bind_core(x: Obj, f: Obj, prio: u32, sync: bool, keep_alive: bool) -> Obj {
    unsafe {
        let m = manager();
        if m.is_none() || (sync && !task_value(to_task(x)).is_null()) {
            return lean_apply_1(f, task_get_own(x));
        }
        let m = m.unwrap_or_else(|| lean_internal_panic("unreachable"));
        let c = mk_closure(task_bind_fn1 as *const (), 3, &[x, f]);
        let new_task = alloc_task(c, if sync { LEAN_SYNC_PRIO } else { prio }, keep_alive);
        m.add_dep(to_task(x), new_task);
        task_obj(new_task)
    }
}

/// `Task.get` on an owned task.
pub unsafe fn task_get_own(t: Obj) -> Obj {
    unsafe {
        let r = lean_task_get(t);
        lean_inc(r);
        lean_dec(t);
        r
    }
}

unsafe extern "C" fn io_as_task_fn(act: Obj, _w: Obj) -> Obj {
    unsafe { lean_apply_1(act, lean_box(0)) }
}

unsafe extern "C" fn io_bind_task_fn(f: Obj, a: Obj) -> Obj {
    unsafe { lean_apply_2(f, a, lean_box(0)) }
}

fn check_canceled() -> bool {
    let t = current_task();
    if t.is_null() {
        return false;
    }
    unsafe {
        let imp = task_imp(t);
        if imp.is_null() {
            lean_internal_panic("the current task has no execution data");
        }
        (*imp).canceled.load(Ordering::Relaxed) || manager().is_some_and(|m| m.lock().shutting_down)
    }
}

/// Creates a promise whose task is resolved by `IO.Promise.resolve`, or to `none` when the
/// promise is dropped.
unsafe fn promise_new() -> Obj {
    unsafe {
        if manager().is_none() {
            lean_internal_panic(
                "`IO.Promise.new` called before the task manager is running; this typically \
                 happens when called (directly or transitively, e.g. via `IO.CancelToken.new`) \
                 from an `initialize` block. Construct lazily on first use instead.",
            );
        }
        let o = lean_alloc_small_object(size_of::<TaskObject>());
        lean_set_mt_header(o, LEAN_TASK, 0);
        let t = to_task(o);
        ptr::addr_of_mut!((*t).value).write(AtomicPtr::new(ptr::null_mut()));
        ptr::addr_of_mut!((*t).imp).write(AtomicPtr::new(alloc_task_imp(Obj::null(), 0, false)));
        let p = alloc_raw(size_of::<PromiseObject>(), LEAN_PROMISE);
        (*(p.ptr() as *mut PromiseObject)).result = t;
        p
    }
}

/// Resolves `promise` (borrowed) with `some value` (consumed).
pub unsafe fn promise_resolve(value: Obj, promise: Obj) {
    unsafe {
        let t = (*(promise.ptr() as *mut PromiseObject)).result;
        resolve_task(t, lean_mk_option_some(value));
    }
}

/// Whether the promise `promise` (borrowed) has been resolved.
pub unsafe fn promise_is_resolved(promise: Obj) -> bool {
    unsafe { !task_value((*(promise.ptr() as *mut PromiseObject)).result).is_null() }
}

/// `IO.Promise.new` for runtime-internal producers (the `uv` primitives).
pub unsafe fn new_promise() -> Obj {
    unsafe { promise_new() }
}

// ---------------------------------------------------------------------------------------------
// Synchronization primitives (`Std.Sync`)
// ---------------------------------------------------------------------------------------------

/// A mutex whose lock and unlock are separate operations, as `std::mutex` in C++.
pub(crate) struct BaseMutex {
    locked: Mutex<bool>,
    released: Condvar,
}

impl BaseMutex {
    fn new() -> Self {
        BaseMutex { locked: Mutex::new(false), released: Condvar::new() }
    }

    fn state(&self) -> MutexGuard<'_, bool> {
        self.locked.lock().unwrap_or_else(|_| lean_internal_panic("Lean mutex state is poisoned"))
    }

    fn lock(&self) {
        let mut g = self.state();
        while *g {
            g = self.released.wait(g).unwrap_or_else(|_| lean_internal_panic("Lean mutex state is poisoned"));
        }
        *g = true;
    }

    fn try_lock(&self) -> bool {
        let mut g = self.state();
        if *g {
            false
        } else {
            *g = true;
            true
        }
    }

    fn unlock(&self) {
        let mut g = self.state();
        if !*g {
            lean_internal_panic("unlocking a mutex that is not locked");
        }
        *g = false;
        drop(g);
        self.released.notify_one();
    }
}

struct RecursiveMutex {
    state: Mutex<(Option<std::thread::ThreadId>, usize)>,
    released: Condvar,
}

impl RecursiveMutex {
    fn guard(&self) -> MutexGuard<'_, (Option<std::thread::ThreadId>, usize)> {
        self.state.lock().unwrap_or_else(|_| lean_internal_panic("Lean recursive mutex state is poisoned"))
    }

    fn lock(&self) {
        let me = std::thread::current().id();
        let mut g = self.guard();
        loop {
            match g.0 {
                None => {
                    *g = (Some(me), 1);
                    return;
                }
                Some(owner) if owner == me => {
                    g.1 += 1;
                    return;
                }
                Some(_) => {
                    g = self
                        .released
                        .wait(g)
                        .unwrap_or_else(|_| lean_internal_panic("Lean recursive mutex state is poisoned"));
                }
            }
        }
    }

    fn try_lock(&self) -> bool {
        let me = std::thread::current().id();
        let mut g = self.guard();
        match g.0 {
            None => {
                *g = (Some(me), 1);
                true
            }
            Some(owner) if owner == me => {
                g.1 += 1;
                true
            }
            Some(_) => false,
        }
    }

    fn unlock(&self) {
        let me = std::thread::current().id();
        let mut g = self.guard();
        if g.0 != Some(me) {
            lean_internal_panic("unlocking a recursive mutex not held by the current thread");
        }
        g.1 -= 1;
        if g.1 == 0 {
            g.0 = None;
            drop(g);
            self.released.notify_one();
        }
    }
}

struct SharedMutex {
    /// (writer holds the lock, number of readers)
    state: Mutex<(bool, usize)>,
    released: Condvar,
}

impl SharedMutex {
    fn guard(&self) -> MutexGuard<'_, (bool, usize)> {
        self.state.lock().unwrap_or_else(|_| lean_internal_panic("Lean shared mutex state is poisoned"))
    }

    fn wait<'a>(&self, g: MutexGuard<'a, (bool, usize)>) -> MutexGuard<'a, (bool, usize)> {
        self.released.wait(g).unwrap_or_else(|_| lean_internal_panic("Lean shared mutex state is poisoned"))
    }

    fn write(&self) {
        let mut g = self.guard();
        while g.0 || g.1 > 0 {
            g = self.wait(g);
        }
        g.0 = true;
    }

    fn try_write(&self) -> bool {
        let mut g = self.guard();
        if g.0 || g.1 > 0 {
            false
        } else {
            g.0 = true;
            true
        }
    }

    fn unlock_write(&self) {
        let mut g = self.guard();
        if !g.0 {
            lean_internal_panic("releasing a write lock that is not held");
        }
        g.0 = false;
        drop(g);
        self.released.notify_all();
    }

    fn read(&self) {
        let mut g = self.guard();
        while g.0 {
            g = self.wait(g);
        }
        g.1 += 1;
    }

    fn try_read(&self) -> bool {
        let mut g = self.guard();
        if g.0 {
            false
        } else {
            g.1 += 1;
            true
        }
    }

    fn unlock_read(&self) {
        let mut g = self.guard();
        if g.1 == 0 {
            lean_internal_panic("releasing a read lock that is not held");
        }
        g.1 -= 1;
        let last = g.1 == 0;
        drop(g);
        if last {
            self.released.notify_all();
        }
    }
}

/// A condition variable usable with any [`BaseMutex`], with the semantics of
/// `std::condition_variable`: waiting atomically releases the mutex, and notifications sent
/// after a waiter released the mutex are never lost.
struct CondVar {
    generation: Mutex<u64>,
    cv: Condvar,
}

impl CondVar {
    fn wait(&self, m: &BaseMutex) {
        let mut g =
            self.generation.lock().unwrap_or_else(|_| lean_internal_panic("Lean condition variable state is poisoned"));
        let seen = *g;
        m.unlock();
        while *g == seen {
            g = self.cv.wait(g).unwrap_or_else(|_| lean_internal_panic("Lean condition variable state is poisoned"));
        }
        drop(g);
        m.lock();
    }

    fn notify(&self, all: bool) {
        let mut g =
            self.generation.lock().unwrap_or_else(|_| lean_internal_panic("Lean condition variable state is poisoned"));
        *g = g.wrapping_add(1);
        drop(g);
        if all {
            self.cv.notify_all();
        } else {
            self.cv.notify_one();
        }
    }
}

unsafe fn finalize_boxed<T>(p: *mut ()) {
    unsafe { drop(Box::from_raw(p as *mut T)) }
}

unsafe fn no_children(_: *mut (), _: &mut dyn FnMut(Obj)) {}

static BASEMUTEX_CLASS: ExternalClass = ExternalClass { finalize: finalize_boxed::<BaseMutex>, for_each: no_children };
static RECMUTEX_CLASS: ExternalClass =
    ExternalClass { finalize: finalize_boxed::<RecursiveMutex>, for_each: no_children };
static SHAREDMUTEX_CLASS: ExternalClass =
    ExternalClass { finalize: finalize_boxed::<SharedMutex>, for_each: no_children };
static CONDVAR_CLASS: ExternalClass = ExternalClass { finalize: finalize_boxed::<CondVar>, for_each: no_children };

unsafe fn external<'a, T>(o: Obj, class: &'static ExternalClass) -> &'a T {
    unsafe {
        if !ptr::eq(lean_get_external_class(o), class) {
            lean_internal_panic("synchronization primitive of the wrong kind");
        }
        &*(lean_get_external_data(o) as *const T)
    }
}

unsafe fn new_external<T>(v: T, class: &'static ExternalClass) -> Obj {
    unsafe { lean_alloc_external(class, Box::into_raw(Box::new(v)) as *mut ()) }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_mk_thunk(c: obj) -> obj {
            alloc_thunk(Obj::null(), c)
        }

        fn lean_thunk_pure(v: obj) -> obj {
            alloc_thunk(v, Obj::null())
        }

        fn lean_thunk_get_own(t: b_obj) -> obj {
            let r = lean_thunk_get(t);
            lean_inc(r);
            r
        }

        fn lean_task_spawn(c: obj, prio: obj) -> obj {
            task_spawn_core(c, lean_unbox(prio) as u32, false)
        }

        fn lean_task_pure(a: obj) -> obj {
            task_obj(alloc_task_value(a))
        }

        fn lean_task_map(f: obj, t: obj, prio: obj, sync: u8) -> obj {
            task_map_core(f, t, lean_unbox(prio) as u32, sync != 0, false)
        }

        fn lean_task_bind(x: obj, f: obj, prio: obj, sync: u8) -> obj {
            task_bind_core(x, f, lean_unbox(prio) as u32, sync != 0, false)
        }

        fn lean_task_get_own(t: obj) -> obj {
            task_get_own(t)
        }

        fn lean_io_as_task(act: obj, prio: obj) -> obj {
            let c = mk_closure(io_as_task_fn as *const (), 2, &[act]);
            task_spawn_core(c, lean_unbox(prio) as u32, true)
        }

        fn lean_io_map_task(f: obj, t: obj, prio: obj, sync: u8) -> obj {
            let c = mk_closure(io_bind_task_fn as *const (), 2, &[f]);
            task_map_core(c, t, lean_unbox(prio) as u32, sync != 0, true)
        }

        fn lean_io_bind_task(t: obj, f: obj, prio: obj, sync: u8) -> obj {
            let c = mk_closure(io_bind_task_fn as *const (), 2, &[f]);
            task_bind_core(t, c, lean_unbox(prio) as u32, sync != 0, true)
        }

        fn lean_io_check_canceled() -> u8 {
            check_canceled() as u8
        }

        fn lean_io_cancel(t: b_obj) -> obj {
            if task_value(to_task(t)).is_null() {
                manager_required().cancel(to_task(t));
            }
            lean_box(0)
        }

        fn lean_io_get_task_state(t: b_obj) -> u8 {
            let task = to_task(t);
            if task_imp(task).is_null() {
                2
            } else {
                manager_required().get_task_state(task)
            }
        }

        fn lean_io_wait(t: obj) -> obj {
            task_get_own(t)
        }

        fn lean_io_wait_any(task_list: b_obj) -> obj {
            let t = manager_required().wait_any(task_list);
            let v = lean_task_get(t);
            lean_inc(v);
            v
        }

        fn lean_option_get_or_block(o: obj) -> obj {
            if o.is_scalar() {
                crate::panic::lean_panic(
                    "PANIC: Promise.result!: promise has been dropped without ever being resolved",
                );
                // Only reachable with non-fatal panics: the result can never become available.
                loop {
                    std::thread::park();
                }
            }
            let v = lean_ctor_get(o, 0);
            lean_inc(v);
            lean_dec(o);
            v
        }

        fn lean_io_promise_new() -> obj {
            promise_new()
        }

        fn lean_io_promise_resolve(value: obj, promise: b_obj) -> obj {
            promise_resolve(value, promise);
            lean_box(0)
        }

        fn lean_io_promise_result_opt(promise: b_obj) -> obj {
            let t = promise_result_task(promise);
            lean_inc_ref(t);
            t
        }

        fn lean_internal_get_hardware_concurrency(_unit: obj) -> u32 {
            hardware_concurrency()
        }

        fn lean_io_basemutex_new() -> obj {
            new_external(BaseMutex::new(), &BASEMUTEX_CLASS)
        }

        fn lean_io_basemutex_lock(m: b_obj) -> obj {
            external::<BaseMutex>(m, &BASEMUTEX_CLASS).lock();
            lean_box(0)
        }

        fn lean_io_basemutex_try_lock(m: b_obj) -> u8 {
            external::<BaseMutex>(m, &BASEMUTEX_CLASS).try_lock() as u8
        }

        fn lean_io_basemutex_unlock(m: b_obj) -> obj {
            external::<BaseMutex>(m, &BASEMUTEX_CLASS).unlock();
            lean_box(0)
        }

        fn lean_io_baserecmutex_new() -> obj {
            new_external(
                RecursiveMutex { state: Mutex::new((None, 0)), released: Condvar::new() },
                &RECMUTEX_CLASS,
            )
        }

        fn lean_io_baserecmutex_lock(m: b_obj) -> obj {
            external::<RecursiveMutex>(m, &RECMUTEX_CLASS).lock();
            lean_box(0)
        }

        fn lean_io_baserecmutex_try_lock(m: b_obj) -> u8 {
            external::<RecursiveMutex>(m, &RECMUTEX_CLASS).try_lock() as u8
        }

        fn lean_io_baserecmutex_unlock(m: b_obj) -> obj {
            external::<RecursiveMutex>(m, &RECMUTEX_CLASS).unlock();
            lean_box(0)
        }

        fn lean_io_basesharedmutex_new() -> obj {
            new_external(
                SharedMutex { state: Mutex::new((false, 0)), released: Condvar::new() },
                &SHAREDMUTEX_CLASS,
            )
        }

        fn lean_io_basesharedmutex_write(m: b_obj) -> obj {
            external::<SharedMutex>(m, &SHAREDMUTEX_CLASS).write();
            lean_box(0)
        }

        fn lean_io_basesharedmutex_try_write(m: b_obj) -> u8 {
            external::<SharedMutex>(m, &SHAREDMUTEX_CLASS).try_write() as u8
        }

        fn lean_io_basesharedmutex_unlock_write(m: b_obj) -> obj {
            external::<SharedMutex>(m, &SHAREDMUTEX_CLASS).unlock_write();
            lean_box(0)
        }

        fn lean_io_basesharedmutex_read(m: b_obj) -> obj {
            external::<SharedMutex>(m, &SHAREDMUTEX_CLASS).read();
            lean_box(0)
        }

        fn lean_io_basesharedmutex_try_read(m: b_obj) -> u8 {
            external::<SharedMutex>(m, &SHAREDMUTEX_CLASS).try_read() as u8
        }

        fn lean_io_basesharedmutex_unlock_read(m: b_obj) -> obj {
            external::<SharedMutex>(m, &SHAREDMUTEX_CLASS).unlock_read();
            lean_box(0)
        }

        fn lean_io_condvar_new() -> obj {
            new_external(CondVar { generation: Mutex::new(0), cv: Condvar::new() }, &CONDVAR_CLASS)
        }

        fn lean_io_condvar_wait(c: b_obj, m: b_obj) -> obj {
            external::<CondVar>(c, &CONDVAR_CLASS).wait(external::<BaseMutex>(m, &BASEMUTEX_CLASS));
            lean_box(0)
        }

        fn lean_io_condvar_notify_one(c: b_obj) -> obj {
            external::<CondVar>(c, &CONDVAR_CLASS).notify(false);
            lean_box(0)
        }

        fn lean_io_condvar_notify_all(c: b_obj) -> obj {
            external::<CondVar>(c, &CONDVAR_CLASS).notify(true);
            lean_box(0)
        }
    }
}

#[cfg(test)]
mod tests {
    //! Expected behaviour was observed from Lean 4.34.1 (`lean --run`) on the corresponding
    //! Lean programs: `Task.spawn`, `Task.map`, `Task.bind`, `IO.getTaskState`, promises
    //! (including dropped ones), cancellation, `IO.waitAny`, `IO.mapTask`, `IO.bindTask`,
    //! dedicated tasks, and the `Std.Sync` primitives.
    use super::externs::*;
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn setup() {
        ensure_task_manager();
    }

    unsafe fn closure(f: *const (), arity: u32, fixed: &[Obj]) -> Obj {
        unsafe { mk_closure(f, arity, fixed) }
    }

    unsafe extern "C" fn const_42(_unit: Obj) -> Obj {
        lean_box(42)
    }

    unsafe extern "C" fn add_one(x: Obj) -> Obj {
        lean_box(lean_unbox(x) + 1)
    }

    unsafe extern "C" fn times_two(x: Obj, _unit: Obj) -> Obj {
        lean_box(lean_unbox(x) * 2)
    }

    unsafe extern "C" fn spawn_doubled(x: Obj) -> Obj {
        unsafe { lean_task_spawn(closure(times_two as *const (), 2, &[x]), lean_box(0)) }
    }

    unsafe fn nat_of_task(t: Obj) -> usize {
        unsafe { lean_unbox(task_get_own(t)) }
    }

    unsafe fn list(xs: &[Obj]) -> Obj {
        unsafe {
            let mut l = lean_box(0);
            for x in xs.iter().rev() {
                let c = lean_alloc_ctor(1, 2, 0);
                lean_ctor_set(c, 0, *x);
                lean_ctor_set(c, 1, l);
                l = c;
            }
            l
        }
    }

    #[test]
    fn spawn_map_and_bind_compute_values() {
        setup();
        unsafe {
            let t = lean_task_spawn(closure(const_42 as *const (), 1, &[]), lean_box(0));
            assert_eq!(lean_unbox(lean_task_get(t)), 42);
            lean_inc(t);
            let m = lean_task_map(closure(add_one as *const (), 1, &[]), t, lean_box(0), 0);
            assert_eq!(nat_of_task(m), 43);
            lean_inc(t);
            let b = lean_task_bind(t, closure(spawn_doubled as *const (), 1, &[]), lean_box(0), 0);
            assert_eq!(nat_of_task(b), 84);
            // `sync` continuations of finished tasks run immediately.
            lean_inc(t);
            let s = lean_task_map(closure(add_one as *const (), 1, &[]), t, lean_box(0), 1);
            assert_eq!(lean_io_get_task_state(s), 2);
            assert_eq!(nat_of_task(s), 43);
            lean_dec(t);
        }
    }

    unsafe extern "C" fn sleep_then(value: Obj, _unit: Obj) -> Obj {
        std::thread::sleep(Duration::from_millis(lean_unbox(value) as u64));
        value
    }

    #[test]
    fn dependents_and_dedicated_tasks_run() {
        setup();
        unsafe {
            let slow = lean_task_spawn(closure(sleep_then as *const (), 2, &[lean_box(50)]), lean_box(0));
            // A sync map on an unfinished task runs when the dependency resolves.
            let m = lean_task_map(closure(add_one as *const (), 1, &[]), slow, lean_box(0), 1);
            assert_eq!(nat_of_task(m), 51);
            // Priority 9 (`Task.Priority.dedicated`) runs on its own thread.
            let d = lean_task_spawn(closure(sleep_then as *const (), 2, &[lean_box(77)]), lean_box(9));
            assert_eq!(nat_of_task(d), 77);
        }
    }

    #[test]
    fn pure_tasks_are_finished() {
        setup();
        unsafe {
            let t = lean_task_pure(lean_box(1));
            assert_eq!(lean_io_get_task_state(t), 2);
            lean_dec(t);
        }
    }

    #[test]
    fn promises_resolve_once_and_dropped_promises_yield_none() {
        setup();
        unsafe {
            let p = lean_io_promise_new();
            let r = lean_io_promise_result_opt(p);
            assert_eq!(lean_io_get_task_state(r), 1);
            lean_io_promise_resolve(lean_box(5), p);
            assert_eq!(lean_io_get_task_state(r), 2);
            let v = lean_task_get(r);
            assert_eq!(lean_obj_tag(v), 1);
            assert_eq!(lean_unbox(lean_ctor_get(v, 0)), 5);
            lean_io_promise_resolve(lean_box(9), p);
            assert_eq!(lean_unbox(lean_ctor_get(lean_task_get(r), 0)), 5);
            lean_dec(r);
            lean_dec(p);

            let p2 = lean_io_promise_new();
            let r2 = lean_io_promise_result_opt(p2);
            lean_dec(p2);
            assert!(lean_task_get(r2).is_scalar());
            assert_eq!(lean_unbox(lean_task_get(r2)), 0);
            lean_dec(r2);
        }
    }

    unsafe extern "C" fn wait_gate_then_check(gate: Obj, _world: Obj) -> Obj {
        unsafe {
            let _ = lean_task_get(gate);
            lean_dec(gate);
            lean_box(lean_io_check_canceled() as usize)
        }
    }

    #[test]
    fn cancellation_is_observed_by_the_task() {
        setup();
        unsafe {
            let gate = lean_io_promise_new();
            let gate_task = lean_io_promise_result_opt(gate);
            let act = closure(wait_gate_then_check as *const (), 2, &[gate_task]);
            let t = lean_io_as_task(act, lean_box(0));
            lean_io_cancel(t);
            lean_io_promise_resolve(lean_box(0), gate);
            assert_eq!(lean_unbox(lean_io_wait(t)), 1);
            lean_dec(gate);

            let gate = lean_io_promise_new();
            let gate_task = lean_io_promise_result_opt(gate);
            let act = closure(wait_gate_then_check as *const (), 2, &[gate_task]);
            let t = lean_io_as_task(act, lean_box(0));
            lean_io_promise_resolve(lean_box(0), gate);
            assert_eq!(lean_unbox(lean_io_wait(t)), 0);
            lean_dec(gate);
            assert_eq!(lean_io_check_canceled(), 0, "the test thread is not a task");
        }
    }

    unsafe extern "C" fn io_const(value: Obj, _world: Obj) -> Obj {
        value
    }

    unsafe extern "C" fn io_add(k: Obj, x: Obj, _world: Obj) -> Obj {
        lean_box(lean_unbox(k) + lean_unbox(x))
    }

    unsafe extern "C" fn io_spawn_add(k: Obj, x: Obj, _world: Obj) -> Obj {
        unsafe {
            let act = closure(io_const as *const (), 2, &[lean_box(lean_unbox(k) + lean_unbox(x))]);
            lean_io_as_task(act, lean_box(0))
        }
    }

    #[test]
    fn io_task_combinators() {
        setup();
        unsafe {
            let slow = lean_io_as_task(closure(sleep_then as *const (), 2, &[lean_box(300)]), lean_box(0));
            let fast = lean_io_as_task(closure(io_const as *const (), 2, &[lean_box(2)]), lean_box(0));
            let l = list(&[slow, fast]);
            assert_eq!(lean_unbox(lean_io_wait_any(l)), 2);
            lean_dec(l);

            let t = lean_task_pure(lean_box(42));
            lean_inc(t);
            let mt = lean_io_map_task(closure(io_add as *const (), 3, &[lean_box(100)]), t, lean_box(0), 0);
            assert_eq!(lean_unbox(lean_io_wait(mt)), 142);
            let bt = lean_io_bind_task(t, closure(io_spawn_add as *const (), 3, &[lean_box(1000)]), lean_box(0), 0);
            assert_eq!(lean_unbox(lean_io_wait(bt)), 1042);
        }
    }

    static THUNK_RUNS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn counted_five(_unit: Obj) -> Obj {
        THUNK_RUNS.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        lean_box(5)
    }

    #[test]
    fn thunks_are_evaluated_once_under_concurrent_access() {
        unsafe {
            let th = lean_mk_thunk(closure(counted_five as *const (), 1, &[]));
            lean_mark_mt(th);
            let shared = SendObj(th);
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    std::thread::spawn(move || {
                        let s = shared;
                        let v = lean_thunk_get_own(s.0);
                        lean_unbox(v)
                    })
                })
                .collect();
            for h in handles {
                assert_eq!(h.join().unwrap(), 5);
            }
            assert_eq!(THUNK_RUNS.load(Ordering::SeqCst), 1);
            lean_dec(th);
            let p = lean_thunk_pure(lean_box(7));
            assert_eq!(lean_unbox(lean_thunk_get_own(p)), 7);
            lean_dec(p);
        }
    }

    #[test]
    fn mutexes_follow_std_sync_semantics() {
        unsafe {
            let m = lean_io_basemutex_new();
            assert_eq!(lean_io_basemutex_try_lock(m), 1);
            assert_eq!(lean_io_basemutex_try_lock(m), 0);
            lean_io_basemutex_unlock(m);
            assert_eq!(lean_io_basemutex_try_lock(m), 1);
            lean_io_basemutex_unlock(m);
            lean_dec(m);

            let r = lean_io_baserecmutex_new();
            lean_io_baserecmutex_lock(r);
            assert_eq!(lean_io_baserecmutex_try_lock(r), 1);
            lean_mark_mt(r);
            let rs = SendObj(r);
            let other = std::thread::spawn(move || {
                let rs = rs;
                lean_io_baserecmutex_try_lock(rs.0)
            });
            assert_eq!(other.join().unwrap(), 0);
            lean_io_baserecmutex_unlock(r);
            lean_io_baserecmutex_unlock(r);
            lean_dec(r);

            let s = lean_io_basesharedmutex_new();
            lean_io_basesharedmutex_read(s);
            assert_eq!(lean_io_basesharedmutex_try_read(s), 1);
            assert_eq!(lean_io_basesharedmutex_try_write(s), 0);
            lean_io_basesharedmutex_unlock_read(s);
            lean_io_basesharedmutex_unlock_read(s);
            assert_eq!(lean_io_basesharedmutex_try_write(s), 1);
            assert_eq!(lean_io_basesharedmutex_try_read(s), 0);
            lean_io_basesharedmutex_unlock_write(s);
            lean_dec(s);
        }
    }

    #[test]
    fn condition_variable_wakes_a_waiter_holding_the_mutex() {
        unsafe {
            let m = lean_io_basemutex_new();
            let c = lean_io_condvar_new();
            lean_mark_mt(m);
            lean_mark_mt(c);
            static READY: AtomicBool = AtomicBool::new(false);
            let (ms, cs) = (SendObj(m), SendObj(c));
            let waiter = std::thread::spawn(move || {
                let (ms, cs) = (ms, cs);
                lean_io_basemutex_lock(ms.0);
                while !READY.load(Ordering::SeqCst) {
                    lean_io_condvar_wait(cs.0, ms.0);
                }
                // The mutex is held again after waking.
                let relock = lean_io_basemutex_try_lock(ms.0);
                lean_io_basemutex_unlock(ms.0);
                relock
            });
            std::thread::sleep(Duration::from_millis(50));
            lean_io_basemutex_lock(m);
            READY.store(true, Ordering::SeqCst);
            lean_io_condvar_notify_all(c);
            lean_io_basemutex_unlock(m);
            assert_eq!(waiter.join().unwrap(), 0);
            lean_dec(m);
            lean_dec(c);
        }
    }

    #[test]
    fn hardware_concurrency_is_positive() {
        unsafe {
            assert!(lean_internal_get_hardware_concurrency(lean_box(0)) > 0);
        }
    }
}
