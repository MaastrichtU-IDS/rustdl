# Closing the Phase-D blind spot — datatypes on real data

Added 2026-06-06. Goal: validate rustdl's Phase-D datatype handling on a *real*
datatype-heavy ontology (previously only synthetic fixtures tested it).

## Fixture: `bibtex`

ORE-2015 `ore_ont_3341` = the `edu.mit.visus.bibtex` ontology. 15 classes,
**41 DataMinCardinality + 40 DataPropertyDomain + 39 DataPropertyRange**, a real
class hierarchy (16-pair atomic closure). Extracted from the (gitignored)
`ore2015_sample.zip`; `fetch_bibtex` in `scripts/fetch-real-ontologies.sh`
reproduces it.

## Result

```
bibtex   rustdl_closure=16   konclude_closure=16   FP=0   MISSED=0
```

**Full parity** — rustdl matches HermiT exactly on a datatype-heavy ontology.
Phase-D's sound under-approximation (drop/normalize unrecognized data axioms)
is validated FP=0 on real data, not just synthetic canaries.

## The meta-finding: real datatypes don't drive classification

I screened ~5 ORE datatype-bearing candidates (ore_ont_4719, 8047=ical, 1618,
7988, 3341=bibtex). The consistent result: **real ontologies assert their class
hierarchies explicitly (told `SubClassOf`); datatype axioms are constraints /
typing, not classification drivers.** Concretely in bibtex:

- `Article ⊑ DataMinCardinality(1, hasTitle)` and
  `DataPropertyDomain(hasTitle, Entry)` form the exact D4 pattern
  (`X ⊑ ≥1 dp` + `Domain(dp, D)` ⟹ `X ⊑ D`) that would derive `Article ⊑ Entry`
  — but `Article ⊑ Entry` is **also told** (an explicit `SubClassOf`), so the
  datatype derivation is redundant. No real ontology in the sample produced a
  *non-redundant* datatype-driven subsumption.
- `ical` (ore_ont_8047, 54 DataMinCardinality) has **no class subsumptions at
  all** — its datatypes describe event properties, inducing no hierarchy.
- `ore_ont_4719` (richest, 10 FunctionalDataProperty) uses anonymous
  individuals, which rustdl doesn't support (Phase 7) — not usable.

**Implication.** The D4/D5 patterns (datatype-driven subsumption, unsat from
functional+cardinality clashes or empty facet intersections) remain exercised
only by the synthetic fixtures (`tests/fixtures/datatype/`) *because real
ontologies don't classify via datatypes*. `bibtex` is the real-data **soundness
regression guard** (confirms dropping data axioms never causes FP, and loses
nothing HermiT derives); the synthetic suite is the **completeness test** for the
D-patterns themselves. Both are needed; neither subsumes the other.

## Reproduce

```sh
scripts/fetch-real-ontologies.sh   # fetch_bibtex extracts + converts
docker/robot/classify-oracle.sh ontologies/real/bibtex.ofn \
    ontologies/real/konclude-input/bibtex-classified.owx
cargo test -p owl-dl-reasoner --release --test konclude_closure_diff \
    bibtex_closure_matches_konclude -- --ignored --nocapture
```
