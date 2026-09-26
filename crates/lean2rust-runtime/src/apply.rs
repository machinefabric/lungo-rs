//! Closure application, ported from Lean's `runtime/apply.cpp`.
//!
//! A closure stores a function pointer, its arity, and the arguments fixed so far. Applying a
//! closure to `n` arguments either saturates it (calling the function), over-saturates it
//! (calling the function and applying the result to the remaining arguments), or produces a new
//! closure with more fixed arguments. Functions with more than [`LEAN_CLOSURE_MAX_ARGS`]
//! parameters take their arguments as an array.

#![allow(clippy::missing_safety_doc)]

use crate::object::*;

type Fn1 = unsafe extern "C" fn(Obj) -> Obj;
type Fn2 = unsafe extern "C" fn(Obj, Obj) -> Obj;
type Fn3 = unsafe extern "C" fn(Obj, Obj, Obj) -> Obj;
type Fn4 = unsafe extern "C" fn(Obj, Obj, Obj, Obj) -> Obj;
type Fn5 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn6 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn7 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn8 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn9 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn10 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn11 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn12 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn13 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn14 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn15 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type Fn16 = unsafe extern "C" fn(Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj, Obj) -> Obj;
type FnN = unsafe extern "C" fn(*mut Obj) -> Obj;

/// Calls the closure function `f` of arity `n` with exactly `n` arguments.
///
/// `f` must have been created from an `extern "C"` function of the matching type: `n` object
/// parameters for `n <= 16`, otherwise a pointer to an argument array. Closure functions use
/// the C calling convention, as in Lean's runtime.
pub unsafe fn curry(f: *const (), args: &mut [Obj]) -> Obj {
    let a = args;
    unsafe {
        match a.len() {
            0 => lean_internal_panic_unreachable(),
            1 => std::mem::transmute::<*const (), Fn1>(f)(a[0]),
            2 => std::mem::transmute::<*const (), Fn2>(f)(a[0], a[1]),
            3 => std::mem::transmute::<*const (), Fn3>(f)(a[0], a[1], a[2]),
            4 => std::mem::transmute::<*const (), Fn4>(f)(a[0], a[1], a[2], a[3]),
            5 => std::mem::transmute::<*const (), Fn5>(f)(a[0], a[1], a[2], a[3], a[4]),
            6 => std::mem::transmute::<*const (), Fn6>(f)(a[0], a[1], a[2], a[3], a[4], a[5]),
            7 => std::mem::transmute::<*const (), Fn7>(f)(a[0], a[1], a[2], a[3], a[4], a[5], a[6]),
            8 => std::mem::transmute::<*const (), Fn8>(f)(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]),
            9 => std::mem::transmute::<*const (), Fn9>(f)(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]),
            10 => std::mem::transmute::<*const (), Fn10>(f)(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9]),
            11 => std::mem::transmute::<*const (), Fn11>(f)(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10],
            ),
            12 => std::mem::transmute::<*const (), Fn12>(f)(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11],
            ),
            13 => std::mem::transmute::<*const (), Fn13>(f)(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11], a[12],
            ),
            14 => std::mem::transmute::<*const (), Fn14>(f)(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11], a[12], a[13],
            ),
            15 => std::mem::transmute::<*const (), Fn15>(f)(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11], a[12], a[13], a[14],
            ),
            16 => std::mem::transmute::<*const (), Fn16>(f)(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11], a[12], a[13], a[14], a[15],
            ),
            _ => std::mem::transmute::<*const (), FnN>(f)(a.as_mut_ptr()),
        }
    }
}

