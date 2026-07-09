//! # multiway
//!
//! A multiway hypergraph rewriting engine with e-graph-style state sharing.
//!
//! States are hypergraphs (multisets of ordered hyperedges over integer
//! vertices), defined *up to isomorphism*. Rules rewrite sub-hypergraphs,
//! Wolfram-model style. The multiway evolution explores every possible
//! rewrite, but canonically equivalent states are merged into a single
//! node — so the object built is not the naive evolution *tree* but a
//! compressed DAG of equivalence classes of states. That merging is the
//! e-graph move at state granularity; sub-state sharing is the roadmap.
//!
//! ```
//! use multiway::rule::{parse_rule, parse_state};
//! use multiway::system::evolve;
//!
//! let rule = parse_rule("{{x,y}}->{{x,y},{y,z}}").unwrap();
//! let init = parse_state("{{0,0}}").unwrap();
//! let mw = evolve(&rule, init, 3);
//! let layers: Vec<usize> = mw.layers.iter().map(|l| l.len()).collect();
//! assert_eq!(layers, vec![1, 1, 2, 4]); // hand-verified canonical layer sizes
//! ```
//!
//! Modules:
//! - [`hypergraph`]: states and hyperedges
//! - [`det`]: determinism primitives — mixing function, deterministic maps, seeded PRNG
//! - [`canon`]: true canonization (individualization–refinement canonical forms) + WL hash and exact isomorphism as oracles
//! - [`rule`]: parsing `{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}` notation
//! - [`matcher`]: backtracking + incremental sub-hypergraph matching and rule application
//! - [`system`]: multiway evolution (optionally parallel) with canonical dedup, branchial pairs, path counting
//! - [`teg`]: token-event graph — causal structure across all updating orders
//! - [`causal`]: single-path evolution with edge provenance -> causal graph
//! - [`confluence`]: critical-pair analysis with strong joinability
//! - [`lint`]: static rule analysis (conservation, termination)
//! - [`export`]: JSON bundling for the HTML viewer
//! - [`report`] / [`stats`]: deterministic terminal rendering

#![deny(missing_docs)]

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
