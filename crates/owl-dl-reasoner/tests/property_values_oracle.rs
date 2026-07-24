//! Integration tests for `inferred_object_property_values` /
//! `inferred_data_property_values` (issue #45, Task 4.1).
//!
//! `inferred_object_property_values` = the sound `materialize_object_property_assertions`
//! seed, plus a budgeted/bounded entailment extension over the seed's own
//! individual-pair neighborhood (never the full `|I|²×|R|` cross-product). The
//! bounded-extension oracle (candidate pairs beyond the seed neighborhood) is
//! Task 4.4's concern; here we confirm the seed surfaces correctly and the
//! public API shape is right.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{inferred_data_property_values, inferred_object_property_values};
use std::io::Cursor;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

/// Symmetric(:r); r(a,b) ⇒ r(b,a) entailed. Both directions are already in the
/// `materialize_object_property_assertions` seed (the `ABox` saturator closes
/// symmetric roles), so this exercises the seed-surfacing path, not the
/// bounded entailment extension.
#[test]
fn object_values_include_asserted_and_symmetric() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(ObjectProperty(:r)) SymmetricObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b))",
    );
    let v = inferred_object_property_values(&o, None).unwrap();
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#b"));
    assert!(has("http://ex/#b", "http://ex/#r", "http://ex/#a"));
}

/// A plain `DataPropertyAssertion` must surface as its 4-tuple (subject,
/// property, lexical, datatype) — `inferred_data_property_values` is a
/// structural passthrough over `materialize_data_property_assertions` with the
/// `lang` element dropped.
#[test]
fn data_values_include_asserted() {
    let o = onto(
        r#"Prefix(:=<http://ex/#>)
          Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(DataProperty(:dp))
            DataPropertyAssertion(:dp :a "42"^^xsd:integer))"#,
    );
    let v = inferred_data_property_values(&o).unwrap();
    assert!(v.quads().iter().any(|(s, p, lex, dt)| {
        s == "http://ex/#a"
            && p == "http://ex/#dp"
            && lex == "42"
            && dt == "http://www.w3.org/2001/XMLSchema#integer"
    }));
    assert!(!v.incomplete());
}

/// `r(a,b)`, `r(b,c)`, `Transitive(r)` ⇒ `r(a,c)` entailed — already closed by
/// the `materialize_object_property_assertions` seed itself (transitive
/// closure is part of that closure), so all three triples are present without
/// the extension needing to add anything new. `r` is non-symmetric, so the
/// candidate-pair neighborhood also probes the reverse orientations
/// (`r(b,a)`, `r(c,b)`, `r(c,a)`) — none are entailed (`Some(true)`), but
/// running those probes at all still marks the result `incomplete()` per the
/// documented honesty policy (any extension probe run ⇒ `incomplete`, even
/// when it adds nothing).
#[test]
fn object_values_transitive_seed_and_honest_incomplete_flag() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
            TransitiveObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :b :c))",
    );
    let v = inferred_object_property_values(&o, None).unwrap();
    let has = |s: &str, p: &str, ob: &str| {
        v.triples()
            .iter()
            .any(|(x, y, z)| x == s && y == p && z == ob)
    };
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#b"));
    assert!(has("http://ex/#b", "http://ex/#r", "http://ex/#c"));
    assert!(has("http://ex/#a", "http://ex/#r", "http://ex/#c"));
    // Never entailed — the reverse orientations must NOT appear (soundness).
    assert!(!has("http://ex/#b", "http://ex/#r", "http://ex/#a"));
    assert!(!has("http://ex/#c", "http://ex/#r", "http://ex/#b"));
    assert!(!has("http://ex/#c", "http://ex/#r", "http://ex/#a"));
    assert!(v.incomplete());
}
