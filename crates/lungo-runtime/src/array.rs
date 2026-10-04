//! `Array`, `ByteArray` and `FloatArray` primitives, ported from `lean.h` and
//! `runtime/object.cpp`.
//!
//! Indices are `Nat`s (or `USize` for the `u*` variants). A non-scalar `Nat` index exceeds
//! `LEAN_MAX_SMALL_NAT` and is therefore out of bounds for every array that fits in memory.
//! Updates are in place when the array is exclusive and copy the array otherwise.

use crate::hash::hash_str;
use crate::object::*;
use crate::panic::lean_panic_fn;
use num_traits::ToPrimitive;

const OUT_OF_BOUNDS: &str = "Error: index out of bounds";

/// `lean_nat_to_size_t`: the value of a `Nat` used as a size or offset, consuming a big `Nat`.
/// A size that does not fit in memory cannot be allocated.
unsafe fn nat_to_size(n: Obj) -> usize {
    unsafe {
        if n.is_scalar() {
            lean_unbox(n)
        } else {
            let v = mpz_value(n).to_usize().unwrap_or_else(|| lean_internal_panic_out_of_memory());
            lean_dec(n);
            v
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Arrays of objects
// ---------------------------------------------------------------------------------------------

/// `lean_copy_expand_array`: copies `a` (consumed), doubling its capacity when `expand`.
pub unsafe fn lean_copy_expand_array(a: Obj, expand: bool) -> Obj {
    unsafe {
        let sz = lean_array_size(a);
        let mut cap = lean_array_capacity(a);
        if expand {
            cap = cap.checked_add(1).and_then(|c| c.checked_mul(2)).unwrap_or_else(|| lean_internal_panic_overflow());
        }
        let r = lean_alloc_array(sz, cap);
        let src = lean_array_cptr(a);
        let dst = lean_array_cptr(r);
        if lean_is_exclusive(a) {
            // Transfer ownership of the elements directly.
            std::ptr::copy_nonoverlapping(src, dst, sz);
            lean_free_object(a);
        } else {
            for i in 0..sz {
                let v = *src.add(i);
                lean_inc(v);
                *dst.add(i) = v;
            }
            lean_dec(a);
        }
        r
    }
}

#[inline]
unsafe fn ensure_exclusive_array(a: Obj) -> Obj {
    unsafe { if lean_is_exclusive(a) { a } else { lean_copy_expand_array(a, false) } }
}

#[inline]
unsafe fn array_uget(a: Obj, i: usize) -> Obj {
    unsafe {
        let r = lean_array_get_core(a, i);
        lean_inc(r);
        r
    }
}

#[inline]
unsafe fn array_uset(a: Obj, i: usize, v: Obj) -> Obj {
    unsafe {
        let r = ensure_exclusive_array(a);
        let slot = lean_array_cptr(r).add(i);
        lean_dec(*slot);
        *slot = v;
        r
    }
}

#[inline]
unsafe fn array_uswap(a: Obj, i: usize, j: usize) -> Obj {
    unsafe {
        let r = ensure_exclusive_array(a);
        let data = lean_array_cptr(r);
        std::ptr::swap(data.add(i), data.add(j));
        r
    }
}

/// `lean_array_get_panic`: reports an out-of-bounds access and returns `def_val` (owned).
pub unsafe fn lean_array_get_panic(def_val: Obj) -> Obj {
    unsafe { lean_panic_fn(def_val, lean_mk_string(OUT_OF_BOUNDS)) }
}

/// `lean_array_set_panic`: reports an out-of-bounds update and returns `a` unchanged.
pub unsafe fn lean_array_set_panic(a: Obj, v: Obj) -> Obj {
    unsafe {
        lean_dec(v);
        lean_panic_fn(a, lean_mk_string(OUT_OF_BOUNDS))
    }
}

/// Builds an `Array` owning `elems`.
pub unsafe fn array_from_vec(elems: Vec<Obj>) -> Obj {
    unsafe {
        let r = lean_alloc_array(elems.len(), elems.len());
        std::ptr::copy_nonoverlapping(elems.as_ptr(), lean_array_cptr(r), elems.len());
        r
    }
}

/// The elements of the `Array` `a` (borrowed); the returned values are borrowed from `a`.
pub unsafe fn array_elements<'a>(a: Obj) -> &'a [Obj] {
    unsafe { std::slice::from_raw_parts(lean_array_cptr(a), lean_array_size(a)) }
}

// ---------------------------------------------------------------------------------------------
// Scalar arrays
// ---------------------------------------------------------------------------------------------

/// `lean_copy_sarray`: copies `a` (consumed) into a scalar array of capacity `cap`.
unsafe fn copy_sarray(a: Obj, cap: usize) -> Obj {
    unsafe {
        let esz = lean_sarray_elem_size(a);
        let sz = lean_sarray_size(a);
        debug_assert!(cap >= sz);
        let r = lean_alloc_sarray(esz, sz, cap);
        std::ptr::copy_nonoverlapping(lean_sarray_cptr(a), lean_sarray_cptr(r), esz as usize * sz);
        lean_dec(a);
        r
    }
}

unsafe fn sarray_ensure_exclusive(a: Obj) -> Obj {
    unsafe { if lean_is_exclusive(a) { a } else { copy_sarray(a, lean_sarray_capacity(a)) } }
}

/// `lean_sarray_ensure_capacity`: copies `a` unless it can hold `min_cap` elements; the copy has
/// exactly `min_cap` capacity when `exact`, twice that otherwise.
unsafe fn sarray_ensure_capacity(a: Obj, min_cap: usize, exact: bool) -> Obj {
    unsafe {
        if min_cap <= lean_sarray_capacity(a) {
            a
        } else {
            let cap =
                if exact { min_cap } else { min_cap.checked_mul(2).unwrap_or_else(|| lean_internal_panic_overflow()) };
            copy_sarray(a, cap)
        }
    }
}

unsafe fn sarray_bytes<'a>(a: Obj) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(lean_sarray_cptr(a), lean_sarray_elem_size(a) as usize * lean_sarray_size(a)) }
}

/// Builds a `ByteArray` from `bytes`.
pub fn byte_array_from_slice(bytes: &[u8]) -> Obj {
    unsafe {
        let r = lean_alloc_sarray(1, bytes.len(), bytes.len());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), lean_sarray_cptr(r), bytes.len());
        r
    }
}

