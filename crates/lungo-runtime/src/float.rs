//! `Float` and `Float32`, ported from `lean.h` and the `Float` sections of `runtime/object.cpp`.
//!
//! Transcendental functions are the platform C math library's, exactly as Lean's native
//! backend binds them through `@[extern]`, so results are bit-for-bit those of native Lean on
//! the same platform.

use crate::int::{lean_int_to_int, lean_scalar_to_int};
use crate::object::*;
use num_bigint::Sign;

/// The platform C math library.
mod c {
    #[cfg(all(windows, target_env = "msvc"))]
    pub use self::msvc::{fabsf, frexpf};

    unsafe extern "C" {
        pub fn acos(x: f64) -> f64;
        pub fn acosf(x: f32) -> f32;
        pub fn acosh(x: f64) -> f64;
        pub fn acoshf(x: f32) -> f32;
        pub fn asin(x: f64) -> f64;
        pub fn asinf(x: f32) -> f32;
        pub fn asinh(x: f64) -> f64;
        pub fn asinhf(x: f32) -> f32;
        pub fn atan(x: f64) -> f64;
        pub fn atanf(x: f32) -> f32;
        pub fn atanh(x: f64) -> f64;
        pub fn atanhf(x: f32) -> f32;
        pub fn cbrt(x: f64) -> f64;
        pub fn cbrtf(x: f32) -> f32;
        pub fn ceil(x: f64) -> f64;
        pub fn ceilf(x: f32) -> f32;
        pub fn cos(x: f64) -> f64;
        pub fn cosf(x: f32) -> f32;
        pub fn cosh(x: f64) -> f64;
        pub fn coshf(x: f32) -> f32;
        pub fn exp(x: f64) -> f64;
        pub fn expf(x: f32) -> f32;
        pub fn exp2(x: f64) -> f64;
        pub fn exp2f(x: f32) -> f32;
        pub fn fabs(x: f64) -> f64;
        #[cfg(not(all(windows, target_env = "msvc")))]
        pub fn fabsf(x: f32) -> f32;
        pub fn floor(x: f64) -> f64;
        pub fn floorf(x: f32) -> f32;
        pub fn log(x: f64) -> f64;
        pub fn logf(x: f32) -> f32;
        pub fn log10(x: f64) -> f64;
        pub fn log10f(x: f32) -> f32;
        pub fn log2(x: f64) -> f64;
        pub fn log2f(x: f32) -> f32;
        pub fn round(x: f64) -> f64;
        pub fn roundf(x: f32) -> f32;
        pub fn sin(x: f64) -> f64;
        pub fn sinf(x: f32) -> f32;
        pub fn sinh(x: f64) -> f64;
        pub fn sinhf(x: f32) -> f32;
        pub fn sqrt(x: f64) -> f64;
        pub fn sqrtf(x: f32) -> f32;
        pub fn tan(x: f64) -> f64;
        pub fn tanf(x: f32) -> f32;
        pub fn tanh(x: f64) -> f64;
        pub fn tanhf(x: f32) -> f32;
        pub fn atan2(x: f64, y: f64) -> f64;
        pub fn atan2f(x: f32, y: f32) -> f32;
        pub fn pow(x: f64, y: f64) -> f64;
        pub fn powf(x: f32, y: f32) -> f32;
        pub fn frexp(x: f64, e: *mut core::ffi::c_int) -> f64;
        #[cfg(not(all(windows, target_env = "msvc")))]
        pub fn frexpf(x: f32, e: *mut core::ffi::c_int) -> f32;
        pub fn scalbn(x: f64, n: core::ffi::c_int) -> f64;
        pub fn scalbnf(x: f32, n: core::ffi::c_int) -> f32;
    }

    /// The Universal CRT exports no `fabsf` or `frexpf` on x64: its `<math.h>` defines them
    /// inline, through the `double` functions, and so do these. Both are exact, since every
    /// `float`, its magnitude and its `frexp` mantissa are exactly representable as `double`.
    #[cfg(all(windows, target_env = "msvc"))]
    mod msvc {
        pub unsafe fn fabsf(x: f32) -> f32 {
            unsafe { super::fabs(f64::from(x)) as f32 }
        }

        pub unsafe fn frexpf(x: f32, e: *mut core::ffi::c_int) -> f32 {
            unsafe { super::frexp(f64::from(x), e) as f32 }
        }
    }
}

