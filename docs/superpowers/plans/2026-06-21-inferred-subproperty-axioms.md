# `materialize_inferred_subproperty_axioms` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two reasoner functions + Python bindings returning the entailed named property-subsumption pairs `(sub, sup)` — object properties (with inverse propagation) and data properties — the RBox analog of `materialize_inferred_subclass_axioms`.

**Architecture:** Pure structural closure over the horned-owl axioms (no engine query, no per-pair tableau). Object properties use a signed-role `(name, inverse_flag)` model to handle `InverseObjectProperties`; data properties are a simple closure. A consistency pre-check (ABox saturator) gates `Inconsistent`.

**Tech Stack:** Rust (edition 2024), horned-owl model, `owl-dl-reasoner`, `owl-dl-py`.

**Spec:** `docs/superpowers/specs/2026-06-21-inferred-subproperty-axioms-design.md`
**Branch:** `feat/inferred-subproperty-axioms`

---

## Key facts (verified)

- `SubObjectPropertyOf { sub: SubObjectPropertyExpression<A>, sup: ObjectPropertyExpression<A> }`.
- `SubObjectPropertyExpression::{ ObjectPropertyExpression(OPE), ObjectPropertyChain(Vec<OPE>) }`.
- `ObjectPropertyExpression::{ ObjectProperty(ObjectProperty<A>), InverseObjectProperty(ObjectProperty<A>) }`; the IRI is `op.0.as_ref()`.
- `Component::InverseObjectProperties(ax)` — tuple struct, `ax.0` / `ax.1` are `ObjectProperty<A>` (IRI `ax.0.0.as_ref()`).
- `Component::EquivalentObjectProperties(ax)` — `ax.0: Vec<ObjectPropertyExpression<A>>`.
- `SubDataPropertyOf { sub: DataProperty<A>, sup: DataProperty<A> }` (IRI `ax.sub.0.as_ref()`); `EquivalentDataProperties(ax)` — `ax.0: Vec<DataProperty<A>>`.
- Reasoner has `materialize_data_property_assertions` (sibling, same file/style), `ReasonError::Inconsistent`, `owl_dl_core::convert::convert_ontology`, `abox_saturation::saturate_abox_consistency`, `justify::{Entailment, entails}` (`Entailment::SubObjectProperty{sub,sup}`, `Entailment::SubDataProperty{sub,sup}`). Functions go in `crates/owl-dl-reasoner/src/lib.rs`.
- Python family: `crates/owl-dl-py/src/materialize.rs` (`#[pyfunction]`, `load::load_path`, `reason_error_to_py` — already handles `Inconsistent`).
- No CLI (matches Python-only `materialize_inferred_subclass_axioms`).

## File structure

- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — two functions.
- **Create** `crates/owl-dl-reasoner/tests/subproperty_axioms.rs` — tests.
- **Modify** `crates/owl-dl-py/src/materialize.rs` — two Python bindings.
- **Modify** `README.md`, `CLAUDE.md` — docs.

---

### Task 1: Reasoner functions + tests

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; Create `crates/owl-dl-reasoner/tests/subproperty_axioms.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/inferred-subproperty-axioms
```

- [ ] **Step 2: Write the failing tests** — create `crates/owl-dl-reasoner/tests/subproperty_axioms.rs`:

