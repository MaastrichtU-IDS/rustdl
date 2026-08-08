# `horn_fixpoint` had no wall-clock bound (`RUSTDL_FIXPOINT_DEADLINE`)

2026-08-08. Opt-in, default OFF pending two flip gates. Found by following the
per-pair budget overshoot recorded in
`docs/2026-08-08-label-cache-aggregate-bound.md`.

## The defect

`horn_fixpoint`'s drain loop was bounded by a `max_iters` **step count** and never
consulted the clock:

```rust
while let Some(ev) = self.worklist.pop() {
    steps += 1;
    if steps > max_iters { return HyperResult::Stalled; }   // step bound only
    if matches!(self.process_event(ev), FireOutcome::Clash) { return HyperResult::Unsat; }
}
```

A fixpoint whose events are individually expensive therefore overran its time
budget without limit. The whole wedge engine consulted `self.deadline` at exactly
**three** places: `solve`'s entry, the strided `enumerate_matches` probe added in
v0.4.16, and `decide_with_deadline` setting it. `horn_fixpoint` and
`solve_at_most` had none.

Symptom on `ore_ont_6134` at `--pair-timeout-ms 50`: **19,906 pairs cost
100–999 ms** and 26 cost ≥1000 ms — a 2–20× overshoot of the stated budget.

## How it was localised (the step that mattered)

The first hypothesis was granularity of the v0.4.16 `enumerate_matches` stride.
Made it env-tunable and swept it, prediction declared in advance:

| stride | rows | fallthrough `ran` | 100–999 ms | ≥1000 ms |
|---|---|---|---|---|
| 4096 | 2,360 | 22,688 | 19,525 | 82 |
| 256 | 2,360 | 22,237 | 19,180 | 46 |
| 64 | 2,360 | 22,194 | 19,640 | **17** |

**Refuted** — the bulk bucket is flat and rows are identical. But the probe
**demonstrably fires** (the ≥1000 ms tail falls 82 → 17, 4.8×), and that is what
makes the flat column a finding rather than a dead instrument. It excluded the
match cross-product and pointed at the loop with no check at all.

## Effect

`ore_ont_6134`, `--pair-timeout-ms 50`, global 240 s, arms differing only in the flag:

| | rows | timed-out pairs | fallthrough ran | rescued | 100–999 ms | ≥1000 ms |
|---|---|---|---|---|---|---|
| OFF | 2,358 | 21,573 | 20,582 | 12 | 16,269 | 354 |
| **ON** | **2,387** | **66,752** | 66,006 | 31 | **0** | **0** |

The overshoot buckets go to **zero** — the budget is finally honoured — and in the
same wall budget the run decides **3.1× more pairs** and finds **+29 more
subsumptions**. Completeness *increases*: pairs that previously burned the budget
and defaulted to `not-subsumed` are now actually proven.

**Consequence beyond this ontology:** a per-pair budget below the overshoot scale
was silently not honoured. That includes the documented `--pair-timeout-ms 25`
wine guidance and the 50 ms adaptive label-cache floor. Not yet re-measured on
wine — stated as an implication of the `6134` data, not a measured wine claim.

## Soundness

The fix returns the **same** `HyperResult::Stalled` the `max_iters` branch two
lines above already returns, so every caller's handling is pre-existing and
exercised. A clock-truncated fixpoint is indistinguishable, to callers, from a
step-truncated one. `Stalled` is never `Sat`, so truncating can only MISS a
subsumption, never manufacture one — **no new verdict and no new soundness
surface.**

Note the direction of the measured change is *more* rows, which is the direction
that warrants a gate rather than celebration. Gates run:

* **FP=0 net with the flag ON: 12 VERIFIED, every closure exact, FP=0 / MISSED=0** —
  galen 27997, notgalen 32739, sio 8904, wine 653, ore-10908 6001, ore-15672 142,
  alehif 247, pizza 499, ro 158, sulo 51, bibtex 16, family inconsistency. Matches
  the reference values exactly.
* Workspace tests 1,596 passed / 0 failed (flag OFF). `owl-dl-py` is excluded for
  a pre-existing pyo3 link failure unrelated to this change.

Caveat on that FP=0 evidence: the curated corpus is largely EL/Horn and takes fast
paths, so it demonstrates **non-regression**, not that the mechanism is sound. The
mechanism's evidence is the reuse-of-`Stalled` argument plus the `6134` table.

## Status: default OFF, and what a flip requires

Both gates, per the house rule that neither alone suffices:

1. **A corpus-scale ΔMISSED arm.** This trades a step bound for a time bound, so a
   fixpoint that would have completed just past its budget now returns `Stalled` —
   a MISS. The `6134` result is +29 rows, but that is one ontology and the sign can
   invert elsewhere.
2. **A full-corpus two-arm sweep.** The MISSED net's frame is drawn from
   *completers*, so it structurally cannot observe an `ok → dnf`.

Canaries: `crates/owl-dl-tableau/tests/fixpoint_deadline_default.rs` (4; all env
rows pinned including `""`). **Sabotage 2 of 2 caught** — dead flag, and the
default-ON idiom. Honest limit: those pin the env plumbing only, not the
mechanism; the mechanism is evidenced by the measurement above.
