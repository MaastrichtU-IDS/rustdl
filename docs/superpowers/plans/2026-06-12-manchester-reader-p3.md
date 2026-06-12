# Manchester Reader P3 — Frames + Ontology Document Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a whole-ontology OWL Manchester Syntax reader to the horned-owl fork — frame parsing for all six entity types plus the document (`Prefix:`/`Ontology:`/frames) — so `read(write(ont))` round-trips structurally, completing the reader half of `io/omn` for the upstream PR.

**Architecture:** Extend `src/grammars/omn.pest` (already carrying the P2 class-expression sub-grammar) with a non-silent `ManchesterDocument` rule covering prefix declarations, an optional ontology header, and six frame kinds with their clause rules. Extend `src/io/omn/reader/from_pair.rs` with: a `PrefixMapping` parse, a frame dispatcher (`insert_frame`) delegating to one sub-function per frame kind, and a two-pass document assembler in `read_with_build` (prefixes first, then frames under a prefix-aware `Context`). Reuse every P2 leaf (`IRI`, `Class`, `ObjectPropertyExpression`, `ClassExpression`, `DataRange`, `Literal`, `Individual`). All clause keywords mirror the writer's output exactly (`SubClassOf:`, `EquivalentTo:`, `Characteristics:`, `Facts:`, …).

**Tech Stack:** Rust (edition 2024), pest / pest_derive, horned-owl object model (`Component`, `SetOntology`, `MutableOntology`, `curie::PrefixMapping`).

**Working directory:** `/data/dumontier/horned-owl-omn` (branch `master`). Toolchain: prepend `PATH` with `/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`.

**Round-trip invariant (the gate):** structural equality of the `Component` set, NOT byte-identity (frame order and grouping may differ). The corpus declares every entity it heads a frame with (Manchester frames conflate declaration and reference — see the limitations note in Task 6). Declarations are non-logical, so emitting one per frame header never affects soundness (FP=0).

---

## Background the implementer needs

The P1 **writer** (`src/io/omn/writer/mod.rs`) emits a document shaped exactly like this (prefix lines, optional `Ontology:` header, then 4-space-indented frames; a trailing `# General axioms` block holds non-Manchester functional-syntax fallbacks — **out of P3 scope**):

```
Prefix: ex: <http://ex/>

Ontology: <http://ex/onto>

Class: ex:A
    SubClassOf: ex:B
    EquivalentTo: ex:C, ex:D
    DisjointWith: ex:E
    DisjointUnionOf: ex:F, ex:G

ObjectProperty: ex:r
    SubPropertyOf: ex:s
    Domain: ex:A
    Range: ex:B
    Characteristics: Functional
    InverseOf: ex:t

DataProperty: ex:p
    Domain: ex:A
    Range: xsd:integer
    Characteristics: Functional

AnnotationProperty: ex:note
    Domain: ex:A
    Range: ex:B

Individual: ex:a
    Types: ex:A
    Facts: ex:r ex:b
    Facts: ex:p "5"^^xsd:integer
    SameAs: ex:c
    DifferentFrom: ex:d

Datatype: ex:dt
```

