//! Conversions between facade Rust types and Lean runtime objects.
//!
//! Every facade type converts to and from Lean's *boxed* representation: the uniform object
//! representation used for polymorphic positions, closure arguments, and object fields.
//! Generated code converts values at unboxed (scalar) positions by boxing or unboxing according
//! to the representation Lean's compiler assigns to the position. Objects are read through
//! Lean's object layout and allocated through the [`Backend`] that owns them.

use crate::backend::{Backend, RustBackend};
use crate::{ByteArray, FloatArray, Int, List, Nat};
use lungo_runtime::{self as rt, Obj};
use num_bigint::{BigInt, BigUint};

/// A Rust type with a Lean runtime representation in the runtime `B`.
///
/// # Safety
///
/// `into_lean` must return an owned object in the boxed representation of the corresponding
/// Lean type, and `from_lean` must accept every borrowed object of that representation.
pub unsafe trait LeanType<B: Backend = RustBackend>: Sized {
    /// Converts into an owned object in boxed representation.
    fn into_lean(self) -> Obj;

    /// Reads a value from a borrowed object in boxed representation.
    ///
    /// # Safety
    ///
    /// `o` must be a valid value of the corresponding Lean type.
    unsafe fn from_lean(o: Obj) -> Self;
}

pub(crate) unsafe fn box_u32<B: Backend>(v: u32) -> Obj {
    if usize::BITS == 32 {
        unsafe {
            let o = B::alloc_ctor(0, 0, 4);
            rt::lean_ctor_set_uint32(o, 0, v);
            o
        }
    } else {
        rt::lean_box(v as usize)
    }
}

pub(crate) unsafe fn box_u64<B: Backend>(v: u64) -> Obj {
    unsafe {
        let o = B::alloc_ctor(0, 0, 8);
        rt::lean_ctor_set_uint64(o, 0, v);
        o
    }
}

pub(crate) unsafe fn box_usize<B: Backend>(v: usize) -> Obj {
    unsafe {
        let o = B::alloc_ctor(0, 0, size_of::<usize>());
        rt::lean_ctor_set_usize(o, 0, v);
        o
    }
}

pub(crate) unsafe fn box_f64<B: Backend>(v: f64) -> Obj {
    unsafe {
        let o = B::alloc_ctor(0, 0, 8);
        rt::lean_ctor_set_float(o, 0, v);
        o
    }
}

pub(crate) unsafe fn box_f32<B: Backend>(v: f32) -> Obj {
    unsafe {
        let o = B::alloc_ctor(0, 0, 4);
        rt::lean_ctor_set_float32(o, 0, v);
        o
    }
}

unsafe impl<B: Backend> LeanType<B> for () {
    fn into_lean(self) -> Obj {
        rt::lean_box(0)
    }
    unsafe fn from_lean(_: Obj) -> Self {}
}

unsafe impl<B: Backend> LeanType<B> for bool {
    fn into_lean(self) -> Obj {
        rt::lean_box(self as usize)
    }
    unsafe fn from_lean(o: Obj) -> Self {
        rt::lean_unbox(o) != 0
    }
}

macro_rules! small_scalar {
    ($t:ty, $u:ty) => {
        unsafe impl<B: Backend> LeanType<B> for $t {
            fn into_lean(self) -> Obj {
                rt::lean_box(self as $u as usize)
            }
            unsafe fn from_lean(o: Obj) -> Self {
                rt::lean_unbox(o) as $u as $t
            }
        }
    };
}
small_scalar!(u8, u8);
small_scalar!(u16, u16);
small_scalar!(i8, u8);
small_scalar!(i16, u16);

macro_rules! boxed_scalar {
    ($t:ty, $box:ident, $unbox:ident, $via:ty) => {
        unsafe impl<B: Backend> LeanType<B> for $t {
            fn into_lean(self) -> Obj {
                unsafe { $box::<B>(self as $via) }
            }
            unsafe fn from_lean(o: Obj) -> Self {
                unsafe { rt::$unbox(o) as $t }
            }
        }
    };
}
boxed_scalar!(u32, box_u32, lean_unbox_uint32, u32);
boxed_scalar!(i32, box_u32, lean_unbox_uint32, u32);
boxed_scalar!(u64, box_u64, lean_unbox_uint64, u64);
boxed_scalar!(i64, box_u64, lean_unbox_uint64, u64);
boxed_scalar!(usize, box_usize, lean_unbox_usize, usize);
boxed_scalar!(isize, box_usize, lean_unbox_usize, usize);

