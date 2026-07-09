//! Engine-level invariants for `evolve` and `causal::run` over random
//! (rule, init, steps) triples, cross-checked against the naive-tree
//! oracle, plus JSON export well-formedness and the script-safety pin
//! for the HTML-embedded bundle.

mod common;

use common::gen::{gen_rule_text, gen_state, RuleCfg, StateCfg};
use common::harness::prop;
use common::jsonck::check_json;
use common::oracle::{iso_classes, naive_tree};
use multiway::export::bundle_json;
use multiway::rule::{parse_rule, parse_state, Rule};
use multiway::system::{evolve, MultiwaySystem};
use std::collections::BTreeMap;

const SEED: u64 = 0x00C0_FFEE_0000_0003;

fn small_init() -> StateCfg {
    StateCfg {
        max_vertices: 4,
        max_edges: 3,
        max_arity: 3,
        dup_pct: 25,
        self_loop_pct: 25,
        sparse_labels: false,
    }
}

fn random_system(rng: &mut common::prng::Rng) -> (Rule, multiway::hypergraph::State, usize) {
    let rule = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
    let init = gen_state(rng, &small_init());
    let steps = rng.range_usize(1, 3);
    (rule, init, steps)
}

/// Layers partition the state ids; ids are dense and step-consistent.
#[test]
fn prop_layers_partition_states() {
    prop(SEED, "prop_layers_partition_states", |rng, _| {
        let (rule, init, steps) = random_system(rng);
        let mw = evolve(&rule, init, steps);

        let mut seen: Vec<usize> = mw.layers.iter().flatten().copied().collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..mw.states.len()).collect::<Vec<_>>());

        for (i, s) in mw.states.iter().enumerate() {
            assert_eq!(s.id, i);
        }
        for (step, layer) in mw.layers.iter().enumerate() {
            for &id in layer {
                assert_eq!(mw.states[id].step, step);
            }
        }
    });
}

/// Events reference valid states with consistent steps, and the engine's
/// back-merge counter matches an independent recount.
#[test]
fn prop_events_well_formed() {
    prop(SEED ^ 1, "prop_events_well_formed", |rng, _| {
        let (rule, init, steps) = random_system(rng);
        let mw = evolve(&rule, init, steps);

        let mut back = 0usize;
        for (i, e) in mw.events.iter().enumerate() {
            assert_eq!(e.id, i);
            assert!(e.from < mw.states.len());
            assert!(e.to < mw.states.len());
            assert_eq!(mw.states[e.from].step, e.step - 1);
            assert!(mw.states[e.to].step <= e.step);
            if mw.states[e.to].step < e.step {
                back += 1;
            }
        }
        assert_eq!(back, mw.back_merges, "back_merges recount disagrees");
    });
}

/// Branchial pairs, recomputed exactly from the event list: per step, for
/// each parent, distinct children in first-occurrence order, all unordered
/// pairs, then sort+dedup per step and concatenate. Exact Vec equality —
/// soundness AND completeness.
#[test]
fn prop_branchial_exact() {
    prop(SEED ^ 2, "prop_branchial_exact", |rng, _| {
        let (rule, init, steps) = random_system(rng);
        let mw = evolve(&rule, init, steps);

        let max_step = mw.layers.len() - 1;
        let mut expected: Vec<(usize, usize)> = Vec::new();
        for step in 1..=max_step {
            let mut by_from: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for e in mw.events.iter().filter(|e| e.step == step) {
                let children = by_from.entry(e.from).or_default();
                if !children.contains(&e.to) {
                    children.push(e.to);
                }
            }
            let mut step_pairs: Vec<(usize, usize)> = Vec::new();
            for children in by_from.values() {
                for i in 0..children.len() {
                    for j in (i + 1)..children.len() {
                        let a = children[i].min(children[j]);
                        let b = children[i].max(children[j]);
                        step_pairs.push((a, b));
                    }
                }
            }
            step_pairs.sort_unstable();
            step_pairs.dedup();
            expected.extend(step_pairs);
        }
        assert_eq!(mw.branchial, expected);
    });
}

