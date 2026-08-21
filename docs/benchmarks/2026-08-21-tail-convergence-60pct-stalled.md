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

---

## CORRECTION: the row-count discriminator was CONFOUNDED — repaired, and the answer survives

The partition above used `direct_subsumptions` row counts. **That is the direct (Hasse) relation,
and it gets RESTRUCTURED rather than merely extended as reasoning progresses** — so it is not a
monotone progress measure:

* **Proving a class unsatisfiable REMOVES rows.** Unsat rows are deliberately elided
  (`classify.rs`: *"Row elided — `Classification::entails` supplies ⊥ ⊑ *"*).
* **Proving classes equivalent REMOVES rows.** They collapse into an `equivalent_groups` entry.

`ore_ont_8475` is the demonstration, on ONE binary with ONE instrument:

| budget | rows | unsat |
|---|---:|---:|
| 20,000 ms | 4,164,366 | 0 |
| 300,000 ms | 262,344 | **1** |

A 16× *drop* in rows, because the longer run **proves one class unsatisfiable** and correctly elides
~3.9 M rows. The 300 s answer is *more* correct with *fewer* rows. So the "non-monotonic row count"
flagged above as a possible defect is **not a defect** — and `ratio < 1` can mean **progress**.

This is the **direct-vs-closure trap**, which this project's own memory records hitting three times
before (`ore_ont_13859`, Konclude's hierarchy, `ore_ont_8388`). Fourth instance. The standing rule —
*always close before comparing* — applies to progress measurement too, not only to oracle
comparison.

### Repaired measurement

Re-ran the 20 s arm with the full instrument capturing **rows + unsat + equiv-group membership**,
same pinned binary, and compared all three against the 300 s side
(`data-2026-08-21-convergence-repaired-3quantity.tsv`):

| class | count |
|---|---:|
| output **IDENTICAL** at 20 s and 300 s (truly stalled) | **26** |
| output CHANGED | 14 |

**The headline count is unchanged at 26, but the MEMBERSHIP was substantially wrong.** Ten members
the row-only analysis called STALLED are actually progressing (`2874` unsat 4731→4749, `12128`
4202→4211, `8475`, `7646` unsat 0→158, `3215`, `9663`, `13242`, `15695`, `10621`, `11270`), while
two it excluded are genuinely stalled (`3794`, `7729` — their large equivalence groups were
*already* found at 20 s). **The right count for the wrong reasons is still wrong**; only the
repaired run establishes it.

Further honesty on the CHANGED 14: several changed by a handful of rows (`10621` 42,205→42,211;
`5857` 105,667→105,665), which is the documented budget-induced nondeterminism, not progress. The
genuinely progressing set is about **7**: `10926` (53k→219k), `11085` (1.68 M→8.05 M), `1194`
(27k→105k), `7646` (203k→442k), `8475` (unsat discovery), `3215`, and the two small unsat gains.

### What survives

**26 of 40 measurable members (65%) produce BYTE-IDENTICAL output across a 15× budget increase.**
That is a stronger claim than the original, because it rests on three quantities rather than one,
and it confirms the conclusion that budget is not a lever for the majority of this frame. The
mechanism partition at the end of this document stands.
