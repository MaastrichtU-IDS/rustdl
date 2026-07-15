# Plan — close `ore_ont_10019`'s residual (2 stalled + 3 MISSED)

**After the card-disjunct-atoms fix** (`docs/superpowers/specs/2026-07-16-cardinality-disjunct-atoms-design.md`):
`ore_ont_10019` is at FP=0, MISSED=3 (159/162), 2 classes still stall. Diagnosis
(2026-07-16) shows the 2 stalled and 3 MISSED are **independent mechanisms** → two tracks.
Measure-first: Phase 0 diagnoses before Phase 1 fixes; gate every fix on FP=0 (non-Horn
`ore_ont_13723` oracle) + curated MISSED=0 byte-identical + `ore_ont_10019` residual cleared.

## KEY FINDING (2026-07-16): the tracks are COUPLED — do Track B first

`RUSTDL_CLASSIFY_DEFINED_SWEEP=1` recovers **2 of the 3** MISSED soundly (MISSED 3→1,
161/162, FP=0) — but takes **263 s**, because the sweep re-verifies defined-sups with the
full tableau and is bottlenecked by the **Track-B stalls** (AcylGroup/KetoneGroup depth-256).
So the cheap completeness win already EXISTS (the sweep) but is gated on fixing the stalls.
**Revised order: Track B first (fix the stalls → sweep becomes fast → default-on-able →
recovers 2/3), then the residual 1** (`SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`, deepest
defined⊑defined, NOT recovered even by the slow sweep — needs its own diagnosis).

## Track A — the 3 MISSED (completeness)

`SulfinicAcidGeneralGroup ⊑ OrganicSulfurGroup`, `SulfoxideGroup ⊑ OrganicSulfurGroup`,
`SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`. All are subsumptions INTO defined `∃`-body
classes (`OrganicSulfurGroup ≡ ∃hasBondWith.SulfurAtom ⊓ OrganicGroup`). Deriving them
needs `¬(defined-sup)` (`= ∀hasBondWith.¬SulfurAtom ⊔ ¬OrganicGroup`) to clash against the
sub-class's `∃hasSingleBond.(SulfurAtom…)` **through the role hierarchy**
(`hasSingleBond ⊑ hasBondWith`) + `CarbonGroup ⊑ OrganicGroup`.

- **A0 (diagnose):** (1) does `RUSTDL_CLASSIFY_DEFINED_SWEEP=1` recover them? (they are
  defined-sup subsumptions — its exact target). (2) else trace `SulfinicAcid ⊓ ¬OrganicSulfurGroup`:
  is `∀hasBondWith.¬SulfurAtom` failing to propagate to the `hasSingleBond` successor
  (a `∀`-over-sub-role gap), or is the label heuristic pruning the pair?
- **A1 (fix):** (a) refine/flip the defined-sup sweep if it recovers them soundly, or
  (b) close the specific `∀`-propagation-through-role-hierarchy gap. Small, targeted.

## Track B — the 2 stalled (tractability; NOT completeness — they cause no MISSED)

`AcylGroup`, `KetoneGroup`: depth-256, 0 blocks fired, driven by `≥2 hasSingle.{OrganicGroup,Alkyl}`
generating a deep chain (symmetric bonds + `Alkyl`/`OrganicGroup` recursion) blocking never
caps. Both are satisfiable and their subsumptions ARE derived — this is wall/robustness only.

- **B0 (diagnose):** (i) the `≥`-part (`AtLeast`) disjuncts of `¬(=n)` aren't
  satisfaction-checked (`head_atom_satisfied` `TODO(HF3)`) → a satisfied `≥` disjunct still
  branches; (ii) the `≥2` chain has recurring labels double-blocking misses — RE-RUN the
  anywhere-blockable probe on the NOW-FIXED search (the earlier 0% was on the over-branching
  search; may differ).
- **B1 (fix):** (i) → add an `AtLeast` satisfaction arm to `head_atom_satisfied` (count
  `≥ n` successors) — small, sound (recognizing a satisfied disjunct only removes branches);
  (ii) → anywhere-blocking in the wedge (larger; double-blocking soundness care for
  symmetric/inverse roles) or a generation refinement.

## Sequencing

Track A first (completeness; likely cheaper if the defined-sweep recovers it). Track B
second. Advisor pass before any default-flag flip (both `AtLeast`-satisfaction and
anywhere-blocking have FP-adjacent surface). Target: `ore_ont_10019` 0 stalled / MISSED=0
(full Konclude∩HermiT parity, 162), FP=0, curated MISSED=0.
