//! Causal-invariance / confluence checking via critical pairs and bounded
//! strong joinability.
//!
//! ## What may honestly be claimed
//!
//! The top verdict is [`Verdict::AllCriticalPairsStronglyJoinable`] —
//! deliberately NOT named "locally confluent": the critical-pair lemma
//! for this formalism (multiset ordered hyperedges, non-injective
//! binding) is a documentation roadmap item, not a code claim. What the
//! checker establishes mechanically:
//!
//! - every enumerated critical pair reconverges within the bound, under
//!   **strong** joinability (the join respects pinned host vertices, so
//!   it survives embedding into any context — plain joinability does not:
//!   `{{x,y}}->{{x}}` vs `{{x,y}}->{{y}}` join plainly but diverge in
//!   context, see the pinned test);
//! - `confluent: true` additionally requires the termination lint (every
//!   rule strictly edge-decreasing — a well-founded measure — so Newman's
//!   lemma applies);
//! - [`Verdict::NotConfluent`] is reported ONLY on double saturation:
//!   both branches exhaustively enumerated (no bound hit) with disjoint
//!   reachable sets — the host is a legal initial state exhibiting an
//!   unjoinable divergence;
//! - a bound hit is always [`Verdict::Inconclusive`]. Note that
//!   edge-growing rules with nontrivial overlaps (e.g. the classic rule)
//!   can never saturate, so their honest ceiling is `Inconclusive`;
//!   zero-pair growing rules get the vacuous
//!   `AllCriticalPairsStronglyJoinable { pairs_checked: 0, confluent: false }`.
//!
//! Pin identity flows through exact colored canonization
//! ([`crate::canon::canonicalize_colored`]) — never through a hash — so a
//! collision can never falsely identify differently-pinned states.

use crate::canon::{canonical_form, canonicalize_colored};
use crate::det::DetSet;
use crate::hypergraph::{Edge, State, Vertex};
use crate::lint::lint;
use crate::matcher::{apply, find_matches, Match};
use crate::rule::Rule;
use std::fmt::Write;

/// A critical pair: two overlapping rule applications on a minimal host.
#[derive(Clone, Debug)]
pub struct CriticalPair {
    /// Index of the first rule in the checked rule set.
    pub rule1: usize,
    /// Index of the second rule (>= rule1).
    pub rule2: usize,
    /// The minimal host state exhibiting the overlap.
    pub host: State,
    /// The first rule's match on the host.
    pub m1: Match,
    /// The second rule's match on the host (shares >= 1 edge instance
    /// with `m1`).
    pub m2: Match,
}

/// Why a check ended without a verdict either way.
#[derive(Clone, Debug)]
pub enum InconclusiveReason {
    /// The joinability search hit its depth or state budget.
    BoundHit {
        /// Levels explored per side when the budget ran out.
        depth: usize,
        /// States enumerated across both sides.
        states: usize,
    },
    /// Both sides saturated and join plainly (up to isomorphism) but NOT
    /// strongly (respecting pinned host vertices) — which licenses no
    /// local-confluence claim (the join does not survive contexts).
    WeakOnly,
}

/// Checker outcome.
#[derive(Debug)]
pub enum Verdict {
    /// Every nontrivial critical pair strongly joinable within the bound.
    /// Evidence for local confluence, not a proof (see module docs).
    AllCriticalPairsStronglyJoinable {
        /// Number of nontrivial critical pairs checked.
        pairs_checked: usize,
        /// Deepest join found.
        max_join_depth: usize,
        /// True only when the termination lint also holds for every rule
        /// (Newman's lemma).
        confluent: bool,
    },
    /// A genuine counterexample: `pair.host` diverges to `s1` vs `s2`,
    /// both branches exhaustively saturated and disjoint.
    NotConfluent {
        /// The offending critical pair.
        pair: Box<CriticalPair>,
        /// First branch's immediate result.
        s1: State,
        /// Second branch's immediate result.
        s2: State,
    },
    /// No verdict at the configured bounds.
    Inconclusive {
        /// The first pair that failed to resolve.
        pair: Box<CriticalPair>,
        /// Why.
        reason: InconclusiveReason,
    },
}

