#![allow(clippy::doc_markdown)]
//! External completeness oracle for `materialize_object_property_assertions`.
//!
//! The property-assertion analog of `konclude_closure_diff`: rustdl's output is
//! diffed against **HermiT**-inferred property assertions (the only reasoner
//! interface that materializes them — Konclude emits the classification hierarchy
//! and individual realization, not inferred role edges; and rustdl's own
//! `entails()` is *weaker* than the materializer here, so it can't be the
//! reference). The oracle is generated offline by `docker/robot/property-oracle.sh`
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
use horned_owl::model::{Component, Individual, ObjectPropertyExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_object_property_assertions;
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
