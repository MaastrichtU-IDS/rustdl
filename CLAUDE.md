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
> **SUPERSEDED 2026-08-03 (v0.4.13) — THE WINE GUIDANCE BELOW IS OBSOLETE. Read this first.**
> **Unbounded `classify ontologies/real/wine.ofn` now completes in ~74 s with no flags**, so
> everything below about wine DNF-ing, needing `--pair-timeout-ms 25`, or having a wall "~linear
> in the budget" describes pre-v0.4.12 behaviour. Under a budget it is now far faster still:
> `--pair-timeout-ms 25` runs in **4.6 s** (was 108.8 s) with identical subsumptions.
>
> **Use `wine` as a freshness canary by its WALL, not its outcome**: **~2.7 s at the DEFAULT**
> on a current binary (v0.4.19), 197 subsumptions. History, because each step is a different
> binary and mixing them misreads a change as staleness: v0.4.13 ~74 s unbounded / ~4.6 s at
> `--pair-timeout-ms 25`; v0.4.18 ~38 s / ~2.7 s (conversion + hot-loop fixes); v0.4.19 ~2.7 s
> at the default, because **the default per-pair budget is now 5 ms, not 1000**. A wine near
> ~38 s means a pre-v0.4.19 binary; near ~74 s means pre-v0.4.18. A wine that DNFs unbounded is now itself the
> stale-binary signal.
>
> **Cause:** `HYPER_WEDGE_DEPTH = 256` was a fixed constant, and a fixed constant is wrong in
> both directions — `ore_ont_10407` genuinely needs depth **319** (at 256 it does **4.4× more work
> than completing**, because a capped branch cannot conclude so the search re-descends through
> sibling disjuncts), while `ore_ont_2182` needs only **≤7**. Replaced by iterative deepening
> (`RUSTDL_ITERATIVE_DEEPENING`, **default ON** since v0.4.12, `=0` reverts): **16 ORE recoveries,
> 0 regressions, 0 closure losses**, FP-safe by construction since a depth cap can only *suppress*
> an `Unsat`, never manufacture one.
>
> **This also retires the "wine wall" closure recorded elsewhere in the design record** as
> requiring a nominal re-architecture with "no cheap entry". It required neither; nominals were
> never touched. The nominal-blocking hypothesis was separately **refuted** — blocking fires on
> 18.4%/20.4% of eligible nodes in the failing ontologies against 5.5% in a same-profile ontology
> that classifies in 0.03 s. See `docs/2026-08-02-iterative-deepening-results.md`,
> `docs/2026-08-02-cardinality-rootcause.md`, `docs/2026-08-02-nominal-blocking-rootcause.md`.

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
  can't cut it (every cycle node is nominal-tainted, excluded from `is_blocked_anywhere` at `lib.rs:1021/1062`)

> **CORRECTED 2026-08-02 — "BOTH predicates" WAS WRONG, and it matters.** Only
> `is_blocked_anywhere` (`owl-dl-tableau/src/lib.rs:1073`) excludes nominal-tainted nodes
> ("A nominal `y` denotes a fixed individual and must not be blocked").
> **`is_blocked_ancestor` (`:961`) has NO such exclusion** — its sole early `return false` is
> for a missing parent — and **classify uses ancestor-blocking by default**. So the deferred
> issue-#35 v4 nominal-aware-blocking redesign **does not apply to the classify path at all**,
> and a plan aimed there would have been aimed at nothing. Verified by reading both predicates.
>
> The measurement that exposed it also refutes the hypothesis outright, in the opposite
> direction: on `ore_ont_2182`/`16481` blocking fires on **18.4%/20.4%** of eligible nodes
> against **5.5%** on `ore_ont_7668`, a same-profile ontology that classifies in 0.03 s.
> Blocking fires 2.2 M times and `RUSTDL_MAX_NODES` is never hit — the graph stays under 200
> nodes. **The search TREE explodes, not the model.** See
> `docs/2026-08-02-nominal-blocking-rootcause.md`.). **Fix shipped = a sound
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

> **THE COMPLETION-GRAPH HALF OF ISSUE #35 v4 IS CLOSED (2026-08-05) — by
> `RUSTDL_DOMAIN_ABSORPTION` becoming the default, and it was NOT the fix anyone was
> looking for.** The load-bearing gate
> `nominal_first_bounded.rs::issue35_v4_completion_graph_is_bounded`, `#[ignore]`d
> since 2026-07-23 *for failing*, is now **live and passing**. Measured on that gate,
> cap OFF: `RUSTDL_DOMAIN_ABSORPTION=0` **does not terminate** (killed at 300 s); the
> new default passes in **0.00 s**.
>
> This is causal, not coincidental, and the text above says why without drawing the
> conclusion: v4's root cause is `ObjectPropertyDomain(r,A)` absorbing to an
> **untriggered residual** `⊤ ⊑ ¬∃r.⊤ ⊔ A`. Domain absorption is *precisely* the pass
> that converts that residual into a **triggered** role rule, so it is never offered on
> a fresh `≥2 r.C` witness and the generating cycle never opens. The 2026-07-23
> conclusion that this needed "a nominal-aware-blocking / NN-rule redesign" was
> **wrong about the required mechanism** — a preprocessing pass that already existed,
> behind a flag, was sufficient. `RUSTDL_NOMINAL_FIRST` remains falsified and OFF;
> nothing here rehabilitates it.
>
> Scope, stated precisely: this closes **graph termination on the v4 reproducer**. The
> `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` and `RUSTDL_MAX_NODES` safety nets stay — they
> protect every pattern domain absorption does not happen to cure, and the companion
> `nominal_first_bounded_divergence_canary.rs` now **pins `RUSTDL_DOMAIN_ABSORPTION=0`**
> in order to keep exercising the `NodeCap` net at all. Whether HermiT-matching
> *realization* on this pattern now follows was **not** re-measured; do not upgrade that
> claim without measuring it.
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
  **Per-CLASS sibling (2026-08-01, `RUSTDL_CLASSIFY_LABELS_AMORTIZE`, **DEFAULT ON
  since 0.4.10**, `=0` reverts — this entry said "default OFF, `=1` opts in" until
  2026-08-17 and was wrong; so did the function's own doc header, while the comment
  inside the predicate said ON. Measured on `ore_ont_12698`, `--pair-timeout-ms 1000`,
  single-thread: unset **5.2 s** / `label_cache_build` 2,028 ms, `=1` 5.7 s / 2,039 ms,
  `=0` **108.1 s** / 104,237 ms — unset behaves as `=1` and disabling costs ~20×.)** That v0.3.39 amortization reached only the per-**pair** oracle.
  `HyperCache::classify_labels` appends the SP2.1/SP3 seed clauses, which are absent
  from `base_indexes`, so it fell back to `HyperEngine::new` — **a full O(#clauses)
  index rebuild per class**, and since `RUSTDL_SAT_SEED` defaults ON that was the
  always-taken branch (the amortized branch was dead code). The flag routes it through
  the same `build_clause_index_delta` + `new_with_prebuilt_extras`, keeping the seed
  clauses. **This settled an explicit R3-vs-R4 reviewer dispute** (both had measured
  with `RUSTDL_SAT_SEED=0`, which removes the rebuild *and* the clauses): min-of-3
  interleaved, pinned binaries, `ore_ont_1508` **197.83 → 94.89 s (−52.0%)** and
  `ore_ont_12698` **98.78 → 5.33 s (−94.6%)**, with `label_cache_build` 163.3 → 59.9 s
  and 95.7 → 2.05 s. **Arm C beats the `SAT_SEED=0` arm on both** (114.12 s / 49.80 s),
  so the cost was the rebuild, not the clause volume. Verdict-preserving: FP=0 net
  flag-ON manifest **identical** to flag-OFF (11 VERIFIED, closures exact).
  **MEASUREMENT WARNING:** under a *truncating* `--pair-timeout-ms` the hierarchy is
  **not run-to-run deterministic** on hard ontologies — `ore_ont_1508` at 20 ms varied
  57–68 timed-out pairs over five runs of ONE binary, and two runs of the SAME binary
  differed by 4 `direct` rows. Compare only at a non-truncating budget (the in-tree
  gate pins `--pair-timeout-ms 1000`); at 1000 ms A vs C is byte-identical, 13 950 rows.
  The "pending a broader ORE bake-off" note is **RESOLVED — the flip already happened
  in 0.4.10**; the candidate population it named (the DNF cluster stalling at
  `after_prepared`, i.e. the label-cache build) is therefore already covered by the
  default. Do not re-run that bake-off expecting an un-flipped flag.
  Spec `docs/superpowers/specs/2026-08-01-clauseindex-per-class-adjudication.md`
  (§ OUTCOME), canaries `classify_labels_amortize_tests` in `reasoner/src/lib.rs` +
  `crates/owl-dl-cli/tests/classify_labels_amortize_identity.rs`.
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
  oracle parity); full `family.ofn` was then a sound MISS — a *scale* stall
  (transitive closure + disjunctive depth) deferred to SP2.
  **SUPERSEDED 2026-08-18: `family.ofn` IS detected.** The
  `family_inconsistency_detected` gate reports
  `[fp0] family (inconsistency): VERIFIED (is_consistent=false)` in ~4 s
  — closed by `abox_saturation` (2026-06-20, indexed 2026-07-23) and
  `RUSTDL_CLASSIFY_INCONSISTENCY` (v0.4.8), not by SP2. It stays
  `#[ignore]`d **only because `family.ofn` is gitignored**, which is a
  different reason from the one recorded here for weeks. `family-stripped`
  is still unverified — its fixture is ABSENT, and that test's
  skip-if-absent guard makes it **pass VACUOUSLY**, so its green is not
  evidence of anything.
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
  `PreparedOntology` build on ABox-free inputs. Stretch goal not met *at
  A1*: family / family-stripped (both HermiT/Konclude-inconsistent in
  <1 s) timed out — their clash needs functional-role-merge of
  `∃hasSex.Female ⊓ ∃hasSex.Male`, beyond P7's range augmentation.
  **RESOLVED for `family.ofn` (verified 2026-08-18, ~4 s) — by
  `abox_saturation` + `RUSTDL_CLASSIFY_INCONSISTENCY`, not by extending
  A1's pre-check patterns.** Do not cite "family still timeouts" as a
  current A1 limitation; `family-stripped` remains untested for want of
  a fixture, NOT measured-and-failing. Next scoping target documented at
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

  > **FP FOUND AND FIXED 2026-08-01 (v0.4.9) — THIS SECTION'S OWN INVARIANT WAS VIOLATED.**
  > `parse_float_oneof` folded `xsd:float` and `xsd:double` into ONE f64-keyed `fo:`
  > bucket, so `∃h.DataOneOf("1.0"^^xsd:float)` and `∃h.DataOneOf("1.0"^^xsd:double)`
  > were reported **EQUIVALENT**. Reproduced on a pinned **v0.4.6** binary, so it is
  > long-standing, not a recent regression. Konclude declares both classes and reports no
  > relation; the discriminating control (float-vs-**float**, where Konclude DOES report
  > equivalence) is what proves its silence is a genuine non-entailment rather than the
  > under-reporting Konclude does elsewhere. Fixed by splitting `fo:` (f32-rounded) from a
  > new `dbo:` (f64) bucket — **unflagged**, since a soundness fix is not opt-in.
  >
  > **Why the FP=0 net did not catch it, and what that means:** the curated corpus is
  > INERT for this area, exactly as `datatype_value_membership.rs` warns —
  > *"the corpus has NO such clash, so these canaries are the ENTIRE safety net."* A
  > green FP=0 net over the curated fixtures is therefore **not** evidence of soundness
  > for DKey work; only the canaries and a Konclude ∪ HermiT adjudication are. Treat any
  > future claim of "FP=0, corpus-wide" in this subsystem as non-regression only.

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

