# Project kickoff: 1-UIP clause learning for the hypertableau wedge

Standalone undertaking, opened 2026-06-06. Follows the conflict-learning thread:
the *simple* dep-set nogood learning is sound but weak (PR #19 — −13.5%, clashes
are leaves), because full-dep-set nogoods only fire at the leaf. **1-UIP** learns
*short asserting clauses* (via resolution over an implication graph) that fire
*high* in the tree and prune whole subtrees — the technique behind CDCL SAT's
exponential gains. This doc frames the undertaking; **no code yet.**

**Status: research-grade.** Full 1-UIP over a *mutating* completion graph (merges,
`≥n` successors, nominals) is not a textbook transcription. Budget a **spike**
before committing to the full build.

## 1. Goal + success criterion

Close the wine-34 (and the hard-SROIQ-disjunction frontier) by making the
complete wedge search tractable. Concrete target: wine `hyper-sat` stalls
**90/137 → 0**, and the wine closure-diff MISالسED **34 → small/0** within the
classify budget, **FP=0 corpus-wide**, default behaviour unchanged (flag-gated).

## 2. What exists to build on (PR #19 foundation + current engine)

- **Decision/canon infra (PR #19):** canonical node identity (interned by
  creation provenance), a decision stack, interned decision ids, a nogood store
  with subset-pruning. Sound (FP=0 corpus-wide, verdicts byte-identical OFF↔ON).
  1-UIP reuses the canon/decision machinery; it changes *what* is learned.
- **Dep tracking:** `label_deps[i]: DepSet` (a `u128` of decision *levels*) per
  label; `fire_clause` sets a head label's deps to the union of its body labels'
  deps (`clause_body_deps`); `clash_deps` carries the conflict's level-set.
- **Search:** `solve(depth)` DFS; `horn_fixpoint` deterministic closure; `≤n`
  merge + `≥n` generation; `save`/`restore` snapshot the whole node graph.

## 3. The core change: an implication graph (not just `DepSet` levels)

`label_deps` collapses *why* a label holds into a level-set, which is enough for
backjumping but **not** for 1-UIP — resolution needs the antecedent *structure*.

Add, per derived label, an **antecedent record**:
`Antecedent = { clause_id, body: [LabelRef] }` where `LabelRef` identifies the
(canonical-node, class) label that fired it. Decision labels (the disjunct
asserted at a branch) are **roots** of the graph tagged with their decision
level. Deterministic horn-derived labels point back to their antecedents.

**Conflict analysis (1-UIP).** At a clash (two contradictory labels / `⊥` /
disjoint pair), start from the conflict's antecedent labels and resolve backward
(pop the most-recently-assigned current-level label, replace it by its
antecedents) until exactly **one** current-decision-level label remains — the
**1-UIP**. The learned clause = ¬UIP ∨ (negations of the other-level labels in
the cut). Backjump to the second-highest level in that clause, assert the UIP
(flipped) with the learned clause as its antecedent, continue.

## 4. The crux / research risk: the completion graph mutates

SAT's implication graph is over fixed variables. Here:
- **`≤n` merges** fuse nodes (union-find `representative`); labels of merged
  nodes combine. An antecedent referencing a node that later merges must remain
  resolvable — `LabelRef` must be merge-stable (key by canonical id, resolve
  through union-find), and merges themselves are non-monotone (they already force
  `DepSet::ALL`). **Decision:** exclude merge-derived labels from learning (as Inc
  1 excludes overflow), or model the merge as an antecedent — the spike must
  determine which is sound *and* useful. Wine is **merge-free on the hot path**
  (measured: stalled classes are `merge=0`), so a first cut can *require* the
  conflict's cut to be merge-free and still address wine.
- **`≥n` successors / nominals** create fresh nodes mid-search; their labels'
  antecedents must use the canonical (provenance) id, not the transient `HNode`.