The corresponding `Component` constructors (verified against the writer's match arms) are:

| Clause | Component |
|---|---|
| `Class:` header | `DeclareClass(Class(iri))` |
| `SubClassOf: D` | `SubClassOf { sub: ClassExpression::Class(subj), sup: D }` (one per list item) |
| `EquivalentTo: L…` | `EquivalentClasses(vec![Class-CE(subj), L…])` (single n-ary axiom) |
| `DisjointWith: L…` | `DisjointClasses(vec![Class-CE(subj), L…])` |
| `DisjointUnionOf: L…` | `DisjointUnion(Class(subj), vec![L…])` |
| `ObjectProperty:` header | `DeclareObjectProperty(ObjectProperty(iri))` |
| `SubPropertyOf: S` | `SubObjectPropertyOf { sub: SubObjectPropertyExpression::ObjectPropertyExpression(OPE::ObjectProperty(subj)), sup: S }` |
| `EquivalentTo: L…` (op frame) | `EquivalentObjectProperties(vec![OPE(subj), L…])` |
| `DisjointWith: L…` (op frame) | `DisjointObjectProperties(vec![OPE(subj), L…])` |
| `InverseOf: P` | `InverseObjectProperties(ObjectProperty(subj), P-as-ObjectProperty)` |
| `Domain: C` (op frame) | `ObjectPropertyDomain { ope: OPE(subj), ce: C }` |
| `Range: C` (op frame) | `ObjectPropertyRange { ope: OPE(subj), ce: C }` |
| `Characteristics: K` (op frame) | `{Functional,InverseFunctional,Reflexive,Irreflexive,Symmetric,Asymmetric,Transitive}ObjectProperty(OPE(subj))` |
| `DataProperty:` header | `DeclareDataProperty(DataProperty(iri))` |
| `SubPropertyOf: S` (dp frame) | `SubDataPropertyOf { sub: DataProperty(subj), sup: S-as-DataProperty }` |
| `EquivalentTo: L…` (dp frame) | `EquivalentDataProperties(vec![DataProperty(subj), L…])` |
| `DisjointWith: L…` (dp frame) | `DisjointDataProperties(vec![DataProperty(subj), L…])` |
| `Domain: C` (dp frame) | `DataPropertyDomain { dp: DataProperty(subj), ce: C }` |
| `Range: R` (dp frame) | `DataPropertyRange { dp: DataProperty(subj), dr: R }` (R is a `DataRange`) |
| `Characteristics: Functional` (dp frame) | `FunctionalDataProperty(DataProperty(subj))` |
| `AnnotationProperty:` header | `DeclareAnnotationProperty(AnnotationProperty(iri))` |
| `SubPropertyOf: S` (ap frame) | `SubAnnotationPropertyOf { sub: AnnotationProperty(subj), sup: AnnotationProperty(S-iri) }` |
| `Domain: I` (ap frame) | `AnnotationPropertyDomain { ap: AnnotationProperty(subj), iri: I-iri }` |
| `Range: I` (ap frame) | `AnnotationPropertyRange { ap: AnnotationProperty(subj), iri: I-iri }` |
| `Individual:` header | `DeclareNamedIndividual(NamedIndividual(iri))` |
| `Types: C` | `ClassAssertion { i: Individual::Named(NamedIndividual(subj)), ce: C }` |
| `Facts: P o` (o = individual) | `ObjectPropertyAssertion { ope: P-as-OPE, from: Named(subj), to: o }` |
| `Facts: not P o` | `NegativeObjectPropertyAssertion { ope, from, to }` |
| `Facts: P lit` (lit = literal) | `DataPropertyAssertion { dp: DataProperty(P-iri), from: Named(subj), to: lit }` |
| `Facts: not P lit` | `NegativeDataPropertyAssertion { dp, from, to }` |
| `SameAs: L…` | `SameIndividual(vec![Named(subj), L…])` |
| `DifferentFrom: L…` | `DifferentIndividuals(vec![Named(subj), L…])` |
| `Datatype:` header | `DeclareDatatype(Datatype(iri))` |

**n-ary collapse reading (deliberate):** the writer turns `EquivalentClasses([A,B,C])` into `A EquivalentTo: B, C`. The reader inverts this by prepending the frame subject to the list and emitting ONE n-ary axiom — `EquivalentClasses([A,B,C])`. This is the exact inverse of the writer (round-trip clean) and semantically sound (these axioms are symmetric). It diverges from OWL-API's pairwise expansion; that is documented in Task 6. The same prepend-and-collapse applies to `DisjointWith`/`SameAs`/`DifferentFrom`/`EquivalentObjectProperties`/`DisjointObjectProperties`/`EquivalentDataProperties`/`DisjointDataProperties`. By contrast `SubClassOf`/`SubPropertyOf`/`Domain`/`Range`/`Characteristics`/`Types`/`Facts` emit ONE axiom PER list item.

**Object vs. data `Facts:` disambiguation:** the trailing token is a `Literal` (starts with `"`) → data assertion; otherwise an IRI individual → object assertion.

**Bare-name limitation (kept from P2):** the grammar's `IRI` accepts `<full>` and `prefix:local` but NOT a bare local name (`A`). The writer only emits bare names when a *default* (`""`) prefix is registered. The round-trip corpus therefore registers a **non-default** prefix (`ex:`), so the writer emits `ex:A` (lexable) and never a bare name. Default-prefix bare names remain a documented limitation (Task 6).

---

## File Structure

- **Modify** `src/grammars/omn.pest` — add document, prefix, ontology-header, and six frame rules + clause/list rules below the existing class-expression grammar. One cohesive grammar file (Task 1).
- **Modify** `src/io/omn/reader/from_pair.rs` — add `PrefixMapping` parse, `insert_frame` dispatcher + six `insert_*_frame` sub-functions, plus small leaf reuse. Largest changes (Tasks 2–5).
- **Modify** `src/io/omn/reader/mod.rs` — add `read` / `read_with_build` public functions and the two-pass document assembler (Task 2).
- **Test** `src/io/omn/reader/from_pair.rs` `#[cfg(test)] mod tests` — frame round-trip unit tests live beside the impls (Tasks 2–5); the whole-ontology capstone round-trip lives here too (Task 6).

Run targets used throughout:
- `cargo test --lib io::omn` — omn unit + integration tests.
- `cargo build -p horned-owl` — fork builds.
- `cargo fmt -- --check` (rc 0; ignore benign `array_width` config note) and `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

---

## Task 1: Grammar — document, prefixes, header, frames

**Files:**
- Modify: `src/grammars/omn.pest` (append after line 103, the end of the data-range rules)

- [ ] **Step 1: Write the failing lexer tests**

Add to the `#[cfg(test)] mod tests` in `src/io/omn/reader/lexer.rs` (after the existing `lex_class_expressions` test):

```rust
fn lex_doc(s: &str) -> bool {
    ManchesterLexer::lex(Rule::ManchesterDocument, s).is_ok()
}

#[test]
fn lex_documents() {
    assert!(lex_doc("Prefix: ex: <http://ex/>"));
    assert!(lex_doc("Prefix: : <http://ex/>")); // default prefix decl
    assert!(lex_doc("Ontology: <http://ex/o>"));
    assert!(lex_doc("Prefix: ex: <http://ex/>\nOntology: <http://ex/o>"));
    assert!(lex_doc("")); // empty document is valid
    assert!(lex_doc("Class: <http://ex/A>"));
    assert!(lex_doc("Class: <http://ex/A>\n    SubClassOf: <http://ex/B>"));
    assert!(lex_doc("Class: <http://ex/A>\n    EquivalentTo: <http://ex/B>, <http://ex/C>"));
    assert!(lex_doc(
        "ObjectProperty: <http://ex/r>\n    Characteristics: Functional\n    InverseOf: <http://ex/t>"
    ));
    assert!(lex_doc("DataProperty: <http://ex/p>\n    Range: <http://ex/dt>"));
    assert!(lex_doc("AnnotationProperty: <http://ex/n>\n    Domain: <http://ex/A>"));
    assert!(lex_doc(
        "Individual: <http://ex/a>\n    Types: <http://ex/A>\n    Facts: <http://ex/r> <http://ex/b>"
    ));
    assert!(lex_doc("Datatype: <http://ex/dt>"));
    // two frames in sequence
    assert!(lex_doc("Class: <http://ex/A>\nClass: <http://ex/B>"));
    // garbage must not lex
    assert!(!lex_doc("Class:"));
    assert!(!lex_doc("Frobnicate: <http://ex/A>"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib io::omn::reader::lexer 2>&1 | tail -15`
Expected: FAIL — `Rule::ManchesterDocument` does not exist (compile error).

- [ ] **Step 3: Add the grammar rules**

Append to `src/grammars/omn.pest`:

```pest
// ---- whole-ontology document ------------------------------------------------
// Non-silent wrapper so from_pair gets one ManchesterDocument node whose inner
// is [PrefixDeclaration*, OntologyHeader?, Frame*].

ManchesterDocument = { SOI ~ PrefixDeclaration* ~ OntologyHeader? ~ Frame* ~ EOI }

PrefixDeclaration = { ^"Prefix:" ~ PrefixName ~ FullIRI }
PrefixName        = { SPARQL_PnameNs }          // matches "ex:" and ":"
OntologyHeader    = { ^"Ontology:" ~ IRI? }

// ---- frames -----------------------------------------------------------------

Frame = {
      ClassFrame
    | ObjectPropertyFrame
    | DataPropertyFrame
    | AnnotationPropertyFrame
    | IndividualFrame
    | DatatypeFrame
}

FrameSubject = { IRI }

ClassFrame = { ^"Class:" ~ FrameSubject ~ ClassClause* }
ClassClause = {
      ^"SubClassOf:"      ~ DescriptionList
    | ^"EquivalentTo:"    ~ DescriptionList
    | ^"DisjointWith:"    ~ DescriptionList
    | ^"DisjointUnionOf:" ~ DescriptionList
}

ObjectPropertyFrame = { ^"ObjectProperty:" ~ FrameSubject ~ ObjectPropertyClause* }
ObjectPropertyClause = {
      ^"SubPropertyOf:"  ~ OpeList
    | ^"EquivalentTo:"   ~ OpeList
    | ^"DisjointWith:"   ~ OpeList
    | ^"InverseOf:"      ~ OpeList
    | ^"Domain:"         ~ DescriptionList
    | ^"Range:"          ~ DescriptionList
    | ^"Characteristics:" ~ CharacteristicList
}

DataPropertyFrame = { ^"DataProperty:" ~ FrameSubject ~ DataPropertyClause* }
DataPropertyClause = {
      ^"SubPropertyOf:"   ~ IriList
    | ^"EquivalentTo:"    ~ IriList
    | ^"DisjointWith:"    ~ IriList
    | ^"Domain:"          ~ DescriptionList
    | ^"Range:"           ~ DataRangeList
    | ^"Characteristics:" ~ CharacteristicList
}

AnnotationPropertyFrame = { ^"AnnotationProperty:" ~ FrameSubject ~ AnnotationPropertyClause* }
AnnotationPropertyClause = {
      ^"SubPropertyOf:" ~ IriList
    | ^"Domain:"        ~ IriList
    | ^"Range:"         ~ IriList
}

IndividualFrame = { ^"Individual:" ~ FrameSubject ~ IndividualClause* }
IndividualClause = {
      ^"Types:"         ~ DescriptionList
    | ^"Facts:"         ~ FactList
    | ^"SameAs:"        ~ IndividualList
    | ^"DifferentFrom:" ~ IndividualList
}

DatatypeFrame = { ^"Datatype:" ~ FrameSubject }

// ---- shared comma-separated lists -------------------------------------------

DescriptionList  = { Description ~ ( "," ~ Description )* }
OpeList          = { ope ~ ( "," ~ ope )* }
IriList          = { IRI ~ ( "," ~ IRI )* }
DataRangeList    = { DataRange ~ ( "," ~ DataRange )* }
IndividualList   = { Individual ~ ( "," ~ Individual )* }
CharacteristicList = { Characteristic ~ ( "," ~ Characteristic )* }
Characteristic = {
      ^"Functional" | ^"InverseFunctional" | ^"Reflexive" | ^"Irreflexive"
    | ^"Symmetric" | ^"Asymmetric" | ^"Transitive"
}

FactList = { Fact ~ ( "," ~ Fact )* }
Fact     = { ^"not"? ~ ope ~ ( Literal | Individual ) }
```

Note: `Characteristic` lists `InverseFunctional` before `Functional`? No — pest ordered choice would match `Functional` as a prefix of nothing here, but `InverseFunctional` and `Functional` share no prefix, so order is irrelevant. Keep as written.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib io::omn::reader::lexer 2>&1 | tail -15`
Expected: PASS — `lex_documents` and the existing `lex_class_expressions` both green.

- [ ] **Step 5: Verify formatting/lint and commit**

Run: `cargo fmt -- --check` (rc 0) and `cargo build -p horned-owl 2>&1 | tail -2` (builds).

```bash
git add src/grammars/omn.pest src/io/omn/reader/lexer.rs
git commit -m "feat(omn/reader): Manchester document + frame grammar

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: PrefixMapping + read() + frame dispatcher (declarations only)

This task delivers a working end-to-end `read` for a declarations-only document (all six frame headers), exercising prefix parsing, the ontology header, and the `OntologyID`. Clause bodies are parsed by the grammar but ignored by the dispatcher (filled in Tasks 3–5).

**Files:**
- Modify: `src/io/omn/reader/from_pair.rs` (add `PrefixMapping` impl + `insert_frame` + six declaration-only sub-fns)
- Modify: `src/io/omn/reader/mod.rs` (add `read` / `read_with_build`)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` at the bottom of `src/io/omn/reader/from_pair.rs` (create the test module if absent; reuse the existing one if present):

```rust
#[test]
fn reads_declarations_round_trip() {
    use crate::io::omn::write;
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();

    let mut o = SetOntology::new_rc();
    o.insert(DeclareClass(b.class("http://ex/A")));
    o.insert(DeclareObjectProperty(b.object_property("http://ex/r")));
    o.insert(DeclareDataProperty(b.data_property("http://ex/p")));
    o.insert(DeclareAnnotationProperty(b.annotation_property("http://ex/n")));
    o.insert(DeclareNamedIndividual(b.named_individual("http://ex/a")));
    o.insert(DeclareDatatype(b.datatype("http://ex/dt")));

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();

    let (parsed, _pm): (SetOntology<_>, PrefixMapping) = crate::io::omn::reader::read_with_build(
        BufReader::new(&buf[..]),
        &b,
    )
    .unwrap();

    let orig: std::collections::BTreeSet<_> =
        o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> =
        parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "declarations did not round-trip");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -15`
Expected: FAIL — `read_with_build` does not exist (compile error).

- [ ] **Step 3: Add the `PrefixMapping` parse and `insert_frame` dispatcher**

In `src/io/omn/reader/from_pair.rs`, after the existing `ClassExpression` impl, add:

```rust
// ---------------------------------------------------------------------------
// Whole-ontology document support.
// ---------------------------------------------------------------------------

/// Build a `PrefixMapping` from a slice of `PrefixDeclaration` pairs.
///
/// `PrefixDeclaration = { ^"Prefix:" ~ PrefixName ~ FullIRI }`
/// `PrefixName        = { SPARQL_PnameNs }`  (e.g. `ex:` or bare `:`)
pub(crate) fn prefixes_from_decls<'a, A: ForIRI>(
    decls: impl Iterator<Item = Pair<'a, Rule>>,
) -> Result<PrefixMapping> {
    let mut prefixes = PrefixMapping::default();
    for decl in decls {
        let mut inner = decl.into_inner();
        let pname = inner.next().unwrap(); // PrefixName
        let full_iri = inner.next().unwrap(); // FullIRI
        // FullIRI = ${ "<" ~ RFC3987_Iri ~ ">" } — its inner is the bare IRI text.
        let iri_text = full_iri.into_inner().next().unwrap().as_str();
        // PrefixName = { SPARQL_PnameNs }; SPARQL_PnameNs = ${ SPARQL_PnPrefix? ~ ":" }
        let prefix_part = pname.into_inner().next().unwrap().into_inner().next();
        match prefix_part {
            Some(p) => prefixes
                .add_prefix(p.as_str(), iri_text)
                .expect("grammar guarantees a valid prefix"),
            None => prefixes
                .add_prefix("", iri_text)
                .expect("empty prefix shouldn't fail"),
        }
    }
    Ok(prefixes)
}

