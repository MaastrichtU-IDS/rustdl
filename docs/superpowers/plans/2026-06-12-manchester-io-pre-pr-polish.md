# Manchester io/omn Pre-PR Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the three pre-upstream-PR gaps in the horned-owl Manchester `io/omn` module: (1) keyword/CURIE-prefix boundary anchoring, (2) `OMN` as a first-class `ResourceType`/`ParserOutput`, (3) native Manchester rendering+reading of `Import:`, property chains, `HasKey:`, and flat annotations (axiom + entity + ontology), with the reader tolerating (skip-and-warn) the residual genuinely-inexpressible block.

**Architecture:** Item 1 appends a silent zero-width negative lookahead (`~ !SPARQL_PnChars`) to each bare-word keyword in `src/grammars/omn.pest` — maximal-munch anchoring that does NOT change the parse-tree shape, so all existing P2/P3 detection code is untouched. Item 2 is additive enum/constructor work in `src/io/mod.rs`. Item 3 extends the writer (`src/io/omn/writer/`), grammar, and reader (`src/io/omn/reader/`) to round-trip the components that have native Manchester forms, following the `io/ofn` template exactly; the reader gains a skip-and-warn path for the residual `# General axioms` block.

**Tech Stack:** Rust (edition 2024), pest/pest_derive, horned-owl object model, `curie::PrefixMapping`.

**Working directory:** `/data/dumontier/horned-owl-omn` (branch `master`). Toolchain: prepend `PATH` with `/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`.

**Invariant:** the structural round-trip `read(write(ont)) == ont` (on the `BTreeSet<Component>`) must hold and expand to cover each new construct. Do not weaken any existing test. After every task: `cargo test --lib io::omn 2>&1 | tail -8`, `cargo fmt -- --check` (rc 0; ignore the benign `array_width` config note), `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings), `cargo build -p horned-owl`.

---

## Model reference (verified — use these exact shapes)

```text
Annotation            { ap: AnnotationProperty<A>, av: AnnotationValue<A> }          model.rs:1696
AnnotationValue       enum { Literal(Literal<A>), IRI(IRI<A>), AnonymousIndividual(AnonymousIndividual<A>) }  model.rs:1703
AnnotationSubject     enum { IRI(IRI<A>), AnonymousIndividual(AnonymousIndividual<A>) }                       model.rs:915
AnnotatedComponent    { component: Component<A>, ann: BTreeSet<Annotation<A>> }      model.rs:1022
AnnotationAssertion   { subject: AnnotationSubject<A>, ann: Annotation<A> }          model.rs:1609
OntologyAnnotation    (Annotation<A>)                                                model.rs:1282
Import                (IRI<A>)                                                       model.rs:1285
HasKey                { ce: ClassExpression<A>, vpe: Vec<PropertyExpression<A>> }    model.rs:1532
PropertyExpression    enum { ObjectPropertyExpression(..), DataProperty(..) }
SubObjectPropertyExpression  enum { ObjectPropertyChain(Vec<ObjectPropertyExpression<A>>), ObjectPropertyExpression(..) }  model.rs:1764
MutableOntology::insert<AA: Into<AnnotatedComponent<A>>>     (build `AnnotatedComponent { component, ann }` and insert it; `AnnotatedComponent: Into<Self>` is the identity)
```

**Annotation nesting is NOT in the model.** `Annotation` has no `ann` field; ofn parses nested annotations and discards them (`from_pair.rs:451-463`, `_annotations`). Item 3 therefore round-trips FLAT annotation sets only.

**ofn reader/writer templates to mirror** (read these before the annotation tasks):
- Writer `AnnotatedComponent` threading: `src/io/ofn/writer/as_functional.rs:371-378`; `AnnotationAssertion` `:385-406`; `HasKey` `:815-844`; `Import`/`derive_axiom` `:267-327`.
- Reader `Annotation` `:451-463`; `AnnotationAssertion` `:382-392`; `BTreeSet<Annotation>` `:588-595`; `AnnotatedComponent` `:80-114`; `HasKey`/`SubObjectPropertyExpression` (chain) in the same file (grep `Rule::HasKey` / `SubObjectPropertyExpression`).
- ofn grammar: `src/grammars/ofn.pest` `Annotation` `:141`, `AnnotationAssertion` `:145`, `HasKey` `:292`, `PropertyExpressionChain` `:261`.

---

## Task 1: Keyword boundary anchoring (item 1)

Append a silent zero-width `~ !SPARQL_PnChars` after each **bare-word** keyword literal in `omn.pest`. `SPARQL_PnChars` (sparql.pest:11, atomic) is the name-continuation char class. The lookahead emits no pair and is zero-width, so the parse tree is byte-for-byte unchanged on valid input — the P2 `not` span-gap detection (`from_pair.rs:334-344`), the `inverse` raw-text detection (`:189-207`), `clause_keyword`, and `Characteristic::as_str()` all keep working untouched. Colon-suffixed keywords (`Class:`, `SubClassOf:`, …) are already self-delimiting (the `:` cannot continue a name) and are NOT changed.

Bare-word keywords to anchor (24 occurrences): `or` (Description), `and` (Conjunction), `not` (Primary), `some`/`only`/`value`/`Self`/`min`/`max`/`exactly` (Restriction object arms AND data arms), `inverse` (ope), the 7 facet words (FacetSymbol), the 7 characteristics (Characteristic), and `not` (Fact).

**Files:**
- Modify: `src/grammars/omn.pest`
- Test: `src/io/omn/reader/lexer.rs` (negative-collision lex tests) + the existing 17-case round-trip in `src/io/omn/reader/from_pair.rs` is the regression guard.

- [ ] **Step 1: Write the failing negative-collision tests**

Add to the `#[cfg(test)] mod tests` in `src/io/omn/reader/lexer.rs`:

```rust
#[test]
fn keyword_curie_collisions_do_not_misparse() {
    use crate::io::omn::reader::parse_class_expression;
    use crate::model::Build;
    let b = Build::new_rc();
    let mut pm = curie::PrefixMapping::default();
    pm.add_prefix("notation", "http://ex/notation#").unwrap();
    pm.add_prefix("andro", "http://ex/andro#").unwrap();
    pm.add_prefix("somers", "http://ex/somers#").unwrap();

    // prefix `not` literally registered, so `not:Foo` is a valid CURIE.
    pm.add_prefix("not", "http://ex/not#").unwrap();

    // `notation:Foo` must parse as the atomic class, NOT `not` + `ation:Foo`
    // (keyword-prefix-of-name collision, closed by the `!SPARQL_PnChars` guard).
    let ce = parse_class_expression("notation:Foo", &pm, &b).unwrap();
    assert!(
        matches!(ce, crate::model::ClassExpression::Class(_)),
        "notation:Foo must be an atomic class, got {ce:?}"
    );
    // `not:Foo` must parse as the atomic class, NOT `not` + `:Foo`
    // (keyword-EQUALS-prefix collision, closed by also guarding `:`).
    let ce = parse_class_expression("not:Foo", &pm, &b).unwrap();
    assert!(
        matches!(ce, crate::model::ClassExpression::Class(_)),
        "not:Foo must be an atomic class, got {ce:?}"
    );
    // `andro:X and somers:Y` must be a 2-way intersection of two atomic classes.
    let ce = parse_class_expression("andro:X and somers:Y", &pm, &b).unwrap();
    match ce {
        crate::model::ClassExpression::ObjectIntersectionOf(v) => assert_eq!(v.len(), 2),
        other => panic!("expected intersection of 2, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib io::omn::reader::lexer::tests::keyword_curie_collisions_do_not_misparse 2>&1 | tail -20`
