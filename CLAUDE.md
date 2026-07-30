# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`rustdl` is a **sound** OWL 2 DL (SROIQ) reasoner in Rust — **sound and complete
on the curated corpus** (as of 2026-07-12: galen, its last holdout, now
classifies with MISSED=0/FP=0 — down from 10 after the 2026-07-11 incremental
functional/≤1-merge fix closed 9, and the 10th closed by the 2026-07-12
label-cache back-fold — see
`docs/known-limitations/galen-inverse-functional-completeness.md` and
`docs/known-limitations/galen-defined-class-monotonicity-residual.md`; the
provable guarantee remains scoped to the EL/Horn fragment, and completeness on
the broader ORE/BioPortal tiers is untested), targeting parity
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

> **Toolchain gotcha (build with `RUSTUP_TOOLCHAIN=stable`).** `rust-toolchain.toml`
> pins `1.95.0`, but that toolchain is often installed *without* the `cargo`
> binary (rustup `profile = minimal`), so a bare `cargo build`/`cargo test` in the
> repo fails with *"the 'cargo' binary … is not applicable to the '1.95.0'
> toolchain"* — and a failed/again-skipped build then **silently reuses a stale
> `target/release/` binary**. Build and benchmark with
> `RUSTUP_TOOLCHAIN=stable cargo …` (or `rustup component add cargo --toolchain 1.95.0`).
> **Always confirm `target/release/rustdl` is freshly built before benchmarking** —
> a ~2-week-stale binary produced a spurious `wine` DNF and inflated hard-case walls
> (2026-07-11). NOTE (re-measured 2026-07-23): the historical "`wine` ~1.8 s" figure
> does NOT reproduce on current hardware/config — `wine` classify **DNFs
> unbounded** and completes only under a per-pair budget, with wall ~linear in the
> budget and completeness flat (`--pair-timeout-ms 25` → ~12 s / 201 subs; `50` →
> ~21 s; `100` → ~41 s; `200` → ~80 s / 203 subs). Its cost is a fixed set of hard
> SROIQ pairs each stalling the full budget (the per-pair wedge search), not a
> stale-binary artifact — so use `wine --pair-timeout-ms 25` as the freshness
> canary, and do not treat a >10 s `wine` as necessarily stale.
> **AMENDED 2026-07-30 — "DNFs unbounded" is too strong on this host.** Unbounded
> `classify ontologies/real/wine.ofn` **completed** in ~5 min with **201 subsumptions**
> (the same count the `--pair-timeout-ms 25` row reports), observed twice on
> independently built binaries (pre-change `main` and a feature branch) during a
> byte-identity run on the 32-core/251 GB host. So unbounded wine is *slow*
> (~300 s), not non-terminating — consistent with "wall ~linear in the budget"
> extrapolated to no cap. Treat the DNF wording as host-dependent; the freshness-canary
> advice above is unaffected.

CI (`.github/workflows/ci.yml`) runs fmt, clippy (`-D warnings`), build+test on
linux/macos/windows, and `cargo-deny`. `RUSTFLAGS: -D warnings` is set in CI, so
**any warning fails the build** — clippy `pedantic` is on workspace-wide (with a
curated allow-list in the root `Cargo.toml`), and `unwrap_used`/`dbg_macro` are
warn-level. Push (to `main`), PRs, and `workflow_dispatch` all trigger CI
(the 2026-05 billing freeze is resolved). The Python suite
(`.github/workflows/python-ci.yml`) likewise runs on push as of 2026-06-22 —
re-enabled after a `test_stubs` regression reached `main` and surfaced only in
the v0.3.11 release wheel build.

> **CI GREEN DOES NOT IMPLY FP=0 (2026-07-29).** The `closure-diff soundness net`
> job in `ci.yml` is `workflow_dispatch`-only *and* its fixtures are never
> provisioned (its own comment: "provisioning is out of Phase 0 scope"), so
> **rustdl's FP=0 corpus gate has never run in CI** — it is a documented stub.
> Green CI means fmt + clippy + unit tests on three platforms; it says nothing
> about soundness. **Run `./scripts/run-soundness-diff.sh` locally before merging
> any change to the fragment gates (`is_pure_el` / `saturator_complete_fragment`),
> to unsat derivation in `owl-dl-saturation`, or to conversion/normalization in
> `owl-dl-core`.** It takes ~4 min, needs ~12 MB of gitignored fixtures
> (`./scripts/fetch-real-ontologies.sh`), and diffs each closure against a
> committed Konclude/HermiT oracle. Grep the run for `^\[fp0\]` to get a
> VERIFIED / NOT VERIFIED manifest. A missing REQUIRED fixture now FAILS with the
> path plus the fetch hint; it used to pass silently — of 22 fixture blocks, 9
> could skip while the suite still reported `ok`, and
> `shoiq_knowledge_closure_matches_konclude` verified nothing at all. Reference
> values on the fixtures present as of 2026-07-29, all FP=0/MISSED=0: galen 27997,
> notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247,
> ro 158, ore-15672 142, sulo 51, bibtex 16. Still NOT VERIFIED for want of
> fixtures: `shoiq-knowledge`, `ro-stripped`, `sulo-stripped`, `sio-stripped`,
> `family-stripped` — promote them to REQUIRED in `konclude_closure_diff.rs` once
> the fixtures are obtainable.

