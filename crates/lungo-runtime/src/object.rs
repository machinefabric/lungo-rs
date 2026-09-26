//! The Lean object model.
//!
//! This is a direct port of the object representation defined by Lean's `lean.h` and
//! `runtime/object.cpp`: objects carry an 8-byte header with a reference count, the number of
//! object fields (or the element size of scalar arrays), and a tag; scalars are tagged pointers
//! with the low bit set. Reference counts are positive for objects owned by one thread, negative
//! for objects shared between threads (adjusted atomically), and zero for persistent objects,
//! which are never freed.
//!
//! Every function here is `unsafe`: callers must pass values that satisfy the representation
//! invariants of Lean's compiler output. Generated code upholds them by construction; the public
//! facade layer only produces values through these functions.

#![allow(clippy::missing_safety_doc)]

use num_bigint::BigInt;
use std::alloc::{Layout, alloc, dealloc};
use std::ptr;
use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

pub const LEAN_CLOSURE_MAX_ARGS: usize = 16;
pub const LEAN_MAX_CTOR_TAG: u8 = 243;
pub const LEAN_PROMISE: u8 = 244;
pub const LEAN_CLOSURE: u8 = 245;
pub const LEAN_ARRAY: u8 = 246;
pub const LEAN_STRUCT_ARRAY: u8 = 247;
pub const LEAN_SCALAR_ARRAY: u8 = 248;
pub const LEAN_STRING: u8 = 249;
pub const LEAN_MPZ: u8 = 250;
pub const LEAN_THUNK: u8 = 251;
pub const LEAN_TASK: u8 = 252;
pub const LEAN_REF: u8 = 253;
pub const LEAN_EXTERNAL: u8 = 254;
pub const LEAN_RESERVED: u8 = 255;

pub const LEAN_MAX_CTOR_FIELDS: u32 = 256;
pub const LEAN_MAX_CTOR_SCALARS_SIZE: usize = 1024;
/// Largest natural number represented as a tagged scalar.
pub const LEAN_MAX_SMALL_NAT: usize = usize::MAX >> 1;
pub const LEAN_MAX_SMALL_INT: i64 = if usize::BITS == 64 { i32::MAX as i64 } else { (i32::MAX >> 1) as i64 };
pub const LEAN_MIN_SMALL_INT: i64 = if usize::BITS == 64 { i32::MIN as i64 } else { (i32::MIN >> 1) as i64 };

const LEAN_RC_STICKY: i32 = i32::MIN + 0x1000_0000;
const LEAN_RC_STICKY_DROP: i32 = i32::MIN + 0x2000_0000;
const LEAN_RC_INC_MAX: usize = 0x10000;
const LEAN_RC_STUCK_ST: i32 = i32::MIN + LEAN_RC_INC_MAX as i32;

/// The object header, laid out exactly like Lean's `lean_object`.
#[repr(C)]
pub struct Object {
    rc: AtomicI32,
    cs_sz: u16,
    other: u8,
    tag: u8,
}

/// A Lean value: a pointer to a heap object, or a tagged scalar when the low bit is set.
#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Obj(*mut Object);

/// An [`Obj`] moved between threads. The object must have been marked multi-threaded (or be
/// persistent or a scalar) before it is shared.
#[derive(Copy, Clone)]
pub struct SendObj(pub Obj);
unsafe impl Send for SendObj {}
unsafe impl Sync for SendObj {}

impl Obj {
    #[inline(always)]
    pub fn is_scalar(self) -> bool {
        (self.0.addr() & 1) == 1
    }

    #[inline(always)]
    pub fn ptr(self) -> *mut Object {
        self.0
    }

    #[inline(always)]
    pub fn addr(self) -> usize {
        self.0.addr()
    }

    /// The null object pointer, used only as the "absent" state of thunk and task slots.
    #[inline(always)]
    pub const fn null() -> Obj {
        Obj(ptr::null_mut())
    }

    #[inline(always)]
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    #[inline(always)]
    pub fn from_raw(p: *mut Object) -> Obj {
        Obj(p)
    }
}

// ---------------------------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------------------------

#[inline(always)]
pub fn lean_box(n: usize) -> Obj {
    Obj(ptr::without_provenance_mut((n << 1) | 1))
}

#[inline(always)]
pub fn lean_unbox(o: Obj) -> usize {
    o.0.addr() >> 1
}

#[inline(always)]
pub fn lean_is_scalar(o: Obj) -> bool {
    o.is_scalar()
}

// ---------------------------------------------------------------------------------------------
// Header access
// ---------------------------------------------------------------------------------------------

#[inline(always)]
unsafe fn header<'a>(o: Obj) -> &'a Object {
    unsafe { &*o.0 }
}

#[inline(always)]
pub unsafe fn lean_ptr_tag(o: Obj) -> u8 {
    unsafe { (*o.0).tag }
}

#[inline(always)]
pub unsafe fn lean_ptr_other(o: Obj) -> u32 {
    unsafe { (*o.0).other as u32 }
}

