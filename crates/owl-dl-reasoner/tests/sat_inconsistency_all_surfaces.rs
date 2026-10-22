//! #310 `RUSTDL_SAT_INCONSISTENCY_ALL_SURFACES` (default ON): `is_consistent` and
//! `realize` read the EL saturator's inconsistency signal, as `classify` does.
//!
//! On the shape below `classify` reported `consistent: false` (since #307) while
//! `rustdl consistent` said consistent, `realize` returned a realization, and `diagnose`
//! and `justify … inconsistent` found nothing. `HermiT` calls it inconsistent.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_inconsistency_all_surfaces`

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

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:W))
    Declaration(Class(:Z)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
    Declaration(Class(:E)) Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c))
    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
";

/// Only the saturator sees this clash: an inverse-functional merge puts `a` in `Z`,
/// disjoint from `W`.
const ONLY_SATURATION: &str = "    InverseFunctionalObjectProperty(:r) SubObjectPropertyOf(:s ObjectInverseOf(:r))
    SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :Y)) SubClassOf(:Y ObjectSomeValuesFrom(:s :Z))
    SubClassOf(ObjectOneOf(:a) :W) DisjointClasses(:W :Z)";

/// `{a} ⊑ ⊥`: `realize` returned an empty realization.
const BOTTOM: &str = "    SubClassOf(ObjectOneOf(:a) owl:Nothing)";

/// `c : E`, `E ⊑ ⊥`: `abox_check` finds it; `realize` returned an empty realization.
const ASSERTED_BOTTOM: &str = "    ClassAssertion(:E :c) SubClassOf(:E owl:Nothing)";

/// FP guard: `Y` is unsat (an inverse-functional merge with `b`), the KB is not.
const IF_CLASS: &str = "    InverseFunctionalObjectProperty(:r) SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectOneOf(:c)))
    SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:r ObjectOneOf(:c))) SubClassOf(:Y :A)
    SubClassOf(ObjectOneOf(:b) :B) DisjointClasses(:A :B)";

/// FP guard: an unsat class reaching `{a}` through a functional role; consistent.
const CONSISTENT: &str =
    "    FunctionalObjectProperty(:R) SubClassOf(:C ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    SubClassOf(:C ObjectSomeValuesFrom(:R ObjectOneOf(:b))) SubClassOf(ObjectOneOf(:a) :A)
    SubClassOf(ObjectOneOf(:b) :B) DisjointClasses(:A :B)";

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://t/x>\n{DECLS}{axioms}\n)\n"
    );
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn consistent(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_INCONSISTENCY_ALL_SURFACES", flag);
    owl_dl_reasoner::is_consistent(&parse(axioms)).expect("is_consistent")
}

/// `true` iff `realize` reports the KB inconsistent.
fn realize_says_inconsistent(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_INCONSISTENCY_ALL_SURFACES", flag);
    matches!(
        owl_dl_reasoner::realize(&parse(axioms)),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    )
}

#[test]
fn is_consistent_reads_the_saturator() {
    assert!(!consistent(ONLY_SATURATION, "1"));
    // This control pins a wedge gap (a trusted `Sat` on this inconsistent KB). If the
    // wedge is fixed for the shape it will fail: FLIP it, do not delete the test.
    assert!(
        consistent(ONLY_SATURATION, "0"),
        "control: without it the clash is missed"
    );
}

#[test]
fn the_bounded_api_reads_the_saturator() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let r = owl_dl_reasoner::is_consistent_with_timeout(
        &parse(ONLY_SATURATION),
        std::time::Duration::from_secs(30),
    )
    .expect("bounded");
    assert_eq!(r, Some(false));
}

#[test]
fn realize_reads_the_saturator() {
    assert!(realize_says_inconsistent(ONLY_SATURATION, "1"));
    assert!(realize_says_inconsistent(BOTTOM, "1"));
    assert!(
        !realize_says_inconsistent(BOTTOM, "0"),
        "control: an empty realization"
    );
}

/// `realize` agrees with `is_consistent` on an inconsistency `abox_check` finds.
#[test]
fn realize_reports_an_asserted_bottom() {
    assert!(!consistent(ASSERTED_BOTTOM, "1"));
    assert!(realize_says_inconsistent(ASSERTED_BOTTOM, "1"));
    assert!(
        !realize_says_inconsistent(ASSERTED_BOTTOM, "0"),
        "control: an empty realization"
    );
}

#[test]
fn a_consistent_kb_stays_consistent_on_both_surfaces() {
    for axioms in [CONSISTENT, IF_CLASS] {
        assert!(consistent(axioms, "1"), "{axioms}");
        assert!(!realize_says_inconsistent(axioms, "1"), "{axioms}");
    }
}
