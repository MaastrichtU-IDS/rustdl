# Query-surface ledger: unknown versus negative

Work package A2 of the correctness assurance programme (#363). The
[completeness guarantee ledger](completeness-guarantee-ledger.md) covers
`classify`. This one covers every other query surface and asks one question:
**when a probe reaches no verdict, does the caller find out?**

Checked against `main` at `0bbe276c` (31 October 2026).

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
| Rust `*_reporting`, `Realization` | `incomplete` (second tuple element, `Realization::incomplete()`) | at least one probe was cut |
| CLI single answer (`consistent`, `sat`, `subclass`, `instance`, `explain`, `prove`) | prints `unknown`, exits 3, explains on stderr | no verdict |
| CLI list answer (`instances`, `realize`) | stderr warning; members printed are sound | members may be missing |
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
| `sat` | node cap (`RUSTDL_QUERY_MAX_NODES` on the CLI/Python entry point; early abandon is armed only under a deadline). A deadline-free depth bail-out is an error (exit 1), not `unknown` | `is_class_satisfiable_with_stats` → `incomplete` | `unknown`, exit 3 | no `--json` | `is_class_satisfiable` warns | `query_give_up.rs`; `test_query_give_up.py` |
| `subclass` | node cap (`RUSTDL_QUERY_MAX_NODES`) | `is_subclass_of_with_stats` → `incomplete` | `unknown`, exit 3 | no `--json` | `is_subclass_of` warns | same |
| `explain` | node cap | `is_subclass_of_with_stats` | `: unknown — tableau gave up`, exit 3 | — | — | `query_give_up.rs` |
| `instance` | per-probe deadline (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, default 750 ms), node cap, depth bail-out | `is_instance_of_reporting` | `unknown`, exit 3 | no `--json` | `is_instance_of` warns | `query_give_up.rs`; `test_query_give_up.py` |
| `instances` | same | `instances_of_reporting` | stderr warning | no `--json` | `instances_of` warns | same |
| `realize` | same, plus the node cap with the per-probe deadline disabled (`=0`) | `Realization::incomplete()` | stderr warning | `incomplete` | `realize` warns | same |
| `sat-expr`, `subclass-expr`, `instances-expr` | as `sat` / `subclass` / `instances`; also `incomplete` whenever the ontology is not pure EL | `CeVerdict` / `CeInstances` `incomplete` | (already reported) | `incomplete` | warns | `class_expr_query` tests |
| `disjoint` (classes) | per-pair deadline, node cap | `DisjointClasses::incomplete` | yes | `incomplete` | warns | `disjoint_nodecap.rs` |
| `individuals` | per-pair deadline, node cap | `incomplete` | yes | `incomplete` | warns | individuals tests |
| `property-values` (object) | per-pair deadline, node cap; fragment whitelist | `incomplete` | yes | `incomplete` | warns | property-values tests |
| `justify` (incl. `--all`, `--laconic`) | any probe under it: node cap (`RUSTDL_MAX_NODES`), a cut instance probe, the consistency fallback deadline. Top-level probe → no verdict; a QuickXplain subset probe → non-minimal; an enumeration branch probe → a justification may be missing (#396) | `JustifyOutcome { status: Verdict, incomplete }` from the `*_reporting` finders; `minimal_guaranteed` cleared on a QuickXplain give-up | top level: `unknown`, exit 3; otherwise stderr warning | `status: "unknown"` (exit 3), `incomplete`; `enumeration_complete: false` under `--all` | `justify`, `justify_all`, `PreparedOntology.justify*` warn | `justify_give_up.rs` (CLI, reasoner); `test_justify_give_up.py` |
| `repair` | as `justify`, plus each candidate's verification probe | `Repairs::incomplete`; `complete` cleared; a candidate whose verification gave up is dropped and counted in `unverified_gave_up` | top level: `unknown`, exit 3; otherwise stderr warning and a note | no `--json` | `repair` warns | same |
| `diagnose` | classify's unsat probes (node cap: a class reported satisfiable without a verdict); the consistency check on an all-classes-unsatisfiable ontology; the justifications under it as `justify` | `Diagnosis::{incomplete, unsat_probes_cut, consistency_gave_up}` | stderr warning per cause; `consistent (NOT witnessed …)`; `coherent: no unsatisfiable class found (N check(s) gave up)`; per-entailment `unknown` lines | no `--json` | `diagnose`, `debug` warn | same |
| `report` | everything under it (diagnose, per-root justify and repair) | — (CLI only) | HTML banner, stderr warning | — | — | `report.rs` unit test |
| `prove` | the subsumption check (node cap); the justification fallback, as `justify` | `prove_entailment_rcstr_reporting` → `(ProveEntailmentResult, incomplete)`; `ProveEntailmentResult::Unknown` | `unknown`, exit 3; fallback give-up: stderr warning | `entailed: false`, `status: "unknown"`, `incomplete`, exit 3 | no binding | `prove_give_up.rs` |

## Not a give-up, but still a gap (filed)

These are not cut probes; they are surfaces that compute less than the
entailments and still report a complete-looking answer. Each needs a decision
about its signal, so each is filed rather than fixed here.

- **realize's pseudo-model prune** drops entailed types without setting
  `incomplete` (`witness_prune_active` reports that the prune ran). It is on for
  nearly every off-fragment ontology with individuals, so a warning on the flag
  alone would fire almost always. #387.
- **realize's classify step** runs under a deadline whose cut pairs are not
  folded into `Realization::incomplete`, so `direct_types` can be non-minimal
  (never a false type). #388.
- **disjoint properties, the property hierarchy and data property values** are
  structural. #389 and #390 give them their own signal, `structural_gaps` (CLI
  `--json`, one stderr line in text mode, Python `StructuralGapWarning`), and fix
  the cheap misses (`Symmetric`, disjointness down the hierarchy, `Asymmetric`,
  top/bottom, one-leg chains, anonymous individuals). `incomplete` stays a
  give-up signal. See the
  [completeness ledger](completeness-guarantee-ledger.md#structural-property-surfaces-389-390).
- **materialize_\*** (`materialize_existential_successors`, Python
  `materialize_inferred_class_assertions`) drop realize's flags. #391.
- **consistency's trusted wedge `Sat`** is a trust decision, not a cut, and has
  no signal beyond `wedge_clauses_refused`. #392.

## Rule for changes

A change that adds a way for a query to give up, or a new query surface, must in
the same PR add or update its row here, report the give-up on every surface the
query is exposed on (Rust, CLI, Python), and add a test that forces the give-up.
`RUSTDL_MAX_NODES=1` on a small out-of-fragment ontology that generates a
successor is a deterministic way to force one (see `query_give_up.rs`); run it in
its own process, because the cap is cached once per process.

## Node caps

The CLI's `sat`, `subclass`, `explain`, `instance` and `prove` (around the whole
call, so its justification fallback too, #402), and the Python
`is_class_satisfiable`, `is_subclass_of` and `is_instance_of`, run their one query
under `owl_dl_reasoner::single_query_node_cap()`, which raises the cap to
`RUSTDL_QUERY_MAX_NODES` (default 500000; `0` disables). Everything else uses
`RUSTDL_MAX_NODES` (default 50000): library calls, classify, consistency, realize,
`justify`/`repair`/`diagnose` (many probes per query) and the class-expression
queries. An explicit `RUSTDL_MAX_NODES` governs both. Both defaults are pinned by
`owl-dl-tableau/tests/node_cap_defaults.rs`, and the scoping by
`owl-dl-reasoner/tests/single_query_node_cap_scope.rs`. The
single-query default was raised from 50000 when the give-up became visible: at
50000, 3 of 43 measurable random ORE ontologies answered `unknown` on `sat` and
`subclass` that finish with a 200k–500k cap.
