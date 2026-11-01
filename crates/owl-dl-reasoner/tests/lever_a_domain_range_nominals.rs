//! #400: Lever A (`RUSTDL_CLASSIFY_TBOX_ONLY`) skips the `ABox` seed on
//! classify's main-tableau probes when `ontology_uses_nominals` is false. That
//! predicate did not walk `ObjectPropertyDomain`/`ObjectPropertyRange`, so a
//! nominal there (`Range(r, {a})`) went unseen, the `ABox` was dropped, and
//! `C ⊑ D` below was silently missed (`incomplete: false`).
//!
//! The wedge answers these fixtures on its own, so the main tableau, the only
//! consumer of Lever A, is reached only with `RUSTDL_HYPERTABLEAU=0`. This file
//! sets that once; it is in its own binary so no sibling test observes it, and
//! every test here wants the same value.
//!
//! Library `classify` has no per-pair deadline, so the verdicts are exact.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::sync::Once;

static WEDGE_OFF: Once = Once::new();

#[allow(unsafe_code)]
fn classify_without_wedge(axioms: &str) -> owl_dl_reasoner::Classification {
    WEDGE_OFF.call_once(|| {
        // SAFETY: set_var is unsafe under edition 2024. Every test in this
        // binary wants the same value, and it is set once before any read.
        unsafe { std::env::set_var("RUSTDL_HYPERTABLEAU", "0") };
    });
    let ofn = format!(
        "Prefix(:=<urn:t#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(<urn:t>\n\
Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
Declaration(Class(:E)) Declaration(Class(:F))\n\
Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))\n\
SubClassOf(:C ObjectSomeValuesFrom(:r owl:Thing))\n\
EquivalentClasses(:D ObjectSomeValuesFrom(:r :B))\n\
SubClassOf(:E ObjectUnionOf(:F ObjectAllValuesFrom(:r :F)))\n\
{axioms}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

const C: &str = "urn:t#C";
const D: &str = "urn:t#D";

/// The #400 reproducer: every `r`-successor is `a`, and `a` is a `B`, so
/// `C ⊑ ∃r.⊤` gives `C ⊑ ∃r.B = D`.
#[test]
fn a_nominal_in_a_range_keeps_the_abox() {
    let c =
        classify_without_wedge("ObjectPropertyRange(:r ObjectOneOf(:a))\nClassAssertion(:B :a)");
    assert!(c.is_subclass(C, D), "Range(r, {{a}}) + a:B entails C ⊑ D");
}

/// The domain arm: `Domain(s, {a})` with `C ⊑ ∃s.⊤` makes every `C` equal to
/// `a`, and `a` is a `B`, so `C ⊑ B`.
#[test]
fn a_nominal_in_a_domain_keeps_the_abox() {
    let c = classify_without_wedge(
        "Declaration(ObjectProperty(:s))\n\
         ObjectPropertyDomain(:s ObjectOneOf(:a))\nClassAssertion(:B :a)\n\
         SubClassOf(:C ObjectSomeValuesFrom(:s owl:Thing))",
    );
    assert!(
        c.is_subclass(C, "urn:t#B"),
        "Domain(s, {{a}}) + a:B + C ⊑ ∃s.⊤ entails C ⊑ B"
    );
}

/// Discriminating control: the GCI spelling of the same range was always
/// detected, because `SubClassOf` was always walked.
#[test]
fn the_gci_spelling_of_the_range_keeps_the_abox() {
    let c = classify_without_wedge(
        "SubClassOf(owl:Thing ObjectAllValuesFrom(:r ObjectOneOf(:a)))\nClassAssertion(:B :a)",
    );
    assert!(c.is_subclass(C, D));
}

/// FP guard: without the assertion `a:B`, nothing forces `B` onto the
/// successor, so `C ⊑ D` must NOT be derived.
#[test]
fn without_the_assertion_c_is_not_subsumed() {
    let c = classify_without_wedge("ObjectPropertyRange(:r ObjectOneOf(:a))");
    assert!(!c.is_subclass(C, D), "C ⊑ D is not entailed without a:B");
}
