//! Single-path evolution with edge provenance -> causal graph.
//!
//! Each hyperedge instance remembers which event created it (event 0 is
//! the initial condition). When event B consumes edges created by event
//! A, the causal graph gains edge A -> B. This is the standard causal
//! graph for one updating order; the multiway token-event graph (causal
//! structure across *all* orders) is on the roadmap.

use crate::hypergraph::State;
use crate::matcher::find_matches;
use crate::rule::Rule;

pub struct CausalRun {
    /// Total events including event 0 (the initial condition).
    pub n_events: usize,
    /// Causal dependencies (creator_event, consumer_event).
    pub deps: Vec<(usize, usize)>,
    pub final_state: State,
}

/// Evolve sequentially for up to `max_events` events, always applying the
/// first match in deterministic enumeration order (oldest edges first).
pub fn run(rule: &Rule, init: State, max_events: usize) -> CausalRun {
    let mut state = init;
    let mut creators: Vec<usize> = vec![0; state.edges.len()];
    let mut deps: Vec<(usize, usize)> = Vec::new();
    let mut t = 0usize;

    while t < max_events {
        let ms = find_matches(&state, rule);
        if ms.is_empty() {
            break;
        }
        let m = &ms[0];
        t += 1;
        let eid = t;

        let mut src: Vec<usize> = m.edge_idx.iter().map(|&i| creators[i]).collect();
        src.sort_unstable();
        src.dedup();
        for s in src {
            deps.push((s, eid));
        }

        let mut mask = vec![true; state.edges.len()];
        for &i in &m.edge_idx {
            mask[i] = false;
        }
        let mut new_edges = Vec::with_capacity(state.edges.len());
        let mut new_creators = Vec::with_capacity(state.edges.len());
        for (i, e) in state.edges.iter().enumerate() {
            if mask[i] {
                new_edges.push(e.clone());
                new_creators.push(creators[i]);
            }
        }
        let mut binding = m.binding.clone();
        let mut next = state.next_vertex;
        for pe in &rule.rhs {
            let e: Vec<u32> = pe
                .iter()
                .map(|&v| {
                    if binding[v].is_none() {
                        binding[v] = Some(next);
                        next += 1;
                    }
                    binding[v].unwrap()
                })
                .collect();
            new_edges.push(e);
            new_creators.push(eid);
        }
        state = State {
            edges: new_edges,
            next_vertex: next,
        };
        creators = new_creators;
    }

    CausalRun {
        n_events: t + 1,
        deps,
        final_state: state,
    }
}
