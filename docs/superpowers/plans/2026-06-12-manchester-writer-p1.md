# Manchester writer (`io/omn` P1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Manchester Syntax **writer** (`AsManchester` trait + frame-grouped `io::omn::write`) to a horned-owl fork, `[patch]`ed into rustdl, and render `rustdl justify` output in readable Manchester.

**Architecture:** Mirror horned-owl's `io/ofn/writer/as_functional.rs` (a `Manchester<'t,T,A>: Display` wrapper + `AsManchester<A>` trait, recursing per OWL type), adding Manchester's two non-functional concerns: operator-precedence parenthesization for class expressions, and subject-grouped frames for whole-ontology output. Reader (parsing) is P2/P3 — out of scope here.

**Tech Stack:** Rust 2024, horned-owl 1.4.0 (local fork via `[patch.crates-io]`), `std::fmt::Display`, `curie::PrefixMapping`.

Spec: `docs/superpowers/specs/2026-06-12-horned-owl-manchester-io-design.md`. This is **P1 of 3** (writer); P2 = reader class-expressions, P3 = reader frames/document.

---

## File structure

- **Create (fork):** `../horned-owl-omn/` — a clone of horned-owl 1.4.0 source (git branch `manchester-io`).
  - `src/io/omn/mod.rs` — module entry + re-exports.
  - `src/io/omn/writer/mod.rs` — `pub fn write<A,AA,W>(writer, &ontology)` (frame-grouped).
  - `src/io/omn/writer/as_manchester.rs` — `AsManchester` trait + `Manchester` wrapper + all `Display` impls.
  - `src/io/mod.rs` — register `pub mod omn;`.
- **Modify (rustdl):** workspace `Cargo.toml` — `[patch.crates-io] horned-owl = { path = "../horned-owl-omn" }`.
- **Modify (rustdl):** `crates/owl-dl-cli/src/main.rs` — `justify` renders via `as_manchester_with_prefixes`.

Reference template (read it): `../horned-owl-omn/src/io/ofn/writer/as_functional.rs` (the `AsFunctional`/`Functional` structure to mirror) and `.../writer/mod.rs` (the `write(W, onto)` entry).

---

### Task 1: Fork setup + `io/omn` skeleton + first render test

**Files:** create the fork; `src/io/omn/{mod.rs, writer/mod.rs, writer/as_manchester.rs}`; `src/io/mod.rs`; rustdl `Cargo.toml`.

- [ ] **Step 1: Clone horned-owl source to the fork path**

```bash
cd /data/dumontier/rustdl
# horned-owl 1.4.0 source is in the cargo registry cache; copy it to a sibling fork dir + init git.
SRC=$(find ~/.cargo/registry/src -maxdepth 1 -type d -name 'horned-owl-1.4.0' | head -1)
cp -r "$SRC" ../horned-owl-omn
cd ../horned-owl-omn && git init -q && git add -A && git commit -q -m "vendor: horned-owl 1.4.0 baseline" && cd -
```
(If a `../horned-owl-omn` already exists, reuse it. The fork is a sibling of the rustdl repo dir.)

- [ ] **Step 2: Patch rustdl to the fork + register the module**

In rustdl's workspace `Cargo.toml`, add at the end:
```toml
[patch.crates-io]
horned-owl = { path = "../horned-owl-omn" }
```
In `../horned-owl-omn/src/io/mod.rs`, add `pub mod omn;` next to the existing `pub mod ofn;`/`owx`/`rdf`.

- [ ] **Step 3: Write the failing test** (in `../horned-owl-omn/src/io/omn/writer/as_manchester.rs`, a `#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Build;

    #[test]
    fn renders_named_class() {
        let b = Build::new_rc();
        let c = b.class("http://example.org/Dog");
        assert_eq!(c.as_manchester().to_string(), "<http://example.org/Dog>");
    }

    #[test]
    fn renders_class_with_prefix() {
        let b = Build::new_rc();
        let c = b.class("http://example.org/Dog");
        let mut pm = curie::PrefixMapping::default();
        pm.add_prefix("ex", "http://example.org/").unwrap();
        assert_eq!(c.as_manchester_with_prefixes(&pm).to_string(), "ex:Dog");
    }
}
```

- [ ] **Step 4: Run to verify it fails** (module/trait absent)

`cd ../horned-owl-omn && cargo test --lib io::omn 2>&1 | tail -15`
Expected: compile error (`as_manchester` not found).

- [ ] **Step 5: Implement the trait + wrapper + Class/IRI rendering** (`src/io/omn/writer/as_manchester.rs`)

Mirror `as_functional.rs`'s `AsFunctional`/`Functional` exactly, renamed, with IRI rendering that abbreviates via the prefix map (Manchester uses bare `prefix:local`, or `<full-iri>` when no prefix matches):

```rust
use std::fmt::{Display, Error, Formatter};
use curie::PrefixMapping;
use crate::model::*;

