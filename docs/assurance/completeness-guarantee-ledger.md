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

All nine must hold.

| # | Condition | Why it voids the guarantee when it fails | Source | Rust stats field | CLI `classify --json` | Python | Pinned by |
|---|---|---|---|---|---|---|---|
| 1 | `fragment == PureEl` | Only on PureEl is the engine that answers (the EL saturator) complete. `Horn` is a clause-shape label, not an engine property, and a Horn ontology exists on which `classify` misses an oracle-confirmed subsumption. `OutOfFragment` relies on the wedge's trusted `Sat`, which can miss silently. | #124 | `fragment` | not serialized (stderr banner `# fragment:` in non-JSON mode) | not exposed | `completeness_contract.rs`: `pure_el_guarantees_completeness`, `out_of_fragment_does_not_guarantee_completeness`, `horn_clause_shape_does_not_guarantee_completeness`, table test |
| 2 | `timed_out_pairs == 0` | A cut pair is reported "not subsumed" without a verdict. A preparation or conversion timeout also bumps this count and forces `OutOfFragment`. | anytime contract; #162 | `timed_out_pairs`, `timed_out_pair_ids` | `incomplete` (also `prep_timed_out`, `conversion_timed_out`) | `timed_out_pairs`, `complete`, `prep_timed_out`, `conversion_timed_out` | table test; `json_output.rs` `classify_reports_a_cut_preparation_or_conversion_in_its_own_field`; `prep_deadline.rs`; `hard_global_deadline.rs` |
| 3 | `!wedge_hierarchy_blind` | With role-hierarchy matching (Layer A) off, the wedge cannot see sub-role, inverse or symmetric reasoning, and its trusted `Sat` can miss. | #214 | `wedge_hierarchy_blind` | `wedge_hierarchy_blind` | `wedge_hierarchy_blind` | table test; `json_output.rs` `classify_reports_a_hierarchy_blind_wedge_in_its_own_field` |
| 4 | `wedge_clauses_refused == 0` | A clause the wedge's matcher refuses never fires. | #353 | `wedge_clauses_refused` | `wedge_clauses_refused` | `wedge_clauses_refused` | table test |
| 5 | `derived_same_undecided == 0` | An individual equality the KB may force was not decided, so the classify wedge may lack it. The count depends on a budget: probing stops after `RUSTDL_CLASSIFY_DERIVED_SAME_STALLS` (default 8) consecutive stalls, or at `RUSTDL_CLASSIFY_DERIVED_SAME_MAX` (default 64) probes. Candidates already proved distinct (`DifferentIndividuals`, told-disjoint told types) are dropped unprobed and do not count, and candidates are grouped by signed role: a shared object is a candidate only under a bound on the inverse role, and a bound reached through a declared inverse now counts (#368). Before #368 such a pair was never a candidate, so `0` could hide it. On current `main` this condition cannot change the result. `derive_nominal_same_with` looks for candidates only when the ontology has a counted role (`≥`, `≤`, functional or inverse-functional), a nominal, and an asserted `ObjectPropertyAssertion` on a counted role. The PureEl gate admits neither nominals nor property assertions. If a TBox-only path to candidates is ever added, re-check this. | #354 | `derived_same_undecided` | `derived_same_undecided` | `derived_same_undecided` | table test |
| 6 | `!consistency_undetermined` | The inconsistency pre-check reached no verdict, or (#446) classify's consistency probe was admitted and gave up (#466: a cut per-class unsat probe admits it on a `TBox`-only run too, where an empty unsat set had read as consistent). If the KB is in fact inconsistent, every class is unsatisfiable and the hierarchy misses entailments. | #162, #228 review | `consistency_undetermined` | `consistency_undetermined` (reported as `undetermined && !inconsistent`) | `consistency_undetermined` (same masking) | table test |
| 7 | `dropped` is empty | The engine reasoned over a strictly weaker KB than the input. | #43, #246 | `dropped` | `dropped` | module-level `rustdl.dropped_axioms(path)` only, not on `Classification` | `completeness_contract.rs` `a_dropped_axiom_voids_the_guarantee` and its control; table test; `json_output.rs` `classify_json_completeness_guaranteed_ends`; `test_classify.py` `test_dropped_axiom_voids_the_guarantee` |
| 8 | `!relies_on_trusted_consistency` | "Consistent" rests on a consistency-wedge `Sat` that no main-tableau run verified. If that `Sat` is wrong, the KB is inconsistent and every class is unsatisfiable, so the hierarchy misses entailments. **This cannot fail beside condition 1 today.** PureEl admits no `ABox` assertion and no nominal, so no individual exists, and the consistency wedge (`classify_wedge_consistency`) needs one to run. The probe that could also trust it runs only after the PureEl dispatch. The condition is kept so the invariant holds if PureEl ever admits an individual. | #429 | `relies_on_trusted_consistency` | `relies_on_trusted_consistency` | `relies_on_trusted_consistency`, plus `TrustedConsistencyWarning` | table test; `trusted_consistency_surfaces.rs` `pure_el_and_trust_never_meet` |
| 9 | `!(universal_role_ordinary && !inconsistent)` | rustdl reads `owl:topObjectProperty` (and any property above it, and `owl:topDataProperty` outside the super-property position) as an ordinary property. That is sound for positives, but it misses entailments of `Δ × Δ`. For example, `⊤ ⊑ ∃r.D` with `∃top.D ⊑ ⊥` is inconsistent, yet on main it read as consistent with this guarantee set. Inert occurrences are not flagged: `x ⊑ top`, a positive `top(a, b)`, Transitive/Reflexive/Symmetric, equivalence and inverse within the universal roles, a chain into them, and #539's built-ins. A proved inconsistency masks the condition, since every entailment holds then. Detection is `owl_dl_core::top_bottom_roles::universal_role_ordinary`. | #543 | `universal_role_ordinary` | `universal_role_ordinary` (masked by `inconsistent`), plus a stderr note | `universal_role_ordinary`, plus `UniversalRoleWarning` | table test; `completeness_contract.rs` `a_proved_inconsistency_masks_the_universal_role_condition`; `top_bottom_roles.rs` (core) detector tests; `universal_role_cli.rs` |

The table test is `each_condition_alone_voids_the_guarantee` in
`crates/owl-dl-reasoner/tests/completeness_contract.rs`. It starts from the one
state that guarantees completeness and flips each condition on its own. Deleting a
conjunct from the predicate makes exactly that row fail, so the test is also the
guard's sabotage check.

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
  a separate `dropped_axioms(path)` call. The guarantee property is the way to read
  conditions 1 and 7 from a `Classification`.
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
| `sat-expr`, `subclass-expr` | `CeVerdict::completeness_guaranteed` | `--json` `completeness_guaranteed` | not exposed (the bindings return `incomplete` only) | pure-EL fast path (`QueryStats::pure_el_mode`), no give-up, nothing dropped by the probed ontology's conversion (the input's drops and the query expression's own; `CeVerdict::dropped` returns that list, and the CLI's stderr warning names it in text and `--json` mode) | `class_expr_query.rs` `each_condition_alone_voids_the_expression_guarantee` (the table test; deleting a conjunct fails its row); `completeness_contract.rs` `a_dropped_axiom_voids_the_expression_query_guarantee` with its `xsd:string` control; `expr_dropped_axiom_cli.rs`: input-side drops and query-side drops (`*_with_a_query_side_drop_warns_and_is_not_guaranteed`, which pin both the flag and the warning) |
| `instances-expr` | `CeInstances::completeness_guaranteed` | `--json` `completeness_guaranteed` | not exposed | realize saturation fast path (`realize_saturation_eligible`), no probe cut, nothing dropped by the probed conversion (`CeInstances::dropped`, warned as above) | same |
| `explain` | — (text only) | the suffix "(input is pure EL; closure is complete)" | no binding | `QueryStats::pure_el_mode` and nothing dropped (the CLI reads `dropped_block`). With a drop, a pure-EL "no" reads "pure EL after dropping axioms it could not convert; closure is NOT complete for the input"; `explain` also prints the stderr drop warning since #540 | `expr_dropped_axiom_cli.rs` `explain_with_a_dropped_axiom_does_not_claim_a_complete_closure` |

Not added to these claims, though `classify` has them: `consistency_undetermined`
and `relies_on_trusted_consistency`. Both are reported in their own fields on the
expression queries (`instances-expr --json` has no `relies_on_trusted_consistency`),
so a caller can see them; whether they belong in the conjunction is an open
follow-up, not settled by #540. `QueryStats::pure_el_mode` itself
stays a path flag: it says the closure is complete for the CONVERTED ontology, and
a caller presenting it as a guarantee must check `dropped_axioms` too.

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

## Rule for changes

A change that adds a completeness signal, or a new way for `classify` to miss an
entailment, must in the same PR:

1. add a row to one of the tables above, saying whether the signal is a condition
   of the guarantee or why it is outside it;
2. if it is a condition, add it to `ClassificationStats::completeness_guaranteed`
   and add a row to the table test;
3. report it on the CLI and Python surfaces, or record here why not.
