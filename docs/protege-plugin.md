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

Consistency, class hierarchy (classify), unsatisfiable classes, and class
assertions (types/instances via realize). Property hierarchies/values,
same/different individuals, disjointness, and complex-class-expression queries
return empty (not yet backed by rustdl). An `incomplete` classification (some
hard class pairs hit the per-pair budget) is logged; results stay sound (no false
subsumptions, some may be missed).

## Config

- `-Drustdl.bin=…` / `RUSTDL_BIN=…` — use a specific binary (default: bundled)
- `-Drustdl.timeout.seconds=…` / `RUSTDL_TIMEOUT_SECONDS=…` — per-call timeout (default 600)

## Build from source

See `protege/README.md`.
