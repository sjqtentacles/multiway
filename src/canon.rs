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

use crate::hypergraph::{State, Vertex};
use std::collections::HashMap;

#[inline]
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Order-sensitive hash of a sequence (deterministic across runs — no
/// std RandomState anywhere, results must be reproducible).
fn hash_seq(xs: &[u64]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &x in xs {
        h = mix(h ^ x);
    }
    mix(h ^ (xs.len() as u64))
}

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

/// Exact isomorphism check: is there a vertex bijection under which the
/// edge multisets coincide? Backtracks over edge-to-edge assignments,
/// maintaining the bijection in both directions. Exponential in the worst
/// case; run it only inside a WL-hash bucket.
pub fn isomorphic(a: &State, b: &State) -> bool {
    if a.edges.len() != b.edges.len() {
        return false;
    }
    if a.vertices().len() != b.vertices().len() {
        return false;
    }
    let mut aa: Vec<usize> = a.edges.iter().map(|e| e.len()).collect();
    let mut bb: Vec<usize> = b.edges.iter().map(|e| e.len()).collect();
    aa.sort_unstable();
    bb.sort_unstable();
    if aa != bb {
        return false;
    }

    fn go(
        k: usize,
        a: &State,
        b: &State,
        used: &mut [bool],
        f: &mut HashMap<Vertex, Vertex>,
        g: &mut HashMap<Vertex, Vertex>,
    ) -> bool {
        if k == a.edges.len() {
            return true;
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
                if go(k + 1, a, b, used, f, g) {
                    return true;
                }
                used[bi] = false;
            }
            for (x, y) in added {
                f.remove(&x);
                g.remove(&y);
            }
        }
        false
    }

    let mut used = vec![false; b.edges.len()];
    let mut f = HashMap::new();
    let mut g = HashMap::new();
    go(0, a, b, &mut used, &mut f, &mut g)
}
