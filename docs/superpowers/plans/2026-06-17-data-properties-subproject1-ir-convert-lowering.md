# First-Class Data Properties — Sub-project 1: IR + Convert Lowering — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Behind a default-OFF `RUSTDL_DATA_PROPERTIES` env gate, lower the
data-property axioms that `convert.rs` currently drops into existing
object-fragment `Axiom`s (data property = object role, literal = `DKey(point v)`
filler), and prove the approach end-to-end with a consistency POC.

**Architecture:** Reuse rustdl's existing "data property as forward object role to
a DKey value-class" encoding (`convert.rs:582-640`, `812-865`) and extend it from
the TBox to the ABox/RBox. No new `Axiom` variants. Gate default-OFF ⇒ converter
byte-identical to today; flipped ON only after later sub-projects validate FP=0.

**Tech Stack:** Rust (edition 2024), `horned-owl` model types, `owl-dl-core` IR
(`ConceptPool`, `Axiom`, `Vocabulary`), `cargo test`.

**Spec:** `docs/superpowers/specs/2026-06-17-first-class-data-properties-design.md`

---

## File structure

- `crates/owl-dl-core/src/convert.rs` — the only production file changed:
  - new `data_properties_enabled()` gate helper,
  - new `data_point_some()` helper (factored from the `DataHasValue` arm),
  - the grouped `Ok(None)` data-axiom arm (`1647-1655`) split into per-variant
    gated arms.
- `crates/owl-dl-core/src/convert.rs` `#[cfg(test)]` module — unit tests for each
  lowering arm (gate ON and OFF) + a small serialized env guard.
- `crates/owl-dl-reasoner/tests/data_properties.rs` — NEW: the end-to-end POC
  (consistency verdict) + gate-OFF corpus byte-identity guard.

## Reference shapes (verified against current code)

- Gate read each call (so tests can toggle): `std::env::var_os(..).is_some()`.
- IR axioms: `Axiom::ClassAssertion { class: ConceptId, individual: IndividualId }`,
  `Axiom::SubObjectPropertyOf { sub: SubRolePath, sup: Role }`,
  `Axiom::EquivalentObjectProperties(Vec<Role>)`,
  `Axiom::DisjointObjectProperties(Vec<Role>)`, `Axiom::FunctionalRole(Role)`,
  `Axiom::ObjectPropertyDomain { role: Role, domain: ConceptId }`,
  `Axiom::ObjectPropertyRange { role: Role, range: ConceptId }`
  (`crates/owl-dl-core/src/ontology.rs:35-94`).
- `SubRolePath::Role(Role)` (`ontology.rs:21`).
- A data property → role: `Role::named(vocab.intern_role(dp_iri))` (as at
  `convert.rs:625`).
- A literal → `∃dp.DKey(point v)` concept: the body of the `DataHasValue` arm
  (`convert.rs:812-865`) — uses `integer_literal_value`/`float_literal_value`/
  `decimal_literal_value`/`date_literal_value`/`datetime_literal_value`/
  `exact_string_literal` + `lower_*_data_to_some`. We factor this into
  `data_point_some`.
- Complement: `pool.not(concept_id)` (`convert.rs:735`, `ir.rs:334`).
- Individual → id: `convert_individual(&ind, vocab)` (`convert.rs:1613`).
- Data range → `(Role, DKey-filler)`: `data_range_dkey(&dr, dp_iri, vocab, pool)`
  returns `Option<(Role, ConceptId)>` (`convert.rs:589`).
- horned-owl field names: `DataPropertyAssertion { dp, from, to }` (`to: Literal`),
  `NegativeDataPropertyAssertion { dp, from, to }`, `SubDataPropertyOf { sup, sub }`
  (both `DataProperty`), `EquivalentDataProperties(Vec<DataProperty>)`,
  `DisjointDataProperties(Vec<DataProperty>)`, `FunctionalDataProperty(DataProperty)`,
  `DataPropertyDomain { dp, ce }`, `DataPropertyRange { dp, dr }`.