/// OWL elements renderable in Manchester syntax.
pub trait AsManchester<A: ForIRI> {
    fn as_manchester(&self) -> Manchester<'_, Self, A> { Manchester(self, None) }
    fn as_manchester_with_prefixes<'t>(&'t self, prefix: &'t PrefixMapping) -> Manchester<'t, Self, A> {
        Manchester(self, Some(prefix))
    }
}

/// Lazy `Display` wrapper for a Manchester-rendered element.
#[derive(Debug)]
pub struct Manchester<'t, T: ?Sized, A: ForIRI>(&'t T, Option<&'t PrefixMapping>);

impl<'t, T, A> Display for Manchester<'t, &'t T, A>
where
    Manchester<'t, T, A>: Display,
    A: ForIRI,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        Manchester(*self.0, self.1).fmt(f)
    }
}

/// Render an IRI: abbreviated `prefix:local` if a prefix matches, else `<iri>`.
fn write_iri<A: ForIRI>(iri: &str, prefix: Option<&PrefixMapping>, f: &mut Formatter<'_>) -> Result<(), Error> {
    if let Some(pm) = prefix {
        if let Some(curie) = pm.shrink_iri(iri).ok() {
            return write!(f, "{curie}");
        }
    }
    write!(f, "<{iri}>")
}

impl<A: ForIRI> Display for Manchester<'_, IRI<A>, A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write_iri::<A>(self.0.as_ref(), self.1, f)
    }
}
impl<A: ForIRI> AsManchester<A> for IRI<A> {}

impl<A: ForIRI> Display for Manchester<'_, Class<A>, A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{}", Manchester(&self.0.0, self.1))
    }
}
impl<A: ForIRI> AsManchester<A> for Class<A> {}
```
In `src/io/omn/writer/mod.rs` add `pub mod as_manchester; pub use as_manchester::{AsManchester, Manchester};`, and in `src/io/omn/mod.rs` add `pub mod writer; pub use writer::{AsManchester, Manchester};`.

Notes: confirm `Build::new_rc()` exists (it does — used in horned-owl tests); confirm `Class<A>` is a tuple-struct `Class(IRI<A>)` (`self.0.0`); confirm `PrefixMapping::shrink_iri` returns `Result<Curie,_>` with `Curie: Display` (check curie crate API — if it returns a different shape, adapt `write_iri`). `IRI<A>: AsRef<str>` (used by ofn).

- [ ] **Step 6: Run to verify it passes**

`cd ../horned-owl-omn && cargo test --lib io::omn 2>&1 | tail -8` → both tests pass.

- [ ] **Step 7: Confirm rustdl builds against the fork**

`cd /data/dumontier/rustdl && cargo build -p owl-dl-reasoner 2>&1 | tail -3` (compiles against the patched horned-owl).

- [ ] **Step 8: Commit (both repos)**

```bash
cd ../horned-owl-omn && git add -A && git commit -q -m "feat(io/omn): Manchester writer skeleton — AsManchester trait + IRI/Class rendering"
cd /data/dumontier/rustdl && git add Cargo.toml Cargo.lock && git commit -m "build: [patch] horned-owl to local manchester-io fork

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Leaf entity rendering (properties, individuals, literals)

