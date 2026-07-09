//! Static rule analysis: syntactic conservation checks.
//!
//! This is the honest v0 of the "typed rule layer" roadmap item — checks,
//! not type-level guarantees. Its one load-bearing hook:
//! `terminating_by_edge_count` (every application strictly decreases edge
//! count, a well-founded measure) is what licenses the confluence
//! checker's Newman upgrade from "all critical pairs strongly joinable"
//! to `confluent: true`.

use crate::rule::Rule;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write;

/// Syntactic facts about a rule.
pub struct RuleLint {
    /// RHS edge count minus LHS edge count (per application).
    pub edge_delta: i64,
    /// Per-arity edge-count delta, sorted by arity; zero entries dropped.
    pub arity_histogram_delta: Vec<(usize, i64)>,
    /// RHS-only variables — fresh vertices minted per application.
    pub fresh_vertices_per_event: usize,
    /// LHS variables absent from the RHS: the matched vertices survive
    /// only if other edges still reference them.
    pub possibly_orphaned_vars: Vec<String>,
    /// Edge count strictly decreases — a well-founded termination measure.
    pub terminating_by_edge_count: bool,
    /// Total vertex slots (sum of arities) RHS minus LHS.
    pub vertex_slot_delta: i64,
}

/// Analyze a rule.
pub fn lint(rule: &Rule) -> RuleLint {
    let lhs_vars: BTreeSet<usize> = rule.lhs.iter().flatten().copied().collect();
    let rhs_vars: BTreeSet<usize> = rule.rhs.iter().flatten().copied().collect();

    let mut histo: BTreeMap<usize, i64> = BTreeMap::new();
    for e in &rule.lhs {
        *histo.entry(e.len()).or_insert(0) -= 1;
    }
    for e in &rule.rhs {
        *histo.entry(e.len()).or_insert(0) += 1;
    }
    let arity_histogram_delta: Vec<(usize, i64)> =
        histo.into_iter().filter(|&(_, d)| d != 0).collect();

    let edge_delta = rule.rhs.len() as i64 - rule.lhs.len() as i64;
    RuleLint {
        edge_delta,
        arity_histogram_delta,
        fresh_vertices_per_event: rhs_vars.difference(&lhs_vars).count(),
        possibly_orphaned_vars: lhs_vars
            .difference(&rhs_vars)
            .map(|&v| rule.var_names[v].clone())
            .collect(),
        terminating_by_edge_count: edge_delta < 0,
        vertex_slot_delta: rule.rhs.iter().map(|e| e.len() as i64).sum::<i64>()
            - rule.lhs.iter().map(|e| e.len() as i64).sum::<i64>(),
    }
}

impl RuleLint {
    /// Deterministic text rendering for the CLI.
    pub fn render(&self, rule: &Rule) -> String {
        let mut out = String::new();
        writeln!(out, "rule {}", rule.text).unwrap();
        writeln!(out, "  edge delta per event      {:+}", self.edge_delta).unwrap();
        writeln!(
            out,
            "  vertex-slot delta         {:+}",
            self.vertex_slot_delta
        )
        .unwrap();
        writeln!(
            out,
            "  fresh vertices per event  {}",
            self.fresh_vertices_per_event
        )
        .unwrap();
        if !self.arity_histogram_delta.is_empty() {
            let parts: Vec<String> = self
                .arity_histogram_delta
                .iter()
                .map(|(a, d)| format!("arity {} {:+}", a, d))
                .collect();
            writeln!(out, "  arity histogram delta     {}", parts.join(", ")).unwrap();
        }
        if !self.possibly_orphaned_vars.is_empty() {
            writeln!(
                out,
                "  possibly orphaned vars    {}",
                self.possibly_orphaned_vars.join(", ")
            )
            .unwrap();
        }
        writeln!(
            out,
            "  terminating (edge count)  {}",
            if self.terminating_by_edge_count {
                "yes — every event strictly decreases edge count"
            } else {
                "not proven"
            }
        )
        .unwrap();
        out
    }
}
