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
report only whether a per-pair deadline fired, which is a strictly narrower thing.
A result can have `complete == True` and `completeness_guaranteed == False`.

## Conditions in the guarantee

All seven must hold.

| # | Condition | Why it voids the guarantee when it fails | Source | Rust stats field | CLI `classify --json` | Python | Pinned by |
|---|---|---|---|---|---|---|---|
| 1 | `fragment == PureEl` | Only on PureEl is the engine that answers (the EL saturator) complete. `Horn` is a clause-shape label, not an engine property, and a Horn ontology exists on which `classify` misses an oracle-confirmed subsumption. `OutOfFragment` relies on the wedge's trusted `Sat`, which can miss silently. | #124 | `fragment` | not serialized (stderr banner `# fragment:` in non-JSON mode) | not exposed | `completeness_contract.rs`: `pure_el_guarantees_completeness`, `out_of_fragment_does_not_guarantee_completeness`, `horn_clause_shape_does_not_guarantee_completeness`, table test |
| 2 | `timed_out_pairs == 0` | A cut pair is reported "not subsumed" without a verdict. A preparation or conversion timeout also bumps this count and forces `OutOfFragment`. | anytime contract; #162 | `timed_out_pairs`, `timed_out_pair_ids` | `incomplete` (also `prep_timed_out`, `conversion_timed_out`) | `timed_out_pairs`, `complete`, `prep_timed_out`, `conversion_timed_out` | table test; `json_output.rs` hard-deadline and conversion-timeout cases |
| 3 | `!wedge_hierarchy_blind` | With role-hierarchy matching (Layer A) off, the wedge cannot see sub-role, inverse or symmetric reasoning, and its trusted `Sat` can miss. | #214 | `wedge_hierarchy_blind` | `wedge_hierarchy_blind` | `wedge_hierarchy_blind` | table test; `json_output.rs` `classify_reports_a_hierarchy_blind_wedge_in_its_own_field` |
| 4 | `wedge_clauses_refused == 0` | A clause the wedge's matcher refuses never fires. | #353 | `wedge_clauses_refused` | `wedge_clauses_refused` | `wedge_clauses_refused` | table test |
| 5 | `derived_same_undecided == 0` | An individual equality the KB may force was not decided, so the classify wedge may lack it. The count depends on a budget: probing stops after `RUSTDL_CLASSIFY_DERIVED_SAME_STALLS` (default 8) consecutive stalls. On current `main` this condition cannot change the result, because candidates need nominals and PureEl excludes them. | #354 | `derived_same_undecided` | `derived_same_undecided` | `derived_same_undecided` | table test |
| 6 | `!consistency_undetermined` | The inconsistency pre-check reached no verdict. If the KB is in fact inconsistent, every class is unsatisfiable and the hierarchy misses entailments. | #162, #204 | `consistency_undetermined` | `consistency_undetermined` (reported as `undetermined && !inconsistent`) | `consistency_undetermined` (same masking) | table test |
| 7 | `dropped` is empty | The engine reasoned over a strictly weaker KB than the input. | #43, #246 | `dropped` | `dropped` | module-level `rustdl.dropped_axioms(path)` only, not on `Classification` | `completeness_contract.rs` `a_dropped_axiom_voids_the_guarantee` and its control; table test; `json_output.rs` `classify_json_completeness_guaranteed_ends`; `test_classify.py` `test_dropped_axiom_voids_the_guarantee` |

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
| `trusted_sat_refutations` | Excluded by composition, not by the predicate: a PureEl classification is answered by the saturator and never consults the wedge's trusted `Sat`. If PureEl ever routes through the wedge, this needs a condition. |
| `incomplete` (CLI), `complete` (Python) | These report only that a deadline fired. They are a view of condition 2, kept separate on purpose: their contract is "a deadline fired", and callers depend on it. |
| Inconsistent KB | `classify_inconsistent` builds normal stats with the ontology's fragment. A pure-EL inconsistent KB with nothing cut or dropped therefore reports `completeness_guaranteed: true`. That is correct: every class is unsatisfiable and every subsumption entailed, which is exactly what is reported. Pinned by `json_output.rs` `classify_json_completeness_guaranteed_ends` (`⊤ ⊑ ⊥`). An inconsistent KB with an ABox is not PureEl, so it reports `false`. |

## Known gaps in the surfaces

- **Python does not expose the fragment** and reports dropped axioms only through
  a separate `dropped_axioms(path)` call. The guarantee property is the way to read
  conditions 1 and 7 from a `Classification`.
- **The JSON schema does not serialize the fragment.** A caller sees why the
  guarantee is false for conditions 2–7, but not for condition 1.
- **Any ABox takes an ontology out of PureEl.** A pure-EL TBox with one
  `ClassAssertion` classifies as `Horn` and reports `false`, even though it runs
  saturation-only. That is conservative (a missing guarantee, never a false one),
  and widening it needs its own completeness argument.
- **The guarantee covers `classify` only.** `realize`, `consistent`, `sat` and the
  inferred-query surface have their own partial signals (`incomplete`,
  `witness_prune_active`, `consistency_undetermined`) and no guarantee predicate.
  Auditing those is a later slice of package A.

## Rule for changes

A change that adds a completeness signal, or a new way for `classify` to miss an
entailment, must in the same PR:

1. add a row to one of the tables above, saying whether the signal is a condition
   of the guarantee or why it is outside it;
2. if it is a condition, add it to `ClassificationStats::completeness_guaranteed`
   and add a row to the table test;
3. report it on the CLI and Python surfaces, or record here why not.
