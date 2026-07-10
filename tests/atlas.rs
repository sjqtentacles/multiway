//! Atlas scoring + ranking: dedup by fingerprint, all-integer scores
//! pinned as reference values (every weight tweak is a visible diff),
//! and a total order that is invariant under input permutation.

mod common;

use common::harness::prop;
use common::prng::Rng;
use multiway::atlas::{rank, score_tier1, AtlasEntry, ConflClass, FINALIST_FACTOR};
use multiway::probe::{probe, ProbeBudget};
use multiway::rule::parse_rule;
use multiway::rulespace::CanonRule;

const SEED: u64 = 0x00C0_FFEE_0000_000A;

fn probed(text: &str) -> (CanonRule, multiway::probe::ProbeResult) {
    let r = parse_rule(text).unwrap();
    (CanonRule::from_rule(&r), probe(&r, &ProbeBudget::default()))
}

/// Reference scores for the gallery rules. Pinned AFTER first
/// computation (scores are defined by the weight table, not derivable
/// independently) — the red teeth are the ORDER assertions below, which
/// encode the intent the weights must realize: a growing, sharing,
/// surviving rule outranks a cycler, which outranks instant death.
#[test]
fn atlas_reference_scores_pinned() {
    let growth = score_tier1(&probed("{{x,y}}->{{x,y},{y,z}}").1);
    let subdivision = score_tier1(&probed("{{x,y}}->{{x,z},{z,y}}").1);
    let reversal = score_tier1(&probed("{{x,y}}->{{y,x}}").1);
    let composition = score_tier1(&probed("{{x,y},{y,z}}->{{x,z}}").1);

    // the intent (these were RED against a zeroed weight table):
    assert!(
        growth > reversal,
        "survive+growth must beat a pure cycler: {} vs {}",
        growth,
        reversal
    );
    assert!(
        subdivision > composition,
        "max sharing must beat terminating composition: {} vs {}",
        subdivision,
        composition
    );
    assert!(
        reversal > composition,
        "periodicity must beat death: {} vs {}",
        reversal,
        composition
    );

    // the pinned values (weight-table change detector):
    assert_eq!(growth, 5_815);
    assert_eq!(subdivision, 7_035);
    assert_eq!(reversal, 2_125);
    assert_eq!(composition, 500);
}

/// Rules sharing a fingerprint collapse to ONE entry whose
/// representative is the least CanonRule and whose alias count is
/// honest — aliases are counted, never dropped. The aliased pair is
/// constructed directly (two distinct rules, one probe): that IS a
/// fingerprint class, however it arose (equivalence or collision).
#[test]
fn atlas_dedup_by_fingerprint() {
    let a = probed("{{x}}->{{x}}");
    let b = probed("{{x}}->{{y}}");
    assert!(a.0 < b.0, "fixture assumes identity < fresh-replace");
    let shared = a.1.clone();
    let entries = rank(
        vec![(b.0.clone(), shared.clone()), (a.0.clone(), shared)],
        10,
    );
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].aliases, 2, "alias count must be honest");
    assert_eq!(
        &entries[0].rule, &a.0,
        "representative = least rule, not first-seen"
    );
    // exact-duplicate input also collapses with aliases == 2
    let dup = rank(vec![a.clone(), a.clone()], 10);
    assert_eq!(dup.len(), 1);
    assert_eq!(dup[0].aliases, 2);
}

/// Ranking is a pure function: same input twice ⇒ identical output,
/// and the order is total (Reverse(score), then rule text).
#[test]
fn atlas_rank_deterministic_and_totally_ordered() {
    let items: Vec<_> = [
        "{{x,y}}->{{x,y},{y,z}}",
        "{{x,y}}->{{y,x}}",
        "{{x,y},{y,z}}->{{x,z}}",
        "{{x,y}}->{{x,z},{z,y}}",
    ]
    .iter()
    .map(|t| probed(t))
    .collect();
    let a = rank(items.clone(), 10);
    let b = rank(items, 10);
    assert_eq!(format!("{:?}", a), format!("{:?}", b));
    for w in a.windows(2) {
        assert!(
            (w[0].score, w[1].rule.text()) >= (w[1].score, w[0].rule.text()),
            "not ordered by (Reverse(score), text): {} then {}",
            w[0].score,
            w[1].score
        );
        assert!(w[0].score >= w[1].score);
    }
    // finalists got a tier-2 confluence class; FINALIST_FACTOR covers
    // this whole tiny input
    assert!(a.len() <= 10.max(FINALIST_FACTOR));
    assert!(a.iter().take(4).all(|e| e.confluence.is_some()));
}

/// THE ordering-integrity property: shuffling the input permutes
/// nothing in the output — dedup, scoring, and ranking are all
/// input-order-independent.
#[test]
fn prop_rank_stable_under_input_permutation() {
    let mut items: Vec<_> = [
        "{{x,y}}->{{x,y},{y,z}}",
        "{{x,y}}->{{y,x}}",
        "{{x,y},{y,z}}->{{x,z}}",
        "{{x,y}}->{{x,z},{z,y}}",
        "{{x,y},{x,y}}->{{x,y}}",
    ]
    .iter()
    .map(|t| probed(t))
    .collect();
    // an aliased pair (two distinct rules, one probe): shuffling flips
    // which member is seen first, so first-seen-wins representative
    // selection cannot survive this prop
    let id = probed("{{x}}->{{x}}");
    let fresh = probed("{{x}}->{{y}}");
    items.push((id.0, fresh.1.clone()));
    items.push((fresh.0, fresh.1));
    let reference = format!("{:?}", rank(items.clone(), 10));
    prop(
        SEED,
        "prop_rank_stable_under_input_permutation",
        |rng, i| {
            if i >= 16 {
                return;
            }
            let mut shuffled = items.clone();
            shuffle(rng, &mut shuffled);
            assert_eq!(
                format!("{:?}", rank(shuffled, 10)),
                reference,
                "rank depends on input order"
            );
        },
    );
}

fn shuffle<T>(rng: &mut Rng, xs: &mut [T]) {
    for i in (1..xs.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        xs.swap(i, j);
    }
}

/// Tier-2 confluence classes on known rules: composition alone is
/// strongly joinable; the classes are a closed enum the atlas can
/// render without surprises.
#[test]
fn atlas_confluence_classes() {
    let entries = rank(vec![probed("{{x,y},{y,z}}->{{x,z}}")], 5);
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0].confluence,
        Some(ConflClass::Confluent) | Some(ConflClass::Inconclusive)
    ));
    let _: &AtlasEntry = &entries[0];
}
