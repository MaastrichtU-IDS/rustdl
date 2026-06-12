//! Canaries for black-box justification (find-one / find-all).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{
    Entailment, Justification, entails, find_one_justification, logical_axioms, ontology_from,
};
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

fn dbgset(j: &Justification<RcStr>) -> std::collections::BTreeSet<String> {
    j.axioms.iter().map(|c| format!("{c:?}")).collect()
}

#[test]
fn find_one_subclassof_exact() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:Z))\n\
                  SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:Z :C)",
    );
    let q = Entailment::SubClassOf {
        sub: "http://t/A".into(),
        sup: "http://t/C".into(),
    };
    let j = find_one_justification(&o, &q).unwrap().expect("entailed");
    assert_eq!(
        j.axioms.len(),
        2,
        "minimal = {{A⊑B, B⊑C}}; got {:?}",
        dbgset(&j)
    );
    assert!(j.minimal_guaranteed, "EL ⇒ minimality guaranteed");
    let (fixed, _) = owl_dl_reasoner::justify::logical_axioms(&o);
    assert!(
        entails(
            &owl_dl_reasoner::justify::ontology_from(&fixed, &j.axioms),
            &q
        )
        .unwrap(),
        "justification must re-entail"
    );
    assert!(
        !entails(
            &owl_dl_reasoner::justify::ontology_from(&fixed, &j.axioms[..1]),
            &q
        )
        .unwrap(),
        "removing an axiom must break entailment (minimal)"
    );
}

#[test]
fn find_one_not_entailed_is_none() {
    let o = onto("Declaration(Class(:A)) Declaration(Class(:B))");
    let q = Entailment::SubClassOf {
        sub: "http://t/A".into(),
        sup: "http://t/B".into(),
    };
    assert!(find_one_justification(&o, &q).unwrap().is_none());
}

#[test]
fn find_one_unsat() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B))\n\
                  DisjointClasses(:A :B) SubClassOf(:A :B)",
    );
    let q = Entailment::Unsatisfiable {
        class: "http://t/A".into(),
    };
    let j = find_one_justification(&o, &q).unwrap().expect("A unsat");
    assert_eq!(j.axioms.len(), 2, "got {:?}", dbgset(&j));
}
