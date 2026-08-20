# DNF tail re-census: 143, and the dominant bucket is MEASURED OUT

**Date:** 2026-08-20 · **Binary:** current `main` (v0.4.19 + the 2026-08-18/19 work)
· **Population:** the 143 not-ok rows of `data-2026-08-15-v0419-two-arm-sweep.csv`
· **Raw data:** `data-2026-08-20-tail143-phase-census.tsv`,
`…-label-cache-addressability-78.tsv`, `…-no-banner-decomp-42.tsv`

**Headline: the tail is 143 (141 DNF + 2 reject), down from 257 (Aug 1) and 164 (Aug 12). Its
largest bucket is 55% of the tail by wall and rescues 2 ontologies. Do not plan against it.**

## The partition, current main

Method: one run per ontology at `--global-timeout-ms 20000`, which makes a would-be DNF return
*with* its phase banner; largest non-`unattributed` phase wins.

| dominant phase | n | share |
|---|---:|---:|
| `label_cache_build` | **78** | 55% |
| *(all phases 0 — see the parser trap)* | 42 | 29% |
| `tier_walk` | 13 | 9% |
| `prepare` | 8 | 6% |
| `sweeps` / `saturate` | 2 | 1% |

## `label_cache_build` (78) is MEASURED OUT as a lever

Every one of the 78 reports **`pruned=0`** — the per-class label cache is built and then consulted
**zero** times — while consuming a median **17,305 ms** of the 20 s budget (mean 15,496). That
looks like an obvious lever: half the tail burning ~78% of its budget on unread work.

**It is not.** Disabling the cache outright (`RUSTDL_LABEL_HEURISTIC=0`), 78 ontologies × 2 arms,
60 s cap:

| | |
|---|---|
| BOTH_DNF | **76** |
| **RESCUED by disabling** | **2** — `ore_ont_10109` (0 → 180 rows), `ore_ont_6333` (0 → 89 rows) |
| ontologies losing rows | **0** (the heuristic is a sound prune; direction confirmed) |
| aggregate wall | 4,683 s → 4,645 s (**+0.8%, flat**) |

**Wall is flat because the freed 17 s is absorbed by the next phase** — the same mechanism the
`unsat_probe_cap` negative already recorded. This *confirms* that result rather than overturning
it: the starved consumer usually cannot finish either, occasionally it can (2/78, 2.6%).

Reclaiming provably-unread work is still worth doing as hygiene (a demand-driven cache costs
nothing where `pruned=0`), but **its prize is 2 ontologies, not 78.** Another instance of *a shape
census sizes a population; it does not predict a rescue.*

## THE PARSER TRAP: "no-banner" is mostly the pure-EL path

The 42 in that row are **not** ontologies without a banner. **On the pure-EL fast path every phase
in `# wall breakdown ms:` reports `0`** — the work happens in the saturator, which the breakdown
does not attribute — so a "largest phase" parser finds nothing and silently buckets them as
`no-banner`.

**This very likely affects the 2026-08-12 census too**, whose `no-banner` bucket was 36 of 164
with a median 1.19 GB RSS. Anyone re-running a phase census must handle the all-zero case
explicitly.

Their real content, on inspection: 1 **parse error** (`ore_ont_10860` — not a reasoning failure at
all), 8 genuine stalls, 2 partial, and 31 large ontologies emitting big partial results.

## RETRACTED WITHIN THIS DOCUMENT: "31 are one pair short of complete"

I first classified those 31 as **NEAR_COMPLETE** because each reported *"1 class pair hit the
timeout"* — e.g. `ore_ont_11085`, 22,642 classes, **1,718,130 rows, 1 pair short**. Thirty-one
ontologies of wildly different sizes all one pair from complete would have been a remarkable
finding, and it is false.

Raising the global budget from 20 s to 60 s:

| ontology | g=20 s | g=60 s |
|---|---|---|
| `ore_ont_10621` | 42,183 rows, inc=**1** | 42,270 rows, inc=**1** |
| `ore_ont_11196` | 23,831 rows, inc=**1** | 23,831 rows, inc=**15,042** |
| `ore_ont_14572` | 37,322 rows, inc=**1** | 37,322 rows, inc=**23,137** |

**The `incomplete` counter counts pairs ATTEMPTED AND CUT, not pairs remaining.** At 20 s only one
pair had been attempted; at 60 s the run reaches a phase that attempts tens of thousands and cuts
them all — while the row count barely moves. Unbounded at `--pair-timeout-ms 1000` both produce
**zero rows at 200 s**.

So a small `incomplete` count is **not** evidence of near-completeness, and reading it that way
inverts the conclusion. These are ordinary members of the tail.

## Where that leaves the tail

Genuinely un-attacked reasoning stalls: **`tier_walk` 13 + `prepare` 8 + true stalls 8 ≈ 29**. The
remainder is either measured out (78) or large-ontology scale behaviour that no single phase lever
addresses — these are 20k–190k-class inputs whose per-class phases dominate.

**Recommended next step: read ONE failing ontology, not another population.** This repo's record
is explicit that both of its largest tail wins came from reading a single instance, and that three
population studies here have been retracted or bounded — two of them in this document. The eight
zero-output stalls (`10929`, `15203`, `15635`, `16744`, `2504`, `4572`, `8445`, `8737`) are the
natural candidates, and Konclude classifies ~91% of this tail, so each is a gap with a
known-achievable answer.
