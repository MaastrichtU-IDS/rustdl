# Changelog

All notable changes to rustdl are documented here. Format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); rustdl follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.17] — 2026-06-28

### Changed

- **`classify` is now bounded by default — it can't hang on wine-class
  ontologies.** The Python default ran each pair at 1000 ms with no global
  bound, so total time grew with the pair count and could hang on combinatorial
  nominal+cardinality+disjunction inputs. New defaults:
  - **`per_pair_timeout_ms` 1000 → 100.** Measured completeness floor: at 100 ms
    the full Konclude-oracle corpus is FP=0/MISSED=0 byte-identical (real corpus
    28 s → 12.8 s, ~2.2×). 25 ms was rejected — it flakily drops 4–8 real
    subsumptions on the standard pizza fixture.
  - **new `global_deadline_ms` = 60000** — bounds the *total* wall (the backstop
    the per-pair cap can't provide). Each probe is cut at
    `min(per_pair, remaining global)`.

  Set either bound to `0` to disable it; both `0` = unbounded/complete. Sound at
  every setting — cut pairs default to "not subsumed" (FP=0); the
  `IncompleteClassificationWarning` and `Classification.complete` /
  `.timed_out_pairs` report any incompleteness.

### Added

- `owl_dl_reasoner::classify_with_budget(ontology, per_pair, global)` — public
  combined per-pair + global-deadline classify entry point.
- `owl-dl-bench corpus --pair-timeout-ms <N>` — per-pair cap for the corpus
  harness (it previously ran unbounded and hung on wine-class inputs);
  `bench-corpus/` fixtures added.

## [0.3.16] — 2026-06-27

### Internal

- **Saturation-only closure-diff mode** (`ORE_ONE_SAT_ONLY=1`) in the
  `konclude_closure_diff` test harness: computes rustdl's closure from the
  saturation-only path (a global fixpoint, no per-class wedge) and diffs it
  against the Konclude oracle. Diagnostic for whether the sound-but-under-
  approximate saturator is already complete on ontologies whose per-class wedge
  classify is too slow to finish. Test-only and env-gated; no engine change.

## [0.3.15] — 2026-06-27

### Performance

- **Restrict the MRV disjunction scan to anchored clauses.**
  `find_open_disjunction` (the wedge's most-constrained-variable ⊔-scan) used to
  examine every `(node × non-Horn clause)` pair per call — up to ~35k pairs/call
  on large ORE ontologies. A disjunctive clause can only be an *open* ⊔ at a node
  when its X-class atoms are in that node's label, so the scan now iterates a
  precomputed ascending `(clause, first-X-class anchor)` list and skips an
  anchored clause via an O(1) `has(anchor)` check (the residual role-only/empty
  bodies are always scanned). Soundness is termination-only here — MRV's choice
  is verdict-invariant and no open ⊔ is missed; ascending order keeps the
  tie-break identical (closures byte-identical, FP=0/MISSED=0 corpus-wide).
  **sio −12%, ore-15672 −10%, wine@1ms −18%, ore-10908 −5%**; flat on EL.

## [0.3.14] — 2026-06-27

### Performance

- **Precompute per-clause match plan (matcher hot loop).** `match_body` — the
  wedge's #1 hotspot by profiling — used to re-partition the clause body
  (role/class/X-class atoms) and re-run the variable-tree `eval_order` on every
  `(clause, node)` call, even though both depend only on the immutable clause
  body. Each clause's `(role_atoms, other_classes, X-classes, eval_order)` (or
  its "unsupported" verdict) is now computed once at index-build and stored in
  the Arc-shared `ClauseIndexes`. Sound by construction (memoizes immutable
  clause-derived data — matches and verdicts byte-identical). **ore-10908
  −8…−11%** (the matcher-heaviest classification), sio −2%; flat on EL and
  tier-walk-bound workloads. The companion to the v0.3.12 `SmallVec`/membership
  matcher wins (which cut allocation; this cuts the recompute).

## [0.3.13] — 2026-06-27

All changes below are **sound** — FP=0/MISSED=0 byte-identical against the
Konclude∩HermiT oracle across the full corpus (wine, galen, notgalen, sio,
ore-10908, ore-15672, pizza, alehif, ro, sulo, bibtex).

### Performance

- **Coupled-saturation precompletion seed (`RUSTDL_SAT_SEED`, default-ON).** The
  classify path now seeds the per-class wedge search with the saturator's named
  subsumers (SP2) and derived existential facts (SP3) — Konclude-style coupled
  saturation. Collapses wine's hard nominal/disjunctive model builds:
  **wine classify 49s → 3.2s (~15×)**, sound. Pairs with MRV ⊔-ordering and the
  adaptive label-cache deadline.
- **Value-derived type-disjointness + tautology-skip (default-ON).** Types forced
  to distinct nominal values on a functional role are treated as disjoint, and
  `a ⊔ ¬a` complement disjunctions are skipped — together they make wine's
  descriptor value-partition fragment tractable (the 8 previously-hardest classes
  now resolve in milliseconds).
- **Label-cache deadline floor 1000ms → 50ms.** The per-class label-build budget
  is `n × per_pair` (the refute-the-row break-even); the old 1s floor sat above
  that and over-invested at tight per-pair caps. Lowering it gives **wine
  −39% at `--pair-timeout-ms 1`** (1.54s → 0.94s). Inactive at the default cap.

### Completeness

Two sound completeness levers (default-ON; pure gains — close real subsumptions,
never introduce false ones):

- **Nominal-filler typing (`RUSTDL_NOMINAL_TYPING`).** `ClassAssertion(C, a)` now
  types the nominal filler so `∃R.{a} ⊑ ∃R.C` is derived (object value-membership,
  the analog of the data D6 lever). DMOP MISSED 31→0.
- **ObjectOneOf common-subsumer (`RUSTDL_ONEOF_SUBSUMER`).** For an enumerated
  class `X ≡ ObjectOneOf(a₁…aₙ)`, `X ⊑ C` is seeded when every member `aᵢ` is
  typed `C` (LHS ⊔-elimination). ORE `ore_ont_5107` MISSED 6→0.

## [0.3.12] — 2026-06-22

### Performance

Three FP-safe constant-factor wins in the hypertableau **wedge** matcher hot loop
(found by profiling; each sound and complete-preserving — closures **byte-identical
corpus-wide**, verified across the whole arc):

- **Matcher allocation reduction (`SmallVec`).** The per-call / per-recursion scratch
  in the wedge's clause matcher (`hyper.rs`) — `match_body` / `enumerate_matches`
  buffers (`role_atoms`, `other_classes`, `targets`), the `Binding` type itself (kills
  the per-match `clone`), and `eval_order`'s scratch — now stays inline instead of
  heap-allocating. Allocator self-time on the wedge hot path dropped from ~35% to ~1%.
- **Linear-scan label membership.** `HyperNode::has` was a binary search over a sorted
  label slice; profiling showed nodes carry only ~5 labels over a universe of hundreds
  to thousands, so the binary search was mostly branch-misprediction. Replaced with a
  branch-predictable linear scan + early-exit (labels stay sorted, same result).

Cumulative effect (vs the pre-arc baseline): **~10–19% faster on out-of-EL SROIQ
classification** (ore-10908 −18.6%, sio −13.9%, ore-15516 −13.2%) and **~2× on the
matcher-bound `family` inconsistency case** (125s → 63s, verified). Flat on EL
ontologies (they route to the saturator, untouched). A `FixedBitSet` label
representation was scoped and rejected by a profiling study (a dense per-node bitset
would bloat the wedge's frequent node-clones 2–65× on sparse-wide nodes — net-negative;
see `docs/wedge-label-bitset-p0-results.md`).

## [0.3.11] — 2026-06-22

### Added

- **Manchester syntax (`.omn`) input.** rustdl now *reads* OWL Manchester syntax,
  not just writes it — `classify`/`debug`/`diagnose`/`justify`/`repair`/`report`
  accept `.omn` files (CLI auto-detects by content sniff + extension; Python
  auto-detects by extension, or `classify_bytes(data, format="omn")`). Front-end
  only — no engine change, FP=0 structurally untouched. rustdl already rendered
  explanations in Manchester; input completes the symmetry. The reader is the
  conformance-tested `horned_owl::io::omn::reader` from the pinned fork rev (no
  dependency on the upstream PR merging).
- **Python QA tutorial** (`docs/python-ontology-qa.md`) — an end-to-end "diagnose
  and fix a broken ontology" walkthrough (classify → `debug()` → justify/repair →
  fix → read inferred facts), fully Manchester-faced and CI-guarded against rot.
  Linked from the main and PyPI READMEs.

