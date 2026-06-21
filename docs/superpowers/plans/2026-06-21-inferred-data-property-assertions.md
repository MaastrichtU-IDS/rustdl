# `materialize_inferred_data_property_assertions` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface inferred DATA property assertions — reasoner `materialize_data_property_assertions`, Python `materialize_inferred_data_property_assertions`, and an extension of CLI `realize --properties` — over named individuals, computed structurally (sub-data-property hierarchy + equivalent-data-properties).

**Architecture:** A pure structural closure over the ontology's horned-owl axioms (no engine edges — data-property assertions lower to `∃dp.DKey` type markers, not named-individual edges). A consistency pre-check (reusing the ABox saturator) gates an `Inconsistent` error. Values are 5-tuples `(subject, property, lexical, datatype, lang)`.

**Tech Stack:** Rust (edition 2024), horned-owl model (`Component`, `Literal`, `Individual`, `DataProperty`), `owl-dl-reasoner`, `owl-dl-py`, `owl-dl-cli`.

**Spec:** `docs/superpowers/specs/2026-06-21-inferred-data-property-assertions-design.md`
**Branch:** `feat/inferred-data-property-assertions`

---

## Key facts (verified)

- `DataPropertyAssertion` (horned-owl): fields `dp` (`DataProperty`), `from` (`Individual`), `to` (`Literal`). `ax.dp.0.as_ref()` = property IRI. `convert.rs` uses exactly `ax.dp.0.as_ref()`, `&ax.to`, `ax.from`.
- `SubDataPropertyOf { sub, sup }` — `ax.sub.0.as_ref()` / `ax.sup.0.as_ref()` = data-property IRIs.
- `EquivalentDataProperties(ax)` — `ax.0: Vec<DataProperty>`, each `d.0.as_ref()` = IRI.
- `Literal<A>` (horned-owl `model.rs:1659`): `Simple { literal: String }`, `Language { literal: String, lang: String }`, `Datatype { literal: String, datatype_iri: IRI<A> }`.
- `Individual<A>` = `Named(NamedIndividual<A>)` | `Anonymous(AnonymousIndividual<A>)`; `Named`'s IRI is `ni.0.as_ref()` (see `justify.rs::ind_iri`).
- `owl_dl_reasoner` exports: `materialize_object_property_assertions` (sibling), `ReasonError::Inconsistent` (already added), `justify::{Entailment, entails}`, `abox_saturation::saturate_abox_consistency`, `owl_dl_core::convert::convert_ontology`. `Entailment::DataPropertyValue { source, prop, value_lexical, value_datatype }` (no lang field).
- The materialize function lives in `crates/owl-dl-reasoner/src/lib.rs` next to `materialize_object_property_assertions` (convert_ontology / abox_saturation in scope there).
- CLI `realize --properties` handler (added last feature) currently prints object property assertions via `materialize_object_property_assertions`. Extend it.
- Python `owl-dl-py/src/materialize.rs` holds the family; `errors.rs::reason_error_to_py` already handles `ReasonError::Inconsistent`.

## File structure

- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — `materialize_data_property_assertions`.
- **Create** `crates/owl-dl-reasoner/tests/data_property_assertions.rs` — tests.
- **Modify** `crates/owl-dl-py/src/materialize.rs` — Python binding.
- **Modify** `crates/owl-dl-cli/src/main.rs` — extend `realize --properties`.
- **Modify** `README.md`, `CLAUDE.md` — docs.

---

### Task 1: Reasoner function + tests

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; Create `crates/owl-dl-reasoner/tests/data_property_assertions.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/inferred-data-property-assertions
```

- [ ] **Step 2: Write the failing test** — create `crates/owl-dl-reasoner/tests/data_property_assertions.rs`:

