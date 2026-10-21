//! #307 `RUSTDL_SAT_UNSAT_NOMINAL_INCONSISTENT` (default ON): an unsatisfiable nominal
//! stand-in makes the KB inconsistent in the EL saturator.
//!
//! A named individual exists in every model, so deriving `N_a ⊑ ⊥` means the KB is
//! inconsistent. `--saturation-only` used to report `consistent: true` beside a
//! non-empty `unsatisfiable` list. Default `classify` already said inconsistent (the
//! `ABox` pre-check or the wedge catches it), so these run through the saturation-only
//! API. `HermiT` agrees with every verdict below (2026-10-21).
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_unsat_nominal_inconsistent`

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

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
    Declaration(Class(:D)) Declaration(Class(:A)) Declaration(Class(:B))
    Declaration(Class(:W)) Declaration(Class(:Z)) Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
";

/// Every declared class above.
const CLASSES: usize = 8;

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://t/x>\n{DECLS}{axioms}\n)\n"
    );
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// The saturation-only answer with the flag set to `flag`: `(consistent, #unsat classes)`.
fn saturation(axioms: &str, flag: &str) -> (bool, usize) {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_UNSAT_NOMINAL_INCONSISTENT", flag);
    let c = owl_dl_reasoner::classify_saturation_only(&parse(axioms)).expect("saturation");
    (!c.stats().inconsistent, c.unsatisfiable_classes().len())
}

/// Default `classify` with the flag set to `flag`: whether it reports consistent.
fn classify_consistent(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_UNSAT_NOMINAL_INCONSISTENT", flag);
    !owl_dl_reasoner::classify(&parse(axioms))
        .expect("classify")
        .stats()
        .inconsistent
}

/// Inconsistent with the flag on, every class unsat; consistent with it off.
fn inconsistent_only_by_the_rule(axioms: &str) {
    assert_eq!(
        saturation(axioms, "1"),
        (false, CLASSES),
        "inconsistent, every class unsat"
    );
    assert!(
        saturation(axioms, "0").0,
        "control: without the rule it reads consistent"
    );
}

/// Needs `RUSTDL_SAT_NOMINAL_EQUIV` (default ON) to read `{a}` on the left.
#[test]
fn a_nominal_under_bottom_is_inconsistent() {
    inconsistent_only_by_the_rule("    SubClassOf(ObjectOneOf(:a) owl:Nothing)");
}

#[test]
fn a_nominal_in_two_disjoint_classes_is_inconsistent() {
    inconsistent_only_by_the_rule(
        "    SubClassOf(ObjectOneOf(:a) :C) SubClassOf(ObjectOneOf(:a) :D) DisjointClasses(:C :D)",
    );
}

#[test]
fn an_individual_asserted_to_nothing_is_inconsistent() {
    inconsistent_only_by_the_rule("    ClassAssertion(owl:Nothing :a)");
}

#[test]
fn an_individual_in_a_class_and_its_complement_is_inconsistent() {
    inconsistent_only_by_the_rule(
        "    ClassAssertion(ObjectIntersectionOf(:Y ObjectComplementOf(:Y)) :a)",
    );
}

/// FP guard: an unsat CLASS that merely points at `{a}` may be empty.
#[test]
fn an_unsat_class_with_a_nominal_edge_is_consistent() {
    assert_eq!(saturation("    SubClassOf(:X owl:Nothing)", "1"), (true, 1));
}

/// FP guard: an unsat class unrelated to the individual's types.
#[test]
fn an_unsat_class_beside_a_typed_individual_is_consistent() {
    assert_eq!(
        saturation("    SubClassOf(:Y owl:Nothing) ClassAssertion(:C :a)", "1"),
        (true, 1)
    );
}

/// Default `classify` too: here only the saturator sees the clash (an inverse-functional
/// merge puts `a` in `Z`, disjoint from `W`). Main reported this consistent.
#[test]
fn default_classify_reports_an_inconsistency_only_the_saturator_finds() {
    let axioms = "    InverseFunctionalObjectProperty(:r) SubObjectPropertyOf(:s ObjectInverseOf(:r))
    SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :Y)) SubClassOf(:Y ObjectSomeValuesFrom(:s :Z))
    SubClassOf(ObjectOneOf(:a) :W) DisjointClasses(:W :Z)";
    assert!(!classify_consistent(axioms, "1"));
    assert!(
        classify_consistent(axioms, "0"),
        "control: without the rule it is missed"
    );
}

/// FP guards: an unsat CLASS reaches the nominal through a rule a future unsound merge
/// could break (functional, `∀`, range). Each class may be empty; the KB is consistent.
const REACHES: [&str; 3] = [
    "    FunctionalObjectProperty(:R) SubClassOf(:C ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectOneOf(:b))) SubClassOf(ObjectOneOf(:a) :A)
    SubClassOf(ObjectOneOf(:b) :B) DisjointClasses(:A :B)",
    "    SubClassOf(:C ObjectAllValuesFrom(:R ObjectOneOf(:a)))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectOneOf(:b))) SubClassOf(ObjectOneOf(:a) :A)
    SubClassOf(ObjectOneOf(:b) :B) DisjointClasses(:A :B)",
    "    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectOneOf(:a))) ObjectPropertyRange(:R :B)
    SubClassOf(ObjectOneOf(:a) ObjectComplementOf(:B))",
];

#[test]
fn an_unsat_class_reaching_a_nominal_is_consistent_in_saturation() {
    for axioms in REACHES {
        assert!(saturation(axioms, "1").0, "{axioms}");
    }
}

#[test]
fn an_unsat_class_reaching_a_nominal_is_consistent_in_classify() {
    for axioms in REACHES {
        assert!(classify_consistent(axioms, "1"), "{axioms}");
    }
}