/// Lean's `Float.toString`: C++ `std::to_string`, i.e. `printf("%f")`, except that every NaN
/// renders as `NaN` so that NaN payloads and signs are not observable.
pub fn float_to_string(a: f64) -> String {
    if a.is_nan() {
        "NaN".to_owned()
    } else if a.is_infinite() {
        if a < 0.0 { "-inf".to_owned() } else { "inf".to_owned() }
    } else {
        // Both Rust's and C's fixed-point formatting print the exactly rounded decimal value
        // (round half to even on the exact binary value), including the sign of negative zero.
        format!("{a:.6}")
    }
}

const QUIET_NAN64: u64 = 0x7ff8_0000_0000_0000;
const QUIET_NAN32: u32 = 0x7fc0_0000;

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn acos(x: f64) -> f64 { c::acos(x) }
        fn acosf(x: f32) -> f32 { c::acosf(x) }
        fn acosh(x: f64) -> f64 { c::acosh(x) }
        fn acoshf(x: f32) -> f32 { c::acoshf(x) }
        fn asin(x: f64) -> f64 { c::asin(x) }
        fn asinf(x: f32) -> f32 { c::asinf(x) }
        fn asinh(x: f64) -> f64 { c::asinh(x) }
        fn asinhf(x: f32) -> f32 { c::asinhf(x) }
        fn atan(x: f64) -> f64 { c::atan(x) }
        fn atanf(x: f32) -> f32 { c::atanf(x) }
        fn atanh(x: f64) -> f64 { c::atanh(x) }
        fn atanhf(x: f32) -> f32 { c::atanhf(x) }
        fn cbrt(x: f64) -> f64 { c::cbrt(x) }
        fn cbrtf(x: f32) -> f32 { c::cbrtf(x) }
        fn ceil(x: f64) -> f64 { c::ceil(x) }
        fn ceilf(x: f32) -> f32 { c::ceilf(x) }
        fn cos(x: f64) -> f64 { c::cos(x) }
        fn cosf(x: f32) -> f32 { c::cosf(x) }
        fn cosh(x: f64) -> f64 { c::cosh(x) }
        fn coshf(x: f32) -> f32 { c::coshf(x) }
        fn exp(x: f64) -> f64 { c::exp(x) }
        fn expf(x: f32) -> f32 { c::expf(x) }
        fn exp2(x: f64) -> f64 { c::exp2(x) }
        fn exp2f(x: f32) -> f32 { c::exp2f(x) }
        fn fabs(x: f64) -> f64 { c::fabs(x) }
        fn fabsf(x: f32) -> f32 { c::fabsf(x) }
        fn floor(x: f64) -> f64 { c::floor(x) }
        fn floorf(x: f32) -> f32 { c::floorf(x) }
        fn log(x: f64) -> f64 { c::log(x) }
        fn logf(x: f32) -> f32 { c::logf(x) }
        fn log10(x: f64) -> f64 { c::log10(x) }
        fn log10f(x: f32) -> f32 { c::log10f(x) }
        fn log2(x: f64) -> f64 { c::log2(x) }
        fn log2f(x: f32) -> f32 { c::log2f(x) }
        fn round(x: f64) -> f64 { c::round(x) }
        fn roundf(x: f32) -> f32 { c::roundf(x) }
        fn sin(x: f64) -> f64 { c::sin(x) }
        fn sinf(x: f32) -> f32 { c::sinf(x) }
        fn sinh(x: f64) -> f64 { c::sinh(x) }
        fn sinhf(x: f32) -> f32 { c::sinhf(x) }
        fn sqrt(x: f64) -> f64 { c::sqrt(x) }
        fn sqrtf(x: f32) -> f32 { c::sqrtf(x) }
        fn tan(x: f64) -> f64 { c::tan(x) }
        fn tanf(x: f32) -> f32 { c::tanf(x) }
        fn tanh(x: f64) -> f64 { c::tanh(x) }
        fn tanhf(x: f32) -> f32 { c::tanhf(x) }
        fn atan2(x: f64, y: f64) -> f64 { c::atan2(x, y) }
        fn atan2f(x: f32, y: f32) -> f32 { c::atan2f(x, y) }
        fn pow(x: f64, y: f64) -> f64 { c::pow(x, y) }
        fn powf(x: f32, y: f32) -> f32 { c::powf(x, y) }

        // ---- float ----
        fn lean_float_add(a: f64, b: f64) -> f64 { a + b }
        fn lean_float_sub(a: f64, b: f64) -> f64 { a - b }
        fn lean_float_mul(a: f64, b: f64) -> f64 { a * b }
        fn lean_float_div(a: f64, b: f64) -> f64 { a / b }
        fn lean_float_negate(a: f64) -> f64 { -a }
        fn lean_float_beq(a: f64, b: f64) -> u8 { (a == b) as u8 }
        fn lean_float_decLe(a: f64, b: f64) -> u8 { (a <= b) as u8 }
        fn lean_float_decLt(a: f64, b: f64) -> u8 { (a < b) as u8 }
        fn lean_float_isnan(a: f64) -> u8 { a.is_nan() as u8 }
        fn lean_float_isfinite(a: f64) -> u8 { a.is_finite() as u8 }
        fn lean_float_isinf(a: f64) -> u8 { a.is_infinite() as u8 }
        fn lean_float_to_string(a: f64) -> obj { lean_mk_string(&float_to_string(a as f64)) }
        fn lean_float_of_bits(u: u64) -> f64 {
            let r = f64::from_bits(u);
            if r.is_nan() { f64::from_bits(QUIET_NAN64) } else { r }
        }
        fn lean_float_to_bits(d: f64) -> u64 { if d.is_nan() { QUIET_NAN64 } else { d.to_bits() } }
        fn lean_float_to_float32(a: f64) -> f32 { a as f32 }
        fn lean_float_to_uint8(a: f64) -> u8 { if 0.0 <= a { if a < 256.0 { a as u8 } else { u8::MAX } } else { 0 } }
        fn lean_float_to_uint16(a: f64) -> u16 { if 0.0 <= a { if a < 65536.0 { a as u16 } else { u16::MAX } } else { 0 } }
        fn lean_float_to_uint32(a: f64) -> u32 { if 0.0 <= a { if (a as f64) < 4294967296.0 { a as u32 } else { u32::MAX } } else { 0 } }
        fn lean_float_to_uint64(a: f64) -> u64 { if 0.0 <= a { if (a as f64) < 18446744073709551616.0 { a as u64 } else { u64::MAX } } else { 0 } }
        fn lean_float_to_usize(a: f64) -> usize {
            if usize::BITS == 64 { lean_float_to_uint64(a) as usize } else { lean_float_to_uint32(a) as usize }
        }
        fn lean_float_to_int8(a: f64) -> u8 {
            let r: i8 = if a.is_nan() { 0 } else if -129.0 < a { if a < 128.0 { a as i8 } else { i8::MAX } } else { i8::MIN };
            r as u8
        }
        fn lean_float_to_int16(a: f64) -> u16 {
            let r: i16 = if a.is_nan() { 0 } else if -32769.0 < a { if a < 32768.0 { a as i16 } else { i16::MAX } } else { i16::MIN };
            r as u16
        }
        fn lean_float_to_int32(a: f64) -> u32 {
            let a = a as f64;
            let r: i32 = if a.is_nan() { 0 } else if -2147483649.0 < a { if a < 2147483648.0 { a as i32 } else { i32::MAX } } else { i32::MIN };
            r as u32
        }
        fn lean_float_to_int64(a: f64) -> u64 {
            let a = a as f64;
            let r: i64 = if a.is_nan() { 0 } else if -9223372036854775809.0 < a { if a < 9223372036854775808.0 { a as i64 } else { i64::MAX } } else { i64::MIN };
            r as u64
        }
        fn lean_float_to_isize(a: f64) -> usize {
            if usize::BITS == 64 { lean_float_to_int64(a) as usize } else { lean_float_to_int32(a) as i32 as isize as usize }
        }
        fn lean_float_scaleb(a: f64, b: b_obj) -> f64 {
            if b.is_scalar() {
                c::scalbn(a, lean_scalar_to_int(b))
            } else if a == 0.0 || mpz_value(b).sign() == Sign::Minus {
                0.0
            } else {
                a as f64 * f64::INFINITY
            }
        }
        fn lean_float_frexp(a: f64) -> obj {
            let mut exp: core::ffi::c_int = 0;
            let m = c::frexp(a, &mut exp);
            let r = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(r, 0, lean_box_float(m));
            lean_ctor_set(r, 1, if a.is_finite() { lean_int_to_int(exp) } else { lean_box(0) });
            r
        }

        // ---- float32 ----
        fn lean_float32_add(a: f32, b: f32) -> f32 { a + b }
        fn lean_float32_sub(a: f32, b: f32) -> f32 { a - b }
        fn lean_float32_mul(a: f32, b: f32) -> f32 { a * b }
        fn lean_float32_div(a: f32, b: f32) -> f32 { a / b }
        fn lean_float32_negate(a: f32) -> f32 { -a }
        fn lean_float32_beq(a: f32, b: f32) -> u8 { (a == b) as u8 }
        fn lean_float32_decLe(a: f32, b: f32) -> u8 { (a <= b) as u8 }
        fn lean_float32_decLt(a: f32, b: f32) -> u8 { (a < b) as u8 }
        fn lean_float32_isnan(a: f32) -> u8 { a.is_nan() as u8 }
        fn lean_float32_isfinite(a: f32) -> u8 { a.is_finite() as u8 }
        fn lean_float32_isinf(a: f32) -> u8 { a.is_infinite() as u8 }
        fn lean_float32_to_string(a: f32) -> obj { lean_mk_string(&float_to_string(a as f64)) }
        fn lean_float32_of_bits(u: u32) -> f32 {
            let r = f32::from_bits(u);
            if r.is_nan() { f32::from_bits(QUIET_NAN32) } else { r }
        }
        fn lean_float32_to_bits(d: f32) -> u32 { if d.is_nan() { QUIET_NAN32 } else { d.to_bits() } }
        fn lean_float32_to_float(a: f32) -> f64 { a as f64 }
        fn lean_float32_to_uint8(a: f32) -> u8 { if 0.0 <= a { if a < 256.0 { a as u8 } else { u8::MAX } } else { 0 } }
        fn lean_float32_to_uint16(a: f32) -> u16 { if 0.0 <= a { if a < 65536.0 { a as u16 } else { u16::MAX } } else { 0 } }
        fn lean_float32_to_uint32(a: f32) -> u32 { if 0.0 <= a { if (a as f64) < 4294967296.0 { a as u32 } else { u32::MAX } } else { 0 } }
        fn lean_float32_to_uint64(a: f32) -> u64 { if 0.0 <= a { if (a as f64) < 18446744073709551616.0 { a as u64 } else { u64::MAX } } else { 0 } }
        fn lean_float32_to_usize(a: f32) -> usize {
            if usize::BITS == 64 { lean_float32_to_uint64(a) as usize } else { lean_float32_to_uint32(a) as usize }
        }
        fn lean_float32_to_int8(a: f32) -> u8 {
            let r: i8 = if a.is_nan() { 0 } else if -129.0 < a { if a < 128.0 { a as i8 } else { i8::MAX } } else { i8::MIN };
            r as u8
        }
        fn lean_float32_to_int16(a: f32) -> u16 {
            let r: i16 = if a.is_nan() { 0 } else if -32769.0 < a { if a < 32768.0 { a as i16 } else { i16::MAX } } else { i16::MIN };
            r as u16
        }
        fn lean_float32_to_int32(a: f32) -> u32 {
            let a = a as f64;
            let r: i32 = if a.is_nan() { 0 } else if -2147483649.0 < a { if a < 2147483648.0 { a as i32 } else { i32::MAX } } else { i32::MIN };
            r as u32
        }
        fn lean_float32_to_int64(a: f32) -> u64 {
            let a = a as f64;
            let r: i64 = if a.is_nan() { 0 } else if -9223372036854775809.0 < a { if a < 9223372036854775808.0 { a as i64 } else { i64::MAX } } else { i64::MIN };
            r as u64
        }
        fn lean_float32_to_isize(a: f32) -> usize {
            if usize::BITS == 64 { lean_float32_to_int64(a) as usize } else { lean_float32_to_int32(a) as i32 as isize as usize }
        }
        fn lean_float32_scaleb(a: f32, b: b_obj) -> f32 {
            if b.is_scalar() {
                c::scalbnf(a, lean_scalar_to_int(b))
            } else if a == 0.0 || mpz_value(b).sign() == Sign::Minus {
                0.0
            } else {
                (a as f64 * f64::INFINITY) as f32
            }
        }
        fn lean_float32_frexp(a: f32) -> obj {
            let mut exp: core::ffi::c_int = 0;
            let m = c::frexpf(a, &mut exp);
            let r = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(r, 0, lean_box_float32(m));
            lean_ctor_set(r, 1, if a.is_finite() { lean_int_to_int(exp) } else { lean_box(0) });
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::float_to_string;
    use crate::object::*;

    // Expected values computed with Lean 4.34.1 (`lean --run`).

    #[test]
    fn to_string_matches_lean() {
        assert_eq!(float_to_string(0.0), "0.000000");
        assert_eq!(float_to_string(-0.0), "-0.000000");
        assert_eq!(float_to_string(f64::NAN), "NaN");
        assert_eq!(float_to_string(-f64::NAN), "NaN");
        assert_eq!(float_to_string(f64::INFINITY), "inf");
        assert_eq!(float_to_string(f64::NEG_INFINITY), "-inf");
        assert_eq!(float_to_string(1e21), "1000000000000000000000.000000");
        assert_eq!(float_to_string(0.1), "0.100000");
        assert_eq!(float_to_string(5e-324), "0.000000");
        assert_eq!(float_to_string(1e-7), "0.000000");
        assert_eq!(float_to_string(0.1f32 as f64), "0.100000");
        assert_eq!(float_to_string(1e21f32 as f64), "1000000020040877342720.000000");
        assert_eq!(float_to_string(18446744073709551615u64 as f64), "18446744073709551616.000000");
        unsafe {
            assert_eq!(round(2.5), 3.0);
            assert_eq!(round(-2.5), -3.0);
        }
    }

    #[test]
    fn saturating_conversions() {
        unsafe {
            assert_eq!(lean_float_to_uint8(300.7), 255);
            assert_eq!(lean_float_to_uint8(-3.7), 0);
            assert_eq!(lean_float_to_uint8(f64::NAN), 0);
            assert_eq!(lean_float_to_int8(-129.5), (-128i8) as u8);
            assert_eq!(lean_float_to_int8(127.9), 127);
            assert_eq!(lean_float_to_int64(1e30), i64::MAX as u64);
            assert_eq!(lean_float_to_int64(f64::NAN), 0);
            assert_eq!(lean_float_to_uint64(1e30), u64::MAX);
            assert_eq!(lean_float_to_int32(-1e30), i32::MIN as u32);
        }
    }

    #[test]
    fn bits_frexp_and_scaleb() {
        unsafe {
            assert_eq!(lean_float_to_bits(f64::NAN), 9221120237041090560);
            assert_eq!(lean_float_to_bits(lean_float_of_bits(0xfff0000000000001)), 9221120237041090560);
            let r = lean_float_frexp(1.5);
            assert_eq!(lean_unbox_float(lean_ctor_get(r, 0)), 0.75);
            assert_eq!(lean_ctor_get(r, 1), lean_box(1));
            let r = lean_float_frexp(f64::INFINITY);
            assert_eq!(lean_unbox_float(lean_ctor_get(r, 0)), f64::INFINITY);
            assert_eq!(lean_ctor_get(r, 1), lean_box(0));
            assert_eq!(lean_float_scaleb(1.0, lean_box(10)), 1024.0);
            let minus_1100 = crate::int::lean_int64_to_int(-1100);
            assert_eq!(lean_float_scaleb(1.0, minus_1100), 0.0);
            let huge = crate::int::lean_cstr_to_int("100000000000000000000");
            assert_eq!(lean_float_scaleb(1.0, huge), f64::INFINITY);
            let neg_huge = crate::int::lean_cstr_to_int("-100000000000000000000");
            assert_eq!(lean_float_scaleb(1.0, neg_huge), 0.0);
        }
    }

    #[test]
    fn libm_matches_lean() {
        unsafe {
            assert_eq!(float_to_string(sqrt(2.0)), "1.414214");
            assert_eq!(float_to_string(exp(1.0)), "2.718282");
            assert_eq!(float_to_string(log(10.0)), "2.302585");
            assert_eq!(float_to_string(sin(1.0)), "0.841471");
            assert_eq!(float_to_string(pow(2.0, 0.5)), "1.414214");
            assert_eq!(float_to_string(cbrt(27.0)), "3.000000");
            assert_eq!(float_to_string(atan2(1.0, -1.0)), "2.356194");
        }
    }
}
