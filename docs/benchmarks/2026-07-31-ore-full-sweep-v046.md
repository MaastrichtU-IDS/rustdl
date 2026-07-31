# ORE full sweep — rustdl v0.4.6, single-thread, 30 s cap

**Date:** 2026-07-31
**Raw data:** `owl-reasoner-harness` repo,
`baselines/2026-07-31-ore-rustdl-v046-t1-c30.jsonl` (one JSONL record per ontology,
plus a provenance header). Not stored here — rustdl holds the interpretation, the
harness holds the measurement.
**Provenance:** rustdl 0.4.6, binary sha256 `fd8ad6573505…`, `RAYON_NUM_THREADS=1`,
30 s cap, `--forbid-marker XCLONEX` verified before the run (i.e. proven to be a clean
build, not an instrumented one).
**Corpus:** `/data/dumontier/ore-run/pool_sample/files`, 1,920 `.owl` files. Note this
is the locally provisioned pool, not ORE 2015 entire; `pilot/` (234) and `work/` (38)
are separate.

> **CORRECTION 2026-07-31 — "DNF" HERE MEANS "EXCEEDED THE 30 s CAP", NOT "DOES NOT TERMINATE".**
> Verified on three of the ontologies this document lists as DNF candidates: `ore_ont_16632`
> **33.45 s exit=0**, `ore_ont_11126` **32.63 s exit=0**, `ore_ont_10425` **49.58 s exit=0** — all
> complete, just past the cap. An independent 24-ontology sample of the **under-1 GB** DNF group at
> a 120 s cap saw **5 complete (~21%)**, implying roughly **52 of the 312 are cap artifacts**
> rather than genuine non-termination.
>
> So read every "DNF" below as **cap-exceeded**. The 312 figure is still the right answer to *"how
> many miss a 30 s budget"* — which is the production question this document set out to answer —
> but it is **not** a count of ontologies rustdl cannot classify. Two things follow: a plan built on
> the latter reading was written and rejected the same day
> (`docs/superpowers/specs/2026-07-31-data-cardinality-counting-design.md`), and the residual
> "unfinished" column in the derived-threshold table below is an over-estimate by roughly that ~52.
>
> The fix for the harness is to separate *cap-exceeded* from *unfinished-at-any-budget*, which needs
> a second pass at a larger cap over the DNF set — cheap, and not yet run.

## Headline

| outcome | count | share |
|---|---|---|
| completed | **1,607** | 83.7% |
| did-not-finish at 30 s | **312** | 16.2% |
| front-end rejection | **1** | 0.1% |

**The DNF population is 312, not 12.** The 12-ontology roster in circulation came from
re-running a 2026-06-08 pilot *lineage* rather than a fresh sweep, and was stale-low by
26×. The "289 ORE DNF ontologies" figure quoted in the CB park record was essentially
right. Any planning that used the small number should be revisited.

Note also that `err_reject` is reported separately and must stay that way: a front-end
rejection is a converter gap, not a reasoning limit, and merging the two is what makes a
DNF roster unactionable. Only 1 remains here, which is worth knowing on its own — the
~23% anonymous-individuals rejection recorded earlier in the corpus history is gone.

## Answer to "what is a pragmatic production timeout?"

Because actual wall was recorded rather than a threshold being baked in, the would-miss
count at any cap ≤30 s is derivable from the single pass:

| cap | would-miss | of which slow-but-finishing | of which unfinished |
|---|---|---|---|
| 1 s | **828** (43.1%) | 516 | 312 |
| 5 s | **419** (21.8%) | 107 | 312 |
| 10 s | **365** (19.0%) | 53 | 312 |
| 30 s | **312** (16.2%) | 0 | 312 |

At an interactive 1 s budget rustdl misses 43% of the corpus; 5 s halves that; past 10 s
the curve flattens because the residual is genuinely unfinished rather than merely slow.
So the useful production settings are ~5 s (interactive, accepts ~22% miss) or ~30 s
(batch, ~16%) — anything above 30 s buys almost nothing without an architectural change.

## RSS — the axis that matters, and the actionable list

Single-thread peak RSS. **Multiply by roughly the core count for the parallel case**: one
ontology previously measured 42 MB at 1 thread versus 1.47 GB across cores, so these are
floors for a real deployment, not ceilings.

| band | ontologies |
|---|---|
| > 1 GB | **92** |
| > 4 GB | **17** |
| > 16 GB | 0 |

14 of the top 15 by RSS are DNF, so memory and non-termination coincide.

### Unfinished with a SMALL input — the tractable candidates

**NOTE (2026-07-31):** three of the rows below (`16632`, `11126`, `10425`) were subsequently
measured to **complete** in 33–50 s, and their large disjointness axiom sets were shown to be
**inert** — `10425`'s classify output is byte-identical with the data channel disabled. So a large
RSS-to-input ratio is a real *memory* signal but NOT evidence of non-termination, and not by itself
evidence that the memory is load-bearing for the answer. Check both before acting on a row.

A large RSS-to-input ratio implies a **local** cause (something quadratic in a
conversion-time construct) rather than a search blowup. This is exactly the signature
`ore_ont_9347` had — 8.6 MB input → 70.7 GB peak — and it was fixed in ~8 lines by the
DKey non-merging-component gate this session.

| ontology | input | peak RSS (1 thread) | ratio |
|---|---|---|---|
| **`ore_ont_11085`** | 5.4 MB | **9.25 GB** | ~1,750× |
| `ore_ont_1833` | 7.3 MB | 7.94 GB | ~1,110× |
| `ore_ont_16632` | 3.6 MB | 7.57 GB | ~2,150× |
| `ore_ont_11126` | 3.3 MB | 7.34 GB | ~2,280× |
| `ore_ont_15655` | 2.4 MB | 6.60 GB | ~2,820× |
| `ore_ont_10425` | 2.0 MB | 5.90 GB | ~3,020× |
| `ore_ont_5368` | 3.9 MB | 2.89 GB | ~760× |
| `ore_ont_3080` | 1.5 MB | 1.43 GB | ~975× |

`ore_ont_10425` is notable: it is the ontology the **bounded DKey seeding** (v0.3.29) was
originally built for, so its conversion path is already known to be fragile — and it is
still allocating 5.9 GB from a 2 MB input.

## Suggested next step

`ore_ont_11085` — the largest RSS-to-input ratio in the corpus, small enough to diagnose
directly. **First diagnostic: does `tbox-stats` complete cheaply while `classify`
balloons?** That is the tell separating a local conversion-time cause (tractable, like
`9347`) from the search-bound tail (repeatedly NO-GO'd, no cheap entry). Check
`concept_rules` and compare against `RUSTDL_DATA_PROPERTIES=0` to see whether the DKey
channel is implicated again.

## Caveats

- Single-thread and a 30 s cap; both are recorded in the run header. Do not compare
  against a differently-pinned or differently-capped run without noting it — the harness's
  `compare` warns when headers disagree, for this reason.
- The 1,920-file pool is what is provisioned locally, not ORE entire.
- `> 4 GB: 17` is a *single-thread* count. The deployment-relevant figure is higher.
