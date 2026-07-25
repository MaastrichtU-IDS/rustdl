#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_data_property_hierarchy, classify_object_property_hierarchy};
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
fn object_property_direct_and_equiv() {
    // r ⊑ s, s ⊑ t (⇒ direct r⊑s, s⊑t; r⊑t is transitive, NOT direct);
    // p ≡ q (equiv group).
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
            Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:p))
            Declaration(ObjectProperty(:q))
            SubObjectPropertyOf(:r :s) SubObjectPropertyOf(:s :t)
            EquivalentObjectProperties(:p :q))",
    );
    let h = classify_object_property_hierarchy(&o).unwrap();
    let direct: Vec<(&str, &str)> = h
        .direct_subsumptions()
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert!(direct.contains(&("http://ex/#r", "http://ex/#s")));
    assert!(direct.contains(&("http://ex/#s", "http://ex/#t")));
    assert!(
        !direct.contains(&("http://ex/#r", "http://ex/#t")),
        "transitive edge must not be direct"
    );
    assert!(h.equivalent_groups().iter().any(|g| {
        g.contains(&"http://ex/#p".to_string()) && g.contains(&"http://ex/#q".to_string())
    }));
}

/// PR #50 review Fix 5 (Minor): `classify_object_property_hierarchy` must
/// error, not silently return a (vacuous) hierarchy, on an inconsistent KB —
/// mirroring `disjoint_oracle.rs::disjoint_classes_errors_on_inconsistent` /
/// `individuals_oracle.rs::same_individuals_errors_on_inconsistent`.
#[test]
fn object_property_hierarchy_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        classify_object_property_hierarchy(&o),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

/// PR #50 review Fix 5 (Minor): `classify_data_property_hierarchy` — the
/// sibling of the object-property test above, and previously never called
/// in any test in this crate at all (test-hole, not just the inconsistent
/// guard). Same inconsistent-KB guard, exercised end to end.
#[test]
fn data_property_direct_and_equiv() {
    // r ⊑ s, s ⊑ t (⇒ direct r⊑s, s⊑t; r⊑t is transitive, NOT direct);
    // p ≡ q (equiv group).
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(DataProperty(:r)) Declaration(DataProperty(:s))
            Declaration(DataProperty(:t)) Declaration(DataProperty(:p))
            Declaration(DataProperty(:q))
            SubDataPropertyOf(:r :s) SubDataPropertyOf(:s :t)
            EquivalentDataProperties(:p :q))",
    );
    let h = classify_data_property_hierarchy(&o).unwrap();
    let direct: Vec<(&str, &str)> = h
        .direct_subsumptions()
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert!(direct.contains(&("http://ex/#r", "http://ex/#s")));
    assert!(direct.contains(&("http://ex/#s", "http://ex/#t")));
    assert!(
        !direct.contains(&("http://ex/#r", "http://ex/#t")),
        "transitive edge must not be direct"
    );
    assert!(h.equivalent_groups().iter().any(|g| {
        g.contains(&"http://ex/#p".to_string()) && g.contains(&"http://ex/#q".to_string())
    }));
}

#[test]
fn data_property_hierarchy_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        classify_data_property_hierarchy(&o),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}
