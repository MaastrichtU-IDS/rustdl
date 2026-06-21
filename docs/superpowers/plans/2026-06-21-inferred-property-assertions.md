# `materialize_inferred_property_assertions` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface inferred object property assertions — reasoner fn `materialize_object_property_assertions`, Python `materialize_inferred_property_assertions`, and CLI `realize --properties` — over named individuals, reusing the ABox saturator's already-computed role edges.

**Architecture:** The ABox saturator computes a `HashSet<RawEdge>` (`RawEdge = (RoleId, IndividualId, IndividualId)`) during its fixpoint but returns only counts. Expose that edge set; a reasoner function maps it to IRI triples; Python + CLI surfaces wrap it. Presentation/extraction only — saturation *logic* unchanged (consistency byte-identical).

**Tech Stack:** Rust (edition 2024), horned-owl model, `owl-dl-core` (`Vocabulary`, `convert_ontology`), `owl-dl-reasoner` (`abox_saturation`), `owl-dl-py` (pyo3).

**Spec:** `docs/superpowers/specs/2026-06-21-inferred-property-assertions-design.md`
**Branch:** `feat/inferred-property-assertions`

---

## Key facts (verified)

- `crates/owl-dl-reasoner/src/abox_saturation.rs`: `type RawEdge = (RoleId, IndividualId, IndividualId);` (role, subject, object — `edge(R,a,b)` means `a R b`). The fixpoint builds `let mut edges: HashSet<RawEdge> = HashSet::new();` (line ~450) and returns `SaturationResult` (constructed at line ~462) with counts only. `RoleId`/`IndividualId` come from `owl_dl_core::ir` (already imported there). Inverse edges are materialized onto base roles, so every edge's `RoleId` is a concrete base property with concrete subject/object.
- `SaturationResult` is `pub` (fields `clash`, `chain2_fires`, `chain3_fires`, `sex_clash_candidates`, `type_additions`, `edge_additions`).
- `owl_dl_core::vocab::Vocabulary` (accessed as `internal.vocabulary`): `individual_iri(IndividualId) -> &str`, `role_iri(RoleId) -> &str`, `class_iri(ClassId) -> &str` (and the `*_id(iri)` inverses).
- `owl_dl_core::convert::convert_ontology(onto) -> Result<InternalOntology, ConversionError>`; `ReasonError: From<ConversionError>` (so `?` works). `ReasonError` (in `lib.rs`) has NO `Inconsistent` variant — this plan adds one.
- `owl_dl_reasoner::justify::{entails, Entailment}` — `Entailment::ObjectPropertyAssertion { source, prop, target }` for the soundness re-check in tests.
- Python: `crates/owl-dl-py/src/materialize.rs` holds the `materialize_*` family (`#[pyfunction]`, `Vec<(String, …)>`, `load::load_path` + `reason_error_to_py`); `crates/owl-dl-py/src/errors.rs` has `reason_error_to_py` (a match on `ReasonError` — will need an arm for the new variant).
- CLI: `crates/owl-dl-cli/src/main.rs` — the `Realize { file, saturation_only }` variant (~line 161) and its handler (calls `realize`/`print_realization`). `parse_ofn_with_pm` etc. available.

## File structure

- **Modify** `crates/owl-dl-reasoner/src/abox_saturation.rs` — add `edges` to `SaturationResult`.
- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — `ReasonError::Inconsistent` + `materialize_object_property_assertions`.
- **Create** `crates/owl-dl-reasoner/tests/property_assertions.rs` — reasoner tests.
- **Modify** `crates/owl-dl-py/src/materialize.rs` + `errors.rs` — Python binding + error arm.
- **Modify** `crates/owl-dl-cli/src/main.rs` — `realize --properties`.
- **Modify** `README.md`, `CLAUDE.md` — docs.

---

### Task 1: Expose the saturator's edge set

**Files:** Modify `crates/owl-dl-reasoner/src/abox_saturation.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/inferred-property-assertions
```

- [ ] **Step 2: Add the `edges` field to `SaturationResult`**

In `crates/owl-dl-reasoner/src/abox_saturation.rs`, add to the `SaturationResult` struct (after `edge_additions`):
```rust
    /// The full set of derived role edges `(role_id, subject, object)` over named
    /// individuals at fixpoint (asserted + propagated via hierarchy/inverse/symmetric/
    /// chains/transitivity). Empty when a clash was found. Sound: every edge is
    /// entailed. Used by `materialize_object_property_assertions`; ignored by the
    /// consistency pre-check.
    pub edges: Vec<(RoleId, IndividualId, IndividualId)>,
```

