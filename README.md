# rustdl

A sound, complete, performant OWL 2 DL reasoner in Rust. Target: parity with
[HermiT](http://www.hermit-reasoner.com/) (hypertableau, Java) and
[Konclude](http://www.derivo.de/produkte/konclude/) (tableau + saturation hybrid,
C++) on the ORE benchmarks.

**Status:** Phase 0 scaffolding (Day 1-2 of the 30-day plan). No reasoning
implemented yet. See
[`owl-dl-reasoner-rust-strategy-v2.md`](owl-dl-reasoner-rust-strategy-v2.md)
for the full strategy.

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
cargo build --workspace

# Run lint and format checks
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run tests
cargo test --workspace
```

Requires Rust 1.88+.

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

## References

- Strategy v2: [`owl-dl-reasoner-rust-strategy-v2.md`](owl-dl-reasoner-rust-strategy-v2.md)
- Strategy v1 (kept for diff): [`owl-dl-reasoner-rust-strategy.md`](owl-dl-reasoner-rust-strategy.md)
