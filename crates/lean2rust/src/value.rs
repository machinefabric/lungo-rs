use crate::backend::{Backend, RustBackend};
use crate::convert::LeanType;
use lean2rust_runtime::{self as rt, Obj};
use std::fmt;
use std::marker::PhantomData;

/// Records the Lean type and backend of a [`LeanValue`] without owning either (the value is
/// `Send`/`Sync` as the object's thread-safe reference counting allows, independently of `T`).
type Marker<T, B> = PhantomData<(fn() -> T, fn() -> B)>;

/// The Rust function behind a closure: receives borrowed arguments, returns an owned result.
type RustFnBody = Box<dyn Fn(&[Obj]) -> Obj + Send + Sync>;

/// A Lean value without a plain Rust representation, such as a value of a dependent type or of
/// a type carrying proofs. It can be passed back to Lean functions; Rust code cannot fabricate
/// one, so the invariants Lean's types guarantee are preserved.
///
/// The type parameter identifies the Lean type. Values are shared between threads safely: the
/// underlying object is marked multi-threaded when the handle is created.
pub struct LeanValue<T: ?Sized, B: Backend = RustBackend> {
    obj: Obj,
    _marker: Marker<T, B>,
}

// The object is multi-threaded (atomically reference counted) or persistent.
unsafe impl<T: ?Sized, B: Backend> Send for LeanValue<T, B> {}
unsafe impl<T: ?Sized, B: Backend> Sync for LeanValue<T, B> {}

impl<T: ?Sized, B: Backend> LeanValue<T, B> {
    /// Takes ownership of `o`.
    #[doc(hidden)]
    pub unsafe fn from_owned(o: Obj) -> Self {
        unsafe { B::mark_mt(o) };
        LeanValue { obj: o, _marker: PhantomData }
    }

    /// Retains the borrowed object `o`.
    #[doc(hidden)]
    pub unsafe fn from_borrowed(o: Obj) -> Self {
        unsafe {
            B::inc(o);
            Self::from_owned(o)
        }
    }

    /// The underlying object, borrowed.
    #[doc(hidden)]
    pub fn as_obj(&self) -> Obj {
        self.obj
    }

    /// Releases ownership of the underlying object to the caller.
    #[doc(hidden)]
    pub fn into_obj(self) -> Obj {
        let o = self.obj;
        std::mem::forget(self);
        o
    }
}

impl<T: ?Sized, B: Backend> Clone for LeanValue<T, B> {
    fn clone(&self) -> Self {
        unsafe { B::inc(self.obj) };
        LeanValue { obj: self.obj, _marker: PhantomData }
    }
}

impl<T: ?Sized, B: Backend> Drop for LeanValue<T, B> {
    fn drop(&mut self) {
        unsafe { B::dec(self.obj) }
    }
}

impl<T: ?Sized, B: Backend> fmt::Debug for LeanValue<T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LeanValue<{}>", std::any::type_name::<T>())
    }
}

unsafe impl<T: ?Sized, B: Backend> LeanType<B> for LeanValue<T, B> {
    fn into_lean(self) -> Obj {
        self.into_obj()
    }

    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { Self::from_borrowed(o) }
    }
}

/// A Lean function value with signature `Sig` (written as a Rust function pointer type such
/// as `fn(Nat) -> Nat`). Lean closures can be called from Rust, and Rust closures can be passed
/// to Lean with `from_fn`.
pub struct LeanClosure<Sig, B: Backend = RustBackend> {
    value: LeanValue<Sig, B>,
}

impl<Sig, B: Backend> Clone for LeanClosure<Sig, B> {
    fn clone(&self) -> Self {
        LeanClosure { value: self.value.clone() }
    }
}

impl<Sig, B: Backend> fmt::Debug for LeanClosure<Sig, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LeanClosure<{}>", std::any::type_name::<Sig>())
    }
}

unsafe impl<Sig, B: Backend> LeanType<B> for LeanClosure<Sig, B> {
    fn into_lean(self) -> Obj {
        self.value.into_obj()
    }

    unsafe fn from_lean(o: Obj) -> Self {
        LeanClosure { value: unsafe { LeanValue::from_borrowed(o) } }
    }
}

