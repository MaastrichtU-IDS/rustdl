# Justification foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give rustdl black-box **justifications** — for an entailment it reports, return a minimal set of the user's axioms responsible for it (find-one, then find-all), across 6 class/individual query types, via CLI + API.

**Architecture:** A new `crates/owl-dl-reasoner/src/justify.rs` drives the *public* reasoner API (`is_subclass_of`/`is_class_satisfiable`/`is_consistent`/`is_instance_of`) — no engine internals. An `entails` oracle reduces each query to those checks; QuickXplain minimizes over the ontology's logical axioms (rebuilding a `SetOntology` per check); results carry a fragment-based minimality guarantee.

**Tech Stack:** Rust 2024, `owl-dl-reasoner`, horned-owl model (`SetOntology`, `Component`, `Build`), `FragmentClassification`.

Spec: `docs/superpowers/specs/2026-06-12-justification-foundation-design.md`.

---

## File structure

- **Create** `crates/owl-dl-reasoner/src/justify.rs` — `Entailment`, `entails`, axiom partition + `ontology_from`, `quickxplain`, `find_one_justification`, `find_all_justifications`, `Justification`.
- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — `pub mod justify;` + re-export the public items.
- **Create** `crates/owl-dl-reasoner/tests/justification.rs` — canaries + correctness invariants.
- **Modify** `crates/owl-dl-cli/src/main.rs` — `Justify` subcommand.

Key API facts (verified):
- `is_subclass_of(&SetOntology<A>, sub: &str, sup: &str) -> Result<bool, ReasonError>`
- `is_class_satisfiable(&SetOntology<A>, class: &str) -> Result<bool, ReasonError>`
- `is_consistent(&SetOntology<A>) -> Result<bool, ReasonError>`
- `is_instance_of(&SetOntology<A>, class_iri: &str, individual_iri: &str) -> Result<bool, ReasonError>` — **class first, individual second.**
- `analyze_fragment(&InternalOntology) -> FragmentClassification` (`classify.rs`); variants `PureEl | Horn | OutOfFragment`. Convert an ontology via `owl_dl_core::convert::convert_ontology(&onto)` then `analyze_fragment(&internal)`.
- Iterate an ontology: `for ac in &onto { match &ac.component { Component::SubClassOf(..) => .. } }` (each item is `&AnnotatedComponent<A>` with `.component: Component<A>`).
- Build one: `let mut o = SetOntology::new(); o.insert(component);` (`MutableOntology::insert`, `Component<A>: Into<AnnotatedComponent<A>>`). Build probe terms with `horned_owl::model::Build::new_rc()` / `Build::<RcStr>::new()` → `build.class("iri")`, `ClassExpression::Class(..)`, `ClassExpression::ObjectIntersectionOf(vec![..])` (see `convert_back.rs` imports for the full model surface).

---

### Task 1: `Entailment` enum + `entails` oracle

**Files:**
- Create: `crates/owl-dl-reasoner/src/justify.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs`
- Create: `crates/owl-dl-reasoner/tests/justification.rs`

- [ ] **Step 1: Write the failing test** (`crates/owl-dl-reasoner/tests/justification.rs`)

```rust
//! Canaries for black-box justification (find-one / find-all).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, entails};
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nOntology(<http://t/o>\n{body}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    o
}

#[test]
fn entails_subclassof_chain() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         SubClassOf(:A :B) SubClassOf(:B :C)",
    );
    assert!(entails(&o, &Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/C".into() }).unwrap());
    assert!(!entails(&o, &Entailment::SubClassOf { sub: "http://t/C".into(), sup: "http://t/A".into() }).unwrap());
}

#[test]
fn entails_disjoint_via_probe() {
    // A ⊑ B, A ⊑ ¬B makes A unsat; but disjointness of B and C requires an
    // explicit clash. Here: D ⊑ E, D ⊑ ObjectComplementOf(E) is unsat, but to
    // test DisjointClasses(B,C): assert B,C disjoint iff B⊓C unsat.
    let o = onto(
        "Declaration(Class(:B)) Declaration(Class(:C))\n\
         DisjointClasses(:B :C)",
    );
    assert!(entails(&o, &Entailment::DisjointClasses { a: "http://t/B".into(), b: "http://t/C".into() }).unwrap());
    let o2 = onto("Declaration(Class(:B)) Declaration(Class(:C))");
    assert!(!entails(&o2, &Entailment::DisjointClasses { a: "http://t/B".into(), b: "http://t/C".into() }).unwrap());
}
```

