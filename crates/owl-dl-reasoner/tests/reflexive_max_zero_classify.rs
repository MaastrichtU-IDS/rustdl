//! #535 at the classify level: the clausifier change (`add_counted_self_loops`
//! emits the self-loop under a `≤0` bound on a reflexive role) reaches the classify
//! wedge too, not only the consistency wedge. Expected answers are `HermiT`'s
//! classification of the same ontologies:
//!
//! - `K1`: `C` and `D` unsat, `B ⊑ A`, `E ⊑ Q`; `A`, `B`, `E` satisfiable.
//! - `K2`: `C` and `E` unsat; `A`, `B`, `D` satisfiable.
//!
//! Sabotage: `RUSTDL_REFLEXIVE_MAX_ZERO_LOOP=0` loses the unsat classes and `E ⊑ Q`
//! (`flag_off_reverts`); an over-strong `≤0 → ⊥` makes `A`/`B` unsat, which the
//! satisfiable controls catch.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const NS: &str = "http://ex/#";

/// `A ≡ ≤0 r.Q` on a reflexive `r` means "not `Q`" (each element is its own
/// `r`-successor), so `C ⊑ A ⊓ Q` is unsat; `D ⊑ ≤0 r⁻` is unsat; `E ⊑ ≤0 r ⊔ Q`
/// gives `E ⊑ Q`.
const K1: &str = "ReflexiveObjectProperty(:r) EquivalentClasses(:A ObjectMaxCardinality(0 :r :Q)) \
     SubClassOf(:B :A) SubClassOf(:C :A) SubClassOf(:C :Q) \
     SubClassOf(:D ObjectMaxCardinality(0 ObjectInverseOf(:r))) \
     SubClassOf(:E ObjectUnionOf(ObjectMaxCardinality(0 :r) :Q)) ReflexiveObjectProperty(:s)";

/// `s` reflexive, `r ⊑ s` (not reflexive): `≤0 r` is satisfiable, `≤0 s.Q` is "not
/// `Q`", and an `r`-successor in `Q` under `∀r.(≤0 s.Q)` clashes.
const K2: &str = "ReflexiveObjectProperty(:s) SubObjectPropertyOf(:r :s) \
     SubClassOf(:A ObjectMaxCardinality(0 :r)) SubClassOf(:B ObjectMaxCardinality(0 :s :Q)) \
     SubClassOf(:C ObjectIntersectionOf(:B :Q)) \
     SubClassOf(:D ObjectAllValuesFrom(:r ObjectMaxCardinality(0 :s :Q))) \
     SubClassOf(:E ObjectIntersectionOf(:D ObjectSomeValuesFrom(:r :Q)))";

fn classify(body: &str) -> owl_dl_reasoner::Classification {
    let ofn = format!(
        "Prefix(:=<{NS}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/k>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:Q)) \
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify(&onto).expect("classify")
}

fn unsat(c: &owl_dl_reasoner::Classification) -> Vec<String> {
    c.unsatisfiable_classes()
        .iter()
        .map(|s| s.trim_start_matches(NS).to_string())
        .collect()
}

fn sub(c: &owl_dl_reasoner::Classification, a: &str, b: &str) -> bool {
    c.is_subclass(&format!("{NS}{a}"), &format!("{NS}{b}"))
}

#[test]
fn k1_satisfiable_controls() {
    let _g = lock();
    let c = classify(K1);
    for name in ["A", "B", "E"] {
        assert!(
            !unsat(&c).contains(&name.to_string()),
            "{name} is satisfiable (HermiT)"
        );
    }
    assert!(sub(&c, "B", "A"));
}

#[test]
fn k1_reflexive_max_zero_unsat_and_entailment() {
    let _g = lock();
    let c = classify(K1);
    assert_eq!(unsat(&c), ["C", "D"], "HermiT: C and D unsat");
    assert!(sub(&c, "E", "Q"), "HermiT: E ⊑ Q");
}

#[test]
fn k2_satisfiable_controls() {
    let _g = lock();
    let c = classify(K2);
    for name in ["A", "B", "D"] {
        assert!(
            !unsat(&c).contains(&name.to_string()),
            "{name} is satisfiable (HermiT)"
        );
    }
}

#[test]
fn k2_reflexive_super_role_max_zero_unsat() {
    let _g = lock();
    let c = classify(K2);
    assert_eq!(unsat(&c), ["C", "E"], "HermiT: C and E unsat");
}

#[test]
fn flag_off_reverts() {
    let _g = lock();
    let _off = SetEnvGuard::set("RUSTDL_REFLEXIVE_MAX_ZERO_LOOP", "0");
    let c1 = classify(K1);
    assert!(
        unsat(&c1).is_empty() && !sub(&c1, "E", "Q"),
        "{:?}",
        unsat(&c1)
    );
    assert!(unsat(&classify(K2)).is_empty());
}
