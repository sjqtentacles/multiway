//! Token-event graph: causal structure across ALL updating orders,
//! computed on the merged canonical multiway DAG.
//!
//! A **token** is an edge instance with identity `(state_id, slot)`, where
//! `slot` indexes the state's canonical sorted edge list. This is one
//! deterministic *section* of the automorphism-quotiented token-event
//! graph: when two histories reach the same canonical form, their edge
//! instances are identified through each raw state's own canonical
//! witness (`Canon::edge_slots`), under the engine's fixed labeling
//! convention (ascending-vertex-id IR branching, first-minimal-leaf,
//! duplicate edges in raw-index slot order). Slots holding byte-identical
//! edges are genuinely interchangeable, so their creator sets are unioned.
//!
//! Consequence embraced, not hidden: a token's creator is path-dependent
//! on a merged DAG, so `creators[state][slot]` is a *set* of event ids.
//! On cyclic merged DAGs (e.g. `{{x,y}}->{{y,x}}`) an event class can
//! even acquire a causal edge to itself — "some instance of this event
//! class consumes a token produced by another instance of the same class
//! across histories." That semantics is pinned by tests.
//!
//! Event id convention matches [`crate::causal`]: id 0 is the initial
//! condition; event `i + 1` is `mw.events[i]`.

use crate::system::MultiwaySystem;

/// The token-event graph derived from a multiway evolution.
pub struct TokenEventGraph {
    /// `creators[state][slot]` = sorted event ids that create this token
    /// along some history (unioned across byte-identical-edge slot runs).
    pub creators: Vec<Vec<Vec<usize>>>,
    /// Causal edges `(creator_event, consumer_event)`, sorted + deduped.
    /// Unlike `causal::run` (one updating order), this covers every order
    /// the multiway evolution explores.
    pub causal: Vec<(usize, usize)>,
    /// Same-step event pairs consuming overlapping token sets — the
    /// event-level branchial structure (finer than the state-level
    /// `MultiwaySystem::branchial`, which is kept for compatibility).
    pub branchial_events: Vec<(usize, usize)>,
}

fn insert_sorted(v: &mut Vec<usize>, x: usize) -> bool {
    match v.binary_search(&x) {
        Ok(_) => false,
        Err(pos) => {
            v.insert(pos, x);
            true
        }
    }
}

/// Build the token-event graph. Worklist fixed point over events in id
/// order — handles back-merges and cyclic merged DAGs; for back-merge-free
/// runs (event ids into a state strictly precede events out of it) one
/// pass suffices and the loop exits after a clean verification sweep.
pub fn build(mw: &MultiwaySystem) -> TokenEventGraph {
    let mut creators: Vec<Vec<Vec<usize>>> = mw
        .states
        .iter()
        .map(|s| vec![Vec::new(); s.state.edges.len()])
        .collect();
    for slot_creators in creators[0].iter_mut() {
        slot_creators.push(0); // event 0: the initial condition
    }

    loop {
        let mut changed = false;
        for (idx, e) in mw.events.iter().enumerate() {
            let eid = idx + 1;
            let et = &mw.event_tokens[idx];
            for &s in &et.produced {
                changed |= insert_sorted(&mut creators[e.to][s], eid);
            }
            for &(ps, cs) in &et.passthrough {
                let src = creators[e.from][ps].clone();
                for c in src {
                    changed |= insert_sorted(&mut creators[e.to][cs], c);
                }
            }
        }
        // Byte-identical duplicate edges are interchangeable tokens: union
        // creator sets across each identical-edge slot run (inside the
        // fixed point, so unions propagate through passthroughs).
        for (sid, s) in mw.states.iter().enumerate() {
            // byte-identical edges ⟺ identical interned ids
            let form = &s.form_ids;
            let mut run_start = 0;
            for slot in 1..=form.len() {
                if slot == form.len() || form[slot] != form[run_start] {
                    if slot - run_start > 1 {
                        let mut union: Vec<usize> = Vec::new();
                        for run_slot in creators[sid][run_start..slot].iter() {
                            for &c in run_slot {
                                insert_sorted(&mut union, c);
                            }
                        }
                        for run_slot in creators[sid][run_start..slot].iter_mut() {
                            if *run_slot != union {
                                *run_slot = union.clone();
                                changed = true;
                            }
                        }
                    }
                    run_start = slot;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Causal edges: every creator of every consumed token.
    let mut causal: Vec<(usize, usize)> = Vec::new();
    for (idx, e) in mw.events.iter().enumerate() {
        let eid = idx + 1;
        for &ps in &mw.event_tokens[idx].consumed {
            for &c in &creators[e.from][ps] {
                causal.push((c, eid));
            }
        }
    }
    causal.sort_unstable();
    causal.dedup();

    // Branchial events: same parent state (hence same step), overlapping
    // consumed slot sets. Consumed vectors are sorted — merge-walk overlap.
    let mut branchial_events: Vec<(usize, usize)> = Vec::new();
    for sid in 0..mw.states.len() {
        let evs: Vec<usize> = (0..mw.events.len())
            .filter(|&i| mw.events[i].from == sid)
            .collect();
        for i in 0..evs.len() {
            for j in (i + 1)..evs.len() {
                if sorted_overlap(
                    &mw.event_tokens[evs[i]].consumed,
                    &mw.event_tokens[evs[j]].consumed,
                ) {
                    branchial_events.push((evs[i] + 1, evs[j] + 1));
                }
            }
        }
    }
    branchial_events.sort_unstable();
    branchial_events.dedup();

    TokenEventGraph {
        creators,
        causal,
        branchial_events,
    }
}

fn sorted_overlap(a: &[usize], b: &[usize]) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    false
}
