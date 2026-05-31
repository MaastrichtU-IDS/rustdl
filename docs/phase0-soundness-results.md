# Phase 0 — broadened-corpus soundness diff results

Run on 2026-05-31 against the Phase 0 fixtures
selected per `docs/phase0-corpus-candidates.md` and wired into
`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` (Task 4).
Oracle: ROBOT v1.9.6 + HermiT (sound + complete) via
`docker/robot/classify-oracle.sh`.

## Results

| Fixture | Expressivity | Classes | Wall | FP | MISSED | Outcome |
|---|---|---|---|---|---|---|
| ore-10908-sroiq | SROIQ | 693 | 34.69 s | 0 | 0 | PASS |
| ore-15672-shoin | SHOIN | 83 | 35.36 s | 0 | 0 | PASS |

## Soundness envelope

Both new ORE fixtures passed with FP=0 and MISSED=0 against the HermiT oracle.
The broadened corpus envelope now holds across the two new expressivity fragments
(SROIQ with inverse roles, complex role hierarchies, and qualified cardinality
restrictions; SHOIN with inverse roles, role hierarchy, and unqualified
cardinality restrictions), in addition to the pre-existing pizza / RO / SULO /
SIO / GALEN / notGALEN / alehif set documented in
`docs/hypertableau-summary.md` §2. No soundness bugs were surfaced by this run.

## Filed FPs (none if FP=0)

None. FP=0 across both fixtures.

## How to re-run

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- ore_10908_sroiq ore_15672_shoin \
    --ignored --nocapture
```

Both fixtures are gitignored; provision per `docs/phase0-corpus-candidates.md`
+ Task 3 of `docs/superpowers/plans/2026-05-31-phase0-soundness-net.md`.
