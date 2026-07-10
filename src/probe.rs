//! Bounded, deterministic behavior probing — the scanner's measurement
//! instrument.
//!
//! The probe runs its OWN bounded multiway loop (mirroring `system.rs`'s
//! dedup + path-count DP) instead of a modified `evolve`: budgets must
//! abort mid-layer, and threading limits through the engine would risk
//! the baseline pins for no benefit. The differential test
//! `prop_probe_matches_evolve` keeps this loop honest against the real
//! engine forever.
//!
//! Every loop is capped by a COUNT — states, events, edges per state,
//! canonization leaves, run events. There is no wall clock anywhere:
//! a probe result is a pure function of `(rule, budget)`.
//!
//! Classification is PER SEED and never averaged: the same rule can die
//! on the loop seed and explode on the path seed, and that disagreement
//! is itself signal (the scanner scores it). Budget-exhausted rules are
//! classified `Exploded(reason)` — which honestly conflates "genuinely
//! explosive" with "expensive"; the atlas labels them `budget-hit`,
//! never "chaotic".

use crate::canon::canonicalize_budgeted;
use crate::det::{hash_seq, log2_milli, DetMap};
use crate::hypergraph::{Edge, State};
use crate::matcher::{apply_full, find_matches};
use crate::rule::Rule;

/// Count-based probe budgets (no wall clock — determinism).
#[derive(Clone, Copy, Debug)]
pub struct ProbeBudget {
    /// Multiway layers to attempt.
    pub steps: usize,
    /// Total canonical states across the run.
    pub max_states: usize,
    /// Total multiway events.
    pub max_events: usize,
    /// Edges per state (a child larger than this explodes the run).
    pub max_edges: usize,
    /// IR leaf cap per canonization (symmetric states cost k! leaves).
    pub max_canon_leaves: u64,
    /// Sequential-run length for period detection.
    pub run_events: usize,
}

impl Default for ProbeBudget {
    fn default() -> Self {
        ProbeBudget {
            steps: 5,
            max_states: 500,
            max_events: 20_000,
            max_edges: 64,
            max_canon_leaves: 10_000,
            run_events: 64,
        }
    }
}

/// Which budget a run exhausted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExplodeReason {
    /// Canonical-state budget.
    States,
    /// Event budget.
    Events,
    /// A state exceeded the per-state edge cap.
    EdgesPerState,
    /// Canonization hit its IR leaf cap (high symmetry).
    CanonBudget,
}

/// How a bounded run ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The frontier emptied before `steps` — genuinely terminated.
    Halted {
        /// Step at which no new states appeared.
        step: usize,
    },
    /// Completed all steps within budget.
    Ran,
    /// A budget was exhausted (see the honesty note in module docs).
    Exploded(ExplodeReason),
}

/// Coarse growth classification from canonical layer sizes (integer
/// arithmetic only). With ≤ 6 layers this is deliberately coarse; real
/// asymptotics need far deeper runs (v2 territory).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrowthClass {
    /// Evolution died (empty layer).
    Dies,
    /// Layer sizes constant.
    Static,
    /// The sequential run revisited a canonical form: period `lambda`
    /// after transient `mu`.
    Periodic {
        /// Transient length.
        mu: u32,
        /// Cycle length.
        lambda: u32,
    },
    /// Layer sizes grow by a constant difference.
    Linear,
    /// Growing, sub-exponential (the edge-of-chaos sweet spot).
    Poly,
    /// Sustained ≥ 1.5× layer-ratio growth.
    Exp,
    /// A budget was hit before classification was possible.
    Exploded,
}

/// Degree-profile shape of the sequential run's final state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalShape {
    /// No edges.
    Empty,
    /// Every edge is an all-same-vertex loop.
    SelfLoops,
    /// Binary connected, two degree-1 endpoints, rest degree 2.
    Path,
    /// Binary connected, every vertex total degree 2.
    Cycle,
    /// One vertex participates in every edge.
    Star,
    /// Binary, edges == vertices - 1 (tree-like edge count).
    Tree,
    /// Edge count > 2 × vertex count.
    Dense,
    /// Anything else.
    Other,
}