#[inline(always)]
pub unsafe fn get_rc(o: Obj) -> i32 {
    unsafe { header(o).rc.load(Ordering::Relaxed) }
}

#[inline(always)]
unsafe fn set_rc(o: Obj, rc: i32) {
    unsafe { header(o).rc.store(rc, Ordering::Relaxed) }
}

#[inline(always)]
pub unsafe fn lean_is_mt(o: Obj) -> bool {
    unsafe { get_rc(o) < 0 }
}

#[inline(always)]
pub unsafe fn lean_is_st(o: Obj) -> bool {
    unsafe { get_rc(o) > 0 }
}

#[inline(always)]
pub unsafe fn lean_is_persistent(o: Obj) -> bool {
    unsafe { get_rc(o) == 0 }
}

#[inline(always)]
unsafe fn is_unstuck_mt(o: Obj) -> bool {
    unsafe { (get_rc(o) as u32) > (LEAN_RC_STICKY as u32) }
}

#[inline(always)]
unsafe fn is_never_freed(o: Obj) -> bool {
    unsafe { (get_rc(o) as u32) <= (LEAN_RC_STICKY_DROP as u32) }
}

#[inline(always)]
pub unsafe fn lean_obj_tag(o: Obj) -> u32 {
    if o.is_scalar() { lean_unbox(o) as u32 } else { unsafe { lean_ptr_tag(o) as u32 } }
}

#[inline(always)]
pub unsafe fn lean_is_exclusive(o: Obj) -> bool {
    unsafe { get_rc(o) == 1 }
}

#[inline(always)]
pub unsafe fn lean_is_exclusive_obj(o: Obj) -> u8 {
    unsafe { lean_is_exclusive(o) as u8 }
}

#[inline(always)]
pub unsafe fn lean_is_shared(o: Obj) -> bool {
    unsafe { get_rc(o) > 1 }
}

// ---------------------------------------------------------------------------------------------
// Allocation
// ---------------------------------------------------------------------------------------------

/// Bytes reserved before every object to record its allocation size.
const PREFIX: usize = 8;
const ALIGN: usize = 8;

/// Allocates `sz` bytes for an object. The header is left for the caller to initialize.
pub unsafe fn lean_alloc_object(sz: usize) -> Obj {
    let total = sz.checked_add(PREFIX).unwrap_or_else(|| lean_internal_panic_out_of_memory());
    let layout = Layout::from_size_align(total, ALIGN).unwrap_or_else(|_| lean_internal_panic_out_of_memory());
    unsafe {
        let base = alloc(layout);
        if base.is_null() {
            lean_internal_panic_out_of_memory();
        }
        (base as *mut usize).write(sz);
        Obj(base.add(PREFIX) as *mut Object)
    }
}

/// Allocates a small object (constructor, thunk, reference, task, big number, external),
/// counting one heartbeat as Lean's `lean_alloc_small_object` does.
#[inline]
pub unsafe fn lean_alloc_small_object(sz: usize) -> Obj {
    crate::io::inc_heartbeat();
    unsafe { lean_alloc_object(sz) }
}

/// The number of bytes allocated for `o`.
pub unsafe fn lean_object_byte_size(o: Obj) -> usize {
    unsafe { *((o.0 as *mut u8).sub(PREFIX) as *const usize) }
}

unsafe fn dealloc_object(o: Obj) {
    unsafe {
        let base = (o.0 as *mut u8).sub(PREFIX);
        let sz = *(base as *const usize);
        dealloc(base, Layout::from_size_align_unchecked(sz + PREFIX, ALIGN));
    }
}

/// Releases the memory of `o` without touching the objects it references.
pub unsafe fn lean_free_object(o: Obj) {
    unsafe {
        if lean_ptr_tag(o) == LEAN_MPZ {
            ptr::drop_in_place(&mut (*(o.0 as *mut MpzObject)).value);
        }
        dealloc_object(o);
    }
}

#[inline(always)]
pub unsafe fn lean_del_object(o: Obj) {
    if !o.is_scalar() {
        unsafe { lean_free_object(o) }
    }
}

#[inline(always)]
pub unsafe fn lean_set_st_header(o: Obj, tag: u8, other: u8) {
    unsafe {
        ptr::write(o.0, Object { rc: AtomicI32::new(1), cs_sz: 0, other, tag });
    }
}

/// Initializes the header of an object that is shared between threads from the start
/// (reference count `-1`), as Lean's task objects are.
#[inline(always)]
pub unsafe fn lean_set_mt_header(o: Obj, tag: u8, other: u8) {
    unsafe {
        ptr::write(o.0, Object { rc: AtomicI32::new(-1), cs_sz: 0, other, tag });
    }
}

// ---------------------------------------------------------------------------------------------
// Reference counting
// ---------------------------------------------------------------------------------------------