/// Bounds for the joinability search.
pub struct CheckCfg {
    /// BFS levels per side.
    pub join_depth: usize,
    /// Total states enumerated per side.
    pub max_states: usize,
    /// Hard cap on enumerated critical pairs (guards pathological rule
    /// sets; exceeding it is an error, not a silent truncation).
    pub pair_cap: usize,
}

impl Default for CheckCfg {
    fn default() -> Self {
        CheckCfg {
            join_depth: 8,
            max_states: 2000,
            pair_cap: 512,
        }
    }
}

/// The checker's report.
#[derive(Debug)]
pub struct ConfluenceReport {
    /// Final verdict (worst pair wins: NotConfluent > Inconclusive > all-strong).
    pub verdict: Verdict,
    /// Total nontrivial critical pairs enumerated.
    pub pairs_total: usize,
}

/// Enumerate every nontrivial critical pair of the rule set: for each
/// rule pair (r1 <= r2), every nonempty arity-matched partial injection
/// between their LHS edge lists (multiset semantics — each shared edge is
/// one instance), with variables unified positionwise (union-find; no
/// occurs-checks needed since every pattern symbol is a variable). The
/// trivial diagonal (same rule, total identification, identical induced
/// matches) is skipped.
pub fn critical_pairs(rules: &[Rule]) -> Result<Vec<CriticalPair>, String> {
    let mut out = Vec::new();
    for r1 in 0..rules.len() {
        for r2 in r1..rules.len() {
            enumerate_pair(rules, r1, r2, &mut out);
        }
    }
    Ok(out)
}

fn enumerate_pair(rules: &[Rule], r1: usize, r2: usize, out: &mut Vec<CriticalPair>) {
    let (l1, l2) = (&rules[r1].lhs, &rules[r2].lhs);
    let mut sigma: Vec<Option<usize>> = vec![None; l1.len()];
    let mut used = vec![false; l2.len()];
    rec_sigma(rules, r1, r2, 0, &mut sigma, &mut used, out);

    fn rec_sigma(
        rules: &[Rule],
        r1: usize,
        r2: usize,
        i: usize,
        sigma: &mut Vec<Option<usize>>,
        used: &mut Vec<bool>,
        out: &mut Vec<CriticalPair>,
    ) {
        let l1 = &rules[r1].lhs;
        let l2 = &rules[r2].lhs;
        if i == l1.len() {
            if sigma.iter().any(|s| s.is_some()) {
                if let Some(pair) = build_pair(rules, r1, r2, sigma) {
                    out.push(pair);
                }
            }
            return;
        }
        // choice: edge i unshared…
        rec_sigma(rules, r1, r2, i + 1, sigma, used, out);
        // …or identified with any unused arity-matched l2 edge
        for j in 0..l2.len() {
            if used[j] || l2[j].len() != l1[i].len() {
                continue;
            }
            used[j] = true;
            sigma[i] = Some(j);
            rec_sigma(rules, r1, r2, i + 1, sigma, used, out);
            sigma[i] = None;
            used[j] = false;
        }
    }
}

