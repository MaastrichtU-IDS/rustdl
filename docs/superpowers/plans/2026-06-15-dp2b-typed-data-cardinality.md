# DP-2b: Typed/faceted from-type data-cardinality inconsistency — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect ABox inconsistency when a told-typed individual carries more than `n` distinct data-property values provably in `dr`, for a `C ⊑ ≤n dp.dr` (`DataMax`/`DataExact`) constraint over numeric/temporal/string ranges — emit `Top ⊑ Bot`.

**Architecture:** One new preprocessing function `emit_data_cardinality_violations_typed` in `crates/owl-dl-core/src/data_axioms.rs`, called from `derive_data_axioms`. It mirrors the existing functional-≤1 check (`emit_functional_dp_cardinality_violations`) but adds class-membership typing, `n>1`, and facet-aware range membership. Membership reuses each range type's existing `point()/singleton()` + soundness-reviewed `subset()` (no new boundary algebra). The two existing cardinality functions are left untouched (Approach A).

**Tech Stack:** Rust (edition 2024), horned-owl model, the D5–D11 range types (`IntegerRange`, `FloatRange`, `OrdRange<T>`, `StrSet`) and `DistinctVal` / `literal_to_distinct_val`, all already in `data_axioms.rs`.

**Spec:** `docs/superpowers/specs/2026-06-15-dp2b-typed-data-cardinality-design.md`

**Build/test prelude (the environment has no `cargo` on PATH by default):**
```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

**Soundness contract (read before any code):** A false `inconsistent` marks
EVERY class unsatisfiable — the catastrophic FP. Every counting decision must be
an UNDER-count when uncertain: count a value toward the `≤n` budget ONLY when it
is *provably* distinct AND *provably* in `dr`. When unsure, do not count.

---

### Task 1: `decimal_as_i64` integer-extraction helper

**Files:**
- Modify: `crates/owl-dl-core/src/data_axioms.rs` (add helper near `literal_to_distinct_val` ~line 2360; add unit tests in the existing `#[cfg(test)] mod tests` ~line 2491)

- [ ] **Step 1: Write the failing unit tests**

Add to `mod tests` in `crates/owl-dl-core/src/data_axioms.rs`:

```rust
#[test]
fn decimal_as_i64_integer_values() {
    assert_eq!(decimal_as_i64(&parse_decimal("5").unwrap()), Some(5));
    assert_eq!(decimal_as_i64(&parse_decimal("-7").unwrap()), Some(-7));
    assert_eq!(decimal_as_i64(&parse_decimal("0").unwrap()), Some(0));
    // Leading-zero normalisation: "007" is the integer 7.
    assert_eq!(decimal_as_i64(&parse_decimal("007").unwrap()), Some(7));
}

#[test]
fn decimal_as_i64_rejects_non_integer_and_overflow() {
    // Non-empty fraction ⇒ not an xsd:integer value.
    assert_eq!(decimal_as_i64(&parse_decimal("1.5").unwrap()), None);
    // Beyond i64::MAX ⇒ unrepresentable here ⇒ None (sound under-count).
    assert_eq!(decimal_as_i64(&parse_decimal("99999999999999999999").unwrap()), None);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-core --lib decimal_as_i64`
Expected: FAIL — `cannot find function decimal_as_i64`.

- [ ] **Step 3: Implement the helper**

`Decimal` is `{ negative: bool, int: String, frac: String }` with `frac == ""`
meaning an integer value (see struct at ~line 770). Add near
`literal_to_distinct_val`:

