# Functional object-property enforcement — TDD plan

**Date:** 2026-06-15
**Spec:** `docs/superpowers/specs/2026-06-15-functional-role-enforcement-design.md`
**Worktree branch:** `worktree-agent-aab99a01ca59f4ef6` (off `main`)
**Status:** in progress. DO NOT merge/push.

## Gap
`Axiom::FunctionalRole(R)` / `InverseFunctionalRole(R)` (convert.rs:1588/1591)
are enforced by the EL saturator (bitset) but DROPPED by the wedge clausifier
(`clause.rs` `_ => {}` at line 364) and never translated to `≤1 R` for the
main tableau. Consistency / ABox-merge / non-EL paths therefore miss
functional-merge clashes.

## Fix — single-point IR translation (convert.rs)
New pass `derive_functional_max_cardinality(&mut out)` run in `convert_ontology`
right before `out.axioms.sort()` (after `decompose_long_chains`). For every
`Axiom::FunctionalRole(R)` emit (in addition, NOT replacing the original):

    SubClassOf { sub = ∃R.⊤ , sup = ≤1 R }
    i.e. pool.some(R, Top)  ⊑  pool.max(1, R, Top)

For every `Axiom::InverseFunctionalRole(R)` emit the same with `R⁻`
(`R.flip()`): `∃R⁻.⊤ ⊑ ≤1 R⁻`.

`Top` filler = unqualified `≤1`. The `FunctionalRole`/`InverseFunctionalRole`
axiom is KEPT untouched (saturator reads it).

### Why it works through existing machinery
- Wedge clausifier: `encode_antecedent(Some(R,Top))` → body `[Role(R,var,y)]`;
  `emit_head(Max(1,R,Top))` → `AtMost(R,None,1,var)`. So the role-triggered
  clause `R(X,y) → AtMost(R,None,1,X)` is produced (clause.rs:503,645).
- Main tableau: absorbs the `≤1` GCI via existing `apply_max`.
- Saturator: a `≤1` GCI it can't process is a sound under-approx (dropped) —
  its `FunctionalRole` bitset handling unchanged.

### Soundness
Emitted GCI is EXACTLY the axiom meaning (`∃R.⊤⊑≤1R ≡ ⊤⊑≤1R` for sat). Additive:
only enables genuine `≤1`-merge clashes. FP surface = backjump deps of the new
merges (`card_clash_deps`) — existing hardened machinery; opus-review the FP dir.

## TDD steps
### Step 1 — discriminator tests FIRST (must FAIL for the *-fires cases)
New `crates/owl-dl-reasoner/tests/functional_enforcement.rs`, harness like
`datatype_inconsistency.rs` (`consistent(body)->bool` via `is_consistent`), with
a `SetEnvGuard`/`ENV_MUTEX` to set `RUSTDL_ABOX_CHECK=0` (isolate engine from A1
P8 pre-check). Tests:
- `forward_functional_merge_disjoint_inconsistent` — `FunctionalObjectProperty(R)`
  `+ A⊑∃R.M⊓∃R.F + DisjointClasses(M,F) + ClassAssertion(A,a)` ⇒ NOT consistent.
- `inverse_functional_predecessor_merge_inconsistent` — `InverseFunctionalObjectProperty(R)`
  arranged so one node has two R-predecessors carrying disjoint types merged by
  `≤1 R⁻` ⇒ NOT consistent.
- `forward_functional_nondisjoint_consistent` (FP guard) — two R-successors with
  SAME / non-disjoint types ⇒ consistent.
- wedge white-box in `hyper.rs` tests if feasible; else rely on engine-level +
  the existing clause.rs `clausifies_objectmax` coverage.

Run: the *-fires tests fail (engine says consistent), the control passes.

### Step 2 — implement `derive_functional_max_cardinality`; fires-tests pass.
### Step 3 — gates: closure-diff FP=0/MISSED=0 all fixtures; saturator workspace test green; perf walls galen/ro/wine; family check (informational).
### Step 4 — fmt + clippy -D warnings; commit per step.

## Inverse-functional contingency
If the inverse `≤1 R⁻` does NOT fire predecessor-merge in the engine: ship
forward-only, document IF as a sound MISS, report the deeper gap. Forward not
blocked on inverse.

## STOP at the soundness gate; report. No merge, no push.
