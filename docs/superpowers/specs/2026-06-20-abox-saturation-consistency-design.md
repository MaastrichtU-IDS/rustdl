# Consequence-based ABox saturation for consistency (family gap) — design

**Goal:** detect the family inconsistency (rustdl's one open *correctness* gap — reports
`consistent`, oracle says inconsistent) via a sound, terminating, consequence-based
saturation over named individuals, as a consistency PRE-CHECK. Build **two integration
variants** (A-gated, B) and choose by a **whole-corpus bake-off**.

## Problem (measured this session)

- family is HermiT/Konclude-inconsistent (<1s); rustdl reports `consistent` (timeout-default).
- The clash (from the ddmin core `docs/family-mech4-ddmin-core.ofn`) is **deterministic**
  (0 disjunctions): role chains (`isMalePartnerIn∘hasFemalePartner⊑hasWife`,
  `isWifeOf∘isBrotherOf∘isParentOf⊑isAuntInLawOf`) + `Marriage⊑∃hasFemalePartner.Woman⊓
  ∃hasMalePartner.Man` + `Man≡∃hasSex.Male`/`Woman≡∃hasSex.Female` + `Functional(hasSex)` +
  `Disjoint(Male,Female)` → an individual forced both Man & Woman → `∃hasSex.Male ⊓
  ∃hasSex.Female` → functional-merge → `Male⊓Female` → ⊥.
- **The calculus already works**: main detects the core inconsistent in 0.00s.
- **Full family stalls** because the wedge's `∃`-generation explodes at 508 individuals
  (`Person⊑∃hasFather.Man`, father is a Person ⊑ `∃hasFather.Man` → unbounded ancestor chain)
  — the same scale wall as wine. A det-only wedge pre-check was REFUTED (stalls identically).
- The **EL saturator skips inverse roles** (every role-chain/transitive/functional/hierarchy
  rule guarded by `!is_inverse()`, `lib.rs:2181-2461`) — and family is inverse-heavy.

## What Konclude does (source-profiled)

One **approximated saturation** engine, inverse-capable via **backward role propagation**
(`CRoleBackwardSaturationPropagationHash`): for every edge `R(x,y)` it propagates
consequences both forward and backward (the `R⁻` direction) WITHOUT materializing infinite
witnesses. Inverses do **not** trigger escalation; only `∀` and `≤n`-critical merges do
(`mInsufficientALLCount`/`mInsufficientATMOSTCount`). Used for both classification and the
consistency cascade. ⇒ backward propagation is the sound, proven inverse mechanism.

## Shared core algorithm (identical in both variants)

A fixpoint over **named individuals only** (no generated witnesses ⇒ finite, terminating):

1. **Seed**: each `ClassAssertion(C,a)` → `C∈types(a)`; each `ObjectPropertyAssertion(R,a,b)`
   → edge `R(a,b)`.
2. **Type propagation** (consequence-based, EL-style): `C∈types(a)` + `C⊑D` → `D∈types(a)`;
   `≡` both ways; `∃R.D`-as-type marker (no witness); domain (`R(a,b)`+`dom(R,C)`→`C∈types(a)`),
   range (`R(a,b)`+`rng(R,D)`→`D∈types(b)`).
3. **Role-edge propagation**: property hierarchy (`R(a,b)`+`R⊑S`→`S(a,b)`); **inverse via
   backward propagation** (`R(a,b)` ⟹ consequences flow as if `R⁻(b,a)`); **role chains**
   (`R(a,b)`+`S(b,c)`+`R∘S⊑T`→`T(a,c)`, incl. 3-hop by decomposition).
4. **Functional/`≤1` merge**: `Functional(R)`+`R(a,b)`+`R(a,c)` ⟹ unify the `∃R`-type markers
   of `b,c` (reuse Phase 2a/2e witness-merge): the single `R`-value carries the union of
   their types ⇒ if those include a disjoint pair, clash.
