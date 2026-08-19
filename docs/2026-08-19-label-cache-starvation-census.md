# The 40-slowest-completer starvation census: 5 live members, and the trade-off has inverted

**Date:** 2026-08-19 · Re-runs the census in
`docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md`, to settle whether
that class was empty after both its named instances stopped reproducing.

**It is not empty. It has 5 members, up from "~2 known", and the aggregate wall trade-off that
justified leaving it unfixed has INVERTED. My own 2026-08-18 "CLOSED" was wrong.**

## Design

Frame: the **40 slowest v0.4.19 completers** (`wall > 0.5 s`), from
`docs/benchmarks/data-2026-08-15-v0419-two-arm-sweep.csv`. Single-threaded, 240 s cap, binary
pinned and verified against a discriminating input. Three arms:

| arm | configuration | purpose |
|---|---|---|
| **A** | default | baseline |
| **B** | `--pair-timeout-ms 1` | the census — the allegedly starving regime |
| **C** | `pt=1` + `RUSTDL_LABEL_CACHE_TIMEOUT_MS=50` (the floor) | **positive control** — forced starvation |

**Arm C exists because a null in arm B would otherwise have been uninterpretable.** The only known
positive instance (`ore_ont_15108`) had already measured negative, so "0 members" and "blind
instrument" would have looked identical. Thresholds (1.5×) and the verdict rule were
**pre-registered in a script before the data landed**, so they could not be tuned to the outcome.

## Result

| | recorded 2026-08-06 | this census |
|---|---:|---:|
| `pt=1` ≥1.5× **SLOWER** | **1** | **5** |
| `pt=1` ≥1.5× faster | 12 | 3 (all clean) |
| within 1.5× | 27 | 32 |
| DNF in an arm | — | 0 |
| aggregate wall | 1499.5 → **1267.0 s** (net *faster*) | 1377.3 → **1624.8 s** (net **+18% slower**) |

### The 5 live members — every one byte-identical

| ontology | default | `pt=1` | arm B | arm C | rows |
|---|---:|---:|---:|---:|---|
| `ore_ont_14272` | 21.76 s | 73.26 s | **3.37×** | 3.37× | identical (835) |
| `ore_ont_4827` | 37.14 s | 123.26 s | **3.32×** | 3.32× | identical (1006) |
| `ore_ont_9864` | 24.53 s | 79.53 s | **3.24×** | 3.24× | identical (904) |
| `ore_ont_8429` | 29.50 s | 90.50 s | **3.07×** | 3.07× | identical (1001) |
| `ore_ont_6923` | 38.36 s | 102.93 s | **2.68×** | 2.69× | identical (1038) |

**Arm B ≡ arm C to within 0.01× on all five.** That is mechanism, not correlation: for these
ontologies `--pair-timeout-ms 1` starves the per-class build *exactly as completely* as pinning
the budget to the 50 ms floor. Same wall, same output, same ratio as the forced case.

### The instrument fires, so the 5 is not a blind count

Arm C is ≥1.5× slower on **12 of 40**, five to outright DNF at 240 s: `15491` 8.96×, `9151`
7.43×, `9299` 6.81×, `5617` 7.20×, `15066` 6.74×. The label cache is emphatically load-bearing on
this frame; arm B's 5 therefore means something.

### Why the two ORIGINAL instances dropped out — and why that is not a fix

Seven ontologies are **arm-C-only**: the cache still matters, but a small per-pair budget no
longer starves it (`15108` armB 1.01× / armC 2.10×; `5617` 0.97/7.20; `15066` 0.92/6.74; `9151`
1.03/7.43; `9299` 1.05/6.81; `13071` 1.09/1.51; `15491` 1.06/8.96). `ore_ont_15010` simply left
the 40-slowest frame (5.98 s).

So the coupling has broken on **7** and holds on **5**. **Membership moved; nothing was fixed.**

## What this changes

The limitation's "why not fixed" rests on *"the pathology is ~2 ontologies against 12 large wins
in the same sample"*, with the aggregate wall improving under `pt=1`. **Both halves are now
false** — 5 against 3, aggregate 18% worse. The section needs re-deciding on current numbers.

That does **not** license simply raising `LABEL_CACHE_FLOOR_MS`: its per-class `n × F` objection
still stands (at n = 1,000 classes a 2 s floor costs up to 2,000 s where the cache cannot
succeed). What is gone is the empirical case for *inaction*.

## A prediction of mine, refuted

I expected the original's "12 large wins" to be partly **truncation** — `--pair-timeout-ms 1` is
a documented sound under-approximation, so a faster arm might merely have given up. **Wrong.**
All 3 faster members have **identical row counts** (`9429` 2706, `934` 107, `4796` 203). Only 2
ontologies change output under `pt=1` at all, and both are flat in wall (`15066` 8986 → 8952,
`9151` 11478 → 11477). **Incompleteness and the wall pathology are disjoint here**, not two faces
of one thing.

## Threats to validity

* The frame comes from the **v0.4.19** sweep, not the 2026-08-06 one, so the two censuses are
  **not paired** — the comparison is of rates and aggregates.
* All 40 files are content-distinct, but `ore_ont_10689` and `ore_ont_868` both return exactly
  981,144 rows, suggesting **logical** duplication no content hash detects; effective *n* may be
  just under 40.
* Arm-C DNFs are censored at 240 s, so those five ratios are **lower bounds**.
* Frame is by construction the slowest completers, so these rates say nothing about the fast
  majority (corpus median ~50 ms), where a small budget can only cost completeness.

## Method note

**Retiring a document's named examples is not retiring its defect.** On 2026-08-18 I measured
both named instances as non-reproducing — correctly, with controls — and wrote CLOSED. The class
was meanwhile larger than recorded. The scope caveat I attached ("what is retired is the
document's evidence, not a proof of absence") was right, and the headline contradicted it. When a
document states a prevalence, re-run the census that produced it before touching the status line.

Raw data: `docs/benchmarks/data-2026-08-19-label-cache-starvation-census40.tsv`
