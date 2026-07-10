# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-07-10

Engine round + pattern discovery + the playground. All exports are
byte-identical to 0.2.0 except the documented `creators` addition to
the TEG section.

### Added

- **Pattern discovery — the headline.** The scanner finds interesting
  rules instead of waiting for you to guess them:
  - `rulespace`: rule-space enumeration modulo behavioral equivalence
    (variable renaming + per-side edge permutations); EXACT class
    counting via Burnside's lemma (pinned: 6,477 binary / 18,143
    default / 16,184,498 arity≤3 — cross-checked against explicit
    enumeration in-tree); deterministic sampling for huge spaces.
  - `probe`: bounded deterministic behavior probe — every budget is a
    count, no wall clock anywhere; three arity-derived seeds per rule;
    growth classification (dies/static/periodic/linear/poly/exp),
    period detection, final-shape profiles, sharing, evolution
    fingerprints; differentially tested against the real engine
    (`prop_probe_matches_evolve`).
  - `atlas`: all-integer interestingness scoring (milli-units — no
    floats near the order) with pinned reference scores; fingerprint
    dedup (representative = least rule, aliases counted, never
    dropped); tier-2 bounded confluence classes on finalists.
  - `scan`: thread-invariant parallel driver (byte-identical output for
    any `--threads`, pinned 1-vs-4); terminal atlas table; scan JSON;
    `--atlas DIR` bakes a self-contained HTML index + one full viewer
    per rule.
  - CLI: `--scan`, `--count` (instant exact Burnside size), `--sample
    N --seed 0xHEX`, `--top`, space/budget flags, exhaustive-cap
    refusal at 200k classes with the exact size (exit 2), `--scan-json`.
- **WASM playground** (`wasm`): the engine compiled to
  wasm32-unknown-unknown (131 KB, `[profile.wasm]`, hand-rolled C ABI —
  `mw_alloc`/`mw_dealloc`/`mw_run`, length-prefixed JSON, zero
  dependencies, no wasm-bindgen). `run_json` mirrors the CLI evolution
  path verbatim, so native↔wasm byte-identity is structural — and CI
  verifies it mechanically with a node script on every push. Dual-mode
  viewer template (`--html` bakes data, `--playground OUT --wasm PATH`
  bakes the engine); playground panel with copy-bundle-JSON;
  auto-deployed to GitHub Pages: https://sjqtentacles.github.io/multiway/
- **`--ascii`**: golden-locked terminal DAG renderer (columns = steps,
  `─ ╲ ╱` connectors, honest `… +N more` overflow, back-merge footer).
- **Creators export** (closes #6): `teg` JSON gains `"creators"` — the
  compact multivalued set `[[stateId, slot, [eventIds...]], ...]`
  sorted by (state, slot); viewer TEG tooltips annotate shared tokens.
- **Automorphism pruning** (canonization V2, #3): equal-key IR leaves
  yield generators; orbit pruning of target cells; forms, witnesses,
  and every golden byte-identical (first-found-wins tie rule), pinned
  by `prop_aut_pruning_form_identical` incl. colored mode.
- **Hash-consed edge store** (`store`, #1): canonical forms intern as
  `EdgeId` vectors in the form's (len, seq) slot order; dedup keys are
  id vectors. The full relational sub-state substrate is documented
  v-next in the module docs.
- **docs/LEMMA.md** (#4, still open): the critical-pair lemma for this
  exact formalism — disjoint-diamond and quotient steps proved (the
  quotient lemma's mechanical half fuzz-pinned by the new
  `prop_quotient_preserves_matches`), factorization gap listed
  explicitly and honestly.
- **Profiling** (`MULTIWAY_PROFILE`, stderr-only) + always-on phase
  timers in `EvolveStats` (never serialized; zero on wasm).
- Scan-safety primitives: `det::log2_milli` (pure-integer fixed-point),
  `canon::canonicalize_budgeted` (IR leaf cap — symmetric states cost
  k! leaves; exhaustion aborts, never risks a duplicate).

### Changed

- **Performance/memory** (classic rule, depth 6): wall 3.0 s → ~2.9 s
  serial / ~2.1 s at 4 threads; RSS 344 MB → ~300 MB; allocations per
  event 180 → 126. Final-step Phase C skipped (was 93% wasted work);
  EventTokens built in parallel Phase A; the serial child clone
  removed; branchial pairs derived lazily from events (453k stored
  pairs gone at depth 6); refine_exact reference-sorts. **Depth 7 now
  practical**: 422.5M naive nodes → 423,975 canonical states, ~5 min /
  2.5 GB at 4 threads.
- Pub API (0.x): `StateRec` gains `form_ids`, drops the dead
  `vertex_map` copy; `EventTokens` slots are `u32`; `EvolveStats` gains
  phase timers; `MultiwaySystem` gains `store` and the `branchial()`
  method (stored field removed); `matcher::delta_matches` takes an
  explicit `child: &State`.
- `teg_json` schema: adds `"creators"` (see Added); everything else
  byte-identical.
- ci.yml: new `wasm` job (wasm clippy, <300 KB size budget, node
  byte-identity, playground artifact); pages.yml deploys the playground
  + a demo atlas via plain `git push` to gh-pages. CONTRIBUTING records
  the zero-dependency policy interpretation (runner-preinstalled tools
  and SHA-pinned GitHub-owned actions are in-bounds).


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
