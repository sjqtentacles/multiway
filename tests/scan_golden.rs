//! Scan output goldens: the terminal atlas table and the scan JSON for
//! the tiny 124-class space, locked byte-for-byte.
//!
//! Regenerate with:
//! `MULTIWAY_BLESS=1 cargo test --test scan_golden -- --test-threads=1`
//! (blessing WRITES then FAILS, so a blessing run can never pass CI).
//!
//! Note: sparkline glyphs are f64/ln DISPLAY (covered by the existing
//! 3-OS golden precedent); the SCORES — the ordering-bearing values —
//! are pure integers, so neither should ever be "fixed" toward the
//! other.

mod common;

use common::jsonck::check_json;
use multiway::probe::ProbeBudget;
use multiway::rulespace::SpaceBudget;
use multiway::scan::{scan, scan_json, scan_text, ScanOpts};
use std::path::PathBuf;

fn tiny_opts() -> ScanOpts {
    ScanOpts {
        space: SpaceBudget {
            max_lhs: 1,
            max_rhs: 2,
            min_arity: 1,
            max_arity: 2,
            max_vars: 3,
        },
        probe: ProbeBudget {
            steps: 3,
            max_states: 60,
            max_events: 2_000,
            max_edges: 24,
            max_canon_leaves: 2_000,
            run_events: 24,
        },
        sample: None,
        top: 10,
        threads: 1,
    }
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn assert_golden(name: &str, actual: &str) -> bool {
    let path = golden_path(name);
    if std::env::var("MULTIWAY_BLESS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return true;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; mint with MULTIWAY_BLESS=1 cargo test --test scan_golden -- --test-threads=1",
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

#[test]
fn golden_scan_tiny() {
    let opts = tiny_opts();
    let entries = scan(&opts).unwrap();
    let txt = scan_text(&opts, &entries);
    let json = scan_json(&opts, &entries);
    check_json(&json).unwrap_or_else(|e| panic!("scan json invalid: {}", e));

    // the header must echo the budgets but NEVER the thread count —
    // output is thread-invariant and the goldens prove it stays so
    assert!(!txt.contains("thread"), "thread count leaked into output");
    assert!(!json.contains("thread"), "thread count leaked into json");
    let mut opts4 = opts;
    opts4.threads = 4;
    let entries4 = scan(&opts4).unwrap();
    assert_eq!(txt, scan_text(&opts4, &entries4));
    assert_eq!(json, scan_json(&opts4, &entries4));

    let mut blessed = false;
    blessed |= assert_golden("scan_tiny.txt", &txt);
    blessed |= assert_golden("scan_tiny.json", &json);
    assert!(
        !blessed,
        "goldens regenerated — rerun without MULTIWAY_BLESS to verify"
    );
}
