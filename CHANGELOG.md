# Changelog

All notable changes to rustdl are documented here. Format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); rustdl follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.2]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.2
[0.2.1]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.1
[0.2.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.0
[0.1.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.1.0
