//! `String` primitives, ported from `runtime/object.cpp`, `runtime/utf8.cpp` and `lean.h`.
//!
//! Strings are UTF-8 byte buffers with a terminating NUL, their byte size (including the NUL)
//! and their length in Unicode scalar values. Positions are byte offsets (`String.Pos.Raw`, a
//! `Nat`); a non-scalar position exceeds `LEAN_MAX_SMALL_NAT` and is therefore out of range for
//! every string that fits in memory, exactly as the C runtime assumes.
//!
//! Symbols such as `lean_string_any` or `lean_substring_extract` are implemented by `@[export]`
//! Lean declarations in `Init`, not by the C runtime; they are compiled from Lean like any other
//! code and are therefore not primitives here.

use crate::hash::hash_str;
use crate::object::*;
use num_bigint::BigInt;

/// `lean_char_default_value`: `instance : Inhabited Char := ⟨'A'⟩`.
pub const CHAR_DEFAULT: u32 = 'A' as u32;

// ---------------------------------------------------------------------------------------------
// Canonical natural numbers used for positions
// ---------------------------------------------------------------------------------------------

fn big_to_nat(v: BigInt) -> Obj {
    match usize::try_from(&v) {
        Ok(n) if n <= LEAN_MAX_SMALL_NAT => lean_box(n),
        _ => alloc_mpz(v),
    }
}

/// `n + d` for a non-scalar `Nat` `n` (borrowed).
unsafe fn big_nat_add(n: Obj, d: u32) -> Obj {
    unsafe { big_to_nat(mpz_value(n) + BigInt::from(d)) }
}

/// `n - 1` for a non-scalar `Nat` `n` (borrowed); non-scalar values are at least 2^63.
unsafe fn big_nat_pred(n: Obj) -> Obj {
    unsafe { big_to_nat(mpz_value(n) - BigInt::from(1u32)) }
}

// ---------------------------------------------------------------------------------------------
// UTF-8
// ---------------------------------------------------------------------------------------------

/// Lean's `get_utf8_size`: the byte length announced by a lead byte (1 for invalid bytes).
#[inline]
pub fn get_utf8_size(c: u8) -> usize {
    if c & 0x80 == 0 {
        1
    } else if c & 0xe0 == 0xc0 {
        2
    } else if c & 0xf0 == 0xe0 {
        3
    } else if c & 0xf8 == 0xf0 {
        4
    } else if c & 0xfc == 0xf8 {
        5
    } else if c & 0xfe == 0xfc {
        6
    } else {
        1
    }
}

#[inline]
fn is_utf8_first_byte(c: u8) -> bool {
    (c & 0x80) == 0 || (c & 0xe0) == 0xc0 || (c & 0xf0) == 0xe0 || (c & 0xf8) == 0xf0
}

/// Lean's `lean_utf8_n_strlen`: the number of scalar values in `s` counted by lead bytes.
pub fn utf8_strlen(s: &[u8]) -> usize {
    let mut r = 0;
    let mut i = 0;
    while i < s.len() {
        i += get_utf8_size(s[i]);
        r += 1;
    }
    r
}

/// Appends the UTF-8 encoding of `code` to `out`, as Lean's `push_unicode_scalar`.
pub fn push_unicode_scalar(out: &mut Vec<u8>, code: u32) -> usize {
    if code < 0x80 {
        out.push(code as u8);
        1
    } else if code < 0x800 {
        out.push(((code >> 6) & 0x1f) as u8 | 0xc0);
        out.push((code & 0x3f) as u8 | 0x80);
        2
    } else if code < 0x10000 {
        out.push(((code >> 12) & 0x0f) as u8 | 0xe0);
        out.push(((code >> 6) & 0x3f) as u8 | 0x80);
        out.push((code & 0x3f) as u8 | 0x80);
        3
    } else {
        out.push(((code >> 18) & 0x07) as u8 | 0xf0);
        out.push(((code >> 12) & 0x3f) as u8 | 0x80);
        out.push(((code >> 6) & 0x3f) as u8 | 0x80);
        out.push((code & 0x3f) as u8 | 0x80);
        4
    }
}

