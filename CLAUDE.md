# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`rustdl` is a sound, complete OWL 2 DL (SROIQ) reasoner in Rust, targeting parity
with HermiT and Konclude on the ORE benchmarks. It is a Konclude-style **hybrid**:
a consequence-based EL **saturation** engine handles the cheap EL fragment, a
**tableau** engine handles the rest of SROIQ, and an **orchestrator** decides per
query which to use. Parsing and the OWL object model come from the external
`horned-owl` crate (LGPL-3.0; our code stays Apache-2.0/MIT).

## Commands

```sh
cargo build --workspace --release          # build (needs Rust 1.88+, edition 2024)
cargo test --workspace                      # all tests
cargo test --workspace --doc                # doctests (CI runs these separately)
cargo test -p owl-dl-tableau <name>         # single crate / filtered test
cargo fmt --all -- --check                  # format check (max_width = 100)
cargo clippy --workspace --all-targets --all-features -- -D warnings   # lint; warnings are errors
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy (`-D warnings`), build+test on
linux/macos/windows, and `cargo-deny`. `RUSTFLAGS: -D warnings` is set in CI, so
**any warning fails the build** — clippy `pedantic` is on workspace-wide (with a
curated allow-list in the root `Cargo.toml`), and `unwrap_used`/`dbg_macro` are
warn-level. The push trigger is currently disabled (billing); PRs and
`workflow_dispatch` still run CI.

Run the reasoner / benchmarks:
```sh
./target/release/rustdl classify path/to/ontology.ofn          # see README for all subcommands
./target/release/rustdl classify --saturation-only file.ofn    # EL-closure only (fast under-approx)
./target/release/rustdl classify --pair-timeout-ms 200 file.ofn # per-pair tableau deadline
./target/release/owl-dl-bench corpus ontologies/real --repeats 5
./target/release/rustdl explain file.ofn <sub> <sup>           # which engine answered: closure/wedge/tableau
./scripts/fetch-real-ontologies.sh                             # corpus is gitignored, pulled on demand
```

`explain` is the go-to tool for diagnosing why the classifier missed a pair.
Beware: hard SROIQ pairs (InterestingPizza- / PathologicalCondition-style) can
take minutes via the full tableau — never probe them in a loop without timeouts.

Diagnostics: `RUSTDL_TRACE=1` (one stderr line per search/branch decision; always
compiled in, off-path is one atomic load), and `RUSTDL_COUNTERS=1` with
`--features counters` (per-rule call counts dumped on `TableauContext::drop`).

## Workspace architecture

Data flows: `horned-owl` parse → `owl-dl-core` (IR + preprocessing) →
`owl-dl-reasoner` orchestrator → dispatches to `owl-dl-saturation` and/or
`owl-dl-tableau`.

- **`crates/owl-dl-core`** — the shared IR and all preprocessing. Concept
  expressions are interned in a `ConceptPool` so structural equality is O(1)
  integer comparison (`ir.rs`) — **this interning invariant is load-bearing for
  the tableau hot loop**. Key passes, in pipeline order: `convert.rs`
  (horned-owl → `InternalOntology`), `normalize.rs` (NNF), `absorb.rs`
  (turns GCIs into lazily-fired `ConceptRule`/`NominalRule`/`RoleRule` triggers
  so the tableau doesn't apply every axiom universally), `told.rs` (told-subsumer
  + told-disjoint tables, transitively closed at build), `locality.rs` +
  `model_cache.rs` analyses. `convert_back.rs` reverses IR → horned-owl.
  `disjunction_existential.rs` (run in `convert_ontology`) derives
  `X ⊑ ∃R.C` from `X ⊑ ∃R.(D₁ ⊔ … ⊔ Dₙ)` when the atomic disjuncts share
  a minimal common told-subsumer C — a sound under-approximation that
  feeds the EL saturator a case-split it otherwise drops. **Closed SIO's
  last 2 MISSES → full corpus parity (FP=0, MISSED=0 across all 9
  fixtures).** See `docs/sio-disjunction-results.md`.

- **`crates/owl-dl-saturation`** — single-file consequence-based EL engine
  following ELK (Kazakov et al., JAR 2014). One fixed-point loop computing the
  subsumer closure over atomic classes: told subsumption, conjunctive triggers,
  CR5 existential propagation, CR9 role hierarchy, length-2 role chains +
  transitivity, domain/range, Tseitin introduction for compound `∃` bodies,
  Bot detection. **Sound but only complete for the supported EL fragment.**
  EL++ functional-role witness-merge (Phase 2a) for sibling sub-properties
  of a functional role — atom-set accumulation design (T4.5) terminates
  by construction. Sound; tested via synthetic canaries; corpus-impact
  on GALEN currently 0 (see `docs/phase2a-results.md` for the falsification).
  Phase 2b + 2b.5 (commits 022ca50 + b64d331) fixed two compound
  existential-body lowering gaps: nested-existential markers in Tseitin
  bodies now emit equivalent (two-way) semantics; the LHS-And arm now
  correctly handles non-atomic existential RHS. Recovered 92 of GALEN's
  109 MISSED (~84%); FP=0 held. See `docs/phase2b-results.md`.
  Phase 2c attempted sub-role witness propagation; reverted at 0/44
  recovery, see `docs/phase2c-results.md`.
  Phase 2d + 2c-redux (commits b78c5fd + 34a2b62) layered two changes:
  (2d) materialize inherited existential facts on subclasses at
  `process_subsumer` and `push_fact` time, and (2c-redux) re-apply the
  sub-role witness-propagation rule reverted at cc2019e — now fires
  because Phase 2d populates `facts_by_sub[X]`. **GALEN MISSED 17 → 0
  (full parity with Konclude); notgalen MISSED 27 → 18 (9 IPBP-cluster
  pairs recovered).** Wall cost: GALEN +6.5%, notgalen +2.7%. FP=0
  held throughout. Resolves dead-end §15. See
  `docs/phase2d-2c-redux-results.md`.
  Phase 2e (commit 883bc2f) closed notgalen's residual 18. The
  witness-merge back-prop skipped the merge-*triggering* sub-role
  (`other.role == fact.role`), so the merged synthetic never reached
  the sub-role an existential body lives on when that role's fact was
  processed second — an order-dependent miss (GALEN hit the good
  order; notgalen's equiv-vs-subclass structure hit the bad one).
  Dropping the skip is sound by functionality of `R_f` (every sub-role
  witness coincides with the single `R_f`-successor carrying the merged
  atom set). **notgalen MISSED 18 → 0 (full Konclude parity, closure
  32739=32739); GALEN stays 0; FP=0 across the whole corpus.** The
  only remaining corpus MISS is SIO's 2 (out-of-EL). Canary
  `functional_role_merge_body_on_sub_role`. See `docs/phase2e-results.md`.
  Nominal lever (2026-06-06): the EL fold now handles nominal-filler
  existentials (`∃R.{a}`, i.e. `ObjectHasValue`) by mapping each
  individual to an opaque per-individual synthetic class (NomKey) at the
  lowering chokepoint (`atomic_or_tseitin_body_with_extras`), plus
  transitive-ABox propagation (`build_abox_nominal_reach`): `X ⊑ ∃R.{a}`,
  `a R⁺ b` (R transitive) ⟹ `X ⊑ ∃R.{b}`. Sound (1:1 individual identity;
  propagation gated on role transitivity; nominal singleton/cardinality
  semantics deliberately unmodeled — under-approximation). Closed the
  wine region/color cluster: **wine MISSED 57 → 34, FP=0 across all 10
  fixtures**. Residual 34 = grape (`≤1` cardinality) + sugar (`∀`+nominal
  set), deferred. Canary `nominal_transitive_abox_fold_classifies`. See
  `docs/nominal-lever-scoping-2026-06-06.md`.

- **`crates/owl-dl-tableau`** — SROIQ tableau. `CompletionGraph` (`graph.rs`)
  of label-carrying nodes; `TableauTrail` (`trail.rs`) gives log-and-undo
  backtracking via `Checkpoint` markers; **`TableauContext` is the only
  sanctioned mutation interface** — every label/edge/node/merge change goes
  through it and is recorded on the trail. `rules.rs` holds the deterministic
  completion rules; `search.rs` is the backtracking driver for the `⊔` rule with
  dependency-directed back-jumping (each disjunction has a `branch_id`; clashes
  carry a `DepSet` so siblings are skipped when the branch didn't contribute).
  `hyper.rs` is the hypertableau engine (Horn hyperresolution + disjunctive
  branching + double-blocking) and is **wired in as the default accelerator
  "wedge"** since 2026-05-29 — the in-tree `hyper.rs` docstring calling it
  standalone/not-wired is stale; trust the `*_enabled()` defaults in
  `reasoner/src/lib.rs`.
  Phase 3 (commit 64bee92) added a bloom prefilter to `needs_deferred_or`
  extending the existing 64-bit `label_sig` (was used only for ancestor
  pair-blocking). GALEN classify wall: 24.7 min → 21.1 min (−14.6%);
  verdicts unchanged. See `docs/phase3-results.md`.
  Phase 3b (commit cf05e22) replaced `are_declared_inverses`'s O(N) linear
  scan with an O(1) hashbrown::HashSet lookup. SIO flamegraph deltas:
  `are_declared_inverses` 25.76% → 3.44%; `apply_max` 27.93% → 6.51%
  (7.5× reduction on inverse-lookup path). FP=0 + verdicts unchanged.
  See `docs/phase3b-results.md`.
  Phase 3d (commit 32aeda6) hoisted the linear-scan fallback in
  `apply_deferred_concept_or_rules` out of the per-trigger loop behind
  a single top-of-function `concept_rules_by_trigger.is_empty()` gate.
  SIO `apply_deferred_concept_or_rules` top-frame attribution 18.16% →
  3.23% (−14.93pp); GALEN classify wall 12.43 min → 11.87 min (−4.5%).
  FP=0 + MISSED=17 unchanged. See `docs/phase3d-results.md`.
  Phase 3e attempted edge-keyed role-rule indexing on `apply_role_rules`;
  reverted (commit a2a4d7f) at +2.34% GALEN wall regression despite a
  SIO flame win (16.36% → 8.87%, −7.49pp) — workload-dependent
  break-even where HashMap-lookup overhead exceeds saved
  `edge_satisfies` cost on edge-heavy / rule-thin patterns. See
  `docs/phase3e-results.md` and dead-end ledger §16.

- **`crates/owl-dl-core`** — Phase 3c (commit 0b5ed36) cached
  `ConceptPool::bot_id` via `OnceLock<ConceptId>` (concurrency-safe;
  `ConceptPool` is Sync across rayon workers). Eliminates the 24.66%
  `apply_role_axioms` / `bot_id` / `find_map` cluster on GALEN+SIO
  flamegraphs. **GALEN classify wall: 24.8 min → 12.2 min (~50%
  reduction)** — this reclaims Phase 2b's full wall regression.
  FP=0 + verdicts unchanged. See `docs/phase3c-results.md`.

- **`crates/owl-dl-reasoner`** — public API + orchestrator (`lib.rs`,
  `classify.rs`, `realize.rs`). Every entry point that issues a tableau query
  first runs saturation and short-circuits on a hit; if the whole ontology is in
  the EL fragment it takes a saturation-only fast path (`stats.pure_el_mode`).
  `PreparedOntology::from_internal` snapshots the post-NNF/absorb/ABox-seed state
  **once** so the O(n²) pairwise classify loop reuses it across pairs; the loop
  runs in parallel via rayon. `is_subclass_of` reduces to satisfiability of
  `sub ⊓ ¬sup`. Phase 4b (commit e31439c) added a `FragmentClassification`
  diagnostic surfaced as `# fragment: …` in the CLI banner and
  `ClassificationStats::fragment` programmatically; it tells users whether
  `trust_sat` is sound by construction or by composition (corpus-validated).
  Phase 4c extended this to three states: `PureEl` / `Horn` / `OutOfFragment`,
  with `Horn` detected via `clausify_with_stats` (`stats.disjunctive == 0 &&
  stats.deferred == 0`). Both `PureEl` and `Horn` are sound-by-construction —
  the saturator carries `PureEl`, the hyper Horn fixpoint carries `Horn`.
  Diagnostic-only — no default-behaviour change. See
  `docs/fragment-completeness.md`.
  Phase 6 added a `visited: Vec<bool>` bitset to the top-down
  `find_direct_parents_top_down` walk so the dense GALEN subsumer
  lattice doesn't re-visit candidates reached via multiple parents.
  GALEN classify wall (under contention): 753.96 s → 684.00 s
  (−9.3 %). Net of the Phase 2d +6.5 % regression, the wall is now
  below the pre-Phase-2d baseline while preserving all completeness
  gains (closure = 27 997 = Konclude, FP=0 / MISSED=0). See
  `docs/phase6-results.md`.
  Phase 7 shipped a HermiT-style per-class label heuristic: a
  `Vec<LabelOracle>` cache is built once at classify-time from
  per-class wedge satisfiability, and the orchestrator skips
  `subsumes_via_tableau` when `D ∉ labels(C)` (sound counterexample-
  model). `RUSTDL_LABEL_HEURISTIC` env gate (default ON) provides
  opt-out for tests exercising the wedge directly. **GALEN classify
  wall 684 s → 455.73 s (−33 %) under contention**, far beyond the
  ±10 % non-regression tolerance the plan set — the heuristic
  short-circuits wedge `NotSubsumed` calls that Phase 5 T3b had
  attributed under `hyper_refuted_pairs` (not `tableau_subsumption_calls`).
  ORE-10908 27.37 s → 19.32 s (−29 %); ORE-15672 flat; small workloads
  −7 % to −25 %. Prune rates 96–100 % across all measured ontologies.
  FP=0 / MISSED=0 preserved across Phase 0 net + GALEN. Konclude-class
  ≤5× ratio not reached on SROIQ (ORE-10908 closed 17× → 12×). See
  `docs/phase7-results.md`.
  Phase 8 (commit `30b641c`) decoupled the label-cache deadline
  from per_pair_timeout — the ~5% SROIQ classes that need a few
  hundred ms of wedge satisfiability no longer get cut off at
  NoVerdict. ORE-10908 19.32 s → 7.48 s (−61 %), Konclude ratio
  12× → 4.32× (then 3.1× post-Horn-shortcircuit per
  `docs/perf-2026-06-04-konclude-vs-rustdl.md`). See
  `docs/phase8-results.md`.
  Phase A1 (commit `6e63c28`) added a sound ABox-driven inconsistency
  pre-check at `crates/owl-dl-reasoner/src/abox_check.rs`. Runs before
  the tableau in both `is_consistent` and `classify`; on a positive
  verdict, classify mirrors Konclude's behaviour (every class marked
  unsatisfiable). Eight clash patterns: P1 direct-Bot, P2 disjoint
  types per individual, P3 NegOPA-vs-OPA (with role-hierarchy
  propagation), P4 SameAs∩DifferentFrom (transitive via union-find),
  P5 Functional + two-distinct-witnesses (+ inverse-functional), P6
  Asymmetric/Irreflexive, P7 domain/range disjointness (stretch), P8
  functional-collapse (`Functional(R)` + individual implies `∃R.q1 ⊓
  ∃R.q2` with `q1,q2` told-disjoint → ⊥; uses inverse-derived
  domain/range so `isFatherOf`/`isMotherOf`-style inverse roles
  contribute types). Note: P8 catches the *shallow* functional-collapse
  pattern but does NOT close the family/family-stripped headline target —
  that inconsistency is a deep multi-step graph entailment (tableau-scale,
  not a pre-check pattern); see `docs/abox-consistency-check-handoff.md`. All
  16 synthetic unit tests pass; FP=0 preserved across every corpus
  closure-diff (alehif, ore-10908, ore-15672, shoiq-knowledge, sio,
  ro, sulo, galen, notgalen). Env gate `RUSTDL_ABOX_CHECK=0` reverts
  to pre-A1 tableau-only behaviour. GALEN classify wall unaffected
  (~0.58 s, within noise of `=0`) via `has_abox_axioms()` skip of
  `PreparedOntology` build on ABox-free inputs. **Stretch goal
  not met**: family / family-stripped (both HermiT/Konclude-
  inconsistent in <1 s) still timeout — their clash needs functional-
  role-merge of `∃hasSex.Female ⊓ ∃hasSex.Male`, beyond P7's range
  augmentation. Next scoping target documented at
  `docs/abox-consistency-check-handoff.md`. Spec:
  `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`.