[0.3.12]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.12
[0.3.11]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.11

## [0.3.10] — 2026-06-21

> Note: the changelog was not maintained for 0.3.2–0.3.9; this entry covers the
> user-facing additions landing in 0.3.10. See `CLAUDE.md` and
> `docs/superpowers/specs/` for the full engineering record of intermediate work.

### Added

- **Explanation & debugging suite** — rustdl is now an ontology-*debugging* tool, a
  niche the fast reasoners (Konclude has no built-in justification/explanation) don't
  serve. All sound by construction; read-only (FP=0 untouched).
  - `rustdl justify --laconic` — weakens each justification axiom to its responsible
    *fragment* (Horridge-style laconic justifications, sound structural weakening).
  - `rustdl diagnose` — partitions unsatisfiable classes into **root** causes vs
    **derived** collateral, and justifies the roots ("where to start fixing");
    detects global inconsistency too.
  - `rustdl repair` — minimal axiom-removal sets to break an unwanted entailment
    (Reiter diagnoses = minimal hitting sets over all justifications), each **verified**.
  - `rustdl report` — a self-contained HTML debugging report (summary + diagnose
    roots/derived + per-root justification + repairs), no external resources.
- **Inference materialization** (Python + reasoner API):
  - `materialize_inferred_property_assertions` — inferred **object** property
    assertions over named individuals (hierarchy / inverse / symmetric / role chains /
    transitivity); also CLI `rustdl realize --properties`.
  - `materialize_inferred_data_property_assertions` — inferred **data** property
    assertions (5-tuple incl. datatype + language tag).
  - `materialize_inferred_subobjectproperty_axioms` /
    `materialize_inferred_subdataproperty_axioms` — the inferred property-hierarchy
    closure (object: told + equivalent + inverse; data: told + equivalent).
  - `materialize_existential_successors` — a blank-node representation of named
    individuals' entailed existential successors (one row per entailed `a : ∃R.C`;
    *not* entailed ground triples — witnesses are model-relative).