/// Decodes the scalar value at `i` of `s` (`lean_string_utf8_get_core`).
fn utf8_get_core(s: &[u8], i: usize) -> Option<u32> {
    let size = s.len();
    let c = s[i] as u32;
    if c & 0x80 == 0 {
        return Some(c);
    }
    if (c & 0xe0) == 0xc0 && i + 1 < size {
        let c1 = s[i + 1] as u32;
        let r = ((c & 0x1f) << 6) | (c1 & 0x3f);
        if r >= 0x80 {
            return Some(r);
        }
    }
    if (c & 0xf0) == 0xe0 && i + 2 < size {
        let c1 = s[i + 1] as u32;
        let c2 = s[i + 2] as u32;
        let r = ((c & 0x0f) << 12) | ((c1 & 0x3f) << 6) | (c2 & 0x3f);
        if r >= 0x800 && !(0xd800..=0xdfff).contains(&r) {
            return Some(r);
        }
    }
    if (c & 0xf8) == 0xf0 && i + 3 < size {
        let c1 = s[i + 1] as u32;
        let c2 = s[i + 2] as u32;
        let c3 = s[i + 3] as u32;
        let r = ((c & 0x07) << 18) | ((c1 & 0x3f) << 12) | ((c2 & 0x3f) << 6) | (c3 & 0x3f);
        if (0x10000..=0x10ffff).contains(&r) {
            return Some(r);
        }
    }
    None
}

/// Lean's `next_utf8`: decodes at `*i` and advances; an invalid byte decodes to itself.
fn next_utf8(s: &[u8], i: &mut usize) -> u32 {
    match utf8_get_core(s, *i) {
        Some(r) => {
            *i += if r < 0x80 {
                1
            } else if r < 0x800 {
                2
            } else if r < 0x10000 {
                3
            } else {
                4
            };
            r
        }
        None => {
            let c = s[*i] as u32;
            *i += 1;
            c
        }
    }
}

/// Validates `s` as UTF-8 exactly as Lean's `validate_utf8`, returning the character count.
pub fn validate_utf8(s: &[u8]) -> Option<usize> {
    let mut pos = 0;
    let mut n = 0;
    while pos < s.len() {
        pos = crate::utf8::validate_one(s, pos)?;
        n += 1;
    }
    Some(n)
}

// ---------------------------------------------------------------------------------------------
// Construction helpers
// ---------------------------------------------------------------------------------------------

/// A string object for a literal whose UTF-8 bytes and character count are known.
pub unsafe fn lean_mk_string_lit(bytes: &[u8], len_chars: usize) -> Obj {
    unsafe { lean_mk_string_unchecked(bytes, len_chars) }
}

/// A string object from bytes known to be valid UTF-8, counting scalar values by lead bytes
/// (`lean_mk_string_from_bytes_unchecked`).
pub unsafe fn lean_mk_string_from_bytes_unchecked(bytes: &[u8]) -> Obj {
    unsafe { lean_mk_string_unchecked(bytes, utf8_strlen(bytes)) }
}

/// Converts a Rust string into a Lean `String` (owned result).
pub fn string_from_rust(s: &str) -> Obj {
    lean_mk_string(s)
}

/// Copies a Lean `String` (borrowed) into a Rust `String`.
pub unsafe fn string_to_rust(o: Obj) -> String {
    unsafe { lean_string_str(o).to_owned() }
}

fn mk_capacity(sz: usize) -> usize {
    sz.checked_mul(2).unwrap_or_else(|| lean_internal_panic_overflow())
}

/// Ensures the exclusive string `o` can hold `extra` more bytes, reallocating if needed.
unsafe fn string_ensure_capacity(o: Obj, extra: usize) -> Obj {
    unsafe {
        let sz = lean_string_size(o);
        let cap = lean_string_capacity(o);
        let needed = sz.checked_add(extra).unwrap_or_else(|| lean_internal_panic_overflow());
        if needed > cap {
            let new_cap = cap.checked_add(needed).unwrap_or_else(|| lean_internal_panic_overflow());
            let r = lean_alloc_string(sz, new_cap, lean_string_len(o));
            std::ptr::copy_nonoverlapping(lean_string_cstr(o), lean_string_cstr(r), sz);
            lean_free_object(o);
            r
        } else {
            o
        }
    }
}

unsafe fn empty_string() -> Obj {
    unsafe { lean_mk_string_unchecked(b"", 0) }
}

