# Manchester reader — class expressions (`io/omn` P2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Manchester Syntax **reader** for OWL class expressions in the horned-owl fork — `omn.pest` grammar (precedence-layered) + a pest lexer + `FromPair` conversion to `ClassExpression<A>` — round-trip-tested against the P1 writer (`parse(ce.as_manchester()) == ce`).

**Architecture:** Mirror horned-owl's `io/ofn/reader` (a multi-`#[grammar=…]` pest `Parser` derive reusing `rfc3987`/`bcp47`/`sparql` for IRIs/literals/langtags; a `FromPair<A>` trait with `from_pair_unchecked(pair, &Context)`; a `Context` carrying the `PrefixMapping` + `Build`). The Manchester grammar's class-expression productions are *precedence-layered* (`or` < `and` < `not`/restriction/atomic) — that layering is the parser's analogue of the writer's `ce_prec`.

**Tech Stack:** Rust 2024, horned-owl fork (`/data/dumontier/horned-owl-omn`, branch `manchester-io`), `pest`/`pest_derive` (already deps, used by `io/ofn`), `curie::PrefixMapping`.

Spec: `/data/dumontier/rustdl/docs/superpowers/specs/2026-06-12-horned-owl-manchester-io-design.md`. **P2 of 3** (reader: class expressions). P1 (writer) done; **P3** (reader: frames + ontology document) is the next plan. Work + commit in the **fork** repo.

Reference template (read it first): `src/grammars/ofn.pest`, `src/io/ofn/reader/{lexer.rs, from_pair.rs, mod.rs}` — especially `Context` (in `reader/mod.rs`), the `FromPair` trait, and the `ClassExpression`/`IRI`/`ObjectPropertyExpression` impls.

---

## File structure (in the fork)

