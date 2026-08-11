# What makes a label-cache class expensive (`ore_ont_6134`)

2026-08-11. Characterises the ~11-ontology DNF cluster whose wall is
`label_cache_build`-dominated. Companion to
`docs/2026-08-08-label-cache-aggregate-bound.md`, which established *that* bounding
the phase backfires (the cache is all-or-nothing) and left *why individual classes
are expensive* open.

## Baseline confirmed at current defaults

The cluster is unchanged after the 08-08…08-10 default flips (domain absorption,
match deadline, fixpoint deadline, consistency probe, fraction gate). Under a 100 s
global budget:

| ontology | classes | `label_cache_build` |
|---|---|---|
| `ore_ont_6134` | 1,682 | 101,165 ms |
| `ore_ont_12432` | 2,748 | 82,316 ms |
| `ore_ont_10080` | 3,533 | 99,880 ms |
| `ore_ont_13122` | 7,120 | 99,992 ms |
| `ore_ont_6910` | 6,131 | 95,452 ms |

## The distribution is extremely skewed

Per-class instrumentation on `ore_ont_6134` (temporary, since reverted; threshold
50 ms):

* **256 of 1,682 classes** cost ≥50 ms.
* Those 256 account for **3,853 s of CPU** — which ÷32 cores is the ~101 s wall.
* The worst single class costs **27.6 s**; the top dozen are **9–31 s each**.

**Correction to an earlier figure:** the "tail of 400–560 ms classes" recorded on
08-08 came from a different configuration (a 1 ms budget, measuring *overshoot*). At
a 50 ms per-pair budget the real tail is **9–31 seconds per class**, an order of
magnitude worse than the number that motivated the aggregate-bound work.

## What the expensive classes have in common — and it is not their own axioms

They are **contiguous high indices (1644–1671)** from one IRI family,
`NIF-GrossAnatomy#nlx_anat_2009*`, and they are **syntactically trivial** — 2–3
axioms each, plain EL:

```
SubClassOf(nlx_anat_20090704 birnlex_1167)
SubClassOf(nlx_anat_20090704 ObjectSomeValuesFrom(ro#proper_part_of …))
```

So the cost is not in the class definition. It is in what the satisfiability check
*reaches* through that role. `ro#proper_part_of` is:

* **transitive** (`TransitiveObjectProperty`),
* the target of a **declared inverse** (`has_proper_part`),
* a **sub-property of `part_of`**,
* and used by **779 existentials** in the ontology (of 790 mentions).

There are 15 transitive roles and no role chains. The expensive classes are the ones
deepest in that part-of chain, which is why they cluster.

## The mismatch this exposes

`ore_ont_6134` is genuinely out-of-EL — **206 `ObjectAllValuesFrom`, 99
`ObjectUnionOf`, 497 `DisjointClasses`, 1 `FunctionalObjectProperty`** — so the
hybrid path is correct *for the ontology*. But the expensive classes are individually
**pure EL**, and each one's label check runs the full out-of-EL wedge over the
**whole** TBox. The ∀ and ⊔ that force the hybrid path live elsewhere in the
ontology and are irrelevant to these classes' satisfiability.

## Candidate lever: per-class locality

rustdl already has ⊥-locality module extraction (`owl-dl-core/src/locality.rs`, built
for `justify`). Running a class's label computation over its **module** rather than
the whole TBox would shrink exactly these graphs.

**The soundness argument needs care, and is the reason this is recorded rather than
built.** The prune is `D ∉ labels(C) ⇒ C ⋢ D`, justified by a *counterexample model*.
Computing labels in a module yields a SMALLER label set, which makes the prune MORE
aggressive — unsound unless the module provably preserves every entailed subsumer of
`C`. Standard modularity theory does give this for `D` in the module's signature, and
for `D` outside it the extraction guarantee means `C ⊑ D` cannot hold; so the
argument is plausibly complete. But "plausibly" is not the standard this prune is
held to, and it must be settled before any build.

**Cheaper thing to try first:** measure whether the cost is graph SIZE or blocking
cost over that graph (blocking is pairwise, so O(n²) in graph size). If it is
blocking, the fix may be local to `is_blocked` rather than requiring modules. That
measurement was not run.

## Status

Characterisation only; no code shipped. Instrumentation (a gated per-class
`eprintln!` in the label-cache loop) was reverted — the recipe is 6 lines and the
threshold used was 50 ms.
