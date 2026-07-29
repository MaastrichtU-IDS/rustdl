## Task 2 Report: ConjunctiveUnsat rule

## Status: DONE

## Commit SHA

`a4019eb`

## Files Changed

**`crates/owl-dl-saturation/src/lib.rs`** — 56 insertions, 0 deletions (7 change sites).

### Insertion points and final code

**Change 1 — Add `ConjunctiveUnsat` struct** (immediately after `ConjunctiveTrigger` definition):

```rust
#[derive(Debug, Clone)]
struct ConjunctiveUnsat {
    bodies: Vec<ClassId>,
}
```

**Change 2 — Add `conjunctive_unsat` field to `ElRules`** (after `conjunctive_triggers` field):

```rust
    conjunctive_unsat: Vec<ConjunctiveUnsat>,
```

**Change 3 — Add `conjunctive_unsat_by_body` field to engine struct** (after `conjunctive_by_body`):

```rust
    conjunctive_unsat_by_body: Vec<Vec<usize>>,
```

**Change 4 — Build the index in `WorklistEngine::new`** (after `conjunctive_by_body` build loop) + `conjunctive_unsat_by_body,` in struct literal:

```rust
        let mut conjunctive_unsat_by_body: Vec<Vec<usize>> = vec![Vec::new(); num_total_classes];
        for (idx, rule) in rules.conjunctive_unsat.iter().enumerate() {
            for &body in &rule.bodies {
                conjunctive_unsat_by_body[body.index() as usize].push(idx);
            }
        }
```

**Change 5 — Grow the index in `introduce_runtime_synthetic`** (after `conjunctive_by_body` growth):

```rust
            while self.conjunctive_unsat_by_body.len() < needed {
                self.conjunctive_unsat_by_body.push(Vec::new());
            }
```

**Change 6 — Consume the rule in `process_subsumer`** (before the disjointness block):

```rust
        for ridx in self.conjunctive_unsat_by_body[d.index() as usize].clone() {
            let bodies = self.rules.conjunctive_unsat[ridx].bodies.clone();
            if bodies.iter().all(|b| self.subsumers.contains(c, *b)) {
                self.enqueue_unsat(c);
            }
        }
```

**Change 7 — Emit rule during rule collection** (And-LHS arm, after `!salvageable` guard):

```rust
            if matches!(pool.get(sup), ConceptExpr::Bot) {
                if !bodies.is_empty() {
                    rules.conjunctive_unsat.push(ConjunctiveUnsat { bodies });
                }
                return;
            }
```

## Commands Run

```sh
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test conjunctive_unsat
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-saturation -p owl-dl-reasoner --all-targets --all-features -- -D warnings
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "fix(saturation): derive unsat from And(b₁…bₙ) ⊑ ⊥ (was silently dropped)"
```

## Task-1 Canaries (VERBATIM)

```
running 3 tests
test conjunctive_bot_derives_unsat ... ok
test conjunctive_bot_does_not_over_fire ... ok
test conjunctive_bot_matches_disjoint_classes_spelling ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## `cargo test -p owl-dl-saturation` (VERBATIM)

```
running 81 tests
[all 78 non-ignored tests pass; 3 ignored as expected]

test result: ok. 78 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.31s

Doc-tests owl_dl_saturation
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## `cargo test -p owl-dl-reasoner` (key results)

All test groups passed with 0 failures across all integration test files.

Key gate test:
```
test classify::tests::saturator_fragment_rejects_conjunctive_bot_with_functional ... ok
```

The `disjoint_ok` guard correctly forces hybrid fallback when a functional role is present — the `ConjunctiveUnsat` rule does not bypass the fragment gate.

Unit test subtotals (from result lines):
- unittests src/lib.rs: 173 passed; 0 failed; 3 ignored
- All other integration test files: 0 failed

## fmt result

Clean — no output (= no violations).

## clippy result

```
    Checking owl-dl-saturation v0.4.5 (...)
    Checking owl-dl-tableau v0.4.5 (...)
    Checking owl-dl-reasoner v0.4.5 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.78s
```

No warnings or errors.

## Pre-existing test status changes

None. All previously-passing tests continue to pass; all previously-ignored tests remain ignored. The 3 Task-1 canary tests went from FAIL to PASS as intended.

## What was NOT done (by design)

Proof/provenance recording for `ConjunctiveUnsat` was omitted — this is deliberate sequencing; Task 3 adds that at the same consumption site in `process_subsumer`.

---

---