- [ ] **Step 2: Run to verify it fails** (`justify` module/`entails` undefined)

Run: `cargo test -p owl-dl-reasoner --test justification entails_ 2>&1 | tail -15`
Expected: compile error (`owl_dl_reasoner::justify` not found).

- [ ] **Step 3: Create `justify.rs` with the enum + oracle**

```rust
//! Black-box justification: minimal responsible-axiom sets for an
//! entailment, found by re-checking subsets of the ontology's axioms via
//! the public reasoner API. No engine internals.

use horned_owl::model::{
    Build, ClassExpression, Component, EquivalentClasses, ForIRI,
};
use horned_owl::ontology::set::SetOntology;
use horned_owl::model::MutableOntology;

use crate::ReasonError;

/// An entailment to justify ("why does this hold?").
#[derive(Debug, Clone)]
pub enum Entailment {
    SubClassOf { sub: String, sup: String },
    EquivalentClasses { a: String, b: String },
    DisjointClasses { a: String, b: String },
    Unsatisfiable { class: String },
    InstanceOf { individual: String, class: String },
    Inconsistent,
}

const PROBE_IRI: &str = "urn:rustdl-justify-probe";

/// Does `onto` entail `q`? Reduces to the public reasoner checks. The
/// `DisjointClasses` case injects a fresh probe class `X ≡ a ⊓ b` and checks
/// `X` unsatisfiable (the probe is query encoding, never part of a result).
pub fn entails<A: ForIRI>(onto: &SetOntology<A>, q: &Entailment) -> Result<bool, ReasonError> {
    match q {
        Entailment::SubClassOf { sub, sup } => crate::is_subclass_of(onto, sub, sup),
        Entailment::EquivalentClasses { a, b } => {
            Ok(crate::is_subclass_of(onto, a, b)? && crate::is_subclass_of(onto, b, a)?)
        }
        Entailment::DisjointClasses { a, b } => {
            let mut probed = onto.clone();
            let build: Build<A> = Build::new();
            let x = build.class(PROBE_IRI);
            // X ≡ a ⊓ b
            probed.insert(Component::EquivalentClasses(EquivalentClasses(vec![
                ClassExpression::Class(x.clone()),
                ClassExpression::ObjectIntersectionOf(vec![
                    ClassExpression::Class(build.class(a.as_str())),
                    ClassExpression::Class(build.class(b.as_str())),
                ]),
            ])));
            Ok(!crate::is_class_satisfiable(&probed, PROBE_IRI)?)
        }
        Entailment::Unsatisfiable { class } => Ok(!crate::is_class_satisfiable(onto, class)?),
        // NOTE: is_instance_of is (class, individual) — class first.
        Entailment::InstanceOf { individual, class } => {
            crate::is_instance_of(onto, class, individual)
        }
        Entailment::Inconsistent => Ok(!crate::is_consistent(onto)?),
    }
}
```

