# rustdl `--json` output schema (v1)

Consumed by the Protégé plugin. Every object carries `"schema_version": 1`.
All arrays are sorted (byte order); pairs are `[sub, sup]`.

## `no_verdict` / `no_verdict_pairs` (#462)

Added without a `schema_version` bump to `classify` (`no_verdict_pairs`, an int) and to
`consistent`, `disjoint`, `individuals`, `property-values`, `justify`, `prove`, `sat-expr` and
`subclass-expr` (`no_verdict`, a bool); since #493 also to `realize` and `instances-expr`
(`no_verdict`, a bool), whose instance probes use `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` (default
750; `0` = no deadline).

**Meaning: a probe ended in `NoVerdict` on an internal limit**, likely a reasoner bug (#449):
- a probe with NO deadline. It searches to a far larger decision depth, so a `NoVerdict` there
  is an internal limit (a saturation stall, as in #437);
- since #475, any probe whose deterministic saturation **stalled** (ran out of iterations),
  budgeted or not.
 The command also prints a "likely a reasoner bug" note on stderr and
**exits 3**. The probe takes its sound default ("satisfiable", "not subsumed", "not entailed",
"consistent"), and `incomplete` (or `status: "unknown"`) is set too.

It does **not** mean "any `NoVerdict`". On a probe with a deadline that did not stall (a pair budget, such as
`classify`'s default `--pair-timeout-ms` or the 1000 ms default of `disjoint`, `individuals` and
`property-values`), a `NoVerdict` is the routine decision-depth cut (#288): it sets only
`incomplete` (and, in `classify`, `depth_capped_pairs` and the depth-limit banner line; in
`realize`, `instances-expr` and `justify`, `depth_capped`), not this field, and the exit code
follows the command's usual give-up convention. So these fields are `false` / `0` on a
budgeted run whatever the tableau did, and `--pair-timeout-ms 0` is the setting under which they
can fire.

**After a positive answer (#493).** The query commands that run the #369 lazy consistency
check (`sat-expr`, `subclass-expr`, `instances-expr`, `individuals`, `property-values`; in text
mode also these, and `instance`, `instances`, `sat`, `subclass` and `explain`) set `no_verdict` when that
check's own probe stalls: the #344 data-counting verify, or the bounded main-tableau tail. Then:
- the answer stands, and `consistency_undetermined` is set with `no_verdict`;
- `incomplete` is NOT set, because the answer itself was decided;
- the command prints #449's note and exits 3.

A decision-depth cut in that check is plain `consistency_undetermined`.

## `classify --json`

```json
{ "schema_version": 1, "consistent": bool, "incomplete": bool,
  "prep_timed_out": bool, "conversion_timed_out": bool, "parse_timed_out": bool,
  "completeness_guaranteed": bool, "trusted_sat_refutations": int,
  "wedge_hierarchy_blind": bool, "wedge_abox_blind": bool,
  "wedge_abox_demoted": int, "consistency_undetermined": bool,
  "matrix_closure_added_pairs": int, "no_verdict_pairs": int,
  "depth_capped_pairs": int,
  "wedge_clauses_deferred": int,
  "consistency_wedge_clauses_refused": int, "consistency_wedge_clauses_deferred": int,
  "unsatisfiable": [iri], "equivalent_groups": [[iri, ...]],
  "direct_subsumptions": [[sub_iri, sup_iri], ...],
  "dropped": { "<axiom kind>": int },
  "collapsed_operands": { "axioms": int, "vacuous": int },
  "universal_role_ordinary": bool }
```

`incomplete` = some class pair or per-class unsat probe was cut without a verdict and took its
sound default (not subsumed; satisfiable), or a preparation cut left a partial closure; the hierarchy is sound (no false subsumptions) but may miss real ones. A cut is
any of: a deadline (per-pair, preparation, conversion or the global one), the node cap, the
tableau's decision-depth limit (counted in `depth_capped_pairs`), a #462 `NoVerdict` (counted
in `no_verdict_pairs`), or a bound-the-tail skip (`RUSTDL_BOUND_DIVERGED_TAIL`,
`RUSTDL_BOUND_STALL_TAIL`). Before #574 this said "hit the time budget" only, which the code
has not matched since the node cap and depth-limit cuts were recorded (#218, #288, #462). With
`RUSTDL_UNSAT_PROBE_CUT_INCOMPLETE=0` a cut per-class unsat probe is not counted.

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

`collapsed_operands` (#538) = n-ary axioms (`DisjointClasses`, `DifferentIndividuals`,
`DisjointUnion`, `Disjoint`/`EquivalentObjectProperties`, …) that repeated an operand.
Their operands are a set (OWL 2 Structural Specification §2.1), so a repeat says
nothing: `DisjointClasses(:C :C)` does not make `C` unsat. `vacuous` counts those left
with one operand, which say nothing at all (a one-member `DisjointUnion`, still `D ≡ C`,
and a one-property `HasKey` are not counted). Not an under-approximation under the set
reading, so it is separate from `dropped` and never affects `completeness_guaranteed`;
Konclude and `HermiT` keep the repeats, which is how a list reading would differ. The count
is taken over every component before conversion, so under a conversion cut it includes
axioms past the cut; `parse_timed_out` reports 0. `consistent --json` carries the same
field.

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
was cut (`incomplete` is false), `wedge_hierarchy_blind`, `wedge_abox_blind` and `consistency_undetermined` are false,
`wedge_clauses_refused` and `derived_same_undecided` are 0, and `dropped` is
empty. `docs/assurance/completeness-guarantee-ledger.md` lists each condition
with its source issue and test.

`wedge_hierarchy_blind` = the wedge decided pairs without its role-hierarchy
matching (Layer A off: `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0`, or the role closure
exceeded `RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE`) on an ontology with a
non-trivial role hierarchy (#214). Entailments needing sub-role, inverse or
symmetric reasoning may then be missing. A separate field, because nothing was
cut: it does not set `incomplete`.

`wedge_abox_blind` = classify trusted a wedge model that touches a nominal while
some `ABox` fact did not apply in it (#533). That wedge sees the facts of the
individuals in the nominal reach only, and object-property assertions only under
`RUSTDL_CLASSIFY_NOMINAL_OPA=1`; a fact it does see applies only on a node that
carries the individual, so a model lacking that individual counts too. A model that touches no
nominal is not counted: it can be united disjointly with any model of the full
KB. A subsumption or an unsatisfiable class that depends on an individual's facts
may be missing. A separate field: nothing was cut, so it does not set
`incomplete`. With `RUSTDL_CLASSIFY_WEDGE_ABOX_DEMOTE=1` (default off) those
models are demoted to the main tableau instead, counted in `wedge_abox_demoted`
(0 with the flag off), and this field stays false. Expect it on almost any input
with both a nominal and an `ABox`: it is set on 353 of the 374 consistent ORE
ontologies of that kind that finish at 60 s (every one of which already had
`completeness_guaranteed: false`).

`consistency_undetermined` = the KB-level inconsistency pre-check reached no
verdict: a route gave up, or was skipped because `--global-timeout-ms` had
already run out (#162), or classify's consistency probe was admitted (an
unsatisfiable class, an incomplete `ABox` run, or counted data values) and gave
up (#446). Under `--saturation-only` it is set whenever the input is not pure
EL (#539): only the saturation closure looked for a clash. `consistent: true`
then means "no clash found", not "proven consistent". A separate field; it does
not set `incomplete`.

`universal_role_ordinary` (#543, no `schema_version` bump) = the ontology uses
`owl:topObjectProperty` (or a property above it, or `owl:topDataProperty` outside the
super-property position) where its universal-role semantics could change an answer, and
rustdl reads it as an ordinary property. Positive answers are sound; a negative answer or
a missing entailment may be wrong. It voids `completeness_guaranteed`, is `false` when the
ontology is proved inconsistent, and comes with one stderr note. The same field, with the
same meaning, is on `consistent`, `realize`, `disjoint`, `individuals`, `property-values`
(where a property above top also sets it: it relates every pair, which the output does not
list), `property-hierarchy`, `sat-expr`, `subclass-expr`, `instances-expr`, `justify`,
`prove` and `verify-el`.

**`incomplete: false` does NOT mean complete.** It means no class pair or per-class unsat probe was cut (other give-ups, such
as `consistency_undetermined` or `derived_same_undecided`, have their own fields).
There are Horn ontologies where `classify` misses a subsumption both Konclude and
HermiT derive while `incomplete` is `false` and `consistent` is `true`. Before #568,
`trusted_sat_refutations` read `0` there too (Horn was exempt), so every field read
clean on a wrong answer; it now reads non-zero (4 on `X ⊑ ∃r.B`, `Domain(r, ∃s.Z)`,
`∃s.Z ⊑ W`), which says verdicts rest on a trusted `Sat`, not which one is wrong.
`completeness_guaranteed` is the field that says the answer may be incomplete, and it
is `false` there.

`trusted_sat_refutations` = verdicts taken on the wedge's own trusted `Sat`
without the tableau (#66, #568): pairs refuted per pair, pairs pruned from the
label cache, snapshot-replay refutations, and classes taken as satisfiable from
their wedge label. A count of decisions, not of distinct pairs (a pair can be pruned
in both the walk and the sweep); undecided pairs below a pruned super are not
included. No fragment is exempt
(`PureEl` reads 0 because no wedge runs there). An exposure count, not a defect
count; `0` does not imply completeness either. The label channels are not gated by
`RUSTDL_HYPERTABLEAU_TRUST_SAT`: to verify everything, set it to `0` together with
`RUSTDL_LABEL_HEURISTIC=0`.

`matrix_closure_added_pairs` = pairs the transitive closure of the recorded
relation added (#448, `RUSTDL_CLASSIFY_CLOSE_MATRIX`, default on; `=0` disables): pairs recorded
not-subsumed (a cut, or a trusted wedge `Sat`) that recorded subsumptions
imply. Each is sound, since every recorded pair is. Diagnostic only: it does
not set or clear `incomplete`. `0` when the closure is off (`=0`). With the closure
on, an incomplete run's `direct_subsumptions` and `equivalent_groups` are
those of the closed relation, so they can differ from the closure-off output
while the transitive closure of the answer is the same.

`no_verdict_pairs` (#462, #475) = of the timed-out pairs, probes (per-class unsat probes and
pair probes) that ended in `NoVerdict` with no deadline (e.g. under `--pair-timeout-ms 0`) or
after their saturation stalled; see the
`no_verdict` section above. Each also counts toward `incomplete`, but not as a depth-limit cut.

`wedge_clauses_deferred` (#600, without a `schema_version` bump) = the classify wedge clausifier's
deferral events: encodings of the input's axioms it skipped, so its trusted `Sat` can miss what they
entail (one axiom can count more than once; 0 is not proof the wedge saw the whole KB). At the
default it counts complex `DisjointClasses` / `DisjointUnion` members (#600); the opt-in
`RUSTDL_CLAUSIFY_COMPLEX_DISJOINT=1` encodes them, except trigger-less ones past a cap of 2 per
clausification, which it defers and counts. Besides those, what we observed deferred on the ORE pool is the antecedent DNF cap; the
reflexive-derived cap can also defer.

`consistency_wedge_clauses_refused` and `consistency_wedge_clauses_deferred` (#558, without a
`schema_version` bump) = when `relies_on_trusted_consistency`, the consistency wedge's refused clauses
and deferral events (as `consistent --json` reports them), 0 otherwise; the refused count also reads 0
with `RUSTDL_REPORT_WEDGE_CLAUSES_REFUSED=0`. That is a different wedge from `wedge_clauses_refused`
here, which is classify's own and clausifies no `ABox`.

`depth_capped_pairs` (#288; in the JSON since #574, without a `schema_version` bump) = of the cut
pairs and probes, those cut by the tableau's decision-depth limit, directly or through early
abandon, rather than by a deadline or the node cap. A larger `--pair-timeout-ms` or node cap does
not recover them; the deadline-free search (`--pair-timeout-ms 0`), whose limit is far larger,
may. Each also counts toward `incomplete`. The text banner's depth-limit line reports the same
count.

`equivalent_groups` lists only *satisfiable* equivalence classes; unsatisfiable
classes are reported in `unsatisfiable` (they are all mutually equivalent to
`owl:Nothing`).

## `consistent --json`

```json
{ "schema_version": 1,
  "consistent": bool,
  "incomplete": bool,
  "relies_on_trusted_consistency": bool,
  "wedge_clauses_refused": int,
  "wedge_clauses_deferred": int,
  "no_verdict": bool,
  "dropped": { "<axiom kind>": int },
  "collapsed_operands": { "axioms": int, "vacuous": int },
  "universal_role_ordinary": bool }
```

`incomplete` (#182) means `consistent: true` is a give-up in its sound
direction (the fall-through tableau's deadline or node cap, the #344
data-counting verification giving up, or (#449) the tableau's internal limit,
`NoVerdict`, on a probe with no deadline, reported on stderr as a likely bug), not a
verdict; the exit code is then 3. A `NoVerdict` on the bounded fall-through probe (the
wedge route, `RUSTDL_CONSISTENCY_FALLBACK_MS`) is the decision-depth cut and counts as
that probe's ordinary give-up (#462), unless the probe's saturation stalled: that is the
likely-bug `NoVerdict` too (#475). On the deadline-free `NoVerdict` path `wedge_clauses_refused` is not reported (0),
`relies_on_trusted_consistency` is `false` and `no_verdict` (#462) is `true`.
`wedge_clauses_refused` (#357) counts clauses the consistency wedge cannot
match, so a wedge `consistent` may miss a clash; 0 when the wedge was not
consulted. `wedge_clauses_deferred` (#558, added without a `schema_version` bump)
counts that wedge clausifier's deferral events: encodings of the input's axioms it
skipped, so the wedge never sees them (one axiom can count more than once, e.g. a
`DisjointUnion` with two complex members reads 3). It has the same consequence.
Unlike `wedge_clauses_refused` it is not silenced by
`RUSTDL_REPORT_WEDGE_CLAUSES_REFUSED=0`, which exists for the refused count's
over-report (#357). Neither 0 proves the wedge saw the whole KB: some drops are not
counted (the axiom kinds the clausifier ignores; and, unless the opt-in
`RUSTDL_CLAUSIFY_COMPLEX_DISJOINT` is on, the pairwise-disjointness half of a
`DisjointUnion` whose members do not encode, which that flag clausifies or counts). `dropped` counts the axioms
rustdl does not reason with, as in `classify --json`.

`relies_on_trusted_consistency` was added in #392, without a `schema_version`
bump. It is `true` when `consistent: true` rests only on the consistency wedge's
`Sat`, which no main-tableau run verified (the wedge is not proven complete).
It is not a give-up: `incomplete` stays `false` and the exit code stays 0. It is
never `true` with `consistent: false`, which is a witnessed clash. The same name
and meaning as in `sat-expr`, `subclass-expr` and `prove --json` (#401).
`RUSTDL_WEDGE_CONSISTENCY=0` decides on the main tableau instead, with no deadline
(`RUSTDL_CONSISTENCY_FALLBACK_MS` bounds only the wedge route's fall-through), so on a hard
ontology it may not finish (wine does not, #490). Its only give-ups are the node cap
(`incomplete`) and the tableau's internal limit (`no_verdict`: a stalled saturation or the
depth cap), #497.

## `realize --json`

```json
{ "schema_version": 1, "incomplete": bool, "witness_prune_active": bool,
  "witness_prune_may_drop": bool, "witness_prune_reasons": [string],
  "direct_types_unverified": bool, "direct_types_depth_capped": bool,
  "relies_on_trusted_consistency": bool,
  "no_verdict": bool, "depth_capped": bool, "saturation_only": bool,
  "universal_role_ordinary": bool,
  "individuals": [ { "iri": iri, "types": [iri], "direct_types": [iri] } ],
  "dropped": { kind: count } }
```

Every listed type is entailed. The flags are separate claims; apart from
`witness_prune_may_drop` (which needs `witness_prune_active`), none implies
another:

- `incomplete` = an instance probe was cut (deadline, node cap or depth limit),
  so a type may be MISSING from `types` and `direct_types`.
- `depth_capped` (#574) = at least one cut instance probe hit the tableau's decision-depth
  limit; implies `incomplete`. A larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or node cap does not
  help; the deadline-free probe (`=0`) may. It covers the instance probes only: a depth cut in
  realize's own classification step sets neither field. Under `--json` the cut warning is also
  printed on stderr since #574.
- `witness_prune_active` = the pseudo-model witness prune was applied, so some
  non-memberships were skipped rather than refuted. It is true on almost every
  off-fragment ontology with individuals.
- `witness_prune_may_drop` = the prune was applied AND its witness has a known
  reason to lack an entailed type (#387), so a type may be MISSING without
  `incomplete` being set. `witness_prune_reasons` names the reasons (empty when
  the prune was not applied):
  - `branched`: the witness search chose a disjunct or a `≤n` merge, so its
    labels are one branch's pre-model;
  - `deferred_clauses`: the clausifier set axioms aside from the witness engine;
  - `refused_clauses`: the witness engine's matcher refuses some clauses;
  - `reseed_net_off`: the #137 re-seed net is not running (`RUSTDL_RESEED_VERIFY=0`
    or `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0`);
  - `inverse_functional_max_off`: `RUSTDL_INVERSE_FUNC_MAX=0`.

  `false` with the prune applied means **no known reason for loss**: the witness
  came from the engine's deterministic closure over its full clause set, with no
  choice taken. It is **not a guarantee**: it rests on that closure being
  complete as implemented, and the bake-off behind it found no loss in either
  bucket, so it has no positive control.
- `direct_types_unverified` = some individual has two `direct_types` whose
  subsumption was left undecided (#388), so a `direct_types` entry may be a strict
  superclass of another; `types` is unaffected. Off the saturation fast path such
  pairs are checked with the main tableau, so it is set only when a check was cut
  (or `RUSTDL_REALIZE_VERIFY_DIRECT=0`). The saturation-only realize runs no check,
  so there any undecided pair sets it. It does not cover a type dropped through
  `incomplete` or the prune, which can also leave a superclass listed. It is also not
  implied by `incomplete`: a cut in realize's own classification step leaves the pair
  to this check, so with both cut, `direct_types_unverified` is set and `incomplete` is
  not (#583).
- `direct_types_depth_capped` (#583, no `schema_version` bump) = with
  `direct_types_unverified`, at least one check was cut by the tableau's decision-depth
  limit: a larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` does not help, `=0` (no deadline) may.
  `false` with `direct_types_unverified` set: a deadline or the node cap cut the checks
  (a larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or `RUSTDL_MAX_NODES` may decide them), or no
  check ran. "At least one": other checks may have been cut otherwise. The text warning
  names the remedies.
- `relies_on_trusted_consistency` (#429): as in `classify --json`; not a give-up.
- `no_verdict` (#493): a probe ended in `NoVerdict` on an internal limit, likely a reasoner bug
  (see the `no_verdict` section): realize's own classification, an instance probe (anonymous
  individuals' included) or a #388 direct-types check. The probe took its sound default ("not an
  instance", or an unchecked pair), `incomplete` is set too, and the command exits 3 after
  printing this JSON. Not covered: the pseudo-model witness build (a stall there only disables
  the prune) and the #369 lazy consistency check.
- `saturation_only` (#539, no `schema_version` bump): the run was `realize --saturation-only`.
  Types come from the EL saturation closure alone, with no tableau probe, so a type may be
  MISSING with every other flag `false`, and an inconsistency the closure does not derive goes
  undetected (an inconsistent ontology entails every type). Every listed type is still
  entailed. `false` on a plain `realize`. The command also prints a stderr note.

`dropped` maps each axiom kind dropped at conversion to its count (empty when
nothing was dropped).

## `disjoint --json`

```json
{ "schema_version": 1, "incomplete": bool,
  "disjoint_classes": [[a_iri, b_iri], ...],
  "disjoint_object_properties": [[a_iri, b_iri], ...],
  "disjoint_data_properties": [[a_iri, b_iri], ...],
  "no_verdict": bool, "consistency_undetermined": bool }
```

`consistency_undetermined` (#369 follow-up, no `schema_version` bump): the property half's
answer is non-empty and the KB's consistency stayed undetermined (see the `*-expr` section). An
inconsistency the consistency wedge finds is an error (exit 1). The class half reports through
classify's own inconsistency verdict.

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
  "consistency_undetermined": bool, "no_verdict": bool,
  "same_groups": [[iri, ...], ...],
  "different_pairs": [[a_iri, b_iri], ...] }
```

`consistency_undetermined` was added in #369 without a `schema_version` bump;
see the `*-expr` section. Here it is set when a probe (not a told or
saturation-derived pair) merged or separated two individuals.

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
  "data_property_values": [[s_iri, p_iri, lexical, datatype_iri], ...],
  "consistency_undetermined": bool, "no_verdict": bool }
```

`consistency_undetermined` (#369 follow-up, no `schema_version` bump): the answer is non-empty
and the KB's consistency stayed undetermined (see the `*-expr` section). An inconsistency the
consistency wedge finds is an error (exit 1).

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
  "incomplete": bool, "no_verdict": bool, "depth_capped": bool,
  "enumeration_complete": bool, "minimal": bool, "laconic": bool,
  "justifications": [ { "ofn": ofn_document } ] }
```

`status` is `"entailed"` iff `justifications` is non-empty (no query holds
without at least one justification). `"unknown"` (exit 3) means the top-level
entailment check gave up (#396), so `justifications` is empty but this is NOT a
non-entailment. `incomplete` means a probe gave up (the same meaning as in the
other query JSONs): either the top level
(`"unknown"`), or a probe inside the search. In that case every justification
still entails, `minimal` is `false` if a justification's own QuickXplain gave
up, and under `--all` `enumeration_complete` is `false`, since a pruned branch
may hide a justification. Added in #396 without a `schema_version` bump.
`depth_capped` (#574, no bump) = a check gave up and at least one instance check hit the
tableau's decision-depth limit, where a larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or node cap does
not help and the deadline-free probe (`=0`) may; it implies `incomplete`, but not that the depth
cut caused the give-up. Only instance checks set it. `minimal` = every returned
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
  "kb_inconsistent": bool, "no_verdict": bool,
  "has_proof": bool,
  "proof_complete": bool | null,
  "proof_uses_non_source_axioms": bool | null,
  "proof": ProofNodeJson | null,
  "justification_fallback": ofn_document | null,
  "justification_minimal": bool | null }
```

```json
// ProofNodeJson
{ "conclusion": ofn_document, "rule": rule_name,
  "axioms": [ofn_document, ...], "axiom_missing": bool,
  "builtin_axioms": [ofn_document, ...],   // omitted when empty
  "derived_axioms": [ofn_document, ...],   // omitted when empty
  "premises": [ProofNodeJson, ...], "unproved_premises": [ofn_document, ...] }
```

Three shapes, mirroring `ProveEntailmentResult`:

- **Step-level EL proof** (entailment held by the EL saturation fragment):
  `entailed: true, has_proof: true`, `proof` is the recursively-built
  `ProofNode` tree rooted at the queried `SUB ⊑ SUP`, `justification_fallback:
  null` — except with `prove --json --source-fallback` (#594): when some step uses a
  non-source axiom (`proof_uses_non_source_axioms: true`: a step has `builtin_axioms`
  or `derived_axioms`), `justification_fallback` also
  carries a source-only justification of the same entailment (the search `justify`
  runs, under `prove`'s query node cap) with `justification_minimal`, for a consumer
  that shows source axioms only (the Protégé plugin shows it instead of the proof). If
  that search fails or finds none, the field stays `null` and a note goes to stderr;
  the proof, `incomplete` and the exit code are unchanged.
  `proof_uses_non_source_axioms` is `null` when there is no proof. Each node's `conclusion` is a one-axiom OFN document rendering that
  node's `DerivedFact` (`Sub(s,p)` → `SubClassOf(s p)`; `Exist(s,r,t)` →
  `SubClassOf(s ObjectSomeValuesFrom(r t))`; `Unsat(c)` → `SubClassOf(c
  owl:Nothing)`) — classes beyond the source vocabulary (Tseitin/marker/
  nominal-key/cardinality-key synthetics the saturator introduced) are
  expanded to their defining expression via the same `SyntheticDef` table the
  text `prove` renderer (`render_proof_with_defs`) uses, so the OFN is
  faithful, never fabricated. `rule` is the `ElRule`'s display name (e.g.
  `"ToldSubsumer"`, `"SubsumerTransitivity(fwd)"`). `axioms` are that step's
  cited source axioms, each its own one-axiom OFN document and each **a component
  of the source ontology as written** (#586; annotations dropped), so a consumer
  can look it up literally: the cited internal axiom is traced back to the source
  component conversion made it from (including the per-axiom rewrites and a long
  chain's decomposition), never reverse-rendered from conversion's normalised form.
  Empty for a step whose rule needs no axiom (reflexivity, transitivity, fact
  inheritance, the unsat propagations). `builtin_axioms` (#539) are axioms
  conversion added from the OWL semantics. `derived_axioms` (#586) are axioms a
  later conversion pass built from one or several source axioms (e.g. a datatype seed
  edge, a data-axiom pattern, the `≤1` GCI of a functional role), so no source axiom is
  their written form; they are rendered from the internal form and
  never listed in `axioms`. `axiom_missing` is `true` when the step's rule does
  apply an axiom but none was recorded for it. `premises` recurses; a leaf step has
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

`kb_inconsistent` was added in #428, also without a bump. It is `true` when the
ontology was found inconsistent: by the named class queries' guard (before the
step proof), or by the #369 lazy check after the entailment held. The entailment
then holds vacuously, the payload has the justification-fallback shape, and since
#456 `justification_fallback` is the inconsistency core: the minimal inconsistent
subset `rustdl justify <file> inconsistent` searches for (`prove` runs it under its own
node cap, so a give-up can leave a different, still inconsistent, set). It is minimal for the
inconsistency, not for `SUB ⊑ SUP`; a smaller set entailing `SUB ⊑ SUP` by
ordinary reasoning is not sought. If the core search finds none, the fallback has
no axioms and `incomplete` is `true`. `false` is not a proof of consistency.

## `sat-expr --json`, `subclass-expr --json`, `instances-expr --json`

```json
{ "schema_version": 1, "incomplete": bool, "completeness_guaranteed": bool,
  "relies_on_trusted_consistency": bool, "consistency_undetermined": bool,
  "no_verdict": bool, "satisfiable": bool }                          // sat-expr
{ "schema_version": 1, "incomplete": bool, "completeness_guaranteed": bool,
  "relies_on_trusted_consistency": bool, "consistency_undetermined": bool,
  "no_verdict": bool, "entailed": bool }                             // subclass-expr
{ "schema_version": 1, "incomplete": bool, "completeness_guaranteed": bool,
  "consistency_undetermined": bool, "no_verdict": bool, "depth_capped": bool,
  "instances": [iri] }                                               // instances-expr
```

- `incomplete`: a probe gave up (node cap; for `instances-expr` also the per-probe deadline or
  depth limit), as in the other query JSONs. `sat-expr`/`subclass-expr` then report the sound
  direction (`satisfiable: true` / `entailed: false`) and **exit 3**; `instances-expr` lists the
  members it proved (sound, possibly short) and exits 0. Until package 2's prerequisite this was
  also `true` whenever the ontology was not pure EL; that is now `completeness_guaranteed`.
- `depth_capped` (`instances-expr` only, #574): at least one cut instance probe hit the tableau's
  decision-depth limit; implies `incomplete`. A larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or node
  cap does not help; the deadline-free probe (`=0`) may. Under `--json` the cut warning is also
  printed on stderr since #574.
- `completeness_guaranteed`: the answer is guaranteed complete: answered on the pure-EL fast path
  (`instances-expr`: the realize saturation fast path) and nothing gave up. `false` means a
  "satisfiable" / "not entailed" / member list is not proven complete, not that it is wrong (the
  meaning `classify --json` uses).
- `relies_on_trusted_consistency` (#401): see the `prove --json` note above.
- `consistency_undetermined` (#369; `instances-expr` since #419, `sat-expr` /
  `subclass-expr` since the follow-up; added without a `schema_version` bump): set only on a
  positive answer (`satisfiable: false`, `entailed: true`, or a probe found a member, anonymous
  members included, so `instances` may be empty) when the KB's consistency stayed undetermined:
  the consistency wedge stalled or ran out of budget (`RUSTDL_QUERY_LAZY_CONSISTENCY_MS`, default
  2000, for `sat-expr` / `subclass-expr`), and the main-tableau tail
  (`RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL=1`) is off by default. On an ontology with no individual
  (#457), an answer the main tableau found runs that tail by default (`RUSTDL_QUERY_NO_SEED_TAIL`)
  and sets the field when it is cut; a told, saturation or query-wedge answer is not checked and
  never sets it (`docs/known-limitations/tbox-only-inconsistency-light-route.md`). On an
  inconsistent KB every such answer holds vacuously, so the answer is then not known to be more
  than that. Not a give-up; does not set `incomplete`.
- `no_verdict` (#462; `instances-expr` since #493): see the `no_verdict` section above. On
  `instances-expr` the listed members are kept (sound, possibly short), `incomplete` is set, and the
  command exits 3 after printing the JSON.
- All three run under the single-query node cap (`RUSTDL_QUERY_MAX_NODES`, default 500000). On an
  inconsistent KB all three error (`Inconsistent`, exit 1) instead of printing JSON. Since #369
  that includes an inconsistency only the consistency wedge finds, checked after a positive
  answer.
