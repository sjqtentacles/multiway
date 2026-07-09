//! Hypergraph states: multisets of ordered hyperedges over integer vertices.

/// A vertex: a plain integer label with no intrinsic meaning.
pub type Vertex = u32;

/// An ordered hyperedge. `vec![a, b]` is a directed binary edge a -> b;
/// arity 1 and arity >= 3 edges are equally valid.
pub type Edge = Vec<Vertex>;

/// A hypergraph state. Semantically a *multiset* of hyperedges —
/// duplicate edges are distinct instances. `next_vertex` is the counter
/// used to mint fresh vertices when a rule's right-hand side introduces
/// new ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// The edge instances (a multiset — duplicates are distinct).
    pub edges: Vec<Edge>,
    /// Counter for minting fresh vertices on RHS-only rule variables.
    pub next_vertex: Vertex,
}

impl State {
    /// Build a state from edges, deriving `next_vertex` = max label + 1.
    pub fn new(edges: Vec<Edge>) -> Self {
        let next_vertex = edges
            .iter()
            .flatten()
            .copied()
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        State { edges, next_vertex }
    }

    /// Sorted, deduplicated vertex set (vertices exist only via edges).
    pub fn vertices(&self) -> Vec<Vertex> {
        let mut vs: Vec<Vertex> = self.edges.iter().flatten().copied().collect();
        vs.sort_unstable();
        vs.dedup();
        vs
    }

    /// Number of edge instances.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Brace notation: `{{0,1},{2}}`; the empty state prints as `{}`.
    /// `parse_state ∘ to_notation` is the identity on edge lists.
    pub fn to_notation(&self) -> String {
        let inner: Vec<String> = self
            .edges
            .iter()
            .map(|e| {
                let vs: Vec<String> = e.iter().map(|v| v.to_string()).collect();
                format!("{{{}}}", vs.join(","))
            })
            .collect();
        format!("{{{}}}", inner.join(","))
    }
}
