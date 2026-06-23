# SP-B2c: union-class subsumptions (EquivalentClasses-Or) — Design

**Increment B2c of SP-B.** Closes the residual wine **Fruit/union-class gap** —
`Fruit ≡ ObjectUnionOf(NonSweetFruit SweetFruit)`. The last piece making the saturator
complete on wine (after B2b's ∀-Course hierarchy), unblocking SP-C routing.

## Gap

Wine sat-only misses 5 Fruit edges: `Fruit ⊑ EdibleThing`, `NonSweetFruit ⊑ Fruit`,
`SweetFruit ⊑ Fruit`, `{Non,Sweet}FruitCourse ⊑ FruitCourse`. Root: `Fruit ≡
ObjectUnionOf(NonSweetFruit SweetFruit)` via `EquivalentClasses`. `disjunction_existential`
(common-subsumer Rule 1) handles only `SubClassOf`-Or, not `EquivalentClasses`-Or.

## Two sound inferences from `X ≡ D₁⊔…⊔Dₙ`

1. **Common-subsumer** (the `X ⊑ ⊔Dᵢ` direction): `X ⊑ E` for every common told-subsumer
   `E` of all `Dᵢ`. (`X ⊑ ⊔Dᵢ`, `⊔Dᵢ ⊑ E` since each `Dᵢ ⊑ E`.) [`Fruit ⊑ EdibleThing`]
   This is exactly `disjunction_existential`'s bare-Or arm — just reached via Equiv too.
2. **Disjunct ⊑ union-class** (the `⊔Dᵢ ⊑ X` direction, EQUIVALENCE-ONLY): `Dᵢ ⊑ X` for
   each atomic `Dᵢ`. (`Dᵢ ⊑ ⊔Dⱼ ≡ X`.) [`NonSweetFruit ⊑ Fruit`, `SweetFruit ⊑ Fruit`]

**Soundness:** both hold by union semantics. #2 requires the EQUIVALENCE (`⊔Dᵢ ⊑ X`); it
does NOT hold for plain `X ⊑ ⊔Dᵢ` (so it is Equiv-only). #1 holds for both. Told subsumers
⊆ entailment. FP=0 by construction.

**Combines with B2b:** `NonSweetFruit ⊑ Fruit` (#2) + B2b's ForallAtomicKey monotonicity
⟹ `∀hasFood.NonSweetFruit ⊑ ∀hasFood.Fruit` ⟹ `NonSweetFruitCourse ⊑ FruitCourse`. So #1+#2
close all 5 Fruit edges → saturator complete on wine.

## Component

Extend `crates/owl-dl-core/src/disjunction_existential.rs::derive_disjunction_existentials`:
add an `Axiom::EquivalentClasses(members)` arm. For each ordered pair `(a, b)` of members
where `a` is `Atomic(X)` and `b` is `Or(disjuncts)`:
- **#1**: `for e in minimal_common_subsumers(b, concepts, told) { push SubClassOf(Atomic(X), Atomic(e)) }`
  (reuse the existing `minimal_common_subsumers`).
- **#2**: `for d in disjuncts where Atomic(Dᵢ) { push SubClassOf(Atomic(Dᵢ), Atomic(X)) }`.
  Atomic disjuncts only (nominal `⊔` = the value-partition case, handled by ForallKey/
  out of scope here; skip non-atomic disjuncts for #2).

Phase-1 (immutable scan) collects the `(X, e)` and `(Dᵢ, X)` pairs; Phase-2 (mutable)
interns + pushes the `SubClassOf` axioms. Mirror the existing two-phase structure.

## Testing (negatives-first)

- **Fruit canary:** `Fruit ≡ NonSweetFruit ⊔ SweetFruit`, `NonSweetFruit ⊑ EdibleThing`,
  `SweetFruit ⊑ EdibleThing` ⟹ `Fruit ⊑ EdibleThing` (#1) AND `NonSweetFruit ⊑ Fruit`,
  `SweetFruit ⊑ Fruit` (#2).
- **Course combine (B2b×B2c):** add `FruitCourse ≡ MealCourse ⊓ ∀hasFood.Fruit`,
  `NonSweetFruitCourse ≡ MealCourse ⊓ ∀hasFood.NonSweetFruit` ⟹ `NonSweetFruitCourse ⊑
  FruitCourse` (via #2 + B2b monotonicity).
- **Negative — #2 is Equiv-only:** `SubClassOf(X, A⊔B)` (NOT Equivalent) ⟹ NO `A⊑X`/`B⊑X`
  (only #1's common-subsumer if any). Guards against the unsound `X⊑Or ⟹ disjunct⊑X`.
- **Nominal `⊔` not touched (#2 atomic-only):** `X ≡ {a}⊔{b}` ⟹ no `{a}⊑X` atomic push.

## FP=0 gate (sacred)

Tuned closure-diff FP=0/MISSED=0 (12 fixtures); ORE pilot/pool `--saturation-only`
before(B2b)/after(B2c): zero spurious-unsat, saturation ≤ oracle. **Plus the milestone
check: wine `--saturation-only` closure now matches the hybrid (saturator COMPLETE on
wine) — the SP-C precondition.**

## Success criteria

Fruit canary + Course-combine green; negative controls green; wine saturator reaches the
full hybrid closure (gap closed); workspace green; fmt/clippy clean; FP=0 gate clean.
Unblocks SP-C (route wine to the fast saturation path → the collapse).
