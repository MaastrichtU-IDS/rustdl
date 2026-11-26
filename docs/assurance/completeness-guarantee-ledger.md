# Completeness guarantee ledger

Work package A of the correctness assurance programme (#363). This ledger records
what rustdl means when it says a classification is **complete**, which conditions
that claim rests on, and where each condition is reported and tested.

Checked against `main` at `42457595` (31 October 2026).

## The claim

`ClassificationStats::completeness_guaranteed()` (in
`crates/owl-dl-reasoner/src/classify.rs`) returns `true` only when the reported
hierarchy misses no subsumption. Every public surface reads that one predicate:

- **Rust:** `Classification::completeness_guaranteed()`, which delegates to the
  stats method;
- **CLI:** the `completeness_guaranteed` field of `classify --json`;
- **Python:** the `Classification.completeness_guaranteed` property.

`false` means completeness is not guaranteed. It does not mean the hierarchy is
incomplete. Answers stay sound either way: a reported subsumption is entailed
whether or not the guarantee holds.

Do not confuse the guarantee with `incomplete` (CLI) or `complete` (Python). Those
report only whether a pair or probe was cut (a deadline, the node cap, the decision-depth
limit, a `NoVerdict` or a tail skip; #574 corrected "a per-pair deadline fired"), which is a
strictly narrower thing.
A result can have `complete == True` and `completeness_guaranteed == False`.

## Conditions in the guarantee

All eleven must hold.

| # | Condition | Why it voids the guarantee when it fails | Source | Rust stats field | CLI `classify --json` | Python | Pinned by |
|---|---|---|---|---|---|---|---|
| 1 | `fragment == PureEl` | Only on PureEl is the engine that answers (the EL saturator) complete. `Horn` is a clause-shape label, not an engine property, and a Horn ontology exists on which `classify` misses an oracle-confirmed subsumption. `OutOfFragment` relies on the wedge's trusted `Sat`, which can miss silently. | #124 | `fragment` | not serialized (stderr banner `# fragment:` in non-JSON mode) | not exposed | `completeness_contract.rs`: `pure_el_guarantees_completeness`, `out_of_fragment_does_not_guarantee_completeness`, `horn_clause_shape_does_not_guarantee_completeness`, table test |
| 2 | `timed_out_pairs == 0` | A cut pair is reported "not subsumed" without a verdict. A preparation or conversion timeout also bumps this count and forces `OutOfFragment`. | anytime contract; #162 | `timed_out_pairs`, `timed_out_pair_ids` | `incomplete` (also `prep_timed_out`, `conversion_timed_out`) | `timed_out_pairs`, `complete`, `prep_timed_out`, `conversion_timed_out` | table test; `json_output.rs` `classify_reports_a_cut_preparation_or_conversion_in_its_own_field`; `prep_deadline.rs`; `hard_global_deadline.rs` |
| 3 | `!wedge_hierarchy_blind` | With role-hierarchy matching (Layer A) off, the wedge cannot see sub-role, inverse or symmetric reasoning, and its trusted `Sat` can miss. | #214 | `wedge_hierarchy_blind` | `wedge_hierarchy_blind` | `wedge_hierarchy_blind` | table test; `json_output.rs` `classify_reports_a_hierarchy_blind_wedge_in_its_own_field` |
| 4 | `wedge_clauses_refused == 0` | A clause the wedge's matcher refuses never fires. This is the CLASSIFY wedge. The consistency wedge is a different clause set (it also clausifies the `ABox`); its refused and deferred counts are reported, when classify trusted its `Sat`, as `consistency_wedge_clauses_refused` / `consistency_wedge_clauses_deferred` (#558), and they void the guarantee through condition 8. | #353 | `wedge_clauses_refused` | `wedge_clauses_refused` | `wedge_clauses_refused` | table test |
| 5 | `derived_same_undecided == 0` | An individual equality the KB may force was not decided, so the classify wedge may lack it. The count depends on a budget: probing stops after `RUSTDL_CLASSIFY_DERIVED_SAME_STALLS` (default 8) consecutive stalls, or at `RUSTDL_CLASSIFY_DERIVED_SAME_MAX` (default 64) probes. Candidates already proved distinct (`DifferentIndividuals`, told-disjoint told types) are dropped unprobed and do not count, and candidates are grouped by signed role: a shared object is a candidate only under a bound on the inverse role, and a bound reached through a declared inverse now counts (#368). Before #368 such a pair was never a candidate, so `0` could hide it. On current `main` this condition cannot change the result. `derive_nominal_same_with` looks for candidates only when the ontology has a counted role (`≥`, `≤`, functional or inverse-functional), a nominal, and an asserted `ObjectPropertyAssertion` on a counted role. The PureEl gate admits neither nominals nor property assertions. If a TBox-only path to candidates is ever added, re-check this. | #354 | `derived_same_undecided` | `derived_same_undecided` | `derived_same_undecided` | table test |
| 6 | `!consistency_undetermined` | The inconsistency pre-check reached no verdict, or (#446) classify's consistency probe was admitted and gave up (#466: a cut per-class unsat probe admits it on a `TBox`-only run too, where an empty unsat set had read as consistent). If the KB is in fact inconsistent, every class is unsatisfiable and the hierarchy misses entailments. | #162, #228 review | `consistency_undetermined` | `consistency_undetermined` (reported as `undetermined && !inconsistent`) | `consistency_undetermined` (same masking) | table test |
| 7 | `dropped` is empty | The engine reasoned over a strictly weaker KB than the input. | #43, #246 | `dropped` | `dropped` | module-level `rustdl.dropped_axioms(path)` only, not on `Classification` | `completeness_contract.rs` `a_dropped_axiom_voids_the_guarantee` and its control; table test; `json_output.rs` `classify_json_completeness_guaranteed_ends`; `test_classify.py` `test_dropped_axiom_voids_the_guarantee` |
| 8 | `!relies_on_trusted_consistency` | "Consistent" rests on a consistency-wedge `Sat` that no main-tableau run verified. If that `Sat` is wrong, the KB is inconsistent and every class is unsatisfiable, so the hierarchy misses entailments. **This cannot fail beside condition 1 today.** PureEl admits no `ABox` assertion and no nominal, so no individual exists, and the consistency wedge (`classify_wedge_consistency`) needs one to run. The probe that could also trust it runs only after the PureEl dispatch. The condition is kept so the invariant holds if PureEl ever admits an individual. | #429 | `relies_on_trusted_consistency` | `relies_on_trusted_consistency` | `relies_on_trusted_consistency`, plus `TrustedConsistencyWarning` | table test; `trusted_consistency_surfaces.rs` `pure_el_and_trust_never_meet` |
| 9 | `!(universal_role_ordinary && !inconsistent)` | rustdl reads `owl:topObjectProperty` (and any property above it, and `owl:topDataProperty` outside the super-property position) as an ordinary property. That is sound for positives, but it misses entailments of `Δ × Δ`. For example, `⊤ ⊑ ∃r.D` with `∃top.D ⊑ ⊥` is inconsistent, yet on main it read as consistent with this guarantee set. Inert occurrences are not flagged: `x ⊑ top`, a positive `top(a, b)`, Transitive/Reflexive/Symmetric, equivalence and inverse within the universal roles, a chain into them, and #539's built-ins. A proved inconsistency masks the condition, since every entailment holds then. Detection is `owl_dl_core::top_bottom_roles::universal_role_ordinary`. | #543 | `universal_role_ordinary` | `universal_role_ordinary` (masked by `inconsistent`), plus a stderr note | `universal_role_ordinary`, plus `UniversalRoleWarning` | table test; `completeness_contract.rs` `a_proved_inconsistency_masks_the_universal_role_condition`; `top_bottom_roles.rs` (core) detector tests; `universal_role_cli.rs` |
| 10 | `!wedge_abox_blind` | Classify's wedge is given the `ABox` facts of the nominal reach only (no object-property assertion unless `RUSTDL_CLASSIFY_NOMINAL_OPA=1`), so its models are models of a weaker KB; a fact it is given applies only on a node carrying that individual. A trusted model that touches a nominal while some fact did not apply in it (not given, or given for an individual the model does not instantiate) can then hide a subsumption or an unsatisfiable class. The no-nominal argument: a completion with no nominal-labelled node shares no element with the named individuals, so it can be united disjointly with any model of the full KB, and unseen facts cannot invalidate it; only models that touch a nominal count. **This cannot fail beside condition 1 today:** PureEl admits no nominal. The condition is kept so the invariant holds if PureEl ever admits one. `RUSTDL_CLASSIFY_WEDGE_ABOX_DEMOTE=1` (default off) demotes such models instead (`wedge_abox_demoted`). | #533 | `wedge_abox_blind` (count: `wedge_abox_blind_models`) | `wedge_abox_blind` | `wedge_abox_blind` | table test; `wedge_abox_blind.rs` |
| 11 | `!tbox_individuals_unseeded` | Every named individual exists in every model, including one named in a `TBox` nominal (`{b}`, `ObjectHasValue(S b)`) with no `ABox` fact. What the `TBox` says about it (`{b} ⊑ ∃S.{a}`) then constrains every class whose model reaches `{a}`. The main tableau's probes seed only the `ABox` individuals, so they leave out one with no `ABox` axiom at all. The wedge instantiates no individual on its own, so it also leaves out one the `ABox` names only in `DifferentIndividuals`, which gives it no `{b}` fact and is not counted by condition 10. A `{b}`-antecedent rule then never fires, and a subsumption or an unsatisfiable class can be missed; `subclass`, seeded since #356, finds it. `RUSTDL_CLASSIFY_TBOX_INDIVIDUALS=1` (default off) seeds those individuals in both probes, and this is then false unless the wedge's nominal semantics are off (`RUSTDL_CLASSIFY_WEDGE_NOMINALS=0`). With no nominal in any clause head the wedge needs no seed (the disjoint-union argument of condition 10). **This cannot fail beside condition 1 today:** PureEl admits no nominal. The condition is kept so the invariant holds if PureEl ever admits one. | #608 | `tbox_individuals_unseeded` (count: `tbox_individuals_unseeded_count`) | `tbox_individuals_unseeded` | `tbox_individuals_unseeded` | table test; `classify_tbox_individuals.rs`; `json_output.rs` `classify_reports_unseeded_tbox_individuals_in_their_own_field` |

The table test is `each_condition_alone_voids_the_guarantee` in
`crates/owl-dl-reasoner/tests/completeness_contract.rs`. It starts from the one
state that guarantees completeness and flips each condition on its own. Deleting a
conjunct from the predicate makes exactly that row fail, so the test is also the
guard's sabotage check.

## The consistency wedge's weaker KB (#558)

The consistency wedge's `Sat` is trusted (condition 8). Its clause set can be
weaker than the input in two ways: clauses its matcher refuses never fire, and
encodings its clausifier defers it never sees. A model of that weaker KB need not
be a model of the input, so the trusted `Sat` can hide an inconsistency (a miss,
never a false positive). The deferred count is of deferral events, not axioms (a
`DisjointUnion` with two complex members reads 3), and 0 does not prove the wedge
saw the whole KB: the axiom kinds the clausifier ignores drop clauses uncounted, and
so, unless the opt-in `RUSTDL_CLAUSIFY_COMPLEX_DISJOINT` is on, does the pairwise-disjointness half
of a `DisjointUnion` whose members do not encode (that flag clausifies or counts it).

#600 adds an **opt-in** fix (`RUSTDL_CLAUSIFY_COMPLEX_DISJOINT=1`, default OFF) that
encodes complex `DisjointClasses` / `DisjointUnion` members and the `DisjointUnion`
covering, the deferrals behind the live misses #558 found. At the default those
misses remain, and `wedge_clauses_deferred` (classify) and the consistency counts
report them. Opt-in because its cost is unbounded on growing ontologies: one
trigger-less fallback beside a 200-class chain took 24.8 s and was cut (main 0.07 s,
complete), and soft-trigger fallbacks with range axioms blow up too (64 axioms:
21.7 s, cut); a cut loses answers main finds. A default flip waits for a cost guard
(follow-up issue). With the flag on, a fallback with no soft trigger
(no body-encodable conjunct: `∀r.B`, `≤1 s.C`, `∀r.B ⊓ ∀s.C`, …) is a `⊤`-bodied
disjunctive clause whose cost grows exponentially with their number, so at most
`TRIGGERLESS_FALLBACK_CAP` (2) are emitted per clausification; the rest are deferred
under their pre-#600 kinds and counted (a trigger-less `DisjointUnion` pair as
`disjoint-union-pair`). Besides those, what we observed deferred on the ORE pool is the
antecedent DNF cap (one file, `ore_ont_8142`, two deferrals); the reflexive-derived cap
can also defer. `classify --json` also reports the classify
wedge's own deferrals as `wedge_clauses_deferred`.

Surfaces that carry the counts (#558):
- `consistent --json`: `wedge_clauses_refused` / `wedge_clauses_deferred`, whenever
  the wedge was consulted, with a stderr warning on a trusted `consistent`;
- `classify --json` and `ClassificationStats`: `consistency_wedge_clauses_refused` /
  `consistency_wedge_clauses_deferred`, only when classify relied on that `Sat`
  (the refused count also obeys `RUSTDL_REPORT_WEDGE_CLAUSES_REFUSED=0`, as on
  `consistent`; the deferred count deliberately does not, since that flag is for
  the refused count's #357 over-report);
- `QueryStats::wedge_clauses_refused` / `wedge_clauses_deferred` from
  `is_consistent_with_stats`, and from a single query that dropped its `ABox` on
  that `Sat`;
- Python `is_consistent`: the counts in the `TrustedConsistencyWarning` text.

Known gaps (no count reaches the caller; for #600 or a follow-up): `realize`,
`sat-expr`, `subclass-expr`, `justify` and `prove` JSON; plain-text `classify`
stderr; the Python `Classification` accessors; the CLI reads `QueryStats`'s
deferred count only on `consistent`; the bounded API (`is_consistent_with_timeout`
returns no stats).

**Part B (verifying such a `Sat`) is not built: its measured reach is one
ontology.** Recorded 2026-11-26 by rustdl-manager under Michel's delegation. The
reach census (`consistent --json`, main `22a69b81`, 60 s cap, over the 1,176 ORE
files that name an individual or a nominal) found 944 trusted consistency-wedge
`Sat`s, **0** with refused clauses (instrument check: the #357 star with
`RUSTDL_WEDGE_BODY_DECOMPOSE=0` reads 1), and **1** with a deferred axiom
(`ore_ont_12698`, one `disjoint-antecedent`; second pass with the part-A binary).
The root cause is clausification: the deferred `DisjointUnion` / `DisjointClasses`
kinds also cost answers on the classify wedge, so they are fixed in the clausifier
(the follow-up issue) rather than verified per query. Should verification ever be
built, the decision on its give-up stands: **a verify give-up keeps today's
output** (`consistent`, `relies_on_trusted_consistency: true`, the gap counts, no
`incomplete`), because those flags already name the gap and turning a consistent
KB into `unknown` (exit 3) would lose answers, against "defaults favour
completeness". Considered and rejected: reporting the give-up as
`incomplete: true` per #574's contract, the #344 behaviour.

## Signals deliberately outside the guarantee

| Signal | Why it is not a separate condition |
|---|---|
| `prep_timed_out`, `conversion_timed_out` | Covered by condition 2: `classify_prep_timeout` bumps `timed_out_pairs` and forces `fragment = OutOfFragment`. |
| `parse_timed_out` (CLI only) | Nothing was read. `parse_timed_out_classify_json` in `json_out.rs` sets `completeness_guaranteed: false` directly, pinned by `parse_deadline.rs`. |
| `trusted_sat_refutations` | Excluded by composition, not by the predicate: a PureEl classification is answered by the saturator and never consults the wedge's trusted `Sat`, for a pair or for a class. If PureEl ever routes pairs or unsat probes through the wedge, this needs a condition. Since #568 the count covers every classify channel that trusts a wedge `Sat` (per-pair refutations, label-cache prunes, snapshot replays, and classes taken as satisfiable from their label) and exempts no fragment, so it reads non-zero on `Horn` too; it is 0 on PureEl by construction, pinned by `nn_cascade.rs` `issue_568_pure_el_reads_zero_trusted_sat`. The consistency wedge's `Sat` is the other trust site, and it is condition 8. |
| `incomplete` (CLI), `complete` (Python) | These report only that a pair or probe was cut (#574: not only by a deadline). They are a view of condition 2, kept separate on purpose: their contract is "something was cut", and callers depend on it. |
| Inconsistent KB | `classify_inconsistent` builds normal stats with the ontology's fragment. A pure-EL inconsistent KB with nothing cut or dropped therefore reports `completeness_guaranteed: true`. That is correct: every class is unsatisfiable and every subsumption entailed, which is exactly what is reported. Pinned by `json_output.rs` `classify_json_completeness_guaranteed_ends` (`⊤ ⊑ ⊥`). An inconsistent KB with an ABox is not PureEl, so it reports `false`. |

## Known gaps in the surfaces

- **Python does not expose the fragment** and reports dropped axioms only through
  a separate `dropped_axioms(path)` call, except for the class-expression queries
  (#552: their `*_result` forms carry `dropped` and `completeness_guaranteed`, and
  they emit `DroppedAxiomsWarning`). The guarantee property is the way to read
  conditions 1 and 7 from a `Classification`. No other Python query warns on dropped
  axioms; widening `DroppedAxiomsWarning` to them is a separate follow-up.
- **The JSON schema does not serialize the fragment.** A caller sees why the
  guarantee is false for conditions 2–7, but not for condition 1.
- **Any ABox takes an ontology out of PureEl.** A pure-EL TBox with one
  `ClassAssertion` is not PureEl (measured: the banner reads `Horn`) and reports `false`, even though it runs
  saturation-only. That is conservative (a missing guarantee, never a false one),
  and widening it needs its own completeness argument.
- **The guarantee predicate covers `classify` only.** The expression queries and
  `explain` make their own narrower claim (next section). `realize`, `consistent`,
  `sat` and the inferred-query surface have their own partial signals (`incomplete`,
  `witness_prune_active`, `witness_prune_may_drop`, `direct_types_unverified`
  on `realize`,
  `consistency_undetermined`, and `structural_gaps` on the
  structural property surfaces, see below) and no guarantee predicate.
  How each reports a give-up is in the
  [query-surface ledger](query-surface-ledger.md).

## Per-query completeness claims (#540)

Three per-query surfaces claim completeness without going through
`ClassificationStats::completeness_guaranteed`. Each reads its own conjunction,
and since #540 each includes condition 7 (`dropped` is empty). Before #540 none
did: on an ontology whose only non-EL content was an unsupported data range,
conversion dropped it, the rest took the pure-EL fast path, and the surface
claimed completeness while missing an entailment of the dropped axiom.

| Surface | Rust | CLI | Python | Conditions | Pinned by |
|---|---|---|---|---|---|
| `sat-expr`, `subclass-expr` | `CeVerdict::completeness_guaranteed`, which reads `QueryStats::completeness_guaranteed` (#552) | `--json` `completeness_guaranteed` | `class_expression_satisfiable_result` / `class_expression_entailed_subclass_result` (#552): `completeness_guaranteed` and `dropped`; the bare functions emit `DroppedAxiomsWarning` | pure-EL fast path (`QueryStats::pure_el_mode`), no give-up, nothing dropped by the probed ontology's conversion (`QueryStats::dropped_axioms`; the input's drops and the query expression's own; `CeVerdict::dropped` returns that list, and the CLI's stderr warning names it in text and `--json` mode), and (#552) consistency neither undetermined nor resting on a trusted wedge `Sat` | `class_expr_query.rs` `each_condition_alone_voids_the_expression_guarantee` (the table test over `QueryStats`; deleting a conjunct fails its row); `query_stats_dropped.rs` (the count on every single-query path, and the shared predicate); `completeness_contract.rs` `a_dropped_axiom_voids_the_expression_query_guarantee` with its `xsd:string` control; `expr_dropped_axiom_cli.rs`: input-side drops and query-side drops (`*_with_a_query_side_drop_warns_and_is_not_guaranteed`, which pin both the flag and the warning) |
| `instances-expr` | `CeInstances::completeness_guaranteed` | `--json` `completeness_guaranteed` | `class_expression_instances_result` (#552), as above | realize saturation fast path (`realize_saturation_eligible`), no probe cut, nothing dropped by the probed conversion (`CeInstances::dropped`, warned as above), and (#552) consistency not undetermined and the eager inconsistency check on (`RUSTDL_QUERY_INCONSISTENCY_GUARD`, default on) | `class_expr_query.rs` `each_condition_alone_voids_the_instances_guarantee`; `expr_consistency_guarantee_cli.rs` (the guard-off row on a real input); as above |
| `explain` | — (text only) | the suffix "(input is pure EL; closure is complete)" | no binding | `QueryStats::completeness_guaranteed` (#552; before, `pure_el_mode` and nothing dropped). `explain owl:Thing owl:Nothing` is the consistency query, whose `pure_el_mode` follows the wider `saturation_fast_path_admits` gate: it reads "(decided by saturation on a fragment where it is complete)" instead, pinned by `expr_consistency_guarantee_cli.rs` `explain_consistency_does_not_claim_pure_el`. With a drop, a pure-EL "no" reads "pure EL after dropping axioms it could not convert; closure is NOT complete for the input"; `explain` also prints the stderr drop warning since #540 | `expr_dropped_axiom_cli.rs` `explain_with_a_dropped_axiom_does_not_claim_a_complete_closure` |

**Consistency conditions (#552, decided).** The expression queries now read
`classify`'s conditions 6 and 8 too, with these reasons (from a code trace, Fable
plan review of #552):

- `relies_on_trusted_consistency` (sat/subclass): the verdict was reached with the
  `ABox` seed dropped on a consistency-wedge `Sat`. If that `Sat` is wrong the KB is
  inconsistent and a "satisfiable" / "not entailed" is a miss, which is exactly
  what the guarantee covers. **It cannot meet the pure-EL fast path today**: every
  `QueryStats` that can carry `pure_el_mode` sets it `false`, and the trusted flag
  comes only from the wedge path after the pure-EL return. So this is a defensive
  invariant, like condition 8 for `classify`.
- `consistency_undetermined` on a sat/subclass verdict is set only on a POSITIVE
  answer (the #369 lazy check), which is entailed even on an inconsistent KB, so it
  cannot void that answer's completeness. It is included because it is cheap and
  conservative, not for `classify`'s reason. It cannot meet the pure-EL path at
  default flags either: the lazy check is clean without individuals, and pure EL
  admits none.
- `consistency_undetermined` on an instance list is a real condition: on an
  inconsistent KB every individual is an instance, so a short list misses members.
  The realize fast path returns default flags, so it cannot meet it today.
- **Negative answers on the complete paths rest on a decided consistency.**
  Sat/subclass: a pure-EL KB has no individual, so it is inconsistent iff `⊤` is
  unsatisfiable, which the query guard's saturation check finds before the query
  runs. Instances: `instance_query_inconsistent_fast` runs eagerly, before any
  answer. Both rest on the fragment gates being complete for `⊤ ⊑ ⊥`, the gates'
  existing contract.
- **The one gap found, closed in code:** with `RUSTDL_QUERY_INCONSISTENCY_GUARD=0`
  the instance fast path ran no consistency check at all and still claimed the
  guarantee. The instances conjunction now requires the guard to be on.

No CLI or Rust output changes at default flags. The Python bare functions do change:
they now emit `DroppedAxiomsWarning` when conversion dropped axioms, so under
`-W error` they raise where they used to return; the other Python queries stay
silent on dropped axioms until #597. With the guard off, sat-expr / subclass-expr
stay safe only because the saturator propagates `⊤ ⊑ ⊥` into every context, so an
inconsistent pure-EL TBox makes every class unsatisfiable in the closure and their
answers positive. `QueryStats::pure_el_mode` itself stays
a path flag: it says the closure is complete for the CONVERTED ontology. Use
`QueryStats::completeness_guaranteed()`, which reads `dropped_axioms` and the
consistency flags too; it is valid for named-class and expression
satisfiability/subsumption verdicts, not for the consistency query, whose
`pure_el_mode` follows the wider `saturation_fast_path_admits` gate.

## Structural property surfaces (#389, #390)

These surfaces are structural closures over told axioms. They never probe the
reasoner, so they have no deadline, no `incomplete` of their own and no guarantee
predicate. Their signal is `structural_gaps`: the constructs present in the
ontology that the closure ignores and that can entail a relation it would report.
An empty list means none of the enumerated routes occurs, not that the answer is
complete. Not enumerated, and so never reported: a property that is empty because
its domain or range is unsatisfiable outside the EL fragment, and routes not yet
found. `structural_gaps` is outside `incomplete` on purpose: nothing was cut, so
more time changes nothing.

| Surface | Rust | CLI | Python | Pinned by |
|---|---|---|---|---|
| object / data property hierarchy | `property_structural_gaps(&o).object_hierarchy()` / `.data_hierarchy()` | `property-hierarchy --json` `structural_gaps` (both sides) | `StructuralGapWarning` from `object_property_hierarchy`, `data_property_hierarchy`, `materialize_inferred_sub{object,data}property_axioms` | `property_structural_gaps.rs`; `json_output.rs` `structural_property_surfaces_report_their_gaps`; `test_queries.py` `test_structural_gap_warning` |
| disjoint object / data properties | `.object_disjoint()` / `.data_disjoint()` | `disjoint --json` `structural_gaps` (property half; the class half keeps `incomplete`) | `StructuralGapWarning` from `disjoint_object_properties`, `disjoint_data_properties` | same |
| inferred data property values | `.data_values()` | `property-values --json` `structural_gaps` (data half; the object half keeps `incomplete`) | `StructuralGapWarning` from `data_property_values`, `materialize_inferred_data_property_assertions` | same |

`StructuralGapWarning` is a sibling of `IncompleteQueryWarning`, not a subclass, so a
caller that turns budget warnings into errors is not tripped by a structural gap.
In text mode each command prints one stderr `warning:` line naming the kinds.

## Justification minimality (#584)

`Justification::minimal_guaranteed` (`justify --json` `minimal`, `prove --json`
`justification_minimal`, `repair`'s `complete`) claims that no proper subset of the
returned set entails the query. `QuickXplain` keeps an axiom exactly when the subset
without it is answered "not entailed", so the claim holds iff every such "not
entailed" is right. It is `true` when all of:

1. the fragment is `PureEl` or `Horn`;
2. no subset probe gave up (#396): a give-up is `Unknown`, never a silent "no";
3. (#584) the search did not run on the saturation-only `Inconsistent` oracle.

Why condition 1 admits `Horn` (the argument, not a fragment-completeness claim; #124
refuted that for classify): for `SubClassOf` and `Unsatisfiable` on named classes the
oracle is the single-query surface (`entails_reporting` →
`is_subclass_of_with_stats` / `is_class_satisfiable_with_stats`). There a "yes" comes
from the EL closure or the query wedge's `Subsumed`, and a "no" from the EL closure
when the probed subset is itself pure EL (a `QuickXplain` subset of a `Horn` input
can be; the closure is complete there) or from the deadline-free main tableau
(`run_satisfiability_keeping` → `decide_query_raw`), whose only give-up is reported
as `Unknown`. On a consistent parent, no trusted wedge `Sat` decides a "no" there. The #401 `ABox` drop on a
consistency-wedge `Sat` is flagged (`relies_on_trusted_consistency`) and cannot cost
minimality for these queries: with the lent verdict every subset probe answers for
the subset's `TBox`, so `QuickXplain` keeps no `ABox` axiom. So minimality on named
queries rests on main-tableau completeness, which holds outside `Horn` too; the
`PureEl | Horn` restriction is conservative, not what carries the claim.

Not covered by that argument:
- `Inconsistent` queries on the full oracle trust the consistency wedge's `Sat`
  (`is_consistent_with_stats`): `minimal` can read `true` beside
  `relies_on_trusted_consistency: true`. No wrong consistency-wedge `Sat` on `Horn` is
  known; that is exposure, not a demonstrated error.
- Named queries on an inconsistent parent: when the #313 guard misses the clash and
  the seeded probe does not finish in its budget, the single-query surface falls back
  to `query_consistency()` (`lib.rs`, the #401 seeded-probe fallback), which can rest
  on the consistency wedge's trusted `Sat` and answer the subset ABox-free.
- The query kinds the oracle answers through `inconsistent_with` (sub/equivalent
  property, same/different individual, data-property queries): they ask a
  consistency check of the subset plus a negated assertion, so they carry the
  `Inconsistent` exposure above.
- `Inconsistent` queries on the saturation-only oracle: condition 3. The `ABox`
  saturator is incomplete even on EL (no anonymous witnesses, no `∀` rule); on
  `h2.ofn` the search returned 7 axioms with `minimal: true` while 5 of them are
  inconsistent. Pinned by `crates/owl-dl-cli/tests/justify_sat_oracle_minimal.rs`
  (`justify` in both query forms, and `prove`'s `justification_minimal`).
- `InstanceOf` queries go through `realize::instance_reporting`; not traced.
- `--all` enumeration on the saturation-only oracle: a branch probe prunes on "not
  refuted", so a justification can be missing; `JustifyOutcome::saturation_oracle`
  makes `justify --json` report `enumeration_complete: false` (#584 review).
- Surfaces that carry no minimality signal at all: Python `justify`, `justify_all`
  and `PreparedOntology.justify*` (their docstrings say "One minimal justification"),
  and the HTML report's hard-coded "(minimal justification)" (`report.rs`). Follow-up
  issue.

## Rule for changes

A change that adds a completeness signal, or a new way for `classify` to miss an
entailment, must in the same PR:

1. add a row to one of the tables above, saying whether the signal is a condition
   of the guarantee or why it is outside it;
2. if it is a condition, add it to `ClassificationStats::completeness_guaranteed`
   and add a row to the table test;
3. report it on the CLI and Python surfaces, or record here why not.