**Files:** `../horned-owl-omn/src/io/omn/writer/as_manchester.rs`

- [ ] **Step 1: Write failing tests** (append to the test module)

```rust
#[test]
fn renders_object_property_and_inverse() {
    let b = Build::new_rc();
    let p = b.object_property("http://example.org/hasParent");
    assert_eq!(ObjectPropertyExpression::ObjectProperty(p.clone()).as_manchester().to_string(),
        "<http://example.org/hasParent>");
    assert_eq!(ObjectPropertyExpression::InverseObjectProperty(p).as_manchester().to_string(),
        "inverse (<http://example.org/hasParent>)");
}

#[test]
fn renders_individual_and_literal() {
    let b = Build::new_rc();
    let i = Individual::Named(b.named_individual("http://example.org/fido"));
    assert_eq!(i.as_manchester().to_string(), "<http://example.org/fido>");
    let lit = Literal::Datatype {
        literal: "5".to_string(),
        datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
    };
    assert_eq!(lit.as_manchester().to_string(), "\"5\"^^<http://www.w3.org/2001/XMLSchema#integer>");
}
```

- [ ] **Step 2: Run to verify it fails.** `cd ../horned-owl-omn && cargo test --lib io::omn 2>&1 | tail`

- [ ] **Step 3: Implement leaf renderers.** Add `Display for Manchester<...>` + `AsManchester` impls for: `ObjectProperty<A>` (→ its IRI), `ObjectPropertyExpression<A>` (`ObjectProperty(p)` → IRI; `InverseObjectProperty(p)` → `inverse (IRI)`), `DataProperty<A>` (→ IRI), `NamedIndividual<A>`/`AnonymousIndividual<A>`/`Individual<A>` (Named → IRI; Anonymous → `_:node`), `Literal<A>` (per the 3 variants: `Simple{literal}` → `"lit"`; `Language{literal,lang}` → `"lit"@lang`; `Datatype{literal,datatype_iri}` → `"lit"^^IRI`), `Datatype<A>` (→ IRI). Mirror the `as_functional.rs` arms for these exact types (it has them all — copy the structure, emit Manchester). Reference the `Literal`/`Individual` enum shapes there.

- [ ] **Step 4: Run to verify it passes.** Expected: both tests pass.

