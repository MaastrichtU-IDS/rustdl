# `materialize_existential_successors` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A reasoner function + Python binding that returns a blank-node representation of the entailed existential successors of named individuals — one row `(subject, property, witness_blank_id, filler_class)` per entailed `a : ∃R.C`.

**Architecture:** Structural — `realize()` gives each named individual's entailed types; a told-`∃` index (`X → {(R,C)}` from `SubClassOf`/`EquivalentClasses`) plus `a:X` yields the sound `a:∃R.C`; emit one stable blank node per distinct `(a,R,C)`. Consistency pre-check gates `Inconsistent`. Sound in any fragment (no gate); 1-step (no recursion).

**Tech Stack:** Rust (edition 2024), horned-owl model, `owl-dl-reasoner` (`realize`), `owl-dl-py`.

**Spec:** `docs/superpowers/specs/2026-06-21-existential-successors-design.md`
**Branch:** `feat/existential-successors`

**IMPORTANT semantics (state in docs):** the rows are NOT entailed ground triples — a
specific `a R _:x` is not entailed; `a : ∃R.C` is. Rows are a representation of
entailed existentials. The function name reflects this (`existential_successors`, not
`property_assertions`).

---

## Key facts (verified)

- `owl_dl_reasoner::realize(onto) -> Result<Realization, ReasonError>`; `Realization::individuals() -> &[String]`, `entailed_types(ind: &str) -> &[String]` (all entailed named-class types).
- `ClassExpression::ObjectSomeValuesFrom { ope: ObjectPropertyExpression<A>, bce: Box<ClassExpression<A>> }`; `ObjectPropertyExpression::ObjectProperty(op)` (IRI `op.0.as_ref()`); `ClassExpression::Class(c)` (IRI `c.0.as_ref()`); `ClassExpression::ObjectIntersectionOf(Vec<CE>)`.
- `Component::SubClassOf { sub, sup }` (both `ClassExpression`); `Component::EquivalentClasses(ax)` with `ax.0: Vec<ClassExpression>`.
- Reasoner siblings in `lib.rs`: `materialize_data_property_assertions`, `ReasonError::Inconsistent`, `owl_dl_core::convert::convert_ontology`, `abox_saturation::saturate_abox_consistency`, `realize`. Put the new fn in `lib.rs`.
- Python family: `crates/owl-dl-py/src/materialize.rs` (`reason_error_to_py` handles `Inconsistent`).
- No CLI.

## File structure

- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — the function.
- **Create** `crates/owl-dl-reasoner/tests/existential_successors.rs` — tests.
- **Modify** `crates/owl-dl-py/src/materialize.rs` — Python binding.
- **Modify** `README.md`, `CLAUDE.md` — docs.

---

### Task 1: Reasoner function + tests

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; Create `crates/owl-dl-reasoner/tests/existential_successors.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/existential-successors
```

- [ ] **Step 2: Write the failing test** — create `crates/owl-dl-reasoner/tests/existential_successors.rs`:

```rust
//! Integration tests for materialize_existential_successors.

use horned_owl::model::{
    Build, ClassAssertion, ClassExpression as CE, DeclareClass, DeclareNamedIndividual,
    DeclareObjectProperty, Individual, MutableOntology, ObjectPropertyExpression as OPE, SubClassOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_existential_successors;

type Rc = std::rc::Rc<str>;

fn some(b: &Build<Rc>, r: &str, c: &str) -> CE<Rc> {
    CE::ObjectSomeValuesFrom {
        ope: OPE::ObjectProperty(b.object_property(r)),
        bce: Box::new(CE::Class(b.class(c))),
    }
}

// Person ⊑ ∃hasParent.Person ; a : Person
//   → exactly one row (a, hasParent, _:b, Person); blank id is fresh; 1-step (no
//     row for the witness itself).
#[test]
fn one_step_existential_successor() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareClass(b.class("urn:Person")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf { sub: CE::Class(b.class("urn:Person")), sup: some(&b, "urn:hasParent", "urn:Person") });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:Person")), i: Individual::Named(b.named_individual("urn:a")) });

    let got = materialize_existential_successors(&o).expect("materialize");
    assert_eq!(got.len(), 1, "exactly one existential successor; got: {got:?}");
    let (s, p, w, c) = &got[0];
    assert_eq!(s, "urn:a");
    assert_eq!(p, "urn:hasParent");
    assert_eq!(c, "urn:Person");
    assert!(w.starts_with("_:"), "witness must be a blank node, got {w}");
    assert_ne!(w, "urn:a", "witness is not a named individual");
}

// Determinism: two calls give identical output.
#[test]
fn deterministic() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareClass(b.class("urn:Person")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf { sub: CE::Class(b.class("urn:Person")), sup: some(&b, "urn:hasParent", "urn:Person") });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:Person")), i: Individual::Named(b.named_individual("urn:a")) });

    let a = materialize_existential_successors(&o).expect("m1");
    let b2 = materialize_existential_successors(&o).expect("m2");
    assert_eq!(a, b2);
}
```

