//! Maximal sharing (`ShareCommon`), ported from `runtime/sharecommon.cpp`.
//!
//! Objects are compared and hashed by their header's tag and field count and the bytes of their
//! body, with big numbers compared by value. The byte extent of each kind of object follows
//! Lean's `lean_object_data_byte_size` in its (mimalloc) release configuration: constructor
//! objects occupy a whole number of words, and padding bytes are zero.

use crate::apply::{lean_apply_2, lean_apply_3};
use crate::hash::{hash_str, mix_hash};
use crate::object::*;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

const HEADER: usize = 8;

fn align8(n: usize) -> usize {
    n.div_ceil(8) * 8
}

/// `lean_object_data_byte_size`.
unsafe fn data_byte_size(o: Obj) -> usize {
    unsafe {
        match lean_ptr_tag(o) {
            LEAN_ARRAY => 24 + size_of::<Obj>() * lean_array_size(o),
            LEAN_SCALAR_ARRAY => 24 + lean_sarray_elem_size(o) as usize * lean_sarray_size(o),
            LEAN_STRING => 32 + lean_string_size(o),
            LEAN_CLOSURE => 24 + size_of::<Obj>() * lean_closure_num_fixed(o) as usize,
            _ => align8(lean_object_byte_size(o)),
        }
    }
}

/// The body of `o` (everything after the header) within its data extent, zero-padded.
unsafe fn body(o: Obj) -> Vec<u8> {
    unsafe {
        let extent = data_byte_size(o) - HEADER;
        let allocated = lean_object_byte_size(o) - HEADER;
        let mut out = vec![0u8; extent];
        let n = extent.min(allocated);
        std::ptr::copy_nonoverlapping((o.ptr() as *const u8).add(HEADER), out.as_mut_ptr(), n);
        out
    }
}

/// `mpz::hash` under GMP: `(unsigned) mpz_get_si(v)`.
fn mpz_hash(v: &num_bigint::BigInt) -> u32 {
    let (sign, mag) = v.to_u64_digits();
    let low = mag.first().copied().unwrap_or(0);
    let si: i64 = match sign {
        num_bigint::Sign::Minus => -1 - (low.wrapping_sub(1) & i64::MAX as u64) as i64,
        _ => (low & i64::MAX as u64) as i64,
    };
    si as u32
}

pub(crate) unsafe fn sharecommon_eq(o1: Obj, o2: Obj) -> bool {
    unsafe {
        if o1.is_scalar() || o2.is_scalar() {
            lean_internal_panic("ShareCommon.Object.eq applied to a scalar");
        }
        if data_byte_size(o1) != data_byte_size(o2) {
            return false;
        }
        let tag = lean_ptr_tag(o1);
        if tag != lean_ptr_tag(o2) || lean_ptr_other(o1) != lean_ptr_other(o2) {
            return false;
        }
        if tag == LEAN_MPZ { mpz_value(o1) == mpz_value(o2) } else { body(o1) == body(o2) }
    }
}

pub(crate) unsafe fn sharecommon_hash(o: Obj) -> u64 {
    unsafe {
        if o.is_scalar() {
            lean_internal_panic("ShareCommon.Object.hash applied to a scalar");
        }
        let tag = lean_ptr_tag(o);
        if tag == LEAN_MPZ {
            mix_hash(tag as u64, mpz_hash(mpz_value(o)) as u64)
        } else {
            // Lean truncates the header hash to `unsigned`.
            let init = mix_hash(tag as u64, lean_ptr_other(o) as u64) as u32;
            hash_str(&body(o), init as u64)
        }
    }
}

/// An object keyed by structural equality.
#[derive(Clone, Copy)]
struct Structural(Obj);

impl PartialEq for Structural {
    fn eq(&self, other: &Self) -> bool {
        unsafe { sharecommon_eq(self.0, other.0) }
    }
}

impl Eq for Structural {}

impl Hash for Structural {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(unsafe { sharecommon_hash(self.0) });
    }
}

/// Copies the scalar area and allocates a constructor like `a` (tag, fields, scalar bytes).
unsafe fn alloc_ctor_like(a: Obj) -> (Obj, u32) {
    unsafe {
        let num_objs = lean_ctor_num_objs(a);
        let scalar_offset = HEADER + num_objs as usize * size_of::<Obj>();
        let scalar_sz = lean_object_byte_size(a) - scalar_offset;
        let new_a = lean_alloc_ctor(lean_ptr_tag(a) as u32, num_objs, scalar_sz);
        if scalar_sz > 0 {
            std::ptr::copy_nonoverlapping(
                (a.ptr() as *const u8).add(scalar_offset),
                (new_a.ptr() as *mut u8).add(scalar_offset),
                scalar_sz,
            );
        }
        (new_a, num_objs)
    }
}