- [ ] **Step 5: fmt + clippy** (in the fork): `cargo fmt && cargo fmt -- --check` (rc 0); `cargo clippy --lib -- -D warnings 2>&1 | tail -4` (clean; the fork's own lint config applies).

- [ ] **Step 6: Commit.** `cd ../horned-owl-omn && git add -A && git commit -q -m "feat(io/omn): leaf rendering — properties, individuals, literals, datatypes"`

---

### Task 3: ClassExpression rendering with operator precedence (the core)

**Files:** `../horned-owl-omn/src/io/omn/writer/as_manchester.rs`

Manchester precedence: `not` (tightest) > `and` > `or` (loosest); restrictions (`R some C`) bind like atoms. A child of a higher-precedence operator that is itself a lower-precedence expression must be parenthesized.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn renders_class_expressions_with_precedence() {
    let b = Build::new_rc();
    let (a, c, d) = (
        ClassExpression::Class(b.class("http://t/A")),
        ClassExpression::Class(b.class("http://t/C")),
        ClassExpression::Class(b.class("http://t/D")),
    );
    let mut pm = curie::PrefixMapping::default();
    pm.add_prefix("", "http://t/").unwrap(); // default prefix → bare local names
    let m = |ce: &ClassExpression<_>| ce.as_manchester_with_prefixes(&pm).to_string();

    // and / or
    assert_eq!(m(&ClassExpression::ObjectIntersectionOf(vec![a.clone(), c.clone()])), "A and C");
    assert_eq!(m(&ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone()])), "A or C");
    // not binds tightest
    assert_eq!(m(&ClassExpression::ObjectComplementOf(Box::new(a.clone()))), "not A");
    // precedence: A or (C and D)  — and binds tighter than or, no parens needed
    let cd = ClassExpression::ObjectIntersectionOf(vec![c.clone(), d.clone()]);
    assert_eq!(m(&ClassExpression::ObjectUnionOf(vec![a.clone(), cd.clone()])), "A or C and D");
    // precedence: (A or C) and D — or under and MUST be parenthesized
    let ac = ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone()]);
    assert_eq!(m(&ClassExpression::ObjectIntersectionOf(vec![ac, d.clone()])), "(A or C) and D");

    let r = b.object_property("http://t/r");
    let ope = ObjectPropertyExpression::ObjectProperty(r);
    assert_eq!(m(&ClassExpression::ObjectSomeValuesFrom { ope: ope.clone(), bce: Box::new(a.clone()) }), "r some A");
    assert_eq!(m(&ClassExpression::ObjectAllValuesFrom { ope: ope.clone(), bce: Box::new(a.clone()) }), "r only A");
    assert_eq!(m(&ClassExpression::ObjectMinCardinality { n: 2, ope: ope.clone(), bce: Box::new(a.clone()) }), "r min 2 A");
}
```
(Adapt field names — `ObjectSomeValuesFrom { ope, bce }` etc. — to the EXACT struct-variant fields in `horned_owl::model::ClassExpression`; read the enum first.)

- [ ] **Step 2: Run to verify it fails.**

- [ ] **Step 3: Implement precedence-aware rendering.** Add a precedence rank + a helper that parenthesizes a child when its rank is looser than the parent's:

```rust
/// Manchester precedence: higher binds tighter. `or`=1, `and`=2, atom/not/restriction=3.
fn prec<A: ForIRI>(ce: &ClassExpression<A>) -> u8 {
    match ce {
        ClassExpression::ObjectUnionOf(_) => 1,
        ClassExpression::ObjectIntersectionOf(_) => 2,
        _ => 3,
    }
}

/// Render `child` as a sub-expression of a parent at `parent_prec`, parenthesizing if looser.
fn child<A: ForIRI>(child: &ClassExpression<A>, parent_prec: u8, pm: Option<&PrefixMapping>, f: &mut Formatter<'_>) -> Result<(), Error> {
    if prec(child) < parent_prec {
        write!(f, "({})", Manchester(child, pm))
    } else {
        write!(f, "{}", Manchester(child, pm))
    }
}
```
Then `Display for Manchester<'_, ClassExpression<A>, A>` with one arm per variant:
- `Class(c)` → `Manchester(c, pm)`.
- `ObjectIntersectionOf(v)` → join `child(e, 2, ..)` with ` and `.
- `ObjectUnionOf(v)` → join `child(e, 1, ..)` with ` or `.
- `ObjectComplementOf(bce)` → `not ` + `child(bce, 3, ..)`.
- `ObjectSomeValuesFrom{ope,bce}` → `{ope} some {child(bce,3,..)}`; `...AllValuesFrom` → `only`; `ObjectHasValue{ope,i}` → `{ope} value {i}`; `ObjectHasSelf(ope)` → `{ope} Self`.
- `ObjectMinCardinality{n,ope,bce}` → `{ope} min {n} {child(bce,3,..)}`; `Max`→`max`; `Exact`→`exactly`.
- `ObjectOneOf(v)` → `{ a, b }` (individuals comma-joined in braces).
- Data variants: `DataSomeValuesFrom{dp,dr}` → `{dp} some {dr}`; `DataAllValuesFrom` → `only`; `DataHasValue{dp,l}` → `{dp} value {l}`; `DataMin/Max/ExactCardinality` → `{dp} min/max/exactly {n} {dr}`.

