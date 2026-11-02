# Query-surface ledger: unknown versus negative

Work package A2 of the correctness assurance programme (#363). The
[completeness guarantee ledger](completeness-guarantee-ledger.md) covers
`classify`. This one covers every other query surface and asks one question:
**when a probe reaches no verdict, does the caller find out?**

Checked against `main` at `0bbe276c` (31 October 2026); the `consistent` trust row
(#392) against `83853936` (2 November 2026).

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

A pair in the committed Konclude closure is a classify miss. A pair absent from it is a
candidate false positive, which fails a run only if HermiT confirms it (`--hermit`).

**Coverage.** No hard law has a live engine-level sabotage on this repository's
curated fixtures. `--self-test` shows that each law can fire, on fabricated operands;
that checks the harness, not the engine. Engine-level sabotages exist only for the
ratchets (`--sabotage`):

- R2: `RUSTDL_COMPLEX_QUALIFIER_VERIFY=0` on `tests/fixtures/laws/complex_qualifier_98.ofn`;
- R3: `RUSTDL_CLASSIFY_INCONSISTENCY=0` on `family.ofn`.

Two laws had **0 checks** in the first full run (30 ontologies, `--pair-timeout-ms 1000`):

- L4: `classify --json` on `family.ofn` reports `consistent: true` with
  `consistency_undetermined: true` on this host (its ABox pre-check runs out of budget
  on a slow machine), so neither L4 nor R3 applies. That is the signal working as
  intended, and it depends on the host.
- L11: `individuals` was killed at 30 s wherever it ran (pizza, ro).

The quick mode is a local gate:
`cargo test --release -p owl-dl-cli --test cross_command_laws -- --ignored`.
