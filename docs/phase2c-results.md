# Phase 2c results — sub-role witness propagation

Run 2026-06-01. See `docs/phase2c-fix-target.md` for the design
analysis and `docs/phase2c-galen-diagnosis.md` for the prior diagnosis.

## Headline

**0 / 44 predicted corpus pairs recovered; rule reverted at commit
cc2019e.** Sound (FP=0 held across the Phase 0 net + GALEN + notgalen)
and terminating (Phase 2a canaries pass; the 4-fan-in structural canary
bumped the counter). Cost was a 7% wall regression on GALEN and
notgalen, paid every classify run for zero benefit.

## Prediction vs measurement

| Fixture    | FP (before → after) | MISSED (before → after) | Wall (before → after)        | Predicted recovery |
|------------|---------------------|-------------------------|------------------------------|--------------------|
| alehif     | 0 → 0               | 0 → 0                   | ~few s → ~few s              | n/a (sanity)       |
| ORE-10908  | 0 → 0               | 0 → 0                   | (37.6 s total for net)       | n/a (sanity)       |
| ORE-15672  | 0 → 0               | 0 → 0                   | (in net)                     | n/a (sanity)       |
| GALEN      | 0 → 0               | 17 → **17 unchanged**   | 12.2 min → **13.1 min (+7%)** | 12 floor / 17 likely / 22 upper |
| notgalen   | 0 → 0               | 27 → **27 unchanged**   | ~30 min → **32.1 min (+7%)** | 12 floor / 22 likely / 22 upper |
| **Total**  | **0 → 0**           | **44 → 44 unchanged**   | —                            | **24 / 39 / 44 vs 0** |

Phase 2c.0 prediction band: 24 confident floor / 39 most-likely / 44
upper bound. Measured: 0.

## Why the rule didn't fire on the corpus

Per T3's design doc (`docs/phase2c-fix-target.md` §"Predicted walkthrough
on pair_06 (and what actually happened)"): ICF has only one existential
fact directly materialized on `facts_by_sub[ICF]`, not the two T3's
walkthrough assumed. The second fact is on a parent class
(`PathologicalBodyProcess`) and inherits to ICF via subsumer-membership
at `process_subsumer` time — but never lands on `facts_by_sub[ICF]`.
Phase 2c's rule is a fact-time rule; it iterates `facts_by_sub[X]`,
sees only one fact, and (correctly) doesn't fire.

The 11 IPBP-derivation pairs in the GALEN+notgalen MISSED lists all
exhibit this shape (IneffectiveCardiacFunction, LeftIneffectiveCardiac*,
RightIneffectiveCardiac*, Postcardiotomy*, Postvalvulotomy*,
CongestiveCardiacFailure ⊑ IPBP variants).

## Why the rule still fires (3× on pair_06) but recovers nothing visible

The rule fires on classes that DO have two existential facts directly
materialized (ClassId 77/79/114 in pair_06's setup). The emissions are
sound — they propagate the merged synthetic to a sub-role — but the
downstream existential triggers don't have heads on those particular
sub-roles, so no subsumer is added. Counter bumped, no closure delta.

## What this leaves for Phase 2d (if pursued)

The architectural change Phase 2c would have needed: materialize a
subclass's inherited existential facts on `facts_by_sub[subclass]` at
`process_subsumer` time, so fact-time rules see them. This is a
significantly larger change than Phase 2c's strictly-additive scope
(it touches the saturator's core fact/subsumer separation, and risks
both blowup and unsoundness on the propagation path itself). See
`docs/hypertableau-dead-ends.md` §15.

## Cross-references

- Phase 2c plan: `docs/superpowers/plans/2026-06-01-phase2c-functional-role-covering.md`
- Phase 2c design / fix target: `docs/phase2c-fix-target.md`
- Phase 2c.0 diagnosis: `docs/phase2c-galen-diagnosis.md`
- pair_06 canary (gap-asserting, kept): `crates/owl-dl-reasoner/tests/phase2c_pair_06_canary.rs`
- Dead-end ledger entry: `docs/hypertableau-dead-ends.md` §15
- T5 measurement logs: `/tmp/p2c-{net,galen,notgalen}.log` (transient)
- Revert commit: cc2019e
