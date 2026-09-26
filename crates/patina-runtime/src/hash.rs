//! Lean's runtime hash functions, ported from `runtime/hash.h` and `runtime/hash.cpp`.

const M: u64 = 0xc6a4a7935bd1e995;
const R: u32 = 47;

/// MurmurHash64A over `data` with `seed`, as Lean's `hash_str`.
pub fn hash_str(data: &[u8], seed: u64) -> u64 {
    let len = data.len();
    let mut h = seed ^ (len as u64).wrapping_mul(M);
    let (chunks, tail) = data.as_chunks::<8>();
    for chunk in chunks {
        let mut k = u64::from_le_bytes(*chunk);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
    }
    if !tail.is_empty() {
        for (i, b) in tail.iter().enumerate().rev() {
            h ^= (*b as u64) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

/// Lean's `hash(h, k)` mixing function (also `lean_uint64_mix_hash`).
#[inline]
pub fn mix_hash(mut h: u64, mut k: u64) -> u64 {
    k = k.wrapping_mul(M);
    k ^= k >> R;
    k ^= M;
    h ^= k;
    h.wrapping_mul(M)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values computed by Lean 4.34.1 (`"...".hash`, which is `hash_str(len, s, 11)`).
    #[test]
    fn string_hashes_match_lean() {
        assert_eq!(hash_str(b"", 11), EMPTY_HASH);
        assert_eq!(hash_str(b"hello", 11), HELLO_HASH);
        assert_eq!(hash_str("héllo wörld, long enough".as_bytes(), 11), LONG_HASH);
    }

    pub(crate) const EMPTY_HASH: u64 = 9877294847684254529;
    pub(crate) const HELLO_HASH: u64 = 9821865621596011261;
    pub(crate) const LONG_HASH: u64 = 12994951943092991614;

    #[test]
    fn mix_hash_matches_lean() {
        // `mixHash 1 2` in Lean 4.34.1.
        assert_eq!(mix_hash(1, 2), MIX_1_2);
    }

    pub(crate) const MIX_1_2: u64 = 16582581243253999004;
}
