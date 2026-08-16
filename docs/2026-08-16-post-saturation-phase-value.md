# What the post-saturation phase is worth, and why no cheap termination test exists

**Date:** 2026-08-16 · Follow-on from the `ore_ont_11311` investigation.

## Corrected measurement

An earlier pass compared `direct` row counts and reported "87% identical, deltas
+15/−6/−5/−3/+2/+1/+1". **That compared the wrong quantity.** `direct` is a transitive
*reduction*, so finding MORE subsumptions can REDUCE the direct-edge count as new
intermediates absorb direct links. The negative deltas were reduction artifacts, and they
persisted unbounded, which is how they were caught.

Re-measured on **closures**, over the ontologies whose `direct` counts differed:

| ontology | sat closure | full closure | full-only | **sat-only** |
|---|---|---|---|---|
| `ore_ont_11378` | 120,179 | 120,413 | **+234** | **0** |
| `ore_ont_7877` | 685 | 693 | +8 | 0 |
| `ore_ont_15010` | 281 | 284 | +3 | 0 |
| `ore_ont_16847` | 914 | 917 | +3 | 0 |
| `5303` / `6967` / `9786` | — | — | +2 each | 0 |

**`sat-only = 0` everywhere** — the saturation closure is always a subset of the full one, as
soundness requires. Every difference is the full path finding *more*, never saturation
over-reporting.

Magnitudes are small: **0.003%–0.19%** of the closure.

## The prize is precisely bounded, and it is large

**Across every gainer the entire value of the phase is carried by ≤7 distinct
SUPERCLASSES**, out of thousands of classes:

| ontology | gained pairs | distinct supers | distinct subs |
|---|---|---|---|
| `ore_ont_11378` | 234 | **2** | 232 |
| `ore_ont_7877` | 8 | 7 | 7 |
| `ore_ont_16847` | 3 | 1 | 3 |
| `ore_ont_15010` | 3 | 2 | 3 |
| `ore_ont_5303` | 2 | 1 | 2 |
| `ore_ont_6967` | 2 | 2 | 1 |
| `ore_ont_9786` | 2 | 2 | 1 |

On `ore_ont_11378` a single hub class (`InheritableType`) accounts for 232 of 234 pairs. So
if the tier walk knew which handful of superclasses mattered, it could skip essentially all
of its work.

## Why no cheap test identifies them

**The hard superclass is syntactically unremarkable.** `InheritableType` is a plain named
class: no `EquivalentClasses` definition, no disjunction, no complement — just 232
`SubClassOf` axioms pointing into it. Nothing distinguishes it from any other hub.

**The mechanism is elsewhere in the ontology.** `Rose ⊑ InheritableType` is reported by
`explain` as *"answered by tableau (closure didn't witness it)"*. `Rose` carries
`ClassAssertion(PlantPartType, Rose)` — COSMO puns classes as individuals — and the ontology
has 495 `ObjectHasValue` (nominals) and 370 `ObjectUnionOf`. The saturator drops the
nominal/punning route. The *subject* is where the unusual construct lives; the *superclass*
that gains is ordinary.

**The bounds argument.** A sound termination test needs the candidate set to be empty, and

> candidates = UPPER bound − LOWER bound

The lower bound is the saturation closure — cheap, already computed. The upper bound is
`D ∈ labels(C)`, which is exactly what the label cache computes, and building it *is* the
expensive phase. **Any termination test built on it is circular.** No cheaper sound upper
bound exists in the codebase today.

## Status of the ideas tried

| idea | verdict |
|---|---|
| static inverse-trigger affected-set | **refuted** — 6% of ontologies, misses the motivating case (`docs/2026-08-16-inverse-trigger-analysis-insufficient.md`) |
| target by superclass syntax | **refuted here** — the gaining supers are ordinary named classes |
| terminate when a round learns nothing | **still open**, but dynamic: needs a fixpoint, and a round that learns nothing still costs a full pass |
| cheap upper bound on subsumers | **not available**; this is the actual blocker |

## What would move it

A cheap upper bound is the whole problem, and there is one known candidate the codebase has
already scoped but not built: **one global model instead of n per-class models**
(`spec/global-model-rewrite`, `docs/global-model-rewrite-spec.md`). A single satisfying model
refutes many pairs at once — `D ∉ witness_types(C)` — which is the same subtractive,
sound direction the shipped `RUSTDL_PSEUDO_MODEL` realize shortcut already uses.

That is the design this measurement argues for, and the measurement now sizes it: on 53
sampled ontologies the phase it would replace changes **nothing at all on 46**, and on the
other 7 its entire output is ≤7 superclasses each.

**Not built. Nothing here is a shipped change.**
