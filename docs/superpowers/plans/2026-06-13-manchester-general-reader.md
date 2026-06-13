# General OWL 2 Manchester Syntax (§2.5) Reader+Writer Conformance — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the horned-owl fork's `io/omn` a *general* OWL 2 Manchester Syntax reader — able to consume any valid Manchester document (OWL-API / Protégé / community output), not just our own writer's output — and extend the writer so the newly-supported constructs round-trip.

**Architecture:** Close the §2.5 gaps the reader validation found, one construct-class per task: version IRI, full data ranges, datatype definitions, the top-level `misc` axiom section, and the full per-item `annotatedList`. Each task adds grammar (`src/grammars/omn.pest`) + reader (`src/io/omn/reader/{from_pair.rs,mod.rs}`) + writer (`src/io/omn/writer/{mod.rs,as_manchester.rs}`) where the writer currently drops the construct to the functional-syntax fallback. A final task adds a semantic-conformance gate using OWL-API (ROBOT) as oracle.

**Tech Stack:** Rust (edition 2024), pest/pest_derive, horned-owl object model, `curie::PrefixMapping`, ROBOT-in-docker (OWL-API) for the conformance oracle.

**Working directory:** `/data/dumontier/horned-owl-omn` (branch `master`). Toolchain: prepend `PATH` with `/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`. After every task: `cargo test --lib io::omn 2>&1 | tail -3`, `cargo fmt -- --check` (rc 0; ignore benign `array_width` note), `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

---

## Reference: §2.5 productions + model + current state

**§2.5 grammar (verbatim, target):**
```
ontology       ::= 'Ontology:' [ ontologyIRI [ versionIRI ] ] { import } { annotations } { frame }
datatypeFrame  ::= 'Datatype:' Datatype { 'Annotations:' annotationAnnotatedList } [ 'EquivalentTo:' annotations dataRange ]
dataRange      ::= dataConjunction 'or' dataConjunction { 'or' dataConjunction } | dataConjunction
dataConjunction::= dataPrimary 'and' dataPrimary { 'and' dataPrimary } | dataPrimary
dataPrimary    ::= [ 'not' ] dataAtomic
dataAtomic     ::= Datatype | '{' literalList '}' | datatypeRestriction | '(' dataRange ')'
datatypeRestriction ::= Datatype '[' facet restrictionValue { ',' facet restrictionValue } ']'
descriptionAnnotatedList ::= [annotations] description { ',' [annotations] description }
misc ::= 'EquivalentClasses:'    annotations description2List
       | 'DisjointClasses:'      annotations description2List
       | 'EquivalentProperties:' annotations objectProperty2List
       | 'DisjointProperties:'   annotations objectProperty2List
       | 'EquivalentProperties:' annotations dataProperty2List
       | 'DisjointProperties:'   annotations dataProperty2List
       | 'SameIndividual:'       annotations individual2List
       | 'DifferentIndividuals:' annotations individual2List
