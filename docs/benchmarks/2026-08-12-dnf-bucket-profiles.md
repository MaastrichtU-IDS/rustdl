# Self-time profiles across the DNF buckets (16 members)

**Date:** 2026-08-12 · **Binary:** `fa46c75` · **Raw data:**
[`data-2026-08-12-dnf-bucket-profiles.tsv`](data-2026-08-12-dnf-bucket-profiles.tsv)
· **Follows:** [the 164-ontology phase census](2026-08-12-dnf164-phase-census.md)

## Why

The census partitioned the tail; this asks whether the buckets are *internally*
homogeneous — specifically whether the wedge-trailing rewrite, justified by **one**
ontology (`ore_ont_6134`), generalises. Members were selected **spread across each
bucket's class-count range**, not sampled randomly, so a size-dependent cause would be
visible.

Method: gdb-attach sampling (12 attaches × all threads), innermost frame = self time,
idle rayon parks (`syscall`/`futex_wait`) excluded so the signal is the working threads.

## Result

| bucket | member | classes | stacks | dominant self-time |
|---|---|---|---|---|
| `label_cache` | `ore_ont_13122` | 7,120 | 396 | **`subset_sorted` 228 + `is_blocked` 94 (81%)** |
| | `ore_ont_7914` | 17,680 | 364 | `enumerate_matches` 41 |
| | `ore_ont_6712` | 42,183 | 396 | `enumerate_matches` 38 |
| | `ore_ont_11311` | 8,022 | 396 | `fire_clause` 20 / `enumerate_matches` 19 |
| | `ore_ont_9540` | 50 | 396 | `index<Option<ClauseMatchPlan>>` 19 |
| | `ore_ont_205` | 5,496 | 396 | `has` 12 |
| | `ore_ont_9429` | 2,636 | 396 | `is_blocked` 3 (diffuse) |
| | `ore_ont_10140` | 83,036 | 364 | `??` 54 (unresolved) |
| `prepare` | `ore_ont_11695` | 11,786 | 108 | `find<(Role,…)>` / `enumerate_matches` |
| | `ore_ont_13846` | 35,706 | **12** | `rotate_left<u64>`, `write<ExistentialFact>` |
| | `ore_ont_14817` | 58,364 | **12** | `eq<ClassId>`, `process_subsumer` |
| | `ore_ont_11460` | 83,482 | 172 | `??`, `as_ref<ClauseMatchPlan>` |
| `no-banner` | `ore_ont_10621` | — | **12** | hashbrown (`likely`, `full`) |
| | `ore_ont_14459` | — | **12** | `__memset_avx2_unaligned_erms` 11 |
| | `ore_ont_20` | — | **12** | BTree node traversal |
| | `ore_ont_7507` | — | **12** | `_mm_movemask_epi8`, `eq<ClassId>` |

## Findings

**1. The wedge-trailing rewrite is NOT justified. `to_vec<HyperNode>` — graph cloning
per branch — appears in 0 of the 8 additional `label_cache` members.** It was the
dominant cost on `ore_ont_6134` and nowhere else. Had this profiling not been run first,
a large rewrite would have been built for one ontology out of 164.

This is the second time in two days that a lever justified by `6134` failed to
generalise: the first was per-class locality, and `ore_ont_12432` (env-lock-bound) was
already a counterexample to `6134` being representative.

**2. Two costs DO recur, and both are self-contained.**
* **`subset_sorted` ← `is_blocked`** — 81% on `ore_ont_13122`, and 18% on `ore_ont_6134`
  in the earlier profile. Blocking is pairwise, so it scales with graph size; the routine
  itself is a 12-line sorted-subset scan.
* **`enumerate_matches`** — dominant on 3 of 8 (17,680 / 42,183 / 8,022 classes), i.e.
  the mid-to-large end. This is the non-Horn fire loop already named as the residual
  wedge-classify cost in the design record.

**3. 39% of the DNF tail is SINGLE-THREADED.** Every `no-banner` member and 2 of 4
`prepare` members yield exactly **12 stacks** where the healthy label-cache members yield
**396** — one working thread against ~33. The frames are data-structure construction
(hashbrown `find`/`full`, BTree traversal, `memset`) plus saturation
(`process_subsumer`, `write<ExistentialFact>`) and clause-plan building
(`as_ref<ClauseMatchPlan>`).

So the 64-ontology preprocessing class is not merely unbounded — it is **unparallelised**
on a 32-core machine, and it is 39% of the tail. That is a larger and better-supported
target than anything in the `label_cache` bucket.

## Caveats

* **`??` frames on the two largest members** (`10140`, `11460`) are unresolved symbols
  that gdb could not attribute. `perf` would resolve them; it is not installed on this
  host (`fsesrv-g1`). Those two rows are therefore uninformative, not evidence of
  absence.
* 12 samples per ontology is enough to identify a *dominant* frame and to distinguish
  1 working thread from 33; it is **not** enough for a percentage breakdown on the
  diffuse members (`9429`, `205`).
* Selection was spread across class count, which is a proxy for cause, not a guarantee of
  coverage. A cause confined to a shape not correlated with size could be missed.
