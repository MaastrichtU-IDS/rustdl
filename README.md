# rustdl

A sound, performant OWL 2 DL (SROIQ) reasoner in Rust. Hybrid
saturation-+-tableau architecture in the style of
[Konclude](http://www.derivo.de/produkte/konclude/), with a hypertableau
wedge accelerator after the [HermiT](http://www.hermit-reasoner.com/)
playbook.

**Status — 0.1 release (2026-06-04):** sound classifier (FP=0 across 9
corpus closure-diff fixtures), sound consistency check (ABox + datatype
patterns), competitive perf:

- **Beats HermiT on every measured workload** — the original
  `outperform-hermit-plan.md` target has been comprehensively met.
- **Wins outright vs Konclude on Horn-fragment workloads** — GALEN
  0.49 s vs Konclude 2.03 s (0.24×); notgalen 0.78 s vs 2.20 s (0.35×);
  alehif 0.16 s vs 2.11 s (0.08×).
- **3.1× Konclude on ORE-10908** (5.35 s vs 1.73 s) — under the ≤5×
  Konclude-class target.
- **Tableau-family gaps remain** vs Konclude's pseudo-model classifier
  on ore-15672 (16.6×) and sio-stripped (13.6×). See [the perf
  doc](docs/perf-2026-06-04-konclude-vs-rustdl.md) for the full
  9-ontology head-to-head.

Public API (`owl-dl-reasoner`): `is_class_satisfiable`, `is_consistent`,
`is_subclass_of`, `is_instance_of`, `instances_of`, `classify`, `realize`.
CLI (`rustdl`) exposes each as a subcommand. `owl-dl-bench` is a separate
binary with `classify` / `synthetic-el` / `corpus` for harness work.

## Architecture

Konclude-style hybrid:

- A consequence-based **saturation** engine handles the EL-ish subset cheaply
  (`crates/owl-dl-saturation`).
- A **tableau** engine handles the rest of SROIQ (`crates/owl-dl-tableau`).
- A hybrid **orchestrator** decides per query whether saturation suffices
  (`crates/owl-dl-reasoner`).
- A shared **IR** with structural sharing (`crates/owl-dl-core`).
- **Datatypes** in a separate crate so concrete domains can grow independently
  (`crates/owl-dl-datatypes`).

Parsing and the OWL model come from
[`horned-owl`](https://github.com/phillord/horned-owl).

## Coverage

**Fully supported (sound + complete on the fragment, validated against
Konclude closure-diff on 9 corpus fixtures):**

- SROIQ object-property reasoning: role hierarchies; transitive,
  symmetric, asymmetric, irreflexive, functional, inverse-functional
  characteristics; inverse roles; named-role chains up to length 2.
- Class expressions: intersection, union, complement, nominals (`{a}`),
  existential / universal restrictions, qualified-cardinality (`≥n R.C`,
  `≤n R.C`).
- DisjointClasses, EquivalentClasses, DisjointUnion.
- ABox: ClassAssertion, ObjectPropertyAssertion,
  NegativeObjectPropertyAssertion, SameIndividual, DifferentIndividuals.
- ABox consistency check (Phase A1, 7 sound clash patterns — direct-Bot
  assertion, disjoint types per individual, NegOPA-vs-OPA with role
  hierarchy, SameAs∩DifferentFrom, Functional + two distinct witnesses,
  Asymmetric / Irreflexive violations, domain/range disjointness).

**Sound under-approximation (silently dropped at parse time, no error):**

- Data properties and data axioms NOT matched by D4/D5 preprocessing.
  Soundness invariant: every reported subsumption holds; positives that
  depend on dropped data axioms may be missed.

**Recognized data-axiom patterns (preprocessed into TBox by D4/D5):**

- `FunctionalDataProperty(dp) + ≥n dp` (n≥2) → derives `C ⊑ ⊥`.
- `≥n dp` together with `≤m dp` where n > m → derives `C ⊑ ⊥`.
- `DataPropertyDomain(dp, D) + C ⊑ ∃dp.…` → derives `C ⊑ D`.
- `SubDataPropertyOf` transitivity closure.
- Intersection-equivalence propagation across data-cardinality bounds.
- Integer-range facet intersection (xsd:integer with
  `minInclusive` / `maxInclusive` / `minExclusive` / `maxExclusive`)
  — derives `C ⊑ ⊥` on empty intersection.

**Unsupported (errors — file an issue if these block you):**

- `HasKey`.
- SWRL rules (silently skipped — see
  [`crates/owl-dl-core/src/convert.rs`](crates/owl-dl-core/src/convert.rs)
  for the rationale).
- Role chains of length > 2.

## Soundness

rustdl's classifier is **sound** by construction and by regression: every
reported subsumption is a genuine entailment. FP=0 vs Konclude is verified
on 9 corpus closure-diff fixtures (`alehif`, `ore-10908-sroiq`,
`ore-15672-shoin`, `shoiq-knowledge`, `sio`, `sio-stripped`, `ro`,
`sulo`, plus `galen` / `notgalen` for the Horn fragment). The tests live
in [`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`](crates/owl-dl-reasoner/tests/konclude_closure_diff.rs)
and gate every release.

Completeness is partial — see [`docs/fragment-completeness.md`](docs/fragment-completeness.md)
for the precise envelope. The default-mode classifier is empirically
near-complete across the measured corpus but not provably complete in
general. Under-approximation modes (`--saturation-only`,
`--pair-timeout-ms`) are sound-but-incomplete by design.

## Workspace layout

Seven publishable crates: `owl-dl-core` (IR + normalization),
`owl-dl-saturation` (EL closure), `owl-dl-tableau` (SROIQ tableau +
hypertableau wedge), `owl-dl-datatypes` (concrete-domain preprocessing),
`owl-dl-reasoner` (orchestrator + public API), `owl-dl-cli` (`rustdl`
binary), `owl-dl-bench` (ORE corpus benchmark harness). Plus `xtask`
for build automation (corpus fetch, license inventory).

## Install

As a library (other crates):
```sh
cargo add owl-dl-reasoner
```

As the CLI binary:
```sh
cargo install --git https://github.com/MaastrichtU-IDS/rustdl owl-dl-cli
```

Or build from source — see [Quick start](#quick-start).

## Quick start

```sh
# Build everything
cargo build --workspace --release

# Run lint and format checks
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run tests
cargo test --workspace
```

Requires Rust 1.88+.

## Using `rustdl`

```sh
# Full classification (sound + complete; the default)
./target/release/rustdl classify path/to/ontology.ofn

# Same with a per-pair tableau deadline — sound under-approximation
# that's robust against pathological SROIQ inputs. Pairs that exceed
# the budget default to "not subsumed".
./target/release/rustdl classify --pair-timeout-ms 200 path/to/ontology.ofn

# Saturation-only mode — skips every tableau probe. Closure-only
# under-approximation: every reported subsumption holds, but
# subsumptions that need tableau reasoning are missed. Dramatically
# faster on mostly-EL inputs:
#
#                   default       --saturation-only
#   sulo-stripped   0.04 s        < 0.01 s
#   pizza           3.48 s        0.03 s
#   sio-stripped    28.1 s        0.22 s
#
# See docs/perf-2026-06-04-konclude-vs-rustdl.md for the current
# 9-ontology head-to-head vs Konclude.
./target/release/rustdl classify --saturation-only path/to/ontology.ofn

# Other queries
./target/release/rustdl consistent path/to/ontology.ofn
./target/release/rustdl sat        path/to/ontology.ofn <class-iri>
./target/release/rustdl subclass   path/to/ontology.ofn <sub-iri> <sup-iri>
./target/release/rustdl instance   path/to/ontology.ofn <class-iri> <individual-iri>
./target/release/rustdl instances  path/to/ontology.ofn <class-iri>
./target/release/rustdl realize    path/to/ontology.ofn

# --saturation-only is accepted by every "does X hold?" query —
# skips the tableau probe and answers from the EL closure only.
# Sound under-approximation; missed positives are possible but no
# false positives. Useful when the default would DNF or take too
# long on a SROIQ-heavy ABox.
./target/release/rustdl subclass  --saturation-only path/to/ontology.ofn <sub> <sup>
./target/release/rustdl instance  --saturation-only path/to/ontology.ofn <cls> <ind>
./target/release/rustdl instances --saturation-only path/to/ontology.ofn <cls>
./target/release/rustdl realize   --saturation-only path/to/ontology.ofn
```

Diagnostic env knobs:

- `RUSTDL_COUNTERS=1` — dump per-rule call counts to stderr on
  `TableauContext::drop`. Requires `--features counters` at build time.
- `RUSTDL_TRACE=1` — one stderr line per `search`/`branch` decision
  for understanding what the tableau is doing on a single probe.
  Off-path is one atomic load; safe to ship enabled.

## Benchmarking

[`scripts/bench-rustdl-modes.sh`](scripts/bench-rustdl-modes.sh)
runs the real-ontology corpus across all three `classify` modes
(default, `--pair-timeout-ms`, `--saturation-only`) and produces
a comparison table + TSV under `bench-results/`. Env vars:

```sh
REPS=5 PAIR_TIMEOUT_MS=200 WALL_CAP_S=600 \
  scripts/bench-rustdl-modes.sh
```

`docs/perf-2026-05-24-new-server.md` §8 has the head-to-head
against HermiT / Pellet / Konclude using the ROBOT-docker
harness.

## Architecture roadmap

[`docs/architecture-roadmap.md`](docs/architecture-roadmap.md)
consolidates the multi-week levers needed to close the default-
mode gap to HermiT on SROIQ-heavy inputs (lazy unfolding, deep
model caching, real ⊥-locality module extraction, etc.) and
records the architectural attempts that have already been
measured to dead-end.

## Licensing

This project is dual-licensed under [Apache-2.0](LICENSE-APACHE) **OR**
[MIT](LICENSE-MIT) at your option.

`rustdl` depends on `horned-owl` (LGPL-3.0). Our own source code remains
permissively licensed; binaries that statically link `horned-owl` inherit
LGPL-3.0 obligations for the `horned-owl` portion (right-to-relink, source
disclosure). See [`NOTICE`](NOTICE) for details. In practice this is satisfied
automatically for open-source distributions because cargo + GitHub make the
underlying sources available.

Contributions are licensed under the same dual-license terms; submitting a
pull request constitutes acceptance of this. We do not require a separate CLA.

## Roadmap

| Phase | Deliverable | Estimate |
|------:|-------------|---------:|
| 0 | Workspace scaffold, IR with structural sharing, told-subsumer tables | ~3 weeks |
| 1 | NNF, structural transformation, absorption | 4-6 weeks |
| 2 | Correct ALC tableau, differential-tested vs HermiT | 8-10 weeks |
| 3 | ALCHIQ + minimal datatypes (boolean / integer-range / string-equality) | 8-10 weeks |
| 4 | Optimization stack (anywhere blocking, backjumping, caching) | 10-12 weeks |
| 5 | Nominals (O), complex role hierarchies (R) — full SROIQ | 8-10 weeks |
| 6 | Saturation engine + hybrid orchestration | 8-10 weeks |
| 7 | Full datatypes / concrete domains | 4-6 weeks |
| 8 | ABox, realization, queries | 8-10 weeks |
| 9+ | Optional: hypertableau rewriting, OWLlink server | — |

Total target: ~24-36 months to ORE-competitive performance.

## Real-ontology corpus

A small set of public ontologies (SIO, SULO, family, pizza, RO, GO) lives
outside the repo and is pulled on demand:

```sh
./scripts/fetch-real-ontologies.sh
./target/release/owl-dl-bench corpus ontologies/real --repeats 5
```

See [`docs/real-ontology-corpus.md`](docs/real-ontology-corpus.md)
for sources, formats, the ROBOT-based conversion, and how to add
new entries. Files land in `ontologies/real/` which is gitignored.

## References

- Strategy v2: [`docs/owl-dl-reasoner-rust-strategy-v2.md`](docs/owl-dl-reasoner-rust-strategy-v2.md)
- Strategy v1 (kept for diff): [`docs/owl-dl-reasoner-rust-strategy.md`](docs/owl-dl-reasoner-rust-strategy.md)
- Performance plan: [`docs/outperform-hermit-plan.md`](docs/outperform-hermit-plan.md)
- Latest perf snapshot: [`docs/perf-2026-05-24-new-server.md`](docs/perf-2026-05-24-new-server.md)
- Real-ontology corpus: [`docs/real-ontology-corpus.md`](docs/real-ontology-corpus.md)
