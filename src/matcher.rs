//! Sub-hypergraph pattern matching and rule application.

use crate::hypergraph::{State, Vertex};
use crate::rule::Rule;

/// A single match of a rule's LHS: which edge instances were consumed
/// (parallel to `rule.lhs`), and what each variable bound to. RHS-only
/// variables remain `None` until application mints fresh vertices.
#[derive(Clone, Debug)]
pub struct Match {
    pub edge_idx: Vec<usize>,
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

/// Apply a match: remove consumed edge instances, append RHS edges with
/// bound variables, minting fresh vertices for RHS-only variables in
/// deterministic order.
pub fn apply(state: &State, rule: &Rule, m: &Match) -> State {
    let mut mask = vec![true; state.edges.len()];
    for &i in &m.edge_idx {
        mask[i] = false;
    }
    let mut edges: Vec<Vec<Vertex>> = Vec::with_capacity(state.edges.len() + rule.rhs.len());
    for (i, e) in state.edges.iter().enumerate() {
        if mask[i] {
            edges.push(e.clone());
        }
    }
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
    State {
        edges,
        next_vertex: next,
    }
}
