//! Completeness guard for `RUSTDL_CARD_DISJUNCT_ATOMS` (default ON): the
//! sufficient (⇐) direction of a defined class with a cardinality conjunct must
//! still derive subsumptions INTO it, while the clausification fix removes the
//! spurious over-branching. Checked-in stand-in for the pizza-parity property
//! (`AmericanHot ⊑ InterestingPizza` etc.), since pizza is a gitignored corpus
//! fixture. See `docs/superpowers/specs/2026-07-16-cardinality-disjunct-atoms-design.md`.
#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// D ≡ A ⊓ =1 r.B (the AcylGroup/CarbonylGroup/InterestingPizza shape). E's
// necessary conditions ⊇ D's definition ⟹ E ⊑ D (a ⇐-direction subsumption).
// F ⊑ A only (no =1 r.B) ⟹ F ⋢ D (the FP guard — the fix must not manufacture
// this from the evaluatable AtMost/AtLeast disjuncts).
const SRC: &str = r"Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))
Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
EquivalentClasses(:D ObjectIntersectionOf(:A ObjectExactCardinality(1 :r :B)))
EquivalentClasses(:E ObjectIntersectionOf(:A ObjectExactCardinality(1 :r :B) ObjectSomeValuesFrom(:s :G)))
SubClassOf(:F :A)
)
";

fn load(src: &str) -> SetOntology<RcStr> {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    o
}

#[test]
fn defined_class_cardinality_subsumption_preserved_and_sound() {
    // Default (fix ON): the ⇐-direction subsumption survives the clausification
    // change (evaluatable AtMost/AtLeast disjuncts, not opaque Q).
    let c = owl_dl_reasoner::classify(&load(SRC)).expect("classify");
    assert!(
        c.is_subclass("http://e#E", "http://e#D"),
        "E ⊑ D must hold: E ⊑ A ⊓ =1 r.B = D (defined-class sufficient direction)"
    );
    // FP guard: F ⊑ A but has no =1 r.B ⟹ F ⋢ D. The evaluatable
    // AtMost(r,B,0)/AtLeast(r,B,2) disjuncts of ¬(=1 r.B) must not manufacture
    // a spurious F ⊑ D.
    assert!(
        !c.is_subclass("http://e#F", "http://e#D"),
        "F ⋢ D — F lacks the =1 r.B conjunct; the fix must stay sound (FP=0)"
    );
}