unsafe impl<B: Backend> LeanType<B> for f64 {
    fn into_lean(self) -> Obj {
        unsafe { box_f64::<B>(self) }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { rt::lean_unbox_float(o) }
    }
}

unsafe impl<B: Backend> LeanType<B> for f32 {
    fn into_lean(self) -> Obj {
        unsafe { box_f32::<B>(self) }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { rt::lean_unbox_float32(o) }
    }
}

unsafe impl<B: Backend> LeanType<B> for char {
    fn into_lean(self) -> Obj {
        unsafe { box_u32::<B>(self as u32) }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        let code = unsafe { rt::lean_unbox_uint32(o) };
        char::from_u32(code)
            .unwrap_or_else(|| rt::lean_internal_panic(&format!("Lean Char {code:#x} is not a Unicode scalar value")))
    }
}

unsafe impl<B: Backend> LeanType<B> for String {
    fn into_lean(self) -> Obj {
        B::mk_string(&self)
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { rt::lean_string_str(o).to_owned() }
    }
}

unsafe impl<B: Backend> LeanType<B> for Nat {
    fn into_lean(self) -> Obj {
        match self.to_u64() {
            Some(v) if v as u128 <= rt::LEAN_MAX_SMALL_NAT as u128 => rt::lean_box(v as usize),
            _ => B::nat_from_big(&BigUint::from(self)),
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        if o.is_scalar() { Nat::from(rt::lean_unbox(o)) } else { Nat::from_biguint(unsafe { B::nat_to_big(o) }) }
    }
}

unsafe impl<B: Backend> LeanType<B> for Int {
    fn into_lean(self) -> Obj {
        match self.to_i64() {
            Some(v) if (rt::LEAN_MIN_SMALL_INT..=rt::LEAN_MAX_SMALL_INT).contains(&v) => rt::int::lean_int64_to_int(v),
            _ => B::int_from_big(&BigInt::from(self)),
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        if o.is_scalar() {
            Int::from(rt::int::lean_scalar_to_int64(o))
        } else {
            Int::from_bigint(unsafe { B::int_to_big(o) })
        }
    }
}

unsafe impl<B: Backend, T: LeanType<B>> LeanType<B> for Option<T> {
    fn into_lean(self) -> Obj {
        match self {
            None => rt::lean_box(0),
            Some(v) => unsafe {
                let o = B::alloc_ctor(1, 1, 0);
                rt::lean_ctor_set(o, 0, v.into_lean());
                o
            },
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { if o.is_scalar() { None } else { Some(T::from_lean(rt::lean_ctor_get(o, 0))) } }
    }
}

unsafe impl<B: Backend, X: LeanType<B>, Y: LeanType<B>> LeanType<B> for (X, Y) {
    fn into_lean(self) -> Obj {
        unsafe {
            let o = B::alloc_ctor(0, 2, 0);
            rt::lean_ctor_set(o, 0, self.0.into_lean());
            rt::lean_ctor_set(o, 1, self.1.into_lean());
            o
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { (X::from_lean(rt::lean_ctor_get(o, 0)), Y::from_lean(rt::lean_ctor_get(o, 1))) }
    }
}

/// `Except ε α`: `error` has tag 0 and `ok` has tag 1.
unsafe impl<B: Backend, X: LeanType<B>, E: LeanType<B>> LeanType<B> for Result<X, E> {
    fn into_lean(self) -> Obj {
        unsafe {
            let (tag, v) = match self {
                Err(e) => (0, e.into_lean()),
                Ok(a) => (1, a.into_lean()),
            };
            let o = B::alloc_ctor(tag, 1, 0);
            rt::lean_ctor_set(o, 0, v);
            o
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe {
            let v = rt::lean_ctor_get(o, 0);
            match rt::lean_ptr_tag(o) {
                0 => Err(E::from_lean(v)),
                1 => Ok(X::from_lean(v)),
                t => rt::lean_internal_panic(&format!("invalid Except constructor tag {t}")),
            }
        }
    }
}

/// `Array α`.
unsafe impl<B: Backend, T: LeanType<B>> LeanType<B> for Vec<T> {
    fn into_lean(self) -> Obj {
        unsafe {
            let n = self.len();
            let a = B::alloc_array(n, n);
            let data = rt::lean_array_cptr(a);
            for (i, v) in self.into_iter().enumerate() {
                *data.add(i) = v.into_lean();
            }
            a
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe {
            let data = rt::lean_array_cptr(o);
            (0..rt::lean_array_size(o)).map(|i| T::from_lean(*data.add(i))).collect()
        }
    }
}

/// `List α`: `nil` is `box(0)`, `cons` has tag 1 and two fields.
unsafe impl<B: Backend, T: LeanType<B>> LeanType<B> for List<T> {
    fn into_lean(self) -> Obj {
        let mut out = rt::lean_box(0);
        for v in self.0.into_iter().rev() {
            unsafe {
                let cell = B::alloc_ctor(1, 2, 0);
                rt::lean_ctor_set(cell, 0, v.into_lean());
                rt::lean_ctor_set(cell, 1, out);
                out = cell;
            }
        }
        out
    }
    unsafe fn from_lean(o: Obj) -> Self {
        let mut out = Vec::new();
        let mut cur = o;
        unsafe {
            while !cur.is_scalar() {
                out.push(T::from_lean(rt::lean_ctor_get(cur, 0)));
                cur = rt::lean_ctor_get(cur, 1);
            }
        }
        List(out)
    }
}

unsafe impl<B: Backend> LeanType<B> for ByteArray {
    fn into_lean(self) -> Obj {
        unsafe {
            let n = self.0.len();
            let a = B::alloc_sarray(1, n, n);
            std::ptr::copy_nonoverlapping(self.0.as_ptr(), rt::lean_sarray_cptr(a), n);
            a
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { ByteArray(std::slice::from_raw_parts(rt::lean_sarray_cptr(o), rt::lean_sarray_size(o)).to_vec()) }
    }
}

unsafe impl<B: Backend> LeanType<B> for FloatArray {
    fn into_lean(self) -> Obj {
        unsafe {
            let n = self.0.len();
            let a = B::alloc_sarray(8, n, n);
            let data = rt::lean_sarray_cptr(a) as *mut f64;
            for (i, v) in self.0.iter().enumerate() {
                data.add(i).write_unaligned(*v);
            }
            a
        }
    }
    unsafe fn from_lean(o: Obj) -> Self {
        unsafe {
            let data = rt::lean_sarray_cptr(o) as *const f64;
            FloatArray((0..rt::lean_sarray_size(o)).map(|i| data.add(i).read_unaligned()).collect())
        }
    }
}

/// Helpers used by generated facades.
pub mod facade {
    use super::*;

