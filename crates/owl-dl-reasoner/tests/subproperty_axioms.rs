//! Integration tests for `materialize_subobjectproperty_axioms` / `materialize_subdataproperty_axioms`.

use horned_owl::model::{
    Build, DeclareDataProperty, DeclareObjectProperty, EquivalentObjectProperties,
    InverseObjectProperties, MutableOntology, ObjectPropertyExpression as OPE, SubDataPropertyOf,
    SubObjectPropertyExpression as SOPE, SubObjectPropertyOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{materialize_subdataproperty_axioms, materialize_subobjectproperty_axioms};

type Rc = std::rc::Rc<str>;

fn op(b: &Build<Rc>, iri: &str) -> OPE<Rc> {
    OPE::ObjectProperty(b.object_property(iri))
}
fn subop(b: &Build<Rc>, sub: &str, sup: &str) -> SubObjectPropertyOf<Rc> {
    SubObjectPropertyOf {
        sub: SOPE::ObjectPropertyExpression(op(b, sub)),
        sup: op(b, sup),
    }
}

#[test]
fn object_transitivity() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:p", "urn:q", "urn:r"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(subop(&b, "urn:p", "urn:q"));
    o.insert(subop(&b, "urn:q", "urn:r"));

    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    let t = |a: &str, c: &str| (a.to_string(), c.to_string());
    assert!(got.contains(&t("urn:p", "urn:r")), "got: {got:?}");
    assert!(got.contains(&t("urn:p", "urn:q")) && got.contains(&t("urn:q", "urn:r")));
    assert!(!got.iter().any(|(a, c)| a == c), "no reflexive pairs");
}

#[test]
fn object_inverse_propagation() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in [
        "urn:hasParent",
        "urn:hasAncestor",
        "urn:hasChild",
        "urn:hasDescendant",
    ] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(subop(&b, "urn:hasParent", "urn:hasAncestor"));
    o.insert(InverseObjectProperties(
        b.object_property("urn:hasParent"),
        b.object_property("urn:hasChild"),
    ));
    o.insert(InverseObjectProperties(
        b.object_property("urn:hasAncestor"),
        b.object_property("urn:hasDescendant"),
    ));

    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    assert!(
        got.contains(&("urn:hasChild".to_string(), "urn:hasDescendant".to_string())),
        "inverse propagation: got: {got:?}"
    );
}

#[test]
fn data_transitivity() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for d in ["urn:a", "urn:m", "urn:z"] {
        o.insert(DeclareDataProperty(b.data_property(d)));
    }
    o.insert(SubDataPropertyOf {
        sub: b.data_property("urn:a"),
        sup: b.data_property("urn:m"),
    });
    o.insert(SubDataPropertyOf {
        sub: b.data_property("urn:m"),
        sup: b.data_property("urn:z"),
    });

    let got = materialize_subdataproperty_axioms(&o).expect("materialize");
    assert!(
        got.contains(&("urn:a".to_string(), "urn:z".to_string())),
        "got: {got:?}"
    );
    assert!(!got.iter().any(|(a, c)| a == c));
}

use owl_dl_reasoner::justify::{Entailment, entails};

#[test]
fn object_equivalent_both_directions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:p", "urn:q"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(EquivalentObjectProperties(vec![
        op(&b, "urn:p"),
        op(&b, "urn:q"),
    ]));
    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    assert!(got.contains(&("urn:p".to_string(), "urn:q".to_string())));
    assert!(got.contains(&("urn:q".to_string(), "urn:p".to_string())));
}

#[test]
fn object_pairs_entailed() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:p", "urn:q", "urn:r"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    o.insert(subop(&b, "urn:p", "urn:q"));
    o.insert(subop(&b, "urn:q", "urn:r"));
    let got = materialize_subobjectproperty_axioms(&o).expect("materialize");
    assert!(!got.is_empty());
    for (s, t) in &got {
        let q = Entailment::SubObjectProperty {
            sub: s.clone(),
            sup: t.clone(),
        };
        assert!(
            entails(&o, &q).expect("entails"),
            "{s} ⊑ {t} must be entailed"
        );
    }
}

#[test]
fn inconsistent_is_error() {
    use horned_owl::model::{
        ClassAssertion, ClassExpression as CE, DeclareClass, DeclareNamedIndividual,
        DisjointClasses, Individual,
    };
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareNamedIndividual(b.named_individual("urn:i")));
    o.insert(DisjointClasses(vec![
        CE::Class(b.class("urn:A")),
        CE::Class(b.class("urn:B")),
    ]));
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:A")),
        i: Individual::Named(b.named_individual("urn:i")),
    });
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:B")),
        i: Individual::Named(b.named_individual("urn:i")),
    });

    assert!(materialize_subobjectproperty_axioms(&o).is_err());
    assert!(materialize_subdataproperty_axioms(&o).is_err());
}