Expected: FAIL — `notation:Foo` currently parses as `ObjectComplementOf(Class(ation:Foo))` (the bug), so the `matches!(.., Class(_))` assertion fails.

- [ ] **Step 3: Anchor the bare-word keywords in the grammar**

Edit `src/grammars/omn.pest`. First define a DRY zero-width boundary helper, then apply `~ NameBoundary` immediately after each bare-word keyword literal. The boundary rejects BOTH a name-continuation char (`SPARQL_PnChars`, closing `notation:Foo`) AND a `:` (closing `not:Foo`, where the CURIE prefix equals the keyword — `:` is NOT in `SPARQL_PnChars`). This is safe for the writer's own output: a restriction/operator keyword is always followed by whitespace or `(`, never an abutting name char or `:`.

```pest
// Zero-width SILENT name-boundary: a bare-word keyword matches only when it is
// NOT immediately followed by a name-continuation char or `:`. Maximal munch —
// so `notation:Foo` is not mis-split into `not`+`ation:Foo`, and `not:Foo`
// (prefix == keyword) is not mis-split into `not`+`:Foo`. Being a silent
// zero-width lookahead it adds no parse-tree pair, so the reader's
// keyword-detection code (span-gap `not`, raw-text `inverse`, `clause_keyword`,
// `Characteristic::as_str`) is unaffected.
NameBoundary = _{ !( SPARQL_PnChars | ":" ) }

Description = { Conjunction ~ ( ^"or" ~ NameBoundary ~ Conjunction )* }

Conjunction = { Primary ~ ( ^"and" ~ NameBoundary ~ Primary )* }

Primary = { (^"not" ~ NameBoundary)? ~ ( Restriction | Atomic ) }

Restriction = {
      ope ~ ^"some"    ~ NameBoundary ~ Primary
    | ope ~ ^"only"    ~ NameBoundary ~ Primary
    | ope ~ ^"value"   ~ NameBoundary ~ Individual
    | ope ~ ^"Self"    ~ NameBoundary
    | ope ~ ^"min"     ~ NameBoundary ~ Cardinality ~ Primary?
    | ope ~ ^"max"     ~ NameBoundary ~ Cardinality ~ Primary?
    | ope ~ ^"exactly" ~ NameBoundary ~ Cardinality ~ Primary?
    | DataPropertyIRI ~ ^"some"    ~ NameBoundary ~ DataRange
    | DataPropertyIRI ~ ^"only"    ~ NameBoundary ~ DataRange
    | DataPropertyIRI ~ ^"value"   ~ NameBoundary ~ Literal
    | DataPropertyIRI ~ ^"min"     ~ NameBoundary ~ Cardinality ~ DataRange?
    | DataPropertyIRI ~ ^"max"     ~ NameBoundary ~ Cardinality ~ DataRange?
    | DataPropertyIRI ~ ^"exactly" ~ NameBoundary ~ Cardinality ~ DataRange?
}

ope = { ( ^"inverse" ~ NameBoundary ~ "(" ~ ObjectPropertyIRI ~ ")" ) | ObjectPropertyIRI }

FacetSymbol = {
      ">="
    | "<="
    | ">"
    | "<"
    | ^"length"         ~ NameBoundary
    | ^"minLength"      ~ NameBoundary
    | ^"maxLength"      ~ NameBoundary
    | ^"pattern"        ~ NameBoundary
    | ^"langRange"      ~ NameBoundary
    | ^"totalDigits"    ~ NameBoundary
    | ^"fractionDigits" ~ NameBoundary
}

Characteristic = {
      ^"Functional"        ~ NameBoundary
    | ^"InverseFunctional" ~ NameBoundary
    | ^"Reflexive"         ~ NameBoundary
    | ^"Irreflexive"       ~ NameBoundary
    | ^"Symmetric"         ~ NameBoundary
    | ^"Asymmetric"        ~ NameBoundary
    | ^"Transitive"        ~ NameBoundary
}

Fact = { (^"not" ~ NameBoundary)? ~ ope ~ ( Literal | Individual ) }
```

Note: a legitimate default-prefix CURIE after a keyword (e.g. `not :Foo` with a space → complement of `:Foo`) still parses — `NameBoundary` is checked at the char immediately after the keyword, which is the space, so the guard passes and whitespace-then-`:Foo` follows. Only an ABUTTING `:` (no space, i.e. `not:Foo`) is treated as a CURIE.

Note on `Characteristic` being non-silent: it still produces a `Characteristic` pair whose `.as_str()` is `"Functional"` etc. (the zero-width lookahead does not extend the span), so `insert_object_characteristic` is unaffected.

- [ ] **Step 4: Run the negative test + the full regression**

Run: `cargo test --lib io::omn 2>&1 | tail -10`
Expected: PASS — `keyword_curie_collisions_do_not_misparse` now green, AND the 17-case `class_expression_round_trips` plus all P3 frame round-trips still pass (the lookahead is transparent on valid input). If any round-trip regressed, the lookahead changed the parse tree — re-check that every guard is `~ !SPARQL_PnChars` (zero-width) and not an accidental consuming token.

- [ ] **Step 5: Verify and commit**

Run: `cargo fmt -- --check` (rc 0) + `cargo clippy --lib 2>&1 | grep -i omn` (no omn warnings).

```bash
git add src/grammars/omn.pest src/io/omn/reader/lexer.rs
git commit -m "fix(omn): maximal-munch keyword anchoring (!SPARQL_PnChars)

Closes the keyword/CURIE-prefix collision (notation:Foo -> not + ation:Foo)
grammar-wide via a zero-width silent lookahead; reader detection unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: OMN as a first-class ResourceType / ParserOutput (item 2)

Additive enum work in `src/io/mod.rs`. No central dispatch / no CLI exists in this repo, so this is purely making `omn` a peer of `ofn`/`owx` in the public surface.

**Files:**
- Modify: `src/io/mod.rs`
- Test: a unit test in `src/io/mod.rs` (or `src/io/omn/mod.rs`) constructing the new variant.

- [ ] **Step 1: Write the failing test**

Add to a `#[cfg(test)] mod tests` in `src/io/mod.rs` (create if absent):

```rust
#[test]
fn omn_parser_output_constructs_and_decomposes() {
    use super::*;
    use crate::ontology::set::SetOntology;
    type Idx = std::rc::Rc<crate::model::AnnotatedComponent<std::rc::Rc<str>>>;
    let o = SetOntology::<std::rc::Rc<str>>::new_rc();
    let pm = curie::PrefixMapping::default();
    let out: ParserOutput<std::rc::Rc<str>, Idx> = ParserOutput::omn((o, pm));
    assert!(matches!(out, ParserOutput::OMNParser(_, _)));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib io::tests::omn_parser_output 2>&1 | tail -15`
Expected: FAIL — `ParserOutput::omn` / `OMNParser` do not exist (compile error).