/// Dispatch a single `Frame` pair to the matching sub-function, inserting the
/// resulting components into `ont`.
pub(crate) fn insert_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>,
    ctx: &Context<'_, A>,
    ont: &mut O,
) -> Result<()> {
    let inner = frame.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::ClassFrame => insert_class_frame(inner, ctx, ont),
        Rule::ObjectPropertyFrame => insert_object_property_frame(inner, ctx, ont),
        Rule::DataPropertyFrame => insert_data_property_frame(inner, ctx, ont),
        Rule::AnnotationPropertyFrame => insert_annotation_property_frame(inner, ctx, ont),
        Rule::IndividualFrame => insert_individual_frame(inner, ctx, ont),
        Rule::DatatypeFrame => insert_datatype_frame(inner, ctx, ont),
        rule => unreachable!("unexpected frame rule: {:?}", rule),
    }
}

/// Parse a frame's `FrameSubject` (the first inner pair) into an `IRI`,
/// returning it plus the remaining clause pairs.
fn frame_subject_and_clauses<'a, A: ForIRI>(
    frame: Pair<'a, Rule>,
    ctx: &Context<'_, A>,
) -> Result<(IRI<A>, pest::iterators::Pairs<'a, Rule>)> {
    let mut inner = frame.into_inner();
    let subject_pair = inner.next().unwrap(); // FrameSubject
    let iri = IRI::from_pair(subject_pair.into_inner().next().unwrap(), ctx)?;
    Ok((iri, inner))
}
```

- [ ] **Step 4: Add the six declaration-only sub-functions**

Still in `from_pair.rs`, add (these are the Task-2 versions: header → Declare, clauses ignored — Tasks 3–5 fill the clause loops in the relevant ones):

```rust
fn insert_class_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, _clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareClass(Class(subject)));
    // clauses handled in Task 3
    Ok(())
}

fn insert_object_property_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, _clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareObjectProperty(ObjectProperty(subject)));
    // clauses handled in Task 4
    Ok(())
}

fn insert_data_property_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, _clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareDataProperty(DataProperty(subject)));
    // clauses handled in Task 4
    Ok(())
}

fn insert_annotation_property_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, _clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareAnnotationProperty(AnnotationProperty(subject)));
    // clauses handled in Task 5
    Ok(())
}

fn insert_individual_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, _clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareNamedIndividual(NamedIndividual(subject)));
    // clauses handled in Task 5
    Ok(())
}

fn insert_datatype_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, _clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareDatatype(Datatype(subject)));
    // Datatype frames carry no clauses.
    Ok(())
}
```

Add `use crate::model::MutableOntology;` to the imports at the top if `crate::model::*` does not already cover it (it does via the glob — verify with the build; if a `MutableOntology` not-in-scope error appears, add the explicit `use`).

- [ ] **Step 5: Add `read` / `read_with_build` + the two-pass assembler**

Replace `src/io/omn/reader/mod.rs` with (keeps `parse_class_expression`, adds the document API):

```rust
pub mod from_pair;
pub mod lexer;

pub use from_pair::{Context, FromPair};
pub use lexer::{ManchesterLexer, Rule};

use std::io::BufRead;

use curie::PrefixMapping;

use crate::error::HornedError;
use crate::io::ParserConfiguration;
use crate::model::{Build, ClassExpression, ForIRI, MutableOntology, Ontology};

