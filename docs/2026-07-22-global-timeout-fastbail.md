# `--global-timeout-ms` fast-bail — profiler-driven fix (2026-07-22)

Branch `perf/classify-global-deadline-fastbail`.

## Symptom

`--global-timeout-ms N` (v0.3.32) did not bound the wall on large out-of-EL
ontologies. `ore_ont_3215` (DL-approximated SNOMED: 54,973 classes, 18,323
disjunctive definitions `C ≡ (∃R.D ⊔ D) ⊓ E`) ran **317 s and peaked at ~26 GB**
under a 90 s budget — a `timed_out_pair_ids` vector of ~n² (3.3 billion) tuples
plus deadline-oblivious per-probe work.

## Investigation (measure, don't guess)

Four wrong hypotheses were ruled out by isolation before the profiler settled it:
- `--saturation-only` = **8 s** ⇒ not saturation / matrix assembly.
- `rustdl subclass` (builds `PreparedOntology` + 1 probe) = **8 s** ⇒ not the prep.
- `--global-timeout-ms 1` (deadline already past) still ran **176 s** ⇒ an
  unbounded phase, not merely "deadline not honored".

A `gdb` sampling profiler (no `perf` on this host) over the running classify
pinned the hot path: `classify.rs` per-class **unsat-probe** `par_iter` →
`decide_classify_with_deadline` → **`decide`** (`lib.rs:5143`), with
`copy_nonoverlapping<ConceptId>` prominent.

## Root cause

`decide` did **`pool.clone()`** — cloning the entire ~200k-concept `ConceptPool` —
plus tableau-context setup **before** checking the deadline (the check only
happened later, inside the search). Under a global deadline, the ~55k post-deadline
probes (unsat pre-pass + tier walk) each paid a full pool clone even though the
search would instant-timeout. That clone churn was the ~168 s.

## Fix

Deadline fast-exit at the top of `decide` (`lib.rs`), before the clone: if the
deadline is already expired, return `Ok(None)` (no verdict — every caller treats
it soundly: unsat probe → satisfiable, subsumption probe → not-subsumed). Plus
(earlier commit) bounded the O(n²) `timed_out_pair_ids` materialization: the sweep
and tier walk record one marker per class/sup and `break` instead of enumerating
every undecided pair, and the entailment-matrix BFS uses a generation-stamped
visited buffer instead of `vec![false; n]` per class.

## Results

`ore_ont_3215`, `--global-timeout-ms 30000`: **317 s / 26 GB → 42 s / 3 GB**;
`timed_out_pairs` 3.3 B → 54,974.

Before/after, `--global-timeout-ms 60000` (external 400 s cap):

| ont | before | after |
|---|---|---|
| ore_ont_3215 | 5:03 / 52 GB | **1:06 / 2 GB** |
| ore_ont_11270 | 2:34 / 14 GB | **1:24 / 1 GB** |
| ore_ont_10140 | TIMEOUT400 / 3 GB | **1:02 / 3 GB** |
| ore_ont_13052 | TIMEOUT400 / 51 GB | **2:48 / 50 GB** |
| ore_ont_12128 | TIMEOUT400 / 7 GB | TIMEOUT400 / 7 GB (unchanged) |

**Verdict-preserving:** 0 output diffs before-vs-after across 40 ORE onts on
default (no-deadline) runs — the fast-exit only fires when the deadline is already
expired, which previously produced the same timeout `None` after a wasted clone.
Full workspace test suite green; fmt/clippy clean; the
`global_deadline_is_sound_and_bounded` invariant test
(`undecided_pairs().len() == timed_out_pairs`) passes.

## Residual (follow-up)

`ore_ont_12128` (∀-heavy, 22,063 `∀`) still times out under a global budget — a
*different* deadline-oblivious phase (not the pool clone), left for a separate
profiler-driven pass. And the fixed overhead (saturation + prep + label cache +
matrix assembly) is inherently unbounded, so the wall floor on a 55k-class ont is
~10–50 s regardless of budget.

## Note on the v0.3.32 claim

The v0.3.32 CHANGELOG said `--global-timeout-ms` "bounds total time regardless of
pair count." That was overstated: it bounds the *probing*, but saturation/prep/
assembly are not deadline-gated, and (pre-this-fix) the per-probe pool clone made
it effectively unbounded. Soften the claim at next release.
