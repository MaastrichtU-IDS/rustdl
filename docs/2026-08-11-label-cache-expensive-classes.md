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

## The cost is the DETERMINISTIC fixpoint, not the disjunctive search

Instrumenting `classify_labels` with the engine's own `SearchStats` settles it. A
**34-second** label class on `ore_ont_6134`:

| metric | value |
|---|---|
| `nodes` | **5,978** |
| `is_blocked_calls` | 1,721,455 |
| `match_attempts` | 3,280,836 |
| `branches_taken` / `restores` | **534 / 534** |
| `node_clones` | 534 |
| `fixpoint_passes` | 281 |
| `max_branch_depth` | 256 (the cap) |

`restores == branches` at saturated depth is *exactly* the `is_diverging` signature
the adaptive budget exists to cut, so the obvious read is "a diverging disjunctive
search, cut too late because `DIV_WINDOW = 500` is only sampled every 500 branches".

**That read is wrong, and two measurements kill it:**

| | `label_cache_build` |
|---|---|
| `RUSTDL_DIV_WINDOW` 500 / 100 / 50 | 101,299 / 101,207 / 101,242 ms |
| `RUSTDL_ADAPTIVE_BUDGET` 1 / 0 | 101,328 / 100,517 ms |

Rows identical (2,349) throughout. If the time were in the 534 branches, cutting at
branch 50 instead of 500 would have saved ~90%. It saved **nothing**, and disabling
the early-cut entirely costs nothing either. This independently re-confirms the
"DIV_WINDOW (null)" verdict already in the design record — now with a mechanism for
*why* it is null on this path.

**So the 34 s is spent before the branching starts**: in the deterministic
`horn_fixpoint` building a ~6,000-node graph by expanding transitive existentials
through `proper_part_of`. 534 branches over a 6,000-node graph is a *thin* search on
a *fat* graph.

## What this implies

The graph is substantially **the same for every class** — it is the part-of hierarchy
reachable from the class, and these classes sit in one chain. It is being rebuilt
**1,682 times**, once per label-cache entry.

That reframes the cluster completely. Every lever tried against it so far has been
aimed at the wrong thing:

* bounding the phase (08-08) — backfires, the cache is all-or-nothing;
* tightening the per-class budget — produces more misses, the wrong direction;
* per-class deadline precision — dominated by the aggregate arithmetic;
* the divergence early-cut / `DIV_WINDOW` — null, as measured above.

The lever the evidence actually points to is **sharing the deterministic graph across
classes** rather than rebuilding it per class — i.e. the "build-once, classify-many"
direction, not a budget or a search heuristic. That is a large piece of work and is
recorded here as a target, not started.

The per-class-locality idea from the previous section is still viable and is now
better motivated: a module would shrink the *graph*, which is where the time is. Its
soundness caveat (a smaller label set makes the prune more aggressive) is unchanged
and still must be settled first.

**Both diagnostics were reverted.** The `SearchStats` dump is ~15 lines in
`classify_labels`; the `RUSTDL_DIV_WINDOW` knob was dropped rather than kept, since a
knob for a constant now measured null twice is noise.

## Pricing the two candidate levers

A graph-wide label signature (temporary, reverted) on `ore_ont_6134`'s expensive
classes:

| | value |
|---|---|
| nodes per class | 2,772 – 2,906 |
| **distinct atomic labels in the graph** | **238 – 245** |
| ontology classes | **1,682** |
| distinct signatures across 204 expensive classes | **106** |

Two things this prices.

**1. Locality is real and quantified.** Each expensive class's graph involves only
**~14% of the ontology's classes**. So the ∀/⊔/DisjointClasses that force the hybrid
path are not merely *irrelevant* to these classes in principle — the search
demonstrably never labels 86% of the vocabulary. A ⊥-locality module would cut the
clause set the fire loop matches against, and `match_attempts` (3.3 M) is where the
time goes. Order-of-magnitude: ~7× fewer classes in scope.

Note what the node count says about the *shape* of the win: 2,800 nodes over 240
distinct labels is ~12 nodes per label, i.e. the graph is mostly **anonymous
witnesses** from transitive existential expansion. A module would therefore **not**
shrink the node count much — it shrinks the *clauses each node must be matched
against*. So the expected saving is in `match_attempts`, not in graph size, and that
should be verified directly before building.

**2. Cross-class duplication is ~2×, not ~200×.** 106 distinct signatures across 204
expensive classes. That is meaningful but far short of the collapse a naive
"compute-one-and-share" cache would need to pay for itself, and it tempers the
build-once framing from the previous section: the graphs are *similar*, not
*identical*.

**Caveat on the signature, stated so it is not over-read:** it hashes the labels of
**all** nodes in the final graph, which is not the label-cache's output — that is
`satisfiability_labels(fresh_q)`, the ROOT node's labels. Two classes sharing a
graph-wide signature therefore have very similar graphs but not provably identical
oracle content. The 106/204 figure prices *structural* sharing, not oracle
deduplication.

## Where this leaves the cluster

Ranked by what the evidence supports:

1. **Per-class locality module** for the label-cache probe — best-supported, since the
   measured 14% scope directly bounds the clause set, and the time is in clause
   matching. Blocked on the soundness argument (a smaller label set makes the prune
   more aggressive), which is analysis, not measurement, and should be settled first.
2. **Verify the mechanism before building it:** confirm that restricting the clause
   set actually cuts `match_attempts` proportionally. Cheap, and it would kill the
   lever early if the fire loop's cost is not clause-count-driven.
3. **Graph sharing / build-once** — reframed downward by the 2× duplication figure.
   Not the order-of-magnitude win the previous section implied.

All diagnostics reverted.
