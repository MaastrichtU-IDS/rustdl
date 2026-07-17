# Anonymous individuals support — design (2026-07-17)

**Deficiency (D1 in `docs/2026-07-17-deficiency-roadmap.md`).** rustdl rejects any ontology
containing anonymous individuals at conversion time (`ConversionError::AnonymousIndividual`,
"planned for Phase 7"). The 2026-07-16 ORE sweep measured this as **446 / 1920 ontologies (23 %)
unreadable** — the single largest coverage gap (EL 0, DL 136, pure-DL 310; 100 % of the ERR1
failures were this one cause). This spec adds anonymous-individual support so those ontologies
classify and consistency-check instead of erroring.

## Goal & non-goals

**Goal:** parse and reason over ontologies that use anonymous individuals, with the anonymous
individuals treated as first-class domain elements that participate in all ABox/identity reasoning
exactly as named individuals do. Success = the ERR1(anon) subset of the ORE corpus classifies /
consistency-checks (soundly) instead of erroring, with the curated oracle net unchanged.

**Non-goals (YAGNI):**
- Surfacing anonymous individuals in reasoning *output* (`instances`, `materialize_*`,
  `realize`) — decision (a): they are reasoning-internal only, filtered from named-individual
  output surfaces. A later increment can add blank-node reporting if a workload needs it.
- Cross-document / import-level blank-node scoping — rustdl classifies a single loaded ontology;
  blank-node identity is scoped to that ontology, which is correct for this use.

## Approach (chosen: intern as first-class `IndividualId` under a reserved namespace)

Alternatives weighed: (B) conservative distinct-Skolem with no equality reasoning — under-
approximates (misses SameAs/merge entailments) and saves nothing since interning yields equality
for free; (C) preprocess anon→fresh *named* IRIs — leaks synthetic IRIs into the named space and
the reporting surface. (A) is the cleanest: one reserved namespace, one filter.

### Component 1 — interning (the single core change)

`crates/owl-dl-core/src/convert.rs`, `convert_individual` (currently line ~1660). Replace the
rejecting arm:

```rust
// before
Individual::Anonymous(_) => Err(ConversionError::AnonymousIndividual),
// after
Individual::Anonymous(anon) => {
    let label: &str = anon.0.as_ref();
    let synthetic = format!("{ANON_IRI_PREFIX}{label}");
    Ok(vocab.intern_individual(&synthetic))
}
```

- **`ANON_IRI_PREFIX`** — a new reserved constant `pub const ANON_IRI_PREFIX: &str =
  "urn:rustdl-anon:";` in `crates/owl-dl-core/src/convert.rs`, next to the existing
  `DKEY_IRI_PREFIX` (`convert.rs:51`). The prefix cannot appear as a real individual IRI in an
  input ontology.
- **Interning dedupes by string** (`Vocabulary::intern_individual` → `interner.intern`), so the
  same-label→same-id contract holds by construction.
- **Identity by label:** `intern_individual` is idempotent by string, so *same anon label → same
  `IndividualId`* (correct intra-document blank-node identity), and *distinct labels → distinct
  ids*. Distinct anon individuals are not asserted `≠` unless `DifferentIndividuals` says so —
  correct under OWL's no-unique-name assumption.
- **Threading is automatic:** every axiom position that mentions an individual —
  `ClassAssertion`, `ObjectPropertyAssertion`, `NegativeObjectPropertyAssertion`,
  `SameIndividual`, `DifferentIndividuals`, `DataPropertyAssertion`, and `ObjectHasValue` /
  nominal (`Concept::Nominal(IndividualId)`) — routes through `convert_individual` /
  `convert_individuals`. Changing the one function threads anonymous support through all of them
  with no per-axiom edits.
- Keep `ConversionError::AnonymousIndividual` in the enum (dead once this ships) only if other
  code matches it; otherwise remove it and its test (`convert.rs:~2667`), replacing that test
  with a positive interning test.

### Component 2 — identity / no-UNA semantics (the soundness core)

No new machinery. Because an anonymous individual is a plain `IndividualId`, it participates in:
- **SameIndividual** — union-find merge (same code path as named).
- **DifferentIndividuals** — `≠` relation.
- **Functional / `≤n` merge** — two anon witnesses forced onto the same successor merge; forced
  distinct + `≤1` clashes. (This is the advisor-flagged sharp edge; it is exercised by the
  fixtures below.)