```

**horned-owl model (verified):**
- `DataRange` enum: `Datatype(Datatype)`, `DataIntersectionOf(Vec<DataRange>)`, `DataUnionOf(Vec<DataRange>)`, `DataComplementOf(Box<DataRange>)`, `DataOneOf(Vec<Literal>)`, `DatatypeRestriction(Datatype, Vec<FacetRestriction>)`.
- `FacetRestriction { f: Facet, l: Literal }`.
- `DatatypeDefinition { kind: Datatype, range: DataRange }`.
- `OntologyID { iri: Option<IRI>, viri: Option<IRI> }` (viri = version IRI).
- Misc axiom Components (all exist): `EquivalentClasses(Vec<ClassExpression>)`, `DisjointClasses(Vec<ClassExpression>)`, `EquivalentObjectProperties(Vec<ObjectPropertyExpression>)`, `DisjointObjectProperties(...)`, `EquivalentDataProperties(Vec<DataProperty>)`, `DisjointDataProperties(...)`, `SameIndividual(Vec<Individual>)`, `DifferentIndividuals(...)`.

**Current state (what's already done — do NOT redo):**
- Writer `DataRange` Display (`as_manchester.rs:247+`) ALREADY renders all DataRange variants (`and`/`or`/`not`/`{oneof}`/facets) with precedence. **Data ranges need READER+GRAMMAR work only.**
- Reader `DataRange::from_pair` (`from_pair.rs:279`) is minimal: parses only `Datatype` + facets (`DataRange = { DatatypeIRI ~ ("[" Facet … "]")? }`). Must be generalized.
- `OntologyHeader = { ^"Ontology:" ~ IRI? ~ ImportDeclaration* ~ Annotations* }` — one IRI; reader arm at `mod.rs:131` with the `has_id` gate.
- `DatatypeFrame = { ^"Datatype:" ~ FrameSubject }` — no clauses; `insert_datatype_frame` at `from_pair.rs:1304` emits only `DeclareDatatype`.
- The writer already has `Component::EquivalentClasses`/`SameIndividual`/etc. arms (`mod.rs:269,651`) that route to a frame when the first member is a named class/property/individual, else `misc.push(...)` (functional syntax). Those misc pushes are what we replace with native `misc`-section emission.
- Bare local names (`SimpleIRI`) — already supported.
- Per-clause leading `Annotations?` (axiom annotation on the whole clause) exists from the earlier P3 work; full per-item annotatedList is Task 5.

---

## Task 1: Version IRI in the ontology header

**Files:** `src/grammars/omn.pest`, `src/io/omn/reader/mod.rs`, `src/io/omn/writer/mod.rs`, test in `src/io/omn/reader/from_pair.rs`.

- [ ] **Step 1: Write the failing round-trip test** (append to the `from_pair.rs` test module):
```rust
#[test]
fn reads_version_iri_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    let mut o = SetOntology::new_rc();
    let mut oid = OntologyID::default();
    oid.iri = Some(b.iri("http://ex/o"));
    oid.viri = Some(b.iri("http://ex/o/1.0.0"));
    o.insert(oid);
    o.insert(DeclareClass(b.class("http://ex/A")));
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let s = String::from_utf8(buf.clone()).unwrap();
    assert!(s.contains("Ontology: ex:o ex:o/1.0.0") || s.contains("Ontology: ex:o <http://ex/o/1.0.0>"),
        "expected version IRI in header, got:\n{s}");
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "version IRI did not round-trip\n{s}");
}
```

- [ ] **Step 2: Run → FAIL** (`cargo test --lib io::omn::reader::from_pair::tests::reads_version_iri_round_trip 2>&1 | tail`): the writer emits no version IRI and/or the grammar can't parse a second header IRI.

- [ ] **Step 3: Grammar — add the optional version IRI.** In `omn.pest`, change:
```pest
OntologyHeader = { ^"Ontology:" ~ ( OntologyIRI ~ VersionIRI? )? ~ ImportDeclaration* ~ Annotations* }
OntologyIRI    = { IRI }
VersionIRI     = { IRI }
```
(Replacing `IRI?` with `( OntologyIRI ~ VersionIRI? )?`. Per §2.5 the version IRI may appear only when the ontology IRI is present.)

- [ ] **Step 4: Reader — set `viri`.** In `mod.rs`'s `Rule::OntologyHeader` arm, the children are now `OntologyIRI?`, `VersionIRI?`, then imports/annotations. Update the loop to match `Rule::OntologyIRI` (sets `oid.iri`, `has_id = true`) and `Rule::VersionIRI` (sets `oid.viri = Some(IRI::from_pair(h.into_inner().next().unwrap(), &ctx)?)`, also `has_id = true`). Keep the `if has_id { ontology.insert(oid) }` gate.

- [ ] **Step 5: Writer — emit the version IRI.** In `mod.rs`, the header block reads the `OntologyID` (`Component::OntologyID(oid)` at ~line 101). After emitting `Ontology: <iri>`, if `oid.viri.is_some()`, append ` {}` with the version IRI rendered via `as_manchester_with_prefixes`. So the line becomes `Ontology: <oiri> <viri>`.

- [ ] **Step 6: Run → PASS; fmt/clippy; commit.**
```bash
git add src/grammars/omn.pest src/io/omn/reader/mod.rs src/io/omn/writer/mod.rs src/io/omn/reader/from_pair.rs
git commit -m "feat(omn): version IRI in the Ontology: header (round-trip)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Full data ranges in the reader (and/or/not/oneOf/parens)

The writer already renders all `DataRange` variants; only the reader+grammar are minimal. Generalize them to the §2.5 `dataRange` grammar.

**Files:** `src/grammars/omn.pest`, `src/io/omn/reader/from_pair.rs`, test in `from_pair.rs`.

