//! #125 — `ObjectPropertyDomain(r⁻, C)` and `ObjectPropertyRange(r⁻, C)` must be
//! lowered, not silently discarded.
//!
//! Both arms of `collect_el_rules` did nothing at all on an inverse role, under
//! the comment *"inverse-role domain = forward range; handled by the tableau"*.
//! The identity was right and the conclusion was false comfort for `classify`:
//! the pair was SILENTLY missed — `direct_subsumptions: []` with
//! `incomplete: false` and `dropped: {}`, while `subclass` answered `yes` and both
//! oracles derived it. D10 shape.
//!
//! **The fix is a TABLE FLIP, not a new rule.** `Domain(r⁻, C) ≡ Range(r, C)` and
//! `Range(r⁻, C) ≡ Domain(r, C)` are logical identities, and `Role::role_id`
//! already returns the underlying `r` for both polarities, so the only thing an
//! inverse changes is which table receives the classes.
//!
//! **DIRECTION OF RISK: flipping the WRONG WAY is a false positive**, because it
//! would assert of a role's sources something only true of its targets. That is
//! what `an_inverse_domain_does_not_constrain_the_forward_sources` guards, and it
//! is the test that would fail first if the polarity were inverted.
//!
//! ORACLE: `HermiT` 1.4.3 agrees on all five probes — it derives both positives,
//! refuses the FP guard, and reports `X` unsatisfiable for both `⊥` cases.
//!
//! **READ ROBOT'S ERROR STREAM, NOT JUST ITS OUTPUT FILE.** On an ontology with an
//! unsatisfiable class, `robot reason` writes NO output file and reports the
//! unsatisfiability on stderr. A check that greps the output file scores that as
//! "the oracle did not confirm it" — which is what the first version of this
//! adjudication did, nearly recording a disagreement with `HermiT` that does not
//! exist. The negative control (drop the `⊥` axiom ⇒ 0 unsatisfiable, `rustdl`
//! says `sat`) is what makes the positive readings meaningful.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

const DECLS: &str = "Declaration(Class(:X)) Declaration(Class(:B)) \
     Declaration(Class(:P)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))";
/// `X` is an `r`-SOURCE. Nothing here makes it an `r`-target.
const SRC: &str = "SubClassOf(:X ObjectSomeValuesFrom(:r :B))";

fn classify(body: &str) -> owl_dl_reasoner::Classification {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex.org/o>\n{DECLS}\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify")
}

fn holds(body: &str, sub: &str, sup: &str) -> bool {
    classify(body).is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

/// `Domain(r⁻, P) ≡ Range(r, P)`, so `X`'s `r`-witness is a `B ⊓ P` and
/// `∃r.(B ⊓ P) ⊑ D` fires. `HermiT` and Konclude both derive `X ⊑ D`; before #125
/// `classify` returned `[]` with `incomplete: false`.
#[test]
fn an_inverse_domain_is_the_forward_range() {
    let body = format!(
        "{SRC}\n\
         ObjectPropertyDomain(ObjectInverseOf(:r) :P)\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(
        holds(&body, "X", "D"),
        "Domain(r⁻,P) must act as Range(r,P)"
    );
}

/// The mirror identity: `Range(r⁻, P) ≡ Domain(r, P)`, so every `r`-SOURCE is a
/// `P`, and `X` is an `r`-source.
#[test]
fn an_inverse_range_is_the_forward_domain() {
    let body = format!("{SRC}\nObjectPropertyRange(ObjectInverseOf(:r) :P)");
    assert!(
        holds(&body, "X", "P"),
        "Range(r⁻,P) must act as Domain(r,P)"
    );
}

/// THE FP GUARD — the test that fails first if the polarity flip is inverted.
///
/// `Domain(r⁻, P)` constrains `r`'s TARGETS. `X` is a `r`-SOURCE and nothing says
/// it is a target, so `X ⊑ P` (and hence `X ⊑ D`) must NOT be derived. Treating
/// the axiom as `Domain(r, P)` — i.e. forgetting the flip — makes both fire.
/// `HermiT` derives neither.
#[test]
fn an_inverse_domain_does_not_constrain_the_forward_sources() {
    let body = format!("{SRC}\nObjectPropertyDomain(ObjectInverseOf(:r) :P)\nSubClassOf(:P :D)");
    assert!(
        !holds(&body, "X", "P"),
        "an r-SOURCE is not constrained by Range(r,·)"
    );
    assert!(
        !holds(&body, "X", "D"),
        "and so must not inherit P's subsumers"
    );
}

/// The other direction of the same mistake: `Range(r⁻, P)` constrains `r`'s
/// SOURCES, so it must not be folded into the witness as a forward range. `X`'s
/// `r`-successor is a `B`, not a `B ⊓ P`, so `∃r.(B ⊓ P) ⊑ D` must not fire.
#[test]
fn an_inverse_range_does_not_constrain_the_forward_targets() {
    let body = format!(
        "{SRC}\n\
         ObjectPropertyRange(ObjectInverseOf(:r) :P)\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(
        !holds(&body, "X", "D"),
        "Range(r⁻,·) must not reach the r-witness"
    );
}

/// `⊥` poisons the role in either polarity and on either side: `Domain(r⁻, ⊥)` is
/// `Range(r, ⊥)`, and both say no `r`-edge can exist in any model, so anything
/// asserting `∃r.B` is empty. `HermiT` reports `X` unsatisfiable for both.
#[test]
fn an_inverse_bot_filler_poisons_the_role_on_either_side() {
    for ax in [
        "ObjectPropertyDomain(ObjectInverseOf(:r) owl:Nothing)",
        "ObjectPropertyRange(ObjectInverseOf(:r) owl:Nothing)",
    ] {
        let c = classify(&format!("{SRC}\n{ax}"));
        assert!(
            c.unsatisfiable_classes().contains(&"http://ex.org/X"),
            "{ax} must poison r, making X unsatisfiable"
        );
    }
}

/// DISCRIMINATING CONTROL for the test above: without the `⊥` axiom, `X` is
/// satisfiable. `HermiT` agrees (0 unsatisfiable classes). Without this the
/// poisoning assertions could be passing for an unrelated reason.
#[test]
fn without_the_bot_axiom_the_source_class_stays_satisfiable() {
    let c = classify(SRC);
    assert!(
        !c.unsatisfiable_classes().contains(&"http://ex.org/X"),
        "X must stay satisfiable when nothing poisons r"
    );
}

/// The non-inverse behaviour is unchanged. Kept because the fix rewrote BOTH arms
/// into one polarity-aware helper, so a mistake in the shared path would break the
/// ordinary case too — and that is the case every other test in this crate leans on.
#[test]
fn the_forward_polarity_still_works() {
    assert!(
        holds(&format!("{SRC}\nObjectPropertyDomain(:r :P)"), "X", "P"),
        "Domain(r,P): every r-source is a P"
    );
    let range_body = format!(
        "{SRC}\n\
         ObjectPropertyRange(:r :P)\n\
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :D)"
    );
    assert!(
        holds(&range_body, "X", "D"),
        "Range(r,P) must reach the r-witness"
    );
}