NOTE: shapes (`SubClassOf{sub,sup}`, `ClassAssertion{ce,i}`, `Individual::Named`, `ObjectSomeValuesFrom{ope,bce:Box}`, `ObjectPropertyExpression::ObjectProperty`, `CE::Class`) verified against `justify.rs`/`diagnose.rs`/`model.rs`. Match those files if anything differs; report.

- [ ] **Step 3: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --test existential_successors` → FAIL (function undefined).

- [ ] **Step 4: Implement** — add to `crates/owl-dl-reasoner/src/lib.rs` (next to `materialize_data_property_assertions`; match its qualified-path style: `owl_dl_core::convert::convert_ontology`, `abox_saturation::...`, `realize` is a crate-public fn — call `realize(onto)?`):

```rust
/// Materialize the entailed existential successors of named individuals as a
/// blank-node representation: one row `(subject_iri, property_iri,
/// witness_blank_id, filler_class_iri)` per entailed `a : ∃R.C`.
///
/// NOTE: these are NOT entailed ground triples — a specific witness edge
/// `a R _:x` is not entailed (witnesses differ across models); what is entailed is
/// `a : ∃R.C`. Each row represents one such entailed existential, with a fresh
/// deterministic blank node. Sound by construction (`a:X` from `realize` +
/// told `X ⊑ ∃R.C`). Under-approximate: told `∃` only, simple named role + named
/// class filler, 1-step (no recursion). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if inconsistent; [`ReasonError::Conversion`].
pub fn materialize_existential_successors<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String, String, String)>, ReasonError> {
    use horned_owl::model::{
        ClassExpression as CE, Component as C, ObjectPropertyExpression as OPE,
    };
    use std::collections::{BTreeMap, BTreeSet};

    let internal = convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    // Collect told (R, C) from a superclass expression (top-level + conjuncts).
    fn collect_exists<A: ForIRI>(sup: &CE<A>, out: &mut BTreeSet<(String, String)>) {
        match sup {
            CE::ObjectIntersectionOf(cs) => {
                for c in cs {
                    collect_exists(c, out);
                }
            }
            CE::ObjectSomeValuesFrom { ope, bce } => {
                if let (OPE::ObjectProperty(r), CE::Class(c)) = (ope, &**bce) {
                    out.insert((r.0.as_ref().to_string(), c.0.as_ref().to_string()));
                }
            }
            _ => {}
        }
    }

    // told-∃ index: X_iri → {(R_iri, C_iri)}.
    let mut told: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for ac in onto {
        match &ac.component {
            C::SubClassOf(ax) => {
                if let CE::Class(x) = &ax.sub {
                    let mut set = BTreeSet::new();
                    collect_exists(&ax.sup, &mut set);
                    if !set.is_empty() {
                        told.entry(x.0.as_ref().to_string()).or_default().extend(set);
                    }
                }
            }
            C::EquivalentClasses(ax) => {
                for (i, mi) in ax.0.iter().enumerate() {
                    if let CE::Class(x) = mi {
                        let mut set = BTreeSet::new();
                        for (j, mj) in ax.0.iter().enumerate() {
                            if i != j {
                                collect_exists(mj, &mut set);
                            }
                        }
                        if !set.is_empty() {
                            told.entry(x.0.as_ref().to_string()).or_default().extend(set);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if told.is_empty() {
        return Ok(Vec::new());
    }

    let realization = realize(onto)?;
    // Distinct (a, R, C) with a:X entailed and X ⊑ ∃R.C told.
    let mut triples: BTreeSet<(String, String, String)> = BTreeSet::new();
    for a in realization.individuals() {
        for x in realization.entailed_types(a) {
            if let Some(rcs) = told.get(x) {
                for (r, c) in rcs {
                    triples.insert((a.clone(), r.clone(), c.clone()));
                }
            }
        }
    }

    // One stable blank id per distinct (a,R,C), in sorted order.
    let out: Vec<(String, String, String, String)> = triples
        .into_iter()
        .enumerate()
        .map(|(i, (a, r, c))| (a, r, format!("_:b{i}"), c))
        .collect();
    Ok(out)
}
```
Confirm `realize` is callable unqualified here (it is `pub fn realize` re-exported / in scope in lib.rs — match the sibling). If `&**bce` pattern doesn't typecheck, use `bce.as_ref()`. Report adjustments.

- [ ] **Step 5: Run** — `cargo test -p owl-dl-reasoner --test existential_successors` → 2 passed. If `one_step_existential_successor` finds 0 rows, `realize` isn't returning `urn:Person` in `entailed_types(urn:a)` — check it includes the asserted type; report. Paste the `test result:` line.

- [ ] **Step 6: Add entailed-type / dedup / negative / inconsistency tests** — append:

```rust
use horned_owl::model::{ClassExpression, DisjointClasses, EquivalentClasses};

// a:Y, Y⊑X, X⊑∃r.C ⇒ row present (uses entailed types, not just asserted).
#[test]
fn entailed_not_asserted_type() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:Y", "urn:C"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf { sub: CE::Class(b.class("urn:Y")), sup: CE::Class(b.class("urn:X")) });
    o.insert(SubClassOf { sub: CE::Class(b.class("urn:X")), sup: some(&b, "urn:r", "urn:C") });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:Y")), i: Individual::Named(b.named_individual("urn:a")) });

    let got = materialize_existential_successors(&o).expect("materialize");
    assert!(
        got.iter().any(|(s, p, _, c)| s == "urn:a" && p == "urn:r" && c == "urn:C"),
        "got: {got:?}"
    );
}

// Two distinct existentials ⇒ two rows, two distinct blanks.
#[test]
fn distinct_existentials_distinct_blanks() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:C", "urn:D"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf {
        sub: CE::Class(b.class("urn:X")),
        sup: ClassExpression::ObjectIntersectionOf(vec![some(&b, "urn:r", "urn:C"), some(&b, "urn:r", "urn:D")]),
    });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:X")), i: Individual::Named(b.named_individual("urn:a")) });

    let got = materialize_existential_successors(&o).expect("materialize");
    let blanks: std::collections::BTreeSet<&String> = got.iter().map(|(_, _, w, _)| w).collect();
    assert_eq!(got.len(), 2, "got: {got:?}");
    assert_eq!(blanks.len(), 2, "distinct existentials → distinct blanks");
}

// No entailed existential ⇒ no rows.
#[test]
fn no_existential_no_rows() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareClass(b.class("urn:Person")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:Person")), i: Individual::Named(b.named_individual("urn:a")) });
    assert!(materialize_existential_successors(&o).expect("materialize").is_empty());
}

// Inconsistent ⇒ Err.
#[test]
fn inconsistent_is_error() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareNamedIndividual(b.named_individual("urn:i")));
    o.insert(DisjointClasses(vec![CE::Class(b.class("urn:A")), CE::Class(b.class("urn:B"))]));
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:A")), i: Individual::Named(b.named_individual("urn:i")) });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:B")), i: Individual::Named(b.named_individual("urn:i")) });
    let _ = EquivalentClasses::<Rc>; // keep import used if needed; remove if unused
    assert!(materialize_existential_successors(&o).is_err());
}
```
If the `let _ = EquivalentClasses::<Rc>;` line causes a problem, delete it and drop `EquivalentClasses` from the import (it was only to avoid an unused-import warning — keep imports clean). Match any differing shapes against `diagnose.rs`. Report.

- [ ] **Step 7: Run all + clippy + fmt**
```bash
cargo test -p owl-dl-reasoner --test existential_successors
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
Expected 5 passed; clippy/fmt green (the 4-tuple return may trip `type_complexity` → add a scoped `#[allow(clippy::type_complexity)]` on the fn, as the sibling materializers do). Re-run after fmt.

