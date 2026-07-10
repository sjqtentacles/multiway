//! Canonization: true canonical forms, invariant hashing, and exact
//! isomorphism checking.
//!
//! Two states that differ only by a renaming of vertices are the *same*
//! multiway node. The engine detects this with **true canonization**
//! ([`canonicalize`]): a nauty-style individualization–refinement search
//! that assigns every state a canonical *form* — a relabeled representative
//! such that `canonical_form(a) == canonical_form(b) ⟺ a ≅ b`. Dedup is a
//! plain map lookup on the form; no bucket scans, no in-loop isomorphism
//! checks.
//!
//! Definition (normative): the canonical form is the minimal leaf of the
//! IR search under a fully specified invariant discipline — exact
//! rank-normalized refinement classes (no hashes anywhere in identity),
//! smallest-cell/smallest-class-id target selection, ascending-vertex-id
//! branching, first-found minimal leaf. It is NOT the global lex-minimum
//! over all n! labelings (computing that is the graph-canonization problem
//! itself); completeness needs only invariance of the discipline plus the
//! fact that every leaf is an actual relabeling.
//!
//! The earlier two-tier scheme survives as test oracles:
//! 1. [`wl_hash`] — Weisfeiler–Leman-style invariant hash (isomorphic
//!    states always collide; not complete).
//! 2. [`isomorphic`] / [`isomorphic_with_budget`] — exact backtracking
//!    check, exponential worst case, optionally fuel-capped.

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

// ---------------------------------------------------------------------------
// True canonization: individualization–refinement producing a canonical FORM.

use crate::det::DetMap;
use crate::hypergraph::Edge;

/// Full canonization result: the canonical form plus the witness maps the
/// token-event graph needs to identify edge instances across histories.
pub struct Canon {
    /// Canonical representative: vertices exactly `0..n-1`, edges sorted by
    /// `(len, sequence)`, `next_vertex == n`.
    pub form: State,
    /// Raw vertex -> canonical label (a bijection onto `0..n-1`).
    pub vertex_map: DetMap<Vertex, Vertex>,
    /// Raw edge index i -> index of its relabeled image in `form.edges`.
    /// Byte-identical duplicate edges occupy their slot run in ascending
    /// raw-index order — the engine's fixed token-identity convention.
    pub edge_slots: Vec<usize>,
}

/// Colored canonization result (see [`canonicalize_colored`]).
pub struct ColoredCanon {
    /// The canon under color-seeded refinement.
    pub canon: Canon,
    /// Input color of the vertex assigned canonical label `i`.
    pub label_colors: Vec<u64>,
}

/// Canonical form only (the dedup key).
pub fn canonical_form(state: &State) -> State {
    canonicalize(state).form
}

/// Canonicalize with witness. Uncolored: applies component decomposition
/// (the k-identical-disjoint-components pathology would otherwise cost k!
/// leaves).
pub fn canonicalize(state: &State) -> Canon {
    canonicalize_with_leaf_count(state).0
}

/// Diagnostic entry point: also reports how many IR leaves the search
/// visited (pinned by tests to keep the search polynomial on the shapes
/// the engine produces).
pub fn canonicalize_with_leaf_count(state: &State) -> (Canon, u64) {
    canonicalize_impl(state, u64::MAX).expect("unlimited leaf budget cannot abort")
}

/// Canonicalize with a hard cap on IR leaves visited, returning `None` on
/// exhaustion. The scan-safety primitive: a symmetric state (a pure
/// out-star) costs k! leaves — fine for a human-chosen rule, fatal inside
/// a scan over thousands of rules. Exhaustion must abort the whole probe
/// run (never continue with a possibly-duplicate state — the dedup-safety
/// note on `isomorphic` applies equally here). `canonicalize` is exactly
/// this with an unlimited budget, byte-identical by construction.
pub fn canonicalize_budgeted(state: &State, max_leaves: u64) -> Option<Canon> {
    canonicalize_impl(state, max_leaves).map(|(c, _)| c)
}

