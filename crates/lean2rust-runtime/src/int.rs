//! Integers, ported from `lean.h` and the `Int` section of `runtime/object.cpp`.
//!
//! An `Int` is a tagged scalar when it lies in
//! [[`LEAN_MIN_SMALL_INT`], [`LEAN_MAX_SMALL_INT`]] and a big-number object otherwise. Scalars
//! use Lean's encoding `lean_box((unsigned)(int)n)`, so non-negative small integers share the
//! representation of the equal `Nat`.

use crate::nat::{lean_nat_succ, nat_from_bigint};
use crate::object::*;
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};

/// Boxes a C `int` as an `Int` scalar.
#[inline(always)]
fn box_int(n: i32) -> Obj {
    lean_box(n as u32 as usize)
}

/// The value of an `Int` scalar.
#[inline(always)]
pub fn lean_scalar_to_int64(a: Obj) -> i64 {
    lean_scalar_to_int(a) as i64
}

/// The value of an `Int` scalar as a C `int`.
#[inline(always)]
pub fn lean_scalar_to_int(a: Obj) -> i32 {
    if usize::BITS == 64 { lean_unbox(a) as u32 as i32 } else { (a.addr() as i32) >> 1 }
}

#[inline(always)]
pub fn lean_int64_to_int(n: i64) -> Obj {
    if (LEAN_MIN_SMALL_INT..=LEAN_MAX_SMALL_INT).contains(&n) { box_int(n as i32) } else { alloc_mpz(BigInt::from(n)) }
}

#[inline(always)]
pub fn lean_int_to_int(n: i32) -> Obj {
    lean_int64_to_int(n as i64)
}

/// Converts a big integer to a canonical `Int`.
pub fn int_from_bigint(v: BigInt) -> Obj {
    match v.to_i64() {
        Some(n) if (LEAN_MIN_SMALL_INT..=LEAN_MAX_SMALL_INT).contains(&n) => box_int(n as i32),
        _ => alloc_mpz(v),
    }
}

/// The value of the `Int` `o` (borrowed).
pub unsafe fn int_to_bigint(o: Obj) -> BigInt {
    if o.is_scalar() { BigInt::from(lean_scalar_to_int64(o)) } else { unsafe { mpz_value(o).clone() } }
}

/// Parses a decimal integer numeral into a canonical `Int`.
pub fn lean_cstr_to_int(s: &str) -> Obj {
    let v: BigInt = s.parse().unwrap_or_else(|_| lean_internal_panic(&format!("invalid Int literal {s:?}")));
    int_from_bigint(v)
}

/// Converts an owned, non-negative big `Int` to a `Nat`.
pub unsafe fn lean_big_int_to_nat(a: Obj) -> Obj {
    unsafe {
        let v = mpz_value(a).clone();
        lean_dec(a);
        nat_from_bigint(v)
    }
}

/// Converts an owned, non-negative `Int` to a `Nat`.
pub unsafe fn lean_int_to_nat(a: Obj) -> Obj {
    if a.is_scalar() {
        if lean_scalar_to_int(a) < 0 {
            lean_internal_panic("negative Int converted to Nat");
        }
        a
    } else {
        unsafe { lean_big_int_to_nat(a) }
    }
}

#[inline]
unsafe fn big(o: Obj) -> BigInt {
    unsafe { int_to_bigint(o) }
}

fn tdiv(n: &BigInt, d: &BigInt) -> BigInt {
    n / d
}

fn tmod(n: &BigInt, d: &BigInt) -> BigInt {
    n % d
}

/// Euclidean-style division as `mpz::ediv`: truncate, then adjust when the remainder is negative.
fn ediv(n: &BigInt, d: &BigInt) -> BigInt {
    let (q, r) = n.div_rem(d);
    if r.sign() == Sign::Minus { if d.sign() == Sign::Plus { q - 1 } else { q + 1 } } else { q }
}

