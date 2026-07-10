//! The parallel scan driver: enumerate (or sample) a rule space, probe
//! every rule, rank the atlas.
//!
//! Threading uses the SAME discipline as `system.rs`'s Phase A: workers
//! own round-robin index sets and results are collected BY INDEX, so
//! the merged vector — and therefore every downstream byte — is
//! identical for any thread count or scheduling (pinned by
//! `scan_thread_invariant`).

use crate::atlas::{rank, AtlasEntry};
use crate::probe::{probe, ProbeBudget, ProbeResult};
use crate::rulespace::{enumerate, sample, space_size, CanonRule, SpaceBudget};

/// Largest space an exhaustive scan will attempt. Beyond it, `scan`
/// refuses with the exact size — never a silent truncation; use
/// sampling instead.
pub const EXHAUSTIVE_CAP: u128 = 200_000;

/// Everything a scan depends on — same opts ⇒ byte-identical atlas.
#[derive(Clone, Copy, Debug)]
pub struct ScanOpts {
    /// The rule space.
    pub space: SpaceBudget,
    /// Per-rule probe budgets.
    pub probe: ProbeBudget,
    /// `Some((n, seed))` probes a deterministic sample instead of the
    /// full space.
    pub sample: Option<(usize, u64)>,
    /// Atlas rows to keep.
    pub top: usize,
    /// Worker threads (1 = serial; output is identical either way).
    pub threads: usize,
}

/// Scan a rule space: enumerate or sample, probe (in parallel), rank.
/// Pure function of `opts`.
pub fn scan(opts: &ScanOpts) -> Result<Vec<AtlasEntry>, String> {
    let rules: Vec<CanonRule> = match opts.sample {
        Some((n, seed)) => sample(&opts.space, n, seed),
        None => {
            let size = space_size(&opts.space);
            if size > EXHAUSTIVE_CAP {
                return Err(format!(
                    "space has {} classes — beyond the exhaustive cap of {}; \
                     use --sample N to scan a deterministic subset",
                    size, EXHAUSTIVE_CAP
                ));
            }
            enumerate(&opts.space)
        }
    };
    let probes = probe_all(&rules, &opts.probe, opts.threads);
    Ok(rank(rules.into_iter().zip(probes).collect(), opts.top))
}

/// Probe every rule; round-robin worker index sets, collected by index.
fn probe_all(rules: &[CanonRule], budget: &ProbeBudget, threads: usize) -> Vec<ProbeResult> {
    let probe_one = |i: usize| -> ProbeResult { probe(&rules[i].to_rule(), budget) };

    let workers = threads.min(rules.len());
    if workers <= 1 {
        return (0..rules.len()).map(probe_one).collect();
    }

    let mut merged: Vec<Option<ProbeResult>> = (0..rules.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|t| {
                let probe_one = &probe_one;
                s.spawn(move || {
                    let mut out = Vec::new();
                    let mut i = t;
                    while i < rules.len() {
                        out.push((i, probe_one(i)));
                        i += workers;
                    }
                    out
                })
            })
            .collect();
        for h in handles {
            for (i, r) in h.join().expect("scan worker panicked") {
                merged[i] = Some(r);
            }
        }
    });
    merged
        .into_iter()
        .map(|r| r.expect("every index probed"))
        .collect()
}