```rust
//! Integration tests for materialize_subobjectproperty_axioms / materialize_subdataproperty_axioms.

use horned_owl::model::{
    Build, DeclareDataProperty, DeclareObjectProperty, EquivalentObjectProperties,
    InverseObjectProperties, MutableOntology, ObjectPropertyExpression as OPE,
    SubDataPropertyOf, SubObjectPropertyExpression as SOPE, SubObjectPropertyOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{materialize_subdataproperty_axioms, materialize_subobjectproperty_axioms};

type Rc = std::rc::Rc<str>;

fn op(b: &Build<Rc>, iri: &str) -> OPE<Rc> {
    OPE::ObjectProperty(b.object_property(iri))
}
fn subop(b: &Build<Rc>, sub: &str, sup: &str) -> SubObjectPropertyOf<Rc> {
    SubObjectPropertyOf {
        sub: SOPE::ObjectPropertyExpression(op(b, sub)),
        sup: op(b, sup),
    }
}

// p ⊑ q ⊑ r ⇒ (p,r) present; (p,p) absent.
#[test]
fn object_transitivity() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:p", "urn:q", "urn:r"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(subop(&b, "urn:p", "urn:q"));
    o.insert(subop(&b, "urn:q", "urn:r"));

    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    let t = |a: &str, c: &str| (a.to_string(), c.to_string());
    assert!(got.contains(&t("urn:p", "urn:r")), "got: {got:?}");
    assert!(got.contains(&t("urn:p", "urn:q")) && got.contains(&t("urn:q", "urn:r")));
    assert!(!got.iter().any(|(a, c)| a == c), "no reflexive pairs");
}

// hasParent ⊑ hasAncestor + inverses ⇒ hasChild ⊑ hasDescendant.
#[test]
fn object_inverse_propagation() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:hasParent", "urn:hasAncestor", "urn:hasChild", "urn:hasDescendant"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(subop(&b, "urn:hasParent", "urn:hasAncestor"));
    o.insert(InverseObjectProperties(b.object_property("urn:hasParent"), b.object_property("urn:hasChild")));
    o.insert(InverseObjectProperties(b.object_property("urn:hasAncestor"), b.object_property("urn:hasDescendant")));

    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    assert!(
        got.contains(&("urn:hasChild".to_string(), "urn:hasDescendant".to_string())),
        "inverse propagation: got: {got:?}"
    );
}

// subDP ⊑ midDP ⊑ supDP ⇒ (subDP, supDP).
#[test]
fn data_transitivity() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for d in ["urn:a", "urn:m", "urn:z"] {
        o.insert(DeclareDataProperty(b.data_property(d)));
    }
    o.insert(SubDataPropertyOf { sub: b.data_property("urn:a"), sup: b.data_property("urn:m") });
    o.insert(SubDataPropertyOf { sub: b.data_property("urn:m"), sup: b.data_property("urn:z") });

    let got = materialize_subdataproperty_axioms(&o).expect("materialize");
    assert!(got.contains(&("urn:a".to_string(), "urn:z".to_string())), "got: {got:?}");
    assert!(!got.iter().any(|(a, c)| a == c));
}
```

NOTE: shapes verified against `convert.rs` (`ObjectPropertyExpression::{ObjectProperty, InverseObjectProperty}`, `SubObjectPropertyExpression::{ObjectPropertyExpression, ObjectPropertyChain}`, `InverseObjectProperties(a,b)`, `EquivalentObjectProperties(Vec)`, `SubDataPropertyOf{sub,sup}`). If any differs, match `convert.rs` and report.

- [ ] **Step 3: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --test subproperty_axioms` → FAIL (functions undefined).

- [ ] **Step 4: Implement** — add BOTH functions to `crates/owl-dl-reasoner/src/lib.rs` (next to `materialize_data_property_assertions`; match its `owl_dl_core::convert::convert_ontology` / `abox_saturation` path style):

```rust
/// Materialize the inferred OBJECT property-subsumption axioms `(sub, sup)` over
/// named object properties (told + equivalent + inverse closure, transitively
/// closed). Sound; complete for the named simple-subsumption fragment (role chains,
/// which give complex subsumption, are excluded). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent; [`ReasonError::Conversion`].
#[allow(clippy::type_complexity)]
pub fn materialize_subobjectproperty_axioms<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    use horned_owl::model::{
        Component as C, ObjectPropertyExpression as OPE, SubObjectPropertyExpression as SOPE,
    };
    use std::collections::BTreeSet;

    let internal = convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    type Signed = (String, bool); // (property IRI, is_inverse)
    fn signed<A: ForIRI>(ope: &OPE<A>) -> Signed {
        match ope {
            OPE::ObjectProperty(op) => (op.0.as_ref().to_string(), false),
            OPE::InverseObjectProperty(op) => (op.0.as_ref().to_string(), true),
        }
    }

    let mut edges: BTreeSet<(Signed, Signed)> = BTreeSet::new();
    for ac in onto {
        match &ac.component {
            C::SubObjectPropertyOf(ax) => {
                if let SOPE::ObjectPropertyExpression(sub_ope) = &ax.sub {
                    edges.insert((signed(sub_ope), signed(&ax.sup)));
                }
                // ObjectPropertyChain sub → complex subsumption, skipped.
            }
            C::EquivalentObjectProperties(ax) => {
                let ss: Vec<Signed> = ax.0.iter().map(signed).collect();
                for (i, si) in ss.iter().enumerate() {
                    for (j, sj) in ss.iter().enumerate() {
                        if i != j {
                            edges.insert((si.clone(), sj.clone()));
                        }
                    }
                }
            }
            C::InverseObjectProperties(ax) => {
                let p = ax.0.0.as_ref().to_string();
                let q = ax.1.0.as_ref().to_string();
                // (p,false) ≡ (q,true) ; (q,false) ≡ (p,true)
                edges.insert(((p.clone(), false), (q.clone(), true)));
                edges.insert(((q.clone(), true), (p.clone(), false)));
                edges.insert(((q.clone(), false), (p.clone(), true)));
                edges.insert(((p.clone(), true), (q.clone(), false)));
            }
            _ => {}
        }
    }

    // Inverse propagation + transitive closure to fixpoint.
    loop {
        let mut new: Vec<(Signed, Signed)> = Vec::new();
        for ((an, af), (bn, bf)) in &edges {
            let cand = ((an.clone(), !*af), (bn.clone(), !*bf));
            if !edges.contains(&cand) {
                new.push(cand);
            }
        }
        for (a, b) in &edges {
            for (b2, c) in &edges {
                if b == b2 {
                    let cand = (a.clone(), c.clone());
                    if !edges.contains(&cand) {
                        new.push(cand);
                    }
                }
            }
        }
        if new.is_empty() {
            break;
        }
        for e in new {
            edges.insert(e);
        }
    }

    const TOP: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
    const BOT: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";
    let mut out: Vec<(String, String)> = edges
        .iter()
        .filter(|((_, af), (_, bf))| !af && !bf)
        .map(|((a, _), (b, _))| (a.clone(), b.clone()))
        .filter(|(a, b)| a != b && a != TOP && a != BOT && b != TOP && b != BOT)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Materialize the inferred DATA property-subsumption axioms `(sub, sup)` over named