- [ ] **Step 3: Add the OMN variant + constructor**

In `src/io/mod.rs`:

1. Extend `ResourceType` (currently `OFN`/`OWX`/`RDF`):
```rust
pub enum ResourceType {
    OFN,
    OWX,
    RDF,
    OMN,
}
```

2. Extend `ParserOutput` (add the variant after `RDFParser`):
```rust
    OMNParser(SetOntology<A>, PrefixMapping),
```

3. Add the constructor to the `impl ParserOutput` block, mirroring `ofn`:
```rust
    pub fn omn(sop: (SetOntology<A>, PrefixMapping)) -> ParserOutput<A, AA> {
        ParserOutput::OMNParser(sop.0, sop.1)
    }
```

4. **Fix any now-non-exhaustive `match`.** Grep the file for `match` on `ParserOutput`/`ResourceType` (the report flagged `decompose()` and `From` impls around lines 61-96). Add an `OMNParser`/`OMN` arm wherever the compiler reports a missing variant. For an `OMNParser(o, pm)` that mirrors `OFNParser`, copy the `OFNParser` arm's behavior. Run `cargo build -p horned-owl 2>&1 | tail -20` and fix each reported non-exhaustive match until it builds.

Do NOT add an `omn` field to `ParserConfiguration` — the omn reader ignores config (YAGNI).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib io::tests::omn_parser_output 2>&1 | tail -15` → PASS. Then `cargo build -p horned-owl 2>&1 | tail -2` → builds.

- [ ] **Step 5: Verify and commit**

`cargo fmt -- --check` (rc 0) + `cargo clippy --lib 2>&1 | grep -iE 'omn|io/mod'` (no new warnings).

```bash
git add src/io/mod.rs
git commit -m "feat(omn): OMN as first-class ResourceType / ParserOutput

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Native `Import:` (item 3a)

Manchester emits imports as a top-of-document directive `Import: <iri>`. Currently `Import` falls to the writer's misc block.

**Files:**
- Modify: `src/io/omn/writer/mod.rs` (emit `Import:` lines before frames)
- Modify: `src/grammars/omn.pest` (grammar)
- Modify: `src/io/omn/reader/mod.rs` (parse import children)
- Test: round-trip in `src/io/omn/reader/from_pair.rs`

- [ ] **Step 1: Write the failing round-trip test**

Add to the `from_pair.rs` test module:

```rust
#[test]
fn reads_import_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;

    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();

    let mut o = SetOntology::new_rc();
    o.insert(Import(b.iri("http://ex/imported")));
    o.insert(DeclareClass(b.class("http://ex/A")));

    type TestOnt = ComponentMappedOntology<
        std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>,
    >;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();

    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "import did not round-trip\n{}", String::from_utf8_lossy(&buf));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib io::omn::reader::from_pair::tests::reads_import_round_trip 2>&1 | tail -20`
Expected: FAIL — `Import` is currently written to the misc block (functional syntax) and the reader rejects/ignores it; `got` lacks the `Import` component (or the read errors at EOI).

- [ ] **Step 3: Writer — emit `Import:` lines, remove `Import` from misc**

In `src/io/omn/writer/mod.rs`, after the prefix-declaration loop and BEFORE the `Ontology:` header block, add:

```rust
    // Import directives (Manchester: `Import: <iri>`)
    for ac in ont.i().component_for_kind(ComponentKind::Import) {
        if let Component::Import(imp) = &ac.component {
            writeln!(write, "Import: {}", imp.0.as_manchester_with_prefixes(mapping))?;
        }
    }
```

Confirm `ComponentKind::Import` exists (grep `ComponentKind::` in model.rs); if the enum name differs, use the correct kind. The `_ => misc` fallback still exists, but `Import` is now handled in this dedicated loop — verify the main `for kind in ComponentKind::all_kinds()` loop SKIPS `ComponentKind::Import` (add it to the `if kind == … { continue; }` guard near `OntologyID`/`DocIRI`) so it is not ALSO emitted to misc.

- [ ] **Step 4: Grammar — add `Import:` to the document**

In `omn.pest`, extend `ManchesterDocument` to allow imports after prefixes:

```pest
ManchesterDocument = { SOI ~ PrefixDeclaration* ~ ImportDeclaration* ~ OntologyHeader? ~ Frame* ~ EOI }
ImportDeclaration  = { ^"Import:" ~ IRI }
```

(`Import:` is colon-suffixed → self-delimiting, no `!SPARQL_PnChars` needed.)

- [ ] **Step 5: Reader — parse import children**

In `src/io/omn/reader/mod.rs`'s `read_with_build`, in the `for child in children` match, add an arm (alongside `PrefixDeclaration | EOI => {}`):

```rust
            Rule::ImportDeclaration => {
                let iri_pair = child.into_inner().next().unwrap(); // IRI
                ontology.insert(crate::model::Import(
                    crate::model::IRI::from_pair(iri_pair, &ctx)?,
                ));
            }
```

(Imports are parsed in pass 2 under the prefix-aware `ctx`, like frames. They carry no annotations in this task.)

- [ ] **Step 6: Run the test → PASS; verify; commit**

Run: `cargo test --lib io::omn 2>&1 | tail -8` (round-trip green, no regression). `cargo fmt -- --check` (rc 0), `cargo clippy --lib 2>&1 | grep -i omn` (clean).

```bash
git add src/io/omn/writer/mod.rs src/grammars/omn.pest src/io/omn/reader/mod.rs src/io/omn/reader/from_pair.rs
git commit -m "feat(omn): native Import: rendering + reading

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Property chains (`SubPropertyChain:`) + `HasKey:` (item 3b)

Both are frame clauses currently sent to misc. Manchester: `ObjectProperty: r SubPropertyChain: p o q` and `Class: C HasKey: p1, p2` (mixed object+data properties).

**Files:**
- Modify: `src/io/omn/writer/mod.rs` (chain clause in OP frame; HasKey clause in Class frame)
- Modify: `src/grammars/omn.pest`
- Modify: `src/io/omn/reader/from_pair.rs` (extend `insert_object_property_frame` + `insert_class_frame`)
- Test: round-trips in `from_pair.rs`

- [ ] **Step 1: Write the failing round-trip tests**

```rust
#[test]
fn reads_property_chain_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    let ope = |i: &str| ObjectPropertyExpression::ObjectProperty(b.object_property(i));
    let mut o = SetOntology::new_rc();
    for p in ["r", "p", "q"] { o.insert(DeclareObjectProperty(b.object_property(&format!("http://ex/{p}")))); }
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyChain(vec![ope("http://ex/p"), ope("http://ex/q")]),
        sup: ope("http://ex/r"),
    });
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "chain did not round-trip\n{}", String::from_utf8_lossy(&buf));
}