- `rustdl.debug()` now returns a typed `Diagnosis` result object (attribute access +
  dict-compatible `Mapping`; `to_dict()` for JSON).

### Fixed

- **family inconsistency** is now detected (a consequence-based ABox-saturation
  pre-check) — the last open correctness gap.

[0.3.10]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.10

## [0.3.1] — 2026-06-05

### Added

- **Bundled example ontologies** for a zero-setup, offline demo:
  `rustdl.examples.pizza()`, `.sulo()`, `.sio()` return local paths to
  ontologies shipped *inside the wheel* (gzip-compressed, decompressed
  into a per-user cache dir on first use — no network, works behind a
  proxy / air-gapped). Companion `PIZZA_NS` / `SULO_NS` / `SIO_NS`
  namespace constants. `pizza()` is the SULO-aligned ontostart pizza
  (88 classes, classifies instantly and completely).

[0.3.1]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.1

## [0.3.0] — 2026-06-05

### Changed

- **Classification now bounds each subsumption test by default**
  (`pair_timeout_ms` / `per_pair_timeout_ms` defaults to **1000 ms**;
  previously unbounded). Pathological SROIQ ontologies (e.g. pizza) no
  longer hang by default. A timed-out pair is recorded as "not
  subsumed" — **sound** (never a false subsumption), but the result may
  be **incomplete**. Pass `0` for the complete, unbounded classification.
  1000 ms is the empirical knee on pizza: higher budgets buy no extra
  completeness (the remaining pairs are intractable at any reasonable
  bound) but cost proportionally more wall time.
