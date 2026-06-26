# SP-B2b (ForallAtomicKey general-∀ propagation) FP=0 gate — results — 2026-06-23

Gate for B2b: `∀R.Atomic(K)` → `ForallAtomicKey(R,K)` synthetic + told monotonicity
edges `ForallAtomicKey(R,K) ⊑ ForallAtomicKey(R,L)` for `K ⊑ L`. FP=0 sacred.
Verdict: **FP=0 — PASS.**

## 1. Tuned closure-diff (oracle, hybrid) — PASS

`konclude_closure_diff`, `RUSTDL_TEST_PAIR_MS=1000`: **FP=0 / MISSED=0 on all distinct
fixtures** (no FP anywhere). B2b is additive + sound.

## 2. ORE pilot sweep (B2a vs B2b) — PASS

`--saturation-only`, isolating B2b. **Zero real-FP signatures** (`a_unsat > b_unsat`).

## 3. Goal achieved + the wine Course hierarchy

B2b closes the wine/food **∀-Course hierarchy** in saturation-only: `FishCourse ⊑
SeafoodCourse`, `ShellfishCourse ⊑ SeafoodCourse`, `BlandFishCourse ⊑ FishCourse ⊑
SeafoodCourse`, the Meat/Fowl/Dessert courses, etc. — all now derived via ForallAtomicKey
monotonicity (`Fish⊑Seafood ⟹ ∀hasFood.Fish ⊑ ∀hasFood.Seafood`). 2 canaries
(Course-hierarchy differentiator + transitive; no-spurious negative control:
unrelated fillers / different role ⟹ no subsumption). B1/B2a canaries unchanged.

## Residual wine gap → B2c (union-class), NOT B2b

5 wine edges remain missing in saturation-only, all **Fruit**-related:
`Fruit ⊑ EdibleThing`, `NonSweetFruit ⊑ Fruit`, `SweetFruit ⊑ Fruit`,
`{Non,Sweet}FruitCourse ⊑ FruitCourse`. Root cause: `Fruit ≡ ObjectUnionOf(NonSweetFruit
SweetFruit)` — a **union class** via `EquivalentClasses`. The needed inferences are
`Fruit ⊑ EdibleThing` (common subsumer of the union — both disjuncts ⊑ EdibleThing) and
`NonSweetFruit ⊑ Fruit` (disjunct ⊑ union-class). Neither is derived because
`disjunction_existential` (common-subsumer Rule 1) only handles `SubClassOf`-Or, NOT
`EquivalentClasses`-Or. **This is a separate, bounded increment B2c (union-class
subsumptions: handle `EquivalentClasses(X, Or)` both directions).** The "sat-only but
not hybrid" Fruit edges are transitively-implied (e.g. `NonSweetFruit ⊑ EdibleThing` ⊂
`NonSweetFruit⊑Fruit⊑EdibleThing`), not FPs.

## Conclusion

B2b is **FP=0**, sound by construction (∀-monotonicity, told subset edges, non-inverse),
and closes the ∀-Course hierarchy — the larger half of the wine saturator gap. Wine
collapse trajectory: **B2c (union-class, the Fruit gap) → saturator complete on wine →
SP-C (route wine to the fast saturation path, completeness-gated).** Default-classifier
output unchanged (foundation).
