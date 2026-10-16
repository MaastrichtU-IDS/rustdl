//! #277 `RUSTDL_CLASSIFY_NOMINAL_TYPES` (default ON): the classify wedge reads
//! `ClassAssertion(C, a)` as `{a} ⊑ C`.
//!
//! `A ⊑ ∀R⁻.{a}`, `C ⊑ ∃R.A`, `a : D` entails `C ⊑ D`: a `C` is an `R⁻`-successor of
//! an `A`, so it is `a`. The wedge labelled that node `{a}` but its clause set carried
//! no class assertion, so `classify` missed `C ⊑ D` while reporting `incomplete:
//! false`. Konclude and `HermiT` agree with every verdict below (2026-10-16), including
//! the FP guards, where both stay silent.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_nominal_types`

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

const DECLS: &str = "    Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(Class(:E)) Declaration(ObjectProperty(:R))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
";

/// The issue's reproducer.
const INVERSE: &str = "    SubClassOf(:A ObjectAllValuesFrom(ObjectInverseOf(:R) ObjectOneOf(:a)))
    SubClassOf(:C ObjectSomeValuesFrom(:R :A))
    ClassAssertion(:D :a)";

/// Classify with `extra` env settings and report `C ⊑ sup`.
fn c_subsumed_by(body: &str, sup: &str, extra: &[(&'static str, &str)]) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flags: Vec<SetEnvGuard> = extra.iter().map(|(k, v)| SetEnvGuard::set(k, v)).collect();
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    c.is_subclass("http://t/C", &format!("http://t/{sup}"))
}

// ---------------------------------------------------------------- positives

#[test]
fn an_inverse_role_forall_over_a_nominal_is_derived() {
    assert!(c_subsumed_by(INVERSE, "D", &[]));
}

/// The asserted class may be a conjunction with a nominal: `a : D ⊓ {b}` makes `a`
/// equal to `b`, so `b`'s types are `a`'s too.
#[test]
fn a_nominal_in_an_asserted_conjunction_is_followed() {
    let body = INVERSE.replace(
        "ClassAssertion(:D :a)",
        "ClassAssertion(ObjectIntersectionOf(:D ObjectOneOf(:b)) :a)
    ClassAssertion(:E :b)",
    );
    assert!(c_subsumed_by(&body, "D", &[]));
    assert!(c_subsumed_by(&body, "E", &[]));
}

// ---------------------------------------------------------------- negatives

/// FP guard: a type of a DIFFERENT individual is not `a`'s.
#[test]
fn another_individuals_type_is_not_derived() {
    let body = INVERSE.replace("ClassAssertion(:D :a)", "ClassAssertion(:D :b)");
    assert!(!c_subsumed_by(&body, "D", &[]));
}

/// FP guard: a FORWARD `∀R.{a}` on the successor says nothing about `C`.
#[test]
fn a_forward_forall_on_the_successor_entails_nothing() {
    let body = INVERSE.replace("ObjectInverseOf(:R)", ":R");
    assert!(!c_subsumed_by(&body, "D", &[]));
}

// ------------------------------------------------- the flag is load-bearing

#[test]
fn flag_off_loses_the_entailment() {
    assert!(!c_subsumed_by(
        INVERSE,
        "D",
        &[("RUSTDL_CLASSIFY_NOMINAL_TYPES", "0")]
    ));
}
