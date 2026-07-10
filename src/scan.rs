//! The parallel scan driver: enumerate (or sample) a rule space, probe
//! every rule, rank the atlas.
//!
//! Threading uses the SAME discipline as `system.rs`'s Phase A: workers
//! own round-robin index sets and results are collected BY INDEX, so
//! the merged vector — and therefore every downstream byte — is
//! identical for any thread count or scheduling (pinned by
//! `scan_thread_invariant`).

use crate::atlas::{rank, showcase_seed, AtlasEntry, ConflClass};
use crate::export::esc;
use crate::probe::{
    probe, ExplodeReason, FinalShape, GrowthClass, Outcome, ProbeBudget, ProbeResult,
};
use crate::rulespace::{enumerate, sample, space_size, CanonRule, SpaceBudget};
use crate::stats::{group_digits, sparkline};
use std::fmt::Write;

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

/// The ranked atlas as a terminal table. Echoes the space and probe
/// budgets (the inputs that DEFINE the output) but never the thread
/// count — output is thread-invariant and mentioning threads would put
/// a non-input in the bytes.
///
/// Sparkline glyphs are f64/ln display (3-OS golden precedent); the
/// scores — the ordering-bearing values — are pure integers.
pub fn scan_text(opts: &ScanOpts, entries: &[AtlasEntry]) -> String {
    let mut out = String::new();
    let size = space_size(&opts.space);
    writeln!(
        out,
        "multiway atlas — top {} of {} rule classes",
        entries.len(),
        group_digits(size)
    )
    .unwrap();
    let b = &opts.space;
    writeln!(
        out,
        "space: lhs<={} rhs<={} arity {}..={} vars<={}",
        b.max_lhs, b.max_rhs, b.min_arity, b.max_arity, b.max_vars
    )
    .unwrap();
    let p = &opts.probe;
    writeln!(
        out,
        "probe: steps {} states<={} events<={} edges<={} leaves<={} run<={}",
        p.steps, p.max_states, p.max_events, p.max_edges, p.max_canon_leaves, p.run_events
    )
    .unwrap();
    match opts.sample {
        Some((n, seed)) => writeln!(out, "sample: {} @ {:#x}", n, seed).unwrap(),
        None => writeln!(out, "scan: exhaustive").unwrap(),
    }
    writeln!(out).unwrap();
    writeln!(
        out,
        "┌─────┬────────┬────┬────────────────────────────────────┬─────────────┬──────────┬──────────────┐"
    )
    .unwrap();
    writeln!(
        out,
        "│   # │  score │  × │ rule                               │ growth      │ layers   │ confluence   │"
    )
    .unwrap();
    writeln!(
        out,
        "├─────┼────────┼────┼────────────────────────────────────┼─────────────┼──────────┼──────────────┤"
    )
    .unwrap();
    for (i, e) in entries.iter().enumerate() {
        let mut rule = e.rule.text();
        if rule.chars().count() > 34 {
            rule = rule.chars().take(33).collect::<String>() + "…";
        }
        let growth: Vec<&str> = e
            .probe
            .seeds
            .iter()
            .map(|s| growth_code(s.growth))
            .collect();
        let show = &e.probe.seeds[showcase_seed(&e.probe)];
        let spark = sparkline(&show.layers.iter().map(|&l| l as u128).collect::<Vec<_>>());
        writeln!(
            out,
            "│ {:>3} │ {:>6} │ {:>2} │ {:<34} │ {:<11} │ {:<8} │ {:<12} │",
            i + 1,
            group_digits(e.score.max(0) as u128),
            e.aliases,
            rule,
            growth.join("/"),
            spark,
            confl_code(e.confluence),
        )
        .unwrap();
    }
    writeln!(
        out,
        "└─────┴────────┴────┴────────────────────────────────────┴─────────────┴──────────┴──────────────┘"
    )
    .unwrap();
    writeln!(
        out,
        "growth codes: die sta per lin pol exp BUD (BUD = budget-hit, NOT proven chaotic)"
    )
    .unwrap();
    out
}

fn growth_code(g: GrowthClass) -> &'static str {
    match g {
        GrowthClass::Dies => "die",
        GrowthClass::Static => "sta",
        GrowthClass::Periodic { .. } => "per",
        GrowthClass::Linear => "lin",
        GrowthClass::Poly => "pol",
        GrowthClass::Exp => "exp",
        GrowthClass::Exploded => "BUD",
    }
}

fn confl_code(c: Option<ConflClass>) -> &'static str {
    match c {
        Some(ConflClass::Confluent) => "confluent",
        Some(ConflClass::NotConfluent) => "divergent",
        Some(ConflClass::Inconclusive) => "inconclusive",
        Some(ConflClass::PairsCapped) => "pairs-capped",
        None => "-",
    }
}

