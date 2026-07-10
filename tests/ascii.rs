//! The --ascii terminal renderer: golden-locked layouts (missing
//! goldens = red; mint with
//! `MULTIWAY_BLESS=1 cargo test --test ascii -- --test-threads=1`),
//! determinism, the honest overflow line, and the back-merge case that
//! must render without panicking.

use multiway::ascii::render_multiway;
use multiway::rule::{parse_rule, parse_state};
use multiway::system::evolve;
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn assert_golden(name: &str, actual: &str) -> bool {
    let path = golden_path(name);
    if std::env::var("MULTIWAY_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, actual).unwrap();
        return true;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; mint with MULTIWAY_BLESS=1 cargo test --test ascii -- --test-threads=1",
            name
        )
    });
    assert_eq!(expected, actual, "golden {} differs", name);
    false
}

fn render(rule: &str, init: &str, steps: usize, width: usize) -> String {
    let rule = parse_rule(rule).unwrap();
    let init = parse_state(init).unwrap();
    render_multiway(&evolve(&rule, init, steps), width)
}

#[test]
fn ascii_goldens() {
    let mut blessed = false;
    // growth at 3 steps: single chain of layers 1/1/2/4
    blessed |= assert_golden(
        "ascii_growth.txt",
        &render("{{x,y}}->{{x,y},{y,z}}", "{{0,0}}", 3, 100),
    );
    // classic at 2 steps: 1/1/3 — the fan-out shape
    blessed |= assert_golden(
        "ascii_classic.txt",
        &render(
            "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
            "{{0,0},{0,0}}",
            2,
            100,
        ),
    );
    // terminating composition: a column vanishes (empty final layer)
    blessed |= assert_golden(
        "ascii_terminating.txt",
        &render("{{x,y},{y,z}}->{{x,z}}", "{{0,1},{1,2}}", 3, 100),
    );
    // reversal back-merges into earlier layers: must render, with the
    // back-merge annotation, without panicking
    blessed |= assert_golden(
        "ascii_reversal.txt",
        &render("{{x,y}}->{{y,x}}", "{{0,1},{1,2}}", 4, 100),
    );
    assert!(
        !blessed,
        "goldens regenerated — rerun without MULTIWAY_BLESS to verify"
    );
}

/// Same system twice ⇒ identical bytes.
#[test]
fn ascii_deterministic() {
    let a = render(
        "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
        "{{0,0},{0,0}}",
        3,
        90,
    );
    let b = render(
        "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
        "{{0,0},{0,0}}",
        3,
        90,
    );
    assert_eq!(a, b);
}

/// Width overflow is HONEST: dropped columns are announced, never
/// silently truncated, and no emitted line exceeds the budget.
#[test]
fn ascii_overflow_announced() {
    let narrow = render(
        "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
        "{{0,0},{0,0}}",
        4,
        40,
    );
    assert!(
        narrow.contains("more step"),
        "dropped columns must be announced: {}",
        narrow
    );
    for line in narrow.lines() {
        assert!(
            line.chars().count() <= 40,
            "line exceeds width budget: {:?}",
            line
        );
    }
    // tall layers get the per-column cap annotation
    let short = render(
        "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}",
        "{{0,0},{0,0}}",
        4,
        200,
    );
    assert!(
        short.contains("more state"),
        "capped rows must be announced: {}",
        short
    );
}

/// The reversal fixture really does back-merge (teeth check for the
/// golden above).
#[test]
fn ascii_reversal_fixture_back_merges() {
    let rule = parse_rule("{{x,y}}->{{y,x}}").unwrap();
    let init = parse_state("{{0,1},{1,2}}").unwrap();
    let mw = evolve(&rule, init, 4);
    assert!(mw.back_merges > 0);
    let out = render_multiway(&mw, 100);
    assert!(
        out.contains("back-merge"),
        "back-merges must be annotated: {}",
        out
    );
}