- **`crates/owl-dl-datatypes`** — concrete-domain reasoners. Scaffolded,
  **not yet wired into reasoning.** Data axioms / data ranges that
  are NOT recognized by the D4 preprocessing pass (see below) are
  silently dropped at conversion time (Phase D1, commit `e34aeb6`):
  sound under-approximation. Corpus-validated near-Konclude parity
  on the data-axiom-bearing fixtures (shoiq-knowledge: 0 MISSED of
  449; sio: 2 MISSED of 8904, both from existing disjunction-
  reasoning gaps unrelated to data). Tier C (concrete-domain
  ranges, datatype facets like xsd:integer min/max value) deferred
  until a future workload exposes a real completeness gap.

  **Phase D4 (commit `eb15c74`)** added a preprocessing pass at
  `crates/owl-dl-core/src/data_axioms.rs` that scans horned-owl
  Components for specific patterns and emits derived class axioms.
  Currently recognized:
  - `Functional(dp) + SubClassOf(C, ≥n dp)` with `n≥2` → `C ⊑ Bot`
  - `SubClassOf(C, ≥n dp) + SubClassOf(C, ≤m dp)` with `n>m` → `C ⊑ Bot`
  - `DataPropertyDomain(dp, D) + C ⊑ DataSome(dp, _)` → `C ⊑ D`
  - SubDataPropertyOf transitivity (`C ⊑ DataSome(specific) +
    DataSome(general) ⊑ D` → `C ⊑ D`, hierarchy closure)
  - Intersection-equivalence propagation: `C ≡ M1 ⊓ M2 ⊓ ...`
    inherits each Mi's data-cardinality bounds (fixpoint).

  Companion saturator change: `ElRules::directly_unsat` field +
  seed-time `enqueue_unsat` so the saturator picks up
  `Atomic ⊑ Bot` axioms (which `atomic_operands_on_right(Bot, _)`
  silently lost pre-D4).

  **Phase D5 (commit `2804cfa`)** added Tier C: integer-range facet
  preprocessing. New `IntegerRange` type with closed-form intersection;
  parses `xsd:integer` `DatatypeRestriction` facets (`minInclusive`,
  `minExclusive`, `maxInclusive`, `maxExclusive`). New pattern:
  `Functional(dp) + ≥2 integer ranges on (C, dp) with empty
  intersection` → `C ⊑ Bot`. Other numeric datatypes (xsd:decimal,
  xsd:double, xsd:dateTime) extend with their own range types but
  share this preprocessing's algebra.

  Synthetic test harness: `crates/owl-dl-reasoner/tests/datatype_completeness.rs`
  (6 fixtures under `tests/fixtures/datatype/`; all 6 pass post-D5).
  Tests are `#[ignore]`d; invoke with `cargo test ... -- --ignored`.

