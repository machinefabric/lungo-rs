//! Miscellaneous runtime primitives: `Name` equality, pointer inspection, runtime marking, and
//! debugging helpers, ported from `runtime/object.cpp` and `runtime/io.cpp`.

use crate::apply::lean_apply_1;
use crate::object::*;
use crate::string::lean_string_eq;

/// The hash stored in a non-anonymous `Name` (`lean_name_hash_ptr`).
#[inline]
unsafe fn name_hash_ptr(n: Obj) -> u64 {
    unsafe { lean_ctor_get_uint64(n, size_of::<Obj>() * 2) }
}

/// `lean_nat_eq` on the numeric components of names.
unsafe fn nat_eq(a: Obj, b: Obj) -> bool {
    unsafe {
        if a.is_scalar() && b.is_scalar() {
            a == b
        } else if a.is_scalar() != b.is_scalar() {
            false
        } else {
            mpz_value(a) == mpz_value(b)
        }
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_name_eq(n1: b_obj, n2: b_obj) -> u8 {
            let (mut n1, mut n2) = (n1, n2);
            if n1 == n2 {
                return 1;
            }
            if n1.is_scalar() != n2.is_scalar() || name_hash_ptr(n1) != name_hash_ptr(n2) {
                return 0;
            }
            loop {
                if lean_ptr_tag(n1) != lean_ptr_tag(n2) {
                    return 0;
                }
                let (c1, c2) = (lean_ctor_get(n1, 1), lean_ctor_get(n2, 1));
                let same = if lean_ptr_tag(n1) == 1 { lean_string_eq(c1, c2) } else { nat_eq(c1, c2) };
                if !same {
                    return 0;
                }
                n1 = lean_ctor_get(n1, 0);
                n2 = lean_ctor_get(n2, 0);
                if n1 == n2 {
                    return 1;
                }
                if n1.is_scalar() != n2.is_scalar() {
                    return 0;
                }
            }
        }

        fn lean_ptr_addr(a: b_obj) -> usize {
            a.addr()
        }

        fn lean_is_scalar(a: b_obj) -> u8 {
            a.is_scalar() as u8
        }

        fn lean_is_exclusive_obj(a: b_obj) -> u8 {
            crate::object::lean_is_exclusive_obj(a)
        }

        fn lean_strict_and(b1: u8, b2: u8) -> u8 {
            (b1 != 0 && b2 != 0) as u8
        }

        fn lean_strict_or(b1: u8, b2: u8) -> u8 {
            (b1 != 0 || b2 != 0) as u8
        }

        // `Runtime.forget`: the owned reference is intentionally never released.
        fn lean_runtime_forget(_o: obj) -> obj {
            lean_box(0)
        }

        fn lean_runtime_hold(_a: b_obj) -> obj {
            lean_box(0)
        }

        fn lean_runtime_mark_multi_threaded(a: obj) -> obj {
            lean_mark_mt(a);
            a
        }

        fn lean_runtime_mark_persistent(a: obj) -> obj {
            lean_mark_persistent(a);
            a
        }

        fn lean_void_mk(a: obj) -> obj {
            lean_dec(a);
            lean_box(0)
        }

        fn lean_dbg_trace(s: obj, f: obj) -> obj {
            crate::io::io_eprintln(lean_string_str(s));
            lean_dec(s);
            lean_apply_1(f, lean_box(0))
        }

        fn lean_dbg_sleep(ms: u32, f: obj) -> obj {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            lean_apply_1(f, lean_box(0))
        }

        fn lean_dbg_trace_if_shared(s: b_obj, a: obj) -> obj {
            if !a.is_scalar() && !crate::object::lean_is_exclusive(a) {
                crate::io::io_eprintln(&format!("shared RC {}", lean_string_str(s)));
            }
            a
        }

        fn lean_dbg_stack_trace(f: obj) -> obj {
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            for line in trace.lines() {
                crate::io::io_eprintln(line);
            }
            lean_apply_1(f, lean_box(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    /// Builds `Name.str p s` with the hash Lean 4.34.1 computes (`mixHash p.hash s.hash`).
    unsafe fn name_str(p: Obj, s: &str) -> Obj {
        unsafe {
            let ph = if p.is_scalar() { 1723 } else { name_hash_ptr(p) };
            let so = lean_mk_string(s);
            let h = crate::hash::mix_hash(ph, crate::hash::hash_str(s.as_bytes(), 11));
            let n = lean_alloc_ctor(1, 2, 8);
            lean_ctor_set(n, 0, p);
            lean_ctor_set(n, 1, so);
            lean_ctor_set_uint64(n, 16, h);
            n
        }
    }

    #[test]
    fn name_equality_is_structural() {
        unsafe {
            let a = name_str(name_str(lean_box(0), "Lean"), "Name");
            let b = name_str(name_str(lean_box(0), "Lean"), "Name");
            let c = name_str(name_str(lean_box(0), "Lean"), "Expr");
            assert_eq!(lean_name_eq(a, b), 1);
            assert_eq!(lean_name_eq(a, c), 0);
            assert_eq!(lean_name_eq(a, lean_box(0)), 0);
            assert_eq!(lean_name_eq(lean_box(0), lean_box(0)), 1);
            lean_dec(a);
            lean_dec(b);
            lean_dec(c);
        }
    }

    #[test]
    fn strict_boolean_operators() {
        unsafe {
            assert_eq!(lean_strict_and(1, 0), 0);
            assert_eq!(lean_strict_or(0, 1), 1);
            assert_eq!(externs::lean_is_scalar(lean_box(3)), 1);
        }
    }
}
