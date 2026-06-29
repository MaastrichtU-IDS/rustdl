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

/// Issue #26: a `TransitiveObjectProperty` must materialize the full transitive
/// closure of its asserted edges, not just the one-step edges.
#[test]
fn transitive_closure_materialized() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:ancestorOf")));
    o.insert(horned_owl::model::TransitiveObjectProperty(
        ObjectPropertyExpression::ObjectProperty(b.object_property("urn:ancestorOf")),
    ));
    for i in ["urn:a", "urn:b", "urn:c", "urn:d"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(opa(&b, "urn:ancestorOf", "urn:a", "urn:b"));
    o.insert(opa(&b, "urn:ancestorOf", "urn:b", "urn:c"));
    o.insert(opa(&b, "urn:ancestorOf", "urn:c", "urn:d"));

    let got = materialize_object_property_assertions(&o).expect("materialize");
    let t = |s: &str, x: &str| (s.to_string(), "urn:ancestorOf".to_string(), x.to_string());
    // one-step (already worked)
    assert!(got.contains(&t("urn:a", "urn:b")), "got: {got:?}");
    assert!(got.contains(&t("urn:b", "urn:c")), "got: {got:?}");
    assert!(got.contains(&t("urn:c", "urn:d")), "got: {got:?}");
    // transitive closure (the regression)
    assert!(got.contains(&t("urn:a", "urn:c")), "a⊑c missing: {got:?}");
    assert!(got.contains(&t("urn:b", "urn:d")), "b⊑d missing: {got:?}");
    assert!(got.contains(&t("urn:a", "urn:d")), "a⊑d missing: {got:?}");
}

/// Issue #26 larger case: sub-property + inverse + transitivity compose, so the
/// transitive edge through the sub-property/inverse rewrites is materialized.
#[test]
fn transitive_with_subproperty_and_inverse() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for p in ["urn:hasParent", "urn:hasChild", "urn:hasAncestor"] {
        o.insert(DeclareObjectProperty(b.object_property(p)));
    }
    for i in ["urn:alice", "urn:mary", "urn:john"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    // hasParent ⊑ hasAncestor ; hasAncestor transitive ; hasChild ≡ hasParent⁻
    o.insert(SubObjectPropertyOf {
        sub: SubObjectPropertyExpression::ObjectPropertyExpression(
            ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasParent")),
        ),
        sup: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasAncestor")),
    });
    o.insert(horned_owl::model::TransitiveObjectProperty(
        ObjectPropertyExpression::ObjectProperty(b.object_property("urn:hasAncestor")),
    ));
    o.insert(horned_owl::model::InverseObjectProperties(
        b.object_property("urn:hasChild"),
        b.object_property("urn:hasParent"),
    ));
    o.insert(opa(&b, "urn:hasChild", "urn:mary", "urn:alice")); // mary hasChild alice
    o.insert(opa(&b, "urn:hasParent", "urn:mary", "urn:john")); // mary hasParent john

    let got = materialize_object_property_assertions(&o).expect("materialize");
    let anc = |s: &str, x: &str| (s.to_string(), "urn:hasAncestor".to_string(), x.to_string());
    // alice hasParent mary (inverse) ⟹ alice hasAncestor mary (sub-property)
    assert!(got.contains(&anc("urn:alice", "urn:mary")), "got: {got:?}");
    // mary hasAncestor john (sub-property)
    assert!(got.contains(&anc("urn:mary", "urn:john")), "got: {got:?}");
    // the composed edge (the regression): alice hasAncestor john
    assert!(
        got.contains(&anc("urn:alice", "urn:john")),
        "composed alice⊑john missing: {got:?}"
    );
}

/// Issue #26 audit: symmetric roles must materialize the reverse edge.
#[test]
fn symmetric_entailed_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:knows")));
    o.insert(horned_owl::model::SymmetricObjectProperty(
        ObjectPropertyExpression::ObjectProperty(b.object_property("urn:knows")),
    ));
    for i in ["urn:a", "urn:b"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(opa(&b, "urn:knows", "urn:a", "urn:b"));
    let got = materialize_object_property_assertions(&o).expect("materialize");
    let t = |s: &str, x: &str| (s.to_string(), "urn:knows".to_string(), x.to_string());
    assert!(got.contains(&t("urn:a", "urn:b")), "got: {got:?}");
    assert!(
        got.contains(&t("urn:b", "urn:a")),
        "reverse missing: {got:?}"
    );
}

/// Issue #26 audit: `SameIndividual` must fold edges across the equivalence class.
#[test]
fn same_individual_edge_folding() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    for i in ["urn:a", "urn:a2", "urn:b"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(horned_owl::model::SameIndividual(vec![
        horned_owl::model::Individual::Named(b.named_individual("urn:a")),
        horned_owl::model::Individual::Named(b.named_individual("urn:a2")),
    ]));
    o.insert(opa(&b, "urn:r", "urn:a", "urn:b"));
    let got = materialize_object_property_assertions(&o).expect("materialize");
    let t = |s: &str, x: &str| (s.to_string(), "urn:r".to_string(), x.to_string());
    assert!(got.contains(&t("urn:a", "urn:b")), "got: {got:?}");
    assert!(
        got.contains(&t("urn:a2", "urn:b")),
        "same-individual fold missing: {got:?}"
    );
}

/// Issue #26 audit: `ObjectHasValue` (nominal filler) yields a ground edge, both
/// asserted directly and via a `C ⊑ ∃R.{b}` GCI on a typed individual.
#[test]
fn object_has_value_ground_edge() {
    use horned_owl::model::{ClassAssertion, ClassExpression, Individual, SubClassOf};
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    for i in ["urn:a", "urn:b", "urn:c"] {
        o.insert(horned_owl::model::DeclareNamedIndividual(
            b.named_individual(i),
        ));
    }
    o.insert(DeclareClass(b.class("urn:C")));
    // direct: a : ∃r.{b}
    o.insert(ClassAssertion {
        ce: ClassExpression::ObjectHasValue {
            ope: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:r")),
            i: Individual::Named(b.named_individual("urn:b")),
        },
        i: Individual::Named(b.named_individual("urn:a")),
    });
    // via GCI: C ⊑ ∃r.{b} and c : C  ⟹  r(c, b)
    o.insert(SubClassOf {
        sub: ClassExpression::Class(b.class("urn:C")),
        sup: ClassExpression::ObjectHasValue {
            ope: ObjectPropertyExpression::ObjectProperty(b.object_property("urn:r")),
            i: Individual::Named(b.named_individual("urn:b")),
        },
    });
    o.insert(ClassAssertion {
        ce: ClassExpression::Class(b.class("urn:C")),
        i: Individual::Named(b.named_individual("urn:c")),
    });
    let got = materialize_object_property_assertions(&o).expect("materialize");
    let t = |s: &str| (s.to_string(), "urn:r".to_string(), "urn:b".to_string());
    assert!(
        got.contains(&t("urn:a")),
        "direct HasValue missing: {got:?}"
    );
    assert!(got.contains(&t("urn:c")), "GCI HasValue missing: {got:?}");
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
