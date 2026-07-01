#![allow(clippy::doc_markdown)]
//! External completeness oracle for `materialize_object_property_assertions`.
//!
//! The property-assertion analog of `konclude_closure_diff`: rustdl's output is
//! diffed against **HermiT**-inferred property assertions (the only reasoner
//! interface that materializes them — Konclude emits the classification hierarchy
//! and individual realization, not inferred role edges; and rustdl's own
//! `entails(OPA)` now routes through `materialize_object` itself (issue #28), so
//! it would be circular as a reference). The oracle is generated offline by
//! `docker/robot/property-oracle.sh`
//! (ROBOT + embedded HermiT) and committed as `*-materialized.owx`, so this test
//! needs no docker at run time.
//!
//! Regenerate after changing the fixture:
//!   bash docker/robot/property-oracle.sh \
//!     crates/owl-dl-reasoner/tests/fixtures/materialize/rbox.ofn \
//!     crates/owl-dl-reasoner/tests/fixtures/materialize/rbox-materialized.owx

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::{Component, Individual, Literal, ObjectPropertyExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    materialize_data_property_assertions, materialize_object_property_assertions,
};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const TOP: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
const BOT: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";

type Triples = BTreeSet<(String, String, String)>;

/// HermiT-inferred object-property assertions between NAMED individuals from the
/// committed oracle (top/bottom filtered, matching `materialize`).
fn oracle_edges(path: &Path) -> Triples {
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut set = Triples::new();
    for ax in &onto {
        if let Component::ObjectPropertyAssertion(opa) = &ax.component
            && let (
                ObjectPropertyExpression::ObjectProperty(p),
                Individual::Named(s),
                Individual::Named(t),
            ) = (&opa.ope, &opa.from, &opa.to)
        {
            let prop = p.0.to_string();
            if prop == TOP || prop == BOT {
                continue;
            }
            set.insert((s.0.to_string(), prop, t.0.to_string()));
        }
    }
    set
}

#[test]
fn materialize_matches_hermit_oracle() {
    let dir = Path::new("tests/fixtures/materialize");
    let file = File::open(dir.join("rbox.ofn")).expect("fixture");
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse fixture");

    let materialized: Triples = materialize_object_property_assertions(&onto)
        .expect("materialize")
        .into_iter()
        .collect();
    let oracle = oracle_edges(&dir.join("rbox-materialized.owx"));

    let missed: Vec<_> = oracle.difference(&materialized).collect();
    let fp: Vec<_> = materialized.difference(&oracle).collect();
    assert!(
        missed.is_empty(),
        "MISSED — HermiT infers, materialize omits: {missed:?}"
    );
    assert!(
        fp.is_empty(),
        "FP — materialize returns, HermiT does not: {fp:?}"
    );
}

/// (subject, data-property, lexical, datatype) 4-tuples; lang is dropped since the
/// fixture is all typed integer literals. Restricted to `DataPropertyAssertion`
/// over NAMED individuals, matching `materialize_data_property_assertions`.
type DataQuads = BTreeSet<(String, String, String, String)>;

/// HermiT-inferred data-property assertions between NAMED individuals from the
/// committed oracle.
///
/// NOTE: HermiT's `InferredPropertyAssertionGenerator` does **not** traverse
/// `EquivalentDataProperties` (an assertion on `p` is not re-emitted on an
/// equivalent `q`), so the fixture deliberately exercises only the constructs
/// where the generator is complete — `SubDataPropertyOf` and `SameIndividual`
/// folding — keeping this a clean bidirectional oracle.
fn oracle_data_quads(path: &Path) -> DataQuads {
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut set = DataQuads::new();
    for ax in &onto {
        if let Component::DataPropertyAssertion(dpa) = &ax.component
            && let Individual::Named(s) = &dpa.from
        {
            let prop = dpa.dp.0.to_string();
            let (lex, dt) = match &dpa.to {
                Literal::Datatype {
                    literal,
                    datatype_iri,
                } => (literal.clone(), datatype_iri.to_string()),
                Literal::Simple { literal } => (
                    literal.clone(),
                    "http://www.w3.org/2001/XMLSchema#string".to_string(),
                ),
                // Language-tagged literals are not part of the oracle fixture.
                Literal::Language { .. } => continue,
            };
            set.insert((s.0.to_string(), prop, lex, dt));
        }
    }
    set
}

#[test]
fn materialize_data_matches_hermit_oracle() {
    let dir = Path::new("tests/fixtures/materialize");
    let file = File::open(dir.join("rbox.ofn")).expect("fixture");
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse fixture");

    // materialize_data returns 5-tuples (subj, prop, lexical, datatype, lang);
    // drop lang (fixture is all typed integer literals ⇒ lang == "").
    let materialized: DataQuads = materialize_data_property_assertions(&onto)
        .expect("materialize data")
        .into_iter()
        .map(|(s, p, lex, dt, _lang)| (s, p, lex, dt))
        .collect();
    let oracle = oracle_data_quads(&dir.join("rbox-materialized.owx"));

    let missed: Vec<_> = oracle.difference(&materialized).collect();
    let fp: Vec<_> = materialized.difference(&oracle).collect();
    assert!(
        missed.is_empty(),
        "MISSED — HermiT infers, materialize_data omits: {missed:?}"
    );
    assert!(
        fp.is_empty(),
        "FP — materialize_data returns, HermiT does not: {fp:?}"
    );
}
