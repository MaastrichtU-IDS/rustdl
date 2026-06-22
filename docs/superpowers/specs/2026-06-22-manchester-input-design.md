# Manchester (`.omn`) input — design

**Date:** 2026-06-22
**Status:** implemented
**Branch:** `feat/manchester-input`

Wire OWL Manchester Syntax as a first-class **input** format for rustdl, giving it
symmetric Manchester I/O — rustdl already *writes* Manchester (justify / diagnose /
repair / report render axioms via `horned_owl::io::omn::AsManchester`); now it
*reads* it too.

## Why this is unblocked (not waiting on upstream)

rustdl pins horned-owl **by git rev** (`micheldumontier/horned-owl` rev
`b188edaf` in the root `Cargo.toml`), and that rev *is* the fork tip and already
contains a conformance-tested Manchester reader (`horned_owl::io::omn::reader::read`,
20/20 reader tests green: declaration pre-pass, HasKey, data properties, cardinality,
adversarial fuzzing). Whether `phillord/horned-owl` ever merges PR #176 is a
community-maintenance question with **zero bearing on rustdl** — we depend on our rev
directly. The only gap was that rustdl never wired `.omn` into its own loaders.

## Scope

Front-end parser dispatch only — **no engine / reasoning change**. The omn reader
produces the same `SetOntology<RcStr>` the ofn/owx/rdf readers produce, so everything
downstream (convert → normalize → reason) is byte-for-byte unchanged. **FP=0 is
structurally untouched** (no new reasoning code path).

## Components

1. **Python loader** — `crates/owl-dl-py/src/load.rs`:
   - `use horned_owl::io::omn::reader::read as read_omn;`
   - extension match adds `Some("omn") => "omn"`.
   - `parse_with_format` adds `"omn" | "manchester" => read_omn(...)`.
   - "unknown extension/format" error strings list `omn`.

2. **CLI loader** — `crates/owl-dl-cli/src/main.rs`:
   - new `OntFormat::Omn` variant.
   - `detect_format`: Manchester is sniffed by its **colon-form** keywords —
     first meaningful line starts with `Prefix:` / `Ontology:` or a frame keyword
     (`Class:` / `ObjectProperty:` / `DataProperty:` / `AnnotationProperty:` /
     `Individual:` / `Datatype:`). Unambiguous against OFN's paren form `Prefix(`
     (the OFN check runs first; the forms don't collide). Extension fallback adds
     `Some("omn") => Omn`.
   - `parse_ofn` and `parse_ofn_with_pm` add `OntFormat::Omn` arms calling
     `read_omn`. The `_with_pm` path uses the reader's real `PrefixMapping`, so
     Manchester explanation output round-trips with abbreviated IRIs.
   - multi-format `--help` doc comments bumped to `.ofn / .owx / .owl / .rdf / .omn`.

3. **Surface/docs:** `__init__.pyi` `classify`/`classify_bytes` docs add `.omn` /
   `"omn"`; native `classify`/`classify_bytes` `///` docs updated; README classify
   comment updated.

## Tests

- **CLI (Rust, `format_detect_tests` + `manchester_parse_tests`):** Manchester
  `Prefix:` / bare `Class:` frames detect as `Omn` (content wins over a misleading
  extension); OFN paren `Prefix(` still wins (no regression); inconclusive `.omn`
  ext → `Omn`; and an end-to-end `parse_ofn` round-trip parses a Manchester source to
  the expected `SubClassOf` axiom.
- **Python (CI, `test_manchester.py`):** `classify(<file>.omn)` reads the subsumption
  chain; `classify_bytes(data, format="omn")` and the `"manchester"` alias work;
  `debug(<broken>.omn)` returns the expected root/derived `Diagnosis`.

## Verification

- 4 new CLI tests + 4 Python tests green; `cargo test --workspace` = 61 groups, 0
  failed; `cargo fmt --all -- --check` clean; `cargo clippy -p owl-dl-cli -p
  owl-dl-py --all-targets --all-features -- -D warnings` clean.
- End-to-end: `rustdl classify pizza.omn` yields the correct subsumption chain;
  `rustdl diagnose broken.omn` partitions root/derived with Manchester-rendered
  justifications.

## Out of scope

- Upstream PR #176 merge (non-blocking; we keep the rev pin).
- A Manchester *serialization* subcommand (we already render Manchester in
  explanations).