> **JUSTIFY/REPORT OUTPUT WAS NOT RUN-TO-RUN REPRODUCIBLE BEFORE 2026-08-17 (#59).
> Any byte-comparison of these surfaces recorded before that commit compared
> noise.** `SetOntology` is `SetIndex(HashSet<AnnotatedComponent>)` and std seeds
> `RandomState` per PROCESS, so `logical_axioms` yielded a different axiom order on
> every run; `extract_bot_module` preserves that order and QuickXplain/HST walk their
> input in order. Five runs of ONE unchanged binary produced **five different**
> `report` files on `pizza.ofn`, and on a fixture with four independent derivations of
> `A ⊑ C`, ten runs of `justify` returned **four different justifications** — each
> valid and minimal, so this was never an FP, but the answer moved. `localized_candidates`
> now sorts the search input (`canonical_order`), which stabilizes **selection**, not
> just display: same justification, same order, same subset under a `max` cap.
> Pinned by `justification_axioms_come_back_in_canonical_order`.
>
> Two consequences. (1) Byte comparison of `justify`/`justify --all`/`repair`/`report`
> is now a valid A/B method — before, only comparing the axiom SETS was.
> (2) A pre-#59 binary is still nondeterministic, so when one arm of a comparison is
> old, compare sets. This is unrelated to the `--pair-timeout-ms` classify
> nondeterminism documented under `owl-dl-tableau` — that one is budget-induced and
> still live; this one was hash-order-induced and is closed.
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

> **Two silent fallbacks were made OBSERVABLE on 2026-08-18, and one remains silent.** Sound
> under-approximations are pervasive here, which is correct — but several were also
> *invisible*, and an invisible sound-MISS is indistinguishable from a complete answer:
>
> * `ClassificationStats::prep_unbounded_budget_spent` — a global budget was set and prep was
>   left UNBOUNDED because the budget was already spent. Distinct from `prep_timed_out`
>   (abandoned ⇒ incomplete); this one means prep ran to completion unbudgeted. Its absence
>   cost PRs #61 and #62, each chasing the same host-speed canary flake.
> * `Realization::incomplete` / `realize --json` `incomplete` — at least one instance probe was
>   CUT (deadline / `RUSTDL_MAX_NODES` / depth bail) rather than refuted. `realize` previously
>   had NO completeness signal at all. Reports cut probes ONLY.
> * **Still silent:** `realize` dropping DERIVED individual equality. No probe is cut, so the
>   new field does not cover it —
>   `docs/known-limitations/realize-drops-derived-individual-equality.md`.
>
> When adding a sound under-approximation, ask whether a caller can *tell*. If not, add the
> signal in the same change.


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

## State of play — 2026-08-18 (read this first)

**The dominant finding of the 2026-08-17/18 sessions is about this file, not the code: the
design record has drifted ahead of the engine, consistently in the OPTIMISTIC direction.**
Five separate proposals during those sessions targeted work that was **already shipped**, and
three named open targets had **already evaporated**. Before planning against anything below,
re-verify it against current `main` — that check retired more work than it cost, every time.

Concrete instances, all corrected:

| stale claim | reality |
|---|---|
| `RUSTDL_CLASSIFY_LABELS_AMORTIZE` "default OFF, bake-off pending" | **default ON since 0.4.10**; `=0` is ~20× slower (`ore_ont_12698` `label_cache_build` 2,028 → 104,237 ms). The bake-off it called pending had already happened. |
| `label_cache_timeout_ms` listed as "dead code" in the constant audit | live and load-bearing at 18× (already corrected 2026-08-06) |
| CB arc "8 in-fragment DNF targets" | all 8 complete on v0.4.6 |
| `ore_ont_10019` dense-SROIQ over-branching target | now classifies in **2.3 s**; the diagnosed-but-unbuilt fix has no target |
| "inverse blocks 71% of the DNF tail" | true as a *fast-path blocker* count; an inverse-capable engine reaches **13%** — the biggest single lever is **cardinality (+44)** |

**Guardrail now in place:** **43 distinct** boolean `RUSTDL_*` flag defaults are pinned
behaviourally — 31 in `crates/owl-dl-reasoner/tests/flag_defaults.rs` (the public accessors)
and 13 in `internal_flag_defaults` in `lib.rs` (the `pub(crate)` ones), overlapping on exactly
one: `RUSTDL_CLASSIFY_LABELS_AMORTIZE`, which is the flag that actually drifted and is therefore
pinned from both sides.
>
> Counted, not remembered. An earlier draft of this paragraph said "45", then "29 flags across
> 30 rows because `ANYWHERE_BLOCKING` appears twice" — both wrong, corrected only by running the
> count. That is the failure mode this whole section documents, reproduced while writing it.
A default change now requires editing the table in the same commit. **Do not audit defaults by
parsing doc comments** — that was tried and was wrong four times running (historical
"was default-ON" narrative, `pub(crate) fn`, `remove_var` inside `#[cfg(test)]`, and defaults
stated in `//` not `///`).

### AN `#[ignore]`d TEST IS AN UNCHECKED CLAIM ABOUT THE ENGINE (2026-08-18)

Two `#[ignore]`d sentinels in `functional_enforcement.rs` existed *to trip when an engine gap
closed*, each carrying an explicit instruction for that moment. **The gap closed on 2026-07-11
(`RUSTDL_INVERSE_FUNC_MERGE`) and nothing noticed for five weeks**, because an `#[ignore]`d test
that starts passing emits exactly what a still-failing one does: nothing. The comment block they
anchored asserted the engine "does not perform the `≤1 R⁻` predecessor merge, so emitting
`∃R⁻.⊤ ⊑ ≤1 R⁻` would be a silent no-op" — **false, and it is why that axiom went unwritten**,
which is the whole of the `realize` derived-equality defect.

Second-order, and the more useful half: sentinel 1 passes at the default via `abox_saturation`,
**not** via the calculus its file header claims to isolate — `RUSTDL_ABOX_CHECK=0` disables the
A1 pre-check but not `abox_saturation` (default ON since 2026-06-20). Un-`#[ignore]`ing it on
"it passes now" would have recorded coverage it does not provide. **Attribute a newly-passing
sentinel to an engine before believing it.**

Both are now live (sentinel 2's assertion flipped, as it instructed), plus a canary pinning the
discriminating experiment. This is the sibling of [[sabotage-your-own-guard-tests]]: that one is
"your guard may not guard"; this one is "your `#[ignore]` may be a stale claim". Cheap
mitigation: run `cargo test -- --ignored` and triage every **pass** as a failure. **Counted:
67 `#[ignore]` attribute sites, of which 50 are fixture/cost and 6 already say "PASSES via …",
leaving ~11 falsifiable "this fails" claims** — the sweep is cheap because that population is
~11, not the ~80 the suite's runtime `ignored=` line suggests (a grep for lines *containing*
`#[ignore` returns 97, inflated by prose; anchor the pattern to the line start). See
`docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`.

### THE PARKED DKey RESIDUAL CLASS OWNS 8 OF THE DNF TAIL (2026-08-20) — UNPARK CASE

`docs/benchmarks/2026-08-20-dkey-residual-class-unpark-case.md`. Found by reading ONE ontology,
not a population. **`ore_ont_10929` is a DNF with 12 CLASSES**: `tbox-stats` (parse+convert only)
is 97.6 s of which `convert_ms` **96,461** — 99% conversion, ~0% reasoning, on ~244k `ABox`
assertions (110,802 `DataPropertyAssertion`, **60,323 distinct literals**).
`RUSTDL_DATA_PROPERTIES=0` → **2.6 s, `convert_ms` 1,480, rules 57,355→12 (36×)**. The merging gate
changes nothing (it **correctly declines** — 2 `FunctionalDataProperty` + 25 `DataPropertyRange`
make the component merge-inducing), and `RUSTDL_BOUNDED_DKEY_DISJOINT=0` is *worse* (203 s).

**Measured, not grepped** (candidates selected by grep, then each measured both arms): **8 tail
members are conversion-bound**, aggregate **1,182 s → 14 s (82×)** — `8445` 275×, `4141` 223×,
**`5368` 194×** (the 27 GB DNF the spec named "the strongest candidate" for unparking), `4572`
125×, `2504` 103×, `1833` 59×, `10929` 36×, `15635` 19×. `4572` and `8445` do not finish
conversion at 300 s. `ore_ont_5548` carries the signature but converts in 0.7 s — a **false
member**, so the set is 8 not 9.

**SUB-CLASSES, and this CORRECTS the "8" above.** Pair profile (`RUSTDL_DKEY_SPLIT_STATS=1`):
**2 members are 100% wasted enumeration** (`15635` 294,744,041 pairs, `10929` 248,465,112 — all
dropped) and **4 genuinely materialise** axioms (`2504` 68.7 M / 98 dropped, `4141` 42.7 M / 6,
`5368` 18.6 M / 0, `1833` 14.0 M / 0), with `8445`/`4572` not finishing enumeration at 300 s. Only
the materialising ones are ORACLE cases, so **the oracle's set is ~4–6 — essentially the spec's
original "~4", and its work-to-reward parking judgement STANDS.** My "8 in the tail" conflated two
defects.
**PARTIAL FIX BUILT — `RUSTDL_DKEY_GROUP_SKIP` (default OFF):** the droppable block is exactly
value-only × value-only, skippable in O(k) instead of O(k²). `10929` 96.5→77.5 s (1.24×), `15635`
92.2→67.4 s (1.37×), `5368` unaffected as predicted; verdict-preserving (`tbox-stats` identical
6/6 **with timing stripped** — `convert_ms` is in that output, so raw hashes report a spurious
DIFFER even on `9347` where the gate skips entirely; curated classify identical; suite 1685/0). A
first attempt requiring the WHOLE group value-only **did not fire** (96.5→94.2 s) — one broadcast
key forming no disjoint pair defeats it at a 100% drop rate.
**IT IS NOT THE FIX: 77 s of 96 s remains** vs the 2.6 s `DATA_PROPERTIES=0` reaches. The skip
touches only the DISJOINTNESS loop; the leading suspect for the residual is **`seed_bucket`, the
SUBSUMPTION seeding**, which walks **k² ORDERED pairs** (~3.6 × 10⁹ at 60,323 distinct string keys,
~26 ns each ≈ the residual). **That is arithmetic, not a measurement — probe the two seeding calls
before touching the loop.**

**This moves the parked decision.** `RUSTDL_DKEY_MERGING_GATE`'s spec parked Lever 2 (on-demand
disjointness oracle) at "~4 ontologies … poor work-to-reward", revisiting "if one of the four is
independently needed (`5368`)". The set is **8 in the tail (~6%)** and includes `5368`.
**Caveats that must travel with it:** (1) **82× is a CEILING, not the lever's value** —
`DATA_PROPERTIES=0` deletes the semantics (same distinction as `RUSTDL_ABOX_CHECK=0`; quote the
fraction, never the bound); (2) the cost is isolated to the data path **as a whole**, NOT to
seeding specifically — separate seeding from DKey interning/lowering before designing; (3) **price
the cheap option first** — merge-inducing ≠ all C(k,2) pairs consumable, so a tighter
consumability test may capture most of the win with no new side-table hooks.

