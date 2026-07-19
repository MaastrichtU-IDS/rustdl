//! Lever A (2026-07-20): the classification pairwise subsumption loop skips the
//! `ABox` seed when the ontology uses NO nominals (the `ABox` is then irrelevant
//! to class subsumption), avoiding the completion-graph blow-up it causes on
//! near-EL `ABox`-bearing ontologies. These canaries guard the two soundness
//! obligations: (a) when nominals ARE present, the `ABox` is KEPT (a
//! nominal+`ABox`-dependent subsumption must still be derived); (b) when
//! nominals are absent, dropping the `ABox` does not change the class hierarchy
//! and individuals never leak into it. Gated by `RUSTDL_CLASSIFY_TBOX_ONLY`
//! (default ON).
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

// (1) SOUNDNESS: a nominal (`ObjectHasValue`) + `ClassAssertion` makes a class
// subsumption depend on the ABox. Lever A MUST detect the nominal and keep the
// ABox, so the subsumption is still derived. If nominal-detection regressed,
// the ABox would be dropped and `HasPetA ⊑ PetOwner` would be MISSED.
#[test]
fn nominal_dependent_subsumption_preserved() {
    let o = onto(
        "Declaration(Class(:Pet)) Declaration(Class(:PetOwner)) Declaration(Class(:HasPetA))\n\
         Declaration(ObjectProperty(:hasPet)) Declaration(NamedIndividual(:a))\n\
         EquivalentClasses(:PetOwner ObjectSomeValuesFrom(:hasPet :Pet))\n\
         EquivalentClasses(:HasPetA ObjectHasValue(:hasPet :a))\n\
         ClassAssertion(:Pet :a)",
    );
    let c = classify(&o).expect("classify");
    assert!(
        c.is_subclass("http://e#HasPetA", "http://e#PetOwner"),
        "nominal+ABox subsumption HasPetA ⊑ PetOwner must be preserved (ABox kept when nominals present)"
    );
}

// (2) CORRECTNESS: a no-nominal ABox must not change the class hierarchy, and
// the ABox individuals must not leak into it. TBox is A⊑B⊑C; the ABox asserts
// individuals + property edges that are irrelevant to class subsumption.
#[test]
fn no_nominal_abox_hierarchy_unchanged() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         SubClassOf(:A :B) SubClassOf(:B :C)\n\
         Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:i1)) Declaration(NamedIndividual(:i2))\n\
         ClassAssertion(:A :i1) ObjectPropertyAssertion(:r :i1 :i2) ClassAssertion(:C :i2)",
    );
    let c = classify(&o).expect("classify");
    assert!(c.is_subclass("http://e#A", "http://e#B"), "A ⊑ B");
    assert!(c.is_subclass("http://e#B", "http://e#C"), "B ⊑ C");
    assert!(
        c.is_subclass("http://e#A", "http://e#C"),
        "A ⊑ C (transitive)"
    );
    // The ABox must not spuriously make unrelated classes subsume.
    assert!(!c.is_subclass("http://e#C", "http://e#A"), "C ⋢ A");
    assert!(!c.is_subclass("http://e#B", "http://e#A"), "B ⋢ A");
}
