//! # multiway
//!
//! A multiway hypergraph rewriting engine with e-graph-style state sharing.
//!
//! States are hypergraphs (multisets of ordered hyperedges over integer
//! vertices), defined *up to isomorphism*. Rules rewrite sub-hypergraphs,
//! Wolfram-model style. The multiway evolution explores every possible
//! rewrite, but canonically equivalent states are merged into a single
//! node — so the object we build is not the naive evolution *tree* but a
//! compressed DAG of equivalence classes of states. That merging is the
//! e-graph move at state granularity; sub-state sharing is the roadmap.
//!
//! Modules:
//! - [`hypergraph`]: states and hyperedges
//! - [`det`]: determinism primitives — mixing function, deterministic maps, seeded PRNG
//! - [`canon`]: true canonization (individualization–refinement canonical forms) + WL hash and exact isomorphism as oracles
//! - [`rule`]: parsing `{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}` notation
//! - [`matcher`]: backtracking sub-hypergraph matching and rule application
//! - [`system`]: multiway evolution with canonical dedup, branchial pairs, path counting
//! - [`causal`]: single-path evolution with edge provenance -> causal graph
//! - [`export`]: JSON bundling for the HTML viewer

pub mod canon;
pub mod causal;
pub mod confluence;
pub mod det;
pub mod export;
pub mod hypergraph;
pub mod lint;
pub mod matcher;
pub mod report;
pub mod rule;
pub mod stats;
pub mod system;
pub mod teg;
