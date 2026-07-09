//! Multiway evolution with canonical-state sharing.
//!
//! The naive multiway object is a tree: every state branches into one
//! child per match. We instead merge isomorphic states globally, so the
//! result is a DAG over canonical states — the e-graph move applied at
//! state granularity. `path_counts` reports how many naive tree paths
//! each canonical node absorbs, i.e. how much work the sharing saved.

use crate::canon::{canonicalize, Canon};
use crate::det::DetMap;
use crate::hypergraph::{Edge, State};
use crate::matcher::{apply_full, delta_matches, find_matches, Match};
use crate::rule::Rule;

pub struct StateRec {
    pub id: usize,
    /// Step at which this canonical state was first reached.
    pub step: usize,
    /// Raw first-reached representative (kept for viewer readability —
    /// matches are found on this).
    pub state: State,
    /// Canonical form + witness (vertex_map, edge_slots). The form is the
    /// dedup key; the witness gives every edge instance a stable slot
    /// identity for the token-event graph.
    pub canon: Canon,
}

pub struct Event {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub step: usize,
}

/// Per-event token flow, in canonical-slot coordinates. A token is
/// `(state id, slot)` where `slot` indexes the state's canonical sorted
/// edge list — the engine's fixed token-identity convention (see `teg`).
pub struct EventTokens {
    /// Parent canonical slots consumed (sorted).
    pub consumed: Vec<usize>,
    /// Child canonical slots produced (sorted).
    pub produced: Vec<usize>,
    /// `(parent_slot, child_slot)` for every edge that rode through
    /// untouched, in parent-edge order.
    pub passthrough: Vec<(usize, usize)>,
}

pub struct MultiwaySystem {
    pub states: Vec<StateRec>,
    pub events: Vec<Event>,
    /// Token flow per event (parallel to `events`).
    pub event_tokens: Vec<EventTokens>,
    /// States first reached at each step (layers[0] = [initial]).
    pub layers: Vec<Vec<usize>>,
    /// Branchial pairs: same-step sibling states produced from a common parent.
    pub branchial: Vec<(usize, usize)>,
    /// Events that merged into a state first reached at an *earlier* step.
    /// If nonzero, `path_counts` no longer aligns with the naive evolution
    /// tree: it counts walks in the merged DAG, which can over- OR
    /// under-state per-layer naive counts (see `path_counts`).
    pub back_merges: usize,
    /// Match-maintenance telemetry.
    pub stats: EvolveStats,
}

/// Telemetry counters for match maintenance (pinned by tests: incremental
/// evolve performs exactly ONE full search — the initial state — and one
/// delta per new canonical state; merged children never get match sets).
#[derive(Default)]
pub struct EvolveStats {
    /// Full `find_matches` invocations.
    pub full_match_calls: usize,
    /// `delta_matches` invocations.
    pub delta_match_calls: usize,
}

/// Evolution options. `threads > 1` panics until the parallel milestone
/// lands. `incremental: false` selects the reference full-search path
/// (kept for the bit-identical differential test).
pub struct EvolveOpts {
    /// Multiway BFS depth.
    pub steps: usize,
    /// Worker threads (currently must be 1).
    pub threads: usize,
    /// Delta-maintain match sets instead of re-searching every state.
    pub incremental: bool,
}

impl Default for EvolveOpts {
    fn default() -> Self {
        EvolveOpts {
            steps: 0,
            threads: 1,
            incremental: true,
        }
    }
}

pub fn evolve(rule: &Rule, init: State, steps: usize) -> MultiwaySystem {
    evolve_opts(
        rule,
        init,
        &EvolveOpts {
            steps,
            ..EvolveOpts::default()
        },
    )
}

