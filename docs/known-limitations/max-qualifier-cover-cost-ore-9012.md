# The `≤n` complex-qualifier cover costs CPU on `ore_ont_9012` on several cores (#561)

**Date:** 2026-11-23 · **Status:** measured, **accepted** (no fix planned). The issue's three
levers were built, tested and measured; they make no difference end to end, so they are parked
on `refs/archive/heads/perf/561-max-qualifier-choose` (`7455eb4d`), not merged.
**Severity:** performance only. Answers are identical in every run below (rows + unsat +
equivalence groups + `consistent`, all `incomplete: false` at `--pair-timeout-ms 1000`).

## Symptom

PR #560 (#554) gave a `≤n R.C` qualifier the cover `C ⊑ Q` (`RUSTDL_MAX_QUALIFIER_COVER`, default
ON). On `ore_ont_9012` it measured +38% CPU at `--pair-timeout-ms 1000` (2.18 → 3.02 CPU-s, 5 reps,
`taskset` to 8 cores). Re-measured on `main` = `91a6f3e8` (release, LTO, coder host, CPU-s,
interleaved with rotating arm order; `off` = the same binary with `RUSTDL_MAX_QUALIFIER_COVER=0`):

| cores | budget | reps | main (cover on) | off | levers (`7455eb4d`) |
|---|---|---|---|---|---|
| 1 (core 84) | 1000 ms | 15 | 1.81 [1.36–1.95] | 1.79 [1.28–1.87] | 1.90 [1.37–1.97] |
| 5 (80–84) | 1000 ms | 9 | 2.24 [1.96–2.45] | 1.90 [1.63–2.03] | 2.27 [1.85–2.51] |
| 1 (core 84) | default (5 ms) | 5 | 1.44 [1.42–1.48] | — | 1.43 [1.06–1.46] |

- **On one core the cover costs nothing measurable.** On five cores it costs about +18% (median).
  The distributions are bimodal (~1.4 and ~1.9 CPU-s on one core), so 5-rep medians can move by a
  mode; that likely explains part of #560's +38%.
- The other three files the cover reaches show no change from the levers either (default budget,
  1 core, 5 reps, identical answers): `15514` 0.11 → 0.11, `9540` 2.98 → 2.94, `11647`
  5.22 [4.55–6.29] → 4.53 [4.18–5.69] (overlapping ranges, `incomplete: true` in both arms).

## The levers, and why they don't pay

`7455eb4d` implements all three of #561's levers, each a conservative extension (every clause is
a tautology under `Q := C`), with clause-level unit tests and HermiT-adjudicated ABox pairs, each
sabotaged:

1. Horn conjuncts of `C` get a global Horn cover `Hᵢ ⊑ Gᵢ` and gate the choose rule from the body;
2. atomic conjuncts go into the body directly (no `N¬A` head atom);
3. the qualifier name is memoised per interned filler.

On 9012 they do what they were meant to: the clause set shrinks from 2162 to 2140 clauses
(disjunctive 38 → 32), and the wedge's `hyper-sat` probe on `RanunculusRepens`, the class with
the nested-`=5` qualifier, drops from 31 branches (depth 22) to 21 (depth 15); `off` gives 20.
But the whole wedge `hyper-sat` pass over all 43 classes takes 0.24–0.26 CPU-s in every arm, so
the choose rule's branching is not where classify's time goes.

An attribution build (5 cores, 9 reps, 1000 ms) dropped each half of the cover in turn:

| arm | median CPU-s |
|---|---|
| cover on | 2.47 [2.00–2.64] |
| off | 2.02 [1.89–2.05] |
| no choose rule (non-Horn qualifiers fall back to `Q ⊑ C`) | 2.41 [2.03–2.47] |
| no Horn cover (Horn qualifiers fall back to `Q ⊑ C`) | 2.51 [2.12–2.64] |

Neither half alone restores `off`, and dropping the choose rule entirely (which the levers only
make cheaper) recovers about 0.06 CPU-s. The cost appears only with several cores, which points at
how classify's parallel phases schedule work once the wedge's ≤-bounds actually count successors,
not at extra branching. That was not attributed further.

## What would change this

A fix would have to come from the parallel classify phase, not the clausifier. Re-check with
`RAYON_NUM_THREADS` varied and a `--features counters` build before reopening. #560's
`clause-stats` census found the cover reaching exactly four ORE files (`11647`, `15514`, `9012`,
`9540`); it was not re-run for the parked levers, since they are not shipped.
