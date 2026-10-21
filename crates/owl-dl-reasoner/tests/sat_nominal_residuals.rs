//! #302 items 2–4: three more places where the EL saturator dropped what it knew about
//! individuals used as nominals. `HermiT` agrees with every verdict below (2026-10-21).
//!
//! * item 2 (`RUSTDL_SAT_NOMINAL_COMPLEX_TYPES`): `c : Y` with a told `Y ⊑ ∃S.{a}` links
//!   `c` to `a`, so `c` gets a stand-in and inherits the edge `c S a`;
//! * item 3 (`RUSTDL_SAT_NOMINAL_EQUIV`): `{a} ⊑ C` is read as `N_a ⊑ C` and `Y ⊑ {a}` as
//!   `Y ⊑ N_a`, so `EquivalentClasses({a}, Y)` makes `N_a ≡ Y`;
//! * item 4 (`RUSTDL_SAT_NOMINAL_INVERSE_EXISTS`): a TOP-LEVEL `∃S⁻.E` on either side of an
//!   axiom uses the #296 stand-in `InvExKey(S, E)`.
//!
//! Positives run with the item's flag on and off. Some are already derived by the wedge
//! in `classify`; those go through the saturation-only API.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_nominal_residuals`

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
    Declaration(Class(:F)) Declaration(Class(:Y)) Declaration(Class(:Z))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
";

const AT_A: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))\n";
const INV: &str = "    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) :E)))\n";

const TYPES: &str = "RUSTDL_SAT_NOMINAL_COMPLEX_TYPES";
const EQUIV: &str = "RUSTDL_SAT_NOMINAL_EQUIV";
const INVEX: &str = "RUSTDL_SAT_NOMINAL_INVERSE_EXISTS";

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{DECLS}{axioms}\n)\n");
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `classify` with `flag` set to `value`; reports `X ⊑ W`.
fn classify_x_is_w(axioms: &str, flag: &'static str, value: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set(flag, value);
    owl_dl_reasoner::classify(&parse(axioms))
        .expect("classify")
        .is_subclass("http://t/X", "http://t/W")
}

/// The EL closure alone with `flag` set to `value`; reports `X ⊑ W`.
fn saturation_x_is_w(axioms: &str, flag: &'static str, value: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set(flag, value);
    owl_dl_reasoner::is_subclass_of_saturation_only(&parse(axioms), "http://t/X", "http://t/W")
        .expect("saturation")
}

fn derived_only_by_the_rule(holds: impl Fn(&str) -> bool) {
    assert!(holds("1"), "the rule must derive it");
    assert!(!holds("0"), "control: without the rule it is missed");
}

// ------------------------------------------------- item 2

#[test]
fn an_individual_in_a_class_with_a_nominal_edge_gets_the_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(:Y :c) SubClassOf(:Y ObjectSomeValuesFrom(:S ObjectOneOf(:a)))
    ClassAssertion(:E :c)\n{INV}"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, TYPES, f));
}

#[test]
fn the_edge_is_inherited_two_told_sub_classes_down() {
    let body = format!(
        "{AT_A}    ClassAssertion(:F :c) SubClassOf(:F :Z) SubClassOf(:Z :Y)
    SubClassOf(:Y ObjectSomeValuesFrom(:S ObjectOneOf(:a))) ClassAssertion(:E :c)\n{INV}"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, TYPES, f));
}

/// FP guard: the class's edge goes to `b`.
#[test]
fn an_edge_to_another_individual_does_not_count() {
    let body = format!(
        "{AT_A}    ClassAssertion(:Y :c) SubClassOf(:Y ObjectSomeValuesFrom(:S ObjectOneOf(:b)))
    ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, TYPES, "1"));
}

/// FP guard: `Y ⊑ ∀S.{a}` gives no `S`-successor.
#[test]
fn a_universal_in_the_class_is_not_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(:Y :c) SubClassOf(:Y ObjectAllValuesFrom(:S ObjectOneOf(:a)))
    ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, TYPES, "1"));
}

/// FP guard: the edge is one disjunct of a union.
#[test]
fn an_edge_in_a_union_is_not_an_edge() {
    let body = format!(
        "{AT_A}    ClassAssertion(:Y :c)
    SubClassOf(:Y ObjectUnionOf(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :Z))
    ClassAssertion(:E :c)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, TYPES, "1"));
}