```rust
/// A [`Decimal`] as an `i64`, or `None` when it is not a representable integer:
/// a non-empty fraction (`1.5` is not an `xsd:integer` value) or a magnitude
/// outside `i64`. `None` ⇒ the value is not counted against an integer range
/// (a sound under-count — never a false-fire).
fn decimal_as_i64(d: &Decimal) -> Option<i64> {
    if !d.frac.is_empty() {
        return None;
    }
    // `int` holds the normalised magnitude digits ("" for zero).
    let mag: i128 = if d.int.is_empty() { 0 } else { d.int.parse().ok()? };
    let signed: i128 = if d.negative { -mag } else { mag };
    i64::try_from(signed).ok()
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p owl-dl-core --lib decimal_as_i64`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-core/src/data_axioms.rs
git commit -m "feat(data_axioms): decimal_as_i64 integer-extraction helper (DP-2b)"
```

---

### Task 2: `value_in_range` membership dispatcher

**Files:**
- Modify: `crates/owl-dl-core/src/data_axioms.rs` (add function after `decimal_as_i64`; add unit tests in `mod tests`)

This is the FP-critical predicate. It reuses each range type's `point()/singleton()` + reviewed `subset()`; the only logic here is the value→family dispatch.

- [ ] **Step 1: Write the failing unit tests**

```rust
#[test]
fn value_in_range_integer_and_decimal() {
    let int5 = literal_to_distinct_val(&dt_lit("5", "integer")).unwrap();
    let dec_1_5 = literal_to_distinct_val(&dt_lit("1.5", "decimal")).unwrap();

    // xsd:integer range [0,10] contains integer 5, excludes 1.5 (non-integer).
    let r_int = DataRange::Datatype(dt("integer"));
    assert!(value_in_range(&int5, &r_int)); // bare xsd:integer admits any integer
    assert!(!value_in_range(&dec_1_5, &r_int)); // 1.5 ∉ xsd:integer

    // Faceted integer [0,3]: 5 is out of range.
    let r_int_0_3 = restriction("integer", &[("minInclusive", "0", "integer"),
                                             ("maxInclusive", "3", "integer")]);
    assert!(!value_in_range(&int5, &r_int_0_3));

    // xsd:decimal range admits the integer-valued 5 (int ⊆ decimal) and 1.5.
    let r_dec = DataRange::Datatype(dt("decimal"));
    assert!(value_in_range(&int5, &r_dec));
    assert!(value_in_range(&dec_1_5, &r_dec));
}

#[test]
fn value_in_range_cross_datatype_and_string() {
    let dbl = literal_to_distinct_val(&dt_lit("5.0", "double")).unwrap();
    // A double never satisfies an xsd:integer range (cross-family).
    assert!(!value_in_range(&dbl, &DataRange::Datatype(dt("integer"))));
    // A double satisfies a bare xsd:double range.
    assert!(value_in_range(&dbl, &DataRange::Datatype(dt("double"))));

    // String enumeration membership.
    let sa = DistinctVal::Str("a".into());
    let sz = DistinctVal::Str("z".into());
    let enum_ab = data_one_of(&["a", "b"]);
    assert!(value_in_range(&sa, &enum_ab));
    assert!(!value_in_range(&sz, &enum_ab));
}
```

Add these test helpers to `mod tests` (constructing horned-owl literals/ranges):

```rust
fn dt(local: &str) -> horned_owl::model::Datatype<RcStr> {
    use horned_owl::model::{Build};
    Build::new_rc().datatype(format!("http://www.w3.org/2001/XMLSchema#{local}"))
}
fn dt_lit(value: &str, local: &str) -> Literal<RcStr> {
    Literal::Datatype {
        literal: value.to_string(),
        datatype_iri: dt(local).0,
    }
}
fn restriction(local: &str, facets: &[(&str, &str, &str)]) -> DataRange<RcStr> {
    use horned_owl::model::{FacetRestriction, Facet};
    let frs = facets.iter().map(|(f, v, vlocal)| FacetRestriction {
        f: match *f {
            "minInclusive" => Facet::MinInclusive,
            "maxInclusive" => Facet::MaxInclusive,
            "minExclusive" => Facet::MinExclusive,
            "maxExclusive" => Facet::MaxExclusive,
            other => panic!("unhandled facet {other}"),
        },
        l: dt_lit(v, vlocal),
    }).collect();
    DataRange::DatatypeRestriction(dt(local), frs)
}
fn data_one_of(members: &[&str]) -> DataRange<RcStr> {
    DataRange::DataOneOf(members.iter().map(|m| Literal::Simple { literal: (*m).to_string() }).collect())
}
```

> NOTE to implementer: verify the exact horned-owl 1.4 constructor names
> (`Build::datatype`, `FacetRestriction { f, l }`, `Facet::MinInclusive`,
> `DataRange::DatatypeRestriction(Datatype, Vec<FacetRestriction>)`) against the
> patterns already used in `parse_integer_range` (~line 1550) and
> `parse_float_range` (~line 1620) — those functions destructure exactly these
> shapes, so copy their field/variant names rather than guessing.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p owl-dl-core --lib value_in_range`
Expected: FAIL — `cannot find function value_in_range`.

