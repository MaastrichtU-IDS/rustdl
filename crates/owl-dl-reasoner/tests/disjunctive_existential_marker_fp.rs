//! Soundness canary (negatives-first) for the disjunctive-existential marker FP.
//!
//! Found 2026-06-20 via the ORE-2015 sweep (`ore_ont_7499`, Vaccine Ontology,
//! FP_strict=1060 — masked because the ontology DNF'd in prior runs and was never
//! diffed). The EL saturator lowered a *disjunctive* existential conjunct
//! `∃R.(A ⊔ B)` (here in `W`'s definition) by REUSING the singleton `∃R.A` marker
//! and then emitting `∃R.B ⊑ that-marker`. That marker is the same synthetic the
//! genuine `X ≡ ∃R.A ⊓ C` keys on, so a class carrying only `∃R.B` (here `Y`)
//! spuriously gained the `∃R.A` marker and the UNENTAILED subsumption `Y ⊑ X` was
//! derived ("answered by saturation"). The fix gives `∃R.(A⊔B)` a FRESH dedicated
//! union marker (`introduce_union_existential_marker`) disjoint from the singleton
//! `∃R.Ci` markers.
//!
//! Ontology (genus-differentia shape — pervasive in OBO):
//!   X ≡ ∃R.A ⊓ C
//!   Y ≡ ∃R.B ⊓ C            (A, B incomparable ⟹ X,Y are siblings, NOT ⊑)
//!   W ≡ ∃R.(A ⊔ B) ⊓ C2     (the trigger: the disjunctive existential conjunct)
//!
//! `Y ⊑ X` and `X ⊑ Y` are both UNENTAILED (each needs B⊑A / A⊑B, absent). We
//! assert neither is derived, AND that the genuine subsumptions still hold (the
//! union marker fix must not cost the real entailments — completeness control).

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const ONT: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/o>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:W))
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:C2))
  Declaration(ObjectProperty(:R))
  EquivalentClasses(:X ObjectIntersectionOf(ObjectSomeValuesFrom(:R :A) :C))
  EquivalentClasses(:Y ObjectIntersectionOf(ObjectSomeValuesFrom(:R :B) :C))
  EquivalentClasses(:W ObjectIntersectionOf(ObjectSomeValuesFrom(:R ObjectUnionOf(:A :B)) :C2))
)";

fn load() -> SetOntology<RcStr> {
    let mut r = Cursor::new(ONT.as_bytes().to_vec());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

fn sub(o: &SetOntology<RcStr>, s: &str, t: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(o, &format!("http://t/{s}"), &format!("http://t/{t}"))
        .expect("classify ok")
}

#[test]
fn disjunctive_existential_marker_no_fp() {
    let o = load();
    // The FP (and its mirror): NEITHER may be derived — both are unentailed.
    assert!(
        !sub(&o, "Y", "X"),
        "FP: Y ⊑ X is NOT entailed (needs B ⊑ A) — the disjunctive ∃R.(A⊔B) marker \
         must not pollute the singleton ∃R.A marker"
    );
    assert!(!sub(&o, "X", "Y"), "X ⊑ Y is NOT entailed (needs A ⊑ B)");
}

#[test]
fn disjunctive_existential_marker_preserves_completeness() {
    let o = load();
    // Genuine subsumptions must survive the fresh-marker fix.
    assert!(sub(&o, "X", "C"), "X ⊑ C holds (X ≡ ∃R.A ⊓ C)");
    assert!(sub(&o, "Y", "C"), "Y ⊑ C holds (Y ≡ ∃R.B ⊓ C)");
    assert!(sub(&o, "W", "C2"), "W ⊑ C2 holds (W ≡ ∃R.(A⊔B) ⊓ C2)");
}