> **BRANCH BACKUP LIVES IN A NON-BRANCH REF NAMESPACE (2026-07-29).** `origin` has
> deliberately only **5** branches (`main` + `feat/complex-class-expression-queries-48`,
> `feat/explanation-surface`, `feat/pseudo-model-realize`,
> `feat/surface-dropped-axioms-43`) — the shared repo's branch list is kept clean on
> purpose. All **45** local branches are nonetheless backed up server-side under
> `refs/archive/heads/*`, which does **not** appear in GitHub's branch list, the PR base
> picker, or `git branch -r`.
>
> ```sh
> git push origin 'refs/heads/*:refs/archive/heads/*'          # re-sync the backup
> git ls-remote origin 'refs/archive/heads/*'                  # list what is backed up
> git fetch origin 'refs/archive/heads/X:refs/heads/X'         # restore ONE branch
> git fetch origin 'refs/archive/heads/*:refs/heads/*'         # restore ALL
> ```
>
> **The backup is a snapshot, not a mirror** — it does not update itself. Re-run the push
> after creating or advancing any branch you care about. `git branch -d` still will not
> protect an unmerged branch locally, so consult `git ls-remote origin
> 'refs/archive/heads/*'` before deleting one.
>
> Several branches are **deliberately parked, not stale** — check the memory/handoff record
> before pruning: `feat/cb-alch-taming` (CB arc, park record in its own tree),
> `feat/cb-b1-integration` (the retired CB engine the above resurrected from, 17
> commits), `feat/model-derived-realize` ("dormant-safe" NO-GO),
> `feat/abox-sat-A-gated` (kept for a future bake-off).
>
> **`git fetch --prune` / `git remote prune origin` is dangerous here.** Many
> remote-tracking refs are stale (branch deleted on the server) while still holding
> commits that are in no local branch; pruning them makes those commits unreachable.
> Six such sets were found and anchored as `archive/*` tags (21 commits:
> `feat-corpus-wine-datatype` 9, `docs-sub-tableau-caching-scoping` 3,
> `feat-sio-disjunction-common-subsumer` 3, `feat-wedge-semantic-branching` 3,
> `feat-phase2e-functional-superrole-merge` 2, `chore-blocking-observability` 1).
> Those tags **are pushed to `origin`** (2026-07-29), so those 21 commits are safe from
> both a local prune and a disk failure. Before any future prune, re-check for NEW
> stale-ref/unique-commit pairs — the check is not once-and-done:
> `for r in $(git for-each-ref --format='%(refname:short)' refs/remotes/origin); do … done`
> comparing against `git ls-remote --heads origin`.
>
> Six worktrees remain under `.claude/worktrees/`; four hold unique CB-engine commits,
> one (`agent-ab06fe71b6797f234`) has **uncommitted** changes to `justify.rs` /
> `data_axioms.rs` and two test files, and one is `feat/cb-b1-integration`.
> `git worktree remove` on the dirty one destroys work git cannot recover.
> Full inventory: `docs/handoff-2026-07-29.md` §4a.

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
  **Blocking (2026-07-23, issue #35 v3):** the main tableau supports both
  ancestor-scoped pair-blocking (`is_blocked_ancestor`) and anywhere-blocking
  (`is_blocked_anywhere`, Motik/Shearer/Horrocks). `reasoner::decide` now enables
  **anywhere-blocking on the deadline-FREE query paths** (`is_consistent` /
  `is_class_satisfiable` / un-timed `realize`/`instance`) and keeps
  ancestor-blocking on the deadline-bounded paths (classify pairs, timed realize).
  Rationale: ancestor-only pair-blocking cannot block a generating `∃`-cycle
  anchored at a **nominal** root (the pairwise parent-subset condition never holds
  near the anchor), so a `{a} ⊓ ¬C` probe over a defined-class + covering-
  disjunction + property-domain ontology with an ABox edge grows the completion
  graph near-unbounded (issue #35 `hang_v3`: realize ~70 s → 0.04 s with the fix,
  verdicts matching HermiT). A 152-ontology ORE + curated-corpus bake-off found
  anywhere-blocking verdict-identical to ancestor-blocking (byte-identical
  closures, 0 panics, no reproducible wall regression), so classify is left on
  ancestor-blocking only to avoid perturbing the tuned hot loop, not for
  correctness. Env `RUSTDL_ANYWHERE_BLOCKING=1` forces it on everywhere (incl.
  classify), `=0` forces ancestor-only everywhere (pre-fix behaviour). Complements
  the 0.3.34 deep-cap fix (search-breadth on bounded graphs); this addresses
  graph-termination on the nominal-anchored-cycle class.
  **Realize termination — issue #35 v4 (2026-07-23).** A *new* pattern still hung
  `realize` on 0.3.38: `ObjectMinCardinality` + `ObjectOneOf` covering +
  `ObjectPropertyDomain` (no ABox). Root cause: `ObjectPropertyDomain(r,A)` absorbs
  to an untriggered residual `⊤ ⊑ ¬∃r.⊤ ⊔ A`; picking its `A` disjunct on a fresh
  `≥2 r.C` witness re-opens the covering nominal disjunction and the o-rule folds
  the witness back into the constraint owner → unbounded generation, and blocking
  can't cut it (every cycle node is nominal-tainted, excluded from
  `is_blocked_anywhere`/`_ancestor` at `lib.rs:1021/1062`). **Fix shipped = a sound
  deterministic safety net, NOT a completeness fix:** (1) `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`
  now **defaults to 750 ms** (was unbounded since 0.3.18; `=0` opts out) — bounds each
  per-individual realize probe → sound MISS; realize-only, never classify/consistency.
  (2) `RUSTDL_MAX_NODES` (default 50000, `0` disables) caps the deadline-free tableau
  search → a distinct `NodeCap` verdict → `Ok(None)` (sound MISS / consistent
  under-approx) with a hard early-return in `search::branch`. Realize on the reproducer:
  >300 s hang → ~0.75 s. The intended *complete* fix (`RUSTDL_NOMINAL_FIRST`
  nominals-first scheduling) was **falsified** (scheduling can't bound this
  domain-residual cycle) and is **default OFF / opt-in**, dormant scaffolding for a
  future nominal-aware-blocking / NN-rule redesign. HermiT-matching realization on
  this pattern is a known limitation. See
  `docs/superpowers/specs/2026-07-23-nominal-cardinality-realize-termination-design.md`
  (§ Outcome).
  **Wedge classify throughput (2026-07-23).** (v0.3.38) `HyperEngine::is_blocked`
  no longer clones the parent-role candidate bucket per call (iterate in place,
  stats in loop-locals) — behaviour-identical. (v0.3.39) The classify subsumption
  oracle (`HyperCache::decide_with_stats`) **amortizes the per-pair
  `ClauseIndexes` rebuild**: it cloned the full clause vector + rebuilt the whole
  index on every decided pair (measured 13,772× ~34.6k clauses on `ore_ont_1508`;
  11–15% of self-time on converging wedge-heavy classify). Now it reuses the
  shared base `Arc<ClauseIndexes>` + an O(#appended-clause) per-pair delta: a
  `ClauseIndexSink` trait routes the base build and the delta through one
  `index_one_clause` (so `x_trigger`, `match_plans`, nonhorn/empty-body, disjoint
  entries can't diverge), the engine branch-routes `clause(ci)`/`match_plan(ci)`
  between base slice and per-pair extras, disjointness is a base+per-pair overlay,
  and the pair-invariant value-disjoint clauses fold into the base once.
  Verdict-preserving (closures byte-identical on vs `RUSTDL_CLASSIFY_AMORTIZE_IDX=0`
  vs pre-change baseline, corpus + ORE; delta-vs-full equivalence unit test).
  ~11–13% on `ore_ont_1508`/`12698`; broad (every wedge-classified ontology).
  NOTE: the residual wedge-classify cost is `enumerate_matches`/`match_body` (the
  non-Horn fire loop, ~25% self-time) — separate in-flight work, not this.
  Plan: `docs/superpowers/plans/2026-07-23-classify-clauseindex-amortization-plan.md`.
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
  SP1 (Konclude-class engine, sub-project 1; merge 377b301) closed a
  wedge completeness gap: `domain`/`range` (single-role-body clauses)
  did not fire through **inverse** or **symmetric** roles. Two sound,
  additive parts: (Part 1) inverse first-leg clauses
  (`Atom::Role(Inverse(p),X,y)→…`, the form `domain`/`range` on an
  inverse role clausifies to) are now triggered at the edge **target**
  via a new `inverse_first_trigger` index — `Event::Edge` previously
  fired first-leg clauses only at the source; (Part 2, "Variant R")
  symmetric/self-inverse handling — `role_matches` treats an edge as
  satisfying a body atom of the same role-id when that role
  `is_symmetric` (regardless of inverse polarity), symmetric first-legs
  are also target-triggered, and the degenerate self-inverse
  `InverseObjectProperties(p,p)` canon-rewrite (which was shadowing a
  genuine declared inverse pair `p⁻=q` via the first-wins `contains_key`
  guard) is suppressed in `build_inverse_canon` + `build_role_hierarchy`.
  Symmetric detection (`RoleHierarchy::is_symmetric`, O(1)) comes from
  `SymmetricObjectProperty` + self-inverse `InverseObjectProperties(p,p)`.
  **Sound by construction** (only adds genuinely-entailed matches — an
  over-fire is a `match_body`-re-verified no-op, never a false clash).
  **Closes the family *calculus* gap**: the 15-axiom ddmin core
  `docs/family-mech4-ddmin-core.ofn` is now `inconsistent` (Konclude
  oracle parity); **full `family.ofn` remains a sound MISS — that is a
  *scale* stall (transitive closure + disjunctive depth), deferred to
  SP2**, so `family*_inconsistency_detected` stay `#[ignore]`d.
  FP=0/MISSED=0 re-verified corpus-wide (galen/notgalen/sio/wine/
  ore-10908/ore-15672/alehif/ro/pizza/bibtex). A bake-off rejected
  "Variant M" (symmetric edge materialization): it fails the family-core
  gate without the canon fix and grows the completion graph. Canaries:
  `crates/owl-dl-reasoner/tests/inverse_symmetric_domain.rs` (8, incl.
  inverse/symmetric/self-inverse domain+range, negative + fire-but-
  harmless controls, family core). Spec
  `docs/superpowers/specs/2026-06-18-wedge-declared-inverse-symmetric-design.md`,
  plan `docs/superpowers/plans/2026-06-18-wedge-inverse-symmetric-sp1.md`.
  SP1.1 (classify-completeness; gated `RUSTDL_CLASSIFY_SAME_TIER`,
  **default OFF**) makes SP1's inverse/symmetric domain/range firing
  reachable from the per-pair **classification** oracle (not just the
  consistency path): Layer A threads the role hierarchy into
  `HyperCache::{decide,classify_labels}`; Layer B broadens the
  same-tier "defined-sup sweep" in `classify_top_down_internal` to
  **label-driven** (the tier walk groups by EL/told subsumer count and
  never compares same-tier classes, so engine-derived same-tier
  subsumptions like `C ⊑ D` via inverse-domain on a generated
  successor are missed). Sound (FP=0, oracle-confirmed, byte-identical
  corpus closures). **Default OFF because it is corpus-invisible**
  (closes zero benchmark subsumptions — the pattern occurs only in the
  `sp11sub` synthetic, not in any fixture) **and costs ~2× wall**
  (ore-15672 138→252s: it surfaces same-tier SP2-hard *non*-subsumptions
  that burn deadlines). Opt-in (`=1`) for the rare DL-heavy ontology
  that needs it; flag-off path is byte-identical to pre-SP1.1. Spec
  `docs/superpowers/specs/2026-06-19-classify-completeness-sp1.1-design.md`,
  plan `docs/superpowers/plans/2026-06-19-classify-completeness-sp1.1.md`.
  Adaptive budget (perf Lever #1; gated `RUSTDL_ADAPTIVE_BUDGET`, **default
  ON**) early-cuts a *diverging* wedge search — `is_diverging` fires when,
  over a `DIV_WINDOW=500`-branch window, ~all branches failed
  (`restores≈branches`, ≥98%) at saturated depth (`max_branch_depth` ==
  the `HYPER_WEDGE_DEPTH` cap) — returning `Stalled` early instead of
  burning the per-pair deadline. **Sound: FP=0 structurally** (early-cut
  only ever yields "not subsumed", a MISS at worst) **and verdict-
  preserving** (corpus MISSED=0, byte-identical closures — every real
  subsumption proof completes within 500 branches, so none is cut; the
  window size is the discriminator vs a converging Unsat proof). The
  divergence is *thrashing through a tiny state set at stable node count*
  (the #2 reuse probe found `e-interaction` revisits ~14 states ~40k×), so
  a node-growth clause was deliberately dropped. **ore-15672 138→91s
  (~34%)**; modest by design (many hard pairs saturate depth after >500
  branches → cut later than the ~100ms ideal); lower `DIV_WINDOW` gains
  more, each step gated by a fresh corpus MISSED net (convergence-risk
  curve). `=0` reverts. Spec
  `docs/superpowers/specs/2026-06-19-adaptive-budget-design.md`, plan
  `docs/superpowers/plans/2026-06-19-adaptive-budget.md`; companion
  scoping (incl. Lever #2 within-search-caching P0 = VIABLE-strong)
  `docs/superpowers/specs/2026-06-19-perf-frontier-levers-scoping.md`.
  Incremental functional/`≤1` merge (2026-07-11, gated
  `RUSTDL_INVERSE_FUNC_MERGE`, **default ON**) closes 9 of galen's 10
  functional-merge-across-inverse misses (see
  `docs/known-limitations/galen-inverse-functional-completeness.md`) by
  firing the `≤1`/functional merge (incl. inverse-induced successors) as
  part of `horn_fixpoint`'s own fact-processing loop instead of the earlier
  whole-graph re-fire that made galen a 6.6-minute DNF when the merge was
  enabled. Resolve-on-read (a head derived onto a folded node resolves to
  the survivor via union-find, not the ghost) makes the incremental fold
  correct; the folded node's `preds` are copied to the survivor so post-merge
  labels back-propagate to its predecessors too. **Sound** (same merge
  semantics as before — closure-diff FP=0 corpus-wide) **and fast**: galen
  MISSED 10 → 1 in well under a second (down from the 6.6-min DNF), wine
  19.78 s → 90 ms. `=0` reverts to the pre-2026-07-11 (MISSED=10, flag-off-
  by-default) behaviour. The 1 residual galen pair
  (`TibialTuberosity ⊑ TibialInterCondylarEminence`) was a *different*,
  harder mechanism — a defined-class ∃-monotonicity subsumption that needs
  disjunctive ¬-expansion + `∀`-propagation to interact with this same merge,
  which the deterministic Horn-only `horn_fixpoint` does not attempt; see
  `docs/known-limitations/galen-defined-class-monotonicity-residual.md`.
  Spec `docs/superpowers/specs/2026-07-11-wedge-incremental-functional-merge-design.md`,
  plan `docs/superpowers/plans/2026-07-11-wedge-incremental-merge.md`.
  **Label-cache back-fold (2026-07-12, gated `RUSTDL_CLASSIFY_BACKFOLD`,
  default ON)** closed that 10th pair too: a sound, branch-free, direct
  `∃`-composition rule over the per-class merge-enriched `sat` graph, injected
  into the class hierarchy the same way as the defined-SUB sweep — **zero
  tableau/search calls**, so it cannot hit the `DEFINED_SWEEP`-style wall
  explosion. Recovers `TibialTuberosity ⊑ TibialInterCondylarEminence` without
  the disjunctive ¬-expansion path. **galen MISSED 1 → 0; corpus FP=0/MISSED=0
  unchanged elsewhere (closure-diff, all curated fixtures); galen wall stays
  ~0.9 s.** `=0` reverts. See
  `docs/known-limitations/galen-defined-class-monotonicity-residual.md` and
  `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md`.
  **Incremental `horn_fixpoint` (SP1, 2026-07-14, gated
  `RUSTDL_HYPER_INCREMENTAL_FIXPOINT`, default ON)** stops re-seeding the whole
  graph on every wedge `solve` frame: with the flag on, `horn_fixpoint` drains
  only the per-branch worklist delta (worklist snapshotted in `save`/`restore`,
  root graph seeded once in `decide_with_deadline`), instead of the
  clear+full-reseed the OFF path still does. **Verdict-preserving** (curated
  FP=0/MISSED=0, byte-identical closures; `ore_ont_13723` non-Horn FP oracle
  0→0) and a real ~10% stall reduction on dense SROIQ (`ore_ont_10019` classify
  incomplete pairs 1626→1465, hierarchy byte-identical — a sound speedup, no new
  decided class; the residual stall is depth-bound, deferred to SP2). Wired at
  the four `HyperCache` classify builders + the four diagnostic probes; NOT
  wired to the default-OFF snapshot path. `=0` reverts to the full-reseed path.
  Correctness gate: `crates/owl-dl-cli/tests/incremental_fixpoint_identity.rs`
  (classify OFF-vs-ON byte-identity on 4 fixtures). See
  `docs/2026-07-13-ore_ont_10019-stall-findings.md`, spec
  `docs/superpowers/specs/2026-07-13-dense-sroiq-tractability-roadmap.md`, plan
  `docs/superpowers/plans/2026-07-13-dense-sroiq-sp0-sp1.md`.

- **`crates/owl-dl-core`** — Phase 3c (commit 0b5ed36) cached
  `ConceptPool::bot_id` via `OnceLock<ConceptId>` (concurrency-safe;
  `ConceptPool` is Sync across rayon workers). Eliminates the 24.66%
  `apply_role_axioms` / `bot_id` / `find_map` cluster on GALEN+SIO
  flamegraphs. **GALEN classify wall: 24.8 min → 12.2 min (~50%
  reduction)** — this reclaims Phase 2b's full wall regression.
  FP=0 + verdicts unchanged. See `docs/phase3c-results.md`.
  **DisjointUnion covering in the wedge (2026-07-25, #40).** `clause.rs`'s
  `DisjointUnion` arm now also emits the covering clause
  `C(X) → D1(X) ∨ … ∨ Dn(X)` (it previously emitted only the pairwise-disjoint
  + `member ⊑ class` halves, deferring the covering `C ⊑ ⊔Di`). The tableau
  (`absorb.rs`) already had it; this closes the wedge/classify gap so
  covering-dependent subsumptions (`C ⊑ ⊔Di ⊑ E ⟹ C ⊑ E`) are no longer MISSED
  under default `trust_sat`. Sound (genuine DisjointUnion semantics — only adds
  entailed subsumptions); byte-identical where no DisjointUnion covering applies.
  NOTE: covering-dependent **same-tier** subsumptions still need
  `RUSTDL_CLASSIFY_SAME_TIER=1` (the separate tier-walk limitation). Spec
  `docs/superpowers/specs/2026-07-25-disjointunion-wedge-covering-design.md`.
  **Graceful degradation + surfaced drops (2026-07-26, #43).** `convert_ontology`
  no longer aborts on an unsupported construct: `ce_or_skip!` propagates the
  error and the conversion loop RECORDS it on `InternalOntology.dropped:
  DroppedAxioms` (`kind → count`) and continues, reasoning over the supported
  fragment. `Ok(None)` is now benign-only (metadata / annotations / bare
  declarations); every unrepresentable CONTENT axiom — unsupported data ranges,
  `HasKey`, SWRL, and (under `RUSTDL_DATA_PROPERTIES=0`) data-property axioms —
  returns `Err` → recorded. Surfaced via `owl_dl_reasoner::dropped_axioms(&onto)`,
  a `dropped` block in `classify`/`consistent`/`realize --json`, a default stderr
  warning, and Python `dropped_axioms(path)`. Sound under-approximation (weaker
  KB ⇒ only missed entailments, never a false one); inert when nothing is dropped
  (empty map ⇒ byte-identical). **`HasKey` and unsupported axioms no longer
  hard-error** — they degrade. Spec
  `docs/superpowers/specs/2026-07-25-surface-dropped-axioms-design.md`.

- **`crates/owl-dl-reasoner`** — public API + orchestrator (`lib.rs`,
  `classify.rs`, `realize.rs`). Every entry point that issues a tableau query
  first runs saturation and short-circuits on a hit; if the whole ontology is in
  the EL fragment it takes a saturation-only fast path (`stats.pure_el_mode`).
  `PreparedOntology::from_internal` snapshots the post-NNF/absorb/ABox-seed state
  **once** so the O(n²) pairwise classify loop reuses it across pairs; the loop
  runs in parallel via rayon. `is_subclass_of` reduces to satisfiability of
  `sub ⊓ ¬sup`.
  **Realize saturation fast path (2026-07-21, `RUSTDL_REALIZE_SATURATION`,
  default ON) — fixes the issue-#35 realization hang.** `realize` /
  `is_instance_of` / `instances_of` previously ran the full `{a} ⊓ ¬C` tableau
  probe for *every* (individual, class) pair; on an EL/Horn ontology with a
  defined class (`≡` + `∃`) + property domain + a property assertion the
  ⊔-search explodes (100k+ branches over a blocked ~134-node graph) and never
  terminates — a >300 s hang, while `classify` on the same file is instant
  (saturation fast path). Now realize mirrors `classify`'s gate: on the
  saturator-complete fragment (`realize_saturation_eligible`: TBox in
  `is_pure_el`/`saturator_complete_fragment`/`tbox_only_saturator_eligible`
  **and** every ABox axiom a shape the seeding captures — atomic/⊓
  `ClassAssertion`, non-inverse `ObjectPropertyAssertion`; `SameIndividual` /
  inverse assertions fall back) it realizes via
  `owl_dl_saturation::saturate_for_realize`, which materializes each named
  individual as a nominal class `N_a` and seeds `N_a ⊑ C` (ClassAssertion),
  `N_a ⊑ ∃r.N_b` (ground edge ⟹ domain-of-`r` + existential-LHS firing) and
  `N_b ⊑ Rng` (ground range); `types(a) = subsumers_of(N_a) ∩ named classes`.
  **Complete == the tableau on the fragment** (incl. the conjunctive-LHS
  `x:D1, x:D2, D1 ⊓ D2 ⊑ E ⊨ x:E` case both saturation engines otherwise drop),
  **sound by construction**, and TERMINATING (no tableau). Off-fragment realize
  keeps the tableau (identical prior logic) plus an opt-in per-pair deadline
  `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` (default UNSET ⟹ no bound; restores the
  caller-side bound removed in 0.3.18 — bounds each pair, not total wall).
  `RUSTDL_REALIZE_SATURATION=0` reverts to the tableau path. Correctness gate:
  `realize::tests::fast_path_matches_tableau_on_terminating_fixture` (byte-
  identity) + conjunctive/existential/domain-range/termination unit tests. See
  `docs/2026-07-21-realize-saturation-fast-path.md`.
  **Reduced-input `abox_check` (2026-07-30, no flag — verdict-identical by
  construction).** Both classify **fast paths** — the block in
  `classify_internal_with_timeout` and the structurally identical one in
  `classify_top_down_internal`, each ending in `return Ok(classify_pure_el(…))` —
  built a **full** `PreparedOntology` (a *second* full EL saturation +
  `HyperCache::build` + `ConsistencyCache::build` + NNF + absorb) *solely* to read
  `abox_verdict()`, then **discarded it**. `abox_check::check` reads only 8 fields
  (`abox`, `axioms`, `told`, `pool`, `inverse_pairs`, `hierarchy`,
  `disjoint_role_pairs`, `closure`) and **never `hyper`/`tbox`** — the expensive ones.
  Those 8 are now extracted into `AboxCheckInputs<'a>` (so the dependency *set* is
  compiler-checked) and built by `build_abox_check_inputs`, reusing the closure
  already in scope. NOTE `abox_verdict()` is **lazily initialised** (`get_or_init`),
  so the two *genuine* `from_internal` builds (whose object is used to classify) had
  nothing to save and are untouched. **Measured** (single-thread, min-of-3, vs
  pre-change `main`): ore_ont_10073 9.74→6.28 s (−35.5%), 10068 3.65→2.42 s (−33.7%),
  1043 2.38→1.77 s (−25.6%, RSS −35.2%), 1115 −18.8%, 11110 −18.7%, 10965 −16.7%;
  **hybrid controls flat** (10127 0.2%, 10838 1.1%). That is **76–94% of the
  `RUSTDL_ABOX_CHECK=0` ceiling** — that flag skips the check entirely and is an upper
  bound, NOT this lever's value; quote the fraction, never the bound. Addressable set,
  counted on the **binding** predicate (the classify path) rather than an
  assertion-count proxy: **25/120** sampled completing ORE onts are fast-path AND
  ABox-bearing, **8/120** fast-path AND ≥50k assertions (~400 / ~107 extrapolated).
  **NOT a memory lever** — 4 of 6 winners show ~0% RSS change, so the saving is
  skipped *compute*; `ore_ont_9347`'s 42 GB is still DKey disjointness at conversion.
  (**Since fixed** — see the non-merging-component gate under `owl-dl-datatypes`:
  `9347` now classifies in 10.72 s / 227 MB. Its true full-classify peak was 70.7 GB,
  the 42 GB figure having measured `from_internal` alone.)
  Gates: FP=0 net 22/0 all closures exact; byte-identity 13/13 vs pre-change `main`
  (classify+consistent ×5 fixtures, `realize --json` ×3); a 23-shape differential over
  P1–P9 + 6 negative controls agreeing on verdict *and* clash pattern 23/23.
  **KNOWN LIMITATION (verified, not assumed):** `build_abox_check_inputs` is a
  hand-copied prefix of `from_internal`, not shared code, and the in-tree differential
  test does **NOT** guard that drift — deleting `expand_role_characteristics`, or
  moving `build_told_tables` / the `axioms` clone after it, each still passed 6/6.
  Those are inert for *today's* `check`, so the hazard is **latent, not absent**: it
  goes live the moment `check` reads something an omitted pass affects. **If you extend
  `abox_check` to read a new field or a lowered axiom form, re-run that sabotage** — if
  it still passes, the differential test is not protecting your new dependency. See the
  doc comment on `build_abox_check_inputs` and
  `docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md`.
  **Inconsistency short-circuit (2026-07-23, v0.3.36).** Off the saturation
  fast-path fragment, `realize_internal` now runs the `saturate_abox_consistency`
  pre-check first and returns `Err(Inconsistent)` on a clash — matching the
  sibling `materialize_{object,data}_property_assertions`. Without it, realize
  ran a `{a} ⊓ ¬C` probe per (individual, class) on an inconsistent KB (every
  membership vacuously holds), which on a deep inconsistency the pre-check catches
  but classify's patterns don't (`family.ofn`) was a multi-minute stall and could
  even return a degenerate `Ok` (an individual typed into mutually-disjoint
  classes). Sound under-approx (clash ⇒ genuinely inconsistent; consistent onts
  unaffected). `family` realize now errors in ~0.7 s (was a hang). Regression
  test `realize_inconsistent_shortcircuit`.
  **Pseudo-model realize shortcut (2026-07-26, `RUSTDL_PSEUDO_MODEL`, default
  ON).** On the off-fragment (tableau) realize path, `realize_tableau_internal`
  computes ONE `ABox` witness model (one wedge `Sat` completion) once per
  `realize` call and prunes each per-(individual, class) `{a} ⊓ ¬C` probe with
  `class ∉ witness_types(individual) ⇒ Ok(false)`, checked after the
  told-closure `Ok(true)` fast path and before the tableau probe is built.
  **Completeness-preserving** (ON vs OFF `realize --json` byte-identical) and
  **sound by construction**: subtractive-only (`Ok(false)` only), and an
  entailed type is in every model — including the witness — so it is never
  pruned; the same "absence in one `Sat` completion's labels ⇒ genuine
  non-entailment" direction as the shipped default-ON Phase-7 label heuristic
  (`satisfiability_labels`), applied to instance-checking instead of
  subsumption-checking — the opposite, sound direction from the FP-unsound
  `RUSTDL_SNAPSHOT_CAPTURE` trap (which asserts a positive from one model).
  Assessment (custom nominal-`ABox` fixture + a 40-decoy-class scaled variant,
  the full ORE-tier corpus bake-off being unreachable in this sandbox):
  verdict-identity held on both fixtures; the prune fired and won (1.59× on
  the scaled fixture; PR #23's original prototype measured 110–630× at
  MIE scale); a HermiT oracle (`robot reason --reasoner hermit
  --axiom-generators ClassAssertion --include-indirect true`, output committed
  as `tests/fixtures/pseudo_model/nominal_abox-hermit.ofn`) matched rustdl
  on every named type, FP=0. `RUSTDL_PSEUDO_MODEL=0` reverts to the pre-shortcut
  per-pair-only behaviour. Silently no-ops (safe) whenever
  `PreparedOntology::realize_base_model_types` returns `None` — i.e.
  `RUSTDL_WEDGE_CONSISTENCY=0` or no `ABox` at all. Regression test
  `nominal_abox_fixture_pseudo_model_on_matches_off` in
  `crates/owl-dl-reasoner/tests/pseudo_model_realize.rs`. See
  `docs/2026-07-26-pseudo-model-assessment.md`.
  `materialize_object_property_assertions` (reasoner) / `materialize_inferred_property_assertions`
  (Python) / `realize --properties` (CLI) surface inferred OBJECT property assertions over named
  individuals, reusing the ABox saturator's derived edges (sound under-approximation: no
  anonymous-witness / disjunctive edges; errors on inconsistency). See
  `docs/superpowers/specs/2026-06-21-inferred-property-assertions-design.md`.
  `materialize_data_property_assertions` (reasoner) / `materialize_inferred_data_property_assertions`
  (Python) surface inferred DATA property assertions (5-tuple subject/property/lexical/datatype/lang)
  via a structural sub-data-property + equivalent-data-property + SameIndividual-folding closure
  (sound; under-approx: no class-axiom-derived, e.g. DataHasValue). SameIndividual folding added
  2026-07-01, guarded by a HermiT/ROBOT DataPropertyAssertion oracle
  (`crates/owl-dl-reasoner/tests/materialize_oracle.rs::materialize_data_matches_hermit_oracle`);
  the oracle fixture deliberately excludes EquivalentDataProperties because HermiT's
  InferredPropertyAssertionGenerator does not traverse it. Folded into `realize --properties`. See
  `docs/superpowers/specs/2026-06-21-inferred-data-property-assertions-design.md`.
  `materialize_sub{object,data}property_axioms` (reasoner) /
  `materialize_inferred_sub{object,data}property_axioms` (Python) return the inferred named
  property-subsumption closure (object: told + equivalent + inverse; data: told + equivalent),
  structural + sound. See `docs/superpowers/specs/2026-06-21-inferred-subproperty-axioms-design.md`.
  `materialize_existential_successors` (reasoner + Python) returns a blank-node
  representation of named individuals' entailed existential successors — one
  `(subject, property, witness_blank, filler_class)` row per entailed `a : ∃R.C`
  (told-exists over realized types, 1-step). NOTE: a representation of entailed
  existentials, NOT entailed ground triples (witnesses are model-relative). See
  `docs/superpowers/specs/2026-06-21-existential-successors-design.md`.
  Python now exposes the explanation/debugging suite — `rustdl.justify` / `justify_all` /
  `diagnose` / `repair` (string/tuple forms) + the one-call `rustdl.debug(path)` (structured
  dict). The justify/repair query parser is shared via `owl_dl_reasoner::justify::parse_query`
  (lifted from the CLI). The `materialize_*` re-exports were also fixed — they were registered
  in `_native` but missing from `__init__.py`. See
  `docs/superpowers/specs/2026-06-21-python-debugging-surface-design.md`.
  The Python package is typed (PEP 561): `python/rustdl/py.typed` + a hand-written
  `__init__.pyi` covering the full surface (functions, the `Classification` class,
  exceptions, and `debug()` TypedDicts). `tests/python/test_stubs.py` guards stub↔`__all__`
  drift. See `docs/superpowers/specs/2026-06-21-python-type-stubs-design.md`.
  `rustdl.debug()` returns a `Diagnosis` dataclass (with `Root`/`Derived`/`Inconsistency`) —
  attribute access + `Mapping` dict-compat + `to_dict()`. See
  `docs/superpowers/specs/2026-06-21-python-result-objects-design.md`.
  **Inferred query surface (2026-07-24/25, issues #44–#48).** The reasoner-API →
  Python → CLI `--json` surface for the OWLReasoner queries beyond class
  classification, all sound (FP=0) by reduction to the existing
  (un)sat/consistency engines (never from a satisfying model), each running the
  mandatory inconsistent-KB guard first and reporting an `incomplete` signal
  (trusted-`Sat` / budget):
  - #47 `disjoint_classes` (entailment: `C ⊓ D` unsat, told-disjoint seed) +
    `disjoint_{object,data}_properties` (structural told closure). CLI `disjoint --json`.
  - #44 `classify_{object,data}_property_hierarchy` → `PropertyClassification`
    (equiv groups + Hasse `direct_subsumptions`, from the told+equiv+inverse
    property closure). CLI `property-hierarchy --json`.
  - #46 `same_individuals` (asserted + functional-forced `derived_same` seed +
    augment-recheck `KB∪{a≠b}` inconsistent) / `different_individuals`
    (`{a}⊓{b}` unsat). CLI `individuals --json`.
  - #45 `inferred_{object,data}_property_values` (materialize seed + budgeted
    entailment extension via a `KB∪{¬R(a,b)}` recheck; data = structural). CLI
    `property-values --json`.
  - #48 `class_expression_{satisfiable,entailed_subclass,instances}` — accept an
    anonymous **Manchester** class expression, reduced by injecting a fresh
    `EquivalentClasses(Q, CE)` probe and querying the named `Q` (the
    `justify::entails` probe pattern generalized; front-end parses via horned-owl
    `parse_class_expression`). CLI `sat-expr`/`subclass-expr`/`instances-expr`.
  Shared infra: `PreparedOntology` now carries `vocabulary` +
  `pair_disjoint_with_deadline` / `pair_individuals_disjoint_with_deadline`
  (`C⊓D` / `{a}⊓{b}` sat probes over the frozen snapshot via `decide`) +
  `consistent_with_extra` (snapshot-preserving augment-and-recheck: injects one
  extra distinct-pair / negative-property fact into the per-probe tableau seed,
  no rebuild). `SaturationResult.derived_same` records functional/inverse-
  functional-forced equalities. Python query wrappers emit `IncompleteQueryWarning`.
  HermiT/ROBOT oracle FP=0 gates (`disjoint_classes`, property values; hand-
  verified for same/different — ROBOT has no such generator; probe-trick oracle
  for class expressions). Specs
  `docs/superpowers/specs/2026-07-24-inferred-query-surface-44-47-design.md`,
  `docs/superpowers/specs/2026-07-25-complex-class-expression-queries-design.md`.
  Phase 4b (commit e31439c) added a `FragmentClassification`
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

- **`crates/owl-dl-datatypes`** — concrete-domain reasoners (`card_sat`,
  interval/finite-set value ranges). **Wired into reasoning** (via the DKey
  side-map + the tableau `concrete_domain_clash`), and as of 2026-06-18 data
  properties are first-class and default-ON — see the `RUSTDL_DATA_PROPERTIES`
  entry in the soundness contract below. Data axioms / data ranges that
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

  **Phase D6 (2026-06-08)** — datatype VALUE-MEMBERSHIP subsumption
  (closes the silent-incompleteness ORE 2015 surfaced on
  `ore_ont_9054`, `docs/ore-2015-results-2026-06-08.md`). The missing
  inference `DataHasValue(p,v) ⊑ DataSomeValuesFrom(p, range)` iff
  `v ∈ range` is enabled via the NomKey-style synthetic-subsumer
  reduction: `convert.rs` lowers (integer-only) `DataHasValue(p,v)` →
  `∃p.DKey([v,v])` and `DataSomeValuesFrom(p, int facets)` →
  `∃p.DKey(range)` (data property `p` as a role; `DKey(range)` an
  opaque filler class, IRI `urn:rustdl-dkey:<min>:<max>`), and
  `seed_dkey_subsumptions` emits told `DKey(r1) ⊑ DKey(r2)` iff
  `IntegerRange::subset` (`r1 ⊆ r2`); same-property keying comes free
  from CR5 role-match; the saturator's `∃`/`⊓`/defined-sup machinery
  propagates. DKeys are filtered from reported classes via
  `reportable_class_iris`. **Sound by construction** (only adds genuine
  value∈range / range⊆range subsumptions); FP=0/MISSED=0 re-verified
  corpus-wide (sio/shoiq-knowledge/wine/ore-10908/ore-15672/pizza/
  alehif). Recovered `ore_ont_9054` MISSED 79→37 (42 pairs).

  **Phase D6 Part A + B (2026-06-08, follow-up)** — closed
  `ore_ont_9054`'s residual 37 → **0** (full Konclude∩HermiT oracle
  parity, closure 676=676, FP=0). Two extensions:
  - **Part A (bare `xsd:integer`)**: `parse_integer_range` now also
    maps a facet-less `DataRange::Datatype(xsd:integer)` to the
    unbounded `IntegerRange`, so `DataSomeValuesFrom(p, xsd:integer)`
    lowers to `∃p.DKey(-∞,+∞)` (keeps Prime/Zoom-style conjunctions
    alive). Closed 6 pairs.
  - **Part B (float/double ranges)**: new `FloatRange { min,
    min_incl, max, max_incl }` with EXPLICIT inclusive/exclusive flags
    (the integer ±1 normalization is INVALID for reals — exclusive
    bounds cannot be shifted). `parse_float_range` /
    `float_literal_value` parse `xsd:float`/`xsd:double`
    `DataHasValue` points + `DatatypeRestriction` facets; NaN/±∞
    rejected at parse → whole range drops. `FloatRange::subset` does
    explicit-boundary containment (equal-endpoint rule: `s==o` OK iff
    `other.incl || !self.incl`). Closed 31 pairs (incl. the lone
    range-vs-range float subset `VeryFastExposure ⊑ FastExposure`,
    `(-∞,0.002) ⊆ (-∞,0.01)`).

  **Datatype keying is soundness-critical**: float `DKey`s are
  datatype-tagged (`urn:rustdl-dkey:f:<minbits>:<i|e>:<maxbits>:<i|e>`,
  bounds via `f64::to_bits()` for exact round-trip) and
  `seed_dkey_subsumptions` buckets by datatype, seeding edges ONLY
  within a bucket — int and float NEVER cross-subsume (conservative
  under-approximation). **Sound by construction**; FP=0/MISSED=0
  re-verified across the full gate (sio/shoiq-knowledge/wine/ore-10908/
  ore-15672/pizza/alehif/galen/notgalen) + `ore_ont_9054` 37→0.
  Negatives-first canaries:
  `crates/owl-dl-reasoner/tests/datatype_value_membership.rs` (24,
  incl. float exclusive-boundary, equal-endpoint incl/excl,
  cross-datatype int↔float, wrong-property, NaN-drop, bare-integer) +
  `IntegerRange::subset` / `FloatRange::subset` / NaN-reject /
  no-±1-normalization unit tests in `data_axioms.rs`.

  **Phase D8 (2026-06-09)** — extended the value-membership reduction
  to three more totally-ordered datatype buckets: `xsd:decimal`,
  `xsd:date`, `xsd:dateTime`. Two soundness landmines, each defused at
  parse time:
  - **decimal ≠ float** — distinct value space; NEVER `f64` (rounding
    two distinct decimals to one `f64` = spurious equality = FP). New
    exact `Decimal { negative, int, frac }` in NORMALIZED lexical form
    (leading/trailing zeros stripped, signed-zero collapsed) with a
    manual `Ord` (sign → int len-then-lex → frac pad-then-lex).
    Exponent notation rejected (that is `xsd:double`).
  - **date/dateTime are PARTIAL orders across timezone presence** —
    compared by component TUPLE (`(i64,u8,u8)` / `(i64,u8,u8,u8,u8,u8)`),
    tuple order = chronological with ZERO calendar arithmetic. Anything
    carrying a `Z`/offset is DROPPED at parse (sidesteps the ±14h
    partial-order); fractional-second dateTimes also drop. So every key
    that reaches the comparison is timezone-free and totally ordered.

    Shared machinery: generic `OrdRange<T: Ord>` (subset + facet-tighten
    written once, same explicit-boundary algebra as `FloatRange`) and a
    generic `seed_bucket` helper (5 near-identical O(k²) loops collapsed).
    Three new datatype-tagged `DKey` namespaces — `dec:` / `date:` /
    `dt:` (integer untagged, float `f:`) — with `.`-separated inner
    components so the `:`-delimited envelope decode stays unambiguous.
    The five `parse_*_dkey_iri` decoders are **pairwise mutually
    exclusive** (pinned by the `parser_matrix_mutual_exclusivity`
    canary — a single off-diagonal `Some` would seed a cross-datatype
    edge = FP). All five buckets stay strictly disjoint (no
    integer⊆decimal cross-seed even though XSD-sound — extra FP surface,
    deferred). Completeness-neutral on the current corpus (no MISSED to
    chase — `ore_ont_9054` already closed by D6/D7); the win is canaries
    + FP=0/MISSED=0 UNCHANGED corpus-wide. **8 new negatives-first
    canaries** (decimal exclusive-boundary, distinct-values-don't-
    collide, date/dateTime boundary, decimal-vs-integer cross-datatype,
    tz-bearing-date-dropped) + 4 unit tests (parser matrix, IRI
    round-trip all buckets, exact decimal ordering, temporal tz/fraction
    drop).

  **Phase D9 (2026-06-09)** — `xsd:string` value membership (the
  EQUALITY-typed, non-ordered datatype; closes the string half of the
  value-membership fragment). New `StrSet { Top, Set(BTreeSet<String>) }`
  with set-containment subset (anything ⊆ `Top`; `Top` ⊄ a finite set);
  own `str:` `DKey` bucket. Members are **hex-encoded** (UTF-8 bytes →
  `[0-9a-f]*`) so arbitrary content — `:`, `.`, unicode — round-trips
  through the `:`-delimited IRI; `*` (not valid hex) marks `Top`. Wired:
  bare `xsd:string` → `Top`, `DataOneOf`-of-strings → `Set`, string
  `DataHasValue` → singleton. **Soundness (the decimal-equality analog)**:
  only EXACT lexical identity within `xsd:string` is set-equal —
  language-tagged literals and any non-string datatype are rejected at
  parse, dropping the whole value/enumeration (a `DataOneOf` with one
  non-string member drops entirely; never a partial set, which would be
  unsound in a sufficient-direction RHS). The `str:` decoder joins the
  pairwise-exclusivity matrix (now 6 buckets). 8 new integration canaries
  (∈/∉ enumeration, ⊆ bare-string-`Top`, set subset/superset, wrong
  property, string-vs-integer cross-datatype, language-tagged-member-drops)
  + a string round-trip/subset unit test. FP=0/MISSED=0 re-verified
  corpus-wide (sio 8904, wine 653, ore-15672 142, ore-10908 6001,
  shoiq-knowledge 449, sulo, ro, bibtex, galen, notgalen, pizza).

  **Phase D11 (2026-06-09)** — `DataAllValuesFrom` (unblocked by the D10
  Horn-shortcircuit fix; data-`∀` now routes to the complete hybrid
  tableau). Two halves:
  - **D11a (lowering)**: `DataAllValuesFrom(p, range)` → `∀p.DKey(range)`
    (object ∀-encoding). Sound — UNDER-approximate: a `DKey(range)` member
    need not be a real in-range value, so object models are MORE permissive
    ⟹ subsumption/unsat can only MISS, never FP. Gives ∀-monotonicity
    (`∀p.DKey(r1) ⊑ ∀p.DKey(r2)` iff `r1 ⊆ r2`, via the told `DKey⊑DKey`
    edges). Refactored `DataSomeValuesFrom`/`DataAllValuesFrom` onto a shared
    `data_range_dkey` `(role, filler)` core.
  - **D11b (disjointness, FP-critical)**: seed
    `DisjointClasses(DKey(ra), DKey(rb))` (native axiom — the form the D10
    ∀-clash probe proved the tableau handles) for every PROVABLY disjoint
    pair within a bucket, enabling the `∃p.DKey(v) ⊓ ∀p.DKey(r)` membership
    clash when `v ∉ r`. The entire FP surface is `definitely_disjoint`
    (`disjoint()` on each range type): CONSERVATIVE — `true` only when no
    value is shared; a shared INCLUSIVE endpoint is OVERLAP (`[0,5]`,`[5,10]`
    not disjoint), excluded only if a boundary is exclusive. **The corpus
    can't validate this (no `∃+∀` clash exists in it), so the boundary unit
    tests (`{integer,float,ord_decimal,ord_date,strset}_disjoint*`) +
    membership canaries are the entire safety net.** Verified end-to-end on
    real data (`/tmp/data-forall-probe.ofn`): value 5 ∉ [0,3] ⟹ C unsat
    (mode hybrid), value 2 ∈ [0,3] ⟹ satisfiable. FP=0/MISSED=0 unchanged
    corpus-wide. Canaries: 10 new in `datatype_value_membership.rs`
    (3 ∀-monotonicity + 7 membership-clash incl. inclusive-boundary,
    float/string buckets, cross-datatype no-clash) + 6 `disjoint` unit tests.

  **Non-merging-component gate (2026-07-30, `RUSTDL_DKEY_MERGING_GATE`, default
  ON, `=0` reverts).** `seed_disjoint_bucket`'s bounded seeding (v0.3.29,
  `RUSTDL_BOUNDED_DKEY_DISJOINT`) gates the **union** of role components on
  merge-inducing roles (`m_star` = functional / inverse-functional / `≤n` /
  `∀role.DKey` / DKey-range, closed downward through sub-roles) — but step (e)
  anchored a DKey to its role's component for **every** `Some`/`All`/`Min`/`Max`
  occurrence regardless. Since `DataPropertyAssertion(p,a,v)` lowers to
  `ClassAssertion(a, ∃p.DKey(v))`, all k values on one data property landed in one
  component and were seeded **all-pairs, C(k,2)**. But `∃p.DKey_a ⊓ ∃p.DKey_b` is
  satisfiable with two **distinct** `p`-successors — the axiom is consumable only if
  a merge forces both keys onto ONE node, i.e. only if the component contains an
  `m_star` role. The gate skips anchoring into non-`m_star` components, so those keys
  fall into the skip `seed_disjoint_bucket` **already** performs ("neither anchored
  nor unanchored … can never reach a node label") — extended from *can't be labelled*
  to *can't be **co**-labelled*. **`ore_ont_9347`** (19,160 `DataPropertyAssertion`
  and **zero** `DataSomeValuesFrom`/`DataAllValuesFrom`/`DataHasValue`/
  `FunctionalDataProperty`/`DataPropertyRange`, so nothing can consume any pair):
  concept_rules **49,571,087 → 113**, classify **DNF @703 s / 70.7 GB → 10.72 s /
  227 MB** (311× RSS, DNF → completes). **`ore_ont_5368` is the negative control and
  is flat to within noise** (701.31→701.22 s, 26,956,136→26,955,920 kB): 15
  `FunctionalDataProperty` + 14 `DataPropertyRange` make its component genuinely
  merge-inducing, so its pairs ARE consumable and the gate correctly declines.
  **Validated breadth** (1,913 ORE onts, pinned binaries both sides): **325 reduced
  (17%)**; of **41** at ≥1M `concept_rules` before, **38 drop below 1M** and only **3**
  remain (`7607` 11.6M→5.42M, `1685` 11.5M→5.42M, `4410` 2.82M→1.27M, all ~2×); **2**
  recovered from a conversion timeout (`9347`→113, `11287`→199,841). `5368` is
  correctly ABSENT from the reduced list — that absence is the cheapest tell that a
  scan used the real gate. **Lever 2 (an on-demand disjointness oracle) is therefore
  PARKED, not queued**: its addressable set is ~4 ontologies (those 3 plus `5368`)
  against new side-table hooks in four consumers, three of which have none — a poor
  work-to-reward ratio here. Revisit only if a user-facing ontology lands in that
  residual class, or if one of the four is independently needed (`5368`, a 27 GB DNF,
  is the strongest candidate). Encouraging if it is ever built: no consumer iterates
  the full pair set, so an oracle stays architecturally feasible. **Sound structurally** — the change only REMOVES axioms ⇒ fewer
  clashes ⇒ never an FP; the exposure is completeness, bounded by `m_star` being the
  complete set of merge sources.
  > **MEASURING THIS GATE: `ore_ont_9347` ALONE CANNOT VALIDATE IT.** `9347` reads
  > **113 under both** the real gate and a build that emits **no** DKey disjointness
  > at all, because all its pairs are dead weight either way. `ore_ont_5368` is the
  > discriminator. A population scan was **retracted** on 2026-07-30 for exactly this
  > (its "after" binary was a canary-sabotage build whose source had been reverted
  > without rebuilding, then copied out of `target/release`).
  >
  > | binary | `9347` | `5368` |
  > |---|---|---|
  > | pre-gate | 49,571,087 | 18,620,251 |
  > | **gate ON (correct)** | **113** | **18,620,251** |
  > | zero-pairs / sabotage | 113 | 12,201 |
  >
  > Pin a binary to a uniquely named path **immediately after the build that produced
  > it**, name the path after the configuration, and verify the pin against the
  > discriminating case before trusting any scan built on it.
  Evidence note: the curated corpus **cannot** validate this area —
  `datatype_value_membership.rs` says so itself ("the corpus has NO such clash, so
  these canaries are the ENTIRE safety net"). FP=0 net (22/0, closures exact) shows
  **inertness**; the real gate is that an unconditional version of this change FAILS
  exactly three canaries (`forall_value_outside_range_clashes`,
  `forall_float_value_outside_clashes`, `forall_string_value_outside_enum_clashes`)
  and that the new conversion-level negative test FAILS under
  `RUSTDL_DKEY_MERGING_GATE=0` — both verified by deliberately breaking them. Spec
  `docs/superpowers/specs/2026-07-30-dkey-nonmerging-component-gate-design.md`.

  **Remaining datatype under-approximation (sound, all still DROP):**
  - **data cardinality** — D4 already catches the unsat-clash patterns;
    full range-size-aware counting (`≥3 p` over a 2-value range → ⊥) is a
    concrete-domain cardinality reasoner with zero measured corpus reward.
  - `xsd:decimal`/`date`/`dateTime` `DataOneOf` enumerations (only
    `xsd:string` enums handled); other non-ordered datatypes;
    `DataComplementOf` / `DataUnionOf` / `DataIntersectionOf` ranges.

  Synthetic test harness: `crates/owl-dl-reasoner/tests/datatype_completeness.rs`
  (6 fixtures under `tests/fixtures/datatype/`; all 6 pass post-D5).
  Tests are `#[ignore]`d; invoke with `cargo test ... -- --ignored`.

- **`crates/owl-dl-cli`** (`rustdl` binary) and **`crates/owl-dl-bench`**
  (`owl-dl-bench`: `classify`/`sat`/`synthetic-el`/`corpus`/`compare-whelk`).
  `xtask/` holds build automation (corpus fetch, license inventory).
  `diagnose` partitions unsatisfiable classes into root (causes) vs derived
  (collateral) via a stingy structural dependency graph and justifies the roots;
  its consistency verdict tracks `classify`'s view (fast pre-checks, not the slow
  main-tableau `is_consistent`), so it is classify-speed (see
  `docs/superpowers/specs/2026-06-21-diagnose-root-derived-unsat-design.md`).
  `justify --laconic` weakens each justification axiom to its responsible fragment
  (sound structural weakening: RHS-conjunction / ∃-filler / equivalence / pairwise
  disjoint; LHS + cardinality deliberately not weakened), re-minimized via
  QuickXplain — sound by construction (every fragment is entailed by an original
  axiom). See `docs/superpowers/specs/2026-06-21-laconic-justifications-design.md`.
  `repair` lists minimal axiom-removal sets (Reiter diagnoses = minimal hitting sets
  over all justifications) to break an unwanted entailment; every repair is verified
  by removal (sound even when the justification set is incomplete). See
  `docs/superpowers/specs/2026-06-21-repair-suggestions-design.md`.
  `report` generates a self-contained HTML debugging report (summary + diagnose
  roots/derived + per-root justification + repairs); presentation-only over the
  shipped reasoner output, read-only, no external resources. See
  `docs/superpowers/specs/2026-06-21-html-report-design.md`.
  **Manchester (`.omn`) input** is wired (2026-06-22): the CLI (`detect_format`
  content-sniff: Manchester's colon-form `Prefix:`/`Class:`/… vs OFN's paren
  `Prefix(`; + extension fallback) and Python (`load.rs` extension /
  `classify_bytes(format="omn"|"manchester")`) both accept `.omn`, routing to
  `horned_owl::io::omn::reader::read` (the conformance-tested reader in the
  **pinned** fork rev — no dependency on the upstream PR merging). Front-end only,
  no engine change, FP=0 structurally untouched; rustdl now has symmetric
  Manchester I/O (it already renders explanations in Manchester). See
  `docs/superpowers/specs/2026-06-22-manchester-input-design.md`.

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
- **Phase 2b / Phase D10**: `RUSTDL_HORN_SHORTCIRCUIT` defaults ON.
  classify dispatches ontologies in the **saturator's complete
  fragment** to the saturation-only fast path instead of the per-pair
  loop. **SOUNDNESS-OF-COMPLETENESS FIX (D10, 2026-06-09):** the gate
  was originally `analyze_fragment == Horn` (clausal Horn). That is
  UNSOUND — the EL saturator has no ∀-rule (nor qualified-cardinality /
  general disjunction), so a clausal-Horn-but-not-EL ontology (`∀` +
  disjointness) was silently mis-classified and reported complete
  (`timed_out_pairs==0`); proven by `/tmp/forall-probe.ofn`
  (`∃p.K3 ⊓ ∀p.K1020` + `K3⊓K1020⊑⊥`: C is unsat, the saturator missed
  it). The "hyper Horn fixpoint is complete on Horn" justification was
  false-as-implemented: the shortcircuit ran the **EL saturator**, not
  the hyper fixpoint. The gate is now `saturator_complete_fragment`
  (`classify.rs`): a STRICT allowlist anchored to the constructs the
  saturator's rules actually process — EL concepts (`Top`/`Atomic`/`⊓`/
  `∃` forward) + role hierarchy / length-≤2 chains / transitivity /
  functional + inverse-functional witness-merge / domain / range.
  Everything else (`∀`, `≤n`, `⊔`, nominals, inverse-role *use*,
  `DisjointClasses`+lowered-`⊥` (`A⊓B⊑⊥`) [admitted when no functional /
  inverse-functional role present; `disjoint_ok` gate], ABox, …) ⟹ fall
  back to the sound+complete hybrid path when outside that allowlist.
  `DisjointUnion` remains deliberately excluded (its disjunctive covering
  is out-of-fragment). This branch (`feat/conjunctive-unsat-negation-gci`,
  2026-07-29) closed the `A⊓B⊑⊥` silent-incompleteness the gate was
  already permitting: see the entry below.
  Real impact: alehif (ALC = has `∀`) + sulo now route to hybrid
  (alehif 0.09 s → ~6.6 s wall) — the old fast path was a *lucky*
  MISSED=0 on the ∀-incomplete saturator; GALEN/notgalen (EL +
  functional, no ∀) keep the fast path (0.59 s / 1.06 s). Set
  `RUSTDL_HORN_SHORTCIRCUIT=0` to disable the functional fast path
  (pure-EL still fast via the `is_pure_el` arm). The D10 fix also
  **unblocks `DataAllValuesFrom`**: data-`∀` now correctly routes to
  the tableau, so the piece-#2 lowering (`∀p.DKey` + DKey-disjointness)
  becomes safe to build. Canaries: `saturator_fragment_{accepts_el_
  plus_functional,rejects_forall,rejects_max_cardinality,rejects_
  disjoint_classes}`. See
  `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §5
  + `docs/phase2a-recon.md`.

  **`⊑ ⊥` completeness + RHS-negation canonicalization (2026-07-29,
  branch `feat/conjunctive-unsat-negation-gci`).** Closes two related
  gaps with one design: the `is_pure_el` gate (since Lever 1b, commit
  `3e3a731`) already admitted `A⊓B⊑⊥` and `X⊑⊥`; `saturator_complete_fragment`
  also admitted them (via its `disjoint_ok` arm). Both gates also admitted
  `∃r.⊤⊑⊥` / `ObjectPropertyDomain(r,⊥)` / `ObjectPropertyRange(r,⊥)` through
  `is_pure_el` (which uses `is_el_concept` — has a `Bot` arm) — but NOT
  through `saturator_complete_fragment` (which uses `is_saturator_concept` —
  no `Bot` arm). All of these were silently dropped by the EL saturator (the
  D10 unsound-completeness class: gate certifies complete, engine drops axiom).
  **Part A (correctness fix, unflagged):** five axiom shapes now handled by the
  saturator — `And(b₁…bₙ)⊑⊥` → new `ConjunctiveUnsat{bodies}` rule feeding
  `enqueue_unsat`; `⊤⊑⊥` → `ElRules::global_unsat` marks every user
  class unsat at seed time; `∃r.A⊑⊥` → marker pushed onto
  `directly_unsat`; `∃r.⊤⊑⊥` / `ObjectPropertyDomain(r,⊥)` /
  `ObjectPropertyRange(r,⊥)` → unified `ElRules::poisoned_roles` (role
  provably having no edges in any model), plus a post-collection pass
  marking nested existential markers on poisoned roles unsat
  (order-independent). Inconsistency reporting: `Subsumers::top_is_unsat`
  (some `C` with `⊤⊑C` is unsat ⟹ `⊤⊑⊥` ⟹ inconsistent); on the
  **pure-EL path** `classify --json` no longer reports `"consistent": true`
  alongside a non-empty `unsatisfiable` list. **Hybrid-path residue
  (known follow-up):** the fix lives in `classify_pure_el`; the hybrid
  path does not carry the same `inconsistent` flag, so a KB that forces
  the hybrid path (e.g. a `∀`-axiom alongside `⊤⊑⊥`) still reports
  `"consistent": true` with a non-empty `unsatisfiable` list in
  `classify --json` output. Soundness note: *all-named-classes-unsat is
  NOT an inconsistency signal* — `{A⊑⊥, B⊑⊥}` empties every named
  class yet has a non-empty domain; `⊤` being unsat is the correct test.
  **Part A is corpus-inert on all measured data** (bibtex/pizza/ro/
  ro-stripped/sio/sulo/sulo-stripped/go-basic closure-diff 57 803 rows
  byte-identical vs `main`) — a correctness fix, no measured completeness
  or performance delta on this corpus. **Known remaining gap (honest — a residual D10 instance):**
  role-chain-induced poison — `SubObjectPropertyOf(Chain(t,u),r)` +
  `ObjectPropertyDomain(r,⊥)` + `C⊑∃t.∃u.A` is still MISSED. Crucially,
  2-leg role chains ARE admitted by `is_el_axiom` (the `SubRolePath::Chain`
  arm, line ~1111), so `is_pure_el` certifies completeness on such an
  ontology while the engine still drops the chain-induced poison — the same
  D10 unsound-completeness class (gate says complete; engine misses). Marking
  `u` poisoned would be unsound for a standalone `∃u.A`, so closing this
  needs a chain-aware rule that Part A does not supply. This is a strict
  improvement over `main` (where `ObjectPropertyDomain(r,⊥)` was silently
  dropped entirely); on the current corpus the chain pattern does not appear.
  Test exists and is `#[ignore]`d with rationale. **Part B (`RUSTDL_NEG_TO_BOT_GCI`,
  default ON, `=0` reverts):** `X⊑¬Y` → `X⊓Y⊑⊥`, run PRE-NNF over
  `InternalOntology.axioms` in `convert_ontology` (pre-NNF is
  load-bearing: post-NNF `¬∃R.C` and `¬(A⊓B)` have already become
  `∀R.¬C` and `¬A⊔¬B`, both out-of-fragment). One `⊥`-GCI per negated
  conjunct (`X⊑¬A⊓¬B` yields two; folding would be strictly weaker).
  `told.rs` extended to recognize `And([A,B])⊑⊥` as a told-disjoint
  pair (also picks up natively-written `A⊓B⊑⊥`). Logical equivalence ⟹
  FP-safe by construction. Measured (ON vs OFF, `--pair-timeout-ms 50`,
  60 s cap): **60 complement-bearing ORE ontologies** swept flag-ON vs
  flag-OFF — 58 byte-identical, 0 diffs; the other 2 DNF on both sides
  with no asymmetry; 0 mode changes. **`ore_ont_9318`** (39 433 classes,
  four genuine top-level `SubClassOf(X, ObjectComplementOf(Y))` axioms —
  the pass fires): ON **0.97 s pure-EL** vs OFF **23.93 s hybrid**,
  closures **byte-identical, 19 470 rows**. **`ore_ont_2397`** and
  **`ore_ont_10032`**: both **DNF at 150 s** flag-OFF; **1.12 s** and
  **2.36 s** flag-ON. **Recovery set, enumerated by GATE PROBE** (comparing
  the `# fragment:` verdict ON vs OFF over every ORE ont carrying a
  one-line `SubClassOf(X ObjectComplementOf(…))`, 60 s/side — grep ≠ gate,
  per the Lever 1 precedent): **13 ontologies flip to `pure-EL`**, of which
  **5 were DNF at 60 s** (`2397`, `6212`, `10016`, `10032`, `15703`) and 8
  moved `Horn` → `pure-EL` (`33`, `6870`, `7275`, `7726`, `9318`, `11906`,
  `14574`, `16299`). Qualification: in the curated 8-fixture closure-diff
  set the pass almost never fires — every `ObjectComplementOf` there sits
  inside `EquivalentClasses` (pizza, wine, family) or
  `ObjectPropertyDomain`/`Range` (ro), shapes the pass does not handle;
  the only firing site is `sulo.ofn`. So "ON-vs-OFF byte-identical across
  the curated corpus" mostly demonstrates inertness, not correctness —
  the 60-ontology ORE sweep and the `9318` identity are what carry it. Spec
  `docs/superpowers/specs/2026-07-29-negation-to-bot-gci-and-conjunctive-unsat-design.md`,
  plan `docs/superpowers/plans/2026-07-29-conjunctive-unsat-and-negation-gci.md`.

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