/// data properties (told + equivalent closure, transitively closed). Sound; complete
/// for that fragment (data properties have no inverses/chains). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent; [`ReasonError::Conversion`].
pub fn materialize_subdataproperty_axioms<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    use horned_owl::model::Component as C;
    use std::collections::BTreeSet;

    let internal = convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for ac in onto {
        match &ac.component {
            C::SubDataPropertyOf(ax) => {
                edges.insert((ax.sub.0.as_ref().to_string(), ax.sup.0.as_ref().to_string()));
            }
            C::EquivalentDataProperties(ax) => {
                let ds: Vec<String> = ax.0.iter().map(|d| d.0.as_ref().to_string()).collect();
                for (i, di) in ds.iter().enumerate() {
                    for (j, dj) in ds.iter().enumerate() {
                        if i != j {
                            edges.insert((di.clone(), dj.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    loop {
        let mut new: Vec<(String, String)> = Vec::new();
        for (a, b) in &edges {
            for (b2, c) in &edges {
                if b == b2 {
                    let cand = (a.clone(), c.clone());
                    if !edges.contains(&cand) {
                        new.push(cand);
                    }
                }
            }
        }
        if new.is_empty() {
            break;
        }
        for e in new {
            edges.insert(e);
        }
    }

    const TOP: &str = "http://www.w3.org/2002/07/owl#topDataProperty";
    const BOT: &str = "http://www.w3.org/2002/07/owl#bottomDataProperty";
    let mut out: Vec<(String, String)> = edges
        .into_iter()
        .filter(|(a, b)| a != b && a != TOP && a != BOT && b != TOP && b != BOT)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}
```
If `ax.0.0.as_ref()` (nested tuple field) or `op.0.as_ref()` doesn't compile, check how `convert.rs` accesses the IRI of an `ObjectProperty` / `InverseObjectProperties` and match. If clippy flags the nested `signed` fn's generic `<A>` shadowing, rename or inline. Report adjustments.

- [ ] **Step 5: Run** — `cargo test -p owl-dl-reasoner --test subproperty_axioms` → 3 passed. If `object_inverse_propagation` fails, the signed-role/inverse logic has a bug — investigate/report. Paste the `test result:` line.

- [ ] **Step 6: Add equivalent + soundness + inconsistency tests** — append:

```rust
use owl_dl_reasoner::justify::{Entailment, entails};

// EquivalentObjectProperties(p, q) ⇒ both (p,q) and (q,p).
#[test]
fn object_equivalent_both_directions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:p", "urn:q"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(EquivalentObjectProperties(vec![op(&b, "urn:p"), op(&b, "urn:q")]));
    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    assert!(got.contains(&("urn:p".to_string(), "urn:q".to_string())));
    assert!(got.contains(&("urn:q".to_string(), "urn:p".to_string())));
}

// Every emitted pair is genuinely entailed.
#[test]
fn object_pairs_entailed() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:p", "urn:q", "urn:r"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(subop(&b, "urn:p", "urn:q"));
    o.insert(subop(&b, "urn:q", "urn:r"));
    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    assert!(!got.is_empty());
    for (s, t) in &got {
        let q = Entailment::SubObjectProperty { sub: s.clone(), sup: t.clone() };
        assert!(entails(&o, &q).expect("entails"), "{s} ⊑ {t} must be entailed");
    }
}

// Inconsistent ontology → Err (both functions).
#[test]
fn inconsistent_is_error() {
    use horned_owl::model::{
        ClassAssertion, ClassExpression as CE, DeclareClass, DeclareNamedIndividual, DisjointClasses,
        Individual,
    };
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareNamedIndividual(b.named_individual("urn:i")));
    o.insert(DisjointClasses(vec![CE::Class(b.class("urn:A")), CE::Class(b.class("urn:B"))]));
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:A")), i: Individual::Named(b.named_individual("urn:i")) });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:B")), i: Individual::Named(b.named_individual("urn:i")) });

    assert!(materialize_subobjectproperty_axioms(&o).is_err());
    assert!(materialize_subdataproperty_axioms(&o).is_err());
}
```

- [ ] **Step 7: Run all + clippy + fmt**
```bash
cargo test -p owl-dl-reasoner --test subproperty_axioms
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
6 passed; clippy/fmt green; re-run after fmt.

