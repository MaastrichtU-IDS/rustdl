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

## Per-class locality: NO-GO, killed by one experiment

Step 2 of the ranked plan above was "verify that restricting the clause set actually
cuts cost proportionally — cheap, and it would kill the lever early". It did.

**First, two dead ends on the way to a usable module:**

* A **syntactic reachability closure** from the expensive class captured **5,353 of
  5,362 axioms** (signature 1,709 IRIs) — useless. The ontology is densely connected;
  everything reaches everything in 6 rounds.
* My earlier claim that "rustdl already has ⊥-locality module extraction in
  `locality.rs`" was **wrong**. `locality.rs` is a `LocalityPartition` computing
  connected components for *disjointness* (`definitely_disjoint`). The real extractor
  is `justify::extract_bot_module`, which is public and is what a build would use.

**The real ⊥-module is usefully small, and extraction is cheap:**

| seed class | module | extraction |
|---|---|---|
| `nlx_anat_20090704` (the 34 s class) | 1,323 / 3,645 candidates (**36.3%**) | 2 ms |
| `nlx_anat_20090505` | 1,321 (36.2%) | 3 ms |
| `birnlex_1167` (a superclass) | **3 axioms (0.1%)** | 0 ms |

Extraction projects to **3.4–5.0 s for all 1,682 classes** — not a blocker, and the
per-class variation (36% vs 0.1%) is exactly the shape a locality lever wants.

**But the cost is not clause-count-driven.** Writing that module out as an ontology
and re-running the label cache over it:

| | worst class | nodes | `match_attempts` | branches |
|---|---|---|---|---|
| full ontology | 35,841 ms | 6,018 | 3,345,616 | 606 |
| its 36.3% module | **36,788 ms** | 6,018 | **2,148,460** | 606 |

`match_attempts` fell **36%**, tracking the clause reduction almost exactly — and the
wall **did not move** (marginally worse, within noise). `nodes` and `branches` are
**identical**, so the graph and the search are unchanged by the module.

**Conclusion: a 36% clause cut buys 0% time.** Per-class locality is a NO-GO for the
label cache. Its soundness question (a smaller label set makes the prune more
aggressive) never needed settling, because the performance premise fails first — and
that is the cheaper thing to test, which is why it was tested first.

## What is still unexplained

The 34 s is not clause matching, not graph size (identical across arms), not the
disjunctive search (identical branch count; the divergence cut is null), and not
blocking granularity. Per operation it is slow: ~17 µs per match attempt, or ~7 µs
across match attempts plus 1.7 M `is_blocked_calls`.

That points at per-operation cost inside the fire loop or the label/graph data
structures, not at any count the current `SearchStats` exposes. **Settling it needs a
profiler, not another counter.** `perf` is unavailable on this host; gdb stack
sampling worked earlier in this arc and is the available tool.

Until that is done, **no further lever should be proposed for this cluster** — four
have now been measured out (phase bounding, per-class budget precision, the divergence
cut, per-class locality), every one of them aimed at a count that turned out not to
drive the wall.

## ROOT CAUSE (profiled): branch save clones the whole graph

Four count-based levers failed because `SearchStats` has no counter for the dominant
cost. Self-time attribution settles it. Sampling profiler (gdb attach ×20, all
threads, 660 thread-stacks) on `ore_ont_6134`'s label-cache phase:

| self-time frame | samples | called from | share |
|---|---|---|---|
| `mprotect` | 149 | `grow_heap` | 23% |
| `subset_sorted` | 120 | **`is_blocked`** | 18% |
| `write<HyperNode>` | 73 | **`to_vec<HyperNode>`** | 11% |
| `sysmalloc` / `_int_malloc` / `memmove` | 100 | allocation | 15% |
| `RwLock` read / read_unlock / is_read_lockable | 78 | — | 12% |

Grouped: **~38% allocation, ~24% blocking, ~11% node copying, ~12% lock traffic.**

The allocation and copying are the same cause. `HyperEngine::save`:

```rust
fn save(&mut self) -> Snapshot {
    self.stats.node_clones += 1;        // counts SAVES, not nodes
    nodes: self.nodes.clone(),          // full clone of the ENTIRE node vector
    representative: ..., neq: ..., block_index: ..., origin: ..., worklist: ...
}
```

**`node_clones = 534` counts 534 *whole-graph* clones.** At ~6,000 nodes each that is
**~3.2 million `HyperNode` copies**, every one with its own label `Vec` — which is
exactly the `to_vec<HyperNode>` → `write<HyperNode>` → `grow_heap`/`mprotect` chain the
profile shows.

### This explains every failed lever

| lever | why it failed |
|---|---|
| clause-set restriction (locality) | graph size unchanged ⇒ clone cost unchanged. `match_attempts` fell 36%, wall unmoved. |
| phase bounding / per-class budgets | budgets do not change per-branch clone cost |
| divergence early-cut / `DIV_WINDOW` | fewer branches would help, but the cut needs `depth_saturated`, which these searches reach late |
| reading `SearchStats` | **`node_clones` is per-save, not per-node** — 534 reads as trivial and is 3.2 M node copies |

