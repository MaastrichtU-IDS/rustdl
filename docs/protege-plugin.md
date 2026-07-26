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

## Explaining inferences

The plugin also backs Protégé's explanation surface, so a computed hierarchy
edge isn't just an answer — you can ask why.

**Justifications ("?" dialog).** Click the "?" next to any inferred axiom
(e.g. a superclass rustdl added to the hierarchy) and the Explanation dialog
now offers two rustdl entries alongside any other installed explanation
sources:

- **rustdl** — minimal justifications: each returned explanation is a
  subset-minimal set of axioms from your ontology that entails the
  selected axiom (`rustdl justify --json` under the hood).
- **rustdl (laconic)** — as above, but each justification axiom is further
  weakened to the fragment actually responsible for the entailment (e.g. one
  disjunct of a conjunction, one `∃`-filler), which is often easier to read
  than the full told axiom (`rustdl justify --laconic --json`).

Both sources only ever surface axioms genuinely present in your ontology —
rustdl cannot fabricate a justification axiom that isn't already there.

**Proof view (step-level proofs).** Where the Explanation dialog shows the
*minimal axiom set*, the proof view shows the *derivation steps* between
them — e.g. how two `SubClassOf` axioms combine via transitivity to entail a
third. For entailments in the EL fragment, rustdl provides a genuine
step-level proof tree (`rustdl prove --json`); outside that fragment it
degrades to a single step showing the justification as one "black box"
inference, rather than refusing to answer.

**Prerequisite — proof-explanation plugin required, like ELK.** The
step-level proof view is a separate Protégé feature backed by the
[Protégé proof-explanation plugin](https://github.com/liveontologies/protege-proof-explanation).
As with ELK, rustdl's proof-tree support only appears once that companion
plugin is also installed — this plugin's own jar does **not** bundle it (the
proof-service dependencies are declared optional and are not embedded in the
`rustdl-protege-<version>.jar`). Without it, rustdl's reasoning, hierarchy
classification, and both Explanation-dialog justification sources above work
exactly as normal; only the step-level proof-tree view is unavailable.

**Config knobs** (system property / environment variable, property wins if
both are set):

- `rustdl.explain.max.justifications` / `RUSTDL_EXPLAIN_MAX_JUSTIFICATIONS` —
  cap on the number of justifications rustdl enumerates per explanation
  request (default 8)
- `rustdl.explain.timeout.seconds` / `RUSTDL_EXPLAIN_TIMEOUT_SECONDS` — per-call
  timeout for the underlying `justify`/`prove` subprocess (default 600)

## Config

- `-Drustdl.bin=…` / `RUSTDL_BIN=…` — use a specific binary (default: bundled)
- `-Drustdl.timeout.seconds=…` / `RUSTDL_TIMEOUT_SECONDS=…` — per-call timeout (default 600)

## Build from source

See `protege/README.md`.
