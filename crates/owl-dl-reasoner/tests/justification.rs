//! Canaries for black-box justification (find-one / find-all).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{
    Entailment, Justification, entails, find_all_justifications, find_one_justification,
    logical_axioms, ontology_from,
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
    // Minimal: removing ANY single axiom must break entailment.
    for i in 0..j.axioms.len() {
        let reduced: Vec<_> = j
            .axioms
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != i)
            .map(|(_, c)| c.clone())
            .collect();
        assert!(
            !entails(
                &owl_dl_reasoner::justify::ontology_from(&fixed, &reduced),
                &q
            )
            .unwrap(),
            "removing axiom {i} must break entailment (genuine minimality)"
        );
    }
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

#[test]
fn find_one_equivalent() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B))\n\
                  SubClassOf(:A :B) SubClassOf(:B :A)",
    );
    let q = Entailment::EquivalentClasses {
        a: "http://t/A".into(),
        b: "http://t/B".into(),
    };
    let j = find_one_justification(&o, &q).unwrap().expect("A≡B");
    assert_eq!(
        j.axioms.len(),
        2,
        "both SubClassOf needed; got {:?}",
        dbgset(&j)
    );
}

#[test]
fn find_one_disjoint() {
    // DisjointClasses(A,B) entailment justification = {DisjointClasses(A,B)};
    // the C⊑A,C⊑B noise must be excluded.
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                  DisjointClasses(:A :B) SubClassOf(:C :A) SubClassOf(:C :B)",
    );
    let q = Entailment::DisjointClasses {
        a: "http://t/A".into(),
        b: "http://t/B".into(),
    };
    let j = find_one_justification(&o, &q)
        .unwrap()
        .expect("A,B disjoint");
    assert_eq!(
        j.axioms.len(),
        1,
        "only DisjointClasses(A,B); got {:?}",
        dbgset(&j)
    );
}

#[test]
fn find_one_instance_of() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))\n\
                  SubClassOf(:A :B) ClassAssertion(:A :x)",
    );
    let q = Entailment::InstanceOf {
        individual: "http://t/x".into(),
        class: "http://t/B".into(),
    };
    let j = find_one_justification(&o, &q).unwrap().expect("x:B");
    assert_eq!(
        j.axioms.len(),
        2,
        "ClassAssertion(A,x) + A⊑B; got {:?}",
        dbgset(&j)
    );
}

#[test]
fn find_one_inconsistent() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(NamedIndividual(:x))\n\
                  SubClassOf(:A <http://www.w3.org/2002/07/owl#Nothing>) ClassAssertion(:A :x)",
    );
    let q = Entailment::Inconsistent;
    let j = find_one_justification(&o, &q)
        .unwrap()
        .expect("inconsistent");
    assert_eq!(j.axioms.len(), 2, "A⊑⊥ + A(x); got {:?}", dbgset(&j));
}

#[test]
fn sroiq_flags_minimality_not_guaranteed() {
    // Disjunction ⇒ out of EL/Horn ⇒ minimal_guaranteed = false.
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                  SubClassOf(:A ObjectUnionOf(:B :C)) SubClassOf(:B :C) SubClassOf(:C :B)",
    );
    let q = Entailment::SubClassOf {
        sub: "http://t/A".into(),
        sup: "http://t/B".into(),
    };
    if let Some(j) = find_one_justification(&o, &q).unwrap() {
        assert!(
            !j.minimal_guaranteed,
            "disjunction ⇒ out-of-fragment ⇒ minimality not guaranteed (fragment={:?})",
            j.fragment
        );
    }
}

#[test]
fn find_all_two_independent_derivations() {
    // A⊑C via A⊑B,B⊑C AND via A⊑D,D⊑C → two minimal justifications.
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
                  SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:A :D) SubClassOf(:D :C)",
    );
    let q = Entailment::SubClassOf {
        sub: "http://t/A".into(),
        sup: "http://t/C".into(),
    };
    let all = find_all_justifications(&o, &q, 10).unwrap();
    assert_eq!(
        all.len(),
        2,
        "two independent minimal justifications; got {}",
        all.len()
    );
    let (fixed, _) = owl_dl_reasoner::justify::logical_axioms(&o);
    for j in &all {
        assert_eq!(j.axioms.len(), 2, "each justification is 2 axioms");
        assert!(
            entails(
                &owl_dl_reasoner::justify::ontology_from(&fixed, &j.axioms),
                &q
            )
            .unwrap(),
            "each justification re-entails"
        );
    }
}

#[test]
fn find_all_respects_cap() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
                  SubClassOf(:A :B) SubClassOf(:B :C) SubClassOf(:A :D) SubClassOf(:D :C)",
    );
    let q = Entailment::SubClassOf {
        sub: "http://t/A".into(),
        sup: "http://t/C".into(),
    };
    assert_eq!(
        find_all_justifications(&o, &q, 1).unwrap().len(),
        1,
        "cap=1"
    );
}

#[test]
fn find_all_not_entailed_is_empty() {
    let o = onto("Declaration(Class(:A)) Declaration(Class(:B))");
    let q = Entailment::SubClassOf {
        sub: "http://t/A".into(),
        sup: "http://t/B".into(),
    };
    assert!(find_all_justifications(&o, &q, 10).unwrap().is_empty());
}
