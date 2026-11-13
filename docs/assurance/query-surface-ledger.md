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
| `consistent` | fall-through tableau deadline (`RUSTDL_CONSISTENCY_FALLBACK_MS`; wedge route only: with no individual or `RUSTDL_WEDGE_CONSISTENCY=0` the main tableau has no deadline, #497), node cap, abandon (#182); #344 data-counting verification gives up; the tableau's internal limit, `NoVerdict` (#449: stderr names it a likely bug) | `is_consistent_with_stats` → `incomplete` | `unknown`, exit 3 | `incomplete`, exit 3 | `is_consistent` warns | `consistency_data_counting.rs`; CLI `consistent` tests |
| `consistent` trusted wedge `Sat` (#392) | not a give-up: `consistent` rests only on the consistency wedge's `Sat`, which no main-tableau run verified (the wedge missed clashes in #290 and #376). Never set on `inconsistent`, on a main-tableau model, or on a #344-verified `Sat`. Broader than `wedge_clauses_refused` | `QueryStats::relies_on_trusted_consistency` (the #401 flag, same meaning) | stdout `consistent`, exit 0, one stderr note | `relies_on_trusted_consistency`, exit 0; the same stderr note | `is_consistent` warns `TrustedConsistencyWarning` | `consistent_trusted_sat.rs` (reasoner, CLI); `consistency_data_counting.rs`; `test_consistent_trusted_sat.py` |
| `sat` | node cap (`RUSTDL_QUERY_MAX_NODES` on the CLI/Python entry point; early abandon is armed only under a deadline). A `NoVerdict` (the tableau's internal limit: saturation out of iterations, or a depth bail-out with time left) is `unknown`, exit 3, with a "likely a reasoner bug" stderr line (#449; it was exit 1) | `is_class_satisfiable_with_stats` → `incomplete` | `unknown`, exit 3 | no `--json` | `is_class_satisfiable` warns | `query_give_up.rs`; `test_query_give_up.py` |
| `subclass` | node cap (`RUSTDL_QUERY_MAX_NODES`); `NoVerdict`, as for `sat` (#449). Not a give-up: the H4 hypertableau wedge tried first runs under `RUSTDL_QUERY_WEDGE_MS` (default 3000, `0` = unbounded; #457) and a stall falls through to the main tableau, under the node cap above | `is_subclass_of_with_stats` → `incomplete` | `unknown`, exit 3 | no `--json` | `is_subclass_of` warns | same |
| `explain` | node cap; `NoVerdict` (#449); the wedge budget as for `subclass` (#457) | `is_subclass_of_with_stats` | `: unknown — tableau gave up` (`: unknown — the tableau reached an internal limit` for `NoVerdict`), exit 3 | — | — | `query_give_up.rs` |
| `instance` | per-probe deadline (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, default 750 ms), node cap, depth bail-out | `is_instance_of_reporting`; `InstanceFlags::depth_cap_hit` (#510 C) marks a deadline or node-cap cut that had hit the decision-depth limit first (report-only) | `unknown`, exit 3; after a depth-limit hit the warning adds that `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` may help as well as a larger budget (#510 C) | no `--json` | `is_instance_of` warns, with the same clause | `query_give_up.rs`; `test_query_give_up.py`; `depth_cap_hit_flags.rs`, `instance_depth_cut_cli.rs`, `test_depth_cut.py` (#510 C) |
| `instances` | same | `instances_of_reporting`; `InstanceFlags::depth_cap_hit` if any probe was so cut (#510 C) | stderr warning, with the same clause (#510 C) | no `--json` | `instances_of` warns (no #510 C clause yet) | same |
| `realize` | same, plus the node cap with the per-probe deadline disabled (`=0`) | `Realization::incomplete()` | stderr warning | `incomplete` | `realize` warns | same |
| `realize` direct types (#388) | the check of two most-specific types the hierarchy left undecided: per-probe deadline, node cap, depth bail-out (or the check is off, `RUSTDL_REALIZE_VERIFY_DIRECT=0`). Not a missing type: a listed type may be a superclass of another. Never set on the saturation fast path | `Realization::direct_types_unverified()` | stderr warning | `direct_types_unverified` | `realize` warns `DirectTypesUnverifiedWarning` | `realize_direct_types.rs`, `realize_direct_types_cap.rs`; `test_direct_types_unverified_warning` |
| `realize` pseudo-model prune (#387) | not a give-up: the prune answered "not a type" from one witness model (no probe), and that witness has a known reason to lack an entailed type: its search chose a disjunct or merge (`branched`), axioms were deferred or refused, or the #137 re-seed net is off (`RUSTDL_RESEED_VERIFY=0` or `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0`), or `RUSTDL_INVERSE_FUNC_MAX=0`. `false` means no known reason for loss, not a guarantee (the 2026-11-02 bake-off found no loss in either bucket, so it has no positive control). `witness_prune_active` alone says only that the prune ran. Both are `false` by construction on the realize saturation fast path, on `RUSTDL_PSEUDO_MODEL=0`, and on `instance` / `instances`, which build no witness | `Realization::witness_prune_may_drop()`, `witness_prune_reasons()` | stderr warning naming the reasons | `witness_prune_may_drop`, `witness_prune_reasons` | `realize` warns `WitnessPruneWarning` | `witness_prune_reasons.rs` (CLI), `materialize_realize_flags.rs`; `test_materialize_flags.py` |
| `materialize_existential_successors` (#391) | as `realize`; the rows join on ENTAILED types, so `direct_types_unverified` does not apply | `materialize_existential_successors_reporting` → `ExistentialSuccessorFlags::{incomplete, witness_prune_active, witness_prune_may_drop}` | no CLI surface | — | warns `IncompleteQueryWarning` and `WitnessPruneWarning` | `materialize_realize_flags.rs`, `materialize_realize_flags_cap.rs`; `test_materialize_flags.py` |
| Python `materialize_inferred_class_assertions` (#391) | as `realize` (most-specific types) | — (Python only, over `realize`) | no CLI surface | — | warns `IncompleteQueryWarning`, `DirectTypesUnverifiedWarning` and `WitnessPruneWarning` | `test_materialize_flags.py` |
| Python `materialize_inferred_subclass_axioms` (#391) | as `classify` | — (Python only, over `classify`) | no CLI surface | — | the same `IncompleteClassificationWarning`s as `classify` | `test_materialize_flags.py` |
| `sat-expr`, `subclass-expr`, `instances-expr` | as `sat` / `subclass` / `instances` (same single-query node cap; `subclass-expr` also takes `subclass`'s wedge budget, #457) | `CeVerdict` / `CeInstances` `incomplete`; separately `completeness_guaranteed` (pure-EL fast path, or for `instances-expr` the realize saturation fast path, and nothing cut) | `sat-expr`/`subclass-expr`: `unknown`, exit 3; `instances-expr`: stderr warning | `incomplete` (`sat-expr`/`subclass-expr` exit 3), `completeness_guaranteed` | warns | `expr_give_up_cli.rs`; `test_expr_give_up.py` |
| `disjoint` (classes) | per-pair deadline, node cap | `DisjointClasses::incomplete` | yes | `incomplete` | warns | `disjoint_nodecap.rs` |
| `individuals` | per-pair deadline, node cap | `incomplete` | yes | `incomplete` | warns | individuals tests |
| `instance`, `instances`, `instances-expr`, `individuals`: consistency (#369) | not a give-up: a POSITIVE answer (an instance, a merge, a difference) on a KB whose consistency the lazy check left undetermined (the consistency wedge stalled or was cut; the main-tableau tail `RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL` is off by default). An inconsistency the check witnesses is `Err(Inconsistent)`, exit 1, "ontology is inconsistent". A negative answer never runs the check | `InstanceFlags::consistency_undetermined`; `SameIndividuals` / `DifferentIndividuals` / `CeInstances` `consistency_undetermined()` | answer unchanged, exit 0, one stderr warning | `consistency_undetermined` (`individuals`, `instances-expr`); `instance` / `instances` have no `--json` | `ConsistencyUndeterminedWarning` | `lazy_query_consistency.rs`, `lazy_query_consistency_cli.rs`, `test_lazy_query_consistency.py` |
| `sat-expr`, `subclass-expr`, `materialize_*` (object/data assertions, sub-property axioms), `property-values`, property disjointness: consistency (#369 follow-up) | not a give-up: a POSITIVE answer ("unsatisfiable", "subsumed", or non-empty) on a KB whose consistency the lazy check left undetermined: the wedge stalled or ran past `RUSTDL_QUERY_LAZY_CONSISTENCY_MS` (default 2000; #419's surfaces keep `RUSTDL_CONSISTENCY_FALLBACK_MS`, 10 000). These hold no prepared ontology, so the check runs the wedge straight from the converted ontology (no saturation or full prepare); a tableau-answered expression and `property-values` reuse the one they built. A witnessed inconsistency is `Err(Inconsistent)`, exit 1. No individual: no wedge seed. Since #457 (`RUSTDL_QUERY_NO_SEED_TAIL`, default on) a tableau-answered expression runs the bounded main-tableau tail (`RUSTDL_CONSISTENCY_FALLBACK_MS`, once per prepared ontology): `Err(Inconsistent)` on a proof, flagged when cut. The light route is not checked and not flagged (`docs/known-limitations/tbox-only-inconsistency-light-route.md`) | `CeVerdict::consistency_undetermined()`; `ObjectPropertyValues` / `DataPropertyValues` `consistency_undetermined()`; `materialize_*_reporting` and `disjoint_*_properties_reporting` return `(rows, consistency_undetermined)` | answer unchanged, exit 0, one stderr warning | `consistency_undetermined` (`sat-expr`, `subclass-expr`, `property-values`, `disjoint` for its property half) | `ConsistencyUndeterminedWarning` | `lazy_query_consistency_followup.rs`, `lazy_query_consistency_cli.rs`, `test_lazy_query_consistency.py` |
| `sat`, `subclass`, `explain`, `prove`: consistency (#428) | not a give-up. First the #313 guard, shared with `sat-expr` / `subclass-expr`: the `ABox` saturation (bounded at 3 s in classify's high-work class, unbounded in the low-work class; `RUSTDL_QUERY_GUARD_ABOX_MS` overrides, `0` = unbounded; a cut is no verdict), the EL saturator's `⊤ ⊑ ⊥`, `abox_check` and the reflexive empty clause. A proved inconsistency is `Err(Inconsistent)`, exit 1, "ontology is inconsistent" (on main: the vacuous unsat / yes, or no answer on `family.ofn`). Then, after a POSITIVE answer that is not a tautology ("unsatisfiable" other than `owl:Nothing`; "subsumed" other than `C ⊑ C`, `C ⊑ ⊤`, `⊥ ⊑ C`), the #369 lazy check; an undetermined verdict keeps the answer and flags it. With no individual (#457 residual 2), an answer the main tableau found (the prepared route) runs the bounded main-tableau tail and errors or flags as above (`prove`: `kb_inconsistent`); a told, saturation or H4-query-wedge answer (the light route) is not checked, a documented gap (`docs/known-limitations/tbox-only-inconsistency-light-route.md`). `sat owl:Thing` and `owl:Thing ⊑ owl:Nothing` are the consistency check (#427) and run neither. A lent consistent verdict (justify's subsets) skips the guard. `prove` runs the guard before its step proof and the lazy check after one, and reports an inconsistent KB as entailed, with the inconsistency core as the justification (#456: the minimal inconsistent subset `justify <file> inconsistent` returns, not a minimal set entailing the subsumption); `false` claims nothing. `RUSTDL_CLASS_QUERY_GUARD=0` reverts | `QueryStats::consistency_undetermined`; `ProveFlags::kb_inconsistent` | `sat`/`subclass`/`explain`: answer unchanged, exit 0, one stderr warning; `prove`: a stderr note pointing at `justify <file> inconsistent` | no `--json` (`sat`/`subclass`/`explain`); `prove --json` `kb_inconsistent` | `is_class_satisfiable` / `is_subclass_of` warn `ConsistencyUndeterminedWarning` | `class_query_guard.rs`, `class_query_guard_cli.rs`, `no_seed_tail_457.rs` |
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
  reasoning and carry no flag. On `sat`/`subclass`/`explain` the #428 guard runs
  first, so a proved inconsistency still errors, as with `subclass-expr`; `prove`
  answers them before its guard (the proof uses no axiom). `sat ⊤` and `⊤ ⊑ ⊥` are the consistency check, so
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
    seed (the gate classify's pre-check and `PreparedOntology` use). On the light
    route nothing is checked or flagged. On the prepared route, since #457, the
    bounded main-tableau tail runs (`RUSTDL_QUERY_NO_SEED_TAIL`, default on): see
    `docs/known-limitations/tbox-only-inconsistency-light-route.md` for the light
    route's gap and why it stays. Individuals that occur only in data property
    assertions or declarations do not count, so `materialize_data_property_assertions`
    and `data_property_values` on such a KB take the same split. A TBox
    inconsistency only the tableau finds is missed on the light route. The gate is also a
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
| L5 | inconsistent KB ⇒ `instance`, `realize`, `disjoint`, `individuals`, `*-expr`, and since #428 `sat` / `subclass` / `explain`, answer yes or error `Inconsistent`, never "no" (`sat`: never "satisfiable"). `sat owl:Thing` is the consistency check (#427) and is not sampled |
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

**Before #502 this was weaker than stated.**
- `--hermit` answered "unavailable" for every candidate when `JAVA_HOME` was unset, and on any
  ontology with an unsat class it could not adjudicate an R1 claim at all. Pizza has two.
- Neither failed the run. So an R1 false positive on pizza failed the cargo gate only through
  the `{}` baseline ratchet, and a budget-cut one, which the ratchet exempts, passed silently.

Since #502:
- HermiT is asked per claim (`--consistency=C`, `--supers=C`, `--equivalents=C`);
- `--hermit` without a JVM is an error;
- a HermiT answer that decides nothing is listed under `unadjudicated` and fails the run.

The cargo gate (`cross_command_laws.rs`) still runs without `--hermit`, so a budget-cut R1 false
positive is still not caught there; only a `--hermit` run adjudicates it.

No reported "0 violations / 0 discrepancies" result depended on HermiT. It is consulted only
for a discrepancy, and those runs had none (#502 lists them).

**Coverage.** `--self-test` shows each law can fire on fabricated operands; that checks
the harness, not the engine. Engine-level evidence comes from two places.

Env sabotages (`--sabotage`), each FIRES on the unmutated binary and not in its control:

- R1: `RUSTDL_MAX_CHOICE_POINT=0` + `RUSTDL_MAX_TRIAL_MERGE=0` on `maxmerge_fp.ofn` (#76:
  `subclass A I` says yes, HermiT refutes it). Either flag alone does not restore the FP,
  and on the Stanford pizza both together still give 499 = 499, so `per_pair_fp_gate.rs`
  has no live sabotage; this is #76's live guard;
- R2: `RUSTDL_COMPLEX_QUALIFIER_VERIFY=0` on `complex_qualifier_98.ofn`;
- R3: `RUSTDL_CLASSIFY_INCONSISTENCY=0 RUSTDL_CLASSIFY_CONSISTENCY_PROBE=0` on `family.ofn`. The probe is off too since #446: otherwise a probe give-up (timing-dependent on family) sets `consistency_undetermined`, which R3 exempts.

Source mutants (package F-lite, `scripts/mutants/`): each patch plants one known fault,
built as its own binary and run through the quick mode with `--hermit`. A law catches a
mutant when the mutant's run has a violation, a HermiT-confirmed false positive, or more
discrepancies for an (ontology, law) than the unmutated control under the same env
(`scripts/mutants/detect.py`). Measured 2026-11-02, base 0090dc9f, seed 363:

| mutant | layer | re-opens | caught by | on the curated corpus? |
|---|---|---|---|---|
| M1 | classify consistency pre-check | all named classes unsat read as inconsistency | L4 (`all_unsat`) | no: fixture only |
| M2 | per-pair tableau, env only | #76 first-pair `≤n` merge (both flags off) | R1, HermiT-confirmed FP (`maxmerge_fp`)¹ | no: fixture only (pizza unaffected) |
| M3 | wedge clausifier | #78 naming clause on `var` | R1 over baseline (`neg_disjunct_forall`); HermiT confirms the miss¹ | no: fixture only |
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

¹ **The HermiT part of these labels cannot be checked from the record.**
- The 2026-11-02 run wrote to `mktemp` directories that no longer exist, and before #502 a
  missing JVM gave "unavailable" silently.
- **The catches stand without HermiT:** `detect.py` also counts discrepancies over the
  unmutated control.
- Re-run on main 5753fbc5 with #502: the #76 sabotage on `maxmerge_fp` is HermiT-refuted (`R1`,
  `refutes`).

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

**A third shape (#490, 2026-11-11): a functional role into a nominal cover.** Fixture
`scripts/gen/fixtures/konclude-functional-nominal-cover.ofn` (9 axioms, from the #490 PR-2 fuzz,
seed 144). `i3` differs from `i0`, `i1` and `i2`, so `⊤ ⊑ {i2, i0, i1} ⊔ C2` puts it in `C2`;
`C2 ⊑ ∃r1.∀r0.{i1}` with `r1` functional and `r1(i3, i0)` gives `i0 : ∀r0.{i1}`; `r0(i0, i2)` then
forces `i2 = i1`, against `DifferentIndividuals(i1 i2)`. HermiT (`InconsistentOntologyException`)
and rustdl say inconsistent; Konclude says consistent. This is a wrong "consistent", not an
under-report, so the same "both peers must agree" rule leaves it unscored.

**rustdl-fail, case 1283 (#437, fixed).** `consistent` and `sat` failed with
`ReasonError::NoVerdict` ("bug indicator") instead of answering or giving up with exit 3.
#437 removed the cause (a `Self(r⁻)` self-loop livelock); since #449 any `NoVerdict` that
reaches the CLI is `unknown`, exit 3. Since #462 every other command keeps its own output on a
`NoVerdict`: `incomplete` (or `status: "unknown"`) plus a `no_verdict` / `no_verdict_pairs` field
for a probe with no deadline, still with #449's note and exit 3. On a budgeted probe a `NoVerdict`
is the routine decision-depth cut (#288) and sets only `incomplete`.
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


## In-repo pure-EL guard (#440, and layer 2)

The `el_guaranteed` net needs Konclude and HermiT, so it never ran in CI, and #435/#436 reached
main through that gap. Two layers now guard the pure-EL claim in CI.

- **Layer 1 (#440)** checks answers. `tests/el_generated_guard.rs` classifies 300 frozen
  `el_guaranteed` cases: all 101 former findings, plus a motif-stratified sample. It compares
  them with the Konclude ∩ HermiT answers. It fails if a case that was certified is no longer
  certified, and there is a floor of 250 certified cases. Reverting #439's range push, its
  `Top` arm, or its trigger lowering turns it red (8, 52 and 11 of 300 cases), and so does
  reverting #114 (93).
- **Layer 2** checks the lowering itself. `owl_dl_saturation::unlowered` records, at 13 call
  sites, every construct the lowering drops, or weakens to `∃r.⊤`, after its fallbacks.
  `certified_unlowered` keeps the records that contradict the fast-path gate. It needs no
  oracle, so it also catches shapes the frozen sample lacks (for example `∃r.⊥`) and drops
  whose answer happens not to change. It does not catch a rule that is emitted but too weak:
  #440's range-push and trigger sabotages stay green under layer 2, and layer 1 catches them.

Measured 2026-11-04. The sweep ran on pin `rustdl-l2-95f94101`, the build before review.
The review change only skips work when no recorder is installed; the sites record the same
when one is. Answer identity and wall were re-measured on the final build:

| population | certified (classify / realize) | drops under certification |
|---|---|---|
| tracked `.ofn` fixtures (CI) | 113 / 100 of 261 | 0 |
| frozen `el_guaranteed` sample (CI) | 300 / 300 | 0 |
| full `el_guaranteed` set, N = 1000 | 998 / 998 | 0 |
| curated (`ontologies/real`, `external`) | 4 / 4 of 21 | 0 |
| ORE pool, 1920 files | 926 / 900 of 1863 read | 0 |

ORE exclusions: 46 files over 100 MB were skipped. 11 were rejected by the front end: 8 do
not parse, and 3 stopped at about 6 GB peak RSS under the 8 GB per-job limit.

**Found while surveying: `⊤ ⊑ ∃r.B` loses the `∃`.** The `Top`-LHS arm of
`lower_sub_class_of` keeps only the atomic conjuncts. So `⊤ ⊑ ∃r.B` with `∃r.B ⊑ X` misses
`X ≡ ⊤` under `completeness_guaranteed: true`. Konclude and HermiT agree on four reproducers.
Site `top-lhs-nonatomic` records it, and a test pins that the guard reports it. It reaches no
certified file in the populations above: the generator never plants a ⊤-LHS GCI, and the ORE
pool has none that is certified. An issue draft went to the manager; no fix is in this
change.

**Fixed (#444, #445).** A non-atomic `⊤ ⊑ D` now lowers as `K⊤ ⊑ D` for a shared ⊤ key
`K⊤`, and `seed` broadcasts every `⊤ ⊑ C` (and `K⊤`) to every class id, not only the user
classes. The site `top-lhs-nonatomic` is retired; the end-to-end "a certified drop is
reported" check now uses `A ⊑ ∃r.⊥` with `RUSTDL_EL_BOT_FILLER=0`
(`el_unlowered_guard_bot_filler_off.rs`, and the CLI's `reports_a_certified_drop`). The
`el_top` generator profile plants ⊤-LHS GCIs, so the off-CI net now covers the shape.

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
  an interleaved `is_consistent`. In the measured run, `is_subclass_of` was left out of the
  interleave because it ran no inconsistency guard and did not finish on an inconsistent KB
  (default case 438). #442 (#428) made it error on an inconsistent KB that its guard detects,
  but case 438 is TBox-only (no individuals), so the pre-checks miss its inconsistency and the
  lazy check has no seed, and `is_subclass_of` still does not finish there: measured on
  0df06f64, more than 150 s, shrunk to 8 axioms, and drafted as a #428 residual. The driver now
  calls `is_subclass_of` only when `is_consistent` returns True, which skips exactly the
  inconsistent cases; it is ungated once that residual is fixed.
  Classify in the warm route uses a 25 s global budget, standing in for the CLI's 30 s kill.
- R5.1's predictor is token-based. Degenerate `ObjectUnionOf(:C :C)` is excluded: it is the only
  predictor token in 4 control cases, 3 of which report pure-EL. `ObjectComplementOf` (neg-to-⊥)
  is not a predictor. The gate correctly admits both.
- R4.2 accepts a cut flagged only by `consistency_undetermined`. In three `--global-timeout-ms 1`
  runs (default 242 and 1766, `data_heavy` 354), an inconsistent KB comes back
  `consistent: true`, `incomplete: false`, with a hierarchy, and that flag is set. This is
  permitted by the plan: the flag is the give-up signal for that case. The opposite shape (an
  unflagged consistency claim under a cut) is R4.4's, below.
- The warm give-up set lacks `parse_timed_out`, because the Python `Classification` does not
  expose it.

### The give-up contract: R4.4 and R6 (#466, #462)

Two defects stayed invisible to the laws above. They come from the same contract: a run that
gives up has to say so in the field meant for it.
- **#466:** a cut run said `consistent: true` with `consistency_undetermined: false` on an
  inconsistent KB. It set `incomplete: true`, and that was enough for R4.2, which any flag
  satisfies.
- **#462:** a tableau `NoVerdict` printed a bare `unknown` instead of the command's JSON. Its
  signal was later split by budget: under #462 option (i), `no_verdict` (`classify`:
  `no_verdict_pairs`) means only that a probe with no deadline ended in `NoVerdict`. A
  budgeted `NoVerdict` is a depth cut and sets only `incomplete`.
- **#475** latches a saturation stall (the iteration cap in `saturate_loop`, not its deadline
  exits). A budgeted probe whose saturation stalled now sets the field too; a real
  decision-depth cut stays a cut. The laws below are in their #475 form; R6.3's and R6.6's
  earlier form is recorded with the 9cbf2b55 measurement.

Plan: `notes/plans/2026-11-05-363-giveup-contract-laws.md` (Fable: go with changes, all
taken).

**The laws.**
- **R4.4 (#466).** For each cut route, a claim of `consistent: true` with
  `consistency_undetermined: false` must agree with full.
  - If full says inconsistent, it is a violation, whatever `incomplete` says. It is info if
    the oracle says consistent.
  - If full is undetermined (or timed out) and the oracle says inconsistent, it is a violation.
  - If full makes the same claim on an oracle-inconsistent KB, it is info, "shared pre-check
    miss": the cut did not cause it. Only a5a2e063 had such a case (1283). On main, rustdl
    decides that case inconsistent, so this arm fires only in the self-test.
  - If full is undetermined and there is no oracle, it is info: there is no reference.
- **R6.1, format.** Every forced route prints the command's JSON. A run killed at the cap is
  counted separately as a timeout, not as a violation.
- **R6.2, one signal.** The field is set ⇔ #449's note ("likely a reasoner bug") is on stderr.
  When the field is set, the run exits 3 and sets `incomplete`.
- **R6.3, a stall is reported at any budget (#475).** Under the forced stall:
  - `nv_budget` sets the field ⇔ `nv_free` does on the same case (same engine, same stall);
  - a budgeted `nv_cons` (the wedge's bounded fall-through) sets it ⇔ it gave up;
  - a decided `nv_cons` never sets it.

  Before #475 the law was option (i): a budgeted probe never sets the field. That is the form
  the 9cbf2b55 measurement below checked.
- **R6.4, a stall cut is sound.** On an oracle-consistent KB, a forced route publishes no FP
  and never says `consistent: false`.
- **R6.5, a natural stall.** The field set on an unforced route (`cons`, or any budgeted
  classify route) is a finding (the #437 kind).
- **R6.6, a depth cut is never the signal (#475).** On the `dcut` route, a run whose stderr
  shows the search-depth limit has the field at 0 and no note. It is unresolved when the cut is
  not reached.

**New routes.** All run with `RAYON_NUM_THREADS=1`. The forced stall is
`RUSTDL_TABLEAU_SATURATE_ITERS=1`.

| route | command | stall | wedge |
|---|---|---|---|
| `nv_free` | `classify --json --pair-timeout-ms 0` | forced | off |
| `nv_free_n2` | the same with `--n2-classify` | forced | off |
| `nv_budget` | `classify --json --pair-timeout-ms 1000` | forced | off |
| `cons` | `consistent --json`, `RUSTDL_TRACE=1` | – | on |
| `nv_cons` | `consistent --json`, `RUSTDL_TRACE=1` | forced | on |
| `dcut` (#475) | `classify --json --pair-timeout-ms 20000` | – | on |

`dcut` is for R6.6. Run it on `scripts/gen/fixtures/depthcut/` as well: the 280-union there
needs 280 decision levels, past the 256 limit.

- **Why the forced classify routes run with the wedge off.** The stall knob is read only by
  the main tableau's `search.rs`. With the wedge on, plain classify's wedge answers the
  generated cases before that search runs.
  - On the 12 smoke cases, measured again on 9cbf2b55: `nv_free` reached the stall on 0 cases
    with the wedge on and on 7 with it off.
  - `--n2-classify` reached it on the same 7, with identical no-verdict counts either way, so
    the wedge setting makes no difference to `nv_free_n2`.
- **Why `nv_cons` keeps the wedge on.** The route R6.3 must watch is the wedge's
  `Stalled` → bounded tableau fall-through, and it exists only with the wedge on.
- Each `consistent` run is classified by its trace line:
  - "wedge Stalled — bounded tableau fall-through": budgeted;
  - any other wedge or saturation verdict, or the ABox pre-check or `abox_check` finding an
    inconsistency: decided;
  - no line: deadline-free (a TBox-only KB takes `decide_consistency(None)`).
- The measured run's classifier did not yet know the two pre-check lines. Its deadline-free
  bucket therefore also holds runs that the ABox pre-check decided: in Fable's review, 9 of 12
  sampled inconsistent runs from that bucket were pre-check runs. No outcome changes, because
  the field cannot be set on a pre-check path.

**Measured 2026-11-06.** The arms ran in parallel, one core each, at a host load of about 12.

| arm | binary | core(s) | routes |
|---|---|---|---|
| A | 9cbf2b55 (main) | 92–93 | full, the four cuts, all five new routes |
| B | 364b88ab (pre-#462) | 94 | full and the five new routes |
| C | S10 (9cbf2b55 with #471's review fix 1 reverted) | 95 | full, `cons`, `nv_cons` |

On A (main), 0 violations:

| law | compared | held | info / timeout | violations |
|---|---:|---:|---:|---:|
| R4.4 `cut_g1` | 2729 | 2729 | – | 0 |
| R4.4 `cut_g5` | 2727 | 2727 | – | 0 |
| R4.4 `cut_g25` | 2965 | 2965 | – | 0 |
| R4.4 `cut_p1` | 3723 | 3723 | – | 0 |
| R6.1 | 20000 | 19999 | 1 timeout | 0 |
| R6.2 | 19999 | 19999 | – | 0 |
| R6.3 | 8690 | 8690 | – | 0 |
| R6.4 | 14803 | 14803 | – | 0 |
| R6.5 | 4000 | 4000 | – | 0 |

R4.4 compares only the runs where the cut claims determined consistency. R6.4 compares only
oracle-consistent KBs. R1–R5 were not re-run.

**Reach on A.**
- The field is set on:
  - `nv_free`: 2454 of 4000 cases;
  - `nv_free_n2`: 2476 of 3999 (plus 1 timeout);
  - `nv_cons`: 1223 of 4000, every one deadline-free, so that is correct.
- `nv_budget` never sets it (R6.3).
- `consistent` probes by trace: 2337 decided, 1655 deadline-free (including the pre-check
  runs; see above), **8 budgeted**.
  - The 8 are default cases 181, 321, 939, 1269, 1646, 1747, 1942 and 1998. They are budgeted
    on both `cons` and `nv_cons`, because the wedge stalls on them without being forced.
  - The unforced `cons` route never set the field (R6.5: 0) and never timed out.

**Positives, declared in advance; each fired.**
- **R4.4, offline on the stored records of the R1–R5 run.**
  - Before #470: a5a2e063 had 26 violations over 10 cases; 0df06f64 had 25 over 11 cases.
  - After #470: 364b88ab (`f462-main`) and the #471 branch had 0 each.
  - R4.2 had passed all 26 hits on a5a2e063 and 24 of the 25 on 0df06f64, because each one
    set `incomplete`. The remaining one is the case-85 candidate (0df06f64, `cut_p1`, no flag
    at all), which R4.2 also catches.
  - Case 438 fires through the oracle arm: full is undetermined there.
- **R4.4, reproB (the #466 reproducer, by hand).** It ran with `RUSTDL_HYPERTABLEAU=0`, the cut
  being `RUSTDL_MAX_NODES=2`.
  - On 0df06f64, the cut says `consistent: true` with `consistency_undetermined: false`, and
    full says inconsistent (4 unsat): a violation.
  - On 9cbf2b55, the cut sets `consistency_undetermined`.
  - The planned `--extra-case` flag was dropped. The offline positive replaced it.
- **R6 on B (pre-#462): 6162 violations over 2492 cases.**
  - R6.1: 4931, the bare `unknown` on `nv_free` (2454) and `nv_free_n2` (2477).
  - R6.2: 1231, the note without the field (the field did not exist yet). This is A's 1223
    deadline-free probes plus the 8 budgeted ones.
- **R6.3 on C (S10): 8 violations.**
  - These are exactly the 8 budgeted probes. All 8 reproduced serially on core 92: S10 sets
    `no_verdict: true` with the note, and main does not.
  - So the generated sets do reach S10's route, on 8 of 4000 cases.
- **R6.4: covered by the self-test only.** No env mutant turns a stall into a definite answer.

**One-arm timeouts, rechecked serially on core 92 (120 s cap).** None reproduced:
- A's `nv_free_n2` timeout (case 1897): 0.9–1.4 s, exit 3, 87 no-verdict pairs.
- A's `full` timeout (case 1898): 0.3–1.5 s wall. Fable re-ran it 3 times on core 93 at
  0.05 CPU-s each.
- B's `nv_free` timeout (case 1368): 0.5–0.6 s.

Host load does not plausibly turn a run of about one second into a 30 s timeout. So these 3
(out of about 76,000 runs) are recorded as unexplained, unreproduced one-sided timeouts, not as
load artefacts. They do not affect any law: each counts as a timeout or a budget cut on one
case. A hang of the #460 kind was a candidate cause. On 2026-11-06 none of the three recurred in
120 reruns (60 serial, 60 with 6 copies per core), on main 3d0fb8b0 or on the arms' own
pre-#472 pins. Since the old pins do not hang either, the reruns attribute nothing to #472; the
three stay unexplained and unreproduced. The other 9 `full` timeouts (cases 321, 438,
812, 939, 1269, 1475, 1646, 1747 and 1998) are the same on every tag, the stored ones included.

**Limits.**
- The stall knob reaches only `search.rs`. The wedge, the EL fast path and the pre-checks
  bypass it. So R6 tests how the main tableau's `NoVerdict` reaches the output, at 61% case
  reach on classify. It does not test a `NoVerdict` inside the wedge.
- S10's route is reached by 8 default cases, and only where the wedge stalls without being
  forced. `RUSTDL_CONSISTENCY_FALLBACK_MS=1` does not widen it.
  - The check ran 6 cases the wedge decides plus 2 of the 8, on main and on S10.
  - The lever sent 2 of the 6 into the fall-through, and one of those only on main.
  - S10 fired on neither. It still fired on the 2 natural cases.
- R6 tests the `classify` and `consistent` commands only. The other query commands are not
  covered.
- Some arms have self-test positives only:
  - R6.4;
  - R6.3's classify arm (`nv_budget` and the unforced routes);
  - R6.2's "field without the note" arm and its "field without exit 3 or `incomplete`" arm;
  - R4.4's two info arms.

  The arms with engine positives are R6.1, R6.2's "note without the field", R6.3 on `nv_cons`,
  and both of R4.4's violation arms.
- R6.5 is effective on `cons` alone: classify's 5 ms default pair budget makes a natural
  `NoVerdict` a depth cut, which R6.3 covered instead. Since #475, R6.6 covers depth cuts.

### #475: the stall latch, measured 2026-11-06

Plan: `notes/plans/2026-11-06-475-saturation-stall-latch.md` (Fable: go with changes, all
taken).

**R6 on the generated net plus `fixtures/depthcut`.**
- Arms: branch 9e667fa3 (#475 with main c06763b2 merged) on cores 93–94, against main
  c06763b2 on core 92.
- Routes: `full`, the five `nv_*` / `cons` routes, and `dcut`.
- Pin check, a forced stall with the wedge off at a 20 s pair budget: main exits 0 with the
  depth-limit line; the branch exits 3 with `no_verdict_pairs` 3 and the note.

| law | main c06763b2 | branch |
|---|---|---|
| R6.1 | 20004 held, 1 timeout | the same |
| R6.2 | 20004 held | the same |
| R6.3 (#475 form) | **2460 violations**, 3987 held | **6447 held, 0 violations** |
| R6.4 | 14804 held | the same |
| R6.5 | 4001 held | the same |
| R6.6 | 4 held, 3992 unresolved | the same |

- R6.3 on main is the declared positive: there, the forced stall reads as a depth cut
  (`nv_budget` 0 where `nv_free` is set).
- `detect` (control = branch, mutant = main): 2455 detecting cases, all R6.3, and no other law
  differs.
- R6.6 resolves on the same 4 cases on both arms: the fixture and default 337, 745 and 1441.
  Each shows the depth-limit line with the field at 0 and no note.
- The timeouts are the same on both arms:
  - `full` and `dcut` on default 438, 939, 1646 and 1998, and `dcut` on 1269;
  - `nv_free_n2` on the fixture (deadline-free over 280 levels, expected). This one is R6.1's
    single timeout.

**The 1,920 ORE files, two arms.**
- Arms: main 3f85ee41 against the branch with 3f85ee41 merged (f977401d).
- Run: default `classify --json`, one thread, a 60 s CPU cap, through the harness, on 10 cores.
  Each core ran both arms on its chunk.
- Instrument proof, run first: a forced-stall smoke on 24 files through the same harness and
  extractor. The branch set the field on 9 files; main set it on none.

Results:
- `no_verdict_pairs` is 0, and the note is absent, on all 1920 files on both arms. There is no
  natural stall.
- On the 1783 files both arms finished:
  - exit code and depth caps are identical;
  - CPU is 6348 s on main and 6401 s on the branch (+0.8%);
  - content is identical except ore_ont_12698. That file is budget-truncated: 3 serial reps per
    arm gave 6 distinct closures, so it varies within each arm.
- The 3 caps hit by one arm only (16008, 3524, 8194) each need 58–66 CPU-s. At a 180 s cap,
  serially, both arms finish them with identical content.
- 134 files hit the cap on both arms. 21 have no JSON on both arms, with identical codes:
  8 parse errors, and 13 that fail a 10 GB allocation under the 8 GB limit.
- No file reaches the depth limit at the default 5 ms pair budget. So the sweep cannot show that
  a depth cut stays a cut; that rests on R6.6 and on the runs below.

**PMD core and the five depth-capped ORE files, at 1000 ms.**
- Arms: main a29a7bc5 against the branch, 3 interleaved reps per arm on one core, by CPU time.
- PMD core is also run at the default budget.

| input | depth-capped probes | CPU s, median of 3: main / branch | outcome |
|---|---:|---|---|
| PMD core, default | 0 | 22.2 / 21.0 | identical on both arms |
| PMD core, 1000 ms | 9 | 20.0 / 18.1 | identical |
| 15514 | 4 | 0.9 / 0.9 | identical |
| 185 | 5 | 0.9 / 0.9 | identical |
| 6934 | 0 | 5.4 / 5.5 | identical |
| 778 | 2174 | 262.6 / 264.2 | identical |
| 9890 | 267 | 75.2 / 74.9 | identical; one main rep had 266 + 1 deadline cut |

The rep ranges overlap on every input: PMD at the default is 21.6–24.0 on main and 20.5–24.1 on
the branch; at 1000 ms it is 19.7–24.1 and 17.2–22.0. The CPU differences are noise, not a
speed-up.

- On every input: `no_verdict_pairs` is 0, there is no note, and the exit code and content
  match.
- Depth caps are unchanged: the depth cut stays a cut.

## Per-query laws: `subclass`, `sat` and `explain` on the generated net (#363 R7)

`scripts/gen/perquery.py` asks every ordered pair and every class of the 4000 generated cases
through the per-query commands. Each answer is checked against the same binary's `classify` and
the cached Konclude ∩ HermiT oracle. The oracle answers only where both peers agree, so a
refuted pair is definite. Plan: `notes/plans/2026-11-06-363-r7-per-query-surface.md` (Fable: go
with changes, all taken).

**Why.** Three defects lived on this surface while the classify nets were green:
- **#76:** an FP;
- **#460:** a hang on a consistent KB that classify answers in 0.25 s;
- **#457:** a hang, and silent answers, on an inconsistent TBox-only KB.

None of them was in a law before. #460 and #457 were found only when other routes stalled.

**How R7 relates to the existing gates.** It extends them; it does not duplicate them.
- **`per_pair_fp_gate.rs`** audits the per-pair tableau by proxy, `classify` under
  `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1`, on 4 curated ontologies. That proxy is R7's `vr`
  route, run on 4000 shapes. #478 added the gate's live control, `per_pair_fp_gate_synthetic.rs`.
- **`cross-command-laws.py`** runs the same commands on the curated corpus, sampled, against
  Konclude with HermiT on demand, with R1/R2 as ratchets. R7 reuses its operand readers.
  Because the generated oracle is definite, R7's FP laws are hard.

**What runs, per case:**
- `full` classify (`--pair-timeout-ms 1000`) and `vr` (the same plus `VERIFY_REFUTATIONS`);
- `subclass` on every ordered pair, and `sat` on every class;
- `explain` on 3 seeded pairs: a classify positive when there is one, a negative, a random
  pair. On an extra case it runs on every pair.

All calls run single-threaded on one core. Per-query calls are capped at 10 s of CPU, with a
60 s wall backstop. After a command's first capped call on a case, its remaining calls there
are skipped as "after-hang".

**The laws.**
- **R7.0:** R2.3's oracle FP check on `vr`, with `full` as the contrast.
- **R7.0c:** `vr` = `full` on the EL sets, where the flag is inert.
- **R7.1, FP:** `subclass`/`explain` = yes where the oracle refutes. Hard.
- **R7.2, false unsat:** `sat` = unsat where the oracle has the class satisfiable. Hard.
- **R7.3, agreement with classify.** Classify implies A ⊑ B when the pair is in its closure,
  A is classify-unsat, or B is classify-Thing-equivalent.
  - Direction 1: classify implies it and the command says no. A violation; classify's rows
    are sound under a cut.
  - Direction 2: the command finds it and classify lacks it. The oracle decides. When confirmed,
    it is a violation (D10) only if classify had no give-up and `completeness_guaranteed`;
    otherwise info.
- **R7.4:** `explain` = `subclass` on the same pair.
- **R7.5, an oracle-inconsistent KB:**
  - a wrong answer (`sat` = sat, `subclass` = no) is hard;
  - a silent answer (correct, exit 0, no undetermined warning) is "known #457" on a TBox-only
    KB, and a violation otherwise;
    since #457 residual 2 (A″), a tableau-answered query there errors instead, so "known #457"
    is in effect the light-route gap (told, saturation or query-wedge answers). On the 39
    known cases the A″ pin left 1418 of the 1622 (204 became `Inconsistent`: 152 `sat`,
    49 `subclass`, 3 `explain`), with 0 violations on 1350 TBox-only cases; its new
    `Inconsistent` errors on cases 214 and 1281 (no oracle verdict) are correct by hand;
  - an answer with the warning, or the `Inconsistent` error, is ok.
  - "TBox-only" means no assertions and no nominals. A bare `Declaration(NamedIndividual)` is
    TBox-only: the guard reports `has_individuals=false` there.
- **R7.6:** capped calls, rechecked serially at 120 s of CPU.
- **R7.7:** misses (a measurement): `subclass` no or `sat` sat where the oracle entails.

**Measured 2026-11-06** on main 3d0fb8b0. Arm A ran on cores 92–93, and arm M on 94–95 under
`RUSTDL_MAX_TRIAL_MERGE=0` and `RUSTDL_MAX_CHOICE_POINT=0` (#76's sabotage). Host load was
about 11.
- **Main has moved since.** a019289e (#477) is search-identical by proof. b6ed041d (#479) is
  not, and it changes R7.6's results; it is covered below.
- **Volume:** 4001 cases (4000 plus `maxmerge_fp.ofn`): 185,600 `subclass`, 28,145 `sat` and
  10,977 `explain` calls per arm.
- **Cost:** A took 1.69 CPU-h. Wall time was about 4.75 h per arm, because the 3 s per-query
  wedge deadline and the classify cap are wall-clock.

| law | A: held | A: violations | M: violations | not compared, and why (A) |
|---|---:|---:|---:|---|
| R7.0 `vr` | 3927 | 0 | 1 (`maxmerge_fp`) | 67 no oracle, 7 classify timeouts |
| R7.0 `full` | 3927 | 0 | 0 | the same |
| R7.0c | 1499 | 0 | 0 | 1 classify give-up |
| R7.1 | 35006 | 0 | 2 (`maxmerge_fp`: `subclass`, `explain`) | 7 capped, 1 SIGSEGV (below) |
| R7.2 | 2608 | 0 | 0 | 4 capped |
| R7.3 | 211539 | 0 | 0 | 99 info (classify misses), 18 with no oracle |
| R7.4 | 10968 | 0 | 0 | 9 with an unknown side |
| R7.5 | 9093 | 0 | 0 | 1622 known #457 (39 cases); 1879 `Inconsistent` errors on 33 cases with no oracle |
| R7.7 | 172335 | 0 misses | 0 misses | – |

**On main, the per-query surface had no FP, no false unsat and no disagreement with classify
on 4000 shapes.** It also had no miss against the oracle.
- The 99 R7.3 infos are oracle-confirmed pairs that `subclass` finds and classify lacks. In
  each, classify did not claim completeness.
- All 1622 R7.5 silent answers are on TBox-only inconsistent KBs: #457. That classification
  was made after the run, not declared in advance.
  - The first laws pass scored 850 of these answers as violations ("silent answer on a KB with
    individuals"), on 21 of the 39 cases.
  - Those cases' individuals are declared but never used: no assertion and no nominal. The
    guard sees no individuals there (`has_individuals=false`), which is #457's shape.
  - They were reclassified in 930fe6f2 (`tbox_only` ignores a bare `Declaration`); the laws
    recompute it from the case file.
  - This is the attribution the plan required for silent answers with individuals. It was not a
    pre-declared carve-out.

**Positives, declared in advance; each fired.**
- **P1, the #76 sabotage on the generated cases: 0 cases.** As declared, I report that the
  generated profiles do not produce #76's shape, and I did not pad the sets.
- **P2, `maxmerge_fp.ofn` as an extra case.** Konclude and HermiT agree that `A ⊑ I` is not
  entailed.
  - In M: R7.1 fires on `subclass A I` and `explain A I`, and R7.0 fires on `vr`. `full`
    classify stays blind: that is #76's blindness, and the reason for the gate's proxy.
  - In A: nothing fires.
- **P3:** case 438 lands as "known #457" (silent `sat` = unsat).
- **P4, case 1911 (#460):** it reaches the cap once per command on the pre-#472 pin 0df06f64,
  and the after-hang skip engages. It never reaches the cap on A. This is also the behavioural
  check that tells the two pins apart: both print `0.4.42`.

**R7.6: per-query hangs, rechecked serially at 120 s of CPU on core 92.**
- All 11 of A's capped calls were rechecked (`recheck-r7-A.txt`).
- In M, only the 5 capped calls on M-only cases were rechecked, under M's env (`hang3.jsonl`).
  M's other 11 capped calls are on cases that hang in A too, so the full M recheck was stopped
  (with the manager's OK).
- P4's serial recheck of case 1911 on 0df06f64 was not run: the smoke cap and the skip are its
  evidence.

| case | KB (oracle) | classify | per-query, 120 s serial | verdict |
|---|---|---|---|---|
| data_heavy 306 | consistent; nominals and an assertion | 0.024 s, no give-up | hangs on 0df06f64 and 3d0fb8b0; **0.02 s on b6ed041d** | fixed by #479 (nominal-disjunction wait) |
| default 1349 | consistent; no nominals or assertions | 0.06 s, no give-up | **hangs on all three**, including b6ed041d | **finding:** shrunk to 2 axioms, filed as #485 |
| default 939, 1646 | consistent | classify times out too (30 s) | hangs | shared hardness, not a per-query defect |
| default 213, 1706 | consistent | 213: 1.85 s, `incomplete` and `consistency_undetermined` in both arms; 1706: 0.06 s in A, 3.4 s and `incomplete` in M | hangs only in M; ≤ 0.11 s CPU in A | the sabotage's own blowup |

Case 1349's reproducer:
`C0 ≡ ≤0 r1.¬C5`, `C5 ≡ ∃r0.(=2 r1.C2)`.
- `sat C5` runs past 125 s on main and on 0df06f64.
- Classify decides it only by trusting the wedge's Sat: with `TRUST_SAT=0` or the wedge off,
  it reports `incomplete: true`.

**Noise and its accounting.**
- **One-arm unknowns between A and M: 125.** All but one are M's capped calls on cases 213
  and 1706 and the after-hang skips that follow them.
- **The other one** is a single A call on default case 1807 that died with SIGSEGV after 3 ms.
  The kernel logged the fault inside `ld-linux-x86-64.so.2`, before `main`. It ran clean 20 of
  20 times serially, so it is recorded as a process-start artefact. The kernel line and the
  rerun exit codes are in `work-363-gen/r7/segv-1807.txt`.
- **A test build overlapped M.** An unrelated one-file test build ran at nice 19 on core 95
  from 03:58 to about 04:05 UTC, overlapping arm M. No M-only cap fell in that window.

**Limits.**
- R7 covers `subclass`, `sat` and `explain`. `instance`, `realize`, `individuals`, `disjoint`
  and the `*-expr` commands are not covered (a possible R7b).
- `explain` is sampled: 3 pairs per case.
- With a 10 s CPU cap per call, a slow but finite query counts as capped. Each one is rechecked
  at 120 s.
- The skip after a hang leaves a command's later calls unexamined on that case: 182 calls in
  A, all on the 4 capped cases.
- P1 shows the #76 shape is absent from the generated profiles. R7's engine positive for #76 is
  the extra case alone.

## Per-query census on real ontologies (2026-11-11)

A one-off measurement, not a gate: the per-query commands on real ontologies against classify,
HermiT and Konclude. Plan: `notes/plans/2026-11-07-perquery-census-real.md` (Fable: go with
changes, all taken). It was run to find #498-kind false positives before users do. #498 was a
main-tableau defect over the ABox that the classify nets could not see, and it lived on main
for 8 days.

**Instrument.** A local copy of `cross-command-laws.py` with these additions:
- **R8:** wedge-off `consistent` (the main tableau over the ABox, the per-query route) against
  default `consistent` and classify.
- **R1e:** `explain` positives against the classify closure.
- **R3q:** a per-query "inconsistent" on a consistent KB.
- **R9 / R10:** `individuals` different pairs and derived memberships, checked by the oracles.
- **Sampling:** every pair on files with ≤ 12 classes, and a nominal-biased sample elsewhere.
- **Adjudication:** HermiT per claim (`-k`, `-S`, or the file plus the claim's negation) and
  Konclude, whatever classify's flags.
- **Caps:** classify 300 s; 30 s per call.

**Instrument proof (F0, declared in advance):**
- c5e and q5 (#498) give 80 HermiT- and Konclude-refuted FPs on the pre-fix pin 5753fbc5;
- the negative control min9 gives 0;
- the same pin with `RUSTDL_NOMINAL_BLOCK_GUARD=0` gives 0 on all three files.

**Frame:** 433 files, cores 92–95.

| part | source | files |
|---|---|---|
| F1 | curated files with an ABox or nominals: wine, family, pizza, pizza-noimport, pizza-ontostart, ro | 6 |
| F2a | ORE files with an ABox and object nominals (`ObjectOneOf` / `ObjectHasValue`), ≤ 1 MB | 177 (all) |
| F2b | the same, 1–5 MB | 220 (all) |
| F3 | ORE files with an ABox and no object nominal, ≤ 5 MB | 30 (a seeded sample of 487) |

**Run 1, on 5753fbc5 (#498 unfixed), 31,200 calls, 10.25 CPU-h:**
- **0 false positives, 0 discrepancies.** Classify missed nothing the oracles saw.
- **Laws:** R8 decided 338 files; R1 6,777 ok; R1e 975; R2 5,966. Every hard law held after one
  harness fix (#507: the L3 gate missed classify's `consistent: false` when `consistent` was
  killed).
- **No file in the frame showed #498** on the unfixed binary, beyond the F0 controls.
- **ABox arm:** ORE pool files declare no individuals, so the main run asked no individual
  query there. A supplement then read the individuals from the assertions:
  - F2a and F3: 483 claims (R9 95, R10 388);
  - F2b, with one `instances-expr` per file: 28 R10 claims;
  - all of them entailed, none refuted.
- **Completeness finding, #510:** 351 `instance` calls on 36 files were cut, and on those files
  `instance` never answered "no". The cause was traced on 3 of the files (13132, 394, 2488; 15
  pairs): the timed probe's 256-level depth cap with early abandon. A deadline-free probe answers
  "no" on all 15 pairs, as both oracles do. The other 33 files were not traced and only look
  alike.

**Run 2, on 0a87ebc3 (#504, #508, #511):** the same frame and the same sample. The pairs are drawn
from run 1's classify (`--draws-from`), so only the binary changes. It also asks the ABox arm's
single `instance` calls.
- **Checks before the run:**
  - the old binary through the patched harness replays every base call from the cache and draws
    the same sample on 433 of 433 files;
  - c5e and q5 come out corrected (6 `sat` and 30 `subclass` per file);
  - wine's `sat` answers in 27 s, where it ran past 150 s before (#511).
- **0 new false positives.** Classify's content is unchanged on the 432 files where both binaries
  finished it. On ore_ont_778 the new binary's classify was killed (a loss, below).
- **New positive answers, all entailed per the oracles:**
  - 2 new `realize` types on both pizza-ontostart copies;
  - SauvignonBlanc ≡ Semillon on ore_ont_7668;
  - 1 `disjoint` pair on ore_ont_5033.
- **Rescues** (the old binary gave no answer, the new one answers):
  - wedge-off `consistent` on 8 files, among them pizza ×2 and 5 wine-family files;
  - `sat` 17, `subclass` 16, `sat-expr` 1: killed before;
  - `instance` 2: cut before;
  - the per-query sample of 6 wine-family files runs for the first time.
- **Losses:** 17 calls in the census, all killed or cut at a wall-clock cap, including ore_ont_778's
  classify (267 CPU-s against the 300 s cap before; killed in the re-run). **None is a regression.**
  - **Recheck** (2026-11-12): one call at a time on a quiet host, both pins, 3 reps each in alternating
    order, caps raised to 120 s / 900 s.
  - **Result:** all 17 answer on the new pin in 3 of 3 reps, with 0 kills. Each call gave the same answer
    in all 6 runs, and it is the old binary's census answer. That covers the `disjoint` pair sets and 778's
    classify content (rows, unsat, equivalence groups, consistent).
  - **Cost:** median CPU per call is 0.1–28.4 s on the new pin and 0.1–27.8 s on the old. On 778 it is
    253–268 CPU-s on the new pin and 264–282 on the old.
  - **So the census kills were contention:** they came at 30 s wall after 22–29 CPU-s. Two calls show it
    plainly: 6483's `subclass` was killed after 0.3 CPU-s, and 16191's `instance` was cut by the 750 ms
    wall-clock pair deadline after 0.09 CPU-s. Serially, both answer in under 0.5 s.
- **Cost:** on the 34,437 calls both binaries were asked, CPU went from 10.19 h to 9.93 h.
  - Those calls are 31,010 of run 1's 31,200 plus 3,427 `instance` calls from the ABox arm. The
    other 190 run-1 calls (0.71 CPU-h) were not asked again; they are all on 6 files where an
    earlier answer changed (5688, 1509, 12253, 778 and the two pizza copies).
  - Kills went from 268 to 244: 42 calls are no longer killed (the 42 non-`instance` rescues),
    and 18 newly are. Those 18 are 16 of the losses plus two calls that gave up on both binaries
    (ore_ont_8148's default `consistent` and one `instance` call on ore_ont_10949) and were
    killed only on the new one. The 17th loss, `instance` on 16191, is a cut, not a kill.

**Limitations.**
- **The 30 s main-tableau cap.** On 76 of 433 files (17%, run 1) the main tableau was killed at 30 s,
  and so was the one-`sat` probe, so those files have no per-query sample. On another 8 the main
  tableau was killed but the probe answered, so they were sampled.
- **R8 reach:** R8 has an outcome on 426 files and decided 338. The other 88 are the 84 files where
  the main tableau was killed and the 4 rustdl can't parse. On the remaining 7 files the main
  tableau answered but default `consistent` was killed, so there was nothing to compare.
- **The ABox arm's reach on F2b is thin.** 170 of F2b's 220 files are eligible (the per-query
  sample ran there). `realize` and `instances-expr` answer on 7 and 8 of them, and `individuals`
  (R9) runs only up to 150 individuals, so on 1 file.
- **F3 is a sample of 30 out of 487.** Its zero says nothing about the rest.
- **Agreement is not correctness.** Where classify and a per-query command share a wrong answer,
  only an oracle sees it. The oracles are asked only about discrepancies, claims and new
  positives.
- **A gap in the laws** (Fable's review of #507): a false "inconsistent" shared by classify and
  the per-query commands, while `consistent` is killed, reads as agreement. A U1 arm (classify
  `consistent: false` against an oracle) is proposed.
- **Oracle reach:** HermiT fails on pizza-ontostart, so claims there are Konclude-only.
- **Not reproducible from the repo.** The instrument (a modified copy of `cross-command-laws.py`)
  and the raw data are on the shared measurement host only (below).

**No new fixtures.** The per-query arm of `per_pair_fp_gate` (#505) keeps c5e and q5 as
positives.

**Data:** on the shared measurement host, under `~/rustdl/work-363-gen/pqc/`:
- run 1: `REPORT-full.md`, `full/`, `abox-*`;
- run 2: `rerun/REPORT-rerun.md`, `rerun/diff.json`;
- the harness copy: `harness/`.
