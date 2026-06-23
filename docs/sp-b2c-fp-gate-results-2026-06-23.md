# SP-B2c (union-class subsumptions) FP=0 gate — results — 2026-06-23

Gate for B2c: `EquivalentClasses(X, ObjectUnionOf(D₁…Dₙ))` derives, in
`owl-dl-core::disjunction_existential`, (#1) `X ⊑ E` for every minimal common
told-subsumer `E` of the disjuncts, and (#2, equivalence-only) `Dᵢ ⊑ X` for each
atomic disjunct. FP=0 sacred. Verdict: **FP=0 — PASS.**

## 1. Milestone — saturator COMPLETE on wine

`rustdl classify --saturation-only wine.ofn` = **201 edges = the full hybrid
closure (201)**. Zero gap, zero extra. B2b closed the ∀-Course hierarchy; B2c
closes the residual 5 Fruit/union-class edges (`Fruit ⊑ EdibleThing` via #1;
`NonSweetFruit ⊑ Fruit`, `SweetFruit ⊑ Fruit` via #2; `{Non,Sweet}FruitCourse ⊑
FruitCourse` via #2 × B2b ForallAtomicKey monotonicity). **The saturator now
reaches the full oracle closure on wine — the SP-C routing precondition.**

## 2. Tuned closure-diff (oracle, hybrid) — PASS

`konclude_closure_diff`, `RUSTDL_TEST_PAIR_MS=1000`: FP=0 / MISSED=0 on the
oracled fixtures. B2c is additive + sound.

## 3. Soundness argument

- **#1 (common-subsumer):** `X ≡ ⊔Dᵢ`, every `Dᵢ ⊑ E` (told) ⟹ `⊔Dᵢ ⊑ E` ⟹
  `X ⊑ E`. Told subsumers ⊆ entailment. Reuses the existing
  `minimal_common_subsumers`.
- **#2 (disjunct ⊑ union-class):** `Dᵢ ⊑ ⊔Dⱼ ≡ X` ⟹ `Dᵢ ⊑ X`. **Requires the
  EQUIVALENCE** (`⊔Dⱼ ⊑ X`); does NOT hold for plain `X ⊑ ⊔Dⱼ`. Guarded by the
  `EquivalentClasses` arm only — the negative control
  `b2c_subclassof_or_no_disjunct_to_x` pins that a plain `SubClassOf(X, A⊔B)`
  yields no `A⊑X`/`B⊑X`.
- **Atomic-only #2:** non-atomic disjuncts (nominals etc.) skipped for #2 — keeps
  the increment-3 nominal-merge FP boundary untouched.

## 4. Canaries

`b2c_union_class_fruit` (#1 + #2 both fire), `b2c_union_course_combine`
(#2 × B2b monotonicity ⟹ `NonSweetFruitCourse ⊑ FruitCourse`),
`b2c_subclassof_or_no_disjunct_to_x` (negative — #2 is equivalence-only).
B1/B2a/B2b canaries unchanged.

## Conclusion

B2c is **FP=0**, sound by construction, and **completes the wine saturator gap**
(sat-only = hybrid closure). Wine collapse trajectory: B2c done → **SP-C (route
wine to the fast saturation path, completeness-gated like the D10
Horn-shortcircuit) → the collapse (1991 s → seconds).** Default-classifier output
unchanged (foundation).