/// The contents of the `ByteArray` `a` (borrowed).
pub unsafe fn byte_array_bytes<'a>(a: Obj) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts(lean_sarray_cptr(a), lean_sarray_size(a)) }
}

/// Builds a `FloatArray` from `values`.
pub fn float_array_from_slice(values: &[f64]) -> Obj {
    unsafe {
        let r = lean_alloc_sarray(8, values.len(), values.len());
        std::ptr::copy_nonoverlapping(values.as_ptr() as *const u8, lean_sarray_cptr(r), values.len() * 8);
        r
    }
}

/// The contents of the `FloatArray` `a` (borrowed).
pub unsafe fn float_array_values(a: Obj) -> Vec<f64> {
    unsafe {
        let p = lean_sarray_cptr(a) as *const f64;
        (0..lean_sarray_size(a)).map(|i| p.add(i).read_unaligned()).collect()
    }
}

#[inline]
unsafe fn float_ptr(a: Obj) -> *mut f64 {
    unsafe { lean_sarray_cptr(a) as *mut f64 }
}

unsafe fn byte_array_uset(a: Obj, i: usize, v: u8) -> Obj {
    unsafe {
        let r = sarray_ensure_exclusive(a);
        *lean_sarray_cptr(r).add(i) = v;
        r
    }
}

