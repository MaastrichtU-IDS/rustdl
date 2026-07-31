# The small-input / high-RSS / DNF cluster: two mechanisms, not one

**Date:** 2026-07-31 (rustdl v0.4.6)
**Method:** `superpowers:systematic-debugging`, Phase 1–3. No fix proposed until root cause.
**Source data:** `owl-reasoner-harness` baseline
`baselines/2026-07-31-ore-rustdl-v046-t1-c30.jsonl`; interpretation in
`docs/benchmarks/2026-07-31-ore-full-sweep-v046.md`.

The full ORE sweep surfaced a cluster of ontologies that **DNF with a small input and a large
peak RSS** — a ratio that suggests a local, conversion-time cause rather than a search blowup
(the signature `ore_ont_9347` had: 8.6 MB → 70.7 GB, fixed in ~8 lines). This is the
investigation of that cluster.

**Headline: it is at least two unrelated mechanisms, and the entering hypothesis was wrong.**

## Phase 1 — component isolation (`--saturation-only` vs `tbox-stats` vs full)

Single-thread, 60 s cap:

| ontology | conversion | saturation | full | dominant |
|---|---|---|---|---|
| `ore_ont_11085` | 0.10 GB / 0.27 s | **21.40 GB** | 21.67 GB | **saturator** |
| `ore_ont_16632` | 1.83 GB / 12.4 s | 2.44 GB | 7.57 GB | conversion + tableau |
| `ore_ont_11126` | 1.77 GB / 11.8 s | 2.36 GB | 7.34 GB | conversion + tableau |
| `ore_ont_10425` | 1.29 GB / 7.4 s | 1.56 GB | 5.90 GB | conversion + tableau |

Structural profile explains why these do not belong together:

| ont | classes | SubClassOf | ∃ | ∀ | `DataPropAssertion` | literals |
|---|---|---|---|---|---|---|
| `11085` | **22,642** | 22,831 | 1,354 | 0 | 0 | 0 |
| `16632` | **11** | 88 | 0 | 1 | **17,415** | 34,830 |
| `11126` | **11** | 84 | 0 | 1 | **16,387** | 32,774 |
| `10425` | **18** | 57 | 1 | 6 | **8,227** | 16,454 |

- **Group A = `11085`**: 22.6 k classes, TBox-driven, cost entirely in the saturator.
- **Group B = `16632` / `11126` / `10425`**: **eleven to eighteen classes**, megabytes of
  data-property assertions, cost in conversion. Eleven classes cannot produce a quadratic
  class matrix, so this cannot be the same bug.

## Three hypotheses refuted by measurement

**1. The D4 eager `num_total_classes²` allocation** (from
`docs/superpowers/specs/2026-07-18-d4-saturator-memory-rootcause.md`, which found exactly this
signature on `ore_ont_3914` at 158 GB). **Refuted for `11085`.** Sampling `/proc/<pid>/status`
during a saturation-only run:

```
   t   VmPeak(GB)  VmHWM(GB)
   3s       1.18       0.92
   6s       2.18       2.01
   9s       4.18       3.15
  12s       8.18       4.29
  27s      16.18       8.27
  30s      16.18       9.24
```

VmPeak **doubles** (1.18 → 2.18 → 4.18 → 8.18 → 16.18, i.e. ×2 + ~0.18). A single eager
allocation in `new()` would reach its final size in the first sample and stay flat. This is a
container reallocating geometrically, with VmHWM trailing as pages are touched. Note also that
RSS is *time-dependent* — `11085` read 9.25 GB at a 30 s cap and 21.67 GB at 60 s — so any
single-number RSS for it is really "RSS at the cap".

**2. Runtime Tseitin synthetic minting** (the Phase 2a functional-role witness-merge, the only
path that allocates class ids during saturation). **Refuted:** `11085` contains **zero**
functional-role axioms, so Phase 2a cannot fire.

