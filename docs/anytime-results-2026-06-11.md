# Anytime-under-deadline results — Phase 1 (per-pair deadline)

Empirical evidence for **sound anytime OWL classification with calibrated
incompleteness**. This is Phase 1: the deadline is *per-pair*
(`--pair-timeout-ms`); a subsumption probe that exceeds it defaults to "not
subsumed" and is recorded in `Classification::undecided_pairs()`. Metrics are
computed against the HermiT/Konclude oracle closure (the same alignment the
`konclude_closure_diff` tests use — excluding reflexive, unsatisfiable, and
`owl:Thing`-equivalent pairs from both sides).

Reproduce:
```
RUSTDL_ANYTIME_CSV=docs/anytime-results-2026-06-11.csv \
  cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- \
  --ignored --nocapture anytime_per_pair_sweep
```
Raw data: `docs/anytime-results-2026-06-11.csv`. Deadlines capped at 100 ms
(see "The wall caveat" below). All numbers transcribed from one sweep run
(2026-06-11); `wall_ms` is host-load-dependent, the rest are deterministic.

## Definitions

- **precision** = reported∩true / reported — soundness. Must be 1.0 (FP=0).
- **recall** = reported∩true / true — completeness.
- **silent_miss** = |missed \ undecided| — calibration. Must be 0 (every missed
  subsumption is flagged undecided, never silently dropped).

## Results

| fixture | true_pairs | deadline | recall | precision | silent_miss | wall (ms) | undecided |
|---|---:|---:|---:|---:|---:|---:|---:|
| galen | 27997 | 5 ms | 1.0000 | 1.0000 | 0 | 473 | 0 |
| galen | 27997 | 25 ms | 1.0000 | 1.0000 | 0 | 480 | 0 |
| galen | 27997 | 100 ms | 1.0000 | 1.0000 | 0 | 472 | 0 |
| alehif | 247 | 5 ms | 1.0000 | 1.0000 | 0 | 245 | 0 |
| alehif | 247 | 25 ms | 1.0000 | 1.0000 | 0 | 297 | 0 |
| alehif | 247 | 100 ms | 1.0000 | 1.0000 | 0 | 306 | 0 |
| sio | 8904 | 5 ms | 1.0000 | 1.0000 | 0 | 6453 | 7916 |
| sio | 8904 | 25 ms | 1.0000 | 1.0000 | 0 | 16232 | 5756 |
| sio | 8904 | 100 ms | 1.0000 | 1.0000 | 0 | 24603 | 620 |
| wine | 653 | 5 ms | 1.0000 | 1.0000 | 0 | 13590 | 8250 |
| wine | 653 | 25 ms | 1.0000 | 1.0000 | 0 | 54002 | 8240 |
| wine | 653 | 100 ms | 1.0000 | 1.0000 | 0 | 205000 | 8152 |
| ore-10908 | 6001 | 5 ms | 1.0000 | 1.0000 | 0 | 602 | 649 |
| ore-10908 | 6001 | 25 ms | 1.0000 | 1.0000 | 0 | 1042 | 59 |
| ore-10908 | 6001 | 100 ms | 1.0000 | 1.0000 | 0 | 843 | 0 |
| ore-15672 | 142 | 5 ms | 1.0000 | 1.0000 | 0 | 1763 | 125 |
| ore-15672 | 142 | 25 ms | 1.0000 | 1.0000 | 0 | 4555 | 105 |
| ore-15672 | 142 | 100 ms | 1.0000 | 1.0000 | 0 | 14963 | 105 |

## Headline findings

1. **Sound at every deadline (precision = 1.0 everywhere).** Across all 18
   (fixture, deadline) points the reported hierarchy contained zero false
   subsumptions. This is also enforced as a hard assertion in the harness
   (`assert_eq!(fp, 0)`), so the sweep doubles as a soundness stress test of the
   anytime path — it passed.

2. **Calibrated (silent_miss = 0 everywhere).** No true subsumption was ever
   *silently* dropped: every miss is in the flagged-undecided set. In this run
   recall is 1.0 at every point, so there were no misses to flag; an earlier
   uncapped run observed `wine @ 5 ms` recall = 0.9985 (one true subsumption
   missed) with **silent_miss = 0** — i.e. that single miss appeared in
   `undecided_pairs()`, demonstrating calibration under an actual miss. The
   reasoner always knows what it does not know.

3. **Per-pair deadline costs *certainty about non-subsumptions* and *wall*, not
   recall.** On all six fixtures the *sound* subsumptions are decided by cheap
   channels (told-subsumer + EL/Horn saturator + fast wedge refutation), so full
   recall holds even at 5 ms/pair. What the deadline changes is (a) how many
   *non*-subsumptions are positively confirmed vs left flagged-undecided —
   `sio` undecided falls 7916 → 620 and `ore-10908` 649 → 0 as the budget grows
   — and (b) total wall. The recall-vs-budget tradeoff the anytime story needs
   is therefore a property of a *global* budget, measured in Phase 2 (a tight
   total deadline cuts the tier walk short before all classes are confirmed);
   the per-pair budget does not degrade recall here.

4. **EL baseline is free (galen, alehif).** galen (pure EL, 27997 pairs) and
   alehif classify fully at the smallest budget with 0 undecided and identical
   wall across deadlines — anytime adds zero overhead on inputs the saturator
   handles.

5. **`ore-15672` undecided plateaus at 105.** Beyond 25 ms the undecided count
   stops falling (125 → 105 → 105): these are the fundamentally-hard
   non-deterministic (SHOIN, ≤n/nominal) non-subsumption pairs characterized
   elsewhere (`docs/superpowers/specs/2026-06-10-global-model-rewrite-design.md`)
   — dead-stable regardless of budget. The reasoner correctly leaves them
   flagged-undecided rather than guessing; recall is unaffected (they are
   non-subsumptions).

## The wall caveat (why 100 ms, and the Phase-2 motivation)

The per-pair deadline does **not** bound total wall: each of thousands of
candidate pairs may burn the full budget. `wine @ 100 ms` takes **205 s**
(8152 pairs that find nothing, each allowed up to 100 ms), and 250/1000 ms would
run for tens of minutes while recall is already saturated. The sweep is capped
at 100 ms for this reason. This unbounded-total-wall behaviour is exactly the
motivation for a **global wall-clock deadline** (Phase 2): a user wants "give me
the best sound hierarchy you can in T seconds," which per-pair budgeting cannot
express.

## Caveat on the `undecided` column

`undecided` counts every timed-out probe pair, including pairs involving classes
excluded from the oracle comparison (`owl:Thing`-equivalent / unsatisfiable
classes). So `undecided` may slightly exceed the number of undecided pairs among
*compared* classes. `silent_miss` is unaffected — it is computed over the
compared (aligned) set only.
