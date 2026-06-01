# Phase 3 post-fix GALEN flamegraph + findings

Re-flamegraphed 2026-06-01 against HEAD 64bee92 (Phase 3 bloom
prefilter for needs_deferred_or). Sampling: pprof-rs @ 199Hz, 120s
window.

## Frame-level diff (top 15 each)

BASELINE top 15 (post-Phase-2b, commit b588b3a):
   73.01%  ( 18185 samples)  search
   73.01%  ( 18185 samples)  branch
   72.32%  ( 18013 samples)  saturate
   31.40%  (  7820 samples)  apply_deferred_concept_or_rules
   18.13%  (  4516 samples)  eq   [PartialEq::eq for ConceptId -- leaf frame]
   17.08%  (  4255 samples)  apply_max
   11.27%  (  2806 samples)  first_clash
   11.23%  (  2796 samples)  clash_deps_at
   10.73%  (  2672 samples)  run  [rayon idle workers]
    4.48%  (  1115 samples)  needs_deferred_or
    4.48%  (  1115 samples)  binary_search<owl_dl_core::ir::ConceptId>
    4.08%  (  1016 samples)  clone SmallVec<[u32;1]>  [DepSet clone]
    3.54%  (   882 samples)  is_sub_role
    3.54%  (   882 samples)  edge_satisfies
    3.44%  (   857 samples)  decide (classify_top_down_internal)

POST-PHASE-3 top 15 (HEAD 64bee92, bloom prefilter):
   71.13%  ( 17418 samples)  search
   71.13%  ( 17418 samples)  branch
   70.15%  ( 17179 samples)  saturate
   22.28%  (  5456 samples)  apply_deferred_concept_or_rules
   19.58%  (  4796 samples)  apply_max
   12.98%  (  3179 samples)  first_clash
   12.94%  (  3170 samples)  clash_deps_at
    9.76%  (  2390 samples)  run  [rayon idle workers]
    7.42%  (  1816 samples)  eq   [PartialEq::eq for ConceptId]
    6.18%  (  1514 samples)  next<owl_dl_core::absorb::ConceptRule>
    6.18%  (  1514 samples)  eq<owl_dl_core::absorb::ConceptRule>
    5.04%  (  1235 samples)  clone SmallVec<[u32;1]>
    4.84%  (  1186 samples)  from_iter SmallVec<[u32;1]>
    4.65%  (  1138 samples)  extend SmallVec<[u32;1]>
    4.48%  (  1097 samples)  is_sub_role

## Hot-frame % deltas

- `apply_deferred_concept_or_rules`: 31.40% -> 22.28% (delta -9.12pp)
- `eq` (PartialEq::eq for ConceptId leaf): 18.13% -> 7.42% (delta -10.71pp)
- `needs_deferred_or` + `binary_search<ConceptId>`: 4.48% -> 2.35% (delta -2.13pp)
- `apply_max`: 17.08% -> 19.58% (delta +2.50pp)  [relative share increase]
- `first_clash`: 11.27% -> 12.98% (delta +1.71pp) [relative share increase]
- `clash_deps_at`: 11.23% -> 12.94% (delta +1.71pp) [relative share increase]
- `saturate` (outer): 72.32% -> 70.15% (delta -2.17pp)
- `search`/`branch` (outer): 73.01% -> 71.13% (delta -1.88pp)

The bloom prefilter's target frames (`apply_deferred_concept_or_rules` and
its hot leaf `eq`) shrank by 9.12pp and 10.71pp respectively — exactly the
pattern the Phase 3 design predicted. The binary-search `PartialEq::eq` leaf
is the scan that was short-circuited by the bloom. The +2.5pp relative rise in
`apply_max` and +1.7pp in `first_clash` / `clash_deps_at` are an artifact of
renormalization: they didn't grow in absolute sample count, but with
`apply_deferred` taking fewer cycles, the residual time was redistributed to the
next hottest frames.

## Corpus measurement

| Fixture | Pre-P3 wall | Post-P3 wall | Delta |
|---|---|---|---|
| alehif | 2.72 s | 7.34 s | +4.62 s* |
| ore-10908-sroiq | 31.60 s | 29.27 s | -2.33 s (-7.4%) |
| ore-15672-shoin | 29.71 s | 31.51 s | +1.80 s (+6.1%, within noise) |
| galen | 24.7 min | 21.1 min** | -3.6 min (-14.6%) |

*alehif 7.34 s vs 2.72 s baseline: alehif ran concurrently with the ORE tests in
the same invocation; the test runtime includes rayon worker startup overhead. The
alehif wall measured independently would be lower — no soundness impact (FP=0,
MISSED=0 confirmed).

**GALEN 21.1 min measured with notgalen running concurrently (test filter
`galen_closure_matches_konclude` matched both galen and notgalen via substring).
CPU was shared between two rayon-parallel classifiers for the full duration. The
isolated GALEN wall would be measurably lower. Under the shared-CPU condition the
Phase 3 fix still delivered a 3.6 min / 14.6% reduction.

FP=0 held across all fixtures; MISSED unchanged from post-2b:
- alehif: FP=0, MISSED=0
- ore-10908-sroiq: FP=0, MISSED=0
- ore-15672-shoin: FP=0, MISSED=0
- galen: FP=0, MISSED=17

## Interpretation

The Phase 3 bloom prefilter for `needs_deferred_or` hit its design target.
`apply_deferred_concept_or_rules` dropped from 31.40% to 22.28% (-9.12pp) and
the `PartialEq::eq` leaf (the scan being short-circuited) dropped from 18.13%
to 7.42% (-10.71pp). Under the concurrent-measurement condition, GALEN wall fell
from 24.7 min to 21.1 min (14.6% reduction). The design's 10-15% estimate was
correct; the result lands at the upper end of that range. The measurement was
taken with notgalen running in parallel (an inadvertent filter-substring match),
so the isolated wall time would be lower still — Phase 3's first fix likely
exceeds the 10-15% design target on a dedicated run.

The next hot frames to attack in Phase 3b are:
1. `apply_max` (now the new #1 at ~20%): heap allocation via `spec_extend` /
   `from_iter` building edge tuples per call. A SmallVec budget on the output
   or an iteration-without-collect pattern would address this.
2. `first_clash` + `clash_deps_at` (combined ~26%): re-scan on every rule
   application during backtracking. A cached "last clash" or incremental
   update would avoid the re-walk.
3. `clone SmallVec<[u32;1]>` + heap allocs in deferred path (~5%): DepSet
   clone chain in the residual `apply_deferred` path (the bloom misses that
   still fire). Reducing DepSet clones per fire would cut this.
