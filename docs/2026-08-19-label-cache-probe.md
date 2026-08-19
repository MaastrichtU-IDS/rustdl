# The label-cache escalation probe: a fix, arrived at by refuting three simpler ones

**Date:** 2026-08-19 · `RUSTDL_LABEL_CACHE_PROBE`, **default OFF** · Closes the fix question
opened by `docs/2026-08-19-label-cache-fix-not-warranted.md`.

**Result: `ore_ont_5107` 6.65 s → 1.92 s (3.46×) with the guard case protected, aggregate +1.5%
on the addressable population and −2.3% (≈50 ms) on the fast majority, 0 row differences across
39 ontologies.** Default OFF pending a full-corpus sweep.

## The target, which only appeared after the frame was corrected

`cache_ms = clamp(n × per_pair, 50, 30_000)`, so a **small-`n`** ontology gets a small budget
regardless of what its builds need. The earlier "no fix warranted" measurement used the 40
*slowest* completers — a frame that skews to large `n` and therefore **structurally cannot see
this**. Re-selecting on **low class count × slow wall** found 19 candidates and one real defect:

| ontology | classes | budget | default | generous |
|---|---:|---:|---:|---:|
| `ore_ont_5107` | 49 | 245 ms | 6.69 s | **0.81 s (8.26×)** |

## Three fixes refuted by measurement, in order

**1. Raise `LABEL_CACHE_FLOOR_MS`.** Decisively wrong. On the same 19, granting the ceiling costs
**112% aggregate wall** and takes `ore_ont_9540` from 8.92 s / 40 rows to **200 s / 0 rows** — an
`ok → DNF` with total output loss, the v0.4.8 failure mode. The trade curve shows no value serves
both:

| ontology | current | 500 ms | 1000 ms | 2000 ms | 5000 ms | 30000 ms |
|---|---:|---:|---:|---:|---:|---:|
| `ore_ont_5107` | 6.68 | 6.85 | **0.81** | 0.81 | 0.81 | 0.81 |
| `ore_ont_9540` | 8.91 | 12.18 | 18.69 | 31.72 | 70.73 | 120 / **0 rows** |

`5107` needs ≥1000 ms; `9540` is harmed monotonically by any increase. **This is where the
`n × F` objection stops being theoretical.**

**2. Probe one class: "does a build succeed at the bigger budget?"** Refuted — `9540`'s class 0
succeeds at *both* budgets while 340 others fail at both, so it escalated and cost **2.1×**.

**3. Differential probe over the FIRST 8 classes.** Protected `9540` (1.00×) but **lost the whole
win** (`5107` 1.00×): its 19 failing classes are not among the first 8. Class indices are not
randomly ordered — the early ones are the cheap ones — so a head sample is biased against exactly
what it looks for.

## The mechanism, found in the counters

| ontology | at 245–250 ms | at 1000 ms |
|---|---|---|
| `ore_ont_5107` | pruned=710, **misses=19** | pruned=729, **misses=0** |
| `ore_ont_9540` | pruned=894, **misses=340** | pruned=894, **misses=340** |

`9540` spends the larger budget and converts **nothing**. So the discriminator is not "does a
build succeed" but **"does a larger budget rescue a build that FAILED at the small one."**

## The shipped design

1. Fire only when `cache_ms < 1000` (small-`n` ontologies), no env override, `n > 1`.
2. Scan a **strided** sample of ≤8 classes at the *current* budget for one that returns
   `NoVerdict`.
3. None failing ⇒ no evidence a bigger budget buys anything ⇒ keep the cheap budget.
4. One failing ⇒ retry **that class** at 1000 ms. A verdict ⇒ escalate all; still `NoVerdict` ⇒
   keep the cheap budget (the `9540` shape).

Bad-case cost is the scan plus **one** escalated build — bounded, and **independent of `n`**,
which is the objection that kills a floor. `LABEL_CACHE_PROBE_MS = 1000` is the measured knee of
the win case, so it is also the cheapest probe that captures it.

## Measurements

**Addressable population — 19 slow small-`n` completers:**

| | |
|---|---|
| aggregate | 196.2 s → **193.2 s (+1.5%)** |
| wins ≥1.5× | **1** (`ore_ont_5107` 6.65 → 1.92 s, 3.46×) |
| losses ≤0.8× | **0** (worst `ore_ont_9540` 0.88×, vs **2.1× under naive escalation**) |
| row differences | **0** |

**Fast majority — 20 completers at 0.02–0.5 s** (the risk the slow-biased gate could not see):

| | |
|---|---|
| aggregate | 2.14 s → 2.19 s (**−2.3%**, ≈50 ms total) |
| ≥1.25× slower | **0 of 20** |

Cheap there because their builds succeed at the small budget, so the scan finds no failing class
and the escalated probe is never paid.

## Honest accounting

* **The +1.5% IS the one win.** The other 18 are within run-to-run noise. The defensible claim is
  "converts one 6.65 s ontology to 1.92 s and is neutral elsewhere", not "1.5% faster".
* **The win shrank from 8.26× to 3.46% because the probe's builds are thrown away.** The scan
  constructs up to 8 classes that the `par_iter` then rebuilds. **Caching those results into the
  main loop is the obvious next optimisation** and would recover most of the overhead; not done
  here because it restructures a hot loop.
* **Default OFF.** It changes the budget on every small-`n` ontology, and this repo's record has a
  12-ontology benchmark hiding four `ok → DNF` regressions. A flip needs the full-corpus two-arm
  sweep.
* **A single-class probe is a heuristic.** If a strided sample's failing class is unrepresentative
  in the *other* direction, an ontology could escalate and pay. None of the 39 measured did, but
  the guard is empirical, not structural.

## Method note

Four designs, three refuted, and each refutation came from measuring the thing rather than
reasoning about it: the floor by the trade curve, the naive probe by the guard case, the
head-scan by the win case. The frame error is the one worth carrying — **a population selected on
"slowest" cannot see a defect whose precondition is "small"**, and I made that mistake twice in
one day before catching it.

Raw data: `docs/benchmarks/data-2026-08-19-label-cache-probe-19.tsv`,
`…-probe-fast20.tsv`, `…-label-cache-default-reachability.tsv`