- **`save`/`restore`** must include (or rebuild) the antecedent records — they're
  per-label state like `label_deps`.

This is the part with no off-the-shelf answer. **Spike first** (see §6).

## 5. Soundness + termination

- **Soundness.** A 1-UIP learned clause is derived by resolution from clauses the
  engine already holds + the conflict, so it is entailed — asserting it and
  pruning on it is sound (same family as Inc 1's monotonicity argument, but the
  clause is now a *resolvent*, not a raw dep-set). Merge/`≠`/NN (non-monotone)
  contributions stay excluded (their `DepSet::ALL`/overflow marks them). **Gate
  exactly as Inc 1:** flag default-OFF; FP=0 corpus-wide; verdicts byte-identical
  to OFF; differential fuzz (learning-ON ≡ OFF on random SROIQ).
- **Termination.** Learned clauses are bounded (over the finite canon×class
  literal space per decide); 1-UIP backjumping is the standard
  terminating CDCL loop; the existing depth bound remains a backstop.

## 5b. Cheap pre-spike measurement (2026-06-06) — GO

Before any structural code (per review), measured the would-be 1-UIP clause
length on the *current* engine: `popcount(clash_deps.bits)` per non-overflow
clash = #decision-levels the conflict depends on = upper bound on the 1-UIP
clause length. Wine (`hyper-sat`, temp counter, reverted):

```
levels/conflict:  =1: 24   2-3: 159 677   4-7: 30   8-15: 0   16+: 0   (n=159 731, mean 2.7)
```

**99.97 % of conflicts depend on just 2-3 decisions.** So 1-UIP clauses are
**short** (~2-3 literals) — the GO criterion. This also explains why Inc 1
(simple dep-set nogoods) was leaf-bound despite short conflicts: the 2-3 levels
span the *depth* (e.g. {0, 5, 12}); simple pruning fires only at the deepest
level (~leaf). 1-UIP learns the short clause, **backjumps to the 2nd-highest
level (5) and asserts**, skipping the entire 5→12 region — the far backjump is
exactly the subtree pruning Inc 1 structurally cannot do. **Verdict: GO for the
spike** (favourable, with the caution that the spike must demonstrate the
asserting-clause + backjump yields *super-linear* gains beyond Inc 1's 13 %).

## 6. Plan — spike first, then build

- **Spike milestone A — antecedent recording only (no learning).** The
  implication-graph recording is itself the load-bearing, mutating-graph-exposed
  work — split it out. Record per derived label its `Antecedent { clause_id,
  body: [LabelRef] }` through `horn_fixpoint`, surviving `save`/`restore` without
  desync. **Gate: corpus closure-diff byte-identical** (pure bookkeeping). If
  this doesn't come together cleanly on merge-free wine, **stop here** — a
  smaller, faster failure than building 1-UIP. (Per review: do not open the
  recording code until §5b's GO — done.)
- **Spike milestone B — restricted 1-UIP on wine.** On top of A, 1-UIP analysis
  restricted to merge-free conflicts, behind the flag. Measure: does wine
  `stalled` drop and `branches` fall *super-linearly* (subtree pruning, beyond
  Inc 1's 13 % leaf pruning)? **Go/no-go:** un-stalls wine classes → commit to
  the full build; else stop — the wine-34 are a permanent architectural limit.
- **Build (multi-week, only if spike is GO).** Full antecedent graph incl.
  save/restore; non-chronological backjumping with learned-clause assertion;
  merge/`≥n`/nominal handling (or principled exclusion); watched-literal
  propagation for learned clauses; the full gate suite. Increment + re-gate FP=0
  at each step.

## 7. Honest framing

This is the genuine multi-week, research-grade lever — the first thing in the
project with no clean precedent (1-UIP over a mutating completion graph). The
spike is the real decision point: it's cheap, it reuses the PR #19 foundation,
and it answers "does subtree-pruning materialise on wine" before any large
commitment. Recommended entry point for the undertaking.