### DNF TAIL RE-CENSUS 2026-08-20: 143, and the 55% bucket is MEASURED OUT

`docs/benchmarks/2026-08-20-tail143-recensus.md`. Tail is **143** (141 DNF + 2 reject), from 257
(Aug 1) and 164 (Aug 12). Partition on current `main`: `label_cache_build` **78 (55%)**,
all-phases-zero **42 (29%)**, `tier_walk` 13, `prepare` 8, `sweeps`/`saturate` 2.

* **`label_cache_build` is dominant by WALL and measured out as a LEVER.** All 78 report
  `pruned=0` — the cache is built then consulted **zero** times — for a median **17.3 s** of a
  20 s budget. Disabling it (`RUSTDL_LABEL_HEURISTIC=0`) over 78×2 arms: **76 BOTH_DNF, 2
  rescued** (`ore_ont_10109`, `6333`), **0 ontologies lost rows**, aggregate wall **flat
  (+0.8%)** — the freed time is absorbed by the next phase, *confirming* the `unsat_probe_cap`
  negative. Prize is **2 ontologies, not 78**.
* **PARSER TRAP, and it probably corrupts the 2026-08-12 census too:** on the **pure-EL path every
  phase in `# wall breakdown ms:` is `0`**, so a "largest phase" parser silently buckets those as
  `no-banner` (36 of 164 there; 42 here). Handle the all-zero case explicitly.
* **The `incomplete` counter counts pairs ATTEMPTED AND CUT, not pairs REMAINING.** I read 31
  ontologies reporting "1 class pair" as *one pair short of complete* — false: raising the budget
  20 s → 60 s takes `ore_ont_11196` from inc=1 to **inc=15,042** with row count unchanged. **A
  small `incomplete` is not evidence of near-completeness.**
* Genuinely un-attacked reasoning stalls ≈ **29** (`tier_walk` 13 + `prepare` 8 + 8 zero-output
  stalls); the rest is large-ontology scale (20k–190k classes). **Next step is to read ONE failing
  ontology, not another population** — both historical tail wins came from a single instance, and
  three population studies here have now been retracted or bounded, two of them in that document.

### RE-VERIFICATION PASS 2026-08-18: of 12 named targets, 2 still fail

`docs/2026-08-18-named-target-reverification.md`. Re-measured the 12 most-cited `ore_ont_*` at
the default, single-thread, 120 s: only **`ore_ont_11311` and `ore_ont_9944`** are genuinely
unfinished. `5368` completes in 85.6 s (the 27 GB DNF framing is stale), `1508` 112.9 s, `9347`
5.6 s, `10019` 2.3 s, `16847` 0.4 s. The small-pair-timeout label-cache starvation stopped reproducing on
both its *named* instances and I recorded it RETIRED; the census then appeared to refute that with
"5 members"; a 2×2 then showed those 5 are **a different defect entirely** (cache-insensitive).
Net: the documented trigger does not fire, the cache is still load-bearing, and there is a NEW
unexplained `pt`-sensitivity. **Three readings in one day — see the entry above for the one that
survived measurement.**

**The near-miss is the transferable part:** I first read `15010` at 6.00 s against the *103.98 s*
in the doc's title and nearly declared it closed — but 103.98 s is the `--pair-timeout-ms 1` arm
and the doc's *default* arm is 5.65 s, so 6.00 s was a **match**. **Read the conditions recorded
WITH a number before calling it non-reproducing.**

### Behaviour changes shipped 2026-08-17/18

| change | effect |
|---|---|
| `--global-timeout-ms` charges parse time (`global_budget_after_parse`) | the flag meant "N ms of *reasoning*"; parse was free. `ore_ont_7192` 72 s → 55 s, rows identical. Inert at the default `0`. |
| **`RUSTDL_PREP_DEADLINE` flipped default-ON** behind `prep_bounding_active` | bounds prep only while the budget is still MEETABLE, falling back to unbounded once it is not — so the bounded path is never worse than the unbounded one. −37.4% wall at a 20 s budget over 45 ontologies, **0 row changes**. It shipped OFF because bounding an already-spent budget made `ore_ont_7192` pay ~18 s and return **0 rows** where unbounded returned 50,753. |
| `realize_saturation_eligible` refuses functional/inverse-functional + `ObjectPropertyAssertion` | the fast path has no equality folding, so a forced `x = y` silently dropped the twin's types. Functional case now correct. **CORPUS-INERT** — fires on 0 of 64 qualifying ORE ontologies; do not cite as a corpus win. |
| `ClassificationStats::prep_unbounded_budget_spent` (new) | the prep fallback above was unobservable, which cost PRs #61 and #62 chasing the same host-speed canary flake twice |
| `Realization::incomplete` + `realize --json` `incomplete` (new) | realize had **no** completeness signal while classify has had one for months. Reports CUT probes only — it does NOT cover the derived-equality gap, where nothing is cut. |

