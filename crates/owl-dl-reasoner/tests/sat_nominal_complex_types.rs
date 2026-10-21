//! #302 `RUSTDL_SAT_NOMINAL_COMPLEX_TYPES` (default ON): a class assertion of an
//! individual used as a nominal reaches the EL saturator in full.
//!
//! Before, only the ATOMIC operands of `ClassAssertion(C, a)` were seeded as `N_a ⊑ A`,
//! and an individual linked to a `TBox` nominal only through a class assertion
//! (`c : ∃S.{a}`) got no stand-in at all. Now `ClassAssertion(C, a)` is lowered as the
//! told `N_a ⊑ C`, exactly like `A ⊑ C` for a named `A`. A nominal inside an asserted
//! class links the two individuals for reachability, as a property assertion does.
//!
//! The wedge reads complex class assertions too (#281), so in `classify` most shapes were
//! already derived. The rule matters where the wedge does not reach: the `∃S⁻` route of
//! #296, and the saturation-only answer. `HermiT` agrees with every verdict below
//! (2026-10-21).
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_nominal_complex_types`

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

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:W)) Declaration(Class(:E))
    Declaration(Class(:F)) Declaration(Class(:A))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S)) Declaration(ObjectProperty(:S1))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
";

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{DECLS}{axioms}\n)\n");
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `classify` with the flag set to `flag`; reports `X ⊑ W`.
fn classify_x_is_w(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_NOMINAL_COMPLEX_TYPES", flag);
    owl_dl_reasoner::classify(&parse(axioms))
        .expect("classify")
        .is_subclass("http://t/X", "http://t/W")
}

/// The EL closure alone, with the flag set to `flag`; reports `X ⊑ W`.
fn saturation_x_is_w(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_NOMINAL_COMPLEX_TYPES", flag);
    owl_dl_reasoner::is_subclass_of_saturation_only(&parse(axioms), "http://t/X", "http://t/W")
        .expect("saturation")
}

fn derived_only_by_the_rule(holds: impl Fn(&str) -> bool) {
    assert!(holds("1"), "the rule must derive it");
    assert!(!holds("0"), "control: without the rule it is missed");
}

const AT_C: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:c)))\n";
const AT_A: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))\n";
const INV: &str = "    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) :E)))\n";

// ------------------------------------------------- classify (the #296 route)

/// The #302 reproducer: `c` is tied to `{a}` only by its class assertion.
#[test]
fn a_nominal_filler_in_a_class_assertion_is_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :c) ClassAssertion(:E :c)\n{INV}"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, f));
}

#[test]
fn the_edge_and_the_type_in_one_conjunction() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectIntersectionOf(:E ObjectSomeValuesFrom(:S ObjectOneOf(:a))) :c)\n{INV}"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, f));
}

/// FP guard: a disjunction of edges asserts neither.
#[test]
fn a_union_of_edges_is_not_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectUnionOf(ObjectSomeValuesFrom(:S ObjectOneOf(:a))
        ObjectSomeValuesFrom(:S ObjectOneOf(:b))) :c) ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

/// FP guard: `∀S.{a}` does not give `c` an `S`-successor.
#[test]
fn a_universal_is_not_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectAllValuesFrom(:S ObjectOneOf(:a)) :c) ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

/// FP guard: `≤1 S.{a}` does not either.
#[test]
fn an_upper_bound_is_not_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectMaxCardinality(1 :S ObjectOneOf(:a)) :c) ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

/// FP guard: nor does a negated edge.
#[test]
fn a_negated_edge_is_not_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectComplementOf(ObjectSomeValuesFrom(:S ObjectOneOf(:a))) :c)
    ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

/// FP guard: the edge goes to `b`, not `a`.
#[test]
fn an_edge_to_another_individual_does_not_count() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:b)) :c) ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

// ------------------------------------------------- saturation alone

#[test]
fn an_asserted_existential_types_the_individual() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectSomeValuesFrom(:S :E) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))"
    );
    derived_only_by_the_rule(|f| saturation_x_is_w(&body, f));
}

/// `a` is reached only through `c`'s class assertion, and its own type still counts.
#[test]
fn the_filler_individual_keeps_its_type() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :c) ClassAssertion(:E :a)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))"
    );
    derived_only_by_the_rule(|f| saturation_x_is_w(&body, f));
}

#[test]
fn an_asserted_existential_meets_a_domain() {
    let body = format!(
        "{AT_C}    ObjectPropertyDomain(:S :F) ClassAssertion(ObjectSomeValuesFrom(:S :E) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :F))"
    );
    derived_only_by_the_rule(|f| saturation_x_is_w(&body, f));
}

#[test]
fn an_asserted_min_gives_the_smaller_min() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectMinCardinality(3 :S) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))"
    );
    derived_only_by_the_rule(|f| saturation_x_is_w(&body, f));
}

/// FP guard: the asserted filler `E` is not `F`.
#[test]
fn a_different_filler_does_not_count() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectSomeValuesFrom(:S :E) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :F)))"
    );
    assert!(!saturation_x_is_w(&body, "1"));
}

/// FP guard: an asserted `≥2` is not a `≥3`.
#[test]
fn an_asserted_min_does_not_give_a_larger_one() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectMinCardinality(2 :S) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(3 :S)))"
    );
    assert!(!saturation_x_is_w(&body, "1"));
}

/// FP guard: an asserted disjunction gives neither disjunct.
#[test]
fn an_asserted_union_gives_neither_disjunct() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectUnionOf(:E :F) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))"
    );
    assert!(!saturation_x_is_w(&body, "1"));
}

/// FP guard (#297 interplay): two class-assertion edges to `a` and `b` are one successor
/// unless `a` and `b` are asserted different.
#[test]
fn two_edges_without_different_individuals_are_not_two() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :c)
    ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:b)) :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))"
    );
    assert!(!saturation_x_is_w(&body, "1"));
}

/// FP guard: `c : ∃S.{a}` and `c : ∃S.E` with `a : E` may be the same successor.
#[test]
fn a_nominal_and_a_class_filler_are_not_two() {
    let body = format!(
        "{AT_C}    ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :c)
    ClassAssertion(ObjectSomeValuesFrom(:S :E) :c) ClassAssertion(:E :a)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))"
    );
    assert!(!saturation_x_is_w(&body, "1"));
}

/// FP guard (polarity): `c : ∃S⁻.{a}` is `a S c`, not an `S`-edge from `c` to `a`.
#[test]
fn an_inverse_nominal_edge_is_not_read_forwards() {
    let body = format!(
        "{AT_A}    ClassAssertion(ObjectSomeValuesFrom(ObjectInverseOf(:S) ObjectOneOf(:a)) :c)
    ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, "1"));
}