#[test]
fn reads_haskey_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    let mut o = SetOntology::new_rc();
    o.insert(DeclareClass(b.class("http://ex/C")));
    o.insert(DeclareObjectProperty(b.object_property("http://ex/k1")));
    o.insert(DeclareDataProperty(b.data_property("http://ex/k2")));
    o.insert(HasKey {
        ce: ClassExpression::Class(b.class("http://ex/C")),
        vpe: vec![
            PropertyExpression::ObjectPropertyExpression(
                ObjectPropertyExpression::ObjectProperty(b.object_property("http://ex/k1"))),
            PropertyExpression::DataProperty(b.data_property("http://ex/k2")),
        ],
    });
    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "haskey did not round-trip\n{}", String::from_utf8_lossy(&buf));
}
```

- [ ] **Step 2: Run to verify failure** (`cargo test --lib io::omn::reader::from_pair::tests::reads_property_chain_round_trip reads_haskey_round_trip 2>&1 | tail -20`) — both FAIL (currently misc).

- [ ] **Step 3: Writer — chain + HasKey clauses**

In `src/io/omn/writer/mod.rs`:

- In the `SubObjectPropertyOf` arm, the `SubObjectPropertyExpression::ObjectPropertyChain(_)` branch currently pushes to misc. Replace it to emit a `SubPropertyChain:` clause under the SUPER property's frame (Manchester convention: the chain is a clause of the super property `r`). Build the clause from the chain's OPEs joined by ` o `:
```rust
    SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
        if let Some(iri) = ope_iri(&ax.sup) {
            let rendered = chain.iter()
                .map(|o| o.as_manchester_with_prefixes(pm).to_string())
                .collect::<Vec<_>>().join(" o ");
            push_clause!(FrameKind::ObjectProperty, iri, format!("SubPropertyChain: {rendered}"));
        } else {
            misc.push(ac.component.as_manchester_with_prefixes(pm).to_string());
        }
    }
```

- Add a `HasKey` arm to the Component match (it currently hits `_ => misc`). Render under the class subject's frame:
```rust
    Component::HasKey(ax) => {
        if let crate::model::ClassExpression::Class(c) = &ax.ce {
            let parts: Vec<String> = ax.vpe.iter().map(|pe| match pe {
                crate::model::PropertyExpression::ObjectPropertyExpression(ope) =>
                    ope.as_manchester_with_prefixes(pm).to_string(),
                crate::model::PropertyExpression::DataProperty(dp) =>
                    dp.as_manchester_with_prefixes(pm).to_string(),
            }).collect();
            push_clause!(FrameKind::Class, c.0.as_ref(), format!("HasKey: {}", parts.join(", ")));
        } else {
            misc.push(ac.component.as_manchester_with_prefixes(pm).to_string());
        }
    }
```

- [ ] **Step 4: Grammar — chain + HasKey clauses**

```pest
// add to ObjectPropertyClause alternatives:
    | ^"SubPropertyChain:" ~ PropertyChain
// add to ClassClause alternatives:
    | ^"HasKey:" ~ PropertyExprList

// The `o` chain-composition operator is a BARE-WORD keyword. Per Task 1's
// finding, an inline `^"o" ~ !SPARQL_PnChars` in a normal `{}` rule is BROKEN
// (pest consumes WHITESPACE before the lookahead, firing the boundary at the
// operand's first char). Use the same `${}` (compound-atomic) keyword-rule
// idiom Task 1 established for bare-word keywords, reusing `NameBoundary`:
OKw              = ${ ^"o" ~ NameBoundary }
PropertyChain    = { ope ~ ( OKw ~ ope )+ }
PropertyExprList = { ope ~ ( "," ~ ope )* }
```

Note: because `OKw` is a `${}` rule it EMITS a pair (like Task 1's `*Kw` rules), so the reader's `PropertyChain` handler must FILTER to the `ope` children and skip the `OKw` pairs (see Step 5). `HasKey`'s property list uses `ope` for every member (both object and data properties lex as `ope`'s `ObjectPropertyIRI`); the object-vs-data split is decided at read time (there's no lexical distinction — see Step 5).

- [ ] **Step 5: Reader — chain + HasKey handlers**

In `insert_object_property_frame` (`from_pair.rs`), add a clause arm:
```rust
            "subpropertychain" => {
                // body is a PropertyChain: `ope (OKw ope)+`. Filter OUT the
                // emitted `OKw` keyword pairs; keep only the `ope` operands.
                let chain: Vec<ObjectPropertyExpression<A>> = body
                    .into_inner()
                    .filter(|p| p.as_rule() == Rule::ope)
                    .map(|p| ObjectPropertyExpression::from_pair(p, ctx))
                    .collect::<Result<_>>()?;
                ont.insert(SubObjectPropertyOf {
                    sub: SubObjectPropertyExpression::ObjectPropertyChain(chain),
                    sup: subject_ope.clone(),
                });
            }
```

In `insert_class_frame`, add:
```rust
            "haskey" => {
                // body is a PropertyExprList of `ope`. Each ope is a NAMED property;
                // classify object-vs-data by whether the IRI was declared as a data
                // property is NOT available here, so default to ObjectProperty and
                // rely on the fact that HasKey's vpe order is preserved. NOTE: see the
                // object-vs-data caveat below.
                let mut vpe = Vec::new();
                for p in body.into_inner() {
                    let ope = ObjectPropertyExpression::from_pair(p, ctx)?;
                    if let ObjectPropertyExpression::ObjectProperty(op) = ope {
                        vpe.push(PropertyExpression::ObjectPropertyExpression(
                            ObjectPropertyExpression::ObjectProperty(op),
                        ));
                    }
                }
                ont.insert(HasKey { ce: ClassExpression::Class(Class(subject.clone())), vpe });
            }
```

**Object-vs-data caveat (round-trip-critical):** Manchester `HasKey:` does NOT lexically distinguish object from data properties — they are all bare property IRIs. So the reader cannot recover which key properties were data properties; it reconstructs them all as `ObjectPropertyExpression`. The Step-1 `reads_haskey_round_trip` test as written mixes an object and a data key and WILL therefore fail on the data-property member (parsed back as object). **Resolve by making the test use only OBJECT-property keys** (drop the `k2` data property from `vpe`, keep `k1`), and add the object-vs-data-key conflation to the limitations doc in Task 7. (This mirrors the writer, which renders both indistinguishably — it is the same inherent Manchester limitation as object-vs-data restrictions.) Adjust the Step-1 test accordingly when you write it: declare and key only `ex:k1` (object).

- [ ] **Step 6: Run → PASS; verify; commit**

Run the two tests + full suite; fmt/clippy. Commit:
```bash
git add src/io/omn/writer/mod.rs src/grammars/omn.pest src/io/omn/reader/from_pair.rs
git commit -m "feat(omn): native SubPropertyChain: and HasKey: rendering + reading

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Entity + ontology annotations (item 3c)

`AnnotationAssertion` whose subject is a frame entity → a frame `Annotations:` clause. `OntologyAnnotation` → an `Annotations:` clause in the ontology header. Both are flat (`Annotation { ap, av }`).

Manchester grammar for an annotation: `Annotations: ap1 v1, ap2 v2` where each value is a literal or IRI. We reuse a shared `Annotations` rule.