### Open, with evidence

* **`realize`'s dropped derived equality is now CLOSED on both halves — the second half by ONE
  MISSING AXIOM, not the "second engine" the spec predicted.** The functional half shipped
  earlier (gate refuses functional-ish role + `ObjectPropertyAssertion`, so the tableau folds it).
  The inverse-functional half: `hyper.rs`'s predecessor-walking merge (`RUSTDL_INVERSE_FUNC_MERGE`,
  default ON) was **already implemented and reachable** — it is triggered by an explicit `≤1`
  constraint, and `convert.rs::derive_functional_max_cardinality` emitted `∃r.⊤ ⊑ ≤1 r.⊤` for
  `FunctionalRole` **only**, so the shared filler never got the constraint that fires the merge it
  needed. `RUSTDL_INVERSE_FUNC_MAX` (**default OFF**) emits the inverse counterpart; the fixture
  goes `x:A`/`y:B` → `x:A,B`/`y:A,B`, its `#[ignore]`d canary is retired, and a negative control
  pins the flag load-bearing. The gate learned the new shape
  (`is_derived_inverse_functional_max`) so the fast path is NOT lost — all three
  `inverse_functional/` fixtures stay `# mode: pure EL` at both settings, closures identical on
  them plus pizza/ro/sio. **OFF because it emits an axiom into every inverse-functional-bearing
  ontology**: a flip needs the two-arm ORE sweep + a ΔMISSED arm, per this file's own record that
  a 12-ontology benchmark is not a population. Flipping it is also what would let
  `RUSTDL_PSEUDO_MODEL` recover the soundness-by-construction argument falsified below.
  **THE FLIP SWEEP IS DONE (2026-08-18) AND THE ANSWER IS STAY OFF — but it is NOT inert.**
  Frame = the **109 of 1,920** ORE ontologies carrying `InverseFunctionalObjectProperty` (the flag
  is inert by construction elsewhere; a grep *superset*, so it cannot miss one). Classify:
  **89 IDENTICAL, 16 both-DNF, 1 rows-differ, 3 REGRESSED** — `ore_ont_9662` 2.26 s, `7532`
  2.82 s, `9786` 7.43 s all → **DNF at 120 s**, re-run sequentially on an idle host at double the
  cap, OFF walls reproducing within 0.04 s (so not the concurrency confound), and still unfinished
  at 120 s (so not slowdowns). Realize: **0 gains** over 22 usable of 53 — corpus-invisible, like
  its functional sibling's 0-of-64. **But the one row-difference is a real completeness gain**:
  `ore_ont_13859` closure 6253 → 6270, **+17 gained / 0 lost, all 17 confirmed by Konclude's
  transitive closure**. So this is a **performance-blocked completeness gain**, not a dead end;
  wall over the 90 both-completed is flat (85.3 vs 84.9 s), the cost being concentrated in three
  ontologies. Mechanism is a **HYPOTHESIS, unmeasured** — the GCI puts an `at_most` on `r⁻` at
  every node with an `r⁻`-successor, plausibly re-creating the cost class that made galen a
  6.6-min DNF before the merge was made incremental. A retry costs <1 h (109 ontologies) and
  should first confirm that mechanism, then narrow the trigger to where a merge is *consumable*
  (the `RUSTDL_DKEY_MERGING_GATE` pattern). Until then **`RUSTDL_PSEUDO_MODEL`'s soundness-by-
  construction argument stays falsified**, since restoring it requires this flag ON. See
  `docs/2026-08-18-inverse-func-max-sweep.md`.
* **`realize` dropped DERIVED individual equality, and the root cause FALSIFIES a shipped
  soundness claim** — `docs/known-limitations/realize-drops-derived-individual-equality.md`.
  The residual is **the pseudo-model prune, not the tableau**: `rustdl justify … instance x B`
  PROVES the merged membership (4-axiom minimal justification), `RUSTDL_PSEUDO_MODEL=0` returns
  it, and the default prunes it. `RUSTDL_INVERSE_FUNC_MERGE` is a red herring.
  **`RUSTDL_PSEUDO_MODEL` (default ON) is documented as "sound by construction — an entailed
  type is in every model, hence in the witness, hence never pruned". That argument needs the
  witness to BE a model, and it is not on an inverse-functional `ABox`: the witness applies
  FUNCTIONAL merges but not INVERSE-functional ones.** Subtractive, so FP=0 is intact — but the
  falsified clause is the stated basis for shipping default-ON *without* the ORE
  verdict-identity bake-off, so **that bake-off is now load-bearing, not optional**. Fix belongs
  in the `ABox`-seeded wedge consistency completion; workaround `RUSTDL_PSEUDO_MODEL=0`.
* **The label cache is consulted ZERO times under a global budget** on `ore_ont_11311`/`9944`:
  58 s of build then `pruned=0 pass_through=0 misses=0`, with `tier_walk` aborting in 8 ms
  (`docs/2026-08-17-classify-has-no-budget-allocation.md`). Classify phases consume the deadline
  greedily in sequence with **no allocation**, so whichever runs first starves the rest — which
  is why the timeout sweep found the DNF tail budget-invariant. Capping one phase just hands the
  budget to `unsat_probe` (1 ms → 47 s). **Starvation-unblocking is already a MEASURED NEGATIVE
  (`unsat_probe_cap`): it rescues nothing, because the starved consumer cannot finish either.**

### Measured out 2026-08-16/18 — do not re-propose without new evidence

* **The shipped timeout defaults are already optimal for complete classifications.** Complete
  results plateau from `--pair-timeout-ms 5`; the DNF tail moves by **2 ontologies across a
  1000× budget range**. `--global-timeout-ms` can never *increase* the complete count (a firing
  deadline yields `incomplete` by construction).
* **Learned/adaptive cap tuning: both concrete formulations refuted.** Skipping predicted-failed
  label builds has no usable operating point (AUC 0.73, but skipping `|closure|>25` sacrifices
  1,253 successes to save 110 s); cheapest-first ordering is unreliable (+1,374 on one ontology,
  −605 on another). **AUC above chance is not evidence of a usable rule.**
* **CB: no cheap fragment.** Coverage of the 141-DNF tail — Horn-ELHI **17**, +role chars 22,
  +`Or`/`Not` 45, +`All` 46, **+`Min`/`Max` 90**, +nominals 101, +`Self`/DKey 125. Market grows
  with how much of SROIQ you implement; cardinality is the big step and its obvious lever
  (second-maximal) already measured **3× worse**. Horn-ELHI's market is **16 distinct
  ontologies** → DEFERRED. Gate + census live as dead code (`cb_eli_eligible`,
  `RUSTDL_CB_ELI_PROBE`) so re-sizing costs one command.
* **The label cache reproduces the saturation closure exactly** on 100% of completed classes on
  the DNF cases — but six mechanisms are now refuted against replacing it, and a certifier is a
  fragment gate needing a PROOF, not a benchmark.

### FP-critical audit (2026-08-18): no defect

`docs/2026-08-18-fp-critical-audit.md`. `InverseFunctionalRole` is admitted by
`saturator_complete_fragment` and **never read** by the saturator — sound because the fragment
excludes nominals, `ABox` and inverse role *use*, so the canonical model is a tree and every
witness has one predecessor. Verified against Konclude on three probes; the arm's comment
claimed a witness-merge that does not exist and is corrected. Bare-declaration observability
(`RUSTDL_FRAGMENT_BARE_DECL`, 44 ORE ontologies) is correct via a downward closure and is now
canaried. DKey bucket keying has **no drift**: 13 buckets, all in mutual-exclusivity matrices,
and re-pointing `parse_float_dkey_iri` at the double tag (the historical v0.4.6–v0.4.9 FP) fails
both.

---

## State of play — v0.4.13 (2026-08-03)

Seven releases (v0.4.7 → v0.4.13) moved the ORE picture materially. Read this before trusting an
older performance or "closed frontier" claim elsewhere in this file.

**Corpus:** rustdl now fails to classify roughly **157 of 1,920** ORE ontologies at a 60 s
single-thread cap, down from **257**. Verified by a full before/after sweep: **91 recoveries, 4
regressions (all fixed), 0 answer changes** among 1,633 completing ontologies, and no RSS growth.
**FP=0 exact throughout**, plus one genuine long-standing false positive fixed (see below).

**Defaults that changed** (each `=0` reverts):

