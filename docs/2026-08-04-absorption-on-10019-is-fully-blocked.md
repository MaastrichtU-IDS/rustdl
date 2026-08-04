# Absorption cannot help `ore_ont_10019`: every conjunctive definition needs a non-atomic conjunct decided

> # ⚠️ TITLE AND CENTRAL INFERENCE CORRECTED, SAME DAY. THE MEASUREMENTS BELOW STAND; THE CONCLUSION DOES NOT.
>
> Binary absorption is **not** blocked on this ontology. It is the **single sufficient mechanism** —
> established by reading Konclude's own absorbed TBox and by ablating its optimisations
> (`docs/2026-08-04-konclude-cardinality-mechanism.md`).
>
> **What I got right:** `..with extra ¬Atomic: 0`, and 0 of 26 heads atomic-only. Both reproduce.
>
> **What I got wrong:** that counter counts only second guards that are *atomic conjuncts already
> written in the definition*. **Konclude's second guard is a freshly minted surrogate propagated
> backward along the role**, obtained from a cardinality conjunct by using only its `≥1`
> consequence — `OxygenAtom ⊑ ∀hDBW.T₁`, `Alkyl ⊑ ∀hSBW.T₂`, `T₁ ⊓ T₂ → T₃`,
> `CarbonAtom ⊓ T₃ → head`. That yields a second guard for **26 of 26** heads. I measured the
> supply of *pre-existing* guards and concluded none could exist, when the mechanism **manufactures**
> them. Consequences: the corpus census figures below (285 pool / 65 survivors) are an
> **under-count of the addressable population**, and "no third variation on atomic absorption can
> work either" is false.
>
> **The §"What 'decided' costs, per engine" analysis is also wrong in its pricing.** It prices
> `∃`-conjunct bodies as needing a new edge-join rule kind — "a substantial engine change".
> Konclude shows **`∀r.surrogate` propagation *is* the edge join**, and that is precisely the shape
> rustdl's existing `RoleRule` already has. The remaining gap is (i) a multi-guard `ConceptRule` —
> a **label lookup**, not a join, and exactly the `absorb.rs:45` "Phase 4 refinement" — and
> (ii) emitting `F ⊑ ∀r⁻.T` per `∃r.F` / `≥n r.F` conjunct.
>
> **The one claim that survives fully:** the *cardinality* reverse derivation (deriving `≤n r.C`
> holds at a node) is genuinely hard — and Konclude never attempts it, keeping the `≤n` disjuncts
> in the head. So it was never on the critical path for this ontology. KM *does* attempt it
> (`card_recog_step`, a deterministic counting rule at a saturated model), which is a separate,
> stronger, and riskier lever.
>
> **The prioritisation argument also survives**: this ontology's unique remaining prize is one
> subsumption, so a build must still be justified on a population, not on `ore_ont_10019`.
>
> **Method note this cost:** I inferred an impossibility from the *absence* of an input, without
> checking whether the mechanism generates its own input. Peer behaviour was available to measure
> the whole time — the reference implementation dumps its absorbed TBox on request.