/// Parse a Manchester Syntax class expression from a string.
///
/// `pm` provides prefix expansions for abbreviated IRIs (`prefix:local`);
/// `build` is the IRI intern arena.
pub fn parse_class_expression<A: ForIRI>(
    s: &str,
    pm: &curie::PrefixMapping,
    build: &Build<A>,
) -> Result<ClassExpression<A>, HornedError> {
    let description = ManchesterLexer::lex(Rule::ClassExpressionDocument, s)?
        .next()
        .ok_or_else(|| HornedError::invalid("empty class expression"))?;
    let ctx = Context::new(build, pm);
    ClassExpression::from_pair(description, &ctx)
}

/// Read a whole ontology from a Manchester Syntax document, using a fresh IRI
/// `Build`.  Mirrors `io::ofn::reader::read`.
///
/// The `# General axioms` block emitted by the writer for components lacking a
/// native Manchester form (in OWL functional syntax) is NOT parsed — see the
/// P3 limitations note.
pub fn read<A: ForIRI, O: MutableOntology<A> + Ontology<A> + Default, R: BufRead>(
    bufread: R,
    _config: ParserConfiguration,
) -> Result<(O, PrefixMapping), HornedError> {
    let b = Build::new();
    read_with_build(bufread, &b)
}

/// Read a whole ontology, interning IRIs into the supplied `build`.
pub fn read_with_build<A: ForIRI, O: MutableOntology<A> + Ontology<A> + Default, R: BufRead>(
    mut bufread: R,
    build: &Build<A>,
) -> Result<(O, PrefixMapping), HornedError> {
    let mut doc = String::new();
    bufread.read_to_string(&mut doc)?;

    let document = ManchesterLexer::lex(Rule::ManchesterDocument, doc.trim())?
        .next()
        .ok_or_else(|| HornedError::invalid("empty Manchester document"))?;

    // Collect the document's children so we can make two passes.
    let children: Vec<_> = document.into_inner().collect();

    // Pass 1: build the prefix mapping from PrefixDeclaration children.
    let prefixes = from_pair::prefixes_from_decls::<A>(
        children
            .iter()
            .filter(|p| p.as_rule() == Rule::PrefixDeclaration)
            .cloned(),
    )?;

    // Pass 2: build the ontology under a prefix-aware context.
    let ctx = Context::new(build, &prefixes);
    let mut ontology: O = Default::default();
    let mut ontology_id = crate::model::OntologyID::default();
    let mut header_present = false;

    for child in children {
        match child.as_rule() {
            Rule::PrefixDeclaration | Rule::EOI => {}
            Rule::OntologyHeader => {
                header_present = true;
                if let Some(iri_pair) = child.into_inner().next() {
                    ontology_id.iri = Some(crate::model::IRI::from_pair(iri_pair, &ctx)?);
                }
            }
            Rule::Frame => from_pair::insert_frame(child, &ctx, &mut ontology)?,
            rule => unreachable!("unexpected document child: {:?}", rule),
        }
    }
    // Only insert an OntologyID when the document actually carried an
    // `Ontology:` header. A fresh `SetOntology` does NOT seed one, and the
    // writer omits the header for a default (empty) OntologyID — so inserting
    // `OntologyID::default()` unconditionally (as ofn does) would add a
    // spurious `Component::OntologyID(None, None)` and break the round-trip
    // against a hand-built ontology that never declared an ID.
    if header_present {
        ontology.insert(ontology_id);
    }

    Ok((ontology, prefixes))
}
```

**CRITICAL (verified during planning):** the `OntologyID` insert is GATED on `header_present`. ofn's `MutableOntologyWrapper` inserts it unconditionally, but ofn's writer always emits `Ontology()` even when empty; the omn writer only emits `Ontology: <iri>` when `iri.is_some()`. Inserting a default OntologyID here would make `parsed` carry a `Component::OntologyID(None, None)` that the Task 2–5 test ontologies (which never insert an OntologyID) lack, failing every `assert_eq!(orig, got)` on that one extra element. Do NOT remove the gate.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -15`
Expected: PASS — `reads_declarations_round_trip` green.

If `b.data_property` / `b.annotation_property` / `b.datatype` / `b.named_individual` method names differ, check `src/model.rs` `impl Build` for the exact constructors and adjust the test.

- [ ] **Step 7: Verify formatting/lint and commit**

Run: `cargo fmt -- --check` (rc 0) and `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

```bash
git add src/io/omn/reader/from_pair.rs src/io/omn/reader/mod.rs
git commit -m "feat(omn/reader): document read + prefix mapping + frame declarations

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Class frame clauses

**Files:**
- Modify: `src/io/omn/reader/from_pair.rs` (`insert_class_frame` + a shared `parse_description_list` helper + test)

- [ ] **Step 1: Write the failing test**

Add to the test module in `from_pair.rs`:

```rust
#[test]
fn reads_class_frame_round_trip() {
    use crate::io::omn::write;
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();

    let a = || ClassExpression::Class(b.class("http://ex/A"));
    let mut o = SetOntology::new_rc();
    for c in ["A", "B", "C", "D", "E", "F", "G"] {
        o.insert(DeclareClass(b.class(&format!("http://ex/{c}"))));
    }
    o.insert(SubClassOf { sub: a(), sup: ClassExpression::Class(b.class("http://ex/B")) });
    o.insert(EquivalentClasses(vec![
        a(),
        ClassExpression::Class(b.class("http://ex/C")),
    ]));
    o.insert(DisjointClasses(vec![
        a(),
        ClassExpression::Class(b.class("http://ex/D")),
    ]));
    // DisjointUnion exercises the disjointunionof clause arm.
    o.insert(DisjointUnion(
        b.class("http://ex/A"),
        vec![
            ClassExpression::Class(b.class("http://ex/F")),
            ClassExpression::Class(b.class("http://ex/G")),
        ],
    ));

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();

    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        crate::io::omn::reader::read_with_build(BufReader::new(&buf[..]), &b).unwrap();

    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "class frame did not round-trip");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib io::omn::reader::from_pair::tests::reads_class_frame_round_trip 2>&1 | tail -20`
Expected: FAIL — parsed set is missing `SubClassOf`/`EquivalentClasses`/`DisjointClasses` (clauses ignored in Task 2).

- [ ] **Step 3: Add the description-list helper**

In `from_pair.rs`, near `insert_frame`, add:

```rust
/// Parse a `DescriptionList` pair into a `Vec<ClassExpression>`.
fn parse_description_list<A: ForIRI>(
    list: Pair<Rule>,
    ctx: &Context<'_, A>,
) -> Result<Vec<ClassExpression<A>>> {
    list.into_inner()
        .map(|d| ClassExpression::from_pair(d, ctx))
        .collect()
}
```

- [ ] **Step 4: Fill in `insert_class_frame`**

Replace the Task-2 `insert_class_frame` body with:

```rust
fn insert_class_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, clauses) = frame_subject_and_clauses(frame, ctx)?;
    let subject_ce = ClassExpression::Class(Class(subject.clone()));
    ont.insert(DeclareClass(Class(subject.clone())));

    for clause in clauses {
        // each clause: keyword token (silent) + a DescriptionList child
        let kw = clause_keyword(&clause);
        let list = clause.into_inner().next().unwrap(); // DescriptionList
        let items = parse_description_list(list, ctx)?;
        match kw.as_str() {
            "subclassof" => {
                for sup in items {
                    ont.insert(SubClassOf { sub: subject_ce.clone(), sup });
                }
            }
            "equivalentto" => {
                let mut all = vec![subject_ce.clone()];
                all.extend(items);
                ont.insert(EquivalentClasses(all));
            }
            "disjointwith" => {
                let mut all = vec![subject_ce.clone()];
                all.extend(items);
                ont.insert(DisjointClasses(all));
            }
            "disjointunionof" => {
                ont.insert(DisjointUnion(Class(subject.clone()), items));
            }
            other => unreachable!("unexpected class clause keyword: {other}"),
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Add the `clause_keyword` helper**

A clause pair's text begins with its keyword (`SubClassOf:` etc.). pest does not emit the bare keyword as a child, so read it off the span. Add near the helpers:

```rust
/// Extract the lower-cased clause keyword (without the trailing colon) from a
/// clause pair, e.g. `"SubClassOf: ..."` -> `"subclassof"`.
fn clause_keyword(clause: &Pair<Rule>) -> String {
    clause
        .as_str()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .flat_map(|c| c.to_lowercase())
        .collect()
}
```

(`take_while(is_ascii_alphabetic)` stops at the `:`, matching the robust keyword scan adopted in the P2 reader review.)

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -15`
Expected: PASS — `reads_class_frame_round_trip` and the Task-2 declaration test both green.