- [ ] **Step 8: Commit**
```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/subproperty_axioms.rs
git commit -m "feat(reasoner): materialize_sub{object,data}property_axioms (structural closure)"
```

---

### Task 2: Python bindings

**Files:** Modify `crates/owl-dl-py/src/materialize.rs`

- [ ] **Step 1: Add bindings** — after `materialize_inferred_data_property_assertions`:

```rust
/// Returns every inferred object property subsumption `(sub, sup)` over named object
/// properties (told + equivalent + inverse closure). Raises if inconsistent.
#[pyfunction]
pub(crate) fn materialize_inferred_subobjectproperty_axioms(
    path: &str,
) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::materialize_subobjectproperty_axioms(&ontology).map_err(reason_error_to_py)
}

/// Returns every inferred data property subsumption `(sub, sup)` over named data
/// properties (told + equivalent closure). Raises if inconsistent.
#[pyfunction]
pub(crate) fn materialize_inferred_subdataproperty_axioms(
    path: &str,
) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::materialize_subdataproperty_axioms(&ontology).map_err(reason_error_to_py)
}
```
Register both in `register`:
```rust
    m.add_function(wrap_pyfunction!(materialize_inferred_subobjectproperty_axioms, m)?)?;
    m.add_function(wrap_pyfunction!(materialize_inferred_subdataproperty_axioms, m)?)?;
```

- [ ] **Step 2: Build + clippy + fmt**
```bash
cargo build -p owl-dl-py
cargo clippy -p owl-dl-py --all-targets -- -D warnings
cargo fmt -p owl-dl-py
```
Green.

- [ ] **Step 3: Commit**
```bash
git add crates/owl-dl-py/src/materialize.rs
git commit -m "feat(py): materialize_inferred_sub{object,data}property_axioms bindings"
```

---

### Task 3: Docs + final gate

**Files:** Modify `README.md`, `CLAUDE.md`

- [ ] **Step 1: README** — if the Python example block lists the `materialize_*` functions, add the two new ones; otherwise add a brief line under the Python section. (No CLI change.)

- [ ] **Step 2: CLAUDE.md** — append to the materialize feature documentation (where the property-assertion materializers are noted):
```
`materialize_sub{object,data}property_axioms` (reasoner) /
`materialize_inferred_sub{object,data}property_axioms` (Python) return the inferred
named property-subsumption closure (object: told + equivalent + inverse; data: told +
equivalent), structural + sound. See
`docs/superpowers/specs/2026-06-21-inferred-subproperty-axioms-design.md`.
```

- [ ] **Step 3: Full workspace gate**
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. After `cargo fmt --all`, run `git status --short` and stage every fmt-touched file. The new `subproperty_axioms` tests (6) must pass; no regression elsewhere. Report the aggregate.

- [ ] **Step 4: Commit**
```bash
cd /data/dumontier/rustdl
git add -A
git status --short
git commit -m "docs(subproperty-axioms): document materialize_inferred_sub{object,data}property_axioms"
```

---

## Self-review notes (author)

- **Spec coverage:** object signed-role closure + inverse propagation → Task 1 Step 4 (`object_inverse_propagation` test); data closure → Task 1; reflexive/top/bottom exclusion → both functions' final filter; soundness re-check via `entails` → Task 1 Step 6; inconsistency error → Task 1 Step 6; Python → Task 2; docs + byte-identical gate → Task 3.
- **Soundness:** told/equivalent/inverse subsumption are sound; re-verified via `entails`. Read-only (consistency pre-check is the existing saturator). Inconsistency → `Err`.
- **No placeholders:** code complete; "match against convert.rs" notes only for the (verified) horned-owl field accessors.
- **Type consistency:** both functions return `Result<Vec<(String,String)>, ReasonError>`; signed-role model `(String, bool)` internal only; output is `(sub_iri, sup_iri)`.
- **API risk flagged inline:** `op.0.as_ref()` / `ax.0.0.as_ref()` nested-field access; `ObjectPropertyExpression::InverseObjectProperty` variant name; the `signed` generic fn — each points at `convert.rs`.
