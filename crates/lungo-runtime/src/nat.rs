//! Natural numbers, ported from `lean.h` and the `Nat` section of `runtime/object.cpp`.
//!
//! A `Nat` is a tagged scalar when it is at most [`LEAN_MAX_SMALL_NAT`] and a big-number (MPZ)
//! object otherwise. Every function returns canonical values: results that fit the small range
//! are always scalars.

use crate::object::*;
use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};

/// Converts a non-negative big integer to a canonical `Nat`.
pub fn nat_from_bigint(v: BigInt) -> Obj {
    if v.sign() == Sign::Minus {
        lean_internal_panic("negative value converted to Nat");
    }
    match v.to_usize() {
        Some(n) if n <= LEAN_MAX_SMALL_NAT => lean_box(n),
        _ => alloc_mpz(v),
    }
}

/// Converts an arbitrary-precision natural number to a canonical `Nat`.
pub fn nat_from_biguint(v: BigUint) -> Obj {
    nat_from_bigint(BigInt::from(v))
}

/// The value of the `Nat` `o` (borrowed).
pub unsafe fn nat_to_bigint(o: Obj) -> BigInt {
    if o.is_scalar() { BigInt::from(lean_unbox(o)) } else { unsafe { mpz_value(o).clone() } }
}

/// The value of the `Nat` `o` (borrowed).
pub unsafe fn nat_to_biguint(o: Obj) -> BigUint {
    unsafe { nat_to_bigint(o).to_biguint().unwrap_or_else(|| lean_internal_panic("Nat object holds a negative value")) }
}

#[inline(always)]
pub fn lean_usize_to_nat(n: usize) -> Obj {
    if n <= LEAN_MAX_SMALL_NAT { lean_box(n) } else { alloc_mpz(BigInt::from(n)) }
}

#[inline(always)]
pub fn lean_unsigned_to_nat(n: u32) -> Obj {
    lean_usize_to_nat(n as usize)
}

#[inline(always)]
pub fn lean_uint64_to_nat(n: u64) -> Obj {
    if n <= LEAN_MAX_SMALL_NAT as u64 { lean_box(n as usize) } else { alloc_mpz(BigInt::from(n)) }
}

/// Parses a decimal numeral (as produced for Lean `Nat` literals) into a canonical `Nat`.
pub fn lean_cstr_to_nat(s: &str) -> Obj {
    let v: BigUint = s.parse().unwrap_or_else(|_| lean_internal_panic(&format!("invalid Nat literal {s:?}")));
    nat_from_biguint(v)
}

/// `n + 1` for a borrowed `Nat`.
#[inline]
pub unsafe fn lean_nat_succ(a: Obj) -> Obj {
    if a.is_scalar() { lean_usize_to_nat(lean_unbox(a) + 1) } else { unsafe { nat_from_bigint(mpz_value(a) + 1u32) } }
}

#[inline]
unsafe fn big(o: Obj) -> BigInt {
    unsafe { nat_to_bigint(o) }
}

/// Converts a borrowed `Nat` to an exponent that must fit in 32 bits, as the C runtime does.
unsafe fn small_exponent(a: Obj, what: &str) -> u32 {
    if !a.is_scalar() || lean_unbox(a) > u32::MAX as usize {
        lean_internal_panic(&format!("{what} exponent is too big"));
    }
    lean_unbox(a) as u32
}

