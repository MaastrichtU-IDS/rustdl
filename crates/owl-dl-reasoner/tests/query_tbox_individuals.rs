//! #356 `RUSTDL_QUERY_TBOX_INDIVIDUALS` (default ON): the per-query tableau probes
//! seed every individual named only in a `TBox` nominal.
//!
//! A named individual exists in every model. `consistent` already seeded these
//! (`decide_consistency`), but the probe behind `sat`, `subclass`, the disjointness
//! and same-individual probes and realize did not, so a probe that never reached
//! `{b}` ignored what the `TBox` says about `b`. On a KB inconsistent only through
//! `b`, `consistent` said inconsistent and `sat Q` said `sat`.
//!
//! The controls are the false-positive side: seeding must add only real clashes.
//! There is no unique name assumption, so `{a} ⊑ {b}` merges and does not clash.
//!
//! Run: `cargo test -p owl-dl-reasoner --test query_tbox_individuals`

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

const FLAG: &str = "RUSTDL_QUERY_TBOX_INDIVIDUALS";
const NS: &str = "http://x.org/f#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://x.org/f>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

fn sat(o: &SetOntology<RcStr>, class: &str) -> bool {
    owl_dl_reasoner::is_class_satisfiable(o, &iri(class)).unwrap()
}

fn subclass(o: &SetOntology<RcStr>, sub: &str, sup: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(o, &iri(sub), &iri(sup)).unwrap()
}

/// The issue's reproducer (`o0082`, #288 fuzz). `b` occurs only in nominals;
/// `{b} ⊑ ≥2 r.(…)` with `r` functional means `b` cannot exist. HermiT: inconsistent.
const ISSUE_356: &str = "\
    Declaration(Class(:A0)) Declaration(Class(:A1)) Declaration(Class(:A2)) Declaration(Class(:A3))\n\
    SubClassOf(:A2 ObjectMinCardinality(3 :r ObjectOneOf(:b)))\n\
    SubClassOf(:A2 ObjectAllValuesFrom(:s ObjectOneOf(:b)))\n\
    SubClassOf(:A0 ObjectSomeValuesFrom(:r ObjectUnionOf(:A3 :A1 ObjectOneOf(:b))))\n\
    SubClassOf(:A3 ObjectMinCardinality(3 :r :A3))\n\
    SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(2 :r ObjectUnionOf(ObjectComplementOf(:A1) ObjectComplementOf(:A2) ObjectComplementOf(:A0))))\n\
    SubClassOf(:A3 ObjectUnionOf(:A0 ObjectComplementOf(:A0) ObjectComplementOf(:A0)))\n\
    SubClassOf(:A3 ObjectMinCardinality(3 :r owl:Thing))\n\
    SubClassOf(ObjectOneOf(:b) ObjectUnionOf(:A0 :A2))\n\
    SubClassOf(ObjectOneOf(:a) ObjectUnionOf(ObjectOneOf(:a) :A3 :A0))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(:Q ObjectIntersectionOf(ObjectUnionOf(ObjectOneOf(:b) ObjectOneOf(:a) :A3) ObjectMaxCardinality(1 :s owl:Thing) ObjectUnionOf(ObjectOneOf(:a) ObjectComplementOf(:A2))))";

/// The smallest shape that isolates the seed: `b` cannot exist, and `Q` never
/// mentions it.
const B_CANNOT_EXIST: &str = "\
    SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(2 :r owl:Thing))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(:Q :A)";

#[test]
fn issue_reproducer_q_is_unsatisfiable() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto(ISSUE_356);
    assert!(!sat(&o, "Q"), "inconsistent KB: Q is unsatisfiable");
    // On an inconsistent KB every subsumption holds.
    assert!(subclass(&o, "Q", "A1"), "inconsistent KB: Q ⊑ A1");
}

#[test]
fn an_individual_that_cannot_exist_makes_every_class_unsatisfiable() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto(B_CANNOT_EXIST);
    assert!(!sat(&o, "Q"));
    assert!(!sat(&o, "B"), "B is not mentioned anywhere and is still empty");
    assert!(subclass(&o, "Q", "C"));
}

