//! Constants and initialization cells for generated C.
//!
//! Generated C declares these cells as zero-initialized statics and reads them with the inline
//! fast paths of `lungo.h`; the functions here are the slow paths. A constant is evaluated
//! exactly once, even when threads race to force it, and its value is marked persistent. The
//! synchronization is per cell (not one global lock), because evaluating one constant may force
//! others.

use crate::object::*;
use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// `lungo_lazy_obj`: an object constant; null until evaluated.
#[repr(C)]
pub struct LazyObj {
    value: AtomicPtr<Object>,
}

/// `lungo_lazy_bits`: a scalar constant, stored as the bits of a `uint64_t`.
#[repr(C)]
pub struct LazyBits {
    bits: AtomicU64,
    ready: AtomicU32,
}

/// `lungo_init_obj`: the value of an `initialize` declaration, set once by its module's
/// initializer.
#[repr(C)]
pub struct InitObj {
    value: AtomicPtr<Object>,
    ready: AtomicU32,
    /// The Lean declaration, for diagnostics.
    name: *const c_char,
}

/// `lungo_init_bits`: a scalar `initialize` declaration.
#[repr(C)]
pub struct InitBits {
    bits: AtomicU64,
    ready: AtomicU32,
    name: *const c_char,
}

/// The once-cell of each constant being evaluated, keyed by the cell's address.
fn once_of<T: Send + Sync + 'static>(
    table: &'static OnceLock<Mutex<HashMap<usize, Arc<OnceLock<T>>>>>,
    cell: usize,
) -> Arc<OnceLock<T>> {
    let map = table.get_or_init(Default::default);
    let mut map = map.lock().unwrap_or_else(|p| p.into_inner());
    map.entry(cell).or_default().clone()
}

static OBJ_ONCES: OnceLock<Mutex<HashMap<usize, Arc<OnceLock<SendObj>>>>> = OnceLock::new();
static BITS_ONCES: OnceLock<Mutex<HashMap<usize, Arc<OnceLock<u64>>>>> = OnceLock::new();

/// Evaluates the constant in `cell` with `init` unless it already is, and returns its value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_lazy_obj_force(cell: *mut LazyObj, init: unsafe extern "C" fn() -> Obj) -> Obj {
    let cell = unsafe { &*cell };
    let v = cell.value.load(Ordering::Acquire);
    if !v.is_null() {
        return Obj::from_raw(v);
    }
    let once = once_of(&OBJ_ONCES, cell as *const LazyObj as usize);
    let v = once
        .get_or_init(|| unsafe {
            let v = init();
            lean_mark_persistent(v);
            SendObj(v)
        })
        .0;
    cell.value.store(v.ptr(), Ordering::Release);
    v
}

/// Evaluates the scalar constant in `cell` with `init` unless it already is, and returns its
/// bits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_lazy_bits_force(cell: *mut LazyBits, init: unsafe extern "C" fn() -> u64) -> u64 {
    let cell = unsafe { &*cell };
    if cell.ready.load(Ordering::Acquire) != 0 {
        return cell.bits.load(Ordering::Relaxed);
    }
    let once = once_of(&BITS_ONCES, cell as *const LazyBits as usize);
    let bits = *once.get_or_init(|| unsafe { init() });
    cell.bits.store(bits, Ordering::Relaxed);
    cell.ready.store(1, Ordering::Release);
    bits
}

unsafe fn cell_name(name: *const c_char) -> String {
    if name.is_null() {
        lean_internal_panic("an initialization cell has no declaration name");
    }
    unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
}

/// The states of an initialization cell's `ready` field: `lungo.h` reads the value once it is
/// `SET`.
const UNSET: u32 = 0;
const SETTING: u32 = 1;
const SET: u32 = 2;

/// Claims an unset initialization cell for its initializer; setting a cell twice is an
/// invariant violation.
fn claim(ready: &AtomicU32, name: *const c_char) {
    if ready.compare_exchange(UNSET, SETTING, Ordering::Acquire, Ordering::Relaxed).is_err() {
        lean_internal_panic(&format!("initializer of '{}' ran twice", unsafe { cell_name(name) }));
    }
}

/// Sets the value of an `initialize` declaration: `v` becomes persistent. Setting it twice is an
/// invariant violation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_init_obj_set(cell: *mut InitObj, v: Obj) {
    let cell = unsafe { &*cell };
    claim(&cell.ready, cell.name);
    unsafe { lean_mark_persistent(v) };
    cell.value.store(v.ptr(), Ordering::Relaxed);
    cell.ready.store(SET, Ordering::Release);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_init_bits_set(cell: *mut InitBits, bits: u64) {
    let cell = unsafe { &*cell };
    claim(&cell.ready, cell.name);
    cell.bits.store(bits, Ordering::Relaxed);
    cell.ready.store(SET, Ordering::Release);
}

/// Reports the use of the `initialize` declaration `name` before its module was initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_panic_uninitialized(name: *const c_char) -> ! {
    lean_internal_panic(&format!("'{}' was used before its module was initialized", unsafe { cell_name(name) }))
}
