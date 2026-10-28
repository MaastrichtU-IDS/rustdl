//! #373: a `¬∃r.Self` clash (how `Irreflexive(r)` is enforced in the main tableau)
//! took only the `¬Self` label's deps, not the self-edge's. That label is added
//! deterministically at every node, so its deps are empty. When a search decision
//! made the self-loop (a `≤n` merge, or a disjunct `∃q.Self` with `q ⊑ r`), the
//! clash did not depend on the decision, the search back-jumped past it, and a
//! satisfiable class read unsatisfiable: a false positive.
//!
//! Verdicts are HermiT's, or follow from the model given in the test.
//!
//! Run: `cargo test -p owl-dl-reasoner --test self_clash_edge_deps`

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

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

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const NS: &str = "http://x.org/f#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://x.org/f>\n\
         Declaration(Class(:A)) Declaration(Class(:C))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:t))\n\
         Declaration(NamedIndividual(:a))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

fn sat(body: &str) -> bool {
    owl_dl_reasoner::is_class_satisfiable(&onto(body), &format!("{NS}A")).unwrap()
}

/// Every `A` has an `r`-successor in `A` and the `s`-predecessor `a`.
const CHAIN: &str = "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
    ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a))))\n";

/// The issue's reproducer. Model: `{a, y0, y1}`, `r = {(y0,y1),(y1,y0)}`,
/// `s = {(a,y0),(a,y1)}`. The first `≤2` merge makes a self-loop; the second
/// option must be tried.
#[test]
fn issue_373_merge_self_loop_is_satisfiable() {
    let _l = lock();
    assert!(sat(&format!(
        "{CHAIN}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
         IrreflexiveObjectProperty(:r)"
    )));
}

/// On `subclass` the false `A ⊑ ⊥` became a false subsumption of an unrelated
/// class `C`.
#[test]
fn issue_373_no_false_subsumption() {
    let _l = lock();
    let o = onto(&format!(
        "{CHAIN}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
         IrreflexiveObjectProperty(:r)"
    ));
    assert!(!owl_dl_reasoner::is_subclass_of(&o, &format!("{NS}A"), &format!("{NS}C")).unwrap());
}

/// The same defect with no `≤n` at all: the disjunct `∃q.Self` makes the self-loop,
/// `q ⊑ r` makes it clash with `Irreflexive(r)`, and the other disjunct is open.
/// The alternative is non-atomic so it is not tried first.
#[test]
fn disjunction_self_loop_through_sub_role_is_satisfiable() {
    let _l = lock();
    assert!(sat(
        "SubClassOf(:A ObjectUnionOf(ObjectHasSelf(:q) ObjectSomeValuesFrom(:t :C)))\n\
         SubObjectPropertyOf(:q :r)\n\
         IrreflexiveObjectProperty(:r)"
    ));
}

// ---- controls: unsatisfiable, and must stay so ----

#[test]
fn controls_stay_unsatisfiable() {
    let _l = lock();
    // Asymmetry leaves no merge that avoids an `r` cycle.
    assert!(!sat(&format!(
        "{CHAIN}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
         IrreflexiveObjectProperty(:r)\nAsymmetricObjectProperty(:r)"
    )));
    // The self-loop is forced, not chosen.
    assert!(!sat(
        "SubClassOf(:A ObjectHasSelf(:r))\nIrreflexiveObjectProperty(:r)"
    ));
    // Forced through a sub-role.
    assert!(!sat(
        "SubClassOf(:A ObjectHasSelf(:q))\nSubObjectPropertyOf(:q :r)\n\
         IrreflexiveObjectProperty(:r)"
    ));
    // Both disjuncts clash.
    assert!(!sat(
        "SubClassOf(:A ObjectUnionOf(ObjectHasSelf(:q) ObjectHasSelf(:r)))\n\
         SubObjectPropertyOf(:q :r)\n\
         IrreflexiveObjectProperty(:r)"
    ));
}

#[test]
fn controls_stay_satisfiable() {
    let _l = lock();
    // `≤3` leaves room without a merge.
    assert!(sat(&format!(
        "{CHAIN}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(3 :s))\n\
         IrreflexiveObjectProperty(:r)"
    )));
}

/// With the `≤n` choice point off, `apply_max` merges by trial and keeps the first
/// pair whose survivor is clash-free. That check now includes `¬∃r.Self`, so the
/// self-looping pair is rejected and the other one is kept.
#[test]
fn trial_merge_rejects_a_self_loop() {
    let _l = lock();
    let _off = SetEnvGuard::set("RUSTDL_MAX_CHOICE_POINT", "0");
    assert!(sat(&format!(
        "{CHAIN}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
         IrreflexiveObjectProperty(:r)"
    )));
}

/// The `≤1` sibling, which did not terminate before #362: each merge reparented a
/// child that nothing could block. The merge-before-expand order (#362) settles
/// it: `A` is satisfiable (`{a, y}`, `r = {(y,y)}`, `s = {(a,y)}`).
#[test]
fn max_one_merge_terminates() {
    let _l = lock();
    let r = owl_dl_reasoner::is_class_satisfiable_with_timeout(
        &onto(&format!(
            "{CHAIN}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :s))"
        )),
        &format!("{NS}A"),
        std::time::Duration::from_secs(20),
    );
    assert!(matches!(r, Ok(Some(true))), "{r:?}");
}