- [ ] **Step 8: Commit**
```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/existential_successors.rs
git commit -m "feat(reasoner): materialize_existential_successors (1-step ∃-witness representation)"
```

---

### Task 2: Python binding

**Files:** Modify `crates/owl-dl-py/src/materialize.rs`

- [ ] **Step 1: Add the binding** — after the existing materializers:

```rust
/// Returns the entailed existential successors of named individuals as 4-tuples
/// `(subject, property, witness_blank_id, filler_class)` — one per entailed
/// `a : ∃R.C`. NOTE: these are a blank-node REPRESENTATION of entailed existential
/// restrictions, NOT entailed ground triples (the specific witness is model-relative).
/// Raises if the ontology is inconsistent.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn materialize_existential_successors(
    path: &str,
) -> PyResult<Vec<(String, String, String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::materialize_existential_successors(&ontology).map_err(reason_error_to_py)
}
```
Register in `register`:
```rust
    m.add_function(wrap_pyfunction!(materialize_existential_successors, m)?)?;
```

- [ ] **Step 2: Build + clippy + fmt**
```bash
cargo build -p owl-dl-py
cargo clippy -p owl-dl-py --all-targets -- -D warnings
cargo fmt -p owl-dl-py
```
Green. (If clippy `doc_markdown` flags `∃R.C`-style text, backtick as needed.)

