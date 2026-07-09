//! Property tests for the canonicalization layer, plus the matcher and
//! causal oracles.
//!
//! The single most load-bearing property in the whole engine lives here:
//! `wl_hash` must be isomorphism-INVARIANT. The exact `isomorphic()` check
//! only ever guards one direction (a hash collision is confirmed before
//! merging), so if isomorphic states could hash differently they would land
//! in different buckets, never be compared, and silently fail to merge —
//! inflated state counts with nothing to catch it. These tests are the
//! something.

mod common;

use common::gen::{
    gen_renaming, gen_rule_text, gen_state, rename, shuffle_edges, RuleCfg, StateCfg,
};
use common::harness::prop;
use common::oracle::{iso_bruteforce, match_bruteforce};
use multiway::canon::{isomorphic, wl_hash};
use multiway::matcher::find_matches;
use multiway::rule::{parse_rule, parse_state};

const SEED: u64 = 0x00C0_FFEE_0000_0001;

/// THE critical missing test from the original suite: relabel a random
/// state (both permutation and fresh-label modes), shuffle its edge order,
/// and the hash must not move.
#[test]
fn prop_wl_hash_invariant_under_relabeling() {
    prop(SEED, "prop_wl_hash_invariant_under_relabeling", |rng, _| {
        let s = gen_state(rng, &StateCfg::wide());
        for fresh in [false, true] {
            let map = gen_renaming(rng, &s, fresh);
            let mut b = rename(&s, &map);
            shuffle_edges(rng, &mut b);
            assert_eq!(
                wl_hash(&s),
                wl_hash(&b),
                "wl_hash not invariant: {:?} vs {:?} (map {:?}, fresh={})",
                s.edges,
                b.edges,
                map,
                fresh
            );
        }
    });
}

/// The exact backtracking check must accept every relabeling.
#[test]
fn prop_isomorphic_true_on_relabelings() {
    prop(SEED ^ 1, "prop_isomorphic_true_on_relabelings", |rng, _| {
        let s = gen_state(rng, &StateCfg::oracle());
        let fresh = rng.chance(1, 2);
        let map = gen_renaming(rng, &s, fresh);
        let mut b = rename(&s, &map);
        shuffle_edges(rng, &mut b);
        assert!(
            isomorphic(&s, &b),
            "isomorphic rejected a relabeling: {:?} vs {:?}",
            s.edges,
            b.edges
        );
        assert!(
            iso_bruteforce(&s, &b),
            "oracle rejected a relabeling (oracle bug): {:?} vs {:?}",
            s.edges,
            b.edges
        );
    });
}

/// Three-way differential over the agreement lattice:
/// `isomorphic` == brute force; brute-force-iso implies equal `wl_hash`
/// (soundness — a missed merge is impossible); WL collisions on
/// non-isomorphic pairs are permitted and tallied, never failed.
///
/// Pair generator: 50% relabelings (the iso=true branch, which independent
/// random pairs almost never hit), 25% independent states, 25% mutated
/// near-isomorphs (relabel then perturb one edge — the pairs most likely to
/// expose a canonizer that drops multiplicity or edge order).
#[test]
fn prop_iso_differential_lattice() {
    let mut wl_collisions = 0usize;
    prop(SEED ^ 2, "prop_iso_differential_lattice", |rng, _| {
        let a = gen_state(rng, &StateCfg::oracle());
        let b = match rng.below(4) {
            0 | 1 => {
                let fresh = rng.chance(1, 2);
                let map = gen_renaming(rng, &a, fresh);
                let mut b = rename(&a, &map);
                shuffle_edges(rng, &mut b);
                b
            }
            2 => gen_state(rng, &StateCfg::oracle()),
            _ => {
                // near-iso: relabel, then perturb one edge
                let map = gen_renaming(rng, &a, false);
                let mut b = rename(&a, &map);
                if !b.edges.is_empty() {
                    let ei = rng.range_usize(0, b.edges.len() - 1);
                    let e = &mut b.edges[ei];
                    let pos = rng.range_usize(0, e.len() - 1);
                    e[pos] = rng.below(6) as u32;
                }
                shuffle_edges(rng, &mut b);
                multiway::hypergraph::State::new(b.edges)
            }
        };

        let bf = iso_bruteforce(&a, &b);
        assert_eq!(
            isomorphic(&a, &b),
            bf,
            "isomorphic() disagrees with brute force: {:?} vs {:?}",
            a.edges,
            b.edges
        );
        if bf {
            assert_eq!(
                wl_hash(&a),
                wl_hash(&b),
                "wl_hash UNSOUND (missed merge possible): {:?} vs {:?}",
                a.edges,
                b.edges
            );
        } else if wl_hash(&a) == wl_hash(&b) {
            wl_collisions += 1; // permitted: WL is invariant, not complete
        }
    });
    println!("wl collisions on non-iso pairs: {}", wl_collisions);
}

/// The backtracking matcher against the enumerate-everything oracle:
/// exact Vec equality, including order (downstream causal/event code
/// depends on the enumeration order).
#[test]
fn prop_matcher_agrees_with_bruteforce_oracle() {
    let small = StateCfg {
        max_vertices: 5,
        max_edges: 5,
        max_arity: 3,
        dup_pct: 25,
        self_loop_pct: 25,
        sparse_labels: false,
    };
    prop(
        SEED ^ 3,
        "prop_matcher_agrees_with_bruteforce_oracle",
        |rng, _| {
            let s = gen_state(rng, &small);
            let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
            let got: Vec<(Vec<usize>, Vec<Option<u32>>)> = find_matches(&s, &rule)
                .into_iter()
                .map(|m| (m.edge_idx, m.binding))
                .collect();
            let want = match_bruteforce(&s, &rule);
            assert_eq!(
                got, want,
                "matcher disagrees with oracle on {:?} with rule {}",
                s.edges, rule.text
            );
        },
    );
}