- **`crates/owl-dl-cli`** (`rustdl` binary) and **`crates/owl-dl-bench`**
  (`owl-dl-bench`: `classify`/`sat`/`synthetic-el`/`corpus`/`compare-whelk`).
  `xtask/` holds build automation (corpus fetch, license inventory).

## Soundness contract (important)

Everything is **sound** — no false-positive subsumptions on any measured
ontology (FP=0 vs Konclude). Completeness is the subtle part:

- The hypertableau **wedge** is the default accelerator, gated by three env
  flags that all **default ON** (since 2026-05-29): `RUSTDL_HYPERTABLEAU`,
  `RUSTDL_HYPER_DOUBLE_BLOCK`, `RUSTDL_HYPERTABLEAU_TRUST_SAT` (set any to `0`
  to disable; see `hyper_*_enabled()` in `reasoner/src/lib.rs`).
- With `trust_sat` on, the wedge concludes "not subsumed" from its own `Sat`
  verdict **without consulting the tableau**. That is sound only if the engine
  is complete on the workload — empirically true across the corpus, but it
  **can MISS** subsumptions the full tableau would find (e.g. notgalen 18 MISSED,
  SIO 2; see `docs/handoff-2026-06-03-snapshot-cache-project-complete.md`). So the practical default classifier is a
  sound, near-complete-but-not-guaranteed-complete approximation, **not** the
  textbook sound-and-complete reasoner. Set `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`
  for the slower, more complete behaviour (`Sat` → fall through to tableau).
  Phase 1 added an opt-in `RUSTDL_HYPER_TRUST_SAT_MIN_MS` env var that
  distrusts a wedge `NotSubsumed` verdict returned in < threshold ms
  and tableau-verifies it instead. **Default 0 (disabled)** —
  the empirical sweep (`docs/phase1-results.md`) showed wall-time is
  not a usable filter at this resolution. Set the var to a positive
  integer to opt in.
