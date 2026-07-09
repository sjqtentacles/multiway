//! Locks every number and claim the README rule gallery makes —
//! structural assertions first (falsifiable a priori), measured values
//! second. If a row's story is wrong, this file goes red, not the README.

mod common;

use multiway::confluence::{check, CheckCfg, Verdict};
use multiway::matcher::find_matches;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;

/// Row: `{{x,y},{y,z}}->{{x,z}}` on the 4-cycle — genuinely terminating.
/// Structural: the final state has NO matches (an empty final layer alone
/// would not prove termination — children can all merge away).
#[test]
fn gallery_terminating_rule_halts() {
    let rule = parse_rule("{{x,y},{y,z}}->{{x,z}}").unwrap();
    let init = parse_state("{{0,1},{1,2},{2,3},{3,0}}").unwrap();
    let mw = evolve(&rule, init, 8);

    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 1, 1, 0]);

    let last = mw.layers[3][0];
    assert!(
        find_matches(&mw.states[last].state, &rule).is_empty(),
        "rewriting has not actually terminated"
    );
}

/// Row: `{{x,y}}->{{y,x}}` — the back-merge demo (period-2 reversal
/// recurs into earlier states). The only rule in the gallery exercising
/// `back_merges > 0`.
#[test]
fn gallery_reversal_rule_back_merges() {
    let rule = parse_rule("{{x,y}}->{{y,x}}").unwrap();
    let init = parse_state("{{0,1},{1,2}}").unwrap();
    let mw = evolve(&rule, init, 3);
    assert_eq!(mw.back_merges, 4);
    assert_eq!(mw.states.len(), 3);
}

/// Row: `{{x,y,z}}->{{x,y,w},{y,w,z}}` — arity-3 hyperedges are
/// first-class. Structural: every edge in every state stays ternary.
#[test]
fn gallery_ternary_rule_grows() {
    let rule = parse_rule("{{x,y,z}}->{{x,y,w},{y,w,z}}").unwrap();
    let init = parse_state("{{0,0,0}}").unwrap();
    let mw = evolve(&rule, init, 3);

    for s in &mw.states {
        assert!(s.state.edges.iter().all(|e| e.len() == 3));
    }
    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 2, 5]);
}

/// Row: `{{x,y}}->{{x,z},{z,y}}` — edge subdivision. Structural: exactly
/// k+1 edges at step k; maximal sharing (one canonical state per layer,
/// absorbing k! naive nodes).
#[test]
fn gallery_subdivision_rule_stats() {
    let rule = parse_rule("{{x,y}}->{{x,z},{z,y}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let mw = evolve(&rule, init, 4);

    for s in &mw.states {
        assert_eq!(s.state.edges.len(), s.step + 1);
    }
    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 1, 1, 1]);

    let tree: Vec<u128> = mw.sharing_per_layer().iter().map(|(_, p, _)| *p).collect();
    assert_eq!(tree, vec![1, 1, 2, 6, 24]);
}

/// Checker-demo rows: the verdicts the README will print must be exactly
/// what the checker emits.
#[test]
fn gallery_checker_rows() {
    // subdivision: no nontrivial self-overlap -> vacuous all-strong, not confluent
    let subdivision = parse_rule("{{x,y}}->{{x,z},{z,y}}").unwrap();
    let r = check(std::slice::from_ref(&subdivision), &CheckCfg::default()).unwrap();
    assert!(matches!(
        r.verdict,
        Verdict::AllCriticalPairsStronglyJoinable {
            pairs_checked: 0,
            confluent: false,
            ..
        }
    ));

    // idempotent parallel-edge merge: the honest `confluent: true`
    let merge = parse_rule("{{x,y},{x,y}}->{{x,y}}").unwrap();
    let r = check(std::slice::from_ref(&merge), &CheckCfg::default()).unwrap();
    assert!(matches!(
        r.verdict,
        Verdict::AllCriticalPairsStronglyJoinable {
            confluent: true,
            ..
        }
    ));

    // the counterexample pair
    let r1 = parse_rule("{{x,y}}->{{x}}").unwrap();
    let r2 = parse_rule("{{x,y}}->{}").unwrap();
    let r = check(&[r1, r2], &CheckCfg::default()).unwrap();
    assert!(matches!(r.verdict, Verdict::NotConfluent { .. }));
}
