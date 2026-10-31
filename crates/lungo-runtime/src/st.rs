//! `ST.Ref` primitives, ported from Lean's `runtime/io.cpp`.
//!
//! References that are shared between threads — marked multi-threaded, or persistent because
//! they were created during initialization — are accessed with atomic exchanges, and values
//! stored into them are marked multi-threaded, since a single-threaded object must never be
//! reachable from a multi-threaded one.
//!
//! Since Lean 4.35.0 a reference is empty only between a `take` and the `put` that refills it
//! (`Ref.modify`); a shared reference's `get` and `swap` wait while it is empty, and `Ref.set` is
//! a `swap` whose old value is dropped. Up to 4.34.1, `Ref.set` (`lean_st_ref_set`) both refills
//! a taken reference and overwrites a full one, so it can write a shared reference while a `get`
//! holds its value; that `get` then drops the value written.

use crate::object::*;
use std::sync::atomic::{AtomicPtr, Ordering};

unsafe fn value_slot<'a>(r: Obj) -> &'a AtomicPtr<Object> {
    // `Obj` is a transparent pointer, so the value field can be accessed atomically.
    unsafe { &*(lean_ref_value_ptr(r) as *const AtomicPtr<Object>) }
}

#[inline]
unsafe fn maybe_mt(r: Obj) -> bool {
    unsafe { lean_is_mt(r) || lean_is_persistent(r) }
}

/// What `get` finds in a shared reference when it puts the value back.
#[derive(Clone, Copy)]
enum Meanwhile {
    /// Nothing: writers wait while the reference is empty (Lean 4.35.0 on).
    Empty,
    /// Possibly a value `Ref.set` wrote, which the read supersedes (Lean up to 4.34.1).
    MaybeWritten,
}