- `--saturation-only` and `--pair-timeout-ms` are also sound under-approximations
  (every reported subsumption holds; positives may be missed).
- **`RUSTDL_SNAPSHOT_CAPTURE` defaults OFF as of 2026-06-08 — SOUNDNESS
  FIX (was default-ON in Phase 1c).** The per-class snapshot cache is
  FP-unsound on the non-Horn fragment: replay trusts ONE satisfying
  model, but on non-Horn `sup ∈ that-model ≠ sub ⊑ sup` (the A1
  analysis, `docs/reuse-trap-A1-scoping-2026-06-08.md`). Its
  `BackPropRisk::Safe` gate excludes inverse/nominal/cardinality but
  **NOT disjunction**, so a disjunctive inv/nom/card-free ontology
  passes as Safe and the cache emits spurious subsumptions — ORE 2015
  surfaced this (`ore_ont_13723` etc.: 30+ FP each vs a Konclude∩HermiT
  oracle, silently, no incompleteness signal;
  `docs/perf-2026-06-08-konclude-vs-rustdl.md`). And its only *sound*
  domain (Horn, canonical model) is already taken by the
  Horn-shortcircuit, so it has no sound active domain. Now opt-in
  (`=1`) for A/B only. Verified: the flip fixes the ORE FP and leaves
  the tuned corpus byte-identical at FP=0/MISSED=0. `RUSTDL_SNAPSHOT_LAZY`
  is moot while capture is off. See
  `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- **New as of Phase 2b**: `RUSTDL_HORN_SHORTCIRCUIT` defaults ON.
  For ontologies classified as `Horn` fragment (`analyze_fragment`
  returns `Horn` — i.e., clausifier produces only Horn clauses with
  no deferred axioms), classify dispatches to the saturation-only
  fast path instead of the per-pair verification loop. Sound by
  composition: the hyper Horn fixpoint is complete on Horn, so the
  saturation closure IS the full classification. Set
  `RUSTDL_HORN_SHORTCIRCUIT=0` to revert to the Phase 1c per-pair
  loop for Horn fragments. Massive wall savings on Horn workloads:
  GALEN 161.95 s → 0.40 s (~405×), notgalen 366.25 s → 0.69 s
  (~531×), alehif 1.63 s → 0.09 s (~18×); out-of-EL fixtures
  unchanged. See
  `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §5
  + `docs/phase2a-recon.md` + `docs/phase2b-snapshot-results.md`.