- **ClassAssertion / OPA edges / NegOPA / DataPropertyAssertion** (data value via the existing
  DKey lowering, subject anon is fine).

rustdl already assumes **no UNA** (distinctness only via `DifferentIndividuals`), so anonymous
individuals inherit the correct semantics with zero special-casing. The soundness obligation is
purely the reserved-prefix non-collision (Component 1) plus the merge/`≠` behaviours, all covered
by tests.

### Component 3 — reporting filter (decision (a))

Anonymous individuals have no reportable IRI. Add an individual-side analogue of
`reportable_class_iris` (`classify.rs:43`): filter out any individual whose IRI
`starts_with(ANON_IRI_PREFIX)` at every named-individual output surface:
- `instances_of` / `instances_of_saturation_only`
- `materialize_object_property_assertions`, `materialize_data_property_assertions`
- `materialize_sub{object,data}property_axioms` (individuals not involved — no change)
- `materialize_existential_successors`
- `realize --properties` (composed from the above)

Classification (`classify`) output is over **classes** and already uses `reportable_class_iris`,
so the class hierarchy is unaffected. A `realize` over an anon-bearing ontology reports only the
named individuals' entailments; anon individuals contribute to the *reasoning* (e.g. they can make
the ontology inconsistent, which classify mirrors by marking all classes unsatisfiable) but are
never listed as subjects/objects.

## Data flow

```
horned-owl Individual::Anonymous(label)
  → convert_individual: intern urn:rustdl-anon:<label>  → IndividualId
  → axioms (ClassAssertion / OPA / SameAs / DifferentFrom / ≤n / nominal) carry that IndividualId
  → ABox saturation + tableau reason over it uniformly (merge / ≠ / clash)
  → classify: class hierarchy (unaffected) ; consistency: reflects anon-driven clashes
  → reporting surfaces: filtered out by ANON_IRI_PREFIX
```

## Error handling

- Anonymous-individual axioms no longer take the unsupported-axiom path; they are processed.
  Best-effort mode is unaffected for *other* unsupported constructs (still dropped).
- No new error variants. If `ConversionError::AnonymousIndividual` becomes unreferenced, delete it.

## Testing & soundness gate

**Non-regression (must hold byte-for-byte):** the curated oracle net (galen, notgalen, sio, wine,
ore-10908, ore-15672, alehif, pizza, ro, shoiq-knowledge) FP=0 / MISSED=0 **unchanged** — anon-free
ontologies never reach the changed arm, so their closures are byte-identical.

**New anonymous-individual oracle fixtures** (small `.ofn`, adjudicated vs HermiT and/or Konclude),
targeting the identity interactions:
1. `ClassAssertion` on an anon individual + a subsumption/consistency query.
2. Anon `ObjectPropertyAssertion` + `≤1`/functional role: two anon witnesses forced onto one
   successor → merge (consistent); forced distinct (`DifferentIndividuals`) + `≤1` → clash
   (inconsistent).
3. `SameIndividual(anon, named)` → an equality entailment that changes a query answer.
4. `DifferentIndividuals(anon₁, anon₂)` + a merge-forcing constraint → clash.
5. A whole-ontology **inconsistency that exists only because of anon-individual reasoning** (e.g.
   an anon witness of a disjoint pair) → classify marks all classes unsatisfiable, matching the
   oracle.

**Reporting negatives:** an anon individual is present in reasoning but MUST NOT appear in
`instances_of` / `materialize_*` output (assert absence).

**Coverage metric (acceptance):** re-run the ORE ERR1 subset — the anon-error ontologies now
return a classification / consistency verdict (OK / DNF / complete) instead of `ERR1`; the ORE
sweep ERR1 count drops from 446 toward the residual non-anon errors. (DNF on big/hard ones is
acceptable — that is the D2/D4 tail, not this feature.)

## Files touched

- `crates/owl-dl-core/src/convert.rs` — `convert_individual` interning arm; remove/repurpose the
  anon-reject test.
- `crates/owl-dl-core/src/convert.rs` — add `ANON_IRI_PREFIX` next to `DKEY_IRI_PREFIX` (line 51).
- `crates/owl-dl-reasoner/src/classify.rs` / `realize.rs` / `lib.rs` — individual reporting filter
  at the surfaces listed in Component 3.
- `crates/owl-dl-reasoner/tests/` — new anon-individual oracle fixtures + reporting-negative tests.
