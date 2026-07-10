//! Token-event graph: causal structure across ALL updating orders, on the
//! merged canonical DAG.
//!
//! Token identity = (state id, canonical slot) under the engine's fixed
//! labeling convention — one deterministic section of the automorphism-
//! quotiented TEG, with creator sets unioned across byte-identical-edge
//! slot runs. Event id convention matches causal.rs: 0 = the initial
//! condition, event i+1 = mw.events[i].

mod common;

use common::gen::{gen_rule_text, gen_state, RuleCfg, StateCfg};
use common::harness::prop;
use common::jsonck::check_json;
use multiway::export::bundle_json;
use multiway::matcher::{apply_full, find_matches};
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;
use multiway::teg;

const PROP_SEED: u64 = 0x00C0_FFEE_0000_000B;

/// The u32-slot representation's range invariant, over RANDOM systems:
/// every token slot indexes inside its state's canonical edge list —
/// the cast to u32 can never have truncated (edges-per-state is orders
/// of magnitude below u32::MAX under any budget).
#[test]
fn prop_token_slots_in_range() {
    prop(PROP_SEED, "prop_token_slots_in_range", |rng, _| {
        let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
        let init = gen_state(
            rng,
            &StateCfg {
                max_vertices: 4,
                max_edges: 3,
                max_arity: 3,
                dup_pct: 25,
                self_loop_pct: 25,
                sparse_labels: false,
            },
        );
        let steps = rng.range_usize(1, 3);
        let mw = evolve(&rule, init, steps);
        for (idx, e) in mw.events.iter().enumerate() {
            let et = &mw.event_tokens[idx];
            let pn = mw.states[e.from].form_ids.len();
            let cn = mw.states[e.to].form_ids.len();
            for &s in &et.consumed {
                assert!((s as usize) < pn, "consumed slot out of range");
            }
            for &s in &et.produced {
                assert!((s as usize) < cn, "produced slot out of range");
            }
            for &(ps, cs) in &et.passthrough {
                assert!((ps as usize) < pn && (cs as usize) < cn);
            }
        }
    });
}

/// apply_full's layout contract (compile-red here in M3 — M5's incremental
/// matcher builds on it, and it would be green-on-arrival there).
#[test]
fn apply_full_layout() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let state = parse_state("{{0,1},{2,3}}").unwrap();
    let ms = find_matches(&state, &rule);
    // first match consumes edge 0 (x=0, y=1)
    let app = apply_full(&state, &rule, &ms[0]);

    assert_eq!(app.kept, vec![(1, 0)], "kept edge {{2,3}} moves to child 0");
    assert_eq!(app.produced, 1..3);
    assert_eq!(
        app.child.edges,
        vec![vec![2, 3], vec![0, 1], vec![1, 4]],
        "kept edges first (parent order), then RHS with fresh z=4"
    );
    assert_eq!(app.child.next_vertex, 5);
}

/// Recording shape on the growth rule: every event consumes 1 slot,
/// produces 2, and passes through parent_edges - 1; all slots in range.
#[test]
fn event_tokens_recorded_growth_rule() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let mw = evolve(&rule, init, 2);

    assert_eq!(mw.event_tokens.len(), mw.events.len());
    for (idx, e) in mw.events.iter().enumerate() {
        let et = &mw.event_tokens[idx];
        let parent_edges = mw.states[e.from].state.edges.len();
        let child_edges = mw.states[e.to].state.edges.len();
        assert_eq!(et.consumed.len(), 1);
        assert_eq!(et.produced.len(), 2);
        assert_eq!(et.passthrough.len(), parent_edges - 1);
        // the u32 slot representation: every slot must fit its
        // state's edge count (far below u32::MAX by budget)
        for &s in &et.consumed {
            assert!((s as usize) < parent_edges);
        }
        for &s in &et.produced {
            assert!((s as usize) < child_edges);
        }
        for &(ps, cs) in &et.passthrough {
            assert!((ps as usize) < parent_edges);
            assert!((cs as usize) < child_edges);
        }
    }
}

