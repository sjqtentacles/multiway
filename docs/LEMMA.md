# The critical-pair lemma for this formalism — statement, transfer lemmas, and the honest gap list

**Status: NOT a complete proof.** Two of the three steps are proved
below (one with its mechanical half fuzz-pinned in the test suite); the
factorization step is argued carefully but has an explicitly listed
gap. Until that gap is closed, `Verdict::AllCriticalPairsStronglyJoinable`
stays what it says — *evidence* — and issue #4 stays open. Nothing in
the code claims local confluence.

## The formalism, precisely

- A **state** is a finite *multiset* of ordered hyperedges over
  vertices (`u32` labels). Duplicate edges are distinct *instances*.
- A **rule** `L → R` has LHS pattern edges over variables and RHS edges
  over LHS variables plus fresh variables.
- A **match** of `L` in host `H` is a choice of pairwise-distinct edge
  instances of `H` (one per LHS edge, arity- and position-compatible)
  together with a variable binding `σ`. **Binding is non-injective**:
  distinct variables may bind the same vertex
  (`matcher.rs::find_matches` doc, pinned by the parser/matcher suite).
- **Application** removes the matched instances, instantiates `R` under
  `σ` with fresh vertices for fresh variables, and adds the results.
  Vertices are never deleted (they are labels; a vertex "disappears"
  only in the sense that no remaining edge mentions it).

Two rewrite steps from the same host **diverge**; a divergence is
**strongly joinable** if both sides rewrite to a common state under an
isomorphism that fixes every *pinned host vertex* (the colored-canon
check in `confluence.rs` — see THEORY.md for why plain
joinable-up-to-isomorphism is not enough, after Plump).

## The claim to be proved

> **(CPL)** If every critical pair enumerated by
> `confluence::critical_pairs` is strongly joinable, then every
> divergence from every host is joinable — i.e. the rewrite relation is
> locally confluent.

The enumeration (`confluence.rs::critical_pairs`): for each rule pair,
every nonempty arity-matched partial injection between LHS edge lists,
variables unified positionwise by union-find, instantiated over the
minimal host; the trivial diagonal is skipped.

The classical proof shape needs three steps. A divergence in `H` either
touches disjoint edge sets (step 0), or overlaps — and an overlap must
factor through an enumerated critical pair via a **vertex quotient**
(step Q) followed by a **context extension** (step C).

## Step 0 — disjoint divergences commute

If the two matches consume disjoint edge instances, each survives the
other's application (instances are removed by identity, not by
content), and applying them in either order yields the same state up to
fresh-vertex renaming, which strong joinability permits (fresh vertices
are not pinned).

*Mechanical status:* fuzz-pinned by
`prop_disjoint_matches_commute_diamond` (tests/confluence.rs). **Done.**

## Step Q — the quotient lemma

