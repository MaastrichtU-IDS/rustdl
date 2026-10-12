//! Soundness of the `⊔` look-ahead (`RUSTDL_SAT_LOOKAHEAD`, default OFF). Two
//! independent ways it reported consistent inputs inconsistent.
//!
//! 1. ID ALIASING. The look-ahead seeds `SeedSaturator::seed_unsat` with a wedge
//!    node's labels. Above the named-class range the wedge and the saturator number
//!    classes differently: the wedge's nominal class for individual `a` is
//!    `num_classes + i`, which the saturator uses for its own first synthetic, here
//!    the `⊥` key minted for `Z ⊑ ∃r.⊥`. So `{a}` was read as `⊥`, every disjunct
//!    at `a` looked dead, and `wine.ofn` was reported inconsistent.
//! 2. BACKJUMP DEPS. A disjunct the look-ahead drops is dead because of the node's
//!    labels, but the dropped disjunct's clash deps were never recorded. With every
//!    disjunct dropped, the clash carried only the clause body's deps, so the
//!    search backjumped past the decision that put the killing label there and
//!    never tried its alternative (found in review).
//!
//! Pruning still happens with the fixes: `owl-dl-tableau/tests/sat_lookahead_drop.rs`
//! asserts `lookahead_dropped ≥ 1`.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_lookahead_soundness`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Every test in this binary
        // holds ENV_MUTEX while the guard lives; restored on Drop.
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

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://t/x>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
         Declaration(Class(:Z)) Declaration(ObjectProperty(:r))\n\
         Declaration(Class(:R)) Declaration(Class(:L)) Declaration(Class(:Lp))\n\
         Declaration(Class(:S)) Declaration(Class(:D1)) Declaration(Class(:D2))\n\
         Declaration(Class(:K))\n\
         Declaration(NamedIndividual(:a))\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// `(is_consistent, classify's consistent)` with the look-ahead on.
fn with_lookahead(body: &str) -> (bool, bool) {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _l = SetEnvGuard::set("RUSTDL_SAT_LOOKAHEAD", "1");
    let o = onto(body);
    let consistent = owl_dl_reasoner::is_consistent(&o).unwrap();
    let c = owl_dl_reasoner::classify(&o).unwrap();
    (consistent, !c.stats().inconsistent)
}

/// The collision: `{a}`'s nominal id is the saturator's `⊥` key. Consistent.
#[test]
fn a_nominal_label_is_not_read_as_a_saturator_synthetic() {
    let (consistent, classify_consistent) = with_lookahead(
        "SubClassOf(:Z ObjectSomeValuesFrom(:r owl:Nothing))
         SubClassOf(:C ObjectUnionOf(:D :E))
         ClassAssertion(:C :a)",
    );
    assert!(
        consistent,
        "is_consistent must not report a false inconsistency"
    );
    assert!(
        classify_consistent,
        "classify must not report a false inconsistency"
    );
}

/// With both disjuncts genuinely unsatisfiable the disjunction still closes, so a
/// real inconsistency is still found. (This holds with the look-ahead off too; it
/// guards that the fix did not stop detection, not that pruning happens.)
#[test]
fn a_genuine_inconsistency_is_still_found() {
    let (consistent, classify_consistent) = with_lookahead(
        "SubClassOf(:D owl:Nothing) SubClassOf(:E owl:Nothing)
         SubClassOf(:C ObjectUnionOf(:D :E))
         ClassAssertion(:C :a)",
    );
    assert!(!consistent);
    assert!(!classify_consistent);
}

/// Under `L` the look-ahead kills `D1` (via `K`, which depends on choosing `L`) and
/// `D2` (`⊥`). The clash must carry `L`'s decision, so the search backtracks into
/// `Lp`, where `R` has a model.
const BACKJUMP: &str = "SubClassOf(:R ObjectUnionOf(:L :Lp))
     SubClassOf(:R ObjectSomeValuesFrom(:r :S))
     SubClassOf(:S ObjectUnionOf(:D1 :D2))
     SubClassOf(:D2 owl:Nothing)
     SubClassOf(:L ObjectAllValuesFrom(:r :K))
     DisjointClasses(:K :D1)";

#[test]
fn a_look_ahead_drop_carries_the_deps_of_the_killing_labels() {
    let (consistent, classify_consistent) =
        with_lookahead(&format!("{BACKJUMP} ClassAssertion(:R :a)"));
    assert!(
        consistent,
        "is_consistent must not report a false inconsistency"
    );
    assert!(classify_consistent);
}

/// The same backjump, `TBox` only: `R` must stay satisfiable and below `Lp`.
#[test]
fn a_look_ahead_drop_does_not_make_a_class_unsatisfiable() {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _l = SetEnvGuard::set("RUSTDL_SAT_LOOKAHEAD", "1");
    let c = owl_dl_reasoner::classify(&onto(BACKJUMP)).unwrap();
    assert!(
        !c.unsatisfiable_classes().contains(&"http://t/R"),
        "R is satisfiable (R ⊓ Lp ⊓ ∃r.(S ⊓ D1))"
    );
    assert!(c.is_subclass("http://t/R", "http://t/Lp"));
}