/// One seed's bounded-run measurements.
#[derive(Clone, Debug)]
pub struct SeedRun {
    /// How the multiway run ended.
    pub outcome: Outcome,
    /// Canonical layer sizes.
    pub layers: Vec<u32>,
    /// Total canonical states / events / back-merges / branchial pairs.
    pub states: u32,
    /// Total events.
    pub events: u32,
    /// Merges into earlier layers.
    pub back_merges: u32,
    /// Same-parent sibling pairs (count only).
    pub branch_pairs: u32,
    /// Per-layer sharing ×1000 (naive paths / canonical); empty when
    /// back-merges make it meaningless (same gating as stats.rs).
    pub sharing_milli: Vec<u64>,
    /// Growth classification.
    pub growth: GrowthClass,
    /// First canonical-form repeat in the sequential run.
    pub period: Option<(u32, u32)>,
    /// Shape of the sequential run's final state.
    pub final_shape: FinalShape,
    /// Σ degree·log2_milli(degree) over the final state.
    pub degree_entropy_milli: u64,
    /// Branchial density ×1000 at the final layer.
    pub branchial_milli: u64,
    /// Evolution fingerprint (per-layer sorted canonical form hashes).
    pub fingerprint: u64,
}

/// A rule's full probe: one [`SeedRun`] per canonical seed, plus the
/// combined fingerprint (equal fingerprints ⇒ the entire bounded
/// canonical evolutions coincide on every seed, up to 64-bit collision —
/// collisions misfile a rule as an alias, never drop it).
#[derive(Clone, Debug)]
pub struct ProbeResult {
    /// Per-seed measurements, in [`seeds_for`] order.
    pub seeds: Vec<SeedRun>,
    /// Rule-level fingerprint over the per-seed fingerprints.
    pub fingerprint: u64,
}

/// The three deterministic seeds, arity-matched to the rule's LHS:
/// **loop** (every LHS edge as an all-`0` self-edge — the classic
/// `{{0,0},{0,0}}` pattern), **path** (edges chained to share one vertex
/// consecutively), and **doubled path** (each path edge duplicated —
/// exposes multiset-sensitive rules).
pub fn seeds_for(rule: &Rule) -> Vec<State> {
    // sorted arity multiset: invariant under rule equivalence (raw LHS
    // edge order is not — [3,1] vs [1,3] would build non-isomorphic
    // path seeds; found red by the fingerprint-invariance prop)
    let mut arities: Vec<usize> = rule.lhs.iter().map(|e| e.len()).collect();
    arities.sort_unstable();

    let loop_seed = State::new(arities.iter().map(|&a| vec![0u32; a]).collect());

    let mut path_edges: Vec<Edge> = Vec::new();
    let mut cursor = 0u32;
    for &a in &arities {
        let edge: Edge = (0..a as u32).map(|i| cursor + i).collect();
        cursor += (a as u32).saturating_sub(1);
        path_edges.push(edge);
    }
    let path_seed = State::new(path_edges.clone());

    let mut doubled = Vec::new();
    for e in &path_edges {
        doubled.push(e.clone());
        doubled.push(e.clone());
    }
    let doubled_seed = State::new(doubled);

    vec![loop_seed, path_seed, doubled_seed]
}

/// Probe a rule under a budget: bounded multiway run + bounded
/// sequential run per seed. Pure function of its arguments.
pub fn probe(rule: &Rule, budget: &ProbeBudget) -> ProbeResult {
    let seeds: Vec<SeedRun> = seeds_for(rule)
        .into_iter()
        .map(|seed| probe_seed(rule, seed, budget))
        .collect();
    let fp_inputs: Vec<u64> = seeds.iter().map(|s| s.fingerprint).collect();
    let fingerprint = hash_seq(&fp_inputs);
    ProbeResult { seeds, fingerprint }
}

