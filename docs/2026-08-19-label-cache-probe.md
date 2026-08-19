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
| wins ≥1.5× | **1** (`ore_ont_5107` 6.65 → **1.14 s, 5.82×** with reuse; 1.92 s / 3.46× without) |
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
* **Probe-result REUSE is now implemented** (the optimisation this section originally listed as
  future work). The scan's builds were being discarded and redone by the `par_iter`. Reusing them
  takes `ore_ont_5107` from **1.92 s to 1.14 s — 3.46× → 5.82×**.
  * **Sound because a VERDICT is budget-independent**: `Sat`/`Unsat` is a completed wedge
    computation, so raising the deadline cannot change it. A `NoVerdict` means only "did not
    finish" and is deliberately **not** carried over; the escalation arm re-runs exactly that
    class. Observably checked by **0 row differences** across all 39 ontologies — a stale verdict
    would change the hierarchy.
  * **It does NOT improve the aggregate, and I am not claiming it does.** 19-population goes
    +1.5% → +1.3%, which is *indistinguishable*: the win saves 0.78 s out of 196 s (0.4%), below
    run-to-run variance. Fast 20 goes −2.3% → −0.5%, likewise noise. **The reuse improves one
    ontology and is invisible in aggregate.**
  * Residual gap to the 8.26× unconditional ceiling (~0.33 s) is the escalated probe build plus
    the non-reusable scan class. That is **intrinsic to probing** — you cannot learn that a bigger
    budget helps without spending one — not a further optimisation.
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

---

## The flip sweep PASSED — and the flip is BLOCKED anyway, by a codegen effect

**Date:** 2026-08-19 (later). The two-arm sweep cleared the probe behaviourally. Flipping the
default nonetheless makes `ore_ont_5107` **2× slower on a code path whose runtime behaviour is
unchanged**, so the flag stays OFF.

### The sweep itself: clean, 830 ontologies

The probe fires only when `cache_ms = clamp(n × per_pair, 50, 30000) < 1000`. At the default 5 ms
per-pair that is `n < 200`; under `--pair-timeout-ms 1` it widens to `n < 1000`. **Both scopes were
swept** — the second exists because the first frame cannot see it, the same frame error that
already cost this investigation twice.

| frame | n | ok→DNF | hash diffs | effect |
|---|---:|---:|---:|---|
| default config, <200 classes | 509 | **0** | **0** | 502 identical, 7 both-DNF, **net +3.71 s** |
| `--pair-timeout-ms 1`, 200–1000 classes | 321 | **0** | **0** | 308 identical, 13 both-DNF, **flat** |
| ≥200 classes at default | ~1,100 | — | — | structurally inert; byte-identical on 3 up to 981k classes |

Quantisation matters in reading the first frame: 424 of 502 have both arms **under 0.10 s**, where
a 10 ms timer cannot resolve them — the 26 apparent "2.00× wins" were all 0.02 s → 0.01 s, one
tick. Among the **78 resolvable**: **1 win** (`ore_ont_5107` 8.47 → 1.13 s, 7.50×), **0 losses**,
total gain 8.50 s against total cost 4.79 s.

### Why the flip is blocked

Flipping `label_cache_probe_enabled()` to default-ON costs **2×** — and it does so **with the
probe still disabled**, which means it is not a semantic effect:

| binary (source differs by ONE predicate line) | `=0` | `=1` | unset |
|---|---:|---:|---:|
| `a296cdf` (default OFF) | **6.65 s** | **1.13 s** | 6.67 s |
| + `is_none_or(\|v\| v != "0")` | **12.90 s** | 14.54 s | 14.55 s |
| + `!is_some_and(\|v\| v == "0")` | **12.91 s** | 14.54 s | 14.55 s |

At `=0` **all three return `false`**, so the executed path is identical — yet the wall doubles.
And the probe's effect *inverts*: on `a296cdf` enabling it is 5.9× faster; on the flipped builds
enabling it is *slower*.

Ruled out, each by measurement:

* **Host drift / contention** — interleaved A/B at the same moment, load 2.3 on 32 cores, and the
  pre-probe pinned binary reproduces its historical 6.65 s throughout.
* **Build nondeterminism** — rebuilt twice, **byte-identical** md5 each time.
* **A stale artifact** — the first rebuild compiled only `owl-dl-cli`; forcing `owl-dl-reasoner`
  to recompile changed nothing. (This is the documented "silently reuses a stale binary" hazard,
  and it did mislead one intermediate measurement.)
* **File corruption** — a `sed` I ran used `|` as delimiter on a pattern containing `|v|`; checked,
  and the constants and guard are intact, `git diff` covers only the two intended files.
* **A second call site** — grep confirms one engine call site (`classify.rs:3402`) and one test.
* **Flag semantics** — both formulations are correct at `=0`/`=1`, and both are slow.

What remains is **codegen**: the predicate is called from the label-cache block of
`classify_internal`, a very large function, and perturbing it evidently shifts an inlining or
layout decision. This codebase has documented env-flag hot-loop sensitivity of exactly this shape
(`docs/benchmarks/2026-08-11-env-flag-hot-loop-fix.md`). **I could not localise it further, and I
am not asserting the mechanism.**

### Disposition

**Flag stays OFF.** The probe is verified as an opt-in (`=1`: `ore_ont_5107` 6.65 → 1.13 s) and the
sweep says it is answer-preserving at scale, but the *act of making it the default* triggers a 2×
regression through a mechanism I cannot explain. Shipping that would trade a one-ontology win for a
corpus-wide unknown.

**What a future attempt should do first:** reproduce the 2× on a smaller unit than
`classify_internal` (or diff the generated assembly for the label-cache block between the two
builds). Until the codegen effect is understood, no formulation of the flip is safe — two
independent ones already failed identically.

Raw data: `docs/benchmarks/data-2026-08-19-probe-sweep-default-509.tsv`,
`…-probe-sweep-pt1-321.tsv`
