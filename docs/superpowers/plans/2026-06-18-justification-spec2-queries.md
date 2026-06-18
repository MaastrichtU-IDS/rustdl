# Justification Spec 2 — Property & Individual Queries (incl. data) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** Add black-box justification query types for object properties, individuals, and (now that the data engine is in) data-property subsumption — extending the existing `justify.rs` engine, CLI, and ⊥-module.

**Architecture:** Each new query reduces to a consistency check on the ontology plus a fresh injected "negation" probe (the pattern the existing `DisjointClasses` query already uses). Probes are injected inside `entails`, never appear as candidate axioms, so never show up in a justification. Object reductions use a single inconsistency check; data-subsumption uses a two-check guard (soundness — see Task 1). Builds on `feat/data-properties-subproject1` (data gate now default-ON).

**Specs:** `docs/superpowers/specs/2026-06-17-justification-spec2-property-individual-queries-design.md` (object/individual) + the data-query addendum here.

## New query types
| Query | CLI verb | Reduction |
|---|---|---|
| `SubObjectProperty {sub,sup}` (P⊑Q) | `subproperty P Q` | `¬consistent(O ∪ {P(_a,_b), ¬Q(_a,_b)})` |
| `EquivalentObjectProperties {a,b}` | `equiv-property P Q` | `Sub(P,Q) ∧ Sub(Q,P)` |
| `DisjointObjectProperties {a,b}` | `disjoint-property P Q` | `¬consistent(O ∪ {P(_a,_b), Q(_a,_b)})` |
| `ObjectPropertyAssertion {source,prop,target}` (a P b) | `property A P B` | `¬consistent(O ∪ {¬P(a,b)})` |
| `SameIndividual {a,b}` (a=b) | `same A B` | `¬consistent(O ∪ {Different(a,b)})` |
| `DifferentIndividuals {a,b}` (a≠b) | `different A B` | `¬consistent(O ∪ {Same(a,b)})` |
| `SubDataProperty {sub,sup}` (dp⊑dq) | `subdata-property DP DQ` | guarded: `consistent(O∪{dp(_a,v)}) ∧ ¬consistent(O∪{dp(_a,v), ¬dq(_a,v)})` |
| `EquivalentDataProperties {a,b}` (dp≡dq) | `equiv-data-property DP DQ` | `SubData(dp,dq) ∧ SubData(dq,dp)` |

