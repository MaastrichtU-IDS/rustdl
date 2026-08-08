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

## Gate results: NO-GO on the default flip

Both gates ran. The fix is correct and sound; it has **no measured corpus value**.

**Gate 1 (ΔMISSED) is INAPPLICABLE, not passed.** Two arms at default args and two
at `--pair-timeout-ms 50`, 400-ontology population, same pinned sha, env recorded
per arm: ΔMISSED **+0**, FP 0 both sides, in both configurations. But an
instrument-fired check shows **answers identical on all 400 ontologies** in every
arm — and the reason is structural: **none of the six affected ontologies
(`6134`, `12432`, `10080`, `13122`, `6910`, `16056`) are in the population**,
because that frame is drawn from *completers* and all six are DNF-tail members.
The net cannot see this flag. Reporting its `+0` as a pass would have been
reporting an arm that never ran the code.

A second trap on the way: **321 of 400 output files differed byte-wise while
answers were identical.** The banner's timings and `wedge-cost-histogram` are
nondeterministic (already documented elsewhere in the design record). A raw
byte-diff here manufactures 321 phantom regressions.

**Gate 2 (full-corpus two-arm sweep), 1,920 ontologies, cap 60 s, `--threads 1`,
`JOBS=4`:**

| | OFF | ON |
|---|---|---|
| outcomes | 1,751 ok / 167 dnf / 2 err_reject | **identical** |
| recovered `dnf → ok` | — | **0** |
| regressed `ok → dnf` | — | **0** |
| total wall (1,751 completers) | 3,583 s | 3,600 s (**+0.5%**) |
| total peak RSS | 281.6 GB | 281.6 GB (**+0.0%**) |

One ontology looked 2.5× slower (`ore_ont_5755`, 1.47 → 3.66 s); min-of-3 puts it
at **1.35 vs 1.36 s** — sweep noise, not a regression.

**The documented small-budget case shows nothing either.** `wine` at
`--pair-timeout-ms 25` — the configuration the CLI's own help text recommends, and
the one this fix should most help — is **204 rows in both arms**, 5.38 s vs 5.51 s
(min-of-3). So the "budgets below the overshoot scale are silently not honoured"
consequence, while true of the histogram, does not translate into a measurable
wine effect.

### Verdict

**Default stays OFF.** The lever is sound, tested, and demonstrably makes the
per-pair budget enforceable — the `ore_ont_6134` overshoot buckets go to zero, and
that run decides 3.1× more pairs and finds +29 more subsumptions. But that effect
required a *specific* configuration (`--pair-timeout-ms 50` + an aggregate
label-cache bound + a global timeout) on an ontology that **still DNFs**, so the
gain never reaches a user. Corpus-wide at default settings the flag is inert, and
flipping it ON would add a strided clock read to a hot loop for no measured
return.

Kept as an opt-in because the *defect* it fixes is real and the fix is
FP-safe by construction; if a future workload makes small per-pair budgets
load-bearing, this is the switch that makes them honest.
