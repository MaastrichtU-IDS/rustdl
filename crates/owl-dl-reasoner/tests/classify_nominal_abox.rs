//! #279: the classify wedge reads more of a nominal individual's `ABox`.
//! `SameIndividual(a, b)` is `{a} ⊑ {b}` and back (`RUSTDL_CLASSIFY_NOMINAL_SAME`,
//! default ON), a complex `ClassAssertion(C, a)` is `{a} ⊑ C`
//! (`RUSTDL_CLASSIFY_NOMINAL_TYPES`, default ON), and `ObjectPropertyAssertion(S, a, c)`
//! is `{a} ⊑ ∃S.{c}` and `{c} ⊑ ∃S⁻.{a}` (`RUSTDL_CLASSIFY_NOMINAL_OPA`, default OFF:
//! generating, and costly on ORE).
//!
//! Each positive below was a silent miss (`incomplete: false`). Konclude and `HermiT`
//! agree with every verdict (2026-10-16), including the controls, where both stay
//! silent.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_nominal_abox`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Every test locks, including the
// read-only ones, so none observes another test's setting.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const DECLS: &str = "    Declaration(Class(:B)) Declaration(Class(:E)) Declaration(Class(:K))
    Declaration(Class(:Q)) Declaration(Class(:W)) Declaration(Class(:X))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c))
    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
";

const NO_SAME: &[(&str, &str)] = &[("RUSTDL_CLASSIFY_NOMINAL_SAME", "0")];
const OPA: &[(&str, &str)] = &[("RUSTDL_CLASSIFY_NOMINAL_OPA", "1")];