/// Takes a reference to a single-threaded object created by `sharecommon_quick`, panicking on
/// overflow rather than wrapping into the sticky range.
unsafe fn inc_st(o: Obj) {
    unsafe {
        let rc = get_rc(o);
        if rc == i32::MAX {
            lean_internal_panic_rc_overflow();
        }
        lean_inc_ref(o);
    }
}

/// `sharecommon_quick_fn`: maximal sharing with local state.
struct Quick {
    /// Shared input objects already visited, by address.
    cache: HashMap<usize, Obj>,
    /// Maximally shared objects (hash-consing table).
    set: HashSet<Structural>,
}

impl Quick {
    unsafe fn check_cache(&mut self, a: Obj) -> Option<Obj> {
        unsafe {
            if !lean_is_exclusive(a)
                && let Some(r) = self.cache.get(&a.addr())
            {
                inc_st(*r);
                return Some(*r);
            }
            None
        }
    }

    unsafe fn save(&mut self, a: Obj, new_a: Obj) -> Obj {
        unsafe {
            let result = match self.set.get(&Structural(new_a)) {
                None => {
                    self.set.insert(Structural(new_a));
                    new_a
                }
                Some(existing) => {
                    let existing = existing.0;
                    lean_dec_ref(new_a);
                    inc_st(existing);
                    existing
                }
            };
            if !lean_is_exclusive(a) {
                self.cache.insert(a.addr(), result);
            }
            result
        }
    }

    unsafe fn visit_terminal(&mut self, a: Obj) -> Obj {
        unsafe {
            let r = match self.set.get(&Structural(a)) {
                None => {
                    self.set.insert(Structural(a));
                    a
                }
                Some(existing) => existing.0,
            };
            lean_inc_ref(r);
            r
        }
    }

    unsafe fn visit_array(&mut self, a: Obj) -> Obj {
        unsafe {
            if let Some(r) = self.check_cache(a) {
                return r;
            }
            let sz = lean_array_size(a);
            let new_a = lean_alloc_array(sz, sz);
            for i in 0..sz {
                let v = self.visit(lean_array_get_core(a, i));
                lean_array_set_core(new_a, i, v);
            }
            self.save(a, new_a)
        }
    }

    unsafe fn visit_ctor(&mut self, a: Obj) -> Obj {
        unsafe {
            if let Some(r) = self.check_cache(a) {
                return r;
            }
            let (new_a, num_objs) = alloc_ctor_like(a);
            for i in 0..num_objs {
                let v = self.visit(lean_ctor_get(a, i));
                lean_ctor_set(new_a, i, v);
            }
            self.save(a, new_a)
        }
    }

    unsafe fn visit(&mut self, a: Obj) -> Obj {
        unsafe {
            if a.is_scalar() {
                return a;
            }
            match lean_ptr_tag(a) {
                LEAN_CLOSURE | LEAN_THUNK | LEAN_TASK | LEAN_PROMISE | LEAN_REF | LEAN_EXTERNAL | LEAN_RESERVED => {
                    lean_inc_ref(a);
                    a
                }
                LEAN_MPZ | LEAN_SCALAR_ARRAY | LEAN_STRING => self.visit_terminal(a),
                LEAN_ARRAY => self.visit_array(a),
                _ => self.visit_ctor(a),
            }
        }
    }
}

/// `sharecommon_state`: a Lean-level `ShareCommon.State` accessed through the functions of a
/// `StateFactory`.
struct LeanState {
    map_find: Obj,
    map_insert: Obj,
    set_find: Obj,
    set_insert: Obj,
    map: Obj,
    set: Obj,
}

impl LeanState {
    unsafe fn new(tc: Obj, s: Obj) -> Self {
        unsafe {
            let st = LeanState {
                map_find: lean_ctor_get(tc, 1),
                map_insert: lean_ctor_get(tc, 2),
                set_find: lean_ctor_get(tc, 3),
                set_insert: lean_ctor_get(tc, 4),
                map: lean_ctor_get(s, 0),
                set: lean_ctor_get(s, 1),
            };
            lean_inc(st.map);
            lean_inc(st.set);
            lean_dec(s);
            st
        }
    }

    unsafe fn pack(&mut self, a: Obj) -> Obj {
        unsafe {
            let st = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(st, 0, self.map);
            lean_ctor_set(st, 1, self.set);
            self.map = lean_box(0);
            self.set = lean_box(0);
            let r = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(r, 0, a);
            lean_ctor_set(r, 1, st);
            r
        }
    }

