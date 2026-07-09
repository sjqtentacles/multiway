//! Canonical (isomorphism-invariant) hashing and exact isomorphism checking.
//!
//! Two states that differ only by a renaming of vertices are the *same*
//! multiway node. We detect this with a two-tier scheme:
//!
//! 1. `wl_hash` — a Weisfeiler–Leman-style color-refinement hash over the
//!    ordered hypergraph. It is isomorphism-*invariant* (isomorphic states
//!    always collide), so it never causes a missed merge. Like WL on
//!    graphs, it is not isomorphism-*complete*: rare non-isomorphic states
//!    can share a hash.
//! 2. `isomorphic` — an exact backtracking check run only within a hash
//!    bucket, so WL collisions cost a little time, never correctness.
//!
//! Roadmap: replace with true canonization (nauty-style refinement with
//! tie-breaking) so states get a canonical *form*, not just a hash — the
//! prerequisite for HashLife-style memoization of local evolution.

use crate::det::{hash_seq, mix};
use crate::hypergraph::{State, Vertex};
use std::collections::HashMap;

/// Isomorphism-invariant hash of a state.
///
/// Color refinement adapted to ordered hyperedges: an edge's signature is
/// the sequence of its vertices' colors (order matters); a vertex's new
/// color folds in the sorted multiset of (edge signature, position) pairs
/// it participates in. Repeated for ~|V| rounds, then the state hash is
/// the sorted multiset of final edge signatures.
pub fn wl_hash(state: &State) -> u64 {
    if state.edges.is_empty() {
        return hash_seq(&[]);
    }
    let verts = state.vertices();
    let n = verts.len();
    let idx: HashMap<Vertex, usize> = verts.iter().enumerate().map(|(i, &v)| (v, i)).collect();

    let mut color = vec![0x5EEDu64; n];
    let mut edge_h = vec![0u64; state.edges.len()];
    let rounds = n.min(24) + 2;

    for _ in 0..rounds {
        for (ei, e) in state.edges.iter().enumerate() {
            let mut seq = Vec::with_capacity(e.len() + 1);
            seq.push(e.len() as u64);
            for v in e {
                seq.push(color[idx[v]]);
            }
            edge_h[ei] = hash_seq(&seq);
        }
        let mut incid: Vec<Vec<u64>> = vec![Vec::new(); n];
        for (ei, e) in state.edges.iter().enumerate() {
            for (pos, v) in e.iter().enumerate() {
                incid[idx[v]].push(mix(edge_h[ei] ^ mix(0xA11CE ^ pos as u64)));
            }
        }
        for i in 0..n {
            incid[i].sort_unstable();
            let mut seq = Vec::with_capacity(incid[i].len() + 1);
            seq.push(color[i]);
            seq.extend_from_slice(&incid[i]);
            color[i] = hash_seq(&seq);
        }
    }

    let mut eh: Vec<u64> = state
        .edges
        .iter()
        .map(|e| {
            let mut seq = Vec::with_capacity(e.len() + 1);
            seq.push(e.len() as u64);
            for v in e {
                seq.push(color[idx[v]]);
            }
            hash_seq(&seq)
        })
        .collect();
    eh.sort_unstable();
    hash_seq(&eh)
}

/// Outcome of a budgeted isomorphism check.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum IsoResult {
    /// A vertex bijection exists.
    Iso,
    /// Proven non-isomorphic (search exhausted or pre-checks failed).
    NotIso,
    /// The backtracking budget ran out before a verdict.
    BudgetExhausted,
}

