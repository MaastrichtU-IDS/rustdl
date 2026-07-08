//! EL-completeness canary: `∃R.⊤` (ObjectSomeValuesFrom(R, owl:Thing)) on the RHS.
//!
//! `∃R.⊤` means "has some R-successor". A class `A ⊑ ∃R.⊤` must get an R-marker so
//! the domain rule (`∃R.⊤ ⊑ C` = domain(R)=C) fires — in particular two classes
//! defined `≡ ∃R.⊤` are equivalent. The saturator dropped `∃R.⊤` on the RHS (the
//! ⊤ filler has no atomic/Tseitin body), so `A ≡ ∃R.⊤`, `B ≡ ∃R.⊤` did not yield
//! `A ≡ B` — a real EL-incompleteness discovered by the whelk-rs EL sweep
//! (`ore_ont_7216`: rustdl 64990 vs whelk/Konclude 74374). Fix: emit a fact to an
//! opaque ⊤-witness for `∃R.⊤`. Asserted at the saturation-closure layer.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

fn sat_sub(ont: &str, x: &str, y: &str) -> bool {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(ont.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    let internal = convert_ontology(&o).expect("lower");
    let subsumers = owl_dl_saturation::saturate(&internal);
    let xid = internal.vocabulary.class_id(x).expect("x");
    let yid = internal.vocabulary.class_id(y).expect("y");
    subsumers.contains(xid, yid)
}

const EQ: &str = r"Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://t/o>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:U)) Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  EquivalentClasses(:A ObjectSomeValuesFrom(:r owl:Thing))
  EquivalentClasses(:B ObjectSomeValuesFrom(:r owl:Thing))
  EquivalentClasses(:U ObjectSomeValuesFrom(:s owl:Thing))
)";

#[test]
fn exists_top_same_role_classes_are_equivalent() {
    // A ≡ ∃r.⊤ and B ≡ ∃r.⊤ ⟹ A ≡ B.
    assert!(
        sat_sub(EQ, "http://t/A", "http://t/B"),
        "A ⊑ B (both ≡ ∃r.⊤)"
    );
    assert!(
        sat_sub(EQ, "http://t/B", "http://t/A"),
        "B ⊑ A (both ≡ ∃r.⊤)"
    );
}

#[test]
fn exists_top_different_role_no_subsumption() {
    // FP guard: U ≡ ∃s.⊤ is a DIFFERENT role — must not subsume/be-subsumed by A (∃r.⊤).
    assert!(
        !sat_sub(EQ, "http://t/A", "http://t/U"),
        "A ⋢ U (different roles r vs s)"
    );
    assert!(
        !sat_sub(EQ, "http://t/U", "http://t/A"),
        "U ⋢ A (different roles)"
    );
}

#[test]
fn exists_top_domain_inference() {
    // ∃r.⊤ ⊑ C (domain) + X ⊑ ∃r.⊤ ⟹ X ⊑ C.
    let ont = r"Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://t/o>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:Z)) Declaration(ObjectProperty(:r))
  SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) :C)
  SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))
)";
    assert!(
        sat_sub(ont, "http://t/X", "http://t/C"),
        "X ⊑ ∃r.⊤ ⊑ C (domain)"
    );
    // FP guard: Z with no r-successor must not be a C.
    assert!(!sat_sub(ont, "http://t/Z", "http://t/C"), "Z ⋢ C (no ∃r.⊤)");
}