    /// Converts `v` and unboxes it to the representation Lean's compiler uses at a `u8`/`u16`
    /// position.
    pub fn to_small<B: Backend, T: LeanType<B>>(v: T) -> usize {
        let o = v.into_lean();
        let r = rt::lean_unbox(o);
        unsafe { B::dec(o) };
        r
    }

    pub fn to_u32<B: Backend, T: LeanType<B>>(v: T) -> u32 {
        unsafe {
            let o = v.into_lean();
            let r = rt::lean_unbox_uint32(o);
            B::dec(o);
            r
        }
    }

    pub fn to_u64<B: Backend, T: LeanType<B>>(v: T) -> u64 {
        unsafe {
            let o = v.into_lean();
            let r = rt::lean_unbox_uint64(o);
            B::dec(o);
            r
        }
    }

    pub fn to_usize<B: Backend, T: LeanType<B>>(v: T) -> usize {
        unsafe {
            let o = v.into_lean();
            let r = rt::lean_unbox_usize(o);
            B::dec(o);
            r
        }
    }

    pub fn to_f64<B: Backend, T: LeanType<B>>(v: T) -> f64 {
        unsafe {
            let o = v.into_lean();
            let r = rt::lean_unbox_float(o);
            B::dec(o);
            r
        }
    }

    pub fn to_f32<B: Backend, T: LeanType<B>>(v: T) -> f32 {
        unsafe {
            let o = v.into_lean();
            let r = rt::lean_unbox_float32(o);
            B::dec(o);
            r
        }
    }

    /// Reads a value from its unboxed `u8`/`u16` representation.
    ///
    /// # Safety
    ///
    /// `v` must be a valid unboxed representation of a `T`.
    pub unsafe fn from_small<B: Backend, T: LeanType<B>>(v: usize) -> T {
        unsafe { T::from_lean(rt::lean_box(v)) }
    }

