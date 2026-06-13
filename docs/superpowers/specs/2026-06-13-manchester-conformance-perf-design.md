# Manchester I/O Conformance & Performance Report — Design

**Date:** 2026-06-13
**Status:** Approved (design); ready for implementation plan
**Scope:** Exhaustive compliance + performance testing of the horned-owl fork's
`io/omn` OWL 2 Manchester Syntax **reader and writer**, producing reports.

## Goal

Produce three durable documents that characterize the `io/omn` reader+writer:

1. **`compliance-report.md`** — §2.5 conformance: a per-construct coverage
   matrix, full-corpus parse + round-trip, semantic axiom-set equality vs the
   OWL-API, and adversarial / no-panic robustness.
2. **`performance-report.md`** — comparative read/write speed + peak memory:
   ours-omn vs **omny** (Python), **OWL-API/ROBOT** (Java), **fastobo
   horned-manchester** (Rust), plus intra-crate **ours-ofn / ours-owx /
   ours-rdf** baselines, across a tiered real-ontology corpus.
3. **`manchester-io-report.md`** — one-page combined summary linking both,
   with the headline conformance + performance numbers.

The cardinal invariant of the parent project (rustdl FP=0) is not touched here;
this work only measures and reports on the Manchester I/O code.

## Where things live (the fork/pymos split)

- **Compliance** is **Rust, in the fork** (`/data/dumontier/horned-owl-omn`),
  under `tests/` + a report-generator. Self-contained: dimensions that need no
  external tool (per-construct matrix, adversarial/fuzz) ship and run with the
  upstream PR; dimensions that need ROBOT (corpus parse, semantic equality)
  **skip-with-note** when docker is absent, so the suite never hard-depends on
  Java/Python.
- **Performance** is in **`pymos/bench`**: a new Rust subcrate
  `pymos/bench/horned-bench/` (path-deps the fork; **NOT** added to the fork, to
  keep the upstream PR free of benchmark code) + new Python workloads that drive
  it the way `parse_owlapi.py` drives ROBOT.

Constraints carried from the parent project: work only in the fork + pymos; do
**not** push the fork; do **not** modify rustdl's `Cargo` `[patch]`; the upstream
PR is the user's to open.

## Confirmed integration facts (no placeholders)

- pymos `measure_in_subprocess` runs *Python* funcs only; the Rust comparators
  follow the **`parse_owlapi.py` pattern**: `subprocess.run` the binary, parse
  its stdout, construct a `Measurement` manually (fields: `wall_cold`,
  `wall_hot_samples`, `wall_hot_median`, `wall_hot_stddev`, `peak_rss_bytes`,
  `cpu_cold`, `extras: dict`).
- fastobo `horned-manchester` **0.4.0** = parser **and** serializer, on
  **horned-owl 0.14.0**. Cargo coexists 0.14 and the fork's 1.x in one crate
  (separate semver-major trees), so `horned-bench` links both and offers a
  `fastobo-omn` format for read **and** write.