In `crates/owl-dl-reasoner/src/lib.rs`, add near the other `mod`/`pub use` lines:
```rust
pub mod justify;
```
(Confirm `is_subclass_of`, `is_class_satisfiable`, `is_consistent`, `is_instance_of` are `pub` at crate root — they are; `justify` calls them as `crate::`.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p owl-dl-reasoner --test justification entails_ 2>&1 | tail -8`
Expected: both `entails_*` PASS. If `Build::new()` type inference fails, annotate `let build: Build<A> = Build::new();` (already done) or use `Build::new_rc()` in the RcStr-concrete spots — match what `convert_back.rs` does.

- [ ] **Step 5: clippy + fmt**

Run: `cargo fmt --all && cargo fmt --all -- --check` (rc 0) and `cargo clippy -p owl-dl-reasoner --all-targets --all-features -- -D warnings 2>&1 | tail -4` (clean).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/justify.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/justification.rs
git commit -m "feat(justify): Entailment query enum + entails oracle (6 query types)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Axiom partition + subset ontology builder

**Files:**
- Modify: `crates/owl-dl-reasoner/src/justify.rs`

- [ ] **Step 1: Write the failing test** (append to `tests/justification.rs`)

```rust
use owl_dl_reasoner::justify::{logical_axioms, ontology_from};

#[test]
fn partition_and_rebuild() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         SubClassOf(:A :B) SubClassOf(:B :C)",
    );
    let (fixed, candidates) = logical_axioms(&o);
    assert_eq!(candidates.len(), 2, "two SubClassOf axioms are candidates");
    assert!(fixed.len() >= 3, "declarations are fixed; got {}", fixed.len());
    // Rebuild from fixed + both candidates → still entails A⊑C.
    let rebuilt = ontology_from(&fixed, &candidates);
    assert!(entails(&rebuilt, &Entailment::SubClassOf {
        sub: "http://t/A".into(), sup: "http://t/C".into() }).unwrap());
    // Rebuild from fixed + only the first candidate → does NOT entail A⊑C.
    let rebuilt1 = ontology_from(&fixed, &candidates[..1]);
    assert!(!entails(&rebuilt1, &Entailment::SubClassOf {
        sub: "http://t/A".into(), sup: "http://t/C".into() }).unwrap());
}
```

- [ ] **Step 2: Run to verify it fails** (`logical_axioms`/`ontology_from` undefined)

Run: `cargo test -p owl-dl-reasoner --test justification partition_and_rebuild 2>&1 | tail -10`

- [ ] **Step 3: Implement partition + builder** (append to `justify.rs`)

```rust
/// Split `onto` into (`fixed`, `candidates`): `fixed` = non-logical axioms
/// (declarations / annotations) kept in every tested ontology; `candidates`
/// = logical axioms, the only possible justification members.
#[must_use]
pub fn logical_axioms<A: ForIRI>(onto: &SetOntology<A>) -> (Vec<Component<A>>, Vec<Component<A>>) {
    let mut fixed = Vec::new();
    let mut candidates = Vec::new();
    for ac in onto {
        let c = ac.component.clone();
        if is_logical(&c) {
            candidates.push(c);
        } else {
            fixed.push(c);
        }
    }
    (fixed, candidates)
}

/// A logical axiom can affect entailment and may appear in a justification.
/// Declarations, annotation assertions, and ontology metadata cannot.
fn is_logical<A: ForIRI>(c: &Component<A>) -> bool {
    !matches!(
        c,
        Component::OntologyID(_)
            | Component::DocIRI(_)
            | Component::Import(_)
            | Component::OntologyAnnotation(_)
            | Component::Declaration(_)
            | Component::AnnotationAssertion(_)
            | Component::SubAnnotationPropertyOf(_)
            | Component::AnnotationPropertyDomain(_)
            | Component::AnnotationPropertyRange(_)
    )
}

/// Build a `SetOntology` from `fixed` + the given candidate `subset`.
#[must_use]
pub fn ontology_from<A: ForIRI>(fixed: &[Component<A>], subset: &[Component<A>]) -> SetOntology<A> {
    let mut o = SetOntology::new();
    for c in fixed.iter().chain(subset.iter()) {
        o.insert(c.clone());
    }
    o
}
```

(If a `Component` variant name in `is_logical` doesn't exist in this horned-owl version, adjust to the actual non-logical variants — the principle is: declarations + all annotation/metadata variants are `fixed`, everything else is a candidate. Verify against `horned_owl::model::Component` and `convert.rs`'s match arms, which enumerate the logical ones rustdl consumes.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p owl-dl-reasoner --test justification partition_and_rebuild 2>&1 | tail -6`

- [ ] **Step 5: clippy + fmt** (as Task 1 Step 5).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/justify.rs crates/owl-dl-reasoner/tests/justification.rs
git commit -m "feat(justify): logical-axiom partition + subset ontology builder

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `find_one_justification` (QuickXplain) + result + fragment flag

**Files:**
- Modify: `crates/owl-dl-reasoner/src/justify.rs`, `crates/owl-dl-reasoner/tests/justification.rs`

- [ ] **Step 1: Write the failing tests** (append to `tests/justification.rs`)

