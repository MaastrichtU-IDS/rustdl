//! EL-completeness canary: `⊤ ⊑ C` (a named class equivalent to owl:Thing) must
//! propagate to `X ⊑ C` for EVERY named class X.
//!
//! Discovered by the whelk-rs EL sweep (2026-07-08): `ore_ont_11522` (textbook pure
//! EL — only `⊑` and `∃`) had 8 classes with `SubClassOf(owl:Thing, C)`; rustdl
//! derived 522 subsumptions vs whelk's complete 1490, missing all 968 Top-equivalent
//! subsumptions. The saturator's `lower_sub_class_of` had no `Top` arm on the sub
//! side, so `⊤ ⊑ C` was silently dropped — a genuine EL-incompleteness that also
//! violated the `completeness_guaranteed()` contract (PureEl + no timeout, yet
//! MISSED>0). Asserted at the SATURATION-CLOSURE layer (saturate() + contains).

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const ONT: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/o>
  Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:X)) Declaration(Class(:E))
  SubClassOf(<http://www.w3.org/2002/07/owl#Thing> :C)
  SubClassOf(:C :D)
  SubClassOf(:X :A)
)";

fn sat_sub(x: &str, y: &str) -> bool {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(ONT.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    let internal = convert_ontology(&o).expect("lower");
    let subsumers = owl_dl_saturation::saturate(&internal);
    let xid = internal
        .vocabulary
        .class_id(&format!("http://t/{x}"))
        .expect("x declared");
    let yid = internal
        .vocabulary
        .class_id(&format!("http://t/{y}"))
        .expect("y declared");
    subsumers.contains(xid, yid)
}

#[test]
fn top_subsumer_propagates_to_all_classes() {
    // ⊤ ⊑ C ⟹ every class ⊑ C.
    assert!(sat_sub("X", "C"), "⊤ ⊑ C ⟹ X ⊑ C");
    assert!(sat_sub("A", "C"), "⊤ ⊑ C ⟹ A ⊑ C");
    assert!(
        sat_sub("E", "C"),
        "⊤ ⊑ C ⟹ E ⊑ C (even a class with no other axioms)"
    );
    // Transitive: ⊤ ⊑ C ⊑ D ⟹ every class ⊑ D.
    assert!(sat_sub("X", "D"), "⊤ ⊑ C, C ⊑ D ⟹ X ⊑ D");
    assert!(sat_sub("E", "D"), "⊤ ⊑ C, C ⊑ D ⟹ E ⊑ D");
    // C itself ⊑ D (told).
    assert!(sat_sub("C", "D"), "C ⊑ D (told)");
}

#[test]
fn top_subsumer_no_spurious_subsumption() {
    // FP guard: the ⊤-broadcast must not make unrelated classes subsume each other.
    // A ⋢ X (A and X share no relationship other than both ⊑ the Top-equiv classes).
    assert!(!sat_sub("A", "X"), "A ⋢ X (no such subsumption)");
    assert!(!sat_sub("C", "A"), "C ⋢ A (C is Top-equiv but A is not)");
    assert!(!sat_sub("D", "A"), "D ⋢ A");
}
