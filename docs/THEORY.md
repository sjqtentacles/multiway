# Theory notes

What the engine computes, why the algorithms are shaped the way they
are, and — most importantly — exactly what each result does and does not
claim. Everything here is pinned by tests in the repository; where a
claim is weaker than you might hope, that weakness is deliberate.

## 1. The objects

A **state** is a finite multiset of ordered hyperedges over integer
vertices: `{{0,0},{0,1}}` has two edge *instances*; duplicates are
distinct instances, edge order within an instance matters (`{0,1}` ≠
`{1,0}` structurally), and vertex labels carry no meaning — states are
defined **up to isomorphism** (vertex relabeling).

A **rule** like `{{x,y},{x,z}} -> {{x,z},{x,w},{y,w},{z,w}}` rewrites a
sub-hypergraph. Wolfram-model matching semantics, both load-bearing:

- **non-injective variable binding**: distinct variables may bind the
  same vertex (so `{{x,y},{x,z}}` matches two self-loops on one vertex);
- **distinct edge instances**: each LHS pattern edge consumes a
  different instance (multiset semantics).

Variables appearing only on the RHS mint fresh vertices.

The **multiway evolution** applies *every* match of *every* state, layer
by layer. Naively this is a tree that grows like the number of updating
orders. The engine's central move: merge isomorphic states globally, so
the object built is a DAG of canonical states. `path_counts` is the DP
that reports how many naive-tree nodes each canonical state absorbs —
the classic rule's depth-5 layer collapses 280,080 naive nodes into
1,776 canonical states, and the ratio *grows* with depth. That is
equality saturation's sharing idea (the e-graph move) applied at state
granularity; sharing *sub-state* structure across states is the roadmap
continuation of the same idea.

## 2. True canonization

Dedup requires deciding isomorphism fast. v0.1 used a two-tier scheme —
a Weisfeiler–Leman-style invariant hash to bucket, plus an exact
backtracking check inside buckets. WL on ordered hyperedges works by
color refinement where an edge's signature is the *sequence* of its
vertices' colors (order matters) and a vertex folds in the sorted
multiset of `(edge signature, position)` incidences — invariant
(isomorphic states always collide) but famously not *complete*.

v0.2 replaces it with **true canonization**: a nauty-style
individualization–refinement (IR) search assigning every state a
canonical *form* — a relabeled representative with

```
canonical_form(a) == canonical_form(b)   ⟺   a ≅ b
```

so dedup is one hash-map lookup on the form. The normative search
discipline (`src/canon.rs`):

- **exact refinement**: per-vertex classes are dense rank-normalized ids
  ordered by `(old class, sorted multiset of (edge-signature id,
  position))` — *exact data, no hashes anywhere in identity*, so a
  collision can never corrupt a form;
- target cell = smallest non-singleton class (ties: smallest class id);
  branch over its members in ascending vertex id; individualize,
  re-refine, recurse;
- a leaf's candidate is its relabeled, `(arity, sequence)`-sorted edge
  list; the minimal leaf wins, first-found on ties (that tie-break is
  what makes the *witness* — `vertex_map`, `edge_slots` — deterministic).

The form is deliberately **not** defined as the global lexicographic
minimum over all n! labelings (computing that *is* the graph-
canonization problem). Completeness needs only two facts: the search
discipline is isomorphism-invariant (so isomorphic inputs explore
isomorphic trees and pick equal minima), and every leaf is an actual
relabeling of the input (so equal forms exhibit an isomorphism). Both
directions are fuzzed against a brute-force all-bijections oracle.

Two practical notes. Component decomposition handles the one realistic
pathology — k identical disjoint components cost k! leaves undivided —
by canonicalizing components independently and sorting their forms.
And graph isomorphism has no known polynomial algorithm; IR is
worst-case exponential. On the small, sparse, position-salted states
this engine produces, refinement is almost always discrete after at
most one individualization (leaf counts are pinned by tests).

## 3. Token identity through merging

The **token-event graph** wants edge-instance ("token") identity: which
event created each edge, which events consumed it. On the merged DAG
this is subtle — two histories reaching the same canonical form disagree
about which raw edge is "the same" edge.

The engine's answer: a token is `(state, slot)` where `slot` indexes the
canonical sorted edge list, and every raw child maps its instances
through *its own* canonical witness. Because merged children have
byte-equal forms, their slots index a shared space. This is one
deterministic **section** of the automorphism-quotiented token-event
graph — if the canonical form has nontrivial automorphisms, a different
(equally valid) convention could permute slots. Slots holding
byte-identical edges genuinely are interchangeable, so their creator
sets are unioned; the residual convention-dependence is documented
rather than hidden, and computing full automorphism orbits is the
V2 upgrade path.

Consequence embraced: a token's creator is path-dependent, so
`creators[state][slot]` is a *set*, computed as a worklist fixed point
(back-merges make the merged DAG cyclic — under `{{x,y}} -> {{y,x}}`
the lone state maps to itself and event 1 becomes its own causal
ancestor, meaning: some instance of this event class consumes a token
produced by another instance of the same class across histories).

