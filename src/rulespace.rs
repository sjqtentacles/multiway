//! Rule-space enumeration modulo equivalence — the scanner's substrate.
//!
//! Two rules are behaviorally identical when they differ only by (1) a
//! bijective renaming of variables applied to both sides (covers fresh
//! RHS variable naming), (2) permutation of LHS edges (the multiway
//! evolution explores ALL matches, so the match *set* — hence the
//! canonical DAG — is order-independent), or (3) permutation of RHS
//! edges (`apply_full` appends RHS edges in rule order and mints fresh
//! vertices in first-appearance order, so raw children differ only by a
//! vertex relabeling plus edge order — which canonization erases).
//! LHS/RHS are **multisets with multiplicity**: `{{x,y},{x,y}}` consumes
//! two distinct instances and is NOT `{{x,y}}`.
//!
//! [`CanonRule`] mirrors `canon.rs`'s philosophy: the canonical form is
//! the minimum over a fully specified discipline — here, over all
//! variable bijections, of the pair of `(arity, sequence)`-sorted sides.
//! With ≤ 6 variables that is ≤ 720 permutations: brute force, exact.
//!
//! ## Load-bearing counting conventions
//!
//! Every pinned constant below depends on ALL of these; any deviation
//! shifts every number:
//! - the variable universe is exactly `max_vars` names, and Burnside
//!   averages over the FULL symmetric group `S_max_vars` — rules using
//!   fewer variables, rules with fresh RHS-only variables, and rules
//!   that drop LHS variables are all counted;
//! - LHS multiset sizes run `1..=max_lhs` (an empty LHS is not a rule);
//!   RHS sizes run `0..=max_rhs` (an empty RHS is legal);
//! - edges are ALL vertex sequences of arity `min_arity..=max_arity`
//!   (repeats allowed — `(x,x)` is an edge); there are no arity-0
//!   pattern edges.
//!
//! Independently verified sizes (Burnside vs. explicit enumeration, and
//! reproduced by a second implementation during plan verification):
//! binary-only ≤ 4 vars = 6,477; arity ≤ 2, ≤ 4 vars = **18,143** (the
//! default exhaustive scan); arity ≤ 3, ≤ 4 vars = 16,184,498
//! (Burnside-only — far past the exhaustive cap; reachable by seeded
//! sampling).

use crate::det::{DetSet, SplitMix};
use crate::rule::{parse_rule, Rule};

/// A rule in canonical form: variables are dense ids `0..n_vars`, both
/// sides sorted by `(arity, sequence)`, and the whole pair is minimal
/// over all variable bijections. `canon(r) == canon(s)` ⟺ r ≡ s.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CanonRule {
    /// LHS pattern edges (variable ids), sorted side.
    pub lhs: Vec<Vec<u8>>,
    /// RHS replacement edges, sorted side.
    pub rhs: Vec<Vec<u8>>,
    /// Number of distinct variables used.
    pub n_vars: u8,
}

/// Inclusive bounds of a rule space.
#[derive(Clone, Copy, Debug)]
pub struct SpaceBudget {
    /// LHS edge-multiset sizes `1..=max_lhs`.
    pub max_lhs: usize,
    /// RHS edge-multiset sizes `0..=max_rhs`.
    pub max_rhs: usize,
    /// Smallest pattern-edge arity (≥ 1).
    pub min_arity: usize,
    /// Largest pattern-edge arity.
    pub max_arity: usize,
    /// Variable-universe size (≤ 6 enforced: canon is a ≤ 720-perm brute
    /// force).
    pub max_vars: usize,
}

impl SpaceBudget {
    fn validate(&self) {
        assert!(self.max_lhs >= 1, "LHS must allow at least one edge");
        assert!(self.min_arity >= 1, "no arity-0 pattern edges");
        assert!(self.min_arity <= self.max_arity, "empty arity range");
        assert!(
            (1..=6).contains(&self.max_vars),
            "max_vars must be 1..=6 (canon is a permutation brute force)"
        );
    }

    /// All edges over the universe, ascending `(arity, sequence)`.
    fn all_edges(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for arity in self.min_arity..=self.max_arity {
            let count = (self.max_vars as u64).pow(arity as u32);
            for mut x in 0..count {
                let mut e = vec![0u8; arity];
                for slot in e.iter_mut().rev() {
                    *slot = (x % self.max_vars as u64) as u8;
                    x /= self.max_vars as u64;
                }
                out.push(e);
            }
        }
        out
    }
}

/// A canonicalized (lhs, rhs) side pair.
type SidePair = (Vec<Vec<u8>>, Vec<Vec<u8>>);

fn sort_side(side: &mut [Vec<u8>]) {
    side.sort_by(|a, b| (a.len(), &a[..]).cmp(&(b.len(), &b[..])));
}