    /// Reads a value from its unboxed `u32` representation.
    ///
    /// # Safety
    ///
    /// `v` must be a valid unboxed representation of a `T`.
    pub unsafe fn from_u32<B: Backend, T: LeanType<B>>(v: u32) -> T {
        unsafe {
            let o = box_u32::<B>(v);
            let r = T::from_lean(o);
            B::dec(o);
            r
        }
    }

    /// Reads a value from its unboxed `u64` representation.
    ///
    /// # Safety
    ///
    /// `v` must be a valid unboxed representation of a `T`.
    pub unsafe fn from_u64<B: Backend, T: LeanType<B>>(v: u64) -> T {
        unsafe {
            let o = box_u64::<B>(v);
            let r = T::from_lean(o);
            B::dec(o);
            r
        }
    }

    /// Reads a value from its unboxed `usize` representation.
    ///
    /// # Safety
    ///
    /// `v` must be a valid unboxed representation of a `T`.
    pub unsafe fn from_usize<B: Backend, T: LeanType<B>>(v: usize) -> T {
        unsafe {
            let o = box_usize::<B>(v);
            let r = T::from_lean(o);
            B::dec(o);
            r
        }
    }

    /// Reads a value from its unboxed `Float` representation.
    ///
    /// # Safety
    ///
    /// `v` must be a valid unboxed representation of a `T`.
    pub unsafe fn from_f64<B: Backend, T: LeanType<B>>(v: f64) -> T {
        unsafe {
            let o = box_f64::<B>(v);
            let r = T::from_lean(o);
            B::dec(o);
            r
        }
    }

    /// Reads a value from its unboxed `Float32` representation.
    ///
    /// # Safety
    ///
    /// `v` must be a valid unboxed representation of a `T`.
    pub unsafe fn from_f32<B: Backend, T: LeanType<B>>(v: f32) -> T {
        unsafe {
            let o = box_f32::<B>(v);
            let r = T::from_lean(o);
            B::dec(o);
            r
        }
    }

    /// Reads a value from an owned object and releases the object.
    ///
    /// # Safety
    ///
    /// `o` must be a live object of `B` representing a `T`, owned by the caller.
    pub unsafe fn take<B: Backend, T: LeanType<B>>(o: Obj) -> T {
        unsafe {
            let r = T::from_lean(o);
            B::dec(o);
            r
        }
    }

    /// An async program (`Lungo.Async.Program`) a generated async function drives: done with a
    /// value, or waiting for the host to perform an operation. Dropped before it is done, it
    /// releases the program, so a future dropped mid-operation leaves nothing behind.
    pub struct AsyncProgram<B: Backend> {
        obj: Obj,
        _backend: core::marker::PhantomData<fn() -> B>,
    }

    // The program object is marked as shared between threads before it is held here.
    unsafe impl<B: Backend> Send for AsyncProgram<B> {}

    /// The layout of `Lungo.Async.Program`, which lungo's worker checks against Lean's compiler:
    /// `done` (tag 0) holds the value in object field 0, `call` (tag 1) the operation in object
    /// field 0 and the continuation in object field 1.
    const DONE_TAG: u8 = 0;
    const CALL_TAG: u8 = 1;

    impl<B: Backend> AsyncProgram<B> {
        /// Takes the program `o`.
        ///
        /// # Safety
        ///
        /// `o` must be a live `Lungo.Async.Program` of `B`, owned by the caller.
        pub unsafe fn new(o: Obj) -> Self {
            unsafe { B::mark_mt(o) };
            AsyncProgram { obj: o, _backend: core::marker::PhantomData }
        }

        fn tag(&self) -> u8 {
            if rt::lean_is_scalar(self.obj) {
                panic!("lungo: an async program is a scalar: its layout is not the one lungo reads");
            }
            match unsafe { rt::lean_ptr_tag(self.obj) } {
                t @ (DONE_TAG | CALL_TAG) => t,
                t => panic!("lungo: an async program has constructor tag {t}: its layout is not the one lungo reads"),
            }
        }

        /// Whether the program is done.
        pub fn is_done(&self) -> bool {
            self.tag() == DONE_TAG
        }

