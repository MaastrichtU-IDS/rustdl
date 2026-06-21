# `materialize_inferred_property_assertions` — inferred object property assertions (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/inferred-property-assertions`

Closes a confirmed API gap (external bug report): rustdl reasons over properties
internally, but no API surfaces **inferred object property assertions**. `realize()`
and `materialize_inferred_class_assertions` return only class types; nothing returns
e.g. `hasAncestor(a,b)` entailed by `hasParent ⊑ hasAncestor` + `hasParent(a,b)`.

## Goal

Surface the inferred object property assertions the engine already computes — as a
reasoner function, a Python binding, and a CLI affordance — so users can retrieve
the `(subject, property, object)` triples entailed over named individuals.

## Soundness framing

The ABox-saturation engine already propagates role edges over **named individuals**
(asserted edges + sub-property hierarchy + inverse + symmetric + role chains +
transitivity), and each rule is entailment-preserving. So **every triple returned is
genuinely entailed** (the FP=0 analog). The result is a **sound under-approximation**:
it omits edges to anonymous existential witnesses and edges that need disjunctive
reasoning. Classification/consistency behaviour is untouched — the saturation *logic*
does not change; we only stop discarding its edge set.

## Architecture & data flow

```
convert → ABox saturator (named-individual fixpoint) → RawEdge set
        → map (RoleId → property IRI, IndividualId → individual IRI)
        → sorted, deduped Vec<(subject, property, object)>
```

Three layers, each independently testable:
1. **Engine** (`abox_saturation.rs`): the fixpoint already builds a
   `RawEdge = (RoleId, IndividualId, IndividualId)` set, then drops it (returns only
   counts). Return the final edge set instead. **No change to the saturation logic**
   ⇒ the consistency pre-check is byte-identical.
2. **Reasoner** (`owl-dl-reasoner`): a public
   `materialize_object_property_assertions(onto) -> Result<Vec<(String,String,String)>,
   ReasonError>` mapping edges → IRIs.
3. **Surfaces**: Python `materialize_inferred_property_assertions(path)` +
   CLI `rustdl realize <file> --properties`.

## Engine change

`saturate_abox_consistency` keeps a working set of `RawEdge`s during saturation.
Expose the final set — either as a new field on `SaturationResult`
(`pub edges: Vec<RawEdge>` / a `Vec<(RoleId, IndividualId, IndividualId)>`) populated
by moving the working set out at the end, or via a thin sibling entry point that runs
the same fixpoint and returns `(clash, edges)`. Either way the consistency path
ignores the edges; memory cost is nil (the set already exists; we move rather than
drop). The plan picks the lower-churn option after reading the function.

## Reasoner function

`pub fn materialize_object_property_assertions<A: ForIRI>(onto: &SetOntology<A>) -> Result<Vec<(String, String, String)>, ReasonError>`:
- `convert_ontology(onto)` → run the saturator.
- If the saturator reports a **clash (inconsistent)** → return `ReasonError`
  (everything is vacuously entailed; enumerating is meaningless — see §Inconsistency).
- Otherwise map each edge: `RoleId` → property IRI, `IndividualId` → individual IRI,
  via the vocabulary's id→IRI lookup (the same reverse mapping `realize` /
  `reportable_class_iris` use).
- Exclude `owl:topObjectProperty` and `owl:bottomObjectProperty`.
- Sort + dedup for deterministic output. Return `(subject, property, object)` triples.

## Python binding

In `owl-dl-py/src/materialize.rs`, matching the existing family
(`#[pyfunction]`, `Vec<(String, …)>` tuples, `load::load_path` + `reason_error_to_py`):

```python
materialize_inferred_property_assertions(path) -> list[tuple[str, str, str]]
# (subject_iri, property_iri, object_iri)
```

## CLI

`rustdl realize <file> --properties` — after the per-individual most-specific types,
print a clearly-delimited section:
```
# inferred object property assertions
urn:a	urn:hasParent	urn:b
urn:a	urn:hasAncestor	urn:b
```
Tab-separated `subject<TAB>property<TAB>object` (mirrors classify's `direct\t…`).
**Default off ⇒ `realize` output is byte-identical without the flag** (no breakage for
existing parsers). On an inconsistent ontology the flag prints a one-line note
("ontology inconsistent — all property assertions trivially entailed").

## Semantics

- Returns the **full entailed closure over named individuals** — asserted *and*
  derived triples — matching `materialize_inferred_subclass_axioms` (which includes
  told pairs). The reported case returns both `hasParent(a,b)` and `hasAncestor(a,b)`.
- Reflexive triples `(a, p, a)` are kept (genuine for reflexive properties).
- `owl:topObjectProperty` / `owl:bottomObjectProperty` excluded (analogous to the
  `owl:Thing`/`owl:Nothing` exclusion in the subclass materializer).

## Inconsistency

A clash from the saturator ⇒ the ontology is inconsistent ⇒ every property assertion
is vacuously entailed. The reasoner function returns `ReasonError` (a distinguished
"inconsistent" variant or message); Python raises; the CLI prints the note. We never
return a misleading enumerated list for an inconsistent ontology.

## Testing

- **Reasoner** (in-memory ontologies, declarations included):
  - the reported case — `hasParent ⊑ hasAncestor`, `hasParent(a,b)`, `hasParent(b,c)`
    ⇒ result contains `hasAncestor(a,b)` and `hasAncestor(b,c)`;
  - inverse — `InverseObjectProperties(hasChild, hasParent)`, `hasParent(a,b)` ⇒
    `hasChild(b,a)`;
  - transitivity / a 2-hop role chain ⇒ the composed edge;
  - **negative control** — a non-entailed triple is absent;
  - inconsistent ontology ⇒ `Err`.
- **Soundness property:** every returned `(s,p,o)` re-checked with
  `entails(Entailment::ObjectPropertyAssertion{source:s, prop:p, target:o})` == true.
- **Python:** smoke test calling `materialize_inferred_property_assertions` on the
  reported case.
- **CLI:** `realize --properties` on a fixture prints the section; **without** the
  flag the output is unchanged (byte-identical assertion).
- **Corpus / FP-safe:** run on ABox-bearing fixtures (e.g. family); assert no panic,
  spot-check soundness, and **classification closure byte-identical** (engine logic
  unchanged).

## Deferred — immediate next candidates (keep as the to-do backlog)

In rough priority order — these are the natural follow-ups, not discarded:

1. **Data-property assertions** — mirror this for inferred *data* property assertions
   (`materialize_inferred_data_property_assertions`); data properties already lower to
   the object fragment, so the same saturator edges likely carry them.
2. **Inferred sub-property / property hierarchy axioms** — a
   `materialize_inferred_subproperty_axioms` (the RBox analog of
   `materialize_inferred_subclass_axioms`).
3. **Existential-witness edges** — edges to anonymous individuals from `∃R.C`
   (requires witness enumeration; bounded representation TBD).
4. **Disjunctive-reasoning-derived edges** — property assertions that need the full
   tableau, not the named-individual fixpoint.

(1) and (2) are small and reuse this work; (3) and (4) are larger and align with the
deeper completeness frontier.