| flag | effect |
|---|---|
| `RUSTDL_ITERATIVE_DEEPENING` | replaces the fixed `HYPER_WEDGE_DEPTH`; **16 recoveries**, wine DNF → ~74 s |
| `RUSTDL_FAST_DIRECT_SUBSUMERS` | an O(k²)-per-class Hasse reduction sat in the **output** loop — `ore_ont_10125` finished *reasoning* in 15 s then spent ≥385 s **emitting**; DNF@900 s → 14.6 s |
| `RUSTDL_FRAGMENT_BARE_DECL` | a bare `Symmetric`/`Inverse` **declaration** refused the fast path; **44 recoveries** |
| `RUSTDL_CLASSIFY_INCONSISTENCY` | `classify --json` and `consistent` no longer contradict each other |
| `RUSTDL_DOMAIN_ABSORPTION` (2026-08-05) | absorbs domain residuals into triggered role rules; **3 ORE recoveries, 0 answer changes over 1,750 both-arm completers**, and it **closed the completion-graph half of issue #35 v4** (a >300 s hang → 0.00 s). Cost: 2 ontologies ~3× slower, 1 crossing a 60 s cap, all with byte-identical output |
| `RUSTDL_EL_BOT_FILLER`, `RUSTDL_DKEY_POST_NNF`, `RUSTDL_DKEY_ONEOF_SEED`, `RUSTDL_DKEY_EMIT_ORDER` | four D10/completeness fixes, one of them a **live non-monotonic** defect |

**A REAL FALSE POSITIVE SHIPPED FOR MONTHS AND THE FP=0 NET COULD NOT SEE IT.**
`parse_float_oneof` folded `xsd:float` and `xsd:double` into one f64 DKey bucket, so
`∃h.DataOneOf("1.0"^^xsd:float)` and its `xsd:double` twin were reported **equivalent**.
Reproduced on a pinned **v0.4.6** binary. It escaped because the curated corpus is **inert** for
the DKey area, exactly as `datatype_value_membership.rs` warns. **Consequence for anyone working
here: a green FP=0 net over the curated fixtures is NOT evidence of soundness for DKey work** —
only the canaries plus a Konclude ∪ HermiT adjudication are. And prove an FP with a
*discriminating control*: Konclude's silence is ambiguous (it is documented to under-report), so
pair the probe with a case where Konclude *does* report the relation.

**Closed by measurement — do not re-propose without new evidence:**
- **Absorption of residual GCIs.** Driving 54 ontologies to **zero** residuals recovered **1**,
  while 77 that kept residuals recovered 3; per unit size the signal is **AUC 0.480, below
  chance**. Qualified-`∃` absorption is a documented NO-GO. Domain absorption ships default OFF
  (sound, 4 recoveries, but alters 1,030/1,913 absorbed TBoxes without a wall check).
- **Iterative deepening does not transplant to the main tableau.** The audit's apparent 14× came
  from probes that reach **no verdict at either depth** — the win is *giving up sooner*, and
  deepening is defined not to. Default OFF as a documented negative result.
- **Five constants**: `FIXPOINT_ITERS` (prior "structurally true" reading **refuted** — it does
  bind, and halving breaks a healthy ontology), `DIV_WINDOW` (null), `RUSTDL_MAX_NODES` (does not
  bind), `ID_SHALLOW_BUDGET_DIVISOR` (flat over 16×), `label_cache_timeout_ms`
(~~**dead code**~~ — **THIS ENTRY IS WRONG, corrected 2026-08-06.** `RUSTDL_LABEL_CACHE_TIMEOUT_MS`
is live by construction — it "always wins" (`lib.rs:2728-2730`) — **and the pathology it rescues is REAL but its
recorded TRIGGER does not fire.** This entry went through two wrong versions in one day; the
measurement that settled it was a 2×2 varying `--pair-timeout-ms` and the cache budget
INDEPENDENTLY.
  * The cache **is** load-bearing: forcing the 50 ms floor takes `ore_ont_15108` from 43.1 s to
    **DNF at 240 s**, and fires on **12 of 40** slowest completers (5 to DNF).
  * Its documented trigger — a small `--pair-timeout-ms` — **does not fire on that frame**. Where
    `n` is large, `n × per_pair` is already a sufficient budget (`15108` moves only 1.13× at
    `pt=1`).
  * **The 5 ontologies I first published as "live starvation members" are NOT starvation** —
    `14272`, `9864`, `6923`, `4827`, `8429` are `pt`-sensitive and cache-INSENSITIVE: at `pt=1`
    they are 2.7–3.4× slower for byte-identical output **at every cache budget including the
    default's own 30 s** (`14272` 73.3 s at 30 000 ms vs 21.9 s at default). That is a **NEW,
    separately-caused defect, cause unknown** — plausibly the tier walk losing prunable verdicts;
    untested.
  * **Why I got it wrong:** I read the agreement between arm B (`pt=1`, `n×per_pair` budget) and
    arm C (`pt=1`, forced 50 ms) as mechanism. Both hold `pt=1` FIXED, so their agreement shows
    only that the cache budget is irrelevant in both. **A control validates an INSTRUMENT; only
    varying the suspected cause independently ATTRIBUTES an effect.** Pre-registering the analysis
    (done) does not protect against mis-attribution (not done).
  The aggregate `pt=1` inversion is real and stands (1377 → 1625 s, +18%, vs the original's
  1499 → 1267 s), but it is now attributable to the NEW defect, not to starvation. See
  `docs/2026-08-19-label-cache-starvation-census.md` § THE ATTRIBUTION WAS WRONG.
  * **A FIX EXISTS: `RUSTDL_LABEL_CACHE_PROBE` (default OFF).** A FLOOR is the wrong fix —
    measured on the small-`n` population it costs **112% aggregate** and takes `ore_ont_9540`
    from 8.92 s/40 rows to **200 s/0 rows**. The working shape is a **differential escalation
    probe**: strided-scan ≤8 classes at the current budget for one that FAILS, retry that one at
    1000 ms, escalate only if the retry succeeds — bad-case cost is one escalated build,
    **independent of `n`**, which is the objection that kills a floor. The discriminator came
    from the counters: at 250 ms vs 1000 ms `9540` is `misses=340 → 340` (converts nothing)
    while `5107` is `misses=19 → 0`. **`ore_ont_5107` 6.65 → 1.92 s (3.46×)**, guard `9540`
    0.88× (vs **2.1× under naive escalation**), aggregate +1.5% over 19 addressable, **−2.3%
    (~50 ms) over 20 fast**, **0 row diffs across 39**. Three simpler fixes were refuted by
    measurement first (flat floor; "does a build succeed" probe; head-scan instead of strided).
    **THE FLIP SWEEP RAN AND PASSED — AND THE FLIP IS BLOCKED ANYWAY (2026-08-19).** Two-arm,
    **830** ontologies, **0 ok→DNF and 0 answer changes**: the 509 with <200 classes at the
    default (net +3.71 s; among the 78 *resolvable* rows — 424 have both arms under 0.10 s where a
    10 ms timer cannot resolve them, and the 26 apparent "2.00× wins" were one tick — **1 win, 0
    losses**) plus the 321 with 200–1000 classes under `--pair-timeout-ms 1` (**flat**; that
    budget widens the guard, so it is a separate scope the first frame could not see). Above the
    guard, no probe code runs.
    **But flipping the default costs 2× WITH THE PROBE STILL DISABLED**: `ore_ont_5107` 6.65 s →
    12.90 s at `=0`, where every formulation returns `false` and the executed path is identical.
    **Two independent formulations** (`is_none_or(|v| v != "0")` and `!is_some_and(|v| v == "0")`)
    are slow identically, and the probe's effect *inverts* (5.9× faster on the OFF-default build,
    slower on the ON-default ones). Ruled out by measurement: host drift (interleaved A/B, pinned
    control reproducing 6.65 s), build nondeterminism (byte-identical rebuilds), a stale artifact
    (forced reasoner recompile), file corruption (constants + guard verified), a second call site
    (one, grepped), and flag semantics. **THE "CODEGEN" DIAGNOSIS WAS WRONG AND
    IS RETRACTED.** Instrumentation showed the probe was *running and failing to escalate* — a
    functional bug in my decision rule, not a compiler artifact. Both builds scan the same 7
    classes and both hit a failing class at i=42; they differ only in whether that ONE class
    finishes inside the 1000 ms retry, so **escalation was a coin flip**. The counters had said so
    all along (slow builds report `pruned=710 misses=19`, *identical to not probing*; fast ones
    `pruned=729 misses=0`). **Fixed** by deciding at **2×** the budget applied — decisive because
    a *uniform* 800 ms budget already makes every class of the win case succeed, so the deciding
    class was never short of budget. Both predicate shapes now escalate reproducibly.
    **Method rule earned here: when two wall measurements of the SAME source disagree, print what
    the code DID rather than theorising about why it was slow.** Three causal stories in this one
    thread had to be withdrawn ("5 live starvation members", "no fix warranted", "codegen 2×").
    **Known wart:** on the fixed binary `=0` gives 12.90 s on `ore_ont_5107` vs 6.65 s shipped —
    that ontology's OFF path has measured 6.63/6.65/8.47/12.90 s across near-identical builds, so
    **`=0` is a FUNCTIONAL revert, not a performance one.**
    **FLIP SETTLED BY SHIP-VS-SHIP: a measured DEAD HEAT, so DEFAULT OFF.** Over the same 509
    frame, shipped-default vs proposed-default (both at default env): 502 identical, **0 ok→DNF,
    0 answer changes**, one win (`ore_ont_5107` **6.65 → 1.76 s, 3.78×**), 0 losses, **aggregate
    233.7 → 234.0 s = net +0.01 s** — the single win exactly cancelled by distributed probe cost.
    **A net-zero aggregate does not justify a default change plus new machinery**; as an opt-in it
    is a real 3.78× fix and is now deterministic. **The within-binary sweep (`unset` vs `=0`) said
    +6.82 s and was INFLATED BY CONSTRUCTION** — its baseline is the 12.90 s `=0` path on the new
    build, not the 6.65 s users have. *Measuring a flag's effect and measuring the ship delta are
    different questions; only the second decides a default.* **Residual: the `pt=1` scope arm was
    not re-run on the fixed binary** (a small per-pair budget widens the guard from n<200 to
    n<1000, +321 ontologies); it does not gate an OFF default but would gate any future flip.
    See `docs/2026-08-19-label-cache-probe.md`.
  * **The frame error worth carrying: a population selected on "SLOWEST" cannot see a defect
    whose precondition is "SMALL".** My "no fix warranted" verdict came from the 40-slowest
    frame; re-selecting on low class count × slow wall found the defect immediately. I made that
    mistake twice in one day.
  * **Superseded — "A FIX IS NOT WARRANTED" (kept for the floor half, which stands).** At the DEFAULT per-pair budget, granting
    every class the 30 s ceiling helps **0 of 40** slowest completers at ≥1.5× (best 1.24×) and
    costs **2.3% aggregate wall (1403 → 1436 s)**. So the `n × F` objection to a floor is now a
    NUMBER, not a projection, and the original "why not fixed" decision is vindicated — on
    different grounds than it argued (it reasoned from `pt=1`; the binding case is the default).
    **Honest residual:** the frame is the 40 SLOWEST, which skews to large `n`, and the budget is
    `clamp(n × 5 ms, 50, 30000)` — so a **small-`n` ontology with an expensive build would be
    floored at 50 ms and this frame cannot see it.** Untested; select on LOW CLASS COUNT × slow
    wall to probe it. See `docs/2026-08-19-label-cache-fix-not-warranted.md`. The
