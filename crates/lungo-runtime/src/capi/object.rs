//! Objects: allocation, the slow paths of reference counting, closure application, boxing and
//! literals.

use crate::apply::lean_apply_n;
use crate::nat::{lean_cstr_to_nat, lean_usize_to_nat};
use crate::object::*;
use std::ffi::{CStr, c_char};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_alloc_ctor(tag: u32, num_objs: u32, scalar_sz: usize) -> Obj {
    if tag > LEAN_MAX_CTOR_TAG as u32 || num_objs >= LEAN_MAX_CTOR_FIELDS || scalar_sz >= LEAN_MAX_CTOR_SCALARS_SIZE {
        lean_internal_panic(&format!(
            "invalid constructor allocation (tag {tag}, {num_objs} object fields, {scalar_sz} scalar bytes)"
        ));
    }
    unsafe { lean_alloc_ctor(tag, num_objs, scalar_sz) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_alloc_closure(fun: *const (), arity: u32, num_fixed: u32) -> Obj {
    if fun.is_null() || arity == 0 || num_fixed >= arity {
        lean_internal_panic(&format!("invalid closure allocation (arity {arity}, {num_fixed} fixed arguments)"));
    }
    unsafe { lean_alloc_closure(fun, arity, num_fixed) }
}

/// The slow path of `lungo_dec_ref`: the object is shared between threads or reaches zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_dec_ref_cold(o: Obj) {
    unsafe { lean_dec_ref_cold(o) }
}

/// The slow path of `lungo_inc_ref_n` for increments above `LUNGO_RC_INC_MAX`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_inc_ref_n_cold(o: Obj, n: usize) {
    unsafe { lean_inc_ref_n(o, n) }
}

/// Releases the memory of `o` (not a scalar) without releasing the objects it references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_free_object(o: Obj) {
    unsafe { lean_free_object(o) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_mark_persistent(o: Obj) {
    unsafe { lean_mark_persistent(o) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_mark_mt(o: Obj) {
    unsafe { lean_mark_mt(o) }
}

macro_rules! apply_k {
    ($($name:ident($($a:ident),+);)*) => {
        $(
            #[unsafe(no_mangle)]
            #[allow(clippy::too_many_arguments)]
            pub unsafe extern "C" fn $name(f: Obj, $($a: Obj),+) -> Obj {
                unsafe { lean_apply_n(f, &[$($a),+]) }
            }
        )*
    };
}

apply_k! {
    lungo_apply_1(a1);
    lungo_apply_2(a1, a2);
    lungo_apply_3(a1, a2, a3);
    lungo_apply_4(a1, a2, a3, a4);
    lungo_apply_5(a1, a2, a3, a4, a5);
    lungo_apply_6(a1, a2, a3, a4, a5, a6);
    lungo_apply_7(a1, a2, a3, a4, a5, a6, a7);
    lungo_apply_8(a1, a2, a3, a4, a5, a6, a7, a8);
    lungo_apply_9(a1, a2, a3, a4, a5, a6, a7, a8, a9);
    lungo_apply_10(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
    lungo_apply_11(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
    lungo_apply_12(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    lungo_apply_13(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
    lungo_apply_14(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    lungo_apply_15(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);
    lungo_apply_16(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16);
}

/// Applies `f` to the `n` arguments at `args` (more than 16), consuming `f` and the arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_apply_m(f: Obj, n: usize, args: *const Obj) -> Obj {
    unsafe { lean_apply_n(f, std::slice::from_raw_parts(args, n)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_box_uint32(v: u32) -> Obj {
    unsafe { lean_box_uint32(v) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_box_uint64(v: u64) -> Obj {
    unsafe { lean_box_uint64(v) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_box_usize(v: usize) -> Obj {
    unsafe { lean_box_usize(v) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_box_float(v: f64) -> Obj {
    unsafe { lean_box_float(v) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_box_float32(v: f32) -> Obj {
    unsafe { lean_box_float32(v) }
}

/// A string object from `size` bytes of valid UTF-8 at `bytes`, which encode `len` scalar
/// values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_mk_string_unchecked(bytes: *const u8, size: usize, len: usize) -> Obj {
    unsafe { lean_mk_string_unchecked(std::slice::from_raw_parts(bytes, size), len) }
}

#[unsafe(no_mangle)]
pub extern "C" fn lungo_usize_to_nat(n: usize) -> Obj {
    lean_usize_to_nat(n)
}

/// The `Nat` of the decimal numeral `digits` (NUL-terminated ASCII digits).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_cstr_to_nat(digits: *const c_char) -> Obj {
    let digits = unsafe { CStr::from_ptr(digits) }
        .to_str()
        .unwrap_or_else(|_| lean_internal_panic("a Nat literal is not ASCII"));
    lean_cstr_to_nat(digits)
}

#[unsafe(no_mangle)]
pub extern "C" fn lungo_panic_unreachable() -> ! {
    lean_internal_panic_unreachable()
}
