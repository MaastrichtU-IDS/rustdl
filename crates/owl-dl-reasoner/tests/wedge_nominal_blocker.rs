//! #361 `RUSTDL_WEDGE_NOMINAL_BLOCK_GUARD` (default ON): in the wedge's pair blocking,
//! a node carrying a nominal a blocked copy could contradict (one a clause head forces,
//! or any when a `≤` counts predecessors) is never a blocker.
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
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:t))\n\
         Declaration(Class(:B))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:d))\n\
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

/// Termination with the guard on: visible-nominal nodes are never blockers, so a cycle
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
/// every nominal stays guarded; that case shows only that it still terminates, since
/// the answer is the same with the guard off. HermiT: as asserted.
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

// ---- #376: a blocker merely NEXT TO an individual (the wedge's NN-rule half) ----

/// The `r`-witnesses are pairwise distinct and each is an `s`-predecessor of an
/// individual whose `s` is functional, so the witnesses must merge, against `≠`.
/// The wedge blocked the second witness by the first, which carries no nominal
/// but is next to `a`; a copy of it adds a second `s`-neighbour to `a`.
/// HermiT: inconsistent.
#[test]
fn issue_376_abox_forms_are_inconsistent() {
    let _l = lock();
    for body in [
        // #362's shape on an individual.
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         FunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         ClassAssertion(:Q :d)",
        // #361's `p0289` reduction: the `≥3` on a nominal.
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b)))\n\
         FunctionalObjectProperty(:s)",
        // S1: three witnesses, `≤2`. The third was blocked by a witness that had
        // not yet reached `b`, and one re-check of the skip was not enough.
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b)))\n\
         SubClassOf(ObjectOneOf(:b) ObjectMaxCardinality(2 :s))",
        // S1 with `≤1`.
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b)))\n\
         SubClassOf(ObjectOneOf(:b) ObjectMaxCardinality(1 :s))",
        // The nominal two edges away (the SHOIQ NN-rule cascade): the `B`s merge
        // at `a`, and the `≤1 t⁻` on `B` then merges the `A`s.
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :B))\n\
         SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(:B ObjectMaxCardinality(1 ObjectInverseOf(:t)))\n\
         FunctionalObjectProperty(:s)\n\
         ClassAssertion(:Q :d)",
    ] {
        assert!(!consistent(body), "should be inconsistent: {body}");
    }
}

/// Controls: no functional role, or room for two, or two nominals to point at.
#[test]
fn issue_376_controls_stay_consistent() {
    let _l = lock();
    for body in [
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         ClassAssertion(:Q :d)",
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         ClassAssertion(:Q :d)",
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a :b)))\n\
         FunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         ClassAssertion(:Q :d)",
        // Two edges away, but nothing bounds the `t`-predecessors of `B`.
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :B))\n\
         SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         FunctionalObjectProperty(:s)\n\
         ClassAssertion(:Q :d)",
        // Forward successors of `a` only: `≥2` within `≤3`.
        "SubClassOf(ObjectOneOf(:a) ObjectIntersectionOf(ObjectMinCardinality(2 :r :A) \
         ObjectMaxCardinality(3 :r)))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :A))",
    ] {
        assert!(consistent(body), "should be consistent: {body}");
    }
}

/// KNOWN LIMITATION: the cascade shape with `InverseFunctional(t)` instead of an
/// explicit `≤1 t⁻` on `B`. The wedge needs the `≤1 t⁻` that conversion emits for
/// an inverse-functional role, and `inv_func_merge_consumable` (owl-dl-core
/// convert.rs) declines it for a single non-nominal `∃t` generator; that gate is a
/// recorded cost decision (19–47× on SIO). HermiT: inconsistent. FLIP if the gate
/// admits this shape.
#[test]
fn known_inverse_functional_bound_not_emitted() {
    let _l = lock();
    assert!(
        consistent(
            "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:t :B))\n\
             SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
             FunctionalObjectProperty(:s)\n\
             InverseFunctionalObjectProperty(:t)\n\
             ClassAssertion(:Q :d)"
        ),
        "FLIP: the inverse-functional bound reaches this shape now"
    );
}

#[test]
fn issue_376_flag_off_restores_the_old_answer() {
    let _l = lock();
    let _off = SetEnvGuard::set("RUSTDL_WEDGE_NN_GUARD", "0");
    assert!(consistent(
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         FunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         ClassAssertion(:Q :d)"
    ));
}
