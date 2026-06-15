# Functional object-property enforcement in the tableau/wedge — Design

**Date:** 2026-06-15
**Status:** Approved (brainstorming + advisor + user), pre-plan
**Author:** rustdl (Michel Dumontier + Claude)

## The gap (honest framing)

**Functional object properties are unenforced in the tableau and the
hypertableau wedge.** `FunctionalObjectProperty(R)` converts to `Axiom::
FunctionalRole(R)` (convert.rs:1588) but is then **dropped** by the wedge
clausifier (`clause.rs` `_ => {}` arm, the same arm that dropped role chains
pre-HF3) and is not translated to a `≤1 R` constraint for the main tableau
either. The EL **saturator** has rich functional-role handling (witness-merge,
ForallKey) so classify of EL ontologies is unaffected — but the tableau/wedge
paths (consistency, ABox-merge, non-EL) never enforce functionality.

**Discriminator (proven empirically):**
- `FunctionalObjectProperty(hasSex)` + `A ⊑ ∃hasSex.Male ⊓ ∃hasSex.Female` +
  `DisjointClasses(Male,Female)` + `ClassAssertion(A,a)`, with `RUSTDL_ABOX_CHECK=0`:
  **wedge → consistent, main tableau (trust_sat=0) → consistent** (WRONG).
- Replace `FunctionalObjectProperty(hasSex)` with explicit
  `ObjectMaxCardinality(1 hasSex)`: **both engines → inconsistent** (CORRECT),
  classify marks `A` unsat.

So the engines *do* perform `≤1`-merge + post-merge disjoint clash on generated
successors; the gap is purely that `FunctionalRole` is never *translated* to
`≤1`. (A1's P8 pre-check catches only the *shallow* directly-asserted case; it
is why the `.ofn` discriminator shows `inconsistent` until abox_check is disabled.)

This is a general completeness gap that the family fixture merely exposed. The
dead-end ledger §21 killed the *shallow A1-pre-check* extension and explicitly
pointed forward to engine-level "role-functionality merging" — which is this
work (+ HF3 chains, already shipped).

## The fix — single-point IR translation

For each `Axiom::FunctionalRole(R)`, **also** emit a derived **role-triggered**
GCI `∃R.⊤ ⊑ ≤1 R` (`ObjectMaxCardinality(1, R)` with the unqualified `≤1`), and
for each `Axiom::InverseFunctionalRole(R)` emit `∃R⁻.⊤ ⊑ ≤1 R⁻`. **Keep the
`FunctionalRole`/`InverseFunctionalRole` axiom itself** — the saturator's
bitset-based handling reads it and must stay untouched.

Why this shape:
- **Single emit point** (in `convert.rs` / a preprocessing pass over the IR
  axioms) feeds *both* engines through machinery already proven by the
  discriminator: the wedge clausifier turns `∃R.⊤ ⊑ ≤1R` into the role-triggered
  clause `R(X,y) → AtMost(R,None,1,X)` (via its existing `∃`-antecedent +
  `ObjectMaxCardinality`-consequent handling), and the main tableau absorbs the
  `≤1` GCI through its existing `apply_max` path.
- **Role-triggered, not global.** `∃R.⊤ ⊑ ≤1R` fires only on nodes that have an
  `R`-successor — not `⊤ ⊑ ≤1R` (which fires on every node). This is the perf
  hedge (functional roles are common; a global ≤1 would add merge work on every
  node).

### Soundness
`FunctionalRole(R)` ≡ `⊤ ⊑ ≤1 R` ≡ (operationally, for clash purposes)
`∃R.⊤ ⊑ ≤1 R` — a node with no `R`-successor trivially satisfies `≤1 R`, so the
role-triggered form is semantically equivalent for satisfiability. The emitted
GCI is *exactly* the axiom's meaning, so it is sound (adds no entailment beyond
`FunctionalRole`). It is **additive**: it can only enable genuine `≤1`-merge
clashes, never spurious ones (the engine's merge is sound). The FP-critical
surface is the **backjump dependency** of the new `≤1` merges (`card_clash_deps`
— residual-C territory); that machinery exists and is hardened, but the new
merges exercise it on a new axiom source ⇒ opus review of the FP direction.

## Scope: forward AND inverse-functional (both in scope)

- **Forward** (`FunctionalRole(R) → ≤1 R`): successor-merge; proven by the
  discriminator (mech3c).
- **Inverse-functional** (`InverseFunctionalRole(R) → ≤1 R⁻`): the
  *predecessor*-merge path, which behaves differently in the wedge's edge
  representation and is **untested**. The plan MUST build the inverse-functional
  analogue of the discriminator FIRST and confirm the wedge merges predecessors
  + fires the disjoint clash. If that path does not already work in the engine,
  it is a deeper engine change — surface it explicitly rather than shipping a
  silent no-op. (Forward must not be blocked on inverse; they land as separate
  verified pieces.)

## Gates (all mandatory)

1. **Discriminator tests (TDD, negatives-first):** forward `FunctionalRole` →
   merge-disjoint clash detected (was missed); inverse-functional analogue →
   detected; a *consistent* control (functional role, two successors with
   *non-disjoint* / same types) stays consistent (no spurious merge-clash).
   Both `.ofn`-level (via `is_consistent` with `RUSTDL_ABOX_CHECK=0` to isolate
   the engine) and wedge white-box.
2. **Corpus closure-diff FP=0/MISSED=0 — the model-validating gate.** Because the
   tuned corpus is *already* MISSED=0, enforcing functional in the engines
   **cannot legitimately move any classify verdict**. The only allowed change is
   a perf-induced `NoVerdict`→not-subsumed timeout. **If any verdict moves for
   any other reason, STOP** — it means the "saturator already covers
   classify-functional" model is wrong and there is something to understand
   before trusting the change. (10/10 fixtures: galen 27997, notgalen 32739,
   sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, bibtex 16,
   ro 158, sulo 51.)
3. **Perf check:** report classify wall deltas on the functional-heavy fixtures
   (galen, ro, wine); a regression is acceptable-but-flag-worthy (role-triggered
   form is the hedge), a *verdict* change is not.
4. **Opus review** of the FP direction (new `≤1` merges → `card_clash_deps`/
   backjump), with a fold-isolating-style discriminator if any new dep path
   appears.
5. **Saturator non-regression:** confirm the saturator ignores the new `≤1` GCI
   gracefully (it should — a `≤1` GCI it can't process is a sound under-approx);
   `FunctionalRole` bitset handling unchanged.

## Family expectation (set now, don't be confused by the result)

This fix + HF3 should let the wedge *reach* family's functional-merge clash. But
family-stripped's 1341-individual ABox will likely still hit the
`FIXPOINT_ITERS` cap (the scale gap; anywhere-blocking already proved it can't
bound the generative nominal ABox). So after this lands, test family and expect
**either** `inconsistent` (scale wasn't binding — bonus) **or** still-capped (→
scale is the lone remaining family gap, the honest stopping point). The value of
this work is the **general** functional-enforcement completeness gain + the
corpus verdict-neutrality proof, NOT the family outcome.

## Out of scope
- The scale/termination gap (`FIXPOINT_ITERS`) — separate, harder, anywhere-
  blocking insufficient.
- `FunctionalDataProperty` (handled separately by DP-2 / data_axioms).
- Changing the saturator's functional handling.
