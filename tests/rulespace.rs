//! Rule-space enumeration: canonical-form invariance, the pinned class
//! counts, and the two-independent-computations cross-check (Burnside
//! formula vs explicit enumeration — they must agree, so a wrong pinned
//! constant self-detects rather than being silently blessed).

mod common;

use common::gen::{gen_rule_text, RuleCfg};
use common::harness::prop;
use common::prng::Rng;
use multiway::rule::parse_rule;
use multiway::rulespace::{enumerate, sample, space_size, CanonRule, SpaceBudget};

const SEED: u64 = 0x00C0_FFEE_0000_0008;

fn default_scan() -> SpaceBudget {
    SpaceBudget {
        max_lhs: 2,
        max_rhs: 3,
        min_arity: 1,
        max_arity: 2,
        max_vars: 4,
    }
}

/// CanonRule is invariant under variable renaming and per-side edge
/// shuffles — the full behavioral-equivalence quotient.
#[test]
fn prop_canon_rule_invariant_under_renaming_and_edge_perms() {
    prop(
        SEED,
        "prop_canon_rule_invariant_under_renaming_and_edge_perms",
        |rng, _| {
            let text = gen_rule_text(rng, &RuleCfg::default());
            let r = parse_rule(&text).unwrap();
            let canon = CanonRule::from_rule(&r);

            // rebuild the rule text with renamed variables and shuffled
            // edge order on both sides
            let mut names: Vec<String> = (0..r.n_vars).map(|i| format!("w{}", i)).collect();
            shuffle_strings(rng, &mut names);
            let rewrite = |edges: &[Vec<usize>], rng: &mut Rng| -> String {
                let mut es: Vec<String> = edges
                    .iter()
                    .map(|e| {
                        let vs: Vec<&str> = e.iter().map(|&v| names[v].as_str()).collect();
                        format!("{{{}}}", vs.join(","))
                    })
                    .collect();
                shuffle_strings(rng, &mut es);
                format!("{{{}}}", es.join(","))
            };
            let scrambled = format!("{}->{}", rewrite(&r.lhs, rng), rewrite(&r.rhs, rng));
            let r2 = parse_rule(&scrambled).unwrap();
            assert_eq!(
                canon,
                CanonRule::from_rule(&r2),
                "canon not invariant: {} vs {}",
                text,
                scrambled
            );
        },
    );
}

fn shuffle_strings(rng: &mut Rng, xs: &mut [String]) {
    for i in (1..xs.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        xs.swap(i, j);
    }
}

/// Canon is idempotent and its text form parses back to itself.
#[test]
fn prop_canon_rule_idempotent_and_parses() {
    prop(
        SEED ^ 1,
        "prop_canon_rule_idempotent_and_parses",
        |rng, _| {
            let r = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
            let c = CanonRule::from_rule(&r);
            let again = CanonRule::from_rule(&c.to_rule());
            assert_eq!(c, again, "canon not idempotent via text round-trip");
        },
    );
}

/// The pinned class counts. Verified three ways before pinning: a
/// Burnside computation, an explicit enumeration (below), and an
/// independent reimplementation during plan verification. The 16.2M
/// constant is Burnside-only (far past enumerable size); its formula is
/// validated by the smaller budgets agreeing with enumeration.
#[test]
fn rulespace_counts_pinned() {
    let binary_only = SpaceBudget {
        min_arity: 2,
        ..default_scan()
    };
    assert_eq!(space_size(&binary_only), 6_477);
    assert_eq!(space_size(&default_scan()), 18_143);

    let ternary = SpaceBudget {
        max_arity: 3,
        ..default_scan()
    };
    assert_eq!(space_size(&ternary), 16_184_498);
}

/// The two independent computations must agree — on random tiny budgets
/// AND on the default scan space (enumeration is ~10M small compares:
/// fast in release, a few seconds in debug).
#[test]
fn prop_space_size_equals_enumerate_len() {
    prop(
        SEED ^ 2,
        "prop_space_size_equals_enumerate_len",
        |rng, i| {
            if i >= 12 {
                return; // 12 random tiny budgets is plenty per run
            }
            let b = SpaceBudget {
                max_lhs: 1 + (rng.next_u64() % 2) as usize,
                max_rhs: (rng.next_u64() % 3) as usize,
                min_arity: 1,
                max_arity: 1 + (rng.next_u64() % 2) as usize,
                max_vars: 2 + (rng.next_u64() % 2) as usize,
            };
            assert_eq!(
                space_size(&b) as usize,
                enumerate(&b).len(),
                "Burnside disagrees with enumeration on {:?}",
                b
            );
        },
    );
}

/// Full default-space cross-check + structural properties: ascending,
/// unique, every element self-canonical.
#[test]
fn enumerate_default_space_sorted_unique_selfcanonical() {
    let rules = enumerate(&default_scan());
    assert_eq!(rules.len(), 18_143, "enumeration disagrees with Burnside");
    for w in rules.windows(2) {
        assert!(w[0] < w[1], "not strictly ascending");
    }
    // spot-check self-canonicality across the space
    for r in rules.iter().step_by(997) {
        assert_eq!(*r, CanonRule::from_rule(&r.to_rule()));
    }
}

/// Sampling is deterministic and stays within the budget's space.
#[test]
fn sample_deterministic_within_budget() {
    let ternary = SpaceBudget {
        max_arity: 3,
        ..default_scan()
    };
    let a = sample(&ternary, 50, 0xC0FFEE);
    let b = sample(&ternary, 50, 0xC0FFEE);
    assert_eq!(a, b, "same seed must give the same sample");
    assert_eq!(a.len(), 50);
    let other = sample(&ternary, 50, 0xBEEF);
    assert_ne!(a, other, "different seeds should differ");
    for c in &a {
        assert!(c.lhs.len() <= 2 && c.rhs.len() <= 3 && c.n_vars <= 4);
        assert_eq!(*c, CanonRule::from_rule(&c.to_rule()));
    }
}

/// The gallery's flagship rules are IN the spaces they should be in.
#[test]
fn known_rules_present() {
    let rules = enumerate(&default_scan());
    for text in [
        "{{x,y}}->{{x,y},{y,z}}", // growth
        "{{x,y}}->{{y,x}}",       // reversal
        "{{x,y}}->{{x,z},{z,y}}", // subdivision
        "{{x,y},{x,y}}->{{x,y}}", // idempotent merge
        "{{x,y},{y,z}}->{{x,z}}", // composition
    ] {
        let c = CanonRule::from_rule(&parse_rule(text).unwrap());
        assert!(
            rules.binary_search(&c).is_ok(),
            "{} missing from the default scan space",
            text
        );
    }
    // the classic rule needs arity<=2 but 4 vars and RHS 4 edges — NOT in
    // the default space (max_rhs 3); pin that too so nobody "fixes" it
    let classic =
        CanonRule::from_rule(&parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap());
    assert!(rules.binary_search(&classic).is_err());
}