Implement `AsManchester<A> for ClassExpression<A> {}`. Render restriction fillers via `child(.., 3, ..)` so e.g. `r some (A or C)` parenthesizes correctly (a bare `or` under a restriction needs parens). Mirror `as_functional.rs`'s ClassExpression arms for the variant set + exact field names; the ONLY additions are `prec`/`child` and the infix joins.

- [ ] **Step 4: Run to verify it passes.** All precedence cases must match (especially `(A or C) and D` and `r some A`).

- [ ] **Step 5: fmt + clippy. Step 6: Commit** `feat(io/omn): class-expression rendering with operator precedence`.

---

### Task 4: DataRange + facet rendering

**Files:** `../horned-owl-omn/src/io/omn/writer/as_manchester.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn renders_data_ranges() {
    let b = Build::new_rc();
    let int = b.datatype("http://www.w3.org/2001/XMLSchema#integer");
    let mut pm = curie::PrefixMapping::default();
    pm.add_prefix("xsd", "http://www.w3.org/2001/XMLSchema#").unwrap();
    let m = |dr: &DataRange<_>| dr.as_manchester_with_prefixes(&pm).to_string();
    assert_eq!(m(&DataRange::Datatype(int.clone())), "xsd:integer");
    // integer[>= 0]
    let fr = FacetRestriction {
        f: crate::vocab::Facet::MinInclusive,
        l: Literal::Datatype { literal: "0".into(), datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer") },
    };
    assert_eq!(m(&DataRange::DatatypeRestriction(int, vec![fr])), "xsd:integer[>= 0]");
}
```
(Adapt `DataRange`/`FacetRestriction` field names + `Facet` variants to the real model; read them. The facet symbol map: `MinInclusive`→`>=`, `MinExclusive`→`>`, `MaxInclusive`→`<=`, `MaxExclusive`→`<`, `Length`→`length`, etc.)

- [ ] **Step 2–6:** run-fail; implement `Display`/`AsManchester` for `DataRange<A>` (`Datatype`→IRI; `DataIntersectionOf`→`and`; `DataUnionOf`→`or`; `DataComplementOf`→`not`; `DataOneOf`→`{ ... }`; `DatatypeRestriction(dt, facets)`→`dt[facet1, facet2]`) + `FacetRestriction<A>` (`{symbol} {literal}`) + a `facet_symbol(Facet)->&str` helper; run-pass; fmt+clippy; commit `feat(io/omn): data range + facet rendering`.

---

### Task 5: Axiom (Component) per-axiom rendering

**Files:** `../horned-owl-omn/src/io/omn/writer/as_manchester.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn renders_axioms_per_line() {
    let b = Build::new_rc();
    let mut pm = curie::PrefixMapping::default();
    pm.add_prefix("", "http://t/").unwrap();
    let m = |c: &Component<_>| c.as_manchester_with_prefixes(&pm).to_string();
    let (a, cc) = (b.class("http://t/A"), b.class("http://t/C"));
    assert_eq!(m(&Component::SubClassOf(SubClassOf {
        sub: ClassExpression::Class(a.clone()), sup: ClassExpression::Class(cc.clone()) })),
        "A SubClassOf C");
    let ind = b.named_individual("http://t/x");
    assert_eq!(m(&Component::ClassAssertion(ClassAssertion {
        ce: ClassExpression::Class(a.clone()), i: Individual::Named(ind) })),
        "x Type A");
}
```