/// The full scan result as JSON, following `export.rs` conventions
/// (hand-built, camelCase, escaped strings, deterministic order).
/// `page` names the per-rule viewer file a `--atlas` bake writes; it is
/// present even in `--scan-json`-only runs (the name is a pure function
/// of the rank). Fingerprints are hex STRINGS — they don't fit a JS
/// number.
pub fn scan_json(opts: &ScanOpts, entries: &[AtlasEntry]) -> String {
    let b = &opts.space;
    let p = &opts.probe;
    let sample = match opts.sample {
        Some((n, seed)) => format!("{{\"n\":{},\"seed\":\"{:#x}\"}}", n, seed),
        None => "null".to_string(),
    };
    let rows: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let seeds: Vec<String> = e.probe.seeds.iter().map(seed_json).collect();
            format!(
                "{{\"rank\":{},\"rule\":\"{}\",\"page\":\"rule-{:03}.html\",\"score\":{},\"aliases\":{},\"confluence\":{},\"fingerprint\":\"{:016x}\",\"showcaseSeed\":{},\"seeds\":[{}]}}",
                i + 1,
                esc(&e.rule.text()),
                i + 1,
                e.score,
                e.aliases,
                match e.confluence {
                    Some(c) => format!("\"{}\"", confl_json(c)),
                    None => "null".to_string(),
                },
                e.probe.fingerprint,
                showcase_seed(&e.probe),
                seeds.join(",")
            )
        })
        .collect();
    format!(
        "{{\"space\":{{\"maxLhs\":{},\"maxRhs\":{},\"minArity\":{},\"maxArity\":{},\"maxVars\":{}}},\"probe\":{{\"steps\":{},\"maxStates\":{},\"maxEvents\":{},\"maxEdges\":{},\"maxCanonLeaves\":{},\"runEvents\":{}}},\"classes\":{},\"sample\":{},\"top\":{},\"entries\":[{}]}}",
        b.max_lhs,
        b.max_rhs,
        b.min_arity,
        b.max_arity,
        b.max_vars,
        p.steps,
        p.max_states,
        p.max_events,
        p.max_edges,
        p.max_canon_leaves,
        p.run_events,
        space_size(&opts.space),
        sample,
        opts.top,
        rows.join(",")
    )
}

fn seed_json(s: &crate::probe::SeedRun) -> String {
    let (outcome, halt_step, explode) = match s.outcome {
        Outcome::Ran => ("ran", "null".to_string(), "null".to_string()),
        Outcome::Halted { step } => ("halted", step.to_string(), "null".to_string()),
        Outcome::Exploded(r) => (
            "exploded",
            "null".to_string(),
            format!("\"{}\"", explode_json(r)),
        ),
    };
    let growth_name = match s.growth {
        GrowthClass::Dies => "dies",
        GrowthClass::Static => "static",
        GrowthClass::Periodic { .. } => "periodic",
        GrowthClass::Linear => "linear",
        GrowthClass::Poly => "poly",
        GrowthClass::Exp => "exp",
        // honesty label: a budget hit is not evidence of chaos
        GrowthClass::Exploded => "budget-hit",
    };
    let (mu, lambda) = match s.growth {
        GrowthClass::Periodic { mu, lambda } => (mu.to_string(), lambda.to_string()),
        _ => ("null".to_string(), "null".to_string()),
    };
    let layers: Vec<String> = s.layers.iter().map(|l| l.to_string()).collect();
    let sharing: Vec<String> = s.sharing_milli.iter().map(|v| v.to_string()).collect();
    format!(
        "{{\"outcome\":\"{}\",\"haltStep\":{},\"explodeReason\":{},\"growth\":\"{}\",\"mu\":{},\"lambda\":{},\"layers\":[{}],\"states\":{},\"events\":{},\"backMerges\":{},\"branchPairs\":{},\"sharingMilli\":[{}],\"shape\":\"{}\",\"degreeEntropyMilli\":{},\"branchialMilli\":{}}}",
        outcome,
        halt_step,
        explode,
        growth_name,
        mu,
        lambda,
        layers.join(","),
        s.states,
        s.events,
        s.back_merges,
        s.branch_pairs,
        sharing.join(","),
        shape_json(s.final_shape),
        s.degree_entropy_milli,
        s.branchial_milli
    )
}

fn explode_json(r: ExplodeReason) -> &'static str {
    match r {
        ExplodeReason::States => "states",
        ExplodeReason::Events => "events",
        ExplodeReason::EdgesPerState => "edgesPerState",
        ExplodeReason::CanonBudget => "canonBudget",
    }
}

fn shape_json(s: FinalShape) -> &'static str {
    match s {
        FinalShape::Empty => "empty",
        FinalShape::SelfLoops => "selfLoops",
        FinalShape::Path => "path",
        FinalShape::Cycle => "cycle",
        FinalShape::Star => "star",
        FinalShape::Tree => "tree",
        FinalShape::Dense => "dense",
        FinalShape::Other => "other",
    }
}

fn confl_json(c: ConflClass) -> &'static str {
    match c {
        ConflClass::Confluent => "confluent",
        ConflClass::NotConfluent => "divergent",
        ConflClass::Inconclusive => "inconclusive",
        ConflClass::PairsCapped => "pairsCapped",
    }
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
