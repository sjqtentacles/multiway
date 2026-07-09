//! Determinism primitives: the project-wide mixing function, deterministic
//! hash maps, and a seeded PRNG.
//!
//! The engine's contract is byte-identical output for identical input.
//! `std`'s default `HashMap` hasher (`RandomState`) is seeded per process,
//! which is fine for point lookups but a landmine the moment any map's
//! iteration order can reach output. [`DetMap`]/[`DetSet`] remove that risk
//! class entirely: same contents, same hashes, every process.
//!
//! Project rule (enforced in review, pinned by byte-determinism tests):
//! any map or set whose iteration order can reach output must be a
//! [`DetMap`]/[`DetSet`] iterated in sorted-key order, or a `Vec`.

/// The splitmix64 finalizer with the golden-ratio increment folded in.
///
/// This is the single mixing lineage for the whole project: `wl_hash`,
/// canonical forms, [`DetHasher`], and the test PRNG all bottom out here.
/// Its outputs are pinned by `mix_reference_values` — changing this
/// function invalidates every golden file and every recorded fuzz seed.
#[inline]
pub fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Order-sensitive hash of a sequence (deterministic across runs — no
/// std RandomState anywhere, results must be reproducible).
pub fn hash_seq(xs: &[u64]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &x in xs {
        h = mix(h ^ x);
    }
    mix(h ^ (xs.len() as u64))
}

/// Deterministic [`std::hash::Hasher`]: folds 8-byte little-endian chunks
/// through [`mix`]. Not designed to resist adversarial collisions — inputs
/// here are the engine's own canonical data, not attacker-controlled keys.
pub struct DetHasher {
    h: u64,
}

impl std::hash::Hasher for DetHasher {
    fn finish(&self) -> u64 {
        self.h
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            self.h = mix(self.h ^ u64::from_le_bytes(buf) ^ ((chunk.len() as u64) << 56));
        }
    }
}

/// [`std::hash::BuildHasher`] for [`DetHasher`] — a fixed seed, no
/// per-process randomness.
#[derive(Clone, Copy, Default)]
pub struct BuildDet;

impl std::hash::BuildHasher for BuildDet {
    type Hasher = DetHasher;
    fn build_hasher(&self) -> DetHasher {
        DetHasher {
            h: 0xCBF2_9CE4_8422_2325,
        }
    }
}

/// `HashMap` with process-independent hashing.
pub type DetMap<K, V> = std::collections::HashMap<K, V, BuildDet>;
/// `HashSet` with process-independent hashing.
pub type DetSet<T> = std::collections::HashSet<T, BuildDet>;

/// splitmix64 PRNG. Deterministic by construction; the test harness builds
/// its generators on top of this so every failing case is reproducible from
/// a printed seed.
pub struct SplitMix(pub u64);

impl SplitMix {
    /// Next pseudo-random value: advance the Weyl sequence, finalize with
    /// [`mix`]. The stream for seed 1 is pinned by
    /// `splitmix_reference_values`.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hasher};

    /// Pins the mixing function forever. Canonical forms, wl_hash values,
    /// golden files, and every recorded fuzz seed depend on these bytes; a
    /// silent "improvement" to `mix` must fail loudly here first.
    #[test]
    fn mix_reference_values() {
        assert_eq!(mix(0), 0xe220a8397b1dcdaf);
        assert_eq!(mix(1), 0x910a2dec89025cc1);
        assert_eq!(mix(0x5EED), 0x09f1fd9d03f0a9b4);
        assert_eq!(mix(u64::MAX), 0xe4d971771b652c20);
    }

    #[test]
    fn hash_seq_reference_values() {
        assert_eq!(hash_seq(&[]), 0xc3817c016ba4ff30);
        assert_eq!(hash_seq(&[1, 2, 3]), 0xab67836aaf9a3881);
    }

    /// Pins the PRNG stream for seed 1. Every generated fuzz case in the
    /// test suite descends from this stream; changing it invalidates every
    /// recorded repro seed.
    #[test]
    fn splitmix_reference_values() {
        let mut rng = SplitMix(1);
        let got: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
        assert_eq!(
            got,
            vec![
                0xbeeb8da1658eec67,
                0xf893a2eefb32555e,
                0x71c18690ee42c90b,
                0x71bb54d8d101b5b9,
            ]
        );
    }

    /// The falsifiable no-RandomState test: a pinned hash of a known key.
    /// `RandomState` seeds per process, so a pinned constant would fail on
    /// every run after the one that minted it; `BuildDet` must reproduce it
    /// always.
    #[test]
    fn det_hasher_reference_values() {
        let mut h = BuildDet.build_hasher();
        h.write(b"multiway");
        assert_eq!(h.finish(), 0x2fe6bb0dd43b9548);

        let mut h2 = BuildDet.build_hasher();
        h2.write(b"multiway!");
        let v2 = h2.finish();
        // one extra byte must change the hash (chunk-length salting)
        assert_ne!(v2, 0x2fe6bb0dd43b9548);
    }
}