- **New as of 2026-06-06**: `RUSTDL_PRECISE_CARD_DEPS` defaults ON.
  At the wedge's `≤n` cardinality-clash pre-check, reports a sound
  over-approximation of the clash's dependency set
  (`parent.at_most_dep ∪ ⋃(birth ∪ label of succs) ∪ parent(birth ∪
  label)`) instead of the conservative `DepSet::ALL`, unblocking
  dependency-directed backjumping on cardinality clashes. **Sound by
  construction** (superset ⟹ backjumping never under-reports; four
  contributors proven, guarded by own-successor / `≠`-only / merge-taint
  fallbacks — see `card_clash_deps` + `docs/backjump-reconcile-2026-06-06.md`).
  The `solve_at_most` partition-exhaustion site is deliberately NOT
  narrowed (kept `DepSet::ALL`). Recovered wine MISSED 34→31
  (algorithmic, budget-independent), FP=0 across
  wine/ore-10908/ore-15672/shoiq-knowledge/sio/alehif; **perf-neutral**
  (the precise-card-deps flip does not move walls — A/B flat corpus-wide,
  GALEN Horn exactly flat; an earlier "−25% wall" figure was a single-run
  host-load artifact, retracted — see
  `docs/perf-2026-06-06-konclude-vs-rustdl.md`); inert on the EL/Horn
  corpus (Horn-shortcircuited). Set `RUSTDL_PRECISE_CARD_DEPS=0` to revert. Verdict-preservation regression tests:
  `precise_card_deps_preserves_{unsat,sat}_verdict` in `owl-dl-tableau`.

