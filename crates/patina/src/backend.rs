//! The runtime that owns Lean objects.
//!
//! Facade conversions read objects through their memory layout, which is identical in the
//! patina runtime and in Lean's C runtime, but must allocate, reference-count, and apply
//! through the runtime that owns the objects. [`RustBackend`] is the patina runtime used by
//! PureRust code; code generated in `LeanOracle` mode provides a backend for Lean's C runtime.

use num_bigint::{BigInt, BigUint};
use patina_runtime::{self as rt, Obj};

/// Allocation, reference counting, and application of Lean objects.
///
/// # Safety
///
/// Implementations must produce objects with Lean's standard layout and follow Lean's
/// reference-counting conventions.
///
/// Every `Obj` passed to a method must be a live object (or tagged scalar) of the runtime this
/// backend implements; "owned" arguments transfer one reference to the callee, "borrowed" ones
/// do not. Objects returned are owned by the caller unless stated otherwise.
pub unsafe trait Backend: 'static {
    /// Allocates a constructor object with `num_objs` object fields and `scalar_size` bytes of
    /// scalar fields.
    ///
    /// # Safety
    ///
    /// Every object field must be initialized before the object is used or released.
    unsafe fn alloc_ctor(tag: u32, num_objs: u32, scalar_size: usize) -> Obj;
    /// Retains `o` (which may be a scalar).
    ///
    /// # Safety
    ///
    /// `o` must be live.
    unsafe fn inc(o: Obj);
    /// Releases `o` (which may be a scalar).
    ///
    /// # Safety
    ///
    /// `o` must be live and the caller must own the released reference.
    unsafe fn dec(o: Obj);
    /// Marks `o` and everything reachable from it as shared between threads.
    ///
    /// # Safety
    ///
    /// `o` must be live and not concurrently accessed during the call.
    unsafe fn mark_mt(o: Obj);
    fn mk_string(s: &str) -> Obj;
    /// A natural number larger than the scalar range.
    fn nat_from_big(v: &BigUint) -> Obj;
    /// Reads a natural number stored as a big-number object.
    ///
    /// # Safety
    ///
    /// `o` must be a live (borrowed) `Nat` big-number object.
    unsafe fn nat_to_big(o: Obj) -> BigUint;
    /// An integer outside the scalar range.
    fn int_from_big(v: &BigInt) -> Obj;
    /// Reads an integer stored as a big-number object.
    ///
    /// # Safety
    ///
    /// `o` must be a live (borrowed) `Int` big-number object.
    unsafe fn int_to_big(o: Obj) -> BigInt;
    /// Allocates an `Array` object with room for `capacity` elements.
    ///
    /// # Safety
    ///
    /// The first `size` elements must be initialized before the array is used or released.
    unsafe fn alloc_array(size: usize, capacity: usize) -> Obj;
    /// Allocates a scalar array (`ByteArray`, `FloatArray`) of `elem_size`-byte elements.
    ///
    /// # Safety
    ///
    /// The first `size` elements must be initialized before the array is read.
    unsafe fn alloc_sarray(elem_size: u32, size: usize, capacity: usize) -> Obj;
    /// Allocates a closure of `fun` taking `arity` boxed arguments, `num_fixed` of them stored.
    ///
    /// # Safety
    ///
    /// `fun` must be an `unsafe extern "C" fn` taking `arity` objects and returning an object,
    /// and the `num_fixed` fixed arguments must be set before the closure is used or released.
    unsafe fn alloc_closure(fun: *const (), arity: u32, num_fixed: u32) -> Obj;
    /// Applies closure `f` (owned) to `args` (owned).
    ///
    /// # Safety
    ///
    /// `f` must be a live closure and every argument a live object.
    unsafe fn apply(f: Obj, args: &[Obj]) -> Obj;
    /// Wraps `data` in an external object whose finalizer is `finalize`.
    ///
    /// # Safety
    ///
    /// `finalize(data)` must be sound to call exactly once, when the object is freed, on any
    /// thread.
    unsafe fn alloc_external(data: *mut (), finalize: unsafe extern "C" fn(*mut ())) -> Obj;
    /// The data pointer of an external object created by `alloc_external`.
    ///
    /// # Safety
    ///
    /// `o` must be a live external object created by this backend's `alloc_external`.
    unsafe fn external_data(o: Obj) -> *mut ();
    /// Renders the `IO.Error` `err` (borrowed) with Lean's `IO.Error.toString`.
    ///
    /// # Safety
    ///
    /// `err` must be a live `IO.Error`.
    unsafe fn io_error_to_string(err: Obj) -> String;
    /// `IO.userError msg`, consuming `msg`.
    ///
    /// # Safety
    ///
    /// `msg` must be a live `String` object owned by the caller.
    unsafe fn mk_io_user_error(msg: Obj) -> Obj;
}

