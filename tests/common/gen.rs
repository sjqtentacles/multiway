//! Deterministic generators for random states, renamings, rule texts, and
//! adversarial strings.
//!
//! The generators are deliberately biased toward the cases where multiset /
//! ordered-hyperedge semantics can silently break: duplicate edge instances,
//! self-loops, repeated vertices within an edge, arity-1 edges.

use super::prng::Rng;
use multiway::hypergraph::{State, Vertex};

pub struct StateCfg {
    pub max_vertices: u32,
    pub max_edges: usize,
    pub max_arity: usize,
    /// % chance each edge is a byte-identical copy of an earlier edge.
    pub dup_pct: u64,
    /// % chance a binary edge is forced into a self-loop.
    pub self_loop_pct: u64,
    /// Relabel through a random injection into 0..999 (catches accidental
    /// dependence on label magnitude or density).
    pub sparse_labels: bool,
}

impl StateCfg {
    /// Sized for the brute-force oracles (≤6 vertices keeps 6! = 720
    /// bijections per check).
    pub fn oracle() -> Self {
        StateCfg {
            max_vertices: 6,
            max_edges: 7,
            max_arity: 3,
            dup_pct: 20,
            self_loop_pct: 20,
            sparse_labels: false,
        }
    }

    /// Larger states for properties with no oracle in the loop
    /// (e.g. pure invariance checks).
    pub fn wide() -> Self {
        StateCfg {
            max_vertices: 12,
            max_edges: 12,
            max_arity: 4,
            dup_pct: 15,
            self_loop_pct: 15,
            sparse_labels: true,
        }
    }
}

pub fn gen_state(rng: &mut Rng, cfg: &StateCfg) -> State {
    let n_verts = rng.range_usize(1, cfg.max_vertices as usize) as u64;
    let n_edges = rng.range_usize(0, cfg.max_edges);
    let mut edges: Vec<Vec<Vertex>> = Vec::with_capacity(n_edges);
    for _ in 0..n_edges {
        if !edges.is_empty() && rng.chance(cfg.dup_pct, 100) {
            let e = rng.pick(&edges).clone();
            edges.push(e);
            continue;
        }
        let arity = rng.range_usize(1, cfg.max_arity);
        let mut e: Vec<Vertex> = (0..arity).map(|_| rng.below(n_verts) as Vertex).collect();
        if e.len() == 2 && rng.chance(cfg.self_loop_pct, 100) {
            e[1] = e[0];
        }
        edges.push(e);
    }
    let s = State::new(edges);
    if cfg.sparse_labels {
        let map = gen_renaming(rng, &s, true);
        rename(&s, &map)
    } else {
        s
    }
}

/// Random injective renaming of exactly the state's vertex set.
/// `fresh = false`: a permutation of the same label set (keeps
/// `next_vertex` comparable). `fresh = true`: an injection into random new
/// labels in 0..999 (catches label-magnitude dependence).
pub fn gen_renaming(rng: &mut Rng, s: &State, fresh: bool) -> Vec<(Vertex, Vertex)> {
    let vs = s.vertices();
    if fresh {
        let mut labels: Vec<Vertex> = (0..1000).collect();
        rng.shuffle(&mut labels);
        vs.iter()
            .enumerate()
            .map(|(i, &v)| (v, labels[i]))
            .collect()
    } else {
        let mut perm = vs.clone();
        rng.shuffle(&mut perm);
        vs.iter().zip(perm).map(|(&v, p)| (v, p)).collect()
    }
}

/// Apply a renaming (unmapped vertices pass through untouched).
pub fn rename(s: &State, map: &[(Vertex, Vertex)]) -> State {
    let lookup = |v: Vertex| {
        map.iter()
            .find(|(from, _)| *from == v)
            .map(|(_, to)| *to)
            .unwrap_or(v)
    };
    State::new(
        s.edges
            .iter()
            .map(|e| e.iter().map(|&v| lookup(v)).collect())
            .collect(),
    )
}

/// Shuffle edge-instance order in place. States are multisets — edge order
/// must never be observable, so invariance tests shuffle it deliberately.
pub fn shuffle_edges(rng: &mut Rng, s: &mut State) {
    rng.shuffle(&mut s.edges);
}

pub struct RuleCfg {
    pub max_lhs: usize,
    pub max_rhs: usize,
    pub max_arity: usize,
    pub max_lhs_vars: usize,
    pub max_fresh_vars: usize,
}

impl Default for RuleCfg {
    fn default() -> Self {
        RuleCfg {
            max_lhs: 2,
            max_rhs: 4,
            max_arity: 3,
            max_lhs_vars: 3,
            max_fresh_vars: 2,
        }
    }
}

/// Generate rule TEXT (not a `Rule` struct) so every random case also
/// exercises the parser and variable interning. LHS variables are sampled
/// with replacement, so repeated variables (non-injective patterns) occur
/// naturally; the RHS mixes LHS vars with fresh ones; empty RHS `{}` is
/// legal and generated.
pub fn gen_rule_text(rng: &mut Rng, cfg: &RuleCfg) -> String {
    let n_lhs = rng.range_usize(1, cfg.max_lhs);
    let n_lhs_vars = rng.range_usize(1, cfg.max_lhs_vars);
    let lhs_vars: Vec<String> = (0..n_lhs_vars).map(|i| format!("v{}", i)).collect();

    let edge = |rng: &mut Rng, pool: &[String]| -> String {
        let arity = rng.range_usize(1, cfg.max_arity);
        let vars: Vec<String> = (0..arity).map(|_| rng.pick(pool).clone()).collect();
        format!("{{{}}}", vars.join(","))
    };

    let lhs: Vec<String> = (0..n_lhs).map(|_| edge(rng, &lhs_vars)).collect();

    let n_fresh = rng.range_usize(0, cfg.max_fresh_vars);
    let mut rhs_pool = lhs_vars.clone();
    rhs_pool.extend((0..n_fresh).map(|i| format!("f{}", i)));
    let n_rhs = rng.range_usize(0, cfg.max_rhs);
    let rhs: Vec<String> = (0..n_rhs).map(|_| edge(rng, &rhs_pool)).collect();

    format!("{{{}}}->{{{}}}", lhs.join(","), rhs.join(","))
}

const ALPHABET: &[char] = &[
    '{', '}', ',', '-', '>', 'x', 'y', 'z', 'w', '0', '1', '2', '9', ' ', '_', 'α', '🦀', '"', '\\',
];

/// Random string over an alphabet chosen to stress the parser (always valid
/// UTF-8; char-boundary-safe by construction).
pub fn gen_garbage(rng: &mut Rng, max_len: usize) -> String {
    let len = rng.range_usize(0, max_len);
    (0..len).map(|_| *rng.pick(ALPHABET)).collect()
}

/// Insert, delete, or replace one char (operating on char vectors, so
/// multi-byte chars never produce invalid slices).
pub fn mutate_string(rng: &mut Rng, s: &str) -> String {
    let mut cs: Vec<char> = s.chars().collect();
    match rng.below(3) {
        0 => {
            let i = rng.range_usize(0, cs.len());
            cs.insert(i, *rng.pick(ALPHABET));
        }
        1 if !cs.is_empty() => {
            let i = rng.range_usize(0, cs.len() - 1);
            cs.remove(i);
        }
        _ if !cs.is_empty() => {
            let i = rng.range_usize(0, cs.len() - 1);
            cs[i] = *rng.pick(ALPHABET);
        }
        _ => {}
    }
    cs.into_iter().collect()
}