- **Incompleteness is now surfaced loudly**, so a bounded run can't
  silently hand back a hierarchy missing real edges:
  - CLI `rustdl classify` prints a prominent `⚠ INCOMPLETE: N pair(s)
    exceeded the timeout …` warning to stderr when any pair times out.
  - Python `rustdl.classify` / `classify_bytes` emit an
    `IncompleteClassificationWarning` (filterable via the `warnings`
    module), and the `Classification` object exposes `.complete` (bool)
    and `.timed_out_pairs` (int).

### Added

- **CLI multi-format input.** `rustdl <cmd> file.{owx,owl,rdf}` now
  works — the reader is chosen by file extension (.owx → OWL/XML,
  .owl/.rdf → RDF/XML, .ofn/other → OWL Functional). Previously the CLI
  read OFN only; the Python bindings already auto-detected. Verified on
  the owlcs pizza ontology (RDF/XML) — 99 classes, 2 unsatisfiable
  (CheeseyVegetableTopping, IceCream), matching the canonical result.

## [0.2.2] — 2026-06-05

### Changed

- **PyO3 0.22 → 0.25.** Clears 35 build warnings: 28 from PyO3 0.22's
  macro-generated code tripping the `unsafe_op_in_unsafe_fn` lint
  (default-warn under Rust edition 2024) and 5 `gil-refs` cfg
  warnings — both fixed by PyO3's edition-2024-clean codegen. The only
  source change required was `get_type_bound::<T>()` →
  `get_type::<T>()`. `cargo build -p owl-dl-py` is now warning-free.
- Silenced 7 `dead_code` warnings on `ClashReason` (fields read only
  via the derived `Debug` impl for `RUSTDL_TRACE` output).

### CI

- **Linux aarch64 wheels now build on a native ARM runner**
  (`ubuntu-24.04-arm`) instead of QEMU emulation on an x86 host. Build
  time drops from ~50 min to ~4 min. QEMU setup step removed.

## [0.2.1] — 2026-06-05

### Changed

- **Python package documentation.** The `rustdl` PyPI page now ships a
  complete README: install, quick-start, full API reference (classify +
  Classification members, one-shot queries, inference materialization,
  exception hierarchy), and the soundness/coverage contract. The 0.2.0
  wheel shipped a four-line placeholder README; this release replaces it.
- Re-added the Linux aarch64 wheel to the release matrix (dropped in 0.2.0
  to speed up release-workflow iteration).

## [0.2.0] — 2026-06-04

### Added

- **Python bindings** (`rustdl` on PyPI). PyO3 + maturin. ABI3 wheel
  for Python 3.10/3.11/3.12/3.13. Top-level API one-to-one with the
  Rust public API (`classify`, `classify_bytes`, `is_consistent`,
  `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`,
  `instances_of`, `realize`) plus inference materialization helpers
  (`materialize_inferred_subclass_axioms`,
  `materialize_inferred_class_assertions`). Auto-detects OFN/OWX/RDF-XML
  format from file extension. 5-platform wheel matrix (Linux x86_64 +
  aarch64, macOS x86_64 + arm64, Windows AMD64) + sdist. PyPI publish
  via trusted publisher (OIDC, no token in CI).
