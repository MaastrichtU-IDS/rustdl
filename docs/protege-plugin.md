# rustdl Protégé plugin

rustdl ships as a Protégé reasoner plugin: install one jar and rustdl appears in
Reasoner ▸ with no separate binary install (the four platform binaries are
bundled and the matching one is extracted at first use).

## Install

Download `rustdl-protege-<version>.jar` from the [GitHub Release](https://github.com/MaastrichtU-IDS/rustdl/releases),
drop it into Protégé's `plugins/` directory, and restart. Select **rustdl** from
Reasoner ▸, then Reasoner ▸ Start reasoner.

Supported platforms: Linux x86_64/aarch64, macOS Apple Silicon, Windows x86_64.
Other platforms (e.g. Intel macOS): build a binary (`cargo build --release --bin
rustdl`) and launch Protégé with `-Drustdl.bin=/path/to/rustdl`.

## What it computes (v1)

Consistency, class hierarchy (classify), unsatisfiable classes, class
assertions (types/instances via realize), object/data property hierarchies,
class/object-property/data-property disjointness, same/different
individuals, and object/data property values (assertions) — all 9 OWLAPI
`InferenceType`s are precomputable and cache-backed. Complex (anonymous)
class-expression queries (`isSatisfiable`/`isEntailed`) instead throw
`UnsupportedOperationException` — a boolean guess there would be unsound;
this is the one remaining empty stub. An `incomplete` result from any
subprocess (some hard pairs hit the per-pair budget) is logged; results
stay sound (no false answers, some may be missed).

## Config

- `-Drustdl.bin=…` / `RUSTDL_BIN=…` — use a specific binary (default: bundled)
- `-Drustdl.timeout.seconds=…` / `RUSTDL_TIMEOUT_SECONDS=…` — per-call timeout (default 600)

## Build from source

See `protege/README.md`.