- [ ] **Step 3: Commit**
```bash
git add crates/owl-dl-py/src/materialize.rs
git commit -m "feat(py): materialize_existential_successors binding"
```

---

### Task 3: Docs + final gate

**Files:** Modify `README.md`, `CLAUDE.md`

- [ ] **Step 1: README** — if the Python `materialize_*` example list exists, add a line:
```python
succ = rustdl.materialize_existential_successors("ontology.ofn")  # entailed ∃-successors (blank-node witnesses)
```
Match the surrounding style. (No CLI.)

- [ ] **Step 2: CLAUDE.md** — append to the materialize feature documentation:
```
`materialize_existential_successors` (reasoner + Python) returns a blank-node
representation of named individuals' entailed existential successors — one
`(subject, property, witness_blank, filler_class)` row per entailed `a : ∃R.C`
(told-∃ over realized types, 1-step). NOTE: a representation of entailed existentials,
NOT entailed ground triples (witnesses are model-relative). See
`docs/superpowers/specs/2026-06-21-existential-successors-design.md`.
```

- [ ] **Step 3: Full workspace gate**
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. After `cargo fmt --all`, `git status --short` and stage every fmt-touched file. The new `existential_successors` tests (5) must pass; no regression. Report the aggregate.

- [ ] **Step 4: Commit**
```bash
cd /data/dumontier/rustdl
git add -A
git status --short
git commit -m "docs(existential-successors): document materialize_existential_successors"
```

---

## Self-review notes (author)

- **Spec coverage:** told-∃ index + realized-types join → Task 1 Step 4; 1-step (no recursion) + one-row assertion → `one_step_existential_successor`; entailed-not-asserted → its test; dedup/distinct-blanks → `distinct_existentials_distinct_blanks`; determinism → `deterministic`; negative + inconsistency → their tests; Python → Task 2; docs/gate → Task 3.
- **Soundness/honesty:** every row maps to a sound `a:∃R.C` (realize-entailed `a:X` + told `X⊑∃R.C`); docs state plainly these are NOT entailed ground triples. Read-only; consistency pre-check for the `Err` path.
- **No placeholders:** code complete; "match against repo" notes only for horned-owl shapes + the `&**bce` deref.
- **Type consistency:** `materialize_existential_successors(&SetOntology<A>) -> Result<Vec<(String,String,String,String)>, ReasonError>`; 4-tuple order (subject, property, witness_blank, filler_class) consistent reasoner↔Python; blank ids `_:b{i}` over sorted distinct `(a,R,C)`.
- **API risk flagged inline:** `realize` call style, `&**bce` vs `bce.as_ref()`, `type_complexity` allow, the throwaway `EquivalentClasses` import line.