unsafe fn float_array_uset(a: Obj, i: usize, v: f64) -> Obj {
    unsafe {
        let r = sarray_ensure_exclusive(a);
        float_ptr(r).add(i).write_unaligned(v);
        r
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_array_get_size(a: b_obj) -> obj {
            lean_box(lean_array_size(a))
        }

        fn lean_array_size(a: b_obj) -> usize {
            crate::object::lean_array_size(a)
        }

        fn lean_array_uget(a: b_obj, i: usize) -> obj {
            array_uget(a, i)
        }

        fn lean_array_uget_borrowed(a: b_obj, i: usize) -> obj {
            lean_array_get_core(a, i)
        }

        fn lean_array_fget(a: b_obj, i: b_obj) -> obj {
            array_uget(a, lean_unbox(i))
        }

        fn lean_array_fget_borrowed(a: b_obj, i: b_obj) -> obj {
            lean_array_get_core(a, lean_unbox(i))
        }

        fn lean_array_get(def_val: b_obj, a: b_obj, i: b_obj) -> obj {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_array_size(a) {
                return array_uget(a, lean_unbox(i));
            }
            lean_inc(def_val);
            lean_array_get_panic(def_val)
        }

        fn lean_array_get_borrowed(def_val: b_obj, a: b_obj, i: b_obj) -> obj {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_array_size(a) {
                return lean_array_get_core(a, lean_unbox(i));
            }
            lean_inc(def_val);
            lean_array_get_panic(def_val)
        }

        fn lean_array_uset(a: obj, i: usize, v: obj) -> obj {
            array_uset(a, i, v)
        }

        fn lean_array_fset(a: obj, i: b_obj, v: obj) -> obj {
            array_uset(a, lean_unbox(i), v)
        }

        fn lean_array_set(a: obj, i: b_obj, v: obj) -> obj {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_array_size(a) {
                return array_uset(a, lean_unbox(i), v);
            }
            lean_array_set_panic(a, v)
        }

        fn lean_array_pop(a: obj) -> obj {
            let r = ensure_exclusive_array(a);
            let sz = crate::object::lean_array_size(r);
            if sz == 0 {
                return r;
            }
            lean_array_set_size(r, sz - 1);
            lean_dec(*lean_array_cptr(r).add(sz - 1));
            r
        }

        fn lean_array_fswap(a: obj, i: b_obj, j: b_obj) -> obj {
            array_uswap(a, lean_unbox(i), lean_unbox(j))
        }

        fn lean_array_swap(a: obj, i: b_obj, j: b_obj) -> obj {
            if !i.is_scalar() || !j.is_scalar() {
                return a;
            }
            let (ui, uj) = (lean_unbox(i), lean_unbox(j));
            let sz = crate::object::lean_array_size(a);
            if ui >= sz || uj >= sz {
                return a;
            }
            array_uswap(a, ui, uj)
        }

        fn lean_array_push(a: obj, v: obj) -> obj {
            let r = if lean_is_exclusive(a) {
                if lean_array_capacity(a) > crate::object::lean_array_size(a) {
                    a
                } else {
                    lean_copy_expand_array(a, true)
                }
            } else {
                let expand = lean_array_capacity(a) < 2 * crate::object::lean_array_size(a) + 1;
                lean_copy_expand_array(a, expand)
            };
            let sz = crate::object::lean_array_size(r);
            *lean_array_cptr(r).add(sz) = v;
            lean_array_set_size(r, sz + 1);
            r
        }

        fn lean_mk_array(n: obj, v: obj) -> obj {
            let sz = nat_to_size(n);
            let r = lean_alloc_array(sz, sz);
            let data = lean_array_cptr(r);
            for k in 0..sz {
                *data.add(k) = v;
            }
            if sz == 0 {
                lean_dec(v);
            } else if sz > 1 {
                lean_inc_n(v, sz - 1);
            }
            r
        }

        fn lean_mk_empty_array_with_capacity(capacity: b_obj) -> obj {
            if !capacity.is_scalar() {
                lean_internal_panic_out_of_memory();
            }
            lean_alloc_array(0, lean_unbox(capacity))
        }

        // `List.toArray`: consumes the list; the array has exactly the list's length as capacity.
        fn lean_array_mk(lst: obj) -> obj {
            let mut elems = Vec::new();
            let mut o = lst;
            while !o.is_scalar() {
                let head = lean_ctor_get(o, 0);
                lean_inc(head);
                elems.push(head);
                o = lean_ctor_get(o, 1);
            }
            lean_dec(lst);
            array_from_vec(elems)
        }

        // `Array.toList`: consumes the array.
        fn lean_array_to_list(a: obj) -> obj {
            let mut r = lean_box(0);
            for v in array_elements(a).iter().rev() {
                lean_inc(*v);
                let cell = lean_alloc_ctor(1, 2, 0);
                lean_ctor_set(cell, 0, *v);
                lean_ctor_set(cell, 1, r);
                r = cell;
            }
            lean_dec(a);
            r
        }

        // ------------------------------------------------------------------------------------
        // Scalar arrays

        fn lean_sarray_size(a: b_obj) -> usize {
            crate::object::lean_sarray_size(a)
        }

        fn lean_sarray_dec_eq(a1: b_obj, a2: b_obj) -> u8 {
            (a1 == a2
                || (crate::object::lean_sarray_size(a1) == crate::object::lean_sarray_size(a2)
                    && sarray_bytes(a1) == sarray_bytes(a2))) as u8
        }

        fn lean_mk_empty_byte_array(capacity: b_obj) -> obj {
            if !capacity.is_scalar() {
                lean_internal_panic_out_of_memory();
            }
            lean_alloc_sarray(1, 0, lean_unbox(capacity))
        }

        fn lean_byte_array_size(a: b_obj) -> obj {
            lean_box(crate::object::lean_sarray_size(a))
        }

        fn lean_byte_array_mk(a: obj) -> obj {
            let elems = array_elements(a);
            let r = lean_alloc_sarray(1, elems.len(), elems.len());
            let dst = lean_sarray_cptr(r);
            for (k, v) in elems.iter().enumerate() {
                *dst.add(k) = lean_unbox(*v) as u8;
            }
            lean_dec(a);
            r
        }

        fn lean_byte_array_data(a: obj) -> obj {
            let bytes = byte_array_bytes(a);
            let r = lean_alloc_array(bytes.len(), bytes.len());
            let dst = lean_array_cptr(r);
            for (k, b) in bytes.iter().enumerate() {
                *dst.add(k) = lean_box(*b as usize);
            }
            lean_dec(a);
            r
        }

        fn lean_byte_array_uget(a: b_obj, i: usize) -> u8 {
            *lean_sarray_cptr(a).add(i)
        }

        fn lean_byte_array_fget(a: b_obj, i: b_obj) -> u8 {
            *lean_sarray_cptr(a).add(lean_unbox(i))
        }

        fn lean_byte_array_get(a: b_obj, i: b_obj) -> u8 {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_sarray_size(a) {
                *lean_sarray_cptr(a).add(lean_unbox(i))
            } else {
                0
            }
        }

        fn lean_byte_array_uset(a: obj, i: usize, v: u8) -> obj {
            byte_array_uset(a, i, v)
        }

        fn lean_byte_array_fset(a: obj, i: b_obj, v: u8) -> obj {
            byte_array_uset(a, lean_unbox(i), v)
        }

        fn lean_byte_array_set(a: obj, i: b_obj, v: u8) -> obj {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_sarray_size(a) {
                byte_array_uset(a, lean_unbox(i), v)
            } else {
                a
            }
        }

        fn lean_byte_array_push(a: obj, b: u8) -> obj {
            let sz = crate::object::lean_sarray_size(a);
            let r = sarray_ensure_exclusive(sarray_ensure_capacity(a, sz + 1, false));
            *lean_sarray_cptr(r).add(sz) = b;
            lean_sarray_set_size(r, sz + 1);
            r
        }

        fn lean_byte_array_copy_slice(src: b_obj, src_off: obj, dest: obj, dest_off: obj, len: obj, exact: u8) -> obj {
            let ssz = crate::object::lean_sarray_size(src);
            let dsz = crate::object::lean_sarray_size(dest);
            let src_off = nat_to_size(src_off);
            if src_off > ssz {
                lean_dec(len);
                lean_dec(dest_off);
                return dest;
            }
            let len = nat_to_size(len).min(ssz - src_off);
            let dest_off = nat_to_size(dest_off).min(dsz);
            let new_dsz = dsz.max(dest_off + len);
            let r = sarray_ensure_exclusive(sarray_ensure_capacity(dest, new_dsz, exact != 0));
            lean_sarray_set_size(r, new_dsz);
            // `r` is exclusive, so it is distinct from `src` unless `src == dest` was exclusive,
            // in which case the ranges may overlap.
            std::ptr::copy(lean_sarray_cptr(src).add(src_off), lean_sarray_cptr(r).add(dest_off), len);
            r
        }

        fn lean_byte_array_hash(a: b_obj) -> u64 {
            hash_str(byte_array_bytes(a), 11)
        }

        fn lean_mk_empty_float_array(capacity: b_obj) -> obj {
            if !capacity.is_scalar() {
                lean_internal_panic_out_of_memory();
            }
            lean_alloc_sarray(8, 0, lean_unbox(capacity))
        }

        fn lean_float_array_size(a: b_obj) -> obj {
            lean_box(crate::object::lean_sarray_size(a))
        }

        fn lean_float_array_mk(a: obj) -> obj {
            let elems = array_elements(a);
            let r = lean_alloc_sarray(8, elems.len(), elems.len());
            let dst = float_ptr(r);
            for (k, v) in elems.iter().enumerate() {
                dst.add(k).write_unaligned(lean_unbox_float(*v));
            }
            lean_dec(a);
            r
        }

        fn lean_float_array_data(a: obj) -> obj {
            let sz = crate::object::lean_sarray_size(a);
            let r = lean_alloc_array(sz, sz);
            let dst = lean_array_cptr(r);
            let src = float_ptr(a);
            for k in 0..sz {
                *dst.add(k) = lean_box_float(src.add(k).read_unaligned());
            }
            lean_dec(a);
            r
        }

        fn lean_float_array_uget(a: b_obj, i: usize) -> f64 {
            float_ptr(a).add(i).read_unaligned()
        }

        fn lean_float_array_fget(a: b_obj, i: b_obj) -> f64 {
            float_ptr(a).add(lean_unbox(i)).read_unaligned()
        }

        fn lean_float_array_get(a: b_obj, i: b_obj) -> f64 {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_sarray_size(a) {
                float_ptr(a).add(lean_unbox(i)).read_unaligned()
            } else {
                0.0
            }
        }

        fn lean_float_array_uset(a: obj, i: usize, v: f64) -> obj {
            float_array_uset(a, i, v)
        }

        fn lean_float_array_fset(a: obj, i: b_obj, v: f64) -> obj {
            float_array_uset(a, lean_unbox(i), v)
        }

        fn lean_float_array_set(a: obj, i: b_obj, v: f64) -> obj {
            if i.is_scalar() && lean_unbox(i) < crate::object::lean_sarray_size(a) {
                float_array_uset(a, lean_unbox(i), v)
            } else {
                a
            }
        }

        fn lean_float_array_push(a: obj, v: f64) -> obj {
            let sz = crate::object::lean_sarray_size(a);
            let r = sarray_ensure_exclusive(sarray_ensure_capacity(a, sz + 1, false));
            float_ptr(r).add(sz).write_unaligned(v);
            lean_sarray_set_size(r, sz + 1);
            r
        }

        fn lean_byteslice_beq(a: b_obj, b: b_obj) -> u8 {
            if a == b {
                return 1;
            }
            let bounds = |s: Obj| -> (Obj, usize, usize) {
                (lean_ctor_get(s, 0), lean_unbox(lean_ctor_get(s, 1)), lean_unbox(lean_ctor_get(s, 2)))
            };
            let (ba, sa, ea) = bounds(a);
            let (bb, sb, eb) = bounds(b);
            let (la, lb) = (ea.wrapping_sub(sa), eb.wrapping_sub(sb));
            if la != lb {
                return 0;
            }
            if la == 0 {
                return 1;
            }
            let pa = std::slice::from_raw_parts(lean_sarray_cptr(ba).add(sa), la);
            let pb = std::slice::from_raw_parts(lean_sarray_cptr(bb).add(sb), lb);
            (pa == pb) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    fn nat_list(o: Obj) -> Vec<usize> {
        unsafe { array_elements(o).iter().map(|v| lean_unbox(*v)).collect() }
    }

    /// TEST0158: push grows and copies on write
    #[test]
    fn test0158_push_grows_and_copies_on_write() {
        unsafe {
            let mut a = lean_alloc_array(0, 0);
            for k in 0..10 {
                a = lean_array_push(a, lean_box(k));
            }
            assert_eq!(nat_list(a), (0..10).collect::<Vec<_>>());
            // Capacity doubles as (cap + 1) * 2 from 0: 2, 6, 14.
            assert_eq!(lean_array_capacity(a), 14);
            lean_inc(a);
            let b = lean_array_set(a, lean_box(3), lean_box(99));
            assert_ne!(a, b);
            assert_eq!(nat_list(a)[3], 3);
            assert_eq!(nat_list(b)[3], 99);
            lean_dec(a);
            // Exclusive update is in place.
            let c = lean_array_set(b, lean_box(4), lean_box(77));
            assert_eq!(b, c);
            lean_dec(c);
        }
    }

    /// TEST0159: out of bounds get returns default
    #[test]
    fn test0159_out_of_bounds_get_returns_default() {
        crate::exports::recording::install();
        unsafe {
            let a = array_from_vec(vec![lean_box(10), lean_box(20)]);
            // Lean 4.34.1: `#[10, 20][5]!` reports "Error: index out of bounds" and yields 0.
            assert_eq!(lean_array_get(lean_box(0), a, lean_box(5)), lean_box(0));
            assert_eq!(lean_array_get(lean_box(0), a, lean_box(1)), lean_box(20));
            let a = lean_array_set(a, lean_box(9), lean_box(1));
            assert_eq!(nat_list(a), vec![10, 20]);
            lean_dec(a);
        }
    }

    /// TEST0160: replicate list round trip and swap
    #[test]
    fn test0160_replicate_list_round_trip_and_swap() {
        unsafe {
            let s = lean_mk_string("x");
            let r = lean_mk_array(lean_box(3), s);
            assert_eq!(get_rc(s), 3);
            let l = lean_array_to_list(r);
            let back = lean_array_mk(l);
            assert_eq!(crate::object::lean_array_size(back), 3);
            assert_eq!(get_rc(s), 3);
            lean_dec(back);
            let a = array_from_vec(vec![lean_box(1), lean_box(2), lean_box(3)]);
            let a = lean_array_swap(a, lean_box(0), lean_box(2));
            assert_eq!(nat_list(a), vec![3, 2, 1]);
            let a = lean_array_swap(a, lean_box(0), lean_box(7));
            assert_eq!(nat_list(a), vec![3, 2, 1]);
            let a = lean_array_pop(a);
            assert_eq!(nat_list(a), vec![3, 2]);
            lean_dec(a);
        }
    }

    /// TEST0161: byte arrays match lean
    #[test]
    fn test0161_byte_arrays_match_lean() {
        unsafe {
            let b = byte_array_from_slice(&[1, 2, 3]);
            // Lean 4.34.1: `ByteArray.hash ⟨#[1,2,3]⟩` and the empty hash.
            assert_eq!(lean_byte_array_hash(b), 11344833645450860537);
            let e = byte_array_from_slice(&[]);
            assert_eq!(lean_byte_array_hash(e), 9877294847684254529);
            lean_dec(e);
            let b = lean_byte_array_push(lean_byte_array_push(b, 4), 5);
            assert_eq!(byte_array_bytes(b), &[1, 2, 3, 4, 5]);
            lean_dec(b);
            let copy = |src: &[u8], so, dst: &[u8], doff, len| {
                let s = byte_array_from_slice(src);
                let r = lean_byte_array_copy_slice(
                    s,
                    lean_box(so),
                    byte_array_from_slice(dst),
                    lean_box(doff),
                    lean_box(len),
                    0,
                );
                let v = byte_array_bytes(r).to_vec();
                lean_dec(r);
                lean_dec(s);
                v
            };
            assert_eq!(copy(&[1, 2, 3, 4, 5], 1, &[9, 9], 1, 3), vec![9, 2, 3, 4]);
            assert_eq!(copy(&[1, 2, 3], 5, &[9, 9], 0, 3), vec![9, 9]);
            assert_eq!(copy(&[1, 2, 3], 0, &[9, 9], 7, 2), vec![9, 9, 1, 2]);
        }
    }

    /// TEST0162: float arrays round trip
    #[test]
    fn test0162_float_arrays_round_trip() {
        unsafe {
            let f = float_array_from_slice(&[1.5, -2.0]);
            let f = lean_float_array_push(f, 3.25);
            assert_eq!(float_array_values(f), vec![1.5, -2.0, 3.25]);
            assert_eq!(lean_float_array_get(f, lean_box(9)), 0.0);
            let d = lean_float_array_data(f);
            let back = lean_float_array_mk(d);
            assert_eq!(float_array_values(back), vec![1.5, -2.0, 3.25]);
            lean_dec(back);
        }
    }
}
