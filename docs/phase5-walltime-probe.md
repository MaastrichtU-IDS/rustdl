# Phase 5 T2 — wall-time probe rules out the saturator as the GALEN regression source

Run 2026-06-02 at HEAD `e7b415f`. Temporary `Instant::now()` instrumentation
added to `crates/owl-dl-saturation/src/lib.rs` (saturate total + Phase 2d
inherit loop + Phase 2d push_fact subs propagation + Phase 2c-redux inner
loop, all reverted after probe). Companion to `docs/phase5-recon.md` (which
showed CPU flame couldn't see this regression because saturation is
single-threaded and contributes <1% of multi-core samples on SIO/GALEN).

## Probe results (GALEN, post-2d+2c-redux on HEAD e7b415f)

```
saturate total              = 0.991 s
phase2d_inherit_loop        = 0.009 s   (process_subsumer's facts_by_sub[d] iter)
phase2d_push_fact_subs      = 0.022 s   (push_fact's recursive subs walk)
phase2c_redux_inner         = 0.016 s   (process_fact witness-merge inner loop)
facts_inherited counter     = 25,702
sub_role_propagations       = 2,515
push_fact_calls             = 228,668
GALEN classify wall total   ≈ 13.36 min = ~802 s
```

**Saturate accounts for 0.12% of total classify wall.** All Phase 2d +
2c-redux propagation work combined is **47 ms** (5.7% of saturate, 0.006%
of total wall).

## Localization

**The +6.5% GALEN wall regression CANNOT be in the saturator.** The
saturator-side propagation costs (47 ms combined) are three orders of
magnitude too small to account for the +49 s wall delta documented in
`docs/phase2d-2c-redux-results.md`.

If the regression is real, it must be downstream of `saturate()` — in
`PreparedOntology::from_internal`, the per-class unsat probes, the
tier-walk in `classify_top_down_internal`, or the entailed-matrix
construction. The plausible mechanism: Phase 2d inflates `facts_by_sub`
which inflates the Subsumers closure (27,980 → 27,997 pairs at the
atomic level is small, but the inherited facts cascade through CR5/CR9
to produce derived subsumer pairs that change tier ordering or
parallel-walk work distribution).

## §16 risk classification of a hypothetical fix

If a downstream probe localizes the regression to e.g. the tier-walk
under inflated Subsumers, the fix shape would be **workload-dependent**:
GALEN's many-classes + dense-subsumer pattern would benefit, but
edge-light ontologies might regress slightly. §16-shape risky.

## What this rules out

- Saturator inner loops: 0.047 s ≪ 49 s.
- Phase 2d's `push_fact` recursion: 0.022 s for 228,668 calls = 96 ns/call. Fast.
- Phase 2c-redux's inner-loop iteration over `facts_by_sub[X]`: 0.016 s
  across 2,515 emissions = 6 µs/emission. Fast.

## What this does NOT rule out

- **The regression may not be real** (single-sample noise). Same shape
  as the Anonymous-349 closure-realization anomaly: T7 was measured
  concurrent with SIO flame + GALEN parallel; T2 baseline was measured
  clean. Rayon scheduling between runs could plausibly account for
  ±5–10% wall variance on this machine.
- **The regression may be downstream**: PreparedOntology / tier-walk /
  entailed-matrix construction work could have grown via Subsumers
  cascade. Would need a second instrumentation pass on
  `classify_top_down_internal`.

## Recommended next step (if pursuing further)

Re-run the GALEN closure-diff test 3 times standalone (no concurrent
load) on commit `34a2b62` (post-2d+2c-redux) and 3 times on `aab6d03`
(pre-2d). Compare medians. If the median delta is < 3%, the +6.5%
T7 reading was single-sample noise; close out. If the median delta
holds at +5% or more, instrument `PreparedOntology::from_internal`
and `classify_top_down_internal` to localize downstream.

Cost: ~4–6 hours of wall time (6 × ~30 min GALEN runs). May not be
worth pursuing given the +6.5% trade was explicit in the Phase 2d +
2c-redux ship decision and the recovery upside (~50 s on a 13 min
classify) is small.

## Cross-references

- Phase 5 recon (CPU flame failure mode):
  `docs/phase5-recon.md`.
- Phase 2d design:
  `docs/phase2d-design.md`.
- Phase 2d + 2c-redux results (where +6.5% was first measured):
  `docs/phase2d-2c-redux-results.md`.
- Anonymous-349 closure-realization anomaly (same single-sample
  concurrency hypothesis):
  `docs/phase2e-notgalen-diagnosis.md` Addendum.
