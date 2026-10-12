//! The `⊔` look-ahead (`RUSTDL_SAT_LOOKAHEAD`, default OFF) must seed the EL
//! saturator with NAMED classes only.
//!
//! THE BUG. The look-ahead seeds `SeedSaturator::seed_unsat` with a wedge node's
//! labels. Above the named-class range the wedge and the saturator number classes
//! differently: the wedge's nominal class for individual `a` is `num_classes + i`,
//! which the saturator uses for its own first synthetic. Here that is the `⊥` key
//! minted for `Z ⊑ ∃r.⊥`. So the label `{a}` was read as `⊥`, every disjunct of
//! `C ⊑ D ⊔ E` at `a` looked dead, and a plainly consistent ontology was reported
//! inconsistent, as `wine.ofn` was (137 classes unsatisfiable). The filter only
//! skipped ids beyond the saturator's whole universe, which let this range through.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_lookahead_named_seeds`

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

/// Non-vacuity: with both disjuncts genuinely unsatisfiable the look-ahead
/// still closes the disjunction, so a real inconsistency is still found.
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
