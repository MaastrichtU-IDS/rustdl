# Stage-4 Konclude path — recon of the residual (post-skip) compound disjunctions

**Status:** recon (durable). After lazy-disjunction left 200k residual branches on Gamay, this
characterizes what those compound (synthetic-disjunct) ⊔ points actually are, to turn "integrated
expansion" into a concrete target.

## Findings (ontology-level)

- Wine has only **2 explicit `ObjectUnionOf`** (Fruit≡NonSweetFruit⊔SweetFruit;
  WineDescriptor≡WineColor⊔WineTaste) — so the residual disjunctions are NOT class-union
  definitions.
- Wine is saturated with **value-partition structure: 33 `ObjectOneOf` + 115 `ObjectAllValuesFrom`**,
  e.g. `CabernetSauvignon ⊑ ∀hasBody.OneOf(Full,Medium)`, `∀hasFlavor.OneOf(Moderate,Strong)`,
  `∀hasSugar.OneOf(Dry,OffDry)`, plus **nested food-pairing ∀-chains**:
  `NonSpicyRedMeatCourse ⊑ ∀hasDrink.∀hasFlavor.OneOf(Moderate,Strong)`.
- `Wine ⊑ ExactCardinality(1) {hasBody,hasColor,hasFlavor,hasSugar,hasMaker}` + functional those
  roles. `Gamay ≡ Wine ⊓ ∃madeFromGrape.{GamayGrape} ⊓ ≤1 madeFromGrape` — Gamay does NOT constrain
  its descriptor values, so they are genuinely free OneOf choices.

## Interpretation

The residual compound ⊔ points (synthetic-id disjuncts ≥137) are largely **`∀hasX.OneOf(values)`
value-partitions** — the disjuncts are NomKey value-singletons (synthetic ids, which is why they
did not resolve to named classes) — together with the nested food-pairing ∀-chains that propagate
value-constraints through `hasDrink`. The explosion is the **product of value-choices across the
descriptor roles, propagated through the food-pairing ∀-network**, under functional/`ExactCard(1)`
constraints. For an unconstrained class like Gamay (free descriptor values), a model exists for any
value combination; the search cost is exploring/backtracking that product when food-pairing
propagation induces clashes.

## What collapses it (and why there's no bounded entry)

Konclude resolves these via **`≤1`-driven value-forcing + ∀-propagation + nominal distinctness**
acting together (the integrated deterministic expansion): where a value is forced, the `OneOf`
collapses without branching; the nested ∀-chains propagate deterministically. This is exactly the
ForallKey/MaxKey/NomKey machinery rustdl HAS for *saturation completeness* — but it is not wired to
deterministically resolve the per-pair WEDGE's `∀R.OneOf` branches (the ∃-seed feeds derived value
facts where the saturator can derive them; for free-valued classes like Gamay it cannot, so the
wedge branches the product). Closing it = making the wedge's `∀R.OneOf`/value-partition expansion
deterministic under `≤1`+distinctness+∀-propagation, i.e. the **integrated ∀+nominal+≤1 expansion**
— a multi-component core, no single bounded rule (told-disjoint SP-B pruned=0; Horn det-resolution
18–34%; lazy-disjunction 200k residual; all confirm).

## Verdict

The recon confirms (does not refute) the convergent conclusion: the lever is the integrated
∀+nominal+≤1 deterministic expansion of the value-partition + food-pairing-∀ structure. It is the
large multi-component core build with no bounded/cheap entry — every probe this session (reuse,
algebraic, precise-deps, M2, M1, lazy-disjunction, and this recon) converges here. Continuing the
Konclude path = committing to that core build; no further cheap measurement will change the
convergence. main = origin/main 1be126d (15× ∃-seed + #25, pushed); this recon is read-only.