/// Build the host + both matches for one identification map, or None for
/// the trivial diagonal.
fn build_pair(
    rules: &[Rule],
    r1: usize,
    r2: usize,
    sigma: &[Option<usize>],
) -> Option<CriticalPair> {
    let rule1 = &rules[r1];
    let rule2 = &rules[r2];
    let (n1, n2) = (rule1.n_vars, rule2.n_vars);

    // union-find over r1 vars (0..n1) ⊎ r2 vars (n1..n1+n2)
    let mut uf: Vec<usize> = (0..n1 + n2).collect();
    fn find(uf: &mut [usize], mut x: usize) -> usize {
        while uf[x] != x {
            uf[x] = uf[uf[x]];
            x = uf[x];
        }
        x
    }
    for (i, s) in sigma.iter().enumerate() {
        if let Some(j) = s {
            for (p, q) in rule1.lhs[i].iter().zip(rule2.lhs[*j].iter()) {
                let (a, b) = (find(&mut uf, *p), find(&mut uf, n1 + *q));
                if a != b {
                    uf[a] = b;
                }
            }
        }
    }

    // host: one fresh vertex per class, labels in discovery order along
    // the host edge construction (lhs1 edges, then unshared lhs2 edges)
    let mut label_of_class: Vec<Option<Vertex>> = vec![None; n1 + n2];
    let mut next_label: Vertex = 0;
    let mut label = |var: usize, uf: &mut Vec<usize>, label_of_class: &mut Vec<Option<Vertex>>| {
        let c = find(uf, var);
        *label_of_class[c].get_or_insert_with(|| {
            let l = next_label;
            next_label += 1;
            l
        })
    };

    let mut host_edges: Vec<Edge> = Vec::new();
    let mut m1_edge_idx: Vec<usize> = Vec::new();
    for (i, pe) in rule1.lhs.iter().enumerate() {
        m1_edge_idx.push(host_edges.len());
        let _ = i;
        host_edges.push(
            pe.iter()
                .map(|&v| label(v, &mut uf, &mut label_of_class))
                .collect(),
        );
    }
    let mut m2_edge_idx: Vec<usize> = vec![usize::MAX; rule2.lhs.len()];
    for (i, s) in sigma.iter().enumerate() {
        if let Some(j) = s {
            m2_edge_idx[*j] = i; // shared instance: host index of lhs1 edge i
        }
    }
    for (j, pe) in rule2.lhs.iter().enumerate() {
        if m2_edge_idx[j] == usize::MAX {
            m2_edge_idx[j] = host_edges.len();
            host_edges.push(
                pe.iter()
                    .map(|&v| label(n1 + v, &mut uf, &mut label_of_class))
                    .collect(),
            );
        }
    }
    let host = State::new(host_edges);

    // reconstruct real Match values (RHS-only vars stay None)
    let lhs_vars1: Vec<bool> = {
        let mut b = vec![false; n1];
        rule1.lhs.iter().flatten().for_each(|&v| b[v] = true);
        b
    };
    let lhs_vars2: Vec<bool> = {
        let mut b = vec![false; n2];
        rule2.lhs.iter().flatten().for_each(|&v| b[v] = true);
        b
    };
    let m1 = Match {
        edge_idx: m1_edge_idx,
        binding: (0..n1)
            .map(|v| lhs_vars1[v].then(|| label(v, &mut uf, &mut label_of_class)))
            .collect(),
    };
    let m2 = Match {
        edge_idx: m2_edge_idx,
        binding: (0..n2)
            .map(|v| lhs_vars2[v].then(|| label(n1 + v, &mut uf, &mut label_of_class)))
            .collect(),
    };

    // trivial diagonal: same rule, total identification, identical matches
    if r1 == r2 && m1.edge_idx == m2.edge_idx && m1.binding == m2.binding {
        return None;
    }

    debug_assert!(
        {
            let real: Vec<(Vec<usize>, Vec<Option<Vertex>>)> = find_matches(&host, rule1)
                .into_iter()
                .map(|m| (m.edge_idx, m.binding))
                .collect();
            real.contains(&(m1.edge_idx.clone(), m1.binding.clone()))
        },
        "reconstructed m1 is not a real match"
    );

    Some(CriticalPair {
        rule1: r1,
        rule2: r2,
        host,
        m1,
        m2,
    })
}

/// Colored key: canonical form + pinned-vertex colors. Host vertices get
/// distinct pins; fresh vertices are indistinct (color 0).
type ColoredKey = (Vec<Edge>, Vec<u64>);

fn colored_key(s: &State, host_next: Vertex) -> ColoredKey {
    let cc = canonicalize_colored(s, &|v| {
        if v < host_next {
            v as u64 + 1
        } else {
            0
        }
    });
    (cc.canon.form.edges, cc.label_colors)
}

enum PairOutcome {
    Strong { depth: usize },
    Weak,
    Disjoint { s1: State, s2: State },
    Bound { depth: usize, states: usize },
}

struct Side {
    frontier: Vec<State>,
    colored: DetSet<ColoredKey>,
    plain: DetSet<Vec<Edge>>,
    saturated: bool,
    total: usize,
}

impl Side {
    fn new(start: State, host_next: Vertex) -> Side {
        let mut colored = DetSet::default();
        colored.insert(colored_key(&start, host_next));
        let mut plain = DetSet::default();
        plain.insert(canonical_form(&start).edges);
        Side {
            frontier: vec![start],
            colored,
            plain,
            saturated: false,
            total: 1,
        }
    }