**Excluded (documented non-goals):** `Disjoint(dp,dq)` query — blocked by deferred gap 2 (disjoint-dp value-identity; the probe wouldn't clash). `a dp v` data assertion — needs CLI literal parsing; defer.

## Files
- `crates/owl-dl-reasoner/src/justify.rs` — `Entailment` (+8 variants), `entails` (+8 arms), `query_seed_signature` (+8 arms), `inconsistent_with` helper, `PROBE_A`/`PROBE_B` consts.
- `crates/owl-dl-cli/src/main.rs` — `parse_justify_query` (+8 verbs) + help text.
- `crates/owl-dl-reasoner/tests/justification.rs` — canaries.

## Reference shapes (verified)
- Existing: `entails`, `query_seed_signature`, `PROBE_IRI = "urn:rustdl-justify-probe"`, `find_one_justification`/`find_all_justifications` (which call `localized_candidates`→`query_seed_signature`).
- horned-owl model: `Component::ObjectPropertyAssertion(ObjectPropertyAssertion { ope, from, to })`, `NegativeObjectPropertyAssertion { ope, from, to }`, `DataPropertyAssertion { dp, from, to }` (`to: Literal`), `NegativeDataPropertyAssertion { dp, from, to }`, `SameIndividual(Vec<Individual>)`, `DifferentIndividuals(Vec<Individual>)`. `ObjectPropertyExpression::ObjectProperty(ObjectProperty)`. `Individual::Named(NamedIndividual)`. `Literal::Datatype { literal: String, datatype_iri: IRI }`.
- `Build<A>`: `build.object_property(iri)`, `build.data_property(iri)`, `build.named_individual(iri)`, `build.iri(iri)`. The existing `DisjointClasses` arm constructs `let build: Build<A> = Build::new();`.
- Public reasoner: `crate::is_consistent(onto)`, `crate::is_subclass_of`, etc.

---

### Task 1: justify.rs — new Entailment variants, reductions, seed

**Files:** `crates/owl-dl-reasoner/src/justify.rs`.

- [ ] **Step 1: imports.** Ensure these horned-owl model types are imported (add to the existing `use horned_owl::model::{...}`): `DataPropertyAssertion, DifferentIndividuals, Individual, Literal, NegativeDataPropertyAssertion, NegativeObjectPropertyAssertion, ObjectPropertyAssertion, ObjectPropertyExpression, SameIndividual`. (`Build, ClassExpression, Component, EquivalentClasses, ForIRI` already there; `Individual`/`ObjectPropertyExpression` likely already there from the ⊥-module work — don't duplicate.)

- [ ] **Step 2: probe consts** near `PROBE_IRI`:
```rust
const PROBE_A: &str = "urn:rustdl-justify-probe-a";
const PROBE_B: &str = "urn:rustdl-justify-probe-b";
const PROBE_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";
```

- [ ] **Step 3: `inconsistent_with` helper** (after `entails`):
```rust
/// `true` iff `onto ∪ extra` is inconsistent. The `extra` axioms are query-
/// encoding probes (fresh `PROBE_*` symbols), never candidate axioms — they
/// appear in every tested subset and never in a justification.
fn inconsistent_with<A: ForIRI>(
    onto: &SetOntology<A>,
    extra: Vec<Component<A>>,
) -> Result<bool, ReasonError> {
    let mut probed = onto.clone();
    for c in extra {
        probed.insert(c);
    }
    Ok(!crate::is_consistent(&probed)?)
}

fn named<A: ForIRI>(b: &Build<A>, iri: &str) -> Individual<A> {
    Individual::Named(b.named_individual(iri))
}
fn ope<A: ForIRI>(b: &Build<A>, iri: &str) -> ObjectPropertyExpression<A> {
    ObjectPropertyExpression::ObjectProperty(b.object_property(iri))
}
```

- [ ] **Step 4: `Entailment` enum — add 8 variants** (after `Inconsistent`):
```rust
    SubObjectProperty { sub: String, sup: String },
    EquivalentObjectProperties { a: String, b: String },
    DisjointObjectProperties { a: String, b: String },
    ObjectPropertyAssertion { source: String, prop: String, target: String },
    SameIndividual { a: String, b: String },
    DifferentIndividuals { a: String, b: String },
    SubDataProperty { sub: String, sup: String },
    EquivalentDataProperties { a: String, b: String },
```

- [ ] **Step 5: `entails` — add 8 arms.** Object (single check, sound — a fresh filler clashes only if the property is empty, making the subsumption vacuously true):
```rust
        Entailment::SubObjectProperty { sub, sup } => {
            let b: Build<A> = Build::new();
            inconsistent_with(onto, vec![
                Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                    ope: ope(&b, sub), from: named(&b, PROBE_A), to: named(&b, PROBE_B),
                }),
                Component::NegativeObjectPropertyAssertion(NegativeObjectPropertyAssertion {
                    ope: ope(&b, sup), from: named(&b, PROBE_A), to: named(&b, PROBE_B),
                }),
            ])
        }
        Entailment::EquivalentObjectProperties { a, b } => Ok(entails(
            onto, &Entailment::SubObjectProperty { sub: a.clone(), sup: b.clone() })?
            && entails(onto, &Entailment::SubObjectProperty { sub: b.clone(), sup: a.clone() })?),
        Entailment::DisjointObjectProperties { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(onto, vec![
                Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                    ope: ope(&bld, a), from: named(&bld, PROBE_A), to: named(&bld, PROBE_B),
                }),
                Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                    ope: ope(&bld, b), from: named(&bld, PROBE_A), to: named(&bld, PROBE_B),
                }),
            ])
        }
        Entailment::ObjectPropertyAssertion { source, prop, target } => {
            let b: Build<A> = Build::new();
            inconsistent_with(onto, vec![
                Component::NegativeObjectPropertyAssertion(NegativeObjectPropertyAssertion {
                    ope: ope(&b, prop), from: named(&b, source), to: named(&b, target),
                }),
            ])
        }
        Entailment::SameIndividual { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(onto, vec![Component::DifferentIndividuals(
                DifferentIndividuals(vec![named(&bld, a), named(&bld, b)]))])
        }
        Entailment::DifferentIndividuals { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(onto, vec![Component::SameIndividual(
                SameIndividual(vec![named(&bld, a), named(&bld, b)]))])
        }
```
Data (GUARDED two-check — **soundness crux**: a single check would false-positive when the probe value violates `sub`'s range; the `c1` baseline guards it):
```rust
        Entailment::SubDataProperty { sub, sup } => {
            let b: Build<A> = Build::new();
            let lit = || Literal::Datatype {
                literal: "0".to_string(),
                datatype_iri: b.iri(PROBE_INT),
            };
            let dp_assert = |dp: &str| Component::DataPropertyAssertion(DataPropertyAssertion {
                dp: b.data_property(dp), from: named(&b, PROBE_A), to: lit(),
            });
            // c1: asserting sub(_a,0) alone must be consistent (else range clash, not subsumption).
            if inconsistent_with(onto, vec![dp_assert(sub)])? {
                return Ok(false);
            }
            // c2: adding ¬sup(_a,0) becomes inconsistent ⟺ sub⊑sup forces sup(_a,0).
            inconsistent_with(onto, vec![
                dp_assert(sub),
                Component::NegativeDataPropertyAssertion(NegativeDataPropertyAssertion {
                    dp: b.data_property(sup), from: named(&b, PROBE_A), to: lit(),
                }),
            ])
        }
        Entailment::EquivalentDataProperties { a, b } => Ok(entails(
            onto, &Entailment::SubDataProperty { sub: a.clone(), sup: b.clone() })?
            && entails(onto, &Entailment::SubDataProperty { sub: b.clone(), sup: a.clone() })?),
```
(If `b.iri(...)` borrow-conflicts inside the `lit`/`dp_assert` closures, build the literal/components without closures — inline `Build::new()` per component as the existing arms do. Keep behavior identical.)

- [ ] **Step 6: `query_seed_signature` — add 8 arms** (all have bounded signatures, so the ⊥-module applies):
```rust
        Entailment::SubObjectProperty { sub, sup }
        | Entailment::SubDataProperty { sub, sup } => {
            s.insert(sub.clone());
            s.insert(sup.clone());
        }
        Entailment::EquivalentObjectProperties { a, b }
        | Entailment::DisjointObjectProperties { a, b }
        | Entailment::SameIndividual { a, b }
        | Entailment::DifferentIndividuals { a, b }
        | Entailment::EquivalentDataProperties { a, b } => {
            s.insert(a.clone());
            s.insert(b.clone());
        }
        Entailment::ObjectPropertyAssertion { source, prop, target } => {
            s.insert(source.clone());
            s.insert(prop.clone());
            s.insert(target.clone());
        }
```
(Place these before the existing `Entailment::Inconsistent => return None` arm.)

- [ ] **Step 7: build + clippy.**
Run: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; cargo build -p owl-dl-reasoner 2>&1 | tail -3; cargo clippy -p owl-dl-reasoner --lib -- -D warnings 2>&1 | tail -5`
Expected: compiles clean. (Tests come in Task 3.)

- [ ] **Step 8: commit.**
```bash
git add crates/owl-dl-reasoner/src/justify.rs
git commit -m "feat(justify): property/individual + data-subproperty query reductions"
```

---

### Task 2: CLI verbs + help

**Files:** `crates/owl-dl-cli/src/main.rs` (`parse_justify_query`).

- [ ] **Step 1: extend `parse_justify_query`** to map the new verbs (read the existing fn first; it matches on `parts[0]` and validates arity). Add:
```rust
        "subproperty" => need(2, Entailment::SubObjectProperty { sub: a(0), sup: a(1) }),
        "equiv-property" => need(2, Entailment::EquivalentObjectProperties { a: a(0), b: a(1) }),
        "disjoint-property" => need(2, Entailment::DisjointObjectProperties { a: a(0), b: a(1) }),
        "property" => need(3, Entailment::ObjectPropertyAssertion { source: a(0), prop: a(1), target: a(2) }),
        "same" => need(2, Entailment::SameIndividual { a: a(0), b: a(1) }),
        "different" => need(2, Entailment::DifferentIndividuals { a: a(0), b: a(1) }),
        "subdata-property" => need(2, Entailment::SubDataProperty { sub: a(0), sup: a(1) }),
        "equiv-data-property" => need(2, Entailment::EquivalentDataProperties { a: a(0), b: a(1) }),
```
Adapt to the EXACT shape of the existing parser (it may not use `need`/`a()` helpers — match its actual style: the existing arms build `Entailment::SubClassOf { sub: parts[1].clone(), sup: parts[2].clone() }` after an arity check. Mirror that; do not invent helpers that don't exist). Update the usage/error string and the `Justify` command doc comment to list the new verbs.

- [ ] **Step 2: build + a smoke run.**
```bash
cargo build --release -p owl-dl-cli 2>&1 | tail -1
./target/release/rustdl justify --help 2>&1 | head -5
```

- [ ] **Step 3: commit.**
```bash
git add crates/owl-dl-cli/src/main.rs
git commit -m "feat(cli): justify verbs for property/individual + data-subproperty queries"
```

---

### Task 3: canaries

**Files:** `crates/owl-dl-reasoner/tests/justification.rs`.

- [ ] **Step 1: add per-query canaries** (entailed + not-entailed each), using the existing `onto(body)` helper. The data tests need data-property axioms (gate is default-ON now, no env needed). Probe-never-in-output check included.
```rust
#[test]
fn justify_subproperty() {
    let o = onto("Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:r))\n\
                  SubObjectPropertyOf(:p :q) SubObjectPropertyOf(:q :r)");
    let q = Entailment::SubObjectProperty { sub: "http://t/p".into(), sup: "http://t/r".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("p⊑r entailed");
    assert_eq!(j.axioms.len(), 2, "got {:?}", j.axioms);
    // not entailed:
    let nq = Entailment::SubObjectProperty { sub: "http://t/r".into(), sup: "http://t/p".into() };
    assert!(find_one_justification(&o, &nq).unwrap().is_none());
}

#[test]
fn justify_object_property_assertion() {
    let o = onto("Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
                  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
                  SubObjectPropertyOf(:p :q) ObjectPropertyAssertion(:p :a :b)");
    let q = Entailment::ObjectPropertyAssertion { source: "http://t/a".into(), prop: "http://t/q".into(), target: "http://t/b".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("a q b entailed");
    assert_eq!(j.axioms.len(), 2);
}

#[test]
fn justify_same_individual() {
    let o = onto("Declaration(ObjectProperty(:p)) Declaration(NamedIndividual(:x)) \
                  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
                  FunctionalObjectProperty(:p) ObjectPropertyAssertion(:p :x :a) ObjectPropertyAssertion(:p :x :b)");
    let q = Entailment::SameIndividual { a: "http://t/a".into(), b: "http://t/b".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("a=b via functional");
    assert!(!j.axioms.is_empty());
}

#[test]
fn justify_different_individuals() {
    let o = onto("Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
                  DifferentIndividuals(:a :b)");
    let q = Entailment::DifferentIndividuals { a: "http://t/a".into(), b: "http://t/b".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("a≠b told");
    assert_eq!(j.axioms.len(), 1);
}

#[test]
fn justify_disjoint_object_properties() {
    let o = onto("Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
                  DisjointObjectProperties(:p :q)");
    let q = Entailment::DisjointObjectProperties { a: "http://t/p".into(), b: "http://t/q".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("p,q disjoint told");
    assert_eq!(j.axioms.len(), 1);
}

#[test]
fn justify_subdata_property() {
    // data gate is default-ON
    let o = onto("Declaration(DataProperty(:dp)) Declaration(DataProperty(:dq)) Declaration(DataProperty(:dr))\n\
                  SubDataPropertyOf(:dp :dq) SubDataPropertyOf(:dq :dr)");
    let q = Entailment::SubDataProperty { sub: "http://t/dp".into(), sup: "http://t/dr".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("dp⊑dr entailed");
    assert_eq!(j.axioms.len(), 2, "got {:?}", j.axioms);
    let nq = Entailment::SubDataProperty { sub: "http://t/dr".into(), sup: "http://t/dp".into() };
    assert!(find_one_justification(&o, &nq).unwrap().is_none());
}

#[test]
fn justify_probe_symbols_never_in_output() {
    let o = onto("Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
                  SubObjectPropertyOf(:p :q)");
    let q = Entailment::SubObjectProperty { sub: "http://t/p".into(), sup: "http://t/q".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("entailed");
    for ax in &j.axioms {
        let s = format!("{ax:?}");
        assert!(!s.contains("rustdl-justify-probe"), "probe leaked: {s}");
    }
}
```

- [ ] **Step 2: run + clippy + fmt.**
```bash
cargo test -p owl-dl-reasoner --test justification 2>&1 | tail -15
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
cargo fmt --all
```
All pass. If `justify_subdata_property` not-entailed case returns Some (FP) or the entailed case returns None (miss), STOP and report.

- [ ] **Step 3: commit.**
```bash
git add crates/owl-dl-reasoner/tests/justification.rs
git commit -m "test(justify): canaries for property/individual + data-subproperty queries"
```

---

### Task 4: docs + wrap-up

- [ ] **Step 1:** In `docs/superpowers/specs/2026-06-17-justification-spec2-property-individual-queries-design.md`, add a short "Implemented (2026-06-18)" note: object/individual queries + data sub/equiv-property shipped; `Disjoint(dp,dq)` query and `a dp v` data-assertion query deferred (gap 2 / CLI literal parsing).
- [ ] **Step 2:** `cargo test -p owl-dl-reasoner 2>&1 | grep 'test result' | tail` and `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3` — all green.
- [ ] **Step 3:** Commit docs. Do NOT push.

## Soundness notes for the implementer
- **Object** subproperty/disjoint/assertion/same/different: single inconsistency check is sound (rustdl consistency is sound ⇒ no false-positive entailment; a fresh-filler domain/range clash implies the property is empty ⇒ the subsumption is vacuously true).
- **Data** subproperty: the `c1` baseline check is mandatory — without it, a probe value violating `sub`'s range would report inconsistency unrelated to `sub⊑sup` = a false positive. Never drop the guard.
- `find_one`'s quickxplain re-verifies entailment of the returned core, and the ⊥-module is entailment-checked, so a returned justification always genuinely entails (the Spec 1 contract).