- [ ] **Step 1: Write the failing round-trip test:**
```rust
#[test]
fn reads_compound_data_ranges_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use crate::vocab::Facet;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    pm.add_prefix("xsd", "http://www.w3.org/2001/XMLSchema#").unwrap();
    let xsd_int = || DataRange::Datatype(b.datatype("http://www.w3.org/2001/XMLSchema#integer"));
    let restr = DataRange::DatatypeRestriction(
        b.datatype("http://www.w3.org/2001/XMLSchema#integer"),
        vec![FacetRestriction { f: Facet::MinInclusive,
            l: Literal::Datatype { literal: "0".into(), datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer") } }],
    );
    let mut o = SetOntology::new_rc();
    o.insert(DeclareDataProperty(b.data_property("http://ex/p")));
    o.insert(DeclareClass(b.class("http://ex/A")));
    // C ⊑ ∃p.( (xsd:integer and [≥0]) or not {"x"} )
    o.insert(SubClassOf {
        sub: ClassExpression::Class(b.class("http://ex/A")),
        sup: ClassExpression::DataSomeValuesFrom {
            dp: b.data_property("http://ex/p"),
            dr: DataRange::DataUnionOf(vec![
                DataRange::DataIntersectionOf(vec![xsd_int(), restr]),
                DataRange::DataComplementOf(Box::new(
                    DataRange::DataOneOf(vec![Literal::Simple { literal: "x".into() }]))),
            ]),
        },
    });
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "compound data range did not round-trip\n{}", String::from_utf8_lossy(&buf));
}
```

- [ ] **Step 2: Run → FAIL** — reader's `DataRange::from_pair` only handles Datatype+facets.

- [ ] **Step 3: Grammar — full data-range productions.** Replace the current `DataRange = { DatatypeIRI ~ ( "[" ~ Facet ~ ( "," ~ Facet )* ~ "]" )? }` with the §2.5 layered grammar (mirroring the class-expression precedence layers, reusing the `OrKw`/`AndKw`/`NotKw` keyword rules already defined for class expressions):
```pest
DataRange       = { DataConjunction ~ ( OrKw  ~ DataConjunction )* }
DataConjunction = { DataPrimary     ~ ( AndKw ~ DataPrimary )* }
DataPrimary     = { NotKw? ~ DataAtomic }
DataAtomic      = { DataOneOf | DatatypeRestriction | "(" ~ DataRange ~ ")" | DatatypeIRI }
DataOneOf       = { "{" ~ Literal ~ ( "," ~ Literal )* ~ "}" }
DatatypeRestriction = { DatatypeIRI ~ "[" ~ Facet ~ ( "," ~ Facet )* ~ "]" }
```
Keep `DatatypeIRI`, `Facet`, `FacetSymbol` as-is. Note ordered choice in `DataAtomic`: `DatatypeRestriction` (datatype `[`…`]`) must come before the bare `DatatypeIRI` so the facet bracket is consumed.

