# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-07-09

### Added

- **True canonization**: nauty-style individualization–refinement
  canonical forms for ordered multiset hypergraphs (`canon::canonicalize`)
  — equal forms ⟺ isomorphic; multiway dedup becomes a single map
  lookup. Exact rank-normalized refinement (identity never touches a
  hash), component decomposition, deterministic witness.
- **Token-event graph** (`teg`): causal structure across all updating
  orders on the merged DAG; tokens are `(state, canonical slot)` through
  the canonization witness; multivalued creators via worklist fixed
  point; event-level branchial pairs; exported in the bundle.
- **Confluence checker** (`confluence`): critical-pair enumeration with
  strong joinability via colored canonization; honest verdicts
  (`AllCriticalPairsStronglyJoinable` / `NotConfluent` only on double
  saturation / `Inconclusive`); Newman upgrade gated on the termination
  lint. CLI `--check-confluence` with repeatable `--rule`.
- **Rule lint** (`lint`): edge/vertex-slot/arity deltas, fresh-vertex
  counts, orphan detection, termination-by-edge-count. CLI `--lint`.
- **Incremental matching** (`matcher::delta_matches`): match sets
  delta-maintained across events, byte-identical to full search; one
  full search per run.
- **Parallel evolve** (`system::evolve_opts`, CLI `--threads`):
  deterministic-by-construction 3-phase threading (~2.4× at depth 5 with
  4 threads); `--order standard` causal mode (maximal disjoint
  generations).
- **Fuel-capped isomorphism** (`canon::isomorphic_with_budget`) with a
  pinned Θ(m!) pathological witness.
- Notation printers (`State::to_notation`, `Rule::to_notation`),
  script-safe JSON escaping, `det` module (deterministic maps, pinned
  mixing lineage, seeded PRNG).
- Terminal polish: box-drawing tables, digit grouping, log sparklines —
  and honest back-merge output (naive-tree columns suppressed with a
  caveat when they would be wrong).
- Viewer: touch + pointer input, responsive layout, rAF rendering,
  evolution scrubber, token-event tab, theme toggle, keyboard
  navigation, PNG export, permalinks, accessibility roles; the
  ~10^5-node layout crash fixed.
- Zero-dep test infrastructure: property harness with seeded repro
  lines, brute-force oracles (isomorphism, naive tree, matcher), JSON
  well-formedness checker, golden files with bless-then-fail, gallery
  locks, CLI integration tests, bench harness.
- Docs: THEORY.md, CONTRIBUTING.md, CITATION.cff, examples/, full
  rustdoc with `deny(missing_docs)`.

### Changed

- `StateRec.hash` replaced by `StateRec.canon` (form + witness).
- `path_counts` docs corrected: with back-merges the DP counts walks in
  the merged DAG (not a "lower bound") and per-layer display is gated.
- CLI stats table format (box drawing, grouped digits, sparkline).
- JSON bundle gains the `teg` section.

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