/// Iterate all permutations of `0..n` (Heap's algorithm), calling `f`.
fn for_each_perm(n: usize, mut f: impl FnMut(&[u8])) {
    let mut a: Vec<u8> = (0..n as u8).collect();
    f(&a);
    let mut c = vec![0usize; n];
    let mut i = 0;
    while i < n {
        if c[i] < i {
            if i % 2 == 0 {
                a.swap(0, i);
            } else {
                a.swap(c[i], i);
            }
            f(&a);
            c[i] += 1;
            i = 0;
        } else {
            c[i] = 0;
            i += 1;
        }
    }
}

fn apply_perm(side: &[Vec<u8>], perm: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = side
        .iter()
        .map(|e| e.iter().map(|&v| perm[v as usize]).collect())
        .collect();
    sort_side(&mut out);
    out
}

/// Canonicalize a `(lhs, rhs)` pair over a universe of `n_universe`
/// variable names: minimum over all bijections of the universe.
fn canon_pair(lhs: &[Vec<u8>], rhs: &[Vec<u8>], n_universe: usize) -> SidePair {
    let mut best: Option<SidePair> = None;
    for_each_perm(n_universe, |perm| {
        let cand = (apply_perm(lhs, perm), apply_perm(rhs, perm));
        match &best {
            Some(b) if *b <= cand => {}
            _ => best = Some(cand),
        }
    });
    best.expect("at least the identity permutation ran")
}

impl CanonRule {
    /// Canonicalize any parsed rule. The rule's own variable count is the
    /// universe (equivalent to any larger universe: the lexicographic
    /// minimum always packs used variables into the lowest labels).
    pub fn from_rule(r: &Rule) -> CanonRule {
        assert!(
            r.n_vars <= 8,
            "rule canonicalization is a permutation brute force (n_vars <= 8)"
        );
        let lhs: Vec<Vec<u8>> = r
            .lhs
            .iter()
            .map(|e| e.iter().map(|&v| v as u8).collect())
            .collect();
        let rhs: Vec<Vec<u8>> = r
            .rhs
            .iter()
            .map(|e| e.iter().map(|&v| v as u8).collect())
            .collect();
        let (l, rr) = canon_pair(&lhs, &rhs, r.n_vars);
        let n_vars = count_vars(&l, &rr);
        CanonRule {
            lhs: l,
            rhs: rr,
            n_vars,
        }
    }

