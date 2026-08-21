# 60% of the non-label-cache tail is genuinely STALLED, not slow (2026-08-21)

**The 20 s phase census cannot distinguish "stalled" from "large, needs 30 s."** This tests the
distinction directly: re-run the 43 non-`label_cache_build` tail members at a **300 s global
budget** — 15× the census — and compare **rows against rows**.

Data: `data-2026-08-21-convergence-43-at-300s.tsv`.

## Method notes that were load-bearing

Two instrument errors were caught and fixed before any interpretation. Both would have produced
confident nonsense.

1. **The row counter must be calibrated against a known value.** `classify --json` rows live in
   `direct_subsumptions` as 2-element arrays; a `grep -c '"sub"'` counts a key that does not exist
   and returns 0 for *every* ontology. Validated against pizza = 181 before use.
2. **The budget is what CAUSES partial emission.** The first arm ran with no budget and read
   `ore_ont_11553` as "19,240 rows at 20 s → nothing at 300 s", which looks catastrophic and is an
   apples-to-oranges comparison: without a budget rustdl runs to completion or emits nothing. Re-run
   under `--global-timeout-ms`, and calibrated first at the census's OWN 20 s budget, where it
   reproduces **19,239 vs 19,240** (documented budget-induced nondeterminism).
3. **`incomplete` is NOT a completion test.** Pizza itself reports `incomplete: true`. And the
   census's `NEAR_COMPLETE` label rested on `incomplete_pairs=1`, a reading this record had already
   retracted (that counter counts pairs *attempted and cut*, not *remaining*). Rows-vs-rows is the
   only discriminator.

## Result

| class | count | share |
|---|---:|---:|
| **STALLED** (rows within 10%) | **26** | **60%** |
| CONVERGED (`incomplete: false`) | 11 | 26% |
| PROGRESSING (>10% more rows) | 3 | 7% |
| NO_OUTPUT | 3 | 7% |

**This overturned the hypothesis it was built to test.** These members were expected to be
large-and-slow — they emit 19k–270k rows and carry 12k–152k classes. They are not: 15× the budget
buys `ratio ≈ 1.00`. `ore_ont_345` is 93,138 → 93,138; `7507` 80,943 → 80,943; `7581`
120,202 → 120,202. `ore_ont_15074` stalls at **1,155 rows**, so this is not about size at all.

**Budget is therefore not a lever for 60% of this frame**, which independently corroborates the
earlier finding that the DNF tail moved by only 2 ontologies across a 1000× timeout range.

## The confound, and why the conclusion survives it

`target/release/rustdl` was **rebuilt mid-run** with the told-table fix, so members processed after
the rebuild ran on a faster binary. That violates the pin-binaries rule and is recorded rather than
hidden.

It does not threaten the result, because **the confound is directional**: a faster binary can only
produce *more* rows or reach COMPLETE. Every STALLED verdict is therefore conservative — understated
if anything. Symmetrically, `ore_ont_9674` CONVERGED at **32.6 s**, exactly the post-fix wall from
the told A/B, so that conversion is a genuine recovery *caused by* the fix, not a measurement
artifact.

## Lead not chased: a NON-MONOTONIC row count

`ore_ont_8475` reports **4,351,944 rows at 20 s and 262,344 at 300 s** — a 16× *drop* from a 15×
*larger* budget. More budget yielding fewer answers is non-monotonic, and in this codebase
non-monotonicity has previously indicated a real defect (cf. the `RUSTDL_DKEY_EMIT_ORDER` case).
Two candidate explanations, untested: the larger budget routes to a different, more
completeness-seeking path that then truncates elsewhere; or one of the two figures is a
mis-attributed census row. **Worth one focused look; do not assume either reading.**

## What this says about where to work

Combined with the same day's other results, the ~140 tail now partitions by *mechanism* rather than
by phase:

* **Deadline-bound (~91)** — `tier_walk` 13 + `label_cache_build` 78. `wall = #units × deadline`,
  so engine speed is absorbed. Immune to profiling-driven optimisation.
* **Genuinely stalled (~26 of this frame)** — more wall buys nothing. Needs a calculus or pruning
  change, not performance work.
* **Conversion-bound (~7 distinct)** — the one class where optimisation converts 1:1 into wall,
  because no budget bounds conversion. This is where the told-table quadratic lived, and it paid
  6.2× conversion / 1.87× classify.
* **Genuinely scale (11 CONVERGED)** — completes given real wall; not a defect.
