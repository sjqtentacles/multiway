//! Sub-hypergraph pattern matching and rule application.

use crate::hypergraph::{State, Vertex};
use crate::rule::Rule;

/// A single match of a rule's LHS: which edge instances were consumed
/// (parallel to `rule.lhs`), and what each variable bound to. RHS-only
/// variables remain `None` until application mints fresh vertices.
#[derive(Clone, Debug)]
pub struct Match {
    /// Consumed edge instances, parallel to `rule.lhs`.
    pub edge_idx: Vec<usize>,
    /// Per-variable binding; RHS-only variables stay `None`.
    pub binding: Vec<Option<Vertex>>,
}

/// Enumerate every match of `rule`'s LHS in `state`.
///
/// Distinct pattern variables may bind the same vertex; each pattern edge
/// must map to a distinct edge instance. Enumeration order is
/// deterministic (edge index order), which the causal runner relies on.
pub fn find_matches(state: &State, rule: &Rule) -> Vec<Match> {
    let mut out = Vec::new();
    let mut used = vec![false; state.edges.len()];
    let mut binding: Vec<Option<Vertex>> = vec![None; rule.n_vars];
    let mut chosen: Vec<usize> = Vec::with_capacity(rule.lhs.len());
    rec(
        state,
        rule,
        0,
        &mut used,
        &mut binding,
        &mut chosen,
        &mut out,
    );
    out
}

fn rec(
    state: &State,
    rule: &Rule,
    k: usize,
    used: &mut [bool],
    binding: &mut [Option<Vertex>],
    chosen: &mut Vec<usize>,
    out: &mut Vec<Match>,
) {
    if k == rule.lhs.len() {
        out.push(Match {
            edge_idx: chosen.clone(),
            binding: binding.to_vec(),
        });
        return;
    }
    let pat = &rule.lhs[k];
    for ei in 0..state.edges.len() {
        if used[ei] || state.edges[ei].len() != pat.len() {
            continue;
        }
        let mut added: Vec<usize> = Vec::new();
        let mut ok = true;
        for (p, v) in pat.iter().zip(state.edges[ei].iter()) {
            match binding[*p] {
                Some(x) if x != *v => {
                    ok = false;
                    break;
                }
                Some(_) => {}
                None => {
                    binding[*p] = Some(*v);
                    added.push(*p);
                }
            }
        }
        if ok {
            used[ei] = true;
            chosen.push(ei);
            rec(state, rule, k + 1, used, binding, chosen, out);
            chosen.pop();
            used[ei] = false;
        }
        for p in added {
            binding[p] = None;
        }
    }
}

/// The result of applying a match, with the layout information the
/// token-event graph and the incremental matcher need.
pub struct Application {
    /// The rewritten state: kept edges first (parent order), then the RHS
    /// edges in rule order.
    pub child: State,
    /// `(parent_idx, child_idx)` for every edge that survived, in parent
    /// order.
    pub kept: Vec<(usize, usize)>,
    /// Child indices of the freshly produced RHS edges.
    pub produced: std::ops::Range<usize>,
}

/// Apply a match: remove consumed edge instances, append RHS edges with
/// bound variables, minting fresh vertices for RHS-only variables in
/// deterministic order. Returns the full layout.
pub fn apply_full(state: &State, rule: &Rule, m: &Match) -> Application {
    let mut mask = vec![true; state.edges.len()];
    for &i in &m.edge_idx {
        mask[i] = false;
    }
    let mut edges: Vec<Vec<Vertex>> = Vec::with_capacity(state.edges.len() + rule.rhs.len());
    let mut kept: Vec<(usize, usize)> = Vec::with_capacity(state.edges.len());
    for (i, e) in state.edges.iter().enumerate() {
        if mask[i] {
            kept.push((i, edges.len()));
            edges.push(e.clone());
        }
    }
    let produced_start = edges.len();
    let mut binding = m.binding.clone();
    let mut next = state.next_vertex;
    for pe in &rule.rhs {
        let e: Vec<Vertex> = pe
            .iter()
            .map(|&v| {
                if binding[v].is_none() {
                    binding[v] = Some(next);
                    next += 1;
                }
                binding[v].unwrap()
            })
            .collect();
        edges.push(e);
    }
    let produced = produced_start..edges.len();
    Application {
        child: State {
            edges,
            next_vertex: next,
        },
        kept,
        produced,
    }
}

