//! Fast hash maps for integer keys (chunk coordinates, LOD section ids, job ids): `HashMap` and
//! `HashSet` with a multiply-rotate hasher (FxHash, as rustc uses) instead of std's SipHash, which
//! the client's hot paths spent most of their time in (docs/DEV_TOOLS.md profile, 2026-10-14). Not
//! for keys an attacker chooses (no DoS resistance) — every key here is the game's own.
//!
//! Construct with `HashMap::default()` / `HashSet::default()` (`new` is std's `RandomState` only).
//! Iteration order is fixed by the keys (std's is random per process), so nothing may rely on it
//! either way.

use std::hash::{BuildHasherDefault, Hasher};

/// FxHash: per word, `(h.rotl(5) ^ word) × K`; `finish` rotates so the low bits (the table index)
/// depend on every input bit, not only the low ones (rustc-hash 2's fix).
#[derive(Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

const K: u64 = 0xf135_7aea_2e62_a9c5;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().expect("8 bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut b = [0u8; 8];
            b[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(b));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.add(u64::from(i as u32));
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash.rotate_left(26)
    }
}

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type HashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
pub type HashSet<T> = std::collections::HashSet<T, FxBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn hash<T: Hash>(t: &T) -> u64 {
        FxBuildHasher::default().hash_one(t)
    }

    #[test]
    fn is_deterministic_and_tells_neighbouring_coordinates_apart() {
        assert_eq!(hash(&(1i32, 2i32, 3i32)), hash(&(1i32, 2i32, 3i32)));
        // A 64³ block of coordinates (chunks around a player): no collisions in the full hash, and
        // the low bits (the table index) spread over the buckets.
        let mut full = HashSet::default();
        let mut low = vec![0u32; 1 << 12];
        for x in -32..32 {
            for y in -32..32 {
                for z in -32..32 {
                    let h = hash(&(x, y, z));
                    assert!(full.insert(h));
                    low[(h & 0xfff) as usize] += 1;
                }
            }
        }
        // 262 144 keys over 4096 buckets: 64 each on average.
        let worst = *low.iter().max().expect("buckets");
        assert!(worst < 160, "worst bucket {worst}");
    }

    #[test]
    fn maps_work_as_maps() {
        let mut m: HashMap<i64, &str> = HashMap::default();
        m.insert(-1, "a");
        m.insert(1 << 40, "b");
        assert_eq!(m[&-1], "a");
        assert_eq!(m.get(&(1 << 40)), Some(&"b"));
        let bytes = hash(&"a string of more than eight bytes");
        assert_ne!(bytes, hash(&"a string of more than eight bytes!"));
    }
}
