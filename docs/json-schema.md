# rustdl `--json` output schema (v1)

Consumed by the Protégé plugin. Every object carries `"schema_version": 1`.
All arrays are sorted (byte order); pairs are `[sub, sup]`.

## `classify --json`

```json
{ "schema_version": 1, "consistent": bool, "incomplete": bool,
  "prep_timed_out": bool, "conversion_timed_out": bool, "parse_timed_out": bool,
  "completeness_guaranteed": bool, "trusted_sat_refutations": int,
  "wedge_hierarchy_blind": bool, "consistency_undetermined": bool,
  "unsatisfiable": [iri], "equivalent_groups": [[iri, ...]],
  "direct_subsumptions": [[sub_iri, sup_iri], ...] }
```

`incomplete` = some class pair hit the time budget (defaulted to not-subsumed);
the hierarchy is sound (no false subsumptions) but may miss real ones.

`prep_timed_out` = the `--global-timeout-ms` budget ran out during preparation
(EL saturation or building the prepared ontology; parsing and conversion are
cut only with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, see `parse_timed_out` and
`conversion_timed_out`), so only a partial saturation closure is reported and
consistency was not fully checked (`consistency_undetermined` is normally set too). Implies
`incomplete`. It can fire at the default settings, whenever the budget is still
meetable when prep bounding is decided. By default a budget already spent at
that point leaves preparation unbounded, and an empty partial closure is re-run
unbounded; `RUSTDL_HARD_GLOBAL_DEADLINE=1` removes both fallbacks, so there the
closure may be empty (#162).

`conversion_timed_out` = with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, the budget ran
out while the ontology was being converted, so only part of it was read (#162).
Axioms past the cut were neither reasoned over nor counted in `dropped`, and
classes they declare may be missing from the output. Implies `prep_timed_out`,
and so `incomplete`. Never set in the default mode, where conversion always
finishes.

`parse_timed_out` = with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, the budget ran out
while the file was still being parsed, so nothing was classified: every list is
empty, `consistent` is `true` only in the sense "not checked", and every other
cut flag (`conversion_timed_out`, `prep_timed_out`, `incomplete`,
`consistency_undetermined`) is set (#162). The parser cannot be interrupted;
the CLI stops waiting for it and exits. `RUSTDL_PARSE_DEADLINE=0` waits for the
parse instead. Never set in the default mode. The Python `Classification` has
no such flag: the Python API parses before any budget applies.

`completeness_guaranteed` = **the flag to read if you want to know whether the
hierarchy can be trusted as complete** (#124). `true` only on a fragment where
the engine that answered is provably complete (pure EL), and only when no pair
timed out, `wedge_hierarchy_blind` and `consistency_undetermined` are false,
`wedge_clauses_refused` and `derived_same_undecided` are 0, and `dropped` is
empty. `docs/assurance/completeness-guarantee-ledger.md` lists each condition
with its source issue and test.

`wedge_hierarchy_blind` = the wedge decided pairs without its role-hierarchy
matching (Layer A off: `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0`, or the role closure
exceeded `RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE`) on an ontology with a
non-trivial role hierarchy (#214). Entailments needing sub-role, inverse or
symmetric reasoning may then be missing. A separate field, because no deadline
fired: it does not set `incomplete`.

`consistency_undetermined` = the KB-level inconsistency pre-check reached no
verdict: a route gave up, or was skipped because `--global-timeout-ms` had
already run out (#162). `consistent: true` then means "no clash found", not
"proven consistent". A separate field; it does not set `incomplete`.

**`incomplete: false` does NOT mean complete.** It means no deadline fired.
There are Horn ontologies where `classify` misses a subsumption both Konclude and
HermiT derive while `incomplete` is `false`, `trusted_sat_refutations` is `0`, and
`consistent` is `true` — every field reading clean on a wrong answer. That is what
`completeness_guaranteed` exists to say, and it is `false` there.

`trusted_sat_refutations` = pairs concluded not-subsumed from the wedge's own
`Sat` verdict outside the fragment where that verdict is complete. An exposure
count, not a defect count; `0` does not imply completeness either.

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
{ "schema_version": 1, "status": "entailed" | "not-entailed" | "unknown",
  "incomplete": bool,
  "enumeration_complete": bool, "minimal": bool, "laconic": bool,
  "justifications": [ { "ofn": ofn_document } ] }
```

`status` is `"entailed"` iff `justifications` is non-empty (no query holds
without at least one justification). `"unknown"` (exit 3) means the top-level
entailment check gave up (#396), so `justifications` is empty but this is NOT a
non-entailment. `incomplete` means a probe gave up (the same meaning as in the
other query JSONs, not classify's deadline-only one): either the top level
(`"unknown"`), or a probe inside the search. In that case every justification
still entails, `minimal` is `false` if a justification's own QuickXplain gave
up, and under `--all` `enumeration_complete` is `false`, since a pruned branch
may hide a justification. Added in #396 without a `schema_version` bump. `minimal` = every returned
justification is *guaranteed* minimal (`Justification::minimal_guaranteed`
across the whole saturator/tableau fragment the query resolved in — some
SROIQ-only entailments return a sound but not-provably-minimal axiom set,
e.g. `--all`-independent disjunctive derivations). `laconic` echoes the
`--laconic` flag (each justification's axioms are weakened to their
responsible fragment when set). `enumeration_complete` is `true` for the
default single-justification query; for `--all`, `false` when the
`--max` cap genuinely truncated the true set (probed via `max + 1`, so a
returned count that happens to equal `max` is not conflated with a cap), or
when a probe gave up during the search.

Each `justifications[i].ofn` is a **self-contained OFN ontology document** —
a fresh ontology holding exactly that justification's axioms (no more, no
fewer), written with the source ontology's prefixes so it re-parses
standalone. This is the shared rendering `prove --json`'s
`justification_fallback` and per-node `conclusion`/`axioms` reuse (see
below).

## `prove --json`

```json
{ "schema_version": 1, "entailed": bool,
  "status": "entailed" | "not-entailed" | "unknown", "incomplete": bool,
  "relies_on_trusted_consistency": bool,
  "has_proof": bool,
  "proof_complete": bool | null,
  "proof": ProofNodeJson | null,
  "justification_fallback": ofn_document | null }
```

```json
// ProofNodeJson
{ "conclusion": ofn_document, "rule": rule_name,
  "axioms": [ofn_document, ...], "axiom_missing": bool,
  "premises": [ProofNodeJson, ...], "unproved_premises": [ofn_document, ...] }
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
  horned-owl axiom, each its own one-axiom OFN document; empty for a step whose
  rule needs no axiom (reflexivity, transitivity, fact inheritance, the unsat
  propagations). `axiom_missing` is `true` when the step's rule does apply an
  axiom but none was recorded for it. `premises` recurses; a leaf step has
  `premises: []`. `unproved_premises` lists, as one-axiom OFN documents,
  premises the step used whose own derivation was not recorded, so no
  sub-proof exists for them. `proof_complete` is `false` when any node has
  `axiom_missing` or a non-empty `unproved_premises`: the entailment still
  holds, but the cited axioms do not show every step. It is `null` when there
  is no step proof.
- **Justification fallback** (entailment holds, but not via the EL
  saturation fragment — SROIQ-only, e.g. genuine disjunctive/cardinality
  tableau reasoning): `entailed: true, has_proof: false`, `proof: null`,
  `justification_fallback` is the axiom-level justification's OFN document
  (may be empty-axioms if none could be found; `incomplete` is then `true`).
- **Not entailed**: `entailed: false, has_proof: false`, `proof: null,
  justification_fallback: null`.

`status` and `incomplete` were added in #402, without a `schema_version` bump.
`status` is `"entailed"` for both entailed shapes. It is `"unknown"` (exit 3)
when the subsumption check gave up (the node cap), in which case the payload has
the not-entailed shape, so `entailed: false` is NOT a non-entailment: read
`status`. `incomplete` means a reasoner check gave up, as in the other query
JSONs. That is the top level (`"unknown"`), or the justification fallback (the
justification may not be minimal, or none was found for an entailment that
holds). `prove` runs under the single-query node cap (`RUSTDL_QUERY_MAX_NODES`,
default 500000), like `subclass` and `explain`.

`relies_on_trusted_consistency` was added in #401, also without a bump. It is
`true` only with `status: "not-entailed"`, when that answer was reached without
the `ABox` (no nominals, so the `ABox` cannot change it on a consistent KB) and
the consistency verdict rests on a trusted wedge `Sat` rather than a
main-tableau model. It is not a give-up and does not set `incomplete`.
