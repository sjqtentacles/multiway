//! True canonization: the canonical *form* must be a complete isomorphism
//! invariant — equal forms ⟺ isomorphic states — with a deterministic
//! witness (vertex_map + edge_slots) for the token-event graph.

mod common;

use common::gen::{gen_renaming, gen_state, rename, shuffle_edges, StateCfg};
use common::harness::prop;
use common::oracle::iso_bruteforce;
use multiway::canon::{canonical_form, canonicalize, canonicalize_with_leaf_count, isomorphic};
use multiway::hypergraph::State;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;

const SEED: u64 = 0x00C0_FFEE_0000_0004;

/// Hand cases pin the basics; the `{{5,5},{5,7}}` constant is a captured
/// golden of the engine's documented search discipline (minimal IR leaf
/// under ascending-vertex-id branching), NOT a hand-derived global
/// lex-minimum — computing that would be the graph-canonization problem
/// itself.
#[test]
fn canonical_form_hand_cases() {
    // empty state
    let empty = parse_state("{}").unwrap();
    let cf = canonical_form(&empty);
    assert_eq!(cf.edges, Vec::<Vec<u32>>::new());
    assert_eq!(cf.next_vertex, 0);

    // single self-loop relabels to vertex 0
    assert_eq!(
        canonical_form(&parse_state("{{5,5}}").unwrap()).edges,
        vec![vec![0, 0]]
    );

    // golden constant (captured once, reviewed): loop-plus-out-edge
    assert_eq!(
        canonical_form(&parse_state("{{5,5},{5,7}}").unwrap()).edges,
        vec![vec![0, 0], vec![0, 1]]
    );

    // arity-0 edges survive with multiplicity
    assert_eq!(
        canonical_form(&parse_state("{{},{}}").unwrap()).edges,
        vec![Vec::<u32>::new(), Vec::<u32>::new()]
    );
}

/// The form is an admissible relabeling of the input: isomorphic to it,
/// dense labels 0..n-1, edges sorted by (len, seq), idempotent.
#[test]
fn prop_canonical_form_is_admissible_relabeling() {
    prop(
        SEED,
        "prop_canonical_form_is_admissible_relabeling",
        |rng, _| {
            let s = gen_state(rng, &StateCfg::oracle());
            let cf = canonical_form(&s);

            assert!(isomorphic(&s, &cf), "form not isomorphic to input");

            let n = s.vertices().len() as u32;
            let vs = cf.vertices();
            assert_eq!(vs, (0..n).collect::<Vec<_>>(), "labels not dense 0..n");
            assert_eq!(cf.next_vertex, n);

            let mut sorted = cf.edges.clone();
            sorted.sort_by(|a, b| (a.len(), a).cmp(&(b.len(), b)));
            assert_eq!(cf.edges, sorted, "form edges not sorted by (len, seq)");

            assert_eq!(canonical_form(&cf), cf, "canonical form not idempotent");
        },
    );
}

/// Multiset and edge-order semantics must survive canonization: duplicate
/// instances stay duplicated, reversal is not duplication, star is not
/// path.
#[test]
fn canonical_form_multiset_and_order() {
    let one_loop = parse_state("{{0,0}}").unwrap();
    let two_loops = parse_state("{{0,0},{0,0}}").unwrap();
    assert_eq!(canonical_form(&two_loops).edges.len(), 2);
    assert_ne!(canonical_form(&one_loop), canonical_form(&two_loops));

    let cycle2 = parse_state("{{0,1},{1,0}}").unwrap();
    let parallel2 = parse_state("{{0,1},{0,1}}").unwrap();
    assert_ne!(canonical_form(&cycle2), canonical_form(&parallel2));

    let star = parse_state("{{0,1},{0,2}}").unwrap();
    let path = parse_state("{{0,1},{1,2}}").unwrap();
    assert_ne!(canonical_form(&star), canonical_form(&path));
}