---

### Task 1: Factor the literal → `∃dp.DKey(point v)` helper (behavior-preserving)

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (the `DataHasValue` arm, ~812-865)

- [ ] **Step 1: Add the `data_point_some` helper** near `data_range_dkey`
  (after `lower_data_to_some`, ~`convert.rs:660`). It returns the
  `∃dp.DKey(point v)` concept, or `None` for an unrecognized literal.

```rust
/// Lower a single literal `l` to the concept `∃dp.DKey(point l)`, reusing the
/// per-datatype point-range DKey encoding. `None` for any literal whose datatype
/// the DKey machinery does not recognize (caller drops — sound under-approximation).
fn data_point_some<A: ForIRI>(
    dp_iri: &str,
    l: &Literal<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Option<ConceptId> {
    if let Some(v) = integer_literal_value(l) {
        Some(lower_data_to_some(IntegerRange::point(v), dp_iri, vocab, pool))
    } else if let Some(v) = float_literal_value(l) {
        Some(lower_float_data_to_some(FloatRange::point(v), dp_iri, vocab, pool))
    } else if let Some(v) = decimal_literal_value(l) {
        Some(lower_ord_data_to_some(&OrdRange::point(v), DKEY_DECIMAL_TAG, decimal_key, dp_iri, vocab, pool))
    } else if let Some(v) = date_literal_value(l) {
        Some(lower_ord_data_to_some(&OrdRange::point(v), DKEY_DATE_TAG, date_key, dp_iri, vocab, pool))
    } else if let Some(v) = datetime_literal_value(l) {
        Some(lower_ord_data_to_some(&OrdRange::point(v), DKEY_DATETIME_TAG, datetime_key, dp_iri, vocab, pool))
    } else if let Some(s) = exact_string_literal(l) {
        Some(lower_str_data_to_some(&StrSet::singleton(s), dp_iri, vocab, pool))
    } else {
        None
    }
}
```

- [ ] **Step 2: Rewrite the `DataHasValue` arm to use the helper.** Replace the
  whole `ClassExpression::DataHasValue { dp, l } => { ... }` body (`convert.rs:812-865`)
  with:

```rust
        ClassExpression::DataHasValue { dp, l } => {
            data_point_some(dp.0.as_ref(), l, vocab, pool)
                .ok_or(ConversionError::UnsupportedDataRange)
        }
```

- [ ] **Step 3: Run the existing datatype tests to confirm behavior is unchanged.**

Run: `cargo test -p owl-dl-core 2>&1 | tail -5`
Expected: PASS (no regressions; the refactor is behavior-preserving).

- [ ] **Step 4: Run the value-membership integration canaries** (they exercise
  `DataHasValue`).

Run: `cargo test -p owl-dl-reasoner --test datatype_value_membership 2>&1 | tail -5`
Expected: PASS (same counts as before).

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "refactor(convert): factor data_point_some helper from DataHasValue arm"
```

---

### Task 2: Env gate + `DataPropertyAssertion` lowering

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (gate helper; split the grouped drop
  arm `1647-1655`; add a `#[cfg(test)]` env guard + tests)

- [ ] **Step 1: Add the gate helper** near the top of `convert.rs` (after the
  `DKEY_*` consts, ~line 160):

```rust
/// First-class data-property lowering is opt-in (default OFF). When OFF, the
/// data-property axiom arms drop to `Ok(None)` exactly as before this arc — so
/// the converter is byte-identical to legacy behavior. Flipped ON only after the
/// later sub-projects validate FP=0 corpus-wide. Read per call so tests can toggle.
fn data_properties_enabled() -> bool {
    std::env::var_os("RUSTDL_DATA_PROPERTIES").is_some()
}
```

- [ ] **Step 2: Split the grouped drop arm.** Replace the single arm at
  `convert.rs:1647-1655`:

```rust
        #[allow(clippy::match_same_arms)]
        C::SubDataPropertyOf(_)
        | C::EquivalentDataProperties(_)
        | C::DisjointDataProperties(_)
        | C::DataPropertyDomain(_)
        | C::DataPropertyRange(_)
        | C::FunctionalDataProperty(_)
        | C::DatatypeDefinition(_)
        | C::DataPropertyAssertion(_)
        | C::NegativeDataPropertyAssertion(_) => Ok(None),
```

with a `DataPropertyAssertion` arm plus a still-grouped remainder (later tasks pull
their variants out of the group):

```rust
        C::DataPropertyAssertion(ax) if data_properties_enabled() => {
            match data_point_some(ax.dp.0.as_ref(), &ax.to, vocab, pool) {
                Some(class) => {
                    let individual = convert_individual(&ax.from, vocab)?;
                    Ok(Some(Axiom::ClassAssertion { class, individual }))
                }
                None => Ok(None), // unrecognized literal datatype — drop (sound)
            }
        }

        #[allow(clippy::match_same_arms)]
        C::SubDataPropertyOf(_)
        | C::EquivalentDataProperties(_)
        | C::DisjointDataProperties(_)
        | C::DataPropertyDomain(_)
        | C::DataPropertyRange(_)
        | C::FunctionalDataProperty(_)
        | C::DatatypeDefinition(_)
        | C::DataPropertyAssertion(_)
        | C::NegativeDataPropertyAssertion(_) => Ok(None),
```

(The guarded `DataPropertyAssertion` arm precedes the catch-all, so gate-OFF falls
through to `Ok(None)`.)

- [ ] **Step 3: Add a serialized env guard + tests** in the `convert.rs`
  `#[cfg(test)]` module. The module already provides `b() -> Build<RcStr>`,
  `named_ind(name)`, and `convert_one(c) -> (InternalOntology, Option<Axiom>)`
  (which calls `convert_component`, so it respects the gate); reuse them. Add this
  guard + helper once near the top of the module (after the existing helpers):

```rust
    static DP_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct DpGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl DpGuard {
        #[allow(unsafe_code)]
        fn on() -> Self {
            let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
            // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
            unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "1") };
            Self { prior }
        }
    }
    impl Drop for DpGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: see DpGuard::on.
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var("RUSTDL_DATA_PROPERTIES", v),
                    None => std::env::remove_var("RUSTDL_DATA_PROPERTIES"),
                }
            }
        }
    }

    fn int_dp_assertion(dp: &str, ind: &str, lexical: &str, dt: &str) -> Component<RcStr> {
        Component::DataPropertyAssertion(ho::DataPropertyAssertion {
            dp: b().data_property(dp),
            from: named_ind(ind),
            to: ho::Literal::Datatype {
                literal: lexical.to_string(),
                datatype_iri: b().iri(dt),
            },
        })
    }
```

(`ho` aliases `horned_owl::model` — the module's `use super::*;` re-exports
`convert.rs`'s imports; if `ho`/`Literal` is not in scope add
`use horned_owl::model::{self as ho, Literal};` to the test module. Verify
`Build::data_property` / `Build::datatype` / `Build::iri` method names against the
`horned_owl::model::Build` API and adjust if they differ.)

```rust
    const XSD_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";

    #[test]
    fn data_property_assertion_lowers_when_gate_on() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = int_dp_assertion("http://t/dp", "http://t/a", "5", XSD_INT);
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::ClassAssertion { .. })),
                "gate ON: data assertion lowers to ClassAssertion; got {ax:?}");
    }

    #[test]
    fn data_property_assertion_dropped_when_gate_off() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // gate not set ⇒ OFF
        let c = int_dp_assertion("http://t/dp", "http://t/a", "5", XSD_INT);
        let (_, ax) = convert_one(&c);
        assert!(ax.is_none(), "gate OFF: data assertion dropped; got {ax:?}");
    }

    #[test]
    fn data_property_assertion_unrecognized_literal_dropped() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        // anyURI is not a DKey-recognized datatype ⇒ drop even with gate ON
        let c = int_dp_assertion("http://t/dp", "http://t/a", "x",
                                 "http://www.w3.org/2001/XMLSchema#anyURI");
        let (_, ax) = convert_one(&c);
        assert!(ax.is_none(), "unrecognized datatype dropped; got {ax:?}");
    }
```

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p owl-dl-core data_property_assertion 2>&1 | tail -8`
Expected: 3 PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): gated DataPropertyAssertion lowering (RUSTDL_DATA_PROPERTIES, default off)"
```

