//! #110 — a conjunctive `ObjectPropertyDomain` / `ObjectPropertyRange` filler must
//! not be dropped.
//!
//! `ObjectPropertyDomain(r, P ⊓ Q)` is `∃r.⊤ ⊑ P ⊓ Q`, which is exactly
//! `∃r.⊤ ⊑ P` and `∃r.⊤ ⊑ Q` — a logical identity, so splitting it is sound and
//! completeness-preserving by construction. The same holds on the range side.
//!
//! Before this, the saturator accepted only an ATOMIC filler and silently dropped a
//! conjunctive one: `classify` returned zero rows with **`incomplete: false`** and
//! `dropped: {}`, under a banner certifying the fragment complete — the D10 shape,
//! where the answer is wrong *and* reported complete. Both peers derive the pairs
//! (`Konclude` v0.7.0-1138 and `HermiT` 1.4.3, measured), and rustdl's own
//! `subclass` proved them, so only `classify` was affected.
//!
//! The atomic controls are what show the cause is the CONJUNCTIVE filler rather
//! than the domain/range mechanism: they derived their pair correctly throughout.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

fn holds(body: &str, sub: &str, sup: &str) -> bool {
    let ofn = format!(
        "Prefix(:=<http://rustdl.test/>)\nOntology(\n\
         Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X))\n\
         Declaration(Class(:B)) Declaration(Class(:D)) Declaration(Class(:Z))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         {body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    result.is_subclass(
        &format!("http://rustdl.test/{sub}"),
        &format!("http://rustdl.test/{sup}"),
    )
}

const EXISTS: &str = "SubClassOf(:X ObjectSomeValuesFrom(:r :B))";

#[test]
fn a_conjunctive_domain_yields_every_conjunct() {
    let body = format!("{EXISTS}\nObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))");
    assert!(holds(&body, "X", "P"), "X ⊑ P (Konclude and HermiT agree)");
    assert!(holds(&body, "X", "Q"), "X ⊑ Q (Konclude and HermiT agree)");
}

#[test]
fn an_atomic_domain_still_works() {
    // The discriminating control: this derived correctly even before #110, which is
    // what isolates the conjunctive filler as the cause.
    let body = format!("{EXISTS}\nObjectPropertyDomain(:r :P)");
    assert!(holds(&body, "X", "P"));
}

#[test]
fn a_conjunctive_range_yields_every_conjunct() {
    // Measured to have the identical defect: `HermiT` derives `X ⊑ D`, rustdl
    // reported `[]` with `incomplete: false`.
    let body = format!(
        "{EXISTS}\nObjectPropertyRange(:r ObjectIntersectionOf(:P :Q))\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(holds(&body, "X", "D"), "X ⊑ D (HermiT agrees)");
}

#[test]
fn an_atomic_range_still_works() {
    let body = format!(
        "{EXISTS}\nObjectPropertyRange(:r :P)\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(holds(&body, "X", "D"));
}

#[test]
fn a_nested_conjunction_is_flattened() {
    // `P ⊓ (Q ⊓ Z)` — the recursion, not just one level.
    let body = format!(
        "{EXISTS}\n\
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectIntersectionOf(:Q :Z)))"
    );
    assert!(holds(&body, "X", "P"));
    assert!(holds(&body, "X", "Q"));
    assert!(holds(&body, "X", "Z"));
}

#[test]
fn a_filler_mixing_atomic_and_non_atomic_parts_still_drops_whole() {
    // ALL-OR-NOTHING, on purpose. Taking the atomic half would be sound in
    // isolation (`⊑ P ⊓ Z` entails `⊑ P`) but would put the ENGINE ahead of the
    // fragment GATE, which decides membership separately — the gate would keep
    // calling this out-of-fragment while the engine acted on part of it. That is
    // the D10 shape this fix exists to close, so it must not be re-created here.
    //
    // If a future change teaches the gate about conjunctive fillers, this becomes
    // the wrong assertion and should be FLIPPED rather than deleted.
    let body = format!(
        "{EXISTS}\n\
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :Z)))"
    );
    assert!(
        !holds(&body, "X", "P"),
        "a mixed filler must drop WHOLE, keeping engine and fragment gate in agreement"
    );
}
