# rustdl

A **sound** OWL 2 DL (SROIQ) reasoner in Rust. Konclude-style hybrid: a
consequence-based **saturation** engine handles the EL-ish fragment, a
**tableau** + hypertableau **wedge** handles the rest of SROIQ, and an
orchestrator picks per query. Parsing and the OWL model come from
[`horned-owl`](https://github.com/phillord/horned-owl).

## Status (v0.3.9)

A working classifier, consistency checker, and instance reasoner for SROIQ(D)
with first-class data properties. The defining property is **soundness**: every
reported subsumption is a genuine entailment.

- **FP = 0 at scale.** Verified against the Konclude ∩ HermiT oracle across all
  201 diffed ORE-2015 pilot ontologies *and* the curated corpus — rustdl asserts
  nothing that neither complete reasoner does.
- **Fastest on EL.** rustdl's saturation kernel beats whelk-rs (1.4–1.9×) and ELK
  (4.5×) on galen/notgalen, deriving a sound *superset* of their closures.
- **Competitive on DL, not the speed leader.** After recent work, most DL
  ontologies classify within ~10–50× of Konclude (the mature C++ tableau, which
  wins on speed); HermiT is slower still and itself DNFs on a hard tail.
  **wine** is rustdl's one DNF (combinatorial nominal+disjunction).
- Detects the **family** inconsistency (a consequence-based ABox-saturation
  pre-check) that the per-pair tableau alone misses.
- **Explains *and* debugs — a full suite.** Built-in CLI commands turn rustdl into
  an ontology-debugging tool, not just a classifier:
  - `justify` — a minimal responsible-axiom set for any entailment (`--laconic`
    weakens each axiom to its responsible *fragment*).
  - `prove` — a step-level proof tree.
  - `diagnose` — partitions unsatisfiable classes into **root** causes vs **derived**
    collateral ("where to start fixing").
  - `repair` — minimal axiom-removal sets to *break* an unwanted entailment, each
    verified.
  - `report` — a self-contained HTML debugging report combining all of the above.

  Every result is **sound by construction** (justifications and repairs are verified
  against the reasoner). **Konclude** (the DL speed leader) has **no built-in
  justification or explanation facility** at all (its interface is classification /
  consistency / realization / SPARQL only) — so the fastest reasoner tells you *that*
  a subsumption holds but not *why*, or *how to fix* a broken ontology. rustdl does.

Full head-to-head (5 reasoners × 2 corpora):
[`docs/reasoner-comparison-2026-06-21.md`](docs/reasoner-comparison-2026-06-21.md).

**Completeness is partial** — the default classifier is empirically near-complete
across the measured corpus but not *provably* complete in general (it trusts the
wedge's `Sat` verdicts; see the soundness contract in `CLAUDE.md` and
[`docs/fragment-completeness.md`](docs/fragment-completeness.md)). The hard residual
is the engineering-maturity gap to Konclude's optimized tableau, not a missing
technique.

## Coverage

**Supported (sound; complete on the fragment, oracle-validated):**

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
  dateTime / string value-membership, faceted ranges, `DataOneOf`, and bounded
  data cardinality.

**Sound under-approximation (silently dropped, never an error):** data ranges /
nested composites outside the recognized set; positives depending on them may be
missed, never falsely asserted.

**Unsupported (errors):** `HasKey`. SWRL rules are skipped (see `convert.rs`).

## Crates

`owl-dl-core` (IR + normalization), `owl-dl-saturation` (EL closure),
`owl-dl-tableau` (SROIQ tableau + hypertableau wedge), `owl-dl-datatypes`
(concrete domains), `owl-dl-reasoner` (orchestrator + public API), `owl-dl-cli`
(`rustdl` binary), `owl-dl-bench` (corpus/benchmark harness), `owl-dl-py` (Python
bindings). `xtask` holds build automation.

## Install

```sh
cargo add owl-dl-reasoner                                              # library
cargo install --git https://github.com/MaastrichtU-IDS/rustdl owl-dl-cli   # CLI
pip install rustdl                                                     # Python 3.10+ (ABI3)
```

```python
import rustdl
result = rustdl.classify("ontology.ofn")   # OFN / OWX / RDF-XML (auto-detected)
print(f"{len(result.classes)} classes; {len(result.unsatisfiable)} unsat")
print(result.is_subclass("http://ex.org/Sub", "http://ex.org/Sup"))
ok = rustdl.is_consistent("ontology.ofn")
edges = rustdl.materialize_inferred_property_assertions("ontology.ofn")  # inferred object property assertions
data_edges = rustdl.materialize_inferred_data_property_assertions("ontology.ofn")  # inferred data property assertions
sub_obj = rustdl.materialize_inferred_subobjectproperty_axioms("ontology.ofn")  # object property hierarchy
sub_dat = rustdl.materialize_inferred_subdataproperty_axioms("ontology.ofn")    # data property hierarchy
```

## Build & test

```sh
cargo build --workspace --release     # Rust 1.88+, edition 2024
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## CLI

```sh
rustdl classify  ontology.ofn               # full class hierarchy (default)
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

Sound under-approximation modes (every reported subsumption still holds; positives
may be missed): `--saturation-only` (skip the tableau, EL-closure only) and
`--pair-timeout-ms N` (per-pair tableau deadline; cut pairs default to "not
subsumed" — robust on pathological SROIQ inputs). Diagnostics: `RUSTDL_TRACE=1`
(one stderr line per search/branch decision); `RUSTDL_COUNTERS=1` with
`--features counters` (per-rule call counts).

## Licensing

Dual-licensed [Apache-2.0](LICENSE-APACHE) **OR** [MIT](LICENSE-MIT). `horned-owl`
is LGPL-3.0; binaries that statically link it inherit LGPL-3.0 obligations for that
portion (see [`NOTICE`](NOTICE)). Contributions are accepted under the same dual
license; no separate CLA.

## More

- Reasoner comparison: [`docs/reasoner-comparison-2026-06-21.md`](docs/reasoner-comparison-2026-06-21.md)
- Perf vs Konclude/HermiT: [`docs/perf-2026-06-08-konclude-vs-rustdl.md`](docs/perf-2026-06-08-konclude-vs-rustdl.md)
- Completeness envelope: [`docs/fragment-completeness.md`](docs/fragment-completeness.md)
- Strategy: [`docs/owl-dl-reasoner-rust-strategy-v2.md`](docs/owl-dl-reasoner-rust-strategy-v2.md)
- Engineering notes & soundness contract: [`CLAUDE.md`](CLAUDE.md)