---

### Task 3: `NegativeDataPropertyAssertion` lowering

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (pull the variant out of the group)

- [ ] **Step 1: Add the guarded arm** immediately after the `DataPropertyAssertion`
  arm; remove `C::NegativeDataPropertyAssertion(_)` from the grouped catch-all:

```rust
        C::NegativeDataPropertyAssertion(ax) if data_properties_enabled() => {
            match data_point_some(ax.dp.0.as_ref(), &ax.to, vocab, pool) {
                Some(some_concept) => {
                    let class = pool.not(some_concept); // ¬∃dp.DKey(point v)
                    let individual = convert_individual(&ax.from, vocab)?;
                    Ok(Some(Axiom::ClassAssertion { class, individual }))
                }
                None => Ok(None),
            }
        }
```

- [ ] **Step 2: Add tests** in the `convert.rs` test module:

```rust
    #[test]
    fn negative_data_property_assertion_lowers_to_complement() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::NegativeDataPropertyAssertion(ho::NegativeDataPropertyAssertion {
            dp: b().data_property("http://t/dp"),
            from: named_ind("http://t/a"),
            to: ho::Literal::Datatype {
                literal: "5".into(),
                datatype_iri: b().iri(XSD_INT),
            },
        });
        let (o, ax) = convert_one(&c);
        let Some(Axiom::ClassAssertion { class, .. }) = ax else {
            panic!("expected ClassAssertion, got {ax:?}");
        };
        // The asserted class is ¬∃dp.DKey — i.e. a Not concept (pool.not(...)).
        assert!(matches!(o.concepts.get(class), ConceptExpr::Not(_)),
                "expected a Not concept");
    }
```

(`ConceptExpr` is already imported in the test module; `o.concepts.get(id)` is the
same accessor the existing `complement` test uses.)

- [ ] **Step 3: Run.**

Run: `cargo test -p owl-dl-core negative_data_property 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 4: Commit.**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): gated NegativeDataPropertyAssertion lowering (¬∃dp.DKey)"
```

---

### Task 4: `SubDataPropertyOf` + `EquivalentDataProperties` lowering

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs`

- [ ] **Step 1: Add the guarded arms**; remove both variants from the grouped
  catch-all:

```rust
        C::SubDataPropertyOf(ax) if data_properties_enabled() => {
            let sub = SubRolePath::Role(Role::named(vocab.intern_role(ax.sub.0.as_ref())));
            let sup = Role::named(vocab.intern_role(ax.sup.0.as_ref()));
            Ok(Some(Axiom::SubObjectPropertyOf { sub, sup }))
        }
        C::EquivalentDataProperties(ax) if data_properties_enabled() => {
            let roles = ax.0.iter()
                .map(|dp| Role::named(vocab.intern_role(dp.0.as_ref())))
                .collect();
            Ok(Some(Axiom::EquivalentObjectProperties(roles)))
        }
```

(Ensure `SubRolePath` and `Role` are imported in `convert.rs`; `Role` already is,
add `use crate::ontology::SubRolePath;` if not present.)

- [ ] **Step 2: Add tests:**

```rust
    #[test]
    fn sub_data_property_lowers_to_role_hierarchy() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::SubDataPropertyOf(ho::SubDataPropertyOf {
            sub: b().data_property("http://t/dp"),
            sup: b().data_property("http://t/dq"),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::SubObjectPropertyOf { .. })), "got {ax:?}");
    }

    #[test]
    fn equivalent_data_properties_lowers_to_equivalent_roles() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::EquivalentDataProperties(ho::EquivalentDataProperties(vec![
            b().data_property("http://t/dp"),
            b().data_property("http://t/dq"),
        ]));
        let (_, ax) = convert_one(&c);
        let Some(Axiom::EquivalentObjectProperties(roles)) = ax else {
            panic!("got {ax:?}");
        };
        assert_eq!(roles.len(), 2);
    }