#[cold]
unsafe fn inc_ref_huge_n(o: Obj, n: usize) {
    unsafe {
        if lean_is_st(o) {
            let rc = get_rc(o);
            if n > (i32::MAX - rc) as usize {
                set_rc(o, LEAN_RC_STUCK_ST);
            } else {
                set_rc(o, rc + n as i32);
            }
        } else {
            let mut n = n;
            while n > 0 && is_unstuck_mt(o) {
                let chunk = n.min(LEAN_RC_INC_MAX);
                header(o).rc.fetch_sub(chunk as i32, Ordering::Relaxed);
                n -= chunk;
            }
        }
    }
}

#[inline(always)]
pub unsafe fn lean_inc_ref_n(o: Obj, n: usize) {
    unsafe {
        if n > LEAN_RC_INC_MAX {
            inc_ref_huge_n(o, n);
            return;
        }
        let rc = get_rc(o);
        if rc > 0 {
            // Wraps into the sticky range on overflow, as in `lean.h`.
            set_rc(o, (rc as u32).wrapping_add(n as u32) as i32);
        } else if is_unstuck_mt(o) {
            header(o).rc.fetch_sub(n as i32, Ordering::Relaxed);
        }
    }
}

#[inline(always)]
pub unsafe fn lean_inc_ref(o: Obj) {
    unsafe { lean_inc_ref_n(o, 1) }
}

#[inline(always)]
pub unsafe fn lean_inc(o: Obj) {
    if !o.is_scalar() {
        unsafe { lean_inc_ref(o) }
    }
}

#[inline(always)]
pub unsafe fn lean_inc_n(o: Obj, n: usize) {
    if !o.is_scalar() {
        unsafe { lean_inc_ref_n(o, n) }
    }
}

#[inline(always)]
pub unsafe fn lean_dec_ref(o: Obj) {
    unsafe {
        let rc = get_rc(o);
        if rc > 1 {
            set_rc(o, rc - 1);
        } else if rc != 0 {
            lean_dec_ref_cold(o);
        }
    }
}

#[inline(always)]
pub unsafe fn lean_dec(o: Obj) {
    if !o.is_scalar() {
        unsafe { lean_dec_ref(o) }
    }
}

/// Frees `o`, which is exclusive, after releasing the first `objs` object fields.
#[inline]
pub unsafe fn lean_dec_ref_known(o: Obj, objs: u32) {
    unsafe {
        if lean_is_exclusive(o) {
            for i in 0..objs {
                lean_dec(lean_ctor_get(o, i));
            }
            lean_free_object(o);
        } else {
            lean_dec_ref(o);
        }
    }
}

#[cold]
#[inline(never)]
pub unsafe fn lean_dec_ref_cold(o: Obj) {
    unsafe {
        if get_rc(o) != 1 {
            if is_never_freed(o) {
                return;
            }
            if header(o).rc.fetch_add(1, Ordering::AcqRel) != -1 {
                return;
            }
        }
        let mut todo: Vec<Obj> = Vec::new();
        let mut o = o;
        loop {
            del_core(o, &mut todo);
            match todo.pop() {
                Some(next) => o = next,
                None => return,
            }
        }
    }
}

#[inline(always)]
unsafe fn dec_into(o: Obj, todo: &mut Vec<Obj>) {
    unsafe {
        if o.is_scalar() {
            return;
        }
        let rc = get_rc(o);
        if rc > 1 {
            set_rc(o, rc - 1);
        } else if rc == 1 {
            todo.push(o);
        } else if is_never_freed(o) {
        } else if header(o).rc.fetch_add(1, Ordering::AcqRel) == -1 {
            todo.push(o);
        }
    }
}

/// Deletes `o`, whose reference count reached zero, queueing children that also reach zero.
/// Deletion is iterative so that long chains of objects do not exhaust the stack.
unsafe fn del_core(o: Obj, todo: &mut Vec<Obj>) {
    unsafe {
        let tag = lean_ptr_tag(o);
        if tag <= LEAN_MAX_CTOR_TAG {
            let n = lean_ctor_num_objs(o);
            let fields = lean_ctor_obj_cptr(o);
            for i in 0..n as usize {
                dec_into(*fields.add(i), todo);
            }
            dealloc_object(o);
            return;
        }
        match tag {
            LEAN_CLOSURE => {
                let n = lean_closure_num_fixed(o);
                let args = lean_closure_arg_cptr(o);
                for i in 0..n as usize {
                    dec_into(*args.add(i), todo);
                }
                dealloc_object(o);
            }
            LEAN_ARRAY => {
                let n = lean_array_size(o);
                let data = lean_array_cptr(o);
                for i in 0..n {
                    dec_into(*data.add(i), todo);
                }
                dealloc_object(o);
            }
            LEAN_SCALAR_ARRAY | LEAN_STRING => dealloc_object(o),
            LEAN_MPZ => lean_free_object(o),
            LEAN_THUNK => {
                let t = o.0 as *mut ThunkObject;
                let c = Obj((*t).closure.load(Ordering::Acquire));
                if !c.is_null() {
                    dec_into(c, todo);
                }
                let v = Obj((*t).value.load(Ordering::Acquire));
                if !v.is_null() {
                    dec_into(v, todo);
                }
                dealloc_object(o);
            }
            LEAN_REF => {
                let v = (*(o.0 as *mut RefObject)).value;
                if !v.is_null() {
                    dec_into(v, todo);
                }
                dealloc_object(o);
            }
            LEAN_TASK => crate::task::deactivate_task(o),
            LEAN_PROMISE => crate::task::deactivate_promise(o),
            LEAN_EXTERNAL => {
                let e = o.0 as *mut ExternalObject;
                ((*(*e).class).finalize)((*e).data);
                dealloc_object(o);
            }
            _ => lean_internal_panic_unreachable(),
        }
    }
}

