//! Incremental match maintenance: a child's match set derives from its
//! parent's — survivors (disjoint from the consumed edges, remapped) plus
//! new matches seeded through the produced edges — and must equal the
//! full search EXACTLY, including enumeration order (event ids, causal
//! first-match, and branchial structure all depend on it).

mod common;

use common::gen::{gen_rule_text, gen_state, RuleCfg, StateCfg};
use common::harness::prop;
use multiway::export::bundle_json;
use multiway::matcher::{apply_full, delta_matches, find_matches, Match};
use multiway::rule::{parse_rule, parse_state};
use multiway::system::{evolve, evolve_opts, EvolveOpts};

const SEED: u64 = 0x00C0_FFEE_0000_0006;

fn as_pairs(ms: &[Match]) -> Vec<(Vec<usize>, Vec<Option<u32>>)> {
    ms.iter()
        .map(|m| (m.edge_idx.clone(), m.binding.clone()))
        .collect()
}

/// Three manual growth steps: after each event, the delta-maintained set
/// equals the full search — exact Vec equality including order.
#[test]
fn delta_equals_full_hand_case() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let mut state = parse_state("{{0,0}}").unwrap();
    let mut ms = find_matches(&state, &rule);
    for _ in 0..3 {
        let m = ms[0].clone();
        let app = apply_full(&state, &rule, &m);
        let dm = delta_matches(&rule, &ms, &m, &app);
        assert_eq!(as_pairs(&dm), as_pairs(&find_matches(&app.child, &rule)));
        state = app.child;
        ms = dm;
    }
}

/// The double-count trap: when a child match uses TWO produced edges,
/// naive per-(position, produced-edge) seeding without the
/// positions-before-the-seed-are-kept-only restriction generates it once
/// per produced edge it touches. Classic rule from the double self-loop:
/// the child is ALL produced edges, so every 2-edge match crosses two of
/// them.
#[test]
fn delta_multi_produced_no_duplicates() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let state = parse_state("{{0,0},{0,0}}").unwrap();
    let ms = find_matches(&state, &rule);
    let m = ms[0].clone();
    let app = apply_full(&state, &rule, &m);
    assert_eq!(
        app.kept.len(),
        0,
        "everything consumed: child is all-produced"
    );

    let dm = delta_matches(&rule, &ms, &m, &app);
    let full = find_matches(&app.child, &rule);
    assert_eq!(as_pairs(&dm), as_pairs(&full));

    // explicitly: no duplicate edge_idx vectors
    let mut idxs: Vec<Vec<usize>> = dm.iter().map(|m| m.edge_idx.clone()).collect();
    let before = idxs.len();
    idxs.sort();
    idxs.dedup();
    assert_eq!(idxs.len(), before, "duplicate matches generated");
}

/// Fuzz: random (state, rule, match) — delta == full, always.
#[test]
fn prop_delta_equals_full_fuzz() {
    prop(SEED, "prop_delta_equals_full_fuzz", |rng, _| {
        let s = gen_state(rng, &StateCfg::oracle());
        let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
        let ms = find_matches(&s, &rule);
        if ms.is_empty() {
            return;
        }
        let pick = rng.range_usize(0, ms.len() - 1);
        let m = ms[pick].clone();
        let app = apply_full(&s, &rule, &m);
        let dm = delta_matches(&rule, &ms, &m, &app);
        assert_eq!(
            as_pairs(&dm),
            as_pairs(&find_matches(&app.child, &rule)),
            "delta != full on {:?} rule {} match {:?}",
            s.edges,
            rule.text,
            m.edge_idx
        );
    });
}

/// The integration gate: incremental evolve must serialize byte-identically
/// to the full-search reference path.
#[test]
fn evolve_incremental_bit_identical() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();

    let inc = evolve_opts(
        &rule,
        init.clone(),
        &EvolveOpts {
            steps: 4,
            threads: 1,
            incremental: true,
        },
    );
    let full = evolve_opts(
        &rule,
        init.clone(),
        &EvolveOpts {
            steps: 4,
            threads: 1,
            incremental: false,
        },
    );
    assert_eq!(
        bundle_json(&rule.text, "{{0,0},{0,0}}", &inc, None),
        bundle_json(&rule.text, "{{0,0},{0,0}}", &full, None)
    );
}

/// Laziness telemetry: one full search (the initial state), one delta per
/// NEW canonical state — merged children never get match sets, and (A2)
/// neither do final-layer states, whose match sets could never be used:
/// the frontier is dead after the last step. Strictly lazier than v0.2.
#[test]
fn single_full_search_telemetry() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 4);
    assert_eq!(mw.stats.full_match_calls, 1);
    // 179 states, minus the initial one, minus the 156 final-layer states
    // whose match sets would be computed only to be dropped: 22 deltas.
    let last_layer = mw.layers.last().unwrap().len();
    assert_eq!(mw.stats.delta_match_calls, mw.states.len() - 1 - last_layer);
    assert_eq!(mw.states.len() - 1 - last_layer, 22); // explicit, for readers
}

/// causal::run switched to the delta path — its deps must not move a byte.
#[test]
fn causal_run_unchanged() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let c = multiway::causal::run(&rule, init, 5);
    assert_eq!(c.n_events, 6);
    assert_eq!(c.deps, vec![(0, 1), (1, 2), (1, 3), (2, 4), (2, 5)]);
    assert_eq!(c.final_state.edge_count(), 6);
}

/// Baseline re-pin after the frontier restructuring.
#[test]
fn baseline_still_green() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 4);
    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 3, 18, 156]);
    assert_eq!(mw.back_merges, 0);
}