```rust
//! Integration tests for materialize_data_property_assertions.

use horned_owl::model::{
    Build, DataProperty, DataPropertyAssertion, DeclareDataProperty, DeclareNamedIndividual,
    Individual, Literal, MutableOntology, SubDataPropertyOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_data_property_assertions;

type Rc = std::rc::Rc<str>;
const XSD_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";

fn dpa(b: &Build<Rc>, dp: &str, subj: &str, lexical: &str, dt: &str) -> DataPropertyAssertion<Rc> {
    DataPropertyAssertion {
        dp: b.data_property(dp),
        from: Individual::Named(b.named_individual(subj)),
        to: Literal::Datatype { literal: lexical.to_string(), datatype_iri: b.iri(dt) },
    }
}
fn subdp(b: &Build<Rc>, sub: &str, sup: &str) -> SubDataPropertyOf<Rc> {
    SubDataPropertyOf { sub: b.data_property(sub), sup: b.data_property(sup) }
}

// hasAge ⊑ hasMeasurement ; hasAge(a, "30"^^int)
//   → result contains (a, hasMeasurement, "30", xsd:integer, "") AND the asserted hasAge.
#[test]
fn subproperty_data_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:hasMeasurement")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(subdp(&b, "urn:hasAge", "urn:hasMeasurement"));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    let t = |s: &str, p: &str, l: &str, d: &str, lang: &str| {
        (s.to_string(), p.to_string(), l.to_string(), d.to_string(), lang.to_string())
    };
    assert!(got.contains(&t("urn:a", "urn:hasMeasurement", "30", XSD_INT, "")), "got: {got:?}");
    assert!(got.contains(&t("urn:a", "urn:hasAge", "30", XSD_INT, "")));
    // negative control: an un-entailed value is absent
    assert!(!got.contains(&t("urn:a", "urn:hasMeasurement", "99", XSD_INT, "")));
}
```

NOTE: horned-owl shapes (`DataPropertyAssertion { dp, from, to }`, `SubDataPropertyOf { sub, sup }`, `Individual::Named(..)`, `Literal::Datatype { literal, datatype_iri }`, `DeclareDataProperty`, `b.data_property`, `b.iri`) are verified against `convert.rs` / `model.rs`. If any differs, match `convert.rs` (it accesses `ax.dp.0`, `ax.sub.0`, `ax.to`, `ax.from`) and `justify.rs`. Report adjustments.

- [ ] **Step 3: Run to confirm FAIL** — `cargo test -p owl-dl-reasoner --test data_property_assertions` → FAIL (`materialize_data_property_assertions` undefined).

- [ ] **Step 4: Implement** — add to `crates/owl-dl-reasoner/src/lib.rs` (next to `materialize_object_property_assertions`):