/// Objects directly referenced by `o`, for the traversals that mark objects persistent or
/// multi-threaded.
unsafe fn for_each_child(o: Obj, mut f: impl FnMut(Obj)) {
    unsafe {
        let tag = lean_ptr_tag(o);
        if tag <= LEAN_MAX_CTOR_TAG {
            let fields = lean_ctor_obj_cptr(o);
            for i in 0..lean_ctor_num_objs(o) as usize {
                f(*fields.add(i));
            }
            return;
        }
        match tag {
            LEAN_SCALAR_ARRAY | LEAN_STRING | LEAN_MPZ => {}
            LEAN_EXTERNAL => {
                let e = o.0 as *mut ExternalObject;
                ((*(*e).class).for_each)((*e).data, &mut f);
            }
            LEAN_TASK => f(crate::task::lean_task_get(o)),
            LEAN_PROMISE => f(crate::task::promise_result_task(o)),
            LEAN_CLOSURE => {
                let args = lean_closure_arg_cptr(o);
                for i in 0..lean_closure_num_fixed(o) as usize {
                    f(*args.add(i));
                }
            }
            LEAN_ARRAY => {
                let data = lean_array_cptr(o);
                for i in 0..lean_array_size(o) {
                    f(*data.add(i));
                }
            }
            LEAN_THUNK => {
                let t = o.0 as *mut ThunkObject;
                let c = Obj((*t).closure.load(Ordering::Acquire));
                if !c.is_null() {
                    f(c);
                }
                let v = Obj((*t).value.load(Ordering::Acquire));
                if !v.is_null() {
                    f(v);
                }
            }
            LEAN_REF => {
                let v = (*(o.0 as *mut RefObject)).value;
                if !v.is_null() {
                    f(v);
                }
            }
            _ => lean_internal_panic_unreachable(),
        }
    }
}

/// Marks `o` and everything reachable from it as persistent: never freed, never counted.
pub unsafe fn lean_mark_persistent(o: Obj) {
    unsafe {
        let mut todo = vec![o];
        while let Some(o) = todo.pop() {
            if !o.is_scalar() && get_rc(o) != 0 {
                set_rc(o, 0);
                for_each_child(o, |c| todo.push(c));
            }
        }
    }
}

#[inline(always)]
unsafe fn is_unshared(o: Obj) -> bool {
    unsafe {
        let rc = get_rc(o);
        rc > 0 || rc <= LEAN_RC_STUCK_ST
    }
}

