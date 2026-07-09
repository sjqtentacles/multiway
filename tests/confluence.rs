//! Causal-invariance / confluence checker: critical pairs + bounded
//! strong joinability.
//!
//! Honesty contract (load-bearing, do not weaken): the top verdict is
//! `AllCriticalPairsStronglyJoinable` — evidence, not a proof of local
//! confluence (the critical-pair lemma for this formalism is a THEORY.md
//! roadmap item). `confluent: true` only with the termination lint.
//! `NotConfluent` only on double saturation. Bound hits are never
//! counterexamples.

mod common;

use common::gen::{gen_state, StateCfg};
use common::harness::prop;
use multiway::canon::canonical_form;
use multiway::confluence::{check, critical_pairs, CheckCfg, InconclusiveReason, Verdict};
use multiway::matcher::{apply, find_matches, Match};
use multiway::rule::{parse_rule, parse_state};

const SEED: u64 = 0x00C0_FFEE_0000_0005;

const CLASSIC: &str = "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}";

/// Classic rule vs itself: exactly 5 nontrivial critical pairs.
///
/// Hand derivation (arity-matched partial injections between two 2-edge
/// LHSs): 6 nonempty maps — 4 single-edge identifications {(0,0)},
/// {(0,1)}, {(1,0)}, {(1,1)}, plus 2 total maps. The identity-total map
/// {(0,0),(1,1)} unifies x~x', y~y', z~z', inducing m1 == m2 — the
/// trivial diagonal, skipped. The crossed-total {(0,1),(1,0)} gives
/// m1.edge_idx=[0,1] vs m2.edge_idx=[1,0] with different bindings —
/// a real divergence. Total: 5.
#[test]
fn critical_pair_count_classic_self_overlap() {
    let rule = parse_rule(CLASSIC).unwrap();
    let pairs = critical_pairs(&[rule]).unwrap();
    assert_eq!(pairs.len(), 5);
}

/// Host construction for σ = {(0,0)} on the classic rule: unifying
/// lhs1[0] = {x,y} with lhs2[0] = {x',y'} gives classes {x,x'}, {y,y'},
/// {z}, {z'} — host {{0,1},{0,2},{0,3}} in discovery order, with
/// m1 = [0,1] and m2 = [0,2], both real matches of the host.
#[test]
fn critical_pair_host_shape_hand_verified() {
    let rule = parse_rule(CLASSIC).unwrap();
    let pairs = critical_pairs(std::slice::from_ref(&rule)).unwrap();

    let p = pairs
        .iter()
        .find(|p| {
            p.host.edges.len() == 3 && p.m1.edge_idx == vec![0, 1] && p.m2.edge_idx == vec![0, 2]
        })
        .expect("σ={(0,0)} pair not found");

    assert_eq!(
        canonical_form(&p.host),
        canonical_form(&parse_state("{{0,1},{0,2},{0,3}}").unwrap())
    );

    // both reconstructed matches must be REAL matches of the host
    let real: Vec<(Vec<usize>, Vec<Option<u32>>)> = find_matches(&p.host, &rule)
        .into_iter()
        .map(|m| (m.edge_idx, m.binding))
        .collect();
    assert!(real.contains(&(p.m1.edge_idx.clone(), p.m1.binding.clone())));
    assert!(real.contains(&(p.m2.edge_idx.clone(), p.m2.binding.clone())));
}

/// The parallel-independence premise of the whole method: matches
/// consuming disjoint edge instances commute — applying them in either
/// order reaches the same canonical state. (apply never renames vertices
/// and bindings derive only from consumed edges, so each match survives
/// the other; fresh-vertex minting differs by order, hence comparison up
/// to isomorphism.)
#[test]
fn prop_disjoint_matches_commute_diamond() {
    let growth = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let classic = parse_rule(CLASSIC).unwrap();
    prop(SEED, "prop_disjoint_matches_commute_diamond", |rng, i| {
        let rule = if i % 2 == 0 { &growth } else { &classic };
        let s = gen_state(rng, &StateCfg::oracle());
        let ms = find_matches(&s, rule);

        // find a disjoint pair
        let mut found: Option<(&Match, &Match)> = None;
        'outer: for a in 0..ms.len() {
            for b in (a + 1)..ms.len() {
                if ms[a].edge_idx.iter().all(|i| !ms[b].edge_idx.contains(i)) {
                    found = Some((&ms[a], &ms[b]));
                    break 'outer;
                }
            }
        }
        let (m1, m2) = match found {
            Some(p) => p,
            None => return, // no disjoint pair in this state; skip
        };

        // order 1: m1 then m2 (remap m2 through the survivor map)
        let app1 = multiway::matcher::apply_full(&s, rule, m1);
        let remap = |m: &Match, kept: &[(usize, usize)]| Match {
            edge_idx: m
                .edge_idx
                .iter()
                .map(|&i| kept.iter().find(|&&(p, _)| p == i).unwrap().1)
                .collect(),
            binding: m.binding.clone(),
        };
        let d1 = apply(&app1.child, rule, &remap(m2, &app1.kept));

        // order 2: m2 then m1
        let app2 = multiway::matcher::apply_full(&s, rule, m2);
        let d2 = apply(&app2.child, rule, &remap(m1, &app2.kept));

        assert_eq!(
            canonical_form(&d1),
            canonical_form(&d2),
            "disjoint matches failed to commute on {:?}",
            s.edges
        );
    });
}