adaptive rule that consumes it couples the label-cache budget to `--pair-timeout-ms`, so a
*small* per-pair budget starves the cache — see
`docs/known-limitations/label-cache-budget-starved-by-small-pair-timeout.md`. A "dead code"
label would deter exactly the investigation that found that).

**`MAX_BODY_VARS = 8` — RESOLVED 2026-08-03, and the answer is "mis-designed, not mis-tuned".**
The silent MISS is **real**: a fixture that provably trips the `> MAX_BODY_VARS` branch (12 body
vars, verified via `RUSTDL_TRACE_BODY_VARS=1`) is decided by **both** Konclude and HermiT and missed
by rustdl. **But raising the cap is a hard stop.** 8 → 16 over a census of all 868 OFN ORE
ontologies (23 binders) recovers **nothing** and **destroys three completers** — `ore_ont_16461`
0.02 s → DNF, `7775` 3.14 s → DNF, `15491` 27.98 s → DNF, **9,773 sound pairs lost** — because in
each case the withheld clause is **disjunctive**, and a wide disjunctive body explodes the search.
No fixed value works either: binders need 9, 11, 12, 16, **25 and 133** vars, so 16 does not even
close its own set.
**If you attempt this, the only plausible shape is admitting wide HORN bodies while continuing to
refuse wide DISJUNCTIVE ones** — unbuilt, and note it would *not* recover the fixture above, whose
clause is itself disjunctive. `RUSTDL_WIDE_BODY_VARS` ships **default OFF** as a documented negative
result; the tracing is what makes this shape observable at all. See
`docs/2026-08-03-max-body-vars.md`.

**A corpus-scale MISSED net now exists** — `owl-reasoner-harness/scripts/missed-net.*`. Reach for it
whenever a change trades completeness for speed; every other gate here is **FP-shaped**
(`run-soundness-diff.sh`) or **outcome-shaped** (a sweep counts `dnf → ok`) and cannot see a lost
entailment. Baseline (v0.4.13): **MISSED = 5,198**, **FP = 0** over a 400-ontology seeded stratified
population against a Konclude ∪ HermiT oracle; a later arm costs **~10 minutes**.

Three usage notes, each of which cost something to learn:
- **Stratify on `label … pass_through > 0`, NOT on `tableau > 0`.** The latter holds on **2 of 546**
  completers, because the label heuristic prunes 96–100% — selecting on the intuitive proxy would
  build a 2-row stratum and a net **vacuous for its primary purpose**.
- **Where Konclude and HermiT disagree, EXCLUDE the ontology rather than picking a side.** A
  contested oracle is not an oracle. Konclude is documented to under-report (three instances now:
  `ore_ont_10407`, `9540`, `15682`).
- **Prove the net's sensitivity before trusting a zero.** A net reporting 0 for everything is
  indistinguishable from a broken net. The shipped one is validated by a pre-registered arm
  (1 ms per-pair ⇒ ΔMISSED +80, all 13 losers search-exercised, 0 in fast-path rows).

**The MISSED net does NOT replace a corpus sweep for a default flip.** Its frame is drawn from
*completers*, so it structurally cannot observe an `ok → dnf` in the DNF tail — precisely the failure
a 12-ontology benchmark missed in v0.4.8, which took four ontologies from ~5 s to DNF. **A flip needs
both.** v0.4.14's early-abandon passed ΔMISSED = 0 *and* a 1,920-ontology two-arm sweep (6
recoveries, 0 regressions, −5.5%).

**Method notes that earned their place** (each cost a retraction to learn):
- **`classify --pair-timeout-ms 1` IS THE ADDRESSABILITY PRE-CHECK for any per-pair-search lever, and
  it costs ~20 minutes.** It caps to ~zero exactly the phase such a lever improves, so an ontology
  that **still DNFs under it cannot be rescued by any amount of branch reduction** — its stall is
  elsewhere (`label_cache_build`, `saturate`, `prepare`). Run it on the candidate targets **before
  planning**, with the phase breakdown, so you know which phase holds the wall. This single check
  independently killed two consecutive absorption plans on 2026-08-04 (**6 of 9 targets DNF at 60 s
  with per-pair search eliminated**), each of which had been justified on a *static* count of
  absorbed-TBox shapes. **A shape census sizes a population; it does not predict a rescue.**
  Corollary, measured the same day: the guard-manufacturable predicate holds on 77.5% of
  peer-solvable ontologies **and 69.2% of peer-unsolvable ones**, and those with *nothing*
  manufacturable are the ones peers do **fastest** (median 1.68 s vs 6.14 s) — **a shape predicate can
  be anti-correlated with the tractability you are selecting for.** Cross-reference every target list
  against the peer triage first; one plan named an ontology **no peer can classify** as a target.
- **A single instance beats a population statistic on this tail.** Three population studies here
  were retracted or bounded; both of the largest wins came from reading one failing ontology.
- **A controlled deletion is only controlled if the intervention changed ONE thing.** A "300×"
  calibration was a mis-attribution: the deleted `EquivalentClasses` was two axioms, and either
  half alone runs in 0.02 s.
- **Deletion is NOT computationally stronger than absorption.** It turns cheap subsumptions into
  non-subsumptions that must be *refuted*, so a cut arm can DNF for work the intact arm never does.
- **Prove the instrument fires**, by a numeric criterion declared in advance. A probe silently
  failed to fire on 4 of 6 targets and would have read as 4 confirmations.
- **A full-corpus sweep is what catches default-flip regressions.** Twelve ontologies is not a
  population: a flag flipped on a 12-ontology benchmark took 4 others from ~5 s to DNF.
- Full record: `docs/benchmarks/2026-08-01-dnf257-characterization.md`,
  `docs/2026-08-0{1,2,3}-*.md`, `docs/reviews-2026-08-01/`.

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
inputs, not wall time.

