# The label cache reproduces the saturation closure — exactly, on 100% of completed classes

**Date:** 2026-08-16 · Follow-on from `docs/2026-08-16-merged-refuter-go-no-go.md`, which
killed the `k ≪ n` merged-refuter idea and restated the target as *"derive each class's
refutation labels from one shared saturation."* This measurement sizes that prize, and it is
larger than expected.

## What was measured

`RUSTDL_DUMP_LABELS=<path>` now emits three sections keyed by the same class indices: the
per-class pseudo-model labels (an UPPER bound on subsumers, `n` wedge searches, expensive),
the EL saturation closure (a LOWER bound, one shared fixpoint, cheap), and the per-class
label-build time in microseconds. Diagnostic only, off by default.

The comparison is therefore direct: **for how many classes, and for how much WALL, does the
expensive upper bound differ from the cheap lower bound?**

## Result

`closure ⊆ labels` holds for **100% of classes in every ontology measured** — the soundness
sanity check the dump has to pass.

| ontology | n | classes `labels == closure` | **share of label-cache WALL** |
|---|---|---|---|
| `ore_ont_16847` | 282 | 83.0% | 76.3% |
| `sio` | 1,585 | 86.6% | 88.5% |
| `ore_ont_11378` | 5,802 | 94.3% | 85.4% |
| `ore_ont_10908` | 692 | 99.9% | 99.9% |
| **`ore_ont_11311`** | 8,022 | **100.0% of completed** | 55.6% (+44.4% timeout) |
| **`ore_ont_9944`** | 8,008 | **100.0% of completed** | 45.4% (+54.6% timeout) |

**The headcount survives the wall check**, which is the check that matters — counting classes
where a lever applies is not the same question as what fraction of the phase's time those
classes consume, and a previous finding in this project died on exactly that distinction. Gap
classes are only modestly more expensive per class (0.6 ms vs 0.2 ms at worst), not
pathologically so.

## On the two ontologies that actually DNF, the number is 100% and 0

These are the bottleneck cases — `label_cache_build` is where they die:

| | `ore_ont_11311` | `ore_ont_9944` |
|---|---|---|
| classes | 8,022 | 8,008 |
| label-cache wall (per-class budget 300 ms) | **230.9 s** | **599.9 s** |
| `labels == closure` | 7,681 — **100.0%** | 6,921 — **100.0%** |
| `labels != closure` | **0** | **0** |
| `NoVerdict` (budget expired) | 341, 102.6 s (44.4%) | 1,087, 327.4 s (54.6%) |

So on the ontologies whose classification is *defined* by this phase:

* **Every class the wedge finished agreed exactly with the closure.** 128 s and 273 s
  respectively spent confirming a result already in hand.
* **The remaining half of the wall produced nothing at all** — `NoVerdict` is sound (it falls
  through to the per-pair path) but it is information-free.

Not a single class in either ontology had a label set the saturation closure did not already
contain.

## What this does and does not establish

**Establishes:** the label cache, on these workloads, is functioning as an extremely
expensive *certifier*. Its output is "the closure was right" — every time. The prize for
replacing it with a cheap certifier is the entire phase, and the entire phase is the DNF
cause.

**Does NOT establish:** that a cheap certifier exists. The circularity from
`docs/2026-08-16-post-saturation-phase-value.md` is untouched — you cannot know *which*
classes agree without computing the labels. What is needed is a test `T(C)` that is cheap and
**sound in the direction that matters**:

> `T(C)` ⟹ `closure(C) ⊇ subsumers(C)`

Since the closure is always a lower bound, `T(C)` makes it *exact*, and then
`D ∉ closure(C) ⟹ C ⋢ D` is sound **and complete** for that class. A certified class needs
no wedge search and no tier-walk verification — it is fully classified by saturation. This
is per-class fragment gating, generalising the existing global
`saturator_complete_fragment` gate from the whole ontology to one class's neighbourhood.

**Selection caveat, stated plainly:** `ore_ont_11311` is already known to be a case where the
inverses are inert (Konclude returns an identical 10,667-`SubClassOf` taxonomy with and
without them, and rustdl's saturation alone gets the complete answer in 1.13 s). So finding
the wedge adds nothing *there* is consistent with — and partly predicted by — what was
already known. Two bottleneck ontologies is not a population; the four smaller ones are the
broader evidence, and they run 83%–99.9% rather than 100%.

## Prior art in this project, and what it implies for feasibility

An earlier certification analysis this session (on `ore_ont_3794`) found **5,474 violations
of 19,930 concepts — ~73% certifiable**. A cruder static inverse-trigger test was refuted at
6% (`docs/2026-08-16-inverse-trigger-analysis-insufficient.md`). So the design space is real
but the strength of the test matters enormously, and a weak test is worth almost nothing.

Combining: certifying ~73% of classes on an ontology where the phase costs 230–600 s would
remove roughly three-quarters of the dominant cost on the DNF tail's largest cluster.

## Next step — measurement, not implementation

The same discipline that produced this result: **propose a candidate `T(C)` and quantify its
certification rate offline against the dump before building it.** The dump already contains
the ground truth (`labels == closure` per class), so any candidate test can be scored
directly — precision must be 100% by construction (soundness), and the number to maximise is
recall. Anything certifying well under ~70% is not worth building, by the same arithmetic
that killed the `k ≪ n` refuter.