```rust
use owl_dl_reasoner::justify::{find_one_justification, Justification};

fn iris_in(j: &Justification<RcStr>) -> std::collections::BTreeSet<String> {
    // Render each axiom's debug form for set comparison in tests.
    j.axioms.iter().map(|c| format!("{c:?}")).collect()
}

#[test]
fn find_one_subclassof_exact() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:Z))\n\
         SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:Z :C)",
    );
    let q = Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/C".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("entailed");
    // EL fragment ⇒ exact minimal justification = {A⊑B, B⊑C}; Z⊑C excluded.
    assert_eq!(j.axioms.len(), 2, "minimal justification has 2 axioms; got {:?}", iris_in(&j));
    assert!(j.minimal_guaranteed, "EL fragment ⇒ minimality guaranteed");
    // Re-check: the justification really entails the query.
    let (fixed, _) = logical_axioms(&o);
    assert!(entails(&ontology_from(&fixed, &j.axioms), &q).unwrap());
    // Minimal: removing either axiom breaks entailment.
    assert!(!entails(&ontology_from(&fixed, &j.axioms[..1]), &q).unwrap());
}

#[test]
fn find_one_not_entailed_is_none() {
    let o = onto("Declaration(Class(:A)) Declaration(Class(:B))");
    let q = Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/B".into() };
    assert!(find_one_justification(&o, &q).unwrap().is_none());
}

#[test]
fn find_one_unsat() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B))\n\
         DisjointClasses(:A :B) SubClassOf(:A :B)",
    );
    let q = Entailment::Unsatisfiable { class: "http://t/A".into() };
    let j = find_one_justification(&o, &q).unwrap().expect("A is unsat");
    assert_eq!(j.axioms.len(), 2, "{:?}", iris_in(&j));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-reasoner --test justification find_one_ 2>&1 | tail -10`

- [ ] **Step 3: Implement QuickXplain + `Justification` + fragment flag** (append to `justify.rs`)

