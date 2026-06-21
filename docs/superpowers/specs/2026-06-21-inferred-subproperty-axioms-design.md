# `materialize_inferred_subproperty_axioms` — inferred property-hierarchy axioms (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/inferred-subproperty-axioms`

Backlog item #2 after the inferred-property-assertions features (#1). The RBox analog
of `materialize_inferred_subclass_axioms` (which returns all entailed `(sub, sup)`
CLASS pairs): the entailed **property-subsumption** closure for object and data
properties.

## Goal

Two reasoner functions + Python bindings returning the entailed named-property
subsumption pairs `(sub_property_iri, super_property_iri)`:
- **object** properties — from `SubObjectPropertyOf` + `EquivalentObjectProperties` +
  inverse interactions + transitivity;
- **data** properties — from `SubDataPropertyOf` + `EquivalentDataProperties` +
  transitivity.

## Approach — structural closure

Computed **structurally** from the horned-owl axioms (no engine query, no per-pair
tableau). Object and data are **separate functions** — they are distinct vocabularies
(though the IR interns both into a shared role table, the source axioms are
unambiguous: `SubObjectPropertyOf` vs `SubDataPropertyOf`). This keeps the buckets
clean by construction and preserves IRIs exactly.

### Object-property closure (signed roles)

Each property expression is a **signed role** `(name, inverse_flag)`:
`p` → `(p, false)`, `ObjectInverseOf(p)` → `(p, true)`.

1. **Edges** from `SubObjectPropertyOf { sub, sup }` where `sub` is a *simple*
   `ObjectPropertyExpression` (skip `ObjectPropertyChain` — chains give complex, not
   simple, subsumption): add `signed(sub) ⊑ signed(sup)`.
2. `EquivalentObjectProperties([e1..en])` → pairwise `⊑` both directions over signed forms.
3. `InverseObjectProperties(p, q)` → `(p,false) ≡ (q,true)` and `(q,false) ≡ (p,true)`
   (four `⊑` edges).
4. **Inverse propagation:** for every edge `(a,fa) ⊑ (b,fb)`, also add
   `(a,¬fa) ⊑ (b,¬fb)` (subsumption is monotone under inverse).
5. **Transitive-close** the edge set over signed roles.
6. **Emit** each closure edge `(a,false) ⊑ (b,false)` (positive→positive) with
   `a ≠ b`, both named object properties, excluding `owl:topObjectProperty` /
   `owl:bottomObjectProperty`. → `(a_iri, b_iri)`.

This derives e.g. `hasChild ⊑ hasDescendant` from `hasParent ⊑ hasAncestor` plus
`InverseObjectProperties(hasParent, hasChild)` and
`InverseObjectProperties(hasAncestor, hasDescendant)`.

### Data-property closure (simple — no inverses)

Transitive closure over `SubDataPropertyOf { sub, sup }` + `EquivalentDataProperties`
(both directions). Emit `(sub_iri, sup_iri)`, `sub ≠ sup`, excluding
`owl:topDataProperty` / `owl:bottomDataProperty`.

## Soundness & scope

- **Sound (FP=0 analog):** every emitted pair is entailed (told / equivalent /
  inverse subsumption are all sound entailments). A test re-verifies each via
  `entails(Entailment::SubObjectProperty/SubDataProperty)`.
- **Complete** for the named simple-property-subsumption fragment (told + equivalent +
  inverse closure = the RBox simple-subsumption relation in SROIQ; role *chains*
  define complex sub-properties, never a simple `p ⊑ q`, so they are correctly
  excluded).
- **Inconsistent ontology → `ReasonError::Inconsistent`** (consistency pre-check,
  parity with the other materializers — an inconsistent KB entails every subsumption).
- Read-only; classification untouched.

## Surfaces

- **Reasoner:** `materialize_subobjectproperty_axioms(onto) -> Result<Vec<(String,String)>, ReasonError>`
  and `materialize_subdataproperty_axioms(onto) -> Result<Vec<(String,String)>, ReasonError>`.
- **Python:** `materialize_inferred_subobjectproperty_axioms(path)` and
  `materialize_inferred_subdataproperty_axioms(path)` → `list[tuple[str,str]]`.
- **No CLI** — matching the Python-only `materialize_inferred_subclass_axioms`
  precedent (the `materialize_*` family is Python-only; `classify`/`realize` cover the
  CLI side). May add later.

## Testing

- **Object** (in-memory, declarations included):
  - `p ⊑ q`, `q ⊑ r` ⇒ `(p, r)` present (transitivity), `(p, p)` absent (reflexive
    excluded);
  - `EquivalentObjectProperties(p, q)` ⇒ both `(p, q)` and `(q, p)`;
  - **inverse case:** `hasParent ⊑ hasAncestor`, `InverseObjectProperties(hasParent,
    hasChild)`, `InverseObjectProperties(hasAncestor, hasDescendant)` ⇒
    `(hasChild, hasDescendant)`;
  - a `SubObjectPropertyOf(ObjectPropertyChain(...), q)` axiom produces **no** simple
    pair (negative control);
  - inconsistent ontology ⇒ `Err`.
- **Data:** `subDP ⊑ midDP ⊑ supDP` ⇒ `(subDP, supDP)`; `EquivalentDataProperties`
  both directions; reflexive excluded.
- **Soundness property:** every emitted object pair re-checked with
  `entails(SubObjectProperty{sub, sup})`, every data pair with
  `entails(SubDataProperty{sub, sup})` == true.
- **Python:** smoke test on the inverse object case.
- **Corpus / FP-safe:** run on RBox-bearing fixtures (ro, galen, family) — no panic,
  spot-check soundness; classification closure byte-identical (read-only).

## Out of scope (v1 → backlog)

- Complex sub-property "axioms" (`chain ⊑ q`).
- Emitting property *equivalence* as a distinct output (equivalence = mutual
  subsumption, derivable from the pairs).
- The remaining backlog: #3 existential-witness edges, #4 disjunctive-derived
  property assertions.
