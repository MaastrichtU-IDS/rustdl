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

## `justify --json`

```json
{ "schema_version": 1, "status": "entailed" | "not-entailed",
  "enumeration_complete": bool, "minimal": bool, "laconic": bool,
  "justifications": [ { "ofn": ofn_document } ] }
```

`status` is `"not-entailed"` iff `justifications` is empty (no query holds
without at least one justification). `minimal` = every returned
justification is *guaranteed* minimal (`Justification::minimal_guaranteed`
across the whole saturator/tableau fragment the query resolved in — some
SROIQ-only entailments return a sound but not-provably-minimal axiom set,
e.g. `--all`-independent disjunctive derivations). `laconic` echoes the
`--laconic` flag (each justification's axioms are weakened to their
responsible fragment when set). `enumeration_complete` is `true` for the
default single-justification query; for `--all`, `false` only when the
`--max` cap genuinely truncated the true set (probed via `max + 1`, so a
returned count that happens to equal `max` is not conflated with a cap).

Each `justifications[i].ofn` is a **self-contained OFN ontology document** —
a fresh ontology holding exactly that justification's axioms (no more, no
fewer), written with the source ontology's prefixes so it re-parses
standalone. This is the shared rendering `prove --json`'s
`justification_fallback` and per-node `conclusion`/`axioms` reuse (see
below).

## `prove --json`

```json
{ "schema_version": 1, "entailed": bool, "has_proof": bool,
  "proof": ProofNodeJson | null,
  "justification_fallback": ofn_document | null }
```

```json
// ProofNodeJson
{ "conclusion": ofn_document, "rule": rule_name,
  "axioms": [ofn_document, ...], "premises": [ProofNodeJson, ...] }
```

Three mutually exclusive shapes, mirroring `ProveEntailmentResult`:

- **Step-level EL proof** (entailment held by the EL saturation fragment):
  `entailed: true, has_proof: true`, `proof` is the recursively-built
  `ProofNode` tree rooted at the queried `SUB ⊑ SUP`, `justification_fallback:
  null`. Each node's `conclusion` is a one-axiom OFN document rendering that
  node's `DerivedFact` (`Sub(s,p)` → `SubClassOf(s p)`; `Exist(s,r,t)` →
  `SubClassOf(s ObjectSomeValuesFrom(r t))`; `Unsat(c)` → `SubClassOf(c
  owl:Nothing)`) — classes beyond the source vocabulary (Tseitin/marker/
  nominal-key/cardinality-key synthetics the saturator introduced) are
  expanded to their defining expression via the same `SyntheticDef` table the
  text `prove` renderer (`render_proof_with_defs`) uses, so the OFN is
  faithful, never fabricated. `rule` is the `ElRule`'s display name (e.g.
  `"ToldSubsumer"`, `"SubsumerTransitivity(fwd)"`). `axioms` are that step's
  cited source axioms — each `node.axiom_refs` index resolved against the
  same `InternalOntology` the query ran over and reverse-converted to a
  horned-owl axiom, each its own one-axiom OFN document; empty for a pure
  transitivity/chain step with no direct axiom. `premises` recurses; a leaf
  step has `premises: []`.
- **Justification fallback** (entailment holds, but not via the EL
  saturation fragment — SROIQ-only, e.g. genuine disjunctive/cardinality
  tableau reasoning): `entailed: true, has_proof: false`, `proof: null`,
  `justification_fallback` is the axiom-level justification's OFN document
  (may be empty-axioms if none could be found).
- **Not entailed**: `entailed: false, has_proof: false`, `proof: null,
  justification_fallback: null`.