That last row is the methodological point: the counter that named the problem
*understated it by ~6,000×*, so every count-driven analysis looked elsewhere.

### The architectural gap

The **main tableau already solves this**: `TableauTrail` gives log-and-undo
backtracking via `Checkpoint` markers — O(changes) per branch. The **wedge** instead
does copy-on-save — O(graph) per branch. On a fat graph with a thin search (6,000
nodes, 534 branches) that is the worst case for copying and the best case for
trailing.

### Ranked targets, now evidence-based

1. **Trail the wedge's branch state instead of cloning it** (~50% of self-time:
   allocation + node copying). Large but well-understood — the main tableau is a
   working reference implementation in-tree.
2. **`subset_sorted` in `is_blocked`** (~18–24%, 1.7 M calls). Self-contained and much
   cheaper to attempt. Note this **corrects an earlier conclusion in this document**
   that the cost was "not blocking" — that was inferred from the `DIV_WINDOW` and
   module experiments, neither of which isolated blocking.
3. **12% `RwLock` traffic** under 32 threads suggests contention on a shared
   structure; worth identifying before assuming the parallel classify scales.

**Do not start (1) without re-verifying the profile on a second cluster member** —
this is one ontology, and the arc's repeated lesson is that a single instance
motivates but does not price a lever.

## The cluster is NOT homogeneous — and the second member's cost was `getenv`

The doc above required verifying the profile on a second cluster member before
starting the trailing rewrite. Doing so found a **completely different** dominant cost.

| self-time | `ore_ont_6134` | `ore_ont_12432` |
|---|---|---|
| `RwLock` (read / read_unlock / is_read_lockable) | 12% | **76%** (244+182+75 of 660) |
| allocation + node copying | ~49% | negligible |
| `subset_sorted` ← `is_blocked` | 18% | 0.8% |
| actual reasoning (`enumerate_matches`, `fire_clause`) | — | **~3.5%** |

The lock is **not** one of ours. Walking up the stacks: `is_read_lockable` ←
`RwLock::read` ← `env_read_lock` ← `getenv` — it is the **process-global environment
lock**, taken by `std::env::var_os`. Under rayon, 32 threads reading feature flags
serialise on it.

Two call sites were doing this per-operation, **both introduced by this arc**:

* `hyper_fixpoint_deadline_enabled()` in `horn_fixpoint`'s drain loop (2026-08-08,
  default-ON 08-10). Worse, the condition read `flag && steps.is_multiple_of(STRIDE)`,
  and `&&` is left-to-right — so the `getenv` ran on **every iteration**, not every
  256th.
* `hyper_match_deadline_enabled()` in `enumerate_matches`' recursion (v0.4.16).

### Fix and effect

Both flags are now read **once per engine** into `HyperEngine` fields, beside the
existing `at_most_exhaust_probe`, and the cheap stride test comes first.

| ontology | before | after | |
|---|---|---|---|
| `ore_ont_12432` `label_cache_build` | 58,608 ms | **12,491 ms** | **4.69×** |
| `ore_ont_6134` `label_cache_build` | 60,724 ms | 60,993 ms | 1.00× |

**Per-ENGINE, not a process-wide `OnceLock`** — that was tried first and is why the
granularity is called out: a `OnceLock` measured *faster* (9.65×) but **broke
`zero_is_off`**, because the canaries set these vars per test and the first test to run
wins. The shipped number is the per-engine one.

**It is a phase speedup, not a recovery.** `ore_ont_12432` still DNFs at 200 s in both
arms — the label cache got 4.69× cheaper and the rest of classify still exceeds the
budget.

### Consequences for the cluster

* **A single lever cannot fix this cluster.** Two members, two unrelated dominant
  costs. The trailing rewrite (ranked #1 from `6134`'s profile) would have done
  **nothing** for `12432`.
* **Every remaining hot-path `std::env::var_os` is suspect.** `classify.rs` has 17
  call sites and `reasoner/src/lib.rs` has 89; most are per-query and harmless, but any
  in a per-node or per-clause path costs the same global lock. Worth an audit.
* The `6134` targets (trailing, `subset_sorted`) stand unchanged, and still need a
  second member that actually exhibits them before being priced.

Gates: workspace **1,605 pass / 0 fail**; FP=0 net with **zero** `FP>0`/`MISSED>0`
lines; canaries 4+4; fmt and clippy clean. A full corpus sweep has **not** been run —
the change is behaviour-preserving by construction (same flag value, read once per
engine instead of per iteration), but a sweep is the honest gate for a hot-loop change
and is the next step.

**Profiling note:** `perf` was unavailable — not because it is missing from the host
the user installed it on, but because this session's shell runs on `fsesrv-g1` while
that install landed on `fsesrv-node000003`. The two share `/data/dumontier` over NFS,
which made the repo and corpus look identical while `/usr/lib/linux-tools` did not
exist at all. All figures in this document are from `fsesrv-g1`, via gdb stack
sampling.
