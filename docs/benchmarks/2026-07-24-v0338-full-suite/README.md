# rustdl full performance suite — ORE-2015, v0.3.38 (2026-07-24)

Time + peak-RSS for **classify** (all 1920 ORE-2015 pool ontologies) and
**realize** (the 1147 ABox-bearing ontologies), on `rustdl 0.3.38` (release commit
`2bf15d5`). Self-contained: the narrative below is backed by the raw per-ont data
and the harness here.

- Raw data: [`classify.tsv`](classify.tsv) (1920 rows), [`realize.tsv`](realize.tsv)
  (1147 rows). Columns: `ont ⇥ wall_s ⇥ peakRSS_MB ⇥ status`.
- Harness: [`harness/`](harness/) + [`run-metadata.json`](run-metadata.json).

## Methodology

Per ont: `rustdl <classify|realize> <ont>.owl`, `RAYON_NUM_THREADS=1` (single-thread,
comparable to the 2026-07-21 memory sweep), output to `/dev/null`. Peak RSS via
`/proc/<pid>/VmHWM` polling. Caps: **48 GB** memory, **120 s** wall (watchdog kills;
status `MEMCAP48G`/`TIMEOUT120`). Driven `-P 4`, passes run **sequentially**
(classify then realize) to stay memory-safe. Machine: 32-core / 251 GB / Linux 5.15.

**Wall caveat:** ~1 s poll floor, so sub-second onts read ~1 s.

## Classify (all 1920) — no regression across 8 releases

| status | v0.3.38 | v0.3.30 (2026-07-21 baseline) |
|---|---|---|
| ok | **1633** | 1630 |
| TIMEOUT120 | 286 | 289 |
| crash (exit1) | 1 (`ore_ont_10860`, parse) | 1 |

Marginally better (+3 classified), **no regression** across v0.3.31–v0.3.38. Peak-RSS
distribution:

| bucket | onts |
|---|---|
| < 1 GB | 1790 |
| 1–4 GB | 101 |
| 4–8 GB | 17 |
| 8–16 GB | 6 |
| 16–32 GB | 4 |
| ≥ 32 GB | 2 (`ore_ont_9347` 44 GB, `ore_ont_11085` 34 GB — both TIMEOUT120) |

**93 % classify under 1 GB.** The memory tail is unchanged from v0.3.30 (no memory
work landed since the sparse-`entailed`-matrix fix; these two are compute-bound
disjunctive/data giants, not a memory leak). The v0.3.36–38 perf changes
(ABox-saturation indexing, wedge `is_blocked` clone-drop) target the
realize/consistency paths, so they correctly do not move classify's pass/timeout
counts.

## Realize (1147 ABox-bearing onts) — first realize baseline

| status | count |
|---|---|
| ok | 365 |
| TIMEOUT120 | 759 |
| exit1 | 23 |

**Completing onts are fast and light** — of the 365 ok: 307 finish in 1–5 s, 50 in
5–30 s, 8 in 30–120 s; 357 under 1 GB, 8 in 1–8 GB. That is the v0.3.31 saturation
fast-path (EL/Horn-fragment ABox) plus the v0.3.36 inconsistency short-circuit doing
their job.

**The 66 % TIMEOUT120 is by design at v0.3.38, not a regression:** `realize` was
**unbounded by default** at this version, so off-EL/Horn-fragment ABox ontologies
fall to the full per-pair tableau and the 120 s watchdog kills them — this pass
mostly measures "fragment fast-path or not." (v0.3.40 later added a default
750 ms per-pair realize timeout + a `RUSTDL_MAX_NODES=50000` sound-MISS node cap,
which is expected to collapse this timeout wall — measured separately.)

**The 23 exit1 are graceful, not crashes:** 1 parse error (`ore_ont_10860`, the
known ORE parse-crash, also the lone classify crash) + 22 clean
`Error: realize / Caused by: ontology is inconsistent; every assertion is trivially
entailed` (the v0.3.36 short-circuit; confirmed on samples). **Zero panics.**

## Before/after: v0.3.38 → v0.3.41 (termination improvements)

Re-ran the full suite on v0.3.41 (commit `7249ae1`), which added the issue-#35
realize-termination safety net (default **750 ms per-pair realize deadline** +
**`RUSTDL_MAX_NODES = 50000`** node cap) and the #38 completion-graph merge
edge-set corruption fix. Same harness. Raw: `*-v0341.tsv`.

**Classify** — no regression, and the merge fix is verdict-neutral:

| status | v0.3.38 | v0.3.41 |
|---|---|---|
| ok | 1633 | **1635** |
| TIMEOUT120 | 286 | **284** |
| crash | 1 | 1 |

Verdict-diff (`classify-verdict-diff-v0338-v0341.txt`): **0 subsumption diffs** on a
merge-exercising set (pizza, wine, ore-15672, ore-10908, + 4 ORE nominal onts) — the
#38 merge-corruption fix shifted no verdicts.

**Realize** — the termination bounds turn hangs into fast terminations:

| status | v0.3.38 | v0.3.41 |
|---|---|---|
| completed (typed) | 365 | **373** (+8) |
| hung to 120 s watchdog | 759 | **712** (−47) |
| bounded early | 23 | **62** (+39) |

**47 ontologies left the 120 s watchdog hang.** The 62 bounded-early
(`realize-v0341-exit1-categorized.txt`): **39 node-cap** ("tableau bailed out — internal
limit"), **21 graceful inconsistent** (short-circuit), 2 other (1 parse `ore_ont_10860`,
1 slow-categorization artifact). **Zero panics.**

**Honest edge:** for `realize`, a node-cap hit surfaces as an *error* ("no verdict"),
not a sound-partial — so those 39 fail fast rather than returning the types they could
determine. Robustness win (no unbounded hangs), not graceful degradation.

An interactive version of these results is published as a Claude artifact.
