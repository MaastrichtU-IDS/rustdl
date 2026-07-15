# Phase 1 findings — `ore_ont_10019` residual (2 stalled + 3 MISSED)

**Outcome:** the transposition memo was built, measured, and **reverted** (sound but
doesn't clear the stalls; not worth banking). The 2 stalls **don't need fixing** (they
cause zero MISSED). The completeness recovery (2 of the 3 MISSED) comes from the
**existing `RUSTDL_CLASSIFY_DEFINED_SWEEP`, run with a bounded per-pair budget** — no new
mechanism. The last 1 is deferred.

## What was built and reverted (commits cbb0768 → bbf0751 revert)

A within-search transposition memo: memoize a `solve` frame's terminal Sat/Unsat verdict
keyed on an exact index-dependent canonical graph-state key (labels + at_most +
at_least_done + edges + ≠ + excluded; deps excluded), `Sat` short-circuit, `Unsat` reused
with `DepSet::ALL`, per-`decide` scope. Sound (worklist empty at the memo point ⟹ graph
state determines the verdict; verified by exact key equality).

**Measured (`ore_ont_10019`, flag ON):** it FIRES — `AcylGroup` 508 hits / **3 distinct
states** / 1278 branches; `KetoneGroup` 1903 hits / 6473 distinct — but does **not clear
the depth-256 stalls** (branches 28.7k→22.8k, still 2 stalled). `AcylGroup`'s
3-distinct-states-at-depth-256 is a **3-state cycle**: the cycle states never reach a
*cached terminal* verdict (their verdict is in-progress up the stack), so a terminal-verdict
memo has nothing to reuse for them. This is exactly SP2's "**depth-bound, not
re-derivation-bound**" caveat — confirmed at the mechanism level.

**Why not the cycle-detection variant** (mark on-path states, cut on revisit): unsound.
"The loop witnesses a model → Sat" is valid only under a *proven* blocking condition
(label subset/equality with double-blocking's inverse-role care). A raw decision-state
repeat is NOT that — SROIQ with inverses + `≤n` can cycle in decision-state with no real
looping model, so returning `Sat` is a false `Sat` = wrongly-not-subsumed (FP-surfaced,
oracle-invisible). It's the reuse-trap in a blocking costume. Off the table.

**Why reverted, not banked:** default-OFF and sound, but ~250 lines of soundness-critical
`state_key` (every field a future soundness obligation), fires every `solve` frame, and the
measured benefit is a ~20% branch trim that clears nothing and helps zero curated fixtures.
Carrying cost, no live payoff — the same "sound-but-inert, revert it" call as SP2's
no-goods and the bound-tail flag.

## The 2 stalls don't need fixing

`AcylGroup`/`KetoneGroup` stall only in the standalone `hyper-sat` *satisfiability probe*.
In `classify` they are bounded by the per-pair timeout (default to not-subsumed) and cause
**zero MISSED** — their subsumptions are derived via saturation + the pairwise loop. So
Track B's premise (fix the stalls) was moot for correctness; its only purpose was to make
Track A's sweep affordable, and that turned out not to require touching the stalls at all.

## Track A — the completeness recovery (the real prize, and it's small)

`RUSTDL_CLASSIFY_DEFINED_SWEEP=1` (existing, verifies defined-superclass candidates with
the main tableau) recovers **2 of the 3** MISSED — the per-pair budget bounds it:

| sweep per-pair budget | result | wall |
|---|---|---|
| 3000 ms | 161/162, MISSED=1, FP=0 | 263 s |
| 250 ms | 161/162, MISSED=1, FP=0 | ~28 s |
| 100 ms | 161/162, MISSED=1, FP=0 | ~12 s |

So the recovery (2/3 → **161/162, FP=0**) is available now, affordably, via the existing
flag at a bounded budget — the main-tableau cost is per-pair-bounded, so lowering the budget
loses no recovery here. **The last 1, `SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`
(deepest defined⊑defined), is missed even by the full 3000 ms sweep** → a genuine residual
completeness gap, deferred (its own diagnosis: nested-defined `∀`/`∃`-through-sub-role).

## Recommendation / open decisions (user's call)

`ore_ont_10019` is at **159/162 (FP=0) by default**, **161/162 with the bounded sweep**.
Options — none require the reverted memo:
1. **Keep the sweep opt-in** (default-OFF, as now) — it re-verifies defined-sups on ALL
   ontologies, so default-ON would tax curated classify; measure curated cost before any
   default flip.
2. **Tune the sweep** to only verify label-heuristic-uncertain defined-sups (narrows its
   cost) — a separate optimization.
3. **Defer the last 1** and call `ore_ont_10019` done at near-parity (161/162, FP=0). The
   prize for more is one subsumption the full sweep can't reach.

The dense-SROIQ tail is now: over-branching fixed (card-disjunct-atoms, shipped, default-ON,
159/162); the residual is one hard defined⊑defined subsumption + two cosmetic probe stalls.

## The last MISS diagnosed (2026-07-16) — `SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`

**It is NOT a calculus gap, and it is NOT separately modularizable. It is the
over-branching root cause manifesting on the `¬sup` side of this one pair.**

- **Not a calculus gap.** On a minimal reproducer (the 2 defs + bond roles + atoms,
  even with `CarbonGroup ≡ Aryl ⊔ Alkyl` and the Alkyl/Aryl bodies added) the tableau
  derives it **instantly** (0.2 s); `explain` on that reproducer reports "answered by
  tableau." The subsumption holds because Sulfoxide's `∃hasSingleBondWith` filler is a
  superset of SulfinicAcid's (adds `∃hasSingleBond.CarbonGroup ⊓ ≥2 hasSingleBond.CarbonGroup`),
  so `Sulfoxide-filler ⊑ SinicAcid-filler` and `∃` is monotone.
- **Two obstacles in the full ont:** (a) the label heuristic prunes the pair by default
  (the defined-sweep bypasses it); (b) the per-pair tableau probe **blows up in the
  full-ont context** — `explain` on the full ont is killed at 60 s.
- **Module extraction is dead for this ont (advisor-confirmed calculus + measured).** A
  *sound* ⊥-locality module for Σ = {SulfoxideGroup, SulfinicAcidGeneralGroup} expands
  Σ (via `CarbonGroup ≡ Aryl ⊔ Alkyl` → `CarbonAtom, hasBondWith`; via domain/range →
  `Atom`) to `{CarbonAtom, hasSingleBondWith, hasDoubleBondWith, OxygenAtom, Atom}`. Every
  one of the **15** functional-group definitions is `SomeGroup ≡ CarbonAtom ⊓ (cardinality/∃
  on bond-roles to Atom-subclasses)`; their **sufficient-direction** `body ⊑ SomeGroup`
  axioms are over exactly that vocabulary, so under ⊥-locality (SomeGroup→⊥) `body ⊑ ⊥` is
  non-tautological → **non-local → the module must keep all 15**. Those 15 are precisely the
  over-branching source (see [[dense-sroiq-root-cause-overbranching]]). The fast hand-curated
  reproducer was fast only because it *deleted* axioms a sound module is obligated to keep.
  Measured: 47 classes, 15 CarbonAtom-anchored defs, 25 bond-role defs — the module ≈ whole ont.
- **Conclusion:** no cheap, sound, separately-scoped lever closes this pair. Closing it means
  further attacking the over-branching itself (surrogate atoms / stronger sufficient-direction
  absorption so the 15 defs stop branching) — the deep frontier, not warranted for one
  subsumption at 161/162 FP=0. **Deferred, with the mechanism recorded.**