/// The patina runtime.
pub enum RustBackend {}

struct External {
    data: *mut (),
    finalize: unsafe extern "C" fn(*mut ()),
}

unsafe fn external_finalize(p: *mut ()) {
    unsafe {
        let e = Box::from_raw(p as *mut External);
        (e.finalize)(e.data);
    }
}

unsafe fn external_for_each(_p: *mut (), _f: &mut dyn FnMut(Obj)) {}

static EXTERNAL_CLASS: rt::ExternalClass =
    rt::ExternalClass { finalize: external_finalize, for_each: external_for_each };

unsafe impl Backend for RustBackend {
    unsafe fn alloc_ctor(tag: u32, num_objs: u32, scalar_size: usize) -> Obj {
        unsafe { rt::lean_alloc_ctor(tag, num_objs, scalar_size) }
    }
    unsafe fn inc(o: Obj) {
        unsafe { rt::lean_inc(o) }
    }
    unsafe fn dec(o: Obj) {
        unsafe { rt::lean_dec(o) }
    }
    unsafe fn mark_mt(o: Obj) {
        unsafe { rt::lean_mark_mt(o) }
    }
    fn mk_string(s: &str) -> Obj {
        rt::lean_mk_string(s)
    }
    fn nat_from_big(v: &BigUint) -> Obj {
        rt::nat::nat_from_biguint(v.clone())
    }
    unsafe fn nat_to_big(o: Obj) -> BigUint {
        unsafe { rt::nat::nat_to_biguint(o) }
    }
    fn int_from_big(v: &BigInt) -> Obj {
        rt::int::int_from_bigint(v.clone())
    }
    unsafe fn int_to_big(o: Obj) -> BigInt {
        unsafe { rt::int::int_to_bigint(o) }
    }
    unsafe fn alloc_array(size: usize, capacity: usize) -> Obj {
        unsafe { rt::lean_alloc_array(size, capacity) }
    }
    unsafe fn alloc_sarray(elem_size: u32, size: usize, capacity: usize) -> Obj {
        unsafe { rt::lean_alloc_sarray(elem_size, size, capacity) }
    }
    unsafe fn alloc_closure(fun: *const (), arity: u32, num_fixed: u32) -> Obj {
        unsafe { rt::lean_alloc_closure(fun, arity, num_fixed) }
    }
    unsafe fn apply(f: Obj, args: &[Obj]) -> Obj {
        unsafe { rt::lean_apply_n(f, args) }
    }
    unsafe fn alloc_external(data: *mut (), finalize: unsafe extern "C" fn(*mut ())) -> Obj {
        let boxed = Box::into_raw(Box::new(External { data, finalize }));
        unsafe { rt::lean_alloc_external(&EXTERNAL_CLASS, boxed as *mut ()) }
    }
    unsafe fn external_data(o: Obj) -> *mut () {
        unsafe { (*(rt::lean_get_external_data(o) as *const External)).data }
    }
    unsafe fn io_error_to_string(err: Obj) -> String {
        unsafe { rt::init::io_error_to_string(err) }
    }
    unsafe fn mk_io_user_error(msg: Obj) -> Obj {
        unsafe { rt::io::mk::user_error(msg) }
    }
}