```

- [ ] **Step 3: Run.**

Run: `cargo test -p owl-dl-core sub_data_property equivalent_data_properties 2>&1 | tail -6`
Expected: PASS.

- [ ] **Step 4: Commit.**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): gated SubDataPropertyOf + EquivalentDataProperties lowering"
```

---

### Task 5: `DisjointDataProperties` + `FunctionalDataProperty` lowering

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs`

- [ ] **Step 1: Add the guarded arms**; remove both variants from the catch-all:

```rust
        C::DisjointDataProperties(ax) if data_properties_enabled() => {
            let roles = ax.0.iter()
                .map(|dp| Role::named(vocab.intern_role(dp.0.as_ref())))
                .collect();
            Ok(Some(Axiom::DisjointObjectProperties(roles)))
        }
        C::FunctionalDataProperty(ax) if data_properties_enabled() => {
            let role = Role::named(vocab.intern_role(ax.0.0.as_ref()));
            Ok(Some(Axiom::FunctionalRole(role)))
        }
```

- [ ] **Step 2: Add tests:**

```rust
    #[test]
    fn disjoint_data_properties_lowers_to_disjoint_roles() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::DisjointDataProperties(ho::DisjointDataProperties(vec![
            b().data_property("http://t/dp"),
            b().data_property("http://t/dq"),
        ]));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::DisjointObjectProperties(_))), "got {ax:?}");
    }

    #[test]
    fn functional_data_property_lowers_to_functional_role() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::FunctionalDataProperty(ho::FunctionalDataProperty(
            b().data_property("http://t/dp"),
        ));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::FunctionalRole(_))), "got {ax:?}");
    }
```

- [ ] **Step 3: Run.**

Run: `cargo test -p owl-dl-core disjoint_data_properties functional_data_property 2>&1 | tail -6`
Expected: PASS.

- [ ] **Step 4: Commit.**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): gated DisjointDataProperties + FunctionalDataProperty lowering"
```

---

### Task 6: `DataPropertyDomain` + `DataPropertyRange` lowering

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (the grouped catch-all should now hold
  only `DatatypeDefinition` — which stays dropped)

- [ ] **Step 1: Add the guarded arms**; remove both variants from the catch-all,
  leaving only `C::DatatypeDefinition(_) => Ok(None)`:

```rust
        C::DataPropertyDomain(ax) if data_properties_enabled() => {
            let role = Role::named(vocab.intern_role(ax.dp.0.as_ref()));
            let domain = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            Ok(Some(Axiom::ObjectPropertyDomain { role, domain }))
        }
        C::DataPropertyRange(ax) if data_properties_enabled() => {
            match data_range_dkey(&ax.dr, ax.dp.0.as_ref(), vocab, pool) {
                Some((role, range)) => Ok(Some(Axiom::ObjectPropertyRange { role, range })),
                None => Ok(None), // unrecognized range — drop (sound)
            }
        }
```

(`ce_or_skip!` returns `Ok(None)` from the enclosing fn if the domain class
expression is unsupported — same macro the object arms use.)

- [ ] **Step 2: Confirm the catch-all is now just `DatatypeDefinition`:**

```rust
        C::DatatypeDefinition(_) => Ok(None),
```

- [ ] **Step 3: Add tests:**

