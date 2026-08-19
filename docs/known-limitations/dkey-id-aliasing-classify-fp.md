# SOUNDNESS: `reportable_class_iris` aliases class ids — false positives in the public `classify()`

**Date:** 2026-08-20
**Severity:** FP ≠ 0 in the shipped public API. This is the failure mode the project treats as
never acceptable.
**Status:** OPEN — reproduced, not fixed. Fixing it touches ~20 call sites and requires a corpus
FP=0 re-validation.
**Pre-existing:** yes. Not introduced by the incremental-reasoning work; found by it.
**Found by:** the Task 8 identity gate (`docs/superpowers/plans/2026-08-19-incremental-reasoning-p1.md`).
First raised as an unreproduced doubt in the Task 7 review, then reproduced.

## The bug

`reportable_class_iris` (`crates/owl-dl-reasoner/src/classify.rs:43`) builds the reported class
vector by enumerating the whole class-id space and then **filtering**:

```rust
(0..internal.vocabulary.num_classes())
    .map(|i| /* iri of ClassId::new(i) */)
    .filter(|iri| !iri.starts_with(owl_dl_core::DKEY_IRI_PREFIX))
    .collect()
```

The filter runs *after* the enumeration, so position `i` in the returned vector is no longer
`ClassId::new(i)` once any `DKey` id has been removed from below it.

But ~20 sites in `classify.rs` map a report-vector index straight back to a class id — the
`owl_dl_core::ClassId::new(u32::try_from(i)…)` pattern at `classify.rs:920`, `:980`, `:1084`,
`:1710`, `:1728`, `:1812`, `:1866`, `:2334` and others (21 `ClassId::new(` sites in the file).

The two agree **only while every filtered-out `DKey` id sits above every reported class.** That
holds for ontologies whose data axioms are lowered last, which is why the curated corpus has not
caught it. It does not hold in general.

## Reproduction

`mie.ofn` — `convert_ontology` puts a `DKey` at id 73 with 83 named classes, so every reported
class above position 73 reads its row off a neighbour:

```
   LocalisedBreastTumour <= PRPositive      direct_api = Ok(false)   <-- FALSE POSITIVE
   Tumour                <= BPMeasurement   direct_api = Ok(false)   <-- FALSE POSITIVE
   Tumour                <= Feature         direct_api = Ok(false)   <-- FALSE POSITIVE
   Tumour                <= InformationObject direct_api = Ok(false) <-- FALSE POSITIVE
   Tumour                <= Quantity        direct_api = Ok(false)   <-- FALSE POSITIVE
   HypertensiveReading   <= BPMeasurement   direct_api = Ok(true)    <-- MISSED
   LocalisedBreastTumour <= Tumour          direct_api = Ok(true)    <-- MISSED
```

Confirmed by two independent oracles:

1. **Inert `Declaration`s change the hierarchy.** Adding declarations that entail nothing shifts
   reported subsumptions — impossible for a correct classifier.
2. **`is_subclass_of` contradicts `classify`.** The direct query API routes through the id space
   without the report projection and disagrees with the hierarchy on the pairs above.

`sulo.ofn` fails the same way (its base half has a DKey below reported classes, producing a
spurious `Capability ⊑ Feature`).

## Why the incremental session makes it worse

Under `convert_ontology_seeded` the ids are no longer in IRI order, so a DKey can be minted below
far more reported classes than in the from-scratch path. A three-axiom session reproducer exists:
`B ⊑ ∃dp.DKey(xsd:integer)`, `C ⊑ B`, then a delta adding `Z ⊑ C` — the session loses entailments
read off the DKey's row.

## Reproducers in the tree

Three `#[ignore]`d tests in `crates/owl-dl-reasoner/tests/incremental_identity_gate.rs`, each of
which **fails when run**:

- `known_bug_dkey_ids_alias_reported_classes_from_scratch`
- `known_bug_dkey_alias_loses_session_entailments`
- (plus the session-path variant)

A fourth test, `gate_fixtures_are_free_of_the_known_dkey_alias_hazard`, **passes** and asserts
that every fixture the identity gate uses is free of the hazard — so the gate is not silently
green because of this bug. `mie.ofn` and `sulo.ofn` are excluded from the gate until it is fixed.

## Fix options (neither taken)

1. **Thread the real id through.** Change `reportable_class_iris` to return `(ClassId, String)`
   pairs and update the four call sites plus the ~20 index-to-id consumers. Correct and explicit;
   the largest diff.
2. **Project last.** Keep report index ≡ `ClassId` throughout and drop DKeys in a final
   order-preserving projection. Smaller, but it makes `classify` loop over DKey subjects, which is
   an unmeasured perf cost and changes `ClassificationStats` counts.

Either way the change needs a full corpus FP=0 / MISSED=0 re-validation before it can be trusted,
which is why it was escalated rather than fixed inside a test task.

## Why the corpus never caught this

Every curated fixture that exercises DKeys lowers its data axioms last, pushing DKeys to the top
of the id space where the aliasing is invisible. The bug needs a DKey interned *below* a reported
class — which `mie.ofn` produces, and which the incremental path produces far more readily.