- [ ] **Step 3: Populate it**

Find where `SaturationResult { … }` is constructed (the `let mut result = SaturationResult { … };` initializer, ~line 462) and add `edges: Vec::new(),` to the initializer. Then, immediately BEFORE the function's final `return result;` / `result` tail expression (after the fixpoint loop completes, where `edges` the local `HashSet` is fully populated), set:
```rust
    result.edges = edges.iter().copied().collect();
```
Place this so it runs on the normal (non-early-clash) return path. If the function early-returns on clash, leave `edges` empty there (the field stays `Vec::new()`), which matches "empty when a clash was found". Read the function's return structure and place the assignment correctly. If `SaturationResult` is constructed in more than one place (e.g. tests), add `edges: Vec::new()` to each.

- [ ] **Step 4: Build + existing saturation tests**

```bash
cargo build -p owl-dl-reasoner
cargo test -p owl-dl-reasoner --lib abox_saturation
cargo test -p owl-dl-reasoner --test '*' 2>/dev/null | grep -iE 'abox|saturation|test result' | tail -5
```
Expected: compiles; existing ABox-saturation tests still pass (logic unchanged). If there is a dedicated abox-saturation test file, run it. Confirm no behavior change.

- [ ] **Step 5: clippy + fmt**

```bash
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
Green. (`edges` is read by Task 2; until then clippy may not flag a `pub` field as dead — pub struct fields are not dead-code-linted. If it does, that's fine to leave as the struct is pub.)

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/abox_saturation.rs
git commit -m "feat(abox-sat): expose derived role edges on SaturationResult"
```

---

### Task 2: Reasoner function + `ReasonError::Inconsistent`

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; Create `crates/owl-dl-reasoner/tests/property_assertions.rs`

- [ ] **Step 1: Write the failing integration test** — create `crates/owl-dl-reasoner/tests/property_assertions.rs`:

```rust
//! Integration tests for materialize_object_property_assertions.

use horned_owl::model::{
    Build, DeclareClass, DeclareObjectProperty, MutableOntology, NamedIndividual,
    ObjectProperty, ObjectPropertyAssertion, ObjectPropertyExpression, SubObjectPropertyOf,
    SubObjectPropertyExpression,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_object_property_assertions;

type Rc = std::rc::Rc<str>;

fn opa(b: &Build<Rc>, prop: &str, s: &str, o: &str) -> ObjectPropertyAssertion<Rc> {
    ObjectPropertyAssertion {
        ope: ObjectPropertyExpression::ObjectProperty(b.object_property(prop)),
        from: b.named_individual(s),
        to: b.named_individual(o),
    }
}

// hasParent ⊑ hasAncestor ; hasParent(a,b), hasParent(b,c)
//   → result contains hasAncestor(a,b) and hasAncestor(b,c) (and the asserted hasParent).
#[test]
fn subproperty_entailed_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasAncestor")));
    for i in ["urn:a", "urn:b", "urn:c"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(b.named_individual(i)));
    }
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyExpression(
            ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasParent")),
        ),
        sup: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasAncestor")),
    });
    o.insert(opa(&b, "urn:hasParent", "urn:a", "urn:b"));
    o.insert(opa(&b, "urn:hasParent", "urn:b", "urn:c"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    let triple = |s: &str, p: &str, t: &str| (s.to_string(), p.to_string(), t.to_string());
    assert!(got.contains(&triple("urn:a", "urn:hasAncestor", "urn:b")), "got: {got:?}");
    assert!(got.contains(&triple("urn:b", "urn:hasAncestor", "urn:c")), "got: {got:?}");
    // asserted edge also present (full closure semantics)
    assert!(got.contains(&triple("urn:a", "urn:hasParent", "urn:b")));
    // negative control: a non-entailed edge is absent
    assert!(!got.contains(&triple("urn:a", "urn:hasAncestor", "urn:a")));
}
```

NOTE: the horned-owl field/variant names above (`ObjectPropertyAssertion { ope, from, to }`, `SubObjectPropertyOf { sub, sup }`, `SubObjectPropertyExpression::ObjectPropertyExpression`, `ObjectPropertyExpression::ObjectProperty`, `DeclareNamedIndividual`, `DeclareObjectProperty`) are best-effort for the pinned rev. If any does not compile, find the exact shape in `crates/owl-dl-reasoner/src/justify.rs` (it constructs `ObjectPropertyAssertion`, `SameIndividual`, etc.) and `convert.rs`, and match it. Keep the test SEMANTICS identical. Report adjustments.

