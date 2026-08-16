# `--global-timeout-ms` does not bound the wall, and the overshoot scales with class count

**Date:** 2026-08-16 · Found while scanning whether an internal global deadline should be a
default. **Defect, not yet fixed.**

## The measurement

Same binary, `--pair-timeout-ms 5`, `--global-timeout-ms 20000`, 1 thread:

| ontology | classes | wall | overshoot |
|---|---|---|---|
| `ore_ont_7192` | 71,033 | **58.1 s** | **2.9×** |
| `ore_ont_2574` | 81,270 | 35.3 s | 1.7× |
| `ore_ont_11311` | 8,022 | 20.8 s | 1.0× |
| `ore_ont_9944` | 8,008 | 20.2 s | 1.0× |
| `ore_ont_12567` | 232,084 | 17.7 s | 0.8× (finished naturally) |

**The overshoot appears only when the deadline FIRES**, and then grows with class count:
~1 s at 8k classes, 15–38 s at 71–81k. `ore_ont_12567` has 232k classes and no overshoot,
because it completes before the deadline — so this is not a function of size alone, it is
post-deadline work.

## Where the time goes

`ore_ont_2574` at a 30 s deadline, wall 46.6 s:

| segment | ms |
|---|---|
| parse + convert (before any phase timer) | 4,063 |
| measured phases, summed | 30,072 — exactly the deadline |
| **unaccounted, after the phases** | **~12,500** |

Ruled out by measurement:

* **Not output writing** — redirecting to `/dev/null` gives 49.7 s, no better.
* **Not the Hasse reduction** — `RUSTDL_FAST_DIRECT_SUBSUMERS=1` vs `=0` is 46.6 s vs
  46.3 s.

The remaining candidates are the per-class structures built after the reasoning phases for
an 81,270-class ontology (class vector, index map, entailment matrix, and teardown of the
`PreparedOntology`), none of which is inside the deadline's accounting. **Not yet localised
further** — the phase instrumentation stops before this region, which is precisely why it
was invisible.

## Why it matters

1. **It is a user-facing contract violation.** `--global-timeout-ms 20000` producing a 58 s
   run is surprising, and there is no flag to bound the difference.
2. **It manufactured 2 of the 3 "regressions"** in the global-deadline scan. `ore_ont_2574`
   and `ore_ont_7192` completed *without* a deadline in ~55 s, and were killed *with* a 55 s
   deadline inside a 60 s cap — the deadline fires, then the unbounded tail exceeds the
   remaining 5 s. `ore_ont_2574` at a 30 s deadline completes at 48.9 s with all 57,851
   rows, confirming the mechanism: the answer was ready, the tail spent the budget.
   (The third, `ore_ont_10689`, was cap-boundary noise — 3/3 repeats complete at 57.2–59.2 s.)
3. **It contaminates any deadline-default decision.** The scan's headline — 113 ontologies
   going from no output to a hierarchy, 3 regressions, +8.3% wall — cannot be read cleanly
   while the deadline's own cost is unbounded and n-proportional.

## What the scan showed anyway

Arm A = v0.4.19 defaults, no internal deadline. Arm B = the same plus
`--global-timeout-ms 55000`, inside a 60 s cap, 1,920 ontologies:

| | |
|---|---|
| produced nothing before → a hierarchy now | **113** |
| `ok → dnf` | 3 (**2 real**, both caused by the defect above) |
| complete in both arms | 1,774 |
| wall over both-arm completers | 3,012 s → 3,260 s (+8.3%) |

113 recoveries is a large effect and the direction is right. But the +8.3% on ontologies
that never needed the deadline is unexplained, and plausibly the same accounting hole.

## Recommendation

**Do not propose a non-zero default until the tail is bounded.** Two orderings are
defensible:

1. **Fix first.** Bring the post-phase region inside the deadline (or subtract a
   class-count-proportional reserve), then re-run the scan. The 2 regressions should
   disappear, and the +8.3% becomes interpretable.
2. **Ship the capability, not the default.** The harness and any batch caller can set an
   internal deadline today and get the 113; the CLI default stays 0, because rustdl cannot
   know the caller's patience.

Either way the measurement to repeat afterwards is the same, and it must include the
repeat-runs for cap-boundary cases — one of the three regressions here was noise, and only
3× repeats distinguished it.
