//! Terminal-rendering primitives: digit grouping, log-scaled sparklines,
//! and the box-drawing stats table. Pure `String` functions — the golden
//! tests lock the CLI output byte-for-byte through these.

use crate::system::MultiwaySystem;
use std::fmt::Write;

/// Thousands separators: `280080` → `"280,080"`.
pub fn group_digits(n: u128) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let offset = digits.len() % 3;
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (i + 3 - offset) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Log-scaled sparkline (layer growth is exponential — linear scaling
/// would render everything but the last value as `▁`). Zeros are treated
/// as 1 for the logarithm; bucket selection is **round-to-nearest** after
/// normalization (floor would give `▁▁▂▃▅█` for the classic series
/// instead of the pinned `▁▁▂▄▆█`).
pub fn sparkline(values: &[u128]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let logs: Vec<f64> = values.iter().map(|&v| (v.max(1) as f64).ln()).collect();
    let (min, max) = logs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    logs.iter()
        .map(|&v| {
            let bucket = if max > min {
                ((v - min) / (max - min) * 7.0).round() as usize
            } else {
                0
            };
            BARS[bucket.min(7)]
        })
        .collect()
}

/// The per-layer stats table. With `back_merges == 0` it shows naive-tree
/// node counts and the sharing ratio; with back-merges those columns are
/// **suppressed** — the path-count DP counts walks in the merged DAG and
/// can over- or under-state naive counts, so printing them would be
/// publishing wrong numbers — and an explicit caveat line is added.
pub fn render_table(mw: &MultiwaySystem) -> String {
    let sharing = mw.sharing_per_layer();
    let mut out = String::new();
    if mw.back_merges == 0 {
        writeln!(out, "┌───────┬──────────────────┬────────────┬──────────┐").unwrap();
        writeln!(out, "│  step │       tree nodes │  canonical │  sharing │").unwrap();
        writeln!(out, "├───────┼──────────────────┼────────────┼──────────┤").unwrap();
        for (step, paths, canon) in &sharing {
            let ratio = if *canon > 0 {
                format!("{:.1}×", *paths as f64 / *canon as f64)
            } else {
                "-".to_string()
            };
            writeln!(
                out,
                "│ {:>5} │ {:>16} │ {:>10} │ {:>8} │",
                step,
                group_digits(*paths),
                group_digits(*canon as u128),
                ratio
            )
            .unwrap();
        }
        writeln!(out, "└───────┴──────────────────┴────────────┴──────────┘").unwrap();
    } else {
        writeln!(out, "┌───────┬────────────┐").unwrap();
        writeln!(out, "│  step │  canonical │").unwrap();
        writeln!(out, "├───────┼────────────┤").unwrap();
        for (step, _, canon) in &sharing {
            writeln!(
                out,
                "│ {:>5} │ {:>10} │",
                step,
                group_digits(*canon as u128)
            )
            .unwrap();
        }
        writeln!(out, "└───────┴────────────┘").unwrap();
        writeln!(
            out,
            "note: back-merges {} — states recur across steps, so naive-tree",
            mw.back_merges
        )
        .unwrap();
        writeln!(out, "      node counts are not meaningful per layer").unwrap();
    }
    let growth: Vec<u128> = sharing.iter().map(|(_, _, c)| *c as u128).collect();
    writeln!(out, "growth {}", sparkline(&growth)).unwrap();
    out
}

/// The one-line run summary with grouped digits.
pub fn render_summary(mw: &MultiwaySystem) -> String {
    format!(
        "canonical states {}   events {}   branchial pairs {}   back-merges {}\n",
        group_digits(mw.states.len() as u128),
        group_digits(mw.events.len() as u128),
        group_digits(mw.branchial.len() as u128),
        mw.back_merges
    )
}