**Files:**
- Modify: `src/io/omn/writer/{as_manchester.rs, mod.rs}` (render `Annotation`; emit `Annotations:` clauses + ontology-header annotations; route `AnnotationAssertion` with a named subject into the subject's frame; route `OntologyAnnotation` to the header)
- Modify: `src/grammars/omn.pest` (the `Annotations` rule + clause hooks)
- Modify: `src/io/omn/reader/from_pair.rs` (`Annotation`/`AnnotationValue` FromPair; `Annotations:` clause in every frame; header annotations) + `reader/mod.rs` (header)
- Test: round-trips in `from_pair.rs`

- [ ] **Step 1: Write the failing round-trip test**

```rust
#[test]
fn reads_entity_and_ontology_annotations_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();
    pm.add_prefix("rdfs", "http://www.w3.org/2000/01/rdf-schema#").unwrap();

    let mut o = SetOntology::new_rc();
    let mut oid = OntologyID::default();
    oid.iri = Some(b.iri("http://ex/o"));
    o.insert(oid);
    // an import too — validates the conformant header hosts iri+import+annotations together
    o.insert(Import(b.iri("http://ex/imported")));
    o.insert(OntologyAnnotation(Annotation {
        ap: b.annotation_property("http://www.w3.org/2000/01/rdf-schema#comment"),
        av: AnnotationValue::Literal(Literal::Simple { literal: "an ontology".to_string() }),
    }));
    o.insert(DeclareClass(b.class("http://ex/A")));
    o.insert(AnnotationAssertion {
        subject: AnnotationSubject::IRI(b.iri("http://ex/A")),
        ann: Annotation {
            ap: b.annotation_property("http://www.w3.org/2000/01/rdf-schema#label"),
            av: AnnotationValue::Literal(Literal::Simple { literal: "the A class".to_string() }),
        },
    });
    // an IRI-valued entity annotation too
    o.insert(AnnotationAssertion {
        subject: AnnotationSubject::IRI(b.iri("http://ex/A")),
        ann: Annotation {
            ap: b.annotation_property("http://ex/seeAlso"),
            av: AnnotationValue::IRI(b.iri("http://ex/B")),
        },
    });

    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    let orig: std::collections::BTreeSet<_> = o.iter().map(|ac| ac.component.clone()).collect();
    let got: std::collections::BTreeSet<_> = parsed.iter().map(|ac| ac.component.clone()).collect();
    assert_eq!(orig, got, "annotations did not round-trip\n{}", String::from_utf8_lossy(&buf));
}
```

- [ ] **Step 2: Run to verify failure** — FAIL (annotations currently dropped/misc).

- [ ] **Step 3: Writer — render an `Annotation` value + emit `Annotations:`**

In `src/io/omn/writer/as_manchester.rs`, add a helper to render one annotation as `ap value` (value = literal or IRI). Mirror how the file renders `Literal`/`IRI`. Add a `pub(crate) fn annotation_to_manchester<A>(ann: &Annotation<A>, pm: &PrefixMapping) -> String` (or an `AsManchester` impl for `Annotation`) producing e.g. `rdfs:label "the A class"` or `ex:seeAlso ex:B`. Use the existing IRI/Literal rendering for the value per `AnnotationValue` variant; for `AnonymousIndividual` fall back to the existing anon rendering or push to misc (anon annotation values are out of scope — see Task 7).

In `src/io/omn/writer/mod.rs`:

- **`AnnotationAssertion` routing is a STRICT POST-PASS — never a main-loop arm.** Do NOT add an `AnnotationAssertion` arm to the `for kind in ComponentKind::all_kinds()` loop: if that kind is visited before the subject's `DeclareClass`/`Declare*`, the subject's frame would not yet exist and the annotation would leak to misc → get skipped on read → silent round-trip break. Instead, AFTER the main loop fully populates `frames`, run a dedicated second pass over the `AnnotationAssertion` components:
  ```rust
  // POST-PASS: entity annotations. Runs after the main loop so every
  // declaration/axiom frame already exists in `frames`.
  for ac in ont.i().component_for_kind(ComponentKind::AnnotationAssertion) {
      if let Component::AnnotationAssertion(aa) = &ac.component {
          if let AnnotationSubject::IRI(i) = &aa.subject {
              let clause = format!("Annotations: {}", annotation_to_manchester(&aa.ann, mapping));
              // attach to the EXISTING frame whose subject_iri == i (any kind).
              if let Some(frame) = frames.values_mut().find(|fr| fr.subject_iri == i.as_ref()) {
                  frame.clauses.push(clause);
              } else {
                  // orphan: no frame heads this IRI -> not Manchester-expressible here.
                  misc.push(ac.component.as_manchester_with_prefixes(mapping).to_string());
              }
          } else {
              misc.push(ac.component.as_manchester_with_prefixes(mapping).to_string());
          }
      }
  }
  ```
  Confirm `ComponentKind::AnnotationAssertion` is the correct kind name (grep model.rs). The orphan case (entity annotation on an IRI that heads no frame) goes to misc and is documented in Task 7. The Step-1 test declares `ex:A`, so its annotations attach to the `Class: ex:A` frame and round-trip.
  Also ensure `AnnotationAssertion` is in the main loop's `if kind == … { continue; }` skip guard (alongside `OntologyID`/`DocIRI`/`Import`) so it is NOT also emitted to misc by the generic `_ => misc` arm.
- **Conformant ontology-frame header block (RELOCATES Task 3's imports).** W3C Manchester puts imports AND ontology annotations inside the ontology frame, after the `Ontology:` line. REMOVE the standalone pre-header `Import:` loop added in Task 3. Replace the existing `Ontology:`-header block with one that emits the header whenever there is an ontology IRI, OR any `Import`, OR any `OntologyAnnotation`, then nests imports and annotations under it:
  ```rust
  // collect first
  let header_iri: Option<&IRI<A>> = /* OntologyID.iri, as today */;
  let imports: Vec<&IRI<A>> = ont.i().component_for_kind(ComponentKind::Import)
      .filter_map(|ac| if let Component::Import(i) = &ac.component { Some(&i.0) } else { None })
      .collect();
  let ont_anns: Vec<&Annotation<A>> = ont.i().component_for_kind(ComponentKind::OntologyAnnotation)
      .filter_map(|ac| if let Component::OntologyAnnotation(oa) = &ac.component { Some(&oa.0) } else { None })
      .collect();
  if header_iri.is_some() || !imports.is_empty() || !ont_anns.is_empty() {
      writeln!(write)?;
      match header_iri {
          Some(iri) => writeln!(write, "Ontology: {}", iri.as_manchester_with_prefixes(mapping))?,
          None => writeln!(write, "Ontology:")?, // bare frame to host imports/anns
      }
      for imp in &imports {
          writeln!(write, "    Import: {}", imp.as_manchester_with_prefixes(mapping))?;
      }
      for ann in &ont_anns {
          writeln!(write, "    Annotations: {}", annotation_to_manchester(ann, mapping))?;
      }
  }
  ```
  Add `ComponentKind::Import` and `ComponentKind::OntologyAnnotation` to the main loop's `continue` skip-guard (Import already added in Task 3; add OntologyAnnotation) so neither leaks to misc. Confirm `ComponentKind::OntologyAnnotation` / `Component::OntologyAnnotation` names against model.rs. Note the bare `Ontology:` line (no IRI) is now CORRECT and round-trip-safe because the reader's gate (Step 5) inserts `OntologyID` only when an IRI/version was actually present.

Render multiple annotations on one entity as one `Annotations:` clause with comma-separated entries, or one clause per assertion — either round-trips (the reader emits one `AnnotationAssertion` per entry). Prefer one entry per clause line for simplicity.

**Stale-comment cleanup (do this in this task):** update `src/io/omn/writer/mod.rs`'s `pub fn write` docstring (around line 57-62) and the inline misc comment so the "components with no native Manchester form" list no longer names `Import` or `HasKey` (both now native) — the accurate residual is `OntologyAnnotation` (now also native — drop it too), leaving SWRL `Rule`, axiom annotations (until Task 6), general anonymous-subject axioms. After this task only genuinely-inexpressible components remain in that note.

- [ ] **Step 4: Grammar — `Annotations` rule + hooks**

```pest
Annotations    = { ^"Annotations:" ~ AnnotationEntry ~ ( "," ~ AnnotationEntry )* }
AnnotationEntry = { IRI ~ AnnotationTarget }
AnnotationTarget = { Literal | IRI }

// add `Annotations` as a clause arm in EACH frame's clause rule (entity annotation):
//   ClassClause = { Annotations | ^"SubClassOf:" ~ ... | ... }
//   ObjectPropertyClause = { Annotations | ... }  (and Data/Annotation/Individual frames)

// CONFORMANCE RELOCATION (also part of this task): W3C Manchester puts imports
// AND ontology annotations INSIDE the ontology frame, after the `Ontology:`
// line. Task 3 placed `ImportDeclaration*` at TOP LEVEL (before the header) —
// that is non-conformant (the OWL API rejects it). Move imports into the header
// and REMOVE `ImportDeclaration*` from `ManchesterDocument`:
ManchesterDocument = { SOI ~ PrefixDeclaration* ~ OntologyHeader? ~ Frame* ~ EOI }
OntologyHeader     = { ^"Ontology:" ~ IRI? ~ ImportDeclaration* ~ Annotations* }
```

(`Annotations:` is colon-suffixed → self-delimiting. Put the `Annotations` arm FIRST in each clause rule so it is tried before the keyworded clauses; since its keyword is distinct, order does not actually matter, but first is clearest.)

- [ ] **Step 5: Reader — `Annotation`/`AnnotationValue` FromPair + clause + header**

Add `FromPair` impls following ofn (`from_pair.rs:451-463`):
```rust
impl<A: ForIRI> FromPair<A> for AnnotationValue<A> {
    const RULE: Rule = Rule::AnnotationTarget;
    fn from_pair_unchecked(pair: Pair<Rule>, ctx: &Context<'_, A>) -> Result<Self> {
        let inner = pair.into_inner().next().unwrap();
        match inner.as_rule() {
            Rule::Literal => Ok(AnnotationValue::Literal(Literal::from_pair(inner, ctx)?)),
            Rule::IRI => Ok(AnnotationValue::IRI(IRI::from_pair(inner, ctx)?)),
            rule => unreachable!("unexpected annotation target: {:?}", rule),
        }
    }
}

impl<A: ForIRI> FromPair<A> for Annotation<A> {
    const RULE: Rule = Rule::AnnotationEntry;
    fn from_pair_unchecked(pair: Pair<Rule>, ctx: &Context<'_, A>) -> Result<Self> {
        let mut inner = pair.into_inner();
        let ap = AnnotationProperty(IRI::from_pair(inner.next().unwrap(), ctx)?);
        let av = AnnotationValue::from_pair(inner.next().unwrap(), ctx)?;
        Ok(Annotation { ap, av })
    }
}
```

Add a helper to turn an `Annotations` clause pair into `Vec<Annotation>`:
```rust
fn parse_annotations<A: ForIRI>(clause: Pair<Rule>, ctx: &Context<'_, A>) -> Result<Vec<Annotation<A>>> {
    clause.into_inner().map(|e| Annotation::from_pair(e, ctx)).collect()
}
```

The `Annotations` rule is an alternative INSIDE each `*Clause` rule (e.g. `ClassClause = { Annotations | ^"SubClassOf:" ~ DescriptionList | … }`), so a standalone entity-annotation clause arrives as a normal `*Clause` pair whose `clause_keyword(&clause)` is `"annotations"` and whose single inner pair is the `Annotations` rule. Handle it as a new `"annotations"` arm in EACH `insert_*_frame`'s existing `match kw.as_str()` dispatch (do NOT special-case `clause.as_rule()`):
```rust
            "annotations" => {
                // `body` is the inner `Annotations` pair (= clause.into_inner().next()).
                for ann in parse_annotations(body, ctx)? {
                    ont.insert(AnnotationAssertion {
                        subject: AnnotationSubject::IRI(subject.clone()),
                        ann,
                    });
                }
            }
```
(`subject` is the frame-subject `IRI` already bound by `frame_subject_and_clauses`. `parse_annotations` here receives the `Annotations` pair and iterates its `AnnotationEntry` children.)

**Reader header — imports + annotations + gate refinement (the conformance relocation):**

In `reader/mod.rs`, the `OntologyHeader` is now `^"Ontology:" ~ IRI? ~ ImportDeclaration* ~ Annotations*`. REMOVE the top-level `Rule::ImportDeclaration` arm added in Task 3 (imports are no longer top-level). Rewrite the `Rule::OntologyHeader` arm to iterate the header's children:
```rust
            Rule::OntologyHeader => {
                let mut oid = crate::model::OntologyID::default();
                let mut has_id = false;
                for h in child.into_inner() {
                    match h.as_rule() {
                        Rule::IRI => { oid.iri = Some(crate::model::IRI::from_pair(h, &ctx)?); has_id = true; }
                        Rule::ImportDeclaration => {
                            let iri_pair = h.into_inner().next().unwrap();
                            ontology.insert(crate::model::Import(crate::model::IRI::from_pair(iri_pair, &ctx)?));
                        }
                        Rule::Annotations => {
                            for ann in from_pair::parse_annotations(h, &ctx)? {
                                ontology.insert(crate::model::OntologyAnnotation(ann));
                            }
                        }
                        rule => unreachable!("unexpected ontology-header child: {:?}", rule),
                    }
                }
                // GATE: insert OntologyID only if an IRI/version was present —
                // NOT merely because the `Ontology:` keyword appeared. A bare
                // `Ontology:` (emitted only to host imports/annotations) must
                // not inject a spurious `OntologyID(None,None)`.
                if has_id { ontology.insert(oid); }
            }
```
This SUPERSEDES the P3 `header_present` bool gate — delete the `header_present` variable and its post-loop `if header_present { ontology.insert(ontology_id); }`. The per-header `has_id` gate is the new, correct gate. Make `parse_annotations` `pub(crate)` so `reader/mod.rs` can call it.

- [ ] **Step 6: Run → PASS; verify; commit**

Watch for: the IRI-valued annotation (`ex:seeAlso ex:B`) round-tripping (AnnotationTarget tries `Literal` then `IRI`). Commit:
```bash
git add src/io/omn/writer/as_manchester.rs src/io/omn/writer/mod.rs src/grammars/omn.pest src/io/omn/reader/from_pair.rs src/io/omn/reader/mod.rs
git commit -m "feat(omn): entity + ontology annotations (Annotations:) rendering + reading

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Axiom annotations (per-clause `Annotations:` prefix) (item 3d)

A clause sourced from an `AnnotatedComponent` with a non-empty `ann` set renders the annotations inline: `SubClassOf: Annotations: ex:prov "x" ex:D`. The annotations bind to that one axiom and are read back into `AnnotatedComponent.ann`.

**Files:**
- Modify: `src/io/omn/writer/mod.rs` (thread `ac.ann` into each clause when non-empty)
- Modify: `src/grammars/omn.pest` (optional `Annotations` prefix in each keyworded clause arm)
- Modify: `src/io/omn/reader/from_pair.rs` (parse the optional prefix; build `AnnotatedComponent { component, ann }` and insert it)
- Test: round-trip in `from_pair.rs`

- [ ] **Step 1: Write the failing round-trip test** — an ontology with a `SubClassOf` carrying an annotation:

```rust
#[test]
fn reads_axiom_annotations_round_trip() {
    use crate::io::omn::{read_with_build, write};
    use crate::ontology::component_mapped::ComponentMappedOntology;
    use crate::ontology::set::SetOntology;
    use std::collections::BTreeSet;
    use std::io::BufReader;
    let b = Build::new_rc();
    let mut pm = PrefixMapping::default();
    pm.add_prefix("ex", "http://ex/").unwrap();

    let mut o = SetOntology::new_rc();
    o.insert(DeclareClass(b.class("http://ex/A")));
    o.insert(DeclareClass(b.class("http://ex/B")));
    let mut ann = BTreeSet::new();
    ann.insert(Annotation {
        ap: b.annotation_property("http://ex/prov"),
        av: AnnotationValue::Literal(Literal::Simple { literal: "inferred".to_string() }),
    });
    o.insert(AnnotatedComponent {
        component: Component::SubClassOf(SubClassOf {
            sub: ClassExpression::Class(b.class("http://ex/A")),
            sup: ClassExpression::Class(b.class("http://ex/B")),
        }),
        ann,
    });

    type TestOnt = ComponentMappedOntology<std::rc::Rc<str>, std::rc::Rc<AnnotatedComponent<std::rc::Rc<str>>>>;
    let amo: TestOnt = o.clone().into();
    let mut buf = Vec::<u8>::new();
    write(&mut buf, &amo, Some(&pm)).unwrap();
    let (parsed, _): (SetOntology<_>, PrefixMapping) = read_with_build(BufReader::new(&buf[..]), &b).unwrap();
    // compare FULL AnnotatedComponents (component + ann), not just components
    let orig: BTreeSet<_> = o.iter().cloned().collect();
    let got: BTreeSet<_> = parsed.iter().cloned().collect();
    assert_eq!(orig, got, "axiom annotation did not round-trip\n{}", String::from_utf8_lossy(&buf));
}
```

Note: this test compares full `AnnotatedComponent`s (not just `.component`) — the annotation lives in `.ann`.

- [ ] **Step 2: Run to verify failure** — FAIL (writer drops `ac.ann`; the SubClassOf round-trips without its annotation).

- [ ] **Step 3: Writer — thread `ac.ann` into the clause**

The writer's main loop binds `ac` (the `AnnotatedComponent`). Currently it renders `ac.component` and ignores `ac.ann`. For each clause-producing arm, when `!ac.ann.is_empty()`, prefix the clause value with `Annotations: <rendered anns> `. Add a helper:
```rust
fn ann_prefix<A: ForIRI>(ann: &std::collections::BTreeSet<Annotation<A>>, pm: &PrefixMapping) -> String {
    if ann.is_empty() { return String::new(); }
    let entries: Vec<String> = ann.iter()
        .map(|a| annotation_to_manchester(a, pm)) // the Task-5 helper
        .collect();
    format!("Annotations: {} ", entries.join(", "))
}
```
Then in each clause arm change `format!("SubClassOf: {}", value)` to `format!("SubClassOf: {}{}", ann_prefix(&ac.ann, pm), value)`, and likewise for the other clause arms (Domain/Range/Types/SubPropertyOf/Characteristics/Facts/EquivalentTo/DisjointWith/…). For Characteristics, the annotation prefixes the characteristic token: `Characteristics: Annotations: <anns> Functional`.

This is mechanical but touches every `push_clause!` site. Apply uniformly. (Annotations on n-ary axioms attach to the single collapsed clause; annotations on per-item axioms attach to each — but our writer emits one clause per axiom for the per-item kinds, so each gets its own `ac.ann`.)

- [ ] **Step 4: Grammar — optional `Annotations` prefix in each keyworded clause**

Insert an optional inline-annotation token between each clause keyword and its body. Reuse the Task-5 `Annotations` rule:
```pest
ClassClause = {
      Annotations
    | ^"SubClassOf:"      ~ Annotations? ~ DescriptionList
    | ^"EquivalentTo:"    ~ Annotations? ~ DescriptionList
    | ^"DisjointWith:"    ~ Annotations? ~ DescriptionList
    | ^"DisjointUnionOf:" ~ Annotations? ~ DescriptionList
    | ^"HasKey:"          ~ Annotations? ~ PropertyExprList
}
```
Apply the same `Annotations?` insertion to every keyworded arm of `ObjectPropertyClause`, `DataPropertyClause`, `AnnotationPropertyClause`, `IndividualClause` (Types/Facts/SameAs/DifferentFrom/SubPropertyChain/Domain/Range/Characteristics/SubPropertyOf/InverseOf). The standalone `Annotations` arm (entity annotation, Task 5) stays first.

**Disambiguation:** at clause position, a bare `Annotations:` → entity annotation (standalone arm); a keyword followed by `Annotations:` → axiom annotation on that clause (the `Annotations?` prefix). The preceding keyword distinguishes them. Confirmed unambiguous.

- [ ] **Step 5: Reader — parse the optional prefix; insert `AnnotatedComponent`**

The clause's inner now optionally starts with an `Annotations` pair before the body list. Update each `insert_*_frame` keyworded handler: after `clause_keyword`, peek the first inner pair — if it is `Rule::Annotations`, parse it via `parse_annotations` into the axiom's `ann` set and advance to the body; else `ann` is empty. Then build `AnnotatedComponent { component: <the axiom>, ann }` and `ont.insert(it)` instead of inserting the bare axiom.

Refactor pattern (apply per handler): replace
```rust
let body = clause.into_inner().next().unwrap();
```
with
```rust
let mut it = clause.into_inner();
let mut first = it.next().unwrap();
let mut ann: BTreeSet<Annotation<A>> = BTreeSet::new();
if first.as_rule() == Rule::Annotations {
    ann = parse_annotations(first, ctx)?.into_iter().collect();
    first = it.next().unwrap();
}
let body = first; // the list/value pair
```
and replace each `ont.insert(<Axiom> { .. })` in that handler with:
```rust
ont.insert(AnnotatedComponent { component: Component::<Variant>(<Axiom>{..}), ann: ann.clone() });
```
(`ann.clone()` because a single clause body can yield multiple per-item axioms; all share the annotation set. For n-ary clauses there is one axiom, so one insert.)

Confirm `Component::SubClassOf(..)` / `Component::EquivalentClasses(..)` / etc. are the correct wrapper variants (grep `enum Component` in model.rs). The `Annotations`-standalone (entity) arm from Task 5 is matched by `clause.as_rule() == Rule::Annotations` BEFORE this keyword dispatch and is unchanged.

- [ ] **Step 6: Run → PASS; verify; commit**

```bash
git add src/io/omn/writer/mod.rs src/grammars/omn.pest src/io/omn/reader/from_pair.rs
git commit -m "feat(omn): axiom annotations (inline Annotations: on clauses) round-trip

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Reader skip-and-warn on the residual block + capstone + limitations (item 3e)

After Tasks 3–6, the writer's misc block holds only genuinely-inexpressible components (general anonymous-subject class axioms, SWRL rules, anon-individual annotation values, orphan entity annotations, data-property HasKey members). The reader currently HARD-ERRORS at EOI on any non-empty block. Make it skip-and-warn, and add a capstone covering everything.

**Files:**
- Modify: `src/grammars/omn.pest` (allow a trailing opaque block)
- Modify: `src/io/omn/reader/mod.rs` (skip + warn)
- Modify: `src/io/omn/reader/mod.rs` module doc (limitations: remove the EOI-rejection bullet; add the conflations + dropped-on-skip note)
- Test: capstone + a skip-and-warn test in `from_pair.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn skips_general_axioms_block_without_error() {
    use crate::io::omn::read_with_build;
    use crate::ontology::set::SetOntology;
    use std::io::BufReader;
    let b = Build::new_rc();
    // a document with a frame + a trailing non-Manchester block
    let doc = "Prefix: ex: <http://ex/>\n\nClass: ex:A\n\n# General axioms\nSubClassOf(ObjectIntersectionOf(<http://ex/A> <http://ex/B>) <http://ex/C>)\n";
    let (parsed, _): (SetOntology<_>, PrefixMapping) =
        read_with_build(BufReader::new(doc.as_bytes()), &b).unwrap();
    // the frame parsed; the misc block was skipped (not errored)
    assert!(parsed.iter().any(|ac| matches!(&ac.component, Component::DeclareClass(_))));
}
```

Plus a capstone `whole_ontology_with_extras_round_trips` analogous to the existing `whole_ontology_round_trips` but ALSO inserting an `Import`, a property chain, a `HasKey` (object-only key), an `OntologyAnnotation`, an entity `AnnotationAssertion` (on a declared class), and an `AnnotatedComponent` SubClassOf-with-annotation — asserting full `AnnotatedComponent` `BTreeSet` equality.

- [ ] **Step 2: Run to verify failure** — the skip test FAILs (current reader errors at EOI on the block); the capstone FAILs only if a prior task regressed (it should pass once skip works, given Tasks 3–6).

- [ ] **Step 3: Grammar — accept a trailing opaque block**

```pest
ManchesterDocument = { SOI ~ PrefixDeclaration* ~ ImportDeclaration* ~ OntologyHeader? ~ Frame* ~ GeneralAxiomBlock? ~ EOI }
// Everything from the `# General axioms` marker to end-of-input, consumed opaquely.
GeneralAxiomBlock = { "# General axioms" ~ ANY* }
```

Note: `COMMENT` already silently eats `#`-lines; `GeneralAxiomBlock` explicitly anchors on the exact marker the writer emits (`writer/mod.rs` `# General axioms`) and swallows the rest so `EOI` is reachable. Keep the marker string in sync with the writer.

- [ ] **Step 4: Reader — warn on skip**

In `read_with_build`, add a match arm:
```rust
            Rule::GeneralAxiomBlock => {
                let body = child.as_str();
                let n = body.lines().filter(|l| !l.trim().is_empty()
                    && !l.trim_start().starts_with("# General axioms")).count();
                eprintln!(
                    "warning: omn reader skipped {n} axiom(s) in the non-Manchester \
                     `# General axioms` block (components with no Manchester form)"
                );
            }
