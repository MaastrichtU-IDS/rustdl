# Merged pseudo-model refuter: NO-GO on "fewer models", untouched on "cheaper models"

**Date:** 2026-08-16 · Runs the go/no-go that
`docs/2026-08-16-global-model-spec-premise-expired.md` and
`docs/2026-08-16-km-architecture-lessons.md` both named as the gate. **Answer: the
`k ≪ n` formulation is structurally impossible.** The reframed target survives.

## The question

rustdl's Phase-7 label cache is `n` independent wedge runs — one per class — and it is where
the failing ontologies die (`ore_ont_11311`: `label_cache_build` 118,479 ms against
`tier_walk` 6 ms). It is also load-bearing, not overhead: with `RUSTDL_LABEL_HEURISTIC=0`,
`ore_ont_11378` goes from 3.0 s to DNF at 300 s.

The proposed replacement was one (or a few) merged pseudo-models. The gate was: **does a
merged model retain the prune rate?** A model pruning 90% instead of 99.9% would put
~120,000 pairs on the tableau for `ore_ont_11378` alone.

## Why it is answerable without building the merge

The refutation rule is `D ∉ labels(C) ⟹ C ⋢ D`, and `labels(C) ∋ C`. So a model `E` can
refute `(C,D)` exactly when `C ∈ labels(E)` and `D ∉ labels(E)`.

**A model is therefore useful only for subjects in its own label set.** That converts the
question into a set-cover computation over the label sets, which a dump of the existing
cache answers directly — no new engine.

Added `RUSTDL_DUMP_LABELS=<path>` (diagnostic only, off by default, reads the cache without
touching it; verified no file is written when unset, and the dump is byte-identical across a
refactor).

## The measurement

| ontology | n | mean \|labels\| | as % of n | own-model refute | **models to cover every subject once** |
|---|---|---|---|---|---|
| `ore_ont_11378` | 5,802 | 21.9 | 0.4% | 99.640% | **3,239 (55.8%)** |
| `sio` | 1,585 | 7.8 | 0.5% | 99.573% | 1,053 (66.4%) |
| `ore_ont_10908` | 692 | 9.7 | 1.4% | 98.745% | 447 (64.6%) |
| `ore_ont_16847` | 282 | 4.5 | 1.6% | 98.737% | 167 (59.2%) |

Greedy coverage curve on `ore_ont_11378` (greedy set cover is an **upper bound** on what any
`k`-model scheme can reach):

```
k=1      80 subjects (1.4%)
k=10    394 subjects (6.8%)
k=100  1,284 subjects (22.1%)
k=1000 3,563 subjects (61.4%)
```

## Verdict: NO-GO for `k ≪ n`

**Label sets are tiny — 0.4%–1.6% of the class count.** Each model is informative about
~1% of subjects, so covering all subjects needs **56%–66% of n models**, and that is only
the cost of touching each subject *once*. Matching the refutation power of a subject's own
model is strictly harder, because refuting `(C,D)` requires a covering model that also
*excludes* `D`.

So the best conceivable saving is ~35–44% of the label-cache builds. The bottleneck needs an
order of magnitude, not a third. **Do not build a few-models refuter.**

This is a structural property, not an artifact of one ontology: it holds across four, spanning
282 to 5,802 classes, and follows directly from label sets being ~1% of n.

## What this does NOT refute — and it is the important half

The measurement kills *"replace `n` models with `k` models"*. It says nothing about
*"keep `n` models and make each one cheap"* — and **that is what Konclude actually does.**

Konclude's model merging computes a pseudo-model per concept, but each is a cheap syntactic
abstraction read off a shared structure, then combined by a cheap pairwise mergeability
test. The `n` never goes away; the per-model *cost* does. rustdl's problem is precisely that
each `labels(C)` costs a full wedge search — ≥14.7 ms per class, against 0.14 ms per class
for the shared saturation fixpoint (≥105×).

Re-read in that light, KM's own diagnosis says the same thing and I had been reading it as
the `k ≪ n` claim:

> "both the per-concept verify funnel and the bare QO branching classifier **re-saturate per
> residue concept** … Konclude instead builds the model ONCE and branches only the small open
> core in place."

"Builds the model once" is about the *shared structure* the per-concept pseudo-models are
read from — not about there being one pseudo-model.

## Restated target

> **Derive each class's refutation labels from ONE shared saturation, instead of running a
> per-class wedge search.**

The obstacle is stated in `docs/2026-08-16-post-saturation-phase-value.md` and is unchanged:
refutation needs an **upper** bound on subsumers, the saturation closure is a **lower**
bound, and no cheap upper bound exists in the codebase today. The label cache *is* the upper
bound, and computing it is the expense.

**Next measurement, not next implementation:** is there a sound upper bound derivable from
the shared saturation structure that is loose enough to be cheap and tight enough to keep
~99% pruning? That is the same go/no-go shape as this one — quantify the prune rate of a
candidate bound offline, from a dump, before building anything.