/// Marks `o` and everything reachable from it as shared between threads.
pub unsafe fn lean_mark_mt(o: Obj) {
    unsafe {
        if o.is_scalar() || !is_unshared(o) {
            return;
        }
        let mut todo = vec![o];
        while let Some(o) = todo.pop() {
            if !o.is_scalar() && is_unshared(o) {
                let rc = get_rc(o);
                let new_rc = if rc < 0 || -rc <= LEAN_RC_STICKY_DROP { LEAN_RC_STICKY } else { -rc };
                set_rc(o, new_rc);
                for_each_child(o, |c| todo.push(c));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Constructor objects
// ---------------------------------------------------------------------------------------------

#[inline(always)]
pub unsafe fn lean_ctor_num_objs(o: Obj) -> u32 {
    unsafe { lean_ptr_other(o) }
}

#[inline(always)]
pub unsafe fn lean_ctor_obj_cptr(o: Obj) -> *mut Obj {
    unsafe { (o.0 as *mut u8).add(size_of::<Object>()) as *mut Obj }
}

#[inline(always)]
pub unsafe fn lean_ctor_scalar_cptr(o: Obj) -> *mut u8 {
    unsafe { lean_ctor_obj_cptr(o).add(lean_ctor_num_objs(o) as usize) as *mut u8 }
}

#[inline(always)]
pub unsafe fn lean_alloc_ctor(tag: u32, num_objs: u32, scalar_sz: usize) -> Obj {
    debug_assert!(
        tag <= LEAN_MAX_CTOR_TAG as u32 && num_objs < LEAN_MAX_CTOR_FIELDS && scalar_sz < LEAN_MAX_CTOR_SCALARS_SIZE
    );
    unsafe {
        let o = lean_alloc_small_object(size_of::<Object>() + size_of::<Obj>() * num_objs as usize + scalar_sz);
        lean_set_st_header(o, tag as u8, num_objs as u8);
        o
    }
}

#[inline(always)]
pub unsafe fn lean_ctor_get(o: Obj, i: u32) -> Obj {
    unsafe { *lean_ctor_obj_cptr(o).add(i as usize) }
}

#[inline(always)]
pub unsafe fn lean_ctor_set(o: Obj, i: u32, v: Obj) {
    unsafe { *lean_ctor_obj_cptr(o).add(i as usize) = v }
}

#[inline(always)]
pub unsafe fn lean_ctor_set_tag(o: Obj, tag: u8) {
    unsafe { (*o.0).tag = tag }
}

#[inline(always)]
pub unsafe fn lean_ctor_release(o: Obj, i: u32) {
    unsafe {
        let slot = lean_ctor_obj_cptr(o).add(i as usize);
        lean_dec(*slot);
        *slot = lean_box(0);
    }
}

#[inline(always)]
pub unsafe fn lean_ctor_get_usize(o: Obj, i: u32) -> usize {
    unsafe { (lean_ctor_obj_cptr(o).add(i as usize) as *const usize).read_unaligned() }
}

#[inline(always)]
pub unsafe fn lean_ctor_set_usize(o: Obj, i: u32, v: usize) {
    unsafe { (lean_ctor_obj_cptr(o).add(i as usize) as *mut usize).write_unaligned(v) }
}

macro_rules! scalar_field {
    ($get:ident, $set:ident, $t:ty) => {
        #[inline(always)]
        pub unsafe fn $get(o: Obj, offset: usize) -> $t {
            unsafe { ((lean_ctor_obj_cptr(o) as *const u8).add(offset) as *const $t).read_unaligned() }
        }
        #[inline(always)]
        pub unsafe fn $set(o: Obj, offset: usize, v: $t) {
            unsafe { ((lean_ctor_obj_cptr(o) as *mut u8).add(offset) as *mut $t).write_unaligned(v) }
        }
    };
}

scalar_field!(lean_ctor_get_uint8, lean_ctor_set_uint8, u8);
scalar_field!(lean_ctor_get_uint16, lean_ctor_set_uint16, u16);
scalar_field!(lean_ctor_get_uint32, lean_ctor_set_uint32, u32);
scalar_field!(lean_ctor_get_uint64, lean_ctor_set_uint64, u64);
scalar_field!(lean_ctor_get_float, lean_ctor_set_float, f64);
scalar_field!(lean_ctor_get_float32, lean_ctor_set_float32, f32);

// ---------------------------------------------------------------------------------------------
// Closures
// ---------------------------------------------------------------------------------------------

#[repr(C)]
pub struct ClosureObject {
    header: Object,
    fun: *const (),
    arity: u16,
    num_fixed: u16,
    objs: [Obj; 0],
}

#[inline(always)]
pub unsafe fn lean_closure_fun(o: Obj) -> *const () {
    unsafe { (*(o.0 as *mut ClosureObject)).fun }
}

#[inline(always)]
pub unsafe fn lean_closure_arity(o: Obj) -> u32 {
    unsafe { (*(o.0 as *mut ClosureObject)).arity as u32 }
}

#[inline(always)]
pub unsafe fn lean_closure_num_fixed(o: Obj) -> u32 {
    unsafe { (*(o.0 as *mut ClosureObject)).num_fixed as u32 }
}

#[inline(always)]
pub unsafe fn lean_closure_arg_cptr(o: Obj) -> *mut Obj {
    unsafe { ptr::addr_of_mut!((*(o.0 as *mut ClosureObject)).objs) as *mut Obj }
}

/// Allocates a closure over `fun`, a Rust function taking `arity` boxed arguments (or, for
/// arities above [`LEAN_CLOSURE_MAX_ARGS`], a pointer to the argument array).
#[inline]
pub unsafe fn lean_alloc_closure(fun: *const (), arity: u32, num_fixed: u32) -> Obj {
    debug_assert!(arity > 0 && num_fixed < arity);
    unsafe {
        let o = lean_alloc_object(size_of::<ClosureObject>() + size_of::<Obj>() * num_fixed as usize);
        lean_set_st_header(o, LEAN_CLOSURE, 0);
        let c = o.0 as *mut ClosureObject;
        (*c).fun = fun;
        (*c).arity = arity as u16;
        (*c).num_fixed = num_fixed as u16;
        o
    }
}

#[inline(always)]
pub unsafe fn lean_closure_get(o: Obj, i: u32) -> Obj {
    unsafe { *lean_closure_arg_cptr(o).add(i as usize) }
}

#[inline(always)]
pub unsafe fn lean_closure_set(o: Obj, i: u32, a: Obj) {
    unsafe { *lean_closure_arg_cptr(o).add(i as usize) = a }
}

// ---------------------------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------------------------

#[repr(C)]
pub struct ArrayObject {
    header: Object,
    size: usize,
    capacity: usize,
    data: [Obj; 0],
}

#[inline]
pub unsafe fn lean_alloc_array(size: usize, capacity: usize) -> Obj {
    let bytes = capacity
        .checked_mul(size_of::<Obj>())
        .and_then(|b| b.checked_add(size_of::<ArrayObject>()))
        .unwrap_or_else(|| lean_internal_panic_overflow());
    unsafe {
        let o = lean_alloc_object(bytes);
        lean_set_st_header(o, LEAN_ARRAY, 0);
        let a = o.0 as *mut ArrayObject;
        (*a).size = size;
        (*a).capacity = capacity;
        o
    }
}

#[inline(always)]
pub unsafe fn lean_array_size(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut ArrayObject)).size }
}

#[inline(always)]
pub unsafe fn lean_array_capacity(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut ArrayObject)).capacity }
}