> **CORRECTED 2026-08-01 — "RSS, not wall time" IS TOO NARROW, AND THE TAIL IS NOT
> INTRINSIC.** Peer triage of the real DNF population (257 ontologies genuinely
> unfinished at a 120 s single-thread cap, out of 1,920 ORE):
> **Konclude classifies 242 of them — 94% — at a median wall of 3.57 s**, 186 under
> 10 s, same cap and host. HermiT/KM can only move ontologies from B to A, so
> **Set A ≥ 242, Set B ≤ 15**: at most ~6% of the tail is plausibly intrinsic. The
> repeated framing elsewhere in this file and in the design record — "all levers
> measured out", "no cheap entry", "the frontier is a clash-driven search rewrite" —
> is defensible only in the narrow sense that *the mechanisms actually investigated*
> were measured out. No peer had been asked; a rustdl-vs-rustdl measurement cannot
> establish intrinsic hardness.
>
> Both halves of the "RSS not wall" claim need splitting. RSS is real
> (`ore_ont_11085`: OOM at 16.96 GB → 491 MB once the saturator's subsumer worklist
> dedups at enqueue instead of at pop — 35×, and its root cause was the worklist, NOT
> the D4 dense matrices, which were re-refuted by direct container dumps). But the
> most extreme peer ratio in the corpus is **wall-only**: `ore_ont_10019` is a
> **47-class** ontology, 182 concept rules, **0.01 GB** peak RSS, conversion and
> saturation each 0.01 s, stalling entirely after the label cache — Konclude does it
> in **0.06 s**. Profiling also puts that stall at **84.6% MAIN TABLEAU** vs 15.3%
> `hyper::solve`, so the standing attribution of the hard tail to wedge disjunctive
> branching does not hold on the named cases either.
>
> The contention caveat is CLOSED: a seeded 20-of-257 sample re-run strictly
> sequentially on an idle host, with the byte-identical binary, gave
> **completed = 0** — so 242 is measured, not inferred.
>
> **Two fixes from this work are DEFAULT ON as of 0.4.7** (`=0` reverts each):
> `RUSTDL_FAST_DIRECT_SUBSUMERS` (an O(k²)-per-class Hasse reduction sat in the
> **output** loop — `ore_ont_10125` finished classifying in ~15 s then spent ≥385 s
> emitting; DNF@900 s → 14.70 s) and `RUSTDL_FRAGMENT_BARE_DECL` (bare
> `SymmetricObjectProperty` / `InverseObjectProperties` **declarations** fell through
> to `_ => false` on the fragment gates; **44 of the 257 now classify**, admitted only
> when the role is provably *unread*). Two more ship default OFF:
> `RUSTDL_SAT_ENQUEUE_DEDUP` (`11085` OOM 16.96 GB → 491 MB, but 687 s — still DNF at
> a production budget) and `RUSTDL_LAZY_ABOX_SATURATION` (correct, but its perf
> premise measured as noise — do NOT quote the "~62% of prep" estimate).
>
> **TEST-IDIOM CHANGE:** a bare `InverseObjectProperties` declaration was a common
> in-tree device for forcing an ontology *out* of the EL fragment. `RUSTDL_FRAGMENT_BARE_DECL`
> dissolves it — three canaries used it that way and now take the fast path (their
> VERDICT assertions still passed; only telemetry failed, which is what identified it
> as dispatch rather than regression). Those canaries pin the flag off. **Do not write
> a new test that relies on a bare declaration to leave the fragment.**
>
> **v0.4.8 CLOSED all three correctness items, each DEFAULT ON (`=0` reverts):**
>
> - **`RUSTDL_CLASSIFY_INCONSISTENCY`** — `classify --json` reported `"consistent": true`
>   with `"unsatisfiable": []` on `family.ofn` while `rustdl consistent` on the SAME FILE
>   reported `inconsistent`; classify was simply wrong (HermiT and Konclude both call it
>   inconsistent in under a second). Now `consistent=false`, 58 unsat, and the two agree.
>   `abox_saturation_inconsistent` is extracted so both surfaces share ONE function and
>   cannot drift again. **The soundness subtlety is TESTED, not assumed:**
>   all-named-classes-unsat is NOT inconsistency — `{A⊑⊥, B⊑⊥}` still reports
>   `consistent=true` with 2 unsat, because the correct test is that **`⊤`** is unsat.
>   Cost measured *before* flipping (it adds an ABox pass to classify): **−1.5%** over 12
>   ABox-bearing ORE ontologies. Six ontologies *appeared* to change output — an
>   **OFF-vs-OFF control reproduced the identical diffs**, and banner-stripped bodies
>   matched 6/6, so those were nondeterministic timing banners, not answers.
>   > **THAT −1.5% SAMPLE WAS TOO SMALL — the flip SHIPPED A REGRESSION, fixed
>   > 2026-08-02 by `RUSTDL_CLASSIFY_INCONSISTENCY_MS` (default 3000, `0` =
>   > unbounded).** A 1,920-ontology v0.4.6-vs-`main` sweep found **four ontologies
>   > that went `ok` → `dnf`** — `ore_ont_10838` 4.86 s, `15846` 21.73 s, `16315`
>   > 4.42 s, `3087` 4.80 s, all **DNF at 60 s** — and bisected every one to this
>   > flag. Mechanism: the flag runs the *unbounded* `abox_saturation` fixpoint on
>   > the classify path, and these ABoxes carry 60k–110k assertions, so the
>   > pre-check dominates the classify it precedes. The 12-ontology cost benchmark
>   > simply did not contain this population.
>   >
>   > The fix budgets the ABox half **on the classify path only** (a timeout ⇒ no
>   > verdict, which is what "no clash" already meant; `is_consistent` / `realize` /
>   > `materialize_*` / `diagnose` stay unbounded). Post-fix: 5.43 / 7.78 / 4.34 /
>   > 4.98 s, three of the four at or below their v0.4.6 wall.
>   >
>   > **The obvious default is wrong: "a few hundred ms" is NOT ample.** `family.ofn`
>   > — the very ontology this flag exists for — needs **~2.0 s** of ABox saturation
>   > in a release build (classify 2.67 s with the pre-check vs 0.67 s without; 506
>   > individuals but a **267k-edge** role-chain closure). Anything under ~2.5 s
>   > silently re-breaks `family`. Measure `family` before touching this default.
>   > Corollary for tests: the unoptimized test profile is several times slower
>   > again, so `family_classify_agrees_with_is_consistent` pins the budget to `0`;
>   > the default-budget property is canaried on a small synthetic ABox clash
>   > instead (`classify_inconsistency_budget.rs`).
>   >
>   > **SUPERSEDED 2026-08-03 — THE FLAT 3000 ms IS NOW AN ADAPTIVE, TWO-LEVEL RULE
>   > (default ON; `RUSTDL_CLASSIFY_INCONSISTENCY_MS` still overrides outright,
>   > incl. `0`).** And the "~2.0 s" figure just above is **RETRACTED**: it is a
>   > confounded subtraction, because a clash **short-circuits** the rest of
>   > classify, so `with − without` measures "pre-check minus the classify it
>   > replaced". Measured *in isolation*, `family.ofn`'s pre-check is **2585 ms**
>   > and the classify-level detection flips between **2600 and 2700 ms** — so
>   > 3000 ms had only **~13% headroom**, and a host 15% slower silently lost the
>   > detection. Measure this fixpoint with
>   > `crates/owl-dl-reasoner/examples/abox_precheck_probe.rs`, never by subtracting
>   > two classify walls.
>   >
>   > **`ABox` SIZE DOES NOT PREDICT THE COST, AND SCALING THE BUDGET UP WITH IT IS
>   > BACKWARDS.** A 1137-ontology isolation scan of the whole ABox-bearing ORE
>   > population found `ore_ont_4510` (114 957 `ObjectPropertyAssertion`) at
>   > **136 ms** against `family.ofn` (1 337) at **2585 ms**, and `ore_ont_6233`
>   > (176 043 `ClassAssertion`) at 17 ms. Global rank correlation *prefers*
>   > `class_assertions` (+0.863 vs +0.462) and would have misled — it is dominated
>   > by the mass of sub-millisecond ontologies, while the decision is about the
>   > tail. The rule keys on
>   > `work_proxy = ObjectPropertyAssertion × max(role-chain + transitive roles, 1)`:
>   > **≤ 300 000 ⟹ 12 000 ms, else 3 000 ms.** The threshold sits in a measured,
>   > **empty 40× gap** (`family` 50 806; cheapest fixpoint-expensive ontology
>   > `ore_ont_16315` 2 047 210). The stingy branch is **bit-identical to the
>   > superseded flat default**, so the rule can only ever *raise* a budget and
>   > cannot reintroduce the four-ontology DNF (re-verified: 5.43 / 8.24 / 4.39 /
>   > 4.90 s, `classify --json` byte-identical).
>   >
>   > **A SECOND COST DRIVER EXISTS AND IS DELIBERATELY NOT MODELLED.** The first
>   > pass of this analysis (409 ontologies) concluded edge multiplication was
>   > *necessary* for expense; extending to 1137 refuted it. `ore_ont_5368` does
>   > **zero** type and **zero** edge additions and still costs 5936 ms — its cost is
>   > the fixpoint's **pre-indexing prelude**, which walks all 18.6 M of its lowered
>   > axioms (~0.3 µs/axiom, stable across cases). The reflex is to add an
>   > axiom-count gate; measurement says that makes things **worse**, because the
>   > prelude runs before the first deadline probe and so its cost is
>   > **budget-independent** (`1833` 4065→4023 ms and `5368` 6059→5871 ms going
>   > 3000→12 000 ms, while `timed_out` flips `true`→`false`). **Honest residual: no
>   > budget bounds the prelude** — pre-existing, identical at 3000 ms, a separate
>   > lever. Pinned by `prelude_dominated_predictors_stay_generous`.
>   >
>   > Population effect: **no wall change and no outcome change across all 1137**,
>   > because 1089 of the 1102 low-work members cost <500 ms (a cap is not an
>   > expenditure) and the 35 high-work members keep exactly today's budget.
>   > Canaries `crates/owl-dl-reasoner/tests/adaptive_inconsistency_budget.rs` (13;
>   > **9 sabotages run, 8 caught first pass, 1 survived** — an unbounded-generous
>   > mutation — closed by `generous_budget_is_bounded_above`, replay caught). The
>   > release-only `family` value test is `#[ignore]`d **with its reason in the
>   > attribute**: 37.9 s in the debug profile vs ~2.6 s in release. Spec
>   > `docs/2026-08-03-adaptive-inconsistency-budget.md`.
> - **`RUSTDL_EL_BOT_FILLER`** — `X ⊑ ∃r.⊥` was certified pure-EL-complete while the
>   lowering dropped it. Fixed in the **saturator**, not by tightening the gate (which
>   would only relocate the MISS to the hybrid path), and made **total** via a recursive
>   predicate — a `Bot` match arm would still have missed `∃r.∃s.⊥`.
> - **`RUSTDL_DKEY_POST_NNF`** — `dkey_components` ran **pre-NNF**, so a `∀p.DKey` arising
>   only after NNF (`¬∃q.¬DKey`, legal OWL 2 DL) was invisible to the role gates. A
>   completeness **regression** from the 07-20/07-30 DKey gates that *either gate alone*
>   loses.
>
> **FIXED behind `RUSTDL_DKEY_EMIT_ORDER` (2026-08-01; DEFAULT ON since 2026-08-03, `=0` reverts)** —
> The 1,920-ontology × 4-arm volume scan that gated the flip found **exactly one ontology whose
> numbers move** (`ore_ont_9303`: `concept_rules` 8886→8887, told-disjoint pairs 6669→6670 — a
> corpus-wide total of +1 axiom), 0 over the >2×/>100k threshold, 0 new conversion timeouts, and
> `ore_ont_5368` unmoved at 18,620,251. The mover was FP-adjudicated (byte-identical ON vs OFF;
> verdict confirmed by Konclude AND HermiT). `RUSTDL_DKEY_ONEOF_SEED` was flipped ON in the same
> pass and moves **nothing** on ORE — the numeric `DataOneOf` pattern does not occur there, so its
> evidence is its canaries plus a Konclude ∪ HermiT adjudication, not the corpus. Both now use the
> house default-ON idiom (`is_none_or(|v| v != "0")`), so **empty enables**; defaults are pinned on
> both halves by `crates/owl-dl-reasoner/tests/dkey_flag_defaults.rs`, which also guards the
> `tbox-stats` told counters the scan was decided on. See `docs/2026-08-03-dkey-volume-scan.md`.
> the THIRD latent completeness defect, and the explanation of why
> `RUSTDL_DKEY_MERGING_GATE=0` found *fewer* entailments than the default.
> `seed_disjoint_bucket::try_emit` ran `emitted.insert(pair)` **BEFORE** the droppable
> test, so a pair spanning two role components was permanently consumed by whichever the
> `BTreeMap` reached first; if that component's collapse/broadcast split declined it, the
> component where it was consumable was never asked and the entailed
> `DisjointClasses(DKey, DKey)` was never emitted. The merging gate masked it by skipping
> the greedy component. **The one-line move was the right fix but not sufficient on its
> own**: the `RUSTDL_DKEY_SPLIT_STATS` counters were per-`(pair, component)`, which
> double-counts a multi-component pair once declining stops spending it, so under the
> lever they are settled after the walk from `|emitted ∪ deferred|` /
> `|deferred \ emitted|`. Dedup is preserved — the lever only *looks* at `emitted` up
> front and claims the pair at the emit site.
>
> **This one is live at DEFAULT settings and is NON-MONOTONIC**, which is what makes it
> worth the flag: `∀p.[0,5] ⊓ ∃p.{9}` is `⊥`, but adding an *unrelated* data property `q`
> that merely mentions the same two keys in value position makes it satisfiable again
> (`would_drop` 1, no axiom emitted). Lever ON recovers it and `would_drop` goes 1 → 0.
>
> **DIRECTION OF RISK IS INVERTED here** — the change emits MORE disjointness, so the
> failure mode is a FALSE POSITIVE, not a miss. Three properties bound it: every pair
> still passes the per-pair `disjoint()` value-space test; `seed_disjoint_bucket` is
> called once per DATATYPE bucket, so no cross-datatype pair is constructible; and the
> result is a SUBSET of what `RUSTDL_DKEY_COLLAPSE_SPLIT=0` already emits.
>
> Gates: FP=0 net **flag ON**, 11 VERIFIED all closures exact; flag-OFF byte-identical to
> pre-change on 10/10 curated fixtures (after stripping `# wedge-cost-histogram`, which an
> OFF-vs-OFF control on one binary shows is nondeterministic); **both DKey-spec
> discriminators unmoved — `ore_ont_9347` 113 and `ore_ont_5368` 18,620,251 at both flag
> settings**. Canaries `crates/owl-dl-reasoner/tests/dkey_emit_order.rs`. **Sabotage: 3 of
> 4 caught.** Reverting the fix fails 3 canaries; deleting the dedup LOOK fails the
> emitted-exactly-once control (which needs a THREE-component fixture — with only one
> keeping component the guard is unobservable); neutering `disjoint()` fails 4. **The one
> that SURVIVED:** making the lever ignore the collapse/broadcast split entirely left all
> 6 green — these canaries pin the split's FP-safety, not its cost bound. Benign (the
> split is subtractive and documented as a cost bound, so ignoring it can only add dead
> weight) but it means a future regression in the split's *volume* will not be caught here.
>
> **Evidence caveat to carry:** the curated corpus is inert for the DKey area by
> `datatype_value_membership.rs`'s own admission, so an all-green FP=0 net shows
> **non-regression only** for `RUSTDL_DKEY_POST_NNF` — its canaries and the
> Konclude ∪ HermiT adjudication are the actual evidence.
>
> See `docs/benchmarks/2026-08-01-dnf257-characterization.md` (results, threats to
> validity, and eight confirmed defects with predicted effects) and
> `docs/reviews-2026-08-01/`.

