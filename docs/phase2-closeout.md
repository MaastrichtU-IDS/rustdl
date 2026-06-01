# Phase 2 — close-out summary

Consolidation across the Phase 2 arc (commits 13dc25d..b588b3a on
branch `plan/soundness-completeness-perf`, 2026-05-31 → 2026-06-01):

- Phase 2a: EL++ functional-role witness-merge rule (atom-set design).
- Phase 2b.0: GALEN MISSED diagnosis (8-pair sample, 5 cluster analysis).
- Phase 2b + 2b.5: compound existential-body lowering fix (two-stage).

## Final corpus state

| Fixture | Baseline MISSED | Final MISSED | Recovered | FP |
|---|---|---|---|---|
| galen | 109 | **17** | **92 (~84%)** | 0 |
| notgalen | 27 | 27 | 0 | 0 |
| alehif | 0 | 0 | (no MISSED to recover) | 0 |
| ore-10908-sroiq | 0 | 0 | (no MISSED to recover) | 0 |
| ore-15672-shoin | 0 | 0 | (no MISSED to recover) | 0 |

**Total Phase 2 recovery: 92 of 136 candidate MISSED (~68%), FP=0
throughout. Soundness preserved across the broadened Phase 0 net.**

## What landed in each sub-phase

**Phase 2a — functional-role witness-merge (commits 13dc25d..f61e06a):**
- Implemented EL++ rule for sibling sub-properties of functional super-role.
- Atom-set redesign (T4.5) replaced T4's synthetic-id tracking after T5
  canary exposed non-termination on 3+ property fan-in.
- Corpus impact: 0 GALEN MISSED recovered (the handoff's trace described
  a different shape than GALEN's actual MISSED).
- Status: shipped (sound, terminating, 4 canaries pass); empirical
  measurement falsified the corpus claim — handed to Phase 2b.0.
- Deliverables: `docs/phase2a-results.md`, `docs/hypertableau-dead-ends.md` §14.

**Phase 2b.0 — GALEN MISSED diagnosis (commits e871e13..dbf1782):**
- 109-pair full MISSED list extracted; 8-pair stratified sample built.
- HermiT-verified minimal modules for each sampled pair.
- Per-pair derivation analysis identified 3 rule shapes across 5 clusters.
- Empirically falsified the spec's `≥n + disjointness` Phase 2b target
  (zero cardinality + zero disjointness axioms in any sampled module).
- Recommended Phase 2b scope reversal: BUG FIX in existing code, NOT a
  new calculus rule.
- Deliverables: `docs/phase2b-galen-diagnosis.md`, `phase2b-galen-pair-analysis.md`,
  `phase2b-galen-sample.md`, `phase2b-galen-missed-pairs.txt`, 8 fixture
  modules + HermiT classifications.

**Phase 2b + 2b.5 — compound existential-body fix (commits 0d8564a..b588b3a):**
- Two complementary saturator-lowering fixes; both use two-way marker
  semantics (`introduce_equivalent_existential_marker`).
- Recovery split: P2b body-side fix recovered 5/92; P2b.5 LHS-And RHS-
  existential fix recovered the remaining 87/92.
- The trace-before-extend pattern (T6 measure 5/60 → trace2 → P2b.5 fix
  → re-measure 92/60+) is the worked example.
- Deliverables: `docs/phase2b-results.md`, `phase2b-trace.md`,
  `phase2b-trace2.md`, 4 saturator canaries.

## Residual gaps — cluster C/D (17 GALEN + 27 notgalen = 44 pairs)

The remaining MISSED are all the "functional-role + covering /
sibling-collapse" pattern documented in `phase2b-galen-pair-analysis.md`
pairs 06 (CongestiveCardiacFailure ⊑ IntrinsicallyPathologicalBodyProcess)
and 07 (AcuteGastricUlcer ⊑ DigestiveSystemPathology). HermiT derives
these via tableau-style negation + functional-role sibling collapse.

The per-pair analysis lists THREE candidate implementations:

1. Full functional-role inference + negation/case-splitting (HermiT-
   style hypertableau extension).
2. Disjointness propagation through the merged witness (extension of
   Phase 2a's atom-set merge).
3. **An EL+ approximation:** materialise the consequence
   `∃hasIntrinsicPathologicalStatus.pathological` directly when the
   conditions of the relevant GCI fire and the `physiological`
   alternative is provably-disjoint. This is a saturator extension,
   not a tableau extension — and is what a future Phase 2c would
   target.

Option 3 means cluster C/D is **tractable but lower-priority than
Phase 3 perf**, not "outside scope."

## Why defer to Phase 2c rather than implement now

1. **Speculative without further diagnosis.** The per-pair analysis
   names options but doesn't pin down the precise rule shape. Same
   discipline that worked for Phase 2b (`trace-before-extend`) says
   we'd need a Phase 2c.0 diagnostic round before designing the rule
   — burning more time before the next measurable diff.

2. **Wall regression is the more pressing concern.** Phase 2b's fixes
   pushed GALEN from 12.5min to 24.7min (~2×) and notgalen to 37.5min.
   The spec's approved Phase 3 (saturator performance) addresses this
   directly. A future Phase 2c that adds more lowering work would
   compound the wall problem unless Phase 3 lands first.

3. **84% recovery is a clean victory.** The spec target was much
   lower (≤49 MISSED, ~60 recovery). Closing the remaining 44 pairs
   is a separate decision once Phase 3 informs whether more saturator
   work is even tractable.

## Decision

**Phase 2 closes here.** Next: Phase 3 (saturator performance), per
the spec's approved sequencing. Phase 2c (cluster C/D EL+ approximation)
is queued for after Phase 3 — its scope and viability depend on what
Phase 3's flamegraph reveals about the saturator hot path.

## Cross-references

- `phase2a-results.md`
- `phase2b-galen-diagnosis.md`
- `phase2b-results.md`
- `hypertableau-dead-ends.md` §14
- Design spec: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