- [ ] **Step 4: Reader — `DataRange::from_pair` for all variants.** Replace the current impl. The new `DataRange::from_pair` receives a `Rule::DataRange` pair (the top `or` layer):
```rust
impl<A: ForIRI> FromPair<A> for DataRange<A> {
    const RULE: Rule = Rule::DataRange;
    fn from_pair_unchecked(pair: Pair<Rule>, ctx: &Context<'_, A>) -> Result<Self> {
        // DataRange = DataConjunction (OrKw DataConjunction)*
        let mut conjs: Vec<DataRange<A>> = pair
            .into_inner()
            .filter(|p| p.as_rule() == Rule::DataConjunction)
            .map(|p| data_conjunction(p, ctx))
            .collect::<Result<_>>()?;
        Ok(if conjs.len() == 1 { conjs.remove(0) } else { DataRange::DataUnionOf(conjs) })
    }
}

fn data_conjunction<A: ForIRI>(pair: Pair<Rule>, ctx: &Context<'_, A>) -> Result<DataRange<A>> {
    // DataConjunction = DataPrimary (AndKw DataPrimary)*
    let mut prims: Vec<DataRange<A>> = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::DataPrimary)
        .map(|p| data_primary(p, ctx))
        .collect::<Result<_>>()?;
    Ok(if prims.len() == 1 { prims.remove(0) } else { DataRange::DataIntersectionOf(prims) })
}

fn data_primary<A: ForIRI>(pair: Pair<Rule>, ctx: &Context<'_, A>) -> Result<DataRange<A>> {
    // DataPrimary = NotKw? DataAtomic
    let mut it = pair.into_inner();
    let mut first = it.next().unwrap();
    let negated = first.as_rule() == Rule::NotKw;
    if negated { first = it.next().unwrap(); }
    let atomic = data_atomic(first, ctx)?;
    Ok(if negated { DataRange::DataComplementOf(Box::new(atomic)) } else { atomic })
}

fn data_atomic<A: ForIRI>(pair: Pair<Rule>, ctx: &Context<'_, A>) -> Result<DataRange<A>> {
    // DataAtomic = DataOneOf | DatatypeRestriction | "(" DataRange ")" | DatatypeIRI
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::DataOneOf => {
            let lits = inner.into_inner().map(|p| Literal::from_pair(p, ctx)).collect::<Result<_>>()?;
            Ok(DataRange::DataOneOf(lits))
        }
        Rule::DatatypeRestriction => {
            let mut parts = inner.into_inner();
            let dt = Datatype::from_pair(parts.next().unwrap(), ctx)?;
            let facets = parts.map(|p| FacetRestriction::from_pair(p, ctx)).collect::<Result<_>>()?;
            Ok(DataRange::DatatypeRestriction(dt, facets))
        }
        Rule::DataRange => DataRange::from_pair(inner, ctx), // parenthesized
        Rule::DatatypeIRI => Ok(DataRange::Datatype(Datatype::from_pair(inner, ctx)?)),
        rule => unreachable!("unexpected data-atomic rule: {:?}", rule),
    }
}
```
(`Datatype::from_pair` already exists via the `impl_wrapper!(Datatype, Rule::DatatypeIRI)`. `FacetRestriction::from_pair` already exists.)

- [ ] **Step 5: Run → PASS** (compound data range round-trips). Also confirm the existing data-property `Range:`/datatype tests still pass (the bare `Datatype` and single facet-restriction cases now flow through the layered grammar). fmt/clippy.