/// Hash of a canonical form's edge list (exact content, order-sensitive
/// — forms are already sorted).
fn form_hash(edges: &[Edge]) -> u64 {
    let mut seq: Vec<u64> = Vec::with_capacity(edges.len() * 3);
    for e in edges {
        seq.push(0xED6E); // edge marker
        seq.push(e.len() as u64);
        seq.extend(e.iter().map(|&v| v as u64));
    }
    hash_seq(&seq)
}

fn probe_seed(rule: &Rule, init: State, b: &ProbeBudget) -> SeedRun {
    // --- bounded multiway run (mirrors system.rs's dedup + path DP) ---
    let mut states: Vec<State> = Vec::new();
    let mut path_counts: Vec<u128> = Vec::new();
    let mut layers: Vec<Vec<usize>> = Vec::new();
    let mut layer_hashes: Vec<Vec<u64>> = Vec::new();
    let mut canon_map: DetMap<Vec<Edge>, usize> = DetMap::default();
    let mut events = 0u32;
    let mut back_merges = 0u32;
    let mut branch_pairs = 0u32;

    let mut state_step: Vec<usize> = Vec::new();
    // (an inner fn instead of a labeled block: `'label: {}` needs 1.65,
    // MSRV is 1.63)
    #[allow(clippy::too_many_arguments)]
    fn run_multiway(
        rule: &Rule,
        init: State,
        b: &ProbeBudget,
        states: &mut Vec<State>,
        state_step: &mut Vec<usize>,
        path_counts: &mut Vec<u128>,
        layers: &mut Vec<Vec<usize>>,
        layer_hashes: &mut Vec<Vec<u64>>,
        canon_map: &mut DetMap<Vec<Edge>, usize>,
        events: &mut u32,
        back_merges: &mut u32,
        branch_pairs: &mut u32,
    ) -> Outcome {
        let c0 = match canonicalize_budgeted(&init, b.max_canon_leaves) {
            Some(c) => c,
            None => {
                return Outcome::Exploded(ExplodeReason::CanonBudget);
            }
        };
        canon_map.insert(c0.form.edges.clone(), 0);
        layer_hashes.push(vec![form_hash(&c0.form.edges)]);
        states.push(init);
        state_step.push(0);
        path_counts.push(1);
        layers.push(vec![0]);

        let mut frontier = vec![0usize];
        for step in 1..=b.steps {
            let mut new_layer: Vec<usize> = Vec::new();
            let mut new_hashes: Vec<u64> = Vec::new();
            for &sid in &frontier {
                let ms = find_matches(&states[sid], rule);
                let mut children: Vec<usize> = Vec::new();
                for m in &ms {
                    *events += 1;
                    if *events as usize > b.max_events {
                        return Outcome::Exploded(ExplodeReason::Events);
                    }
                    let app = apply_full(&states[sid], rule, m);
                    if app.child.edges.len() > b.max_edges {
                        return Outcome::Exploded(ExplodeReason::EdgesPerState);
                    }
                    let c = match canonicalize_budgeted(&app.child, b.max_canon_leaves) {
                        Some(c) => c,
                        None => {
                            return Outcome::Exploded(ExplodeReason::CanonBudget);
                        }
                    };
                    let cid = match canon_map.get(&c.form.edges) {
                        Some(&cid) => {
                            if state_step[cid] < step {
                                *back_merges += 1;
                            }
                            cid
                        }
                        None => {
                            let cid = states.len();
                            if cid + 1 > b.max_states {
                                return Outcome::Exploded(ExplodeReason::States);
                            }
                            canon_map.insert(c.form.edges.clone(), cid);
                            new_hashes.push(form_hash(&c.form.edges));
                            states.push(app.child);
                            state_step.push(step);
                            path_counts.push(0);
                            new_layer.push(cid);
                            cid
                        }
                    };
                    // path-count DP in event order
                    let add = path_counts[sid];
                    path_counts[cid] = path_counts[cid].saturating_add(add);
                    if !children.contains(&cid) {
                        children.push(cid);
                    }
                }
                let k = children.len() as u32;
                *branch_pairs += k.saturating_sub(1) * k / 2;
            }
            layers.push(new_layer.clone());
            layer_hashes.push(new_hashes);
            if new_layer.is_empty() {
                return Outcome::Halted { step };
            }
            frontier = new_layer;
        }
        Outcome::Ran
    }

    let outcome = run_multiway(
        rule,
        init,
        b,
        &mut states,
        &mut state_step,
        &mut path_counts,
        &mut layers,
        &mut layer_hashes,
        &mut canon_map,
        &mut events,
        &mut back_merges,
        &mut branch_pairs,
    );

    // --- derived features ---
    let layer_sizes: Vec<u32> = layers.iter().map(|l| l.len() as u32).collect();
    let sharing_milli: Vec<u64> = if back_merges == 0 {
        layers
            .iter()
            .map(|l| {
                let paths: u128 = l.iter().map(|&i| path_counts[i]).sum();
                (paths * 1000)
                    .checked_div(l.len() as u128)
                    .unwrap_or(0)
                    .min(u64::MAX as u128) as u64
            })
            .collect()
    } else {
        Vec::new()
    };

    // --- bounded sequential run: period + final shape ---
    let (period, final_state) = sequential_run(rule, states.first().cloned(), b);
    let final_shape = classify_shape(final_state.as_ref());
    let degree_entropy_milli = degree_entropy(final_state.as_ref());

    let branchial_milli = {
        let last = layer_sizes
            .iter()
            .rev()
            .find(|&&s| s > 0)
            .copied()
            .unwrap_or(0) as u64;
        let denom = last * last.saturating_sub(1) / 2;
        if denom == 0 {
            0
        } else {
            // branch_pairs is cumulative; density is a coarse signal
            (branch_pairs as u64 * 1000) / denom.max(1)
        }
    };

    let growth = classify_growth(outcome, &layer_sizes, period);

    // fingerprint: per-layer sorted form hashes + outcome discriminant
    let mut fp_seq: Vec<u64> = Vec::new();
    for lh in &layer_hashes {
        let mut sorted = lh.clone();
        sorted.sort_unstable();
        fp_seq.push(0x1A4E5); // layer marker
        fp_seq.extend(sorted);
    }
    fp_seq.push(match outcome {
        Outcome::Halted { step } => 0x4A17 + step as u64,
        Outcome::Ran => 0x4A00,
        Outcome::Exploded(_) => 0x4AFF,
    });
    let fingerprint = hash_seq(&fp_seq);

    SeedRun {
        outcome,
        layers: layer_sizes,
        states: states.len() as u32,
        events,
        back_merges,
        branch_pairs,
        sharing_milli,
        growth,
        period,
        final_shape,
        degree_entropy_milli,
        branchial_milli,
        fingerprint,
    }
}

