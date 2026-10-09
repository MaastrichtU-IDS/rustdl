//! #192: with the wedge off there are no labels, so the label-driven same-tier
//! sweep never tested a same-tier subsumption whose super-class is PRIMITIVE.
//! The main tableau's per-class model labels are now used as candidates.
//!
//! In its own file because it sets `RUSTDL_HYPERTABLEAU=0`, which sibling
//! tests in one binary would observe.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn classify(axioms: &str) -> owl_dl_reasoner::Classification {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/r>\n\
         Declaration(Class(:A)) Declaration(Class(:A2)) Declaration(Class(:B))\n\
         Declaration(Class(:C)) Declaration(ObjectProperty(:r))\n{axioms}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

#[test]
#[allow(unsafe_code)]
fn tableau_labels_nominate_same_tier_primitive_sups_without_a_wedge() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has one test.
    unsafe { std::env::set_var("RUSTDL_HYPERTABLEAU", "0") };

    // #192's probe (Konclude, HermiT and `rustdl subclass` all derive
    // `A ⊑ B`): the functional reflexive `r` makes `A`'s `r`-successor `A`.
    let c = classify(
        "ReflexiveObjectProperty(:r) FunctionalObjectProperty(:r) \
         SubClassOf(:A ObjectSomeValuesFrom(:r :B)) \
         SubClassOf(:A2 ObjectIntersectionOf(ObjectSomeValuesFrom(:r :B) \
         ObjectComplementOf(:B)))",
    );
    assert!(c.is_subclass("http://ex.org/A", "http://ex.org/B"));
    assert!(c.unsatisfiable_classes().contains(&"http://ex.org/A2"));

    // FP guard: a model of `A ⊑ B ⊔ C` labels the root with ONE disjunct, so
    // `B` (or `C`) is a candidate that is NOT entailed. Verification must
    // reject it.
    let d = classify("SubClassOf(:A ObjectUnionOf(:B :C))");
    assert!(!d.is_subclass("http://ex.org/A", "http://ex.org/B"));
    assert!(!d.is_subclass("http://ex.org/A", "http://ex.org/C"));
}