/// Exact isomorphism check with a fuel cap.
///
/// `budget` counts backtracking node visits (one unit per `go` entry).
/// The backtracker is exponential in the worst case — see the pathological
/// star-pair witness in `tests/prop_canon.rs`, which burns Θ(m!) visits —
/// so callers that cannot tolerate unbounded latency cap it and handle
/// [`IsoResult::BudgetExhausted`].
///
/// Pinned semantics: the cheap pre-checks (edge count, vertex count, arity
/// profile) run *before* fuel accounting, so a pre-check-rejected pair
/// returns `NotIso` even with `budget = 0`, while any pair reaching the
/// backtracker with `budget = 0` — including `(a, a)` — returns
/// `BudgetExhausted`.
pub fn isomorphic_with_budget(a: &State, b: &State, budget: u64) -> IsoResult {
    if a.edges.len() != b.edges.len() {
        return IsoResult::NotIso;
    }
    if a.vertices().len() != b.vertices().len() {
        return IsoResult::NotIso;
    }
    let mut aa: Vec<usize> = a.edges.iter().map(|e| e.len()).collect();
    let mut bb: Vec<usize> = b.edges.iter().map(|e| e.len()).collect();
    aa.sort_unstable();
    bb.sort_unstable();
    if aa != bb {
        return IsoResult::NotIso;
    }

    /// `Some(found)` on a verdict, `None` on fuel exhaustion (mappings are
    /// unwound before propagating so the caller sees clean state).
    fn go(
        k: usize,
        a: &State,
        b: &State,
        used: &mut [bool],
        f: &mut HashMap<Vertex, Vertex>,
        g: &mut HashMap<Vertex, Vertex>,
        fuel: &mut u64,
    ) -> Option<bool> {
        if *fuel == 0 {
            return None;
        }
        *fuel -= 1;
        if k == a.edges.len() {
            return Some(true);
        }
        let ea = &a.edges[k];
        for bi in 0..b.edges.len() {
            if used[bi] || b.edges[bi].len() != ea.len() {
                continue;
            }
            let eb = &b.edges[bi];
            let mut added: Vec<(Vertex, Vertex)> = Vec::new();
            let mut ok = true;
            for (x, y) in ea.iter().zip(eb.iter()) {
                let fx = f.get(x).copied();
                let gy = g.get(y).copied();
                match (fx, gy) {
                    (Some(v), _) if v != *y => {
                        ok = false;
                        break;
                    }
                    (_, Some(u)) if u != *x => {
                        ok = false;
                        break;
                    }
                    (None, None) => {
                        f.insert(*x, *y);
                        g.insert(*y, *x);
                        added.push((*x, *y));
                    }
                    _ => {}
                }
            }
            if ok {
                used[bi] = true;
                match go(k + 1, a, b, used, f, g, fuel) {
                    Some(true) => return Some(true),
                    Some(false) => {}
                    None => {
                        used[bi] = false;
                        for (x, y) in added {
                            f.remove(&x);
                            g.remove(&y);
                        }
                        return None;
                    }
                }
                used[bi] = false;
            }
            for (x, y) in added {
                f.remove(&x);
                g.remove(&y);
            }
        }
        Some(false)
    }

    let mut used = vec![false; b.edges.len()];
    let mut f = HashMap::new();
    let mut g = HashMap::new();
    let mut fuel = budget;
    match go(0, a, b, &mut used, &mut f, &mut g, &mut fuel) {
        Some(true) => IsoResult::Iso,
        Some(false) => IsoResult::NotIso,
        None => IsoResult::BudgetExhausted,
    }
}

/// Exact isomorphism check: is there a vertex bijection under which the
/// edge multisets coincide? Backtracks over edge-to-edge assignments,
/// maintaining the bijection in both directions. Exponential in the worst
/// case; run it only inside a WL-hash bucket (or use
/// [`isomorphic_with_budget`] where latency must be bounded).
///
/// NOTE: `evolve` deliberately stays on this unbudgeted form — budget
/// exhaustion inside dedup would silently duplicate states and corrupt
/// layer counts.
pub fn isomorphic(a: &State, b: &State) -> bool {
    isomorphic_with_budget(a, b, u64::MAX) == IsoResult::Iso
}
