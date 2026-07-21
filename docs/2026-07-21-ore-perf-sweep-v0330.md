# ORE-2015 full-corpus perf sweep — rustdl v0.3.30 (2026-07-21)

Measures **wall time + peak RSS** for every one of the 1920 ORE-2015 pool
ontologies on the freshly-built v0.3.30 binary, to quantify the effect of the
sparse `Classification.entailed` matrix (the D4 giant-ontology memory + print
wall; see `2026-07-21-sparse-classification-results.md`).

## Harness

- Per ont: `rustdl classify <ont>.owl` with `RAYON_NUM_THREADS=1` (single-thread,
  comparable to prior sweeps), output discarded to `/dev/null` (the hierarchy
  **print still runs** — it is part of the classify cost).
- Peak RSS via `/proc/<pid>/VmHWM` polling (survives `SIGKILL`, unlike
  `/usr/bin/time -v`). Caps: **48 GB** memory (kill), **120 s** wall (kill).
- Driven `-P 4` (4 × 48 GB = 192 GB < box RAM) over all 1920 onts.
- **Wall caveat:** the 1 s poll granularity puts a ~1 s floor on measured wall, so
  sub-second onts read ~1 s. Precise enough for the memory / DNF-tail story, not
  for fast-ont wall. Raw rows: `perf_ore_v0330.tsv` (share drive).

## Headline — the giant-ontology memory tail is closed

- **0 / 1920 onts exceeded 48 GB** (max peak now **35.7 GB**). Before v0.3.30, at a
  40 GB cap ≥8 onts OOM'd — all ≥44 GB, up to a measured **116 GB** on `ore_ont_868`.
- The **8 giants that OOM'd pre-fix all complete now**, each **≤3.3 GB / ≤67 s**:

  | ont | classes | pre-v0.3.30 | v0.3.30 |
  |---|---|---|---|
  | ore_ont_868 | 981,151 | OOM (≥44 GB) | ok / 67 s / 3.3 GB |
  | ore_ont_10689 | ~981k | OOM | ok / 63 s / 3.3 GB |
  | ore_ont_9674 | ~981k | OOM | ok / 65 s / 3.3 GB |
  | ore_ont_16008 | ~733k | OOM | ok / 45 s / 2.2 GB |
  | ore_ont_14459 | ~848k | OOM | ok / 51 s / 2.5 GB |
  | ore_ont_8486 | ~904k | OOM | ok / 53 s / 2.6 GB |
  | ore_ont_14042 | ~517k | OOM/growing | ok / 29 s / 2.3 GB |
  | ore_ont_11395 | ~517k | OOM/growing | ok / 27 s / 2.4 GB |

## Peak RSS distribution (all 1920)

| bucket | onts |
|---|---|
| < 256 MB | 1506 |
| 256 MB – 1 GB | 283 |
| 1 – 4 GB | 103 |
| 4 – 8 GB | 16 |
| 8 – 16 GB | 6 |
| 16 – 32 GB | 4 |
| ≥ 32 GB | 2 (9347 = 35.7 GB, 11085 = 33.7 GB — both TIMEOUT) |

**98.5 % (1892 / 1920) classify under 4 GB.**

## Completion

- **1630 ok (85 %)**, **289 TIMEOUT120**, **1 exit1** (`ore_ont_10860`,
  parse/convert failure).
- The 289 timeouts are **compute-bound** (disjunctive-SROIQ / deep-saturation
  scale), now **memory-bounded** (≤ 36 GB even at the 120 s kill) — a *time*
  frontier, not OOM.
- The ≥ 8 GB residual-memory set is 10-of-12 TIMEOUT onts: their footprint is the
  EL-closure / data-DKey **working set** (reached before completion), which is the
  next memory target — distinct from the n² **output** matrix this release fixed.
  `ore_ont_11085` (33.7 GB) is the separate data/DKey convert transient.

## Takeaway

v0.3.30 eliminates the OOM tail corpus-wide and turns the previously
unclassifiable million-class ontologies into fast, low-memory completions, with no
verdict change. The remaining frontier is compute *time* (the hard disjunctive /
scale tail), not memory. The next memory lever is the engine working set on the
densest still-timing-out inputs.
