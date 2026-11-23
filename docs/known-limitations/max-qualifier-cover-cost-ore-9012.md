# The `≤n` complex-qualifier cover costs CPU on `ore_ont_9012` (#561)

**Date:** 2026-11-23 · **Status:** measured, **unattributed**. #561 stays open.
**Severity:** performance only. Answers are identical in every run below. The comparison covers
rows, unsat, equivalence groups and `consistent`.

The three #561 levers were built, tested and measured, and no end-to-end gain was detected. They
are parked on `refs/archive/heads/perf/561-max-qualifier-choose` (`7455eb4d`) and not merged.
Related: [classify-misses-unsat-complex-cardinality-qualifier.md](classify-misses-unsat-complex-cardinality-qualifier.md)
(the #554 defect that #560's cover fixed). Raw data, the pinned binaries and the scripts are on the
coder host in `~/rustdl/sweeps/w561/`.

## Conditions

- **Binaries:** `main` = `91a6f3e8` and the levers = `7455eb4d`, both release builds with LTO. Both
  were pinned and verified behaviourally: the #554 L5 repro reads `inconsistent`, and with
  `RUSTDL_MAX_QUALIFIER_COVER=0` it reads `consistent`.
- **`off` arm:** `main` run with `RUSTDL_MAX_QUALIFIER_COVER=0`. That makes it a flag delta, not a
  separate pre-#560 binary. #560 states that `=0` restores the old encoding byte for byte, and
  `clause-stats` agrees on 9012.
- **Measurement:** CPU time (user + sys), interleaved runs with rotating arm order.
  `RAYON_NUM_THREADS` was unset, so the thread count follows the `taskset` mask.
- **Host:** contended. Two other workers were running on cores 85–95. In the one-core runs:
  - wall time exceeded CPU time by up to about 1 s;
  - the low-mode values cluster in time across all arms;
  - the same bimodality recurs on `9540` and on 9012 at the default budget.

  CLAUDE.md's rule (recheck serially, by CPU time, on a quiet host) was therefore **not met**, and
  the one-core data cannot resolve a difference smaller than about 10%.

## Measurements

All runs below use `--pair-timeout-ms 1000` unless the row says otherwise. Every 9012 run at that
budget has `incomplete: false`.

### One core (core 84): unresolved, 0 to +14% depending on the run

| run | reps | main (cover on) | off | levers |
|---|---|---|---|---|
| first window | 7 | 1.88 | 1.65 | 1.85 |
| second window | 15 | 1.81 [1.36–1.95] | 1.79 [1.28–1.87] | 1.90 [1.37–1.97] |
| pooled | 22 | median 1.84, mean 1.78 | median 1.75, mean 1.66 | median 1.89, mean 1.78 |

- The cover costs about +14% over `off` in the first window and about 0% in the second. Pooled,
  it costs +5% at the median and +7% at the mean.
- The distributions are bimodal, at about 1.4 and about 1.9 CPU-s.

### Five cores (80–84): a discrete lump, real but not attributed

The cost appears as a discrete lump of about 0.4 CPU-s. A "high-state run" below is one above
2.2 CPU-s; across the two windows:
- `off` was never in the high state (0 of 18 runs);
- cover-on was in it in 13 of 18 runs (6 of 9 in the first window, 7 of 9 in the second).

| window | reps | cover on | off | median Δ |
|---|---|---|---|---|
| 1 (`main` vs `off` vs levers) | 9 | 2.24 [1.96–2.45] | 1.90 [1.63–2.03] | +18% |
| 2 (attribution build) | 9 | 2.47 [2.00–2.64] | 2.02 [1.89–2.05] | +22% |

In window 1 the levers arm measured 2.27 [1.85–2.51], also 6 of 9 in the high state. No gain was
detected at n = 9.

The cause is not attributed. That it comes from how classify's parallel phases schedule work is an
**untested hypothesis**: `RAYON_NUM_THREADS` was not varied and no counters build was run.

#560 measured +38% (2.18 → 3.02 CPU-s, 5 reps) with `taskset` set to 8 cores. That setting
(8 cores, or no core restriction) was not re-measured, so #560's figure stands as neither
reproduced nor refuted.

### Default budget, one core, and the other reach files

| ontology | reps | main | levers |
|---|---|---|---|
| 9012 | 5 | 1.44 [1.42–1.48] | 1.43 [1.06–1.46] |
| 15514 | 5 | 0.11 [0.09–0.11] | 0.11 [0.10–0.12] |
| 9540 | 5 | 2.98 [2.17–3.01] | 2.94 [2.18–2.96] |
| 11647 | 5 | 5.22 [4.55–6.29] | 4.53 [4.18–5.69] |

All four comparisons are `incomplete: true` in both arms, which marks budget truncation. The
ranges overlap.

## The levers

`7455eb4d` implements the three levers from #561. Each one is a conservative extension: every
clause stays a tautology under `Q := C`. Each has a clause-level unit test, there are three
HermiT-adjudicated ABox pairs, and each lever was sabotaged to confirm a test fails.

1. **Horn conjuncts gate the choose rule.** Each Horn conjunct `Hᵢ` of `C` gets a global Horn cover
   `Hᵢ ⊑ Gᵢ`, and `Gᵢ` gates the choose rule from the body.
2. **Atomic conjuncts move into the body directly.** No `N¬A` head atom is needed.
3. **The qualifier name is memoised** per interned filler.

**Effect on the clauses and the wedge (9012):**
- The clause set shrinks from 2162 to 2140, and disjunctive clauses go from 38 to 32.
- In the `hyper-sat` probe, `RanunculusRepens` (the class with the nested-`=5` qualifier) drops
  from 31 branches at depth 22 to 21 at depth 15. `off` gives 20.

**What `hyper-sat` does and does not show:**
- **Scope:** it runs the wedge's class-satisfiability check on all 295 classes, 31 of which
  branch. It does not run classify's per-pair probes.
- **Cost of the full pass, one rep each:**

  | arm | user CPU-s | wall ms | total branches |
  |---|---|---|---|
  | `main` | 0.24 | 88 | 434 |
  | `off` | 0.24 | 84 | 423 |
  | levers | 0.26 | 98 | 424 |

  By wall time, the levers arm was the slowest.

### Attribution build: dropping each half of the cover

The build was the levers commit plus two temporary toggles:
- one sent non-Horn qualifiers back to `Q ⊑ C`, which removes the choose rule;
- the other did the same for Horn qualifiers, which removes the Horn cover.

The build was run on five cores, 9 reps per arm, at 1000 ms. Its source diff and its run script
were not saved; only the binary (`rustdl-x561-attrib`) and the outputs remain.

| arm | median CPU-s | high-state runs (> 2.2 CPU-s) |
|---|---|---|
| off | 2.02 [1.89–2.05] | 0 of 9 |
| no choose rule | 2.41 [2.03–2.47] | 5 of 9 |
| cover on | 2.47 [2.00–2.64] | 7 of 9 |
| no Horn cover | 2.51 [2.12–2.64] | 8 of 9 |

Neither half alone restores `off`. This data does not show which part of the cover produces the
lump.

## Before reopening

- Re-measure on a quiet host, both serially and at 8 cores (or with no core restriction).
- Vary `RAYON_NUM_THREADS`.
- Use a `--features counters` build to attribute the five-core lump.

#560's `clause-stats` census found that the cover reaches exactly four ORE files: `11647`, `15514`,
`9012` and `9540`. The census was not re-run for the parked levers.
