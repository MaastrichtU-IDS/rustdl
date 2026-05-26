# rustdl

A sound, complete, performant OWL 2 DL reasoner in Rust. Target: parity with
[HermiT](http://www.hermit-reasoner.com/) (hypertableau, Java) and
[Konclude](http://www.derivo.de/produkte/konclude/) (tableau + saturation hybrid,
C++) on the ORE benchmarks.

**Status:** Phases 0–5 complete; Phase 6 (EL saturation engine + hybrid
orchestrator) operational. End-to-end reasoning over SROIQ on the tableau
side, with a consequence-based EL saturation closure that handles told
subsumption, conjunctive triggers, existential propagation (CR5), role
hierarchy (CR9), length-2 role chains + transitive properties, property
domain/range, Tseitin introduction for compound `∃` bodies, and Bot
detection via DisjointClasses. The orchestrator consults the closure first
and takes a saturation-only fast path when the input lives entirely inside
the EL fragment; otherwise it falls back to the tableau on misses.

Public API (`owl-dl-reasoner`): `is_class_satisfiable`, `is_consistent`,
`is_subclass_of`, `is_instance_of`, `instances_of`, `classify`, `realize`.
CLI (`rustdl`) exposes each as a subcommand. `owl-dl-bench` is a separate
binary with `classify` / `synthetic-el` / `corpus` for harness work.

See [`owl-dl-reasoner-rust-strategy-v2.md`](owl-dl-reasoner-rust-strategy-v2.md)
for the full multi-year strategy.

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

## Workspace layout

```
crates/
  owl-dl-core/        # IR, normalization, told-subsumers, told-disjoints
  owl-dl-saturation/  # Consequence-based EL engine (Phase 6)
  owl-dl-tableau/     # SROIQ tableau engine (Phase 2-5)
  owl-dl-datatypes/   # Datatype reasoners (Phase 3 minimal, Phase 7 full)
  owl-dl-reasoner/    # Hybrid orchestrator + public API
  owl-dl-cli/         # `rustdl` binary
  owl-dl-bench/       # ORE corpus benchmark harness
xtask/                # Build automation (corpus fetch, license inventory, ...)
```

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
#                   default          --saturation-only   edge loss
#   sulo-stripped   0.23 s           0.01 s              0
#   pizza           28.9 s           0.03 s              20.6 %
#   sio-stripped    266 s            0.22 s              0.19 %
#
# See docs/perf-2026-05-24-new-server.md §8 for the full table
# vs HermiT/Pellet/Konclude.
./target/release/rustdl classify --saturation-only path/to/ontology.ofn

# Other queries
./target/release/rustdl consistent path/to/ontology.ofn
./target/release/rustdl sat        path/to/ontology.ofn <class-iri>
./target/release/rustdl subclass   path/to/ontology.ofn <sub-iri> <sup-iri>
./target/release/rustdl realize    path/to/ontology.ofn

# Realize accepts the same --saturation-only flag (skips every
# tableau probe in both classify and per-individual instance check).
# family-stripped's 300+ individuals realize in 0.16 s with the
# flag vs. DNF without.
./target/release/rustdl realize --saturation-only path/to/ontology.ofn
```

Diagnostic env knobs:

- `RUSTDL_COUNTERS=1` — dump per-rule call counts to stderr on
  `TableauContext::drop`. Requires `--features counters` at build time.
- `RUSTDL_TRACE=1` — one stderr line per `search`/`branch` decision
  for understanding what the tableau is doing on a single probe.
  Off-path is one atomic load; safe to ship enabled.

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
