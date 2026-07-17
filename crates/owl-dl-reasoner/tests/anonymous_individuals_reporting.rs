//! Anonymous individuals reason but are never reported on named-individual
//! output surfaces (decision (a) in the anon-individuals spec).
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::instances_of;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

#[test]
fn instances_of_excludes_anonymous_individuals() {
    // Named :a and anonymous _:x are both asserted to be :A.
    let o = onto(
        "Declaration(Class(:A)) Declaration(NamedIndividual(:a))\n\
         ClassAssertion(:A :a)\n\
         ClassAssertion(:A _:x)",
    );
    let members = instances_of(&o, "http://e#A").expect("instances_of");
    assert!(
        members.iter().any(|m| m == "http://e#a"),
        "named :a must be reported"
    );
    assert!(
        members.iter().all(|m| !m.starts_with("urn:rustdl-anon:")),
        "anonymous individuals must NOT appear in instances_of output: {members:?}"
    );
}

#[test]
fn materialize_object_property_excludes_anonymous_subjects_and_objects() {
    use owl_dl_reasoner::materialize_object_property_assertions;
    // Fixture has two anon-involving edges AND one named->named edge (:a -> :b).
    // The named edge must survive; the anon edges must be filtered out.
    let o = onto(
        "Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a))\n\
         Declaration(NamedIndividual(:b))\n\
         ObjectPropertyAssertion(:r :a _:x)\n\
         ObjectPropertyAssertion(:r _:x :a)\n\
         ObjectPropertyAssertion(:r :a :b)",
    );
    let rows = materialize_object_property_assertions(&o).expect("materialize");
    // The named->named edge must be present.
    assert!(
        rows.iter()
            .any(|(s, _, ob)| s == "http://e#a" && ob == "http://e#b"),
        "named edge :a -:r-> :b must survive: {rows:?}"
    );
    // No anon subject or object must appear.
    for (s, _p, ob) in &rows {
        assert!(
            !s.starts_with("urn:rustdl-anon:"),
            "anon subject leaked: {s}"
        );
        assert!(
            !ob.starts_with("urn:rustdl-anon:"),
            "anon object leaked: {ob}"
        );
    }
}

#[test]
fn realize_excludes_anonymous_individuals() {
    use owl_dl_reasoner::realize;
    // :a is a named individual; _:x is anonymous. Both are asserted to :A.
    // realize().individuals() must contain :a but no urn:rustdl-anon: IRI.
    let o = onto(
        "Declaration(Class(:A))\n\
         Declaration(NamedIndividual(:a))\n\
         ClassAssertion(:A :a)\n\
         ClassAssertion(:A _:x)",
    );
    let realization = realize(&o).expect("realize");
    let individuals = realization.individuals();
    assert!(
        individuals.iter().any(|i| i == "http://e#a"),
        "named individual :a must appear in realize output: {individuals:?}"
    );
    assert!(
        individuals
            .iter()
            .all(|i| !i.starts_with("urn:rustdl-anon:")),
        "anonymous individuals must NOT appear in realize().individuals(): {individuals:?}"
    );
}