```rust
/// Materialize the inferred DATA property assertions entailed over **named
/// individuals** — `(subject_iri, property_iri, lexical, datatype_iri, lang)`
/// 5-tuples (the full entailed closure under sub-data-property hierarchy and
/// equivalent-data-properties). Sound; complete for that fragment. Under-
/// approximate: omits `SameIndividual` folding and class-axiom-derived assertions
/// (e.g. `DataHasValue`). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] on lowering failure.
pub fn materialize_data_property_assertions<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String, String, String, String)>, ReasonError> {
    use horned_owl::model::{Component as C, Individual, Literal};
    use std::collections::BTreeSet;

    const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
    const LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";

    // Consistency pre-check (parity with the object-property version).
    let internal = convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    // Asserted data-property assertions over NAMED individuals + hierarchy edges.
    let mut asserted: Vec<(String, String, (String, String, String))> = Vec::new();
    let mut hierarchy: Vec<(String, String)> = Vec::new();
    for ac in onto {
        match &ac.component {
            C::DataPropertyAssertion(ax) => {
                let Individual::Named(ni) = &ax.from else {
                    continue; // named individuals only
                };
                let subj = ni.0.as_ref().to_string();
                let dp = ax.dp.0.as_ref().to_string();
                let value = match &ax.to {
                    Literal::Simple { literal } => {
                        (literal.clone(), XSD_STRING.to_string(), String::new())
                    }
                    Literal::Language { literal, lang } => {
                        (literal.clone(), LANG_STRING.to_string(), lang.clone())
                    }
                    Literal::Datatype { literal, datatype_iri } => {
                        (literal.clone(), datatype_iri.as_ref().to_string(), String::new())
                    }
                };
                asserted.push((subj, dp, value));
            }
            C::SubDataPropertyOf(ax) => {
                hierarchy.push((ax.sub.0.as_ref().to_string(), ax.sup.0.as_ref().to_string()));
            }
            C::EquivalentDataProperties(ax) => {
                let dps: Vec<String> = ax.0.iter().map(|d| d.0.as_ref().to_string()).collect();
                for (i, di) in dps.iter().enumerate() {
                    for (j, dj) in dps.iter().enumerate() {
                        if i != j {
                            hierarchy.push((di.clone(), dj.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Transitive closure: dp → { dp } ∪ super-data-properties.
    let closure = |dp: &str| -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        set.insert(dp.to_string());
        let mut stack = vec![dp.to_string()];
        while let Some(cur) = stack.pop() {
            for (s, sup) in &hierarchy {
                if s == &cur && set.insert(sup.clone()) {
                    stack.push(sup.clone());
                }
            }
        }
        set
    };

    let mut out: Vec<(String, String, String, String, String)> = Vec::new();
    for (subj, dp, (lex, dt, lang)) in &asserted {
        for sup in closure(dp) {
            out.push((subj.clone(), sup, lex.clone(), dt.clone(), lang.clone()));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
```
Confirm `ax.dp.0.as_ref()` / `ax.sub.0.as_ref()` / `datatype_iri.as_ref()` / `ni.0.as_ref()` compile (these mirror `convert.rs` / `justify.rs`); if `datatype_iri.as_ref()` doesn't yield `&str`, use `datatype_iri.to_string()` (it `Display`s as the IRI). Adjust and report.

- [ ] **Step 5: Run** — `cargo test -p owl-dl-reasoner --test data_property_assertions subproperty_data_assertions` → PASS. Paste the `test result:` line. If `hasMeasurement` is missing, the closure/scan has a bug — investigate/report.

- [ ] **Step 6: Add equivalent / language-tag / inconsistency / soundness tests** — append:

```rust
use horned_owl::model::EquivalentDataProperties;
use owl_dl_reasoner::justify::{Entailment, entails};

// hasAge ≡ age ; hasAge(a,"30"^^int) → result contains (a, age, ...) too.
#[test]
fn equivalent_data_properties() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:age")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(EquivalentDataProperties(vec![b.data_property("urn:hasAge"), b.data_property("urn:age")]));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    assert!(
        got.iter().any(|(s, p, l, _, _)| s == "urn:a" && p == "urn:age" && l == "30"),
        "got: {got:?}"
    );
}

// Language-tagged literal round-trips its lang + langString datatype.
#[test]
fn language_tag_round_trips() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:label")));
    o.insert(DeclareDataProperty(b.data_property("urn:name")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(subdp(&b, "urn:label", "urn:name"));
    o.insert(DataPropertyAssertion {
        dp: b.data_property("urn:label"),
        from: Individual::Named(b.named_individual("urn:a")),
        to: Literal::Language { literal: "hi".to_string(), lang: "en".to_string() },
    });

    let got = materialize_data_property_assertions(&o).expect("materialize");
    assert!(
        got.iter().any(|(s, p, l, d, lang)| s == "urn:a"
            && p == "urn:name"
            && l == "hi"
            && d == "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString"
            && lang == "en"),
        "got: {got:?}"
    );
}

// Every emitted NON-language triple is genuinely entailed.
#[test]
fn every_data_triple_is_entailed() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:hasMeasurement")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(subdp(&b, "urn:hasAge", "urn:hasMeasurement"));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    assert!(!got.is_empty());
    for (s, p, lex, dt, lang) in &got {
        if !lang.is_empty() {
            continue; // DataPropertyValue has no lang field; language triples sound by construction
        }
        let q = Entailment::DataPropertyValue {
            source: s.clone(),
            prop: p.clone(),
            value_lexical: lex.clone(),
            value_datatype: dt.clone(),
        };
        assert!(entails(&o, &q).expect("entails"), "{s} {p} {lex} must be entailed");
    }
}

// Inconsistent ontology → Err.
#[test]
fn inconsistent_is_error() {
    use horned_owl::model::{ClassAssertion, ClassExpression as CE, DeclareClass, DisjointClasses};
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareNamedIndividual(b.named_individual("urn:i")));
    o.insert(DisjointClasses(vec![CE::Class(b.class("urn:A")), CE::Class(b.class("urn:B"))]));
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:A")), i: Individual::Named(b.named_individual("urn:i")) });
    o.insert(ClassAssertion { ce: CE::Class(b.class("urn:B")), i: Individual::Named(b.named_individual("urn:i")) });

    assert!(materialize_data_property_assertions(&o).is_err());
}
```
Match any differing shapes against `justify.rs` / `diagnose.rs` (`ClassAssertion { ce, i }`, `DisjointClasses(vec![..])`). Report adjustments.

