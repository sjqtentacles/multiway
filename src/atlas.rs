//! Interestingness scoring + ranking — the scanner's editorial layer.
//!
//! Everything here is INTEGER arithmetic in milli-units: a 1-ulp float
//! difference must never reorder the atlas across OSes, so there are no
//! floats anywhere near the order. The weights are named constants and
//! the reference-score tests pin exact values for the gallery rules —
//! every weight tweak is a visible, reviewed diff, never a silent drift.
//!
//! Honesty notes (also in the README when the atlas ships):
//! interestingness is a HEURISTIC — defended by determinism and pinned
//! scores, not by pretending it's objective. `Exploded` means
//! budget-hit, which conflates "genuinely explosive" with "expensive";
//! it is labeled that way, never "chaotic". Dedup is by 64-bit
//! fingerprint: a collision misfiles a rule as an alias of another
//! (listed, never dropped); exact top-K verification is v1.1.

use crate::confluence::{check, CheckCfg, Verdict};
use crate::det::{log2_milli, DetMap};
use crate::probe::{FinalShape, GrowthClass, Outcome, ProbeResult};
use crate::rulespace::CanonRule;

/// Weight: fraction of seeds that survive all steps within budget
/// (neither dies nor explodes — the edge-of-chaos precondition).
pub const W_SURVIVE: i64 = 3000;
/// Weight: growth class, peaking at Poly (growing but sub-exponential).
pub const W_GROWTH: i64 = 2000;
/// Weight: path sharing, log-capped (subdivision-style maximal sharing).
pub const W_SHARING: i64 = 1500;
/// Weight: periodicity (longer cycles more interesting, log-capped).
pub const W_PERIOD: i64 = 1200;
/// Weight: branchial density, peaking mid-range (some interference,
/// not total).
pub const W_BRANCH: i64 = 800;
/// Weight: seed sensitivity — distinct growth classes across seeds.
pub const W_SEEDVAR: i64 = 700;
/// Weight: final-state shape (recognizable geometry beats soup).
pub const W_SHAPE: i64 = 500;
/// Weight: tier-2 confluence class (finalists only).
pub const W_CONFL: i64 = 300;

/// How many candidates (as a multiple of `top`) receive the tier-2
/// confluence pass. Sound: tier-2 only ADDS to a score, and every
/// finalist's tier-1 score already ≥ every non-finalist's, so no
/// non-finalist could have entered the top set.
pub const FINALIST_FACTOR: usize = 4;

/// Tier-2 confluence classification (closed set the atlas can render).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflClass {
    /// Every critical pair strongly joinable at the probe bounds.
    Confluent,
    /// A genuine divergence witness — interesting in its own right.
    NotConfluent,
    /// No verdict at the bounds.
    Inconclusive,
    /// Pair enumeration exceeded the cap (recorded, not an error).
    PairsCapped,
}

/// One ranked atlas row.
#[derive(Clone, Debug)]
pub struct AtlasEntry {
    /// Representative rule — the LEAST [`CanonRule`] of its
    /// fingerprint class.
    pub rule: CanonRule,
    /// How many scanned rules share this fingerprint (≥ 1, counting the
    /// representative; aliases are counted, never dropped).
    pub aliases: u32,
    /// The representative's probe.
    pub probe: ProbeResult,
    /// Total integer score (tier-1, plus tier-2 for finalists).
    pub score: i64,
    /// Tier-2 confluence class — `Some` only for finalists.
    pub confluence: Option<ConflClass>,
}