**Date:** 2026-08-04
**Status:** measurements valid; conclusion superseded — see the banner above and
`docs/2026-08-04-konclude-cardinality-mechanism.md`.
**Reviews that produced it:** `docs/reviews-2026-08-04/R1-technical.md` (NO-GO),
`docs/reviews-2026-08-04/R2-value.md` (DON'T DO IT).

## The result

`ore_ont_10019` has 29 `EquivalentClasses`, 26 of them conjunctive. Counting conjunct kinds
per head (from `canon.owx`, flattening nested `ObjectIntersectionOf`):

| | heads |
|---|---:|
| conjunctive definitions | 26 |
| …with ≥1 **cardinality** conjunct | 15 |
| …with ≥1 **existential** conjunct | 17 |
| **…atomic-only (no `∃`, no cardinality)** | **0** |
| …with **≥2 atomic** conjuncts (binary-absorption candidates) | **0** |

**Zero of 26 heads can be absorbed by any technique that only manipulates atomic conjuncts.**
Both figures are corroborated by the shipped instrument, which measures exactly this population
(`crates/owl-dl-core/src/residual_absorbability.rs:47-56` names it *the* binary-absorption
payoff column):

```
$ rustdl residual-absorbability ore_ont_10019.owl/canon.owx
# concept_rules:                182
#   conclusion_is_or:           29
#   ..with extra ¬Atomic:       0  (binary-absorption candidates)
```

## Why this closes two proposals at once, not one

Both candidate mechanisms are blocked by the same measurement, and neither was blocked by
anything subtler:

1. **Definitorial/surrogate absorption over atomic conjuncts** (the retracted plan's Phase 1).
   Needs ≥2 atomic conjuncts to form a chain — with one, `S ≡ A` fires on exactly `A`'s node
   set and the residual head is unchanged. **0 candidates.**

2. **Multi-trigger (binary) absorption in `absorb.rs`** — R1's constructive alternative, and
   the refinement `absorb.rs:45` has named as unbuilt since the file was written
   (*"Multi-trigger absorption (`A ⊓ B ⊑ C`) is a Phase 4 refinement"*). Restricted to
   **atomic** guards it is the attractive option: sound *and* complete (for an atomic class,
   label presence **is** the semantics), and it mints **no new class**, so it inherits none of
   R1's id-space / output-leak / told-table / fragment-gate hazards. But its addressable set on
   this ontology is **the same 0**, for the same reason.

The distinction matters because the two proposals fail for one shared cause, so no third
variation on atomic absorption will work either. What every head actually needs is a
**non-atomic** conjunct — `∃r.C` or `=n r.C` — **decided**.

## What "decided" costs, per engine

`absorb_gci` picks the one atomic conjunct as trigger and dumps the rest into a disjunctive
conclusion. For `AmideGroup ≡ CarbonAtom ⊓ ∃hDBW.O ⊓ ∃hSBW.N`, the ⇐ direction becomes

```
ConceptRule { CarbonAtom → Or([ ∀hDBW.¬O, ∀hSBW.¬N, AmideGroup ]) }
```

— a 3-way disjunction on **every** `CarbonAtom` node, and `CarbonAtom` triggers **10** of the
26 heads (not the ~15 stated at `docs/2026-08-04-ore-10019-rootcause.md:157`; that figure is
corrected here). Making it deterministic requires deciding `∃hDBW.O` at the node, which is an
**edge join**, not a label lookup.

- **The wedge already does this** for `∃`. `absorb_hard_antecedent`
  (`crates/owl-dl-core/src/clause.rs:442-491`) sorts each antecedent conjunct into *soft* —
  anything `encode_antecedent` accepts, which includes `Some` — and *hard*, putting all soft
  conjuncts in the clause **body** as a genuine edge join (`r(X,y) ∧ C(y) → D(X)`) and negating
  only hard ones into the head. So the 11 `∃`-only heads are **already fully Horn in the
  wedge**, with no surrogate and no disjunction.
- **The main tableau has none of it.** `ConceptRule { trigger: ClassId, conclusion: ConceptId }`
  (`absorb.rs:145-148`) carries a **single atomic** trigger and is dispatched by scanning single
  `Atomic(cls)` labels (`owl-dl-tableau/src/rules.rs:171-179`). There is no edge-join rule kind:
  `RoleRule` is `∀R.D` propagation, a different shape. Giving the main tableau `∃`-conjunct
  bodies means giving it the wedge's clause machinery — a substantial engine change, and
  squarely contradicting the retracted plan's "preprocessing only, no engine change".

## The binding sub-problem, now fully localized

Both remaining paths converge on the same wall:

- In the **main tableau**, the `∃` half needs new machinery and the cardinality half needs that
  *plus* cardinality satisfaction.
- In the **wedge**, the `∃` half is already Horn — and it **still stalls**: 373,919 branches on
  `KetoneGroup` (`docs/2026-08-04-ore-10019-rootcause.md:92-98`). `KetoneGroup` has **0
  existential and 2 cardinality** conjuncts, so the wedge treats its non-atomic conjuncts as
  *hard* and puts `¬(=1 r.C)` back in the head.

So the one mechanism that would move this ontology on either engine is **deciding cardinality
conjunct satisfaction**. That is precisely what `RUSTDL_OR_CARD_SATISFIED` attempted: it was
built, it fired on its pre-declared criterion (branches halved, `options=5` 1175→6), it
**decided no additional class**, and it converted a branch-bound stall into a generation-bound
one (nodes 135→257). Reverted.

**This is a sharpening, not a dead end.** Before this round the candidate set was "some form of
absorption, probably binary, possibly definitorial, plus a deferred cardinality piece". It is
now exactly one sub-problem with one recorded failed attempt — and the wedge is a working
reference showing that solving the `∃` half alone is *insufficient*, which the retracted plan
would have spent two tasks discovering.

## Consequences for prioritisation

`ore_ont_10019` is a **97 s / 159-of-162 / FP=0** ontology, and `RUSTDL_CLASSIFY_SAME_TIER=1`
already recovers 2 of the 3 missing pairs (→ 161). Its unique remaining completeness prize is
**one subsumption** (`SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`), behind the sub-problem above.
Both reviews independently ranked other work higher; R2's ranking, with the corpus-wide
population counts from the committed census
(`docs/benchmarks/2026-08-01-residual-absorbability-census.tsv`, n=1,913 OK):

| | pool | v0.4.10 DNF survivors |
|---|---:|---:|
| `concept_rule_or > 0` | 888 | 137 |
| **`extra ¬Atomic > 0`** (atomic absorption addressable) | **285** | **65** |
| `or > 0` but `extra == 0` (atomic-absorption **inert**) | 603 | 72 |

So atomic absorption is **not** globally inert — 285 pool / 65 survivor ontologies have a
non-zero candidate count (median 6 heads, max 30,509). `ore_ont_10019` simply is not one of
them; it sits in the 603/72 column. **If binary absorption is built, it must be justified and
gated on that 285/65 population, never on `ore_ont_10019`** — and per
`docs/2026-08-01-domain-absorption-results.md`'s own standing instruction, only after the
one-directional deletion falsification it prescribed (and which has still not been run).

## Method notes

- **The blocking number was already printed, twice, in documents the plan cited** — the
  root-cause doc at `:133` and the plan's own "defect" section. Two independent reviewers found
  it in minutes. Before planning on a mechanism, run the instrument that counts the mechanism's
  addressable set on the target, and put the number in the plan next to the decision rule.
- **Both measurement gates were invalid for the change they were meant to gate**, in ways only
  a pre-check would have caught: `ore_ont_10019` is **absent** from the 400-ontology MISSED-net
  population, and **7 of 8** curated fixtures have `extra ¬Atomic == 0`, so "closures exact"
  would have measured inertness. Check that a gate *can fire* before trusting it.
- **A cheaper analogue of a proposed mechanism may already exist elsewhere in the tree.** The
  wedge had the `∃` half of the proposal all along, and its continued stall is the strongest
  available evidence about the proposal's ceiling. Look for the analogue before building.