- [ ] **Step 7: Run all + clippy + fmt**
```bash
cargo test -p owl-dl-reasoner --test data_property_assertions
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
cargo fmt -p owl-dl-reasoner
```
4 passed; clippy/fmt green; re-run after fmt.

- [ ] **Step 8: Commit**
```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/data_property_assertions.rs
git commit -m "feat(reasoner): materialize_data_property_assertions (structural closure)"
```

---

### Task 2: Python binding

**Files:** Modify `crates/owl-dl-py/src/materialize.rs`

- [ ] **Step 1: Add the binding** — after `materialize_inferred_property_assertions`:

```rust
/// Returns every inferred data property assertion as a 5-tuple
/// `(subject, property, lexical, datatype, lang)` entailed over named individuals
/// (asserted + derived via sub-data-property hierarchy / equivalent-data-properties).
/// `lang` is "" except for language-tagged literals. Sound under-approximation
/// (no SameIndividual / class-axiom-derived assertions). Raises if inconsistent.
#[pyfunction]
pub(crate) fn materialize_inferred_data_property_assertions(
    path: &str,
) -> PyResult<Vec<(String, String, String, String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::materialize_data_property_assertions(&ontology).map_err(reason_error_to_py)
}
```
And register it in `register`:
```rust
    m.add_function(wrap_pyfunction!(materialize_inferred_data_property_assertions, m)?)?;
```

- [ ] **Step 2: Build + clippy + fmt**
```bash
cargo build -p owl-dl-py
cargo clippy -p owl-dl-py --all-targets -- -D warnings
cargo fmt -p owl-dl-py
```
Green. (`reason_error_to_py` already handles `ReasonError::Inconsistent`.)

- [ ] **Step 3: Commit**
```bash
git add crates/owl-dl-py/src/materialize.rs
git commit -m "feat(py): materialize_inferred_data_property_assertions binding"
```

---

### Task 3: Extend CLI `realize --properties`

**Files:** Modify `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add the data section** — in the `Command::Realize { … }` handler, find the `if properties { … }` block (added in the previous feature; it prints the object property section). AFTER the object-property `match` (still inside `if properties { … }`), add the data-property section:

```rust
                match owl_dl_reasoner::materialize_data_property_assertions(&onto) {
                    Ok(triples) => {
                        println!("# inferred data property assertions");
                        for (s, p, lex, dt, lang) in triples {
                            println!("{s}\t{p}\t{lex}\t{dt}\t{lang}");
                        }
                    }
                    Err(e) => {
                        eprintln!("# data property assertions unavailable: {e}");
                    }
                }