- [ ] **Step 2–6:** run-fail; implement `Display`/`AsManchester for Component<A>` with a per-axiom Manchester clause per variant (the rules from the spec's writer section — `SubClassOf`→`{sub} SubClassOf {sup}`; `EquivalentClasses`→`{a} EquivalentTo {b}` (join all members with `EquivalentTo`/`,` per Manchester); `DisjointClasses`→`{..} DisjointWith {..}`; `SubObjectPropertyOf`→`{sub} SubPropertyOf {sup}`; characteristics→`{p} Characteristics: Functional` etc.; `ObjectPropertyDomain`/`Range`→`{p} Domain/Range {ce}`; `ClassAssertion`→`{i} Type {ce}`; `ObjectPropertyAssertion`→`{from} {ope} {to}`; `SameIndividual`/`DifferentIndividuals`→`{..} SameAs/DifferentFrom {..}`; data + annotation analogues). For any `Component` variant with no natural single-axiom Manchester form (e.g. `OntologyID`, declarations), render the functional fallback `format!("{}", c.as_functional())` or a `# <variant>` comment — but log which (these mostly won't appear in justifications). Read `as_functional.rs`'s `Component` arms for the full variant list. run-pass; fmt+clippy; commit `feat(io/omn): per-axiom Manchester rendering for Components`.

---

### Task 6: Whole-ontology `write` with frame grouping

**Files:** `../horned-owl-omn/src/io/omn/writer/mod.rs`

- [ ] **Step 1: Write failing test**

```rust
// in src/io/omn/writer/mod.rs #[cfg(test)]
#[test]
fn writes_grouped_frames() {
    let b = Build::new_rc();
    let mut o = crate::ontology::set::SetOntology::new_rc();
    use crate::model::MutableOntology;
    o.declare(b.class("http://t/A"));
    o.insert(SubClassOf { sub: ClassExpression::Class(b.class("http://t/A")),
        sup: ClassExpression::Class(b.class("http://t/B")) });
    let mut out = Vec::new();
    write(&mut out, &o.into()).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("Class: <http://t/A>"));
    assert!(s.contains("SubClassOf: <http://t/B>"));
}
```
(Adapt the `write` signature + ontology type to match `io/ofn/writer/mod.rs::write` exactly — same `<A, AA: ForIndex<A>, W: Write>` generics + the same ontology arg type. Read it first.)

- [ ] **Step 2–6:** run-fail; implement `pub fn write<A,AA,W>(writer, &ontology)` mirroring `io/ofn/writer/mod.rs::write` — emit `Prefix:` decls (if a `PrefixMapping` is threaded as ofn does), then group axioms by subject entity into `Class:`/`ObjectProperty:`/`DataProperty:`/`Individual:`/`Datatype:`/`AnnotationProperty:` frames, each followed by indented clauses (`SubClassOf:`, `EquivalentTo:`, `Types:`, `Facts:`, …) rendered via the Task 5 per-clause logic. Axioms with no single subject (e.g. `DisjointClasses` of many) go in a `DisjointClasses:` misc section or a frame on the first member per Manchester convention. run-pass; fmt+clippy; commit `feat(io/omn): frame-grouped whole-ontology Manchester writer`.

(If frame grouping proves large, this Task may itself split — frame grouping has no `io/ofn` template. Keep `write` correct-but-simple first: a valid Manchester document that round-trips structurally, even if frame grouping is coarse.)

---

### Task 7: Wire `rustdl justify` to Manchester + readable canary

**Files:** `crates/owl-dl-cli/src/main.rs`; `crates/owl-dl-reasoner/tests/justification.rs`

- [ ] **Step 1: Write a failing CLI-level rendering canary** (append to `tests/justification.rs`)

```rust
#[test]
fn justification_renders_manchester() {
    use horned_owl::io::omn::AsManchester;
    let o = onto("Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                  SubClassOf(:A :B) SubClassOf(:B :C)");
    let q = Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/C".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("entailed");
    // Each justification axiom renders to a readable Manchester string (not Debug).
    let rendered: Vec<String> = j.axioms.iter().map(|c| c.as_manchester().to_string()).collect();
    assert!(rendered.iter().any(|s| s.contains("SubClassOf")), "got {rendered:?}");
    assert!(!rendered.iter().any(|s| s.contains("SubClassOf { sub:")), "must not be Debug output");
}
```