5. **Clash**: `{A,B}⊆types(a)` with `Disjoint(A,B)` (or `A⊑¬B`), or `⊥∈types(a)` ⟹
   **inconsistent** (via the saturator's existing `directly_unsat`/`enqueue_unsat`).
6. No clash ⟹ **no verdict** — fall through to the existing hybrid consistency path unchanged.

**Soundness (FP=0 sacred):** every derived type/edge/merge/clash is entailed (sound rules
only); a reported clash is a real inconsistency. **Under-approximate**: handles only the
deterministic fragment (no `∀`-driven, no `≤n>1`-choice, no disjunctive inconsistencies) —
those don't clash here and fall through (a MISS at worst, never an FP). **Terminating**:
named individuals are a fixed finite set; `∃`-as-type adds no nodes; type/edge sets are
finite ⇒ fixpoint terminates.

## Variant A-gated

Implement the shared core **inside the existing saturator**, with inverse/backward
propagation added but **gated to the ABox/consistency path** (e.g. a `saturate_abox`
entry + a `backward_propagation` mode flag). The EL *classification* path keeps its current
`!is_inverse()` fast behavior byte-identical. *Risk:* touches the tuned saturator — EL
classification walls + FP=0/MISSED=0 must be re-proven.

## Variant B

Implement the shared core as a **separate `abox_saturation` module** (in `owl-dl-reasoner`
or a small new sub-crate), invoked from `is_consistent` between `abox_check` (lib.rs:2120)
and `consistency_wedge` (lib.rs:2138). Reuses `owl-dl-core` IR + role hierarchy; duplicates
the rule-application logic. *Risk:* duplication; piecemeal efficiency (the user's concern).

## Integration (both variants)

`is_consistent`: `abox_check` (P1-P8 patterns) → **ABox-saturation pre-check** (this) → on
clash return `false`; else `consistency_wedge` → tableau. Gated `RUSTDL_ABOX_SATURATION`
(default off during build; flip after the bake-off). `has_abox_axioms()`-guarded so ABox-free
inputs skip it (no cost on galen/ore/etc.).

## Build-order P0 (gates the whole two-variant effort — do FIRST)

Before building both full variants, validate the load-bearing assumption: that the
**non-generating** saturation (`∃`-as-type + functional-merge of markers + role chains +
backward/inverse propagation) actually **reaches family's clash** — the calculus is proven
via the *witness-generating* wedge, not via `∃`-as-type. Build a minimal prototype of the
shared core (Variant B is the faster prototype vehicle — isolated) and confirm:
1. it detects the **family ddmin core** inconsistent via saturation (not the wedge), AND
2. it detects **full family** inconsistent, fast, AND
3. a consistent inverse/chain ABox stays consistent (FP smoke).

If the prototype reaches family's clash → proceed to both full variants + bake-off. If it
does NOT (e.g. functional-merge-of-markers doesn't reproduce the witness-merge clash, or the
chain propagation misses the path) → STOP and rethink the algorithm before investing in two
variants. This is the cheap gate that the session's discipline demands.

## Whole-corpus bake-off (the decision gate; run when BOTH variants complete)

Run for EACH variant, vs flag-off baseline, on the full canonical corpus (`docs/corpus.md`):
1. **Soundness:** `konclude_closure_diff` FP=0/MISSED=0 byte-identical, all fixtures + the
   classification corpus (A-gated must re-prove classification unchanged).
2. **family:** detected **inconsistent**, and fast (target < a few s).
3. **EL walls** (galen, go-basic, ro, sulo, notgalen, bibtex): must stay fast — A-gated's
   key risk (must not slow EL classification); B's cost is only the consistency pre-check.
4. **DL walls + family detection time:** B's key risk (piecemeal efficiency).
5. **Full `perf-flag-sweep.sh`:** no wall regression on any fixture, either variant.

**Decision:** A-gated wins iff EL stays fast AND family solved AND classification FP=0/MISSED=0
unchanged. B wins iff family solved efficiently AND zero classification impact. If both pass,
prefer the simpler/faster (likely B for isolation unless A-gated is clearly faster on family).
The winner merges; the loser's branch is kept as a record.

## Components / files

- **Shared:** `owl-dl-core` role hierarchy + inverse map (exists); the seed extraction
  (`ClassAssertion`/`ObjectPropertyAssertion` from `InternalOntology`).
- **A-gated:** `crates/owl-dl-saturation/src/lib.rs` — `saturate_abox(...)` + backward-prop
  mode (new rules behind the gate); reuse `directly_unsat`/`enqueue_unsat`, functional-merge,
  role chains. Branch `feat/abox-sat-A-gated`.
- **B:** `crates/owl-dl-reasoner/src/abox_saturation.rs` (new) — standalone fixpoint;
  wired into `is_consistent`. Branch `feat/abox-sat-B-standalone`.
- **Tests (both):** family-core canary (already inconsistent) + **full family** (the target)
  + negatives-first (a consistent ABox with inverses/chains must stay consistent — FP guard)
  + the bake-off harness.

## Out of scope
`∀`-driven, `≤n>1`-choice, and disjunctive inconsistencies (fall through to the hybrid path);
classification *completeness* gains from inverses (consistency clash-detection only).