/// Hand-verified creators on one growth step: the initial slot belongs to
/// event 0; both slots of the step-1 state are produced by event 1
/// (nothing passes through — the only parent edge is consumed).
#[test]
fn teg_creators_hand_verified() {
    let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let mw = evolve(&rule, init, 1);
    let t = teg::build(&mw);

    assert_eq!(t.creators[0], vec![vec![0]]);
    assert_eq!(t.creators[1], vec![vec![1], vec![1]]);
    assert_eq!(t.causal, vec![(0, 1)]);
}

/// Single-path oracle: the rule {{x}}->{{x,y},{y}} keeps exactly one
/// arity-1 edge per state, so the multiway evolution IS a linear chain and
/// the TEG's causal edges must coincide with the trusted single-path
/// causal module's deps.
#[test]
fn teg_matches_single_path_causal_oracle() {
    let rule = parse_rule("{{x}}->{{x,y},{y}}").unwrap();
    let init = parse_state("{{0}}").unwrap();

    let mw = evolve(&rule, init.clone(), 4);
    // linear chain: one state and one event per layer
    for layer in &mw.layers {
        assert_eq!(layer.len(), 1);
    }
    let t = teg::build(&mw);

    let c = multiway::causal::run(&rule, init, 4);
    let mut expected = c.deps.clone();
    expected.sort_unstable();
    expected.dedup();
    assert_eq!(t.causal, expected);
}

/// THE merge test: canonical merging makes a token's creator path-
/// dependent, so some slot in the depth-2 classic system must have ≥2
/// creators. Pins the quotient semantics.
#[test]
fn teg_merge_gives_multivalued_creators() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 2);
    let t = teg::build(&mw);

    let multi = t
        .creators
        .iter()
        .flatten()
        .any(|creators| creators.len() >= 2);
    assert!(
        multi,
        "24 naive histories merge into 3 canonical states — some token must have multiple creators"
    );
}

/// Branchial events pair only on overlapping consumption: three disjoint
/// single-edge matches produce no pairs; the classic init's two matches
/// consume the same two edges and must pair.
#[test]
fn teg_branchial_events_overlap_only() {
    let growth = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
    let disjoint = parse_state("{{0,1},{1,2},{3,4}}").unwrap();
    let mw = evolve(&growth, disjoint, 1);
    assert_eq!(mw.events.len(), 3);
    let t = teg::build(&mw);
    assert!(
        t.branchial_events.is_empty(),
        "disjoint consumption must not pair: {:?}",
        t.branchial_events
    );

    let classic = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&classic, init, 1);
    assert_eq!(mw.events.len(), 2);
    let t = teg::build(&mw);
    assert_eq!(
        t.branchial_events,
        vec![(1, 2)],
        "both events consume both self-loops — they must pair"
    );
}

/// Cyclic merged DAG: {{x,y}}->{{y,x}} maps the state to itself up to
/// isomorphism. The creator fixed point must converge with the slot
/// carrying BOTH the initial condition and event 1, and the causal graph
/// must contain the self-dependency (1,1) — "some instance of this event
/// class consumes a token produced by another instance of the same class
/// across histories."
#[test]
fn teg_self_loop_rule_semantics() {
    let rule = parse_rule("{{x,y}}->{{y,x}}").unwrap();
    let init = parse_state("{{0,1}}").unwrap();
    let mw = evolve(&rule, init, 1);

    assert_eq!(mw.states.len(), 1, "reversal merges into the initial state");
    assert_eq!(mw.back_merges, 1);

    let t = teg::build(&mw);
    assert_eq!(t.creators[0], vec![vec![0, 1]]);
    assert_eq!(t.causal, vec![(0, 1), (1, 1)]);
}

/// Build twice, export twice: byte-equal (no iteration-order leak
/// anywhere in the TEG pipeline).
#[test]
fn teg_deterministic_bytes() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();

    let render = || {
        let mw = evolve(&rule, init.clone(), 3);
        bundle_json(&rule.text, "{{0,0},{0,0}}", &mw, None)
    };
    let a = render();
    let b = render();
    assert_eq!(a, b);
    check_json(&a).unwrap();
    assert!(
        a.contains("\"teg\""),
        "bundle must carry the teg section for the viewer"
    );
}

/// Baseline re-pin after the recording changes to evolve.
#[test]
fn baseline_still_green() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 4);
    let layer_sizes: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
    assert_eq!(layer_sizes, vec![1, 1, 3, 18, 156]);
    assert_eq!(mw.back_merges, 0);
}
