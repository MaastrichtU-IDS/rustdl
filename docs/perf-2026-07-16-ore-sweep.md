# ORE performance sweep — 2026-07-16 (re-measured, new machine)

Full `rustdl classify` performance sweep over the ORE 2015 `pool_sample` corpus.
**Re-measurement on a different machine** — numbers differ from prior docs by design.

## Setup

- **Machine:** 32 cores, 251 GB RAM (235 GB free, idle) — the multi-GB RSS tail is not
  memory-constrained here.
- **Binary:** fresh release build at `ca24083` (HEAD), default config (wedge + label
  heuristic + Horn-shortcircuit + all default-ON flags).
- **Corpus:** `/data/dumontier/ore-run/pool_sample/files/*.owl` — **1920** ontologies,
  20.8 GB total, median 1.19 MB, p90 24 MB, p99 156 MB, max 563 MB.
- **Command:** `rustdl classify <file>` — **default mode** (unbounded per-pair; no
  `--pair-timeout-ms`), **60 s per-file outer wall cap**, 4-way concurrent, 8 rayon
  threads each.
- Raw data: `bench-results/ore-perf-sweep-20260716.tsv`.

## Headline

| outcome | count | of 1920 |
|---|---|---|
| **front-end reject** (anonymous individuals — Phase 7 gap) | 446 | 23 % |
| reached reasoner | 1474 | 77 % |
| — **classified < 60 s** | **1180** | 80 % of reached |
| — **DNF > 60 s / OOM** | **294** | 20 % of reached |

The 446 ERR1 are **100 % the same known front-end limitation** ("anonymous individuals are
not supported") — a converter gap, not a reasoner failure. They reject fast (median 0.05 s)
and are excluded from the timing denominator.

## Timing (the 1180 that classified)

- **median 0.050 s**, mean 2.72 s, p90 5.89 s, p95 16.5 s, p99 48.0 s, max 59.6 s.
- **836 (71 %) under 0.5 s**; 928 (79 %) under 1 s.

| wall bucket | count |
|---|---|
| < 0.5 s | 836 |
| 0.5–1 s | 92 |
| 1–5 s | 117 |
| 5–15 s | 69 |
| 15–30 s | 30 |
| 30–60 s | 36 |

The bulk of the ORE corpus classifies **essentially instantly**; the cost lives entirely in
a tail.

## How performance scales with size

| size band | reached reasoner | classified | DNF | median (classified) |
|---|---|---|---|---|
| < 1 MB | 823 | 776 (94 %) | 47 | 0.02 s |
| 1–5 MB | 303 | 269 (89 %) | 34 | 0.60 s |
| 5–20 MB | 169 | 94 (56 %) | 75 | 4.48 s |
| 20–50 MB | 121 | 35 (29 %) | 86 | 7.07 s |
| > 50 MB | 58 | 6 (10 %) | 52 | 8.87 s |

DNF rate climbs steeply with size: 6 % (<1 MB) → 90 % (>50 MB). **Two tail components:**

1. **Scale / memory** — big files dominate the DNF tail (52 of 77 files >50 MB DNF). This is
   the known RSS/scale asymptote.
2. **Algorithmic (small-but-hard SROIQ)** — 81 DNF are <5 MB, and the slowest *classified*
   files span sizes (`ore_ont_12174` 2.1 MB → 55.7 s; `ore_ont_13071` 5.2 MB → 58.1 s). These
   are the disjunctive/nominal search-explosion class characterized this session (dense-SROIQ
   tail, dead disj deps).

## Caveats

- **DNF = `KILLED`** conflates the 60 s cap with OOM; with 235 GB free, OOM was rare, so
  `KILLED` ≈ ">60 s". Some DNF files finish given more wall (e.g. `ore_ont_15672`-class) or a
  `--pair-timeout-ms` bound.
- **This is default mode** — the honest out-of-the-box view. A bounded mode
  (`--pair-timeout-ms`) is a sound under-approximation that would convert much of the
  *small-hard* DNF tail into fast "not-subsumed" verdicts (at some completeness cost);
  measuring that mode is a separate sweep.
- Not a head-to-head: native Konclude classifies most of these in ms–seconds and wins across
  the board (see `docs/perf-2026-06-08-konclude-vs-rustdl.md`). This sweep characterizes
  rustdl's own distribution on this machine.

## Takeaway

On the ORE corpus, default `rustdl`: rejects 23 % up front (anonymous-individuals converter
gap), and of what it reasons on, **classifies 80 % within 60 s — 71 % of those instantly
(< 0.5 s), median 50 ms**. The 20 % DNF tail is size-dominated (big-file scale/RSS) plus a
smaller algorithmic core (small-hard SROIQ). This matches the standing characterization:
rustdl is fast on the bulk and EL/Horn fragment; the open frontier is the SROIQ scale tail
(memory) and the search-reuse tail (the reuse-trap, ruled NO-GO on soundness this session) —
not the common case.