- [ ] **Step 2: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --test property_assertions` → FAIL (`materialize_object_property_assertions` undefined).

- [ ] **Step 3: Add `ReasonError::Inconsistent`** — in `crates/owl-dl-reasoner/src/lib.rs`, add a variant to `enum ReasonError`:
```rust
    /// The ontology is inconsistent — every assertion is vacuously entailed, so
    /// enumerating (e.g. property assertions) is meaningless.
    #[error("ontology is inconsistent; every assertion is trivially entailed")]
    Inconsistent,
```
If any `match` on `ReasonError` in `owl-dl-reasoner` is now non-exhaustive (compiler will say), add an arm. (`owl-dl-py`'s `reason_error_to_py` is a separate crate, handled in Task 3.)

- [ ] **Step 4: Implement `materialize_object_property_assertions`** — add to `crates/owl-dl-reasoner/src/lib.rs` (near `realize` / `is_consistent`; `convert_ontology` is already imported there):

```rust
/// Materialize the inferred OBJECT property assertions entailed over **named
/// individuals** — `(subject_iri, property_iri, object_iri)` triples, the full
/// entailed closure (asserted + derived via sub-property hierarchy / inverse /
/// symmetric / role chains / transitivity). Sound under-approximation: omits edges
/// to anonymous existential witnesses and disjunctive-derived edges. Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent (everything is
/// vacuously entailed); [`ReasonError::Conversion`] on lowering failure.
pub fn materialize_object_property_assertions<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String, String)>, ReasonError> {
    let internal = convert_ontology(onto)?;
    let result = abox_saturation::saturate_abox_consistency(&internal);
    if result.clash {
        return Err(ReasonError::Inconsistent);
    }
    let vocab = &internal.vocabulary;
    const TOP: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
    const BOT: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";
    let mut out: Vec<(String, String, String)> = result
        .edges
        .iter()
        .map(|&(rid, a, b)| {
            (
                vocab.individual_iri(a).to_string(),
                vocab.role_iri(rid).to_string(),
                vocab.individual_iri(b).to_string(),
            )
        })
        .filter(|(_, p, _)| p != TOP && p != BOT)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}
```
Confirm `convert_ontology`, `abox_saturation`, `ForIRI`, `SetOntology` are in scope in `lib.rs` (they are — used by `is_consistent`). If `abox_saturation` is a private mod, call it as `crate::abox_saturation::...` (it is `pub mod abox_saturation;`).

- [ ] **Step 5: Run** — `cargo test -p owl-dl-reasoner --test property_assertions` → PASS. Paste the `test result:` line. If `hasAncestor(a,b)` is missing, the saturator isn't propagating sub-properties as expected — STOP and report (real bug).

- [ ] **Step 6: Add inverse + inconsistency + soundness tests** — append to `crates/owl-dl-reasoner/tests/property_assertions.rs`:

```rust
use owl_dl_reasoner::justify::{Entailment, entails};