```rust
    #[test]
    fn data_property_domain_lowers_to_object_domain() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::DataPropertyDomain(ho::DataPropertyDomain {
            dp: b().data_property("http://t/dp"),
            ce: ClassExpression::Class(b().class("http://t/C")),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::ObjectPropertyDomain { .. })), "got {ax:?}");
    }

    #[test]
    fn data_property_range_integer_lowers_to_object_range() {
        let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _g = DpGuard::on();
        let c = Component::DataPropertyRange(ho::DataPropertyRange {
            dp: b().data_property("http://t/dp"),
            dr: DataRange::Datatype(b().datatype("http://www.w3.org/2001/XMLSchema#integer")),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::ObjectPropertyRange { .. })), "got {ax:?}");
    }
```

(Confirm `DataRange` and `ClassExpression` are in the test module scope via
`use super::*;`; add `use horned_owl::model::{ClassExpression, DataRange};` if not.)

- [ ] **Step 4: Run all the convert data tests + full crate.**

Run: `cargo test -p owl-dl-core 2>&1 | tail -6`
Expected: PASS (all new arms + no regression).

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-core/src/convert.rs
git commit -m "feat(convert): gated DataPropertyDomain + DataPropertyRange lowering"
```

---

### Task 7: End-to-end POC + gate-OFF byte-identity guard (the B-vs-A gate)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/data_properties.rs`

- [ ] **Step 1: Write the POC test.** Gate ON, `dp⊑dq` + `dp(a,5)` +
  `¬dq(a,5)` must be **inconsistent**; the `¬dq(a,6)` variant must be
  **consistent**. This proves role-hierarchy propagation to the DKey value-node +
  `∀`/complement end-to-end.

```rust
//! Sub-project 1 POC: first-class data-property lowering, end-to-end.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static DP_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct DpGuard {
    prior: Option<std::ffi::OsString>,
}
impl DpGuard {
    #[allow(unsafe_code)]
    fn on() -> Self {
        let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
        // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
        unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "1") };
        Self { prior }
    }
}
impl Drop for DpGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see DpGuard::on.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var("RUSTDL_DATA_PROPERTIES", v),
                None => std::env::remove_var("RUSTDL_DATA_PROPERTIES"),
            }
        }
    }
}

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://t/o>\n{body}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    o
}

#[test]
fn poc_sub_data_property_forces_inconsistency() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _g = DpGuard::on();
    // dp ⊑ dq, a dp 5, ¬(a dq 5)  ⇒  inconsistent (dp⊑dq forces a dq 5).
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(DataProperty(:dq))\n\
         Declaration(NamedIndividual(:a))\n\
         SubDataPropertyOf(:dp :dq)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)\n\
         NegativeDataPropertyAssertion(:dq :a \"5\"^^xsd:integer)",
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&o).unwrap(),
        "dp⊑dq + dp(a,5) + ¬dq(a,5) must be inconsistent"
    );
}

#[test]
fn poc_sub_data_property_consistent_when_values_differ() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _g = DpGuard::on();
    // ¬(a dq 6) does not contradict the forced a dq 5  ⇒  consistent.
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(DataProperty(:dq))\n\
         Declaration(NamedIndividual(:a))\n\
         SubDataPropertyOf(:dp :dq)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)\n\
         NegativeDataPropertyAssertion(:dq :a \"6\"^^xsd:integer)",
    );
    assert!(
        owl_dl_reasoner::is_consistent(&o).unwrap(),
        "distinct value must stay consistent"
    );
}

#[test]
fn poc_functional_data_property_two_distinct_values_inconsistent() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _g = DpGuard::on();
    // Functional(dp), a dp 5, a dp 6  ⇒  inconsistent (5 ≠ 6 can't merge).
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(NamedIndividual(:a))\n\
         FunctionalDataProperty(:dp)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)\n\
         DataPropertyAssertion(:dp :a \"6\"^^xsd:integer)",
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&o).unwrap(),
        "functional dp with two distinct values must be inconsistent"
    );
}
```

- [ ] **Step 2: Run the POC.**

Run: `cargo test -p owl-dl-reasoner --test data_properties 2>&1 | tail -10`
Expected: 3 PASS. **If any fail, STOP** — the B architecture has a gap; report the
verdict (which case, expected vs actual) before proceeding. This is the explicit
B-vs-A decision gate.