- [ ] **Step 3: Implement the dispatcher**

```rust
/// `true` iff `v` is **provably** a value of `dr`. Reuses each range type's
/// reviewed `subset` via its singleton (`point`/`singleton`) constructor —
/// `{v} ⊆ dr` is exactly `v ∈ dr`, with no new boundary algebra. The parsers
/// are pairwise mutually exclusive by datatype, so a value is tested only when
/// its family matches `dr`; any parser returning `None` ⇒ not provably in `dr`
/// ⇒ `false` (sound under-count). The two soundness subtleties:
/// (1) an integer-valued `Num` is also a decimal — an `xsd:decimal` range
///     correctly admits it; a non-integer `Num` (`1.5`) is NOT an `xsd:integer`
///     value and an integer range correctly rejects it (`decimal_as_i64`);
/// (2) cross-family values never match (a `Double` against an integer range,
///     etc.) because the matching parser returns `None`.
fn value_in_range<A: ForIRI>(v: &DistinctVal, dr: &DataRange<A>) -> bool {
    match v {
        DistinctVal::Num(dec) => {
            if let Some(ir) = parse_integer_range(dr) {
                return decimal_as_i64(dec).is_some_and(|i| IntegerRange::point(i).subset(ir));
            }
            parse_decimal_range(dr).is_some_and(|r| OrdRange::point(dec.clone()).subset(&r))
        }
        DistinctVal::Double(f) => {
            parse_float_range(dr).is_some_and(|r| FloatRange::point(f.0).subset(r))
        }
        DistinctVal::Date(d) => {
            parse_date_range(dr).is_some_and(|r| OrdRange::point(d.clone()).subset(&r))
        }
        DistinctVal::DateTime(d) => {
            parse_datetime_range(dr).is_some_and(|r| OrdRange::point(d.clone()).subset(&r))
        }
        DistinctVal::Str(s) => {
            parse_string_range(dr).is_some_and(|r| StrSet::singleton(s.clone()).subset(&r))
        }
    }
}
```

> `OrdF64` is `OrdF64(pub f64)` so `f.0` is the inner `f64`. `OrdRange::point`
> needs `T: Ord + Clone` — `Decimal`, `DateKey`, `DateTimeKey` already satisfy
> this (they back existing `OrdRange` buckets).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p owl-dl-core --lib value_in_range`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-core/src/data_axioms.rs
git commit -m "feat(data_axioms): value_in_range membership via point().subset() reuse (DP-2b)"
```

---

### Task 3: `emit_data_cardinality_violations_typed` + wiring + integration canaries

**Files:**
- Modify: `crates/owl-dl-core/src/data_axioms.rs` (add the function; call it in `derive_data_axioms` ~line 94)
- Test: `crates/owl-dl-reasoner/tests/datatype_inconsistency.rs` (append canaries)

- [ ] **Step 1: Write the failing integration canaries**

Append to `crates/owl-dl-reasoner/tests/datatype_inconsistency.rs` (it already has
`consistent(body) -> bool` over OFN; an inconsistent fixture asserts `!consistent(...)`):