## Review Follow-ups (2026-07-29)

Three items from the post-merge reviewer were applied to
`crates/owl-dl-saturation/src/lib.rs`.

### Change Sites

**Finding 1 — `introduce_runtime_synthetic`: capture `before_conjunctive_unsat` + debug tripwire**

Added the capture binding alongside the existing sibling bindings at the top of
the function, and placed the `debug_assert_eq!` after the `conjunctive_unsat_by_body`
growth loop (on the non-dedup path only — the early `return` on the dedup path means
the binding is never unused there because it is consumed by the assert on the
non-dedup path; Rust considers a variable "used" if it appears in any reachable code
path after its declaration).

Top of function (addition in context):
```rust
    fn introduce_runtime_synthetic(&mut self, body: Vec<ClassId>) -> ClassId {
        let before_atomic = self.rules.atomic_subsumptions.len();
        let before_conjunctive = self.rules.conjunctive_triggers.len();
        let before_conjunctive_unsat = self.rules.conjunctive_unsat.len();
```

After the `conjunctive_by_body` indexing loop (new block):
```rust
        // Tripwire: `introduce` must never append a ConjunctiveUnsat rule at runtime.
        // If it ever does, the rule would be silently un-indexed and silently dropped —
        // the exact bug class this work exists to prevent.  The debug_assert fires in
        // `cargo test` (debug profile) but compiles out in release.
        debug_assert_eq!(
            before_conjunctive_unsat,
            self.rules.conjunctive_unsat.len(),
            "runtime rule addition must re-index conjunctive_unsat_by_body",
        );
```

**Finding 2 — `process_subsumer`: remove avoidable `.bodies.clone()` in the hottest EL loop**

Replaced the `let bodies = self.rules.conjunctive_unsat[ridx].bodies.clone();`
binding (which cloned the inner `Vec<ClassId>` on every iteration) with an
in-place borrow. NLL ends the `self.rules` borrow at the close of the condition,
exactly as the neighbouring `ConjunctiveTrigger` loop already relies on.

```rust
        for ridx in self.conjunctive_unsat_by_body[d.index() as usize].clone() {
            if self.rules.conjunctive_unsat[ridx]
                .bodies
                .iter()
                .all(|b| self.subsumers.contains(c, *b))
            {
                self.enqueue_unsat(c);
                break; // enqueue_unsat is idempotent; remaining rules for c are pointless
            }
        }
```

**Finding 3 — `process_subsumer`: `break` after `enqueue_unsat`**

Added `break;` after `self.enqueue_unsat(c);` in the `ConjunctiveUnsat` loop
(shown in the snippet above). Behaviour-preserving: `enqueue_unsat` is idempotent
(guards on the `unsatisfiable` bitset), so the only effect is fewer redundant pushes.

### Commands Run

```sh
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test conjunctive_unsat
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-saturation -p owl-dl-reasoner --all-targets --all-features -- -D warnings
```

### Command 1: `cargo test -p owl-dl-reasoner --test conjunctive_unsat` (VERBATIM)

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Running tests/conjunctive_unsat.rs (target/debug/deps/conjunctive_unsat-f73ae6eaf25598c7)

running 3 tests
test conjunctive_bot_does_not_over_fire ... ok
test conjunctive_bot_derives_unsat ... ok
test conjunctive_bot_matches_disjoint_classes_spelling ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### Command 2: `cargo test -p owl-dl-saturation` (VERBATIM)

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/lib.rs (target/debug/deps/owl_dl_saturation-6df6983380f1ab6a)