#[inline(always)]
pub unsafe fn lean_array_set_size(o: Obj, size: usize) {
    unsafe { (*(o.0 as *mut ArrayObject)).size = size }
}

#[inline(always)]
pub unsafe fn lean_array_cptr(o: Obj) -> *mut Obj {
    unsafe { ptr::addr_of_mut!((*(o.0 as *mut ArrayObject)).data) as *mut Obj }
}

#[inline(always)]
pub unsafe fn lean_array_get_core(o: Obj, i: usize) -> Obj {
    unsafe { *lean_array_cptr(o).add(i) }
}

#[inline(always)]
pub unsafe fn lean_array_set_core(o: Obj, i: usize, v: Obj) {
    unsafe { *lean_array_cptr(o).add(i) = v }
}

#[repr(C)]
pub struct SArrayObject {
    header: Object,
    size: usize,
    capacity: usize,
    data: [u8; 0],
}

#[inline]
pub unsafe fn lean_alloc_sarray(elem_size: u32, size: usize, capacity: usize) -> Obj {
    let bytes = capacity
        .checked_mul(elem_size as usize)
        .and_then(|b| b.checked_add(size_of::<SArrayObject>()))
        .unwrap_or_else(|| lean_internal_panic_overflow());
    unsafe {
        let o = lean_alloc_object(bytes);
        lean_set_st_header(o, LEAN_SCALAR_ARRAY, elem_size as u8);
        let a = o.0 as *mut SArrayObject;
        (*a).size = size;
        (*a).capacity = capacity;
        o
    }
}

#[inline(always)]
pub unsafe fn lean_sarray_elem_size(o: Obj) -> u32 {
    unsafe { lean_ptr_other(o) }
}

#[inline(always)]
pub unsafe fn lean_sarray_size(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut SArrayObject)).size }
}

#[inline(always)]
pub unsafe fn lean_sarray_capacity(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut SArrayObject)).capacity }
}

#[inline(always)]
pub unsafe fn lean_sarray_set_size(o: Obj, size: usize) {
    unsafe { (*(o.0 as *mut SArrayObject)).size = size }
}

#[inline(always)]
pub unsafe fn lean_sarray_cptr(o: Obj) -> *mut u8 {
    unsafe { ptr::addr_of_mut!((*(o.0 as *mut SArrayObject)).data) as *mut u8 }
}

// ---------------------------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------------------------

#[repr(C)]
pub struct StringObject {
    header: Object,
    /// Byte length including the terminating NUL.
    size: usize,
    capacity: usize,
    /// Length in Unicode scalar values.
    length: usize,
    data: [u8; 0],
}

#[inline]
pub unsafe fn lean_alloc_string(size: usize, capacity: usize, len: usize) -> Obj {
    let bytes = capacity.checked_add(size_of::<StringObject>()).unwrap_or_else(|| lean_internal_panic_overflow());
    unsafe {
        let o = lean_alloc_object(bytes);
        lean_set_st_header(o, LEAN_STRING, 0);
        let s = o.0 as *mut StringObject;
        (*s).size = size;
        (*s).capacity = capacity;
        (*s).length = len;
        o
    }
}

#[inline(always)]
pub unsafe fn lean_string_size(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut StringObject)).size }
}

#[inline(always)]
pub unsafe fn lean_string_capacity(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut StringObject)).capacity }
}

#[inline(always)]
pub unsafe fn lean_string_len(o: Obj) -> usize {
    unsafe { (*(o.0 as *mut StringObject)).length }
}

#[inline(always)]
pub unsafe fn lean_string_set_size_len(o: Obj, size: usize, len: usize) {
    unsafe {
        (*(o.0 as *mut StringObject)).size = size;
        (*(o.0 as *mut StringObject)).length = len;
    }
}

#[inline(always)]
pub unsafe fn lean_string_cstr(o: Obj) -> *mut u8 {
    unsafe { ptr::addr_of_mut!((*(o.0 as *mut StringObject)).data) as *mut u8 }
}

/// The UTF-8 contents of a string object (without the terminating NUL).
#[inline(always)]
pub unsafe fn lean_string_bytes<'a>(o: Obj) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(lean_string_cstr(o), lean_string_size(o) - 1) }
}

#[inline(always)]
pub unsafe fn lean_string_str<'a>(o: Obj) -> &'a str {
    unsafe { std::str::from_utf8_unchecked(lean_string_bytes(o)) }
}

