#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    class_expression_entailed_subclass, class_expression_instances, class_expression_satisfiable,
};
use std::io::Cursor;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}
fn cls(b: &Build<RcStr>, iri: &str) -> ClassExpression<RcStr> {
    ClassExpression::Class(b.class(iri))
}

const TBOX: &str = r"Prefix(:=<http://ex/#>)
  Ontology(<http://ex/>
    Declaration(Class(:A)) Declaration(Class(:B))
    Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y))
    ClassAssertion(:A :x) ClassAssertion(:B :y))";

#[test]
fn ce_satisfiable_and_unsatisfiable() {
    let o = onto(TBOX);
    let b = Build::<RcStr>::new();
    // A ⊔ B satisfiable
    let union =
        ClassExpression::ObjectUnionOf(vec![cls(&b, "http://ex/#A"), cls(&b, "http://ex/#B")]);
    assert!(class_expression_satisfiable(&o, &union).unwrap().holds());
    // A ⊓ ¬A unsatisfiable
    let contradiction = ClassExpression::ObjectIntersectionOf(vec![
        cls(&b, "http://ex/#A"),
        ClassExpression::ObjectComplementOf(Box::new(cls(&b, "http://ex/#A"))),
    ]);
    assert!(
        !class_expression_satisfiable(&o, &contradiction)
            .unwrap()
            .holds()
    );
}

#[test]
fn ce_entailed_subclass_positive_and_negative() {
    let o = onto(TBOX);
    let b = Build::<RcStr>::new();
    let a_and_b = ClassExpression::ObjectIntersectionOf(vec![
        cls(&b, "http://ex/#A"),
        cls(&b, "http://ex/#B"),
    ]);
    // A ⊓ B ⊑ A  (entailed)
    assert!(
        class_expression_entailed_subclass(&o, &a_and_b, &cls(&b, "http://ex/#A"))
            .unwrap()
            .holds()
    );
    // A ⊑ B  (NOT entailed)
    assert!(
        !class_expression_entailed_subclass(&o, &cls(&b, "http://ex/#A"), &cls(&b, "http://ex/#B"))
            .unwrap()
            .holds()
    );
}

#[test]
fn ce_instances_of_union() {
    let o = onto(TBOX);
    let b = Build::<RcStr>::new();
    let union =
        ClassExpression::ObjectUnionOf(vec![cls(&b, "http://ex/#A"), cls(&b, "http://ex/#B")]);
    let inds = class_expression_instances(&o, &union).unwrap();
    let set: std::collections::HashSet<&str> =
        inds.individuals().iter().map(String::as_str).collect();
    assert!(set.contains("http://ex/#x")); // x:A ⇒ x:(A⊔B)
    assert!(set.contains("http://ex/#y")); // y:B ⇒ y:(A⊔B)
    // the synthetic probe IRI must NOT leak into instances:
    assert!(
        !inds
            .individuals()
            .iter()
            .any(|i| i.starts_with("urn:rustdl-ce-probe"))
    );
}

#[test]
fn ce_probe_iri_collision_errors() {
    // An ontology that already declares the probe IRI as a class ⇒ error, not silent overwrite.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
      Ontology(<http://ex/> Declaration(Class(<urn:rustdl-ce-probe:q>)) Declaration(Class(:A)))",
    );
    let b = Build::<RcStr>::new();
    assert!(class_expression_satisfiable(&o, &cls(&b, "http://ex/#A")).is_err());
}
