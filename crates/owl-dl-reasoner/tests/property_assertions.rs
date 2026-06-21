//! Integration tests for `materialize_object_property_assertions`.

use horned_owl::model::{
    Build, DeclareClass, DeclareObjectProperty, MutableOntology, ObjectPropertyAssertion,
    ObjectPropertyExpression, SubObjectPropertyExpression, SubObjectPropertyOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_object_property_assertions;

type Rc = std::rc::Rc<str>;

fn opa(b: &Build<Rc>, prop: &str, s: &str, o: &str) -> ObjectPropertyAssertion<Rc> {
    ObjectPropertyAssertion {
        ope: ObjectPropertyExpression::ObjectProperty(b.object_property(prop)),
        from: horned_owl::model::Individual::Named(b.named_individual(s)),
        to: horned_owl::model::Individual::Named(b.named_individual(o)),
    }
}

#[test]
fn subproperty_entailed_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasAncestor")));
    for i in ["urn:a", "urn:b", "urn:c"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyExpression(
            ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasParent")),
        ),
        sup: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasAncestor")),
    });
    o.insert(opa(&b, "urn:hasParent", "urn:a", "urn:b"));
    o.insert(opa(&b, "urn:hasParent", "urn:b", "urn:c"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    let triple = |s: &str, p: &str, t: &str| (s.to_string(), p.to_string(), t.to_string());
    assert!(
        got.contains(&triple("urn:a", "urn:hasAncestor", "urn:b")),
        "got: {got:?}"
    );
    assert!(
        got.contains(&triple("urn:b", "urn:hasAncestor", "urn:c")),
        "got: {got:?}"
    );
    assert!(got.contains(&triple("urn:a", "urn:hasParent", "urn:b")));
    assert!(!got.contains(&triple("urn:a", "urn:hasAncestor", "urn:a")));
}

use owl_dl_reasoner::justify::{Entailment, entails};

#[test]
fn inverse_entailed_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasChild")));
    for i in ["urn:a", "urn:b"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(horned_owl::model::InverseObjectProperties(
        b.object_property("urn:hasChild"),
        b.object_property("urn:hasParent"),
    ));
    o.insert(opa(&b, "urn:hasParent", "urn:a", "urn:b"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    assert!(
        got.contains(&(
            "urn:b".to_string(),
            "urn:hasChild".to_string(),
            "urn:a".to_string()
        )),
        "got: {got:?}"
    );
}

#[test]
fn every_triple_is_entailed() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasAncestor")));
    for i in ["urn:a", "urn:b", "urn:c"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyExpression(
            ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasParent")),
        ),
        sup: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasAncestor")),
    });
    o.insert(opa(&b, "urn:hasParent", "urn:a", "urn:b"));
    o.insert(opa(&b, "urn:hasParent", "urn:b", "urn:c"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    assert!(!got.is_empty());
    for (s, p, t) in &got {
        let q = Entailment::ObjectPropertyAssertion {
            source: s.clone(),
            prop: p.clone(),
            target: t.clone(),
        };
        assert!(
            entails(&o, &q).expect("entails"),
            "{s} {p} {t} must be entailed"
        );
    }
}

#[test]
fn inconsistent_is_error() {
    use horned_owl::model::ClassExpression as CE;
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(horned_owl::model::DeclareNamedIndividual(
        b.named_individual("urn:i"),
    ));
    o.insert(horned_owl::model::DisjointClasses(vec![
        CE::Class(b.class("urn:A")),
        CE::Class(b.class("urn:B")),
    ]));
    o.insert(horned_owl::model::ClassAssertion {
        ce: CE::Class(b.class("urn:A")),
        i: b.named_individual("urn:i").into(),
    });
    o.insert(horned_owl::model::ClassAssertion {
        ce: CE::Class(b.class("urn:B")),
        i: b.named_individual("urn:i").into(),
    });

    assert!(materialize_object_property_assertions(&o).is_err());
}
