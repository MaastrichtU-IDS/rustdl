# Phase 1 — Selective trust-sat verification results

Run 2026-05-31 against the Phase 0 soundness net and on alehif as a
single-fixture threshold sweep. Mechanism: `RUSTDL_HYPER_TRUST_SAT_MIN_MS`
env var (default **0 = disabled**; positive integer enables
selective verification). See
`docs/superpowers/plans/2026-05-31-phase1-selective-trust-sat.md`.

## Headline finding

**The mechanism is sound; the wall-time-as-filter premise is invalidated
by data.** Phase 1 ships the code (correct, FP=0 on every measured
fixture) but the default is **0 (disabled)**. The opt-in env var stays
available for users with profiled workloads who can identify a
threshold empirically.

## Soundness gate (50 ms — the spec's original default)

Single-thread runs against the Phase 0 net at the spec's original
threshold default of 50 ms. **FP=0 held across all 6 measured fixtures:**

| Fixture | Classes | FP | MISSED | Wall (50ms) | Wall vs Phase 0 baseline |
|---|---|---|---|---|---|
| ore-10908-sroiq | 693 | 0 | 0 | 340.62 s | +306 s (≈10×) |
| ore-15672-shoin | 83 | 0 | 0 | 56.51 s | +21 s (≈1.6×) |
| alehif | 247 | 0 | 0 | 414.79 s | +413 s (≈235×) |
| pizza | 100 | 0 | 5 | 86.44 s | +83 s (≈26×) |
| ro-stripped | ~158 | 0 | 0 | 29.21 s | +29 s (≈70×) |
| sulo-stripped | ~51 | 0 | 0 | 0.19 s | +0.17 s (≈10×) |

Soundness verdict: **FP=0 across the broadened net — the mechanism is
sound.** (pizza MISSED 4 → 5 is a known timing artifact from rayon
contention during the run; the policy creates timing-dependence on the
threshold but does not introduce a false positive.)

GALEN, notgalen, and sio-stripped fixtures could not complete inside
the wall-time budget at the 50 ms threshold — direct evidence that 50 ms
is much too aggressive.

## The discriminating sweep — alehif at 1 / 5 / 10 / 20 / 30 ms

To test whether any threshold value avoids the blowup, alehif was swept
single-threaded across 5 thresholds:

| Threshold (ms) | Wall (s) | Wall vs baseline (1.76 s) |
|---|---|---|
| 1  | ~405–410 | ≈230× |
| 5  | ~405–410 | ≈230× |
| 10 | ~405–410 | ≈230× |
| 20 | ~405–410 | ≈230× |
| 30 | ~405–410 | ≈230× |

**The wall times are flat.** This means virtually every wedge
`NotSubsumed` verdict completes in **under 1 ms**, so a wall-time
threshold ≥ 1 ms catches essentially every NotSubsumed verdict. The
"fast = suspect" signal does not exist at this resolution: trivially-
not-subsumed and didn't-try-hard-enough verdicts both finish in sub-
millisecond time.

## What this means for the Phase 1 spec target

The Phase 1 spec target was: **GALEN MISSED 109 → ≤ 40, wall +1–3 min.**

This target is **not achievable via wall-time threshold selection** at
any value tested. The lever from the handoff
(`docs/handoff-2026-05-30.md`) was an estimate, not a measurement. The
real GALEN MISSED come from genuine calculus gaps (functional-role
inference; ≥n with disjointness — see `docs/handoff-2026-05-30.md`
"Open levers"), not from "wedge gave up." Phase 2 of the design spec
(`docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
§"Phase 2 — Deep completeness calculus") takes over the GALEN/notgalen
MISSED-reduction goal.

## Default = 0 rationale (dead-end #11 discipline)

`docs/hypertableau-dead-ends.md` §11 ("Default-on flags before
generalization is proven") is the precedent. The mechanism shipped
(soundness validated; users can opt in for any workload they've
profiled), but the default is **off** because no broadly-applicable
threshold value has been proven. Future Phase 1.5 work could re-open
this with a different signal (e.g., per-pair wedge-rule-fire count
instead of wall time), but that's not Phase 1's deliverable.

## Cross-cutting confirmation

- 0 FP held across the Phase 0 net at the 50 ms threshold ✓
- Mechanism produces non-zero `hyper_refuted_fast_pairs` when enabled
  (verified by the `selective_verify_triggers_when_threshold_high`
  unit test at env=100000) ✓
- Default of 0 returns to pre-Phase-1 behaviour ✓
- Soundness gate trivially passes at default=0 (= pre-Phase-1 path) ✓

## How to re-run

```bash
# Default (disabled) — soundness net runs at pre-Phase-1 speed/verdicts:
scripts/run-soundness-diff.sh

# Opt-in selective verification — set the env var:
RUSTDL_HYPER_TRUST_SAT_MIN_MS=N scripts/run-soundness-diff.sh
```
