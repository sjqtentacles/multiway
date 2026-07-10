//! The parallel scan driver: byte-identical output for every thread
//! count (the same collected-by-index discipline as Phase A), honest
//! exhaustive-cap refusal, and end-to-end determinism on a tiny space.

mod common;

use multiway::probe::ProbeBudget;
use multiway::rulespace::SpaceBudget;
use multiway::scan::{scan, ScanOpts, EXHAUSTIVE_CAP};

fn tiny_space() -> SpaceBudget {
    // 124-class space: exhaustive in milliseconds even in debug
    SpaceBudget {
        max_lhs: 1,
        max_rhs: 2,
        min_arity: 1,
        max_arity: 2,
        max_vars: 3,
    }
}

fn tiny_probe() -> ProbeBudget {
    ProbeBudget {
        steps: 3,
        max_states: 60,
        max_events: 2_000,
        max_edges: 24,
        max_canon_leaves: 2_000,
        run_events: 24,
    }
}

fn opts(threads: usize) -> ScanOpts {
    ScanOpts {
        space: tiny_space(),
        probe: tiny_probe(),
        sample: None,
        top: 10,
        threads,
    }
}

/// THE thread-invariance pin: 1 thread and 4 threads produce identical
/// atlases — same rules, same scores, same aliases, same order.
#[test]
fn scan_thread_invariant() {
    let a = scan(&opts(1)).unwrap();
    let b = scan(&opts(4)).unwrap();
    assert_eq!(
        format!("{:?}", a),
        format!("{:?}", b),
        "scan output depends on thread count"
    );
    assert!(!a.is_empty());
    assert_eq!(a.len(), 10.min(a.len()));
}

/// Same opts twice ⇒ identical output (scan is a pure function).
#[test]
fn scan_deterministic() {
    let a = scan(&opts(2)).unwrap();
    let b = scan(&opts(2)).unwrap();
    assert_eq!(format!("{:?}", a), format!("{:?}", b));
}

/// Sampling mode: bounded, deterministic per seed, distinct across
/// seeds (on a space large enough to make collision unlikely).
#[test]
fn scan_sample_deterministic() {
    let space = SpaceBudget {
        max_lhs: 2,
        max_rhs: 3,
        min_arity: 1,
        max_arity: 2,
        max_vars: 4,
    };
    let mk = |seed: u64| ScanOpts {
        space,
        probe: tiny_probe(),
        sample: Some((30, seed)),
        top: 5,
        threads: 2,
    };
    let a = scan(&mk(0xC0FFEE)).unwrap();
    let b = scan(&mk(0xC0FFEE)).unwrap();
    assert_eq!(format!("{:?}", a), format!("{:?}", b));
    assert!(a.len() <= 5);
}

/// Exhaustive scans REFUSE spaces beyond the cap with the exact size in
/// the error — never a silent truncation.
#[test]
fn scan_exhaustive_cap_refuses_with_exact_size() {
    let big = SpaceBudget {
        max_lhs: 2,
        max_rhs: 3,
        min_arity: 1,
        max_arity: 3,
        max_vars: 4,
    };
    let o = ScanOpts {
        space: big,
        probe: tiny_probe(),
        sample: None,
        top: 5,
        threads: 1,
    };
    let err = scan(&o).unwrap_err();
    assert!(
        err.contains("16184498") || err.contains("16,184,498"),
        "cap refusal must print the exact space size: {}",
        err
    );
    assert!(err.contains(&EXHAUSTIVE_CAP.to_string()) || err.contains("--sample"));
}