unsafe fn get(r: Obj, meanwhile: Meanwhile) -> Obj {
    unsafe {
        if maybe_mt(r) {
            let slot = value_slot(r);
            loop {
                // Take the reference's token, duplicate it, and put one back: reading and
                // incrementing in place could race with a writer dropping the last reference.
                let v = slot.swap(std::ptr::null_mut(), Ordering::AcqRel);
                if !v.is_null() {
                    let v = Obj::from_raw(v);
                    lean_inc(v);
                    let tmp = slot.swap(v.ptr(), Ordering::AcqRel);
                    match meanwhile {
                        Meanwhile::Empty => {
                            if !tmp.is_null() {
                                lean_internal_panic("a shared reference was written while it was empty");
                            }
                        }
                        Meanwhile::MaybeWritten => {
                            if !tmp.is_null() {
                                lean_dec(Obj::from_raw(tmp));
                            }
                        }
                    }
                    return v;
                }
                std::hint::spin_loop();
            }
        } else {
            let v = *lean_ref_value_ptr(r);
            if v.is_null() {
                lean_internal_panic("null reference read");
            }
            lean_inc(v);
            v
        }
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_st_mk_ref(a: obj) -> obj {
            alloc_ref(a)
        }

        fn lean_st_ref_get(r: b_obj) -> obj since(4, 35, 0) {
            get(r, Meanwhile::Empty)
        }

        fn lean_st_ref_get_v4_34_1(r: b_obj) -> obj as lean_st_ref_get until(4, 34, 1) {
            get(r, Meanwhile::MaybeWritten)
        }

        /// Writes the reference, full or emptied by `take`, dropping the value it held.
        fn lean_st_ref_set(r: b_obj, a: obj) -> obj until(4, 34, 1) {
            if maybe_mt(r) {
                lean_mark_mt(a);
                let old = value_slot(r).swap(a.ptr(), Ordering::AcqRel);
                if !old.is_null() {
                    lean_dec(Obj::from_raw(old));
                }
            } else {
                let p = lean_ref_value_ptr(r);
                if !(*p).is_null() {
                    lean_dec(*p);
                }
                *p = a;
            }
            lean_box(0)
        }

        fn lean_st_ref_take(r: b_obj) -> obj {
            if maybe_mt(r) {
                let slot = value_slot(r);
                loop {
                    let v = slot.swap(std::ptr::null_mut(), Ordering::AcqRel);
                    if !v.is_null() {
                        return Obj::from_raw(v);
                    }
                    std::hint::spin_loop();
                }
            } else {
                let p = lean_ref_value_ptr(r);
                let v = *p;
                if v.is_null() {
                    lean_internal_panic("null reference read");
                }
                *p = Obj::null();
                v
            }
        }

        /// Refills a reference emptied by `take`.
        fn lean_st_ref_put(r: b_obj, a: obj) -> obj since(4, 35, 0) {
            if maybe_mt(r) {
                lean_mark_mt(a);
                let old = value_slot(r).swap(a.ptr(), Ordering::AcqRel);
                if !old.is_null() {
                    lean_internal_panic("a shared reference was refilled while it was full");
                }
            } else {
                let p = lean_ref_value_ptr(r);
                if !(*p).is_null() {
                    lean_dec(*p);
                }
                *p = a;
            }
            lean_box(0)
        }

        fn lean_st_ref_swap(r: b_obj, a: obj) -> obj {
            if maybe_mt(r) {
                lean_mark_mt(a);
                let slot = value_slot(r);
                loop {
                    // Only a full reference is swapped: one emptied by `take` waits for its `put`.
                    let old = slot.load(Ordering::Acquire);
                    if !old.is_null()
                        && slot.compare_exchange(old, a.ptr(), Ordering::AcqRel, Ordering::Acquire).is_ok()
                    {
                        return Obj::from_raw(old);
                    }
                    std::hint::spin_loop();
                }
            } else {
                let p = lean_ref_value_ptr(r);
                let old = *p;
                if old.is_null() {
                    lean_internal_panic("null reference read");
                }
                *p = a;
                old
            }
        }

        fn lean_st_ref_ptr_eq(r1: b_obj, r2: b_obj) -> u8 {
            (r1 == r2) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use crate::object::*;

    /// TEST0203: get swap take put
    #[test]
    fn test0203_get_swap_take_put() {
        unsafe {
            let r = lean_st_mk_ref(lean_mk_string("a"));
            let v = lean_st_ref_get(r);
            assert_eq!(lean_string_str(v), "a");
            lean_dec(v);
            lean_dec(lean_st_ref_swap(r, lean_mk_string("b")));
            let old = lean_st_ref_swap(r, lean_mk_string("c"));
            assert_eq!(lean_string_str(old), "b");
            lean_dec(old);
            let taken = lean_st_ref_take(r);
            assert_eq!(lean_string_str(taken), "c");
            // `take` leaves the reference empty until `put` refills it.
            lean_st_ref_put(r, taken);
            let r2 = lean_st_mk_ref(lean_box(0));
            assert_eq!(lean_st_ref_ptr_eq(r, r), 1);
            assert_eq!(lean_st_ref_ptr_eq(r, r2), 0);
            lean_dec(r);
            lean_dec(r2);
        }
    }

    /// TEST0204: values stored in shared references become shared
    #[test]
    fn test0204_values_stored_in_shared_references_become_shared() {
        unsafe {
            let r = lean_st_mk_ref(lean_box(0));
            lean_mark_mt(r);
            let v = lean_mk_string("shared");
            lean_dec(lean_st_ref_swap(r, v));
            let got = lean_st_ref_get(r);
            assert!(lean_is_mt(got));
            assert_eq!(lean_string_str(got), "shared");
            lean_dec(got);
            let v = std::sync::Arc::new(crate::object::SendObj(r));
            let threads: Vec<_> = (0..4)
                .map(|i| {
                    let v = v.clone();
                    std::thread::spawn(move || {
                        let r = v.0;
                        for _ in 0..1000 {
                            let s = lean_st_ref_get(r);
                            lean_dec(s);
                            lean_dec(lean_st_ref_swap(r, lean_mk_string(&format!("t{i}"))));
                            let taken = lean_st_ref_take(r);
                            lean_st_ref_put(r, taken);
                        }
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            let last = lean_st_ref_get(r);
            assert!(lean_string_str(last).starts_with('t'));
            lean_dec(last);
            lean_dec(r);
        }
    }

    /// TEST0381: up to Lean 4.34.1, `Ref.set` overwrites a shared reference while other threads
    /// read it, and refills one emptied by `take`
    #[test]
    fn test0381_up_to_lean_4_34_1_set_overwrites_a_shared_reference_while_others_read_it() {
        unsafe {
            let r = lean_st_mk_ref(lean_mk_string("start"));
            lean_mark_mt(r);
            let v = std::sync::Arc::new(crate::object::SendObj(r));
            let threads: Vec<_> = (0..4)
                .map(|i| {
                    let v = v.clone();
                    std::thread::spawn(move || {
                        let r = v.0;
                        for n in 0..2000 {
                            if i % 2 == 0 {
                                // A blind write: 4.34.1's `Ref.set`.
                                lean_st_ref_set(r, lean_mk_string(&format!("t{i}-{n}")));
                            } else {
                                let s = lean_st_ref_get_v4_34_1(r);
                                assert!(lean_string_str(s).starts_with('t') || lean_string_str(s) == "start");
                                lean_dec(s);
                            }
                        }
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            // `Ref.modify` up to 4.34.1: `take`, then `set` refills.
            let taken = lean_st_ref_take(r);
            lean_dec(taken);
            lean_st_ref_set(r, lean_mk_string("refilled"));
            let got = lean_st_ref_get_v4_34_1(r);
            assert_eq!(lean_string_str(got), "refilled");
            lean_dec(got);
            lean_dec(r);
        }
    }
}
