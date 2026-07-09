//! Golden-file tests: three known systems' JSON bundles and stats text,
//! locked byte-for-byte, plus a CLI end-to-end run proving the binary adds
//! nothing nondeterministic on top of the library.
//!
//! Regenerate with:
//! `MULTIWAY_BLESS=1 cargo test --test golden -- --test-threads=1`
//! (blessing WRITES then FAILS the test, so a blessing run can never pass
//! CI; single-threaded because tests share golden files).

mod common;

use common::jsonck::check_json;
use multiway::export::bundle_json;
use multiway::report::stats_text;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;
use std::path::PathBuf;

struct GoldenSystem {
    name: &'static str,
    rule: &'static str,
    init: &'static str,
    steps: usize,
    causal_events: usize,
}

const SYSTEMS: [GoldenSystem; 3] = [
    GoldenSystem {
        name: "growth",
        rule: "{{x,y}}->{{x,y},{y,z}}",
        init: "{{0,0}}",
        steps: 4,
        causal_events: 6,
    },
    GoldenSystem {
        name: "classic",
        rule: "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
        init: "{{0,0},{0,0}}",
        steps: 3,
        causal_events: 10,
    },
    GoldenSystem {
        name: "ternary",
        rule: "{{x,y,z}}->{{x,y},{y,z}}",
        init: "{{0,1,2},{0,0,0}}",
        steps: 3,
        causal_events: 0,
    },
];

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// Compare against the committed golden. In bless mode, writes the file
/// and returns `true` — the CALLER fails the test after minting everything,
/// so a blessing run can never pass while still regenerating all files in
/// one pass.
fn assert_golden(name: &str, actual: &str) -> bool {
    let path = golden_path(name);
    if std::env::var("MULTIWAY_BLESS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return true;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; mint with MULTIWAY_BLESS=1 cargo test --test golden -- --test-threads=1",
            name
        )
    });
    if expected != actual {
        for (i, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
            if e != a {
                panic!(
                    "golden {} differs at line {}:\n  expected: {}\n  actual:   {}",
                    name,
                    i + 1,
                    e,
                    a
                );
            }
        }
        panic!(
            "golden {} differs in length: expected {} bytes, actual {}",
            name,
            expected.len(),
            actual.len()
        );
    }
    false
}

fn render(sys: &GoldenSystem) -> (String, String) {
    let rule = parse_rule(sys.rule).unwrap();
    let init = parse_state(sys.init).unwrap();
    let causal = if sys.causal_events > 0 {
        Some(multiway::causal::run(
            &rule,
            init.clone(),
            sys.causal_events,
        ))
    } else {
        None
    };
    let mw = evolve(&rule, init, sys.steps);
    let json = bundle_json(&rule.text, sys.init, &mw, causal.as_ref());
    let txt = stats_text(&rule.text, sys.init, &mw, causal.as_ref());
    (json, txt)
}

#[test]
fn golden_bundles_and_stats() {
    let mut blessed = false;
    for sys in &SYSTEMS {
        let (json, txt) = render(sys);
        check_json(&json)
            .unwrap_or_else(|e| panic!("golden {} bundle is invalid JSON: {}", sys.name, e));
        blessed |= assert_golden(&format!("{}.json", sys.name), &json);
        blessed |= assert_golden(&format!("{}.txt", sys.name), &txt);
    }
    assert!(
        !blessed,
        "goldens regenerated — rerun without MULTIWAY_BLESS to verify"
    );
}

/// End-to-end through the real binary: `--json` output must byte-equal the
/// library-rendered golden, and non-quiet stdout must equal the stats
/// golden plus the `wrote <path>` line (stripped before compare;
/// `\r\n`-normalized for Windows).
#[test]
fn cli_end_to_end_classic() {
    let sys = &SYSTEMS[1];
    let tmp = std::env::temp_dir().join(format!("multiway-golden-{}.json", std::process::id()));
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_multiway"))
        .args([
            "--rule",
            sys.rule,
            "--init",
            sys.init,
            "--steps",
            &sys.steps.to_string(),
            "--causal",
            &sys.causal_events.to_string(),
            "--json",
            tmp.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run multiway binary");
    assert!(out.status.success(), "CLI exited nonzero: {:?}", out);

    let json = std::fs::read_to_string(&tmp).unwrap();
    std::fs::remove_file(&tmp).ok();
    let (expected_json, expected_txt) = render(sys);
    assert_eq!(
        json, expected_json,
        "CLI --json differs from library render"
    );

    let stdout = String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n");
    let stripped: String = stdout
        .lines()
        .filter(|l| !l.starts_with("wrote "))
        .map(|l| format!("{}\n", l))
        .collect();
    assert_eq!(stripped, expected_txt, "CLI stdout differs from stats_text");
}