- [ ] **Step 6: Commit.**
```bash
git add src/grammars/omn.pest src/io/omn/reader/from_pair.rs src/io/omn/reader/from_pair.rs
git commit -m "feat(omn/reader): full data ranges (and/or/not/oneOf/parens)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Datatype-definition frame clause (`Datatype: D EquivalentTo: dataRange`)

**Files:** `src/grammars/omn.pest`, `src/io/omn/reader/from_pair.rs` (`insert_datatype_frame`), `src/io/omn/writer/mod.rs`, test in `from_pair.rs`.

- [ ] **Step 1: Write the failing round-trip test:**
```rust
#[test]
fn reads_datatype_definition_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use crate::vocab::Facet;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    pm.add_prefix("xsd", "http://www.w3.org/2001/XMLSchema#").unwrap();
    let mut o = SetOntology::new_rc();
    o.insert(DeclareDatatype(b.datatype("http://ex/SmallInt")));
    o.insert(DatatypeDefinition {
        kind: b.datatype("http://ex/SmallInt"),
        range: DataRange::DatatypeRestriction(
            b.datatype("http://www.w3.org/2001/XMLSchema#integer"),
            vec![FacetRestriction { f: Facet::MaxInclusive,
                l: Literal::Datatype { literal: "255".into(), datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer") } }]),
    });
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let s = String::from_utf8(buf.clone()).unwrap();
    assert!(s.contains("Datatype: ex:SmallInt") && s.contains("EquivalentTo:"), "got:\n{s}");
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "datatype definition did not round-trip\n{s}");
}
```

- [ ] **Step 2: Run → FAIL** — `DatatypeDefinition` currently goes to the writer's misc fallback; the datatype frame has no clauses.

- [ ] **Step 3: Grammar — datatype frame clauses.**
```pest
DatatypeFrame = { ^"Datatype:" ~ FrameSubject ~ DatatypeClause* }
DatatypeClause = { Annotations | ^"EquivalentTo:" ~ Annotations? ~ DataRange }
```
(`Annotations` first = entity annotation on the datatype; the keyworded arm = the definition. The `Annotations?` prefix is the axiom-annotation slot, consistent with other frames.)

- [ ] **Step 4: Reader — handle the `EquivalentTo:` clause.** In `insert_datatype_frame` (`from_pair.rs:1304`), after the `DeclareDatatype`, iterate clauses (like other frames). For the standalone `Annotations` arm emit `AnnotationAssertion` (subject = the datatype IRI). For `clause_keyword == "equivalentto"`: the body is a `DataRange` → `ont.insert(DatatypeDefinition { kind: Datatype(subject.clone()), range: DataRange::from_pair(body, ctx)? })`. (Use the existing `frame_subject_and_clauses` + `clause_keyword` helpers; mirror `insert_class_frame`'s structure.)

- [ ] **Step 5: Writer — emit the definition under the Datatype frame.** In `mod.rs`, add a `Component::DatatypeDefinition(ax)` arm (it currently hits `_ => misc`): push an `EquivalentTo: <dr>` clause to the `FrameKind::Datatype` frame keyed by `ax.kind.0.as_ref()`, with the data range rendered via `ax.range.as_manchester_with_prefixes(pm)`. Add `ComponentKind::DatatypeDefinition` handling (it is no longer misc). Confirm `ComponentKind::DatatypeDefinition` is the exact name (grep model.rs).

- [ ] **Step 6: Run → PASS; fmt/clippy; commit.**
```bash
git add src/grammars/omn.pest src/io/omn/reader/from_pair.rs src/io/omn/writer/mod.rs
git commit -m "feat(omn): datatype definitions (Datatype: D EquivalentTo: dataRange) round-trip

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Top-level `misc` axiom section

n-ary equivalence/disjointness/same/different axioms whose members are NOT all named (so they don't fit a frame) currently go to the writer's misc functional-syntax fallback. §2.5's `misc` section expresses them natively. Add the grammar + reader + writer.

**Files:** `src/grammars/omn.pest`, `src/io/omn/reader/{from_pair.rs,mod.rs}`, `src/io/omn/writer/mod.rs`, test in `from_pair.rs`.

- [ ] **Step 1: Write the failing round-trip test** (a `DisjointClasses` over *complex* expressions — has no named first member, so it currently can't be framed):
```rust
#[test]
fn reads_misc_disjoint_complex_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    let some = |r: &str, c: &str| ClassExpression::ObjectSomeValuesFrom {
        ope: ObjectPropertyExpression::ObjectProperty(b.object_property(r)),
        bce: Box::new(ClassExpression::Class(b.class(c))),
    };
    let mut o = SetOntology::new_rc();
    for n in ["r","s"] { o.insert(DeclareObjectProperty(b.object_property(&format!("http://ex/{n}")))); }
    for n in ["A","B"] { o.insert(DeclareClass(b.class(&format!("http://ex/{n}")))); }
    o.insert(DisjointClasses(vec![some("http://ex/r","http://ex/A"), some("http://ex/s","http://ex/B")]));
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let s = String::from_utf8(buf.clone()).unwrap();
    assert!(s.contains("DisjointClasses:"), "expected misc DisjointClasses:, got:\n{s}");
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "misc DisjointClasses did not round-trip\n{s}");
}
```

- [ ] **Step 2: Run → FAIL** — currently this `DisjointClasses` (complex members) goes to the functional-syntax misc block, which the reader skip-and-warns.

- [ ] **Step 3: Grammar — the `misc` production at document level.**
```pest
ManchesterDocument = { SOI ~ PrefixDeclaration* ~ OntologyHeader? ~ ( Frame | Misc )* ~ GeneralAxiomBlock? ~ EOI }

Misc = {
      ^"EquivalentClasses:"    ~ Annotations? ~ DescriptionList
    | ^"DisjointClasses:"      ~ Annotations? ~ DescriptionList
    | ^"EquivalentProperties:" ~ Annotations? ~ OpeList
    | ^"DisjointProperties:"   ~ Annotations? ~ OpeList
    | ^"SameIndividual:"       ~ Annotations? ~ IndividualList
    | ^"DifferentIndividuals:" ~ Annotations? ~ IndividualList
}
```
Notes: reuse the existing `DescriptionList`/`OpeList`/`IndividualList`/`Annotations` rules. `EquivalentProperties:`/`DisjointProperties:` are ambiguous between object and data properties in Manchester — parse the list as `OpeList` (objects); a data-property equivalence/disjointness whose members are bare IRIs will be read as object-property axioms (document this as the same object-vs-data ambiguity as elsewhere; the writer only emits the object form here, so round-trip holds for object properties). `Misc` is added as an alternative to `Frame` in the document body.

- [ ] **Step 4: Reader — parse `Misc` into the n-ary axioms.** In `mod.rs`'s document loop, add `Rule::Misc => from_pair::insert_misc(child, &ctx, &mut ontology)?`. Add `insert_misc` in `from_pair.rs`:
```rust
pub(crate) fn insert_misc<A: ForIRI, O: MutableOntology<A>>(
    misc: Pair<Rule>, ctx: &Context<'_, A>, ont: &mut O,
) -> Result<()> {
    let kw = clause_keyword(&misc); // reads the leading keyword off the span
    let mut it = misc.into_inner();
    let mut first = it.next().unwrap();
    let mut ann: BTreeSet<Annotation<A>> = BTreeSet::new();
    if first.as_rule() == Rule::Annotations {
        ann = parse_annotations(first, ctx)?.into_iter().collect();
        first = it.next().unwrap();
    }
    let body = first; // DescriptionList | OpeList | IndividualList
    let component = match kw.as_str() {
        "equivalentclasses" => Component::EquivalentClasses(EquivalentClasses(parse_description_list(body, ctx)?)),
        "disjointclasses"   => Component::DisjointClasses(DisjointClasses(parse_description_list(body, ctx)?)),
        "equivalentproperties" => Component::EquivalentObjectProperties(EquivalentObjectProperties(parse_ope_list(body, ctx)?)),
        "disjointproperties"    => Component::DisjointObjectProperties(DisjointObjectProperties(parse_ope_list(body, ctx)?)),
        "sameindividual"        => Component::SameIndividual(SameIndividual(parse_individual_list(body, ctx)?)),
        "differentindividuals"  => Component::DifferentIndividuals(DifferentIndividuals(parse_individual_list(body, ctx)?)),
        other => unreachable!("unexpected misc keyword: {other}"),
    };
    ont.insert(AnnotatedComponent { component, ann });
    Ok(())
}
```
Add a `parse_individual_list` helper if absent (mirror `parse_ope_list`: `list.into_inner().map(|p| Individual::from_pair(p, ctx)).collect()`). Confirm the `Component::*` and axiom wrapper names against model.rs (`EquivalentClasses(Vec<…>)` etc.).

- [ ] **Step 5: Writer — emit `misc` for non-frameable n-ary axioms.** In `mod.rs`, the arms for `EquivalentClasses`/`DisjointClasses`/`EquivalentObjectProperties`/`DisjointObjectProperties`/`SameIndividual`/`DifferentIndividuals` currently `misc.push(functional)` when the first member is not a named class/property/individual. Replace those `misc.push(...)` branches with pushing a *Manchester* misc line into a new `misc_axioms: Vec<String>` collected separately, e.g. `format!("DisjointClasses: {}", members.iter().map(render).collect::<Vec<_>>().join(", "))`. Emit these lines (each on its own, top-level, after the frames) — they are valid Manchester `Misc`. Keep the functional-syntax `# General axioms` fallback ONLY for the genuinely-inexpressible remainder (SWRL rules, complex-LHS `SubClassOf`, anon-subject assertions). Confirm the reader's `GeneralAxiomBlock` still only triggers on the `# General axioms` marker (the misc lines have no marker and parse as `Misc`).

- [ ] **Step 6: Run → PASS; confirm prior tests green (esp. the frame-routed n-ary cases still go to frames); fmt/clippy; commit.**
```bash
git add src/grammars/omn.pest src/io/omn/reader/from_pair.rs src/io/omn/reader/mod.rs src/io/omn/writer/mod.rs
git commit -m "feat(omn): top-level Misc axioms (EquivalentClasses/DisjointClasses/SameIndividual/...) round-trip

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Full per-item `annotatedList` annotations

§2.5 allows `[Annotations:] item` per element of a clause's list (`descriptionAnnotatedList`). We currently support a single leading `Annotations?` (axiom annotation on the whole clause). Generalize so each list item can carry its own annotations, threaded into per-item `AnnotatedComponent`s.

**Files:** `src/grammars/omn.pest`, `src/io/omn/reader/from_pair.rs`, `src/io/omn/writer/mod.rs`, test in `from_pair.rs`.

- [ ] **Step 1: Write the failing round-trip test** — two `SubClassOf`s on one class, one annotated, expressed as an annotatedList `SubClassOf: B, Annotations: ex:p "x" C`:
```rust
#[test]
fn reads_per_item_annotated_list_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::collections::BTreeSet;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    let mut o = SetOntology::new_rc();
    for n in ["A","B","C"] { o.insert(DeclareClass(b.class(&format!("http://ex/{n}")))); }
    o.insert(SubClassOf { sub: ClassExpression::Class(b.class("http://ex/A")), sup: ClassExpression::Class(b.class("http://ex/B")) });
    let mut ann = BTreeSet::new();
    ann.insert(Annotation { ap: b.annotation_property("http://ex/p"),
        av: AnnotationValue::Literal(Literal::Simple { literal: "x".into() }) });
    o.insert(AnnotatedComponent {
        component: Component::SubClassOf(SubClassOf {
            sub: ClassExpression::Class(b.class("http://ex/A")),
            sup: ClassExpression::Class(b.class("http://ex/C")) }),
        ann,
    });
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: BTreeSet<_> = o.iter().cloned().collect();
    let got: BTreeSet<_> = parsed.iter().cloned().collect();
    assert_eq!(orig, got, "per-item annotated list did not round-trip\n{}", String::from_utf8_lossy(&buf));
}
```

- [ ] **Step 2: Run → FAIL** — the reader's list parsing attaches at most a single leading annotation to all items; the writer emits one clause per axiom (so this currently round-trips as two separate `SubClassOf:` clauses — confirm whether it already passes; if the writer already emits one-clause-per-axiom and the reader handles a leading `Annotations?` per clause, this test may PASS without change, in which case Task 5 reduces to the *reader* accepting a comma-list with per-item annotations from external Manchester — adjust the test to a single clause with a comma-list `SubClassOf: B, Annotations: ex:p "x" C` built by hand as a string and parsed via `parse_class_expression`-style or a raw `read` of that text, asserting two SubClassOf axioms with the right annotations).

- [ ] **Step 3: Grammar — per-item annotations in the list rules.** Generalize the shared list rules to the §2.5 annotatedList shape:
```pest
DescriptionList = { Annotations? ~ Description ~ ( "," ~ Annotations? ~ Description )* }
OpeList         = { Annotations? ~ ope         ~ ( "," ~ Annotations? ~ ope )* }
IriList         = { Annotations? ~ IRI         ~ ( "," ~ Annotations? ~ IRI )* }
IndividualList  = { Annotations? ~ Individual  ~ ( "," ~ Annotations? ~ Individual )* }
```
(Each item may be preceded by its own `Annotations:`. The existing single leading `Annotations?` on the clause keyword stays; this adds per-item annotations inside the list.)

- [ ] **Step 4: Reader — thread per-item annotations.** Generalize the list-parsing helpers (`parse_description_list`, `parse_ope_list`, etc.) to return `Vec<(BTreeSet<Annotation>, Item)>` pairs (item + its preceding annotations), and update the clause handlers (`insert_class_frame` etc.) so each per-item axiom is inserted as `AnnotatedComponent { component, ann: <clause-level ann> ∪ <item ann> }`. For n-ary clauses (`EquivalentTo:` etc.) where one axiom is built from the whole list, merge all per-item annotations into the single axiom's `ann` (§2.5 semantics: per-item annotations on an n-ary list annotate the axiom). Keep behavior identical when no per-item annotations are present (the common case). This is the broadest change — apply it uniformly across the per-item-axiom clauses (SubClassOf/Domain/Range/Types/Facts/SubPropertyOf/Characteristics) and the n-ary clauses.

- [ ] **Step 5: Writer.** The writer already emits one clause per per-item axiom with the clause-level `Annotations:` prefix (Task 6 of the earlier P3 work), so the common case round-trips. No writer change is required for the per-item form unless Step 2 reveals a gap; if it does, emit each annotated list item with its own leading `Annotations:` inside the comma-list. Verify against the test.

- [ ] **Step 6: Run → PASS; confirm all prior annotation tests still green; fmt/clippy; commit.**
```bash
git add src/grammars/omn.pest src/io/omn/reader/from_pair.rs src/io/omn/writer/mod.rs
git commit -m "feat(omn): full per-item annotatedList annotations (§2.5)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Semantic-conformance gate (OWL-API oracle) + Protégé input + docs

Validate the general reader against externally-produced Manchester, with OWL-API as the oracle.

**Files:** a test harness script + an integration test or a documented manual gate; `src/io/omn/reader/mod.rs` module doc; the validation doc.

- [ ] **Step 1: Build the conformance oracle (shell, run from the fork).** For each corpus ontology `X` (use the rustdl corpus at `/data/dumontier/rustdl/ontologies/real`: `pizza`, `family`, `sio-fp-module`, `go-basic`, `ro`, plus `bibtex`/`anch-module`/`asp-module`/`np-module`/`sio-450-module`):
  1. `robot convert --input X.ofn --output X.owlapi.omn` (OWL-API emits Manchester).
  2. Parse `X.owlapi.omn` with our reader (the `/tmp/omn-render` `omnread` harness, or `cargo run`), and record OK/parse-error.
  3. For the OK ones, compare axiom counts/sets against the source `X.ofn` parsed by our own ofn reader (use `omnread` extended to print a component-kind histogram; a structural mismatch beyond the documented declaration-conflation is a finding).
  Record a table: per ontology, `our-reader-on-OWLAPI-omn = OK/FAIL(construct)`.

- [ ] **Step 2: Add a Protégé-saved `.omn`.** If a Protégé Manchester export is available (ask the user, or save pizza via Protégé), parse it with our reader and record the result. If none is available, note it and rely on the OWL-API oracle (OWL-API is the reference Protégé uses).

- [ ] **Step 3: Add a fork integration test** that locks the gate for a representative external file: check in a small OWL-API-emitted `.omn` (e.g. a converted pizza fragment, or generate one in the test via a committed fixture) under `src/io/omn/tests/` or as a `#[test]` that parses a hand-written §2.5 document exercising version IRI + a misc axiom + a datatype definition + a compound data range + a per-item annotation, asserting the axiom set. Name it `parses_general_manchester_document`.

- [ ] **Step 4: Update the reader module doc** (`src/io/omn/reader/mod.rs`): change the "round-trip-scoped, not a general parser" caveat to state the reader now targets full §2.5 (version IRI, misc axioms, datatype definitions, full data ranges, per-item annotatedList, bare names), and list the residual genuine non-§2.5 constructs it still cannot represent (SWRL rules, complex-LHS general class axioms — inherent Manchester limits).

- [ ] **Step 5: Update `docs/superpowers/manchester-validation-2026-06-13.md`** (in the rustdl repo) with a "General reader (§2.5)" section recording the OWL-API-oracle results.

- [ ] **Step 6: Full verification + commit.** `cargo test --lib io::omn 2>&1 | tail -3` (all green), `cargo build -p horned-owl`, fmt/clippy, and `cd /data/dumontier/rustdl && cargo build -p owl-dl-reasoner 2>&1 | tail -2`.
```bash
git add -A
git commit -m "test(omn): general §2.5 reader conformance gate (OWL-API oracle) + docs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Pre-upstream-PR follow-ups (out of scope, documented)

- The reader still cannot represent constructs outside §2.5 (SWRL `Rule:`, complex-LHS general class axioms) — inherent Manchester limits; these stay in the writer's `# General axioms` fallback and are skip-and-warned on read.
- `EquivalentProperties:`/`DisjointProperties:` object-vs-data ambiguity in the `Misc` reader (parsed as object) — documented limitation, mirrors the existing object-vs-data restriction ambiguity.

---

## Self-Review notes

- **Spec coverage:** version IRI (T1), full data ranges (T2), datatype definitions (T3), misc axioms (T4), per-item annotatedList (T5), conformance gate (T6) — covers every §2.5 gap the reader validation enumerated. Bare names already done (prior work).
- **Writer-already-done:** data-range *rendering* and the n-ary-axiom *frame routing* exist; T2 is reader-only and T4 only swaps the writer's misc functional-syntax push for a Manchester `Misc` line.
- **Type/name verification flagged inline** for the implementer: `ComponentKind::DatatypeDefinition`, the `Component::*`/axiom wrapper names for misc axioms, `OntologyID.viri`, `DatatypeDefinition { kind, range }`, `FacetRestriction { f, l }` — all to confirm against `src/model.rs` (the snippets above use the verified shapes).
- **T5 caveat:** Step 2 explicitly checks whether the common case already round-trips (it may), narrowing T5 to the reader accepting external per-item annotatedLists — the test is written to force the real gap.