/// Tier-1 interestingness score: a pure integer function of the probe.
pub fn score_tier1(p: &ProbeResult) -> i64 {
    let n = p.seeds.len().max(1) as i64;

    // survive: seeds that ran all steps within budget
    let ran = p
        .seeds
        .iter()
        .filter(|s| matches!(s.outcome, Outcome::Ran))
        .count() as i64;
    let survive = W_SURVIVE * (ran * 1000 / n) / 1000;

    // growth: best class across seeds, Poly is the peak
    let growth_milli = p
        .seeds
        .iter()
        .map(|s| match s.growth {
            GrowthClass::Dies => 0i64,
            GrowthClass::Static => 100,
            GrowthClass::Exploded => 100,
            GrowthClass::Periodic { .. } => 400,
            GrowthClass::Exp => 500,
            GrowthClass::Linear => 600,
            GrowthClass::Poly => 1000,
        })
        .max()
        .unwrap_or(0);
    let growth = W_GROWTH * growth_milli / 1000;

    // sharing: log2 of the best last-layer sharing factor, capped at
    // 10 bits (2^10 = 1024x sharing saturates the term)
    let best_sharing = p
        .seeds
        .iter()
        .filter_map(|s| s.sharing_milli.last())
        .max()
        .copied()
        .unwrap_or(0);
    let sharing_bits_milli = log2_milli((best_sharing / 1000).max(1) as u128).min(10_000) as i64;
    let sharing = W_SHARING * sharing_bits_milli / 10_000;

    // period: log2(1 + lambda), capped at 4 bits (period 16 saturates)
    let best_lambda = p
        .seeds
        .iter()
        .filter_map(|s| s.period.map(|(_, l)| l))
        .max()
        .unwrap_or(0);
    let period_bits_milli = log2_milli(1 + best_lambda as u128).min(4_000) as i64;
    let period = W_PERIOD * period_bits_milli / 4_000;

    // branchial density: tent peaking at 500 milli (some sibling
    // interference is interesting; zero and saturation are not)
    let best_branch = p
        .seeds
        .iter()
        .map(|s| s.branchial_milli.min(1000) as i64)
        .max()
        .unwrap_or(0);
    let branch_milli = 1000 - (best_branch - 500).abs() * 2;
    let branch = W_BRANCH * branch_milli.max(0) / 1000;

    // seed sensitivity: distinct growth classes (1..=3 seeds)
    let mut distinct = 0i64;
    let mut seen: Vec<GrowthClass> = Vec::new();
    for c in p.seeds.iter().map(|s| s.growth) {
        if !seen.contains(&c) {
            seen.push(c);
            distinct += 1;
        }
    }
    let seedvar = W_SEEDVAR * ((distinct - 1).max(0) * 500).min(1000) / 1000;

    // shape: recognizable geometry from the best seed
    let shape_milli = p
        .seeds
        .iter()
        .map(|s| match s.final_shape {
            FinalShape::Cycle | FinalShape::Star => 1000i64,
            FinalShape::Tree => 800,
            FinalShape::Path => 600,
            FinalShape::Dense => 500,
            FinalShape::Other => 300,
            FinalShape::SelfLoops => 200,
            FinalShape::Empty => 0,
        })
        .max()
        .unwrap_or(0);
    let shape = W_SHAPE * shape_milli / 1000;

    survive + growth + sharing + period + branch + seedvar + shape
}

/// Tier-2 confluence score contribution.
fn score_confl(c: ConflClass) -> i64 {
    let milli = match c {
        // a bounded divergence witness is the rarest, coolest find
        ConflClass::NotConfluent => 1000,
        ConflClass::Confluent => 800,
        ConflClass::Inconclusive => 300,
        ConflClass::PairsCapped => 100,
    };
    W_CONFL * milli / 1000
}

/// Run the bounded confluence check on a single rule (scanner budgets —
/// far below the CLI defaults; overflow is a recorded class, not an
/// error).
fn confluence_class(rule: &CanonRule) -> ConflClass {
    let cfg = CheckCfg {
        join_depth: 4,
        max_states: 200,
        pair_cap: 64,
    };
    match check(&[rule.to_rule()], &cfg) {
        Ok(report) => match report.verdict {
            Verdict::AllCriticalPairsStronglyJoinable { .. } => ConflClass::Confluent,
            Verdict::NotConfluent { .. } => ConflClass::NotConfluent,
            Verdict::Inconclusive { .. } => ConflClass::Inconclusive,
        },
        // check() errors only on the pair cap at these budgets
        Err(_) => ConflClass::PairsCapped,
    }
}

/// Dedup by fingerprint, score, rank, tier-2 the finalists, return the
/// top `top` entries.
///
/// Order-independence: the fingerprint → class map is a [`DetMap`] and
/// the representative is the LEAST rule of each class, so the result is
/// a pure function of the input SET (pinned by
/// `prop_rank_stable_under_input_permutation`).
pub fn rank(items: Vec<(CanonRule, ProbeResult)>, top: usize) -> Vec<AtlasEntry> {
    // dedup: fingerprint -> (least rule, its probe, alias count)
    let mut classes: DetMap<u64, (CanonRule, ProbeResult, u32)> = DetMap::default();
    for (rule, probe) in items {
        match classes.get_mut(&probe.fingerprint) {
            Some(entry) => {
                entry.2 += 1;
                if rule < entry.0 {
                    entry.0 = rule;
                    entry.1 = probe;
                }
            }
            None => {
                classes.insert(probe.fingerprint, (rule, probe, 1));
            }
        }
    }

    let mut entries: Vec<AtlasEntry> = classes
        .into_iter()
        .map(|(_, (rule, probe, aliases))| {
            let score = score_tier1(&probe);
            AtlasEntry {
                rule,
                aliases,
                probe,
                score,
                confluence: None,
            }
        })
        .collect();

    // total order: highest score first, ties broken by rule text
    entries.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.rule.cmp(&b.rule)));

    // tier-2 on the finalists (sound: adds only — see FINALIST_FACTOR)
    let finalists = (top.saturating_mul(FINALIST_FACTOR)).min(entries.len());
    for e in entries.iter_mut().take(finalists) {
        let c = confluence_class(&e.rule);
        e.confluence = Some(c);
        e.score += score_confl(c);
    }
    entries[..finalists].sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.rule.cmp(&b.rule)));

    entries.truncate(top);
    entries
}