```
Read the existing `if properties` block and place this after the object-property output so a single `--properties` flag emits both the object section then the data section. Default off ⇒ realize output unchanged.

- [ ] **Step 2: Build + smoke-test**
```bash
cat > /tmp/dprop-smoke.ofn <<'EOF'
Prefix(:=<urn:>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
  Declaration(NamedIndividual(:a))
  Declaration(DataProperty(:hasAge)) Declaration(DataProperty(:hasMeasurement))
  SubDataPropertyOf(:hasAge :hasMeasurement)
  DataPropertyAssertion(:hasAge :a "30"^^xsd:integer)
)
EOF
cargo build -p owl-dl-cli --release
echo "--- WITHOUT --properties: hasMeasurement count must be 0 ---"
./target/release/rustdl realize /tmp/dprop-smoke.ofn | grep -c 'hasMeasurement' || true
echo "--- WITH --properties ---"
./target/release/rustdl realize /tmp/dprop-smoke.ofn --properties
```
Expected: without the flag, `hasMeasurement` count 0 (unchanged). With `--properties`, output includes `# inferred data property assertions` and a line `urn:a	urn:hasMeasurement	30	http://www.w3.org/2001/XMLSchema#integer	` (plus the asserted `hasAge`). Paste the actual `--properties` output. If `hasMeasurement` is absent with the flag, STOP and report.

- [ ] **Step 3: clippy + fmt**
```bash
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```

- [ ] **Step 4: Commit**
```bash
git add crates/owl-dl-cli/src/main.rs
git commit -m "feat(cli): realize --properties also emits inferred data property assertions"
```

---

### Task 4: Docs + final gate

**Files:** Modify `README.md`, `CLAUDE.md`

- [ ] **Step 1: README** — update the `realize --properties` comment to mention data too, e.g.:
```
rustdl realize   ontology.ofn [--properties] # per-individual types (+ inferred object & data property assertions)
```
If the README Python block lists materialize functions, add `materialize_inferred_data_property_assertions` alongside the object one.

- [ ] **Step 2: CLAUDE.md** — append to the relevant bullet:
```
`materialize_data_property_assertions` (reasoner) / `materialize_inferred_data_property_assertions`
(Python) surface inferred DATA property assertions (5-tuple subject/property/lexical/datatype/lang)
via a structural sub-data-property + equivalent-data-property closure (sound; under-approx: no
SameIndividual / class-axiom-derived). Folded into `realize --properties`. See
`docs/superpowers/specs/2026-06-21-inferred-data-property-assertions-design.md`.
```

- [ ] **Step 3: Full workspace gate**
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. After `cargo fmt --all`, run `git status --short` and stage EVERY file fmt touched (a prior feature left an fmt change uncommitted → CI red). The new `data_property_assertions` tests (4) must pass; no regression elsewhere. Report the aggregate.

- [ ] **Step 4: Commit**
```bash
cd /data/dumontier/rustdl
git add -A
git status --short
git commit -m "docs(data-property-assertions): document materialize / realize --properties"
```

---

## Self-review notes (author)

- **Spec coverage:** structural scan + hierarchy/equiv closure + consistency-precheck error → Task 1; 5-tuple value (all 3 Literal variants, lang round-trip) → Task 1 Steps 4/6; soundness re-check via `entails(DataPropertyValue)` → Task 1 Step 6; Python → Task 2; CLI `realize --properties` data section (default-off unchanged) → Task 3; docs + byte-identical gate → Task 4.
- **Soundness:** every triple from sub-property/equiv entailment; re-verified for non-language triples. Read-only; classification untouched (the consistency pre-check is the existing saturator, used read-only). Inconsistency → `Err` (reuses `ReasonError::Inconsistent`).
- **No placeholders:** code complete; "match against repo" notes only for horned-owl axiom-constructor shapes and the `datatype_iri.as_ref()`-vs-`to_string()` detail.
- **Type consistency:** `materialize_data_property_assertions(&SetOntology<A>) -> Result<Vec<(String,String,String,String,String)>, ReasonError>`; 5-tuple order (subject, property, lexical, datatype, lang) consistent across reasoner/Python/CLI.
- **API risk flagged inline:** `Literal` variant names, `DataPropertyAssertion`/`SubDataPropertyOf`/`EquivalentDataProperties` field access, `Individual::Named` IRI access, `datatype_iri` string conversion — each points at `convert.rs` / `justify.rs`.
