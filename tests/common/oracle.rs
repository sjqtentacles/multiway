//! Ground-truth oracles, independent of the code under test.
//!
//! `iso_bruteforce` enumerates every vertex bijection — the definitional
//! truth for isomorphism on small states. `naive_tree` builds the unshared
//! multiway evolution tree — the definitional truth for path counts and
//! layer class counts. `match_bruteforce` enumerates every ordered tuple of
//! distinct edge instances and checks bindings positionally with no
//! backtracking — the definitional truth for the matcher.
//!
//! None of these call `wl_hash`, `canonical_form`, or the matcher's pruning
//! logic, so a bug in the code under test cannot hide inside its own oracle.

use multiway::hypergraph::{Edge, State, Vertex};
use multiway::matcher::{apply, find_matches};
use multiway::rule::Rule;
use std::collections::BTreeMap;

/// Iterative Heap's algorithm. Calls `f` with each permutation of `0..n`;
/// stops early and returns `true` the first time `f` returns `true`.
pub fn for_each_permutation(n: usize, mut f: impl FnMut(&[usize]) -> bool) -> bool {
    let mut a: Vec<usize> = (0..n).collect();
    if f(&a) {
        return true;
    }
    let mut c = vec![0usize; n];
    let mut i = 0;
    while i < n {
        if c[i] < i {
            if i % 2 == 0 {
                a.swap(0, i);
            } else {
                a.swap(c[i], i);
            }
            if f(&a) {
                return true;
            }
            c[i] += 1;
            i = 0;
        } else {
            c[i] = 0;
            i += 1;
        }
    }
    false
}

/// Exact isomorphism by exhaustive vertex-bijection enumeration.
///
/// Ground truth for states with at most 7 vertices (7! = 5040 bijections);
/// asserts loudly on misuse rather than silently taking forever.
pub fn iso_bruteforce(a: &State, b: &State) -> bool {
    if a.edges.len() != b.edges.len() {
        return false;
    }
    let va = a.vertices();
    let vb = b.vertices();
    if va.len() != vb.len() {
        return false;
    }
    assert!(
        va.len() <= 7,
        "iso_bruteforce guard: {} vertices > 7",
        va.len()
    );
    let mut aa: Vec<usize> = a.edges.iter().map(|e| e.len()).collect();
    let mut bb: Vec<usize> = b.edges.iter().map(|e| e.len()).collect();
    aa.sort_unstable();
    bb.sort_unstable();
    if aa != bb {
        return false;
    }

    let mut b_sorted: Vec<Edge> = b.edges.clone();
    b_sorted.sort();

    for_each_permutation(va.len(), |perm| {
        // vertex va[i] of `a` maps to vb[perm[i]] of `b`
        let mut mapped: Vec<Edge> = a
            .edges
            .iter()
            .map(|e| {
                e.iter()
                    .map(|v| vb[perm[va.binary_search(v).unwrap()]])
                    .collect()
            })
            .collect();
        mapped.sort();
        mapped == b_sorted
    })
}

/// The naive (unshared) multiway evolution tree: `layers[0] = [init]`,
/// `layers[k+1]` = every `apply` of every match on every state in layer k.
/// Returns `None` if the total node count would exceed `cap` — callers skip
/// (and tally) such cases rather than hanging.
pub fn naive_tree(rule: &Rule, init: &State, steps: usize, cap: usize) -> Option<Vec<Vec<State>>> {
    let mut layers = vec![vec![init.clone()]];
    let mut total = 1usize;
    for _ in 0..steps {
        let mut next: Vec<State> = Vec::new();
        for s in layers.last().unwrap() {
            for m in find_matches(s, rule) {
                total += 1;
                if total > cap {
                    return None;
                }
                next.push(apply(s, rule, &m));
            }
        }
        layers.push(next);
    }
    Some(layers)
}

/// Exact label-independent invariant: (vertex count, edge count, sorted
/// arity profile, sorted per-vertex incidence-degree profile).
type IsoKey = (usize, usize, Vec<usize>, Vec<usize>);

/// Exact label-independent invariants used to prefilter `iso_classes`.
/// Deliberately NOT `wl_hash` — the oracle must not share code with the
/// system under test.
fn iso_invariant_key(s: &State) -> IsoKey {
    let vs = s.vertices();
    let mut arities: Vec<usize> = s.edges.iter().map(|e| e.len()).collect();
    arities.sort_unstable();
    let mut deg = vec![0usize; vs.len()];
    for e in &s.edges {
        for v in e {
            deg[vs.binary_search(v).unwrap()] += 1;
        }
    }
    deg.sort_unstable();
    (vs.len(), s.edges.len(), arities, deg)
}

/// Partition a layer of states into isomorphism classes using
/// `iso_bruteforce` within invariant-key groups. Returns the classes as
/// index lists into `layer`, in a deterministic order.
pub fn iso_classes(layer: &[State]) -> Vec<Vec<usize>> {
    let mut groups: BTreeMap<IsoKey, Vec<usize>> = BTreeMap::new();
    for (i, s) in layer.iter().enumerate() {
        groups.entry(iso_invariant_key(s)).or_default().push(i);
    }
    let mut classes: Vec<Vec<usize>> = Vec::new();
    for members in groups.values() {
        let mut group_classes: Vec<Vec<usize>> = Vec::new();
        for &i in members {
            let mut placed = false;
            for class in group_classes.iter_mut() {
                if iso_bruteforce(&layer[class[0]], &layer[i]) {
                    class.push(i);
                    placed = true;
                    break;
                }
            }
            if !placed {
                group_classes.push(vec![i]);
            }
        }
        classes.extend(group_classes);
    }
    classes
}

/// A brute-force match: consumed edge indices plus the variable binding.
pub type BruteMatch = (Vec<usize>, Vec<Option<Vertex>>);

/// Matcher ground truth: enumerate every ordered tuple of distinct edge
/// indices (odometer order — lexicographic in the tuple, matching
/// `find_matches`' documented enumeration order) and accept a tuple iff the
/// positional variable bindings are consistent. No backtracking, no pruning.
pub fn match_bruteforce(state: &State, rule: &Rule) -> Vec<BruteMatch> {
    let k = rule.lhs.len();
    let n = state.edges.len();
    let mut out = Vec::new();
    let mut tuple = vec![0usize; k];

    fn rec(
        pos: usize,
        k: usize,
        n: usize,
        state: &State,
        rule: &Rule,
        tuple: &mut Vec<usize>,
        out: &mut Vec<BruteMatch>,
    ) {
        if pos == k {
            // full tuple: check arities and positional binding consistency
            let mut binding: Vec<Option<Vertex>> = vec![None; rule.n_vars];
            for (p, &ei) in tuple.iter().enumerate() {
                if state.edges[ei].len() != rule.lhs[p].len() {
                    return;
                }
                for (var, v) in rule.lhs[p].iter().zip(state.edges[ei].iter()) {
                    match binding[*var] {
                        None => binding[*var] = Some(*v),
                        Some(x) if x != *v => return,
                        _ => {}
                    }
                }
            }
            out.push((tuple.clone(), binding));
            return;
        }
        for ei in 0..n {
            if tuple[..pos].contains(&ei) {
                continue;
            }
            tuple[pos] = ei;
            rec(pos + 1, k, n, state, rule, tuple, out);
        }
    }
    rec(0, k, n, state, rule, &mut tuple, &mut out);
    out
}
