//! Canaries for black-box justification (find-one / find-all).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, entails, logical_axioms, ontology_from};
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/o>\n{body}\n)\n");
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    o
}

#[test]
fn entails_subclassof_chain() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                  SubClassOf(:A :B) SubClassOf(:B :C)",
    );
    assert!(
        entails(
            &o,
            &Entailment::SubClassOf {
                sub: "http://t/A".into(),
                sup: "http://t/C".into()
            }
        )
        .unwrap()
    );
    assert!(
        !entails(
            &o,
            &Entailment::SubClassOf {
                sub: "http://t/C".into(),
                sup: "http://t/A".into()
            }
        )
        .unwrap()
    );
}

#[test]
fn entails_disjoint_via_probe() {
    let o = onto("Declaration(Class(:B)) Declaration(Class(:C))\nDisjointClasses(:B :C)");
    assert!(
        entails(
            &o,
            &Entailment::DisjointClasses {
                a: "http://t/B".into(),
                b: "http://t/C".into()
            }
        )
        .unwrap()
    );
    let o2 = onto("Declaration(Class(:B)) Declaration(Class(:C))");
    assert!(
        !entails(
            &o2,
            &Entailment::DisjointClasses {
                a: "http://t/B".into(),
                b: "http://t/C".into()
            }
        )
        .unwrap()
    );
}

#[test]
fn partition_and_rebuild() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                  SubClassOf(:A :B) SubClassOf(:B :C)",
    );
    let (fixed, candidates) = logical_axioms(&o);
    assert_eq!(candidates.len(), 2, "two SubClassOf axioms are candidates");
    assert!(
        fixed.len() >= 3,
        "declarations are fixed; got {}",
        fixed.len()
    );
    let rebuilt = ontology_from(&fixed, &candidates);
    assert!(
        entails(
            &rebuilt,
            &Entailment::SubClassOf {
                sub: "http://t/A".into(),
                sup: "http://t/C".into()
            }
        )
        .unwrap()
    );
    let rebuilt1 = ontology_from(&fixed, &candidates[..1]);
    assert!(
        !entails(
            &rebuilt1,
            &Entailment::SubClassOf {
                sub: "http://t/A".into(),
                sup: "http://t/C".into()
            }
        )
        .unwrap()
    );
}
