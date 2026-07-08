//! Canary for `Classification::completeness_guaranteed()` — the honest C2 contract.
//!
//! `completeness_guaranteed()` ⟹ `MISSED == 0`. It holds only on the
//! provably-complete fragments (PureEl / Horn) with no timed-out pairs. On
//! `OutOfFragment` inputs it returns `false` even when nothing times out, because
//! `trust_sat` can silently miss a subsumption on complement/disjunction structure
//! (the ORE-measured silent-miss hole — see
//! `docs/paper-calibration-decomposition-2026-07-08.md`). This test guards that the
//! flag is honest: `true` only when completeness is provable, never on a fragment
//! where a silent miss is possible.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

#[test]
fn pure_el_guarantees_completeness() {
    // A ⊑ B ⊑ C — pure EL, saturator complete.
    let c = classify(
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         SubClassOf(:A :B) SubClassOf(:B :C)\n)",
    );
    assert!(
        c.completeness_guaranteed(),
        "pure-EL: completeness must be guaranteed"
    );
}

#[test]
fn out_of_fragment_does_not_guarantee_completeness() {
    // A complement-defined class puts the ontology OutOfFragment: trust_sat may
    // silently miss, so completeness is NOT guaranteed even with nothing timed out.
    let c = classify(
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))\n\
         EquivalentClasses(:D ObjectIntersectionOf(:A ObjectComplementOf(:B)))\n\
         SubClassOf(:A :B)\n)",
    );
    assert!(
        !c.completeness_guaranteed(),
        "OutOfFragment: completeness must NOT be guaranteed (trust_sat may silently miss)"
    );
}