/// `⌊log₂ v⌋`, or zero when `v` is zero, as GMP's `mpz_sizeinbase(v, 2) - 1`.
fn big_log2(v: &BigInt) -> u64 {
    if v.sign() != Sign::Plus { 0 } else { v.bits() - 1 }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_nat_add(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_usize_to_nat(lean_unbox(a1) + lean_unbox(a2))
            } else {
                nat_from_bigint(big(a1) + big(a2))
            }
        }

        fn lean_nat_sub(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (n1, n2) = (lean_unbox(a1), lean_unbox(a2));
                lean_box(n1.saturating_sub(n2))
            } else {
                let (b1, b2) = (big(a1), big(a2));
                if b1 < b2 { lean_box(0) } else { nat_from_bigint(b1 - b2) }
            }
        }

        fn lean_nat_mul(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let n1 = lean_unbox(a1);
                if n1 == 0 {
                    return a1;
                }
                let n2 = lean_unbox(a2);
                match n1.checked_mul(n2) {
                    Some(r) if r <= LEAN_MAX_SMALL_NAT => lean_box(r),
                    _ => nat_from_bigint(BigInt::from(n1) * BigInt::from(n2)),
                }
            } else {
                nat_from_bigint(big(a1) * big(a2))
            }
        }

        fn lean_nat_div(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let n2 = lean_unbox(a2);
                lean_box(lean_unbox(a1).checked_div(n2).unwrap_or(0))
            } else {
                let b2 = big(a2);
                if b2.is_zero() { lean_box(0) } else { nat_from_bigint(big(a1) / b2) }
            }
        }

        fn lean_nat_div_exact(a1: b_obj, a2: b_obj) -> obj {
            // `Nat.divExact` assumes `a2 ∣ a1`; truncating division computes the exact quotient.
            lean_nat_div(a1, a2)
        }

        fn lean_nat_mod(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (n1, n2) = (lean_unbox(a1), lean_unbox(a2));
                if n2 == 0 { lean_box(n1) } else { lean_box(n1 % n2) }
            } else {
                let b2 = big(a2);
                if b2.is_zero() {
                    lean_inc(a1);
                    a1
                } else {
                    nat_from_bigint(big(a1) % b2)
                }
            }
        }

        fn lean_nat_dec_eq(a1: b_obj, a2: b_obj) -> u8 {
            if a1.is_scalar() && a2.is_scalar() {
                (a1 == a2) as u8
            } else if a1.is_scalar() || a2.is_scalar() {
                // Canonical representation: a scalar never equals a big number.
                0
            } else {
                (mpz_value(a1) == mpz_value(a2)) as u8
            }
        }

        fn lean_nat_dec_le(a1: b_obj, a2: b_obj) -> u8 {
            if a1.is_scalar() && a2.is_scalar() {
                (lean_unbox(a1) <= lean_unbox(a2)) as u8
            } else {
                (big(a1) <= big(a2)) as u8
            }
        }

        fn lean_nat_dec_lt(a1: b_obj, a2: b_obj) -> u8 {
            if a1.is_scalar() && a2.is_scalar() {
                (lean_unbox(a1) < lean_unbox(a2)) as u8
            } else {
                (big(a1) < big(a2)) as u8
            }
        }

        fn lean_nat_land(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_box(lean_unbox(a1) & lean_unbox(a2))
            } else {
                nat_from_bigint(big(a1) & big(a2))
            }
        }

        fn lean_nat_lor(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_box(lean_unbox(a1) | lean_unbox(a2))
            } else {
                nat_from_bigint(big(a1) | big(a2))
            }
        }

        fn lean_nat_lxor(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_box(lean_unbox(a1) ^ lean_unbox(a2))
            } else {
                nat_from_bigint(big(a1) ^ big(a2))
            }
        }

        fn lean_nat_shiftl(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && lean_unbox(a1) == 0 {
                return lean_box(0);
            }
            let k = small_exponent(a2, "Nat.shiftl");
            nat_from_bigint(big(a1) << k)
        }

        fn lean_nat_shiftr(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                let (s1, s2) = (lean_unbox(a1), lean_unbox(a2));
                return lean_box(if s2 < usize::BITS as usize { s1 >> s2 } else { 0 });
            }
            if !a2.is_scalar() {
                // Such a large shift clears every bit.
                return lean_box(0);
            }
            let a = big(a1);
            let s = lean_unbox(a2);
            if s > u32::MAX as usize {
                if big_log2(&a) >= s as u64 {
                    lean_internal_panic("Nat.shiftr exponent is too big");
                }
                return lean_box(0);
            }
            nat_from_bigint(a >> s)
        }

        fn lean_nat_pow(a1: b_obj, a2: b_obj) -> obj since(4, 35, 0) {
            // An exponent beyond a machine word would overflow memory for any base ≥ 2; for 0
            // and 1 the result is cheap and total (the exponent is positive here).
            if (!a2.is_scalar() || lean_unbox(a2) > u32::MAX as usize) && a1.is_scalar() && lean_unbox(a1) <= 1 {
                return lean_box(lean_unbox(a1));
            }
            let k = small_exponent(a2, "Nat.pow");
            nat_from_bigint(num_traits::pow::Pow::pow(big(a1), k))
        }

        /// Up to Lean 4.34.1, an exponent beyond a machine word panics whatever the base.
        fn lean_nat_pow_v4_34_1(a1: b_obj, a2: b_obj) -> obj as lean_nat_pow until(4, 34, 1) {
            let k = small_exponent(a2, "Nat.pow");
            nat_from_bigint(num_traits::pow::Pow::pow(big(a1), k))
        }

        /// `Nat.powMod b e m`: `b ^ e % m`, computed by modular exponentiation; with `m = 0`,
        /// `b ^ e` (Lean's `_ % 0 = _`), which panics on an exponent beyond a machine word as
        /// `Nat.pow` does.
        fn lean_nat_powmod(b: b_obj, e: b_obj, m: b_obj) -> obj since(4, 35, 0) {
            if m.is_scalar() && lean_unbox(m) == 0 {
                return lean_nat_pow(b, e);
            }
            nat_from_bigint(big(b).modpow(&big(e), &big(m)))
        }

        fn lean_nat_gcd(a1: b_obj, a2: b_obj) -> obj {
            if a1.is_scalar() && a2.is_scalar() {
                lean_box(lean_unbox(a1).gcd(&lean_unbox(a2)))
            } else {
                nat_from_bigint(big(a1).gcd(&big(a2)))
            }
        }

        fn lean_nat_log2(a: b_obj) -> obj {
            if a.is_scalar() {
                let n = lean_unbox(a);
                lean_box(if n == 0 { 0 } else { (usize::BITS - 1 - n.leading_zeros()) as usize })
            } else {
                lean_box(big_log2(mpz_value(a)) as usize)
            }
        }

        fn lean_nat_pred(a: b_obj) -> obj {
            lean_nat_sub(a, lean_box(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    fn nat(s: &str) -> Obj {
        lean_cstr_to_nat(s)
    }

    fn show(o: Obj) -> String {
        unsafe { nat_to_biguint(o).to_string() }
    }

    const MAX_SMALL: &str = "9223372036854775807";
    const MAX_SMALL_PLUS_1: &str = "9223372036854775808";

    /// TEST0192: small big boundary is canonical
    #[test]
    fn test0192_small_big_boundary_is_canonical() {
        assert!(nat(MAX_SMALL).is_scalar());
        assert!(!nat(MAX_SMALL_PLUS_1).is_scalar());
        unsafe {
            let s = lean_nat_add(nat(MAX_SMALL), lean_box(1));
            assert!(!s.is_scalar());
            assert_eq!(show(s), MAX_SMALL_PLUS_1);
            let d = lean_nat_sub(s, lean_box(1));
            assert!(d.is_scalar(), "a big result in the small range must become a scalar");
            assert_eq!(show(d), MAX_SMALL);
            assert_eq!(lean_nat_dec_eq(s, nat(MAX_SMALL_PLUS_1)), 1);
            assert_eq!(lean_nat_dec_lt(nat(MAX_SMALL), s), 1);
        }
    }

    /// TEST0193: arithmetic matches lean
    #[test]
    fn test0193_arithmetic_matches_lean() {
        // Expected values computed with Lean 4.34.1 (`#eval`).
        unsafe {
            assert_eq!(show(lean_nat_sub(lean_box(3), lean_box(5))), "0");
            assert_eq!(show(lean_nat_div(lean_box(7), lean_box(0))), "0");
            assert_eq!(show(lean_nat_mod(lean_box(7), lean_box(0))), "7");
            assert_eq!(show(lean_nat_mod(nat("100000000000000000000"), lean_box(0))), "100000000000000000000");
            assert_eq!(show(lean_nat_div(nat("100000000000000000000"), lean_box(0))), "0");
            assert_eq!(show(lean_nat_mul(nat("4294967296"), nat("4294967296"))), "18446744073709551616");
            assert_eq!(show(lean_nat_pow(lean_box(2), lean_box(100))), "1267650600228229401496703205376");
            assert_eq!(show(lean_nat_shiftl(lean_box(1), lean_box(70))), "1180591620717411303424");
            assert_eq!(show(lean_nat_shiftr(nat("1180591620717411303424"), lean_box(69))), "2");
            assert_eq!(show(lean_nat_shiftr(lean_box(5), lean_box(200))), "0");
            assert_eq!(show(lean_nat_log2(lean_box(0))), "0");
            assert_eq!(show(lean_nat_log2(nat("1180591620717411303424"))), "70");
            assert_eq!(show(lean_nat_gcd(nat("18446744073709551616"), lean_box(12))), "4");
            assert_eq!(show(lean_nat_lxor(nat("18446744073709551615"), lean_box(1))), "18446744073709551614");
            assert_eq!(show(lean_nat_land(nat("18446744073709551617"), lean_box(3))), "1");
            assert_eq!(show(lean_nat_pred(lean_box(0))), "0");
        }
    }

    const TEST0382: &str = "nat::tests::test0382_pow_of_zero_and_one_to_a_huge_exponent_follows_the_release";
    const CHILD_CASE: &str = "LUNGO_RUNTIME_TEST_CASE";

    /// Runs `case` of the test `test` in a child process (this test binary, running that test
    /// alone): an internal panic ends the process.
    fn in_child(test: &str, case: &str) -> std::process::Output {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(CHILD_CASE, case)
            .env_remove("LEAN_ABORT_ON_PANIC")
            .output()
            .unwrap()
    }

    /// TEST0382: since Lean 4.35.0, zero and one raised to an exponent beyond a machine word are
    /// zero and one; up to 4.34.1 that exponent panics whatever the base
    #[test]
    fn test0382_pow_of_zero_and_one_to_a_huge_exponent_follows_the_release() {
        // Expected outcomes from `lean --run` with Lean 4.35.0 and 4.34.1.
        let huge = || nat("100000000000000000000");
        if let Ok(case) = std::env::var(CHILD_CASE) {
            unsafe {
                match case.as_str() {
                    "4.34.1 base 0" => lean_nat_pow_v4_34_1(lean_box(0), huge()),
                    "4.34.1 base 1" => lean_nat_pow_v4_34_1(lean_box(1), huge()),
                    "4.35.0 base 2" => lean_nat_pow(lean_box(2), huge()),
                    other => panic!("unknown case {other}"),
                };
            }
            std::process::exit(0);
        }
        unsafe {
            assert_eq!(show(lean_nat_pow(lean_box(0), huge())), "0");
            assert_eq!(show(lean_nat_pow(lean_box(1), huge())), "1");
            assert_eq!(show(lean_nat_pow(lean_box(1), lean_box(u32::MAX as usize + 1))), "1");
            assert_eq!(show(lean_nat_pow_v4_34_1(lean_box(3), lean_box(4))), "81");
        }
        for case in ["4.34.1 base 0", "4.34.1 base 1", "4.35.0 base 2"] {
            let out = in_child(TEST0382, case);
            assert!(!out.status.success(), "{case}: the program continued");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(stderr.contains("INTERNAL PANIC: Nat.pow exponent is too big"), "{case}: {stderr}");
        }
    }

    /// TEST0383: `Nat.powMod` reduces by modular exponentiation; modulo zero it is `Nat.pow`
    #[test]
    fn test0383_powmod_reduces_by_modular_exponentiation_and_modulo_zero_is_pow() {
        // Expected values from `lean --run` with Lean 4.35.0.
        unsafe {
            assert_eq!(show(lean_nat_powmod(lean_box(3), lean_box(4), lean_box(5))), "1");
            assert_eq!(show(lean_nat_powmod(lean_box(2), lean_box(10), lean_box(1000))), "24");
            assert_eq!(show(lean_nat_powmod(lean_box(3), lean_box(4), lean_box(0))), "81");
            assert_eq!(show(lean_nat_powmod(lean_box(7), nat("100000000000000000000"), lean_box(1))), "0");
            assert_eq!(
                show(lean_nat_powmod(lean_box(2), nat("100000000000000000000"), nat("1000000007"))),
                "855473248"
            );
        }
    }
}