fn classify(body: &str, extra: &[(&'static str, &str)]) -> owl_dl_reasoner::Classification {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flags: Vec<SetEnvGuard> = extra.iter().map(|(k, v)| SetEnvGuard::set(k, v)).collect();
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify(&onto).expect("classify")
}

fn subsumed(body: &str, sub: &str, sup: &str, extra: &[(&'static str, &str)]) -> bool {
    classify(body, extra).is_subclass(&format!("http://t/{sub}"), &format!("http://t/{sup}"))
}

fn unsat(body: &str, class: &str) -> bool {
    let c = classify(body, &[]);
    let iri = format!("http://t/{class}");
    c.unsatisfiable_classes().contains(&iri.as_str())
}

/// The issue's reproducer: `a = b`, `b : E`, `X ⊑ ∃R.{a}`, `K ≡ ∃R.E`.
const SAME: &str = "    SameIndividual(:a :b)
    ClassAssertion(:E :b)
    EquivalentClasses(:K ObjectSomeValuesFrom(:R :E))";

const W_SE: &str = "EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))";
const W_INV: &str =
    "EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) :E)))";

// ---------------------------------------------------------------- positives

#[test]
fn same_individual_carries_the_other_members_types() {
    assert!(subsumed(SAME, "X", "K", &[]));
}

/// The same with the members listed the other way round, so the equality is used in
/// the direction the `SameIndividual` lists second.
#[test]
fn same_individual_works_in_both_directions() {
    let body = SAME.replace("SameIndividual(:a :b)", "SameIndividual(:b :a)");
    assert!(subsumed(&body, "X", "K", &[]));
}

/// Equality is followed transitively: `a = b`, `b = c`, `c : E`.
#[test]
fn same_individual_is_followed_through_a_chain() {
    let body = "    SameIndividual(:a :b)
    SameIndividual(:b :c)
    ClassAssertion(:E :c)
    EquivalentClasses(:K ObjectSomeValuesFrom(:R :E))";
    assert!(subsumed(body, "X", "K", &[]));
}

/// FP guard for the #205 alias shape: `a = b` and `a ≠ c`, with the label heuristic
/// off so every pair reaches the wedge. `c`'s type must not reach `X`.
#[test]
fn same_and_different_individuals_do_not_alias() {
    let body = "    SameIndividual(:a :b)
    DifferentIndividuals(:a :c)
    ClassAssertion(:E :c)
    EquivalentClasses(:K ObjectSomeValuesFrom(:R :E))";
    assert!(!subsumed(
        body,
        "X",
        "K",
        &[("RUSTDL_LABEL_HEURISTIC", "0")]
    ));
}

/// `a S c`, `c : E` gives `{a} ⊑ ∃S.E`.
#[test]
fn a_property_assertion_from_the_nominal_is_followed() {
    let body =
        format!("    ObjectPropertyAssertion(:S :a :c)\n    ClassAssertion(:E :c)\n    {W_SE}");
    assert!(subsumed(&body, "X", "W", OPA));
}

/// `c S a`, `c : E` gives `{a} ⊑ ∃S⁻.E`.
#[test]
fn a_property_assertion_into_the_nominal_is_followed() {
    let body =
        format!("    ObjectPropertyAssertion(:S :c :a)\n    ClassAssertion(:E :c)\n    {W_INV}");
    assert!(subsumed(&body, "X", "W", OPA));
}

/// A complex asserted class, `a : ∃S.E`.
#[test]
fn a_complex_class_assertion_is_followed() {
    let body = format!("    ClassAssertion(ObjectSomeValuesFrom(:S :E) :a)\n    {W_SE}");
    assert!(subsumed(&body, "X", "W", &[]));
}

/// A negated asserted class, `a : ¬B`, makes `Q ≡ X ⊓ ∀R.B` unsatisfiable.
#[test]
fn a_negated_class_assertion_makes_a_class_unsatisfiable() {
    let body = "    ClassAssertion(ObjectComplementOf(:B) :a)
    EquivalentClasses(:Q ObjectIntersectionOf(:X ObjectAllValuesFrom(:R :B)))";
    assert!(unsat(body, "Q"));
}

// ---------------------------------------------------------------- controls

/// FP guard: a type of an individual NOT equal to `a` is not `a`'s.
#[test]
fn a_type_of_an_unrelated_individual_is_not_derived() {
    let body = SAME.replace("SameIndividual(:a :b)", "SameIndividual(:b :c)");
    assert!(!subsumed(&body, "X", "K", &[]));
}

/// FP guard: the edge goes `a → c`, so nothing follows along `S⁻` from `a`.
#[test]
fn a_property_assertion_is_not_read_backwards() {
    let body =
        format!("    ObjectPropertyAssertion(:S :a :c)\n    ClassAssertion(:E :c)\n    {W_INV}");
    assert!(!subsumed(&body, "X", "W", OPA));
}

/// FP guard: the successor's type is `B`, not `E`.
#[test]
fn a_complex_assertion_with_another_filler_entails_nothing() {
    let body = format!("    ClassAssertion(ObjectSomeValuesFrom(:S :B) :a)\n    {W_SE}");
    assert!(!subsumed(&body, "X", "W", &[]));
}

/// FP guard: `a : ¬E` says nothing about `B`.
#[test]
fn a_negated_assertion_of_another_class_is_not_a_clash() {
    let body = "    ClassAssertion(ObjectComplementOf(:E) :a)
    EquivalentClasses(:Q ObjectIntersectionOf(:X ObjectAllValuesFrom(:R :B)))";
    assert!(!unsat(body, "Q"));
}

// ------------------------------------------------- the flags are load-bearing

/// Since the saturator also reads `SameIndividual` of nominal individuals
/// (`RUSTDL_SAT_NOMINAL_EDGES`), both are pinned off to see the wedge flag alone.
#[test]
fn same_flag_off_loses_the_equality() {
    assert!(!subsumed(
        SAME,
        "X",
        "K",
        &[
            ("RUSTDL_CLASSIFY_NOMINAL_SAME", "0"),
            ("RUSTDL_SAT_NOMINAL_EDGES", "0")
        ]
    ));
    assert!(subsumed(SAME, "X", "K", NO_SAME));
}

/// The wedge's property-assertion clauses are opt-in. A forward edge is still
/// followed by default, by the saturator (`RUSTDL_SAT_NOMINAL_EDGES`); with that off
/// the default does not follow it.
#[test]
fn property_assertions_are_not_followed_by_the_wedge_by_default() {
    let fwd =
        format!("    ObjectPropertyAssertion(:S :a :c)\n    ClassAssertion(:E :c)\n    {W_SE}");
    assert!(subsumed(&fwd, "X", "W", &[]));
    assert!(!subsumed(
        &fwd,
        "X",
        "W",
        &[("RUSTDL_SAT_NOMINAL_EDGES", "0")]
    ));
}

/// Since #302 the saturator also reads complex class assertions of nominal individuals
/// (`RUSTDL_SAT_NOMINAL_COMPLEX_TYPES`), so both are pinned off to see the wedge flag alone.
#[test]
fn types_flag_off_loses_the_complex_assertion() {
    let body = format!("    ClassAssertion(ObjectSomeValuesFrom(:S :E) :a)\n    {W_SE}");
    assert!(!subsumed(
        &body,
        "X",
        "W",
        &[
            ("RUSTDL_CLASSIFY_NOMINAL_TYPES", "0"),
            ("RUSTDL_SAT_NOMINAL_COMPLEX_TYPES", "0")
        ]
    ));
}
