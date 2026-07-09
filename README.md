# multiway

A multiway hypergraph rewriting engine with e-graph-style state sharing.
Zero dependencies, pure Rust, ships its own interactive viewer.

States are hypergraphs — multisets of ordered hyperedges over integer
vertices — defined *up to isomorphism*. Rules rewrite sub-hypergraphs,
Wolfram-model style. The engine explores **every** possible rewrite, but
instead of building the naive evolution tree it merges isomorphic states
globally, producing a compressed DAG of canonical states. That is the
e-graph move (equality saturation's sharing) applied at state granularity.

The compression is not cosmetic. The classic Wolfram-model rule
`{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}` from `{{0,0},{0,0}}`:

| depth | naive tree nodes | canonical states | sharing |
|------:|-----------------:|-----------------:|--------:|
| 2     | 24               | 3                | 8.0×    |
| 3     | 408              | 18               | 22.7×   |
| 4     | 9,504            | 156              | 60.9×   |
| 5     | 280,080          | 1,776            | 157.7×  |

Depth 5 — over a quarter million tree nodes collapsed to 1,776 states —
runs in ~0.1 s. The sharing factor *grows* with depth, which is the whole
argument for building on this representation.

## Quick start

```sh
cargo test                    # includes hand-verified multiway layer counts
cargo run --release -- \
  --rule "{{x,y},{x,z}}->{{x,z},{x,w},{y,w},{z,w}}" \
  --init "{{0,0},{0,0}}" \
  --steps 4 --causal 40 \
  --html demo.html
open demo.html                # interactive multiway + causal explorer
```

The viewer is a single self-contained HTML file (data baked in, no CDN, no
network): layered multiway graph with branchial pairs, causal DAG of one
evolution, and a per-state hypergraph inspector. Light and dark mode.

## How it works

**Canonicalization is two-tier.** `canon::wl_hash` is a Weisfeiler–Leman
color-refinement hash adapted to ordered hyperedges — isomorphism
*invariant* (isomorphic states always collide, so a merge is never
missed), but like WL not isomorphism *complete*. Every hash-bucket hit is
therefore confirmed by `canon::isomorphic`, an exact backtracking check.
WL collisions cost time, never correctness.

**Matching** (`matcher`) is backtracking sub-hypergraph matching with
Wolfram-model semantics: distinct pattern variables may bind the same
vertex; each pattern edge consumes a distinct edge instance; RHS-only
variables mint fresh vertices.

**Multiway evolution** (`system`) is BFS with global canonical dedup.
Each layer records branchial pairs (same-step siblings of a common
parent). `path_counts` is the DP that answers "how many naive tree nodes
does this canonical state absorb?" — the sharing numbers above.

**Causal structure** (`causal`) runs a single deterministic evolution
with edge provenance: every hyperedge remembers its creator event, and
consuming an edge created by event A makes the consumer causally depend
on A.

**No global RNG, no wall clock.** Hashes are deterministic (no seeded
`std` hasher), fresh vertices are counter-minted, and even the viewer's
force layout is seeded arithmetically — identical inputs give identical
outputs everywhere.

## Layout

```
src/hypergraph.rs   states and ordered hyperedges
src/canon.rs        WL-style invariant hash + exact isomorphism check
src/rule.rs         parser for {{x,y},{x,z}} -> {...} notation
src/matcher.rs      backtracking matcher + rule application
src/system.rs       multiway BFS, canonical dedup, branchial, path counts
src/causal.rs       single-path evolution with provenance -> causal DAG
src/export.rs       JSON bundling (handwritten, zero deps)
src/main.rs         CLI; bakes data into viewer/template.html
viewer/template.html  self-contained interactive explorer
tests/engine.rs     including hand-computed layer counts [1,1,2,4]
```

## Roadmap

- **True canonization** — replace hash+verify with nauty-style refinement
  and tie-breaking so states get a canonical *form*. Prerequisite for
  memoizing local evolution (the "HashLife for hypergraphs" problem).
- **Token-event graph** — causal structure across *all* updating orders,
  not one path; branchial space then falls out per-layer.
- **Causal-invariance checker** — critical-pair (Knuth–Bendix-style)
  analysis over rule sets: prove all divergences reconverge or exhibit a
  counterexample. Confluence for graph rewriting is undecidable in
  general, so this is a prover + counterexample hunter, not an oracle.
- **Sub-state sharing** — the actual e-graph: share common sub-hypergraphs
  *across* states (egglog-style relational representation is the candidate
  substrate). State-level dedup is the v0 of this idea.
- **Incremental matching** — don't re-search the whole state per step;
  RETE/differential-dataflow-style maintenance of match sets.
- **Parallel rewriting** — non-overlapping events commute; the scheduler's
  dependency graph *is* the causal graph.
- **WebGPU front-end** — compile the engine to WASM, render in the browser,
  shareable universes.
- **A typed rule layer** — conservation laws as compile-time guarantees
  (linear types: no duplication, no deletion).
