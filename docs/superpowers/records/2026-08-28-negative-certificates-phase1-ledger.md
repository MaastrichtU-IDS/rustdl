# Execution record — negative certificates Phase 1

Ledger from the subagent-driven execution of
`docs/superpowers/plans/2026-08-28-negative-certificates-phase1.md`.
Preserved from the git-ignored SDD workspace, which is deleted at completion.
It carries 22 rulings made during execution and every deferred/parked finding.

```
# SDD ledger — plan: docs/superpowers/plans/2026-08-28-negative-certificates-phase1.md

Spec: docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md (v3) — read, reachable.
Branch: feat/negative-certificates-phase1 (local only; origin keeps a curated branch set).

## Ruling: workspace is a local BRANCH, not a git worktree
A worktree needs its own `target/` (current one is 4.7 GB, so ~5 GB more), this session already had
to free ~537 GB, and CLAUDE.md records six existing worktrees under `.claude/worktrees/` as a
management burden — one with uncommitted changes git cannot recover. Tree is clean and I am the sole
actor. COST IF WRONG: no filesystem isolation from other work in this checkout.

## Pre-flight scan — task pairs sharing a file or interface

| A | B | A produces / B consumes | finding |
|---|---|---|---|
| T1 | T2-T13 | `Element`, `Interpretation` | clean |
| T2 | T4 | `FiniteModel`, stubbed `Interpretation` methods | clean — T2 stubs, T4 fills; plan says so |
| T2 | T4 | `seed(internal, subs, facts)` signature | **RESOLVED pre-flight** — T4 originally said hierarchy is "set in seed"; now a separate `with_hierarchy` builder so T2's tests keep compiling |
| T3 | T4,T5,T6 | `build_role_hierarchy`, `effective_ranges` | clean |
| T4 | T5 | `Bounds`, `UnresolvedReason`, `expand` | clean |
| T5 | T6 | `build_model` | **RESOLVED pre-flight** — T5 originally called `chain_range_out_of_profile`, which T6 defines; T5 now carries a placeholder comment and T6 inserts the call |
| T6 | T12,T13 | chain edges, refusal | clean |
| T7 | T8,T9 | `Judgement`, `eval_concept` | clean |
| T8 | T9 | `AxiomVerdict`, `check_axiom` | clean |
| T8 | T2/model.rs | needs `test_only_remove_from_label` ON `FiniteModel` | **GAP — see Ruling 1** |
| T9,T10 | T10,T11 | `check_axiom` -> `verify` -> `VerifiedModel` | clean |
| T10 | T12,T13 | `verify`, `Verdict` | clean |
| T12 | fixtures | `chainrange_ctl.ofn` listed as a HEALTHY control | **GAP — see Ruling 2** |
| T13 | T10 | CLI over `verify`/`build_model` | clean |
| T14 | — | docs + saturation doc-drift only | clean |

## Pre-flight scan — per-task self-consistency

T1 clean. T2 clean (path typo fixed pre-flight). T3 clean. T4 clean. T5 clean.
T6 clean. T7 clean — all 12 `ConceptExpr` variants listed explicitly.
T8/T9 **variant counts corrected pre-flight** to 8 + 5 = the 13 checked variants
(`SubObjectPropertyOf` is ONE variant with two arms). T10 clean. T11 clean. T12 see Ruling 2.
T13 clean. T14 clean.

## Ruling 1: T8's Files list must include `Modify: src/model.rs`
T8's sabotage matrix needs `FiniteModel::test_only_remove_from_label`, which lives in `model.rs`,
but T8's Files list names only `src/eval.rs`. Gate it behind
`#[cfg(any(test, feature = "test-mutations"))]` so production code cannot mutate a model. Carried in
T8's dispatch. COST IF WRONG: an implementer refuses to touch model.rs and cannot write the matrix.

## Ruling 2: `chainrange_ctl.ofn` is a DEFECT case, not a healthy control
T12 lists it among controls that must return `Verified`. **Measured: rustdl MISSES `C ⊑ D` on it and
Konclude reports it** — it is issue #80's second instance. The review that produced this fixture
labelled it a control; the measurement contradicts the label. Move it to the defect list; the
healthy controls are `unsatconj.ofn`, `flat-mono.ofn`, `label-closure-range-sub.ofn`.
COST IF WRONG: T12 asserts `Verified` on a genuinely-broken ontology and fails for the right reason
with a misleading message.

## API pre-verification (so implementers do not re-derive)
`Vocabulary::{role_id :110, class_id :105, intern_class :71}`, `ConceptPool::{atomic :322,
and :367 — takes impl IntoIterator}`, all `pub`. OFN test parsing: `read_ofn(&mut reader, cfg)`
returns a TUPLE `(SetOntology<RcStr>, _)`; see `crates/owl-dl-cli/src/main.rs::parse_ofn`.