    /// Expand one BFS level: all rules, all matches, dedup by colored key.
    /// Returns false when the state budget is exhausted.
    fn expand(&mut self, rules: &[Rule], host_next: Vertex, max_states: usize) -> bool {
        let mut next: Vec<State> = Vec::new();
        for s in &self.frontier {
            for rule in rules {
                for m in find_matches(s, rule) {
                    let child = apply(s, rule, &m);
                    let key = colored_key(&child, host_next);
                    if self.colored.contains(&key) {
                        continue;
                    }
                    self.total += 1;
                    if self.total > max_states {
                        return false;
                    }
                    self.colored.insert(key);
                    self.plain.insert(canonical_form(&child).edges);
                    next.push(child);
                }
            }
        }
        self.saturated = next.is_empty();
        self.frontier = next;
        true
    }

    fn intersects(&self, other: &Side) -> bool {
        let (small, big) = if self.colored.len() <= other.colored.len() {
            (&self.colored, &other.colored)
        } else {
            (&other.colored, &self.colored)
        };
        small.iter().any(|k| big.contains(k))
    }

    fn intersects_plain(&self, other: &Side) -> bool {
        let (small, big) = if self.plain.len() <= other.plain.len() {
            (&self.plain, &other.plain)
        } else {
            (&other.plain, &self.plain)
        };
        small.iter().any(|k| big.contains(k))
    }
}

fn join_pair(rules: &[Rule], pair: &CriticalPair, cfg: &CheckCfg) -> PairOutcome {
    let host_next = pair.host.next_vertex;
    let s1 = apply(&pair.host, &rules[pair.rule1], &pair.m1);
    let s2 = apply(&pair.host, &rules[pair.rule2], &pair.m2);

    let mut a = Side::new(s1.clone(), host_next);
    let mut b = Side::new(s2.clone(), host_next);
    if a.intersects(&b) {
        return PairOutcome::Strong { depth: 0 };
    }

    for depth in 1..=cfg.join_depth {
        let mut progressed = false;
        let mut exhausted = false;
        if !a.saturated {
            exhausted |= !a.expand(rules, host_next, cfg.max_states);
            progressed = true;
        }
        if !exhausted && !b.saturated {
            exhausted |= !b.expand(rules, host_next, cfg.max_states);
            progressed = true;
        }
        if exhausted {
            return PairOutcome::Bound {
                depth,
                states: a.total + b.total,
            };
        }
        if a.intersects(&b) {
            return PairOutcome::Strong { depth };
        }
        if a.saturated && b.saturated {
            return if a.intersects_plain(&b) {
                PairOutcome::Weak
            } else {
                PairOutcome::Disjoint { s1, s2 }
            };
        }
        if !progressed {
            break;
        }
    }
    PairOutcome::Bound {
        depth: cfg.join_depth,
        states: a.total + b.total,
    }
}

/// Plain joinability (up to isomorphism, no pins): do the two states
/// reach a common canonical form within the bounds? This is the right
/// notion for divergences from a *concrete* shared state (used by the
/// certified-rule fuzz oracle) — strong joinability is only needed for
/// critical-pair hosts that stand for arbitrary contexts.
pub fn plainly_joinable(
    rules: &[Rule],
    s1: &State,
    s2: &State,
    depth: usize,
    max_states: usize,
) -> bool {
    // pin nothing: host_next = 0 makes every vertex color 0
    let mut a = Side::new(s1.clone(), 0);
    let mut b = Side::new(s2.clone(), 0);
    if a.intersects_plain(&b) {
        return true;
    }
    for _ in 1..=depth {
        for side in [&mut a, &mut b] {
            if !side.saturated && !side.expand(rules, 0, max_states) {
                return false;
            }
        }
        if a.intersects_plain(&b) {
            return true;
        }
        if a.saturated && b.saturated {
            return false;
        }
    }
    false
}

