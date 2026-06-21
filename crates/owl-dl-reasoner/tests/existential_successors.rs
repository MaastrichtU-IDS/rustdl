//! Integration tests for materialize_existential_successors.
#![allow(clippy::doc_markdown, clippy::many_single_char_names)]

use horned_owl::model::{
    Build, ClassAssertion, ClassExpression as CE, DeclareClass, DeclareNamedIndividual,
    DeclareObjectProperty, Individual, MutableOntology, ObjectPropertyExpression as OPE,
    SubClassOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_existential_successors;

type Rc = std::rc::Rc<str>;

fn some(b: &Build<Rc>, r: &str, c: &str) -> CE<Rc> {
    CE::ObjectSomeValuesFrom {
        ope: OPE::ObjectProperty(b.object_property(r)),
        bce: Box::new(CE::Class(b.class(c))),
    }
}

// Person ⊑ ∃hasParent.Person ; a : Person
//   → exactly one row (a, hasParent, _:b, Person); blank id is fresh; 1-step (no
//     row for the witness itself).
#[test]
fn one_step_existential_successor() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareClass(b.class("urn:Person")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf {
        sub: CE::Class(b.class("urn:Person")),
        sup: some(&b, "urn:hasParent", "urn:Person"),
    });
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:Person")),
        i: Individual::Named(b.named_individual("urn:a")),
    });

    let got = materialize_existential_successors(&o).expect("materialize");
    assert_eq!(
        got.len(),
        1,
        "exactly one existential successor; got: {got:?}"
    );
    let (s, p, w, c) = &got[0];
    assert_eq!(s, "urn:a");
    assert_eq!(p, "urn:hasParent");
    assert_eq!(c, "urn:Person");
    assert!(w.starts_with("_:"), "witness must be a blank node, got {w}");
    assert_ne!(w, "urn:a", "witness is not a named individual");
}

// Determinism: two calls give identical output.
#[test]
fn deterministic() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareClass(b.class("urn:Person")));
    o.insert(DeclareObjectProperty(b.object_property("urn:hasParent")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf {
        sub: CE::Class(b.class("urn:Person")),
        sup: some(&b, "urn:hasParent", "urn:Person"),
    });
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:Person")),
        i: Individual::Named(b.named_individual("urn:a")),
    });

    let a = materialize_existential_successors(&o).expect("m1");
    let b2 = materialize_existential_successors(&o).expect("m2");
    assert_eq!(a, b2);
}

use horned_owl::model::{ClassExpression, DisjointClasses, EquivalentClasses};

// a:Y, Y⊑X, X⊑∃r.C ⇒ row present (uses entailed types, not just asserted).
#[test]
fn entailed_not_asserted_type() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:Y", "urn:C"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf {
        sub: CE::Class(b.class("urn:Y")),
        sup: CE::Class(b.class("urn:X")),
    });
    o.insert(SubClassOf {
        sub: CE::Class(b.class("urn:X")),
        sup: some(&b, "urn:r", "urn:C"),
    });
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:Y")),
        i: Individual::Named(b.named_individual("urn:a")),
    });

    let got = materialize_existential_successors(&o).expect("materialize");
    assert!(
        got.iter()
            .any(|(s, p, _, c)| s == "urn:a" && p == "urn:r" && c == "urn:C"),
        "got: {got:?}"
    );
}

// Two distinct existentials ⇒ two rows, two distinct blanks.
#[test]
fn distinct_existentials_distinct_blanks() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:C", "urn:D"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(SubClassOf {
        sub: CE::Class(b.class("urn:X")),
        sup: ClassExpression::ObjectIntersectionOf(vec![
            some(&b, "urn:r", "urn:C"),
            some(&b, "urn:r", "urn:D"),
        ]),
    });
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:X")),
        i: Individual::Named(b.named_individual("urn:a")),
    });

    let got = materialize_existential_successors(&o).expect("materialize");
    let blanks: std::collections::BTreeSet<&String> = got.iter().map(|(_, _, w, _)| w).collect();
    assert_eq!(got.len(), 2, "got: {got:?}");
    assert_eq!(blanks.len(), 2, "distinct existentials → distinct blanks");
}

// No entailed existential ⇒ no rows.
#[test]
fn no_existential_no_rows() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareClass(b.class("urn:Person")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:Person")),
        i: Individual::Named(b.named_individual("urn:a")),
    });
    assert!(
        materialize_existential_successors(&o)
            .expect("materialize")
            .is_empty()
    );
}

// Inconsistent ⇒ Err.
#[test]
fn inconsistent_is_error() {
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
    let _ = EquivalentClasses::<Rc>; // keep import used if needed; remove if unused
    assert!(materialize_existential_successors(&o).is_err());
}