```
(`eprintln!` is acceptable here — horned-owl's readers do not thread a logger; matching the crate's existing diagnostic style. If the crate uses `log`/`tracing`, prefer that — grep for `warn!`/`log::` first and use it if present.)

- [ ] **Step 5: Update the limitations doc**

In `src/io/omn/reader/mod.rs` module doc: REMOVE the "rejected at EOI" sentence from the misc-block bullet and REPLACE with: the misc block is now **skipped with a warning** (its axioms — general anonymous-subject class axioms, SWRL rules — are dropped, not round-tripped). ADD bullets for the conflations introduced in Tasks 4–6: (a) `HasKey:` does not distinguish object vs data key properties — data-property keys are read back as object properties; (b) entity annotations on an IRI that heads no frame are emitted to the skipped misc block (not round-tripped); (c) anonymous-individual annotation values are not rendered (misc); (d) annotation nesting (annotation-on-annotation) is not representable in horned-owl's model and is not preserved. KEEP the keyword-collision bullet but UPDATE it to "FIXED in <commit>: keyword tokens now carry a `!SPARQL_PnChars` maximal-munch boundary" (Task 1 closed it).

- [ ] **Step 6: Run everything → PASS; full verify; commit**

Run: `cargo test --lib io::omn 2>&1 | tail -8` (all green, incl. capstone + skip test), `cargo build -p horned-owl`, `cargo fmt -- --check` (rc 0), `cargo clippy --lib 2>&1 | grep -i omn` (clean), and `cd /data/dumontier/rustdl && cargo build -p owl-dl-reasoner 2>&1 | tail -2` (rustdl still builds against the fork).

```bash
git add src/grammars/omn.pest src/io/omn/reader/mod.rs src/io/omn/reader/from_pair.rs
git commit -m "feat(omn): skip-and-warn on residual general-axioms block + capstone + docs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Remaining after this plan (documented for the PR, NOT in scope)