/// Run the checker over a rule set.
pub fn check(rules: &[Rule], cfg: &CheckCfg) -> Result<ConfluenceReport, String> {
    let pairs = critical_pairs(rules)?;
    if pairs.len() > cfg.pair_cap {
        return Err(format!(
            "{} critical pairs exceeds the cap of {} — raise CheckCfg::pair_cap",
            pairs.len(),
            cfg.pair_cap
        ));
    }

    let mut max_join_depth = 0usize;
    let mut first_inconclusive: Option<(usize, InconclusiveReason)> = None;
    for (i, pair) in pairs.iter().enumerate() {
        match join_pair(rules, pair, cfg) {
            PairOutcome::Strong { depth } => max_join_depth = max_join_depth.max(depth),
            PairOutcome::Weak => {
                first_inconclusive.get_or_insert((i, InconclusiveReason::WeakOnly));
            }
            PairOutcome::Bound { depth, states } => {
                first_inconclusive
                    .get_or_insert((i, InconclusiveReason::BoundHit { depth, states }));
            }
            PairOutcome::Disjoint { s1, s2 } => {
                return Ok(ConfluenceReport {
                    verdict: Verdict::NotConfluent {
                        pair: Box::new(pair.clone()),
                        s1,
                        s2,
                    },
                    pairs_total: pairs.len(),
                });
            }
        }
    }

    let verdict = match first_inconclusive {
        Some((i, reason)) => Verdict::Inconclusive {
            pair: Box::new(pairs[i].clone()),
            reason,
        },
        None => {
            // Newman upgrade: all pairs strongly joinable AND every rule
            // strictly edge-decreasing (well-founded measure).
            let terminating =
                !rules.is_empty() && rules.iter().all(|r| lint(r).terminating_by_edge_count);
            Verdict::AllCriticalPairsStronglyJoinable {
                pairs_checked: pairs.len(),
                max_join_depth,
                confluent: terminating,
            }
        }
    };
    Ok(ConfluenceReport {
        verdict,
        pairs_total: pairs.len(),
    })
}

impl ConfluenceReport {
    /// Deterministic text rendering for the CLI.
    pub fn render(&self, rules: &[Rule]) -> String {
        let mut out = String::new();
        writeln!(
            out,
            "confluence check: {} rule{}, {} nontrivial critical pair{}",
            rules.len(),
            if rules.len() == 1 { "" } else { "s" },
            self.pairs_total,
            if self.pairs_total == 1 { "" } else { "s" }
        )
        .unwrap();
        match &self.verdict {
            Verdict::AllCriticalPairsStronglyJoinable {
                pairs_checked,
                max_join_depth,
                confluent,
            } => {
                writeln!(
                    out,
                    "verdict: all critical pairs strongly joinable ({} checked, max join depth {})",
                    pairs_checked, max_join_depth
                )
                .unwrap();
                if *confluent {
                    writeln!(
                        out,
                        "confluent: YES — every rule strictly decreases edge count \
                         (termination) + strong joinability (Newman)"
                    )
                    .unwrap();
                } else {
                    writeln!(
                        out,
                        "confluent: not proven — termination not established \
                         (this is evidence of local confluence, not a proof)"
                    )
                    .unwrap();
                }
            }
            Verdict::NotConfluent { pair, s1, s2 } => {
                writeln!(out, "verdict: NOT CONFLUENT — genuine counterexample").unwrap();
                writeln!(out, "  host      {}", pair.host.to_notation()).unwrap();
                writeln!(out, "  branch 1  {}", s1.to_notation()).unwrap();
                writeln!(out, "  branch 2  {}", s2.to_notation()).unwrap();
                writeln!(
                    out,
                    "  both branches saturated with disjoint reachable sets"
                )
                .unwrap();
            }
            Verdict::Inconclusive { pair, reason } => {
                writeln!(out, "verdict: INCONCLUSIVE").unwrap();
                writeln!(out, "  host      {}", pair.host.to_notation()).unwrap();
                match reason {
                    InconclusiveReason::BoundHit { depth, states } => writeln!(
                        out,
                        "  bound hit at depth {} ({} states explored) — raise --join-depth/--max-states",
                        depth, states
                    )
                    .unwrap(),
                    InconclusiveReason::WeakOnly => writeln!(
                        out,
                        "  joins up to isomorphism but NOT strongly (pinned vertices \
                         diverge) — no local-confluence claim is licensed"
                    )
                    .unwrap(),
                }
            }
        }
        out
    }
}