running 81 tests
test tests::b1_nominal_disjunction_not_touched ... ok
test seed_sat::tests::out_of_universe_seed_is_silently_skipped ... ok
test tests::and_left_conjunctive_trigger_fires ... ok
test seed_sat::tests::told_disjoint_seed_is_unsat ... ok
test seed_sat::tests::forall_key_seed_is_unsat ... ok
test tests::b1_undetermined_forces_nothing ... ok
test tests::equivalent_classes_both_directions ... ok
test tests::b2c_subclassof_or_no_disjunct_to_x ... ok
test tests::b1_forced_disjunct_via_derived_subsumer ... ok
test tests::and_right_distributes ... ok
test tests::b1_inherited_disjunction ... ok
test tests::b1_forced_to_bot ... ok
test tests::b1_forced_disjunct_via_told_subsumer ... ok
test tests::b2_forced_disjunct_via_deep_incompatibility ... ok
test tests::collect_el_rules_records_functional_roles_and_their_supers ... ok
test tests::equivalent_classes_with_compound_existential_decomposes ... ok
test tests::b2c_union_class_fruit ... ok
test tests::id_matrix_dense_and_sparse_are_semantically_identical ... ok
test tests::b2b_forall_no_spurious ... ok
test tests::b2b_forall_course_hierarchy ... ok
test tests::disjoint_classes_makes_intersection_unsat ... ok
test tests::b2_forced_to_bot_via_deep ... ok
test tests::existential_propagation_pizza_food ... ok
test tests::existential_gci_compound_consequent_negative_control ... ok
test tests::b2c_union_course_combine ... ok
test tests::min_cardinality_on_rhs_lowers_to_existential ... ok
test tests::functional_role_merge_canary_recovers_entailment ... ok
test tests::lhs_and_with_existential_rhs_canary_recovers_entailment ... ok
test tests::compound_existential_body_canary_recovers_entailment ... ok
test tests::existential_gci_compound_consequent_propagates ... ok
test tests::proof_faithfulness_corpus_galen ... ignored, requires real ontology files; run with --include-ignored
test tests::proof_faithfulness_corpus_go_basic ... ignored, requires real ontology files; run with --include-ignored
test tests::proof_faithfulness_corpus_pizza ... ignored, requires real ontology files; run with --include-ignored
test tests::existential_with_union_body_on_trigger_lhs_fires_per_operand ... ok
test tests::compound_existential_body_cluster_a_paired_anatomy_canary ... ok
test tests::lhs_conjunction_with_unsupported_operand_is_dropped ... ok
test tests::oneof_subsumer_all_members_typed ... ok
test tests::functional_role_merge_chained_functional_supers ... ok
test tests::lhs_conjunction_with_existential_operand_fires ... ok
test tests::nominal_transitive_abox_fold_classifies ... ok
test tests::oneof_subsumer_non_nominal_disjunct_no_seed ... ok
test tests::existential_with_unsat_body_propagates_to_source ... ok
test tests::functional_role_merge_body_on_sub_role ... ok
test tests::lhs_conjunction_existential_marker_is_shared_across_conjunctions ... ok
test tests::nominal_filler_typing_lifts_existential ... ok
test tests::nested_existential_in_outer_body_lowers_via_marker ... ok
test tests::lhs_conjunction_with_union_existential_body_fires ... ok
test tests::functional_role_merge_3_sub_property_fan_in ... ok
test tests::forall_oneof_nominal_sugar_classifies ... ok
test tests::compound_existential_body_deeper_nesting_canary ... ok
test tests::oneof_subsumer_disagreeing_members_no_seed ... ok
test tests::oneof_subsumer_cascades_via_told ... ok
test tests::proof_recording_reflexivity_seeded ... ok
test tests::max_cardinality_nominal_varietal_classifies ... ok
test tests::min_cardinality_with_super_role_chains_through_union ... ok
test tests::oneof_subsumer_typeless_member_no_seed ... ok
test tests::proof_recording_role_chain ... ok
test tests::out_of_fragment_axioms_dont_panic ... ok
test tests::subclass_of_complement_disjointness_is_directional_and_sound ... ok
test tests::proof_recording_trace_non_empty_on_nontrivial_ontology ... ok
test tests::subclass_of_complement_conjunct_makes_class_unsat ... ok
test tests::phase2d_fact_inherits_to_subclass ... ok
test tests::proof_recording_verdicts_identical_to_baseline ... ok
test tests::functional_role_merge_4_sub_property_fan_in ... ok
test tests::proof_recording_transitivity ... ok
test tests::proof_recording_el_chain_pizza_food ... ok
test tests::property_range_does_not_force_target_type_subsumption ... ok
test tests::role_hierarchy_propagates_through_existential ... ok
test tests::forall_oneof_functional_existential_classifies ... ok
test tests::transitive_subsumption_closes ... ok
test tests::property_range_via_super_role_constrains_witness ... ok
test tests::property_domain_propagates_to_subjects ... ok
test tests::tseitin_introduces_synthetic_for_compound_existential_body ... ok
test tests::transitive_role_chains_three_hops ... ok
test tests::role_chain_propagates_through_two_existentials ... ok
test tests::tseitin_trigger_side_compound_body_classifies ... ok
test tests::transitive_role_chains_two_existentials ... ok
test tests::transitive_abox_without_tbox_nominals_allocates_no_nomkeys ... ok
test tests::phase2c_sub_role_propagation_counter_bumps_on_4_fan_in ... ok
test tests::property_range_constrains_synthetic_witness_via_tseitin ... ok
test tests::id_matrix_with_capacity_picks_rep_by_size ... ok

