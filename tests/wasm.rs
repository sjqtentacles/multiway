//! The wasm module's host-testable core, exercised NATIVELY on every CI
//! cell: `run_json` must byte-equal the committed goldens (byte-identity
//! across the architecture boundary is structural — it calls the exact
//! evolution + export path the CLI uses), the length-prefix codec must
//! round-trip, and every input-shaped failure must come back as an
//! `{"error":...}` JSON, never a panic.

use multiway::wasm::{decode_result, encode_result, run_json};

fn golden(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// run_json == the committed golden bundles, byte for byte. This is the
/// determinism brag's load-bearing test: the SAME function runs inside
/// the playground's .wasm, so native/wasm byte-identity reduces to the
/// engine's own determinism (checked again across the real boundary by
/// the CI node script).
#[test]
fn run_json_matches_goldens() {
    let cases = [
        (
            "growth.json",
            "{{x,y}}->{{x,y},{y,z}}",
            "{{0,0}}",
            4usize,
            6usize,
        ),
        (
            "classic.json",
            "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
            "{{0,0},{0,0}}",
            3,
            10,
        ),
        (
            "ternary.json",
            "{{x,y,z}}->{{x,y},{y,z}}",
            "{{0,1,2},{0,0,0}}",
            3,
            0,
        ),
    ];
    for (name, rule, init, steps, causal) in cases {
        assert_eq!(
            run_json(rule, init, steps, causal, false),
            golden(name),
            "run_json differs from golden {}",
            name
        );
    }
}

/// Parse and validation failures are JSON errors, never panics.
#[test]
fn run_json_error_paths() {
    let bad_rule = run_json("{{x,y}}", "{{0,0}}", 2, 0, false);
    assert!(bad_rule.starts_with("{\"error\":\""), "{}", bad_rule);
    let bad_init = run_json("{{x,y}}->{{y,x}}", "{{0,", 2, 0, false);
    assert!(bad_init.starts_with("{\"error\":\""), "{}", bad_init);
    // the CLI's "nothing to do" corner: steps == 0 && causal == 0
    let nothing = run_json("{{x,y}}->{{y,x}}", "{{0,1}}", 0, 0, false);
    assert!(nothing.starts_with("{\"error\":\""), "{}", nothing);
    // errors are themselves valid JSON: the hostile quote arrives
    // backslash-escaped, so the error value never terminates early
    let quoted = run_json("{{x\"y}}", "{{0}}", 1, 0, false);
    assert!(quoted.starts_with("{\"error\":\""));
    assert!(
        quoted.contains("\\\"y"),
        "quote must arrive escaped: {}",
        quoted
    );
    assert!(quoted.ends_with("\"}"), "error JSON must close: {}", quoted);
}

/// The order flag's WIRING pin: run_json under each flag value must
/// byte-equal the library composition with the corresponding
/// UpdateOrder. (The exported bundle is deliberately order-INVARIANT
/// whenever both orders fire the same event set — `generations`, the
/// field that distinguishes them, is CLI/stats-level and pinned in
/// tests/parallel.rs; empirically every small fixture coincides because
/// sequential's queue-like edge handling is already breadth-first. So a
/// black-box inequality test would be vacuous; equality against the
/// explicit library path is the honest pin.)
#[test]
fn run_json_order_wiring() {
    use multiway::causal::{run_ordered, UpdateOrder};
    use multiway::export::bundle_json;
    use multiway::rule::{parse_rule, parse_state};
    use multiway::system::{evolve_opts, EvolveOpts};
    let (rule_s, init_s) = ("{{x,y}}->{{x,y},{y,z}}", "{{0,0},{1,1}}");
    let rule = parse_rule(rule_s).unwrap();
    for (flag, order) in [
        (false, UpdateOrder::Sequential),
        (true, UpdateOrder::StandardGenerations),
    ] {
        let init = parse_state(init_s).unwrap();
        let mw = evolve_opts(
            &rule,
            init.clone(),
            &EvolveOpts {
                steps: 2,
                threads: 1,
                incremental: true,
            },
        );
        let causal = run_ordered(&rule, init, 6, order);
        let expected = bundle_json(&rule.text, init_s, &mw, Some(&causal));
        assert_eq!(
            run_json(rule_s, init_s, 2, 6, flag),
            expected,
            "flag {} must select {:?}",
            flag,
            causal.generations
        );
    }
}

/// Length-prefix codec: u32 LE length + UTF-8 payload, exact-capacity
/// allocation, round-trips arbitrary content including multibyte.
#[test]
fn codec_round_trip() {
    for s in ["", "x", "{\"a\":1}", "héllo — ünïcode ×", "{{0,0}}"] {
        let boxed = encode_result(s);
        let back = decode_result(&boxed);
        assert_eq!(back, s, "codec must round-trip {:?}", s);
        let n = u32::from_le_bytes([boxed[0], boxed[1], boxed[2], boxed[3]]) as usize;
        assert_eq!(n, s.len(), "length prefix must be the byte length");
        assert_eq!(boxed.len(), 4 + n, "exact capacity — no slack");
    }
}
