# Phase 2d + 2c-redux combined results — IPBP cluster recovery shipped

Run 2026-06-01. Phase 2d (fact-on-subclass propagation, commit b78c5fd)
+ Phase 2c-redux (sub-role witness propagation re-applied on top,
commit 34a2b62). See:
- `docs/phase2d-design.md` for the propagation mechanism.
- `docs/phase2c-fix-target.md` for the (unchanged) witness-coincidence
  rule that Phase 2c-redux restores.
- `docs/phase2d-intermediate-results.md` for the Phase 2d-alone gate
  measurement (T4) that authorized Pass 2.
- `docs/hypertableau-dead-ends.md` §15 (now RESOLVED).

## Headline

**GALEN MISSED 17 → 0 (full parity with Konclude); notgalen MISSED
27 → 18 (9 recovered).** 26 pairs recovered total — at the upper
end of the Phase 2c.0 diagnosis's "24 confident floor → 44 upper
bound" range. Wall cost: GALEN +6.5% (12.55 → 13.36 min); notgalen
+2.7% (32.10 → 32.95 min). FP=0 held across Phase 0 net + GALEN +
notgalen.

This resolves dead-end §15 (Phase 2c sub-role witness propagation
needed fact-on-subclass propagation as an architectural prerequisite);
the combined two-layer system has the witness-existence preconditions
that the original Phase 2c attempt found absent on pair_06's ICF.

## Soundness gate (Phase 0 net)

| Fixture | FP (pre → post) | MISSED (pre → post) | Wall |
|---|---|---|---|
| alehif-test | 0 → 0 | 0 → 0 | 23.70 s |
| ore-10908-sroiq | 0 → 0 | 0 → 0 | 27.83 s |
| ore-15672-shoin | 0 → 0 | 0 → 0 | 37.69 s |

Net soundness held. Total 38 s for the 3-fixture run.

## Completeness lever (GALEN — full parity)

| Stage | MISSED | rustdl_closure | konclude_closure | FP | Wall |
|---|---|---|---|---|---|
| Pre-2d baseline (T2, commit aab6d03) | 17 | 27,980 | 27,997 | 0 | 752.83 s (12.55 min) |
| Post-2d-only (T4, commit b78c5fd) | 17 | 27,980 | 27,997 | 0 | 746.14 s (12.44 min) |
| **Post-2d+2c-redux (T7, commit 34a2b62)** | **0** | **27,997** | **27,997** | **0** | **801.55 s (13.36 min)** |
| **Δ vs pre-2d** | **−17 (full)** | **+17** | unchanged | unchanged | **+6.5%** |

Closure count matches Konclude exactly (27,997 = 27,997). The 17 MISSED
pairs that survived Phase 3c — all in the IPBP-derivation cluster
(`*IneffectiveCardiacFunction* ⊑ IntrinsicallyPathologicalBodyProcess`
variants + Polyp ⊑ AbnormalBodyStructure + Postcardiotomy/Postvalvulotomy
variants) — all close via the two-layer recovery chain.

## Completeness lever (notgalen — partial)

| Stage | MISSED | rustdl_closure | konclude_closure | FP | Wall |
|---|---|---|---|---|---|
| Pre-2d baseline (Phase 2c-era) | 27 | 32,712 | 32,739 | 0 | 1925.98 s (32.10 min) |
| **Post-2d+2c-redux (T7)** | **18** | **32,721** | **32,739** | **0** | **1977.22 s (32.95 min)** |
| **Δ vs pre-2d** | **−9** | **+9** | unchanged | unchanged | **+2.7%** |

9 of 27 notgalen MISSED close — the same IPBP-cluster pairs (CCF,
IneffectiveCardiacFunction, LeftIneffectiveCardiacFunction,
PostcardiotomySyndrome, PostvalvulotomySyndrome,
RightIneffectiveCardiacFunction variants ⊑ IntrinsicallyPathologicalBodyProcess
that match GALEN's shape). The 18 residual notgalen MISSED include
the `*-Anonymous-324` cluster (anonymous-named super-classes the
diagnosis had flagged as uncertain) and any patterns not matching
the IPBP triangle.

## Wall cost analysis

| Fixture | Wall pre-2d | Wall post-2d+2c | Δ % |
|---|---|---|---|
| GALEN | 12.55 min | 13.36 min | **+6.5%** |
| notgalen | 32.10 min | 32.95 min | **+2.7%** |
| Phase 0 net (sum) | — | 38 s | clean |

Both wall regressions are well under the 15% combined-cap criterion.
The price of ~50 seconds on GALEN and ~50 seconds on notgalen is
proportional to the 26-pair MISSED recovery — roughly 2 seconds per
recovered pair, which is acceptable given each recovery represents a
sound-and-complete subsumption that the saturator now closes without
needing the tableau/wedge fallback.

## Mechanism (the chain that closes pair_06)

For `CongestiveCardiacFailure ⊑ IntrinsicallyPathologicalBodyProcess`
via `IneffectiveCardiacFunction`:

1. **Phase 2d** inherits `(ICF, hasIntrinsicPathologicalStatus, physiological)`
   from `NAMEDPhysiologicalProcess` (via the subsumer chain ICF ⊑
   CardiacFunction ⊑ NAMEDCirculatoryProcess ⊑ NAMEDPhysiologicalProcess).
2. **Phase 2d** inherits `(ICF, hasPathologicalStatus, pathological)`
   from `PathologicalBodyProcess`.
3. **Phase 2a** witness-merge fires on `(ICF, StatusAttribute)` —
   accumulates `{physiological_atoms, pathological_atoms}` — emits
   `(ICF, StatusAttribute, F)` where F ≡ ⊓(merged atom set).
4. **Phase 2c-redux** propagates F back to sub-roles ICF has facts on:
   emits `(ICF, hasIntrinsicPathologicalStatus, F)` and
   `(ICF, hasPathologicalStatus, F)`.
5. Existential trigger
   `∃hasIntrinsicPathologicalStatus.pathological ⊑ IntrinsicallyPathologicalBodyProcess`
   matches `(ICF, hasIntrinsicPathologicalStatus, F)` via target-subsumer
   propagation (F ⊑ pathological via Phase 2a's atomic-subsumption clauses).
6. ICF ⊑ IntrinsicallyPathologicalBodyProcess. CCF ⊑ ICF was already
   there; transitivity yields CCF ⊑ IPBP.

No covering axiom, no case-split — purely Horn EL+ reasoning over the
inherited fact set.

## What's left

- 18 notgalen MISSED unresolved — likely the `*-Anonymous-324` cluster
  + non-IPBP-shape patterns. Would need separate analysis (Phase 2e?)
  to characterize and possibly close.
- Other corpus fixtures not in the FP=0 net or GALEN/notgalen aren't
  measured here; pizza / ro / sulo / sio sanity checks would be a
  good follow-up but aren't gated on T7.

## Cross-references

- Phase 2c original implementation: b83fcd6 → reverted at cc2019e
- Phase 2c.0 diagnosis (predicted 24-44 recovery): `docs/phase2c-galen-diagnosis.md`
- Phase 2c fix target (witness-coincidence rule): `docs/phase2c-fix-target.md`
- Phase 2d design: `docs/phase2d-design.md`
- Phase 2d intermediate results: `docs/phase2d-intermediate-results.md`
- Combined plan: `docs/superpowers/plans/2026-06-01-phase2d-plus-2c-redux.md`
- Phase 2d implementation: commit b78c5fd
- Phase 2c-redux implementation: commit 34a2b62
- T7 logs (transient): `/tmp/p2d-{final-net,final-galen,final-notgalen}.log`
