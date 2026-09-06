//! #114 — `SubClassOf(ObjectSomeValuesFrom(r, owl:Thing), C)` with a NON-ATOMIC `C`.
//!
//! That GCI is the spelling of `ObjectPropertyDomain(r, C)`. The saturator routed its
//! head through `atomic_operands_on_right`, which keeps only `Atomic` operands and
//! returns nothing for anything else — so `∃r.⊤ ⊑ ∃s.S` pushed NO domain entry and the
//! axiom vanished.
//!
//! # Why this was the worst shape in the D10 family
//!
//! `is_el_concept` admits `∃s.S`, so the ontology is certified **`pure-EL`** — the
//! fragment where the saturator is claimed complete on its own and **no tableau runs at
//! all**. `classify` returned zero rows with `incomplete: false` and `dropped: {}` while
//! Konclude v0.7.0-1138 and `HermiT` 1.4.3 both derive `X ⊑ Z` (measured). A wrong
//! answer under the strongest completeness banner the project makes.
//!
//! The mixed head `P ⊓ ∃s.S` was subtler and is canaried separately: the atomic conjunct
//! survived, so the output looked partially right rather than empty.
//!
//! # Known sibling, deliberately NOT fixed here
//!
//! The `ObjectPropertyDomain(r, ∃s.S)` AXIOM spelling has the same gap and still misses
//! (`a_nonatomic_domain_on_the_axiom_spelling_is_a_known_sibling_miss` pins it). Its arm
//! runs in Pass 1, BEFORE `effective_ranges` is built, so the marker machinery this fix
//! uses is not yet available there; closing it needs a pass restructure, not a shared
//! helper. Recorded rather than silently left.

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
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex.org/g>\n\
         Declaration(Class(:X)) Declaration(Class(:B)) Declaration(Class(:S))\n\
         Declaration(Class(:Z)) Declaration(Class(:P)) Declaration(Class(:W))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         {body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    result.is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

/// `X` has an `r`-successor, so it is in `r`'s domain.
const SRC: &str = "SubClassOf(:X ObjectSomeValuesFrom(:r :B))";
/// Consumes the domain's existential conclusion.
const SINK: &str = "SubClassOf(ObjectSomeValuesFrom(:s :S) :Z)";

#[test]
fn an_existential_gci_domain_head_reaches_the_source() {
    // The issue's reproducer. Konclude and HermiT both derive `X ⊑ Z`.
    let body = format!(
        "SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) ObjectSomeValuesFrom(:s :S))\n{SRC}\n{SINK}"
    );
    assert!(holds(&body, "X", "Z"), "X ⊑ ∃r.⊤ ⊑ ∃s.S ⊑ Z");
}

#[test]
fn a_mixed_gci_domain_head_yields_both_conjuncts() {
    // The subtler variant: before the fix this returned `[X ⊑ P]` — the ATOMIC conjunct
    // survived while the existential one was discarded, so the answer looked partially
    // right instead of empty. Both must now hold.
    let body = format!(
        "SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) \
         ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :S)))\n{SRC}\n{SINK}"
    );
    assert!(
        holds(&body, "X", "P"),
        "the atomic conjunct (worked before)"
    );
    assert!(
        holds(&body, "X", "Z"),
        "the existential conjunct (was dropped)"
    );
}

#[test]
fn an_atomic_gci_domain_head_still_works() {
    // The discriminating control: this derived correctly BEFORE the fix, which is what
    // isolates the non-atomic head as the cause rather than the GCI-domain mechanism.
    let body = format!("SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) :P)\n{SRC}");
    assert!(holds(&body, "X", "P"));
}

#[test]
fn a_nested_existential_domain_head_reaches_the_source() {
    // Two levels deep, so the head needs the recursive marker lowering rather than a
    // one-level special case.
    let body = format!(
        "SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) \
         ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s :S)))\n{SRC}\n\
         SubClassOf(ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s :S)) :W)"
    );
    assert!(holds(&body, "X", "W"));
}

#[test]
fn a_source_without_an_r_successor_gains_nothing() {
    // THE FP GUARD. The domain fires on `r`-SOURCES only: a class with no `r`-successor
    // must not inherit the head. Without this, "the head reaches the source" would be
    // satisfied by an implementation that asserts the head of everything.
    let body = format!(
        "SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) ObjectSomeValuesFrom(:s :S))\n\
         Declaration(Class(:Unrelated))\n{SINK}"
    );
    assert!(
        !holds(&body, "Unrelated", "Z"),
        "a class with no r-successor is not in r's domain — FALSE POSITIVE if this trips"
    );
}

#[test]
fn a_nonatomic_domain_on_the_axiom_spelling_is_a_known_sibling_miss() {
    // `ObjectPropertyDomain(r, ∃s.S)` is the SAME semantics as the fixed GCI above and
    // still misses: its arm runs in Pass 1, before `effective_ranges` exists, so the
    // marker machinery is unavailable there and closing it needs a pass restructure.
    //
    // Konclude and HermiT both derive `X ⊑ Z` here, so this pins a REAL gap, not a
    // design choice. A future fix should FLIP this assertion, never delete it.
    let body = format!("ObjectPropertyDomain(:r ObjectSomeValuesFrom(:s :S))\n{SRC}\n{SINK}");
    assert!(
        !holds(&body, "X", "Z"),
        "if this now HOLDS the sibling gap is closed — flip this test and update #114"
    );
}