```rust
use crate::classify::{analyze_fragment, FragmentClassification};

/// A minimal (on EL/Horn) responsible-axiom set for an entailment.
#[derive(Debug, Clone)]
pub struct Justification<A: ForIRI> {
    pub axioms: Vec<Component<A>>,
    pub fragment: FragmentClassification,
    pub minimal_guaranteed: bool,
}

/// Find ONE justification for `q` in `onto`, or `Ok(None)` if `onto` does not
/// entail `q`. QuickXplain over the logical axioms; minimal on EL/Horn,
/// guaranteed-entailing on SROIQ.
///
/// # Errors
/// Propagates [`ReasonError`] from the underlying reasoner.
pub fn find_one_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let (fixed, candidates) = logical_axioms(onto);
    // Precondition: the whole ontology must entail q.
    if !entails(&ontology_from(&fixed, &candidates), q)? {
        return Ok(None);
    }
    let core = quickxplain(&fixed, &candidates, q)?;
    let fragment = fragment_of(onto);
    let minimal_guaranteed =
        matches!(fragment, FragmentClassification::PureEl | FragmentClassification::Horn);
    Ok(Some(Justification { axioms: core, fragment, minimal_guaranteed }))
}

fn fragment_of<A: ForIRI>(onto: &SetOntology<A>) -> FragmentClassification {
    owl_dl_core::convert::convert_ontology(onto)
        .map_or(FragmentClassification::OutOfFragment, |internal| analyze_fragment(&internal))
}

/// QuickXplain (Junker 2004): minimal `C' ⊆ candidates` with
/// `fixed ∪ C' ⊨ q`. Precondition: `fixed ∪ candidates ⊨ q`.
fn quickxplain<A: ForIRI>(
    fixed: &[Component<A>],
    candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Vec<Component<A>>, ReasonError> {
    // Background (fixed) alone entails ⇒ no candidate needed.
    if entails(&ontology_from(fixed, &[]), q)? {
        return Ok(Vec::new());
    }
    if candidates.len() <= 1 {
        return Ok(candidates.to_vec());
    }
    qx(fixed, true, candidates, q)
}

/// `delta_nonempty`: whether the last addition to `fixed` was non-empty (skip
/// the entailment check at the root call where it is meaningless).
fn qx<A: ForIRI>(
    fixed: &[Component<A>],
    delta_nonempty: bool,
    candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Vec<Component<A>>, ReasonError> {
    if delta_nonempty && entails(&ontology_from(fixed, &[]), q)? {
        return Ok(Vec::new());
    }
    if candidates.len() == 1 {
        return Ok(candidates.to_vec());
    }
    let mid = candidates.len() / 2;
    let (c1, c2) = candidates.split_at(mid);
    // D2 ⊆ C2 needed given fixed ∪ C1.
    let fixed_c1: Vec<Component<A>> = fixed.iter().chain(c1.iter()).cloned().collect();
    let d2 = qx(&fixed_c1, !c1.is_empty(), c2, q)?;
    // D1 ⊆ C1 needed given fixed ∪ D2.
    let fixed_d2: Vec<Component<A>> = fixed.iter().chain(d2.iter()).cloned().collect();
    let d1 = qx(&fixed_d2, !d2.is_empty(), c1, q)?;
    let mut out = d1;
    out.extend(d2);
    Ok(out)
}
```

Notes for the implementer:
- `analyze_fragment` and `FragmentClassification` are in `crate::classify` — confirm they're `pub`/`pub(crate)` reachable; if `analyze_fragment` is `pub` at crate root use that path. `convert_ontology` is `owl_dl_core::convert::convert_ontology`.
- `ontology_from(fixed, &[])` rebuilds the fixed-only ontology; reused a lot — fine for correctness (perf optimization deferred).
- The `core` returned is a minimal justification by QuickXplain's contract (monotone property = entailment).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p owl-dl-reasoner --test justification find_one_ 2>&1 | tail -10`
Expected: all three pass. If `find_one_subclassof_exact` returns 3 axioms (not 2), QuickXplain isn't minimizing — debug the `qx` split/recursion before proceeding.

- [ ] **Step 5: clippy + fmt.**

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/justify.rs crates/owl-dl-reasoner/tests/justification.rs
git commit -m "feat(justify): find_one_justification via QuickXplain + fragment minimality flag

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: per-query canaries + correctness invariants

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/justification.rs`

- [ ] **Step 1: Add canaries for the remaining query types + the SROIQ flag**

```rust
#[test]
fn find_one_equivalent_and_disjoint() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B))\n\
         SubClassOf(:A :B) SubClassOf(:B :A)",
    );
    let j = find_one_justification(&o, &Entailment::EquivalentClasses {
        a: "http://t/A".into(), b: "http://t/B".into() }).unwrap().expect("equiv");
    assert_eq!(j.axioms.len(), 2);

    let d = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         DisjointClasses(:A :B) SubClassOf(:C :A) SubClassOf(:C :B)",
    );
    // C is unsat (it's both A and B which are disjoint); but DisjointClasses(A,B)
    // entailment justification = {DisjointClasses(A,B)} alone.
    let jd = find_one_justification(&d, &Entailment::DisjointClasses {
        a: "http://t/A".into(), b: "http://t/B".into() }).unwrap().expect("disjoint");
    assert_eq!(jd.axioms.len(), 1, "{:?}", iris_in(&jd));
}

#[test]
fn find_one_instance_of() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))\n\
         SubClassOf(:A :B) ClassAssertion(:A :x)",
    );
    let j = find_one_justification(&o, &Entailment::InstanceOf {
        individual: "http://t/x".into(), class: "http://t/B".into() }).unwrap().expect("x:B");
    assert_eq!(j.axioms.len(), 2, "{:?}", iris_in(&j)); // ClassAssertion(A,x) + A⊑B
}

#[test]
fn find_one_inconsistent() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(NamedIndividual(:x))\n\
         SubClassOf(:A owl:Nothing) ClassAssertion(:A :x)",
    );
    let j = find_one_justification(&o, &Entailment::Inconsistent).unwrap().expect("inconsistent");
    assert_eq!(j.axioms.len(), 2, "{:?}", iris_in(&j));
}