## 4. Causal structure, three ways

- `causal::run` — one deterministic history (always the first match);
  edge provenance gives the standard causal DAG of one updating order.
- `causal::run_ordered(StandardGenerations)` — Wolfram's standard
  updating order: per generation, the greedy maximal set of pairwise
  edge-disjoint matches fires simultaneously. The generation layering is
  one antichain decomposition of the causal DAG — the "scheduler view":
  events that could run in parallel are exactly the causally
  independent ones.
- `teg::build` — causal edges across **all** updating orders at once,
  via token flow on the merged DAG, plus event-level branchial pairs
  (same-step events consuming overlapping tokens).

## 5. Confluence: what the checker may honestly claim

Two matches consuming **disjoint** edge instances commute — `apply`
never renames vertices, bindings derive only from a match's own edges,
so each survives the other and the divergence closes in one step (the
diamond; fuzz-pinned). All the danger is in **overlaps**, and the
checker enumerates them as critical pairs: for each rule pair, every
nonempty arity-matched partial injection between LHS edge lists,
variables unified positionwise, instantiated over a minimal host.

Joinability is checked **strongly**: the two branches are evolved
without relabeling, deduplicated by *colored* canonical forms in which
every host vertex carries a distinct pin (exact colors through exact
refinement — pin identity never touches a hash). Plain
joinable-up-to-isomorphism is provably not enough. The pinned
counterexample (after Plump's analysis of hypergraph rewriting):

```
{{x,y}} -> {{x}}     vs     {{x,y}} -> {{y}}
```

On host `{{a,b}}` the branches give `{{a}}` and `{{b}}` — isomorphic!
But embed the host in `{{a,b},{a,c}}` and they diverge to `{{a},{a,c}}`
vs `{{b},{a,c}}`. A join that doesn't respect which host vertices
survive does not survive contexts. The checker reports this
`Inconclusive { WeakOnly }`.

The verdict vocabulary is deliberately narrow:

- `AllCriticalPairsStronglyJoinable` — **evidence**, not "locally
  confluent": the critical-pair lemma for this exact formalism (multiset
  ordered hyperedges, non-injective binding) — i.e. that every
  overlapping divergence in every host factors through an enumerated
  pair with strong joinability transferring — is a proof note this
  project still owes (see the roadmap issue). What *is* mechanically
  fuzzed: for the rule the checker certifies, random overlapping
  divergences on random hosts reconverge.
- `confluent: true` additionally requires every rule to strictly
  decrease edge count — a well-founded measure, so rewriting terminates
  and Newman's lemma lifts local to global confluence.
- `NotConfluent` fires **only** on double saturation: both branches
  exhaustively enumerated, no bound hit, reachable sets disjoint — the
  host is a concrete initial state exhibiting an unjoinable divergence.
- Everything else is `Inconclusive`. In particular, edge-growing rules
  with nontrivial overlaps (the classic rule included) can never
  saturate, so their honest ceiling is `Inconclusive { BoundHit }` at
  any bound. Confluence of graph rewriting is undecidable in general
  (Plump); this is a prover-and-counterexample-hunter, never an oracle.

## 6. Determinism as a contract

Identical inputs produce byte-identical outputs, everywhere, on every
platform and thread count. The load-bearing choices: one mixing lineage
(`det::mix`, pinned to reference values); no `RandomState` where
iteration order can reach output (`det::DetMap`); no wall clock in any
export; counter-minted fresh vertices; matcher enumeration order
lexicographic in consumed-edge indices (delta maintenance reproduces it
byte-exactly, sorted-merge over survivors + seeded news); parallel
evolve structured so the serial bookkeeping phase is a pure function of
index-sorted parallel results — thread-count invariance by construction,
not by testing (though it is also tested). The committed golden files
passing on Linux, macOS, and Windows in CI are the cross-platform proof.

## 7. References

- S. Wolfram, *A Class of Models with the Potential to Represent
  Fundamental Physics*, Complex Systems 29(2), 2020 — the multiway /
  branchial / causal-invariance program this engine implements a
  substrate for.
- M. Willsey et al., *egg: Fast and Extensible Equality Saturation*,
  POPL 2021 — the e-graph sharing discipline whose state-granularity
  analogue is this engine's central move.
- B. Weisfeiler, A. Leman (1968); C. Morris et al., *Weisfeiler and
  Leman go Machine Learning* (survey) — color refinement, the
  refinement pass inside canonization.
- B. McKay, A. Piperno, *Practical Graph Isomorphism, II*, J. Symbolic
  Computation 60, 2014 — nauty/Traces; the individualization-refinement
  playbook adapted here to ordered multiset hyperedges.
- D. Knuth, P. Bendix, *Simple Word Problems in Universal Algebras*,
  1970 — critical pairs.
- D. Plump, *Hypergraph rewriting: critical pairs and undecidability of
  confluence* (in *Term Graph Rewriting*, 1993); *Confluence of graph
  transformation revisited* (2005) — strong joinability, and why plain
  joinability is not enough; undecidability of confluence for graph
  rewriting.