/// A directed 3-cycle is vertex-transitive: refinement alone leaves one
/// cell of size 3, so a no-search implementation (label by refined color
/// or input order) gives input-dependent forms. This test drives the
/// individualization step, and pins the leaf count (3 branches, each
/// discrete after one individualization).
#[test]
fn symmetric_cycle_forces_individualization() {
    let a = parse_state("{{0,1},{1,2},{2,0}}").unwrap();
    let b = parse_state("{{7,3},{3,9},{9,7}}").unwrap();
    assert_eq!(canonical_form(&a), canonical_form(&b));

    let (_, leaves) = canonicalize_with_leaf_count(&a);
    assert_eq!(leaves, 3, "3-cell individualization should visit 3 leaves");
}

/// Witness validity: edge_slots is a permutation mapping each raw edge to
/// its relabeled image in the form, and vertex_map is a bijection from the
/// vertex set onto 0..n-1.
#[test]
fn prop_canonicalize_witness_valid() {
    prop(SEED ^ 1, "prop_canonicalize_witness_valid", |rng, _| {
        let s = gen_state(rng, &StateCfg::oracle());
        let c = canonicalize(&s);

        // vertex_map: bijection vertex-set -> 0..n-1
        let vs = s.vertices();
        assert_eq!(c.vertex_map.len(), vs.len());
        let mut images: Vec<u32> = vs.iter().map(|v| c.vertex_map[v]).collect();
        images.sort_unstable();
        assert_eq!(images, (0..vs.len() as u32).collect::<Vec<_>>());

        // edge_slots: permutation of 0..edges.len()
        let mut slots = c.edge_slots.clone();
        slots.sort_unstable();
        assert_eq!(slots, (0..s.edges.len()).collect::<Vec<_>>());

        // each raw edge relabels to exactly the form edge in its slot
        for (i, e) in s.edges.iter().enumerate() {
            let relabeled: Vec<u32> = e.iter().map(|v| c.vertex_map[v]).collect();
            assert_eq!(
                c.form.edges[c.edge_slots[i]], relabeled,
                "slot {} does not hold the image of raw edge {}",
                c.edge_slots[i], i
            );
        }

        // byte-identical duplicate edges occupy slots in raw-index order
        // (the fixed token convention)
        for i in 0..s.edges.len() {
            for j in (i + 1)..s.edges.len() {
                if s.edges[i] == s.edges[j] {
                    assert!(
                        c.edge_slots[i] < c.edge_slots[j],
                        "duplicate edges {} and {} out of raw-index slot order",
                        i,
                        j
                    );
                }
            }
        }
    });
}

/// THE workhorse: canonical-form equality must coincide exactly with
/// brute-force isomorphism — both directions, on the same adversarial
/// pair generator as the hash lattice (relabelings, independents, mutated
/// near-isomorphs).
#[test]
fn prop_canonical_form_complete_wrt_bruteforce() {
    prop(
        SEED ^ 2,
        "prop_canonical_form_complete_wrt_bruteforce",
        |rng, _| {
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
                    let map = gen_renaming(rng, &a, false);
                    let mut b = rename(&a, &map);
                    if !b.edges.is_empty() {
                        let ei = rng.range_usize(0, b.edges.len() - 1);
                        let pos = rng.range_usize(0, b.edges[ei].len() - 1);
                        b.edges[ei][pos] = rng.below(6) as u32;
                    }
                    shuffle_edges(rng, &mut b);
                    State::new(b.edges)
                }
            };

            let bf = iso_bruteforce(&a, &b);
            assert_eq!(
                canonical_form(&a) == canonical_form(&b),
                bf,
                "canonical form incomplete/unsound on {:?} vs {:?}",
                a.edges,
                b.edges
            );
        },
    );
}