- [ ] **Step 7: Verify and commit**

Run: `cargo fmt -- --check` (rc 0) and `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

```bash
git add src/io/omn/reader/from_pair.rs
git commit -m "feat(omn/reader): class frame clauses (SubClassOf/EquivalentTo/DisjointWith/DisjointUnionOf)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: ObjectProperty + DataProperty frame clauses

**Files:**
- Modify: `src/io/omn/reader/from_pair.rs` (`insert_object_property_frame`, `insert_data_property_frame`, list helpers, tests)

- [ ] **Step 1: Write the failing tests**

Add to the test module:

```rust
#[test]
fn reads_object_property_frame_round_trip() {
    use crate::io::omn::write;
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    let ope = |i: &str| ObjectPropertyExpression::ObjectProperty(b.object_property(i));

    let mut o = SetOntology::new_rc();
    o.insert(DeclareObjectProperty(b.object_property("http://ex/r")));
    o.insert(DeclareObjectProperty(b.object_property("http://ex/s")));
    o.insert(DeclareObjectProperty(b.object_property("http://ex/t")));
    o.insert(DeclareClass(b.class("http://ex/A")));
    o.insert(DeclareClass(b.class("http://ex/B")));
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyExpression(ope("http://ex/r")),
        sup: ope("http://ex/s"),
    });
    o.insert(EquivalentObjectProperties(vec![ope("http://ex/r"), ope("http://ex/s")]));
    o.insert(DisjointObjectProperties(vec![ope("http://ex/r"), ope("http://ex/t")]));
    o.insert(ObjectPropertyDomain { ope: ope("http://ex/r"), ce: ClassExpression::Class(b.class("http://ex/A")) });
    o.insert(ObjectPropertyRange { ope: ope("http://ex/r"), ce: ClassExpression::Class(b.class("http://ex/B")) });
    // every characteristic arm (round-trip only — semantic consistency irrelevant)
    o.insert(FunctionalObjectProperty(ope("http://ex/r")));
    o.insert(InverseFunctionalObjectProperty(ope("http://ex/r")));
    o.insert(ReflexiveObjectProperty(ope("http://ex/r")));
    o.insert(IrreflexiveObjectProperty(ope("http://ex/r")));
    o.insert(SymmetricObjectProperty(ope("http://ex/r")));
    o.insert(AsymmetricObjectProperty(ope("http://ex/r")));
    o.insert(TransitiveObjectProperty(ope("http://ex/r")));
    o.insert(InverseObjectProperties(
        b.object_property("http://ex/r"),
        b.object_property("http://ex/t"),
    ));

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        crate::io::omn::reader::read_with_build(BufReader::new(&buf[..]), &b).unwrap();

    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "object property frame did not round-trip");
}

#[test]
fn reads_data_property_frame_round_trip() {
    use crate::io::omn::write;
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    pm.add_prefix("xsd", "http://www.w3.org/2001/XMLSchema#").unwrap();

    let dp = |i: &str| b.data_property(i);
    let mut o = SetOntology::new_rc();
    o.insert(DeclareDataProperty(dp("http://ex/p")));
    o.insert(DeclareDataProperty(dp("http://ex/q")));
    o.insert(DeclareDataProperty(dp("http://ex/u")));
    o.insert(DeclareDataProperty(dp("http://ex/v")));
    o.insert(DeclareClass(b.class("http://ex/A")));
    o.insert(SubDataPropertyOf { sub: dp("http://ex/p"), sup: dp("http://ex/q") });
    o.insert(EquivalentDataProperties(vec![dp("http://ex/p"), dp("http://ex/u")]));
    o.insert(DisjointDataProperties(vec![dp("http://ex/p"), dp("http://ex/v")]));
    o.insert(DataPropertyDomain { dp: dp("http://ex/p"), ce: ClassExpression::Class(b.class("http://ex/A")) });
    o.insert(DataPropertyRange {
        dp: dp("http://ex/p"),
        dr: DataRange::Datatype(b.datatype("http://www.w3.org/2001/XMLSchema#integer")),
    });
    o.insert(FunctionalDataProperty(dp("http://ex/p")));

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        crate::io::omn::reader::read_with_build(BufReader::new(&buf[..]), &b).unwrap();

    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "data property frame did not round-trip");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -20`
Expected: FAIL — object/data property clauses are dropped (Task 2 stubs).

- [ ] **Step 3: Add the `ope` / `IRI` / `DataRange` list helpers**

In `from_pair.rs` near `parse_description_list`:

```rust
fn parse_ope_list<A: ForIRI>(
    list: Pair<Rule>, ctx: &Context<'_, A>,
) -> Result<Vec<ObjectPropertyExpression<A>>> {
    list.into_inner().map(|p| ObjectPropertyExpression::from_pair(p, ctx)).collect()
}

fn parse_iri_list<A: ForIRI>(
    list: Pair<Rule>, ctx: &Context<'_, A>,
) -> Result<Vec<IRI<A>>> {
    list.into_inner().map(|p| IRI::from_pair(p, ctx)).collect()
}

fn parse_data_range_list<A: ForIRI>(
    list: Pair<Rule>, ctx: &Context<'_, A>,
) -> Result<Vec<DataRange<A>>> {
    list.into_inner().map(|p| DataRange::from_pair(p, ctx)).collect()
}
```

- [ ] **Step 4: Fill in `insert_object_property_frame`**

Replace the Task-2 body:

```rust
fn insert_object_property_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, clauses) = frame_subject_and_clauses(frame, ctx)?;
    let subject_ope = ObjectPropertyExpression::ObjectProperty(ObjectProperty(subject.clone()));
    ont.insert(DeclareObjectProperty(ObjectProperty(subject.clone())));

    for clause in clauses {
        let kw = clause_keyword(&clause);
        let body = clause.into_inner().next().unwrap();
        match kw.as_str() {
            "subpropertyof" => {
                for sup in parse_ope_list(body, ctx)? {
                    ont.insert(SubObjectPropertyOf {
                        sub: SubObjectPropertyExpression::ObjectPropertyExpression(
                            subject_ope.clone(),
                        ),
                        sup,
                    });
                }
            }
            "equivalentto" => {
                let mut all = vec![subject_ope.clone()];
                all.extend(parse_ope_list(body, ctx)?);
                ont.insert(EquivalentObjectProperties(all));
            }
            "disjointwith" => {
                let mut all = vec![subject_ope.clone()];
                all.extend(parse_ope_list(body, ctx)?);
                ont.insert(DisjointObjectProperties(all));
            }
            "inverseof" => {
                for inv in parse_ope_list(body, ctx)? {
                    // InverseObjectProperties takes ObjectProperty, not OPE;
                    // the writer only emits a plain property here.
                    if let ObjectPropertyExpression::ObjectProperty(p) = inv {
                        ont.insert(InverseObjectProperties(
                            ObjectProperty(subject.clone()),
                            p,
                        ));
                    } else {
                        return Err(HornedError::invalid(
                            "InverseOf: expected a named object property",
                        ));
                    }
                }
            }
            "domain" => {
                for ce in parse_description_list(body, ctx)? {
                    ont.insert(ObjectPropertyDomain { ope: subject_ope.clone(), ce });
                }
            }
            "range" => {
                for ce in parse_description_list(body, ctx)? {
                    ont.insert(ObjectPropertyRange { ope: subject_ope.clone(), ce });
                }
            }
            "characteristics" => {
                for ch in body.into_inner() {
                    insert_object_characteristic(ch.as_str(), &subject_ope, ont)?;
                }
            }
            other => unreachable!("unexpected object-property clause keyword: {other}"),
        }
    }
    Ok(())
}

fn insert_object_characteristic<A: ForIRI, O: MutableOntology<A>>(
    kw: &str, ope: &ObjectPropertyExpression<A>, ont: &mut O,
) -> Result<()> {
    let ope = ope.clone();
    match kw.to_ascii_lowercase().as_str() {
        "functional" => ont.insert(FunctionalObjectProperty(ope)),
        "inversefunctional" => ont.insert(InverseFunctionalObjectProperty(ope)),
        "reflexive" => ont.insert(ReflexiveObjectProperty(ope)),
        "irreflexive" => ont.insert(IrreflexiveObjectProperty(ope)),
        "symmetric" => ont.insert(SymmetricObjectProperty(ope)),
        "asymmetric" => ont.insert(AsymmetricObjectProperty(ope)),
        "transitive" => ont.insert(TransitiveObjectProperty(ope)),
        other => return Err(HornedError::invalid(&format!("unknown characteristic: {other}"))),
    };
    Ok(())
}
```

