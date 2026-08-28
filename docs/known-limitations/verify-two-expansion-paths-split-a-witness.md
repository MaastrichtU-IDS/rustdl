# The two model-expansion paths can label one logical witness differently

**Found:** 2026-08-28 (Task 14, `owl-dl-verify`) · **Status:** OPEN, not shown unsound ·
**Severity:** untested completeness risk in a diagnostic-only tool

## The defect

`owl-dl-verify`'s `FiniteModel::build_model` runs **two** expansion passes over the same
saturation closure — `expand` (the fact-driven path, over the saturator's own
`(ClassId, RoleId, ClassId)` fact triples) and `expand_from_axioms` (the axiom-driven path,
walking `InternalOntology.axioms` directly via `materialise_exists`). Both exist because the
saturator emits **no fact** for a nested existential body: `X ⊑ ∃r.∃s.C` gets a Tseitin marker
with an empty subsumer set, so the fact path alone has no element for the nested witness at all
(`expand_from_axioms`'s own doc comment, `crates/owl-dl-verify/src/model.rs:455-467`).

**The two paths can label that same nested witness differently.** In `materialise_exists`'s
`ConceptExpr::Some(role, body)` arm (`model.rs:566-643`), when `body` is itself opaque (no
`required_atoms`, e.g. another nested `∃`) the witness's label is built from `eff.get(&r)` — the
role's *effective ranges* — which is frequently **empty** (`model.rs:613-622`). The fact path
(`expand`, `model.rs:379-434`) instead resolves the same shape via `target_label`, whose `Ok`
arm ultimately bottoms out at `subs.subsumers_of(Tseitin Q)` — and a Tseitin marker's subsumer
set is never empty; it contains at least `{Q}` itself.

`intern` (`model.rs:164-177`) dedups **purely by label content** (`label_ix: HashMap<Box<[ClassId]>,
Element>`), with no notion that two labels might denote the same underlying existential
witness. So when both paths visit the same nested `∃`, they can produce two *different* label
vectors for what is logically one witness — and `intern` allocates **two separate `Element`s**
for it, one under-labelled (from `eff_ranges`, often `[]`) and one correctly labelled (from the
Tseitin marker's subsumers).

This directly contradicts the design spec's "one canonical interpretation" framing (§3/§5 of
`docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`): the model built is
not canonical when the same witness can appear twice under different labels.

## Why this is not (yet) shown to be an unsound checker

The under-labelled element (from `eff_ranges`) carries no edges of its own beyond what
`materialise_exists` immediately builds under it, and an extra, edge-poor element in a finite
model can only make MORE axioms fail to be witnessed, never fewer — `eval::check_axiom`'s
existential checks look for *some* satisfying successor across the whole domain, and an
edge-less duplicate never satisfies one that the correctly-labelled element wouldn't already
satisfy. So the split element does not appear to let a genuinely-failing axiom pass as `Holds`
in the checks this crate currently runs (`eval.rs` only ever checks `SubClassOf` /
`EquivalentClasses` / role-hierarchy shapes over the domain as a whole, never a check keyed to
one specific `Element` by identity).

**What is untested:** whether a future *concept-level* check (e.g. "does element `e`'s label
already entail `D`?", something this crate does not currently do) could read the WEAKER of the
two labels for the same real-world witness and report a false `Violated`, or a check that
depends on iterating "all successors of `x` under `r`" and only visits one of the two split
elements could silently see the wrong one. Nothing in the current evaluator does this, but
nothing in the code prevents a future one from being written that would.

## Fix sketch (not built)

The two paths would need to converge on the target label BEFORE calling `intern`, e.g. by having
`materialise_exists`'s opaque-body branch also union in `subs.subsumers_of` over the Tseitin
markers `expand`'s `target_label` would have reached, rather than falling back to
`eff.get(&r)` alone. That requires exposing the fact path's marker resolution to the axiom path,
which the two functions do not currently share (`model.rs:379` vs `model.rs:545` take disjoint
parameter sets built by different callers in `build_model`).

## Where this is recorded in code

`materialise_exists`'s opaque-body branch (`crates/owl-dl-verify/src/model.rs`, the
`if atoms.is_empty() { ... }` block inside the `ConceptExpr::Some` arm) carries a matching
inline comment pointing back at this file.
