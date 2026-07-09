//! Deterministic text rendering of run statistics.
//!
//! Extracted from `main.rs` so the CLI's stdout and the golden tests share
//! one renderer — the golden files lock this output byte-for-byte.

use crate::causal::CausalRun;
use crate::system::MultiwaySystem;
use std::fmt::Write;

/// The stats block the CLI prints for a run (without the trailing
/// `wrote <path>` lines, which depend on filesystem paths).
pub fn stats_text(
    rule_text: &str,
    init_text: &str,
    mw: &MultiwaySystem,
    causal: Option<&CausalRun>,
) -> String {
    let mut out = String::new();
    writeln!(out, "rule   {}", rule_text).unwrap();
    writeln!(out, "init   {}", init_text).unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "{:>5}  {:>16}  {:>10}  {:>9}",
        "step", "tree nodes", "canonical", "sharing"
    )
    .unwrap();
    for (step, paths, canon) in mw.sharing_per_layer() {
        let ratio = if canon > 0 {
            format!("{:.1}x", paths as f64 / canon as f64)
        } else {
            "-".to_string()
        };
        writeln!(
            out,
            "{:>5}  {:>16}  {:>10}  {:>9}",
            step, paths, canon, ratio
        )
        .unwrap();
    }
    writeln!(out).unwrap();
    writeln!(
        out,
        "canonical states {}   events {}   branchial pairs {}   back-merges {}",
        mw.states.len(),
        mw.events.len(),
        mw.branchial.len(),
        mw.back_merges
    )
    .unwrap();
    if let Some(c) = causal {
        writeln!(
            out,
            "causal run: {} events, {} causal edges, final state {} edges",
            c.n_events,
            c.deps.len(),
            c.final_state.edge_count()
        )
        .unwrap();
    }
    out
}