> **`classify` vs `consistent` contradiction on `family.ofn` — FIXED behind
> `RUSTDL_CLASSIFY_INCONSISTENCY` (2026-08-01, default OFF, `=1` enables).**
> `rustdl classify --json ontologies/real/family.ofn` reported
> `"consistent": true, "incomplete": false, "unsatisfiable": []` while
> `rustdl consistent` on the same file reported `inconsistent` — two wrong answers
> from one binary (`family.ofn` *is* inconsistent; HermiT, Konclude and rustdl's own
> `abox_saturation` pre-check all agree in under a second).
>
> Root cause, as diagnosed: the `top_is_unsat` inconsistency test lived **only** in
> `classify_pure_el`, and `abox_saturation` — which `is_consistent` has run by default
> since 2026-06-20, and which `realize_internal` gained in v0.3.36 — was **absent from
> `classify.rs` entirely**. The hybrid path therefore had no inconsistency signal at all.
>
> Fix (reuse, not parallel-invent): `classify_inconsistency_precheck` in `lib.rs`
> combines the two signals already shipped elsewhere — the saturator's
> `globally_inconsistent() || top_is_unsat()` (verbatim `classify_pure_el`'s test) and
> `abox_saturation_inconsistent` (newly factored out so `is_consistent` and `classify`
> call **literally the same function** and cannot drift). Both classify drivers call it
> once, before the fast-path branch, and a positive verdict returns the existing Phase-A1
> `classify_inconsistent` — every class unsatisfiable, mirroring Konclude and rustdl's own
> ABox-pre-check handling. `family.ofn`: `consistent: false`, 58/58 classes unsatisfiable.
>
> **SOUNDNESS SUBTLETY, load-bearing:** *all-named-classes-unsat is NOT an inconsistency
> signal.* `{A ⊑ ⊥, B ⊑ ⊥}` empties every named class yet has a non-empty model. The test
> is that **`⊤`** is unsatisfiable; nothing in the pre-check inspects the unsatisfiable-class
> list. Pinned by `all_classes_unsat_is_still_consistent`.
>
> Gates: flag-OFF byte-identical to pre-change `main` on 9/9 curated fixtures
> (bibtex/pizza/ro/ro-stripped/sulo/sulo-stripped/sio/go-basic/family) plus `wine`;
> flag-ON byte-identical to flag-OFF on all of them **except `family.ofn`**, the one
> ontology that is genuinely inconsistent; FP=0 net with the flag ON, 11 VERIFIED,
> all closures exact (galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653,
> pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16). Canaries in
> `crates/owl-dl-reasoner/tests/classify_inconsistency.rs`; **both were sabotaged and
> both failed** (dropping the ABox-saturation signal fails the family canary; inferring
> inconsistency from all-classes-unsat fails the negative control).
>
> **KNOWN RESIDUAL (recorded, not hidden):** classify's inconsistency detection stays a
> sound UNDER-approximation. `docs/family-mech4-ddmin-core.ofn` is inconsistent via the
> **wedge consistency route** (`consistency: wedge Unsat`), which neither pre-check
> reaches, and a bounded global `decide(Top)` probe on the classify path is a measured
> dead-end (hangs on consistent `alehif`/`pizza`). So classify can still MISS a
> tableau-only inconsistency and report `consistent: true`. What the change guarantees is
> that the two surfaces cannot disagree at the *pre-check* tier, because that tier is now
> shared code. `#[ignore]`d test `ddmin_core_residual_divergence` documents it.
> Unrelated pre-existing note: `rustdl consistent ontologies/real/sio.ofn` DNFs at 600 s
> on both the pre-change and post-change binaries — a stall, not a verdict disagreement.

Performance claims in docs are backed by the corpus harness
— re-measure with `scripts/bench-rustdl-modes.sh` (on a **freshly built** binary,
per the toolchain gotcha above) rather than trusting stale numbers.
