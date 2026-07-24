# rustdl Protégé plugin

Bundles the rustdl OWL 2 reasoner as a Protégé reasoner plugin.

## Build (dev)

    brew install maven          # JDK 11+ + Maven
    mvn -f protege/pom.xml package
    # → protege/target/rustdl-protege-0.0.0-SNAPSHOT.jar

The dev jar contains no bundled binaries; run Protégé with
`-Drustdl.bin=/path/to/rustdl` (build one via `cargo build --release --bin rustdl`).

## Install

Drop the release jar (which bundles all four platform binaries) into Protégé's
`plugins/` directory and restart. "rustdl" then appears in Reasoner ▸.

- macOS: `~/.Protege/plugins/` or `Protege.app/Contents/Java/plugins/`
- Linux: `~/.Protege/plugins/` or `<Protege>/plugins/`
- Windows: `%USERPROFILE%\.Protege\plugins\` or `<Protege>\plugins\`

## Config (system property overrides env overrides default)

- `rustdl.bin` / `RUSTDL_BIN` — path to a rustdl binary (default: the bundled one)
- `rustdl.timeout.seconds` / `RUSTDL_TIMEOUT_SECONDS` — per-call timeout (default 600)