    /// Render with variable names `a, b, c, …`; `parse_rule(text)`
    /// round-trips.
    pub fn text(&self) -> String {
        let name = |v: u8| -> char { (b'a' + v) as char };
        let side = |edges: &[Vec<u8>]| -> String {
            let inner: Vec<String> = edges
                .iter()
                .map(|e| {
                    let vs: Vec<String> = e.iter().map(|&v| name(v).to_string()).collect();
                    format!("{{{}}}", vs.join(","))
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        };
        format!("{}->{}", side(&self.lhs), side(&self.rhs))
    }

    /// Parse back into an engine [`Rule`].
    pub fn to_rule(&self) -> Rule {
        parse_rule(&self.text()).expect("canonical rule text always parses")
    }
}

fn count_vars(lhs: &[Vec<u8>], rhs: &[Vec<u8>]) -> u8 {
    let mut seen = [false; 256];
    let mut n = 0u8;
    for e in lhs.iter().chain(rhs.iter()) {
        for &v in e {
            if !seen[v as usize] {
                seen[v as usize] = true;
                n += 1;
            }
        }
    }
    n
}

/// Exact number of canonical classes in the budget — WITHOUT enumerating.
///
/// Burnside's lemma over `S_max_vars` acting simultaneously on both edge
/// multisets: for each permutation σ, count the `(L, R)` pairs it fixes.
/// A multiset is fixed by σ iff every σ-orbit of edges appears with
/// uniform multiplicity, so fixed k-multisets are counted by a DP over
/// orbit sizes; fixed pairs are the product of the two sides' counts.
pub fn space_size(b: &SpaceBudget) -> u128 {
    b.validate();
    let edges = b.all_edges();
    let mut total: u128 = 0;
    let mut n_perms: u128 = 0;

    for_each_perm(b.max_vars, |perm| {
        n_perms += 1;
        // orbit sizes of σ acting on the edge set
        let mut visited = vec![false; edges.len()];
        let index: crate::det::DetMap<&[u8], usize> = edges
            .iter()
            .enumerate()
            .map(|(i, e)| (e.as_slice(), i))
            .collect();
        let index_of = |e: &[u8]| -> usize { index[e] };
        let mut orbit_sizes: Vec<usize> = Vec::new();
        for start in 0..edges.len() {
            if visited[start] {
                continue;
            }
            let mut size = 0usize;
            let mut cur = start;
            loop {
                visited[cur] = true;
                size += 1;
                let img: Vec<u8> = edges[cur].iter().map(|&v| perm[v as usize]).collect();
                cur = index_of(&img[..]);
                if cur == start {
                    break;
                }
            }
            orbit_sizes.push(size);
        }
        // DP: ways[j] = #multisets of total size j with uniform
        // multiplicity per orbit
        let max_k = b.max_lhs.max(b.max_rhs);
        let mut ways = vec![0u128; max_k + 1];
        ways[0] = 1;
        for &s in &orbit_sizes {
            for j in (0..=max_k).rev() {
                let mut m = 1;
                while m * s <= j {
                    let add = ways[j - m * s];
                    ways[j] += add;
                    m += 1;
                }
            }
        }
        let fixed_lhs: u128 = (1..=b.max_lhs).map(|k| ways[k]).sum();
        let fixed_rhs: u128 = (0..=b.max_rhs).map(|k| ways[k]).sum();
        total += fixed_lhs * fixed_rhs;
    });

    assert_eq!(total % n_perms, 0, "Burnside sum must divide evenly");
    total / n_perms
}

/// Exhaustively enumerate the canonical classes, strictly ascending in
/// `CanonRule`'s derived order. A pair is emitted iff it equals its own
/// canon — no dedup set needed; the final sort normalizes the
/// generation order (multiset generation groups by size, which is not
/// the derived lexicographic order).
pub fn enumerate(b: &SpaceBudget) -> Vec<CanonRule> {
    b.validate();
    let edges = b.all_edges();
    let lhs_multisets = multisets(&edges, 1, b.max_lhs);
    let rhs_multisets = multisets(&edges, 0, b.max_rhs);
    let mut out = Vec::new();
    for l in &lhs_multisets {
        for r in &rhs_multisets {
            let (cl, cr) = canon_pair(l, r, b.max_vars);
            if &cl == l && &cr == r {
                let n_vars = count_vars(l, r);
                out.push(CanonRule {
                    lhs: l.clone(),
                    rhs: r.clone(),
                    n_vars,
                });
            }
        }
    }
    out.sort();
    out
}

/// All multisets of `edges` with sizes `min_k..=max_k`, each as a
/// non-decreasing index tuple materialized to edges, ascending order.
fn multisets(edges: &[Vec<u8>], min_k: usize, max_k: usize) -> Vec<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    for k in min_k..=max_k {
        if k == 0 {
            out.push(Vec::new());
            continue;
        }
        if edges.is_empty() {
            continue;
        }
        let mut idx = vec![0usize; k];
        loop {
            out.push(idx.iter().map(|&i| edges[i].clone()).collect());
            // advance to the next non-decreasing tuple
            let mut i = k;
            let mut advanced = false;
            while i > 0 {
                i -= 1;
                if idx[i] + 1 < edges.len() {
                    let v = idx[i] + 1;
                    for slot in idx.iter_mut().skip(i) {
                        *slot = v;
                    }
                    advanced = true;
                    break;
                }
            }
            if !advanced {
                break;
            }
        }
    }
    out
}

/// Seeded sampling of large spaces: draw random pairs, canonicalize,
/// dedup, until `n` distinct classes or a deterministic attempt cap
/// (`n * 200`) is exhausted. Same `(budget, n, seed)` ⇒ same output.
pub fn sample(b: &SpaceBudget, n: usize, seed: u64) -> Vec<CanonRule> {
    b.validate();
    let edges = b.all_edges();
    let mut rng = SplitMix(seed);
    let mut seen: DetSet<CanonRule> = DetSet::default();
    let mut out: Vec<CanonRule> = Vec::new();
    let mut attempts = 0usize;
    let cap = n.saturating_mul(200).max(1000);
    while out.len() < n && attempts < cap {
        attempts += 1;
        let lk = 1 + (rng.next_u64() as usize) % b.max_lhs;
        let rk = (rng.next_u64() as usize) % (b.max_rhs + 1);
        let mut l: Vec<Vec<u8>> = (0..lk)
            .map(|_| edges[(rng.next_u64() as usize) % edges.len()].clone())
            .collect();
        let mut r: Vec<Vec<u8>> = (0..rk)
            .map(|_| edges[(rng.next_u64() as usize) % edges.len()].clone())
            .collect();
        sort_side(&mut l);
        sort_side(&mut r);
        let (cl, cr) = canon_pair(&l, &r, b.max_vars);
        let n_vars = count_vars(&cl, &cr);
        let c = CanonRule {
            lhs: cl,
            rhs: cr,
            n_vars,
        };
        if seen.insert(c.clone()) {
            out.push(c);
        }
    }
    out
}
