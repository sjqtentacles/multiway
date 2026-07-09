//! Terminal rendering: box-drawing table, digit grouping, log sparkline,
//! and — the load-bearing part — back-merge-aware output: when
//! `back_merges > 0` the tree-node/sharing columns are MEANINGLESS (walk
//! counts in the merged DAG; the reversal rule reports the initial state
//! as 2 "tree nodes") and must be suppressed with an explicit caveat.

mod common;

use multiway::report::stats_text;
use multiway::rule::{parse_rule, parse_state};
use multiway::stats::{group_digits, sparkline};
use multiway::system::evolve;

/// Exponential growth must be log-scaled: linear scaling of the classic
/// layer sizes would render "▁▁▁▁▁█".
#[test]
fn sparkline_log_scales_exponential_growth() {
    assert_eq!(sparkline(&[1, 1, 3, 18, 156, 1776]), "▁▁▂▄▆█");
    assert_ne!(sparkline(&[1, 1, 3, 18, 156, 1776]), "▁▁▁▁▁█");
}

/// Rounding rule (documented): round-to-nearest after log normalization —
/// floor would give "▁▁▂▃▅█" for the classic series.
#[test]
fn sparkline_degenerate_inputs() {
    assert_eq!(sparkline(&[]), "");
    assert_eq!(sparkline(&[5, 5, 5]), "▁▁▁");
    assert_eq!(sparkline(&[0]), "▁");
    assert_eq!(sparkline(&[0, 8]), "▁█");
}

#[test]
fn group_digits_thousands() {
    assert_eq!(group_digits(0), "0");
    assert_eq!(group_digits(999), "999");
    assert_eq!(group_digits(1000), "1,000");
    assert_eq!(group_digits(280080), "280,080");
    assert_eq!(group_digits(1234567890), "1,234,567,890");
}

/// The full table: box drawing, grouped digits, ratio column, sparkline.
#[test]
fn table_contains_box_drawing_and_ratios() {
    let rule = parse_rule("{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}").unwrap();
    let init = parse_state("{{0,0},{0,0}}").unwrap();
    let mw = evolve(&rule, init, 4);
    let out = stats_text(&rule.text, "{{0,0},{0,0}}", &mw, None);

    for needle in ["┌", "│", "└", "step", "tree nodes", "canonical", "sharing"] {
        assert!(out.contains(needle), "missing {:?} in:\n{}", needle, out);
    }
    assert!(out.contains("9,504"), "grouped digits missing:\n{}", out);
    assert!(out.contains("60.9×"), "ratio missing:\n{}", out);
    assert!(out.contains("growth"), "sparkline line missing:\n{}", out);
}

/// Correction 8 (the previously uncovered branch): with back-merges the
/// per-layer naive comparison is not meaningful — tree-node and sharing
/// columns must disappear, replaced by an explicit caveat.
#[test]
fn back_merges_suppress_tree_columns_with_caveat() {
    let rule = parse_rule("{{x,y}}->{{y,x}}").unwrap();
    let init = parse_state("{{0,1},{1,2}}").unwrap();
    let mw = evolve(&rule, init, 3);
    assert_eq!(mw.back_merges, 4, "precondition: this rule back-merges");

    let out = stats_text(&rule.text, "{{0,1},{1,2}}", &mw, None);
    assert!(
        !out.contains("tree nodes") && !out.contains("sharing"),
        "meaningless columns rendered despite back-merges:\n{}",
        out
    );
    assert!(
        out.contains("back-merges 4") && out.contains("not meaningful"),
        "caveat missing:\n{}",
        out
    );
}
