# Cheap per-class certification: refuted (third mechanism), and the prize is still there

**Date:** 2026-08-16 · Scores the candidate proposed by
`docs/2026-08-16-label-cache-reproduces-the-closure.md`. **Negative result.** The measurement
cost about twenty minutes; building it would have cost far more.

## What was being tested

The prize is established: on the two ontologies that DNF in `label_cache_build`, **100% of the
classes the wedge completed reproduced the EL saturation closure exactly** — 128 s and 273 s
spent confirming a result already in hand, plus another 103 s / 327 s producing `NoVerdict`.

The needed test is `T(C) ⟹ closure(C) ⊇ subsumers(C)`. Since the closure is always a lower
bound, `T(C)` makes it exact, so a certified class skips the wedge AND the tier-walk.

The crudest candidate, and the one to falsify first:

> **Taint** every class mentioned by an axiom outside `saturator_complete_fragment`, then
> certify `C` when neither `C` nor anything in `closure(C)` is tainted.

`tainted_classes()` reuses the shipped gate's prelude (functional roles, `disjoint_ok`,
`BareRoleDecls`) **verbatim**, so the taint and the real gate cannot disagree about what
"out of fragment" means. It is called only from the `RUSTDL_DUMP_LABELS` path and gates
nothing.

## Score

Ground truth is `labels == closure` per class, already in the dump. Precision must be 100% by
soundness; the number to maximise is recall.

| ontology | n | tainted | certified | **recall** | precision | wall saved |
|---|---|---|---|---|---|---|
| `ore_ont_11378` | 5,802 | 1,035 | 23 | **0.4%** | 100.0% | 0.0% |
| `sio` | 1,585 | 337 | 0 | **0.0%** | — | 0.0% |
| `ore_ont_10908` | 692 | 39 | 9 | **1.3%** | 100.0% | 0.1% |
| `ore_ont_16847` | 282 | 82 | 47 | **20.1%** | 100.0% | 0.4% |

**Refuted.** Recall 0.0%–20.1%, wall saved ≤0.4%. Precision was 100% everywhere it was
defined — no certified class disagreed with its labels — so the taint notion is at least not
obviously unsound; it is simply far too conservative to be useful.

## Why, and why the obvious refinement also fails

Tainted classes sit high in the hierarchy, so `closure(C) ∩ tainted = ∅` almost never holds.
The natural refinement is to exempt the worst offenders — but the blocking distribution is a
**long tail, not a hub**:

`ore_ont_11378`, classes blocked by the top-8 tainted classes:
`3468, 3306, 3268, 3197, 2610, 2380, 2050, 1937` (of 5,802).

| tainted classes exempted | `ore_ont_11378` certified | `sio` certified |
|---|---|---|
| 0 | 0.4% | 0.0% |
| 5 | 0.9% | 10.0% |
| 10 | 2.6% | 21.3% |
| 25 | 8.4% | 49.6% |
| 50 | **32.1%** | **72.4%** |

There is no single blocker to fix. Reaching a useful recall needs ~50 classes *cleared*, and
clearing one means proving that its out-of-fragment axiom cannot affect subsumption — which
is the semantic question the whole exercise was trying to avoid. Verifying a hub class with
its own wedge run does **not** clear it either: `labels(X) == closure(X)` says the closure is
exact *for X*, not that X's taint cannot affect a descendant differently.

## Three mechanisms now refuted

| mechanism | granularity | result |
|---|---|---|
| static inverse-trigger analysis | per-ontology | 6% of ontologies, misses the motivating case |
| `k ≪ n` merged pseudo-model refuter | — | needs 56–66% of n models to cover subjects once |
| **taint-free closure** | **per-class** | **0.0%–20.1% recall, ≤0.4% wall** |

The per-class framing was worth testing — the per-ontology gate has 0% recall against a truth
of 100% on `ore_ont_11311`, so the headroom is real — but this particular per-class test does
not capture it.

## Where that leaves it

**The prize is unchanged and large; no cheap mechanism has reached it.** What remains is
semantic certification: deciding, per class, whether an out-of-fragment axiom actually
participates in that class's subsumer derivation. Nothing measured so far suggests that is
cheaper than the per-class wedge search it would replace, and this is the same wall recorded
in `docs/2026-08-16-post-saturation-phase-value.md` ("a cheap upper bound is the whole
problem; this is the actual blocker").

**Do not build a syntactic certifier.** If this line is picked up again, the first move should
be to characterise, on `ore_ont_11311` specifically, *why* every class agrees with the closure
despite the ontology carrying inverse roles — i.e. what the inertness actually consists of —
rather than proposing another conservative syntactic approximation. The instrumentation to do
that (`RUSTDL_DUMP_LABELS`, emitting labels + closure + taint + per-class time) is now in the
tree and costs nothing when unset.
