//! Integration tests for `diagnose`: cascade fixture, inconsistency, conservation.

use horned_owl::model::{Build, MutableOntology};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::diagnose;

type Rc = std::rc::Rc<str>;

fn b() -> Build<Rc> {
    Build::new_rc()
}

// Root = Bad (Bad ⊑ A ⊓ ¬A); Derived = SubBad (SubBad ⊑ Bad).
#[test]
fn root_and_derived_cascade() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    // Bad ⊑ A ⊓ ¬A  → Bad unsat (a root: depends on no other unsat class)
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:Bad")),
        sup: CE::ObjectIntersectionOf(vec![
            CE::Class(b.class("urn:A")),
            CE::ObjectComplementOf(Box::new(CE::Class(b.class("urn:A")))),
        ]),
    });
    // SubBad ⊑ Bad  → SubBad unsat (derived from Bad)
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:SubBad")),
        sup: CE::Class(b.class("urn:Bad")),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(d.consistent, "ontology is consistent (no ABox clash)");
    assert_eq!(d.roots, vec!["urn:Bad".to_string()]);
    assert_eq!(d.derived.len(), 1);
    assert_eq!(d.derived[0].iri, "urn:SubBad");
    assert_eq!(d.derived[0].roots, vec!["urn:Bad".to_string()]);
    // conservation
    let mut union: std::collections::BTreeSet<String> = d.roots.iter().cloned().collect();
    union.extend(d.derived.iter().map(|x| x.iri.clone()));
    let all: std::collections::BTreeSet<String> = d.all_unsat.iter().cloned().collect();
    assert_eq!(union, all);
}
