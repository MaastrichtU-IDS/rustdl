# `materialize_inferred_data_property_assertions` — inferred data property assertions (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/inferred-data-property-assertions`

Backlog item #1 after `materialize_object_property_assertions` shipped (see
`docs/superpowers/specs/2026-06-21-inferred-property-assertions-design.md`). The
**data**-property analog.

## Why a different mechanism than the object version

The object version reused the ABox saturator's named-individual *edges*. Data-property
assertions, however, lower to **type markers**: `DataPropertyAssertion(dp, a, v)` →
`ClassAssertion(a, ∃dp.DKey(v))` (`convert.rs`). They are **not** named-individual
edges, so the saturator's edge set does not carry them. Instead, data-property
assertion entailment is narrow — OWL 2 data properties cannot be inverse, transitive,
or in role chains — so the only inferences are:
- **sub-data-property hierarchy** (`hasAge ⊑ hasMeasurement`),
- **equivalent data properties** (`hasAge ≡ age`),
- (deferred) SameIndividual, and class-axiom-derived assertions.

These are computable **structurally** from the horned-owl axioms — sound, lossless
(the literal is preserved exactly), and simpler than decoding `∃dp.DKey` markers.

## Soundness framing

Sound (FP=0 analog): every emitted triple is entailed via sub-property /
equivalent-property entailment, re-verified in tests via `entails`. **Complete for the
hierarchy + equivalent-data-property fragment** over named individuals.
**Under-approximate** (documented, deferred): SameIndividual folding and class-axiom-
derived data assertions (e.g. `C ⊑ DataHasValue(dp, v)` + `a:C` ⟹ `dp(a,v)`).
Read-only; classification untouched.

## Algorithm

```
1. consistency pre-check: abox_saturation::saturate_abox_consistency(internal).clash
      → if clash: return Err(ReasonError::Inconsistent)   (parity with object version)
2. scan the ontology's Components:
     DataPropertyAssertion(dp, a, lit)    → asserted (a_iri, dp_iri, value)
     SubDataPropertyOf(sub, sup)          → hierarchy edge sub_iri → sup_iri
     EquivalentDataProperties([d1..dn])    → pairwise edges, both directions
3. closure(dp) = { dp } ∪ { transitive super-data-properties of dp }
4. for each asserted (a, dp, value), for each sup ∈ closure(dp): emit (a, sup, value)
5. dedup + sort for deterministic output
```

Mirrors `data_axioms.rs::closure_sub_dp` (the existing sub-data-property transitive
closure); the implementation may reuse or reimplement the small closure.

## Value representation

A 5-tuple `(subject_iri, property_iri, lexical, datatype_iri, lang)` — fully lossless.
horned-owl `Literal` variants:
- `Literal::Simple { literal }` → `(literal, "http://www.w3.org/2001/XMLSchema#string", "")`
- `Literal::Datatype { literal, datatype_iri }` → `(literal, datatype_iri, "")`
- `Literal::Language { literal, lang }` → `(literal, "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString", lang)`

(Confirm the exact variant names against horned-owl at the pinned rev; `justify.rs`
uses `Literal::Datatype { literal, datatype_iri }`.)

## Surfaces (matching the object version)

- **Reasoner:** `pub fn materialize_data_property_assertions<A: ForIRI>(onto: &SetOntology<A>) -> Result<Vec<(String, String, String, String, String)>, ReasonError>`.
- **Python:** `materialize_inferred_data_property_assertions(path) -> list[tuple[str,str,str,str,str]]` in `owl-dl-py/src/materialize.rs`.
- **CLI:** extend the existing `rustdl realize --properties` to ALSO print a
  `# inferred data property assertions` section (after the object-property section),
  tab-separated `subject<TAB>property<TAB>lexical<TAB>datatype<TAB>lang`. One flag
  shows both. Default off ⇒ realize output byte-identical.

## Inconsistency

Reuses `ReasonError::Inconsistent` (added for the object version). A clash from the
saturator pre-check ⇒ everything vacuously entailed ⇒ return the error rather than a
misleading list (Python raises; CLI prints the note line, as for object properties).

## Testing

- **Reasoner** (in-memory ontologies, declarations included):
  - `hasAge ⊑ hasMeasurement`, `hasAge(a, "30"^^xsd:integer)` ⇒ result contains
    `(a, hasMeasurement, "30", xsd:integer, "")` AND the asserted `(a, hasAge, …)`;
  - equivalent-data-property (`hasAge ≡ age`) ⇒ both directions;
  - **language-tagged** literal (`label(a, "x"@en)`, `label ⊑ name`) ⇒ result carries
    `lang = "en"` and the langString datatype;
  - **negative control** — an un-entailed `(a, hasMeasurement, "99", …)` absent;
  - inconsistent ontology ⇒ `Err`.
- **Soundness property:** every emitted NON-language triple re-checked with
  `entails(Entailment::DataPropertyValue{source, prop, value_lexical, value_datatype})`
  == true. (Language-tagged triples are sound by construction — hierarchy entailment;
  `DataPropertyValue` has no lang field, so they are excluded from this mechanical
  re-check.)
- **Python:** smoke test on the `hasAge ⊑ hasMeasurement` case.
- **CLI:** `realize --properties` prints both object and data sections; without the
  flag the output is unchanged (byte-identical).
- **Corpus / FP-safe:** run on ABox+data fixtures (e.g. shoiq-knowledge / sio if they
  carry data-property assertions) — no panic, spot-check soundness; classification
  closure byte-identical.

## Out of scope (v1 → backlog)

- **SameIndividual folding** — `dp(a,v)` + `a ≡ b` ⟹ `dp(b,v)` (needs union-find over
  `SameIndividual` axioms).
- **Class-axiom-derived data assertions** — `C ⊑ DataHasValue(dp, v)` + `a:C` ⟹
  `dp(a,v)` (needs the `∃dp.DKey` marker-decoding route, harder).
- Anything beyond pass-through of the language tag.
