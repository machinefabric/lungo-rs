//! Fixed-width integers (`UInt8`…`UInt64`, `USize`, `Int8`…`Int64`, `ISize`) and `Bool`
//! conversions, ported from `lean.h` and the `UInt`/`IntX` sections of `runtime/object.cpp`.
//!
//! Signed types are represented by the unsigned type of the same width, exactly as in Lean's
//! runtime; every operation reproduces the modular semantics of the C implementation.
//!
//! This file's `externs` block is generated from a table of widths; each family below is the
//! same set of operations instantiated at one width.

use crate::int::{lean_int64_to_int, lean_scalar_to_int64};
use crate::object::*;
use num_bigint::{BigInt, Sign};

/// The low 64 bits of the two's-complement representation of `v` (GMP's `fdiv_r_2exp(v, 64)`).
pub(crate) fn big_low_u64(v: &BigInt) -> u64 {
    let low = v.iter_u64_digits().next().unwrap_or(0);
    if v.sign() == Sign::Minus { low.wrapping_neg() } else { low }
}

/// The low bits of a borrowed big `Nat` or `Int` object.
#[inline]
unsafe fn big_low(a: Obj) -> u64 {
    unsafe { big_low_u64(mpz_value(a)) }
}

/// `⌊log₂ a⌋`, and zero for zero, as the loops in `lean.h`.
macro_rules! log2 {
    ($a:expr, $t:ty) => {{
        let a: $t = $a;
        if a == 0 { 0 } else { (<$t>::BITS - 1 - a.leading_zeros()) as $t }
    }};
}

pub mod externs {
    use super::*;