- **Create** `src/grammars/omn.pest` — the Manchester grammar (P2: class-expression productions; P3 extends with frames).
- **Create** `src/io/omn/reader/mod.rs` — `ManchesterLexer` re-export + `Context` (reuse/mirror ofn's) + a public `parse_class_expression(&str, &PrefixMapping, &Build) -> Result<ClassExpression>` test/entry helper.
- **Create** `src/io/omn/reader/lexer.rs` — `#[derive(Parser)]` lexer over the grammars (mirror `ofn/reader/lexer.rs`).
- **Create** `src/io/omn/reader/from_pair.rs` — `FromPair<A>` trait (omn's own, keyed to omn's `Rule`) + impls for `ClassExpression<A>` + the leaves it needs (`IRI`, `Class`, `ObjectPropertyExpression`, `Individual`, `Literal`, `DataRange` references).
- **Modify** `src/io/omn/mod.rs` — `pub mod reader;` + re-exports.

---

### Task 1: `omn.pest` class-expression grammar + lexer (lex-only)

**Files:** `src/grammars/omn.pest`, `src/io/omn/reader/lexer.rs`, `src/io/omn/reader/mod.rs`, `src/io/omn/mod.rs`.

- [ ] **Step 1: Read the template** — `src/grammars/ofn.pest` (how it imports `rfc3987`/`bcp47`/`sparql` rules: `IRI`, `full_iri`, `prefixed_name`/`PNAME_LN`, `quoted_string`/literals, langtag) and `src/io/ofn/reader/lexer.rs`. Note the EXACT shared-grammar rule names you can reuse for IRIs + literals (e.g. the rule that matches `<...>` and `prefix:local`, and the literal rule). The Manchester grammar reuses these for terminals.

- [ ] **Step 2: Write `src/grammars/omn.pest`** — the class-expression sub-grammar, precedence-layered. (Reuse the IRI/literal terminal rules from the shared grammars by name — replace `IRI`/`Literal`/`PNAME` below with the actual shared rule names found in Step 1.)

```pest
WHITESPACE = _{ " " | "\t" | "\n" | "\r" }
COMMENT    = _{ "#" ~ (!"\n" ~ !"\r" ~ ANY)* }

// Entry for P2 testing: a single class expression, whole input.
ClassExpressionDocument = _{ SOI ~ Description ~ EOI }

// Precedence layering: Description (or) < Conjunction (and) < Primary (not / restriction / atomic).
Description  = { Conjunction ~ ( ^"or" ~ Conjunction )* }
Conjunction  = { Primary ~ ( ^"and" ~ Primary )* }
Primary      = { (^"not")? ~ ( Restriction | Atomic ) }

Atomic       = { ClassIRI | ObjectOneOf | "(" ~ Description ~ ")" }
ObjectOneOf  = { "{" ~ Individual ~ ( "," ~ Individual )* ~ "}" }

// Restrictions: object + data. `ope` is an object property expression (handles `inverse`).
Restriction  = {
      ope ~ ^"some"    ~ Primary
    | ope ~ ^"only"    ~ Primary
    | ope ~ ^"value"   ~ Individual
    | ope ~ ^"Self"
    | ope ~ ^"min"     ~ Cardinality ~ Primary?
    | ope ~ ^"max"     ~ Cardinality ~ Primary?
    | ope ~ ^"exactly" ~ Cardinality ~ Primary?
    | DataPropertyIRI ~ ^"some"    ~ DataRange
    | DataPropertyIRI ~ ^"only"    ~ DataRange
    | DataPropertyIRI ~ ^"value"   ~ Literal
    | DataPropertyIRI ~ ^"min"     ~ Cardinality ~ DataRange?
    | DataPropertyIRI ~ ^"max"     ~ Cardinality ~ DataRange?
    | DataPropertyIRI ~ ^"exactly" ~ Cardinality ~ DataRange?
}

ope          = { (^"inverse" ~ "(" ~ ObjectPropertyIRI ~ ")") | ObjectPropertyIRI }
Cardinality  = { ASCII_DIGIT+ }

// Terminals — reuse the shared-grammar rules (rename to the real ones from Step 1).
ClassIRI          = { IRI }
ObjectPropertyIRI = { IRI }
DataPropertyIRI   = { IRI }
Individual        = { IRI }   // P2: named individuals only; anon (_:x) deferred to P3 if needed
DataRange         = { Datatype ~ ("[" ~ Facet ~ ( "," ~ Facet )* ~ "]")? }
Datatype          = { IRI }
Facet             = { FacetSymbol ~ Literal }
FacetSymbol       = { ">=" | "<=" | ">" | "<" | ^"length" | ^"minLength" | ^"maxLength" | ^"pattern" | ^"langRange" | ^"totalDigits" | ^"fractionDigits" }
```
Notes: `^"or"` = case-insensitive keyword. `Primary?` after `min/max/exactly Cardinality` makes the qualifier optional (unqualified cardinality `r min 2`). The `Restriction` ordering puts `some/only/value/Self` before `min/max/exactly` and object before data — pest is PEG (ordered choice), so list the more-specific/longer alternatives first. **Disambiguation gotcha:** `Atomic`'s `ClassIRI` could greedily match the property IRI of a `Restriction`; PEG ordered choice in `Primary` tries `Restriction` before `Atomic`, so `r some A` parses as a restriction (good). Verify with the lex tests below.

- [ ] **Step 3: Write `src/io/omn/reader/lexer.rs`** mirroring `ofn/reader/lexer.rs`:
```rust
use pest::iterators::Pairs;
use pest_derive::Parser;
use crate::error::HornedError;

/// The OWL Manchester Syntax lexer.
#[derive(Debug, Parser)]
#[grammar = "grammars/bcp47.pest"]
#[grammar = "grammars/rfc3987.pest"]
#[grammar = "grammars/sparql.pest"]
#[grammar = "grammars/omn.pest"]
pub struct ManchesterLexer;

impl ManchesterLexer {
    pub fn lex(rule: Rule, input: &str) -> Result<Pairs<'_, Rule>, HornedError> {
        <Self as pest::Parser<Rule>>::parse(rule, input).map_err(From::from)
    }
}
```
In `src/io/omn/reader/mod.rs`: `pub mod lexer; pub mod from_pair; pub use lexer::{ManchesterLexer, Rule};` (P2). In `src/io/omn/mod.rs`: add `pub mod reader;`.

- [ ] **Step 4: Lex-only tests** (in `lexer.rs` `#[cfg(test)]`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn lexes(s: &str) -> bool { ManchesterLexer::lex(Rule::ClassExpressionDocument, s).is_ok() }
    #[test]
    fn lex_class_expressions() {
        assert!(lexes("<http://t/A>"));
        assert!(lexes("<http://t/A> and <http://t/C>"));
        assert!(lexes("<http://t/A> or <http://t/C> and <http://t/D>"));
        assert!(lexes("(<http://t/A> or <http://t/C>) and <http://t/D>"));
        assert!(lexes("not <http://t/A>"));
        assert!(lexes("<http://t/r> some <http://t/A>"));
        assert!(lexes("<http://t/r> min 2 <http://t/A>"));
        assert!(lexes("inverse (<http://t/r>) some <http://t/A>"));
        assert!(lexes("{ <http://t/a>, <http://t/b> }"));
    }
}
```

- [ ] **Step 5: Run** `cd /data/dumontier/horned-owl-omn && cargo test --lib io::omn::reader::lexer 2>&1 | tail -15`. If a grammar rule name collides with a shared-grammar rule (pest errors on duplicate rule names across the `#[grammar]` files), rename the omn rule. If `IRI` isn't the shared rule name, use the real one. All `lexes(...)` must pass.

- [ ] **Step 6: fmt + commit** `cargo fmt && cargo fmt -- --check`; `git add -A && git commit -q -m "feat(io/omn): Manchester class-expression grammar + lexer (lex-only)"`

---

### Task 2: `Context` + `FromPair` scaffold + leaf conversions

**Files:** `src/io/omn/reader/from_pair.rs`, `src/io/omn/reader/mod.rs`.

- [ ] **Step 1: Read ofn's `Context` + `FromPair` for `IRI`/`Class`/`ObjectPropertyExpression`** (`src/io/ofn/reader/from_pair.rs` lines ~846 IRI, ~923 OPE; `Context` in `ofn/reader/mod.rs`). Note how `Context` holds the `PrefixMapping` + `Build`, and how an IRI pair (full `<...>` vs prefixed `p:l`) is resolved to an `IRI<A>`.

- [ ] **Step 2: Define omn's `Context` + `FromPair` trait** in `from_pair.rs`, mirroring ofn but keyed to omn's `Rule` (from `lexer::Rule`). The `Context<'a, A>` = `{ build: &'a Build<A>, prefixes: &'a PrefixMapping }` (copy ofn's shape). The `FromPair<A>` trait = ofn's (RULE const + `from_pair`/`from_pair_unchecked(pair, ctx)`).

- [ ] **Step 3: failing test** (a leaf IRI→Class round-trip):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Build;
    fn ctx_parse_class(s: &str) -> Class<crate::model::RcStr> {
        let b = Build::new_rc();
        let pm = curie::PrefixMapping::default();
        let ctx = Context::new(&b, &pm);
        let mut pairs = crate::io::omn::reader::ManchesterLexer::lex(super::super::lexer::Rule::ClassIRI, s).unwrap();
        Class::from_pair(pairs.next().unwrap(), &ctx).unwrap()
    }
    #[test]
    fn parses_class_iri() {
        assert_eq!(ctx_parse_class("<http://t/A>"), Build::new_rc().class("http://t/A"));
    }
}
```
(Adapt `Context::new` + the `Rule::ClassIRI` path + the `from_pair` call to the real shapes from Step 2 / ofn.)

- [ ] **Step 4: implement** `FromPair` for the leaves used by class expressions: `IRI<A>` (resolve full/prefixed via `Context.prefixes` + `Context.build.iri(...)` — mirror ofn's IRI impl), `Class<A>` (wrap an IRI pair), `ObjectProperty<A>`/`DataProperty<A>`/`Datatype<A>` (wrap IRI), `ObjectPropertyExpression<A>` (the `ope` rule → `ObjectProperty` or `InverseObjectProperty`), `Individual<A>` (named IRI → `Individual::Named`), `Literal<A>` (reuse ofn's literal-pair handling — mirror it), `DataRange<A>` (`Datatype` + optional facets; `Facet`/`FacetRestriction` via the `FacetSymbol`→`Facet` inverse of the writer's `facet_symbol`). Add a `facet_from_symbol(&str) -> Facet` (inverse of P1's `facet_symbol`).

- [ ] **Step 5: run** `cargo test --lib io::omn::reader::from_pair 2>&1 | tail -10` (leaf test passes).

- [ ] **Step 6: fmt + commit** `feat(io/omn): reader Context + FromPair leaf conversions (IRI/Class/property/individual/literal/datarange)`.

---

### Task 3: `FromPair for ClassExpression` (precedence layers + restrictions)

**Files:** `src/io/omn/reader/from_pair.rs`, `src/io/omn/reader/mod.rs`.

- [ ] **Step 1: failing test** — the public `parse_class_expression` entry + structural asserts:
```rust
#[test]
fn parses_class_expressions() {
    use crate::model::*;
    let b = Build::new_rc();
    let pm = curie::PrefixMapping::default();
    let p = |s: &str| crate::io::omn::reader::parse_class_expression::<RcStr>(s, &pm, &b).unwrap();
    let a = ClassExpression::Class(b.class("http://t/A"));
    let c = ClassExpression::Class(b.class("http://t/C"));
    assert_eq!(p("<http://t/A>"), a);
    assert_eq!(p("<http://t/A> and <http://t/C>"),
        ClassExpression::ObjectIntersectionOf(vec![a.clone(), c.clone()]));
    assert_eq!(p("not <http://t/A>"), ClassExpression::ObjectComplementOf(Box::new(a.clone())));
    // precedence: (A or C) and D groups correctly
    let d = ClassExpression::Class(b.class("http://t/D"));
    let aorc = ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone()]);
    assert_eq!(p("(<http://t/A> or <http://t/C>) and <http://t/D>"),
        ClassExpression::ObjectIntersectionOf(vec![aorc, d]));
    let r = ObjectPropertyExpression::ObjectProperty(b.object_property("http://t/r"));
    assert_eq!(p("<http://t/r> some <http://t/A>"),
        ClassExpression::ObjectSomeValuesFrom { ope: r, bce: Box::new(a) });
}
```

- [ ] **Step 2: run, expect fail** (`parse_class_expression` undefined).

- [ ] **Step 3: implement `FromPair for ClassExpression<A>`** over the precedence-layer rules + a `parse_class_expression` entry in `reader/mod.rs`:
  - `Description` pair: collect the `Conjunction` children; 1 child → that child's CE; ≥2 → `ObjectUnionOf(children)`.
  - `Conjunction` pair: collect `Primary` children; 1 → that; ≥2 → `ObjectIntersectionOf(children)`.
  - `Primary` pair: if it begins with `not`, `ObjectComplementOf(Box::new(inner))`; else the `Restriction`/`Atomic` child's CE.
  - `Atomic` pair: `ClassIRI` → `Class`; `ObjectOneOf` → `ObjectOneOf(individuals)`; parenthesized `Description` → recurse.
  - `Restriction` pair: dispatch on the keyword token + property kind (object vs data) → the matching `ClassExpression` variant (`ObjectSomeValuesFrom`/`AllValuesFrom`/`HasValue`/`HasSelf`/`MinCardinality`/etc. and the Data* analogues), recursing into `Primary`/`Individual`/`DataRange`/`Literal`/`Cardinality`. The keyword (`some`/`only`/`value`/`Self`/`min`/`max`/`exactly`) determines the variant; `Cardinality` → `n: u32`; an optional trailing `Primary`/`DataRange` is the qualifier (qualified cardinality).
  
  `parse_class_expression<A>(s, pm, build) -> Result<ClassExpression<A>>`: `ManchesterLexer::lex(Rule::ClassExpressionDocument, s)?`, take the `Description` pair, `ClassExpression::from_pair(pair, &Context::new(build, pm))`.

  This is the inverse of the P1 writer's `ClassExpression` rendering. Cross-check variant/field names (`ObjectSomeValuesFrom{ope,bce}`, `ObjectMinCardinality{n,ope,bce}`, etc.) against `src/model.rs` (same names the writer used). Match the actual omn `Rule` enum names produced by your grammar (inspect a parse tree with a debug print if needed).

- [ ] **Step 4: run, expect PASS** — all `parses_class_expressions` cases, esp. the precedence one `(A or C) and D`. `cargo test --lib io::omn::reader 2>&1 | tail -12`.

- [ ] **Step 5: fmt + clippy** (`cargo fmt -- --check` rc 0; `cargo clippy --lib 2>&1 | grep -iA3 omn | head` — no omn warnings). **Step 6: commit** `feat(io/omn): FromPair for ClassExpression (precedence layers + restrictions)`.

---

### Task 4: round-trip gate (writer ⊕ reader) + commit

**Files:** `src/io/omn/reader/from_pair.rs` (test module) — or a new `src/io/omn/tests.rs`.

- [ ] **Step 1: write the round-trip test** — the strongest correctness gate: every class expression the P1 writer emits must parse back to the same value.
```rust
#[test]
fn class_expression_round_trips() {
    use crate::model::*;
    use crate::io::omn::AsManchester;
    let b = Build::new_rc();
    let pm = curie::PrefixMapping::default();
    let a = ClassExpression::Class(b.class("http://t/A"));
    let c = ClassExpression::Class(b.class("http://t/C"));
    let d = ClassExpression::Class(b.class("http://t/D"));
    let r = ObjectPropertyExpression::ObjectProperty(b.object_property("http://t/r"));
    let cases: Vec<ClassExpression<RcStr>> = vec![
        a.clone(),
        ClassExpression::ObjectIntersectionOf(vec![a.clone(), c.clone()]),
        ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone(), d.clone()]),
        ClassExpression::ObjectComplementOf(Box::new(a.clone())),
        // precedence-sensitive nestings
        ClassExpression::ObjectIntersectionOf(vec![
            ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone()]), d.clone()]),
        ClassExpression::ObjectComplementOf(Box::new(
            ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone()]))),
        ClassExpression::ObjectSomeValuesFrom { ope: r.clone(), bce: Box::new(a.clone()) },
        ClassExpression::ObjectAllValuesFrom { ope: r.clone(), bce: Box::new(
            ClassExpression::ObjectUnionOf(vec![a.clone(), c.clone()])) },
        ClassExpression::ObjectMinCardinality { n: 2, ope: r.clone(), bce: Box::new(a.clone()) },
    ];
    for ce in &cases {
        let rendered = ce.as_manchester().to_string();
        let parsed = crate::io::omn::reader::parse_class_expression::<RcStr>(&rendered, &pm, &b)
            .unwrap_or_else(|e| panic!("parse failed for {rendered:?}: {e}"));
        assert_eq!(&parsed, ce, "round-trip mismatch: {rendered:?}");
    }
}
```

- [ ] **Step 2: run** `cargo test --lib io::omn 2>&1 | tail -12`. The round-trip MUST pass for every case — this proves writer⊕reader consistency, including precedence (`(A or C) and D`, `not (A or C)`, `r only (A or C)`). If a case fails, the writer's parenthesization and the reader's precedence layering disagree — fix the grammar/FromPair (the writer is the spec'd reference). Do not weaken the test.

- [ ] **Step 3: fmt + clippy + full omn test run** (`cargo test --lib io::omn 2>&1 | tail -4` — writer + reader tests all green; `cargo fmt -- --check`; clippy no omn warnings).

- [ ] **Step 4: confirm rustdl still builds against the fork** (the reader is additive; rustdl uses the writer): `cd /data/dumontier/rustdl && cargo build -p owl-dl-reasoner 2>&1 | tail -2`. (Note: rustdl's `[patch]` is rev-pinned to `dc89415` — it will NOT pick up the new reader commits until the rev is bumped; that's fine, rustdl only needs the writer. Do NOT bump the rustdl rev in P2.)

- [ ] **Step 5: commit (fork)** `git add -A && git commit -q -m "test(io/omn): class-expression writer⊕reader round-trip gate"`.

---

## Notes for the implementer

- **Work in the fork** `/data/dumontier/horned-owl-omn` (branch `manchester-io`); commit there. rustdl is untouched in P2 (its `[patch]` rev stays pinned to the writer commit `dc89415` — the reader isn't needed by rustdl).
- **The grammar (`omn.pest`) is the deliverable's core** — get the precedence layering right (`Description`/`Conjunction`/`Primary`), and the `Restriction` ordered-choice. Use the lex-only tests (Task 1) to validate the grammar before wiring `FromPair`.
- **`FromPair` mirrors ofn's trait + `Context`** but is omn's own (keyed to omn's `Rule`). The CE conversion is the *inverse* of the P1 writer — match the same `model.rs` variant/field names the writer used.
- **The round-trip test (Task 4) is the gate** — writer⊕reader must agree on every construct, especially precedence/parenthesization. It's the P2 acceptance criterion.
- **Iterate `FromPair` against the compiler + the generated `Rule` names** — pest generates the `Rule` enum from `omn.pest`; the exact variant names match your rule names. A `dbg!(pair)` on a parse tree shows the structure if a `from_pair` arm doesn't match.
- **Do NOT push** the fork (the upstream PR is the user's; it comes after P3 + the pre-PR polish).
- **P3 (next plan):** frames (`Class:`/`ObjectProperty:`/`Individual:`/… with clauses) + the full ontology-document `read` (prefixes + `Ontology:` header + frames) + whole-ontology round-trip. P3 reuses this CE reader.
