# rustdl `--json` output schema (v1)

Consumed by the Protégé plugin. Every object carries `"schema_version": 1`.
All arrays are sorted (byte order); pairs are `[sub, sup]`.

## `classify --json`

```json
{ "schema_version": 1, "consistent": bool, "incomplete": bool,
  "unsatisfiable": [iri], "equivalent_groups": [[iri, ...]],
  "direct_subsumptions": [[sub_iri, sup_iri], ...] }
```

`incomplete` = some class pair hit the time budget (defaulted to not-subsumed);
the hierarchy is sound (no false subsumptions) but may miss real ones.

`equivalent_groups` lists only *satisfiable* equivalence classes; unsatisfiable
classes are reported in `unsatisfiable` (they are all mutually equivalent to
`owl:Nothing`).

## `consistent --json`

```json
{ "schema_version": 1, "consistent": bool }
```

## `realize --json`

```json
{ "schema_version": 1,
  "individuals": [ { "iri": iri, "types": [iri], "direct_types": [iri] } ] }
```

## `disjoint --json`

```json
{ "schema_version": 1, "incomplete": bool,
  "disjoint_classes": [[a_iri, b_iri], ...],
  "disjoint_object_properties": [[a_iri, b_iri], ...],
  "disjoint_data_properties": [[a_iri, b_iri], ...] }
```

`disjoint_classes` pairs (`a < b` by IRI, sorted, deduplicated) are entailed
disjoint named classes — `A ⊓ B` proven unsatisfiable, or told-disjoint
(`DisjointClasses`/`DisjointUnion`/etc.) — over satisfiable named classes
(`owl:Thing`/`owl:Nothing` and unsatisfiable classes are excluded).
`disjoint_object_properties` / `disjoint_data_properties` are STRUCTURAL only
(read directly off `DisjointObjectProperties`/`DisjointDataProperties` axioms;
no entailment probe, so they carry no incompleteness of their own).

`incomplete` = `true` iff a per-pair `C ⊓ D` probe timed out, OR the
class-hierarchy pass this query reuses to find the unsatisfiable/candidate
class set was not proven complete (`Classification::completeness_guaranteed()`
— `false` outside the `PureEl`/`Horn` fragment even when nothing timed out).
Sound under-approximation: `disjoint_classes` may be missing entailed pairs
whenever `incomplete` is `true`; it never reports a pair that isn't entailed.

## `property-hierarchy --json`

```json
{ "schema_version": 1, "incomplete": bool,
  "object_properties": {
    "equivalent_groups": [[iri, ...], ...],
    "direct_subsumptions": [[sub_iri, sup_iri], ...]
  },
  "data_properties": {
    "equivalent_groups": [[iri, ...], ...],
    "direct_subsumptions": [[sub_iri, sup_iri], ...]
  }
}
```

Object/data property hierarchies as a STRUCTURAL closure over declared
properties: told + equivalent + inverse subsumption for object properties,
told + equivalent for data properties. No entailment probe is run.
`equivalent_groups` lists each equivalence class (size ≥ 2, sorted);
`direct_subsumptions` are the Hasse edges between distinct groups
(`[sub_iri, sup_iri]`).

`incomplete` is always `false`: this query is complete-by-construction for the
(structural) fragment it reasons over — there is no probe that can time out
or under-approximate.

## `individuals --json`

```json
{ "schema_version": 1, "incomplete": bool,
  "same_groups": [[iri, ...], ...],
  "different_pairs": [[a_iri, b_iri], ...] }
```

`same_groups` = entailed same-individual equivalence groups (size ≥ 2, each
sorted, outer list sorted) — seeded from asserted `SameIndividual` plus the
`ABox` saturator's derived functional-merge, extended by an
inconsistency-of-`KB ∪ {a≠b}` probe. `different_pairs` = entailed-distinct
pairs (`a < b` by IRI) — `a ≠ b` is only ever reported from a PROVEN
`{a} ⊓ {b}` unsatisfiability (no Unique Name Assumption), seeded from told
`DifferentIndividuals`/`AllDifferent`.

`incomplete` = `same_individuals`'s `incomplete() || different_individuals`'s
`incomplete()`. For `same_groups`: `true` iff ANY pairwise extension probe
beyond the sound seed was consulted at all (even one that adds nothing) or
timed out. For `different_pairs`: `true` iff a pairwise `{a} ⊓ {b}` probe
timed out (or hit the deadline-free `NodeCap` safety net). Sound
under-approximation in both directions: a reported group/pair is always
genuinely entailed; `incomplete` warns the set may be missing entailed
merges/distinctions.

## `property-values --json`

```json
{ "schema_version": 1, "incomplete": bool,
  "object_property_values": [[s_iri, p_iri, o_iri], ...],
  "data_property_values": [[s_iri, p_iri, lexical, datatype_iri], ...] }
```

`object_property_values` = the sound `materialize_object_property_assertions`
seed (asserted + sub-property/inverse/symmetric/role-chain/transitive
closure, `ObjectHasValue` ground edges, `SameIndividual` folding — a Horn-only
fixpoint over named individuals) plus a BUDGETED, hard-bounded entailment
extension over the seed's own individual-pair neighborhood (never the full
`|individuals|² × |properties|` cross product). `data_property_values` = a
pure structural passthrough over the inferred data-property-assertion closure
with the `lang` element dropped to a 4-tuple (`[subject, property, lexical,
datatype]`); no entailment probe.

`incomplete` = object values' `incomplete() || data values`' `incomplete()`
(the latter is always `false` — data values are complete for their
structural fragment). Object values' `incomplete` is `true` iff: the
ontology contains an axiom outside `object_property_edge_complete`'s
whitelist — the sound over-approximation of the ABox saturator's OWN
genuinely edge-complete fragment (NOT `analyze_fragment`'s `PureEl`/`Horn`,
which measures a different engine — the classification wedge/EL-saturator —
and was found to under-report: a Horn-classified TBox can still contain a
conjunctive antecedent, e.g. `SubClassOf(A ⊓ B, C)`, that the ABox
saturator's own indexing silently drops in its entirety, missing an edge
while reporting complete); e.g. a conjunctive antecedent or a disjunctive
`ObjectHasValue` case-split can each entail an edge the seed never derives,
and the bounded extension has no candidate pair to even probe when that
happens; OR the bounded extension ran at all (even if it added nothing — the
extension is itself a non-exhaustive, seed-neighborhood-only policy); OR an
extension probe timed out. `false` is a genuine guarantee — every entailed
object-property edge over named individuals is included — and requires BOTH
that every axiom is in the whitelist AND that the seed alone was returned
(no extension candidates). Sound under-approximation: a
reported triple is always genuinely entailed (FP=0); `incomplete` warns real
edges may be missing.