#[test]
fn sroiq_flags_minimality_not_guaranteed() {
    // An ontology with disjunction (out of EL/Horn) ⇒ minimal_guaranteed = false.
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         SubClassOf(:A ObjectUnionOf(:B :C)) SubClassOf(:B :C) SubClassOf(:C :B)",
    );
    // Pick any entailment that holds; assert the flag, not the exact set.
    let q = Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/B".into() };
    if let Some(j) = find_one_justification(&o, &q).unwrap() {
        assert!(!j.minimal_guaranteed, "disjunction ⇒ SROIQ/out-of-fragment ⇒ not guaranteed");
    }
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p owl-dl-reasoner --test justification 2>&1 | tail -15`
Expected: all pass. If an axiom-count assertion is off by the framework counting the disjointness probe or a declaration, inspect `iris_in` output and adjust the EXPECTED count to the true minimal set (do not change the engine — verify the justification is genuinely minimal by the re-entail/remove checks). If `sroiq_flags_minimality_not_guaranteed`'s `find_one` returns `None` (not entailed), pick an entailment that does hold for that ontology.

- [ ] **Step 3: clippy + fmt + commit**

```bash
git add crates/owl-dl-reasoner/tests/justification.rs
git commit -m "test(justify): per-query canaries (equiv/disjoint/instance/inconsistent) + SROIQ flag

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `find_all_justifications` (Hitting Set Tree)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/justify.rs`, `crates/owl-dl-reasoner/tests/justification.rs`

- [ ] **Step 1: Write the failing test**

```rust
use owl_dl_reasoner::justify::find_all_justifications;

#[test]
fn find_all_two_independent_derivations() {
    // A⊑C via A⊑B,B⊑C  AND independently via A⊑D,D⊑C. Two minimal justifications.
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
         SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:A :D) SubClassOf(:D :C)",
    );
    let q = Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/C".into() };
    let all = find_all_justifications(&o, &q, 10).unwrap();
    assert_eq!(all.len(), 2, "two independent minimal justifications");
    for j in &all {
        assert_eq!(j.axioms.len(), 2);
        let (fixed, _) = logical_axioms(&o);
        assert!(entails(&ontology_from(&fixed, &j.axioms), &q).unwrap());
    }
}

#[test]
fn find_all_respects_cap() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
         SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:A :D) SubClassOf(:D :C)",
    );
    let q = Entailment::SubClassOf { sub: "http://t/A".into(), sup: "http://t/C".into() };
    assert_eq!(find_all_justifications(&o, &q, 1).unwrap().len(), 1, "cap=1");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-reasoner --test justification find_all_ 2>&1 | tail -10`

- [ ] **Step 3: Implement HST** (append to `justify.rs`)

```rust
use std::collections::BTreeSet;

/// Find up to `max` minimal justifications for `q` via a Reiter Hitting-Set
/// Tree over `find_one`. Returns `[]` if `q` is not entailed.
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_all_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let (fixed, candidates) = logical_axioms(onto);
    let mut found: Vec<Vec<Component<A>>> = Vec::new();
    // Worklist of "removed-axiom index sets" to explore (HST nodes).
    let mut worklist: Vec<BTreeSet<usize>> = vec![BTreeSet::new()];
    let mut explored: BTreeSet<BTreeSet<usize>> = BTreeSet::new();
    while let Some(removed) = worklist.pop() {
        if found.len() >= max {
            break;
        }
        if !explored.insert(removed.clone()) {
            continue;
        }
        // Candidates with `removed` indices excluded.
        let subset: Vec<Component<A>> = candidates
            .iter()
            .enumerate()
            .filter(|(i, _)| !removed.contains(i))
            .map(|(_, c)| c.clone())
            .collect();
        if !entails(&ontology_from(&fixed, &subset), q)? {
            continue; // this branch cannot yield a justification
        }
        let j = quickxplain(&fixed, &subset, q)?;
        // Record if new.
        let key: BTreeSet<String> = j.iter().map(|c| format!("{c:?}")).collect();
        let is_new = !found.iter().any(|f| {
            f.iter().map(|c| format!("{c:?}")).collect::<BTreeSet<String>>() == key
        });
        if is_new {
            found.push(j.clone());
        }
        // Branch: remove each axiom of this justification (by its index in
        // `candidates`) and explore.
        for c in &j {
            if let Some(idx) = candidates.iter().position(|x| format!("{x:?}") == format!("{c:?}")) {
                let mut next = removed.clone();
                next.insert(idx);
                worklist.push(next);
            }
        }
    }
    let fragment = fragment_of(onto);
    let minimal_guaranteed =
        matches!(fragment, FragmentClassification::PureEl | FragmentClassification::Horn);
    Ok(found
        .into_iter()
        .map(|axioms| Justification { axioms, fragment, minimal_guaranteed })
        .collect())
}
```

(The `format!("{c:?}")` axiom-identity key is a pragmatic equality for `Component<A>` — horned-owl `Component` is `Eq`, so prefer `candidates.iter().position(|x| x == c)` and dedup via `Vec<Component<A>>` set membership with `==` if `Component<A>: Eq + Ord` allows a `BTreeSet`; if it's `Eq` but not `Ord`, keep the debug-string key. Confirm `Component<A>: Eq` and simplify to `==` where possible.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p owl-dl-reasoner --test justification find_all_ 2>&1 | tail -8`