> **(Q)** Let `q : V → V'` be any vertex identification (surjective on
> the host's vertices) and `q(H)` the edgewise image. Every match
> `(E, σ)` of `L` in `H` maps to the match `(E, q∘σ)` of `L` in `q(H)`,
> and `q(apply(H, r, (E,σ))) = apply(q(H), r, (E, q∘σ))` up to
> fresh-vertex renaming.

*Proof.* Match preservation: position-wise compatibility of `q∘σ` with
`q(edge)` follows by applying `q` to each compatibility equation; the
consistency constraint `σ(x) = σ(y) ⟹ (q∘σ)(x) = (q∘σ)(y)` holds
because `q` is a function — and the CONVERSE constraint (that a binding
must stay injective) does not exist in this matcher, which is exactly
what makes the lemma true here. Edge instances keep their positions
under the edgewise image, so distinctness of instances is preserved
(two identified-content edges remain distinct *instances* in the
multiset). Application commutes: removal is by instance (preserved),
and RHS instantiation under `q∘σ` is the `q`-image of instantiation
under `σ` except at fresh vertices, where both sides mint fresh labels
— equal up to fresh renaming. ∎

*Mechanical status:* the match-preservation half is fuzz-pinned by
`prop_quotient_preserves_matches` (tests/prop_engine.rs) — random host,
random surjective quotient, every host match's `(edge_idx, q∘binding)`
image located among the quotient's matches. MUTATION-CHECKED: making
the matcher require injective binding fails the prop immediately (the
quotient identifies two bound vertices and the image match vanishes).
The application-commutes half is not separately fuzzed; it is the same
argument `prop_evolve_deterministic` and the golden suite exercise
indirectly, but no dedicated test isolates it. **Proved; one half
fuzz-pinned.**

## Step C — the context lemma

> **(C)** If `K ⊆ H` (sub-multiset) and both divergence matches touch
> only instances of `K`, then a strong join of the divergence in `K`
> lifts to `H`: run the same rewrite sequences; the extra instances
> `H ∖ K` ride along untouched.

*Proof.* Matches used by the join sequences exist in `H` because a
match needs only its own instances to exist (adding instances never
*removes* a match; it may add new ones, but the join replays the
specific sequences found in `K`). Removal by instance never touches
`H ∖ K`. The final states differ from the `K`-joins exactly by the
common untouched `H ∖ K`, so equality-with-pinned-vertices transfers —
THIS is why joinability must be strong: the join isomorphism fixes the
pinned host vertices that `H ∖ K` may mention. A weak join can permute
them and die in context (the pinned `WeakOnly` counterexample in
THEORY.md). ∎

*Mechanical status:* the necessity direction (weak joins die in
context) is pinned by `weak_joinability_is_not_a_proof`
(tests/confluence.rs). The lifting direction has no dedicated fuzz.
**Proved modulo the note below.**

*Note (C-gap, minor):* "adding instances never removes a match" is
immediate for this matcher (matching never inspects non-matched edges —
there are no negative application conditions). If NACs are ever added,
(C) breaks and this file must be revisited.

## Step F — factorization (THE GAP)

> **(F)** Every overlapping divergence `(H, m₁, m₂)` factors as: an
> enumerated critical pair `(K, m₁', m₂')`, followed by a vertex
> quotient `q`, followed by a context extension into `H`.

*Argument.* Restrict `H` to the instances touched by `m₁ ∪ m₂`; call it
`K_H`. The pattern-side overlap structure (which LHS edges of `r₁`
coincide with which of `r₂`) is an arity-matched partial injection —
one of the enumerated ones. The enumerated pair's host `K` is built
from the *finest* positionwise unification for that injection; the
concrete bindings in `K_H` satisfy every unification equation and
possibly more, so the variable-to-vertex map factors through `K`'s
variables: `K_H = q(K)` for a vertex quotient `q` (Lemma Q's setting),
and `H = K_H + context` (Lemma C's setting). Strong joinability of the
pair transfers through Q then C. ∎?

**The gap, explicitly:**

1. **Join transfer through Q is asserted, not proved.** Lemma Q moves
   *one* rewrite step through a quotient. The join transfer needs: the
   entire join *sequences* found on `K` remain valid on `q(K)` AND
   still meet in a common state *with the pinned-vertex condition
   transported along `q`*. When `q` identifies two pinned vertices of
   `K`, the strong-join isomorphism on `K` need not descend to a
   well-defined isomorphism on `q(K)` — functions descend along
   surjections only when they respect the kernel. We believe the
   first-found joins do (they fix pins pointwise, and pointwise-fixing
   maps always descend), and that observation may be the missing
   half-page — but it is written nowhere rigorously and no test
   isolates it. **Open.**
2. **Same-rule total overlaps with automorphic match difference**: the
   enumeration skips the trivial diagonal (same rule, total
   identification, identical induced matches). A total identification
   with *different* induced bindings is kept — but the argument that
   every automorphic-match divergence induces a *different* unification
   (and is therefore kept) is folklore here, not written. Believed
   easy. **Open (small).**

What IS mechanically fuzzed today, short of (F):
`prop_certified_rule_divergences_join` — for rules the checker
certifies, random overlapping divergences on random hosts reconverge.
That is a direct statistical test of (CPL)'s conclusion, and it has
never found a counterexample.

## What closing the gap would earn

Documentation wording only (the `Verdict` enum is API-stable):
"evidence for local confluence" becomes "locally confluent (lemma:
docs/LEMMA.md)", and `confluent: true` (which additionally needs the
strict edge-count termination lint, giving Newman) becomes a theorem
about the bounded check rather than a strong heuristic. Issue #4
closes then, not before.

## References

- D. Plump, *Critical pairs in term graph rewriting* (1994); *Confluence
  of graph transformation revisited* (2005) — strong joinability, and
  the counterexample family THEORY.md pins.
- THEORY.md § confluence for this engine's checker semantics.
