//! #279 `RUSTDL_SAT_NOMINAL_EDGES` (default ON): property assertions of individuals
//! used as nominals reach the EL saturator as `NomKey(a) ⊑ ∃S.NomKey(c)`.
//!
//! `X ⊑ ∃R.{a}`, `a S c`, `c : E`, `W ≡ ∃R.∃S.E` entails `X ⊑ W`. The classify wedge
//! can read the edge only through generating clauses, which cost up to 65× on ORE
//! (`RUSTDL_CLASSIFY_NOMINAL_OPA`, default OFF), so `classify` missed it while
//! reporting `incomplete: false`. In the saturator the same facts are polynomial.
//! Konclude and `HermiT` agree with every verdict below (2026-10-16).
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_nominal_edges`

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
    Declaration(Class(:F)) Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:T)) Declaration(NamedIndividual(:a))
    Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
";

/// The forward reproducer.
const FORWARD: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :a :c)
    ClassAssertion(:E :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))";

/// Classify with `extra` env settings and report `X ⊑ W`.
fn x_is_w(body: &str, extra: &[(&'static str, &str)]) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flags: Vec<SetEnvGuard> = extra.iter().map(|(k, v)| SetEnvGuard::set(k, v)).collect();
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    c.is_subclass("http://t/X", "http://t/W")
}

// ---------------------------------------------------------------- positives

#[test]
fn a_forward_edge_of_a_nominal_is_followed() {
    assert!(x_is_w(FORWARD, &[]));
}

/// A cycle `a S c`, `c S a` with a deep definition: the wedge's blocking cuts such a
/// cycle, the saturator does not need to unroll it.
#[test]
fn a_cycle_of_edges_is_followed_to_any_depth() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :a :c)
    ObjectPropertyAssertion(:S :c :a)
    ClassAssertion(:E :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S
      ObjectSomeValuesFrom(:S ObjectSomeValuesFrom(:S :E)))))";
    assert!(x_is_w(body, &[]));
}

/// An assertion through an inverse role is read forwards: `S⁻(c, a)` is `S(a, c)`.
#[test]
fn an_inverse_role_assertion_is_read_forwards() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(ObjectInverseOf(:S) :c :a)",
    );
    assert!(x_is_w(&body, &[]));
}

/// The range of a super-role types the edge's target.
#[test]
fn the_range_of_a_super_role_types_the_target() {
    let body = FORWARD.replace("ClassAssertion(:E :c)", "").replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(:S :a :c)
    SubObjectPropertyOf(:S :T)
    ObjectPropertyRange(:T :E)",
    );
    assert!(x_is_w(&body, &[]));
}

/// `SameIndividual(a, b)` with the edge on `b`.
#[test]
fn an_edge_of_a_same_individual_is_followed() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "SameIndividual(:a :b)
    ObjectPropertyAssertion(:S :b :c)",
    );
    assert!(x_is_w(&body, &[]));
}

// ---------------------------------------------------------------- negatives

/// FP guard: the edge goes the other way (`c S a`), so `a` has no `S`-successor.
#[test]
fn a_reversed_edge_entails_nothing() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(:S :c :a)",
    );
    assert!(!x_is_w(&body, &[]));
}

/// FP guard: the edge is on another role.
#[test]
fn an_edge_on_another_role_entails_nothing() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(:T :a :c)",
    );
    assert!(!x_is_w(&body, &[]));
}

/// FP guard: the edge's target lacks `E`; a type of another individual is not its.
#[test]
fn a_type_of_another_individual_entails_nothing() {
    let body = FORWARD.replace("ClassAssertion(:E :c)", "ClassAssertion(:E :b)");
    assert!(!x_is_w(&body, &[]));
}

// ------------------------------------------------- the flag is load-bearing

/// `=0` loses the entailment (the wedge's own edge clauses are default OFF).
#[test]
fn flag_off_loses_the_entailment() {
    assert!(!x_is_w(FORWARD, &[("RUSTDL_SAT_NOMINAL_EDGES", "0")]));
}
