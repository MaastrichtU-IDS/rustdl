//! #274 `RUSTDL_SAT_INVERSE_FORALL_NOMINAL` (default ON): an inverse-role edge into a
//! top-level `∀R.{a₁…aₙ}`.
//!
//! `A ⊑ ∀R.{a}`, `S ≡ R⁻`, `C ⊑ ∃S.A`, `a : D` entails `C ⊑ D`: a `C` is an
//! `R`-successor of an `A`, so it is `a`. `classify` missed it while reporting
//! `incomplete: false`, because the inverse-`∀` lowering visited only the atomic-filler
//! keys and never the nominal-set key `ForallKey(R, {a})`.
//!
//! The saturator cannot assert the disjunction `{a₁…aₙ}`, so the trigger is headed by
//! the classes every member is asserted to. Konclude and `HermiT` agree with every verdict
//! below (2026-10-16), including the three FP guards, where both stay silent.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_inverse_forall_nominal`

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
    Declaration(Class(:E)) Declaration(Class(:F)) Declaration(Class(:G))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S)) Declaration(ObjectProperty(:T))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
";

/// The issue's reproducer.
const SINGLE: &str = "    SubClassOf(:A ObjectAllValuesFrom(:R ObjectOneOf(:a)))
    InverseObjectProperties(:S :R)
    SubClassOf(:C ObjectSomeValuesFrom(:S :A))
    ClassAssertion(:D :a)";

/// Two members: `C ⊑ G`, the one class both are typed into (through told
/// `D ⊑ G`, `E ⊑ G`). `D` and `F` hold of `a` only, so `C ⊑ D` and `C ⊑ F` do not.
const MULTI: &str = "    SubClassOf(:A ObjectAllValuesFrom(:R ObjectOneOf(:a :b)))
    InverseObjectProperties(:S :R)
    SubClassOf(:C ObjectSomeValuesFrom(:S :A))
    ClassAssertion(:D :a)
    ClassAssertion(:E :b)
    ClassAssertion(:F :a)
    SubClassOf(:D :G)
    SubClassOf(:E :G)";

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
fn inverse_edge_into_a_singleton_forall_is_derived() {
    assert!(c_subsumed_by(SINGLE, "D", &[]));
}

/// Also with Layer A off, the above-cap condition where the wedge cannot read the
/// inverse, so only the saturator rule can answer.
#[test]
fn singleton_is_derived_with_layer_a_off() {
    assert!(c_subsumed_by(
        SINGLE,
        "D",
        &[("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0")]
    ));
}

#[test]
fn inverse_edge_into_a_set_forall_gets_the_shared_type() {
    assert!(c_subsumed_by(MULTI, "G", &[]));
}

// ---------------------------------------------------------------- negatives

/// FP guard: a type of only ONE member must not be asserted of the source.
#[test]
fn a_type_of_one_member_is_not_derived() {
    assert!(!c_subsumed_by(MULTI, "D", &[]));
    assert!(!c_subsumed_by(MULTI, "F", &[]));
}

/// FP guard: a member with no asserted type empties the intersection.
#[test]
fn an_untyped_member_blocks_the_shared_type() {
    let body = MULTI.replace("ClassAssertion(:E :b)", "");
    assert!(!c_subsumed_by(&body, "G", &[]));
}

/// FP guard: a FORWARD sub-role edge into `∀R.{a}` says nothing about its source.
/// `R ≡ T⁻` keeps the rule live (it runs only when some axiom crosses polarity);
/// without it this guard passes with the polarity check deleted.
#[test]
fn a_forward_edge_entails_nothing() {
    let body = SINGLE.replace(
        "InverseObjectProperties(:S :R)",
        "SubObjectPropertyOf(:S :R)\n    InverseObjectProperties(:R :T)",
    );
    assert!(!c_subsumed_by(&body, "D", &[]));
}

// ------------------------------------------------- the flag is load-bearing

/// `=0` loses both positives, so they are this rule's doing.
#[test]
fn flag_off_loses_the_entailments() {
    let off = [("RUSTDL_SAT_INVERSE_FORALL_NOMINAL", "0")];
    assert!(!c_subsumed_by(SINGLE, "D", &off));
    assert!(!c_subsumed_by(MULTI, "G", &off));
}