// InverseObjectProperties(hasChild, hasParent), hasParent(a,b) → hasChild(b,a).
#[test]
fn inverse_entailed_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasChild")));
    for i in ["urn:a", "urn:b"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(b.named_individual(i)));
    }
    o.insert(horned_owl::model::InverseObjectProperties(
        b.object_property("urn:hasChild"),
        b.object_property("urn:hasParent"),
    ));
    o.insert(opa(&b, "urn:hasParent", "urn:a", "urn:b"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    assert!(
        got.contains(&("urn:b".to_string(), "urn:hasChild".to_string(), "urn:a".to_string())),
        "got: {got:?}"
    );
}

// Every returned triple is genuinely entailed (soundness property).
#[test]
fn every_triple_is_entailed() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasAncestor")));
    for i in ["urn:a", "urn:b", "urn:c"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(b.named_individual(i)));
    }
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyExpression(
            ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasParent")),
        ),
        sup: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasAncestor")),
    });
    o.insert(opa(&b, "urn:hasParent", "urn:a", "urn:b"));
    o.insert(opa(&b, "urn:hasParent", "urn:b", "urn:c"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    assert!(!got.is_empty());
    for (s, p, t) in &got {
        let q = Entailment::ObjectPropertyAssertion {
            source: s.clone(),
            prop: p.clone(),
            target: t.clone(),
        };
        assert!(entails(&o, &q).expect("entails"), "{s} {p} {t} must be entailed");
    }
}

// Inconsistent ontology → Err.
#[test]
fn inconsistent_is_error() {
    use horned_owl::model::ClassExpression as CE;
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(horned_owl::model::DeclareNamedIndividual(b.named_individual("urn:i")));
    o.insert(horned_owl::model::DisjointClasses(vec![CE::Class(b.class("urn:A")), CE::Class(b.class("urn:B"))]));
    o.insert(horned_owl::model::ClassAssertion { ce: CE::Class(b.class("urn:A")), i: b.named_individual("urn:i").into() });
    o.insert(horned_owl::model::ClassAssertion { ce: CE::Class(b.class("urn:B")), i: b.named_individual("urn:i").into() });

    assert!(materialize_object_property_assertions(&o).is_err());
}
```
Match any differing horned-owl shapes against `justify.rs` / `diagnose.rs` (e.g. `ClassAssertion { ce, i }`, `InverseObjectProperties(..)`, `DisjointClasses(vec![..])` are used there). Report adjustments.

- [ ] **Step 7: Run all + clippy + fmt**
```bash
cargo test -p owl-dl-reasoner --test property_assertions
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
4 passed; clippy/fmt green; re-run after fmt.

- [ ] **Step 8: Commit**
```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/property_assertions.rs
git commit -m "feat(reasoner): materialize_object_property_assertions + ReasonError::Inconsistent"
```

---

### Task 3: Python binding

**Files:** Modify `crates/owl-dl-py/src/materialize.rs`, `crates/owl-dl-py/src/errors.rs`

- [ ] **Step 1: Handle the new error variant** — in `crates/owl-dl-py/src/errors.rs`, `reason_error_to_py` matches on `ReasonError`. Add an arm for `ReasonError::Inconsistent` mapping to a Python exception (mirror the existing arms' style, e.g. a `PyValueError`/the crate's error type with the message "ontology is inconsistent; every assertion is trivially entailed"). READ the function first to match its exact mapping style.

- [ ] **Step 2: Add the binding** — in `crates/owl-dl-py/src/materialize.rs`, add after `materialize_inferred_class_assertions`:

```rust
/// Returns every inferred object property assertion `(subject, property, object)`
/// entailed over named individuals (asserted + derived via property hierarchy /
/// inverse / symmetric / role chains / transitivity). Sound under-approximation
/// (no anonymous-witness or disjunctive-derived edges). Raises if the ontology is
/// inconsistent.
#[pyfunction]
pub(crate) fn materialize_inferred_property_assertions(
    path: &str,
) -> PyResult<Vec<(String, String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::materialize_object_property_assertions(&ontology).map_err(reason_error_to_py)
}
```
And register it in `register`:
```rust
    m.add_function(wrap_pyfunction!(materialize_inferred_property_assertions, m)?)?;
```

- [ ] **Step 3: Build** — `cargo build -p owl-dl-py`. Must compile (the new error arm + binding). If `owl-dl-py` needs a special build (maturin/abi3), plain `cargo build -p owl-dl-py` still type-checks the Rust; that's the gate here.

- [ ] **Step 4: clippy + fmt**
```bash
cargo clippy -p owl-dl-py --all-targets -- -D warnings
cargo fmt -p owl-dl-py
```

- [ ] **Step 5: Commit**
```bash
git add crates/owl-dl-py/src/materialize.rs crates/owl-dl-py/src/errors.rs
git commit -m "feat(py): materialize_inferred_property_assertions binding"
```

---

### Task 4: CLI `realize --properties`

**Files:** Modify `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add the `--properties` flag** — in `enum Command`, the `Realize { … }` variant, add:
```rust
        /// Also print inferred object property assertions (subject<TAB>property<TAB>object).
        #[arg(long)]
        properties: bool,
```

- [ ] **Step 2: Update the handler** — in `Command::Realize { … } => { … }`: add `properties,` to the destructured fields; after the existing `print_realization(&r);` call, add:
```rust
            if properties {
                match owl_dl_reasoner::materialize_object_property_assertions(&onto) {
                    Ok(triples) => {
                        println!("# inferred object property assertions");
                        for (s, p, o2) in triples {
                            println!("{s}\t{p}\t{o2}");
                        }
                    }
                    Err(e) => {
                        eprintln!("# object property assertions unavailable: {e}");
                    }
                }
            }
```
NOTE: the `Realize` handler reuses `onto` (it parses with `parse_ofn`). Confirm `onto` is in scope at that point (the handler parses it). If the handler consumes `onto` before this point, re-parse or reorder so `onto` is available. The `saturation_only` arm: `--properties` works regardless (materialize uses its own saturator); keep it simple — print properties after whichever realization ran.

- [ ] **Step 3: Build + smoke-test**
```bash
cat > /tmp/prop-smoke.ofn <<'EOF'
Prefix(:=<urn:>)
Ontology(
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
  Declaration(ObjectProperty(:hasParent)) Declaration(ObjectProperty(:hasAncestor))
  SubObjectPropertyOf(:hasParent :hasAncestor)
  ObjectPropertyAssertion(:hasParent :a :b)
  ObjectPropertyAssertion(:hasParent :b :c)
)
EOF
cargo build -p owl-dl-cli --release
echo "--- without --properties (must be unchanged: no property section) ---"
./target/release/rustdl realize /tmp/prop-smoke.ofn | grep -c 'hasAncestor'
echo "--- with --properties ---"
./target/release/rustdl realize /tmp/prop-smoke.ofn --properties
```
Expected: without the flag, `hasAncestor` count = 0 (output unchanged). With `--properties`, output includes `# inferred object property assertions` and lines `urn:a\turn:hasAncestor\turn:b`, `urn:b\turn:hasAncestor\turn:c` (plus the asserted hasParent edges). Paste the actual `--properties` output. If `hasAncestor` is absent with the flag, STOP and report.

- [ ] **Step 4: clippy + fmt**
```bash
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```

- [ ] **Step 5: Commit**
```bash
git add crates/owl-dl-cli/src/main.rs
git commit -m "feat(cli): realize --properties (inferred object property assertions)"
```

---

### Task 5: Docs + final gate

**Files:** Modify `README.md`, `CLAUDE.md`

- [ ] **Step 1: README** — in the CLI block, change the `realize` line / add a note:
```
rustdl realize   ontology.ofn [--properties]  # per-individual types (+ inferred object property assertions)
```
Match column alignment. Also, if the README/Python example section mentions the materialize_* family, add `materialize_inferred_property_assertions` alongside (optional, only if such a list exists).

- [ ] **Step 2: CLAUDE.md** — append to the `owl-dl-reasoner` (or `owl-dl-cli`) area a sentence:
```
`materialize_object_property_assertions` (reasoner) / `materialize_inferred_property_assertions`
(Python) / `realize --properties` (CLI) surface inferred OBJECT property assertions over
named individuals, reusing the ABox saturator's derived edges (sound under-approximation:
no anonymous-witness/disjunctive edges; errors on inconsistency). See
`docs/superpowers/specs/2026-06-21-inferred-property-assertions-design.md`.
```

- [ ] **Step 3: Full workspace gate**
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. Report any NON-ignored failure verbatim; fix only feature-related clippy, stop+report on unrelated pre-existing issues. NOTE the `cargo test --workspace` must show the new `property_assertions` tests passing and **no regression** in `abox_saturation` / consistency tests (the engine-logic-unchanged guarantee).

- [ ] **Step 4: Commit**
```bash
git add README.md CLAUDE.md
git commit -m "docs(property-assertions): document materialize/realize --properties"
```

---

## Self-review notes (author)

- **Spec coverage:** engine edge exposure → Task 1; reasoner fn + inconsistency error + semantics (full named closure, top/bottom excluded, sorted/deduped) → Task 2; soundness property test (`entails` re-check) → Task 2 Step 6; Python binding → Task 3; CLI `realize --properties` (default-off, output-unchanged) → Task 4; docs + byte-identical-consistency gate → Task 5.
- **Soundness:** every triple comes from the saturator's entailment-preserving edge rules; the soundness test re-verifies each via `entails`. Consistency/classification logic unchanged (only the discarded edge set is now returned).
- **No placeholders:** code complete; the only "match against repo" notes are horned-owl axiom-constructor shapes (Task 2) and `reason_error_to_py` style (Task 3) — each points at an in-repo example.
- **Type consistency:** `materialize_object_property_assertions(&SetOntology<A>) -> Result<Vec<(String,String,String)>, ReasonError>` and the `(subject, property, object)` order are consistent across reasoner/Python/CLI; `RawEdge = (RoleId, IndividualId, IndividualId)` mapped as (subject=a, property=rid, object=b).
- **API risk flagged inline:** horned-owl `ObjectPropertyAssertion`/`SubObjectPropertyOf`/`InverseObjectProperties` shapes, `ReasonError` exhaustive-match sites, `reason_error_to_py` mapping, and `onto`-in-scope in the Realize handler.
