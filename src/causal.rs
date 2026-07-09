//! Single-path evolution with edge provenance -> causal graph.
//!
//! Each hyperedge instance remembers which event created it (event 0 is
//! the initial condition). When event B consumes edges created by event
//! A, the causal graph gains edge A -> B. This is the standard causal
//! graph for one updating order; the multiway token-event graph (causal
//! structure across *all* orders) is on the roadmap.

use crate::hypergraph::State;
use crate::matcher::{apply_full, delta_matches, find_matches};
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
/// Match sets are delta-maintained ([`delta_matches`] reproduces the full
/// search byte-exactly), so long runs never re-search the whole state.
pub fn run(rule: &Rule, init: State, max_events: usize) -> CausalRun {
    let mut state = init;
    let mut creators: Vec<usize> = vec![0; state.edges.len()];
    let mut deps: Vec<(usize, usize)> = Vec::new();
    let mut ms = find_matches(&state, rule);
    let mut t = 0usize;

    while t < max_events {
        if ms.is_empty() {
            break;
        }
        let m = ms[0].clone();
        t += 1;
        let eid = t;

        let mut src: Vec<usize> = m.edge_idx.iter().map(|&i| creators[i]).collect();
        src.sort_unstable();
        src.dedup();
        for s in src {
            deps.push((s, eid));
        }

        let app = apply_full(&state, rule, &m);
        let mut new_creators = vec![eid; app.child.edges.len()];
        for &(pi, ci) in &app.kept {
            new_creators[ci] = creators[pi];
        }
        ms = delta_matches(rule, &ms, &m, &app);
        state = app.child;
        creators = new_creators;
    }

    CausalRun {
        n_events: t + 1,
        deps,
        final_state: state,
    }
}