/// Growth rule: the only self-overlap is the trivial diagonal, so 0
/// nontrivial pairs — vacuously all-strongly-joinable, but NOT confluent
/// (edge delta +1: termination unproven, Newman unavailable).
#[test]
fn growth_rule_verdict() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let report = check(&[rule], &CheckCfg::default()).unwrap();
    match report.verdict {
        Verdict::AllCriticalPairsStronglyJoinable {
            pairs_checked,
            confluent,
            ..
        } => {
            assert_eq!(pairs_checked, 0);
            assert!(!confluent, "edge-growing rule must not claim confluence");
        }
        v => panic!("expected AllCriticalPairsStronglyJoinable, got {:?}", v),
    }
}

/// A genuine counterexample: {{x,y}}->{{x}} vs {{x,y}}->{} diverge on
/// host {{0,1}} to {{0}} vs {} — both normal forms, non-isomorphic,
/// reached with both sides exhaustively saturated. This is the only path
/// to NotConfluent.
#[test]
fn genuine_counterexample_detected() {
    let r1 = parse_rule("{{x,y}}->{{x}}").unwrap();
    let r2 = parse_rule("{{x,y}}->{}").unwrap();
    let report = check(&[r1, r2], &CheckCfg::default()).unwrap();
    match report.verdict {
        Verdict::NotConfluent { s1, s2, .. } => {
            let forms = [canonical_form(&s1).edges, canonical_form(&s2).edges];
            assert!(forms.contains(&vec![vec![0u32]]), "one side is {{0}}");
            assert!(forms.contains(&Vec::<Vec<u32>>::new()), "one side is {{}}");
        }
        v => panic!("expected NotConfluent, got {:?}", v),
    }
}

/// Plump's trap, pinned: {{x,y}}->{{x}} vs {{x,y}}->{{y}} joins PLAINLY
/// ({{0}} ≅ {{1}}) but not STRONGLY — in context {{a,b},{a,c}} the
/// results {{a},{a,c}} vs {{b},{a,c}} genuinely diverge, so plain
/// joinability licenses no local-confluence claim. The colored keys keep
/// the pinned host vertices distinct, and the verdict must be
/// Inconclusive{WeakOnly}, never AllCriticalPairsStronglyJoinable.
#[test]
fn weak_joinability_is_not_a_proof() {
    let r1 = parse_rule("{{x,y}}->{{x}}").unwrap();
    let r2 = parse_rule("{{x,y}}->{{y}}").unwrap();
    let report = check(&[r1, r2], &CheckCfg::default()).unwrap();
    match report.verdict {
        Verdict::Inconclusive { reason, .. } => {
            assert!(matches!(reason, InconclusiveReason::WeakOnly));
        }
        v => panic!("expected Inconclusive(WeakOnly), got {:?}", v),
    }
}

/// The one honest path to `confluent: true`: every critical pair strongly
/// joinable AND every rule strictly edge-decreasing (termination by a
/// well-founded measure; Newman's lemma). Idempotent parallel-edge merge
/// is the canonical example — its overlaps join immediately.
#[test]
fn termination_lint_upgrades_claim() {
    let rule = parse_rule("{{x,y},{x,y}}->{{x,y}}").unwrap();
    let report = check(&[rule], &CheckCfg::default()).unwrap();
    match report.verdict {
        Verdict::AllCriticalPairsStronglyJoinable { confluent, .. } => {
            assert!(
                confluent,
                "terminating + all-strong must upgrade to confluent"
            );
        }
        v => panic!("expected AllCriticalPairsStronglyJoinable, got {:?}", v),
    }
}

/// The honesty test: a bound hit is INCONCLUSIVE, never a counterexample.
/// The classic rule's branches grow forever, so a depth-1 budget cannot
/// conclude anything.
#[test]
fn bound_hit_reports_inconclusive() {
    let rule = parse_rule(CLASSIC).unwrap();
    let cfg = CheckCfg {
        join_depth: 1,
        max_states: 50,
        ..CheckCfg::default()
    };
    let report = check(&[rule], &cfg).unwrap();
    match report.verdict {
        Verdict::Inconclusive { reason, .. } => {
            assert!(matches!(reason, InconclusiveReason::BoundHit { .. }));
        }
        Verdict::NotConfluent { .. } => {
            panic!("bound hit must NEVER be reported as a counterexample")
        }
        v => panic!("expected Inconclusive(BoundHit), got {:?}", v),
    }
}

/// The oracle the critique demanded for claim (b): for a rule the checker
/// certifies AllCriticalPairsStronglyJoinable + confluent, arbitrary
/// overlapping divergences in arbitrary random hosts must reconverge
/// within a small bound. Exercises the checker's headline claim outside
/// its own critical-pair hosts.
#[test]
fn prop_certified_rule_divergences_join() {
    let rule = parse_rule("{{x,y},{x,y}}->{{x,y}}").unwrap();
    // precondition: the checker certifies this rule (test above)
    let dup_heavy = StateCfg {
        max_vertices: 4,
        max_edges: 6,
        max_arity: 2,
        dup_pct: 60,
        self_loop_pct: 20,
        sparse_labels: false,
    };
    prop(
        SEED ^ 1,
        "prop_certified_rule_divergences_join",
        |rng, _| {
            let s = gen_state(rng, &dup_heavy);
            let ms = find_matches(&s, &rule);
            for a in 0..ms.len() {
                for b in (a + 1)..ms.len() {
                    if ms[a].edge_idx.iter().any(|i| ms[b].edge_idx.contains(i)) {
                        let s1 = apply(&s, &rule, &ms[a]);
                        let s2 = apply(&s, &rule, &ms[b]);
                        assert!(
                            multiway::confluence::plainly_joinable(
                                std::slice::from_ref(&rule),
                                &s1,
                                &s2,
                                6,
                                500
                            ),
                            "certified rule left an unjoinable divergence on {:?}",
                            s.edges
                        );
                    }
                }
            }
        },
    );
}
