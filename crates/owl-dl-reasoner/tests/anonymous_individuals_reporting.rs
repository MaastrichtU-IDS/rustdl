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
    let o = onto(
        "Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x)\n\
         ObjectPropertyAssertion(:r _:x :a)",
    );
    let rows = materialize_object_property_assertions(&o).expect("materialize");
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
