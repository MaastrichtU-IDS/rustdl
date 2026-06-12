# horned-owl Manchester Syntax `io/omn` (reader + writer) — design (2026-06-12)

## Goal

Add **OWL Manchester Syntax** support to **horned-owl** as a new `io/omn`
module (reader + writer), peer to its existing `io/ofn` (functional), `io/owx`
(OWL/XML), and `io/rdf`. The immediate driver: make rustdl's `rustdl justify`
output human-readable (`A SubClassOf B and (R some C)` instead of Rust `Debug`).
The broader goal: contribute the missing Manchester capability to the horned-owl
ecosystem (it has none today), upstreamed via PR.

**Reference grammar:** `omny` (the user's package, github.com/MaastrichtU-IDS/omny)
— a pure-Python Manchester parser/renderer for owlready2 whose grammar is
vendored from **owlapy** (the canonical Manchester grammar). omny + the W3C OWL 2
Manchester Syntax spec are the conformance anchors; this work brings the same
capability to the Rust/horned-owl world.

## Mechanism (build + upstream)

- Clone horned-owl 1.4.0 source to a sibling path `../horned-owl-omn/` (a git
  branch). Add to rustdl's workspace `Cargo.toml`:
  `[patch.crates-io] horned-owl = { path = "../horned-owl-omn" }`. rustdl builds
  against the patched fork; `justify` uses the new writer immediately.
- The fork branch is kept PR-shaped (mirrors horned-owl's tree + test layout).
  **The upstream PR to horned-owl is owned by the user** (push to their fork,
  open the PR). rustdl tracks the fork via `[patch]` until it merges, then drops
  the patch.

## How this mirrors horned-owl's existing io (validated against the source)

`io/ofn` is the direct template (Manchester, like functional, is a text syntax →
**pest** reader; owx/rdf diverge only because they are XML/triples):

| module | reader | writer | frames | LOC |
|---|---|---|---|---|
| ofn | pest (`ofn.pest`→`from_pair`) | `write(W,onto)` + `AsFunctional` trait | no | 2703 |
| owx | quick_xml | `write(W,onto)` | no | 3617 |
| rdf | rio/RdfFormatter | `write(W,onto)` + formatters | no | 5106 |
| **omn (this)** | **pest** (`omn.pest`→`from_pair`) | **`write(W,onto)` + `AsManchester` trait** | **yes** | ~3–4k (est.) |

Two conventions taken from the survey:
1. **All io's expose `pub fn write<A: ForIRI, AA: ForIndex<A>, W: Write>(writer, &ontology)`** — `io/omn` provides this (the frame-grouped whole-ontology writer). `AsManchester` (per-element) is the convenience layer under it (what `justify` calls).
2. **The reader mirrors `io/ofn/reader`** (`grammars/omn.pest` + `reader/{lexer,from_pair,mod}.rs`), NOT owx/rdf.

**Where Manchester genuinely diverges (no sibling template — designed from the W3C spec + omny; highest risk):**
- **Frame grouping** — ofn/owx/rdf are flat (axiom list / XML / triples); Manchester groups clauses under `Class:`/`ObjectProperty:`/`Individual:`/… frames. Novel in both writer (group axioms by subject) and reader (frame grammar).
- **Operator precedence** — functional is uniform prefix (`ObjectIntersectionOf(...)`); Manchester is infix `not` > `and` > `or` with restrictions `some/only/value/min/max/exactly/Self`. The grammar needs precedence handling (pest precedence-climbing) and the writer needs minimal-parenthesization.

## Module layout (in the horned-owl fork)

- `src/grammars/omn.pest` — the Manchester grammar.
- `src/io/omn/mod.rs` — module entry, public `read(...)` + re-exports (mirror `io/ofn/mod.rs`).
- `src/io/omn/writer/mod.rs` — `pub fn write<A,AA,W>(writer, &ontology)` (frame-grouped whole-ontology).
- `src/io/omn/writer/as_manchester.rs` — `AsManchester<A>` trait + `Manchester<'t,T,A>: Display` wrapper + `as_manchester_with_prefixes(&PrefixMapping)` (mirror `as_functional.rs`).
- `src/io/omn/reader/{lexer.rs, from_pair.rs, mod.rs}` — pest parse → `Component`/`ClassExpression` (mirror `io/ofn/reader`).
- `src/io/mod.rs` — register the `omn` module.

## The writer (`AsManchester`)

Trait `AsManchester<A: ForIRI>` → lazy `Manchester<'t, Self, A>: Display`, plus
`as_manchester_with_prefixes(&PrefixMapping)` for abbreviated IRIs (exactly the
`AsFunctional`/`Functional` shape). Implemented for `ClassExpression<A>`,
`Component<A>`, and leaf types (`Class`, `ObjectProperty`, `IRI`, `Literal`,
`Individual`, `DataRange`, facets).

**Class-expression rendering** (precedence `not` > `and` > `or`; parenthesize
only where a lower-precedence child sits under a higher-precedence parent):
- `Class`→IRI (prefix-abbreviated); `ObjectIntersectionOf`→`A and B`;
  `ObjectUnionOf`→`A or B`; `ObjectComplementOf`→`not A`;
  `ObjectSomeValuesFrom`→`R some C`; `ObjectAllValuesFrom`→`R only C`;
  `ObjectHasValue`→`R value a`; `ObjectMin/Max/ExactCardinality`→`R min/max/exactly n C`;
  `ObjectHasSelf`→`R Self`; `ObjectOneOf`→`{a, b}`.
- Data variants analogously (`R some xsd:integer[> 0]` for facet-restricted
  ranges; `DataOneOf`→`{1, 2}`).

**Axiom rendering — per-axiom primary** (what `justify` needs): `SubClassOf`→
`A SubClassOf B`; `EquivalentClasses`→`A EquivalentTo B`; `DisjointClasses`→
`A DisjointWith B`; `SubObjectPropertyOf`→`P SubPropertyOf Q`; property
characteristics→`P Characteristics: Functional` (or the standalone clause);
`ObjectPropertyDomain/Range`→`P Domain/Range C`; `ClassAssertion`→`a Type C`;
`ObjectPropertyAssertion`→`a R b`; `SameIndividual`/`DifferentIndividuals`→
`a SameAs/DifferentFrom b`; data-property + annotation analogues.

**Whole-ontology (`write`) — frame grouping (secondary):** group axioms by
subject entity into `Class:`/`ObjectProperty:`/`DataProperty:`/`Individual:`/
`Datatype:`/`AnnotationProperty:` frames with the appropriate clauses; emit
prefix declarations from the `PrefixMapping`. This is the standard io `write`
entry; `justify` uses the per-axiom `AsManchester` layer instead.

## The reader (`omn.pest` + `from_pair`)

`src/grammars/omn.pest` — the full Manchester grammar, ported from omny's
owlapy grammar + W3C Manchester EBNF:
- IRIs: full `<...>`, abbreviated `prefix:local`, prefix declarations
  (`Prefix:`), reusing horned-owl's `rfc3987.pest`/`bcp47.pest` where ofn does.
- Class-expression sub-grammar with precedence + all restrictions
  (`some/only/value/min/max/exactly/Self`), `{...}` enumerations, parenthesized
  groups.
- Data ranges + facets (`integer[>= 0, < 10]`, `{1,2}`, `not`, `and`, `or`).
- Literals (typed `"5"^^xsd:integer`, lang-tagged `"x"@en`, plain).
- Frame syntax: `Class:`/`ObjectProperty:`/`DataProperty:`/`Individual:`/
  `Datatype:`/`AnnotationProperty:` with clauses `SubClassOf:`/`EquivalentTo:`/
  `DisjointWith:`/`SubPropertyOf:`/`InverseOf:`/`Characteristics:`/`Domains:`/
  `Ranges:`/`Types:`/`Facts:`/`SameAs:`/`DifferentFrom:`/`Annotations:`.
- Ontology document: `Prefix:` decls + `Ontology:` header + frames.

`reader/from_pair.rs` maps pest pairs → horned-owl `Component`/`ClassExpression`
(inverse of the writer). `lexer.rs`/`mod.rs` mirror `io/ofn/reader` (the public
`read(BufRead, ParserConfiguration) -> (SetOntology, PrefixMapping)` entry).

## Phasing (the implementation plan — one project, sequenced)

- **P1 — writer + rustdl wiring.** `AsManchester` (per-element) + the per-axiom
  rendering of every `Component`/`ClassExpression` variant + the frame-grouped
  `write(W, onto)`. Wire `rustdl justify` to `as_manchester_with_prefixes`.
  Ships the readable-justify win. De-risks the model→Manchester mapping.
- **P2 — reader: class expressions.** `omn.pest` class-expression sub-grammar
  (with precedence) + `from_pair` for `ClassExpression`/literals/data ranges.
  Round-trip-tested against P1's writer at the expression level.
- **P3 — reader: frames + ontology document.** The frame grammar + `from_pair`
  for axioms + the full-document `read`. Round-trip whole ontologies.

Each phase is independently testable and shippable (P1 alone delivers justify).

## Testing / conformance

- **Round-trip (the strongest gate):** for a corpus of `Component`s/
  `ClassExpression`s, `read(x.as_manchester()) == x` — writer⊕reader
  consistency, applied per phase (P2: expressions; P3: full ontologies).
  Round-trip is **structural equality**, not byte-identity (frame order,
  parenthesization, prefix abbreviation are renderer choices).
- **Conformance:** render fixed expressions/axioms → expected Manchester strings
  (from omny output + Protégé + W3C Manchester spec examples); parse the W3C
  Manchester spec's worked examples → expected `Component`s. A small curated
  table of (model ↔ Manchester) pairs is the conformance suite.
- **Real ontologies:** parse OFN → render Manchester → re-parse → structurally
  equal, over rustdl's corpus fixtures (galen/sio/wine/pizza). Cross-check a
  handful against omny's rendering of the same ontology where feasible.
- **horned-owl test layout:** mirror its `io` test conventions (e.g.
  `src/io/omn/` `#[cfg(test)]` + any `tests/` fixtures) so the PR is review-ready.
- **rustdl side:** `rustdl justify` output canaries (the existing justification
  tests assert axiom *content*; add a rendering canary that the Manchester
  string of a known axiom matches the expected human-readable form).

## rustdl integration

Once P1 lands (fork `[patch]`ed in), `crates/owl-dl-cli/src/main.rs`'s `justify`
handler renders each justification axiom via
`ax.as_manchester_with_prefixes(&prefixes)` instead of `{ax:?}` (the prefixes
come from the parsed ontology's `PrefixMapping`). The `Justification` API is
unchanged; only the CLI rendering improves.

## Scope / non-goals

- **In:** the full `io/omn` reader + writer for horned-owl (class expressions,
  data ranges/facets, all axiom types horned-owl models, frames, ontology
  document, prefixes, annotations); the uniform `write(W,onto)` + `AsManchester`;
  rustdl justify wiring; round-trip + conformance tests; a PR-shaped fork branch.
- **Out:** Manchester features horned-owl's model can't represent (none expected
  for OWL 2 DL); a streaming/incremental reader; perf tuning beyond what ofn
  does; the actual upstream PR *merge* (user-owned). Rendering *byte-identical*
  to Protégé (structural equality is the bar).
- **Effort:** ~3–4k LOC (between ofn 2.7k and owx 3.6k); multi-week. `ofn` is the
  direct structural guide; frames + precedence are the from-spec, highest-risk
  parts.

## Soundness/correctness invariants

- **Round-trip fidelity:** `read ∘ write = id` (structurally) on every supported
  construct — the writer must not emit something the reader can't parse back to
  the same model. This is the parser↔writer contract and the primary gate.
- **No silent loss:** every `Component`/`ClassExpression` variant horned-owl
  supports must render (writer) and parse (reader); an unsupported construct is
  an explicit error, never silent omission (which would corrupt round-trip).
- **Conformance over convenience:** follow the W3C Manchester grammar (via omny/
  owlapy) for both directions; where the spec is ambiguous, match Protégé/omny
  observable behavior and document the choice.
