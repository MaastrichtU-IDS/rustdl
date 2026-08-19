# The label-cache starvation fix is NOT warranted — the default does not starve, measured

**Date:** 2026-08-19 · Closes the question opened by
`docs/2026-08-19-label-cache-starvation-census.md`: should the per-class label-cache budget be
given a floor (with bounded abandonment to answer the `n × F` objection)?

**No. On the 40 slowest completers, granting every class the maximum budget helps ZERO ontologies
at the ≥1.5× threshold and costs 2.3% aggregate wall. There is nothing to fix at the default.**

## The decisive experiment

For each of the 40 slowest v0.4.19 completers, at the **default** `--pair-timeout-ms`:
default cache budget vs `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000` (the ceiling — the most generous
budget any floor could grant). Single-threaded, 200 s cap, pinned binary.

If the default starved the cache, the generous arm would be markedly faster. It is not.

| bucket | n |
|---|---:|
| ≥1.5× faster with a generous budget (**the threshold**) | **0** |
| 1.2–1.5× | 1 (`ore_ont_13071`, 42.7 → 34.5 s) |
| 1.05–1.2× | 2 (`9899` 1.12×, `15059` 1.10×) |
| <1.05× (no benefit / noise) | 37 |
| **aggregate wall** | default **1403.3 s** → generous **1436.0 s** (**−2.3%**, i.e. WORSE) |

## The `n × F` objection is now MEASURED, not theoretical

The limitation's *why not fixed* argued this on paper: *"the budget is per class, so a floor `F`
costs up to `n × F` on any ontology where the label cache genuinely cannot succeed. At n = 1,000
classes a 2 s floor is…"*. It is now a number: **granting 30 s per class costs 2.3% aggregate wall
across the 40 slowest completers, with no compensating win anywhere.** A floor is not free, and on
this frame it is net negative.

That **vindicates the original decision not to fix** — though on different grounds than it argued.
The original reasoned from the `--pair-timeout-ms 1` regime; the binding question is the
**default**, and the default is already at or near the right budget.

## What remains real, and what it is not

* **The cache is load-bearing.** Forcing the 50 ms floor takes `ore_ont_15108` 43.1 s → **DNF at
  240 s** and degrades 12 of 40. So the budget matters — the default simply already grants enough.
* **The `pt`-sensitivity defect is real and unexplained.** `14272`, `9864`, `6923`, `4827`, `8429`
  run 2.7–3.4× slower at `--pair-timeout-ms 1` for byte-identical output, **at every cache budget
  including the default's own 30 s**. Not starvation. Cause **unknown**; the untested candidate is
  the tier walk losing prunable verdicts and probing far more pairs. **Only reachable under a
  user-chosen extreme budget**, so it is a genuine but low-priority finding.

## Honest residual: the untested population

This frame is **the 40 slowest completers**, which skews toward large `n`. The default budget is
`clamp(n × per_pair, 50, 30000)` with a 5 ms default per-pair, so:

| n | default budget |
|---:|---:|
| ≥ 6,000 | 30,000 ms (ceiling) |
| 100 | 500 ms |
| 10 | **50 ms (floored)** |

**A small-`n` ontology with an expensive label build would therefore get ~50 ms and be starved —
and this frame cannot see it**, because such an ontology is unlikely to be among the 40 slowest.
Whether that population is non-empty is **untested**. That is the probe to run if anyone revisits
this: select on *low class count* and *slow wall*, not on wall alone.

I am not claiming the class is empty. I am claiming it is empty **on the frame that the
limitation's own census defined**, and that the fix has no target there.

## Method note

I was one step from implementing the floor-plus-bounded-abandonment design when the 2×2
(`§ THE ATTRIBUTION WAS WRONG` in the census doc) showed my five target ontologies were
cache-insensitive. Had I built it, I would have shipped a change that helps 1 ontology by 1.24×,
costs 2.3% aggregate, and does nothing for any of the five it was aimed at.

**The addressability pre-check belongs before the design, not after it** — a rule this repo
already records, arriving late here but before the code.

Raw data: `docs/benchmarks/data-2026-08-19-label-cache-default-reachability.tsv`