// ---------------------------------------------------------------------------------------------
// Operations shared with other modules
// ---------------------------------------------------------------------------------------------

/// `lean_string_eq`.
pub unsafe fn lean_string_eq(s1: Obj, s2: Obj) -> bool {
    unsafe {
        s1 == s2 || (lean_string_size(s1) == lean_string_size(s2) && lean_string_bytes(s1) == lean_string_bytes(s2))
    }
}

/// `lean_string_lt`: bytewise lexicographic order.
pub unsafe fn lean_string_lt(s1: Obj, s2: Obj) -> bool {
    unsafe { lean_string_bytes(s1) < lean_string_bytes(s2) }
}

/// The byte range `[start, end)` of a `String.Slice` (fields: string, start, end).
unsafe fn slice_bytes<'a>(slice: Obj) -> &'a [u8] {
    unsafe {
        let s = lean_ctor_get(slice, 0);
        let start = lean_ctor_get(slice, 1);
        let end = lean_ctor_get(slice, 2);
        if !start.is_scalar() || !end.is_scalar() {
            lean_internal_panic("String.Slice bounds are not valid positions");
        }
        match lean_string_bytes(s).get(lean_unbox(start)..lean_unbox(end)) {
            Some(b) => b,
            None => lean_internal_panic("String.Slice bounds exceed the string"),
        }
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_string_push(s: obj, c: u32) -> obj {
            let sz = lean_string_size(s);
            let len = lean_string_len(s);
            let r = if !lean_is_exclusive(s) {
                let r = lean_alloc_string(sz, mk_capacity(sz + 5), len);
                std::ptr::copy_nonoverlapping(lean_string_cstr(s), lean_string_cstr(r), sz - 1);
                lean_dec_ref(s);
                r
            } else {
                string_ensure_capacity(s, 5)
            };
            let mut enc = Vec::with_capacity(4);
            let consumed = push_unicode_scalar(&mut enc, c);
            let data = lean_string_cstr(r);
            std::ptr::copy_nonoverlapping(enc.as_ptr(), data.add(sz - 1), consumed);
            *data.add(sz + consumed - 1) = 0;
            lean_string_set_size_len(r, sz + consumed, len + 1);
            r
        }

        fn lean_string_append(s1: obj, s2: b_obj) -> obj {
            let sz1 = lean_string_size(s1);
            let sz2 = lean_string_size(s2);
            let new_len = lean_string_len(s1) + lean_string_len(s2);
            let new_sz = sz1 + sz2 - 1;
            let r = if !lean_is_exclusive(s1) {
                let r = lean_alloc_string(new_sz, mk_capacity(new_sz), new_len);
                std::ptr::copy_nonoverlapping(lean_string_cstr(s1), lean_string_cstr(r), sz1 - 1);
                lean_dec_ref(s1);
                r
            } else {
                if s1 == s2 {
                    lean_internal_panic("lean_string_append: an exclusive string appended to itself");
                }
                string_ensure_capacity(s1, sz2 - 1)
            };
            let data = lean_string_cstr(r);
            std::ptr::copy_nonoverlapping(lean_string_cstr(s2), data.add(sz1 - 1), sz2 - 1);
            *data.add(new_sz - 1) = 0;
            lean_string_set_size_len(r, new_sz, new_len);
            r
        }

        fn lean_string_length(s: b_obj) -> obj {
            lean_box(lean_string_len(s))
        }

        fn lean_string_utf8_byte_size(s: b_obj) -> obj {
            lean_box(lean_string_size(s) - 1)
        }

        fn lean_string_dec_eq(s1: b_obj, s2: b_obj) -> u8 {
            lean_string_eq(s1, s2) as u8
        }

        fn lean_string_dec_lt(s1: b_obj, s2: b_obj) -> u8 {
            lean_string_lt(s1, s2) as u8
        }

        // Constructor indices of `Ordering`: lt = 0, eq = 1, gt = 2.
        fn lean_string_compare(s1: b_obj, s2: b_obj) -> u8 {
            match lean_string_bytes(s1).cmp(lean_string_bytes(s2)) {
                std::cmp::Ordering::Less => 0,
                std::cmp::Ordering::Equal => 1,
                std::cmp::Ordering::Greater => 2,
            }
        }

        fn lean_string_hash(s: b_obj) -> u64 {
            hash_str(lean_string_bytes(s), 11)
        }

        fn lean_string_of_usize(n: usize) -> obj {
            let s = n.to_string();
            lean_mk_string_unchecked(s.as_bytes(), s.len())
        }

        fn lean_string_mk(cs: obj) -> obj {
            let mut bytes = Vec::new();
            let mut len = 0;
            let mut o = cs;
            while !o.is_scalar() {
                push_unicode_scalar(&mut bytes, lean_unbox_uint32(lean_ctor_get(o, 0)));
                o = lean_ctor_get(o, 1);
                len += 1;
            }
            lean_dec(cs);
            lean_mk_string_unchecked(&bytes, len)
        }

        fn lean_string_data(s: obj) -> obj {
            let bytes = lean_string_bytes(s).to_vec();
            lean_dec_ref(s);
            let mut chars = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                chars.push(next_utf8(&bytes, &mut i));
            }
            let mut r = lean_box(0);
            for c in chars.into_iter().rev() {
                let cell = lean_alloc_ctor(1, 2, 0);
                lean_ctor_set(cell, 0, lean_box_uint32(c));
                lean_ctor_set(cell, 1, r);
                r = cell;
            }
            r
        }

        fn lean_string_utf8_get(s: b_obj, i0: b_obj) -> u32 {
            if !i0.is_scalar() {
                return CHAR_DEFAULT;
            }
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i >= bytes.len() {
                return CHAR_DEFAULT;
            }
            utf8_get_core(bytes, i).unwrap_or(CHAR_DEFAULT)
        }

        fn lean_string_utf8_get_fast(s: b_obj, i0: b_obj) -> u32 {
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i >= bytes.len() {
                lean_internal_panic("String.Pos.Raw.get': position is not in range");
            }
            utf8_get_core(bytes, i).unwrap_or(CHAR_DEFAULT)
        }

        fn lean_string_utf8_get_opt(s: b_obj, i0: b_obj) -> obj {
            if !i0.is_scalar() {
                return lean_box(0);
            }
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i >= bytes.len() {
                return lean_box(0);
            }
            match utf8_get_core(bytes, i) {
                Some(c) => {
                    let r = lean_alloc_ctor(1, 1, 0);
                    lean_ctor_set(r, 0, lean_box_uint32(c));
                    r
                }
                None => lean_box(0),
            }
        }

        fn lean_string_utf8_get_bang(s: b_obj, i0: b_obj) -> u32 {
            let bytes = lean_string_bytes(s);
            let c = if i0.is_scalar() && lean_unbox(i0) < bytes.len() {
                utf8_get_core(bytes, lean_unbox(i0))
            } else {
                None
            };
            match c {
                Some(c) => c,
                None => {
                    crate::panic::lean_panic("Error: invalid `String.Pos` at `String.get!`");
                    CHAR_DEFAULT
                }
            }
        }

        fn lean_string_utf8_next(s: b_obj, i0: b_obj) -> obj {
            if !i0.is_scalar() {
                return big_nat_add(i0, 1);
            }
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i >= bytes.len() {
                return crate::nat::lean_usize_to_nat(i + 1);
            }
            let c = bytes[i];
            if c & 0x80 == 0 {
                lean_box(i + 1)
            } else if c & 0xe0 == 0xc0 {
                lean_box(i + 2)
            } else if c & 0xf0 == 0xe0 {
                lean_box(i + 3)
            } else if c & 0xf8 == 0xf0 {
                lean_box(i + 4)
            } else {
                lean_box(i + 1)
            }
        }

        fn lean_string_utf8_next_fast(s: b_obj, i0: b_obj) -> obj {
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i >= bytes.len() {
                lean_internal_panic("String.Pos.Raw.next': position is not in range");
            }
            let c = bytes[i];
            if c & 0x80 == 0 {
                lean_box(i + 1)
            } else if c & 0xe0 == 0xc0 {
                lean_box(i + 2)
            } else if c & 0xf0 == 0xe0 {
                lean_box(i + 3)
            } else if c & 0xf8 == 0xf0 {
                lean_box(i + 4)
            } else {
                lean_box(i + 1)
            }
        }

        fn lean_string_utf8_prev(s: b_obj, i0: b_obj) -> obj {
            if !i0.is_scalar() {
                return big_nat_pred(i0);
            }
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i == 0 {
                return lean_box(0);
            }
            if i > bytes.len() {
                return lean_box(i - 1);
            }
            let mut i = i - 1;
            while !is_utf8_first_byte(bytes[i]) {
                if i == 0 {
                    lean_internal_panic("String.Pos.Raw.prev: string does not start with a lead byte");
                }
                i -= 1;
            }
            lean_box(i)
        }

        fn lean_string_is_valid_pos(s: b_obj, i0: b_obj) -> u8 {
            if !i0.is_scalar() {
                return 0;
            }
            let i = lean_unbox(i0);
            let bytes = lean_string_bytes(s);
            if i > bytes.len() {
                0
            } else if i == bytes.len() {
                1
            } else {
                is_utf8_first_byte(bytes[i]) as u8
            }
        }

        fn lean_string_utf8_at_end(s: b_obj, i0: b_obj) -> u8 {
            (!i0.is_scalar() || lean_unbox(i0) >= lean_string_size(s) - 1) as u8
        }

        fn lean_string_utf8_extract(s: b_obj, b0: b_obj, e0: b_obj) -> obj {
            let b = if b0.is_scalar() { lean_unbox(b0) } else { usize::MAX };
            let mut e = if e0.is_scalar() { lean_unbox(e0) } else { usize::MAX };
            let bytes = lean_string_bytes(s);
            let sz = bytes.len();
            if b >= e || b >= sz {
                return empty_string();
            }
            if !is_utf8_first_byte(bytes[b]) {
                return empty_string();
            }
            if e > sz {
                e = sz;
            }
            if e < sz && !is_utf8_first_byte(bytes[e]) {
                e = sz;
            }
            lean_mk_string_from_bytes_unchecked(&bytes[b..e])
        }

        fn lean_string_utf8_extract_fast(s: b_obj, b0: b_obj, e0: b_obj) -> obj {
            let b = lean_unbox(b0);
            let e = lean_unbox(e0);
            let bytes = lean_string_bytes(s);
            if !b0.is_scalar() || !e0.is_scalar() || b > bytes.len() || e > bytes.len() {
                lean_internal_panic("String.extract: positions are not in range");
            }
            if b >= e {
                return empty_string();
            }
            lean_mk_string_from_bytes_unchecked(&bytes[b..e])
        }

        fn lean_string_utf8_set(s: obj, i0: b_obj, c: u32) -> obj {
            if !i0.is_scalar() {
                return s;
            }
            let i = lean_unbox(i0);
            let sz = lean_string_size(s) - 1;
            if i >= sz {
                return s;
            }
            let data = lean_string_cstr(s);
            if lean_is_exclusive(s) && *data.add(i) < 128 && c < 128 {
                *data.add(i) = c as u8;
                return s;
            }
            if !is_utf8_first_byte(*data.add(i)) {
                return s;
            }
            let mut new_s = lean_string_bytes(s).to_vec();
            let len = lean_string_len(s);
            lean_dec(s);
            let old = match get_utf8_size(new_s[i]) {
                n @ 1..=4 => n,
                _ => 1,
            };
            let mut enc = Vec::with_capacity(4);
            push_unicode_scalar(&mut enc, c);
            let end = (i + old).min(new_s.len());
            new_s.splice(i..end, enc);
            lean_mk_string_unchecked(&new_s, len)
        }

        fn lean_string_get_byte_fast(s: b_obj, i0: b_obj) -> u8 {
            let i = lean_unbox(i0);
            if !i0.is_scalar() || i >= lean_string_size(s) {
                lean_internal_panic("String.getUTF8Byte: index out of range");
            }
            *lean_string_cstr(s).add(i)
        }

        fn lean_string_uget_byte_fast(s: b_obj, i: usize) -> u8 {
            if i >= lean_string_size(s) {
                lean_internal_panic("String.ugetUTF8Byte: index out of range");
            }
            *lean_string_cstr(s).add(i)
        }

        fn lean_string_memcmp(s1: b_obj, s2: b_obj, lstart: b_obj, rstart: b_obj, len: b_obj) -> u8 {
            if !lstart.is_scalar() || !rstart.is_scalar() || !len.is_scalar() {
                lean_internal_panic("String memcmp: bounds are not valid positions");
            }
            let (l, r, n) = (lean_unbox(lstart), lean_unbox(rstart), lean_unbox(len));
            match (lean_string_bytes(s1).get(l..l.saturating_add(n)), lean_string_bytes(s2).get(r..r.saturating_add(n))) {
                (Some(a), Some(b)) => (a == b) as u8,
                _ => lean_internal_panic("String memcmp: range exceeds the string"),
            }
        }

        fn lean_string_to_utf8(s: b_obj) -> obj {
            let bytes = lean_string_bytes(s);
            let r = lean_alloc_sarray(1, bytes.len(), bytes.len());
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), lean_sarray_cptr(r), bytes.len());
            r
        }

        fn lean_string_from_utf8_unchecked(a: obj) -> obj {
            let bytes = std::slice::from_raw_parts(lean_sarray_cptr(a), lean_sarray_size(a));
            let r = lean_mk_string_from_bytes_unchecked(bytes);
            lean_dec(a);
            r
        }

        fn lean_string_validate_utf8(a: b_obj) -> u8 {
            let bytes = std::slice::from_raw_parts(lean_sarray_cptr(a), lean_sarray_size(a));
            validate_utf8(bytes).is_some() as u8
        }

        fn lean_slice_hash(s: b_obj) -> u64 {
            hash_str(slice_bytes(s), 11)
        }

        fn lean_slice_dec_lt(s1: b_obj, s2: b_obj) -> u8 {
            (slice_bytes(s1) < slice_bytes(s2)) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    const S: &str = "aé€𝄞z";

    unsafe fn with_s<R>(f: impl FnOnce(Obj) -> R) -> R {
        let s = lean_mk_string(S);
        let r = f(s);
        unsafe { lean_dec(s) };
        r
    }

    fn string(o: Obj) -> String {
        unsafe {
            let r = string_to_rust(o);
            lean_dec(o);
            r
        }
    }

    // Expected values from Lean 4.34.1: `String.Pos.Raw.get/next/prev/isValid/atEnd/get?` on
    // "aé€𝄞z" at byte positions 0..=11.
    const GET: [u32; 12] = [97, 233, 65, 8364, 65, 65, 119070, 65, 65, 65, 122, 65];
    const NEXT: [usize; 12] = [1, 3, 3, 6, 5, 6, 10, 8, 9, 10, 11, 12];
    const PREV: [usize; 12] = [0, 0, 1, 1, 3, 3, 3, 6, 6, 6, 6, 10];
    const VALID: [u8; 12] = [1, 1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 1];

    /// TEST0205: positions match lean
    #[test]
    fn test0205_positions_match_lean() {
        unsafe {
            with_s(|s| {
                assert_eq!(lean_unbox(lean_string_utf8_byte_size(s)), 11);
                assert_eq!(lean_unbox(lean_string_length(s)), 5);
                for i in 0..12 {
                    let p = lean_box(i);
                    assert_eq!(lean_string_utf8_get(s, p), GET[i], "get {i}");
                    assert_eq!(lean_unbox(lean_string_utf8_next(s, p)), NEXT[i], "next {i}");
                    assert_eq!(lean_unbox(lean_string_utf8_prev(s, p)), PREV[i], "prev {i}");
                    assert_eq!(lean_string_is_valid_pos(s, p), VALID[i], "valid {i}");
                    assert_eq!(lean_string_utf8_at_end(s, p), (i >= 11) as u8, "atEnd {i}");
                    let opt = lean_string_utf8_get_opt(s, p);
                    let expect_some = matches!(i, 0 | 1 | 3 | 6 | 10);
                    assert_eq!(!opt.is_scalar(), expect_some, "get? {i}");
                    if expect_some {
                        assert_eq!(lean_unbox_uint32(lean_ctor_get(opt, 0)), GET[i]);
                    }
                    lean_dec(opt);
                }
            })
        }
    }

    /// TEST0206: big positions match lean
    #[test]
    fn test0206_big_positions_match_lean() {
        unsafe {
            with_s(|s| {
                let big = alloc_mpz(BigInt::from(1u8) << 70);
                assert_eq!(lean_string_utf8_get(s, big), 65);
                let next = lean_string_utf8_next(s, big);
                assert_eq!(mpz_value(next).to_string(), "1180591620717411303425");
                let prev = lean_string_utf8_prev(s, big);
                assert_eq!(mpz_value(prev).to_string(), "1180591620717411303423");
                lean_dec(next);
                lean_dec(prev);
                lean_dec(big);
            })
        }
    }

    /// TEST0207: extract matches lean
    #[test]
    fn test0207_extract_matches_lean() {
        unsafe {
            with_s(|s| {
                let ex = |b, e| string(lean_string_utf8_extract(s, lean_box(b), lean_box(e)));
                assert_eq!(ex(1, 3), "é");
                assert_eq!(ex(1, 2), "é€𝄞z");
                assert_eq!(ex(2, 3), "");
                assert_eq!(ex(3, 1), "");
                assert_eq!(ex(1, 100), "é€𝄞z");
                assert_eq!(ex(3, 4), "€𝄞z");
            })
        }
    }

    /// TEST0208: set matches lean
    #[test]
    fn test0208_set_matches_lean() {
        unsafe {
            let set = |i, c: char| {
                let s = lean_mk_string(S);
                string(lean_string_utf8_set(s, lean_box(i), c as u32))
            };
            assert_eq!(set(0, '€'), "€é€𝄞z");
            assert_eq!(set(2, 'x'), "aé€𝄞z");
            assert_eq!(set(1, 'x'), "ax€𝄞z");
            assert_eq!(set(100, 'x'), "aé€𝄞z");
            // Shared strings are copied, leaving the original unchanged.
            let s = lean_mk_string("abc");
            lean_inc(s);
            let t = lean_string_utf8_set(s, lean_box(1), 'X' as u32);
            assert_eq!(lean_string_str(s), "abc");
            assert_eq!(string(t), "aXc");
            lean_dec(s);
        }
    }

    /// TEST0209: push and append copy on write
    #[test]
    fn test0209_push_and_append_copy_on_write() {
        unsafe {
            let s = lean_mk_string("ab");
            lean_inc(s);
            let t = lean_string_push(s, '𝄞' as u32);
            assert_eq!(lean_string_str(s), "ab");
            assert_eq!(lean_string_str(t), "ab𝄞");
            assert_eq!(lean_string_len(t), 3);
            let u = lean_string_append(t, s);
            assert_eq!(lean_string_len(u), 5);
            assert_eq!(string(u), "ab𝄞ab");
            // Growing an exclusive string repeatedly keeps content and counts intact.
            let mut v = lean_mk_string("");
            for _ in 0..100 {
                v = lean_string_push(v, 'é' as u32);
            }
            assert_eq!(lean_string_len(v), 100);
            assert_eq!(lean_string_size(v), 201);
            assert_eq!(string(v), "é".repeat(100));
            lean_dec(s);
        }
    }

    /// TEST0210: compare hash and lists match lean
    #[test]
    fn test0210_compare_hash_and_lists_match_lean() {
        unsafe {
            let cmp = |a: &str, b: &str| {
                let (x, y) = (lean_mk_string(a), lean_mk_string(b));
                let r = lean_string_compare(x, y);
                lean_dec(x);
                lean_dec(y);
                r
            };
            assert_eq!(cmp("abc", "abd"), 0);
            assert_eq!(cmp("ab", "abc"), 0);
            assert_eq!(cmp("b", "abc"), 2);
            assert_eq!(cmp("x", "x"), 1);
            let h = lean_mk_string("hello");
            assert_eq!(lean_string_hash(h), 9821865621596011261);
            lean_dec(h);
            let data = lean_string_data(lean_mk_string("aé"));
            assert_eq!(lean_unbox_uint32(lean_ctor_get(data, 0)), 97);
            assert_eq!(lean_unbox_uint32(lean_ctor_get(lean_ctor_get(data, 1), 0)), 233);
            let back = lean_string_mk(data);
            assert_eq!(string(back), "aé");
            assert_eq!(string(lean_string_of_usize(12345678901234)), "12345678901234");
        }
    }

    /// TEST0211: utf8 validation matches lean
    #[test]
    fn test0211_utf8_validation_matches_lean() {
        assert_eq!(validate_utf8(&[0xe2, 0x82, 0xac]), Some(1));
        assert_eq!(validate_utf8(&[0xed, 0xa0, 0x80]), None);
    }
}