- New GitHub Actions workflows: `python-ci.yml` (PR/dispatch gate) and
  `release-python.yml` (cibuildwheel + maturin publish on `v*.*.*` tag).

### Deferred to roadmap

- owlready2 / omny integration (separate brainstorm queued).
- Black-box `rustdl.explain(path, sub, sup)` axiom-justifications.
- `rustdl.Reasoner(path)` stateful class for batch queries.
- Native pyhornedowl `Ontology` pass-through.
- See the spec at `docs/superpowers/specs/2026-06-04-python-bindings-design.md`
  for the full deferred-feature list.

## [0.1.0] — 2026-06-04

First tagged release. The engine is sound on every measured workload
and competitive (or winning) against HermiT and Konclude on most.

### Added

- Sound OWL 2 DL (SROIQ) classifier with hybrid saturation+tableau
  orchestrator.
- Hypertableau wedge accelerator (default engine since 2026-05-29).
- Per-class label heuristic (Phase 7) — sound non-subsumption pruner
  via per-class wedge satisfiability.
- Cache-deadline decoupling (Phase 8) — independent deadline for the
  label-cache build, so SROIQ classes needing a few hundred ms of
  wedge satisfiability no longer get cut off at NoVerdict.
- Horn-shortcircuit fast path (Phase 2b) — Horn-fragment ontologies
  dispatch straight to saturation, skipping the per-pair tableau loop.
- ABox consistency check (Phase A1) — seven sound clash patterns:
  direct-Bot assertion, disjoint types per individual, NegOPA vs OPA
  with role-hierarchy propagation, SameAs ∩ DifferentFrom (transitive
  via union-find), Functional + two distinct witnesses,
  Asymmetric / Irreflexive violations, domain/range disjointness.
- Datatype preprocessing (D1–D5) — sound under-approximation for data
  axioms not directly supported; recognized patterns derived as TBox
  axioms (Functional + DataMin, DataMin > DataMax, DataPropertyDomain
  inference, SubDataPropertyOf transitivity,
  intersection-equivalence propagation, integer-range facet
  intersection).
- 9-corpus closure-diff regression harness — FP=0 invariant gated
  against Konclude on every commit.

### Performance

Compared with the May 2026 baseline:

- **GALEN**: 445 s → **0.49 s** (now beats Konclude — 0.24× ratio).
- **notgalen**: 1168 s → **0.78 s** (now beats Konclude — 0.35× ratio).
- **alehif**: 2.28 s → **0.16 s** (0.08× Konclude).
- **ORE-10908**: 17× Konclude → **3.1×** (under the ≤5× target).
- **sio-stripped**: 4.3× absolute wall improvement (still 13.6×
  Konclude — out-of-EL fragment, timeout-bound; see dead-end §18).

### Known limitations

- Data-axiom patterns outside the D4/D5 recognizers are silently
  dropped (sound under-approximation; missed positives possible).
- `HasKey` not supported (errors at parse time).
- SWRL rules silently skipped.
- Role chains of length > 2 error at parse time.
- family-class workloads need ABox saturation (open scoping target
  per dead-end §21).
- ore-15672 has a 3-class intrinsic intractability cluster — sub-model
  caching is the only known path (multi-month research-engineering;
  dead-end §18).

### Dead-ends documented

21 entries in [`docs/hypertableau-dead-ends.md`](docs/hypertableau-dead-ends.md)
covering soundness traps, perf optimizations that didn't materialize,
and design decisions that recon ruled out before implementation. The
ledger is the canonical record of "we tried X; here's what killed it."

### Soundness contract

FP=0 vs Konclude verified on every release. The closure-diff tests in
[`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`](crates/owl-dl-reasoner/tests/konclude_closure_diff.rs)
are the soundness tripwire — any change that introduces a false-positive
subsumption fails CI.

[0.3.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.0
[0.2.2]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.2
[0.2.1]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.1
[0.2.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.0
[0.1.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.1.0