/// Exact hand-computed causal dependency list for the growth rule.
///
/// Derivation (first match in deterministic order is always edge 0; each
/// event consumes its edge and appends `{x,y},{y,z}` with a fresh z):
///   e1 consumes {0,0}   (creator 0) -> (0,1); state [{0,0},{0,1}]
///   e2 consumes {0,0}   (creator 1) -> (1,2); state [{0,1},{0,0},{0,2}]
///   e3 consumes {0,1}   (creator 1) -> (1,3); state [{0,0},{0,2},{0,1},{1,3}]
///   e4 consumes {0,0}   (creator 2) -> (2,4)
///   e5 consumes {0,2}   (creator 2) -> (2,5)
#[test]
fn causal_deps_hand_verified() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let c = multiway::causal::run(&rule, init, 5);
    assert_eq!(c.n_events, 6);
    assert_eq!(c.deps, vec![(0, 1), (1, 2), (1, 3), (2, 4), (2, 5)]);
}

// ---------------------------------------------------------------------------
// Fuel-capped isomorphism (M1)

use common::harness::shrink_state;
use multiway::canon::{isomorphic_with_budget, IsoResult};

/// Budget semantics pins: pre-checks run BEFORE fuel accounting, so
/// budget=0 on a pre-check-rejected pair is NotIso, while budget=0 on a
/// pair that reaches the backtracker is BudgetExhausted — including (a, a).
#[test]
fn iso_budget_trivial_cases() {
    let a = parse_state("{{0,1},{1,2}}").unwrap();
    let star = parse_state("{{0,1},{0,2}}").unwrap();
    let bigger = parse_state("{{0,1},{1,2},{2,3}}").unwrap();

    assert_eq!(isomorphic_with_budget(&a, &a, u64::MAX), IsoResult::Iso);
    assert_eq!(
        isomorphic_with_budget(&a, &star, u64::MAX),
        IsoResult::NotIso
    );
    // pre-check rejection (edge counts differ): NotIso even with zero fuel
    assert_eq!(isomorphic_with_budget(&a, &bigger, 0), IsoResult::NotIso);
    // reaches the backtracker with zero fuel: exhausted, even on (a, a)
    assert_eq!(
        isomorphic_with_budget(&a, &a, 0),
        IsoResult::BudgetExhausted
    );
}

/// Unlimited budget must agree with both the boolean API and brute force.
#[test]
fn prop_iso_budget_unlimited_agrees() {
    prop(SEED ^ 4, "prop_iso_budget_unlimited_agrees", |rng, _| {
        let a = gen_state(rng, &StateCfg::oracle());
        let b = if rng.chance(1, 2) {
            let map = gen_renaming(rng, &a, false);
            rename(&a, &map)
        } else {
            gen_state(rng, &StateCfg::oracle())
        };
        let bf = iso_bruteforce(&a, &b);
        let expect = if bf {
            IsoResult::Iso
        } else {
            IsoResult::NotIso
        };
        assert_eq!(isomorphic_with_budget(&a, &b, u64::MAX), expect);
        assert_eq!(isomorphic(&a, &b), bf);
    });
}

/// Build the Θ(m!) witness: an out-star with m spokes plus a 2-edge tail
/// hanging off spoke 1; the pair differs only in the tail's final edge
/// direction, so the backtracker churns through spoke pairings before
/// every branch dies at the tail.
fn star_pair(m: u32) -> (multiway::hypergraph::State, multiway::hypergraph::State) {
    use multiway::hypergraph::State;
    let mut ea: Vec<Vec<u32>> = (1..=m).map(|i| vec![0, i]).collect();
    let mut eb = ea.clone();
    ea.push(vec![1, m + 1]);
    ea.push(vec![m + 1, m + 2]);
    eb.push(vec![1, m + 1]);
    eb.push(vec![m + 2, m + 1]); // reversed tail edge
    (State::new(ea), State::new(eb))
}

/// The blowup is real (documented in code, not folklore): at m=10 a
/// 10_000-node budget exhausts; the m=4 shrunken variant fits the
/// brute-force oracle and is genuinely non-isomorphic.
#[test]
fn iso_budget_exhausts_on_pathological_stars() {
    let (a10, b10) = star_pair(10);
    assert_eq!(
        isomorphic_with_budget(&a10, &b10, 10_000),
        IsoResult::BudgetExhausted
    );

    let (a4, b4) = star_pair(4); // 7 vertices: oracle-checkable
    assert!(!iso_bruteforce(&a4, &b4));
    assert_eq!(
        isomorphic_with_budget(&a4, &b4, u64::MAX),
        IsoResult::NotIso
    );

    // shrink_state smoke: dropping any edge from the m=4 witness makes the
    // pair distinguishable cheaply or changes edge counts — the shrinker
    // must terminate and return a state that still "fails" its predicate.
    let shrunk = shrink_state(a4.clone(), |s| s.edge_count() >= 2);
    assert_eq!(shrunk.edge_count(), 2);
}