/// The flag is load-bearing: off, both cases fall back to the old `sat`.
#[test]
fn flag_off_restores_the_old_answer() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _off = SetEnvGuard::set(FLAG, "0");
    assert!(sat(&onto(ISSUE_356), "Q"), "flag off: the #356 miss");
    assert!(sat(&onto(B_CANNOT_EXIST), "Q"), "flag off: the minimal miss");
}

/// No unique name assumption: `{a} ⊑ {b}` merges the two seeded nodes.
#[test]
fn control_nominal_subsumption_merges_and_does_not_clash() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto("SubClassOf(ObjectOneOf(:a) ObjectOneOf(:b))\nSubClassOf(:Q :A)");
    assert!(sat(&o, "Q"));
    assert!(!subclass(&o, "Q", "B"));
}

/// A disjunction at the seeded node: one branch is empty, the other is open.
#[test]
fn control_choice_at_a_seeded_individual() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto(
        "SubClassOf(ObjectOneOf(:b) ObjectUnionOf(:A :B))\n\
         SubClassOf(:A owl:Nothing)",
    );
    assert!(sat(&o, "Q"));
    assert!(sat(&o, "B"));
    assert!(!subclass(&o, "Q", "B"));
}

/// The probe reaches `b` itself and merges into the seeded node; `b`'s own
/// successor must not leak a type onto `Q`.
#[test]
fn control_probe_reaches_the_seeded_individual() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto(
        "SubClassOf(:Q ObjectIntersectionOf(ObjectSomeValuesFrom(:r ObjectOneOf(:b)) ObjectMaxCardinality(1 :r owl:Thing)))\n\
         SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:s :C))",
    );
    assert!(sat(&o, "Q"));
    assert!(!subclass(&o, "Q", "C"));
}

/// A seeded individual pointing at an `ABox` individual: `a` gains no type.
#[test]
fn control_instance_query_is_unchanged() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto(
        "ClassAssertion(:D :a)\n\
         SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:s ObjectOneOf(:a)))",
    );
    assert!(owl_dl_reasoner::is_instance_of(&o, &iri("D"), &iri("a")).unwrap());
    assert!(!owl_dl_reasoner::is_instance_of(&o, &iri("C"), &iri("a")).unwrap());
}

/// `same_individuals` probes `KB ∪ {a ≠ b}`. With `a`, `b` only in the `TBox`, the
/// `≠` fact used to find no node and was dropped, so `{a} ⊑ {b}` read "not same".
#[test]
fn same_individuals_sees_tbox_only_individuals() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto("SubClassOf(ObjectOneOf(:a) ObjectOneOf(:b))\nSubClassOf(:Q :A)");
    let same = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert_eq!(same.groups(), &[vec![iri("a"), iri("b")]]);
}

/// The frame counter `tbox-stats` reports: `b` is named only in the `TBox`; `a`
/// is declared but unused, so it is in no nominal and is not seeded.
#[test]
fn tbox_stats_counts_tbox_only_individuals() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let stats = owl_dl_reasoner::tbox_stats(&onto(B_CANNOT_EXIST)).unwrap();
    assert_eq!(stats.tbox_only_individuals, 1);
    let abox = onto("ClassAssertion(:A :b)\nSubClassOf(ObjectOneOf(:b) :A)");
    assert_eq!(owl_dl_reasoner::tbox_stats(&abox).unwrap().tbox_only_individuals, 0);
}

/// `disjoint_classes` runs classify, whose pre-check proves this KB inconsistent;
/// it used to go on to report "no disjoint pairs" (every class unsat).
#[test]
fn disjoint_classes_errors_when_classify_proves_inconsistency() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let r = owl_dl_reasoner::disjoint_classes(&onto(ISSUE_356), None);
    assert!(
        matches!(r, Err(owl_dl_reasoner::ReasonError::Inconsistent)),
        "expected Err(Inconsistent), got {:?}",
        r.map(|d| d.pairs().len())
    );
}