/// The flagship correctness property: when nothing back-merges, per-layer
/// path-count sums equal the naive tree's layer sizes exactly, and the
/// engine's canonical layer count equals the oracle's isomorphism-class
/// count. Any dedup bug — missed merge or overmerge — breaks one of the
/// two equalities.
#[test]
fn prop_path_counts_exact_vs_naive_tree() {
    let mut skipped_cap = 0usize;
    let mut skipped_verts = 0usize;
    let mut skipped_backmerge = 0usize;
    let mut checked = 0usize;
    prop(
        SEED ^ 3,
        "prop_path_counts_exact_vs_naive_tree",
        |rng, _| {
            let (rule, init, steps) = random_system(rng);

            let naive = match naive_tree(&rule, &init, steps, 300) {
                Some(n) => n,
                None => {
                    skipped_cap += 1;
                    return;
                }
            };
            if naive.iter().flatten().any(|s| s.vertices().len() > 7) {
                skipped_verts += 1;
                return;
            }

            let mw = evolve(&rule, init, steps);
            if mw.back_merges != 0 {
                skipped_backmerge += 1;
                return;
            }
            checked += 1;

            let sharing = mw.sharing_per_layer();
            for (step, layer) in naive.iter().enumerate() {
                // evolve stops recording layers once the frontier empties; the
                // naive tree keeps emitting empty layers to `steps`.
                if step >= sharing.len() {
                    assert!(
                        layer.is_empty(),
                        "engine stopped at step {} but naive tree still has {} states",
                        sharing.len() - 1,
                        layer.len()
                    );
                    continue;
                }
                let (_, paths, canon) = sharing[step];
                assert_eq!(
                    paths,
                    layer.len() as u128,
                    "path-count sum != naive layer size at step {} (rule {})",
                    step,
                    rule.text
                );
                assert_eq!(
                    canon,
                    iso_classes(layer).len(),
                    "canonical layer count != oracle iso classes at step {} (rule {})",
                    step,
                    rule.text
                );
            }
        },
    );
    println!(
        "path-counts oracle: {} checked, {} skipped (cap), {} skipped (>7 verts), {} skipped (back-merges)",
        checked, skipped_cap, skipped_verts, skipped_backmerge
    );
    assert!(checked > 0, "generator drift: oracle never ran");
}

/// Byte determinism: two evolutions of cloned inputs serialize identically.
#[test]
fn prop_evolve_deterministic() {
    prop(SEED ^ 4, "prop_evolve_deterministic", |rng, _| {
        let (rule, init, steps) = random_system(rng);
        let a = evolve(&rule, init.clone(), steps);
        let b = evolve(&rule, init.clone(), steps);
        let ja = bundle_json(&rule.text, &init.to_notation(), &a, None);
        let jb = bundle_json(&rule.text, &init.to_notation(), &b, None);
        assert_eq!(ja, jb, "evolve output not byte-deterministic");
    });
}

/// Causal runs: dependencies point strictly forward at valid ids, and the
/// final edge count obeys the per-event edge delta (signed — shrinking
/// rules must not underflow the test).
#[test]
fn prop_causal_deps_forward() {
    prop(SEED ^ 5, "prop_causal_deps_forward", |rng, _| {
        let (rule, init, _) = random_system(rng);
        let init_edges = init.edge_count() as i64;
        let c = multiway::causal::run(&rule, init, 5);
        for &(a, b) in &c.deps {
            assert!(a < b, "dep not forward: ({}, {})", a, b);
            assert!(b < c.n_events);
        }
        let delta = rule.rhs.len() as i64 - rule.lhs.len() as i64;
        assert_eq!(
            c.final_state.edge_count() as i64,
            init_edges + (c.n_events as i64 - 1) * delta
        );
    });
}

/// Every exported bundle is well-formed JSON (checker written against the
/// grammar, independent of the emitter).
#[test]
fn prop_bundle_json_well_formed() {
    prop(SEED ^ 6, "prop_bundle_json_well_formed", |rng, _| {
        let (rule, init, steps) = random_system(rng);
        let causal = multiway::causal::run(&rule, init.clone(), 4);
        let mw = evolve(&rule, init.clone(), steps);
        let json = bundle_json(&rule.text, &init.to_notation(), &mw, Some(&causal));
        check_json(&json).unwrap_or_else(|e| panic!("bad JSON ({}) in {:?}", e, json));
    });
}

/// Script-safety pin for the HTML-embedded bundle (red-first: check_json
/// alone can never catch this — a raw '<' and raw U+2028/U+2029 are
/// perfectly legal JSON, but a literal "</script>" inside the inline
/// <script> block would terminate it, and U+2028/29 are illegal in JS
/// string literals under pre-ES2019 parsers).
#[test]
fn esc_script_safety() {
    let rule = parse_rule("{{x,y}}->{{x,y}}").unwrap();
    let init = parse_state("{{0,0}}").unwrap();
    let mw: MultiwaySystem = evolve(&rule, init, 0);
    let hostile = "</script><script>alert(1)</script>\u{2028}\u{2029}";
    let json = bundle_json(hostile, hostile, &mw, None);
    assert!(
        !json.contains('<'),
        "raw '<' escaped the emitter: {:?}",
        json
    );
    assert!(!json.contains('\u{2028}'), "raw U+2028 in output");
    assert!(!json.contains('\u{2029}'), "raw U+2029 in output");
    check_json(&json).unwrap();
}

/// The checker itself, sanity-pinned on hand cases (both directions).
#[test]
fn jsonck_hand_cases() {
    for good in [
        "{}",
        "[]",
        "null",
        "-0.5e+10",
        r#""a<b""#,
        r#"{"k":[1,2,{"x":"\n"}],"z":null}"#,
    ] {
        check_json(good).unwrap_or_else(|e| panic!("rejected good JSON {:?}: {}", good, e));
    }
    for bad in [
        "",
        "{",
        "[1,]",
        "{\"k\":}",
        "01",
        "1.",
        "\"\x01\"",
        r#""\q""#,
        "true false",
        "[1] extra",
    ] {
        assert!(check_json(bad).is_err(), "accepted bad JSON {:?}", bad);
    }
}