- [ ] **Step 5: Fill in `insert_data_property_frame`**

```rust
fn insert_data_property_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareDataProperty(DataProperty(subject.clone())));

    for clause in clauses {
        let kw = clause_keyword(&clause);
        let body = clause.into_inner().next().unwrap();
        match kw.as_str() {
            "subpropertyof" => {
                for iri in parse_iri_list(body, ctx)? {
                    ont.insert(SubDataPropertyOf {
                        sub: DataProperty(subject.clone()),
                        sup: DataProperty(iri),
                    });
                }
            }
            "equivalentto" => {
                let mut all = vec![DataProperty(subject.clone())];
                all.extend(parse_iri_list(body, ctx)?.into_iter().map(DataProperty));
                ont.insert(EquivalentDataProperties(all));
            }
            "disjointwith" => {
                let mut all = vec![DataProperty(subject.clone())];
                all.extend(parse_iri_list(body, ctx)?.into_iter().map(DataProperty));
                ont.insert(DisjointDataProperties(all));
            }
            "domain" => {
                for ce in parse_description_list(body, ctx)? {
                    ont.insert(DataPropertyDomain { dp: DataProperty(subject.clone()), ce });
                }
            }
            "range" => {
                for dr in parse_data_range_list(body, ctx)? {
                    ont.insert(DataPropertyRange { dp: DataProperty(subject.clone()), dr });
                }
            }
            "characteristics" => {
                for ch in body.into_inner() {
                    // Only Functional is valid on a data property.
                    if ch.as_str().eq_ignore_ascii_case("functional") {
                        ont.insert(FunctionalDataProperty(DataProperty(subject.clone())));
                    } else {
                        return Err(HornedError::invalid(
                            "data properties only support the Functional characteristic",
                        ));
                    }
                }
            }
            other => unreachable!("unexpected data-property clause keyword: {other}"),
        }
    }
    Ok(())
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -15`
Expected: PASS — both new tests plus all prior tests green.

- [ ] **Step 7: Verify and commit**

Run: `cargo fmt -- --check` (rc 0) and `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

```bash
git add src/io/omn/reader/from_pair.rs
git commit -m "feat(omn/reader): object- and data-property frame clauses

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: AnnotationProperty + Individual frame clauses

**Files:**
- Modify: `src/io/omn/reader/from_pair.rs` (`insert_annotation_property_frame`, `insert_individual_frame`, a `Fact` helper, tests)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn reads_annotation_property_frame_round_trip() {
    use crate::io::omn::write;
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();

    let mut o = SetOntology::new_rc();
    o.insert(DeclareAnnotationProperty(b.annotation_property("http://ex/n")));
    o.insert(SubAnnotationPropertyOf {
        sub: b.annotation_property("http://ex/n"),
        sup: b.annotation_property("http://ex/m"),
    });
    o.insert(AnnotationPropertyDomain { ap: b.annotation_property("http://ex/n"), iri: b.iri("http://ex/A") });
    o.insert(AnnotationPropertyRange { ap: b.annotation_property("http://ex/n"), iri: b.iri("http://ex/B") });

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        crate::io::omn::reader::read_with_build(BufReader::new(&buf[..]), &b).unwrap();

    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "annotation property frame did not round-trip");
}

#[test]
fn reads_individual_frame_round_trip() {
    use crate::io::omn::write;
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    pm.add_prefix("xsd", "http://www.w3.org/2001/XMLSchema#").unwrap();
    let named = |i: &str| Individual::Named(b.named_individual(i));

    let mut o = SetOntology::new_rc();
    o.insert(DeclareNamedIndividual(b.named_individual("http://ex/a")));
    o.insert(DeclareClass(b.class("http://ex/A")));
    o.insert(ClassAssertion { i: named("http://ex/a"), ce: ClassExpression::Class(b.class("http://ex/A")) });
    o.insert(ObjectPropertyAssertion {
        ope: ObjectPropertyExpression::ObjectProperty(b.object_property("http://ex/r")),
        from: b.named_individual("http://ex/a").into(),
        to: b.named_individual("http://ex/b").into(),
    });
    o.insert(DataPropertyAssertion {
        dp: b.data_property("http://ex/p"),
        from: b.named_individual("http://ex/a").into(),
        to: Literal::Datatype {
            literal: "5".to_string(),
            datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        },
    });
    // negative facts exercise the `Facts: not …` negation-detection path
    o.insert(NegativeObjectPropertyAssertion {
        ope: ObjectPropertyExpression::ObjectProperty(b.object_property("http://ex/r")),
        from: b.named_individual("http://ex/a").into(),
        to: named("http://ex/b"),
    });
    o.insert(NegativeDataPropertyAssertion {
        dp: b.data_property("http://ex/p"),
        from: b.named_individual("http://ex/a").into(),
        to: Literal::Datatype {
            literal: "6".to_string(),
            datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        },
    });
    o.insert(SameIndividual(vec![named("http://ex/a"), named("http://ex/c")]));
    o.insert(DifferentIndividuals(vec![named("http://ex/a"), named("http://ex/d")]));

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        crate::io::omn::reader::read_with_build(BufReader::new(&buf[..]), &b).unwrap();

    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "individual frame did not round-trip");
}
```

Check `Individual` construction: the test uses `.into()` on `NamedIndividual` for `from`/`to` of `ObjectPropertyAssertion`. If `From<NamedIndividual> for Individual` is not implemented, use `Individual::Named(b.named_individual(..))` instead (verify against `src/model.rs`; the `named` closure already shows the explicit form).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -20`
Expected: FAIL — annotation/individual clauses dropped.

- [ ] **Step 3: Fill in `insert_annotation_property_frame`**