- Fork tests are inline `#[cfg(test)]` (reader/from_pair.rs, reader/lexer.rs,
  writer/*.rs); the compliance harness is **new integration tests** under
  `horned-owl-omn/tests/`.
- ROBOT oracle = `obolibrary/robot:v1.9.6` docker (`robot convert`); omny v0.2.2
  at `/tmp/verify-022/bin/python` (`omny.parse`). No host JRE.
- pymos corpus manifest: `pymos/bench/corpus.py` (tiers tiny→huge, download URLs,
  checksums, axiom counts, source fmt); pre-converted `.omn` fixtures already
  exist for koala / obi-core / hp under `pymos/bench/data/`.

---

## Part A — Compliance harness (fork, Rust)

Four dimensions. Each produces one section of `compliance-report.md` via a
report-generator (a `tests/`-level harness, run on demand, that writes the file).

### A1. §2.5 per-construct coverage matrix (synthetic)

A **data-driven table** `CONSTRUCTS: &[ConstructCase]`, one entry per W3C OWL 2
Manchester Syntax §2.5 production. Each case = `{ id, omn_snippet,
expected_components, residual: Option<ResidualKind> }`. The harness, per case:

1. `read` the snippet → assert the parsed `Vec<AnnotatedComponent>` equals
   `expected_components` (modulo the canonicalization in A3).
2. `write` those components → re-read → assert structural round-trip.
3. Record `read ✓/✗`, `write ✓/✗`, `round-trip ✓/✗`, `note`.

Coverage list (must include every one):
- **Frames:** Class, ObjectProperty, DataProperty, AnnotationProperty,
  Individual, Datatype.
- **Class clauses:** SubClassOf, EquivalentTo, DisjointWith, DisjointUnionOf,
  HasKey, Annotations.
- **ObjectProperty clauses:** Domain, Range, SubPropertyOf, EquivalentTo,
  DisjointWith, InverseOf, Characteristics (Functional, InverseFunctional,
  Reflexive, Irreflexive, Symmetric, Asymmetric, Transitive),
  SubPropertyChain.
- **DataProperty clauses:** Domain, Range, SubPropertyOf, EquivalentTo,
  DisjointWith, Characteristics (Functional).
- **Restriction forms:** `some`, `only`, `value`, `min N`, `max N`,
  `exactly N`, qualified `min/max/exactly N C`, `Self`.
- **Class-expression operators:** `and`, `or`, `not`, `{ individualList }`
  (ObjectOneOf), `inverse R`, nested/parenthesized.
- **Data ranges:** datatype, `and`, `or`, `not`, `{ literalList }`
  (DataOneOf), parenthesized, facet restriction `dt[ facet val, … ]`.
- **Facets:** length, minLength, maxLength, pattern, langRange,
  minInclusive, minExclusive, maxInclusive, maxExclusive.
- **Literals:** typed `"v"^^dt`, plain string, lang-tagged string,
  bare integer, bare decimal, bare float (`f`/`F` suffix).
- **Misc axioms:** EquivalentClasses, DisjointClasses, EquivalentProperties,
  DisjointProperties, SameIndividual, DifferentIndividuals.
- **Datatype definition:** `Datatype: D EquivalentTo: <dataRange>`.
- **Annotations:** entity-frame Annotations, per-list-item leading annotation
  (binds first item only), post-comma annotation, ontology annotation,
  nested annotation-on-annotation, anonymous-individual annotation value.
- **Header:** Ontology IRI, version IRI, Import declaration.
- **Individuals:** named, anonymous (`_:id`) as subject / fact target / list
  member / annotation value.

**Residual rows** (documented limitations) are explicit, tagged
`ResidualKind`, and asserted to behave as documented — never silent:
- `SwrlRule` — no §2.5 form; reader stops at `Rule:`.
- `ComplexLhsGci` — emitted to `# General axioms`; reader skips-with-warning.
- `NestedAnnotationDropped` — parsed, inner nesting dropped (model limit).
- `DataRestrictionAsObject` — `dp some dt` parses as object restriction.
- `HasKeyObjectDataConflation` — data-property keys read back as object keys.
- `BareNameNeedsPrefix` — default `""`-prefix bare name not lexable.

Report column: `id | construct | read | write | round-trip | residual | note`.

### A2. Corpus parse + structural round-trip

Over the **full pymos-tiered corpus** (fetched via a script that reads
`pymos/bench/corpus.py` URLs, or reuses already-downloaded `pymos/bench/data/`).
Per ontology:

1. source → ROBOT `robot convert -o X.owlapi.omn` → our reader: record
   `parse_ok`, `component_count`, or on failure the **first blocking
   construct** (grep the error's line).
2. our writer output (`ofn → our omn`) re-read by **us**: `roundtrip_stable`
   (structural component-set equality).
3. our writer output accepted by **OWL-API** (ROBOT re-parse) → `owlapi_accepts`.
4. our writer output accepted by **omny** (`omny.parse`) → `omny_accepts`.

Report table: `ontology | tier | bytes | parse_ok | components | roundtrip |
owlapi_accepts | omny_accepts | blocking_construct`.

### A3. Semantic axiom-set equality vs OWL-API (the strong oracle)

source → ROBOT → `.omn` → our reader; **canonicalize** both the parsed set and
the source component set, then diff. The canonicalizer encodes **each documented
normalization as an explicit equivalence** so they are not false mismatches:

- **Declaration conflation** — frames emit `Declare*`; ignore declaration
  presence/absence when matching logical axioms (report declaration delta
  separately).
- **n-ary ↔ pairwise** — OWL-API may expand `EquivalentClasses(a,b,c)` to
  pairwise; canonicalize both to a sorted n-ary set.
- **Leading-annotation-to-first** — already correct in the reader; the
  canonicalizer asserts the annotation lands on the first list item.
- **Bare-name / prefix resolution** — compare resolved full IRIs.

Output per ontology: `matched | missing | extra | normalized_away(reason)`.
A non-empty `missing`/`extra` after canonicalization is a **real conformance
finding** and is highlighted. This is the highest-design-risk task — the
canonicalizer is TDD'd against small fixtures with known OWL-API transforms
before being run corpus-wide.

### A4. Adversarial / edge + no-panic fuzz

- **Edge fixtures** (hand-written, read+write+round-trip): unicode IRIs &
  string literals (incl. emoji, RTL), very large multi-line literals, deeply
  nested class expressions (e.g. 50-deep `and`/`not`), every facet on every
  numeric/string datatype, prefix edge cases (empty local, dotted local,
  percent-encoding), comment handling, CRLF line endings.
- **No-panic fuzz:** a `proptest` generator over random byte/token strings
  asserting `read` returns `Ok`/`Err` but **never panics**; report the
  iteration count and any panic seed found.

Report: pass/fail list + fuzz iterations + panics (expected: 0).

### Report generator (A)

A `tests/conformance_report.rs` harness (run with `--ignored` or behind a
feature so normal `cargo test` stays fast) that runs A1–A4 and writes
`docs/manchester/compliance-report.md` (path inside the fork) plus a machine
JSON sidecar. A1 + A4 run unconditionally; A2 + A3 detect ROBOT/omny and emit a
`SKIPPED (no docker)` banner when absent.

---

## Part B — Performance harness (pymos/bench)

### B1. `horned-bench` Rust subcrate

`pymos/bench/horned-bench/` — `Cargo.toml` path-deps the fork
(`horned-owl = { path = "/data/dumontier/horned-owl-omn" }`) and crates.io
`horned-manchester = "0.4"` (pulls horned-owl 0.14, coexisting). Binary CLI:

```
horned-bench --format {omn|ofn|owx|rdf|fastobo-omn} \
             --mode {parse|render} \
             --hot N --warmup M <file>
```

Behavior:
- **parse:** read file text once; warmup M parses; time N hot parses
  in-process; report median/min wall + peak RSS + `component_count` + `bytes`.
- **render:** parse once; warmup M renders; time N hot renders; report likewise
  + `bytes_emitted`.
- `--format fastobo-omn` routes to `horned_manchester::from_str` /
  serializer on the 0.14 model (read + write comparison).
- Peak RSS via `getrusage(RUSAGE_SELF).ru_maxrss` (or `/proc/self/status`).
- Output: one JSON line
  `{format, mode, wall_hot_median_s, wall_hot_min_s, wall_cold_s, peak_rss_bytes, component_count, bytes}`.

Internal timing (not subprocess wall) sidesteps the Rust ~2 ms cold-start that
distorts the ROBOT comparison.

### B2. pymos workloads

`pymos/bench/workloads/parse_horned.py` + `render_horned.py`, mirroring
`parse_owlapi.py`: `subprocess.run(["horned-bench", "--format", fmt, …])`, parse
JSON, build a `Measurement` (`wall_cold`, `wall_hot_samples`=[min,median],
`wall_hot_median`, `peak_rss_bytes`, `extras={"backend": f"horned-{fmt}",
"component_count": …, "bytes": …}`). A runner enumerates the comparison cells:

- **read:** `horned-omn`, `omny` (existing `parse.py`), `owlapi`
  (existing `parse_owlapi.py`), `fastobo-omn`, plus `horned-ofn`,
  `horned-owx`, `horned-rdf`.
- **write:** `horned-omn`, `omny` (existing `render.py`), `fastobo-omn`,
  plus `horned-ofn`, `horned-owx`.

Across tiers tiny→large; **huge** (go-basic) gated behind a flag/time budget.
Inputs: each ontology needs an `.omn` for the Manchester readers and its native
`.ofn`/`.owx`/`.rdf` for the intra-crate baselines — produced by a one-time
ROBOT pre-conversion step (reuse `pymos/bench/data/` where present).

### Report generator (B)

A pymos script aggregates the cells into `performance-report.md`: per-ontology
tables of median wall + peak RSS + throughput (MB/s) per parser, **ratios vs the
OWL-API baseline**, with explicit caveats (Rust internal-timing vs ROBOT
container-startup-subtracted vs omny pure-Python; render comparison excludes any
reader-only comparator). Raw CSV saved under `pymos/bench/results/<date>-manchester/`.

---

## Combined summary

`docs/manchester/manchester-io-report.md` (fork) — links A + B reports, states
the headline conformance (constructs covered, corpus fully-parsed N/M,
axiom-equality rate) and performance (ours vs omny/OWL-API/fastobo ratios), and
restates the residual limitations once, authoritatively.

## Risks & mitigations

- **A3 canonicalizer false mismatches** (highest risk) — TDD against small
  fixtures encoding each known OWL-API transform before corpus-wide runs.
- **Two horned-owl versions in `horned-bench`** — verify the dual-dep compiles
  early (Task 0 smoke build); if it fails, fall back to a separate fastobo bench
  bin invoked side-by-side.
- **ROBOT/docker absence** — every ROBOT-dependent step degrades to
  SKIPPED-with-note, never a hard failure.
- **Corpus download flakiness** — reuse already-downloaded fixtures; checksum
  per the manifest; skip-with-note on fetch failure.

## Out of scope

- Fixing any conformance finding A3 surfaces (report only; fixes are follow-up).
- Reasoning, datatypes, or any rustdl-core change.
- Pushing the fork or opening the upstream PR.
