# Wedge node-label bitset — P0 profiling study results

**Date:** 2026-06-22. Spec: `docs/superpowers/specs/2026-06-22-wedge-label-bitset-design.md`.
Plan: `docs/superpowers/plans/2026-06-22-wedge-label-bitset.md` (Task 1).

## Verdict: **ABORT** the bitset (and hybrid). Pursue a linear-scan `.has` instead.

The P0 gate exists to decide pure-bitset / adaptive-hybrid / abort *before* sinking the
FP-critical migration. The measurement says abort, decisively.

## Method

Throwaway `RUSTDL_LABELSTATS=1` instrumentation (reverted, not shipped) printed one line
per wedge `decide_with_deadline` call: universe width `W` (max `ClassId.index()+1` over
nodes incl. synthetics), node count `N`, total labels, max labels/node, and
`SearchStats.node_clones`. Aggregated per ontology over a `classify --pair-timeout-ms 1000`
run (slow onts capped at 150s, partial stats still valid). `r*` = ORE-pilot canon.owx.

## Data

| ont | maxW | wedge calls | avg labels/node | max labels | total node-clones | bitset clone (W/8 B) | vec clone (4·avg B) | regime |
|---|---|---|---|---|---|---|---|---|
| galen | — | 0 | — | — | — | — | — | EL fast-path (saturator; no wedge) |
| ore10908 | 717 | 694 | 4.3 | 22 | 5 491 | 90 | 17 | vec cheaper + branchy |
| ore15516 | 304 | 314 | 5.8 | 14 | 24 493 | 38 | 23 | vec cheaper + branchy |
| ore15672 | 149 | 106 | 3.0 | 7 | 312 | 19 | 12 | vec cheaper, few clones |
| alehif | 1623 | 167 | 3.3 | 12 | 0 | 203 | 13 | no branching (bitset ok) |
| sio | 1660 | 1850 | 5.3 | 15 | 3 735 | 208 | 21 | vec cheaper + branchy |
| pizza | 116 | 315 | 4.6 | 13 | 96 936 | 14 | 19 | bitset cheaper clone |
| r10080 | 4743 | 3533 | 8.5 | 41 | 600 179 | 593 | 34 | vec cheaper + branchy |
| r7499 | 5730 | 5394 | 5.2 | 53 | 151 702 | 716 | 21 | vec cheaper + branchy |
| r699 | 11246 | 6463 | 8.7 | 80 | 11 888 | 1406 | 35 | vec cheaper + branchy |
| r12698 | 18177 | 17542 | 6.7 | 75 | 5 432 | 2272 | 27 | vec cheaper + branchy |

## Why ABORT

1. **Nodes are sparse-wide.** Avg labels/node is **3–9** regardless of ontology, while W
   is **300–18 000**. The sorted `Vec<ClassId>` (≈4·avg = 12–36 B/node) is already a
   near-optimal sparse representation; a dense bitset (W/8 = 38–2272 B/node) is **2–65×
   larger** per node.
2. **The wedge clones whole nodes per branch, frequently** (r10080: 600 k clones;
   ore15516: 24 k; sio: 3.7 k; pizza: 97 k). The bitset would inflate every one of those
   clones by the 2–65× factor.
3. **Every perf-pain ontology is in the bitset-hostile regime** (large W, sparse labels,
   many clones). The only bitset-favorable cases are pizza (tiny W=116) and the
   zero-clone alehif — neither is a workload we need to speed up. So a HYBRID selector
   would route all the onts that matter to the Vec → no win.

The `.has` O(1) win the bitset was chasing (the ~11% `select_unpredictable` /
`binary_search` self-time) is far smaller than the clone-bloat loss it would incur.

## The cheaper lever the data surfaced (follow-up)

`select_unpredictable` is the **branch-misprediction select inside `binary_search_by_key`**.
With only ~5 labels/node, **a linear scan is branch-predictable and likely faster than
binary search**, with **zero change to clone cost** (the `Vec` stays). This captures the
membership win without any bloat. Candidate: `has()` → linear `iter().any()` for small
label sets (or unconditionally, given the measured tiny `avg`/`maxl`). To be A/B-tested
with the same harness (galen EL control, closure-md5, high-N, ±2% floor). The bitset spec
+ plan are retained as the design record for why the representation change was rejected.
