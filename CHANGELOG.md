# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-07-09

### Added

- Multiway hypergraph rewriting engine: states are multisets of ordered
  hyperedges over integer vertices, defined up to isomorphism.
- Wolfram-model-style rule parser (`{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}`
  notation) with non-injective variable binding and fresh-vertex minting.
- Backtracking sub-hypergraph matcher consuming distinct edge instances.
- Multiway BFS evolution with global canonical dedup: Weisfeiler–Leman
  invariant hashing plus exact backtracking isomorphism confirmation.
- Branchial pairs, per-state naive-tree path counting, back-merge tracking.
- Single-path causal evolution with edge provenance producing a causal DAG.
- Zero-dependency handwritten JSON export.
- Self-contained interactive HTML viewer (multiway graph, causal graph,
  per-state hypergraph inspector; light/dark; deterministic layouts).
- CLI: `--rule`, `--init`, `--steps`, `--causal`, `--json`, `--html`, `--quiet`.
- Hand-verified multiway layer counts as integration tests.