test result: ok. 78 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.29s

   Doc-tests owl_dl_saturation

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### Command 3: `cargo test -p owl-dl-reasoner` (VERBATIM — result lines only; full output saved to scratchpad)

```
test result: ok. 173 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 72 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 66 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.32s
test result: ok. 4 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.54s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 36 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 0 passed; 0 failed; 22 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.76s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 1 passed; 0 failed; 11 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 14 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### Command 4: `cargo fmt --all -- --check` (VERBATIM)

```
(no output — exit 0)
```

### Command 5: `cargo clippy -p owl-dl-saturation -p owl-dl-reasoner --all-targets --all-features -- -D warnings` (VERBATIM)

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.16s
```

---

### What was implemented (prior task — OVERWRITTEN)

Created `crates/owl-dl-cb/tests/cb_blowup.rs` with:

**1. `adversarial(n_pairs: usize) -> InternalOntology` generator**

Builds an OFN string and parses it (using the `parse` helper copied from
`cb_sequoia_diff.rs`). The adversarial pattern uses `2·n_pairs` atoms
split into `n_pairs` pairs `(A0,B0), …, (A{n-1},B{n-1})`:
- All atoms pairwise disjoint (via `SubClassOf(ObjectIntersectionOf(:Xi :Xj) owl:Nothing)`)
- `SubClassOf(:C ObjectSomeValuesFrom(:R owl:Thing))` — forces a successor context
- `SubClassOf(owl:Thing ObjectAllValuesFrom(:R ObjectUnionOf(:Ai :Bi)))` for each pair i

The Succ rule spawns a context for the ⊤-filler; R∀ back-propagates each
`∀R.(Ai⊔Bi)` into that context. The `k` resulting clauses `{Ai,Bi}` form
a subset-antichain (pairwise incomparable under ⊆), so S1's Elim rule cannot
collapse any pair → ≈ 2^k derived clauses.

**2. `run_with_timeout(f, timeout) -> Option<Duration>` helper**

Uses `mpsc::channel` + `recv_timeout` (not `thread::join` which has no timeout).
Worker thread is deliberately leaked on timeout. Exposed as `pub(crate)` for
Task 3/5 (taming regression tests).

**3. `agreement_on_tiny` (non-ignored, runs in CI)**

Both B1 and S1 agree on `adversarial(2)` (4 atoms, 6 disjointness axioms,
2 universals). Confirms: valid in-fragment ALCH, S1 not unsound at small scale,
both terminate fast.

**4. `s1_blows_up_on_adversarial` (ignored baseline)**

`#[ignore = "baseline: S1 expected to hang; run explicitly to verify"]`
Spawns `classify_sequoia(&adversarial(13))` on a thread, waits 30s, asserts
it did NOT finish. PASSES.

### Smallest N that blows up

**N_BLOWUP = 13** (debug build, 32-core/251GB Linux, 2026-07-28)

Wall-time sweep:
```
n= 4 →   6 ms
n= 5 →  17 ms
n= 6 →  44 ms
n= 7 → 114 ms
n= 8 → 298 ms
n= 9 → 790 ms
n=10 →   2.1 s
n=11 →   5.6 s
n=12 →  15.4 s
n=13 → TIMEOUT (>35 s)  ← N_BLOWUP
```
Growth ≈ 2.5–2.7× per step, consistent with ≈ 2^k antichain clause accumulation.

### Verification

```
cargo test -p owl-dl-cb                                                 → 93 pass (92 existing + agreement_on_tiny), 1 ignored, 0 failed
cargo test -p owl-dl-cb --test cb_blowup -- --ignored s1_blows_up_on_adversarial → PASS (30.03 s, S1 did not finish)
cargo build -p owl-dl-cb --tests                                        → clean
cargo clippy -p owl-dl-cb --tests -- -D warnings                        → clean
cargo fmt -p owl-dl-cb -- --check                                       → clean
```

### Files changed

- `crates/owl-dl-cb/tests/cb_blowup.rs` (new file, 213 lines)

### Concerns

None. The blowup reproduces cleanly on the first pattern tried — no tuning
required. The exponential growth curve is clear and the N_BLOWUP=13 gives
a comfortable safety margin above the 30s threshold (n=12 → 15s, n=13 → >35s).
