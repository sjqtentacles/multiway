//! Hash-consed edge store: every distinct canonical edge stored once,
//! shared across all states — the v0.5 of the egglog-style relational
//! substrate (issue #1).
//!
//! Canonical forms are label-normalized, so their edges repeat massively
//! across states: at depth k of the classic rule every state's vertices
//! are exactly `0..=k`, giving at most `(k+1)^2` distinct binary edges
//! for the *entire* run. Interning collapses each per-state
//! `Vec<Vec<u32>>` form into a `Vec<EdgeId>` — one contiguous u32 slice
//! per state — and turns the dedup key's hashing/equality into a u32
//! slice compare.
//!
//! Determinism: ids are assigned in first-intern order, and interning
//! happens ONLY in evolve's serial Phase B, in `(step, frontier, match)`
//! order — so ids are a pure function of the evolution, identical for
//! every thread count. (Id *values* are never load-bearing: all
//! consumers rely only on `id equality ⟺ edge byte-equality` and on
//! [`EdgeStore::resolve`]. The load-bearing order is the FORM's own
//! `(len, seq)` edge order, which `form_ids` preserves — see the module
//! docs' slot-identity note.)
//!
//! Slot identity: a state's `form_ids[slot]` lists interned ids **in the
//! form's existing (len, seq) edge order** — NOT numerically sorted
//! (ids are first-encounter-ordered, so numeric order would scramble
//! `edge_slots`/token identity; pinned by the classic-rule slot test).
//!
//! Relational read-off (v-next, not implemented): `form_ids` is already
//! the edge column of the egglog-style relation — the full relation is
//! the tuple set `(edge_id, position, vertex)` obtained by exploding
//! each stored edge, at which point sub-state structure becomes joinable
//! across states.

use crate::det::DetMap;
use crate::hypergraph::Edge;

/// Interned canonical-edge id. Dense; assigned in first-intern order.
pub type EdgeId = u32;

/// The store: each distinct edge kept once, id ↔ content both ways.
#[derive(Default)]
pub struct EdgeStore {
    edges: Vec<Edge>,
    ids: DetMap<Edge, EdgeId>,
}

impl EdgeStore {
    /// Get-or-insert the id for an edge.
    pub fn intern(&mut self, e: &Edge) -> EdgeId {
        if let Some(&id) = self.ids.get(e) {
            return id;
        }
        let id = self.edges.len() as EdgeId;
        self.edges.push(e.clone());
        self.ids.insert(e.clone(), id);
        id
    }

    /// The edge content for an id (panics on a foreign id — ids are only
    /// minted by this store's `intern`).
    pub fn resolve(&self, id: EdgeId) -> &Edge {
        &self.edges[id as usize]
    }

    /// Number of distinct edges interned.
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// True when nothing has been interned.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }
}