    crate::lean_externs! {

        // ---- uint8 ----
        fn lean_uint8_add(a: u8, b: u8) -> u8 { a.wrapping_add(b) }
        fn lean_uint8_sub(a: u8, b: u8) -> u8 { a.wrapping_sub(b) }
        fn lean_uint8_mul(a: u8, b: u8) -> u8 { a.wrapping_mul(b) }
        fn lean_uint8_div(a: u8, b: u8) -> u8 { a.checked_div(b).unwrap_or(0) }
        fn lean_uint8_mod(a: u8, b: u8) -> u8 { if b == 0 { a } else { a % b } }
        fn lean_uint8_land(a: u8, b: u8) -> u8 { a & b }
        fn lean_uint8_lor(a: u8, b: u8) -> u8 { a | b }
        fn lean_uint8_xor(a: u8, b: u8) -> u8 { a ^ b }
        fn lean_uint8_shift_left(a: u8, b: u8) -> u8 { a << (b % 8_u8) }
        fn lean_uint8_shift_right(a: u8, b: u8) -> u8 { a >> (b % 8_u8) }
        fn lean_uint8_complement(a: u8) -> u8 { !a }
        fn lean_uint8_neg(a: u8) -> u8 { a.wrapping_neg() }
        fn lean_uint8_log2(a: u8) -> u8 { log2!(a, u8) }
        fn lean_uint8_dec_eq(a: u8, b: u8) -> u8 { (a == b) as u8 }
        fn lean_uint8_dec_lt(a: u8, b: u8) -> u8 { (a < b) as u8 }
        fn lean_uint8_dec_le(a: u8, b: u8) -> u8 { (a <= b) as u8 }
        fn lean_uint8_of_nat(a: b_obj) -> u8 {
            if a.is_scalar() { lean_unbox(a) as u8 } else { big_low(a) as u8 }
        }
        fn lean_uint8_of_nat_mk(a: obj) -> u8 {
            let r = lean_uint8_of_nat(a);
            lean_dec(a);
            r
        }
        fn lean_uint8_to_float(a: u8) -> f64 { a as f64 }
        fn lean_uint8_to_float32(a: u8) -> f32 { a as f32 }
        fn lean_uint8_to_nat(a: u8) -> obj { crate::nat::lean_usize_to_nat(a as usize) }
        fn lean_uint8_to_uint16(a: u8) -> u16 { a as u16 }
        fn lean_uint8_to_uint32(a: u8) -> u32 { a as u32 }
        fn lean_uint8_to_uint64(a: u8) -> u64 { a as u64 }
        fn lean_uint8_to_usize(a: u8) -> usize { a as usize }

        // ---- uint16 ----
        fn lean_uint16_add(a: u16, b: u16) -> u16 { a.wrapping_add(b) }
        fn lean_uint16_sub(a: u16, b: u16) -> u16 { a.wrapping_sub(b) }
        fn lean_uint16_mul(a: u16, b: u16) -> u16 { a.wrapping_mul(b) }
        fn lean_uint16_div(a: u16, b: u16) -> u16 { a.checked_div(b).unwrap_or(0) }
        fn lean_uint16_mod(a: u16, b: u16) -> u16 { if b == 0 { a } else { a % b } }
        fn lean_uint16_land(a: u16, b: u16) -> u16 { a & b }
        fn lean_uint16_lor(a: u16, b: u16) -> u16 { a | b }
        fn lean_uint16_xor(a: u16, b: u16) -> u16 { a ^ b }
        fn lean_uint16_shift_left(a: u16, b: u16) -> u16 { a << (b % 16_u16) }
        fn lean_uint16_shift_right(a: u16, b: u16) -> u16 { a >> (b % 16_u16) }
        fn lean_uint16_complement(a: u16) -> u16 { !a }
        fn lean_uint16_neg(a: u16) -> u16 { a.wrapping_neg() }
        fn lean_uint16_log2(a: u16) -> u16 { log2!(a, u16) }
        fn lean_uint16_dec_eq(a: u16, b: u16) -> u8 { (a == b) as u8 }
        fn lean_uint16_dec_lt(a: u16, b: u16) -> u8 { (a < b) as u8 }
        fn lean_uint16_dec_le(a: u16, b: u16) -> u8 { (a <= b) as u8 }
        fn lean_uint16_of_nat(a: b_obj) -> u16 {
            if a.is_scalar() { lean_unbox(a) as u16 } else { big_low(a) as u16 }
        }
        fn lean_uint16_of_nat_mk(a: obj) -> u16 {
            let r = lean_uint16_of_nat(a);
            lean_dec(a);
            r
        }
        fn lean_uint16_to_float(a: u16) -> f64 { a as f64 }
        fn lean_uint16_to_float32(a: u16) -> f32 { a as f32 }
        fn lean_uint16_to_nat(a: u16) -> obj { crate::nat::lean_usize_to_nat(a as usize) }
        fn lean_uint16_to_uint8(a: u16) -> u8 { a as u8 }
        fn lean_uint16_to_uint32(a: u16) -> u32 { a as u32 }
        fn lean_uint16_to_uint64(a: u16) -> u64 { a as u64 }
        fn lean_uint16_to_usize(a: u16) -> usize { a as usize }

        // ---- uint32 ----
        fn lean_uint32_add(a: u32, b: u32) -> u32 { a.wrapping_add(b) }
        fn lean_uint32_sub(a: u32, b: u32) -> u32 { a.wrapping_sub(b) }
        fn lean_uint32_mul(a: u32, b: u32) -> u32 { a.wrapping_mul(b) }
        fn lean_uint32_div(a: u32, b: u32) -> u32 { a.checked_div(b).unwrap_or(0) }
        fn lean_uint32_mod(a: u32, b: u32) -> u32 { if b == 0 { a } else { a % b } }
        fn lean_uint32_land(a: u32, b: u32) -> u32 { a & b }
        fn lean_uint32_lor(a: u32, b: u32) -> u32 { a | b }
        fn lean_uint32_xor(a: u32, b: u32) -> u32 { a ^ b }
        fn lean_uint32_shift_left(a: u32, b: u32) -> u32 { a << (b % 32_u32) }
        fn lean_uint32_shift_right(a: u32, b: u32) -> u32 { a >> (b % 32_u32) }
        fn lean_uint32_complement(a: u32) -> u32 { !a }
        fn lean_uint32_neg(a: u32) -> u32 { a.wrapping_neg() }
        fn lean_uint32_log2(a: u32) -> u32 { log2!(a, u32) }
        fn lean_uint32_dec_eq(a: u32, b: u32) -> u8 { (a == b) as u8 }
        fn lean_uint32_dec_lt(a: u32, b: u32) -> u8 { (a < b) as u8 }
        fn lean_uint32_dec_le(a: u32, b: u32) -> u8 { (a <= b) as u8 }
        fn lean_uint32_of_nat(a: b_obj) -> u32 {
            if a.is_scalar() { lean_unbox(a) as u32 } else { big_low(a) as u32 }
        }
        fn lean_uint32_of_nat_mk(a: obj) -> u32 {
            let r = lean_uint32_of_nat(a);
            lean_dec(a);
            r
        }
        fn lean_uint32_to_float(a: u32) -> f64 { a as f64 }
        fn lean_uint32_to_float32(a: u32) -> f32 { a as f32 }
        fn lean_uint32_to_nat(a: u32) -> obj { crate::nat::lean_usize_to_nat(a as usize) }
        fn lean_uint32_to_uint8(a: u32) -> u8 { a as u8 }
        fn lean_uint32_to_uint16(a: u32) -> u16 { a as u16 }
        fn lean_uint32_to_uint64(a: u32) -> u64 { a as u64 }
        fn lean_uint32_to_usize(a: u32) -> usize { a as usize }

        // ---- uint64 ----
        fn lean_uint64_add(a: u64, b: u64) -> u64 { a.wrapping_add(b) }
        fn lean_uint64_sub(a: u64, b: u64) -> u64 { a.wrapping_sub(b) }
        fn lean_uint64_mul(a: u64, b: u64) -> u64 { a.wrapping_mul(b) }
        fn lean_uint64_div(a: u64, b: u64) -> u64 { a.checked_div(b).unwrap_or(0) }
        fn lean_uint64_mod(a: u64, b: u64) -> u64 { if b == 0 { a } else { a % b } }
        fn lean_uint64_land(a: u64, b: u64) -> u64 { a & b }
        fn lean_uint64_lor(a: u64, b: u64) -> u64 { a | b }
        fn lean_uint64_xor(a: u64, b: u64) -> u64 { a ^ b }
        fn lean_uint64_shift_left(a: u64, b: u64) -> u64 { a << (b % 64_u64) }
        fn lean_uint64_shift_right(a: u64, b: u64) -> u64 { a >> (b % 64_u64) }
        fn lean_uint64_complement(a: u64) -> u64 { !a }
        fn lean_uint64_neg(a: u64) -> u64 { a.wrapping_neg() }
        fn lean_uint64_log2(a: u64) -> u64 { log2!(a, u64) }
        fn lean_uint64_dec_eq(a: u64, b: u64) -> u8 { (a == b) as u8 }
        fn lean_uint64_dec_lt(a: u64, b: u64) -> u8 { (a < b) as u8 }
        fn lean_uint64_dec_le(a: u64, b: u64) -> u8 { (a <= b) as u8 }
        fn lean_uint64_of_nat(a: b_obj) -> u64 {
            if a.is_scalar() { lean_unbox(a) as u64 } else { big_low(a) }
        }
        fn lean_uint64_of_nat_mk(a: obj) -> u64 {
            let r = lean_uint64_of_nat(a);
            lean_dec(a);
            r
        }
        fn lean_uint64_to_float(a: u64) -> f64 { a as f64 }
        fn lean_uint64_to_float32(a: u64) -> f32 { a as f32 }
        fn lean_uint64_to_nat(a: u64) -> obj { crate::nat::lean_uint64_to_nat(a) }
        fn lean_uint64_to_uint8(a: u64) -> u8 { a as u8 }
        fn lean_uint64_to_uint16(a: u64) -> u16 { a as u16 }
        fn lean_uint64_to_uint32(a: u64) -> u32 { a as u32 }
        fn lean_uint64_to_usize(a: u64) -> usize { a as usize }

        // ---- usize ----
        fn lean_usize_add(a: usize, b: usize) -> usize { a.wrapping_add(b) }
        fn lean_usize_sub(a: usize, b: usize) -> usize { a.wrapping_sub(b) }
        fn lean_usize_mul(a: usize, b: usize) -> usize { a.wrapping_mul(b) }
        fn lean_usize_div(a: usize, b: usize) -> usize { a.checked_div(b).unwrap_or(0) }
        fn lean_usize_mod(a: usize, b: usize) -> usize { if b == 0 { a } else { a % b } }
        fn lean_usize_land(a: usize, b: usize) -> usize { a & b }
        fn lean_usize_lor(a: usize, b: usize) -> usize { a | b }
        fn lean_usize_xor(a: usize, b: usize) -> usize { a ^ b }
        fn lean_usize_shift_left(a: usize, b: usize) -> usize { a << (b % (usize::BITS) as usize) }
        fn lean_usize_shift_right(a: usize, b: usize) -> usize { a >> (b % (usize::BITS) as usize) }
        fn lean_usize_complement(a: usize) -> usize { !a }
        fn lean_usize_neg(a: usize) -> usize { a.wrapping_neg() }
        fn lean_usize_log2(a: usize) -> usize { log2!(a, usize) }
        fn lean_usize_dec_eq(a: usize, b: usize) -> u8 { (a == b) as u8 }
        fn lean_usize_dec_lt(a: usize, b: usize) -> u8 { (a < b) as u8 }
        fn lean_usize_dec_le(a: usize, b: usize) -> u8 { (a <= b) as u8 }
        fn lean_usize_of_nat(a: b_obj) -> usize {
            if a.is_scalar() { lean_unbox(a) } else { big_low(a) as usize }
        }
        fn lean_usize_of_nat_mk(a: obj) -> usize {
            let r = lean_usize_of_nat(a);
            lean_dec(a);
            r
        }
        fn lean_usize_to_float(a: usize) -> f64 { a as f64 }
        fn lean_usize_to_float32(a: usize) -> f32 { a as f32 }
        fn lean_usize_to_nat(a: usize) -> obj { crate::nat::lean_usize_to_nat(a as usize) }
        fn lean_usize_to_uint8(a: usize) -> u8 { a as u8 }
        fn lean_usize_to_uint16(a: usize) -> u16 { a as u16 }
        fn lean_usize_to_uint32(a: usize) -> u32 { a as u32 }
        fn lean_usize_to_uint64(a: usize) -> u64 { a as u64 }

        // ---- int8 ----
        fn lean_int8_neg(a: u8) -> u8 { a.wrapping_neg() }
        fn lean_int8_add(a: u8, b: u8) -> u8 { a.wrapping_add(b) }
        fn lean_int8_sub(a: u8, b: u8) -> u8 { a.wrapping_sub(b) }
        fn lean_int8_mul(a: u8, b: u8) -> u8 { a.wrapping_mul(b) }
        fn lean_int8_div(a: u8, b: u8) -> u8 {
            let (lhs, rhs) = (a as i8, b as i8);
            if rhs == 0 { 0 } else { lhs.wrapping_div(rhs) as u8 }
        }
        fn lean_int8_mod(a: u8, b: u8) -> u8 {
            let (lhs, rhs) = (a as i8, b as i8);
            if rhs == 0 { lhs as u8 } else { lhs.wrapping_rem(rhs) as u8 }
        }
        fn lean_int8_land(a: u8, b: u8) -> u8 { a & b }
        fn lean_int8_lor(a: u8, b: u8) -> u8 { a | b }
        fn lean_int8_xor(a: u8, b: u8) -> u8 { a ^ b }
        fn lean_int8_shift_right(a: u8, b: u8) -> u8 {
            let size = 8_i8;
            let rhs = ((b as i8) % size + size) % size;
            ((a as i8) >> rhs) as u8
        }
        fn lean_int8_shift_left(a: u8, b: u8) -> u8 {
            let size = 8_i8;
            let rhs = ((b as i8) % size + size) % size;
            a << (rhs as u8)
        }
        fn lean_int8_complement(a: u8) -> u8 { !a }
        fn lean_int8_abs(a: u8) -> u8 { if (a as i8) < 0 { a.wrapping_neg() } else { a } }
        fn lean_int8_dec_eq(a: u8, b: u8) -> u8 { (a as i8 == b as i8) as u8 }
        fn lean_int8_dec_lt(a: u8, b: u8) -> u8 { ((a as i8) < (b as i8)) as u8 }
        fn lean_int8_dec_le(a: u8, b: u8) -> u8 { (a as i8 <= b as i8) as u8 }
        fn lean_int8_of_int(a: b_obj) -> u8 {
            if a.is_scalar() { lean_scalar_to_int64(a) as i8 as u8 } else { big_low(a) as i8 as u8 }
        }
        fn lean_int8_of_nat(a: b_obj) -> u8 {
            if a.is_scalar() { lean_unbox(a) as i8 as u8 } else { big_low(a) as i8 as u8 }
        }
        fn lean_int8_to_float(a: u8) -> f64 { a as i8 as f64 }
        fn lean_int8_to_float32(a: u8) -> f32 { a as i8 as f32 }
        fn lean_int8_to_int(a: u8) -> obj { lean_int64_to_int(a as i8 as i64) }
        fn lean_int8_to_int16(a: u8) -> u16 { a as i8 as i16 as u16 }
        fn lean_int8_to_int32(a: u8) -> u32 { a as i8 as i32 as u32 }
        fn lean_int8_to_int64(a: u8) -> u64 { a as i8 as i64 as u64 }
        fn lean_int8_to_isize(a: u8) -> usize { a as i8 as isize as usize }

        // ---- int16 ----
        fn lean_int16_neg(a: u16) -> u16 { a.wrapping_neg() }
        fn lean_int16_add(a: u16, b: u16) -> u16 { a.wrapping_add(b) }
        fn lean_int16_sub(a: u16, b: u16) -> u16 { a.wrapping_sub(b) }
        fn lean_int16_mul(a: u16, b: u16) -> u16 { a.wrapping_mul(b) }
        fn lean_int16_div(a: u16, b: u16) -> u16 {
            let (lhs, rhs) = (a as i16, b as i16);
            if rhs == 0 { 0 } else { lhs.wrapping_div(rhs) as u16 }
        }
        fn lean_int16_mod(a: u16, b: u16) -> u16 {
            let (lhs, rhs) = (a as i16, b as i16);
            if rhs == 0 { lhs as u16 } else { lhs.wrapping_rem(rhs) as u16 }
        }
        fn lean_int16_land(a: u16, b: u16) -> u16 { a & b }
        fn lean_int16_lor(a: u16, b: u16) -> u16 { a | b }
        fn lean_int16_xor(a: u16, b: u16) -> u16 { a ^ b }
        fn lean_int16_shift_right(a: u16, b: u16) -> u16 {
            let size = 16_i16;
            let rhs = ((b as i16) % size + size) % size;
            ((a as i16) >> rhs) as u16
        }
        fn lean_int16_shift_left(a: u16, b: u16) -> u16 {
            let size = 16_i16;
            let rhs = ((b as i16) % size + size) % size;
            a << (rhs as u16)
        }
        fn lean_int16_complement(a: u16) -> u16 { !a }
        fn lean_int16_abs(a: u16) -> u16 { if (a as i16) < 0 { a.wrapping_neg() } else { a } }
        fn lean_int16_dec_eq(a: u16, b: u16) -> u8 { (a as i16 == b as i16) as u8 }
        fn lean_int16_dec_lt(a: u16, b: u16) -> u8 { ((a as i16) < (b as i16)) as u8 }
        fn lean_int16_dec_le(a: u16, b: u16) -> u8 { (a as i16 <= b as i16) as u8 }
        fn lean_int16_of_int(a: b_obj) -> u16 {
            if a.is_scalar() { lean_scalar_to_int64(a) as i16 as u16 } else { big_low(a) as i16 as u16 }
        }
        fn lean_int16_of_nat(a: b_obj) -> u16 {
            if a.is_scalar() { lean_unbox(a) as i16 as u16 } else { big_low(a) as i16 as u16 }
        }
        fn lean_int16_to_float(a: u16) -> f64 { a as i16 as f64 }
        fn lean_int16_to_float32(a: u16) -> f32 { a as i16 as f32 }
        fn lean_int16_to_int(a: u16) -> obj { lean_int64_to_int(a as i16 as i64) }
        fn lean_int16_to_int8(a: u16) -> u8 { a as i16 as i8 as u8 }
        fn lean_int16_to_int32(a: u16) -> u32 { a as i16 as i32 as u32 }
        fn lean_int16_to_int64(a: u16) -> u64 { a as i16 as i64 as u64 }
        fn lean_int16_to_isize(a: u16) -> usize { a as i16 as isize as usize }

        // ---- int32 ----
        fn lean_int32_neg(a: u32) -> u32 { a.wrapping_neg() }
        fn lean_int32_add(a: u32, b: u32) -> u32 { a.wrapping_add(b) }
        fn lean_int32_sub(a: u32, b: u32) -> u32 { a.wrapping_sub(b) }
        fn lean_int32_mul(a: u32, b: u32) -> u32 { a.wrapping_mul(b) }
        fn lean_int32_div(a: u32, b: u32) -> u32 {
            let (lhs, rhs) = (a as i32, b as i32);
            if rhs == 0 { 0 } else { lhs.wrapping_div(rhs) as u32 }
        }
        fn lean_int32_mod(a: u32, b: u32) -> u32 {
            let (lhs, rhs) = (a as i32, b as i32);
            if rhs == 0 { lhs as u32 } else { lhs.wrapping_rem(rhs) as u32 }
        }
        fn lean_int32_land(a: u32, b: u32) -> u32 { a & b }
        fn lean_int32_lor(a: u32, b: u32) -> u32 { a | b }
        fn lean_int32_xor(a: u32, b: u32) -> u32 { a ^ b }
        fn lean_int32_shift_right(a: u32, b: u32) -> u32 {
            let size = 32_i32;
            let rhs = ((b as i32) % size + size) % size;
            ((a as i32) >> rhs) as u32
        }
        fn lean_int32_shift_left(a: u32, b: u32) -> u32 {
            let size = 32_i32;
            let rhs = ((b as i32) % size + size) % size;
            a << (rhs as u32)
        }
        fn lean_int32_complement(a: u32) -> u32 { !a }
        fn lean_int32_abs(a: u32) -> u32 { if (a as i32) < 0 { a.wrapping_neg() } else { a } }
        fn lean_int32_dec_eq(a: u32, b: u32) -> u8 { (a as i32 == b as i32) as u8 }
        fn lean_int32_dec_lt(a: u32, b: u32) -> u8 { ((a as i32) < (b as i32)) as u8 }
        fn lean_int32_dec_le(a: u32, b: u32) -> u8 { (a as i32 <= b as i32) as u8 }
        fn lean_int32_of_int(a: b_obj) -> u32 {
            if a.is_scalar() { lean_scalar_to_int64(a) as i32 as u32 } else { big_low(a) as i32 as u32 }
        }
        fn lean_int32_of_nat(a: b_obj) -> u32 {
            if a.is_scalar() { lean_unbox(a) as i32 as u32 } else { big_low(a) as i32 as u32 }
        }
        fn lean_int32_to_float(a: u32) -> f64 { a as i32 as f64 }
        fn lean_int32_to_float32(a: u32) -> f32 { a as i32 as f32 }
        fn lean_int32_to_int(a: u32) -> obj { lean_int64_to_int(a as i32 as i64) }
        fn lean_int32_to_int8(a: u32) -> u8 { a as i32 as i8 as u8 }
        fn lean_int32_to_int16(a: u32) -> u16 { a as i32 as i16 as u16 }
        fn lean_int32_to_int64(a: u32) -> u64 { a as i32 as i64 as u64 }
        fn lean_int32_to_isize(a: u32) -> usize { a as i32 as isize as usize }

        // ---- int64 ----
        fn lean_int64_neg(a: u64) -> u64 { a.wrapping_neg() }
        fn lean_int64_add(a: u64, b: u64) -> u64 { a.wrapping_add(b) }
        fn lean_int64_sub(a: u64, b: u64) -> u64 { a.wrapping_sub(b) }
        fn lean_int64_mul(a: u64, b: u64) -> u64 { a.wrapping_mul(b) }
        fn lean_int64_div(a: u64, b: u64) -> u64 {
            let (lhs, rhs) = (a as i64, b as i64);
            if rhs == 0 { 0 } else { lhs.wrapping_div(rhs) as u64 }
        }
        fn lean_int64_mod(a: u64, b: u64) -> u64 {
            let (lhs, rhs) = (a as i64, b as i64);
            if rhs == 0 { lhs as u64 } else { lhs.wrapping_rem(rhs) as u64 }
        }
        fn lean_int64_land(a: u64, b: u64) -> u64 { a & b }
        fn lean_int64_lor(a: u64, b: u64) -> u64 { a | b }
        fn lean_int64_xor(a: u64, b: u64) -> u64 { a ^ b }
        fn lean_int64_shift_right(a: u64, b: u64) -> u64 {
            let size = 64_i64;
            let rhs = ((b as i64) % size + size) % size;
            ((a as i64) >> rhs) as u64
        }
        fn lean_int64_shift_left(a: u64, b: u64) -> u64 {
            let size = 64_i64;
            let rhs = ((b as i64) % size + size) % size;
            a << (rhs as u64)
        }
        fn lean_int64_complement(a: u64) -> u64 { !a }
        fn lean_int64_abs(a: u64) -> u64 { if (a as i64) < 0 { a.wrapping_neg() } else { a } }
        fn lean_int64_dec_eq(a: u64, b: u64) -> u8 { (a as i64 == b as i64) as u8 }
        fn lean_int64_dec_lt(a: u64, b: u64) -> u8 { ((a as i64) < (b as i64)) as u8 }
        fn lean_int64_dec_le(a: u64, b: u64) -> u8 { (a as i64 <= b as i64) as u8 }
        fn lean_int64_of_int(a: b_obj) -> u64 {
            if a.is_scalar() { lean_scalar_to_int64(a) as u64 } else { big_low(a) as i64 as u64 }
        }
        fn lean_int64_of_nat(a: b_obj) -> u64 {
            if a.is_scalar() { lean_unbox(a) as i64 as u64 } else { big_low(a) as i64 as u64 }
        }
        fn lean_int64_to_float(a: u64) -> f64 { a as i64 as f64 }
        fn lean_int64_to_float32(a: u64) -> f32 { a as i64 as f32 }
        fn lean_int64_to_int_sint(a: u64) -> obj { lean_int64_to_int(a as i64) }
        fn lean_int64_to_int8(a: u64) -> u8 { a as i64 as i8 as u8 }
        fn lean_int64_to_int16(a: u64) -> u16 { a as i64 as i16 as u16 }
        fn lean_int64_to_int32(a: u64) -> u32 { a as i64 as i32 as u32 }
        fn lean_int64_to_isize(a: u64) -> usize { a as i64 as isize as usize }

        // ---- isize ----
        fn lean_isize_neg(a: usize) -> usize { a.wrapping_neg() }
        fn lean_isize_add(a: usize, b: usize) -> usize { a.wrapping_add(b) }
        fn lean_isize_sub(a: usize, b: usize) -> usize { a.wrapping_sub(b) }
        fn lean_isize_mul(a: usize, b: usize) -> usize { a.wrapping_mul(b) }
        fn lean_isize_div(a: usize, b: usize) -> usize {
            let (lhs, rhs) = (a as isize, b as isize);
            if rhs == 0 { 0 } else { lhs.wrapping_div(rhs) as usize }
        }
        fn lean_isize_mod(a: usize, b: usize) -> usize {
            let (lhs, rhs) = (a as isize, b as isize);
            if rhs == 0 { lhs as usize } else { lhs.wrapping_rem(rhs) as usize }
        }
        fn lean_isize_land(a: usize, b: usize) -> usize { a & b }
        fn lean_isize_lor(a: usize, b: usize) -> usize { a | b }
        fn lean_isize_xor(a: usize, b: usize) -> usize { a ^ b }
        fn lean_isize_shift_right(a: usize, b: usize) -> usize {
            let size = (isize::BITS) as isize;
            let rhs = ((b as isize) % size + size) % size;
            ((a as isize) >> rhs) as usize
        }
        fn lean_isize_shift_left(a: usize, b: usize) -> usize {
            let size = (isize::BITS) as isize;
            let rhs = ((b as isize) % size + size) % size;
            a << (rhs as usize)
        }
        fn lean_isize_complement(a: usize) -> usize { !a }
        fn lean_isize_abs(a: usize) -> usize { if (a as isize) < 0 { a.wrapping_neg() } else { a } }
        fn lean_isize_dec_eq(a: usize, b: usize) -> u8 { (a as isize == b as isize) as u8 }
        fn lean_isize_dec_lt(a: usize, b: usize) -> u8 { ((a as isize) < (b as isize)) as u8 }
        fn lean_isize_dec_le(a: usize, b: usize) -> u8 { (a as isize <= b as isize) as u8 }
        fn lean_isize_of_int(a: b_obj) -> usize {
            if a.is_scalar() { lean_scalar_to_int64(a) as isize as usize } else { big_low(a) as isize as usize }
        }
        fn lean_isize_of_nat(a: b_obj) -> usize {
            if a.is_scalar() { lean_unbox(a) as isize as usize } else { big_low(a) as isize as usize }
        }
        fn lean_isize_to_float(a: usize) -> f64 { a as isize as f64 }
        fn lean_isize_to_float32(a: usize) -> f32 { a as isize as f32 }
        fn lean_isize_to_int(a: usize) -> obj { lean_int64_to_int(a as isize as i64) }
        fn lean_isize_to_int8(a: usize) -> u8 { a as isize as i8 as u8 }
        fn lean_isize_to_int16(a: usize) -> u16 { a as isize as i16 as u16 }
        fn lean_isize_to_int32(a: usize) -> u32 { a as isize as i32 as u32 }
        fn lean_isize_to_int64(a: usize) -> u64 { a as isize as i64 as u64 }

        // ---- Bool ----
        fn lean_bool_to_uint8(a: u8) -> u8 { a as u8 }
        fn lean_bool_to_uint16(a: u8) -> u16 { a as u16 }
        fn lean_bool_to_uint32(a: u8) -> u32 { a as u32 }
        fn lean_bool_to_uint64(a: u8) -> u64 { a as u64 }
        fn lean_bool_to_usize(a: u8) -> usize { a as usize }
        fn lean_bool_to_int8(a: u8) -> u8 { a as i8 as u8 }
        fn lean_bool_to_int16(a: u8) -> u16 { a as i16 as u16 }
        fn lean_bool_to_int32(a: u8) -> u32 { a as i32 as u32 }
        fn lean_bool_to_int64(a: u8) -> u64 { a as i64 as u64 }
        fn lean_bool_to_isize(a: u8) -> usize { a as isize as usize }

        fn lean_uint64_mix_hash(h: u64, k: u64) -> u64 {
            let m: u64 = 0xc6a4a7935bd1e995;
            let r = 47;
            let mut k = k.wrapping_mul(m);
            k ^= k >> r;
            k ^= m;
            let h = h ^ k;
            h.wrapping_mul(m)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use crate::int::lean_cstr_to_int;
    use crate::nat::lean_cstr_to_nat;

    // Expected values computed with Lean 4.34.1 (`lean --run`).

    #[test]
    fn unsigned_wraparound_division_and_shifts() {
        unsafe {
            assert_eq!(lean_uint8_add(200, 100), 44);
            assert_eq!(lean_uint8_sub(3, 5), 254);
            assert_eq!(lean_uint8_div(7, 0), 0);
            assert_eq!(lean_uint8_mod(7, 0), 7);
            assert_eq!(lean_uint8_shift_left(1, 9), 2);
            assert_eq!(lean_uint8_shift_right(128, 9), 64);
            assert_eq!(lean_uint8_log2(0), 0);
            assert_eq!(lean_uint8_log2(255), 7);
            assert_eq!(lean_uint64_shift_left(1, 65), 2);
            assert_eq!(lean_uint64_sub(0, 1), u64::MAX);
            assert_eq!(lean_uint64_mix_hash(1, 2), 16582581243253999004);
            assert_eq!(lean_bool_to_uint64(1), 1);
            assert_eq!(lean_usize_to_nat(usize::MAX >> 1), crate::object::lean_box(usize::MAX >> 1));
            assert!(!lean_usize_to_nat(usize::MAX).is_scalar());
            assert_eq!(crate::nat::nat_to_biguint(lean_uint64_to_nat(u64::MAX)).to_string(), "18446744073709551615");
            assert_eq!(lean_uint8_to_nat(255), crate::object::lean_box(255));
        }
    }

    #[test]
    fn of_nat_reduces_big_numbers_modulo_the_width() {
        unsafe {
            let n = lean_cstr_to_nat("18446744073709551617");
            assert_eq!(lean_uint64_of_nat(n), 1);
            assert_eq!(lean_usize_of_nat(n), 1);
            assert_eq!(lean_uint32_of_nat(lean_cstr_to_nat("4294967297")), 1);
        }
    }

    #[test]
    fn signed_semantics() {
        unsafe {
            let i8 = |v: i8| v as u8;
            assert_eq!(lean_int8_div(i8(-128), i8(-1)), i8(-128));
            assert_eq!(lean_int8_mod(i8(-128), i8(-1)), 0);
            assert_eq!(lean_int8_div(i8(-7), 2), i8(-3));
            assert_eq!(lean_int8_mod(i8(-7), 2), i8(-1));
            assert_eq!(lean_int8_div(5, 0), 0);
            assert_eq!(lean_int8_mod(5, 0), 5);
            assert_eq!(lean_int8_shift_right(i8(-1), 1), i8(-1));
            assert_eq!(lean_int8_shift_left(1, 7), i8(-128));
            assert_eq!(lean_int8_shift_left(1, i8(-1)), i8(-128));
            assert_eq!(lean_int8_abs(i8(-128)), i8(-128));
            assert_eq!(lean_int8_of_int(lean_cstr_to_int("200")), i8(-56));
            assert_eq!(lean_int8_of_int(lean_cstr_to_int("-200")), 56);
            assert_eq!(lean_int8_of_nat(lean_cstr_to_nat("300")), 44);
            assert_eq!(lean_int64_of_int(lean_cstr_to_int("-18446744073709551617")), u64::MAX);
            assert_eq!(lean_int16_of_int(lean_cstr_to_int("100000000000000000000")), 0);
            assert_eq!(lean_int8_to_int64(i8(-5)), (-5i64) as u64);
            assert_eq!(crate::int::int_to_bigint(lean_int8_to_int(i8(-5))).to_string(), "-5");
            assert_eq!(lean_int64_div(i64::MIN as u64, u64::MAX), i64::MIN as u64);
            assert_eq!(lean_int64_mod(i64::MIN as u64, u64::MAX), 0);
            assert_eq!(lean_int32_to_float((-3i32) as u32), -3.0);
        }
    }
}