## Progress
Task 1: dispatched (fable, mechanical transcription) BASE=7e635fe
Task 1: implementer DONE_WITH_CONCERNS -> reviewing (commits 7e635fe..465b44c)
  concern: `cargo` not on PATH in the subagent sandbox; use
  /home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo
  CARRY THIS INTO EVERY SUBSEQUENT DISPATCH.
  concern: Cargo.lock committed (new workspace member) — correct, keep.
  concern: tests/fixtures/*.ofn pre-existed on the branch (my earlier fixture commits) — expected.
  controller-verified: bare `cargo check` (no -p) compiles owl-dl-verify => default-members
  wiring is correct; 11 workspace members. The implementer's -p commands could not show this.
Task 1: complete (commits 7e635fe..465b44c, review clean — spec OK, quality approved)
Task 1: minor (deferred): the `_ =>` guard is a literal string scan over eval.rs — false-negative on
  `other =>` / `_ if cond =>`, and FALSE-POSITIVE if eval.rs prose ever contains the literal `_ =>`.
  CARRY: warn Task 7 not to write that literal in a doc comment.
Task 1: minor (deferred): the `owl_dl_saturation` substring guard proves less than it claims — it
  cannot catch eval.rs reaching engine state indirectly via `crate::model`. Ruling: accept; the real
  enforcement is eval's SIGNATURES (only `&ConceptPool` + `&impl Interpretation`), which Task 7/8/9
  dispatches will require explicitly. COST IF WRONG: a future eval.rs could smuggle engine state
  through a model-typed argument and the guard would stay green.
Task 1: minor (deferred): Interpretation uses RPITIT so the trait is not object-safe (no
  Box<dyn Interpretation>). Spec already notes this; Phase 3 inherits it.
Task 1: note: fixtures were pre-staged by the plan author — reviewer flags this as an inherited
  hazard for whichever task consumes them (Task 12). Carry to Task 12.
Ruling: PLAN DEFECT fixed mid-flight — `sort_unstable_by_key(ClassId::index)` /
  `binary_search_by_key(.., ClassId::index)` do NOT compile (E0631: the adapters pass `&ClassId`,
  but `index` takes `self`). Verified with a standalone rustc probe, not assumed. The plan had this
  in SIX places across Tasks 2-5; all rewritten to closures plus a Global Constraints note so a
  later reader does not tidy them back. Task 2's implementer had already hit and fixed it locally.
  COST IF WRONG: none identified — the closure form is the documented idiom and is what compiles.
Task 2: complete (commits 465b44c..a11ce69, review clean — spec OK, quality approved)
  reviewer independently traced Subsumers::subsumers_of -> row_ascending, confirming the
  binary_search precondition holds; and confirmed ClassId's Ord agrees with .index() order, so the
  closure rewrite is semantically identical, not merely compile-shaped.
Task 2: minor (deferred): `intern` clones the label key on every new label (from my brief) — an
  extra heap alloc; `entry`-style access would avoid it.
Task 2: minor (deferred): the OFN `load()` test helper is local to tests/model.rs. Tasks 3-6 also
  use tests/model.rs so no copy is needed there; the risk is TASK 7, which creates
  tests/evaluator.rs. CARRY: tell Task 7 to extract `load()` into tests/common/mod.rs rather than
  copy it.
Ruling: PLAN DEFECT fixed pre-emptively (Task 4) — the plan said `hierarchy: RoleHierarchy` "(it is
  Default)". It is NOT: RoleHierarchy derives only Debug+Clone (role_hierarchy.rs:132), while
  FiniteModel derives Default and `seed` uses `..Self::default()`, so the field would have broken
  both. Changed to `Option<RoleHierarchy>`, which keeps the derive and composes with the
  out-of-range rule (no hierarchy == empty extension, same as an unknown role). Caught by reading
  the derive rather than waiting for Task 4 to fail. COST IF WRONG: an extra Option match in one
  private accessor.
Task 3: complete (commits 7f25985..9b20f56, review clean — spec OK, quality approved)
  reviewer hand-traced that swapping super_roles->sub_roles would fail BOTH assertions in the new
  test, so the direction is genuinely pinned, not merely asserted.
Task 3: ⚠️ RESOLVED BY CONTROLLER: the reviewer had no cargo on PATH and could not run the gates.
  I ran them on the tree: fmt rc=0, clippy -D warnings rc=0, tests 6 passed (2 independence,
  4 model). CARRY: give REVIEWERS the PATH export too, not just implementers.
Task 3: minor (deferred): EquivalentObjectProperties handling is an O(k^2) double loop incl. self
  pairs (harmless no-ops); fine at expected group sizes.
Task 4: complete (commits 21fcda8..c08f11d, 2 brief tests #[ignore]d with data-backed reasons)

Ruling: SPEC-LEVEL DEFECT — the construction cannot be driven by the saturator's fact list alone,
  and Task 4's implementer was RIGHT to ignore two brief tests. Adjudicated by controller probe
  (throwaway example, since deleted), dumping saturate_with_exists_facts:

    flat   C ⊑ ∃u.A + Range(u,F):  1 fact C--u-->T#3,  subsumers(T#3) = [A, F, T#3]
    nested C ⊑ ∃t.∃u.A:            1 fact C--t-->T#3,  subsumers(T#3) = [T#3]

  So a FLAT existential yields a properly range-folded successor (aug empty, exactly as the spec
  predicted), but a NESTED one yields an OPAQUE, EMPTY-LABELLED element with NO outgoing fact. The
  model is therefore shallower than the ontology, and `eval(∃t.∃u.F, x_C)` is vacuously false —
  meaning the instrument as specified would MISS issues #80 and #81, its own headline prey.

  RULING: insert Task 4b — expansion must ALSO be axiom-driven, walking ConceptPool structure for
  positive ∃ occurrences, creating one element per nested existential body labelled by that body's
  closure. This is additive to Task 4's fact-driven expansion, and is arguably better for
  independence: deriving structure from AXIOMS rather than from engine facts reduces the model's
  dependence on engine internals.
  COST IF WRONG: Task 4b adds elements the facts do not justify; a spurious element can only make
  an axiom check fail (false Violated, noisy), never pass (no false all-clear), because an element
  with a too-small label cannot satisfy an axiom it should.

  BY-PRODUCT: this probe independently found the MECHANISM of issue #80 — the nested Tseitin marker
  is an opaque atom with an empty subsumer set, so nothing about its body propagates. Posted to #80.
Task 4: complete (commits 21fcda8..c08f11d, review clean — spec OK, quality approved)
  reviewer re-ran all gates AND `-- --ignored`, confirming the two #[ignore] reasons reproduce the
  documented failure messages (checked, not merely plausible), and that the replacement PAIR does
  exercise the Err(aug)/LabelNotClosed branch (the AND-wrapped test: eff[u]=[F], subsumers(A)=[A]
  => aug=[F] => Err). So that path is not dark.
Ruling: 4th PLAN DEFECT, found by the implementer — the brief's PROBE_B test never called
  `.with_hierarchy(h)`, so `successors` would have returned &[] unconditionally, INDEPENDENTLY of the
  saturator gap. They added the call so the only remaining failure reason is the real gap. Accepted;
  Task 4b's own test includes `.with_hierarchy(hier)`. COST IF WRONG: none — the fix is required for
  the test to mean anything.
Task 4: minor (deferred): Bounds.max_rounds is unused by `expand` (Task 4b/5 consume it).
Task 4: minor (deferred): five UnresolvedReason variants are as-yet unconstructed (later tasks).
Task 4b: review — spec OK, quality approved WITH ONE IMPORTANT finding. Reviewer independently
  reproduced both #[ignore] probe claims by temporarily wiring expand_from_axioms itself (reverted
  clean), confirming one would pass and one still fails at in_concept(witness, F) — so the deferral
  to Task 5 is tracked, not dropped (plan commit dc36608 requires it).
Ruling: Task 4b Important finding is MINE (brief-inherited) and correct. On target_label's Err path
  the code added NOTHING for the atom — not even its own uncontested subsumers_of — so a leaf label
  came back EMPTY and in_concept(leaf, A) was false though the axiom entails it. FIX: on Err, still
  extend with subsumers_of(atom) and keep pushing LabelNotClosed for the augmentation. Sound because
  subsumers_of(A) is entailed of the witness unconditionally; only the range augmentation is in
  doubt, and that stays reported. A fuller label can only make a check pass that would otherwise
  have failed SPURIOUSLY, so no false Verified is introduced. Plus a regression test for the
  reviewer's exact probe shape, which was untested.
  COST IF WRONG: a witness carries a class it should not, which could mask a genuine violation —
  bounded by the fact that the added classes are exactly the atom's own entailed subsumers.
Task 4b: fix round 1/5 dispatched (resumed original implementer).
Task 4b: minor (deferred): the Axiom::EquivalentClasses branch of expand_from_axioms is untested.
Task 4b: minor (deferred): EquivalentClasses rule generation is O(k^2) in member count.
Task 4b: fix round 1/5 (1 addressed, 0 open; commits 588fd54..a789c87) — re-reviewer confirmed the
  range classes are PROVABLY excluded (Err payload discarded, so the fix path is disjoint from the
  rejected classes), LabelNotClosed still fires unchanged, and the new regression test genuinely
  lands on the Err branch. Gates re-run: 8 passed / 2 ignored, clippy clean.
Task 4b: complete (commits 1e96d08..a789c87, review clean after 1 fix round)
Ruling: MY PREMISE WAS WRONG and Task 5 measured it — I asserted cascade.ofn needs three injection
  rounds. It converges in ONE at any max_rounds>=1, because that analysis described the FACT-driven
  path (conditional exists-RHS has no anchor class) and Task 4b's axiom-driven expansion removes the
  limitation. The implementer rewrote the test AND documented the contradiction in-code rather than
  silently adjusting the assertion — correct behaviour. Plan and spec both corrected.
  CONSEQUENCE, carried to Task 13: the fixpoint past round 1 is now UNTESTED MACHINERY. Task 13 must
  either add a fixture needing >=2 rounds or record that the loop is unexercised. Do not let a
  green suite imply the loop works.
  COST IF WRONG: none for correctness; the risk is unjustified confidence in an unexercised loop.
Task 5: concern accepted — a filter excludes Tseitin-marker-targeted LabelNotClosed from the
  INJECTION WORKLIST because marker ids drift every round (exactly the drift hazard the spec warns
  about), so `pending` would never converge. Reports are still RETURNED, not dropped. Sound.
Task 5: minor (deferred): expand_from_axioms' internal round cap SHARES bounds.max_rounds with
  build_model's outer loop, so max_rounds:1 starves both at once and neither can be isolated in a
  test. CARRY to Task 13's bound tests.
Task 5: minor (deferred): the RunDelta-on-unsatisfiable-injected-Q branch is implemented but
  exercised by NO fixture — flagged honestly by the implementer rather than claimed as tested.
Ruling: MY PREMISE WAS WRONG and Task 5 measured it — I asserted cascade.ofn needs three injection
  rounds; it converges in ONE at any max_rounds>=1. That analysis described the FACT-driven path
  (conditional exists-RHS has no anchor class); Task 4b's axiom-driven expansion removes it. The
  implementer rewrote the test AND documented the contradiction in-code rather than silently
  adjusting the assertion. Spec corrected (the plan carried no round-count claim).
  CONSEQUENCE, carried to Task 13: the fixpoint past round 1 is UNTESTED MACHINERY. Task 13 must add
  a fixture needing >=2 rounds or record that the loop is unexercised. A green suite must not be
  read as evidence the loop works. COST IF WRONG: unjustified confidence in an unexercised loop.
Task 5: concern accepted — a filter excludes Tseitin-marker-targeted LabelNotClosed from the
  INJECTION WORKLIST because marker ids drift every round (the exact drift hazard the spec warns
  about), so `pending` would never converge. Reports are still RETURNED, not dropped. Sound.
Task 5: minor (deferred): expand_from_axioms' internal round cap SHARES bounds.max_rounds with
  build_model's outer loop, so max_rounds:1 starves both and neither can be isolated. CARRY to
  Task 13's bound tests.
Task 5: minor (deferred): the RunDelta-on-unsatisfiable-injected-Q branch is implemented but
  exercised by NO fixture — flagged honestly rather than claimed as tested.
Task 5: review — spec ❌, quality CHANGES NEEDED. Three Important findings:
  (1) THE FIXPOINT DOES NOT CONVERGE on AND_WRAPPED_NESTED_RANGE, a fixture already in the suite:
      expand()'s Err branch never got the injected-Q lookup materialise_exists received, so it
      re-reports LabelNotClosed every round, pending never empties, and max_rounds trips. Verified by
      the reviewer with a scratch probe diffed clean against the committed code. Fails SAFE
      (BoundTripped, not a false Verified) but defeats the task's purpose, and no test drove
      build_model (vs bare expand) on that fixture.
  (2) STALE CLAIM INHERITED FROM MY BRIEF — run_deltas' doc says "Measured on unsatnested.ofn:
      injection flips X from satisfiable to unsatisfiable". It does not reproduce: step=[] round 0,
      no injection, reasons=[].
  (3) RunDelta has ZERO test coverage despite being a headline feature of the task.
Ruling: finding (2) is MY error and I can explain it — that observation came from a MANUAL experiment
  in which an investigator hand-wrote `Q ≡ ∃s.Y ⊓ F` into the ontology. It does NOT describe the
  automatic aug-driven injection, which never fires on that fixture. I conflated a manual probe with
  the automatic path when writing the comment. Fix: re-measure and rewrite; keep the conceptual point
  (a run-delta on an original class IS a defect signal) but stop attributing it to a fixture where
  nothing fires. COST IF WRONG: none — the correction only removes an unsupported attribution.
Ruling: finding (1) fix is a SHARED injected-Q lookup called from both Err branches, so the two
  expansion paths cannot disagree about whether a gap is already closed, plus a build_model-level
  convergence test. COST IF WRONG: if expand() should genuinely be exempt, the shared helper is dead
  weight in one caller — cheap, and the test still pins convergence either way.
Task 5: fix round 1/5 dispatched (resumed original implementer).
Task 5: minor (deferred): pending.sort/dedup diverges from the brief without an in-code rationale.
Task 5: fix round 1/5 (3 addressed, 0 open; commits 4728efa..e429728) — re-reviewer SABOTAGED F1
  (reverted expand's Err arm, saw the test reproduce BoundTripped{max_rounds:8} + repeated identical
  LabelNotClosed, restored clean), so the convergence test is proven non-vacuous. F2's rewritten
  comment independently re-measured (reasons==[], X unsat=false) and its "no fixture can trigger
  this" line is hedged as an inability-to-construct plus a structural argument, not asserted as a
  theorem. F3's RunDelta genuinely arrives from the injected-Q-unsatisfiable branch (traced), and the
  still-uncovered top-level comparison is documented IN SOURCE, not only in the report.
Task 5: complete (commits a789c87..e429728, review clean after 1 fix round)
Task 6: implementer flagged that three fixtures "have the exact shape the predicate refuses" and
  worried a later task expects them un-refused. RESOLVED by controller probe (throwaway example,
  deleted) — they had not accounted for the Bot skip. Measured `build_model` on all 11 fixtures:
    chainpoison             BUILT  domain=4   reasons=none      <- crown jewel A1 intact
    chain-range-bot         BUILT  domain=4   reasons=none      <- A2 intact (Range(r,Bot) is a Bot
                                                                  filler, skipped from eff_ranges,
                                                                  so the refusal CANNOT fire)
    chainrange              REFUSED ChainRangeOutOfProfile      <- matches Task 12's A5
    chainrange_ctl          REFUSED ChainRangeOutOfProfile
    cascade                 BUILT  domain=11  reasons=3 LabelNotClosed
    unsatnested/unsatconj/nested-mono/flat-mono/label-closure/botfiller  BUILT, reasons=none
Ruling: TWO amendments to Task 12 follow from this measurement.
  (a) `chainrange_ctl.ofn` is REFUSED, so it cannot be a detection. My earlier ruling moved it from
      "healthy control" to "defect list"; it now belongs in a THIRD bucket: refused, i.e. a known
      coverage loss that Phase 2's fold would recover. Listing it as a detection would make Task 12
      assert something the refusal makes impossible.
  (b) `cascade.ofn` builds but carries 3 LabelNotClosed, so A3 will likely come back UNRESOLVED
      rather than VIOLATED. That is honest but WEAKER than the plan claims for the instrument's
      sharpest prey. Task 12 must record which acceptance fixtures land on Unresolved rather than
      quietly counting them as detections.
  COST IF WRONG: overstating the instrument's coverage in the very table meant to demonstrate it.
Task 6: review — spec ✅, quality approved with TWO Important findings.
  (1) LATENT INFINITE LOOP: close_chains_and_transitivity sets changed=true whenever push_edge
      returns false, but push_edge returns false BOTH on append and on a missing bucket. Not live at
      the sole call site (reflexive has_edge guard + same `working` used for seed), but it is a pub fn
      and a caller passing a mismatched `internal` would HANG rather than error, on an unstated
      unchecked invariant.
  (2) The report's "behaviourally identical" claim about push_edge is wrong on one axis: push_edge
      increments the SHARED edge_count, so max_edges is now a combined budget across expand +
      expand_from_axioms + closure, not per-call. Better semantics (and what I instructed) but not
      identical — and "identical" would stop a later reader looking.
Ruling: 5th PLAN DEFECT, mine — the brief's Step 1 test used role_id("http://t/r") but
  chainpoison.ofn declares http://ex.org/ (I borrowed the IRI from chain-range-bot.ofn, which does
  use http://t/). The implementer fixed it correctly but did not document it; asked to add it to the
  adaptation list. COST IF WRONG: none, already fixed in code.
Ruling: prefer making push_edge distinguish "appended" from "no bucket" over documenting the
  precondition, because a hang is the worst failure mode to leave to a comment. Either is acceptable
  if the doc is explicit AND an assertion exists. COST IF WRONG: a slightly wider push_edge return
  type used by three call sites.
Task 6: fix round 1/5 dispatched (resumed original implementer). Also asked for the reviewer's
  soundness argument to be recorded in the doc comment — it is stronger than "3 tests pass" and would
  otherwise be lost with the review.
Task 6: minor (deferred): raw `Chain(r,r) ⊑ r` and inverse-polarity legs/heads are correct by code
  reading but untested — no fixture exercises either form.
Task 6: fix round 1/5 (2 Important + 2 doc addressed, 0 open; commits 66ab95b..10a16e5) —
  PushOutcome{Appended,AlreadyPresent,NoBucket,BoundTripped}; re-reviewer checked ALL THREE
  push_edge call sites, confirmed changed=true only on Appended, BoundTripped still propagates with
  the bound named, and no conflation of AlreadyPresent/NoBucket with success. Soundness argument
  recorded accurately at model.rs:596-615.
Task 6: complete (commits e429728..10a16e5, review clean after 1 fix round)
Task 7: review — spec ✅, quality CHANGES NEEDED, one CRITICAL.
  The And-propagation test does not exercise the And arm: ConceptPool::and NORMALISES — it drops Top
  as identity and returns a single surviving operand UNWRAPPED — so pool.and([top, all_r_x]) IS
  all_r_x, and the test silently re-tested the CE::All arm. Reviewer PROVED it by hardcoding the And
  tail to Judgement::True (the exact collapse my brief names as the primary risk): all 8 evaluator
  tests still passed. Sabotaging the other three crux directions each failed correctly. So 3 of 4
  guarded, and the unguarded one is the one I listed FIRST.
  Implementation is correct; this is a coverage gap, not a shipped bug.
Ruling: fix = a genuine non-identity second operand, PLUS an in-test assertion that the pool really
  produced an And node, PLUS the implementer sabotage-verifying their own repaired test. The shape
  assertion is the durable part — it documents the trap in place and stops a future fixture edit
  silently re-vacuating the test. COST IF WRONG: one extra assertion per compound-concept test.
CARRY TO TASK 8/9: ConceptPool::and normalises (drops Top; unwraps a single operand). Any test that
  builds a compound concept and asserts about the compound ARM must verify the pool produced that
  shape. Task 8 builds And-shaped axiom fixtures and will hit this.
Task 7: fix round 1/5 dispatched (resumed original implementer).
Task 7: fix round 1/5 completed BY THE CONTROLLER — the implementer subagent was killed by an API
  spend-limit error (HTTP 429, session limit resets 18:10 Europe/Amsterdam) after applying the fix
  but before committing or sabotage-verifying it. Subagent dispatch unavailable.
Ruling: finish the round in-session rather than stop. The user said continue; a spend limit is not one
  of the four stop conditions; and the fix was fully specified by my own instruction, so no judgement
  was delegated back to me that I had not already made. COST IF WRONG, stated plainly: a
  controller-made fix SKIPS the independent review seat. Mitigated by (a) running the sabotage myself
  and (b) recording the gap in task-7-report.md and here, so the final whole-branch review can
  re-examine it. Flagged for that review.
  Sabotage result: And tail -> Judgement::True fails EXACTLY ONE test (left: True, right:
  Unresolved("All")); before the repair the same sabotage left all 8 passing. eval.rs restored
  byte-identical. Gates: fmt 0, clippy 0, 27 tests pass / 2 ignored.
Task 7: complete (commits 10a16e5..HEAD, 1 fix round, fix NOT independently reviewed)
--- spend limit lifted 18:56 CEST; subagent dispatch resumed ---
Controller work done while blocked: FULL workspace gates green (157 suites / 1756 passed / 0 failed,
  exit 0; fmt --all clean; clippy --workspace --all-targets --all-features -D warnings clean), so the
  new crate breaks nothing. Task 14 Step 1 doc-drift fix landed (f8ed1fb: RUSTDL_EL_BOT_FILLER
  documented OFF at lib.rs:103 while the predicate at :149 is the default-ON idiom). Branch backed up
  to refs/archive/heads/feat/negative-certificates-phase1.
Task 8: dispatched (sonnet). Carried: the ConceptPool::and normalisation trap from Task 7's Critical;
  Ruling 1 (model.rs is in scope for test_only_remove_from_label, and integration tests are separate
  crates so #[cfg(test)] will not reach them — a feature is needed); index+witness pinning; the two
  eval.rs text guards; class_iri panics on Tseitin ids so Fails notes must render by label.
Task 8: complete (commits f8ed1fb..d3becd8, review clean — spec ✅, quality approved, Minors only)
  Reviewer INDEPENDENTLY reproduced 5/5 sabotages (each arm stubbed to Holds fails exactly its own
  test), verified via `strings` on the built rlib that test_only_remove_from_label is ABSENT from
  production builds and present only once tests compile, and probed Unresolved propagation itself.
  Judged the DisjointClasses deviation sound: label removal is truth-DECREASING so it structurally
  cannot manufacture "two members true"; FiniteModel::intern makes a real element that elements()
  iterates and in_concept binary-searches, i.e. the same code paths a legitimate element hits.
Task 8: minor (deferred): no COMMITTED test pins Unresolved propagation at the check_axiom level for
  Task 8's 8 variants (reviewer verified the behaviour by ad hoc probe; nothing regression-protects
  it). CARRIED INTO TASK 9's unhandled-variant loop.
Task 8: minor (deferred): Domain/Range baseline Holds tests do not themselves assert edges(p) is
  non-empty — non-vacuousness rests on the sibling sabotage test. CARRIED INTO TASK 9.
Task 8: minor (deferred): axiom_index() returns the first match with no uniqueness check — safe on
  today's fixtures, latent if one later carries two axioms of the same shape.
Task 8: minor (deferred): the Fails `note` text is never asserted by any test.
Task 9: complete (commits d3becd8..4f5dac8, review clean — spec ✅, approved, Minors only)
  Reviewer reproduced 7/7 sabotages independently, incl. the narrow InverseObjectProperties
  "drop only the q check" variant, which failed EXACTLY the q-test while the p-test and the
  bare-empty test stayed green. unhandled_axiom_samples() returns 15 (12 + Chain len1 + Chain len3 +
  EquivProps-with-inverse). All three carried Task 8 Minors folded in.

Adjudication 1 — MY READING WAS RIGHT BUT INCOMPLETE, and the correction matters.
  Confirmed provably: since build_role_hierarchy records `sub ⊑ sup` into the SAME closure that
  has_edge/edges walk, sub_roles(sub) ⊆ sub_roles(sup) unconditionally, so any edge scanned as the
  antecedent is already in the consequent's search space — for the identical physical edge. No edge
  mutation (add OR delete, stronger than the implementer's "deletion") can sever one from the other.
  So SubObjectPropertyOf(Role) and EquivalentObjectProperties are TRUE BY CONSTRUCTION and cannot
  catch a build_role_hierarchy bug...
  ...BUT ONLY in the "verify a freshly-built model against its own ontology" mode, i.e. Goal 1's D10
  gate. `still_holds_after` checks ADDED axioms against the EXISTING unchanged model, where
  `sub ⊑ sup` is NOT yet in the hierarchy (the spec explicitly anticipates "the edit introduces a
  role"), so there the check is GENUINE and non-vacuous. The arms are therefore NOT dead weight, and
  I would have recorded them as such without this nuance.
  CARRY TO TASK 11 and TASK 14: put this scoping on check_axiom's doc comment so a future reader does
  not delete the arms as useless.

Adjudication 2 — a genuine PRE-EXISTING architectural finding (earlier tasks, not Task 9).
  materialise_exists' opaque-body branch labels a nested-existential witness from eff_ranges only
  (often empty), while the fact path labels the same logical position from subsumers_of(Tseitin Q)
  (non-empty, at least {Q}). intern() dedups purely by LABEL CONTENT, so one logical witness becomes
  TWO elements — which contradicts the spec's own "canonical model / one interpretation" framing.
  Not shown unsound for Task 9's checks (an extra edge-less element contributes to no composed pair),
  but it is a precision gap, and whether a future concept-level check could read a WEAKER answer at
  the under-labelled witness is untested.
  CARRY TO TASK 14: needs a docs/known-limitations entry plus a comment on the opaque-body branch —
  it currently survives only in a task report and test comments, which a future model.rs reader will
  miss.
Task 9: minor (deferred): four role-shaped arms share an almost-identical scan shape; a helper would
  cut duplication. Chain/EquivProps checks are O(|edges|^2)/O(members^2 * |edges|) — fine at current
  Bounds.
Task 10: review — spec ✅, quality CHANGES NEEDED, two Important.
  (1) The DEADLINE path has zero test coverage: requirement 4 is implemented correctly but no test
      passes a deadline at all (grep for BoundTripped/deadline/Instant in tests/evaluator.rs: zero
      matches; the only BoundTripped tests exercise build_model's CONSTRUCTION bounds). `limit: None`
      is what distinguishes a deadline from a count, and nothing pins it.
  (2) A CARRIED INSTRUCTION WAS BYPASSED. The Task 9 carry said "put this scoping on check_axiom's
      doc comment"; Task 10 put it on `verify` instead, citing eval.rs's text guards. The reviewer
      checked the guards: they scan only for "owl_dl_saturation" and "_ =>", so a plain doc pointer
      trips NEITHER. The justification does not hold, and the failure mode the note prevents is
      unguarded from the one file most likely to be open when the thought occurs.
Ruling: both fixes are one-liners and go back to the implementer. On (2) specifically — a carried
  instruction declined for a reason that does not survive checking is worse than one declined openly,
  because the ledger then records it as honoured. Reinstated. COST IF WRONG: a redundant doc pointer.
Ruling: adopt the reviewer's compile_fail-doctest suggestion. rustdoc's compile_fail fence is stock
  cargo test with no new dependency and actually attempts to compile the call rather than grepping a
  string — strictly better than the source scan, and its failure message when Task 11 lands the
  method is unambiguous. This is what "assert it by a doc-comment invariant" should have meant in my
  brief. COST IF WRONG: a doctest that must be deleted in Task 11 — which is the point, and it is now
  explicitly labelled as such.
Task 10: fix round 1/5 dispatched (resumed original implementer). Also: the report's claim that BOTH
  scans need Task 11 updates is wrong — one scans model.rs and stays valid; corrected so Task 11's
  implementer does not edit the wrong test.
Task 10: fix round 1/5 (5 items addressed, 0 open; commits 9939152..2551f61) — re-reviewer verified
  the compile_fail doctest BY EXPERIMENT: converting the fence yields exactly
  `E0599: no method named 'still_holds_after' found for struct 'FiniteModel'`, not an unresolved
  import or unrelated error, so it tests what it claims. Restored clean.
Task 10: complete (commits 4f5dac8..2551f61, review clean after 1 fix round)
Task 11: complete (commits 2551f61..b2fb73b, review clean — spec ✅, approved, 2 Minors)
  Reviewer sabotaged still_holds_after to return Verified unconditionally: 3 of 6 tests fired,
  including the designated essential negative AND the bonus non-vacuity test. Also sabotaged the
  fresh-role guard (panics "index out of bounds: the len is 1 but the index is 42"), confirming it is
  load-bearing and that edges/has_edge/successors all route through it.
  NON-VACUITY CONFIRMED: TWO_ROLES_FIXTURE declares p and q with NO linking axiom, so the model has
  one p-edge and zero q-edges, and hierarchy_sub_roles(q) = {q} only — the added
  SubObjectPropertyOf(p,q) genuinely cannot be satisfied. So the incremental path does real work; it
  has not inherited the true-by-construction tautology that applies when verifying a model against
  its own ontology.
Task 11: minor (deferred): Violation's struct doc is written from verify's perspective and does not
  mention that still_holds_after also produces Violations, whose axiom_index indexes `added` rather
  than internal.axioms. CARRY TO TASK 14.
Task 11: minor (deferred): still_holds_after's ~25-line loop near-duplicates verify's; consolidation
  candidate if a third caller of that shape appears.
Task 12: implementer reports 5 detections / 0 weaker / 2 refused / 3 controls verified.
  MY PREDICTION WAS WRONG IN THE GOOD DIRECTION: I expected cascade.ofn to land on Unresolved
  (it carries 3 LabelNotClosed); it comes back a genuine Violated, traced by hand by the implementer
  and confirmed not a checker artifact. So the instrument's sharpest prey IS a full detection.
Ruling: I verified the one provenance claim I had recorded WITHOUT witnessing — chainrange.oracle's
  `provenance: hermit`. Ran HermiT myself (robot v1.9.6 via the harness wrapper): it reports
  SubClassOf(C,D), so the claim is CORRECT. Also ran it on chain-range-bot: 0 non-trivial rows, so
  all three reasoners fail to confirm that one. Annotated the oracle accordingly, but explicitly NOT
  as a refutation — robot's default axiom generators may not emit unsatisfiable classes, so three
  silences are not three disagreements. COST IF WRONG: the annotation overstates HermiT's coverage of
  unsat; hedged in place.
Task 12: complete (commits b2fb73b..4f0bec2, review clean — spec ✅, approved, Minors only)
  SABOTAGE PAIR IS THE KEY RESULT: always-Verified fails the invariant test (chainpoison, "Got
  Verified"); always-Unresolved makes the invariant test PASS VACUOUSLY and is caught only by
  healthy_controls_are_verified (fails on unsatconj). So the two tests are a genuine pair and the
  second is load-bearing, not redundant.
  VACUITY MEASURED: 3 of 10 fixtures have a FALSE antecedent (the three controls, where rustdl agrees
  with the oracle) and pass the invariant for free; the other 7 (5 detections + 2 refused) have a live
  antecedent. Not a mostly-vacuous suite.
  MY BRIEF'S PROVENANCE WAS COARSER THAN THE SPEC'S and the implementer was right to prefer the spec:
  issue #81 covers TWO fixtures — cascade (Konclude) and unsatnested (HermiT) — so my issue-level
  "Konclude-confirmed" summary was wrong at fixture granularity.
  My chain-range-bot annotation judged careful: says "fail to confirm", never "disagree", and hedges
  the format limitation, so three silences cannot be misread as three disagreements.
Ruling: ACCEPT the spec deviation of adding owl-dl-reasoner as a DEV-dependency (spec §7 says the
  crate must never depend on it). Scoped to [dev-dependencies] only, no cycle (verified: nothing in
  owl-dl-reasoner's manifest names owl-dl-verify), and using the real hybrid classifier is what stops
  the acceptance suite grading its own homework. COST IF WRONG: a dev-only edge in the dependency
  graph; the no-cycle property is what actually matters and it holds. Spec §7 should be amended in
  Task 14 to say "not a runtime dependency".
Task 12: minor (deferred): the reviewer could not re-run Konclude/HermiT itself (docker broken with
  the same byte-identical error, no usable java), so it validated provenance by internal consistency
  against the design record rather than from first principles. My own HermiT run stands as the
  firsthand check for chainrange.
Task 13: review — spec ✅, quality CHANGES NEEDED, one Important.
  SWEEP RESULT: 16/20 Verified, 4 exit-124 timeouts (ore_ont_13752, 13902, 11739, 14879), 0 Violated,
  0 Unresolved. So inertness is ESTABLISHED ON 16 real pure-EL ORE ontologies with zero spurious
  rejections; the other 4 are UNMEASURED, not passed. Conversion was fast on the timeouts
  (convert_ms=35 / 186, controller- and reviewer-reproduced), so the stall is downstream of
  conversion — hedged in the report as suggestive, not proven.
  Reviewer independently reran 155/155, exercised all five exit codes by hand, byte-compared --json
  itself, and judged the 124-vs-3 separation "honest and well-hedged".
  BETTER ANSWER THAN THE IMPLEMENTER GAVE on which loop max_rounds pins: empirically, the PRE-EXISTING
  label-closure-range-sub test trips BOTH loops (two distinct BoundTripped entries), while the NEW
  conjtrigger test trips only the OUTER build_model loop (one entry — expand_from_axioms exits via
  !grew before consulting its own bound). So the new test isolates the outer loop more cleanly than
  its own comment claims.
  IMPORTANT: fold_build_reasons' `Verified -> Unresolved` downgrade arm (main.rs:850-865) has ZERO
  coverage — the reviewer ran all 11 fixtures and none combine a Verified check result with non-empty
  build reasons. That arm is what stops the CLI exiting 0 over an admitted build-time gap, i.e. a
  false all-clear reaching a user.
Ruling: require a fixture that trips the arm, OR an explicit unreachability statement in BOTH the
  report and a comment at the arm, with what was tried. A structurally unreachable branch is a fine
  answer; an untested-and-unremarked one is not. Do not weaken or delete the arm. COST IF WRONG: a
  defensive branch stays unexercised, but it is now remarked either way.
Task 13: fix round 1/5 dispatched (resumed original implementer).
Task 13: fix round 1/5 (1 addressed, 0 open; commits 91c3325..e317801) — implementer CONSTRUCTED a
  real fixture (markerresidue.ofn) producing Verified-plus-3-LabelNotClosed rather than declaring the
  arm unreachable. Re-reviewer reproduced the sabotage: passing the Verified through gives
  `left: Some(0), right: Some(3)` with stdout "verified: 13 axiom(s) checked over a domain of 8
  element(s)" — exactly the false all-clear. Tests at BOTH library and CLI level, the CLI one
  asserting the real process exit code. Sweep reporting confirmed untouched.
Task 13: complete (commits 4f0bec2..e317801, review clean after 1 fix round)
Task 14: complete (commits e317801..30679f2)
  Gates: fmt --all clean; clippy --workspace --all-targets --all-features -D warnings clean;
  cargo test --workspace --exclude owl-dl-py = 160 suites / 1819 passed / 0 failed / 83 ignored
  (was 157 / 1756 before the branch, so +3 suites and +63 tests, nothing broken).
  SOUNDNESS NET: 22 passed / 0 failed in 67s. 12 fixtures VERIFIED with FP=0 / MISSED=0 on every one
  (alehif 247, bibtex 16, pizza 499, galen 27997, galen-5s 27997, notgalen 32739, ore-10908 6001,
  ore-15672 142, ro 158, sio 8904, sulo 51, wine 653, + alehif global-deadline-diff untimed=timed).
  3 NOT VERIFIED purely for ABSENT gitignored fixtures (ro-stripped, sulo-stripped, sio-stripped) —
  not diffs. Unchanged from before the branch, as expected for a crate that touches no reasoning path.
Ruling: FOLD Task 14's task review into the final whole-branch review rather than dispatching a
  separate seat. Task 14 is documentation-only (no logic), and its principal risk — coverage numbers
  rounded or overstated in CLAUDE.md — is exactly what a whole-branch reviewer is best placed to audit
  against the branch's actual behaviour, since it can see the tests and the sweep table. COST IF
  WRONG: a docs-only diff gets one review seat instead of two; recorded so the final reviewer knows to
  cover it.
```
