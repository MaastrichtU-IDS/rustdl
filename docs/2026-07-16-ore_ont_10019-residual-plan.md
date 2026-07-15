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

## Track B — the 2 stalled (tractability ONLY; they cause NO MISSED)

`AcylGroup`, `KetoneGroup`: depth-256, 0 blocks fired. Both are satisfiable and their
subsumptions ARE derived — this is wall/robustness only, and **the sole reason to fix it is
to make Track-A's defined-sweep affordable** (the sweep's 263 s is these two stalls).

**B0 DONE (2026-07-16) — diagnosed as TRANSPOSITION, not the mechanisms first guessed.**
`n_nodes` is pinned at **4** for the entire depth-256 stack (advisor: if `≥` disjuncts were
*taken*, `generate_at_least` would grow the graph — it doesn't). A per-class recurrence
probe (index-independent whole-graph signature) on the NOW-FIXED engine:
`AcylGroup` **repeat_frac 0.997, 2 distinct states** (766/768 revisits); `KetoneGroup`
**0.816, 3770 distinct** (16768/20538). The search re-derives the same handful of states via
different decision orders — pure no-progress transposition. This is **not** unrecognized
`≥`-disjuncts and **not** blocking (a 4-node graph has nothing to block). The 2.4% GLOBAL
recurrence that killed the transposition memo earlier was dominated by the (now-fixed)
over-branching classes; **on these two it is 99.7% / 81.6%** — the memo is live here.

**A COUNT-BASED `AtLeast` satisfaction-check is OFF THE TABLE (advisor):** unsound — `≥n`
breaks under a pending merge and is read mid-construction (`=2` = `≥2 ⊓ ≤2` with the `≤2`/
merge in flight), so recognizing `≥n` off a transient count wrongly skips a branch →
wrongly-not-subsumed, invisible to FP=0-vs-oracle. Do not install it.

**B1 (fix) = within-search transposition memo** (the advisor-scoped design): within a single
`decide()`, memoize the (Sat/Unsat) verdict keyed on the index-independent whole-graph
structural signature (labels + edges + merges + `≠` + `at_most` + `excluded`; deps EXCLUDED).
`Sat` short-circuits (sound — fixed query `sub ⊓ ¬sup`, so a Sat state genuinely means this
query is satisfiable; NOT the cross-query reuse-trap). `Unsat` reused only with
`DepSet::ALL` (sound superset; disables backjump on the hit but correct). Scope per-`decide`
(cleared each call). Run with `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0` OR include the worklist
in the key (advisor: worklist is part of saved/restored state and affects firing).

**B1 soundness gate (tractability-fix ⟹ MUST NOT trade soundness for speed):** FP=0 on the
non-Horn `ore_ont_13723` oracle + curated MISSED=0 byte-identical + a dedicated adversarial
canary that a memo hit cannot turn a real clash into a skip (the key must fully determine the
subtree — a missing key component = a wrong reuse = FP). If the sound memo does NOT cleanly
make the two probes terminate, the fallback is to **bound those two probes** (they lose zero
completeness) rather than risk correctness for a wall win.

## Sequencing

Track A first (completeness; likely cheaper if the defined-sweep recovers it). Track B
second. Advisor pass before any default-flag flip (both `AtLeast`-satisfaction and
anywhere-blocking have FP-adjacent surface). Target: `ore_ont_10019` 0 stalled / MISSED=0
(full Konclude∩HermiT parity, 162), FP=0, curated MISSED=0.