- **First-class data properties (2026-06-18): `RUSTDL_DATA_PROPERTIES` defaults
  ON** (`=0` opts out). An OWL 2 DL reasoner reasons about data properties by
  default (cf. Konclude/HermiT); rustdl now does too. `convert.rs` lowers
  data-property axioms to the object fragment (data property = object role,
  literal = `DKey(point)` filler): `DataPropertyAssertion`→`ClassAssertion(a,
  ∃dp.DKey)`, `SubDataPropertyOf`→role hierarchy, `Functional`→`FunctionalRole`,
  `Disjoint`→`DisjointObjectProperties`, domain/range→object domain/range, `¬dp`
  →`¬∃dp.DKey`. Sound: `xsd:float` is f32-exact (separate `f:`/`db:` DKey buckets
  from `xsd:double`); `DisjointDataProperties` same-value clash via the gated
  `DP-DJ` preprocessing (`data_axioms.rs::emit_disjoint_dp_same_value_clash`).
  Justify gains property/individual + data-property query types (`subproperty`,
  `equiv-property`, `disjoint-property`, `property`, `same`, `different`,
  `subdata-property`, `equiv-data-property`, `data-value`,
  `disjoint-data-property`). **FP=0/MISSED=0 re-validated at default-ON across the
  full Konclude-oracle net.** Concrete-domain coverage extended 2026-06-18 to the
  composite data ranges — `DataIntersectionOf` (exact range intersection;
  empty→⊥; integer⊂decimal FP-guard), `DataUnionOf` (`∃` → class-level
  disjunction, all-or-drop), `DataComplementOf` (`¬DKey(r)`; clash only via told
  `⊑` edges) — plus `DataOneOf` for all datatypes and bounded-integer cardinality
  counting (all FP=0-validated corpus-wide). Sound-but-incomplete now ONLY on the
  true asymptote: nested composites (`DataComplementOf(DataUnionOf(…))`, dropped),
  `∀`/range/cardinality over union/complement (dropped), and deep theoretical
  SROIQ(D) completeness — all ~0 corpus presence. Spec:
  `docs/superpowers/specs/2026-06-17-first-class-data-properties-design.md`.

