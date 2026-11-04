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
| `realize` pseudo-model prune (#387) | not a give-up: the prune answered "not a type" from one witness model (no probe), and that witness has a known reason to lack an entailed type: its search chose a disjunct or merge (`branched`), axioms were deferred or refused, or the #137 re-seed net is off (`RUSTDL_RESEED_VERIFY=0` or `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0`), or `RUSTDL_INVERSE_FUNC_MAX=0`. `false` means no known reason for loss, not a guarantee (the 2026-11-02 bake-off found no loss in either bucket, so it has no positive control). `witness_prune_active` alone says only that the prune ran. Both are `false` by construction on the realize saturation fast path, on `RUSTDL_PSEUDO_MODEL=0`, and on `instance` / `instances`, which build no witness | `Realization::witness_prune_may_drop()`, `witness_prune_reasons()` | stderr warning naming the reasons | `witness_prune_may_drop`, `witness_prune_reasons` | `realize` warns `WitnessPruneWarning` | `witness_prune_reasons.rs` (CLI), `materialize_realize_flags.rs`; `test_materialize_flags.py` |
| `materialize_existential_successors` (#391) | as `realize`; the rows join on ENTAILED types, so `direct_types_unverified` does not apply | `materialize_existential_successors_reporting` → `ExistentialSuccessorFlags::{incomplete, witness_prune_active, witness_prune_may_drop}` | no CLI surface | — | warns `IncompleteQueryWarning` and `WitnessPruneWarning` | `materialize_realize_flags.rs`, `materialize_realize_flags_cap.rs`; `test_materialize_flags.py` |
| Python `materialize_inferred_class_assertions` (#391) | as `realize` (most-specific types) | — (Python only, over `realize`) | no CLI surface | — | warns `IncompleteQueryWarning`, `DirectTypesUnverifiedWarning` and `WitnessPruneWarning` | `test_materialize_flags.py` |
| Python `materialize_inferred_subclass_axioms` (#391) | as `classify` | — (Python only, over `classify`) | no CLI surface | — | the same `IncompleteClassificationWarning`s as `classify` | `test_materialize_flags.py` |
| `sat-expr`, `subclass-expr`, `instances-expr` | as `sat` / `subclass` / `instances` (same single-query node cap) | `CeVerdict` / `CeInstances` `incomplete`; separately `completeness_guaranteed` (pure-EL fast path, or for `instances-expr` the realize saturation fast path, and nothing cut) | `sat-expr`/`subclass-expr`: `unknown`, exit 3; `instances-expr`: stderr warning | `incomplete` (`sat-expr`/`subclass-expr` exit 3), `completeness_guaranteed` | warns | `expr_give_up_cli.rs`; `test_expr_give_up.py` |
| `disjoint` (classes) | per-pair deadline, node cap | `DisjointClasses::incomplete` | yes | `incomplete` | warns | `disjoint_nodecap.rs` |
| `individuals` | per-pair deadline, node cap | `incomplete` | yes | `incomplete` | warns | individuals tests |
| `instance`, `instances`, `instances-expr`, `individuals`: consistency (#369) | not a give-up: a POSITIVE answer (an instance, a merge, a difference) on a KB whose consistency the lazy check left undetermined (the consistency wedge stalled or was cut; the main-tableau tail `RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL` is off by default). An inconsistency the check witnesses is `Err(Inconsistent)`, exit 1, "ontology is inconsistent". A negative answer never runs the check | `InstanceFlags::consistency_undetermined`; `SameIndividuals` / `DifferentIndividuals` / `CeInstances` `consistency_undetermined()` | answer unchanged, exit 0, one stderr warning | `consistency_undetermined` (`individuals`, `instances-expr`); `instance` / `instances` have no `--json` | `ConsistencyUndeterminedWarning` | `lazy_query_consistency.rs`, `lazy_query_consistency_cli.rs`, `test_lazy_query_consistency.py` |
| `sat-expr`, `subclass-expr`, `materialize_*` (object/data assertions, sub-property axioms), `property-values`, property disjointness: consistency (#369 follow-up) | not a give-up: a POSITIVE answer ("unsatisfiable", "subsumed", or non-empty) on a KB whose consistency the lazy check left undetermined: the wedge stalled or ran past `RUSTDL_QUERY_LAZY_CONSISTENCY_MS` (default 2000; #419's surfaces keep `RUSTDL_CONSISTENCY_FALLBACK_MS`, 10 000). These hold no prepared ontology, so the check runs the wedge straight from the converted ontology (no saturation or full prepare); a tableau-answered expression and `property-values` reuse the one they built. A witnessed inconsistency is `Err(Inconsistent)`, exit 1. No individual: no wedge seed, not checked and not flagged | `CeVerdict::consistency_undetermined()`; `ObjectPropertyValues` / `DataPropertyValues` `consistency_undetermined()`; `materialize_*_reporting` and `disjoint_*_properties_reporting` return `(rows, consistency_undetermined)` | answer unchanged, exit 0, one stderr warning | `consistency_undetermined` (`sat-expr`, `subclass-expr`, `property-values`, `disjoint` for its property half) | `ConsistencyUndeterminedWarning` | `lazy_query_consistency_followup.rs`, `lazy_query_consistency_cli.rs`, `test_lazy_query_consistency.py` |
| `sat`, `subclass`, `explain`, `prove`: consistency (#428) | not a give-up. First the #313 guard, shared with `sat-expr` / `subclass-expr`: the `ABox` saturation (bounded at 3 s in classify's high-work class, unbounded in the low-work class; `RUSTDL_QUERY_GUARD_ABOX_MS` overrides, `0` = unbounded; a cut is no verdict), the EL saturator's `⊤ ⊑ ⊥`, `abox_check` and the reflexive empty clause. A proved inconsistency is `Err(Inconsistent)`, exit 1, "ontology is inconsistent" (on main: the vacuous unsat / yes, or no answer on `family.ofn`). Then, after a POSITIVE answer that is not a tautology ("unsatisfiable" other than `owl:Nothing`; "subsumed" other than `C ⊑ C`, `C ⊑ ⊤`, `⊥ ⊑ C`), the #369 lazy check; an undetermined verdict keeps the answer and flags it. `sat owl:Thing` and `owl:Thing ⊑ owl:Nothing` are the consistency check (#427) and run neither. A lent consistent verdict (justify's subsets) skips the guard. `prove` runs the guard before its step proof and the lazy check after one, and reports an inconsistent KB as entailed with a minimal entailing set (typically an inconsistency core); `false` claims nothing. `RUSTDL_CLASS_QUERY_GUARD=0` reverts | `QueryStats::consistency_undetermined`; `ProveFlags::kb_inconsistent` | `sat`/`subclass`/`explain`: answer unchanged, exit 0, one stderr warning; `prove`: a stderr note pointing at `justify <file> inconsistent` | no `--json` (`sat`/`subclass`/`explain`); `prove --json` `kb_inconsistent` | `is_class_satisfiable` / `is_subclass_of` warn `ConsistencyUndeterminedWarning` | `class_query_guard.rs`, `class_query_guard_cli.rs` |
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
exit 1, which is not the exit 3 of `unknown`). `prove` used to answer "NOT
entailed" instead, even for `Aa ⊑ Aa` (#409).

- **`owl:Thing` / `owl:Nothing` are not unknown (#427).** Every surface above
  resolves them to the built-in ⊤ / ⊥ before the vocabulary, declared or not. A
  declaration interns nothing; it used to intern an axiom-free ordinary class,
  which gave wrong answers such as `sat owl:Nothing` → `sat`. Each argument
  resolves before any shortcut, so a typo beside a built-in is still
  `UnknownClass`. The tautologies (`C ⊑ ⊤`, `⊥ ⊑ C`) are answered without
  reasoning and carry no flag. `sat ⊤` and `⊤ ⊑ ⊥` are the consistency check, so
  they carry its `incomplete` (a give-up reported as consistent: CLI exit 3 with
  the consistency-budget wording) and its #392 `relies_on_trusted_consistency`
  (CLI: `consistent`'s note). Instance queries of either built-in keep the
  #313/#419 `Inconsistent` error (they take the ordinary instance path on a
  fresh `Q ≡ ⊤` / `Q ≡ ⊥`). `Classification::is_subclass` answers `⊤ ⊑ C`
  `false` (not materialised) unless the ontology is inconsistent, where
  everything holds. `explain owl:Thing owl:Nothing` uses the consistency wording
  and note too. Pinned by
  `builtin_class_queries.rs` (reasoner, CLI) and `test_builtin_classes.py`.

## Not a give-up, but still a gap (filed)

These are not cut probes; they are surfaces that compute less than the
entailments and still report a complete-looking answer. Each needs a decision
about its signal, so each is filed rather than fixed here.

- **disjoint properties, the property hierarchy and data property values** are
  structural. #389 and #390 give them their own signal, `structural_gaps` (CLI
  `--json`, one stderr line in text mode, Python `StructuralGapWarning`), and fix
  the cheap misses (`Symmetric`, disjointness down the hierarchy, `Asymmetric`,
  top/bottom, one-leg chains, anonymous individuals). `incomplete` stays a
  give-up signal. See the
  [completeness ledger](completeness-guarantee-ledger.md#structural-property-surfaces-389-390).
- **other readers of a trusted consistency `Sat` (#429, fixed).** `consistent`
  reports the fact itself (see its row). Since #429 the other readers report it
  under the same name, `relies_on_trusted_consistency`, as a trust decision, not a
  give-up. It is never set beside an inconsistency and changes no verdict.
  - **classify**: the pre-check's wedge `Sat` (`classify_inconsistency_precheck`)
    or the probe's wedge layer (`probe_says_inconsistent`, not counting) sets it.
    The probe's main-tableau layer finding a model clears it. Reported as
    `ClassificationStats` and `classify --json`, one stderr note, and the Python
    `Classification` getter plus `TrustedConsistencyWarning` from `classify`,
    `classify_bytes` and `materialize_inferred_subclass_axioms`. It is condition 8
    of the completeness guarantee, which cannot fail beside PureEl today.
  - **realize** and the `materialize_*` functions over it: copied from that
    classification (`Realization`, `realize --json`, note, warning).
  - **justify** (`--all`, laconic, prepared) and **repair**: set when an oracle
    call answered "not entailed" on a trusted `Sat`, whether the trust came from
    #392's consistency check or #401's lent consistency. On a not-entailed
    result the verdict rests on it; on a justification only its minimality can.
    For repair, a repair may break the query only if the wedge is right.
    `JustifyOutcome` / `Repairs`, `justify --json`, note, warning. Inside justify
    the instance arm takes no trust (`_no_lazy`, nothing lent for `InstanceOf`).
  - **diagnose**: the classification's flag, unless the all-classes-unsatisfiable
    gate's consistency check ran and did not give up, in which case that check
    decides it. `Diagnosis`, note, warning.

  Still not reported: the instance queries outside justify (`InstanceFlags` has
  no trust field) and prove's justification fallback (`ProveFlags`' flag is
  defined on `NotEntailed` only). Pinned by `trusted_consistency_surfaces.rs`,
  `trusted_consistency_cli.rs` and `test_trusted_consistency_surfaces.py`.
- **#369's lazy consistency check: what it does not cover.** All the query
  surfaces above run it after a positive answer. Left out or partial:
  - `materialize_existential_successors` runs `realize`, which already errors
    on classify's wedge pre-check, so a check there would be redundant.
  - `realize --properties` prints a `materialize_*` error as a `# … unavailable`
    comment and continues (the realization above it has already printed); its
    `consistency_undetermined` is not reported. Unchanged by design.
  - An ontology with no individual in an `ABox` assertion (class, object property,
    negative object property, same or different) and no nominal: the wedge needs a
    seed, so nothing is checked or flagged (the gate classify's pre-check and
    `PreparedOntology` use). Individuals that occur only in data property
    assertions or declarations do not count, so `materialize_data_property_assertions`
    and `data_property_values` on such a KB are unchecked too. A TBox inconsistency
    only the tableau finds is missed there, as on main. The gate is also a
    soundness guard: run unseeded, the wedge tests the first class instead
    (`lazy_query_consistency_followup.rs` pins it).
  - With the tail off (`RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL`, default off), an
    inconsistency only the main tableau can find (the wedge stalls) is reported
    as `consistency_undetermined`, not as an error; on the ORE pool the tail ran
    82 times and found none.
  - The wedge's build (one clausify) has no deadline, and its `decide` can run
    past its budget: on `ore_ont_15655` the light check took about 6.6 s (build
    3.4 s, decide 3.2 s against a 2 s budget). Its memory is about 0.5 GB per
    million lowered axioms, so since #428 the light route builds no wedge above
    `RUSTDL_QUERY_LAZY_LIGHT_MAX_AXIOMS` (default 5,000,000; `0` = no limit) and
    the verdict is undetermined. On the ORE pool that skips `ore_ont_1833` and
    `ore_ont_5368` (14.1 M and 18.7 M axioms), where the build took ~7 GB and
    `subclass-expr` (and #428's `subclass`) aborted under an 8 GB limit; nothing
    lies between 4.77 M and 14.1 M. The prepared route (a tableau-answered query)
    still builds the cache inside `PreparedOntology`, as on main.
  - **Not guarded** (#428): `is_class_satisfiable_with_timeout` (`owl-dl-bench
    --timeout`) and `subclass --saturation-only` / `is_subclass_of_saturation_only`.
    L5 does not sample them. The guard's `abox_check` clones its inputs: ~2.3 GB
    extra on `ore_ont_1833`, within an 8 GB limit (as on `sat-expr` before).
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
| L5 | inconsistent KB ⇒ `instance`, `realize`, `disjoint`, `individuals`, `*-expr`, and since #428 `sat` / `subclass`, answer yes or error `Inconsistent`, never "no" (`sat`: never "satisfiable"). `sat owl:Thing` is the consistency check (#427) and is not sampled |
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
`owl:Nothing` group. The harness repo's scorers had the same exclusion. `normalise.compare`,
`missed-net` and `cross-check.sh` were fixed in owl-reasoner-harness#2 (merged as 8060fc94),
with no movement in the recorded MISSED baselines. `release-corpus-report` compares arms with
each other, not with an oracle, and is unchanged.

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

## Generated-ontology oracle net (#363 step 3)

`scripts/gen/` (see its README) generates small, legal OWL 2 DL ontologies:
- 4–10 classes, 2–4 roles, 0–2 data properties, 0–3 individuals and 3–15 axioms;
- drawn from 11 construct families;
- regularity and simplicity by construction.

It runs `classify`, `classify --saturation-only` and `consistent` on each case, and scores
the answers against what Konclude and HermiT **both** agree on. A case where the peers
disagree or fail is counted and never scored. FP kinds are never exempted for a budget cut.
A miss is a finding only where rustdl claims completeness (`completeness_guaranteed`, or the
`consistent --json` gates); otherwise it is "exposed", a limitation rustdl declares.

Measured 2026-11-03 on 7b5c0f9f (0.4.42): seed 20261103, default profile, cores 92–94,
peers capped at 60 s, rustdl at 30 s. Seeds and generator hashes: `scripts/gen/seeds.txt`.

**Control, N = 2000: no findings, so 0 false alarms.**

| bucket | cases |
|---|---:|
| clean | 1945 |
| peer-disagree | 32 |
| peer-fail | 12 (Konclude: 10 timeouts, 2 failures; HermiT ok on all) |
| exposed | 10: 17 misses (13 pairs, 4 unsat classes), each under a declared limitation (trusted Sat refutations on 7 cases, a deadline on 3, case 1646 has both, `completeness_guaranteed: false` alone on 1) |
| rustdl-fail | 1 (case 1283, below) |
| invalid | 0 (HermiT accepted every file as legal) |
| finding | **0** |

Two more control cases have a classify pre-check consistency miss (`consistent: true` on a KB
both peers call inconsistent): case 475, bucketed clean because that tier is informational, and
case 1283. The two targeted controls below add 1000 cases, also with 0 findings.

**What the scored cases exercised.**
- **The guaranteed tier is thin.** Of the 1794 agreed-consistent cases, 79 (4.4%) report
  `completeness_guaranteed: true`. So a classify miss could be a finding on only those.
  **"0 findings" here is not evidence of completeness.** The `el_guaranteed` profile below
  (step 3a) puts 99.8% of cases in that tier and finds guaranteed-tier misses on unmodified
  main in 101 of 1000 cases.
- **The oracle branches all got real cases:** 162 agreed-inconsistent, 283 with a
  Thing-equivalent class, 537 with an unsat class.
- **Noise floor.** A control-vs-control rerun of the first 500 cases differs on 0.
- **Load-driven deadlines.** Under load, a per-pair deadline can fire in one run and not the
  next. `detect` therefore reports exposed-tier differences whose arm says `incomplete: true` as
  "budget-cut", and never counts them (see M8a below).

**Peer disagreements (32, not scored).**
- 31 are HermiT reporting more than Konclude:
  - 21 Konclude-consistent and HermiT-inconsistent;
  - 5 unsat classes;
  - 3 Thing-equivalent classes;
  - 2 pairs.
- 1 is Konclude reporting a Thing-equivalent class HermiT lacks.
- rustdl sides with HermiT in 30 of 32.

The 2 where it sides with Konclude are unadjudicated:
- **case 723**: HermiT has `C4 ⊑ C7`;
- **case 1725**: HermiT has `C1` unsatisfiable.

JFact (via robot) sides with Konclude and rustdl on both, so HermiT's claims stand 1 against 3.
rustdl reports `completeness_guaranteed: false` on both, so they would be exposed-tier either way.

**A known oracle defect, and the blind spot it makes.** The 21 Konclude-consistent /
HermiT-inconsistent cases all declare a named individual. There are two shapes:
- **5 are `ReflexiveObjectProperty(r)` + `AsymmetricObjectProperty(r)`** (fixture
  `scripts/gen/fixtures/konclude-reflexive-asymmetric.ofn`). Konclude answers that pair
  correctly with no individual, and says consistent once one is declared, even with no
  assertion about it.
- **16 are ABox role assertions** (`r(a,a)` or `r(a,b)`) that break `Asymmetric`,
  `Irreflexive` or `DisjointObjectProperties`, some through the role hierarchy, plus one
  `EquivalentClasses(C ObjectComplementOf(C))`.

HermiT, JFact (checked on the fixture) and rustdl say inconsistent. The oracle rule stays
"both peers must agree", so **these inconsistencies are never scored**: a rustdl miss of this
kind would be invisible to the net.

**rustdl-fail, case 1283 (an issue draft, not filed).** `consistent` and `sat` fail with
`ReasonError::NoVerdict` ("bug indicator") instead of answering or giving up with exit 3.
- Shrunk under peer agreement to 5 axioms (consistent KB) and 6 axioms (inconsistent KB).
- It fails in about 5 s; the 13-axiom original takes 18–24 s.
- classify answers the 5-axiom KB correctly.

**Sensitivity.** Each mutant runs on the first B = 500 cases. A detection is a new result in
the mutant's run that the control run lacks on the same case under the same env. It counts if
it is finding-tier, or exposed-tier with no deadline in the mutant's arm.

| mutant | detected at B | cases | first case | kind (tier) | note |
|---|---|---:|---|---|---|
| M1 | **yes** | 1 | 160 | consistency-FP on classify's `consistent` (finding) | |
| M3 | **yes** | 2 | 198 | pair-MISSED (exposed, no deadline) | 5/5 runs per arm identical |
| M7 | **yes** | 209 | 0 | unsat-FP (366), consistency-FP (7) | under its env; the env's own control has 0 findings and 41 exposed. Pre-declared as a likely non-detection; it was not |
| M8a | no | 0 | – | – | pre-declared. At N = 2000: 0 (3 budget-cut, 1 pre-check difference). Targeted `el_only`, 500: 0 (1 budget-cut). Every candidate gave identical answers in both arms on rerun |
| M8b | no | 0 | – | – | **not** pre-declared. At N = 2000: 1 (case 1560, unsat-FP, `C ⊑ ¬A ⊓ ≥1 d` + `Functional(d)`). Targeted `data_heavy`, 500: 2 (cases 202, 361, unsat-FP) |
| M9 | **yes** | 340 | 1 | pair-FP (also satonly-FP) | |
| M12 | **yes** | 304 | 1 | satonly-FP | structural, as in the laws harness |

So 5 of 7 are detected within B. M8b is detected at N and under its targeted profile. **M8a
is not detected anywhere.**

M8a and M8b live on paths every surface shares (the EL saturator, D4 conversion). The
cross-command laws cannot see them; only an oracle can, and only on an input with the shape.
The generator rarely draws those shapes:
- M8b needs `Functional(d)` with `≥1 d` reachable by D4: 1 of 2000 default cases, 2 of 500
  `data_heavy` cases.
- M8a needs a pure-EL KB with a non-atomic GCI domain head and an entailment that depends on
  it: 2 of 500 `el_only` cases have the head, and the mutant changes no answer on either.

Each detection is shrunk under peer agreement, at the tier it was found, to a fixture in
`scripts/gen/fixtures/mutants/`. The unmutated build shows no such finding on any of them.
- M1: 4 axioms (every named class unsatisfiable on a consistent KB);
- M3: 6 axioms, carrying `∀r.(B ⊔ ¬C)`;
- M7: 1 axiom (timing-dependent: it fired in 20 of 20 runs on one core, and in 9 of 12);
- M8b: 2 axioms;
- M9 and M12: 1 axiom each.

Out of scope, because the net does not query those surfaces: M4, M5, M6, M10, M11. M2 is an
env sabotage of the per-pair path.

**Limits.**
- The oracle is agreement-only, so it is blind where the peers disagree: the 21 ABox and
  role-characteristic inconsistencies above, and the unadjudicated cases.
- The guaranteed tier covers 4% of scored cases, so this net mostly tests soundness, not
  completeness.
- One default profile and seed: the family weights decide what is reachable (M8a, M8b).
- Exposed-tier results are budget-sensitive under load.
- The net does not run in CI: a 2000-case control took 89 minutes on 3 cores, against the
  plan's estimate of about 25.

### A guaranteed-tier profile: `el_guaranteed` (#363 step 3a)

The default profile rarely stays inside rustdl's pure-EL gate, so `completeness_guaranteed`
held on 4% of its cases. The `el_guaranteed` profile is a separate generator (`ElCase`) that
emits only what `is_pure_el` admits:
- EL concepts (atomic, ⊓, forward ∃, ⊤, ⊥);
- atomic `DisjointClasses`;
- forward sub-roles, length-2 chains, transitivity and role equivalence;
- atomic domain and range;
- non-atomic domain heads only as the GCI `∃r.⊤ ⊑ X`.

Half the cases carry a planted #114- or #81-shaped motif, recorded as `# motif:`.

The default, `el_only` and `data_heavy` outputs are unchanged: all 3000 earlier cases
regenerate byte-identically, checked in full.

Measured 2026-11-03 on 21b1986e: seed 20261106, N = 1000.

| | result |
|---|---|
| `completeness_guaranteed` | **998 of 1000** scored cases (99.8%); 100% on motif-free cases. Both exceptions (cases 64, 731) are the pre-declared `has_composed_chain_range` (#108) exit |
| buckets | 899 clean, **101 finding**; no peer disagreement, peer failure, rustdl failure or invalid case |
| findings | all guaranteed-tier misses (146 pairs and 50 unsat classes over the 101 cases); **0 FP** |
| control-vs-control | 0 of 500 differ |
| M8a (pre-declared: detected within B = 500) | **held**: 103 of 500, first at case 2, all finding tier; 211 of 1000. Fixture `fixtures/mutants/M8a.ofn`: motif 1's three axioms |
| guaranteed rate (pre-declared ≥ 50%; expected ≥ 80%) | **held**: 99.8% |

**The 101 findings on unmodified main have two mechanisms, both about `∃r.⊤`.** Every case is
attributed by an entailment-preserving rewrite that makes the miss disappear. On every rewritten
file the miss disappears with `completeness_guaranteed` still true, so the route does not change.
The attribution is **sequential**: the range rewrite is tried first. The ⊤-filler rewrite repairs
the range gap at every depth as well, so some of the 89 also contain a nested instance of
mechanism 1.

1. **12 cases: the ⊤-witness for `X ⊑ ∃r.⊤` never receives `Range(r)`.** The minimal case is
   `X ⊑ ∃r.⊤`, `Range(r,F)`, `∃r.F ⊑ W`, which misses `X ⊑ W`. CLAUDE.md records the
   subsumer-less witness as a design decision (`topwitness.ofn`). **Closed by #435:** the
   witness now stands for the set of `r`-successors and carries the ranges of `r` and every
   `⊤ ⊑ C`.
2. **89 cases: `∃s.⊤` below the top level has no lowering.** `atomic_or_tseitin_body_with_extras`
   has no `Top` arm, so a nested `∃r.∃s.⊤`, or a domain-GCI head containing `∃s.⊤`, returns
   `None` and the axiom is dropped. The minimal case is `X ⊑ ∃r.∃s.⊤`, `∃r.∃s.⊤ ⊑ W`, which
   misses `X ⊑ W`.
   **Closed by #436:** a nested ⊤ filler lowers by position. In a fact or head it is the
   ⊤-witness with its range folds; in a trigger it is a ⊤-marker that any `s`-successor fires.
   The `el_guaranteed` rerun on the fix gives 1000 of 1000 cases clean, against 899 before.

They share a root: a ⊤ filler has no typed representation, an untyped witness at the top level
and nothing below it. One fix can close both, provided the range part lands first. Both are
confirmed by Konclude and HermiT (`work-363-gen/el/repro/` holds the reproducers and their
oracle records), and missed by `classify` and `subclass` alike. In
both, the gate certifies completeness and the engine drops the inference: the D10 shape. The
curated corpus does not carry either; the FP=0 net reports MISSED=0 on every fixture. Issue
drafts went to the manager; no fix is in this change.


## Route-pair laws (#363 package E, stateful half)

`scripts/gen/routes.py` asks the same question by different routes and checks that the
semantic answer (`consistent`, the unsat set, the closure) does not depend on the route. It runs
over the 4000 generated cases (the default control, `el_only`, `data_heavy`, `el_guaranteed`),
whose cached Konclude ∩ HermiT oracle breaks ties. A law compares two runs only when neither
gave up; R4 is the exception and skips only if the full run gave up. A run gave up if any of
these holds: `incomplete`, `consistency_undetermined`, `derived_same_undecided`, prep,
conversion or parse timed out, or the run was killed at the 30 s cap. Plan:
`notes/plans/2026-11-03-363-package-e-stateful.md` (v2, Fable-reviewed).

Measured 2026-11-03 on a5a2e063, cores 92–94. R1–R3 use `--pair-timeout-ms 1000` on both
sides.

| law | route pair | compared | held | budget-cut | violations |
|---|---|---:|---:|---:|---:|
| R1 | `RAYON_NUM_THREADS` 1 vs 8 (one core) | 3984 | 3984 | 16 | 0 |
| R2 (a) | default vs `HYPERTABLEAU_TRUST_SAT=0` | 3959 | 3950 | 41 | 0 |
| R2 (b) | default vs `HYPERTABLEAU=0` | 3926 | 3898 | 74 | 0 |
| R3.1 | cold CLI vs warm Python (one process, 4000 ontologies) | 3984 | 3984 | 16 | 0 |
| R3.2 | warm in order vs warm shuffled | 3985 | 3985 | 15 | 0 |
| R3.3 | `is_subclass` vs `direct_subsumers` + `equivalent_classes` | 4000 (also on the 15 given-up runs) | 3999 | – | **1, reproduced (below)** |
| R4 | `--global-timeout-ms` 1, 5, 25 and `--pair-timeout-ms 1` vs full | 3984 each | 3984 each | 16 | 0 |
| R5.1 | a must-leave-PureEl token, yet `# fragment: pure-EL` | 2721 | 2721 | – | 0 |
| R5.2 | an `el_guaranteed` case reports PureEl | 1000 | 998 | – | 2: the #108 composed-chain-range exit, as pre-declared |
| R5.3 | text output vs JSON output | 3984 | 3984 | 16 | 0 |

**R2 is not "default ⊆ reference".** R2.3, the FP check, runs on every completed run, given-up
runs included, because a cut that publishes a wrong answer is exactly what makes a run
`incomplete`. It finds 0 FPs on main. All 37 differences in R2 were adjudicated against the
oracle, and none is an FP on either route:
- 14: the reference recovers an oracle-confirmed entailment the default misses. The default
  reports `completeness_guaranteed: false` on all of them, so these are its declared exposure.
- 3: the no-wedge route misses what the default finds.
- 6: no oracle.
- 14: the difference is only on classes the oracle excludes (unsat or Thing-equivalent).

**R4 found a non-monotonic budget, and it is explained.** On `data_heavy` case 422, a
`--pair-timeout-ms 1` run derives `C4 ⊑ C6` and `C2 ⊑ C6`, which the 1000 ms run misses.
- The oracle confirms both pairs.
- The 1000 ms run reports 34 trusted Sat refutations, and `TRUST_SAT=0` recovers the pairs.
- So the longer budget lets the wedge reach a `Sat` that is trusted and wrong, while the 1 ms
  cut falls through to the tableau.

This is the documented `trust_sat` exposure, not a finding.

**R3.3: a reproduced contradiction on an incomplete classification (issue draft sent to the
manager).** On default case 812, at `per_pair_timeout_ms=1000` and `global_timeout_ms=25000`,
one Python `Classification` object contradicts itself:
- `direct_subsumers` gives `C1 ⊑ C7` and `C7 ⊑ C0`, yet `is_subclass(C1, C0)` is false;
  `C4` behaves the same.
- The oracle confirms `C1 ⊑ C0`, so the answer is still sound: it misses a subsumption its own
  direct relation implies.
- It reproduced in both warm passes, then 5 of 5 times in isolation at those settings (11 cut
  pairs each time).
- My first attempts at 1–100 ms per pair, or with global budgets of 20 ms to 3 s, did not
  reproduce it. They never used the observed settings, so they are not evidence against it.

The mechanism is in the code. `direct_subsumers` is the Hasse reduction of the subsumption
matrix, and `is_subclass` reads the matrix itself. A cut that records `C1 ⊑ C0` as not subsumed,
while both legs were decided, leaves the matrix non-transitive. The CLI's
`direct_subsumptions` is the same reduction, so a consumer closing it gets the (true) pair that
`is_subclass` denies.

**Mutants.** Two new mutants are in `scripts/mutants/`. Both are detected, as pre-declared. On
500 cases (125 per set):
- **ME1** (a cut per-pair probe published as subsumed) was caught in **8 cases**:
  - by R4.1, as an FP under a cut that the oracle refutes (4 cases at `--pair-timeout-ms 1`,
    1 at `--global-timeout-ms 25`);
  - by R2.3, as an oracle-refuted FP on the reference routes (3 on refA, 26 findings on refB).
- **ME2** (a process-global saturation memo keyed by class count) was caught by the warm route
  in **97 cases**: R3.2 on all 97, R3.1 on 48, R3.3 on 7.
  - The memo is written only by `saturate()`. Warm detection relies on a call that takes that
    path: prep, or the interleaved `is_consistent`.
  - Only the cold `full` CLI route was run on ME2. Its answers are identical to main's on all
    500, as expected for one ontology per process. R1, R2, R4 and R5 were not run on ME2.
  - ME2 is the third version. Two earlier versions were also measured on these 500 cases and
    detected by nothing, so they were replaced:
    - the first cached the told tables, which do not decide the hierarchy;
    - the second memoised only `saturate()`, which a classify with a global budget bypasses.

**Limits.**
- R1 compares 1 and 8 threads on one core. It tests interleavings, not real parallel speed, and
  no thread mutant was built: a planted race cannot be made to fire on purpose.
- The warm route covers what the Python API exposes: `classify`, `Classification` queries and
  an interleaved `is_consistent`. `is_subclass_of` was dropped from the interleave because it
  runs no inconsistency guard and does not finish on an inconsistent KB (default case 438).
  That is #428; once it lands, the interleave can include `is_subclass_of` again.
  Classify in the warm route uses a 25 s global budget, standing in for the CLI's 30 s kill.
- R5.1's predictor is token-based. Degenerate `ObjectUnionOf(:C :C)` is excluded: it is the only
  predictor token in 4 control cases, 3 of which report pure-EL. `ObjectComplementOf` (neg-to-⊥)
  is not a predictor. The gate correctly admits both.
- R4.2 accepts a cut flagged only by `consistency_undetermined`. In three `--global-timeout-ms 1`
  runs (default 242 and 1766, `data_heavy` 354), an inconsistent KB comes back
  `consistent: true`, `incomplete: false`, with a hierarchy, and that flag is set. This is
  permitted by the plan: the flag is the give-up signal for that case.
- The warm give-up set lacks `parse_timed_out`, because the Python `Classification` does not
  expose it.
