# rustdl memory-oriented performance — ORE-2015 full corpus (v0.3.30, 2026-07-21)

The authoritative **memory** benchmark for rustdl: peak RSS (and wall) for every
one of the 1920 ORE-2015 pool ontologies on v0.3.30, the release that made the
classification result matrix sparse. Self-contained — the narrative below is
backed by the raw data and harness in this directory.

- Raw per-ont data: [`perf_ore_v0330.tsv`](perf_ore_v0330.tsv) — `ont ⇥ wall_s ⇥ peakRSS_MB ⇥ status`.
- Trajectory probes: [`gate868-before-trajectory.txt`](gate868-before-trajectory.txt),
  [`gate868-after-trajectory.txt`](gate868-after-trajectory.txt),
  [`probe9347-trajectory.txt`](probe9347-trajectory.txt),
  [`probe11085-trajectory.txt`](probe11085-trajectory.txt).
- Harness (reproducible): [`harness/`](harness/) + [`run-metadata.json`](run-metadata.json).
- Design + correctness of the fix: `../../superpowers/specs/2026-07-21-sparse-classification-entailed-matrix-spec.md`,
  `../../2026-07-21-sparse-classification-results.md`.
- **Soundness/completeness of the recovered giants vs ELK:**
  [`GIANT-VALIDATION.md`](GIANT-VALIDATION.md) — every distinct giant's output is
  byte-for-byte ELK's closure (FP=0/MISSED=0, up to 981k classes / 14.8M
  subsumptions). Raw: [`elk-giant-validation.log`](elk-giant-validation.log).

## Methodology

Per ont: `rustdl classify <ont>.owl` with `RAYON_NUM_THREADS=1` (single-thread,
comparable to prior sweeps), output discarded to `/dev/null` — **the hierarchy
print still runs**, so wall includes it. Peak RSS via `/proc/<pid>/VmHWM` polling
(survives `SIGKILL`, unlike `/usr/bin/time -v`). Caps: **48 GB** memory, **120 s**
wall (either kills the process). Driven `-P 4` (4 × 48 GB < box RAM). Machine:
32-core / 251 GB / Linux 5.15. Binary: `rustdl 0.3.30` (`a92f264`).

**Wall caveat:** the 1 s poll granularity puts a ~1 s floor on measured wall, so
sub-second onts read ~1 s. Precise enough for the memory / DNF-tail story; not for
fast-ont wall.

## The fix under test: sparse `Classification.entailed`

Pre-v0.3.30 the classifier stored the subsumption result as a dense
`Vec<FixedBitSet>` n×n matrix, allocated up front at **n²/8 bytes regardless of
content**. On the largest ORE ontologies (hundreds of thousands to ~1M classes)
that alone was tens to >100 GB, and printing the hierarchy from it was O(n²).
v0.3.30 makes the matrix adaptive: `Dense` for ≤ 60k classes (byte-identical, keeps
the EL niche fast) / `Sparse` per-class sorted rows above (unsatisfiable rows
elided; a single `entails(i,j)` choke-point reintroduces `⊥ ⊑ *`). Verdict-
preserving: dense-vs-sparse byte-identical on galen/sio; corpus FP=0/MISSED=0.

## Headline — the OOM tail is closed

- **0 / 1920 onts exceeded 48 GB** (max peak now **35.7 GB**). Pre-v0.3.30, at a
  40 GB cap ≥ 8 onts OOM'd — all ≥ 44 GB, up to a measured **116 GB** on
  `ore_ont_868` (see `gate868-before-trajectory.txt`).
- The **8 giants that OOM'd pre-fix now all complete**, each **≤ 3.3 GB / ≤ 67 s**:

  | ont | classes | pre-v0.3.30 | v0.3.30 |
  |---|---|---|---|
  | ore_ont_868 | 981,151 | OOM (≥ 44 GB) | ok / 67 s / 3.3 GB |
  | ore_ont_10689 | ~981k | OOM | ok / 63 s / 3.3 GB |
  | ore_ont_9674 | ~981k | OOM | ok / 65 s / 3.3 GB |
  | ore_ont_16008 | ~733k | OOM | ok / 45 s / 2.2 GB |
  | ore_ont_14459 | ~848k | OOM | ok / 51 s / 2.5 GB |
  | ore_ont_8486 | ~904k | OOM | ok / 53 s / 2.6 GB |
  | ore_ont_14042 | ~517k | OOM/growing | ok / 29 s / 2.3 GB |
  | ore_ont_11395 | ~517k | OOM/growing | ok / 27 s / 2.4 GB |

  `ore_ont_868` end-to-end: **> 20 min / 116 GB (unfinished) → 69 s / 3.3 GB, full
  981,153-line hierarchy** (before/after trajectory files).

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

- **1630 ok (85 %)**, **289 TIMEOUT120**, **1 exit1** (`ore_ont_10860`, parse).
- The 289 timeouts are **compute-bound** (disjunctive-SROIQ / deep-saturation
  scale), now **memory-bounded** (≤ 36 GB even at the 120 s kill) — a *time*
  frontier, not OOM.

## Residual memory is a symptom of the compute frontier, not an independent lever

The ≥ 8 GB set is 10-of-12 TIMEOUT onts. They are ABox / data-flood-heavy
(`ore_ont_9347`: 114 classes but ~55k ObjectPropertyAssertion + DataPropertyAssertion;
`ore_ont_11085`: 22k classes + 9.5k ClassAssertion, no data) — their footprint is
the engine **working set**, distinct from the n² **output** matrix this release
fixed. Two 360 s / 100 GB trajectory probes show **neither is
memory-bound-would-complete** — both TIMEOUT with **0 lines output**:

- `ore_ont_9347` — RSS **grows unbounded** (35 GB @ 120 s → 69 GB @ 360 s):
  compute *diverging*.
- `ore_ont_11085` — RSS **plateaus** at ~33 GB by 90 s, then churns compute for
  5+ min with no output: memory *stable, not limiting*.

So reducing working-set memory would not recover these onts — the binding
constraint is compute-time / non-convergence, the documented from-scratch
clash-driven-search frontier. This is the mirror image of the 8 recovered giants
(cheap, terminating compute; huge *output*). **Conclusion: the v0.3.30 output-matrix
fix is the genuine memory win; a further working-set memory fix is a NO-GO as a
completeness/recovery lever.**