- **ABox-saturation consistency pre-check (2026-06-20): `RUSTDL_ABOX_SATURATION`
  defaults ON** (`=0` opts out). Closes rustdl's last open *correctness* gap —
  **family** is HermiT/Konclude-inconsistent (<1s) but rustdl previously reported
  `consistent` (the wedge's `∃`-witness generation stalls at 508 individuals).
  `crates/owl-dl-reasoner/src/abox_saturation.rs` runs a sound, terminating,
  **consequence-based fixpoint over NAMED individuals only** (no witness
  generation ⇒ finite) as a pre-check in `is_consistent`, before the hybrid path:
  seed asserted types/edges → type propagation (`⊑`/`≡`/domain/range) → role-edge
  propagation (property hierarchy, **inverse materialization**, role chains incl.
  3-hop) → **functional/`≤1` merge of `∃R`-as-type markers** (reaches family's
  `∃hasSex.Male ⊓ ∃hasSex.Female` → `Male⊓Female` clash) → disjoint/⊥ clash. A
  derived clash ⇒ `inconsistent`; **no clash ⇒ no verdict, falls through to the
  hybrid path unchanged** (under-approximate: `∀`-driven / `≤n>1`-choice /
  disjunctive inconsistencies are not handled here — a MISS at worst, never an FP).
  **Sound by construction** (every derived type/edge/merge is entailed).
  `has_abox_axioms`-guarded ⇒ ABox-free inputs skip it (zero cost on
  galen/ore/etc.; EL classify byte-identical). Whole-corpus bake-off (2026-06-20):
  **family detected inconsistent in 1.6s, FP=0/MISSED=0 byte-identical corpus-wide.**
  A second integration variant (A-gated, backward/inverse propagation *inside* the
  EL saturator, `RUSTDL_ABOX_SAT_GATED`) tied B on every axis; B was chosen for
  isolation (separate module, zero classification risk by construction); the
  A-gated branch (`feat/abox-sat-A-gated`) is kept for a future bake-off with
  inverse-aware *classification* work. Spec:
  `docs/superpowers/specs/2026-06-20-abox-saturation-consistency-design.md`.
  **Indexed (2026-07-23, v0.3.36/v0.3.37).** The fixpoint's two brute-scan hot
  loops were indexed, verdict-preserving (the checks write only `.clash`; 79/79
  ORE ABox onts + curated corpus verdict-identical indexed-vs-brute):
  (1) **role-chain closure** — the inner "r-edges leaving node b" leg was an O(E)
  rescan per chain rule per iteration over the full edge set (family: ~267k-edge
  transitive closure); now `(role,src)`/`(role,dst)` indexed
  (`RUSTDL_ABOX_CHAIN_BRUTE=1` reverts). (2) **Rule 8 disjoint-clash + Rule 7b
  functional-marker clash** — were `O(|disjoint_pairs|×|individuals|)` /
  linear-scan per iteration; now a `disjoint_of` symmetric class adjacency +
  normalized membership set, Rule 8 type-driven (`RUSTDL_ABOX_DISJOINT_BRUTE=1`
  reverts). Impact: **family** inconsistency detection ~21 s → ~0.7 s;
  `ore_ont_9899` pre-check ~27 s → ~0.5 s. Plans:
  `docs/superpowers/plans/2026-07-23-abox-saturation-{chain,disjoint}-index-plan.md`.

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
walls Konclude wins on every real-reasoning ontology, 2.2×–809×; the "beats
Konclude"/"ORE-10908 ≤5×" claims were docker artifacts). **rustdl is sound
corpus-wide (FP=0) and, as of 2026-07-12, sound *and complete* on the whole
curated corpus vs the Konclude∩HermiT oracle — galen (the last holdout) now
classifies with MISSED=0. Two independent default-ON fixes got there: the
functional/≤1-role merge across an inverse edge (`RUSTDL_INVERSE_FUNC_MERGE`,
made incremental in `horn_fixpoint` on 2026-07-11 so it derives the merge fast
instead of the old whole-graph re-fire that made galen DNF; `=0` reverts to
the pre-fix, MISSED=10 behaviour) closed 9 of galen's 10 misses; the label-cache
back-fold (`RUSTDL_CLASSIFY_BACKFOLD`, default ON since 2026-07-12) closed the
10th — a defined-class ∃-monotonicity subsumption, via a sound branch-free
direct `∃`-composition rather than the disjunctive ¬-expansion + ∀-propagation
path; see
`docs/known-limitations/galen-inverse-functional-completeness.md` and
`docs/known-limitations/galen-defined-class-monotonicity-residual.md`. `wine`
classifies (201–203 subs) but only under a per-pair budget — unbounded it DNFs,
and wall is ~linear in the budget (~12 s at `--pair-timeout-ms 25` up to ~80 s at
`200`, re-measured 2026-07-23; the earlier "~1.8 s" figure does not reproduce).
Its cost is the deep per-pair SROIQ wedge search (a fixed hard-pair tail), the
same frontier as the ORE hard cases — NOT the ABox/classify-index paths the
2026-07 perf work sped up. This completeness result is scoped
to the curated corpus — the ORE/BioPortal tiers are untested here and remain an
empirical, not provable, claim.** The
remaining rustdl weakness is the multi-GB RSS tail on a few pathological SROIQ
inputs, not wall time. Performance claims in docs are backed by the corpus harness
— re-measure with `scripts/bench-rustdl-modes.sh` (on a **freshly built** binary,
per the toolchain gotcha above) rather than trusting stale numbers.