/// k identical disjoint components is the one realistic IR pathology
/// (k! leaves without decomposition). With component decomposition the
/// leaf count stays linear in k. NOTE: the baseline init {{0,0},{0,0}} is
/// ONE component (two self-loops on the same vertex) — the genuine k=2
/// case needs vertex-disjoint copies.
#[test]
fn disjoint_identical_components() {
    let k2 = parse_state("{{0,0},{1,1}}").unwrap();
    let cf = canonical_form(&k2);
    assert_eq!(cf.edges, vec![vec![0, 0], vec![1, 1]]);

    // 4 scrambled disjoint copies of the 2-edge path {a,b},{b,c}
    let copies_a =
        parse_state("{{0,1},{1,2},{10,11},{11,12},{20,21},{21,22},{30,31},{31,32}}").unwrap();
    let copies_b =
        parse_state("{{31,32},{1,2},{11,12},{21,22},{0,1},{30,31},{10,11},{20,21}}").unwrap();
    assert_eq!(canonical_form(&copies_a), canonical_form(&copies_b));

    let (_, leaves) = canonicalize_with_leaf_count(&copies_a);
    assert!(
        leaves <= 8,
        "leaf count {} suggests k! blowup — component decomposition missing",
        leaves
    );
}

/// After the EdgeStore migration: dedup keys on interned form ids, the
/// baseline pin is unchanged, every stored form resolves back to exactly
/// `canonical_form(raw)` — same assertion strength as when the full form
/// was stored — and keys are pairwise distinct.
#[test]
fn evolve_uses_canonical_dedup() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 3);

    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 3, 18]);

    for s in &mw.states {
        let resolved: Vec<Vec<u32>> = s
            .form_ids
            .iter()
            .map(|&id| mw.store.resolve(id).clone())
            .collect();
        assert_eq!(
            resolved,
            canonical_form(&s.state).edges,
            "stored form_ids inconsistent with canonical_form for state {}",
            s.id
        );
    }
    for i in 0..mw.states.len() {
        for j in (i + 1)..mw.states.len() {
            assert_ne!(
                mw.states[i].form_ids, mw.states[j].form_ids,
                "states {} and {} share a canonical key — dedup failed",
                i, j
            );
        }
    }
}

// ---------------------------------------------------------------------------
// E1: budgeted canonization (scan safety)

use multiway::canon::canonicalize_budgeted;

/// A directed out-star is the IR pathology: refinement cannot split the
/// spoke endpoints, so the search visits k! leaves. star-20 = 20! ≈ 2.4e18
/// leaves unpruned — a scan hitting one such state would hang forever.
/// The budget must abort cheaply.
#[test]
fn star_canonization_hits_leaf_budget() {
    let edges: Vec<Vec<u32>> = (1..=20).map(|i| vec![0, i]).collect();
    let star = State::new(edges);
    assert!(
        canonicalize_budgeted(&star, 1000).is_none(),
        "star-20 must exhaust a 1000-leaf budget"
    );
    // small stars fit comfortably
    let edges: Vec<Vec<u32>> = (1..=4).map(|i| vec![0, i]).collect();
    assert!(canonicalize_budgeted(&State::new(edges), 1000).is_some());
}

/// With an unlimited budget the budgeted path is the same computation:
/// form, witness, and slots all identical.
#[test]
fn prop_budgeted_max_agrees_with_canonicalize() {
    prop(
        SEED ^ 3,
        "prop_budgeted_max_agrees_with_canonicalize",
        |rng, _| {
            let s = gen_state(rng, &StateCfg::oracle());
            let a = canonicalize(&s);
            let b = canonicalize_budgeted(&s, u64::MAX).expect("unlimited budget aborted");
            assert_eq!(a.form, b.form);
            assert_eq!(a.edge_slots, b.edge_slots);
            let mut am: Vec<(u32, u32)> = a.vertex_map.iter().map(|(k, v)| (*k, *v)).collect();
            let mut bm: Vec<(u32, u32)> = b.vertex_map.iter().map(|(k, v)| (*k, *v)).collect();
            am.sort_unstable();
            bm.sort_unstable();
            assert_eq!(am, bm);
        },
    );
}
