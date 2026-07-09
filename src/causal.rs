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

/// One deterministic history: its causal dependencies and final state.
pub struct CausalRun {
    /// Total events including event 0 (the initial condition).
    pub n_events: usize,
    /// Causal dependencies (creator_event, consumer_event).
    pub deps: Vec<(usize, usize)>,
    /// The state after the last applied event.
    pub final_state: State,
    /// Events per generation: `[1; n]` for sequential runs; for the
    /// standard updating order, the size of each maximal disjoint set.
    /// The generation layering is one antichain decomposition of the
    /// causal DAG — the scheduler view of causal structure.
    pub generations: Vec<usize>,
}

/// Which single-history updating order to run.
#[derive(Clone, Copy, Debug)]
pub enum UpdateOrder {
    /// One event at a time: always the first match in deterministic
    /// enumeration order.
    Sequential,
    /// Wolfram's standard updating order: per generation, the greedy
    /// maximal set of pairwise edge-disjoint matches in enumeration
    /// order, applied simultaneously (fresh vertices minted per event in
    /// acceptance order).
    StandardGenerations,
}

/// Evolve sequentially for up to `max_events` events, always applying the
/// first match in deterministic enumeration order (oldest edges first).
/// Match sets are delta-maintained ([`delta_matches`] reproduces the full
/// search byte-exactly), so long runs never re-search the whole state.
pub fn run(rule: &Rule, init: State, max_events: usize) -> CausalRun {
    run_ordered(rule, init, max_events, UpdateOrder::Sequential)
}

/// Evolve one history under the chosen updating order.
pub fn run_ordered(rule: &Rule, init: State, max_events: usize, order: UpdateOrder) -> CausalRun {
    match order {
        UpdateOrder::Sequential => run_sequential(rule, init, max_events),
        UpdateOrder::StandardGenerations => run_generations(rule, init, max_events),
    }
}

fn run_sequential(rule: &Rule, init: State, max_events: usize) -> CausalRun {
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
        generations: vec![1; t],
    }
}

/// Standard updating order: greedy maximal pairwise edge-disjoint match
/// sets, applied as simultaneous generations.
fn run_generations(rule: &Rule, init: State, max_events: usize) -> CausalRun {
    let mut state = init;
    let mut creators: Vec<usize> = vec![0; state.edges.len()];
    let mut deps: Vec<(usize, usize)> = Vec::new();
    let mut generations: Vec<usize> = Vec::new();
    let mut t = 0usize;

    while t < max_events {
        // Full search per generation: delta maintenance is defined for
        // one application at a time and does not compose across a
        // simultaneous multi-event step. Generations are few; nothing of
        // consequence is lost.
        let ms = find_matches(&state, rule);

        // greedy maximal pairwise-disjoint set, in enumeration order
        let mut taken = vec![false; state.edges.len()];
        let mut selected: Vec<&crate::matcher::Match> = Vec::new();
        for m in &ms {
            if m.edge_idx.iter().any(|&i| taken[i]) {
                continue;
            }
            for &i in &m.edge_idx {
                taken[i] = true;
            }
            selected.push(m);
            if t + selected.len() >= max_events {
                break;
            }
        }
        if selected.is_empty() {
            break;
        }

        // apply the generation simultaneously: consume the union, then
        // append each event's RHS in acceptance order
        let mut mask = vec![true; state.edges.len()];
        for m in &selected {
            for &i in &m.edge_idx {
                mask[i] = false;
            }
        }
        let mut new_edges: Vec<Vec<u32>> = Vec::with_capacity(state.edges.len());
        let mut new_creators: Vec<usize> = Vec::with_capacity(state.edges.len());
        for (i, e) in state.edges.iter().enumerate() {
            if mask[i] {
                new_edges.push(e.clone());
                new_creators.push(creators[i]);
            }
        }
        let mut next = state.next_vertex;
        let gen_size = selected.len();
        for m in &selected {
            t += 1;
            let eid = t;
            let mut src: Vec<usize> = m.edge_idx.iter().map(|&i| creators[i]).collect();
            src.sort_unstable();
            src.dedup();
            for s in src {
                deps.push((s, eid));
            }
            let mut binding = m.binding.clone();
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
        }
        state = State {
            edges: new_edges,
            next_vertex: next,
        };
        creators = new_creators;
        generations.push(gen_size);
    }

    CausalRun {
        n_events: t + 1,
        deps,
        final_state: state,
        generations,
    }
}
