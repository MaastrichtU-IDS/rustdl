#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::disjoint_classes;
use std::io::Cursor;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

#[test]
fn disjoint_classes_inherits_through_subclass() {
    // A,B told disjoint; C⊑A, D⊑B ⇒ C,D entailed disjoint (not told).
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(Class(:C)) Declaration(Class(:D))
            DisjointClasses(:A :B) SubClassOf(:C :A) SubClassOf(:D :B))",
    );
    let r = disjoint_classes(&o, None).unwrap();
    let has = |x: &str, y: &str| {
        r.pairs()
            .iter()
            .any(|(a, b)| (a == x && b == y) || (a == y && b == x))
    };
    assert!(has("http://ex/#A", "http://ex/#B"), "told pair present");
    assert!(
        has("http://ex/#C", "http://ex/#D"),
        "inherited pair inferred: {:?}",
        r.pairs()
    );
}

#[test]
fn disjoint_classes_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        disjoint_classes(&o, None),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

#[test]
fn disjoint_object_properties_told() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
            DisjointObjectProperties(:p :q))",
    );
    let pairs = owl_dl_reasoner::disjoint_object_properties(&o).unwrap();
    assert_eq!(
        pairs,
        vec![("http://ex/#p".to_string(), "http://ex/#q".to_string())]
    );
}

#[test]
fn disjoint_data_properties_told() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
            DisjointDataProperties(:p :q))",
    );
    let pairs = owl_dl_reasoner::disjoint_data_properties(&o).unwrap();
    assert_eq!(
        pairs,
        vec![("http://ex/#p".to_string(), "http://ex/#q".to_string())]
    );
}