- [ ] **Step 5: clippy + fmt + commit**

```bash
git add crates/owl-dl-reasoner/src/justify.rs crates/owl-dl-reasoner/tests/justification.rs
git commit -m "feat(justify): find_all_justifications via Reiter Hitting-Set Tree (capped)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: CLI `justify` subcommand + corpus smoke + docs

**Files:**
- Modify: `crates/owl-dl-cli/src/main.rs`
- Modify: `crates/owl-dl-reasoner/tests/justification.rs`
- Modify: `docs/superpowers/specs/2026-06-12-justification-foundation-design.md` (status)

- [ ] **Step 1: Add the `Justify` subcommand**

In `crates/owl-dl-cli/src/main.rs`, add a `Justify` variant to the command enum (model it on the existing `Explain { file, sub, sup }` variant and its handler at the `Command::Explain` match arm). Use a query sub-argument. Minimal shape:

```rust
    /// Explain WHY an entailment holds: print a minimal responsible-axiom set.
    Justify {
        file: std::path::PathBuf,
        /// Query: subclass <S> <T> | unsat <C> | instance <I> <C> |
        /// equivalent <A> <B> | disjoint <A> <B> | inconsistent
        #[arg(num_args = 1..)]
        query: Vec<String>,
        /// Print all minimal justifications (capped by --max).
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 10)]
        max: usize,
    },
```

Handler (add a `Command::Justify { .. } =>` arm near `Command::Explain`):

```rust
        Command::Justify { file, query, all, max } => {
            let onto = parse_ofn(&file)?;
            let q = parse_query(&query)?; // helper below
            let render = |j: &owl_dl_reasoner::justify::Justification<_>| {
                let note = if j.minimal_guaranteed {
                    format!("minimal ({})", j.fragment)
                } else {
                    format!("entailing; minimality NOT guaranteed ({})", j.fragment)
                };
                println!("# justification ({} axioms) — {note}", j.axioms.len());
                for ax in &j.axioms {
                    println!("  {ax:?}");
                }
            };
            if all {
                let js = owl_dl_reasoner::justify::find_all_justifications(&onto, &q, max)?;
                if js.is_empty() {
                    println!("not entailed (no justification)");
                } else {
                    println!("# {} justification(s)", js.len());
                    for j in &js { render(j); }
                }
            } else {
                match owl_dl_reasoner::justify::find_one_justification(&onto, &q)? {
                    Some(j) => render(&j),
                    None => println!("not entailed (no justification)"),
                }
            }
        }
```

Add a `parse_query` helper near the other CLI helpers:

```rust
fn parse_query(parts: &[String]) -> anyhow::Result<owl_dl_reasoner::justify::Entailment> {
    use owl_dl_reasoner::justify::Entailment;
    let kind = parts.first().map(String::as_str).unwrap_or("");
    Ok(match (kind, parts.len()) {
        ("subclass", 3) => Entailment::SubClassOf { sub: parts[1].clone(), sup: parts[2].clone() },
        ("equivalent", 3) => Entailment::EquivalentClasses { a: parts[1].clone(), b: parts[2].clone() },
        ("disjoint", 3) => Entailment::DisjointClasses { a: parts[1].clone(), b: parts[2].clone() },
        ("unsat", 2) => Entailment::Unsatisfiable { class: parts[1].clone() },
        ("instance", 3) => Entailment::InstanceOf { individual: parts[1].clone(), class: parts[2].clone() },
        ("inconsistent", 1) => Entailment::Inconsistent,
        _ => anyhow::bail!(
            "usage: justify <file> (subclass S T | equivalent A B | disjoint A B | unsat C | instance I C | inconsistent)"
        ),
    })
}
```

(Match the CLI's actual derive/clap version and `parse_ofn` helper — both already exist for `Explain`. The `{ax:?}` debug render is acceptable for v1; a Manchester/OFN renderer is a later polish.)

- [ ] **Step 2: Build the CLI + manual smoke**

Run:
```bash
cargo build -p owl-dl-cli --release 2>&1 | tail -2
cat > /tmp/just.ofn <<'EOF'
Prefix(:=<http://t/>)
Ontology(<http://t/o>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:Z))
  SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:Z :C))