```rust
// ─── DP-2b: typed/faceted from-type data-cardinality ──────────────────

/// FIRES: C ⊑ ≤2 dp.xsd:integer, individual:C with 3 distinct integers.
#[test]
fn typed_card_three_integers_over_max_two_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(2 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)
    DataPropertyAssertion(:p :a "3"^^xsd:integer)"#
    ));
}

/// FIRES: faceted range [0,10], ≤1, two distinct in-range values.
#[test]
fn typed_card_faceted_range_two_in_range_over_max_one_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "3"^^xsd:integer)
    DataPropertyAssertion(:p :a "7"^^xsd:integer)"#
    ));
}

/// FIRES: DataExactCardinality(1) behaves as ≤1.
#[test]
fn typed_card_exact_one_two_values_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataExactCardinality(1 :p xsd:double))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1.0"^^xsd:double)
    DataPropertyAssertion(:p :a "2.0"^^xsd:double)"#
    ));
}

/// FIRES: values split across dp and a sub-dp dp' ⊑ dp sum past n.
#[test]
fn typed_card_subproperty_routing_is_inconsistent() {
    assert!(!consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(NamedIndividual(:a))
    SubDataPropertyOf(:q :p)
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:q :a "2"^^xsd:integer)"#
    ));
}

// ── FP GUARDS (must stay consistent) ──

/// Out-of-range value doesn't count: ≤1 over [0,10], values 5 & 20 ⇒ only 5
/// in range ⇒ consistent.
#[test]
fn typed_card_out_of_range_value_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "5"^^xsd:integer)
    DataPropertyAssertion(:p :a "20"^^xsd:integer)"#
    ));
}

/// Cross-datatype: ≤1 over xsd:integer, one integer + one double ⇒ only the
/// integer counts ⇒ consistent.
#[test]
fn typed_card_cross_datatype_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2.0"^^xsd:double)"#
    ));
}

/// Duplicate values count once: "1" and "01" are the same integer ⇒ ≤1 holds.
#[test]
fn typed_card_duplicate_values_count_once_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "01"^^xsd:integer)"#
    ));
}

/// Boundary: max-exclusive [0,5), value 5 ⇒ 5 ∉ range ⇒ only one in-range ⇒
/// consistent.
#[test]
fn typed_card_exclusive_boundary_uncounted_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxExclusive "5"^^xsd:integer)))
    ClassAssertion(:C :a)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)
    DataPropertyAssertion(:p :a "5"^^xsd:integer)"#
    ));
}

/// Untyped individual (no ClassAssertion to C) ⇒ constraint doesn't apply ⇒
/// consistent even with 3 values.
#[test]
fn typed_card_untyped_individual_is_consistent() {
    assert!(consistent(
        r#"    Declaration(Class(:C)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
    SubClassOf(:C DataMaxCardinality(1 :p xsd:integer))
    DataPropertyAssertion(:p :a "1"^^xsd:integer)
    DataPropertyAssertion(:p :a "2"^^xsd:integer)
    DataPropertyAssertion(:p :a "3"^^xsd:integer)"#
    ));
}
```

