# Query-surface ledger: unknown versus negative

Work package A2 of the correctness assurance programme (#363). The
[completeness guarantee ledger](completeness-guarantee-ledger.md) covers
`classify`. This one covers every other query surface and asks one question:
**when a probe reaches no verdict, does the caller find out?**

Checked against `main` at `0bbe276c` (31 October 2026); the `consistent` trust row
(#392) against `83853936`, the #369 consistency rows against `47c28f32`, and the
#369 follow-up against `997a1334`
(both 2 November 2026).

## The rule

A probe that reaches no verdict (a deadline, the live-node cap
`RUSTDL_MAX_NODES`, the search-depth bail-out, a verification that gives up) is
answered in its **sound direction**: "satisfiable", "not subsumed", "not an
instance", "not disjoint", "consistent". That is never a false positive. But it
is not a verdict either, so the caller must be told. #363: *unknown answers must
not be silently reported as non-entailment, satisfiability or consistency.*

No surface in the #363 audit turned a give-up into a definite **positive**.
#396 found two that did, both now fixed:
- `repair` accepted a candidate whose verification probe gave up, i.e. a removal
  never shown to break the entailment;
- the data-property guard of `justify subdata-property` / `disjoint-data-property`
  read its own give-up as "consistent" and could report a non-entailed axiom as
  entailed.

## How a give-up is reported

| Surface | Signal | Meaning |
|---|---|---|
| Rust `*_with_stats` | `QueryStats::incomplete` | the verdict is a give-up in its sound direction |
| Rust `*_reporting`, `Realization` | `incomplete` (second tuple element, or a field of the flags struct there, e.g. `ExistentialSuccessorFlags`; `Realization::incomplete()`) | at least one probe was cut |
| CLI single answer (`consistent`, `sat`, `subclass`, `instance`, `explain`, `prove`, `sat-expr`, `subclass-expr`) | prints `unknown`, exits 3, explains on stderr | no verdict |
| CLI list answer (`instances`, `realize`, `instances-expr`) | stderr warning; members printed are sound | members may be missing |
| CLI `--json` | `incomplete` field | as the Rust field |
| Python | `IncompleteQueryWarning`; the return value keeps its sound direction | as the Rust field |

These `incomplete` fields mean "a probe gave up". That differs from classify's
`incomplete`, which means only "a per-pair deadline fired" (see the
completeness ledger and #220/#221). Each surface's field documents its own
meaning.

The plain Rust functions (`is_consistent`, `is_class_satisfiable`,
`is_subclass_of`, `is_instance_of`, `instances_of`) return a bare value and
cannot carry the signal. Their docs say so and point to the reporting form.

## Surfaces

| Surface | Give-up sources | Rust | CLI text | CLI `--json` | Python | Pinned by |
|---|---|---|---|---|---|---|
| `consistent` | fall-through tableau deadline (`RUSTDL_CONSISTENCY_FALLBACK_MS`), node cap, abandon (#182); #344 data-counting verification gives up | `is_consistent_with_stats` → `incomplete` | `unknown`, exit 3 | `incomplete`, exit 3 | `is_consistent` warns | `consistency_data_counting.rs`; CLI `consistent` tests |
| `consistent` trusted wedge `Sat` (#392) | not a give-up: `consistent` rests only on the consistency wedge's `Sat`, which no main-tableau run verified (the wedge missed clashes in #290 and #376). Never set on `inconsistent`, on a main-tableau model, or on a #344-verified `Sat`. Broader than `wedge_clauses_refused` | `QueryStats::relies_on_trusted_consistency` (the #401 flag, same meaning) | stdout `consistent`, exit 0, one stderr note | `relies_on_trusted_consistency`, exit 0; the same stderr note | `is_consistent` warns `TrustedConsistencyWarning` | `consistent_trusted_sat.rs` (reasoner, CLI); `consistency_data_counting.rs`; `test_consistent_trusted_sat.py` |
| `sat` | node cap (`RUSTDL_QUERY_MAX_NODES` on the CLI/Python entry point; early abandon is armed only under a deadline). A deadline-free depth bail-out is an error (exit 1), not `unknown` | `is_class_satisfiable_with_stats` → `incomplete` | `unknown`, exit 3 | no `--json` | `is_class_satisfiable` warns | `query_give_up.rs`; `test_query_give_up.py` |
| `subclass` | node cap (`RUSTDL_QUERY_MAX_NODES`) | `is_subclass_of_with_stats` → `incomplete` | `unknown`, exit 3 | no `--json` | `is_subclass_of` warns | same |
| `explain` | node cap | `is_subclass_of_with_stats` | `: unknown — tableau gave up`, exit 3 | — | — | `query_give_up.rs` |
| `instance` | per-probe deadline (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, default 750 ms), node cap, depth bail-out | `is_instance_of_reporting` | `unknown`, exit 3 | no `--json` | `is_instance_of` warns | `query_give_up.rs`; `test_query_give_up.py` |
| `instances` | same | `instances_of_reporting` | stderr warning | no `--json` | `instances_of` warns | same |
| `realize` | same, plus the node cap with the per-probe deadline disabled (`=0`) | `Realization::incomplete()` | stderr warning | `incomplete` | `realize` warns | same |
| `realize` direct types (#388) | the check of two most-specific types the hierarchy left undecided: per-probe deadline, node cap, depth bail-out (or the check is off, `RUSTDL_REALIZE_VERIFY_DIRECT=0`). Not a missing type: a listed type may be a superclass of another. Never set on the saturation fast path | `Realization::direct_types_unverified()` | stderr warning | `direct_types_unverified` | `realize` warns `DirectTypesUnverifiedWarning` | `realize_direct_types.rs`, `realize_direct_types_cap.rs`; `test_direct_types_unverified_warning` |
| `materialize_existential_successors` (#391) | as `realize`; the rows join on ENTAILED types, so `direct_types_unverified` does not apply | `materialize_existential_successors_reporting` → `ExistentialSuccessorFlags::{incomplete, witness_prune_active}` | no CLI surface | — | warns `IncompleteQueryWarning` | `materialize_realize_flags.rs`, `materialize_realize_flags_cap.rs`; `test_materialize_flags.py` |
| Python `materialize_inferred_class_assertions` (#391) | as `realize` (most-specific types) | — (Python only, over `realize`) | no CLI surface | — | warns `IncompleteQueryWarning` and `DirectTypesUnverifiedWarning` | `test_materialize_flags.py` |
| Python `materialize_inferred_subclass_axioms` (#391) | as `classify` | — (Python only, over `classify`) | no CLI surface | — | the same `IncompleteClassificationWarning`s as `classify` | `test_materialize_flags.py` |
| `sat-expr`, `subclass-expr`, `instances-expr` | as `sat` / `subclass` / `instances` (same single-query node cap) | `CeVerdict` / `CeInstances` `incomplete`; separately `completeness_guaranteed` (pure-EL fast path, or for `instances-expr` the realize saturation fast path, and nothing cut) | `sat-expr`/`subclass-expr`: `unknown`, exit 3; `instances-expr`: stderr warning | `incomplete` (`sat-expr`/`subclass-expr` exit 3), `completeness_guaranteed` | warns | `expr_give_up_cli.rs`; `test_expr_give_up.py` |
| `disjoint` (classes) | per-pair deadline, node cap | `DisjointClasses::incomplete` | yes | `incomplete` | warns | `disjoint_nodecap.rs` |
| `individuals` | per-pair deadline, node cap | `incomplete` | yes | `incomplete` | warns | individuals tests |
| `instance`, `instances`, `instances-expr`, `individuals`: consistency (#369) | not a give-up: a POSITIVE answer (an instance, a merge, a difference) on a KB whose consistency the lazy check left undetermined (the consistency wedge stalled or was cut; the main-tableau tail `RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL` is off by default). An inconsistency the check witnesses is `Err(Inconsistent)`, exit 1, "ontology is inconsistent". A negative answer never runs the check | `InstanceFlags::consistency_undetermined`; `SameIndividuals` / `DifferentIndividuals` / `CeInstances` `consistency_undetermined()` | answer unchanged, exit 0, one stderr warning | `consistency_undetermined` (`individuals`, `instances-expr`); `instance` / `instances` have no `--json` | `ConsistencyUndeterminedWarning` | `lazy_query_consistency.rs`, `lazy_query_consistency_cli.rs`, `test_lazy_query_consistency.py` |
| `sat-expr`, `subclass-expr`, `materialize_*` (object/data assertions, sub-property axioms), `property-values`, property disjointness: consistency (#369 follow-up) | not a give-up: a POSITIVE answer ("unsatisfiable", "subsumed", or non-empty) on a KB whose consistency the lazy check left undetermined: the wedge stalled or ran past `RUSTDL_QUERY_LAZY_CONSISTENCY_MS` (default 2000; #419's surfaces keep `RUSTDL_CONSISTENCY_FALLBACK_MS`, 10 000). These hold no prepared ontology, so the check runs the wedge straight from the converted ontology (no saturation or full prepare); a tableau-answered expression and `property-values` reuse the one they built. A witnessed inconsistency is `Err(Inconsistent)`, exit 1. No individual: no wedge seed, not checked and not flagged | `CeVerdict::consistency_undetermined()`; `ObjectPropertyValues` / `DataPropertyValues` `consistency_undetermined()`; `materialize_*_reporting` and `disjoint_*_properties_reporting` return `(rows, consistency_undetermined)` | answer unchanged, exit 0, one stderr warning | `consistency_undetermined` (`sat-expr`, `subclass-expr`, `property-values`, `disjoint` for its property half) | `ConsistencyUndeterminedWarning` | `lazy_query_consistency_followup.rs`, `lazy_query_consistency_cli.rs`, `test_lazy_query_consistency.py` |
| `property-values` (object) | per-pair deadline, node cap; fragment whitelist | `incomplete` | yes | `incomplete` | warns | property-values tests |
| `justify` (incl. `--all`, `--laconic`) | any probe under it: node cap (`RUSTDL_MAX_NODES`), a cut instance probe, the consistency fallback deadline. Top-level probe → no verdict; a QuickXplain subset probe → non-minimal; an enumeration branch probe → a justification may be missing (#396) | `JustifyOutcome { status: Verdict, incomplete }` from the `*_reporting` finders; `minimal_guaranteed` cleared on a QuickXplain give-up | top level: `unknown`, exit 3; otherwise stderr warning | `status: "unknown"` (exit 3), `incomplete`; `enumeration_complete: false` under `--all` | `justify`, `justify_all`, `PreparedOntology.justify*` warn | `justify_give_up.rs` (CLI, reasoner); `test_justify_give_up.py` |
| `repair` | as `justify`, plus each candidate's verification probe | `Repairs::incomplete`; `complete` cleared; a candidate whose verification gave up is dropped and counted in `unverified_gave_up` | top level: `unknown`, exit 3; otherwise stderr warning and a note | no `--json` | `repair` warns | same |
| `diagnose` | classify's unsat probes (node cap: a class reported satisfiable without a verdict); the consistency check on an all-classes-unsatisfiable ontology; the justifications under it as `justify` | `Diagnosis::{incomplete, unsat_probes_cut, consistency_gave_up}` | stderr warning per cause; `consistent (NOT witnessed …)`; `coherent: no unsatisfiable class found (N check(s) gave up)`; per-entailment `unknown` lines | no `--json` | `diagnose`, `debug` warn | same |
| `report` | everything under it (diagnose, per-root justify and repair) | — (CLI only) | HTML banner, stderr warning | — | — | `report.rs` unit test |
| `prove` | the subsumption check (node cap); the justification fallback, as `justify` | `prove_entailment_rcstr_reporting` → `(ProveEntailmentResult, ProveFlags)`; `ProveEntailmentResult::Unknown` | `unknown`, exit 3; fallback give-up: stderr warning | `entailed: false`, `status: "unknown"`, `incomplete`, exit 3 | no binding | `prove_give_up.rs` |

`completeness_guaranteed: true` on the expression queries is only as strong as the
fast-path gate behind it (`is_pure_el`, or for `instances-expr`
`realize_saturation_eligible`). Any recorded gate/engine gap there, such as the
chain-induced poison residual, surfaces as a guaranteed answer, the same exposure
`classify --json` carries; no corpus net covers these surfaces.

An unknown class IRI is not a give-up and gets no verdict on any of these
surfaces: `sat`, `subclass`, `explain`, `instance`, `instances`, `justify` and
`prove` return `ReasonError::UnknownClass` (CLI: `class IRI not in ontology`,
exit 1, which is not the exit 3 of `unknown`). That includes `owl:Thing` and
`owl:Nothing`, which are not in the named-class vocabulary unless the ontology
declares them (a declared one resolves to an ordinary, axiom-free class).
`prove` used to answer "NOT entailed" instead, even for `Aa ⊑ Aa` (#409).

## Not a give-up, but still a gap (filed)

These are not cut probes; they are surfaces that compute less than the
entailments and still report a complete-looking answer. Each needs a decision
about its signal, so each is filed rather than fixed here.

- **realize's pseudo-model prune** drops entailed types without setting
  `incomplete` (`witness_prune_active` reports that the prune ran). It is on for
  nearly every off-fragment ontology with individuals, so a warning on the flag
  alone would fire almost always. #387.
- **disjoint properties, the property hierarchy and data property values** are
  structural. #389 and #390 give them their own signal, `structural_gaps` (CLI
  `--json`, one stderr line in text mode, Python `StructuralGapWarning`), and fix
  the cheap misses (`Symmetric`, disjointness down the hierarchy, `Asymmetric`,
  top/bottom, one-leg chains, anonymous individuals). `incomplete` stays a
  give-up signal. See the
  [completeness ledger](completeness-guarantee-ledger.md#structural-property-surfaces-389-390).
- **`witness_prune_active` in Python**: no Python surface reports it, `realize`
  and the materialize functions alike, until #387 settles the signal (see the
  pseudo-model prune above). The Rust `materialize_existential_successors_reporting`
  carries it.
- **other readers of a trusted consistency `Sat`** do not pass #392's flag on.
  `consistent` itself reports it (see its row). These also trust the wedge's
  `Sat` and do not say so: `classify`'s consistency pre-check (`lib.rs`,
  `classify_inconsistency_precheck`) and its second probe (`classify.rs`,
  `probe_says_inconsistent`), so `classify --json`'s `consistent: true`, and
  `realize` and `diagnose`, which read `Classification::stats().inconsistent`;
  `justify`'s inconsistency oracle (`justify.rs`,
  `Verdict::from_check(!consistent, stats.incomplete)`); and `diagnose`'s
  all-classes-unsatisfiable gate (`diagnose.rs`), where passing the flag on is a
  one-line change. A wrong trusted `Sat` there is a miss, never a
  false positive. The Protégé plugin reads `classify --json`'s `consistent`, not
  `consistent --json`. Not filed.
- **#369's lazy consistency check: what it does not cover.** All the query
  surfaces above run it after a positive answer. Left out or partial:
  - `materialize_existential_successors` runs `realize`, which already errors
    on classify's wedge pre-check, so a check there would be redundant.
  - `realize --properties` prints a `materialize_*` error as a `# … unavailable`
    comment and continues (the realization above it has already printed); its
    `consistency_undetermined` is not reported. Unchanged by design.
  - An ontology with no individual (and no nominal): the wedge needs a seed, so
    nothing is checked or flagged (as classify's pre-check). A TBox inconsistency
    only the tableau finds is missed there, as on main. The gate is also a
    soundness guard: run unseeded, the wedge tests the first class instead
    (`lazy_query_consistency_followup.rs` pins it).
  - With the tail off (`RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL`, default off), an
    inconsistency only the main tableau can find (the wedge stalls) is reported
    as `consistency_undetermined`, not as an error; on the ORE pool the tail ran
    82 times and found none.
  - The wedge's build (one clausify) has no deadline, and its `decide` can run
    past its budget: on `ore_ont_15655` the light check took about 6.6 s (build
    3.4 s, decide 3.2 s against a 2 s budget). No answer was lost to the 30 s
    cap; gating the build by a size predictor is a follow-up (#369).
  - The light route skips #344's data-counting verification, so a wedge `Sat` on
    a data-counting KB is trusted there (as on main), unlike #419's surfaces.
- **single queries that drop the `ABox` (#401)**: `sat`, `subclass`, `explain`,
  `sat-expr` and `subclass-expr` drop the `ABox` seed when the ontology and the
  query have no nominal and the KB is consistent (`RUSTDL_QUERY_TBOX_ONLY`,
  default on). That is exact by disjoint-union closure of nominal-free SROIQ(D)
  (rustdl has no universal role, the one construct that breaks it), and dropping
  axioms can never add an entailment. When consistency rests on a trusted wedge
  `Sat` (#392) the answer inherits that trust, so it has its own signal:
  `QueryStats::relies_on_trusted_consistency`, `relies_on_trusted_consistency` in
  `sat-expr`/`subclass-expr --json`, a stderr note in CLI text mode, and Python
  `TrustedConsistencyWarning`. Not folded into `incomplete`. An undetermined
  consistency keeps the seeded path. `prove` reports the same signal on a "NOT
  entailed" (stderr note, `relies_on_trusted_consistency` in `prove --json`).
  Before any of this the old seeded probe runs under `RUSTDL_QUERY_SEEDED_FIRST_MS`
  (default 3000, a wall-clock budget, with the deadline-free search settings), and
  a verdict it reaches is returned as is. So whether an answer carries the trust
  signal depends on whether that probe finished in time: on a slower or loaded host
  the skip may answer instead, with the same answer and the signal set. A sweep
  diff on that field alone is not a regression. Pinned by `query_tbox_only.rs`,
  `query_tbox_only_cli.rs`, `test_query_tbox_only.py`.

## Rule for changes

A change that adds a way for a query to give up, or a new query surface, must in
the same PR add or update its row here, report the give-up on every surface the
query is exposed on (Rust, CLI, Python), and add a test that forces the give-up.
`RUSTDL_MAX_NODES=1` on a small out-of-fragment ontology that generates a
successor is a deterministic way to force one (see `query_give_up.rs`); run it in
its own process, because the cap is cached once per process.

## Node caps

The CLI's `sat`, `subclass`, `explain`, `instance`, `instances`, `sat-expr`,
`subclass-expr`, `instances-expr` and `prove` (around the whole call, so its
justification fallback too, #402), and the Python `is_class_satisfiable`,
`is_subclass_of`, `is_instance_of`, `instances_of` and the three
`class_expression_*` functions, run their one query under
`owl_dl_reasoner::single_query_node_cap()`, which raises the cap to
`RUSTDL_QUERY_MAX_NODES` (default 500000; `0` disables). The list and expression
forms took it in package 2's prerequisite, so a named query and its expression
or list form cannot disagree on the cap. Everything else uses `RUSTDL_MAX_NODES`
(default 50000): library calls, classify, consistency, realize and
`justify`/`repair`/`diagnose` (many probes per query). An explicit `RUSTDL_MAX_NODES` governs both. Both defaults are pinned by
`owl-dl-tableau/tests/node_cap_defaults.rs`, and the scoping by
`owl-dl-reasoner/tests/single_query_node_cap_scope.rs`. The
single-query default was raised from 50000 when the give-up became visible: at
50000, 3 of 43 measurable random ORE ontologies answered `unknown` on `sat` and
`subclass` that finish with a 200k–500k cap.

## Cross-command laws (#363, package 2)

`scripts/cross-command-laws.py` runs the CLI on real ontologies and checks that
commands answering equivalent questions agree. A disagreement between two definite
answers is a bug in one of them; agreement is not proof of correctness. Each operand
is yes / no / unknown, read from the give-up signals in the table above (exit 3,
`unknown`, a give-up flag, an error other than `Inconsistent`, or a 30 s kill). A law
is checked only when every operand it needs is definite; otherwise it is counted as
unresolved, with the reason.

| law | statement |
|---|---|
| L1, L2 | a classify subsumption or equivalence ⇒ `subclass` = yes |
| L3 | a classify-unsatisfiable class ⇒ `sat` = unsat |
| L4 | `classify --json` `consistent: false` ⇒ `consistent` = inconsistent (smoke) |
| L5 | inconsistent KB ⇒ `instance`, `realize`, `disjoint`, `individuals`, `*-expr` answer yes or error `Inconsistent`, never "no". `sat`/`subclass`/`explain` are excluded: they run no inconsistency guard and do not finish on `family.ofn` |
| L6 | named and expression surfaces agree (smoke: they differ only by the injected `Q ≡ C`) |
| L7 | a `disjoint` pair (A, B) ⇒ `sat-expr "A and B"` = unsat, and `subclass A B` ≠ yes unless A is unsatisfiable |
| L8 | the `classify --saturation-only` closure ⊆ the classify closure |
| L10 | a realize type C of a ⇒ `instance C a` = yes |
| L11 | `individuals` different (a, b) ⇒ `sat-expr "{a} and {b}"` = unsat |

Ratchets, counted against `crates/owl-dl-cli/tests/fixtures/laws/baseline.json`:

- **R1**: `subclass` says yes where the classify closure lacks the pair.
- **R2**: `sat` says unsat where classify lists the class as satisfiable.
- **R3**: `consistent` says inconsistent while classify says `consistent: true`. A
  `consistency_undetermined: true` from classify does not count.
- **U1** (#363 step 1.5): classify calls a class unsatisfiable that the Konclude oracle (or a
  synthetic spec's hand-checked `known_sat` list, a hard violation) has satisfiable. Never
  exempted for a budget cut. Oracle unsat classes classify misses are listed, not ratcheted.

A pair in the committed Konclude closure is a classify miss. A pair absent from it is a
candidate false positive, which fails a run only if HermiT confirms it (`--hermit`). U1 is
stricter: any U1 discrepancy over the baseline fails the run, and a U1 baseline entry needs a
recorded HermiT verdict, never Konclude alone.

**Coverage.** `--self-test` shows each law can fire on fabricated operands; that checks
the harness, not the engine. Engine-level evidence comes from two places.

Env sabotages (`--sabotage`), each FIRES on the unmutated binary and not in its control:

- R1: `RUSTDL_MAX_CHOICE_POINT=0` + `RUSTDL_MAX_TRIAL_MERGE=0` on `maxmerge_fp.ofn` (#76:
  `subclass A I` says yes, HermiT refutes it). Either flag alone does not restore the FP,
  and on the Stanford pizza both together still give 499 = 499, so `per_pair_fp_gate.rs`
  has no live sabotage; this is #76's live guard;
- R2: `RUSTDL_COMPLEX_QUALIFIER_VERIFY=0` on `complex_qualifier_98.ofn`;
- R3: `RUSTDL_CLASSIFY_INCONSISTENCY=0` on `family.ofn`.

Source mutants (package F-lite, `scripts/mutants/`): each patch plants one known fault,
built as its own binary and run through the quick mode with `--hermit`. A law catches a
mutant when the mutant's run has a violation, a HermiT-confirmed false positive, or more
discrepancies for an (ontology, law) than the unmutated control under the same env
(`scripts/mutants/detect.py`). Measured 2026-11-02, base 0090dc9f, seed 363:

| mutant | layer | re-opens | caught by | on the curated corpus? |
|---|---|---|---|---|
| M1 | classify consistency pre-check | all named classes unsat read as inconsistency | L4 (`all_unsat`) | no: fixture only |
| M2 | per-pair tableau, env only | #76 first-pair `≤n` merge (both flags off) | R1, HermiT-confirmed FP (`maxmerge_fp`) | no: fixture only (pizza unaffected) |
| M3 | wedge clausifier | #78 naming clause on `var` | R1 over baseline (`neg_disjunct_forall`); HermiT confirms the miss | no: fixture only |
| M4 | expression surfaces | probe injects only `Q ⊑ CE` | L6 | yes: pizza, ro, sulo, bibtex and 6 fixtures |
| M5 | `disjoint` | told disjointness propagated upward | L7 | yes: sulo, and `disjoint_up` |
| M6 | `realize` | pseudo-model prune inverted | L10 (`realize_disjunction`) | no: fixture only, though ro and pizza have individuals |
| M7 | classify unsat probe | a cut probe published as unsat (env `RUSTDL_UNSAT_PROBE_MS=1 RUSTDL_LABEL_HEURISTIC=0`) | L3; also L4 on ro | yes: pizza, ro, sulo, bibtex |
| M8a | EL saturator (shared) | #114 non-atomic GCI domain head dropped | **nothing** | **not caught** |
| M8b | conversion, D4 (shared) | `Functional(d)` + `≥1 d` read as unsat | nothing in the first run; since step 1.5, U1 (`functional_data_min1`, an oracle check, not a cross-command law) | no: fixture only |
| M9 | classify JSON | a class grouped as equivalent to its parent | L2, L1, L8 | yes: all four |
| M10 | `disjoint` | inconsistency guards removed | L5 (`inconsistent_small`) | no: fixture only |
| M11 | `individuals` | every probed pair published as different | L11 (`individuals_three`) | no: fixture only |
| M12 | `--saturation-only` JSON | one reversed edge (structural) | L8 | yes: all four |

**Not caught by any cross-command law: M8a and M8b, and that is structural.** Both faults sit in code every surface
shares (the EL saturator; conversion), so `classify`, `subclass`, `sat` and the expression
queries all agree on the wrong answer: M8a loses the entailed `X ⊑ Z` everywhere, M8b makes
`C` unsatisfiable everywhere. Cross-command laws compare rustdl with itself, so they cannot see
a fault on a shared path; only an oracle diff can (the FP=0 closure-diff net for M8b's false
unsat, the MISSED net for M8a's miss), and only on an ontology that carries the shape.

### Which other gates see M8a and M8b (#363 step 1.5)

Measured 2026-11-02 on 7c038ba3 (each cell compared with an unmutated control under the same
settings). "Didn't run" in every arm: the 84 `#[ignore]`d tests of the default suite, and the
`alehif-test` and `notgalen` blocks of the FP=0 net (fixtures absent).

| gate | M8a | M8b |
|---|---|---|
| default `cargo test` (core, saturation, reasoner, cli; what CI runs) | **caught**: 3 tests in `gci_domain_nonatomic_head.rs` | not caught: the D4 tests pinned only the `≥2` side |
| FP=0 closure-diff net (`konclude_closure_diff`, local) | not caught | **not caught although it fired**: 41 classes of `ore_ont_10080` unsat (Konclude 0, control 0), net read FP=0 |
| per-pair FP gate | not caught | not caught (no fixture carries the shape) |

**Why the net missed M8b.** `oracle_diff::aligned_closures` excludes
`oracle_unsat ∪ rustdl_unsat` from BOTH sides before diffing pairs, so a class rustdl wrongly
calls unsatisfiable never reached the comparison. A false unsat is the strongest false positive
(the class is subsumed by everything), and neither net could report one.

**Fixed (#363 step 1.5).** `oracle_diff::unsat_diff` compares the unsat SETS; every in-repo
consumer now uses it:

- the FP=0 net (`unsat_FP`/`unsat_MISSED` on each `[fp0]` line; unsat FP is folded into the
  gated FP and counted by `closure-diff.sh`, unsat MISSED is informational);
- the ore-10080 deadline test, both anytime sweeps (unsat FP gated at every deadline), and the
  global-deadline differential (unsat sets must match);
- the per-pair gate;
- `owl-dl-bench matrix` (separate `unsat_fp`/`unsat_missed` columns, since `fp` counts pairs);
- the laws harness: new ratchet **U1** against the Konclude oracle, or a spec's hand-checked
  `known_sat` list on synthetics; never budget-exempted, HermiT-adjudicated like R2.

The oracle reader also counts `SubClassOf(C, owl:Nothing)`, the form OWLAPI-based oracles
(ROBOT/HermiT) write, as unsat; without it a correct unsat class would read as an unsat FP.

After the fix, unmutated main: every fixture present reports `unsat_FP=0` (galen, ore-10080,
ore-10908, ore-15672, sio, wine, bibtex, ro, sulo, the ontostart pizza in both nets, and the
Stanford pizza at 499 = 499); the laws quick set has 5 U1 comparisons, all agreeing. With M8b
applied: the net fails on `unsat_FP=41` at `ore_ont_10080`, the new default-run
`data_axioms::tests::functional_dp_min1_is_not_unsat` fails, and U1 reports 2 violations on
`functional_data_min1`. So **M8b is now caught in CI** (the unit test) and locally (net, U1).

Still open: M8a is caught only by its own canaries; no oracle gate carries its shape (0 of 157
ORE GCI-domain ontologies have a non-atomic head). The inconsistent-oracle branch of
`unsat_diff` is covered by unit tests only: no fixture oracle puts `owl:Thing` in its
`owl:Nothing` group. The harness repo's scorers (`normalise.compare`, `missed-net`,
`cross-check.sh`, `release-corpus-report`) have the same exclusion and are not changed here.

Other limits, stated:

- Six of the eleven caught mutants are caught only on a fixture written for them. The fixtures
  are now in the quick mode, so the unmutated engine is checked against those shapes from now
  on, but the curated corpus alone would have missed M1, M2, M3, M6, M10 and M11.
- M12 is structural: `classify` unions the saturation closure, so no engine fault can break L8;
  M12 only shows the law's plumbing works.
- Mutant choice is a sample of fault classes, not coverage. Four mutants were inert on their
  first fixture or settings and were corrected before the runs (M3: the saturator matched an
  identical `∀` body; M5: the pair is visited in the other order; M7: the label heuristic
  answers before the probe; M10: the `disjoint` command has three guards).

With the fixtures in the quick mode, every hard law now has checks on the unmutated engine
(control run: L4 1, L11 10). In the first full run (30 ontologies) L4 and L11 had **0 checks**:
`classify --json` on `family.ofn` reports `consistency_undetermined: true` on a slow host (so
neither L4 nor R3 applies there), and `individuals` was killed at 30 s on pizza and ro.

The quick mode is a local gate:
`cargo test --release -p owl-dl-cli --test cross_command_laws -- --ignored`.
