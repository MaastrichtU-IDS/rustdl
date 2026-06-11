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

---

# Phase 2 — global wall-clock deadline

A single shared budget `T` for the whole classification (`classify_with_global_deadline`).
Unconfirmed pairs at `T` (in-flight, not-yet-reached, and the label-cache build
past `T`) are reported "not subsumed" and flagged undecided. Reproduce:
```
RUSTDL_ANYTIME_CSV=docs/anytime-results-2026-06-11.csv \
  cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- \
  --ignored --nocapture anytime_global_sweep
```

| fixture | true_pairs | budget | recall | precision | silent_miss | wall (ms) | undecided |
|---|---:|---:|---:|---:|---:|---:|---:|
| galen | 27997 | 100 ms | 1.0000 | 1.0000 | 0 | 473 | 0 |
| galen | 27997 | 1 s | 1.0000 | 1.0000 | 0 | 467 | 0 |
| galen | 27997 | 10 s | 1.0000 | 1.0000 | 0 | 467 | 0 |
| galen | 27997 | 30 s | 1.0000 | 1.0000 | 0 | 469 | 0 |
| alehif | 247 | 100 ms | 1.0000 | 1.0000 | 0 | 854 | 14554 |
| alehif | 247 | 1 s | 1.0000 | 1.0000 | 0 | 283 | 0 |
| alehif | 247 | 10 s | 1.0000 | 1.0000 | 0 | 291 | 0 |
| alehif | 247 | 30 s | 1.0000 | 1.0000 | 0 | 286 | 0 |
| sio | 8904 | 100 ms | 0.9996 | 1.0000 | 0 | 3090 | 1193164 |
| sio | 8904 | 1 s | 0.9996 | 1.0000 | 0 | 3472 | 1193164 |
| sio | 8904 | 10 s | 1.0000 | 1.0000 | 0 | 11429 | 47169 |
| sio | 8904 | 30 s | 1.0000 | 1.0000 | 0 | 20876 | 4 |
| wine | 653 | 100 ms | 0.9678 | 1.0000 | 0 | 373 | 15783 |
| wine | 653 | 1 s | 0.9678 | 1.0000 | 0 | 1273 | 15783 |
| wine | 653 | 10 s | 0.9678 | 1.0000 | 0 | 10274 | 15783 |
| wine | 653 | 30 s | 0.9678 | 1.0000 | 0 | 30274 | 15783 |
| ore-10908 | 6001 | 100 ms | 1.0000 | 1.0000 | 0 | 262 | 213583 |
| ore-10908 | 6001 | 1 s | 1.0000 | 1.0000 | 0 | 868 | 0 |
| ore-10908 | 6001 | 10 s | 1.0000 | 1.0000 | 0 | 863 | 0 |
| ore-10908 | 6001 | 30 s | 1.0000 | 1.0000 | 0 | 899 | 0 |
| ore-15672 | 142 | 100 ms | 1.0000 | 1.0000 | 0 | 133 | 4334 |
| ore-15672 | 142 | 1 s | 1.0000 | 1.0000 | 0 | 1035 | 4334 |
| ore-15672 | 142 | 10 s | 1.0000 | 1.0000 | 0 | 10038 | 4334 |
| ore-15672 | 142 | 30 s | 1.0000 | 1.0000 | 0 | 30037 | 4334 |

## Phase 2 findings

1. **Still sound (precision = 1.0) and calibrated (silent_miss = 0) at every
   budget** — including where recall < 1.0. At `wine @ 100 ms` rustdl returns a
   96.8%-complete hierarchy and flags **all 21** unconfirmed true subsumptions as
   undecided; not one is silently dropped. This is the central claim, now shown
   under genuine incompleteness (the per-pair phase never dropped recall).

2. **The global budget produces the real recall curve.** Unlike the per-pair
   deadline (recall flat at 1.0 — sound subsumptions are cheap-channel-decided),
   a tight *total* budget cuts the tier walk short: `sio` recall 0.9996 → 1.0 as
   `T` grows 100 ms → 30 s, and the undecided set shrinks 1.19 M → 4. The
   reasoner trades coverage for time while never sacrificing soundness.

3. **`wine` plateaus at recall 0.9678 through 30 s.** ~96.8% of wine's hierarchy
   is confirmed almost instantly (cheap channels); the last 21 subsumptions
   require expensive probes that do not complete within 30 s of wall (the
   per-pair phase reached recall 1.0 only by spending ~205 s of total probe
   time). They stay flagged-undecided — the calibrated-incompleteness guarantee
   holding on the known-hard `wine` SROIQ wall.

# Konclude all-or-nothing contrast

A complete reasoner returns nothing until it finishes; rustdl returns a sound
partial hierarchy at any budget. Konclude classification wall (`W_k`) per
fixture — galen/alehif measured fresh here (native `Konclude v0.7.0-1138`,
`classification -w AUTO`); the rest cited from
`docs/perf-2026-06-08-konclude-vs-rustdl.md` (same native binary, robot-converted
`.owx` inputs — Konclude's parser rejects our RDF/XML `.owl` and OFN files, and
`robot` is unavailable on this host to reconvert):

| fixture | Konclude `W_k` (wall) | Konclude reasoning | rustdl recall at 100 ms / 1 s |
|---|---:|---:|---|
| galen | 0.27 s | 17 ms | 1.0000 / 1.0000 |
| alehif | 0.19 s | 1 ms | 1.0000 / 1.0000 |
| sio | 0.24 s | 59 ms | 0.9996 / 0.9996 |
| ore-10908 | 0.08 s | 23 ms | 1.0000 / 1.0000 |
| ore-15672 | 0.04 s | 5 ms | 1.0000 / 1.0000 |
| wine | 0.13 s | 33 ms | 0.9678 / 0.9678 |

## The honest verdict (this is the paper's motivating-application risk, in data)

**On this corpus the anytime contrast does not favour rustdl: Konclude classifies
every fixture in well under one second.** rustdl's own setup (parse + saturate +
prepare, ≈0.5–2 s) is already slower than Konclude's *complete* answer, so there
is **no deadline window** in which rustdl's sound-partial output beats Konclude's
complete output — Konclude wins outright on wall, and on these inputs it is also
complete where rustdl (out-of-EL) is not.

What the experiment *does* establish, unconditionally and corpus-wide, is the
**guarantee**: at any deadline rustdl's reported hierarchy is sound (precision
1.0 across all 42 measured points — 18 per-pair + 24 global, FP gate enforced) and
calibrated (silent_miss 0 — every miss flagged), with recall degrading gracefully
under a tight global budget. That
guarantee is real; its *value* simply does not show against a state-of-the-art
reasoner on benchmarks this reasoner finishes in milliseconds.

**Implication for the paper.** The anytime contribution needs a motivating
setting the standard ORE/benchmark corpus does not provide: either (a) workloads
where *complete* classification is genuinely expensive for the best available
reasoner (very large or pathological ontologies, streaming/partial inputs), or
(b) deployment contexts where a complete reasoner is unavailable (embedded /
EL-only / no-JVM, where rustdl's cold-start and footprint win — see the
embeddability analysis). Absent such a setting, "sound anytime classification"
is a correct property without a benchmark that rewards it. This is the #1 risk to
state and address head-on, not paper over.