EOF
./target/release/rustdl justify /tmp/just.ofn subclass http://t/A http://t/C
```
Expected: prints "# justification (2 axioms) — minimal (...)" with the two SubClassOf axioms, excluding `Z⊑C`.

- [ ] **Step 3: Corpus smoke test** (append to `tests/justification.rs`)

```rust
/// On a real fixture, a known entailment's justification must be a subset of
/// the ontology that re-entails the query (no oracle for "the" justification).
#[test]
#[ignore = "needs the fetched corpus"]
fn corpus_justification_invariants() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ontologies/real/sio.ofn");
    if !path.exists() { eprintln!("SKIP: sio.ofn absent"); return; }
    let file = std::fs::File::open(&path).unwrap();
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut std::io::BufReader::new(file), ParserConfiguration::default()).unwrap();
    // Use classify to find a real entailed pair, then justify it.
    let c = owl_dl_reasoner::classify(&o).unwrap();
    // Pick the first non-reflexive entailed pair from the closure.
    // (Implementer: iterate c's hierarchy; for the first (sub,sup), assert the
    // justification is a subset that re-entails.)
    let _ = c; // see note
}
```

Note for implementer: flesh out `corpus_justification_invariants` to pull one real `(sub,sup)` from `classify`'s result (use the `Classification` API to get an entailed pair), call `find_one_justification`, and assert (a) every axiom is in the ontology and (b) `entails(ontology_from(fixed, j.axioms), q)`. Keep it `#[ignore]`d (corpus-dependent). If extracting a pair from `Classification` is awkward, use a hardcoded known SIO subsumption (e.g. a documented `SIO_*` pair) instead.

- [ ] **Step 4: Full test run + fmt + clippy**

Run:
```bash
cargo test -p owl-dl-reasoner --test justification 2>&1 | tail -5
cargo fmt --all -- --check; echo "fmt=$?"
cargo clippy -p owl-dl-reasoner -p owl-dl-cli --all-targets --all-features -- -D warnings 2>&1 | tail -4
```
Expected: all non-ignored justification tests pass; fmt rc 0; clippy clean.

- [ ] **Step 5: Mark the spec implemented**

In `docs/superpowers/specs/2026-06-12-justification-foundation-design.md`, add a `## Status (implemented 2026-06-12)` section near the top: commits, the query types working, find-one + find-all done, CLI `justify` shipped, test summary, and a note that the corpus smoke is `#[ignore]`d.

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "feat(cli): rustdl justify subcommand + corpus smoke; mark foundation implemented

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Notes for the implementer

- **No engine internals.** Everything goes through the public `is_*`/`classify` API. If you find yourself editing `hyper.rs`/`rules.rs`/`saturate`, stop — the design is black-box.
- **Soundness framing is the point:** never present a SROIQ justification as provably minimal. The `minimal_guaranteed`/`fragment` fields + the CLI note carry this; don't drop them.
- **Run `cargo fmt --all -- --check`** before each commit (CI fmt is live on push).
- **horned-owl API drift:** the exact `Component` variant names, `Build` constructor (`Build::new()` vs `Build::new_rc()`), and `MutableOntology::insert` signature may differ slightly from the snippets — `convert_back.rs` (IR→horned-owl) and `convert.rs` (horned-owl→IR) are the authoritative in-repo exemplars; match them.
- **Do NOT push** (CI is live; the push decision is the user's).
- This is **Spec 1 of 3**; do not implement property-query reductions, laconic, or root/derived here — they are separate specs.
