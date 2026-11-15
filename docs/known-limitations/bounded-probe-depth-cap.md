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

`instance` now also retries such a cut (next section); the other surfaces only report it.

**A budget cut after a depth-cap hit (#510 C).** A depth-bound probe can still end on its
deadline or the node cap before early abandon's 32nd hit: with early abandon off
(`RUSTDL_TABLEAU_EARLY_ABANDON=0`), or on a loaded or slow host. The same pair can be
`depth_capped` on a quiet run and a plain deadline cut on a busy one.
- The tableau counts depth-cap hits on every probe (the deadline-free probe's deeper cap
  included). Nothing in the search reads the count.
- `InstanceFlags::depth_cap_hit` marks a deadline or node-cap cut that had at least one hit.
- `instance`, `instances` and Python `is_instance_of` then add that
  `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` may help as well as a larger budget. Both may help here,
  unlike a `depth_capped` cut.
- On the single `instance` query, `depth_cap_hit` also triggers the deep retry described below,
  as `depth_capped` does; the clause above then shows only with the retry off
  (`RUSTDL_INSTANCE_DEPTH_FALLBACK_MS=0`). Iterative deepening's over-attribution (next point)
  can therefore cost a retry, up to its budget; that is sound, since the retry only answers
  what the deep probe decides.
- Under iterative deepening (`RUSTDL_TABLEAU_ITERATIVE_DEEPENING=1`, default off) the count is
  reset when the final level starts, whose cap is at least `MAX_SEARCH_DEPTH`. Two exits
  leave a shallow level's hits in the count, so the flag can over-attribute there:
  - a non-final level stops on the node cap (`NodeCap` ends the loop before the final level);
  - the caller's deadline passes after a shallow level, so the final level never starts.
  In both, `depth_cap_hit` can be set although `MAX_SEARCH_DEPTH` was never reached.

What the flags do not see:
- **`depth_cap_hit` on the other surfaces.** `instances-expr`, `realize`, `justify`, `repair` and
  the other Python calls report a hit-then-budget cut as a plain budget cut (#510 C2).
  `classify`'s `depth_capped_pairs` does not count it either.
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

## `instance` retries a depth cut on the deep path (#510, 2b)

When the bounded probe of a single `instance` query (CLI `instance`, Rust
`is_instance_of_reporting` / `is_instance_of`, Python `is_instance_of`) ends `depth_capped`, or
is cut by its deadline or the node cap after a depth-cap hit (`depth_cap_hit`, #510 C), it is
retried once on the deep path: depth 1,000,000, anywhere-blocking, no early abandon. That is
the probe `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` runs, and the one #401's seeded-first attempt runs
(the two share `PreparedOntology::decide_deep_with_deadline`). It runs under its own deadline,
`RUSTDL_INSTANCE_DEPTH_FALLBACK_MS` (default 60,000 ms), and the node cap in force: the
single-query cap (`RUSTDL_QUERY_MAX_NODES`) on the CLI and in Python, the default cap
(`RUSTDL_MAX_NODES`) for a Rust caller that does not hold `single_query_node_cap()`.
- **Retry decides:** the answer is the retry's. No cut flag is set and
  `InstanceFlags::depth_fallback` is `Decided`. The #369 lazy consistency check runs on the
  retry's answer. The CLI prints a note naming the knob.
- **Retry cut by its budget or the node cap:** `unknown`, exit 3, `incomplete`, and
  `depth_capped` is false (the retry's own limit was the budget). `depth_fallback` is `Cut`,
  and the CLI and Python's warning advise raising `RUSTDL_INSTANCE_DEPTH_FALLBACK_MS` or
  `RUSTDL_QUERY_MAX_NODES`.
- **Retry ends in `NoVerdict`** (its saturation stalls, or the deep search bails at its own
  1,000,000-level limit): reported as #462's `no_verdict`, as `=0` reports it.
  `is_instance_of` and `is_instance_of_internal` then return `Err(NoVerdict)`, and Python
  `is_instance_of` raises `RustdlError`, where the depth cut used to be `Ok(false)` /
  `False` with `incomplete`. The CLI prints `unknown` with #449's note, after a note that the
  retry ran.

`RUSTDL_INSTANCE_DEPTH_FALLBACK_MS=0` turns the retry **off** (#514's behaviour). This differs
from `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` (no deadline at all) and
`RUSTDL_CONSISTENCY_FALLBACK_MS=0` (a deadline already expired). With
`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` there is no bounded probe and so no retry.

Not retried:
- **A budget cut with no depth-cap hit.** Only a `depth_capped` cut, or a budget cut after at least one
  hit (`depth_cap_hit`, #510 C), is retried.
- **`instances`, `instances-expr`, `realize`, `justify` / `repair`.** They report the cut as
  before. `justify` asks one instance check per QuickXplain step, so a retry there could cost
  one budget per step; it calls an entry without the retry. `instance` can therefore answer
  "no" where `instances` reports the same pair as `incomplete`, which is sound.

The retry runs at most once per `instance` call; no batch or loop inside rustdl reaches it. A
call whose retry never decides takes up to the budget (60 s) longer than it did. One whose retry
answers "yes" just inside the budget also runs the #369 lazy check
(`RUSTDL_CONSISTENCY_FALLBACK_MS`, 10 s), so the bound is about 70 s over prep. A caller that
loops over `is_instance_of*` or the CLI pays the budget once per call, and can lower it or turn
the retry off with `RUSTDL_INSTANCE_DEPTH_FALLBACK_MS=0`. `scripts/cross-command-laws.py` sets
20 s, below its own 30 s call cap.

**The trigger depends on timing only before the first hit.** Before #510 (C), the bounded
probe was retried only when it reached 32 cap hits (early abandon) before its 750 ms deadline;
under load, or with `RUSTDL_TRACE=1`, the same probe could reach the deadline first, stay a
deadline cut and the pair stay `unknown`. On #510's census calls the base reported 258 depth
cuts under load (8 lanes) and 263 on a quiet core; one call, on `ore_ont_12698`, answered "no"
in 2 of 3 quiet reps. Since #510 (C) a deadline that comes after at least one hit makes the
probe `depth_cap_hit`, which is retried too. Only a probe whose deadline comes before its first
hit stays a plain budget cut, so a pair can still stay `unknown` under heavy load, and early
abandon off (`RUSTDL_TABLEAU_EARLY_ABANDON=0`) no longer hides a depth-bound probe from the
retry.

Measured on the per-query census's 3,566 `instance` calls (ORE F2a/F3 files), at a 10 s retry
budget:
- **Under load** (8 lanes), 252 calls changed from `unknown` to "no".
- **On a quiet core** (3 interleaved reps of each of the 263 depth-cut calls), 253 changed. The
  other 10, all on `ore_ont_13113`, were cut at 10 s; at 30 s they answer "no" in 19.8–26.0
  CPU-s for the whole call (one run each, quiet core). The 60 s default is the smallest round
  budget with at least 2x slack over them, a rule set before the data. The budget is
  wall-clock, so under load these can still be cut (`unknown`, never a wrong answer).
- HermiT and Konclude both refute every one of these "no" answers, through the
  `ClassAssertion(ObjectComplementOf(C) a)` consistency control. There is no new "yes", no
  changed answer and no lost answer.
- **Cost per retried call** (extra CPU): median +0.69 s, p90 +3.03 s. A retry decided at 10 s
  took at most +4.99 s. `ore_ont_778` changes at 4.1–5.1 CPU-s per call, `ore_ont_9890` at
  0.43 s.

**Re-measured on main 73c35e8d with #510 C's hit trigger** (M0′, against main: 3,576 census
calls, 8 lanes, declared before the data):
- **Answers:** at the default, 293 calls change from `unknown` to "no" (279 depth-capped, 14
  hit-triggered); with early abandon off, 296 do. HermiT and Konclude both refute every one.
  There are 0 new "yes" and no other change after serial rechecks.
- **Cost per retried call:** median +0.5 CPU-s, p90 +4.1 s, max +30.5 s.
- **The 60 s default's 2x rule held on #510's census pairs, not on this tree.** The slowest
  retried calls (whole call, under 8-lane load) are:
  - `ore_ont_5688` (10 calls, outside #510's census): 28.9–31.0 CPU-s, up to 34 s wall, so
    1.8–1.9x slack;
  - `ore_ont_13113`: 21.8–23.2 CPU-s, 2.4–2.6x;
  - `f1-wine`: 18.5–19.0 CPU-s, 2.6–3.2x.
- The default stays 60 s (manager's decision). The budget is wall-clock, so under load a retry
  can be cut. A cut retry stays `unknown`, never a wrong answer.

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
