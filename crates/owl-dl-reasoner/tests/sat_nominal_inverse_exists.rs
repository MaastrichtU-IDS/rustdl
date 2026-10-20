//! #296 `RUSTDL_SAT_NOMINAL_INVERSE_EXISTS` (default ON): an inverse existential
//! `∃S⁻.E` inside a filler is read by the EL saturator through nominal edges.
//!
//! `X ⊑ ∃R.{a}`, `c S a`, `c : E`, `W ≡ ∃R.∃S⁻.E` entails `X ⊑ W`. `classify` missed it
//! while reporting `incomplete: false`: the saturator did not lower `∃S⁻.E`, and the
//! wedge does not see the assertions of nominal individuals
//! (`RUSTDL_CLASSIFY_NOMINAL_OPA` is off). The same KB with a NAMED inverse of `S` was
//! already derived. `∃S⁻.E` now gets a stand-in class, derived for a nominal `a` from an
//! edge `c S a` of a nominal `c` in `E`. Only nominals on both ends: a class that has an
//! `S`-edge to `{a}` may be empty. `HermiT` agrees with every verdict below (2026-10-20).
//!
//! The `=0` arm of each positive relies on the wedge missing these too: if
//! `RUSTDL_CLASSIFY_NOMINAL_OPA` ever defaults ON, those controls will fail and should
//! be pinned to it off, not read as a regression.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_nominal_inverse_exists`

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
    Declaration(Class(:F)) Declaration(Class(:Y))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S)) Declaration(ObjectProperty(:S1))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
";

const NOM: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))\n";
const DEF: &str = "    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) :E)))\n";

/// `classify` with the flag set to `flag`; reports `X ⊑ W`.
fn x_is_w(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_NOMINAL_INVERSE_EXISTS", flag);
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{DECLS}{NOM}{axioms}\n)\n");
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify(&onto)
        .expect("classify")
        .is_subclass("http://t/X", "http://t/W")
}

/// Positive with the flag on, and missed with it off.
fn derived_only_by_the_rule(axioms: &str) {
    assert!(x_is_w(axioms, "1"), "the rule must derive it");
    assert!(
        !x_is_w(axioms, "0"),
        "control: without the rule it is missed"
    );
}

#[test]
fn an_inverse_existential_of_a_nominal_edge_is_followed() {
    derived_only_by_the_rule(&format!(
        "    ObjectPropertyAssertion(:S :c :a) ClassAssertion(:E :c)\n{DEF}"
    ));
}

#[test]
fn an_edge_on_a_sub_role_counts() {
    derived_only_by_the_rule(&format!(
        "    SubObjectPropertyOf(:S1 :S) ObjectPropertyAssertion(:S1 :c :a) ClassAssertion(:E :c)\n{DEF}"
    ));
}

#[test]
fn a_type_derived_by_subsumption_counts() {
    derived_only_by_the_rule(&format!(
        "    ObjectPropertyAssertion(:S :c :a) ClassAssertion(:F :c) SubClassOf(:F :E)\n{DEF}"
    ));
}

#[test]
fn an_assertion_written_through_the_inverse_counts() {
    derived_only_by_the_rule(&format!(
        "    ObjectPropertyAssertion(ObjectInverseOf(:S) :a :c) ClassAssertion(:E :c)\n{DEF}"
    ));
}

#[test]
fn two_inverse_steps_are_followed() {
    derived_only_by_the_rule(
        "    ObjectPropertyAssertion(:S :c :a) ObjectPropertyAssertion(:S :b :c) ClassAssertion(:E :b)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S)
        ObjectSomeValuesFrom(ObjectInverseOf(:S) :E))))",
    );
}

/// FP guard: `a : E` says nothing about `a`'s `S`-predecessors.
#[test]
fn the_type_on_the_wrong_end_does_not_count() {
    assert!(!x_is_w(
        &format!("    ObjectPropertyAssertion(:S :c :a) ClassAssertion(:E :a)\n{DEF}"),
        "1"
    ));
}

/// FP guard: `c` is not known to be an `E`.
#[test]
fn an_untyped_predecessor_does_not_count() {
    assert!(!x_is_w(
        &format!("    ObjectPropertyAssertion(:S :c :a)\n{DEF}"),
        "1"
    ));
}

/// FP guard: a CLASS with an `S`-edge to `{a}` may be empty, so it gives `a` no
/// predecessor.
#[test]
fn a_class_source_does_not_count() {
    assert!(!x_is_w(
        &format!(
            "    SubClassOf(:Y ObjectSomeValuesFrom(:S ObjectOneOf(:a))) SubClassOf(:Y :E)\n{DEF}"
        ),
        "1"
    ));
}

/// FP guard: the edge goes to `b`, not `a`.
#[test]
fn an_edge_to_another_individual_does_not_count() {
    assert!(!x_is_w(
        &format!("    ObjectPropertyAssertion(:S :c :b) ClassAssertion(:E :c)\n{DEF}"),
        "1"
    ));
}

/// FP guard: an edge on a SUPER-role of `S` is not an `S`-edge.
#[test]
fn an_edge_on_a_super_role_does_not_count() {
    assert!(!x_is_w(
        &format!(
            "    SubObjectPropertyOf(:S :S1) ObjectPropertyAssertion(:S1 :c :a) ClassAssertion(:E :c)\n{DEF}"
        ),
        "1"
    ));
}

/// FP guard: `c : F ⊑ ∃S.Y` gives `c` some `S`-successor in `Y`, not every `Y`, so `Y` is not
/// `∃S⁻.E`. The target must be a nominal too. (The unrelated `c S1 a` makes `c` reachable
/// from `{a}`, so the rule sees `c`'s facts at all.)
#[test]
fn a_class_target_does_not_count() {
    assert!(!x_is_w(
        &format!(
            "    ClassAssertion(:E :c) ClassAssertion(:F :c) SubClassOf(:F ObjectSomeValuesFrom(:S :Y))
    ObjectPropertyAssertion(:S1 :c :a) SubClassOf(:X ObjectSomeValuesFrom(:R :Y))\n{DEF}"
        ),
        "1"
    ));
}