/// Creates a string object from valid UTF-8 bytes with `len` scalar values.
pub unsafe fn lean_mk_string_unchecked(s: &[u8], len: usize) -> Obj {
    unsafe {
        let rsz = s.len() + 1;
        let r = lean_alloc_string(rsz, rsz, len);
        ptr::copy_nonoverlapping(s.as_ptr(), lean_string_cstr(r), s.len());
        *lean_string_cstr(r).add(s.len()) = 0;
        r
    }
}

/// Creates a string object from `s`.
pub fn lean_mk_string(s: &str) -> Obj {
    unsafe { lean_mk_string_unchecked(s.as_bytes(), s.chars().count()) }
}

/// Creates a string object from arbitrary bytes, replacing invalid UTF-8 sequences with
/// U+FFFD exactly as Lean's `lean_mk_string_from_bytes` does.
pub fn lean_mk_string_from_bytes(bytes: &[u8]) -> Obj {
    lean_mk_string(&crate::utf8::decode_lossy(bytes))
}

// ---------------------------------------------------------------------------------------------
// Big numbers
// ---------------------------------------------------------------------------------------------

#[repr(C)]
pub struct MpzObject {
    header: Object,
    value: BigInt,
}

/// Allocates a big-number object. Canonical representation (small values as scalars) is the
/// responsibility of the numeric primitives.
pub fn alloc_mpz(value: BigInt) -> Obj {
    unsafe {
        let o = lean_alloc_small_object(size_of::<MpzObject>());
        lean_set_st_header(o, LEAN_MPZ, 0);
        ptr::addr_of_mut!((*(o.0 as *mut MpzObject)).value).write(value);
        o
    }
}

#[inline(always)]
pub unsafe fn mpz_value<'a>(o: Obj) -> &'a BigInt {
    unsafe { &(*(o.0 as *mut MpzObject)).value }
}

// ---------------------------------------------------------------------------------------------
// Thunks and references
// ---------------------------------------------------------------------------------------------

#[repr(C)]
pub struct ThunkObject {
    header: Object,
    pub(crate) value: AtomicPtr<Object>,
    pub(crate) closure: AtomicPtr<Object>,
}

#[repr(C)]
pub struct RefObject {
    header: Object,
    pub(crate) value: Obj,
}

pub unsafe fn lean_ref_value_ptr(o: Obj) -> *mut Obj {
    unsafe { ptr::addr_of_mut!((*(o.0 as *mut RefObject)).value) }
}

pub unsafe fn lean_thunk_ptr(o: Obj) -> *mut ThunkObject {
    o.0 as *mut ThunkObject
}

pub unsafe fn alloc_ref(value: Obj) -> Obj {
    unsafe {
        let o = lean_alloc_small_object(size_of::<RefObject>());
        lean_set_st_header(o, LEAN_REF, 0);
        (*(o.0 as *mut RefObject)).value = value;
        o
    }
}

pub unsafe fn alloc_thunk(value: Obj, closure: Obj) -> Obj {
    unsafe {
        let o = lean_alloc_small_object(size_of::<ThunkObject>());
        lean_set_st_header(o, LEAN_THUNK, 0);
        let t = o.0 as *mut ThunkObject;
        ptr::addr_of_mut!((*t).value).write(AtomicPtr::new(value.0));
        ptr::addr_of_mut!((*t).closure).write(AtomicPtr::new(closure.0));
        o
    }
}

// ---------------------------------------------------------------------------------------------
// External objects
// ---------------------------------------------------------------------------------------------

/// The class of an external object: how to finalize its data and visit the Lean objects it
/// references.
pub struct ExternalClass {
    pub finalize: unsafe fn(*mut ()),
    pub for_each: unsafe fn(*mut (), &mut dyn FnMut(Obj)),
}

#[repr(C)]
pub struct ExternalObject {
    header: Object,
    class: *const ExternalClass,
    data: *mut (),
}

pub unsafe fn lean_alloc_external(class: &'static ExternalClass, data: *mut ()) -> Obj {
    unsafe {
        let o = lean_alloc_small_object(size_of::<ExternalObject>());
        lean_set_st_header(o, LEAN_EXTERNAL, 0);
        let e = o.0 as *mut ExternalObject;
        (*e).class = class;
        (*e).data = data;
        o
    }
}

pub unsafe fn lean_get_external_class(o: Obj) -> *const ExternalClass {
    unsafe { (*(o.0 as *mut ExternalObject)).class }
}

pub unsafe fn lean_get_external_data(o: Obj) -> *mut () {
    unsafe { (*(o.0 as *mut ExternalObject)).data }
}

/// Allocates a raw object for runtime-internal structures (tasks, promises), with `tag`.
pub unsafe fn alloc_raw(size: usize, tag: u8) -> Obj {
    unsafe {
        let o = lean_alloc_object(size);
        lean_set_st_header(o, tag, 0);
        o
    }
}

/// Releases the memory of a runtime-internal object without inspecting its tag.
pub unsafe fn dealloc_raw(o: Obj) {
    unsafe { dealloc_object(o) }
}