pub fn evolve_opts(rule: &Rule, init: State, opts: &EvolveOpts) -> MultiwaySystem {
    assert!(
        opts.threads == 1,
        "parallel evolve lands in M6 (--threads > 1 not yet supported)"
    );
    let steps = opts.steps;
    let mut mw = MultiwaySystem {
        states: Vec::new(),
        events: Vec::new(),
        event_tokens: Vec::new(),
        layers: Vec::new(),
        branchial: Vec::new(),
        back_merges: 0,
        stats: EvolveStats::default(),
    };
    // Dedup key: the canonical form's edge list. No bucket scans, no
    // in-loop isomorphism checks — form equality IS isomorphism.
    let mut canon_map: DetMap<Vec<Edge>, usize> = DetMap::default();

    let c0 = canonicalize(&init);
    canon_map.insert(c0.form.edges.clone(), 0);
    mw.states.push(StateRec {
        id: 0,
        step: 0,
        state: init,
        canon: c0,
    });
    mw.layers.push(vec![0]);

    mw.stats.full_match_calls += 1;
    let init_matches = find_matches(&mw.states[0].state, rule);
    let mut frontier: Vec<(usize, Vec<Match>)> = vec![(0, init_matches)];
    for step in 1..=steps {
        let mut new_layer: Vec<usize> = Vec::new();
        let mut next_frontier: Vec<(usize, Vec<Match>)> = Vec::new();
        let mut branch_pairs: Vec<(usize, usize)> = Vec::new();

        for (sid, matches) in &frontier {
            let sid = *sid;
            let mut children: Vec<usize> = Vec::new();

            for m in matches {
                let app = apply_full(&mw.states[sid].state, rule, m);
                let c = canonicalize(&app.child);

                // Token flow in canonical-slot coordinates. Matches are
                // found on the parent's raw representative, so the
                // parent's own edge_slots apply; the child's slots index
                // the SHARED canonical edge list even when the child
                // merges (byte-equal forms).
                let mut consumed: Vec<usize> = m
                    .edge_idx
                    .iter()
                    .map(|&i| mw.states[sid].canon.edge_slots[i])
                    .collect();
                consumed.sort_unstable();
                let mut produced: Vec<usize> =
                    app.produced.clone().map(|i| c.edge_slots[i]).collect();
                produced.sort_unstable();
                let passthrough: Vec<(usize, usize)> = app
                    .kept
                    .iter()
                    .map(|&(pi, ci)| (mw.states[sid].canon.edge_slots[pi], c.edge_slots[ci]))
                    .collect();

                let cid = match canon_map.get(&c.form.edges) {
                    Some(&cid) => {
                        if mw.states[cid].step < step {
                            mw.back_merges += 1;
                        }
                        cid
                    }
                    None => {
                        let cid = mw.states.len();
                        canon_map.insert(c.form.edges.clone(), cid);
                        // Lazy match maintenance: only NEW canonical
                        // states get a match set (merged children are
                        // never expanded).
                        let child_matches = if opts.incremental {
                            mw.stats.delta_match_calls += 1;
                            delta_matches(rule, matches, m, &app)
                        } else {
                            mw.stats.full_match_calls += 1;
                            find_matches(&app.child, rule)
                        };
                        next_frontier.push((cid, child_matches));
                        mw.states.push(StateRec {
                            id: cid,
                            step,
                            state: app.child,
                            canon: c,
                        });
                        new_layer.push(cid);
                        cid
                    }
                };

                let eid = mw.events.len();
                mw.events.push(Event {
                    id: eid,
                    from: sid,
                    to: cid,
                    step,
                });
                mw.event_tokens.push(EventTokens {
                    consumed,
                    produced,
                    passthrough,
                });
                if !children.contains(&cid) {
                    children.push(cid);
                }
            }

            for i in 0..children.len() {
                for j in (i + 1)..children.len() {
                    let a = children[i].min(children[j]);
                    let b = children[i].max(children[j]);
                    branch_pairs.push((a, b));
                }
            }
        }

        branch_pairs.sort_unstable();
        branch_pairs.dedup();
        mw.branchial.extend(branch_pairs);
        mw.layers.push(new_layer);
        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }
    mw
}

impl MultiwaySystem {
    /// For each canonical state, the number of distinct paths from the
    /// initial state — i.e. how many nodes of the naive (unshared)
    /// evolution tree this single node represents. Computed by DP in
    /// event-creation order, which is topological as long as no event
    /// merges into an earlier layer.
    ///
    /// With back-merges (`back_merges > 0`) the DP is NOT a lower bound —
    /// it counts walks in the merged DAG, which can over- or under-state
    /// per-layer naive counts (a cyclic merge can even report the initial
    /// state as having multiple "tree nodes"). Gate any per-layer display
    /// on `back_merges == 0`.
    pub fn path_counts(&self) -> Vec<u128> {
        let mut p = vec![0u128; self.states.len()];
        if !p.is_empty() {
            p[0] = 1;
        }
        for e in &self.events {
            p[e.to] = p[e.to].saturating_add(p[e.from]);
        }
        p
    }

    /// Naive-tree node count per layer vs canonical count per layer.
    pub fn sharing_per_layer(&self) -> Vec<(usize, u128, usize)> {
        let pc = self.path_counts();
        self.layers
            .iter()
            .enumerate()
            .map(|(step, ids)| {
                let paths: u128 = ids.iter().map(|&i| pc[i]).sum();
                (step, paths, ids.len())
            })
            .collect()
    }
}
