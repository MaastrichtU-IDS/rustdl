# CLAUDE.md

Guidance for Claude Code working in this repository. This file is loaded on every turn, so it
holds only rules and pointers. The dated record of fixes, measurements and retractions that used
to live here is in `docs/claude-md-history.md`, verbatim. Read it when you work on a specific
subsystem, flag or `ore_ont_*` target.

## What this is

`rustdl` is a **sound** OWL 2 DL (SROIQ) reasoner in Rust, Konclude-style **hybrid**: a
consequence-based EL **saturation** engine handles the cheap EL fragment, a **tableau**
(plus a hypertableau accelerator, the "wedge") handles the rest of SROIQ, and an
**orchestrator** picks per query. It is sound and complete on the curated corpus vs a
Konclude∩HermiT oracle; the provable guarantee covers only the EL/Horn fragment, and
completeness on ORE/BioPortal is empirical, not proven. Parsing and the OWL object model come from
`horned-owl` (LGPL-3.0; our code is Apache-2.0/MIT).

## Commands

```sh
cargo build --workspace --release
cargo test --workspace
cargo test --workspace --doc                # LOCAL ONLY: CI has no doctest job
cargo test -p owl-dl-tableau <name>
cargo fmt --all -- --check                  # max_width = 100
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- **Toolchain.** `rust-toolchain.toml` pins **1.95.0**. Bare `cargo` differs per host (g1:
  `/usr/bin/cargo` 1.75.0, which fails with a misleading ``feature `edition2024` is required``;
  n3: none; Mac: 1.96.0), and `stable` is a different compiler on each host. For anything
  compared across hosts, use the pinned path:
  `PATH="$HOME/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$PATH" cargo …`.
  Tools that shell out to `cargo` themselves (e.g. `maturin` / `pip install ./crates/owl-dl-py`)
  hit this from inside a PEP 517 build.
- **Confirm `target/release/rustdl` is freshly built before any benchmark.** A failed build
  silently reuses a stale binary, and stale binaries have produced false DNFs and false
  "the fix doesn't work" readings more than once. `wine` at the default runs in **~12–13 CPU-s**
  on a current binary; a very different figure means a different binary or host.
- CI (`.github/workflows/ci.yml`): fmt, clippy, build+test on linux/macos/windows, cargo-deny,
  pytest. `RUSTFLAGS=-D warnings`, clippy `pedantic` workspace-wide. **Green CI says nothing
  about soundness**: the closure-diff job is a `workflow_dispatch`-only stub with no fixtures.
  Green pytest does not prove the Python surface is right either (stubs aren't runtime-checked).

```sh
./target/release/rustdl classify file.ofn               # see README for subcommands
./target/release/rustdl classify --pair-timeout-ms 200 file.ofn
./target/release/rustdl explain file.ofn <sub> <sup>    # which engine answered
./target/release/rustdl verify-el file.ofn [--json]     # diagnostic EL-closure cross-check
./target/release/owl-dl-bench corpus ontologies/real --repeats 5
./scripts/fetch-real-ontologies.sh                      # inputs only, NOT oracles
./scripts/konclude-oracle.sh                            # oracles (sio/sulo/ro/wine loop)
```

Hard SROIQ pairs can take minutes in the full tableau; never probe them in a loop without
timeouts. Diagnostics: `RUSTDL_TRACE=1`; `RUSTDL_COUNTERS=1` with `--features counters`.
Host paths (ORE pool, Konclude, robot.jar) differ per host; see the history file's host table.

## Workspace architecture

Data flow: `horned-owl` parse → `owl-dl-core` (IR + preprocessing) → `owl-dl-reasoner`
orchestrator → `owl-dl-saturation` and/or `owl-dl-tableau`.

- **`owl-dl-core`**: shared IR; concepts interned in `ConceptPool`, so structural equality is an
  integer compare. **This interning invariant is load-bearing for the tableau hot loop.** Passes:
  `convert.rs` (horned-owl → IR; unsupported content is recorded in `dropped`, never a hard
  error), `normalize.rs` (NNF), `absorb.rs` (GCIs → lazily fired rules), `told.rs`,
  `data_axioms.rs`, `disjunction_existential.rs`, `clause.rs` (wedge clausification).
- **`owl-dl-saturation`**: ELK-style EL engine. Sound; complete only for its fragment.
- **`owl-dl-tableau`**: SROIQ tableau. **`TableauContext` is the only sanctioned mutation
  interface** (everything goes through the trail). `search.rs` does backtracking with
  dependency-directed backjumping. `hyper.rs` is the hypertableau wedge, **wired in by default**
  (its docstring saying "standalone" is stale; trust the `*_enabled()` defaults in
  `reasoner/src/lib.rs`).
- **`owl-dl-reasoner`**: public API and orchestrator (`lib.rs`, `classify.rs`, `realize.rs`,
  `abox_check.rs`, `abox_saturation.rs`, `justify`). Every tableau entry point runs saturation
  first. `PreparedOntology` snapshots prep once for the pairwise classify loop (rayon).
- **`owl-dl-datatypes`**: concrete domains (DKey buckets per datatype, ranges, enumerations).
- **`owl-dl-verify`**: diagnostic-only model check of the EL closure (`verify-el`). A `Violated`
  is a lead to adjudicate, not a proof.
- **`owl-dl-cli`** (`rustdl`), **`owl-dl-bench`**, **`owl-dl-py`**, `xtask/`, Protégé plugin.

## Soundness contract and the rules that prevent wrong answers

The failure that matters most is an unsound **positive** (a false subsumption, unsat class or
inconsistency). Sound under-approximations (MISSES) are pervasive and acceptable, but must be
observable.

- **FP=0 is a claim about `classify` closures.** The per-pair surface
  (`subclass`/`explain`/`is_subclass_of`) is gated separately by
  `crates/owl-dl-reasoner/tests/per_pair_fp_gate.rs` (`#[ignore]`d; runs under
  `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1`). A real FP (#76) lived there for months while the
  classify net was green.
- **Run `./scripts/run-soundness-diff.sh` locally before merging any change to the fragment
  gates (`is_pure_el` / `saturator_complete_fragment`), unsat derivation in `owl-dl-saturation`,
  or conversion/normalization in `owl-dl-core`.** ~4 min. Grep `^\[fp0\]` for the VERIFIED
  manifest. `alehif-test` and `notgalen` fixtures are unobtainable; those 2 failures are expected.
- **The curated corpus is inert for most narrow fixes**, and especially for DKey/datatype work
  (`datatype_value_membership.rs` says so). A green net there shows non-regression only. The
  evidence is negatives-first canaries plus a Konclude ∪ HermiT adjudication (for
  integer-vs-decimal, use HermiT: Konclude is wrong there). A float/double FP shipped for months
  this way.
- **D10 shape**: a gate certifies a fragment complete while the engine drops an axiom, giving a
  wrong answer with `incomplete: false` and `dropped: {}`. When changing a gate or a lowering,
  check that the engine actually processes everything the gate admits.
- **`incomplete` in `classify --json` means ONLY "a deadline fired".** Every other cause has its
  own field (`trusted_sat_refutations`, `consistency_undetermined`, `completeness_guaranteed`,
  `wedge_hierarchy_blind`, realize's `witness_prune_active`). Read the field docs in
  `crates/owl-dl-cli/src/json_out.rs` before touching it. When you add a sound
  under-approximation, add a signal so a caller can tell.
- **`trust_sat`** (`RUSTDL_HYPERTABLEAU_TRUST_SAT`, default ON) means the default classifier is
  sound but not guaranteed complete. `RUSTDL_SNAPSHOT_CAPTURE` stays OFF: it is FP-unsound on
  non-Horn (one model's labels do not imply subsumption). A subtractive prune from one `Sat`
  completion is only sound if the witness really is a model (see `RUSTDL_PSEUDO_MODEL` in the
  history file).
- **Defaults favour completeness**: never shorten a default budget if it loses answers or signal;
  optimize the code instead.
- **Flag defaults are pinned behaviourally** in `crates/owl-dl-reasoner/tests/flag_defaults.rs`
  (public accessors) and `internal_flag_defaults` in `crates/owl-dl-reasoner/src/lib.rs`
  (`pub(crate)` ones). A default change edits the table in the same commit. Do not audit defaults
  by reading doc comments: they have been wrong repeatedly.
- **Tests can pin the bug.** When a fix breaks an existing test, first ask whether that test
  asserted the defect (several have, some unlabelled). Flip it, don't delete it, and when a test
  fails, suspect the code before the test. An `#[ignore]`d sentinel that starts passing is
  silent: run `cargo test -- --ignored` and treat passes as findings, then attribute which
  engine made it pass.
- **Sabotage your own guards.** Revert or loosen the fix and confirm a named test fails. Canaries
  that all share one shape (two-member union, singleton `self` side, `Min` only) leave the
  general case unpinned. Run sabotages against neighbouring test files too.
- Don't write a test that relies on a bare `InverseObjectProperties`/`Symmetric` declaration to
  leave the EL fragment (`RUSTDL_FRAGMENT_BARE_DECL` admits it).

## Measurement method (one line each; reasons and examples in the history file)

- **Use CPU time (`/usr/bin/time -f "%U %S"`), not wall.** This host is shared; wall has varied
  2.4× for identical code. See `docs/benchmarks/2026-09-18-wall-clock-is-not-measurable-on-this-host.md`.
- **Interleave arms (A,B,A,B…) or alternate arm order.** A fixed order adds a ~3.4% page-cache
  phantom. Take ≥5 reps for timing; report median and range.
- **Pin each binary to a uniquely named path right after building it, and verify the pin
  behaviourally** on a discriminating input before trusting a scan.
- **Run ≥3 reps (sometimes 9) on budget-truncated ontologies** (`incomplete: true`, e.g.
  `ore_ont_1508`, `12698`, `9540`). Budget cuts make answers nondeterministic; compare at a
  non-truncating budget (`--pair-timeout-ms 1000`) when possible.
- **Recheck every sweep timeout, `ok → dnf` and DIFFER serially, by CPU time, on a quiet host.**
  Parallel contention has manufactured every such regression seen so far. A one-sided timeout at
  a cap is a candidate, not a loss: raise the cap.
- **Compare content as rows + unsat + equivalence groups (+ `consistent`)**, never rows alone (an
  unsat proof elides rows); report `incomplete` separately, since it's a timing flag.
- **Measure the ship delta, not the flag delta.** Comparing a flag within one binary is not the
  same as comparing the shipped binary to the proposed one.
- **Konclude under-reports.** Prove an FP or MISS with a discriminating control (a case where the
  oracle *does* report the relation). Where Konclude and HermiT disagree, exclude the ontology
  or go to the semantics; majority isn't an oracle. A tool that ports a disputant can't break a tie.
- **Run a control through the same command that exhibits the defect** (`sat` vs `classify` vs
  `subclass` take different paths).
- **Prove an instrument fires** (numeric criterion, declared in advance, smoke-tested on a known
  positive) before reading a zero. Calibrate against a known value under the baseline's own
  conditions. `grep -c` exits 1 on zero; a probe that cannot produce a positive proves nothing.
- **Grep counts a superset of the frame, not a rescue or a reach.** Size reach by the real gate;
  shape census ≠ prediction. `classify --pair-timeout-ms 1` is the addressability pre-check for
  any per-pair-search lever.
- **Under a deadline a profile shows where time goes, not what removal saves.** Check
  `wall/budget` across two budgets before optimising a hot frame.
- **Re-baseline on the host you gate on**: the corpus gate's `lost_ontologies` is host-dependent.
- **A default flip needs a full 1,920-ontology two-arm sweep plus the MISSED net**
  (`owl-reasoner-harness/scripts/missed-net.*`, stratified on `label … pass_through > 0`).
  Samples are smoke tests.
- **Re-verify any "open target" or doc claim against current `main` before planning.** The record
  has drifted optimistic many times.
- One timing sweep at a time on a shared host; no workspace test build during a sweep; kill only
  PIDs you started (no `pkill -f`); distinct tags in `~/owl-reasoner-harness/baselines`. Use
  one `CARGO_TARGET_DIR` per worktree.
- When two runs of "the same" source disagree, print what the code did before theorising.
- Use the `corpus-measurement` skill for population figures and before/after comparisons.

## Repo hygiene (load-bearing)

- `origin` keeps few branches on purpose; all local branches are backed up under
  `refs/archive/heads/*` (a snapshot, not a mirror):
  `git push origin 'refs/heads/*:refs/archive/heads/*'`,
  `git ls-remote origin 'refs/archive/heads/*'`,
  `git fetch origin 'refs/archive/heads/X:refs/heads/X'`. Check it before deleting a branch.
- **Never `git fetch --prune` / `git remote prune origin`**: stale remote refs hold commits that
  are on no local branch. Some are anchored as `archive/*` tags; re-check before any prune.
- Some branches are deliberately parked, and some `.claude/worktrees/` are dirty
  (`agent-ab06fe71b6797f234`); `git worktree remove` destroys uncommitted work. Inventory:
  `docs/handoff-2026-07-29.md` §4a.
- No force-push without asking Michel. Issue drafts go to rustdl-manager first. See the memory
  index for commit identity and cleanup rules.

## Where to read more

- `docs/claude-md-history.md`: the full former CLAUDE.md, verbatim. It holds the per-crate
  history (phases, flags, canaries), the soundness-contract details, every "State of play"
  section (v0.4.13 through 2026-10-15) and the measured-out levers. Search it by flag or ontology
  name before proposing a lever, since many are recorded there as measured out.
- `docs/architecture-roadmap.md` (levers and dead-ends), `docs/owl-dl-reasoner-rust-strategy-v2.md`,
  `docs/hypertableau-*-scoping.md`.
- `docs/known-limitations/` for open defects and residuals; `docs/superpowers/specs/` and
  `docs/superpowers/plans/` for designs; `docs/benchmarks/` for sweeps; `docs/releases/`.
- `docs/fragment-completeness.md`, `docs/handoff-2026-06-03-snapshot-cache-project-complete.md`,
  `docs/perf-2026-06-08-konclude-vs-rustdl.md`, `docs/2026-08-18-fp-critical-audit.md`.
- Re-measure performance with `scripts/bench-rustdl-modes.sh` on a fresh binary rather than
  trusting a number in the docs.