```rust
fn insert_annotation_property_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, clauses) = frame_subject_and_clauses(frame, ctx)?;
    ont.insert(DeclareAnnotationProperty(AnnotationProperty(subject.clone())));

    for clause in clauses {
        let kw = clause_keyword(&clause);
        let body = clause.into_inner().next().unwrap();
        let iris = parse_iri_list(body, ctx)?;
        match kw.as_str() {
            "subpropertyof" => {
                for iri in iris {
                    ont.insert(SubAnnotationPropertyOf {
                        sub: AnnotationProperty(subject.clone()),
                        sup: AnnotationProperty(iri),
                    });
                }
            }
            "domain" => {
                for iri in iris {
                    ont.insert(AnnotationPropertyDomain {
                        ap: AnnotationProperty(subject.clone()),
                        iri,
                    });
                }
            }
            "range" => {
                for iri in iris {
                    ont.insert(AnnotationPropertyRange {
                        ap: AnnotationProperty(subject.clone()),
                        iri,
                    });
                }
            }
            other => unreachable!("unexpected annotation-property clause keyword: {other}"),
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Fill in `insert_individual_frame` + the `Fact` parser**

```rust
fn insert_individual_frame<A: ForIRI, O: MutableOntology<A>>(
    frame: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let (subject, clauses) = frame_subject_and_clauses(frame, ctx)?;
    let subject_ind = Individual::Named(NamedIndividual(subject.clone()));
    ont.insert(DeclareNamedIndividual(NamedIndividual(subject.clone())));

    for clause in clauses {
        let kw = clause_keyword(&clause);
        let body = clause.into_inner().next().unwrap();
        match kw.as_str() {
            "types" => {
                for ce in parse_description_list(body, ctx)? {
                    ont.insert(ClassAssertion { i: subject_ind.clone(), ce });
                }
            }
            "facts" => {
                for fact in body.into_inner() {
                    insert_fact(fact, ctx, &subject_ind, ont)?;
                }
            }
            "sameas" => {
                let mut all = vec![subject_ind.clone()];
                for p in body.into_inner() {
                    all.push(Individual::from_pair(p, ctx)?);
                }
                ont.insert(SameIndividual(all));
            }
            "differentfrom" => {
                let mut all = vec![subject_ind.clone()];
                for p in body.into_inner() {
                    all.push(Individual::from_pair(p, ctx)?);
                }
                ont.insert(DifferentIndividuals(all));
            }
            other => unreachable!("unexpected individual clause keyword: {other}"),
        }
    }
    Ok(())
}

/// `Fact = { ^"not"? ~ ope ~ ( Literal | Individual ) }`
///
/// A trailing `Literal` => (negative) data-property assertion; a trailing
/// `Individual` => (negative) object-property assertion. The leading `not`
/// keyword is a bare literal (no child pair), so detect it from the span.
fn insert_fact<A: ForIRI, O: MutableOntology<A>>(
    fact: Pair<Rule>,
    ctx: &Context<'_, A>,
    from: &Individual<A>,
    ont: &mut O,
) -> Result<()> {
    let negated = fact
        .as_str()
        .trim_start()
        .get(..3)
        .is_some_and(|h| h.eq_ignore_ascii_case("not"));
    let mut inner = fact.into_inner();
    let ope_pair = inner.next().unwrap(); // ope
    let ope = ObjectPropertyExpression::from_pair(ope_pair, ctx)?;
    let target = inner.next().unwrap();
    match target.as_rule() {
        Rule::Literal => {
            // data-property assertion; the ope's inner IRI is the data property.
            let lit = Literal::from_pair(target, ctx)?;
            let dp = match &ope {
                ObjectPropertyExpression::ObjectProperty(p) => DataProperty(p.0.clone()),
                ObjectPropertyExpression::InverseObjectProperty(_) => {
                    return Err(HornedError::invalid("inverse property in a data fact"));
                }
            };
            if negated {
                ont.insert(NegativeDataPropertyAssertion { dp, from: from.clone(), to: lit });
            } else {
                ont.insert(DataPropertyAssertion { dp, from: from.clone(), to: lit });
            }
        }
        Rule::Individual => {
            let to = Individual::from_pair(target, ctx)?;
            if negated {
                ont.insert(NegativeObjectPropertyAssertion { ope, from: from.clone(), to });
            } else {
                ont.insert(ObjectPropertyAssertion { ope, from: from.clone(), to });
            }
        }
        rule => unreachable!("unexpected fact target: {:?}", rule),
    }
    Ok(())
}
```

Verify `ObjectPropertyExpression::InverseObjectProperty`'s exact variant name against `src/model.rs` (it may be `InverseObjectProperty(ObjectProperty)`); adjust the match if the name differs. Verify `DataProperty` wraps an `IRI` accessible as `p.0` on `ObjectProperty`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib io::omn::reader 2>&1 | tail -15`
Expected: PASS — annotation + individual round-trip tests green, all prior tests green.

- [ ] **Step 6: Verify and commit**

Run: `cargo fmt -- --check` (rc 0) and `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

```bash
git add src/io/omn/reader/from_pair.rs
git commit -m "feat(omn/reader): annotation-property and individual frame clauses

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Whole-ontology round-trip capstone + public API + limitations

**Files:**
- Modify: `src/io/omn/mod.rs` (re-export `read` / `read_with_build`)
- Modify: `src/io/omn/reader/from_pair.rs` (capstone round-trip test + a module-level limitations doc comment)

- [ ] **Step 1: Re-export `read` from the module entry**

In `src/io/omn/mod.rs`, extend the re-exports:

```rust
//! OWL Manchester Syntax I/O.
pub mod reader;
pub mod writer;
pub use reader::{parse_class_expression, read, read_with_build};
pub use writer::{AsManchester, Manchester, write};
```

- [ ] **Step 2: Write the capstone round-trip test**

Add to the `from_pair.rs` test module — one mixed ontology touching every frame kind and clause, asserting full structural equality:

```rust
#[test]
fn whole_ontology_round_trips() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    pm.add_prefix("xsd", "http://www.w3.org/2001/XMLSchema#").unwrap();

    let ce = |i: &str| ClassExpression::Class(b.class(i));
    let ope = |i: &str| ObjectPropertyExpression::ObjectProperty(b.object_property(i));
    let named = |i: &str| Individual::Named(b.named_individual(i));

    let mut o = SetOntology::new_rc();
    // ontology header
    let mut oid = OntologyID::default();
    oid.iri = Some(b.iri("http://ex/onto"));
    o.insert(oid);
    // declarations
    for c in ["A", "B", "C", "D"] {
        o.insert(DeclareClass(b.class(&format!("http://ex/{c}"))));
    }
    o.insert(DeclareObjectProperty(b.object_property("http://ex/r")));
    o.insert(DeclareObjectProperty(b.object_property("http://ex/t")));
    o.insert(DeclareDataProperty(b.data_property("http://ex/p")));
    o.insert(DeclareAnnotationProperty(b.annotation_property("http://ex/n")));
    o.insert(DeclareNamedIndividual(b.named_individual("http://ex/a")));
    o.insert(DeclareNamedIndividual(b.named_individual("http://ex/b")));
    o.insert(DeclareDatatype(b.datatype("http://ex/dt")));
    // class axioms
    o.insert(SubClassOf { sub: ce("http://ex/A"), sup: ce("http://ex/B") });
    o.insert(EquivalentClasses(vec![ce("http://ex/A"), ce("http://ex/C")]));
    o.insert(DisjointClasses(vec![ce("http://ex/A"), ce("http://ex/D")]));
    // object property axioms
    o.insert(ObjectPropertyDomain { ope: ope("http://ex/r"), ce: ce("http://ex/A") });
    o.insert(FunctionalObjectProperty(ope("http://ex/r")));
    o.insert(InverseObjectProperties(
        b.object_property("http://ex/r"),
        b.object_property("http://ex/t"),
    ));
    // data property axioms
    o.insert(DataPropertyRange {
        dp: b.data_property("http://ex/p"),
        dr: DataRange::Datatype(b.datatype("http://www.w3.org/2001/XMLSchema#integer")),
    });
    // annotation property axioms
    o.insert(AnnotationPropertyDomain { ap: b.annotation_property("http://ex/n"), iri: b.iri("http://ex/A") });
    // individual axioms
    o.insert(ClassAssertion { i: named("http://ex/a"), ce: ce("http://ex/A") });
    o.insert(ObjectPropertyAssertion { ope: ope("http://ex/r"), from: named("http://ex/a"), to: named("http://ex/b") });
    o.insert(DataPropertyAssertion {
        dp: b.data_property("http://ex/p"),
        from: named("http://ex/a"),
        to: Literal::Datatype { literal: "5".into(), datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer") },
    });

    let amo: ComponentMappedOntology<_, _> = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();

    let (parsed, parsed_pm): (SetOntology<_>, PrefixMapping) =
        read_with_build(BufReader::new(&buf[..]), &b).unwrap();

    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(
        orig, got,
        "whole ontology did not round-trip\n--- document ---\n{}",
        String::from_utf8_lossy(&buf)
    );
    // prefixes survive the round-trip
    assert_eq!(parsed_pm.expand_curie_string("ex:A").unwrap(), "http://ex/A");
}
```