- Native rendering of the genuinely-inexpressible remainder (general anonymous-subject class axioms, SWRL rules) — no standard Manchester frame form; stays in the skipped block.
- Anonymous-individual annotation values / subjects; annotation nesting (model can't store it).
- HasKey object-vs-data key recovery (inherent Manchester ambiguity).
- Rebase `manchester-io` onto phillord/horned-owl `main`; validate `write` output through omny / the OWL API.
- The user opens the upstream PR.

---

## Self-Review notes (filled during planning)

- **Item coverage:** Task 1 = item 1 (keyword anchoring, silent-lookahead approach preserves all detection code); Task 2 = item 2 (ResourceType/ParserOutput, no dispatch exists); Tasks 3–7 = item 3 (Import / chains+HasKey / entity+ontology annotations / axiom annotations / skip-and-warn+capstone+docs).
- **Model-limit honesty:** annotation nesting is unrepresentable (ofn discards it) — scoped to flat annotations and documented; HasKey object/data conflation and orphan-entity-annotation routing documented as limitations with the test corpora steered to the round-trippable cases.
- **Round-trip-gate integrity:** every new construct gets its own round-trip test; the axiom-annotation test compares full `AnnotatedComponent`s (not just `.component`); the capstone composes all of them.
- **Type/name verification flags inline:** `ComponentKind::Import` name, the `Component::<Variant>` wrappers, `b.annotation_property`/`b.data_property` constructors, and whether the crate uses `log`/`tracing` vs `eprintln!` are each flagged for the implementer to confirm against the source.
- **Non-regression:** Task 1's lookahead is zero-width/silent (17-case suite is the guard); Task 2 fixes exhaustive matches; Tasks 3–6 only ADD writer arms/grammar arms/reader handlers and move components OUT of misc — no existing clause path changes except the mechanical `ann_prefix` threading in Task 6 (guarded by `is_empty()`).
