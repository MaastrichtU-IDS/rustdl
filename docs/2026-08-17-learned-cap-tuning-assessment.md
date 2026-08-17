# Can we learn a dynamic function for the caps? Assessed — not on this evidence

**Date:** 2026-08-17 · Prompted by "can we learn an optimal dynamic function to set the various
caps?" **Answer: the idea is sound in principle and bounded in risk, but the two concrete
formulations both fail when measured, and the codebase's own history explains why.**

## What is favourable, and it is worth stating first

**Caps cannot break soundness.** Every cap in rustdl is a sound under-approximation — a cut
probe yields "not subsumed", a MISS at worst, never a false positive. So unlike a fragment
gate (the D10 bug class, six instances), a mis-set cap cannot produce a wrong answer labelled
complete. The risk is confined to wall time and completeness, both measurable. That makes this
a *much* safer place to be adaptive than certification.

**Precedent exists in-tree.** Two adaptive rules already ship: `adaptive_label_cache_ms`
(`clamp(n × per_pair, 50, 30_000)`) and the two-level adaptive inconsistency budget
(`work_proxy = ObjectPropertyAssertion × max(role-chains + transitive, 1)`, ≤300k ⇒ 12 s else
3 s). And per-instance algorithm configuration is an established field (SATzilla, SMAC,
ParamILS).

## Why the search space is smaller than it looks

**Most caps do not bind.** The 2026-08-03 constant audit measured five and found
`FIXPOINT_ITERS`, `DIV_WINDOW`, `RUSTDL_MAX_NODES` and `ID_SHALLOW_BUDGET_DIVISOR` either
non-binding or flat over a 16× range. There is nothing to learn for a cap that does not bind.

**The two that demonstrably bind are COUPLED, and the coupling is a known defect.**
`--pair-timeout-ms` feeds `adaptive_label_cache_ms`, so a *smaller* per-pair budget starves
the label cache and makes the whole classification **18× slower for byte-identical output**
(`ore_ont_15010`: 5.65 s → 103.98 s, restored exactly by
`RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000`). So this is joint configuration of a non-monotone
system, not independent per-cap tuning.

**The successful precedent is ELIMINATING a cap, not tuning it.** `HYPER_WEDGE_DEPTH = 256`
was wrong in *both* directions — `ore_ont_10407` needs depth **319**, `ore_ont_2182` needs
**≤7**. Iterative deepening removed the need to predict at all and won **16 recoveries, 0
regressions**. No learned constant can beat that, because a runtime-adaptive search has no
prediction error.

**This repo already ran the "fit a cost model" experiment, and it misled.** The adaptive
inconsistency budget analysis found global rank correlation *preferred the wrong feature*
(`class_assertions` +0.863 vs `ObjectPropertyAssertion` +0.462) because it "is dominated by
the mass of sub-millisecond ontologies, while the decision is about the tail". The rule that
shipped came from **mechanism**, not from fitting. It also found a second cost driver — the
fixpoint's pre-indexing prelude — that is **budget-independent**, so modelling it makes
things worse.

## The one target with a real prize, and it was tested

The largest measured waste is per-class label-cache builds that time out and produce
`NoVerdict` — i.e. nothing:

| | label phase | `NoVerdict` waste |
|---|---|---|
| `ore_ont_9944` | 600 s | **327 s (55%)** |
| `ore_ont_11311` | 231 s | **103 s (44%)** |

Skipping a predicted-failure is **verdict-identical** to letting it time out (both give
`NoVerdict`, falling through to the per-pair path), so it is FP-safe *and* completeness-neutral
by construction. That is an unusually clean learning target.

### Formulation 1 — skip predicted failures. REFUTED at the operating point.

Cheap feature: saturation closure size. **AUC 0.731 (`9944`) / 0.704 (`11311`)** — above
chance, and it *looked* promising. Median closure 22 for timeouts vs 15 for completions, with
heavy overlap.

The operating point kills it. Skipping every class with `|closure| > t`, on `ore_ont_9944`:

| t | time saved | successful classes skipped |
|---|---|---|
| 30 | 39 s | 598 |
| 25 | 110 s | **1,253** |
| 22 | 163 s | **1,536** |

At every threshold the sacrifice exceeds the catch, and the costs are **asymmetric**: a
skipped success loses pruning that runs at 96–100%, sending its ~n pairs to the per-pair path.
High precision is required and AUC 0.70 does not deliver it.

**Method note worth keeping: AUC above chance is not evidence of a usable rule.** The
absorption work was killed by AUC 0.480 (below chance); this is killed at 0.73, by the cost
asymmetry at every threshold. Always compute the operating point.

### Formulation 2 — order cheapest-first under an aggregate budget. UNRELIABLE.

Ordering cannot lose anything (nothing is skipped), so a weak predictor should still help.
Simulated from the per-class timing dumps against `RUSTDL_LABEL_CACHE_TOTAL_MS`-style
aggregate budgets, counting classes completed:

| budget | `ore_ont_9944` | `ore_ont_11311` |
|---|---|---|
| 25% of phase | **+538** | **+703** |
| 50% | **+1,374** | **−605** |
| 75% | +250 | **−565** |

Helps substantially on one, **hurts on the other at two of three budgets.** At AUC ~0.70 the
predictor is not good enough to reliably beat index order.

*Simulation caveat:* it stops at the first class that does not fit the remaining budget, which
is what a real aggregate-budget implementation would do, but it makes the result sensitive to
one mispredicted-cheap class landing early. A real implementation could differ; the point is
that no reliable gain is demonstrated.

## Recommendation

**Do not build a learned cap function.** Not because learning is wrong here — the soundness
properties are unusually favourable — but because on this evidence the signal is too weak at
the one place with a large prize, and most other caps do not bind.

If it is revisited, the conditions that would make it credible:

1. **Derive rules from mechanism, not from fitting.** The one shipped adaptive rule that works
   came from a causal proxy; the fitted correlation preferred the wrong feature.
2. **Evaluate on the tail, never the corpus.** Median ORE wall is ~50 ms; a model fit on 1,920
   ontologies optimises the mass and ignores the ~141 that decide anything.
3. **Expect noisy labels.** The `sweeps` phase varies **1.4–2.4× run-to-run** single-threaded,
   so "the optimal cap" is itself a noisy target.
4. **Prefer eliminating the cap.** Iterative deepening is the template: it beat a constant that
   was wrong in both directions, with no predictor.
5. **Richer features may raise AUC** — reachable-disjunction count, existential depth, told
   subsumer count — but the bar is set by the cost asymmetry above, not by AUC, and the burden
   is to show a usable operating point rather than a better ranking.

## Instrumentation

All of the above was computed from `RUSTDL_DUMP_LABELS` (labels + closure + taint + per-class
build time; diagnostic-only, off by default), which is already in the tree. Re-running any of
these assessments costs one classify per ontology.
