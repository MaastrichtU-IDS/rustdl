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

- **`crates/owl-dl-reasoner`** — public API + orchestrator (`lib.rs`,
  `classify.rs`, `realize.rs`). Every entry point that issues a tableau query
  first runs saturation and short-circuits on a hit; if the whole ontology is in
  the EL fragment it takes a saturation-only fast path (`stats.pure_el_mode`).
  `PreparedOntology::from_internal` snapshots the post-NNF/absorb/ABox-seed state
  **once** so the O(n²) pairwise classify loop reuses it across pairs; the loop
  runs in parallel via rayon. `is_subclass_of` reduces to satisfiability of
  `sub ⊓ ¬sup`.

- **`crates/owl-dl-datatypes`** — concrete-domain reasoners. Scaffolded, **not
  yet wired into reasoning.**

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
  **can MISS** subsumptions the full tableau would find (e.g. GALEN ~109, SIO 2;
  see `docs/handoff-2026-05-30.md`). So the practical default classifier is a
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

When changing the saturation/wedge engines or caches, the failure mode that
matters most is an unsound *positive*. See `docs/handoff-2026-05-30.md` for
current engine state, characterized MISSED, open levers, and dead-ends;
`docs/model-caching-plan.md` / `docs/moms-plan.md` explain why model caching is
a deliberately un-integrated Phase-1 stub.

## Where to read more

`docs/` is the design record. Start with `architecture-roadmap.md` (levers to
close the SROIQ gap to HermiT + dead-ends already measured),
`owl-dl-reasoner-rust-strategy-v2.md` (full strategy), and the
`hypertableau-*-scoping.md` series for the in-progress hypertableau work.
`docs/perf-2026-05-24-new-server.md` §8 has the head-to-head vs
HermiT/Pellet/Konclude. Performance claims in docs are backed by the corpus
harness — re-measure with `scripts/bench-rustdl-modes.sh` rather than trusting
stale numbers.
