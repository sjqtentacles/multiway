//! Deterministic text rendering of run statistics.
//!
//! Extracted from `main.rs` so the CLI's stdout and the golden tests share
//! one renderer — the golden files lock this output byte-for-byte. The
//! table primitives live in [`crate::stats`].

use crate::causal::CausalRun;
use crate::stats::{group_digits, render_summary, render_table};
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
    out.push_str(&render_table(mw));
    writeln!(out).unwrap();
    out.push_str(&render_summary(mw));
    if let Some(c) = causal {
        let gens = if c.generations.iter().all(|&g| g == 1) {
            String::new()
        } else {
            format!(" in {} generations", c.generations.len())
        };
        writeln!(
            out,
            "causal run: {} events{}, {} causal edges, final state {} edges",
            group_digits((c.n_events) as u128),
            gens,
            group_digits(c.deps.len() as u128),
            group_digits(c.final_state.edge_count() as u128)
        )
        .unwrap();
    }
    out
}
