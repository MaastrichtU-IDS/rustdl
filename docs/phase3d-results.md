# Phase 3d — apply_deferred_concept_or_rules indexed-lookup hoist results

Run 2026-06-01. Fix: hoist the linear-scan fallback in
`apply_deferred_concept_or_rules` out of the per-trigger loop, behind
a single top-of-function gate on
`tbox.concept_rules_by_trigger.is_empty()`. Finalized TBoxes (the
common case) skip missing-trigger lookups with `continue` instead of
falling through to an O(R) scan of `&tbox.concept_rules`. See
`docs/phase3d-fix-target.md` for the design + soundness invariant
(verified against `crates/owl-dl-core/src/absorb.rs:110-119` where
the index is populated) and `docs/phase3d-recon.md` for the recon.

## Headline finding

**GALEN classify wall dropped from 12.43 min (post-3c clean baseline)
to 11.87 min (post-3d clean) — −4.5% wall reduction.** SIO
`apply_deferred_concept_or_rules` top-frame attribution dropped from
**18.16% → 3.23%** (−14.93pp). FP=0 + MISSED=17 unchanged on GALEN;
FP=0 + MISSED=0 unchanged on the Phase 0 soundness net.

The recon-identified bottleneck — the per-trigger `else` fallback at
`rules.rs:577-593` doing an O(R) `&tbox.concept_rules` scan whenever
`concept_rules_by_trigger.get(trigger)` returned `None` (~96% of the
18.16% frame, 2,713 / 2,838 samples) — is eliminated. The
`eq<ConceptRule>` and `next<ConceptRule>` child frames (8.81% + 8.56%
of pre-3d) are gone from the post-3d flame.

## Soundness gate (Phase 0 net)

| Fixture | Pre-3d FP | Post-3d FP | Pre-3d MISSED | Post-3d MISSED | Wall (Post-3d) |
|---|---|---|---|---|---|
| alehif | 0 | 0 | 0 | 0 | ~2.7s (in 30s net) |
| ore-10908-sroiq | 0 | 0 | 0 | 0 | ~25.9s (in net) |
| ore-15672-shoin | 0 | 0 | 0 | 0 | ~29.9s (in net) |

FP=0 / MISSED-unchanged held across the Phase 0 soundness net.

## Wall lever (GALEN)

| Stage | Wall | MISSED | FP |
|---|---|---|---|
| Pre-Phase-3d (post-3c, T2 baseline) | 12.43 min (745.57s) | 17 | 0 |
| Post-Phase-3d (clean) | 11.87 min (711.93s) | 17 | 0 |
| Delta | **−0.56 min (−4.5%)** | — | — |

**Note on contention noise:** The first post-3d GALEN measurement
(850.88s = +14%) ran concurrent with the SIO flamegraph capture
(`cargo build --features profile` + 60s pprof sample). The clean
re-measure (711.93s, this row) is the durable number; the +14%
contention reading is discarded as system noise.

## Flame delta (SIO, post-3c → post-3d)

| Frame | Post-3c | Post-3d | Δ |
|---|---|---|---|
| `apply_deferred_concept_or_rules` (top variant) | **18.16%** | **3.23%** | **−14.93pp** |
| `eq<ConceptRule>` (under apply_deferred…) | 8.81% | (gone from top frames) | −8.81pp |
| `next<ConceptRule>` (under apply_deferred…) | 8.56% | (gone from top frames) | −8.56pp |

The frame still appears at multiple call sites in the post-3d flame:
top three at 577 / 89 / 382 samples (3.23% / 0.50% / 2.14%);
total ~1048 samples = ~5.87% summed (vs the 18.16% single-top in the
post-3c flame). The summed residual (5.87%) reflects clean inner work
in the indexed path, not the eliminated scan.

Post-3d flame: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3d.svg`.

## What this fix does

`apply_deferred_concept_or_rules` previously iterated triggers and, for
each trigger, looked up `concept_rules_by_trigger.get(trigger)`. On a
hit it processed the indexed rules; on a miss it fell through to a
linear scan of `&tbox.concept_rules` to handle rules that had not yet
been indexed (the pre-finalization path).

For finalized TBoxes — the common case — every rule lives in the
index, so the fallback was pure overhead: O(R) per trigger × T triggers
= O(T·R) per call. The recon attributed 96% of the 18.16% frame to
that fallback (2,713 / 2,838 samples).

Fix: hoist the index-empty check out of the loop. When
`concept_rules_by_trigger.is_empty()` is false, missing triggers
`continue` instead of falling through to the scan. The legacy scan
path is preserved for the pre-finalization case (when the index is
genuinely empty), so behavior on partially-built TBoxes is unchanged.

Soundness: the index is populated by `absorb.rs:110-119` to be
complete with respect to `concept_rules` (every rule with a trigger
ends up indexed). When the index is non-empty, a missing-trigger
lookup is genuinely "no rule fires for this trigger" — equivalent to
the empty result the scan would have produced. Verdicts unchanged
across all workspace tests + Phase 0 net + GALEN.

## What's left

- `apply_role_rules` (post-3c: 16.36%) — Phase 3e candidate, untouched.
- `apply_max` (post-3c: 14.34%) — already-attacked by Phase 3b (inverse-role
  lookup); residual is less structural / more fundamental.
- `from_iter / collect` heap-alloc cluster (post-3c: 6.51%) — Phase 3e
  secondary target.
- Residual `apply_deferred_concept_or_rules` (3.23% top variant, ~5.87%
  summed across call sites) — clean inner work, no obvious further
  optimization without restructuring per-node iteration.

## Cross-references

- Phase 3d plan: `docs/superpowers/plans/2026-06-01-phase3d-apply-deferred-concept-or-rules.md`
- Phase 3d recon: `docs/phase3d-recon.md` (commit 4ef3e4b)
- Phase 3d design: `docs/phase3d-fix-target.md` (commit c8cd0f7)
- Phase 3d implementation: `crates/owl-dl-tableau/src/rules.rs:546-617` (commit 32aeda6)
- Phase 3c results (prior baseline): `docs/phase3c-results.md`

## How to re-run

```bash
# Soundness net (FP=0 gate):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN wall (clean run; avoid concurrent profile capture):
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture
```