/// A new closure with the arguments of `f` followed by `args`, consuming `f`.
unsafe fn fix_args(f: Obj, args: &[Obj]) -> Obj {
    unsafe {
        let arity = lean_closure_arity(f);
        let fixed = lean_closure_num_fixed(f);
        let new_fixed = fixed + args.len() as u32;
        debug_assert!(new_fixed < arity);
        let r = lean_alloc_closure(lean_closure_fun(f), arity, new_fixed);
        let source = lean_closure_arg_cptr(f);
        let target = lean_closure_arg_cptr(r);
        if !lean_is_exclusive(f) {
            for i in 0..fixed as usize {
                let v = *source.add(i);
                lean_inc(v);
                *target.add(i) = v;
            }
            lean_dec_ref(f);
        } else {
            for i in 0..fixed as usize {
                *target.add(i) = *source.add(i);
            }
            lean_free_object(f);
        }
        for (i, a) in args.iter().enumerate() {
            *target.add(fixed as usize + i) = *a;
        }
        r
    }
}

/// Applies the closure `f` to `args`, consuming `f` and the arguments.
pub unsafe fn lean_apply_n(f: Obj, args: &[Obj]) -> Obj {
    unsafe {
        if f.is_scalar() {
            // `f` is an erased proof.
            for a in args {
                lean_dec(*a);
            }
            return f;
        }
        let n = args.len() as u32;
        let arity = lean_closure_arity(f);
        let fixed = lean_closure_num_fixed(f);
        if arity == fixed + n {
            let mut all = Vec::with_capacity(arity as usize);
            let source = lean_closure_arg_cptr(f);
            let exclusive = lean_is_exclusive(f) && arity as usize <= LEAN_CLOSURE_MAX_ARGS;
            for i in 0..fixed as usize {
                let v = *source.add(i);
                if !exclusive {
                    lean_inc(v);
                }
                all.push(v);
            }
            all.extend_from_slice(args);
            let r = curry(lean_closure_fun(f), &mut all);
            if exclusive {
                // The fixed arguments were moved into the call.
                lean_free_object(f);
            } else {
                lean_dec_ref(f);
            }
            r
        } else if arity < fixed + n {
            let first = (arity - fixed) as usize;
            let mut all = Vec::with_capacity(arity as usize);
            let source = lean_closure_arg_cptr(f);
            for i in 0..fixed as usize {
                let v = *source.add(i);
                lean_inc(v);
                all.push(v);
            }
            all.extend_from_slice(&args[..first]);
            let new_f = curry(lean_closure_fun(f), &mut all);
            lean_dec_ref(f);
            lean_apply_n(new_f, &args[first..])
        } else {
            fix_args(f, args)
        }
    }
}

macro_rules! apply_k {
    ($name:ident, $($a:ident),+) => {
        // The arities mirror `lean.h`'s `lean_apply_1` .. `lean_apply_16`.
        #[allow(clippy::too_many_arguments)]
        #[inline]
        pub unsafe fn $name(f: Obj, $($a: Obj),+) -> Obj {
            unsafe { lean_apply_n(f, &[$($a),+]) }
        }
    };
}

apply_k!(lean_apply_1, a1);
apply_k!(lean_apply_2, a1, a2);
apply_k!(lean_apply_3, a1, a2, a3);
apply_k!(lean_apply_4, a1, a2, a3, a4);
apply_k!(lean_apply_5, a1, a2, a3, a4, a5);
apply_k!(lean_apply_6, a1, a2, a3, a4, a5, a6);
apply_k!(lean_apply_7, a1, a2, a3, a4, a5, a6, a7);
apply_k!(lean_apply_8, a1, a2, a3, a4, a5, a6, a7, a8);
apply_k!(lean_apply_9, a1, a2, a3, a4, a5, a6, a7, a8, a9);
apply_k!(lean_apply_10, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
apply_k!(lean_apply_11, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
apply_k!(lean_apply_12, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
apply_k!(lean_apply_13, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
apply_k!(lean_apply_14, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
apply_k!(lean_apply_15, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);
apply_k!(lean_apply_16, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16);

/// Applies `f` to more than 16 arguments.
pub unsafe fn lean_apply_m(f: Obj, args: &[Obj]) -> Obj {
    unsafe { lean_apply_n(f, args) }
}
