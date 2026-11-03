# Bounded classify cuts deep unsat probes at 256 decision levels (#288)

**Date:** 2026-11-03 · **Status:** measured, **NOT fixed** (a deeper cap was a no-go; issue left open)
**Severity:** `incomplete: true` on bounded classify. No answer is wrong; on the measured files no row is missing either.

## Symptom

A bounded main-tableau probe (any run with a per-pair or global timeout) stops at
`MAX_SEARCH_DEPTH = 256` decision levels, directly or through early-abandon, which fires
after 32 depth-limit hits. No per-pair budget recovers such a probe.

- On PMD core, the 9 per-class unsat probes for `PMD_0010100`, `PMD_0020213`,
  `PMD_0020229`–`0232` and `PMD_0020238`–`0240` need ~1,044 levels. They are cut at 100,
  300 and 1000 ms alike.
- The unbounded run (`--pair-timeout-ms 0 --global-timeout-ms 0`, depth 1,000,000) finishes
  in ~15 s with identical rows.
- Since #424 the INCOMPLETE banner and the Python warning report these cuts as depth-limit
  cuts (`depth_capped_pairs`), not as timeouts.

The original #288 stall (`sat` never finishing, unbounded classify never completing) was
fixed by #382 (`RUSTDL_MAX_FIRST`, `RUSTDL_TOP_FILLER`); a guard test is in
`crates/owl-dl-reasoner/tests/pmd_core3_max_first.rs`.

## Why the cap stays at 256

Measured on main 8144a9d1 at `--pair-timeout-ms 1000`, 1 thread, 90 s cap. A cap of 2048
was set with `RUSTDL_TABLEAU_ITERATIVE_DEEPENING=1 RUSTDL_TABLEAU_ID_SCHEDULE=2048`, a
single final level, which is a plain cap with early-abandon armed fresh. (`256,2048` is not
equivalent: there, 256 is a ≤ 20 ms shallow pass, and early-abandon latches across levels.)

- **Population.** Of the 1,920-file ORE pool, 36 files reach an armed main-tableau probe,
  and 5 have a depth-capped probe: 15514, 185, 6934, 778 (DNF) and 9890.
- **Default vs cap 2048**, 3 interleaved reps:

| file | default | cap 2048 |
|---|---|---|
| ore_ont_9890 | ok, 76.6–81.4 s | **DNF, still at a 300 s cap** |
| ore_ont_15514 | 1.2 s | 4.0 s, no gain (depth cuts become timeout cuts) |
| ore_ont_185 | 1.3 s | 5.7 s, no gain |
| PMD core | 9 cuts | complete, rows identical, 1.03× |

  The aggregate is +8.1% wall, and no row is gained or lost anywhere.
- **Early-abandon after 4 hits** on top of the 2048 cap rescues none of it.
- **A deeper cap for the per-class unsat probe only** was not built. 9890's 267 capped
  probes (627 classes) fit per-class unsat probes, the kind PMD's are, so such a cap is
  expected to regress 9890 the same way. This is inferred, not measured.

At 2048 levels these probes do not finish within their 1 s budget. They run it out instead
of being abandoned cheaply at 256.

## What would change this

A probe-level signal that a deeper search will finish (for example, a near-linear descent
with almost no backtracking, as on PMD), so that only those probes are deepened. Any such
change must pass the #288 bar, set before the data:
- 0 new false positives;
- 0 ok→DNF;
- aggregate CPU ≤ +5%;
- no file > 1.5× and > 1 s slower without a gain.

Data: #288 issue comments; `~/rustdl/work-288b/` on the shared measurement host
(`plan-c.md` §5, `q1/`, `q2/`, `q2c/`).
