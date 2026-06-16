//! Fragment-gate canaries: out-of-ALCH constructs must yield `OutOfFragment`.
//!
//! Each test sends a construct that is outside the ALCH fragment to
//! `owl_dl_cb::classify` and asserts it returns `CbOutcome::OutOfFragment(_)`.
//! This ensures the orchestrator will never receive a (possibly wrong) CB
//! hierarchy for these inputs — it defers to the existing engine instead.
//!
//! **Status:** RED pending Task A (normalize/gate) integration. The gate logic
//! lives in `normalize::normalize`; until Task A lands every call panics via
//! `todo!()`. That is the expected pre-integration state.
//!
//! **Tested out-of-ALCH constructs:**
//! 1. `ObjectMaxCardinality` (≤n)
//! 2. `ObjectMinCardinality` (≥n)
//! 3. Inverse role (`ObjectInverseOf`) used in a class expression
//! 4. Nominal (`ObjectOneOf`)
//! 5. `ObjectHasValue` (nominal via individual filler)
//! 6. Datatype property usage (`DataSomeValuesFrom`) → lowers to `∃p.DKey(...)`;
//!    the gate must recognize DKey-IRI fillers as out-of-ALCH.
//! 7. `ObjectHasSelf`
//! 8. Role chain (`SubObjectPropertyOf(ObjectPropertyChain(...), ...)`)
//! 9. `TransitiveObjectProperty`
//!
//! **Note on the datatype gate (test 6):** `convert_ontology` lowers
//! `DataSomeValuesFrom(p, xsd:integer facets)` to `∃p.DKey(range)`, where the
//! filler class has a `urn:rustdl-dkey:` IRI. Task A's normalizer must detect
//! these synthetic `DKey` IRIs and return `Err` (out-of-fragment), since ALCH does
//! not include concrete-domain datatypes. Any datatype construct surviving
//! convert as `DKey` is out of ALCH and must not reach the CB calculus.
//!
//! Run: `cargo test -p owl-dl-cb --test cb_fragment_gate`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_cb::CbOutcome;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// Parse OFN `body` and classify. Returns the outcome for assertion by the caller.
fn outcome(body: &str) -> CbOutcome {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("OFN parse error");
    let internal = convert_ontology(&onto).expect("convert_ontology error");
    owl_dl_cb::classify(&internal)
}

/// Assert that the classify outcome is `OutOfFragment` for the given body.
fn assert_out_of_fragment(body: &str) {
    assert!(
        matches!(outcome(body), CbOutcome::OutOfFragment(_)),
        "expected OutOfFragment for construct, got Classified instead.\nBody:\n{body}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 1. ObjectMaxCardinality (≤n) over a NAMED role — now IN the B2 ALCHQ fragment
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_max_cardinality_admitted() {
    assert!(
        matches!(
            outcome(
                r"    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectMaxCardinality(1 :R owl:Thing))"
            ),
            CbOutcome::Classified(_)
        ),
        "≤n over a named role is in the B2 ALCHQ fragment"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 2. ObjectMinCardinality (≥n) over a NAMED role — now IN the B2 ALCHQ fragment
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_min_cardinality_admitted() {
    assert!(
        matches!(
            outcome(
                r"    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectMinCardinality(2 :R owl:Thing))"
            ),
            CbOutcome::Classified(_)
        ),
        "≥n over a named role is in the B2 ALCHQ fragment"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 2b. Cardinality over an INVERSE role stays out of fragment (B3).
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_inverse_cardinality_rejected() {
    assert_out_of_fragment(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:R) :B))",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 3. Inverse role in a class expression — B3 construct, not ALCH
//
//   `ObjectSomeValuesFrom(ObjectInverseOf(:R), :B)` lowers to
//   `Some(Role { id: R, is_inverse: true }, B)` in the IR.
//   Task A must reject any clause whose role has `is_inverse = true`.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_inverse_role_in_some() {
    assert_out_of_fragment(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:R) :B))",
    );
}

#[test]
fn gate_inverse_role_in_all() {
    assert_out_of_fragment(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectAllValuesFrom(ObjectInverseOf(:R) :B))",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 4. Nominal (ObjectOneOf) — B4 construct, not ALCH
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_nominal_one_of() {
    assert_out_of_fragment(
        r"    Declaration(Class(:A))
    Declaration(NamedIndividual(:a))
    SubClassOf(:A ObjectOneOf(:a))",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 5. ObjectHasValue (nominal via individual filler) — B4 construct, not ALCH
//
//   `ObjectHasValue(:R, :a)` lowers to `Some(R, Nominal(a))` in the IR.
//   Any `Nominal` in a concept pool is out of ALCH.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_object_has_value() {
    assert_out_of_fragment(
        r"    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    Declaration(NamedIndividual(:a))
    SubClassOf(:A ObjectHasValue(:R :a))",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 6. Datatype usage via DataSomeValuesFrom → DKey synthetic
//
//   `convert_ontology` lowers `DataSomeValuesFrom(p, xsd:integer facets)` to
//   `∃p.DKey(range)` where the filler's IRI begins with `urn:rustdl-dkey:`.
//   Task A must recognize DKey IRIs in concept fillers as out-of-ALCH
//   (datatypes are a concrete-domain extension outside the ALCH fragment).
//
//   If the chosen datatype expression is dropped entirely by convert (unrecognized
//   datatype pattern → `UnsupportedDataRange`), the test would silently get
//   `Classified` and fail to gate. We use `xsd:integer` with a simple facet
//   which IS recognized by D5/D6 and survives as a DKey. See the NOTE in the
//   module doc above.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_datatype_some_values_from_via_dkey() {
    // Use a raw-string literal so the OFN quoted literals ("1"^^...) parse
    // correctly without double-escaping inside a Rust string.
    let body = "    Declaration(Class(:A))\n    Declaration(DataProperty(:p))\n    SubClassOf(:A DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))";
    assert_out_of_fragment(body);
}

// ══════════════════════════════════════════════════════════════════════════════
// 7. ObjectHasSelf — SRI construct (self-restriction), not ALCH
//
//   `ObjectHasSelf(R)` lowers to `SelfRestriction(R)` in the IR.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_object_has_self() {
    assert_out_of_fragment(
        r"    Declaration(Class(:A))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectHasSelf(:R))",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 8. Role chain (SubObjectPropertyOf(ObjectPropertyChain(...), ...)) — B4 /RBox
//
//   `SubObjectPropertyOf(ObjectPropertyChain(:R :S) :T)` lowers to
//   `Axiom::RoleChain([R, S], T)` in the IR. Chains are out of ALCH.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_role_chain() {
    assert_out_of_fragment(
        r"    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:T))
    SubObjectPropertyOf(ObjectPropertyChain(:R :S) :T)",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 9. TransitiveObjectProperty — B4/RBox construct, not ALCH
//
//   `TransitiveObjectProperty(R)` lowers to `Axiom::TransitiveRole(R)` in the
//   IR. Transitivity is outside ALCH (it's in the RBox / S fragment).
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn gate_transitive_role() {
    assert_out_of_fragment(
        r"    Declaration(ObjectProperty(:R))
    TransitiveObjectProperty(:R)",
    );
}
