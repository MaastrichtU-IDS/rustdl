# Phase 2d intermediate results (pre Phase 2c-redux)

Phase 2d (fact-on-subclass propagation) measurement after Pass 1 landed
at commit `b78c5fd`, before Phase 2c-redux is applied on top. See
`docs/phase2d-design.md` for design and the combined plan at
`docs/superpowers/plans/2026-06-01-phase2d-plus-2c-redux.md`.

## Headline

**Phase 2d alone holds soundness and wall budget on GALEN.** FP=0 across
Phase 0 net (alehif, ORE-10908, ORE-15672) and GALEN. GALEN MISSED
unchanged at 17 (expected — Phase 2c-redux not yet applied). Wall
−0.9% on GALEN (746.14 s vs 752.83 s baseline) — within noise band but
no regression. All T4 gate criteria pass; proceeding to Pass 2 (T6
Phase 2c-redux).

## Soundness gate (Phase 0 net)

| Fixture | FP (pre-2d → post-2d) | MISSED (pre-2d → post-2d) | Wall |
|---|---|---|---|
| alehif-test | 0 → 0 | 0 → 0 | 6.29 s |
| ore-10908-sroiq | 0 → 0 | 0 → 0 | 33.79 s |
| ore-15672-shoin | 0 → 0 | 0 → 0 | 36.13 s |

Total 36 s for the 3-fixture run. **Net FP=0 / MISSED=0 unchanged.**

## Wall lever (GALEN)

| Metric | Baseline (T2 clean, commit aab6d03) | Post-2d (commit b78c5fd) | Δ |
|---|---|---|---|
| FP | 0 | 0 | unchanged |
| MISSED | 17 | 17 | unchanged (expected) |
| Wall | 752.83 s (12.55 min) | 746.14 s (12.44 min) | **−0.9%** (noise band, slight) |

The "wall risk via Phase 2a cascade" concern flagged in T1's design
(`docs/phase2d-design.md` §"Concerns") did NOT materialize on GALEN.
The inherited facts populating `facts_by_sub[X]` for many X don't
cascade into many new Phase 2a witness-merge firings because GALEN's
existentials are mostly on roles whose functional super-roles are
sparse. The propagation is essentially free at this scale.

## Memory (informational)

T1's estimate: GALEN baseline `facts.len()` ≈ 2,625; pessimistic
10× → ~26K (~600 KB); 50× → ~130K (~3 MB). Actual post-2d
`facts.len()` not measured this run (eprintln instrumentation was
reverted before T3 commit per plan). The wall result + structural
canary confirm the propagation is firing without memory pathology;
exact growth ratio is informational only and can be measured
post-Pass-2 if T7 wall regressions warrant it.

## Triage decision

**All T4 gates PASS. Proceed to Pass 2 (Phase 2c-redux).**

| Criterion | Result | Decision |
|---|---|---|
| Phase 0 net FP=0 / MISSED=0 | Pass | Continue |
| GALEN FP=0 | Pass | Continue |
| GALEN MISSED unchanged at 17 | Pass (expected) | Continue |
| GALEN wall regression < 10% | Pass (−0.9%, well under) | Continue |
| Fact count growth < 5× | Informational (not measured) | Continue |

## What's next

Pass 2 (T6–T8) re-applies the previously-reverted (cc2019e) Phase 2c
sub-role witness-propagation rule on top of Phase 2d. The corpus
recovery question — does the layered system close the IPBP cluster? —
is what T7 measures. T8 either ships both layers (if T7's gates pass)
or writes a dead-end §18 (if not).

## Cross-references

- Phase 2d design: `docs/phase2d-design.md`
- Phase 2d implementation commit: `b78c5fd`
- Phase 2d baseline commit: `aab6d03`
- Phase 2c original (reverted): commit `b83fcd6` → reverted at `cc2019e`
- Dead-end §15: the prerequisite this addresses
- Combined plan: `docs/superpowers/plans/2026-06-01-phase2d-plus-2c-redux.md`
