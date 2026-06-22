# Python QA tutorial — make rustdl adoptable, sub-project 4 (design)

**Date:** 2026-06-22
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/manchester-input` (rides on the just-landed `.omn` input work)

Fourth and capstone "make adoptable" sub-project: an end-to-end **"diagnose and fix
a broken ontology with rustdl"** walkthrough — the onboarding face for the Python
debugging surface (debug/justify/diagnose/repair) and the inference-materialization
family shipped this session. Reach / usability only; **no engine change**.

## Form & location

- A single narrative Markdown tutorial: **`docs/python-ontology-qa.md`**.
- Linked from the main **`README.md`** and the PyPI README
  (**`crates/owl-dl-py/README.md`**) so it surfaces both on GitHub and on the
  package page.
- Code is Python (the adoptability target). The CLI equivalents appear as short
  asides, not the main thread.

## Manchester-faced (enabled by the `.omn` input work)

Because rustdl now *reads* Manchester (`.omn`) as well as renders it, every
ontology the reader sees is in **Manchester syntax** — no OWL functional syntax
anywhere. The broken example is a small Manchester string written to a file and
loaded directly; the diagnosis/justification/repair output is already Manchester.

## The two ontologies (both zero-setup, offline)

- **Clean opener** — `rustdl.examples.pizza()` (bundled, coherent, classifies
  completely). Loaded by path; its source is never shown, so "no OFN" holds.
- **Broken example** — a tiny Manchester `.omn` written inline in the tutorial
  (and by the test). The disjoint-topping bug, recognizable to anyone who has done
  the classic pizza tutorial:
  ```
  Prefix: : <urn:pizza#>
  Ontology: <urn:pizza>
  Class: CheeseTopping
  Class: VegetableTopping
  DisjointClasses: CheeseTopping, VegetableTopping
  Class: CheeseyVegetableTopping
      SubClassOf: CheeseTopping, VegetableTopping
  Class: SpicyCheeseyVegetableTopping
      SubClassOf: CheeseyVegetableTopping
  ```
  Root-unsatisfiable: `CheeseyVegetableTopping` (subclass of two disjoint classes).
  Derived: `SpicyCheeseyVegetableTopping` (collateral — clears once the root is
  fixed). This is exactly the shape verified end-to-end during the `.omn` work.

## Narrative arc (the QA workflow)

1. **Install** — `pip install rustdl`.
2. **Classify a healthy ontology** — `classify(examples.pizza())`; inspect
   `.classes`, `.unsatisfiable` (empty), `.complete`.
3. **A broken ontology** — show the Manchester source, write it to `broken.omn`,
   `classify(...)` → `.unsatisfiable` is non-empty. Motivates "why?".
4. **`debug()` — the centerpiece** — `d = rustdl.debug("broken.omn")` returns a
   `Diagnosis`. Walk: `d.consistent`, `d.unsatisfiable`; **`d.roots`** (the root
   `CheeseyVegetableTopping`, its Manchester `.justification`, `.repairs`,
   `.derives`) vs **`d.derived`** (the collateral). Show attribute access, dict
   access (`d["roots"][0]["justification"]`), and `json.dumps(d.to_dict())`.
5. **Zoom in** — `rustdl.justify("broken.omn", ["unsat", "urn:pizza#CheeseyVegetableTopping"])`;
   mention `justify --laconic` and the CLI `diagnose` / `repair` / `report`
   equivalents in a short aside.
6. **Fix & re-check** — remove the `DisjointClasses` axiom (the repair `debug`
   suggested), write the fixed Manchester, re-`debug()` → `d.consistent is True`,
   `d.unsatisfiable == ()`. Closes the loop.
7. **Bonus: materialize inferences** — a small Manchester ABox
   (`hasParent SubPropertyOf: hasAncestor`, `hasParent(a, b)`) →
   `materialize_inferred_property_assertions` yields the inferred `hasAncestor(a, b)`.
   One line each for `materialize_inferred_data_property_assertions`,
   `materialize_inferred_subobjectproperty_axioms`, and
   `materialize_existential_successors`, with the honest caveat that existential
   successors are a *representation* of entailed existentials, not ground triples.

## Rot-prevention (CI-tested workflow)

New pytest **`crates/owl-dl-py/tests/python/test_tutorial.py`** runs the tutorial's
end-to-end workflow as the staleness guard (runs in `python-ci.yml`):

- classify `examples.pizza()` → coherent (no unsat);
- write the broken Manchester `.omn`, classify → unsat present;
- `debug()` → assert `d.consistent`, root == `CheeseyVegetableTopping`, derived
  contains `SpicyCheeseyVegetableTopping`, root has a non-empty justification +
  repairs;
- apply the repair (write the fixed Manchester without `DisjointClasses`),
  re-`debug()` → consistent, no unsat;
- write the Manchester ABox, `materialize_inferred_property_assertions` →
  assert the inferred `hasAncestor(a, b)` tuple is present.

The Markdown snippets are kept in lock-step with this test (same ontology strings,
same calls). If the API drifts, CI fails on the test, signaling the tutorial needs
updating.

## Soundness / scope

Pure documentation + a test over the shipped, sound API. No engine change;
read-only; FP=0 untouched.

## Out of scope (→ later)

RDF/pandas output formats (lane 5); backlog #4 (disjunctive-derived property
assertions); any new capability.
