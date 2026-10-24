//! #354 — classify uses individual equalities the KB forces.
//!
//! In the classify wedge a nominal `{a}` is an ordinary class, and individual
//! equality reached it only as an asserted `SameIndividual`. So `x : W`,
//! `W ⊑ ≤1 f`, `f(x,a)`, `f(x,b)` (which forces `a = b`) left `∃p.{a}` and
//! `∃p.{b}` apart, with `incomplete: false` (`ore_ont_7668`, wine:
//! `SauvignonBlanc ≡ Semillon`). Classify now confirms candidate equalities on the
//! `ABox`-seeded wedge (`KB ∪ {a ≠ b}` inconsistent) and adds them as
//! `SameIndividual` (`RUSTDL_CLASSIFY_DERIVED_SAME`, default ON).
//!
//! ORACLE: `HermiT` 1.4.5 (`y : C1 ⊓ ¬C2` inconsistent or not) agrees on every
//! verdict below.
//!
//! Every test locks the env mutex, since some set flags.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

const C1: &str = "http://ex.org/C1";
const C2: &str = "http://ex.org/C2";

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for one test and restores it on drop (the `adaptive_budget.rs`
/// pattern); callers hold [`lock`].
struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one test,
        // serialised via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// `C1 ≡ ∃p.{a}`, `C2 ≡ ∃p.{b}` plus `body`.
fn classify(body: &str) -> Classification {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C1)) Declaration(Class(:C2)) Declaration(Class(:K))\n\
         Declaration(ObjectProperty(:f)) Declaration(ObjectProperty(:g))\n\
         Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:a))\n\
         Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))\n\
         EquivalentClasses(:C1 ObjectHasValue(:p :a))\n\
         EquivalentClasses(:C2 ObjectHasValue(:p :b))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    classify_top_down_with_timeout(&onto, Duration::from_secs(30)).expect("classify")
}

fn equivalent(c: &Classification) -> bool {
    c.is_subclass(C1, C2) && c.is_subclass(C2, C1)
}

const MAX1: &str = "ClassAssertion(:W :x) SubClassOf(:W ObjectMaxCardinality(1 :f))\n\
     ObjectPropertyAssertion(:f :x :a) ObjectPropertyAssertion(:f :x :b)";

/// The issue's probe: a `≤1` on a class of `x` makes `a = b`.
#[test]
fn a_max_one_on_a_class_forces_the_equality() {
    let _g = lock();
    let c = classify(MAX1);
    assert!(equivalent(&c));
    assert_eq!(c.stats().derived_same, 1);
    assert_eq!(c.stats().derived_same_undecided, 0);
    assert_eq!(c.stats().timed_out_pairs, 0);
}

/// The wine shape: the equality follows only by contradiction. If `a ≠ b`, `x`
/// is an `M` (`≥2 g`), so its colour is `red`, but it is `white` and colour is
/// functional.
#[test]
fn an_equality_forced_by_contradiction_is_found() {
    let _g = lock();
    let c = classify(
        "Declaration(Class(:M)) FunctionalObjectProperty(:col)\n\
         EquivalentClasses(:M ObjectIntersectionOf(:W ObjectMinCardinality(2 :g)))\n\
         SubClassOf(:M ObjectHasValue(:col :red))\n\
         DifferentIndividuals(:red :white)\n\
         ClassAssertion(:W :x) ObjectPropertyAssertion(:col :x :white)\n\
         ObjectPropertyAssertion(:g :x :a) ObjectPropertyAssertion(:g :x :b)",
    );
    assert!(equivalent(&c));
}

/// The inverse shape: `a` and `b` share the object `x`, which allows one
/// `f`-predecessor.
#[test]
fn a_shared_object_with_an_inverse_bound_forces_the_equality() {
    let _g = lock();
    let c = classify(
        "ClassAssertion(:W :x) SubClassOf(:W ObjectMaxCardinality(1 ObjectInverseOf(:f)))\n\
         ObjectPropertyAssertion(:f :a :x) ObjectPropertyAssertion(:f :b :x)",
    );
    assert!(equivalent(&c));
}

/// Only `a` is a nominal of the TBox; `c` is not, but `a = c` gives `{a}` the type
/// `D` of `c`, so `C1 ⊑ K ≡ ∃p.D`.
#[test]
fn one_individual_in_reach_is_enough() {
    let _g = lock();
    let c = classify(
        "EquivalentClasses(:K ObjectSomeValuesFrom(:p :D))\n\
         ClassAssertion(:W :x) SubClassOf(:W ObjectMaxCardinality(1 :f))\n\
         ObjectPropertyAssertion(:f :x :a) ObjectPropertyAssertion(:f :x :c)\n\
         ClassAssertion(:D :c)",
    );
    assert!(c.is_subclass(C1, "http://ex.org/K"));
}

/// Controls. `≤2` over three objects forces no particular pair to be equal; no
/// bound at all forces nothing; neither may yield `C1 ≡ C2`.
#[test]
fn no_equality_is_invented() {
    let _g = lock();
    let c = classify(
        "ClassAssertion(:W :x) SubClassOf(:W ObjectMaxCardinality(2 :f))\n\
         ObjectPropertyAssertion(:f :x :a) ObjectPropertyAssertion(:f :x :b)\n\
         ObjectPropertyAssertion(:f :x :c)",
    );
    assert!(!c.is_subclass(C1, C2));
    assert_eq!(c.stats().derived_same, 0);
    let c = classify(
        "SubClassOf(:W ObjectMaxCardinality(1 :g))\n\
         ObjectPropertyAssertion(:f :x :a) ObjectPropertyAssertion(:f :x :b)",
    );
    assert!(!c.is_subclass(C1, C2));
}

/// `a ≠ b` asserted against the `≤1`: the KB is inconsistent and classify says so.
#[test]
fn an_inconsistent_kb_is_still_reported_inconsistent() {
    let _g = lock();
    let c = classify(&format!("{MAX1}\nDifferentIndividuals(:a :b)"));
    assert!(c.stats().inconsistent);
}

/// The probe cap at 0 decides nothing: the candidate is reported undecided (not as
/// a timed-out pair) and the equality is not used.
#[test]
fn a_candidate_past_the_cap_is_reported_undecided() {
    let _g = lock();
    let _env = SetEnvGuard::set("RUSTDL_CLASSIFY_DERIVED_SAME_MAX", "0");
    let c = classify(MAX1);
    assert!(!c.is_subclass(C1, C2));
    assert_eq!(c.stats().derived_same_undecided, 1);
    assert_eq!(c.stats().timed_out_pairs, 0);
}

/// `RUSTDL_CLASSIFY_DERIVED_SAME=0` reverts.
#[test]
fn the_flag_off_reverts() {
    let _g = lock();
    let _env = SetEnvGuard::set("RUSTDL_CLASSIFY_DERIVED_SAME", "0");
    let c = classify(MAX1);
    assert!(!c.is_subclass(C1, C2));
    assert_eq!(c.stats().derived_same, 0);
}