> NOTE: confirm the OFN facet keyword spelling against an existing faceted
> fixture in the repo (the DP-1 range tests / `parse_integer_range` show whether
> horned-owl's OFN reader expects `xsd:minInclusive` vs `minInclusive`). Adjust
> the fixture text to whatever the reader accepts; the *assertions* are the spec.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p owl-dl-reasoner --test datatype_inconsistency typed_card`
Expected: the four `*_is_inconsistent` tests FAIL (currently `consistent` returns
`true`); the `*_is_consistent` guards already PASS (nothing fires yet).

- [ ] **Step 3: Implement `emit_data_cardinality_violations_typed`**

Add to `data_axioms.rs`, modelled on `emit_functional_dp_cardinality_violations`
(~line 2394). Field names confirmed in the codebase: `C::ClassAssertion(ax)` →
`ax.ce` / `ax.i`; `C::SubClassOf(ax)` → `ax.sub` / `ax.sup`;
`ClassExpression::DataMaxCardinality { n, dp, dr }` and `DataExactCardinality
{ n, dp, dr }`; `class_iri(ce) -> Option<String>` (atomic only); `dpe_iri(dp)`;
`individual_iri(&ax.i)`; `closure_sub_dp(edges)`.

```rust
/// DP-2b: a typed/faceted from-type data-cardinality violation ⇒ global
/// inconsistency. `ClassAssertion(C₀, a)` with `C₀ ⊑* C` and
/// `C ⊑ ≤n dp.dr` (`DataMax`/`DataExact`) bounds the count of `a`'s `dp`-fillers
/// that lie in `dr`. When `a` is asserted MORE than `n` distinct values
/// provably in `dr` (directly or via a sub-dp `dp' ⊑ dp`), the ABox has no
/// model ⇒ emit `Top ⊑ Bot`.
///
/// Sound by construction: distinctness via canonical [`DistinctVal`] keys
/// (integer+decimal folded, `xsd:float`/language-tagged excluded); membership
/// via `value_in_range` (provably-in-range only; cross-family never counts);
/// `DataMax`/`DataExact` only (never `DataMin`); sub→super dp routing; told
/// reflexive-transitive `SubClassOf` typing; anonymous individuals ignored.
/// Leaves the existing functional-≤1 and bare-string-`≤n` checks untouched;
/// overlap is a harmless idempotent `Top ⊑ Bot`.
fn emit_data_cardinality_violations_typed<A: ForIRI>(
    src: &SetOntology<A>,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    use Component as C;

    let mut sub_dp: Vec<(String, String)> = Vec::new();
    let mut subclass_atomic: Vec<(String, String)> = Vec::new();
    // (ind, dp) → distinct parsed values.
    let mut ind_dp_vals: BTreeMap<(String, String), BTreeSet<DistinctVal>> = BTreeMap::new();
    // ind → asserted atomic types.
    let mut ind_classes: Vec<(String, String)> = Vec::new();
    // Constraints: (class, dp, n, dr) — dr borrowed from src.
    let mut constraints: Vec<(String, String, u32, &DataRange<A>)> = Vec::new();

    for ac in src {
        match &ac.component {
            C::SubDataPropertyOf(ax) => {
                let (sub, sup) = (dpe_iri(&ax.sub), dpe_iri(&ax.sup));
                if !sub.is_empty() && !sup.is_empty() {
                    sub_dp.push((sub, sup));
                }
            }
            C::EquivalentDataProperties(ax) => {
                let iris: Vec<String> = ax.0.iter().map(dpe_iri).collect();
                for i in 0..iris.len() {
                    for j in 0..iris.len() {
                        if i != j {
                            sub_dp.push((iris[i].clone(), iris[j].clone()));
                        }
                    }
                }
            }
            C::SubClassOf(ax) => {
                // Atomic ⊑ atomic edge for typing closure.
                if let (Some(s), Some(t)) = (class_iri(&ax.sub), class_iri(&ax.sup)) {
                    subclass_atomic.push((s, t));
                }
                // Atomic ⊑ ≤n dp.dr constraint.
                if let Some(c) = class_iri(&ax.sub) {
                    match &ax.sup {
                        ClassExpression::DataMaxCardinality { n, dp, dr }
                        | ClassExpression::DataExactCardinality { n, dp, dr } => {
                            constraints.push((c, dpe_iri(dp), *n, dr));
                        }
                        _ => {}
                    }
                }
            }
            C::ClassAssertion(ax) => {
                if let (Some(c), Some(ind)) = (class_iri(&ax.ce), individual_iri(&ax.i)) {
                    ind_classes.push((ind, c));
                }
            }
            C::DataPropertyAssertion(ax) => {
                let Some(ind) = individual_iri(&ax.from) else {
                    continue;
                };
                if let Some(v) = literal_to_distinct_val(&ax.to) {
                    ind_dp_vals.entry((ind, dpe_iri(&ax.dp))).or_default().insert(v);
                }
            }
            _ => {}
        }
    }

    if constraints.is_empty() || ind_classes.is_empty() || ind_dp_vals.is_empty() {
        return;
    }

    let class_closure = closure_sub_dp(&subclass_atomic); // class → {self ∪ supers}
    let dp_closure = closure_sub_dp(&sub_dp); // dp → {self ∪ supers}

    // ind → all told types (reflexive-transitive).
    let mut ind_types: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (ind, c) in &ind_classes {
        let entry = ind_types.entry(ind.as_str()).or_default();
        match class_closure.get(c) {
            Some(supers) => entry.extend(supers.iter().map(String::as_str)),
            None => {
                entry.insert(c.as_str());
            }
        }
    }

    for (class, dp, n, dr) in &constraints {
        for (ind, types) in &ind_types {
            if !types.contains(class.as_str()) {
                continue;
            }
            // Distinct fillers across dp and every sub-dp of dp, provably in dr.
            let mut distinct: BTreeSet<&DistinctVal> = BTreeSet::new();
            for ((i, q), vals) in &ind_dp_vals {
                if i.as_str() != *ind {
                    continue;
                }
                let is_sub = dp_closure
                    .get(q.as_str())
                    .map_or(q == dp, |supers| supers.contains(dp));
                if !is_sub {
                    continue;
                }
                for v in vals {
                    if value_in_range(v, dr) {
                        distinct.insert(v);
                    }
                }
            }
            if distinct.len() > *n as usize {
                out.push(Axiom::SubClassOf { sub: top_id, sup: bot_id });
                return;
            }
        }
    }
}
```

> `BTreeSet<&DistinctVal>` dedups by value identity (`DistinctVal: Ord`); the
> same value asserted on `dp` and a sub-dp collapses to one. If borrow-checker
> friction arises from `&DistinctVal` keys, fall back to `BTreeSet<DistinctVal>`
> with `.clone()` — semantically identical.

- [ ] **Step 4: Wire it into `derive_data_axioms`**

In `derive_data_axioms` (~line 94), add the call after the functional check:

```rust
    emit_functional_dp_cardinality_violations(src, top_id, bot_id, &mut out);
    emit_data_cardinality_violations_typed(src, top_id, bot_id, &mut out);
    out
```

- [ ] **Step 5: Run the canaries**

Run: `cargo test -p owl-dl-reasoner --test datatype_inconsistency`
Expected: ALL pass — the four `*_is_inconsistent` now fire, the FP guards stay
consistent, and the pre-existing DP-1/DP-1b/DP-2 canaries are unaffected.

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt -p owl-dl-core && cargo clippy -p owl-dl-core --all-targets 2>&1 | grep -c warning`
Expected: `0`.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-core/src/data_axioms.rs crates/owl-dl-reasoner/tests/datatype_inconsistency.rs
git commit -m "feat(data_axioms): DP-2b typed/faceted from-type data-cardinality inconsistency"
```

---

### Task 4: Corpus regression + soundness verification

**Files:** none (verification only). Build release once:
`cargo build --release -p owl-dl-cli`.

- [ ] **Step 1: Existing datatype + reasoner suites stay green**

Run: `cargo test -p owl-dl-core --lib && cargo test -p owl-dl-reasoner --test datatype_inconsistency --test datatype_value_membership --test wedge_consistency`
Expected: all PASS (no regression in DP-1/DP-1b/DP-2/functional/value-membership/wedge-consistency).

- [ ] **Step 2: Consistent fixtures stay consistent (the FP gate)**

```sh
for f in pizza ro family wine sulo bibtex; do
  printf "%-12s " "$f:"; ./target/release/rustdl consistent ontologies/real/$f.ofn
done
./target/release/rustdl consistent ontologies/external/shoiq-knowledge.ofn
```
Expected: every line `consistent` (DP-2b must not flip any of these).

- [ ] **Step 3: Genuinely-inconsistent fixture stays inconsistent**

Run: `./target/release/rustdl consistent ontologies/external/ore-15516-alchoiq.ofn`
Expected: `inconsistent`.

- [ ] **Step 4: Classify closure unchanged (FP=0/MISSED=0)**

```sh
for f in ontologies/external/ore-15672-shoin.ofn ontologies/external/shoiq-knowledge.ofn; do
  printf "%-30s " "$(basename $f):"
  ./target/release/rustdl classify "$f" 2>/dev/null | grep "^# subsumption"
done
```
Expected: `ore-15672 saturation=142`, `shoiq-knowledge saturation=443` (unchanged
from the pre-DP-2b baseline).

- [ ] **Step 5: Opus review gate**

The membership/counting logic is soundness-critical (a false `inconsistent` is
catastrophic). Before declaring done, the controller dispatches an **opus**
spec-compliance + code-quality review (not sonnet), per the hardened rule for
FP-critical datatype work. Address any finding, re-verify Steps 1–4, then finalize.

- [ ] **Step 6: No commit** (verification task; any fixes land under their own task's commit).

---

## Notes for the executor

- The hardened review rule: **soundness-critical merge/dep/datatype work → opus, not sonnet.** Task 4 Step 5 is mandatory.
- The corpus has zero naturally-occurring DP-2b fire (validated during scoping); the canaries are the entire functional safety net. Do not weaken them.
- If any consistent fixture flips to `inconsistent`, STOP — that is the catastrophic FP. The most likely cause is an over-permissive `value_in_range` (a parser matching the wrong family, or `decimal_as_i64` accepting a non-integer). Bisect with the failing fixture reduced to its `≤n dp.dr` + assertions.