- [ ] **Step 3: Run the capstone test**

Run: `cargo test --lib io::omn::reader::from_pair::tests::whole_ontology_round_trips 2>&1 | tail -30`
Expected: PASS. If it fails, the panic message prints the written document — diff `orig` vs `got` to find the offending clause and fix the responsible `insert_*_frame`.

- [ ] **Step 4: Add the limitations doc comment**

At the top of `src/io/omn/reader/mod.rs` (module doc), add:

```rust
//! OWL Manchester Syntax reader.
//!
//! Parses prefix declarations, an optional `Ontology:` header, and the six
//! entity frames (`Class:`, `ObjectProperty:`, `DataProperty:`,
//! `AnnotationProperty:`, `Individual:`, `Datatype:`) into a mutable ontology.
//! It is the structural inverse of [`crate::io::omn::write`].
//!
//! ## Known limitations (P3)
//! - The writer's trailing `# General axioms` block (OWL functional syntax for
//!   components with no native Manchester form — `Import`, `HasKey`,
//!   `OntologyAnnotation`, axiom annotations, SWRL rules, property chains,
//!   n-ary axioms over anonymous subjects) is NOT parsed. The `#` line itself
//!   is consumed as a `COMMENT`, but the functional-syntax lines beneath it
//!   match no `Frame` rule, so a document carrying a non-empty misc block is
//!   REJECTED at EOI (a hard parse error) rather than partially parsed. Such
//!   documents do not round-trip; the round-trip corpus avoids misc-only
//!   axioms.
//! - Frame headers conflate declaration and reference: every frame yields a
//!   `Declare*` axiom, so an entity used without an explicit declaration gains
//!   one on round-trip. Declarations are non-logical (entailment-neutral).
//! - n-ary `EquivalentTo:`/`DisjointWith:`/`SameAs:`/`DifferentFrom:` lists are
//!   read as a SINGLE n-ary axiom with the frame subject prepended (the exact
//!   inverse of the writer), not OWL-API's pairwise expansion.
//! - A bare local name as a frame subject or IRI (emitted by the writer only
//!   when a default `""` prefix is registered) is not lexable; use `<full>` or
//!   `prefix:local`. Round-tripping requires a non-default prefix.
//! - `Annotations:` clauses are not parsed (the writer does not emit them).
//! - **Keyword / CURIE-prefix collision (correctness gap, MUST FIX BEFORE the
//!   upstream PR).** Manchester keywords (`not`, `and`, `or`, `some`, `only`,
//!   `value`, `min`, `max`, `exactly`, `Self`, `inverse`, and the facet words)
//!   are matched without a name-boundary, so an *abbreviated* CURIE whose
//!   prefix begins with a keyword is silently mis-parsed — e.g. `notation:foo`
//!   lexes as `not` + `ation:foo`, and `andx:bar` as `and` + `x:bar`. Full
//!   `<...>` IRIs are immune (they start with `<`). This reader round-trips the
//!   **writer's own output** completely (the writer never emits such CURIEs),
//!   but it is therefore NOT yet a general hand-written-Manchester parser. The
//!   fix is maximal-munch boundary anchoring on every keyword token —
//!   `@{ ^"not" ~ !PnChar }` rather than a trailing-whitespace guard (which
//!   would break `not(C and D)`) — applied across BOTH the P2 class-expression
//!   rules and the P3 frame rules, with a per-keyword negative test and the
//!   full P2 round-trip suite as regression. See the pre-upstream-PR list.
```

- [ ] **Step 5: Full omn suite + build + lint**

Run:
- `cargo test --lib io::omn 2>&1 | tail -8` — all omn tests pass (P1 writer + P2 expression reader + P3 frames).
- `cargo build -p horned-owl 2>&1 | tail -2` — builds.
- `cargo fmt -- --check` (rc 0) and `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).
- `cd /data/dumontier/rustdl && cargo build -p owl-dl-reasoner 2>&1 | tail -2` — rustdl still builds against the fork (its `[patch]` is rev-pinned to the writer commit and does not pick up the reader; this only confirms no incidental breakage).

- [ ] **Step 6: Commit**

```bash
git add src/io/omn/mod.rs src/io/omn/reader/mod.rs src/io/omn/reader/from_pair.rs
git commit -m "feat(omn/reader): whole-ontology read + round-trip gate + limitations

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Pre-upstream-PR follow-ups (out of P3 scope, documented for the PR)

- **Keyword boundary anchoring (correctness gap — must fix before PR).** Make
  every Manchester keyword token maximal-munch so it never eats a CURIE-prefix
  prefix: replace each `^"keyword"` with `@{ ^"keyword" ~ !PnChar }` (a
  name-boundary negative lookahead — handles `not `, `not(`, `not<`, rejects
  `notation`; do NOT use `~ WHITESPACE+`, which breaks `not(C and D)`). Apply to
  ALL keywords in BOTH the P2 class-expression rules (`not`/`and`/`or`/`some`/
  `only`/`value`/`min`/`max`/`exactly`/`Self`/`inverse`) and the P3 frame/clause
  rules. Add a per-keyword negative test (`notation:foo`, `andx:bar`, …) and run
  the full P2 17-case round-trip suite as regression — P2's `not` detection
  (`child.start() > pair.start()`) is load-bearing and must not be disturbed
  without that guard. (Surfaced by the Task 5 review; deferred as a dedicated
  unit per the advisor — not half-fixed in `Fact` alone.)
- Native `read` integration into `src/io/mod.rs` (`ResourceType::OMN`, a `ParserOutput::OMNParser` variant) so callers dispatch on extension — touches the shared `ParserOutput` enum; defer to the PR-prep pass alongside the writer's `Import:`/`Annotations:` follow-ups.
- Parse the `# General axioms` block (or, better, make the writer emit native Manchester for those variants so the block disappears) to close the round-trip on imports/keys/annotations/chains.
- Accept bare local names against a default prefix (resolve keyword-vs-IRI ambiguity) to round-trip default-prefix documents.
- Rebase `manchester-io` onto phillord/horned-owl `main` and validate `write` output through omny / the OWL API before opening the PR.

---

## Self-Review notes (filled during planning)

- **Spec coverage:** frame grammar (Task 1), `from_pair` for axioms (Tasks 2–5), full-document `read` (Task 2), round-trip whole ontologies (Task 6) — all four P3 spec bullets (design spec lines 120–121, 128–130) covered.
- **Type consistency:** `insert_frame` / `insert_*_frame` / `frame_subject_and_clauses` / `clause_keyword` / `parse_*_list` names are used identically across tasks; `read_with_build` signature matches ofn's. Component constructors cross-checked against the writer's match arms (the table above).
- **Verification flags:** three model-shape items the implementer must confirm against `src/model.rs` are called out inline (Build constructor names; `From<NamedIndividual> for Individual`; the `InverseObjectProperty` variant name + `ObjectProperty.0` IRI access). These are the only places the plan infers a name it did not directly read.
