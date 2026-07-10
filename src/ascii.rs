//! Terminal renderer for the multiway DAG: columns are steps, rows are
//! states within a layer, events draw as `─ ╲ ╱` connectors across the
//! column gaps. Budgeted honestly — dropped columns and capped rows are
//! announced (`… +N more`), never silently truncated. Back-merge events
//! (into an earlier layer) are counted in a footer annotation instead
//! of drawn: an arrow going BACKWARD through already-rendered columns
//! would cross every gap and read as noise.
//!
//! Pure function of the system — golden-locked byte-for-byte.

use crate::stats::group_digits;
use crate::system::MultiwaySystem;

/// Per-column cap on rendered states; taller layers are cut with an
/// honest `… +N more states` cell.
const MAX_ROWS: usize = 12;

/// Render the multiway DAG as box-drawing text no wider than `width`
/// columns (minimum 20 — narrower budgets are clamped up).
pub fn render_multiway(mw: &MultiwaySystem, width: usize) -> String {
    let width = width.max(20);
    let n_layers = mw.layers.len();

    // rendered row of each state (row index within its column), None
    // when cut by MAX_ROWS
    let mut row_of: Vec<Option<usize>> = vec![None; mw.states.len()];
    for layer in &mw.layers {
        for (i, &sid) in layer.iter().enumerate().take(MAX_ROWS) {
            row_of[sid] = Some(i);
        }
    }

    // column text width: '●' + id digits, sized per layer
    let col_w: Vec<usize> = mw
        .layers
        .iter()
        .map(|l| {
            1 + l
                .iter()
                .take(MAX_ROWS)
                .map(|&sid| sid.to_string().len())
                .max()
                .unwrap_or(1)
        })
        .collect();

    // gap width before each column c (from c-1): wide enough for a pure
    // diagonal to cover the worst row delta, floor 3
    let mut gap_w: Vec<usize> = vec![0; n_layers];
    for e in &mw.events {
        if mw.states[e.to].step != e.step {
            continue; // back-merge: annotated, not drawn
        }
        if let (Some(a), Some(b)) = (row_of[e.from], row_of[e.to]) {
            let d = a.abs_diff(b) * 2; // rows are 2 canvas lines apart
            let g = &mut gap_w[e.step];
            *g = (*g).max(d + 1);
        }
    }
    for c in 1..n_layers {
        // floor 3, and wide enough that the previous column's "step N"
        // caption never runs into this column's caption
        let caption_room = (format!("step {}", c - 1).len() + 1).saturating_sub(col_w[c - 1]);
        gap_w[c] = gap_w[c].max(3).max(caption_room);
    }

    // how many columns fit the width budget
    let mut x_of: Vec<usize> = vec![0; n_layers];
    let mut shown_cols = 0;
    let mut x = 0usize;
    for c in 0..n_layers {
        x += gap_w[c];
        // caption "step N" may exceed the node width — reserve the max
        let need = col_w[c].max(format!("step {}", c).len());
        if x + need > width {
            break;
        }
        x_of[c] = x;
        x += col_w[c];
        shown_cols = c + 1;
    }

    let rows_shown: usize = mw
        .layers
        .iter()
        .take(shown_cols)
        .map(|l| l.len().min(MAX_ROWS))
        .max()
        .unwrap_or(0);
    // canvas: caption line + 2 lines per row + one annotation line per
    // column that overflows MAX_ROWS
    let any_cut = mw
        .layers
        .iter()
        .take(shown_cols)
        .any(|l| l.len() > MAX_ROWS);
    let h = 1 + rows_shown * 2 + if any_cut { 1 } else { 0 };
    let mut canvas: Vec<Vec<char>> = vec![vec![' '; width]; h.max(2)];

    let put = |canvas: &mut Vec<Vec<char>>, x: usize, y: usize, ch: char| {
        if y < canvas.len() && x < width {
            let cell = canvas[y][x];
            canvas[y][x] = match (cell, ch) {
                (' ', c) => c,
                (a, b) if a == b => a,
                // two different connector chars crossing
                ('─', '╲') | ('─', '╱') | ('╲', '─') | ('╱', '─') | ('╲', '╱') | ('╱', '╲') => {
                    '┼'
                }
                (a, _) => a, // nodes and captions win
            };
        }
    };

    // captions + nodes
    for (c, &cx) in x_of.iter().enumerate().take(shown_cols) {
        for (k, ch) in format!("step {}", c).chars().enumerate() {
            put(&mut canvas, cx + k, 0, ch);
        }
        let layer = &mw.layers[c];
        for (i, &sid) in layer.iter().enumerate().take(MAX_ROWS) {
            let y = 1 + i * 2;
            put(&mut canvas, cx, y, '●');
            for (k, ch) in sid.to_string().chars().enumerate() {
                put(&mut canvas, cx + 1 + k, y, ch);
            }
        }
        if layer.len() > MAX_ROWS {
            let note = format!("… +{} more states", layer.len() - MAX_ROWS);
            let y = 1 + MAX_ROWS * 2;
            for (k, ch) in note.chars().enumerate() {
                put(&mut canvas, cx + k, y, ch);
            }
        }
    }

    // connectors: diagonal to cover the row delta, then horizontal
    let mut back_merges = 0usize;
    let mut off_canvas = 0usize;
    for e in &mw.events {
        if mw.states[e.to].step != e.step {
            back_merges += 1;
            continue;
        }
        if e.step >= shown_cols {
            continue; // beyond the width budget (announced below)
        }
        let (a, b) = match (row_of[e.from], row_of[e.to]) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                off_canvas += 1;
                continue;
            }
        };
        let c = e.step;
        let x0 = x_of[c - 1] + col_w[c - 1];
        let x1 = x_of[c];
        let (mut y, y1) = (1 + a * 2, 1 + b * 2);
        let mut x = x0;
        while x < x1 && y != y1 {
            if y < y1 {
                put(&mut canvas, x, y + 1, '╲');
                y += 2;
            } else {
                put(&mut canvas, x, y - 1, '╱');
                y -= 2;
            }
            x += 1;
        }
        while x < x1 {
            put(&mut canvas, x, y, '─');
            x += 1;
        }
    }

    let mut out = String::new();
    for line in &canvas {
        let s: String = line.iter().collect();
        out.push_str(s.trim_end());
        out.push('\n');
    }
    if shown_cols < n_layers {
        out.push_str(&format!(
            "… +{} more steps (width {})\n",
            n_layers - shown_cols,
            width
        ));
    }
    if back_merges > 0 {
        out.push_str(&format!(
            "↩ {} back-merges into earlier layers (not drawn)\n",
            group_digits(back_merges as u128)
        ));
    }
    if off_canvas > 0 {
        out.push_str(&format!(
            "· {} events into row-capped states (not drawn)\n",
            group_digits(off_canvas as u128)
        ));
    }
    out
}