        /// The program's value.
        ///
        /// # Safety
        ///
        /// The program must be done, with a value representing a `T`.
        pub unsafe fn value<T: LeanType<B>>(&self) -> T {
            debug_assert!(self.is_done());
            unsafe { T::from_lean(rt::lean_ctor_get(self.obj, 0)) }
        }

        /// The operation the program waits for.
        ///
        /// # Safety
        ///
        /// The program must wait for an operation representing an `O`.
        pub unsafe fn operation<O: LeanType<B>>(&self) -> O {
            debug_assert!(!self.is_done());
            unsafe { O::from_lean(rt::lean_ctor_get(self.obj, 0)) }
        }

        /// Continues the program with the answer to the operation it waits for.
        ///
        /// # Safety
        ///
        /// The program must wait for an operation answered with an `A`.
        pub unsafe fn resume<A: LeanType<B>>(&mut self, answer: A) {
            debug_assert!(!self.is_done());
            unsafe {
                let resume = rt::lean_ctor_get(self.obj, 1);
                B::inc(resume);
                B::dec(self.obj);
                let next = B::apply(resume, &[answer.into_lean()]);
                B::mark_mt(next);
                self.obj = next;
            }
        }
    }

    impl<B: Backend> Drop for AsyncProgram<B> {
        fn drop(&mut self) {
            unsafe { B::dec(self.obj) }
        }
    }

    /// Unpacks an owned `IO` result.
    ///
    /// # Safety
    ///
    /// `r` must be a live `EStateM.Result` of `B` whose value represents a `T`, owned by the
    /// caller.
    pub unsafe fn take_io<B: Backend, T: LeanType<B>>(r: Obj) -> Result<T, crate::IoError<B>> {
        unsafe {
            let out = if rt::lean_ptr_tag(r) == 0 {
                Ok(T::from_lean(rt::lean_ctor_get(r, 0)))
            } else {
                Err(<crate::IoError<B> as LeanType<B>>::from_lean(rt::lean_ctor_get(r, 0)))
            };
            B::dec(r);
            out
        }
    }

    /// Unpacks an owned `EIO ε` result.
    ///
    /// # Safety
    ///
    /// `r` must be a live `EStateM.Result` of `B` whose value represents a `T` and whose error
    /// represents an `E`, owned by the caller.
    pub unsafe fn take_eio<B: Backend, T: LeanType<B>, E: LeanType<B>>(r: Obj) -> Result<T, E> {
        unsafe {
            let out = if rt::lean_ptr_tag(r) == 0 {
                Ok(T::from_lean(rt::lean_ctor_get(r, 0)))
            } else {
                Err(E::from_lean(rt::lean_ctor_get(r, 0)))
            };
            B::dec(r);
            out
        }
    }

    unsafe fn io_result<B: Backend>(tag: u32, v: Obj) -> Obj {
        unsafe {
            let o = B::alloc_ctor(tag, 1, 0);
            rt::lean_ctor_set(o, 0, v);
            o
        }
    }

    /// Packs a Rust result as an owned `IO` result.
    pub fn make_io<B: Backend, T: LeanType<B>>(r: Result<T, crate::IoError<B>>) -> Obj {
        unsafe {
            match r {
                Ok(v) => io_result::<B>(0, v.into_lean()),
                Err(e) => io_result::<B>(1, <crate::IoError<B> as LeanType<B>>::into_lean(e)),
            }
        }
    }

    /// Packs a Rust result as an owned `EIO ε` result.
    pub fn make_eio<B: Backend, T: LeanType<B>, E: LeanType<B>>(r: Result<T, E>) -> Obj {
        unsafe {
            match r {
                Ok(v) => io_result::<B>(0, v.into_lean()),
                Err(e) => io_result::<B>(1, e.into_lean()),
            }
        }
    }

    /// Allocates a constructor object in the runtime `B`.
    ///
    /// # Safety
    ///
    /// As [`Backend::alloc_ctor`].
    pub unsafe fn alloc_ctor<B: Backend>(tag: u32, num_objs: u32, scalar_size: usize) -> Obj {
        unsafe { B::alloc_ctor(tag, num_objs, scalar_size) }
    }

    /// Releases `o` in the runtime `B`.
    ///
    /// # Safety
    ///
    /// As [`Backend::dec`].
    pub unsafe fn dec<B: Backend>(o: Obj) {
        unsafe { B::dec(o) }
    }
}
