//! Baseline regression pins for the flagship classic-rule numbers.
//!
//! These are the load-bearing constants every subsequent milestone
//! (canonization, token-event graph, incremental matching, parallelism)
//! must reproduce exactly. The depth-4 pin is additionally cross-checked
//! against the naive-tree oracle at depth 3, so the pinned values are
//! *proven* against ground truth, not merely observed once.

mod common;

use common::oracle::{iso_classes, naive_tree};
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;

const CLASSIC: &str = "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}";
const INIT: &str = "{{0,0},{0,0}}";

/// Classic Wolfram-model rule from a double self-loop, depth 4.
///
/// `back_merges == 0` is provable, not just observed: every event of this
/// rule consumes edges over existing vertices and mints exactly one fresh
/// vertex `w` (x, y, z all reappear on the RHS, and `apply` never deletes
/// vertices that remain in other edges — here x persists via `{x,z}`).
/// The initial state has 1 vertex, so every state first reached at step k
/// has exactly k+1 vertices; states at different steps therefore differ in
/// vertex count and can never be isomorphic, which is what a back-merge
/// would require.
#[test]
fn classic_rule_depth4_baseline() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state(INIT).unwrap();
    let mw = evolve(&rule, init.clone(), 4);

    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 3, 18, 156]);

    let tree_nodes: Vec<u128> = mw.sharing_per_layer().iter().map(|(_, p, _)| *p).collect();
    assert_eq!(tree_nodes, vec![1, 2, 24, 408, 9504]);

    assert_eq!(mw.back_merges, 0);

    // Ground-truth cross-check at depth 3: the unshared evolution tree has
    // layer sizes [1,2,24,408], and its depth-3 layer partitions into
    // exactly 18 isomorphism classes — the engine's canonical layer count.
    // Depth-3 naive states have exactly 4 vertices (see doc comment), well
    // under the brute-force oracle's 7-vertex guard.
    let naive = naive_tree(&rule, &init, 3, 1000).expect("435 nodes fits the 1000 cap");
    let naive_sizes: Vec<usize> = naive.iter().map(|l| l.len()).collect();
    assert_eq!(naive_sizes, vec![1, 2, 24, 408]);
    assert_eq!(iso_classes(&naive[3]).len(), 18);
}

/// Depth-5 variant: ~0.1s in release, tens of seconds in debug — run by the
/// CI release job via `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn classic_rule_depth5_baseline() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state(INIT).unwrap();
    let mw = evolve(&rule, init, 5);

    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 3, 18, 156, 1776]);

    let tree_nodes: Vec<u128> = mw.sharing_per_layer().iter().map(|(_, p, _)| *p).collect();
    assert_eq!(tree_nodes, vec![1, 2, 24, 408, 9504, 280080]);

    assert_eq!(mw.back_merges, 0);
}

/// Perf smoke with a 100× margin: catches catastrophic regressions,
/// immune to CI noise. `#[ignore]`d — run by the release CI job only.
#[test]
#[ignore]
fn perf_smoke_classic_depth5() {
    let rule = parse_rule(CLASSIC).unwrap();
    let init = parse_state(INIT).unwrap();
    let t = std::time::Instant::now();
    let mw = evolve(&rule, init, 5);
    assert_eq!(mw.states.len(), 1955);
    assert!(
        t.elapsed() < std::time::Duration::from_secs(10),
        "depth-5 took {:?} — baseline is ~0.1s release",
        t.elapsed()
    );
}
