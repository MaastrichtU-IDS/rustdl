# SP-B2b: general-∀ propagation via ForallAtomicKey — Design

**Increment B2b of SP-B.** Closes the wine/food **Course-hierarchy gap** — the
∀-driven defined-class subsumptions the saturator misses for lack of a general
`∀R.K` rule (e.g. `FishCourse ⊑ SeafoodCourse`). The wine-relevant increment
(supersedes B3: the nominal value-partitions are already handled by ForallKey).

## Gate (passed, measure-first)

Wine `--saturation-only` = 200 edges vs hybrid 201, FP=0; the gap is the food Course
hierarchy, defined *exactly* as `Course ≡ MealCourse ⊓ ∀hasFood.AtomicClass`
(`FishCourse ≡ MealCourse ⊓ ∀hasFood.Fish`, `SeafoodCourse ≡ … ∀hasFood.Seafood`, with
`Fish ⊑ Seafood` told). So `∀R.AtomicClass` monotonicity (`K⊑L ⟹ ∀R.K ⊑ ∀R.L`) closes
it — `ForallAtomicKey` is the exact mechanism.

## Mechanism (mirror the proven ForallKey / DKey synthetic-subsumer pattern)

`∀R.K` (atomic `K`, non-inverse `R`) is lowered to an opaque synthetic class
`ForallAtomicKey(R, K)` (a Tseitin synthetic, like `ForallKey(R,S)` for `∀R.OneOf` and
`DKey` for datatype ranges). Then seed told **monotonicity subset edges**
`ForallAtomicKey(R, K) ⊑ ForallAtomicKey(R, L)` for every told `K ⊑ L`. The existing
conjunctive-trigger + subsumer-closure machinery does the rest:

`FishCourse ≡ MealCourse ⊓ ∀hasFood.Fish` ⟹ body `{MealCourse, ForallAtomicKey(hasFood,Fish)}`.
`SeafoodCourse ≡ MealCourse ⊓ ∀hasFood.Seafood` ⟹ trigger on `{MealCourse,
ForallAtomicKey(hasFood,Seafood)}`. `Fish⊑Seafood` ⟹ edge `ForallAtomicKey(hasFood,Fish)
⊑ ForallAtomicKey(hasFood,Seafood)` ⟹ FishCourse gets `ForallAtomicKey(hasFood,Seafood)`
as a subsumer ⟹ has both of SeafoodCourse's body atoms ⟹ `FishCourse ⊑ SeafoodCourse`.

**Soundness:** `K ⊑ L ⟹ ∀R.K ⊑ ∀R.L` for non-inverse `R` (∀-monotonicity, textbook).
Subset edges use *told* `K ⊑ L` (sound subset of true entailment). `ForallAtomicKey(R,K)`
is a NECESSARY-condition marker for `∀R.K`; it only ever adds entailed ∀-monotonicity
subsumptions — FP=0 by construction. (It is a sufficient-direction under-approximation:
derived-only `K⊑L` and inverse `R` are not handled — a MISS at worst.) No interaction
with `∃` (the ∀+∃ clash is a separate concern, SP1-incr-1, out of scope here): B2b is a
pure subsumption-propagation marker.

## Components (`crates/owl-dl-saturation/src/lib.rs`)

- `forall_atomic_member(c, pool) -> Option<(RoleId, ClassId)>` — recognises
  `All(R, Atomic(K))` with non-inverse `R`. Mirror `forall_oneof_members`.
- `TseitinAllocator`: `forall_atomic_key_by_role: HashMap<(RoleId, ClassId), ClassId>`
  + `introduce_forall_atomic_key(role, k) -> ClassId` (dedup by `(R,K)`). Mirror
  `forall_key_by_role` / `introduce_forall_key`.
- **Lowering**: in the And-conjunction body lowering (the arm near line 2971 that maps
  `∀R.OneOf` → `introduce_forall_key`) and the sibling non-And site (~2869), add an arm:
  `_ if forall_atomic_member(op, pool).is_some() => bodies.push(introduce_forall_atomic_key(R, K))`.
  So a defined class `X ≡ … ⊓ ∀R.K` gets `ForallAtomicKey(R,K)` in its body, and a told
  `X ⊑ ∀R.K` emits `X ⊑ ForallAtomicKey(R,K)`.
- **Subset edges**: after all keys are collected, for each `(R,K)` key, for each told
  super `L` of `K` (`build_told_tables(internal).super_classes(K)`), if `(R,L)` is also a
  key (or introduce it), push `AtomicSubsumption { sub: key(R,K), sup: key(R,L) }`. Mirror
  `seed_dkey_subsumptions`. (Introduce `(R,L)` keys for every told-super `L` so the target
  marker exists even if `∀R.L` wasn't directly written — needed so SeafoodCourse's body
  marker is reachable.)
- **Reportable filtering**: automatic — `ForallAtomicKey` synthetics are Tseitin ids
  outside the vocabulary, so `reportable_class_iris` (iterates the vocabulary) excludes them.

## Testing (negatives-first)

- **Course-hierarchy canary (the differentiator):** `FishCourse ≡ MealCourse ⊓
  ∀hasFood.Fish`, `SeafoodCourse ≡ MealCourse ⊓ ∀hasFood.Seafood`, `Fish ⊑ Seafood` ⟹
  `FishCourse ⊑ SeafoodCourse` (and transitively `BlandFishCourse ⊑ FishCourse ⊑
  SeafoodCourse` with `BlandFish ⊑ Fish`).
- **∀-monotonicity direct:** `X ≡ ∀R.K`, `Y ≡ ∀R.L`, `K ⊑ L` ⟹ `X ⊑ Y`.
- **Negative (no spurious ⊑):** `K ⋢ L` (unrelated) ⟹ `∀R.K ⋢ ∀R.L` (no edge, no
  subsumption). And `X ≡ ∀R.K`, `Y ≡ ∀S.K` with `R ≠ S` ⟹ `X ⋢ Y` (different role).
- **Inverse-role not touched (scope):** `∀R⁻.K` ⟹ no ForallAtomicKey (non-inverse only).
- Wine end-to-end: `--saturation-only wine` closure rises toward 201 (Course hierarchy
  derived), FP=0.

## FP=0 gate (sacred)

Tuned closure-diff FP=0/MISSED=0 (12 fixtures; MISSED may *drop* — recovery — but no FP);
ORE pilot/pool `--saturation-only` before(B2a)/after(B2b) sweep: zero spurious-unsat,
saturation ≤ oracle on oracled onts (= oracle on more, ideally — B2b should *recover*
the Course hierarchy on wine + similar onts). Watch for any `>oracle` (would be an FP in
the monotonicity edges).

## Success criteria

Course-hierarchy canary green; wine saturation-only closure rises (Course subsumptions
derived); workspace green; fmt/clippy clean; FP=0 gate clean. Sets up SP-C: once the
saturator is complete on wine (saturation = oracle), route wine to the fast saturation
path (completeness-gated) ⟹ the wine collapse (1991s-DNF → seconds).
