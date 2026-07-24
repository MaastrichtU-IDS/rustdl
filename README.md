# rustdl

[![CI](https://github.com/MaastrichtU-IDS/rustdl/actions/workflows/ci.yml/badge.svg)](https://github.com/MaastrichtU-IDS/rustdl/actions/workflows/ci.yml)
[![version](https://img.shields.io/github/v/tag/MaastrichtU-IDS/rustdl?sort=semver&label=version&color=blue)](https://github.com/MaastrichtU-IDS/rustdl/tags)
[![license](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#licensing)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Python](https://img.shields.io/badge/Python-3.10%2B-blue?logo=python&logoColor=white)](https://pypi.org/project/rustdl/)
[![Protégé plugin](https://img.shields.io/badge/Prot%C3%A9g%C3%A9-plugin-blue)](protege/README.md)

A **sound** OWL 2 DL (SROIQ) reasoner in Rust. Konclude-style hybrid: a
consequence-based **saturation** engine handles the EL-ish fragment, a
**tableau** + hypertableau **wedge** handles the rest of SROIQ, and an
orchestrator picks per query. Parsing and the OWL model come from
[`horned-owl`](https://github.com/phillord/horned-owl).

## Status

A working classifier, consistency checker, and instance reasoner for SROIQ(D)
with first-class data properties.

- **Sound.** Every reported subsumption is a genuine entailment.
- **Consistent.** Consistency via a consequence-based ABox-saturation pre-check
  plus the tableau.
- **Completeness.** On the EL/Horn fragment with no timeout the hierarchy is
  complete by construction; beyond it the classifier trusts the wedge's `Sat`
  verdicts — empirically near-complete on the measured corpus, not *provably*
  complete.
- **Proofs.** A step-level, rule-by-rule proof tree for any entailment — a
  checkable certificate of soundness on the EL/Horn fragment.
- **Explain.** A minimal responsible-axiom set for any entailment, narrowable to
  the responsible fragment.
- **Repair.** Root-cause diagnosis of unsatisfiability plus minimal, verified
  fixes.
- **Report.** A self-contained HTML report bundling the explanations, diagnosis,
  and repairs.

## Coverage

**Supported (sound; complete on the EL/Horn fragment where the guarantee holds — see completeness note):**

- SROIQ object-property reasoning — role hierarchies; transitive, symmetric,
  asymmetric, irreflexive, functional, inverse-functional; inverse roles; role
  chains (longer chains decomposed to 2-leg cascades).
- Class expressions — intersection, union, complement, nominals (`{a}`),
  existential / universal restrictions, qualified cardinality (`≥n R.C`, `≤n R.C`).
- `DisjointClasses`, `EquivalentClasses`, `DisjointUnion`.
- ABox — `ClassAssertion`, `ObjectPropertyAssertion`,
  `NegativeObjectPropertyAssertion`, `SameIndividual`, `DifferentIndividuals`;
  consistency via an ABox-saturation pre-check + clash-pattern checks.
- **Data properties are first-class** (default-on) — data-property axioms lower to
  the object fragment; concrete domains cover integer / float / decimal / date /
  dateTime / string value-membership, faceted ranges, `DataOneOf`, flat
  `DataUnionOf` / `DataIntersectionOf` / `DataComplementOf`, and bounded data
  cardinality.

**Sound under-approximation (silently dropped, never an error):** the narrow
datatype tail outside that recognized set — nested composite ranges (e.g.
`DataComplementOf(DataUnionOf(…))`), `∀` / range / cardinality *over* a union or
complement, and lexically-unparseable or cross-datatype literals. Positives
depending on them may be missed, never falsely asserted.

**Unsupported (errors):** `HasKey`. SWRL rules are skipped (see `convert.rs`).

## Crates

`owl-dl-core` (IR + normalization), `owl-dl-saturation` (EL closure),
`owl-dl-tableau` (SROIQ tableau + hypertableau wedge), `owl-dl-datatypes`
(concrete domains), `owl-dl-reasoner` (orchestrator + public API), `owl-dl-cli`
(`rustdl` binary), `owl-dl-bench` (corpus/benchmark harness), `owl-dl-py` (Python
bindings). `xtask` holds build automation.

## Quickstart

### Python

```sh
pip install rustdl          # Python 3.10+, prebuilt ABI3 wheels
```

```python
import rustdl

# classify — input format (OFN / OWX / RDF-XML / Manchester .omn) is auto-detected.
# Bounded by default: per_pair_timeout_ms=100, global_timeout_ms=60000 (ms).
result = rustdl.classify("ontology.ofn")
print(f"{len(result.classes)} classes, {len(result.unsatisfiable)} unsatisfiable")
result.complete                                                # False if a timeout cut any pair
result.is_subclass("http://ex.org/Sub", "http://ex.org/Sup")   # -> bool
result.direct_subsumers("http://ex.org/Sub")                   # -> list[str]

# Tune the budgets (ms) — each cut pair defaults to "not subsumed" (sound); 0 disables a bound:
rustdl.classify("ontology.ofn", per_pair_timeout_ms=25, global_timeout_ms=30000)
rustdl.classify("ontology.ofn", per_pair_timeout_ms=0, global_timeout_ms=0)   # unbounded = complete

rustdl.is_consistent("ontology.ofn")   # -> bool
rustdl.debug("ontology.ofn")           # consistency + root/derived unsat + justifications + repairs
```

More surface — inferred property assertions, subproperty axioms, existential
successors, `justify`/`repair` — in the end-to-end
[QA tutorial](docs/python-ontology-qa.md).

### Rust

```sh
cargo add owl-dl-reasoner horned-owl   # in-process library — no JVM, no subprocess
```

```rust
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

let src = std::fs::read_to_string("ontology.ofn")?;
let (onto, _): (SetOntology<RcStr>, _) =
    read_ofn(&mut std::io::Cursor::new(src), ParserConfiguration::default())?;

let h = classify(&onto)?;                          // full class hierarchy
println!("{} classes", h.classes().len());
h.is_subclass("http://ex.org/Sub", "http://ex.org/Sup");   // -> bool
for parent in h.direct_subsumers("http://ex.org/Sub") {
    println!("{parent}");
}
```

Bounded variants (`classify_with_budget`, `classify_with_timeout`) and
consistency / instance queries live in the same crate; see
`cargo run -p owl-dl-reasoner --example embed_classify`. For the command-line
tool instead, see [CLI](#cli).

## Build & test

```sh
cargo build --workspace --release     # Rust 1.88+, edition 2024
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## CLI

```sh
cargo install --git https://github.com/MaastrichtU-IDS/rustdl owl-dl-cli   # install the `rustdl` binary

rustdl classify  ontology.ofn               # full class hierarchy (default)
rustdl classify  ontology.ofn --pair-timeout-ms 25 --global-timeout-ms 60000  # bounded run
rustdl consistent ontology.ofn
rustdl subclass  ontology.ofn <sub> <sup>
rustdl instances ontology.ofn <class>
rustdl realize   ontology.ofn [--properties] # per-individual types (+ inferred object & data property assertions)
rustdl justify   ontology.ofn <query…>      # minimal responsible-axiom set (why it holds)
rustdl justify --laconic ontology.ofn <query…>  # pinpoint the responsible PART of each axiom
rustdl repair    ontology.ofn <query…>      # minimal axiom removals to break an entailment
rustdl prove     ontology.ofn <sub> <sup>   # step-level DL proof tree
rustdl diagnose  ontology.ofn               # root vs derived unsatisfiable classes (where to start fixing)
rustdl report    ontology.ofn -o report.html # self-contained HTML debugging report
rustdl explain   ontology.ofn <sub> <sup>   # which engine answered (saturation vs tableau)
```

**Bounded classification** — sound under-approximation (every reported subsumption
still holds; pairs not decided in time default to "not subsumed", never a false
one):

- `--pair-timeout-ms N` — cap each pairwise tableau probe (default 1000; `0` =
  unbounded). Good for pathological SROIQ where a few pairs never terminate.
- `--global-timeout-ms N` — bound the **reasoning** wall for the whole classify
  (`0` = unbounded): each probe is cut at the smaller of the per-pair and global
  budgets, so total probing can't grow with the pair count. The fixed saturation +
  preprocessing overhead (seconds to tens of seconds on very large ontologies) is
  *not* deadline-gated, so the wall floor isn't zero.
- `--saturation-only` — skip the tableau entirely and report only the EL closure.

Any run that hits a bound prints a prominent `INCOMPLETE` warning to stderr.
Diagnostics: `RUSTDL_TRACE=1` (one stderr line per search/branch decision);
`RUSTDL_COUNTERS=1` with `--features counters` (per-rule call counts).

## Protégé plugin

rustdl runs as a Protégé Desktop reasoner.

**Install:** download `rustdl-protege-<version>.jar` from the
[latest release](https://github.com/MaastrichtU-IDS/rustdl/releases/latest), drop
it into Protégé's `plugins/` directory, and restart. Choose **rustdl** under
Reasoner ▸, then Reasoner ▸ Start reasoner.

**Computes (v1):** consistency, the inferred class hierarchy, unsatisfiable
classes, and class assertions (types / instances). Property hierarchies & values,
same/different individuals, disjointness, and complex-class-expression queries
return empty for now.

**Config** (JVM system property, else env var): `-Drustdl.bin=…` / `RUSTDL_BIN`
for a specific binary; `-Drustdl.timeout.seconds=…` / `RUSTDL_TIMEOUT_SECONDS`
(default 600) for the per-call timeout.

Requires Protégé 5.6.x (Java 11+). Build-from-source and the full contract:
[`protege/README.md`](protege/README.md), [`docs/protege-plugin.md`](docs/protege-plugin.md).

## Licensing

Dual-licensed [Apache-2.0](LICENSE-APACHE) **OR** [MIT](LICENSE-MIT). `horned-owl`
is LGPL-3.0; binaries that statically link it inherit LGPL-3.0 obligations for that
portion (see [`NOTICE`](NOTICE)). Contributions are accepted under the same dual
license; no separate CLA.