- [ ] **Step 3: Add the gate-OFF byte-identity guard.** With the gate OFF, a
  data-bearing fixture classifies identically to `main`. Append to the same file:

```rust
#[test]
fn gate_off_classification_unchanged_on_data_fixture() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // gate not set ⇒ OFF ⇒ converter behaves as legacy.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ontologies/real/shoiq-knowledge.ofn");
    if !path.exists() {
        eprintln!("SKIP: corpus fixture absent");
        return;
    }
    let file = std::fs::File::open(&path).unwrap();
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut std::io::BufReader::new(file), ParserConfiguration::default()).unwrap();
    // Must not panic and must classify (the assertion is "runs clean gate-OFF").
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(!c.classes().is_empty(), "gate-OFF classify produces a hierarchy");
}
```

(This is a smoke guard; the rigorous gate-OFF closure-diff vs `main` is run
manually with `scripts/closure-diff.sh` and recorded in the task's commit message.)

- [ ] **Step 4: Run the full file + confirm gate-OFF corpus is unchanged.**

Run:
```bash
cargo test -p owl-dl-reasoner --test data_properties 2>&1 | tail -6
# Manual gate-OFF closure check (must be byte-identical to main):
./target/release/rustdl classify ontologies/real/shoiq-knowledge.ofn > /tmp/dp-off.txt 2>/dev/null
git stash && cargo build --release -p owl-dl-cli 2>&1 | tail -1
./target/release/rustdl classify ontologies/real/shoiq-knowledge.ofn > /tmp/dp-main.txt 2>/dev/null
git stash pop && diff /tmp/dp-off.txt /tmp/dp-main.txt && echo "GATE-OFF BYTE-IDENTICAL"
```
Expected: 4 PASS; `GATE-OFF BYTE-IDENTICAL`.

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-reasoner/tests/data_properties.rs
git commit -m "test(data): sub-project 1 POC — first-class dp lowering forces expected consistency verdicts; gate-OFF byte-identical"
```

---

### Task 8: Lint, format, and sub-project wrap-up

**Files:** none (verification only)

- [ ] **Step 1: Format.**

Run: `cargo fmt --all`

- [ ] **Step 2: Clippy (warnings are errors).**

Run: `cargo clippy -p owl-dl-core -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5`
Expected: clean (no `error`/`warning`).

- [ ] **Step 3: Full workspace test (no regressions).**

Run: `cargo test -p owl-dl-core -p owl-dl-reasoner 2>&1 | tail -8`
Expected: all PASS.

- [ ] **Step 4: Commit any fmt/clippy fixes.**

```bash
git add -A
git commit -m "chore(data): fmt + clippy for sub-project 1"
```

- [ ] **Step 5: Do NOT push.** Report completion and the POC verdict to the user;
  pushing waits for explicit approval (project rule). Sub-project 2 (tableau/solver
  validation) is the next plan.

---

## Notes for the implementer

- **Soundness rule, everywhere:** an unrecognized literal/datatype/range ⇒ drop the
  axiom (`Ok(None)`), never guess. Dropping loses entailments (sound); guessing
  risks FP.
- **The gate is sacred for incrementality:** every arm must fall through to
  `Ok(None)` when `data_properties_enabled()` is false, so gate-OFF is byte-identical
  to `main`. The Task 7 byte-identity check enforces this.
- **`ho::` alias / `Build` helpers:** match the existing `convert.rs` test module
  conventions; if a `b.data_property(..)` / `b.datatype(..)` helper name differs,
  use whatever the `horned_owl::model::Build` API actually exposes (check
  `Build`’s methods).
- **If the POC (Task 7 Step 2) fails:** that is signal, not a bug to paper over —
  it means role-hierarchy/`∀` propagation to DKey value-nodes needs work in the
  tableau (sub-project 2) or that approach A is needed. Report and stop.