/// FP guard: with no individual in `Y`, `Y` may be empty.
#[test]
fn a_class_with_no_individual_gives_no_predecessor() {
    let body = format!(
        "{AT_A}    SubClassOf(:Y ObjectSomeValuesFrom(:S ObjectOneOf(:a))) SubClassOf(:Y :E)\n{INV}"
    );
    assert!(!classify_x_is_w(&body, TYPES, "1"));
}

// ------------------------------------------------- item 3

#[test]
fn a_nominal_equivalence_types_the_individual() {
    let body = format!(
        "{AT_A}    EquivalentClasses(ObjectOneOf(:a) :Y) SubClassOf(:Y :E)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))"
    );
    derived_only_by_the_rule(|f| saturation_x_is_w(&body, EQUIV, f));
}

#[test]
fn a_nominal_equivalence_types_the_class() {
    let body =
        "    SubClassOf(:X ObjectSomeValuesFrom(:R :Y)) EquivalentClasses(ObjectOneOf(:a) :Y)
    ClassAssertion(:E :a) EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    derived_only_by_the_rule(|f| saturation_x_is_w(body, EQUIV, f));
}

/// `Y ⊑ {a}` alone: every `Y` is `a`, so a `Y`-successor is an `E`.
#[test]
fn a_class_under_a_nominal_takes_its_types() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R :Y)) SubClassOf(:Y ObjectOneOf(:a))
    ClassAssertion(:E :a) EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    derived_only_by_the_rule(|f| saturation_x_is_w(body, EQUIV, f));
}

/// FP guard: `{a} ⊑ Y` says nothing about other `Y`s.
#[test]
fn a_nominal_under_a_class_does_not_type_the_class() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R :Y)) SubClassOf(ObjectOneOf(:a) :Y)
    ClassAssertion(:E :a) EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    assert!(!saturation_x_is_w(body, EQUIV, "1"));
}

/// FP guard (direction): `Y ⊑ {a}` does not put `a` in `Y`; `Y` may be empty.
#[test]
fn a_class_under_a_nominal_does_not_contain_it() {
    let body = format!(
        "{AT_A}    SubClassOf(:Y ObjectOneOf(:a)) EquivalentClasses(:W ObjectSomeValuesFrom(:R :Y))"
    );
    assert!(!saturation_x_is_w(&body, EQUIV, "1"));
}

/// FP guard: `{a, b}` is not `{a}`.
#[test]
fn a_two_member_enumeration_is_not_a_nominal() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R :Y)) SubClassOf(:Y ObjectOneOf(:a :b))
    ClassAssertion(:E :a) EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    assert!(!saturation_x_is_w(body, EQUIV, "1"));
}

// ------------------------------------------------- item 4

#[test]
fn a_top_level_inverse_existential_on_the_left_is_read() {
    let body = format!(
        "{AT_A}    ObjectPropertyAssertion(:S :c :a) ClassAssertion(:E :c)
    SubClassOf(ObjectSomeValuesFrom(ObjectInverseOf(:S) :E) :F)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :F))"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, INVEX, f));
}

#[test]
fn a_top_level_inverse_existential_on_the_right_is_read() {
    let body = format!(
        "    SubClassOf(:X ObjectSomeValuesFrom(:R :Y))
    SubClassOf(:Y ObjectSomeValuesFrom(ObjectInverseOf(:S) :E))\n{INV}"
    );
    derived_only_by_the_rule(|f| saturation_x_is_w(&body, INVEX, f));
}

/// FP guard: the told filler is `F`, not `E`.
#[test]
fn a_told_inverse_existential_with_another_filler_does_not_count() {
    let body = format!(
        "    SubClassOf(:X ObjectSomeValuesFrom(:R :Y))
    SubClassOf(:Y ObjectSomeValuesFrom(ObjectInverseOf(:S) :F))\n{INV}"
    );
    assert!(!saturation_x_is_w(&body, INVEX, "1"));
}

/// FP guard (polarity): `a S c` gives `a` an `S`-successor, not an `S⁻`-successor.
#[test]
fn a_forward_edge_does_not_fire_an_inverse_trigger() {
    let body = format!(
        "{AT_A}    ObjectPropertyAssertion(:S :a :c) ClassAssertion(:E :c)
    SubClassOf(ObjectSomeValuesFrom(ObjectInverseOf(:S) :E) :F)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :F))"
    );
    assert!(!classify_x_is_w(&body, INVEX, "1"));
}