- [ ] **Step 2: Run to verify it fails** (`horned_owl::io::omn::AsManchester` import or method resolves only once the fork is patched — it should compile now; the assertion drives the CLI change). `cargo test -p owl-dl-reasoner --test justification justification_renders_manchester 2>&1 | tail -10`

- [ ] **Step 3: Wire the CLI.** In `crates/owl-dl-cli/src/main.rs`'s `Command::Justify` handler, change the `render` closure's axiom loop from `println!("  {ax:?}")` to:
```rust
                for ax in &j.axioms {
                    println!("  {}", ax.as_manchester_with_prefixes(&prefixes));
                }
```
where `prefixes` is the `PrefixMapping` from parsing the ontology (capture it from `parse_ofn` — if `parse_ofn` discards it, thread it out, or use `ax.as_manchester()` with full IRIs as the v1 fallback). Add `use horned_owl::io::omn::AsManchester;` to the CLI imports.

- [ ] **Step 4: Run the canary + manual smoke**

```bash
cargo test -p owl-dl-reasoner --test justification justification_renders_manchester 2>&1 | tail -6
cargo build -p owl-dl-cli --release 2>&1 | tail -1
./target/release/rustdl justify /tmp/just.ofn subclass http://t/A http://t/C
```
Expected: the canary passes; the CLI prints `A SubClassOf C` / `B SubClassOf C`-style lines (Manchester), not Debug.

- [ ] **Step 5: Full test + fmt + clippy (both repos)**

`cd ../horned-owl-omn && cargo test --lib io::omn 2>&1 | tail -4 && cargo clippy --lib -- -D warnings 2>&1 | tail -3`
`cd /data/dumontier/rustdl && cargo test -p owl-dl-reasoner --test justification 2>&1 | tail -4 && cargo fmt --all -- --check && cargo clippy -p owl-dl-cli --all-targets --all-features -- -D warnings 2>&1 | tail -3`

- [ ] **Step 6: Commit + prepare the upstream PR branch**

```bash
cd ../horned-owl-omn && git add -A && git commit -q -m "test(io/omn): writer unit coverage" && git log --oneline | head
cd /data/dumontier/rustdl && git add -A && git commit -m "feat(cli): render justify output in Manchester syntax via io/omn writer

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
The `../horned-owl-omn` branch `manchester-io` is now the PR-ready writer contribution (P2/P3 add the reader before the upstream PR is complete, OR submit the writer as a standalone PR — user's call).

---

## Notes for the implementer

- **Two repos:** the writer lives in `../horned-owl-omn` (the horned-owl fork); only `Cargo.toml`/`Cargo.lock` + the CLI + a test change live in rustdl. Commit in each repo separately.
- **Mirror `as_functional.rs` arm-for-arm** for the mechanical per-variant rendering — read the EXACT `ClassExpression`/`Component`/`Literal`/`DataRange` enum shapes + field names there; this plan's field names (`bce`, `ope`, `ce`, `i`, `sub`, `sup`) are the expected horned-owl 1.4.0 names but VERIFY against the model.
- **The only non-mechanical parts** are: precedence/parenthesization (Task 3 `prec`/`child`), frame grouping (Task 6), and IRI abbreviation (`write_iri`). Get those right; the rest is a faithful Manchester transcription of the functional arms.
- **No reader yet** — round-trip testing arrives in P2. P1's tests are render→expected-string + the rustdl canary.
- **`curie::PrefixMapping::shrink_iri`** API: verify the return type (it may be `Result<Curie, ...>` or `Option`); `as_functional.rs`'s `with_prefixes` path shows the exact usage — copy it.
- **Do NOT push** either repo (the upstream PR is the user's; rustdl pushes are the user's call).
- horned-owl fork builds/tests run from `../horned-owl-omn` (its own `cargo`, its own lint config).
