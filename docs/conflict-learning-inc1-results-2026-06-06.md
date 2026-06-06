# Conflict-driven learning — Inc 1 built + measured (2026-06-06)

Production Inc 1 of the design in `docs/conflict-learning-design-2026-06-06.md`
(PR #18). Behind `RUSTDL_HYPER_LEARNING` (default **OFF**). **Sound, but the
simple lever is weak — STOP here; the real lever is 1-UIP CDCL.**

## What was built

- **Canonical node identity** (`Learn::node_canon`): interned by
  `(parent_canon, role, qualifier, decision-path-at-creation, disamb)` — stable
  across subtrees for the same logical node so a learned nogood transfers.
  `disamb` separates the `n` symmetric `≥n` successors.
- **Decision stack** (per level `d`) + interned decision ids `(clause, node
  canon, disjunct)` and an interned decision path.
- **Nogood store**: at a non-overflow clash, record the clash's dep-set as a
  nogood (set of decision ids); index by decision id. Before recursing a
  disjunct, if the active decision set ⊇ a stored nogood, prune it (sound:
  deps ⊇ nogood ⟹ clash, by monotonicity; overflow clashes — the non-monotone
  `≤n`/`≠`/NN cases — are excluded).
- Plumbed via `HyperEngine::with_learning()` + `hyper_learning_enabled()`
  (`RUSTDL_HYPER_LEARNING`, default off) at all wedge-construction sites.

## Measurement (wine, `hyper-sat`, 1 s/class)

```
recorded = 1 015 nogoods    pruned = 157 733 disjuncts
total_branches 1 165 104 → 1 005 819 (−13.5%)    stalled 90 → 90 (0 un-stalled)
```

## Soundness — validated empirically (the key positive)

Corpus closure-diff with `RUSTDL_HYPER_LEARNING=1` is **byte-identical to OFF**:
FP=0 and every MISSED unchanged across **alehif, galen, notgalen,
shoiq-knowledge, ore-10908, ore-15672, sio, wine**. This confirms the
monotonicity + overflow-exclusion soundness argument on real
SROIQ/nominal/cardinality inputs — a rare, valuable result for a learning
mechanism.

## Why the simple lever is weak (a legitimate stop, not a bug)

`recorded`/`pruned` match the recurrence probe almost exactly (1 015 ≈ 1 017;
157 733 ≈ 158 891) — so the probe was **not** inflated and pruning fires
massively. But the recurring clashes are **leaves**: each prune removes ~1 branch
(157 733 prunes ≈ the 13.5% drop), never a subtree. The nogood is the *full*
clash dependency set, so it is only satisfied at the leaf and can't prune higher.

**The real lever is 1-UIP clause learning** (CDCL): short asserting clauses
derived by resolution over an implication graph are satisfied high in the tree
and prune whole subtrees. That needs an implication graph + resolution +
learned-clause-driven backjumping — substantially larger than this dep-set
recording, and the precise next step if the wine-34 / hard-SROIQ-disjunction
frontier is pursued.

## Decision

STOP at simple learning (the design's own stop criterion). This sound,
default-OFF implementation + measurement stay on `feat/conflict-learning` as the
**foundation for a future 1-UIP effort** — **not merged to `main`** (a
default-OFF 13% feature is maintenance cost without 1-UIP). The wine-34 remain
the documented frontier, now precisely scoped as a 1-UIP CDCL project.
