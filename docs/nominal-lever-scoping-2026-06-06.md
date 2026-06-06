# Scoping: nominal-reasoning lever for wine's 57 MISSED

Scoped 2026-06-06. Question: can wine's 57 MISالسES (all nominal/value-restriction
subsumptions) be closed by a sound lever like the SIO disjunction pass?

**Verdict.** Partially. A sound, bounded lever closes the **region cluster
(~16–18 of 57)**; the remainder (~37: sugar `∀`+nominal-set, grape
`≤1`+nominal) needs universal-restriction and cardinality reasoning that is
genuinely harder (tableau-grade, not a clean saturator/preprocessing pass).
Unlike SIO (one pass closed all), wine's nominal frontier is multi-mechanism and
only ~1/3 is cheap.

## Discriminators (the SIO playbook, re-run)

1. **trust_sat=0 does NOT recover any of the 57** (wine closure-diff,
   `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`, 200 ms budget: MISالسED 57 → 57, FP=0).
   So it is *not* a cheap trust_sat-policy under-approximation the complete
   engine already handles cheaply.
2. **Complete tableau is intractable at scale.** `subclass` (classic tableau,
   unbounded) on full wine for one region pair (`AlsatianWine ⊑ FrenchWine`)
   **times out >120 s**. Same shape as SIO — the per-pair complete path is not
   viable; a saturator-side lever is the justified route.
3. **The saturator does not fold nominal-filler existentials** even given the
   fact directly: minimal module `X ⊑ ∃locatedIn.{FrenchRegion}` + `FrenchWine ≡
   Wine ⊓ ∃locatedIn.{FrenchRegion}` → `saturation-only = no`, `complete = yes`.
   So a fold change (B below) is required; this is *not* SIO-style
   preprocessing-only.

## Mechanism breakdown of the 57

| Bucket | Count | Mechanism | Lever |
|---|---|---|---|
| Region (`FrenchWine`×15, `AmericanWine`) | ~16 | `∃locatedIn.{a}` + transitive ABox `a R* b` → `∃locatedIn.{b}`, then fold | **(A)+(B), tractable** |
| Color (`WhiteWine`, parts of `WhiteLoire`) | ~2–4 | `∃hasColor.{White}` + fold (no transitivity) | (B), tractable if color fact present |
| Sugar / non-sweet (`WhiteNonSweetWine`×5, `DryWhiteWine`) | ~6 | `∀hasSugar.{Dry,OffDry}` — **universal** over a nominal set | hard (∀ + OneOf) |
| Grape (`Gamay`,`Merlot`,`PinotNoir`×2,`Chardonnay`,`SauvignonBlanc`,`PinotBlanc`,`CabernetSauvignon`,`SemillonOrSauvignonBlanc`) | ~9 | `∃madeFromGrape.{g}` + **`≤1 madeFromGrape`** | hard (nominal + max-cardinality) |
| Misc (`Fruit ⊑ ConsumableThing/EdibleThing`) | ~2 | food-hierarchy; characterize at impl | unknown |

## The tractable lever (region + color, ~18–20 pairs)

**(A) Preprocessing — transitive-ABox nominal propagation.** For each
`X ⊑ ∃R.{a}` (`ObjectHasValue(R,a)`), emit `X ⊑ ∃R.{b}` for every `b` reachable
from `a` via the transitive closure of `R` over the named-individual ABox
(`ObjectPropertyAssertion`s + sub-property edges). **Soundness gate: only along
roles that are transitive** (`X R a`, `a R b`, `R⁺` ⟹ `X R b`); on a
non-transitive role this is unsound, so the pass must restrict propagation to
transitive roles (`locatedIn` is `TransitiveObjectProperty`, verified). Bounded:
ABox is 65 assertions; closure is small.

**(B) Saturator — fold nominal-filler existentials.** Index existential
triggers/facts by `Nominal(a)` as well as `Atomic(c)`, so `C ≡ D ⊓ ∃R.{a}`
fires when `X ⊑ ∃R.{a}` and `X ⊑ D`. Sound: matching `∃R.{a}` by individual
identity is definitional unfolding (structural; sound-but-incomplete — misses
`{a} ⊑ A` via ClassAssertion, the correct under-approximation posture). FP-safe.

(B) is a real saturator change (new key kind in the existential index), not a
preprocessing-only pass. (A) is a new preprocessing pass parallel to
`disjunction_existential.rs`, gated on role transitivity.

## Recommendation

The region lever (A)+(B) is sound and bounded but buys only ~1/3 of the 57 for a
saturator change plus a preprocessing pass — a worse effort/payoff than SIO. The
remaining ~37 (sugar `∀`, grape `≤1`) are the harder tableau-grade nominal
reasoning. Options:

1. **Build (A)+(B)** for the region/color cluster (~18–20 pairs); leave
   sugar/grape as documented tableau-deferred (parallels the SIO non-atomic /
   derived-subsumer deferrals).
2. **Defer the whole nominal frontier** — keep wine as a sound (FP=0) stressor
   with MISSED informational, and spend effort elsewhere (e.g. a real
   datatype-heavy fixture to close the Phase-D blind spot, or the SROIQ perf
   gap).

Either way the test asserts only FP=0 today (MISالسED informational, as for
sio/ro/sulo), so no regression risk in deferring.

## SHIPPED (2026-06-06) — option 1, region/color cluster

Built (A)+(B) **saturator-private** (no tableau impact, lowest FP risk):
- **(B)** `atomic_or_tseitin_body_with_extras` maps `Nominal(a)` bodies to an
  opaque per-individual synthetic class (`TseitinAllocator::introduce_nominal`),
  so the EL fold of `C ≡ D ⊓ ∃R.{a}` matches the `X ⊑ ∃R.{a}` fact. Fact side
  (`atomic_existential_rhs`) emits the bare NomKey (bypassing the range-extras
  wrap, which would hide the NomKey behind a synthetic and defeat (A)).
- **(A)** `build_abox_nominal_reach` builds the transitive closure of each
  transitive role over the named-individual ABox; `process_fact` propagates
  `X ⊑ ∃R.{a}` → `X ⊑ ∃R.{b}` for `b` in `a`'s reach.

**Result: wine MISSED 57 → 34 (23 recovered: region + color), FP=0 across all
10 corpus fixtures; the 9 parity fixtures stay MISSED=0.** Soundness gate held
corpus-wide. Residual 34 = grape (`≤1` cardinality) + sugar (`∀`+nominal set) +
misc — the hard buckets, deferred as planned. Canary
`nominal_transitive_abox_fold_classifies` (also asserts the unsound reverse does
NOT hold).
