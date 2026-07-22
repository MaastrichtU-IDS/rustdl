# rustdl

[![version](https://img.shields.io/github/v/tag/MaastrichtU-IDS/rustdl?sort=semver&label=version&color=blue)](https://github.com/MaastrichtU-IDS/rustdl/tags)
[![license](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#licensing)

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

## Install

```sh
cargo add owl-dl-reasoner                                              # library
cargo install --git https://github.com/MaastrichtU-IDS/rustdl owl-dl-cli   # CLI
pip install rustdl                                                     # Python 3.10+ (ABI3)
```

```python
import rustdl
result = rustdl.classify("ontology.ofn")   # OFN / OWX / RDF-XML / Manchester (.omn) — auto-detected
print(f"{len(result.classes)} classes; {len(result.unsatisfiable)} unsat")
print(result.is_subclass("http://ex.org/Sub", "http://ex.org/Sup"))
ok = rustdl.is_consistent("ontology.ofn")
edges = rustdl.materialize_inferred_property_assertions("ontology.ofn")  # inferred object property assertions
data_edges = rustdl.materialize_inferred_data_property_assertions("ontology.ofn")  # inferred data property assertions
sub_obj = rustdl.materialize_inferred_subobjectproperty_axioms("ontology.ofn")  # object property hierarchy
sub_dat = rustdl.materialize_inferred_subdataproperty_axioms("ontology.ofn")    # data property hierarchy
succ = rustdl.materialize_existential_successors("ontology.ofn")  # entailed exists-successors (blank-node witnesses)
report = rustdl.debug("ontology.ofn")   # consistency + root/derived unsat + justifications + repairs
```

New to the Python API? Walk through
[**Debugging an ontology with rustdl**](docs/python-ontology-qa.md) — an end-to-end
QA tutorial (classify → `debug()` → justify/repair → fix → read inferred facts).

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
