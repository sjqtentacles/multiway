//! Multiway evolution with canonical-state sharing.
//!
//! The naive multiway object is a tree: every state branches into one
//! child per match. We instead merge isomorphic states globally, so the
//! result is a DAG over canonical states — the e-graph move applied at
//! state granularity. `path_counts` reports how many naive tree paths
//! each canonical node absorbs, i.e. how much work the sharing saved.

use crate::canon::{isomorphic, wl_hash};
use crate::hypergraph::State;
use crate::matcher::{apply, find_matches};
use crate::rule::Rule;
use std::collections::HashMap;

pub struct StateRec {
    pub id: usize,
    /// Step at which this canonical state was first reached.
    pub step: usize,
    pub state: State,
    pub hash: u64,
}

pub struct Event {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub step: usize,
}

pub struct MultiwaySystem {
    pub states: Vec<StateRec>,
    pub events: Vec<Event>,
    /// States first reached at each step (layers[0] = [initial]).
    pub layers: Vec<Vec<usize>>,
    /// Branchial pairs: same-step sibling states produced from a common parent.
    pub branchial: Vec<(usize, usize)>,
    /// Events that merged into a state first reached at an *earlier* step.
    /// If nonzero, `path_counts` is a lower bound (see comment there).
    pub back_merges: usize,
}

pub fn evolve(rule: &Rule, init: State, steps: usize) -> MultiwaySystem {
    let mut mw = MultiwaySystem {
        states: Vec::new(),
        events: Vec::new(),
        layers: Vec::new(),
        branchial: Vec::new(),
        back_merges: 0,
    };
    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();

    let h0 = wl_hash(&init);
    mw.states.push(StateRec {
        id: 0,
        step: 0,
        state: init,
        hash: h0,
    });
    buckets.insert(h0, vec![0]);
    mw.layers.push(vec![0]);

    let mut frontier: Vec<usize> = vec![0];
    for step in 1..=steps {
        let mut new_layer: Vec<usize> = Vec::new();
        let mut branch_pairs: Vec<(usize, usize)> = Vec::new();

        for &sid in &frontier {
            let matches = find_matches(&mw.states[sid].state, rule);
            let mut children: Vec<usize> = Vec::new();

            for m in &matches {
                let child = apply(&mw.states[sid].state, rule, m);
                let h = wl_hash(&child);

                let mut found: Option<usize> = None;
                if let Some(bucket) = buckets.get(&h) {
                    for &cid in bucket {
                        if isomorphic(&mw.states[cid].state, &child) {
                            found = Some(cid);
                            break;
                        }
                    }
                }

                let cid = match found {
                    Some(cid) => {
                        if mw.states[cid].step < step {
                            mw.back_merges += 1;
                        }
                        cid
                    }
                    None => {
                        let cid = mw.states.len();
                        mw.states.push(StateRec {
                            id: cid,
                            step,
                            state: child,
                            hash: h,
                        });
                        buckets.entry(h).or_default().push(cid);
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
        mw.layers.push(new_layer.clone());
        frontier = new_layer;
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
    /// merges into an earlier layer; with back-merges (`back_merges > 0`)
    /// treat the counts as lower bounds.
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