fn canonicalize_impl(state: &State, max_leaves: u64) -> Option<(Canon, u64)> {
    // Split into connected components (arity-0 edges carry no vertices and
    // pass straight through to the form).
    let verts = state.vertices();
    if verts.is_empty() {
        // only arity-0 edges (or empty): form is the state itself
        let form = State {
            edges: state.edges.clone(),
            next_vertex: 0,
        };
        let edge_slots = (0..state.edges.len()).collect();
        return Some((
            Canon {
                form,
                vertex_map: DetMap::default(),
                edge_slots,
            },
            1,
        ));
    }

    // union-find over vertex positions
    let vidx: DetMap<Vertex, usize> = verts.iter().enumerate().map(|(i, &v)| (v, i)).collect();
    let mut parent: Vec<usize> = (0..verts.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for e in &state.edges {
        if let Some((first, rest)) = e.split_first() {
            let r0 = find(&mut parent, vidx[first]);
            for v in rest {
                let r = find(&mut parent, vidx[v]);
                parent[r] = r0;
            }
        }
    }
    let mut roots: Vec<usize> = Vec::new();
    for i in 0..verts.len() {
        let r = find(&mut parent, i);
        if !roots.contains(&r) {
            roots.push(r);
        }
    }

    if roots.len() == 1 {
        return canonicalize_connected(state, None, max_leaves);
    }

    // Per-component sub-states (raw edge indices retained); arity-0 edges
    // form their own bucket.
    struct Comp {
        canon: Canon,
        min_vertex: Vertex,
    }
    let mut comps: Vec<Comp> = Vec::new();
    let mut arity0: Vec<usize> = Vec::new();
    let mut total_leaves = 0u64;
    for &root in &roots {
        let members: Vec<Vertex> = verts
            .iter()
            .enumerate()
            .filter(|(i, _)| find(&mut parent, *i) == root)
            .map(|(_, &v)| v)
            .collect();
        let sub = State::new(
            state
                .edges
                .iter()
                .filter(|e| !e.is_empty() && members.contains(&e[0]))
                .cloned()
                .collect(),
        );
        let remaining = max_leaves.checked_sub(total_leaves)?;
        let (canon, leaves) = canonicalize_connected(&sub, None, remaining)?;
        total_leaves += leaves;
        comps.push(Comp {
            canon,
            min_vertex: *members.iter().min().unwrap(),
        });
    }
    for (i, e) in state.edges.iter().enumerate() {
        if e.is_empty() {
            arity0.push(i);
        }
    }

    // Interchangeable identical components: order by canonical form, ties
    // by smallest raw vertex (witness determinism; the form is unaffected).
    comps.sort_by(|a, b| {
        edge_list_key(&a.canon.form.edges)
            .cmp(&edge_list_key(&b.canon.form.edges))
            .then(a.min_vertex.cmp(&b.min_vertex))
    });

    // Compose: label offsets in sorted component order.
    let mut vertex_map: DetMap<Vertex, Vertex> = DetMap::default();
    let mut offset: u32 = 0;
    for comp in &comps {
        for (&raw, &lab) in comp.canon.vertex_map.iter() {
            vertex_map.insert(raw, lab + offset);
        }
        offset += comp.canon.vertex_map.len() as u32;
    }
    Some(finish_from_vertex_map(
        state,
        vertex_map,
        total_leaves.max(1),
    ))
}

/// Total edge order used everywhere: (arity, vertex sequence).
fn edge_list_key(edges: &[Edge]) -> Vec<(usize, Edge)> {
    edges.iter().map(|e| (e.len(), e.clone())).collect()
}

/// Given a final vertex bijection, build the sorted form and the slot map
/// (duplicate edges take slots in ascending raw-index order).
fn finish_from_vertex_map(
    state: &State,
    vertex_map: DetMap<Vertex, Vertex>,
    leaves: u64,
) -> (Canon, u64) {
    let relabeled: Vec<Edge> = state
        .edges
        .iter()
        .map(|e| e.iter().map(|v| vertex_map[v]).collect())
        .collect();
    let mut order: Vec<usize> = (0..relabeled.len()).collect();
    order.sort_by(|&i, &j| {
        (relabeled[i].len(), &relabeled[i], i).cmp(&(relabeled[j].len(), &relabeled[j], j))
    });
    let mut edge_slots = vec![0usize; relabeled.len()];
    let mut form_edges: Vec<Edge> = Vec::with_capacity(relabeled.len());
    for (slot, &raw) in order.iter().enumerate() {
        edge_slots[raw] = slot;
        form_edges.push(relabeled[raw].clone());
    }
    let n = vertex_map.len() as u32;
    (
        Canon {
            form: State {
                edges: form_edges,
                next_vertex: n,
            },
            vertex_map,
            edge_slots,
        },
        leaves,
    )
}

/// Exact color refinement: per-vertex classes are dense ids `0..k` ordered
/// by an invariant key built purely from class ids and edge positions —
/// no hashes anywhere, so colored identities (confluence pins) can never
/// be lost to a collision. Monotone (classes only split); stops when the
/// class count stabilizes.
fn refine_exact(edges: &[Edge], vidx: &DetMap<Vertex, usize>, classes: &mut [usize]) {
    let n = classes.len();
    let mut rounds = 0usize;
    loop {
        rounds += 1;
        assert!(rounds <= n + 2, "refinement failed to stabilize");
        let count_before = class_count(classes);

        // exact edge signatures under current classes
        let esigs: Vec<(usize, Vec<usize>)> = edges
            .iter()
            .map(|e| (e.len(), e.iter().map(|v| classes[vidx[v]]).collect()))
            .collect();
        let mut sig_sorted: Vec<(usize, Vec<usize>)> = esigs.clone();
        sig_sorted.sort();
        sig_sorted.dedup();

        // per-vertex invariant key: (old class, sorted multiset of
        // (edge signature id, position))
        let mut vkeys: Vec<(usize, Vec<(usize, usize)>)> =
            classes.iter().map(|&c| (c, Vec::new())).collect();
        for (ei, e) in edges.iter().enumerate() {
            let sig_id = sig_sorted.binary_search(&esigs[ei]).unwrap();
            for (pos, v) in e.iter().enumerate() {
                vkeys[vidx[v]].1.push((sig_id, pos));
            }
        }
        for k in vkeys.iter_mut() {
            k.1.sort_unstable();
        }

        // rank-normalize to dense ids ordered by key
        let mut uniq: Vec<(usize, Vec<(usize, usize)>)> = vkeys.clone();
        uniq.sort();
        uniq.dedup();
        for (i, k) in vkeys.iter().enumerate() {
            classes[i] = uniq.binary_search(k).unwrap();
        }

        if class_count(classes) == count_before {
            return;
        }
    }
}

fn class_count(classes: &[usize]) -> usize {
    let mut seen: Vec<usize> = classes.to_vec();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

struct IrCtx<'a> {
    edges: &'a [Edge],
    verts: &'a [Vertex],
    vidx: &'a DetMap<Vertex, usize>,
    /// input color per vertex position (colored mode; None = uncolored)
    input_colors: Option<&'a [u64]>,
    /// best leaf so far: (sorted edge list, label_colors, labels per position)
    best: Option<(Vec<Edge>, Vec<u64>, Vec<u32>)>,
    leaves: u64,
    /// Leaf budget; exceeding it aborts the search (scan safety).
    max_leaves: u64,
}

/// The IR search. Branching discipline (normative, the witness depends on
/// it): target cell = smallest non-singleton class, ties by smallest class
/// id; branch over members in ascending raw-vertex-id order; individualize,
/// re-refine, recurse; keep the minimal leaf by (edge list, label_colors),
/// first-found winning ties.
/// Returns `false` iff the leaf budget was exhausted (search aborted).
fn ir_search(ctx: &mut IrCtx, mut classes: Vec<usize>, depth: usize) -> bool {
    let n = classes.len();
    assert!(depth <= n + 1, "IR search exceeded depth bound");
    refine_exact(ctx.edges, ctx.vidx, &mut classes);

    let k = class_count(&classes);
    if k == n {
        // discrete: label = class id
        ctx.leaves += 1;
        if ctx.leaves > ctx.max_leaves {
            return false;
        }
        let labels: Vec<u32> = classes.iter().map(|&c| c as u32).collect();
        let mut edge_list: Vec<Edge> = ctx
            .edges
            .iter()
            .map(|e| e.iter().map(|v| labels[ctx.vidx[v]]).collect())
            .collect();
        edge_list.sort_by(|a, b| (a.len(), &a[..]).cmp(&(b.len(), &b[..])));
        let label_colors: Vec<u64> = match ctx.input_colors {
            Some(cols) => {
                let mut lc = vec![0u64; n];
                for (pos, &lab) in labels.iter().enumerate() {
                    lc[lab as usize] = cols[pos];
                }
                lc
            }
            None => Vec::new(),
        };
        let candidate_key = (edge_list, label_colors, labels);
        match &ctx.best {
            Some((be, bc, _)) if (be, bc) <= (&candidate_key.0, &candidate_key.1) => {}
            _ => ctx.best = Some(candidate_key),
        }
        return true;
    }

    // target cell: smallest non-singleton class, ties by smallest class id
    let mut sizes: Vec<usize> = vec![0; k];
    for &c in &classes {
        sizes[c] += 1;
    }
    let target = (0..k)
        .filter(|&c| sizes[c] > 1)
        .min_by_key(|&c| (sizes[c], c))
        .expect("non-discrete partition must have a non-singleton cell");

    // members in ascending raw-vertex-id order (verts is sorted, and
    // positions follow vertex order)
    for pos in 0..n {
        if classes[pos] == target {
            let mut branch = classes.clone();
            branch[pos] = k; // fresh class; next refine rank-normalizes
            if !ir_search(ctx, branch, depth + 1) {
                return false;
            }
        }
    }
    true
}

/// Canonicalize a connected state (no component decomposition). `colors`
/// seeds refinement in colored mode.
fn canonicalize_connected(
    state: &State,
    colors: Option<&[u64]>,
    max_leaves: u64,
) -> Option<(Canon, u64)> {
    let verts = state.vertices();
    let n = verts.len();
    let vidx: DetMap<Vertex, usize> = verts.iter().enumerate().map(|(i, &v)| (v, i)).collect();

    // initial classes: uniform, or rank-normalized input colors
    let init_classes: Vec<usize> = match colors {
        None => vec![0; n],
        Some(cols) => {
            let mut uniq: Vec<u64> = cols.to_vec();
            uniq.sort_unstable();
            uniq.dedup();
            cols.iter()
                .map(|c| uniq.binary_search(c).unwrap())
                .collect()
        }
    };

    let mut ctx = IrCtx {
        edges: &state.edges,
        verts: &verts,
        vidx: &vidx,
        input_colors: colors,
        best: None,
        leaves: 0,
        max_leaves,
    };
    if !ir_search(&mut ctx, init_classes, 0) {
        return None;
    }
    let (_, _, labels) = ctx.best.expect("search visited no leaves");

    let vertex_map: DetMap<Vertex, Vertex> = ctx
        .verts
        .iter()
        .enumerate()
        .map(|(pos, &v)| (v, labels[pos]))
        .collect();
    Some(finish_from_vertex_map(state, vertex_map, ctx.leaves))
}

/// Colored canonization: refinement seeded by exact per-vertex colors,
/// leaf comparison on `(edge list, label_colors)`. Used by the confluence
/// checker to pin host vertices — identity flows through exact class ids,
/// never through a hash, so a collision can never falsely identify two
/// differently-pinned states. Component decomposition is deliberately
/// DISABLED in colored mode (identically-shaped components carrying
/// different pins must not be interchanged).
pub fn canonicalize_colored(state: &State, color: &dyn Fn(Vertex) -> u64) -> ColoredCanon {
    let verts = state.vertices();
    let cols: Vec<u64> = verts.iter().map(|&v| color(v)).collect();
    let (canon, _) = canonicalize_connected(state, Some(&cols), u64::MAX)
        .expect("unlimited leaf budget cannot abort");
    let n = verts.len();
    let mut label_colors = vec![0u64; n];
    for (pos, &v) in verts.iter().enumerate() {
        label_colors[canon.vertex_map[&v] as usize] = cols[pos];
    }
    ColoredCanon {
        canon,
        label_colors,
    }
}