/// A Rust function behind a closure created by `LeanClosure::from_fn`. It receives borrowed
/// arguments and returns an owned result.
struct RustFn<B: Backend> {
    f: RustFnBody,
    _backend: PhantomData<fn() -> B>,
}

unsafe extern "C" fn finalize_rust_fn<B: Backend>(data: *mut ()) {
    drop(unsafe { Box::from_raw(data as *mut RustFn<B>) });
}

/// Calls the Rust function stored in the external object `env` with `args`; both are owned by
/// the call, as closure arguments are in Lean's calling convention.
unsafe fn invoke<B: Backend>(env: Obj, args: &[Obj]) -> Obj {
    unsafe {
        let f = &*(B::external_data(env) as *const RustFn<B>);
        let r = (f.f)(args);
        for a in args {
            B::dec(*a);
        }
        B::dec(env);
        r
    }
}

impl<Sig, B: Backend> LeanClosure<Sig, B> {
    /// Wraps a Rust function as a Lean closure. The signature is inferred from the function:
    /// `LeanClosure::from_fn(|n: Nat| n)` is a `LeanClosure<fn(Nat) -> Nat>`.
    pub fn from_fn<F: RustClosure<Sig, B>>(f: F) -> Self {
        f.into_closure()
    }
}

/// Rust functions that can be wrapped as Lean closures of signature `Sig` (functions of one to
/// eight arguments whose argument and result types convert to Lean values).
pub trait RustClosure<Sig, B: Backend>: Send + Sync + 'static {
    #[doc(hidden)]
    fn into_closure(self) -> LeanClosure<Sig, B>;
}

macro_rules! closure_arity {
    ($tramp:ident, $n:expr, $($a:ident : $A:ident),+) => {
        unsafe extern "C" fn $tramp<B: Backend>(env: Obj, $($a: Obj),+) -> Obj {
            unsafe { invoke::<B>(env, &[$($a),+]) }
        }

        impl<B: Backend, $($A: LeanType<B>,)+ R: LeanType<B>> LeanClosure<fn($($A),+) -> R, B> {
            /// Applies the Lean function to the arguments.
            // Closures of up to eight arguments are supported, as by `from_fn`.
            #[allow(clippy::too_many_arguments)]
            pub fn call(&self, $($a: $A),+) -> R {
                unsafe {
                    let f = self.value.as_obj();
                    B::inc(f);
                    let r = B::apply(f, &[$($a.into_lean()),+]);
                    let v = R::from_lean(r);
                    B::dec(r);
                    v
                }
            }
        }

        impl<B: Backend, $($A: LeanType<B>,)+ R: LeanType<B>, F> RustClosure<fn($($A),+) -> R, B> for F
        where
            F: Fn($($A),+) -> R + Send + Sync + 'static,
        {
            fn into_closure(self) -> LeanClosure<fn($($A),+) -> R, B> {
                let rust = RustFn::<B> {
                    f: Box::new(move |args: &[Obj]| {
                        let mut it = args.iter();
                        $(let $a = unsafe { $A::from_lean(*it.next().expect("closure arity")) };)+
                        self($($a),+).into_lean()
                    }),
                    _backend: PhantomData,
                };
                unsafe {
                    let env = B::alloc_external(Box::into_raw(Box::new(rust)) as *mut (), finalize_rust_fn::<B>);
                    let c = B::alloc_closure($tramp::<B> as *const (), $n + 1, 1);
                    rt::lean_closure_set(c, 0, env);
                    LeanClosure { value: LeanValue::from_owned(c) }
                }
            }
        }
    };
}

closure_arity!(trampoline_1, 1, a1: A1);
closure_arity!(trampoline_2, 2, a1: A1, a2: A2);
closure_arity!(trampoline_3, 3, a1: A1, a2: A2, a3: A3);
closure_arity!(trampoline_4, 4, a1: A1, a2: A2, a3: A3, a4: A4);
closure_arity!(trampoline_5, 5, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5);
closure_arity!(trampoline_6, 6, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6);
closure_arity!(trampoline_7, 7, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7);
closure_arity!(trampoline_8, 8, a1: A1, a2: A2, a3: A3, a4: A4, a5: A5, a6: A6, a7: A7, a8: A8);
