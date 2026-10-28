//! #361 `RUSTDL_WEDGE_NOMINAL_BLOCK_GUARD` (default ON): the wedge's pair blocking
//! never blocks a node carrying a nominal, nor blocks against one or against a node
//! merged into another.
//!
//! `{a} : ≥3 s.{a,b}` is inconsistent: three pairwise-distinct `s`-fillers must each be
//! `a` or `b`. The wedge made three witnesses, blocked two by the first, and the first
//! then picked `{a}` and merged into `a` while staying their blocker. The disjunction
//! never ran on the other two, the pigeonhole clash never formed, and `consistent`
//! said consistent.
//!
//! Verdicts are HermiT's. The controls are the false-positive side.
//!
//! Run: `cargo test -p owl-dl-reasoner --test wedge_nominal_blocker`

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
         Ontology(<http://x.org/f>\n\
         Declaration(Class(:A)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:q))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

fn consistent(body: &str) -> bool {
    owl_dl_reasoner::is_consistent(&onto(body)).unwrap()
}

const I361: &str = "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :a)";

#[test]
fn issue_361_is_inconsistent() {
    let _l = lock();
    assert!(!consistent(I361));
}

/// The issue's variant table.
#[test]
fn issue_361_variants_are_inconsistent() {
    let _l = lock();
    for body in [
        "SubClassOf(ObjectOneOf(:a :b) ObjectMinCardinality(3 :s ObjectOneOf(:a :b)))",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectUnionOf(ObjectOneOf(:a) ObjectOneOf(:b))) :a)",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :c)",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :a)\nDifferentIndividuals(:a :b)",
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :s ObjectOneOf(:a :b)))",
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 ObjectInverseOf(:s) ObjectOneOf(:a :b)))",
    ] {
        assert!(!consistent(body), "should be inconsistent: {body}");
    }
}

/// Enough nominals for the witnesses: consistent.
#[test]
fn controls_stay_consistent() {
    let _l = lock();
    for body in [
        "ClassAssertion(ObjectMinCardinality(2 :s ObjectOneOf(:a :b)) :a)",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b :c)) :a)",
        "ClassAssertion(ObjectMinCardinality(2 :s ObjectOneOf(:a :b)) :c)",
    ] {
        assert!(consistent(body), "should be consistent: {body}");
    }
}

/// The flag is load-bearing: off, the wedge's `Sat` is back.
#[test]
fn flag_off_restores_the_old_answer() {
    let _l = lock();
    let _off = SetEnvGuard::set("RUSTDL_WEDGE_NOMINAL_BLOCK_GUARD", "0");
    assert!(consistent(I361), "flag off: the #361 miss");
}

/// Termination with the guard on: nominal nodes are never blockers, so a cycle
/// through a nominal must close by merging, not by blocking. Both are consistent.
#[test]
fn nominal_cycles_terminate() {
    let _l = lock();
    // #284's residual-free cycle, on an individual.
    assert!(consistent(
        "SubClassOf(:Q ObjectSomeValuesFrom(:s ObjectOneOf(:b)))\n\
         SubClassOf(ObjectOneOf(:b) :Q)\n\
         ClassAssertion(:Q :c)"
    ));
    // An `∃`-chain whose every node points back at `{a}`.
    assert!(consistent(
        "SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:s :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:A \
         ObjectSomeValuesFrom(:q ObjectOneOf(:a)))))"
    ));
}

/// `ABox`-only individuals reached through counting. The guard skips a nominal no
/// clause mentions, unless a clause counts predecessors. Forward functional `f`:
/// the copy of an individual has the same successors, so skipping is sound, and the
/// verdicts must still be right. Inverse-functional `g` counts predecessors, so
/// every nominal stays guarded. HermiT: as asserted.
#[test]
fn abox_only_individuals_through_counting() {
    let _l = lock();
    let base = "FunctionalObjectProperty(:s)\n\
        ClassAssertion(ObjectSomeValuesFrom(:s :A) :a)\n\
        ObjectPropertyAssertion(:s :a :b)\n\
        SubClassOf(:A ObjectSomeValuesFrom(:q :A))";
    assert!(consistent(base));
    assert!(!consistent(&format!(
        "{base}\nClassAssertion(:Q :b)\nDisjointClasses(:A :Q)"
    )));
    let inv = "InverseFunctionalObjectProperty(:q)\n\
        ClassAssertion(ObjectSomeValuesFrom(:q :A) :a)\n\
        ObjectPropertyAssertion(:q :b :c)\n\
        ClassAssertion(ObjectSomeValuesFrom(:q :A) :b)\n\
        SubClassOf(:A ObjectSomeValuesFrom(:s :A))";
    assert!(consistent(inv));
}