    unsafe fn map_find(&mut self, k: Obj) -> Obj {
        unsafe {
            lean_inc(self.map_find);
            lean_inc(self.map);
            lean_inc(k);
            lean_apply_2(self.map_find, self.map, k)
        }
    }

    unsafe fn map_insert(&mut self, k: Obj, v: Obj) {
        unsafe {
            lean_inc(self.map_insert);
            self.map = lean_apply_3(self.map_insert, self.map, k, v);
        }
    }

    unsafe fn set_find(&mut self, o: Obj) -> Obj {
        unsafe {
            lean_inc(self.set_find);
            lean_inc(self.set);
            lean_inc(o);
            lean_apply_2(self.set_find, self.set, o)
        }
    }

    unsafe fn set_insert(&mut self, o: Obj) {
        unsafe {
            lean_inc(self.set_insert);
            self.set = lean_apply_2(self.set_insert, self.set, o);
        }
    }
}

impl Drop for LeanState {
    fn drop(&mut self) {
        unsafe {
            lean_dec(self.map);
            lean_dec(self.set);
        }
    }
}

/// `sharecommon_fn`: maximal sharing with a persistent Lean-level state.
struct Persistent {
    state: LeanState,
    children: Vec<Obj>,
    todo: Vec<Obj>,
}

impl Persistent {
    unsafe fn push_child(&mut self, a: Obj) -> bool {
        unsafe {
            if a.is_scalar() {
                self.children.push(a);
                return true;
            }
            match lean_ptr_tag(a) {
                LEAN_RESERVED => lean_internal_panic_unreachable(),
                LEAN_THUNK | LEAN_TASK | LEAN_REF | LEAN_EXTERNAL | LEAN_CLOSURE | LEAN_PROMISE => {
                    self.children.push(a);
                    return true;
                }
                _ => {}
            }
            let o = self.state.map_find(a);
            if o != lean_box(0) {
                let r = lean_ctor_get(o, 0);
                lean_dec(o);
                // The map still holds a reference to `r`.
                self.children.push(r);
                return true;
            }
            self.todo.push(a);
            false
        }
    }

    unsafe fn save(&mut self, a: Obj, new_a: Obj) {
        unsafe {
            debug_assert_eq!(self.todo.last().copied(), Some(a));
            self.todo.pop();
            let found = self.state.set_find(new_a);
            if found != lean_box(0) {
                lean_dec(new_a);
                let existing = lean_ctor_get(found, 0);
                lean_inc(existing);
                lean_dec(found);
                lean_inc(a);
                self.state.map_insert(a, existing);
            } else {
                lean_inc(a);
                lean_inc_n(new_a, 3);
                self.state.set_insert(new_a);
                self.state.map_insert(a, new_a);
                self.state.map_insert(new_a, new_a);
            }
        }
    }

    unsafe fn visit_array(&mut self, a: Obj) {
        unsafe {
            self.children.clear();
            let sz = lean_array_size(a);
            let mut missing = false;
            for i in 0..sz {
                if !self.push_child(lean_array_get_core(a, i)) {
                    missing = true;
                }
            }
            if missing {
                return;
            }
            let new_a = lean_alloc_array(sz, sz);
            for i in 0..sz {
                let c = self.children[i];
                lean_inc(c);
                lean_array_set_core(new_a, i, c);
            }
            self.save(a, new_a);
        }
    }

    unsafe fn visit_sarray(&mut self, a: Obj) {
        unsafe {
            let sz = lean_sarray_size(a);
            let esz = lean_sarray_elem_size(a);
            let new_a = lean_alloc_sarray(esz, sz, sz);
            std::ptr::copy_nonoverlapping(lean_sarray_cptr(a), lean_sarray_cptr(new_a), esz as usize * sz);
            self.save(a, new_a);
        }
    }

    unsafe fn visit_string(&mut self, a: Obj) {
        unsafe {
            let sz = lean_string_size(a);
            let new_a = lean_alloc_string(sz, sz, lean_string_len(a));
            std::ptr::copy_nonoverlapping(lean_string_cstr(a), lean_string_cstr(new_a), sz);
            self.save(a, new_a);
        }
    }

    unsafe fn visit_mpz(&mut self, a: Obj) {
        unsafe {
            let new_a = alloc_mpz(mpz_value(a).clone());
            self.save(a, new_a);
        }
    }

    unsafe fn visit_ctor(&mut self, a: Obj) {
        unsafe {
            self.children.clear();
            let num_objs = lean_ctor_num_objs(a);
            let mut missing = false;
            for i in 0..num_objs {
                if !self.push_child(lean_ctor_get(a, i)) {
                    missing = true;
                }
            }
            if missing {
                return;
            }
            let (new_a, _) = alloc_ctor_like(a);
            for i in 0..num_objs {
                let c = self.children[i as usize];
                lean_inc(c);
                lean_ctor_set(new_a, i, c);
            }
            self.save(a, new_a);
        }
    }