/// First-match sequential run with canonical-form period detection.
fn sequential_run(
    rule: &Rule,
    init: Option<State>,
    b: &ProbeBudget,
) -> (Option<(u32, u32)>, Option<State>) {
    let mut state = match init {
        Some(s) => s,
        None => return (None, None),
    };
    let mut seen: DetMap<Vec<Edge>, u32> = DetMap::default();
    if let Some(c) = canonicalize_budgeted(&state, b.max_canon_leaves) {
        seen.insert(c.form.edges, 0);
    } else {
        return (None, Some(state));
    }
    for t in 1..=b.run_events as u32 {
        let ms = find_matches(&state, rule);
        if ms.is_empty() {
            return (None, Some(state));
        }
        let app = apply_full(&state, rule, &ms[0]);
        if app.child.edges.len() > b.max_edges {
            return (None, Some(state));
        }
        state = app.child;
        let c = match canonicalize_budgeted(&state, b.max_canon_leaves) {
            Some(c) => c,
            None => return (None, Some(state)),
        };
        if let Some(&first) = seen.get(&c.form.edges) {
            return (Some((first, t - first)), Some(state));
        }
        seen.insert(c.form.edges, t);
    }
    (None, Some(state))
}

fn classify_growth(outcome: Outcome, layers: &[u32], period: Option<(u32, u32)>) -> GrowthClass {
    if let Outcome::Exploded(_) = outcome {
        return GrowthClass::Exploded;
    }
    // Periodic BEFORE Halted: a cycler's multiway frontier empties
    // because every child was already seen — that is recurrence, not
    // death (found red by the reversal-rule pin).
    if let Some((mu, lambda)) = period {
        if lambda >= 1 {
            return GrowthClass::Periodic { mu, lambda };
        }
    }
    if matches!(outcome, Outcome::Halted { .. }) {
        return GrowthClass::Dies;
    }
    if layers.len() < 3 {
        return GrowthClass::Static;
    }
    if layers.windows(2).all(|w| w[0] == w[1]) {
        return GrowthClass::Static;
    }
    let d1: Vec<i64> = layers
        .windows(2)
        .map(|w| w[1] as i64 - w[0] as i64)
        .collect();
    if d1.windows(2).all(|w| w[0] == w[1]) && d1[0] > 0 {
        return GrowthClass::Linear;
    }
    // sustained >= 1.5x ratio over the last two transitions => Exp
    let n = layers.len();
    let exp_tail = (n - 2..n)
        .filter(|&i| i >= 1)
        .all(|i| layers[i] as u64 * 2 >= layers[i - 1] as u64 * 3 && layers[i - 1] > 0);
    if exp_tail {
        return GrowthClass::Exp;
    }
    GrowthClass::Poly
}