**3. `seed_bucket`'s unbounded O(k²) walk** — the genuinely unfixed sibling of the bounded
`seed_disjoint_bucket`. `seed_bucket(out, keys, subset)` takes **no** component argument and its
body is an unguarded ordered-pair double loop (`for i … for j … if i == j { continue }`). This
looked like Group B's cause, and the arithmetic appeared to agree (17,415 assertions → ~303 M
calls ≈ 12 s vs 12.36 s measured). **Refuted by direct measurement:**

```
ore_ont_16632   keys=1789  calls=3,198,732   emitted=1788  ms=11
                keys=6934  calls=48,073,422  emitted=6933  ms=777
```

Keys are **deduplicated** (6,934 distinct, not 17,415), so the real cost is ~788 ms of a
12,360 ms conversion — **about 6%**. The arithmetic agreement was coincidence. This is the
reason to confirm a hypothesis by measurement rather than by a matching number.

## What Group B actually is

`RUSTDL_DKEY_SPLIT_STATS` on v0.4.6:

| ont | concept_rules | of which `DKey` disjointness pairs | split judges droppable |
|---|---|---|---|
| `16632` | 6,614,036 | **6,605,217 (99.87%)** | **0** |
| `10425` | 4,228,740 | **4,223,387 (99.87%)** | **0** |

And `RUSTDL_DATA_PROPERTIES=0` collapses both to **24** and **33** rules respectively, so
essentially the entire cost is the data channel.

**The split reporting `would_drop = 0` is CORRECT, not a bug.** `16632` carries **74
`DataMaxCardinality`** axioms (and 1 `ObjectMaxCardinality`). A `≤n` is a genuine COLLAPSE
source: it forces two distinct successors onto one node, so two distinct data *values* really can
share a label, and the pairwise disjointness is what detects the resulting clash. These pairs are
**consumable**. Bounding them further would be a completeness regression.

Group B is therefore **not an over-seeding defect at all. It is a missing algorithm**, and
CLAUDE.md already names it:

> *"data cardinality — D4 already catches the unsat-clash patterns; full range-size-aware counting
> (`≥3 p` over a 2-value range → ⊥) is a concrete-domain cardinality reasoner with **zero measured
> corpus reward**."*

**Group B is that reward, now measured:** three ORE ontologies, 4.2–6.6 M materialised axioms
each, all DNF. `DataMaxCardinality(≤n, p)` with k distinct values needs `k > n` decided
**arithmetically** — O(k) — instead of materialising C(k,2) pairwise-disjointness axioms to let
the clash rule rediscover it. That reframes the work from "add another gate" to "add the counting
rule", and it now has evidence it previously lacked.

## Status per group

**Group B — cause identified, fix is a design task.**
1. *Recorded, deliberately NOT built:* the `seed_bucket` singleton skip. Sound by the code's own
   stated invariant (*"distinct keys ⟹ strict subset, since equal ranges share one ClassId"*) — a
   singleton range cannot be a strict superset of anything, so every pair whose `sup` is a
   singleton provably emits nothing. Confirmed by the measurement above: **`emitted = keys − 1`
   in all six buckets**, i.e. exactly one key (the bare-datatype `Top`) is a proper superset and
   every other is a singleton beneath it. Implementing it needs an `is_singleton` predicate
   threaded through 7 generic call sites, and buys **~6%** while changing no outcome — so it is
   logged as a small, sound, low-priority win rather than done now.
2. *The real fix:* a concrete-domain cardinality counting rule, needing its own spec.

**Group A (`11085`) — OPEN.** 22,642 classes, saturator-resident, geometric doubling to ≥21.7 GB,
D4 refuted, Phase 2a refuted, cause unidentified. Next step is to find which container doubles:
the doubling steps (~1 → 2 → 4 → 8 → 16 GB) and its 0 functional / 0 ∀ / 1,354 ∃ / 9,486
`ClassAssertion` profile are the constraints any candidate must fit.

## Method note

The entering hypothesis came with strong prior art and matching arithmetic, and was still wrong
on all three counts. What separated signal from coincidence each time was a **component-boundary
isolation** (`--saturation-only`, `tbox-stats`, `RUSTDL_DATA_PROPERTIES=0`) followed by a **direct
count**, never a plausible number. See `skills/corpus-measurement/SKILL.md` in the
`owl-reasoner-harness` repo.