    unsafe fn run(&mut self, a: Obj) -> Obj {
        unsafe {
            if self.push_child(a) {
                let r = *self.children.last().expect("pushed child");
                lean_inc(r);
                lean_dec(a);
                return self.state.pack(r);
            }
            while let Some(&curr) = self.todo.last() {
                match lean_ptr_tag(curr) {
                    LEAN_ARRAY => self.visit_array(curr),
                    LEAN_SCALAR_ARRAY => self.visit_sarray(curr),
                    LEAN_STRING => self.visit_string(curr),
                    LEAN_MPZ => self.visit_mpz(curr),
                    LEAN_CLOSURE | LEAN_THUNK | LEAN_TASK | LEAN_PROMISE | LEAN_REF | LEAN_EXTERNAL | LEAN_RESERVED => {
                        lean_internal_panic_unreachable()
                    }
                    _ => self.visit_ctor(curr),
                }
            }
            let o = self.state.map_find(a);
            if o == lean_box(0) {
                lean_internal_panic("ShareCommon: the root object was not recorded in the sharing map");
            }
            let r = lean_ctor_get(o, 0);
            lean_inc(r);
            lean_dec(o);
            lean_dec(a);
            self.state.pack(r)
        }
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_sharecommon_eq(o1: b_obj, o2: b_obj) -> u8 {
            sharecommon_eq(o1, o2) as u8
        }

        fn lean_sharecommon_hash(o: b_obj) -> u64 {
            sharecommon_hash(o)
        }

        // `ShareCommon.shareCommon'`: the argument is borrowed; the result is a new reference.
        fn lean_sharecommon_quick(a: b_obj) -> obj {
            let mut q = Quick { cache: HashMap::new(), set: HashSet::new() };
            q.visit(a)
        }

        // `ShareCommon.State.shareCommon {σ : @& StateFactory} (s : State σ) (a : α) : α × State σ`.
        fn lean_state_sharecommon(tc: b_obj, s: obj, a: obj) -> obj {
            let mut f = Persistent { state: LeanState::new(tc, s), children: Vec::new(), todo: Vec::new() };
            f.run(a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use super::*;

    unsafe fn pair(a: Obj, b: Obj) -> Obj {
        unsafe {
            let r = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(r, 0, a);
            lean_ctor_set(r, 1, b);
            r
        }
    }

    #[test]
    fn quick_sharing_deduplicates_equal_subterms() {
        unsafe {
            // (("ab", 1), ("ab", 1)) built from distinct but equal objects.
            let x = pair(lean_mk_string("ab"), lean_box(1));
            let y = pair(lean_mk_string("ab"), lean_box(1));
            let t = pair(x, y);
            assert_ne!(lean_ctor_get(t, 0), lean_ctor_get(t, 1));
            let r = lean_sharecommon_quick(t);
            assert_eq!(lean_ctor_get(r, 0), lean_ctor_get(r, 1));
            // Equality is shallow: the constructors differ only by pointers to equal strings,
            // while the strings themselves are equal objects.
            assert_eq!(lean_sharecommon_eq(lean_ctor_get(t, 0), lean_ctor_get(t, 1)), 0);
            let (s1, s2) = (lean_ctor_get(lean_ctor_get(t, 0), 0), lean_ctor_get(lean_ctor_get(t, 1), 0));
            assert_eq!(lean_sharecommon_eq(s1, s2), 1);
            assert_eq!(lean_sharecommon_hash(s1), lean_sharecommon_hash(s2));
            // The shared string is one object referenced from both components.
            assert_eq!(lean_ctor_get(lean_ctor_get(r, 0), 0), lean_ctor_get(lean_ctor_get(r, 1), 0));
            lean_dec(r);
            lean_dec(t);
        }
    }

    #[test]
    fn structural_equality_distinguishes_scalars_and_tags() {
        unsafe {
            let a = pair(lean_box(1), lean_box(2));
            let b = pair(lean_box(1), lean_box(3));
            assert_eq!(lean_sharecommon_eq(a, b), 0);
            let big1 = alloc_mpz(num_bigint::BigInt::from(u64::MAX) * 7);
            let big2 = alloc_mpz(num_bigint::BigInt::from(u64::MAX) * 7);
            assert_eq!(lean_sharecommon_eq(big1, big2), 1);
            assert_eq!(lean_sharecommon_hash(big1), lean_sharecommon_hash(big2));
            for o in [a, b, big1, big2] {
                lean_dec(o);
            }
        }
    }
}
