//! `ST.Ref` primitives, ported from Lean's `runtime/io.cpp`.
//!
//! References that are shared between threads — marked multi-threaded, or persistent because
//! they were created during initialization — are accessed with atomic exchanges, and values
//! stored into them are marked multi-threaded, since a single-threaded object must never be
//! reachable from a multi-threaded one.

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

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_st_mk_ref(a: obj) -> obj {
            alloc_ref(a)
        }

        fn lean_st_ref_get(r: b_obj) -> obj {
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
                        if !tmp.is_null() {
                            // Another thread wrote the reference meanwhile.
                            lean_dec(Obj::from_raw(tmp));
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

        fn lean_st_ref_set(r: b_obj, a: obj) -> obj {
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

        fn lean_st_ref_swap(r: b_obj, a: obj) -> obj {
            if maybe_mt(r) {
                lean_mark_mt(a);
                let slot = value_slot(r);
                loop {
                    let old = slot.swap(a.ptr(), Ordering::AcqRel);
                    if !old.is_null() {
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

    #[test]
    fn get_set_swap_take() {
        unsafe {
            let r = lean_st_mk_ref(lean_mk_string("a"));
            let v = lean_st_ref_get(r);
            assert_eq!(lean_string_str(v), "a");
            lean_dec(v);
            lean_st_ref_set(r, lean_mk_string("b"));
            let old = lean_st_ref_swap(r, lean_mk_string("c"));
            assert_eq!(lean_string_str(old), "b");
            lean_dec(old);
            let taken = lean_st_ref_take(r);
            assert_eq!(lean_string_str(taken), "c");
            // `take` leaves the reference empty until it is set again.
            lean_st_ref_set(r, taken);
            let r2 = lean_st_mk_ref(lean_box(0));
            assert_eq!(lean_st_ref_ptr_eq(r, r), 1);
            assert_eq!(lean_st_ref_ptr_eq(r, r2), 0);
            lean_dec(r);
            lean_dec(r2);
        }
    }

    #[test]
    fn values_stored_in_shared_references_become_shared() {
        unsafe {
            let r = lean_st_mk_ref(lean_box(0));
            lean_mark_mt(r);
            let v = lean_mk_string("shared");
            lean_st_ref_set(r, v);
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
                            lean_st_ref_set(r, lean_mk_string(&format!("t{i}")));
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
}
