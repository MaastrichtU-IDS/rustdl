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
- Since #475 a depth cut is told apart from a saturation stall on a budgeted probe. A stall
  is reported as #462's `no_verdict` signal (a likely bug), while these cuts keep
  `no_verdict_pairs: 0` and no note.

The original #288 stall (`sat` never finishing, unbounded classify never completing) was
fixed by #382 (`RUSTDL_MAX_FIRST`, `RUSTDL_TOP_FILLER`); a guard test is in
`crates/owl-dl-reasoner/tests/pmd_core3_max_first.rs`.

## Single-query surfaces (#510)

`instance`, `instances`, `instances-expr` and `realize` run their probes under the per-pair
realize deadline (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, default 750), so they take the same bounded
branch and the same 256-level cap. On `instance` a cut costs the whole answer (`unknown`, exit
3), not one pair of a classify run. The per-query census found 36 ORE files where `instance`
never answered "no"; on 13132, 394 and 2488 every trace shows exactly 32 cap hits (early
abandon), and `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` answers "no" on all 15 cut pairs, as HermiT
and Konclude do.

Since #510, the instance-probe surfaces say when the cut was the depth limit and point to
`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`:
- `instance` and `instances` (`InstanceFlags::depth_capped`);
- `instances-expr` (`CeInstances::depth_capped`);
- `realize` (`Realization::depth_capped`), where `=0` also takes the deadline off the
  classification step;
- `justify` / `repair`'s instance checks (`JustifyOutcome::depth_capped`,
  `Repairs::depth_capped`);
- the matching Python calls' `IncompleteQueryWarning` text.

Changing the search itself for a single query (no early abandon, a deeper cap, or a
deadline-free retry) is open and needs the bar below.

What the flag does not see:
- **Early abandon off** (`RUSTDL_TABLEAU_EARLY_ABANDON=0`). A probe that keeps hitting the cap
  then runs into its deadline instead, and is reported as a deadline cut, with the advice to
  raise the timeout, which cannot recover it. Depth-cap hits before a deadline are not tracked.
- **`realize`'s classification step and #388's direct-type checks.** They don't feed
  `Realization::incomplete`, so they don't feed `depth_capped` either. `Realization::depth_capped`
  counts named individuals' probes only, while `InstanceFlags` / `CeInstances` count anonymous
  individuals' too.
- **`--json`.** No JSON output carries the cut cause, and under `--json` `instances-expr` and
  `realize` print no cut warning (`justify --json` does).
- **`justify`'s other checks** (subclass, unsatisfiability, consistency) run without a deadline,
  so their depth limit is #462's `no_verdict`.
- A deadline-free probe that ends at its own depth limit is #462's `no_verdict` signal, never
  `depth_capped`.
- **`justify` / `repair` report a depth cut only on an outcome that is incomplete anyway**; a depth
  cut never makes one incomplete. A depth cut on the ⊥-module check is discarded (the full set
  re-decides), so a definite answer stays complete and unflagged. The flip side: such a discarded
  depth cut followed by an unrelated give-up (the node cap, or a deadline on the full set) still
  reports `depth_capped`, and the depth advice is shown although the cut that mattered was not a
  depth cut. The warning text ("at least one instance check hit …") stays literally true.

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