// ---------------------------------------------------------------------------------------------
// Boxing
// ---------------------------------------------------------------------------------------------

#[inline(always)]
pub unsafe fn lean_box_uint32(v: u32) -> Obj {
    if usize::BITS == 32 {
        unsafe {
            let r = lean_alloc_ctor(0, 0, 4);
            lean_ctor_set_uint32(r, 0, v);
            r
        }
    } else {
        lean_box(v as usize)
    }
}

#[inline(always)]
pub unsafe fn lean_unbox_uint32(o: Obj) -> u32 {
    if usize::BITS == 32 { unsafe { lean_ctor_get_uint32(o, 0) } } else { lean_unbox(o) as u32 }
}

#[inline(always)]
pub unsafe fn lean_box_uint64(v: u64) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 0, 8);
        lean_ctor_set_uint64(r, 0, v);
        r
    }
}

#[inline(always)]
pub unsafe fn lean_unbox_uint64(o: Obj) -> u64 {
    unsafe { lean_ctor_get_uint64(o, 0) }
}

#[inline(always)]
pub unsafe fn lean_box_usize(v: usize) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 0, size_of::<usize>());
        lean_ctor_set_usize(r, 0, v);
        r
    }
}

#[inline(always)]
pub unsafe fn lean_unbox_usize(o: Obj) -> usize {
    unsafe { lean_ctor_get_usize(o, 0) }
}

#[inline(always)]
pub unsafe fn lean_box_float(v: f64) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 0, 8);
        lean_ctor_set_float(r, 0, v);
        r
    }
}

#[inline(always)]
pub unsafe fn lean_unbox_float(o: Obj) -> f64 {
    unsafe { lean_ctor_get_float(o, 0) }
}

#[inline(always)]
pub unsafe fn lean_box_float32(v: f32) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 0, 4);
        lean_ctor_set_float32(r, 0, v);
        r
    }
}

#[inline(always)]
pub unsafe fn lean_unbox_float32(o: Obj) -> f32 {
    unsafe { lean_ctor_get_float32(o, 0) }
}

// ---------------------------------------------------------------------------------------------
// Option
// ---------------------------------------------------------------------------------------------

/// `Option.none`.
#[inline(always)]
pub fn lean_mk_option_none() -> Obj {
    lean_box(0)
}

/// `Option.some v`, consuming `v`.
#[inline(always)]
pub unsafe fn lean_mk_option_some(v: Obj) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(1, 1, 0);
        lean_ctor_set(r, 0, v);
        r
    }
}

// ---------------------------------------------------------------------------------------------
// IO results
// ---------------------------------------------------------------------------------------------

#[inline(always)]
pub unsafe fn lean_io_result_mk_ok(a: Obj) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(0, 1, 0);
        lean_ctor_set(r, 0, a);
        r
    }
}

#[inline(always)]
pub unsafe fn lean_io_result_mk_error(e: Obj) -> Obj {
    unsafe {
        let r = lean_alloc_ctor(1, 1, 0);
        lean_ctor_set(r, 0, e);
        r
    }
}

#[inline(always)]
pub unsafe fn lean_io_result_is_ok(r: Obj) -> bool {
    unsafe { lean_ptr_tag(r) == 0 }
}

#[inline(always)]
pub unsafe fn lean_io_result_is_error(r: Obj) -> bool {
    unsafe { lean_ptr_tag(r) == 1 }
}

#[inline(always)]
pub unsafe fn lean_io_result_get_value(r: Obj) -> Obj {
    unsafe { lean_ctor_get(r, 0) }
}

#[inline(always)]
pub unsafe fn lean_io_result_get_error(r: Obj) -> Obj {
    unsafe { lean_ctor_get(r, 0) }
}

#[inline(always)]
pub unsafe fn lean_io_result_take_value(r: Obj) -> Obj {
    unsafe {
        let v = lean_ctor_get(r, 0);
        lean_inc(v);
        lean_dec(r);
        v
    }
}

// ---------------------------------------------------------------------------------------------
// Internal panics
// ---------------------------------------------------------------------------------------------

/// Reports a violated runtime invariant and terminates the process, as Lean's runtime does.
#[cold]
pub fn lean_internal_panic(msg: &str) -> ! {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "INTERNAL PANIC: {msg}");
    if std::env::var_os("LEAN_ABORT_ON_PANIC").is_some() {
        std::process::abort();
    }
    std::process::exit(1)
}

#[cold]
pub fn lean_internal_panic_out_of_memory() -> ! {
    lean_internal_panic("out of memory")
}

#[cold]
pub fn lean_internal_panic_unreachable() -> ! {
    lean_internal_panic("unreachable code has been reached")
}

#[cold]
pub fn lean_internal_panic_rc_overflow() -> ! {
    lean_internal_panic("reference counter overflowed")
}

#[cold]
pub fn lean_internal_panic_overflow() -> ! {
    lean_internal_panic("integer overflow in runtime computation")
}
