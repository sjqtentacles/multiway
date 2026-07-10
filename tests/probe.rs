//! The behavior probe: budget safety (THE scan-safety property),
//! differential honesty against the real engine, and classification
//! pins on the gallery rules.

mod common;

use common::gen::{gen_rule_text, RuleCfg};
use common::harness::prop;
use multiway::probe::{probe, seeds_for, ExplodeReason, GrowthClass, Outcome, ProbeBudget};
use multiway::rule::parse_rule;
use multiway::rulespace::CanonRule;
use multiway::system::evolve;

const SEED: u64 = 0x00C0_FFEE_0000_0009;

fn tiny_budget() -> ProbeBudget {
    ProbeBudget {
        steps: 3,
        max_states: 60,
        max_events: 2_000,
        max_edges: 24,
        max_canon_leaves: 2_000,
        run_events: 24,
    }
}

/// The terminating composition rule halts on the path seed (a genuine
/// `Halted`, not a budget hit).
#[test]
fn probe_terminating_rule_halts() {
    let rule = parse_rule("{{x,y},{y,z}}->{{x,z}}").unwrap();
    let r = probe(&rule, &ProbeBudget::default());
    // path seed = {{0,1},{1,2}} — composes down to a single edge
    let path = &r.seeds[1];
    assert!(
        matches!(path.outcome, Outcome::Halted { .. }),
        "expected Halted, got {:?}",
        path.outcome
    );
    assert_eq!(path.growth, GrowthClass::Dies);
}

/// Subdivision shows maximal sharing on the loop seed: every layer is
/// one canonical state while naive paths grow factorially.
#[test]
fn probe_subdivision_max_sharing() {
    let rule = parse_rule("{{x,y}}->{{x,z},{z,y}}").unwrap();
    let r = probe(&rule, &ProbeBudget::default());
    let loop_seed = &r.seeds[0];
    assert_eq!(loop_seed.outcome, Outcome::Ran);
    assert!(
        loop_seed.layers.iter().all(|&l| l <= 1),
        "subdivision layers should stay canonical-1: {:?}",
        loop_seed.layers
    );
    // sharing at the last layer: 5 steps absorb 5! = 120 naive paths
    assert_eq!(loop_seed.sharing_milli.last(), Some(&120_000));
}

/// Reversal cycles. The probe's path seed for a 1-edge LHS is a single
/// directed edge, and `{{0,1}}` reversed is `{{1,0}}` — isomorphic to
/// itself — so the honest period there is 1 (the original test
/// expectation of 2 assumed the two-edge gallery init; the probe's
/// seeds are arity-derived, not the gallery's). Period 2 appears on the
/// two-edge chain, asserted separately.
#[test]
fn probe_reversal_periodic() {
    let rule = parse_rule("{{x,y}}->{{y,x}}").unwrap();
    let r = probe(&rule, &ProbeBudget::default());
    let path = &r.seeds[1];
    assert_eq!(
        path.growth,
        GrowthClass::Periodic { mu: 0, lambda: 1 },
        "single directed edge is reversal-symmetric up to iso (got {:?})",
        path.growth
    );
    assert!(path.back_merges > 0);

    // the classic 2-chain shows the true period-2 cycle (checked via a
    // direct sequential trace: forms repeat at t=2)
    use multiway::canon::canonical_form;
    use multiway::matcher::{apply_full, find_matches};
    let mut state = multiway::rule::parse_state("{{0,1},{1,2}}").unwrap();
    let f0 = canonical_form(&state).edges;
    for _ in 0..2 {
        let ms = find_matches(&state, &rule);
        state = apply_full(&state, &rule, &ms[0]).child;
    }
    assert_eq!(canonical_form(&state).edges, f0, "2-chain period is 2");
}

/// Explosions are BOUNDED and still return a result: the classic rule
/// under a tiny state budget reports Exploded(States), never hangs.
#[test]
fn probe_explosion_bounded() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let b = ProbeBudget {
        max_states: 30,
        ..ProbeBudget::default()
    };
    let r = probe(&rule, &b);
    // loop seed is the flagship {{0,0},{0,0}} evolution: 179 states at
    // depth 4 >> 30
    assert_eq!(r.seeds[0].outcome, Outcome::Exploded(ExplodeReason::States));
    assert_eq!(r.seeds[0].growth, GrowthClass::Exploded);
}

/// THE safety property: whatever the rule, every budget is respected —
/// states, events, per-state edges all within bounds, and the probe
/// always returns.
#[test]
fn prop_probe_respects_budgets() {
    let b = tiny_budget();
    prop(SEED, "prop_probe_respects_budgets", |rng, _| {
        let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
        let r = probe(&rule, &b);
        for seed in &r.seeds {
            assert!(
                (seed.states as usize) <= b.max_states,
                "state budget breached: {}",
                seed.states
            );
            assert!(
                (seed.events as usize) <= b.max_events + 1,
                "event budget breached: {}",
                seed.events
            );
        }
    });
}

/// Differential honesty: when the probe does NOT explode, its layer
/// sizes must equal the real engine's for the same (rule, seed, steps).
#[test]
fn prop_probe_matches_evolve() {
    let b = tiny_budget();
    let mut checked = 0usize;
    prop(SEED ^ 1, "prop_probe_matches_evolve", |rng, _| {
        let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
        let r = probe(&rule, &b);
        for (seed_state, seed_run) in seeds_for(&rule).into_iter().zip(&r.seeds) {
            if matches!(seed_run.outcome, Outcome::Exploded(_)) {
                continue;
            }
            let mw = evolve(&rule, seed_state, b.steps);
            let engine_layers: Vec<u32> = mw.layers.iter().map(|l| l.len() as u32).collect();
            assert_eq!(
                seed_run.layers, engine_layers,
                "probe disagrees with evolve on rule {}",
                rule.text
            );
            assert_eq!(seed_run.back_merges as usize, mw.back_merges);
            checked += 1;
        }
    });
    assert!(checked > 0, "generator drift: differential never ran");
}

/// Fingerprints quotient correctly: equivalent rules (renamed vars,
/// shuffled sides) probe to identical fingerprints.
#[test]
fn prop_fingerprint_invariant_under_rule_equivalence() {
    let b = tiny_budget();
    prop(
        SEED ^ 2,
        "prop_fingerprint_invariant_under_rule_equivalence",
        |rng, _| {
            let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
            // canonical representative probes identically to the raw rule
            let canon_rule = CanonRule::from_rule(&rule).to_rule();
            let a = probe(&rule, &b);
            let c = probe(&canon_rule, &b);
            assert_eq!(
                a.fingerprint, c.fingerprint,
                "fingerprint not invariant for {} vs {}",
                rule.text, canon_rule.text
            );
        },
    );
}

/// Same (rule, budget) twice ⇒ identical results (spot: fingerprint +
/// layers + growth).
#[test]
fn probe_deterministic() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let a = probe(&rule, &ProbeBudget::default());
    let b = probe(&rule, &ProbeBudget::default());
    assert_eq!(a.fingerprint, b.fingerprint);
    for (x, y) in a.seeds.iter().zip(&b.seeds) {
        assert_eq!(x.layers, y.layers);
        assert_eq!(x.growth, y.growth);
        assert_eq!(x.fingerprint, y.fingerprint);
    }
}
