//! Test-side PRNG: convenience methods over the crate's own
//! [`multiway::det::SplitMix`], so the whole project has exactly one
//! mixing lineage and every generated case replays byte-identically.

use multiway::det::SplitMix;

pub struct Rng(SplitMix);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(SplitMix(seed))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform-ish value in `0..n`. Plain modulo — the bias is irrelevant
    /// for test-case generation and the simplicity is worth it.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0);
        self.next_u64() % n
    }

    /// Inclusive range.
    pub fn range_usize(&mut self, lo: usize, hi_incl: usize) -> usize {
        lo + self.below((hi_incl - lo + 1) as u64) as usize
    }

    /// True with probability `num/den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    /// Fisher–Yates, high-to-low.
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = self.below((i + 1) as u64) as usize;
            xs.swap(i, j);
        }
    }

    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}
