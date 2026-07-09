use multiway::canon::{isomorphic, wl_hash};
use multiway::causal;
use multiway::matcher::find_matches;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;

#[test]
fn parse_rule_basics() {
    let r = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    assert_eq!(r.lhs.len(), 1);
    assert_eq!(r.rhs.len(), 2);
    assert_eq!(r.n_vars, 3);

    let classic = parse_rule("{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}").unwrap();
    assert_eq!(classic.lhs.len(), 2);
    assert_eq!(classic.rhs.len(), 4);
    assert_eq!(classic.n_vars, 4);
}

#[test]
fn hash_is_isomorphism_invariant() {
    // Same structure under a vertex renaming (0,1,2) -> (5,7,3).
    let a = parse_state("{{0,1},{1,2},{0,0}}").unwrap();
    let b = parse_state("{{5,7},{7,3},{5,5}}").unwrap();
    assert_eq!(wl_hash(&a), wl_hash(&b));
    assert!(isomorphic(&a, &b));
}

#[test]
fn non_isomorphic_states_distinguished() {
    // Out-star vs directed path: same edge count, different structure.
    let star = parse_state("{{0,1},{0,2}}").unwrap();
    let path = parse_state("{{0,1},{1,2}}").unwrap();
    assert_ne!(wl_hash(&star), wl_hash(&path));
    assert!(!isomorphic(&star, &path));

    // Ordered edges: reversal matters.
    let ab = parse_state("{{0,1}}").unwrap();
    let ba = parse_state("{{1,0}}").unwrap();
    assert_eq!(wl_hash(&ab), wl_hash(&ba)); // iso: rename 0<->1
    assert!(isomorphic(&ab, &ba));
    let loop_ = parse_state("{{0,0}}").unwrap();
    assert_ne!(wl_hash(&ab), wl_hash(&loop_));
    assert!(!isomorphic(&ab, &loop_));
}

#[test]
fn match_counting() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let star3 = parse_state("{{0,0},{0,1},{0,2}}").unwrap();
    assert_eq!(find_matches(&star3, &rule).len(), 3);

    // Two-edge LHS sharing the first variable on a double self-loop:
    // both orderings of the two instances match -> 2 matches.
    let classic = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    assert_eq!(find_matches(&init, &classic).len(), 2);
}

#[test]
fn multiway_layer_counts_hand_verified() {
    // Growth rule from a self-loop. Hand-computed canonical layer sizes:
    //   step 0: {{0,0}}                                     -> 1 state
    //   step 1: {{0,0},{0,1}}                               -> 1 state
    //   step 2: star vs path                                -> 2 states
    //   step 3: star4 | {loop,0->1,0->2,1->3} (~= one of the
    //           path children) | two more path children     -> 4 states
    // and the naive tree has 1,1,2,6 nodes at those depths.
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let mw = evolve(&rule, init, 3);

    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 2, 4]);

    let events_per_step: Vec<usize> = (1..=3)
        .map(|s| mw.events.iter().filter(|e| e.step == s).count())
        .collect();
    assert_eq!(events_per_step, vec![1, 2, 6]);

    let sharing = mw.sharing_per_layer();
    let tree_nodes: Vec<u128> = sharing.iter().map(|(_, p, _)| *p).collect();
    assert_eq!(tree_nodes, vec![1, 1, 2, 6]);

    // Two same-parent step-2 siblings -> at least one branchial pair.
    assert!(!mw.branchial.is_empty());
    assert_eq!(mw.back_merges, 0);
}

#[test]
fn causal_run_growth_rule() {
    // Growth rule: each event consumes 1 edge, adds 2 -> +1 edge per event.
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let c = causal::run(&rule, init, 5);
    assert_eq!(c.n_events, 6); // event 0 (init) + 5 rewrites
    assert_eq!(c.final_state.edge_count(), 6);
    assert!(!c.deps.is_empty());
    // Every dependency points forward in time.
    assert!(c.deps.iter().all(|(a, b)| a < b));
}