When changing the saturation/wedge engines or caches, the failure mode that
matters most is an unsound *positive*. See `docs/handoff-2026-06-03-snapshot-cache-project-complete.md` and `docs/abox-consistency-check-handoff.md` for
current engine state, characterized MISSED, open levers, and dead-ends;
`docs/model-caching-plan.md` / `docs/moms-plan.md` explain why model caching is
a deliberately un-integrated Phase-1 stub.

## Where to read more

`docs/` is the design record. Start with `architecture-roadmap.md` (levers to
close the SROIQ gap to HermiT + dead-ends already measured),
`owl-dl-reasoner-rust-strategy-v2.md` (full strategy), and the
`hypertableau-*-scoping.md` series for the in-progress hypertableau work.
`docs/perf-2026-06-08-konclude-vs-rustdl.md` has the current head-to-head vs
Konclude across the corpus (**native Konclude binary** — supersedes the 06-03/04
docs whose ratios used docker walls inflated by ~1.5 s container startup; on native
walls Konclude wins on every real-reasoning ontology, 2.2×–809×, and rustdl's
out-of-EL numbers are incomplete/DNF — the "beats Konclude"/"ORE-10908 ≤5×" claims
were docker artifacts). Performance claims in docs are backed by the corpus harness
— re-measure with `scripts/bench-rustdl-modes.sh` rather than trusting stale numbers.
