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

## Inverse-functional contingency — TRIGGERED (forward-only shipped)
The inverse-functional discriminator did NOT fire, and isolation proved the
blocker is a PRE-EXISTING engine gap, not the IR translation: an EXPLICIT
`ObjectMaxCardinality(1, ObjectInverseOf(r))` predecessor-merge over named
individuals ALSO reports `consistent` (`/tmp/inv-explicit.ofn`). The engine
does not perform `≤1 R⁻` predecessor merge. Forward `≤1 R` (explicit and
derived) DOES fire. So:
- Emit FORWARD-only (`FunctionalRole` → `∃R.⊤ ⊑ ≤1 R`). Inverse-functional NOT
  emitted (would be a silent no-op).
- Gate set in `saturator_complete_fragment` is forward-only too (matches emit).
- Inverse-functional documented as a sound MISS; two `#[ignore]`d sentinels in
  `functional_enforcement.rs` (`inverse_functional_predecessor_merge_inconsistent`,
  `inverse_max_cardinality_explicit_is_a_known_sound_miss`) trip when the engine
  learns inverse-role predecessor merging.

## RESULTS
- Discriminators: forward fires (consistent→inconsistent); 3 controls stay
  consistent; user-unqualified-Max-without-functional rejects; derived-Max-GCI
  accepted; EL+functional stays in fragment. Inverse fires-tests `#[ignore]`d.
- Gate 2 closure-diff (single-thread, clean): galen 27997 / notgalen 32739 /
  sio 8904 / wine 653 / ore-10908 6001 / ore-15672 142 / alehif 247 / bibtex 16
  / ro 158 / sulo 51 — ALL FP=0 MISSED=0, NO verdict moved.
- Gate 3 walls (from the closure run): galen 0.49 s, ro 0.03 s — fast path
  preserved (sub-second; the fragment-gate fix held). wine see report.
- Gate 4: full `cargo test --workspace` green.
- Gate 5 (family, informational): family.ofn still `consistent` (capped) — the
  separate scale gap, as the spec predicted; monotonicity (we only ADD axioms)
  means we cannot have flipped it from inconsistent.
- Gate 6 (FP self-review): the derived `Max(1,R,⊤)` flows through the SAME
  `apply_max`/`card_clash_deps` path as user `≤1`; no new dep path. Adding
  `≤1` constraints only shrinks the model set ⇒ can only turn SAT→UNSAT ⇒ only
  ADD genuine subsumptions, never spurious ones. Corpus verdict-neutrality
  (closure-diff) is the empirical confirmation.

## STOP at the soundness gate; report. No merge, no push.
