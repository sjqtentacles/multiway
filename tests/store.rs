//! EdgeStore: hash-consing correctness and — the load-bearing part —
//! slot identity through interned canonical forms.

mod common;

use common::gen::{gen_state, StateCfg};
use common::harness::prop;
use multiway::canon::canonicalize;
use multiway::rule::{parse_rule, parse_state};
use multiway::store::EdgeStore;
use multiway::system::evolve;

const SEED: u64 = 0x00C0_FFEE_0000_0007;

/// Ids are dense, first-intern-ordered, and stable across re-interns.
#[test]
fn store_intern_id_stability() {
    let mut s = EdgeStore::default();
    let a: Vec<u32> = vec![0, 1];
    let b: Vec<u32> = vec![1, 2];
    let c: Vec<u32> = vec![0, 1, 2];
    assert_eq!(s.intern(&a), 0);
    assert_eq!(s.intern(&b), 1);
    assert_eq!(s.intern(&a), 0, "re-intern must return the original id");
    assert_eq!(s.intern(&c), 2);
    assert_eq!(s.intern(&b), 1);
    assert_eq!(s.len(), 3);
    assert_eq!(s.resolve(0), &a);
    assert_eq!(s.resolve(1), &b);
    assert_eq!(s.resolve(2), &c);
}

/// Interned-key equality ⟺ form equality (the dedup key's contract):
/// interning two canonical forms into ONE store yields equal id vectors
/// exactly when the forms are byte-equal.
#[test]
fn prop_form_ids_key_equality_iff_form_equality() {
    prop(
        SEED,
        "prop_form_ids_key_equality_iff_form_equality",
        |rng, _| {
            let a = canonicalize(&gen_state(rng, &StateCfg::oracle())).form;
            let b = canonicalize(&gen_state(rng, &StateCfg::oracle())).form;
            let mut store = EdgeStore::default();
            let ka: Vec<u32> = a.edges.iter().map(|e| store.intern(e)).collect();
            let kb: Vec<u32> = b.edges.iter().map(|e| store.intern(e)).collect();
            assert_eq!(
                ka == kb,
                a.edges == b.edges,
                "key equality diverged from form equality"
            );
        },
    );
}

/// THE slot-identity pin, on the CLASSIC rule at 3 steps — deliberately:
/// growth-rule interning order coincidentally matches (len, seq) form
/// order and would mask a numeric-sort bug; the classic run is the first
/// place a form holds both a later-interned-but-smaller edge (e.g.
/// [0,3]) and an earlier-interned-but-larger one (e.g. [1,2]). Every
/// state's form_ids must resolve back to its canonical form's edges IN
/// FORM ORDER — never in numeric id order.
#[test]
fn classic_form_ids_preserve_slot_order() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 3);

    let mut nonmonotone_ids = 0usize;
    for s in &mw.states {
        let form = multiway::canon::canonical_form(&s.state);
        assert_eq!(
            s.form_ids.len(),
            form.edges.len(),
            "state {}: form_ids length mismatch",
            s.id
        );
        for (slot, id) in s.form_ids.iter().enumerate() {
            assert_eq!(
                mw.store.resolve(*id),
                &form.edges[slot],
                "state {}: slot {} does not resolve to the form edge",
                s.id,
                slot
            );
        }
        if s.form_ids.windows(2).any(|w| w[0] > w[1]) {
            nonmonotone_ids += 1;
        }
    }
    // the trap is real only if some state's ids are NOT numerically
    // sorted — otherwise a sorted-ids bug would be invisible here
    assert!(
        nonmonotone_ids > 0,
        "test lost its teeth: every form's ids happen to be sorted"
    );
}
