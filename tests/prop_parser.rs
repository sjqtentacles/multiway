//! Parser round-trips, error paths, and fuzz-ish no-panic coverage.

mod common;

use common::gen::{gen_garbage, gen_rule_text, gen_state, mutate_string, RuleCfg, StateCfg};
use common::harness::prop;
use multiway::rule::{parse_rule, parse_state};

const SEED: u64 = 0x00C0_FFEE_0000_0002;

/// `parse ∘ to_notation` is the identity on edge lists.
#[test]
fn prop_state_notation_round_trip() {
    prop(SEED, "prop_state_notation_round_trip", |rng, _| {
        let s = gen_state(rng, &StateCfg::wide());
        let text = s.to_notation();
        let back = parse_state(&text)
            .unwrap_or_else(|e| panic!("printer produced unparseable text {:?}: {}", text, e));
        assert_eq!(
            back.edges, s.edges,
            "round trip changed edges via {:?}",
            text
        );
    });
    // empty state prints and parses
    assert_eq!(parse_state("{}").unwrap().to_notation(), "{}");
}

/// `parse ∘ to_notation` is a fixed point on rules (structure and a second
/// print both stabilize).
#[test]
fn prop_rule_notation_fixed_point() {
    prop(SEED ^ 1, "prop_rule_notation_fixed_point", |rng, _| {
        let r = parse_rule(&gen_rule_text(rng, &RuleCfg::default())).unwrap();
        let text = r.to_notation();
        let r2 = parse_rule(&text)
            .unwrap_or_else(|e| panic!("printer produced unparseable rule {:?}: {}", text, e));
        assert_eq!(r2.lhs, r.lhs);
        assert_eq!(r2.rhs, r.rhs);
        assert_eq!(r2.n_vars, r.n_vars);
        assert_eq!(r2.to_notation(), text, "second print not a fixed point");
    });
}

/// Every malformed input is an `Err` with a non-empty message — never a
/// panic, never a silent `Ok`.
#[test]
fn parser_error_paths() {
    let bad_states = [
        "{{0,1}",      // unbalanced
        "",            // empty input
        "{",           // lone brace
        "{{0,1}}rest", // trailing garbage
        "{{a,b}}",     // non-integer vertices
        "{{{{{{",      // deep nesting garbage
        "{{0,1},}",    // trailing comma
    ];
    for s in bad_states {
        let r = parse_state(s);
        assert!(r.is_err(), "parse_state accepted {:?}", s);
        assert!(!r.unwrap_err().is_empty());
    }

    let bad_rules = [
        "{{x,y}}",         // no arrow
        "{}->{{x}}",       // empty LHS is rejected
        "{{x,y}}->{{y}}z", // trailing garbage
        "{{α,β}}->{{β}}",  // non-ASCII identifiers: Err, not panic
        "->{{x}}",         // missing LHS entirely
        "{{x,y}}->",       // missing RHS list
    ];
    for s in bad_rules {
        let r = parse_rule(s);
        assert!(r.is_err(), "parse_rule accepted {:?}", s);
        assert!(!r.unwrap_err().is_empty());
    }

    // and the legal-but-easy-to-forget cases stay legal:
    assert!(parse_rule("{{x,y}}->{}").is_ok(), "empty RHS is legal");
    assert!(parse_state("{}").is_ok(), "empty state is legal");
}

/// Arity-0 edges are reachable syntax (`{{}}` = one empty edge) and must
/// survive multiset semantics: two empty edges are two distinct instances.
#[test]
fn state_arity0_edges() {
    let one = parse_state("{{}}").unwrap();
    assert_eq!(one.edges, vec![Vec::<u32>::new()]);
    assert_eq!(one.vertices(), Vec::<u32>::new());

    let two = parse_state("{{},{}}").unwrap();
    assert_eq!(two.edge_count(), 2);
    assert_eq!(two.to_notation(), "{{},{}}");
    assert_eq!(parse_state(&two.to_notation()).unwrap().edges, two.edges);

    use multiway::canon::{isomorphic, wl_hash};
    assert!(!isomorphic(&one, &two));
    assert_ne!(wl_hash(&one), wl_hash(&two));
}

/// The parser must never panic: random garbage over a parser-stressing
/// alphabet, plus one-char mutations of valid texts. The assertion is that
/// the call completes (Ok or Err); CaseGuard turns any panic into a seeded
/// repro line.
#[test]
fn prop_parser_never_panics_fuzzish() {
    prop(SEED ^ 2, "prop_parser_never_panics_fuzzish", |rng, _| {
        for _ in 0..5 {
            let garbage = gen_garbage(rng, 60);
            let _ = parse_rule(&garbage);
            let _ = parse_state(&garbage);

            let rule_text = gen_rule_text(rng, &RuleCfg::default());
            let mutated = mutate_string(rng, &rule_text);
            let _ = parse_rule(&mutated);

            let state_text = gen_state(rng, &StateCfg::oracle()).to_notation();
            let mutated = mutate_string(rng, &state_text);
            let _ = parse_state(&mutated);
        }
    });
}