fn emod(n: &BigInt, d: &BigInt) -> BigInt {
    let r = n % d;
    if r.sign() == Sign::Minus { if d.sign() == Sign::Plus { r + d } else { r - d } } else { r }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_nat_to_int(a: obj) -> obj {
            if a.is_scalar() {
                let v = lean_unbox(a);
                if v as u64 <= LEAN_MAX_SMALL_INT as u64 { a } else { alloc_mpz(BigInt::from(v)) }
            } else {
                a
            }
        }

        fn lean_nat_abs(i: b_obj) -> obj {
            if lean_int_dec_lt(i, lean_box(0)) != 0 {
                lean_int_to_nat(lean_int_neg(i))
            } else {
                lean_inc(i);
                lean_int_to_nat(i)
            }
        }

        fn lean_int_neg(a: b_obj) -> obj {
            if a.is_scalar() {
                lean_int64_to_int(-lean_scalar_to_int64(a))
            } else {
                int_from_bigint(-mpz_value(a))
            }
        }

        fn lean_int_neg_succ_of_nat(a: obj) -> obj {
            let s = lean_nat_succ(a);
            lean_dec(a);
            let i = lean_nat_to_int(s);
            let r = lean_int_neg(i);
            lean_dec(i);
            r
        }

        fn lean_int_add(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_int64_to_int(lean_scalar_to_int64(a1) + lean_scalar_to_int64(a2))
            } else {
                int_from_bigint(big(a1) + big(a2))
            }
        }

        fn lean_int_sub(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_int64_to_int(lean_scalar_to_int64(a1) - lean_scalar_to_int64(a2))
            } else {
                int_from_bigint(big(a1) - big(a2))
            }
        }

        fn lean_int_mul(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_int64_to_int(lean_scalar_to_int64(a1) * lean_scalar_to_int64(a2))
            } else {
                int_from_bigint(big(a1) * big(a2))
            }
        }

        fn lean_int_div(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (v1, v2) = (lean_scalar_to_int64(a1), lean_scalar_to_int64(a2));
                if v2 == 0 { lean_box(0) } else { lean_int64_to_int(v1 / v2) }
            } else {
                let d = big(a2);
                if d.is_zero() { lean_box(0) } else { int_from_bigint(tdiv(&big(a1), &d)) }
            }
        }

        fn lean_int_div_exact(a1: b_obj, a2: b_obj) -> obj {
            // `Int.divExact` assumes the divisor divides the dividend; truncating division then
            // computes the exact quotient.
            lean_int_div(a1, a2)
        }

        fn lean_int_mod(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (v1, v2) = (lean_scalar_to_int64(a1), lean_scalar_to_int64(a2));
                if v2 == 0 { a1 } else { lean_int64_to_int(v1 % v2) }
            } else {
                let d = big(a2);
                if d.is_zero() {
                    lean_inc(a1);
                    a1
                } else {
                    int_from_bigint(tmod(&big(a1), &d))
                }
            }
        }

        fn lean_int_ediv(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (n, d) = (lean_scalar_to_int64(a1), lean_scalar_to_int64(a2));
                if d == 0 {
                    lean_box(0)
                } else {
                    let mut q = n / d;
                    if n % d < 0 {
                        q = if d > 0 { q - 1 } else { q + 1 };
                    }
                    lean_int64_to_int(q)
                }
            } else {
                let d = big(a2);
                if d.is_zero() { lean_box(0) } else { int_from_bigint(ediv(&big(a1), &d)) }
            }
        }

        fn lean_int_emod(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (n, d) = (lean_scalar_to_int64(a1), lean_scalar_to_int64(a2));
                if d == 0 {
                    a1
                } else {
                    let mut r = n % d;
                    if r < 0 {
                        r = if d > 0 { r + d } else { r - d };
                    }
                    lean_int64_to_int(r)
                }
            } else {
                let d = big(a2);
                if d.is_zero() {
                    lean_inc(a1);
                    a1
                } else {
                    int_from_bigint(emod(&big(a1), &d))
                }
            }
        }

        fn lean_int_dec_eq(a1: b_obj, a2: b_obj) -> u8 {
            if a1.is_scalar() && a2.is_scalar() {
                (a1 == a2) as u8
            } else if a1.is_scalar() || a2.is_scalar() {
                // Canonical representation: a scalar never equals a big number.
                0
            } else {
                (mpz_value(a1) == mpz_value(a2)) as u8
            }
        }

        fn lean_int_dec_le(a1: b_obj, a2: b_obj) -> u8 {
            if a1.is_scalar() && a2.is_scalar() {
                (lean_scalar_to_int(a1) <= lean_scalar_to_int(a2)) as u8
            } else {
                (big(a1) <= big(a2)) as u8
            }
        }

        fn lean_int_dec_lt(a1: b_obj, a2: b_obj) -> u8 {
            if a1.is_scalar() && a2.is_scalar() {
                (lean_scalar_to_int(a1) < lean_scalar_to_int(a2)) as u8
            } else {
                (big(a1) < big(a2)) as u8
            }
        }

        fn lean_int_dec_nonneg(a: b_obj) -> u8 {
            if a.is_scalar() {
                (lean_scalar_to_int(a) >= 0) as u8
            } else {
                (mpz_value(a).sign() != Sign::Minus) as u8
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    fn int(s: &str) -> Obj {
        lean_cstr_to_int(s)
    }

    fn show(o: Obj) -> String {
        unsafe { int_to_bigint(o).to_string() }
    }

    #[test]
    fn small_range_boundaries_are_canonical() {
        assert!(int("2147483647").is_scalar());
        assert!(!int("2147483648").is_scalar());
        assert!(int("-2147483648").is_scalar());
        assert!(!int("-2147483649").is_scalar());
        unsafe {
            let r = lean_int_sub(int("2147483648"), lean_box(1));
            assert!(r.is_scalar());
            assert_eq!(show(r), "2147483647");
            let n = lean_int_neg(int("-2147483648"));
            assert!(!n.is_scalar());
            assert_eq!(show(n), "2147483648");
            // Non-negative small integers share the `Nat` scalar representation.
            assert_eq!(int("5"), lean_box(5));
            assert_eq!(lean_int_dec_eq(int("-1"), int("-1")), 1);
        }
    }

    #[test]
    fn division_conventions_match_lean() {
        // Expected values from Lean 4.34.1: `Int.div` rounds toward zero (T-division),
        // `Int.emod`/`Int.ediv` (the `/` and `%` instances) are Euclidean, `x / 0 = 0`,
        // `x % 0 = x`.
        let cases: &[(&str, &str, &str, &str, &str, &str)] = &[
            // n, d, tdiv, tmod, ediv, emod
            ("7", "2", "3", "1", "3", "1"),
            ("-7", "2", "-3", "-1", "-4", "1"),
            ("7", "-2", "-3", "1", "-3", "1"),
            ("-7", "-2", "3", "-1", "4", "1"),
            ("-7", "0", "0", "-7", "0", "-7"),
            ("-100000000000000000000", "3", "-33333333333333333333", "-1", "-33333333333333333334", "2"),
            ("100000000000000000000", "-7", "-14285714285714285714", "2", "-14285714285714285714", "2"),
            ("-2147483648", "-1", "2147483648", "0", "2147483648", "0"),
        ];
        for (n, d, q, r, eq, er) in cases {
            unsafe {
                assert_eq!(show(lean_int_div(int(n), int(d))), *q, "{n}.div {d}");
                assert_eq!(show(lean_int_mod(int(n), int(d))), *r, "{n}.mod {d}");
                assert_eq!(show(lean_int_ediv(int(n), int(d))), *eq, "{n} / {d}");
                assert_eq!(show(lean_int_emod(int(n), int(d))), *er, "{n} % {d}");
            }
        }
    }

    #[test]
    fn conversions() {
        unsafe {
            assert_eq!(show(lean_int_neg_succ_of_nat(lean_box(4))), "-5");
            assert_eq!(show(lean_nat_abs(int("-100000000000000000000"))), "100000000000000000000");
            assert_eq!(show(lean_nat_abs(int("-3"))), "3");
            let big_nat = lean_box(3_000_000_000);
            let as_int = lean_nat_to_int(big_nat);
            assert!(!as_int.is_scalar());
            assert_eq!(show(as_int), "3000000000");
            assert_eq!(lean_int_dec_nonneg(int("-1")), 0);
            assert_eq!(lean_int_dec_lt(int("-100000000000000000000"), int("-1")), 1);
        }
    }
}