/// Apply a match, returning only the rewritten state.
pub fn apply(state: &State, rule: &Rule, m: &Match) -> State {
    apply_full(state, rule, m).child
}

/// Incrementally maintain a match set across one application.
///
/// The child's matches are exactly:
/// - **survivors**: parent matches whose consumed edges are disjoint from
///   `m`'s (a match using only kept edges IS a surviving parent match —
///   `apply` never renames vertices and bindings derive only from a
///   match's own edges), with edge indices remapped through `app.kept`;
/// - **new matches**: every match using ≥ 1 produced edge, generated
///   exactly once by seeding the backtracker at its *smallest* pattern
///   position holding a produced edge — positions before the seed are
///   restricted to kept edges, the seed position is pinned, and positions
///   after are unrestricted.
///
/// The result is sorted by `edge_idx`, which reproduces `find_matches`'
/// enumeration order exactly (its recursion scans edge indices ascending
/// per position, so its output is lexicographic in `edge_idx`, and
/// `edge_idx` determines the binding). Byte-stable downstream: event ids,
/// causal first-match, branchial pairs.
pub fn delta_matches(
    rule: &Rule,
    parent_matches: &[Match],
    m: &Match,
    app: &Application,
) -> Vec<Match> {
    let child = &app.child;
    let mut out: Vec<Match> = Vec::new();

    // survivors (app.kept is in parent order — binary search by parent idx)
    for pm in parent_matches {
        if pm.edge_idx.iter().any(|i| m.edge_idx.contains(i)) {
            continue;
        }
        let edge_idx: Vec<usize> = pm
            .edge_idx
            .iter()
            .map(|&pi| {
                let pos = app
                    .kept
                    .binary_search_by_key(&pi, |&(p, _)| p)
                    .expect("surviving match references a consumed edge");
                app.kept[pos].1
            })
            .collect();
        out.push(Match {
            edge_idx,
            binding: pm.binding.clone(),
        });
    }

    // new matches, seeded per (pattern position, produced edge)
    let prod_start = app.produced.start;
    let mut used = vec![false; child.edges.len()];
    let mut binding: Vec<Option<Vertex>> = vec![None; rule.n_vars];
    let mut chosen: Vec<usize> = Vec::with_capacity(rule.lhs.len());
    for k0 in 0..rule.lhs.len() {
        for pe in app.produced.clone() {
            if child.edges[pe].len() != rule.lhs[k0].len() {
                continue;
            }
            seeded_rec(
                child,
                rule,
                0,
                k0,
                pe,
                prod_start,
                &mut used,
                &mut binding,
                &mut chosen,
                &mut out,
            );
        }
    }

    out.sort_by(|a, b| a.edge_idx.cmp(&b.edge_idx));
    out
}

/// Backtracking with position `k0` pinned to edge `pin` and positions
/// before `k0` restricted to kept edges (child index < `prod_start`).
#[allow(clippy::too_many_arguments)]
fn seeded_rec(
    state: &State,
    rule: &Rule,
    k: usize,
    k0: usize,
    pin: usize,
    prod_start: usize,
    used: &mut [bool],
    binding: &mut [Option<Vertex>],
    chosen: &mut Vec<usize>,
    out: &mut Vec<Match>,
) {
    if k == rule.lhs.len() {
        out.push(Match {
            edge_idx: chosen.clone(),
            binding: binding.to_vec(),
        });
        return;
    }
    let pat = &rule.lhs[k];
    let candidates: std::ops::Range<usize> = if k == k0 {
        pin..pin + 1
    } else if k < k0 {
        0..prod_start
    } else {
        0..state.edges.len()
    };
    for ei in candidates {
        if used[ei] || state.edges[ei].len() != pat.len() {
            continue;
        }
        let mut added: Vec<usize> = Vec::new();
        let mut ok = true;
        for (p, v) in pat.iter().zip(state.edges[ei].iter()) {
            match binding[*p] {
                Some(x) if x != *v => {
                    ok = false;
                    break;
                }
                Some(_) => {}
                None => {
                    binding[*p] = Some(*v);
                    added.push(*p);
                }
            }
        }
        if ok {
            used[ei] = true;
            chosen.push(ei);
            seeded_rec(
                state,
                rule,
                k + 1,
                k0,
                pin,
                prod_start,
                used,
                binding,
                chosen,
                out,
            );
            chosen.pop();
            used[ei] = false;
        }
        for p in added {
            binding[p] = None;
        }
    }
}