fn classify_shape(state: Option<&State>) -> FinalShape {
    let s = match state {
        Some(s) => s,
        None => return FinalShape::Other,
    };
    if s.edges.is_empty() {
        return FinalShape::Empty;
    }
    if s.edges.iter().all(|e| e.windows(2).all(|w| w[0] == w[1])) {
        return FinalShape::SelfLoops;
    }
    let vs = s.vertices();
    let nv = vs.len();
    if s.edges.len() > 2 * nv {
        return FinalShape::Dense;
    }
    // hub check: one vertex in every edge
    if let Some(&hub) = vs.iter().find(|&&v| s.edges.iter().all(|e| e.contains(&v))) {
        let _ = hub;
        if s.edges.len() >= 3 {
            return FinalShape::Star;
        }
    }
    if !s.edges.iter().all(|e| e.len() == 2) {
        return FinalShape::Other;
    }
    // binary graph: total degree profile
    let mut deg = vec![0usize; nv];
    for e in &s.edges {
        for v in e {
            deg[vs.binary_search(v).unwrap()] += 1;
        }
    }
    let ones = deg.iter().filter(|&&d| d == 1).count();
    let twos = deg.iter().filter(|&&d| d == 2).count();
    if ones == 2 && twos == nv - 2 && s.edges.len() == nv - 1 {
        return FinalShape::Path;
    }
    if twos == nv && s.edges.len() == nv {
        return FinalShape::Cycle;
    }
    if s.edges.len() == nv.saturating_sub(1) {
        return FinalShape::Tree;
    }
    FinalShape::Other
}

fn degree_entropy(state: Option<&State>) -> u64 {
    let s = match state {
        Some(s) => s,
        None => return 0,
    };
    let vs = s.vertices();
    let mut deg = vec![0u128; vs.len()];
    for e in &s.edges {
        for v in e {
            deg[vs.binary_search(v).unwrap()] += 1;
        }
    }
    deg.iter()
        .map(|&d| d as u64 * log2_milli(d).min(1 << 20))
        .sum()
}
