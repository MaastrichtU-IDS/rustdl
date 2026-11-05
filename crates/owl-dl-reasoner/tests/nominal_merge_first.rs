//! #460: a node bound to a nominal merges into the nominal's node before it
//! generates successors (`RUSTDL_NOMINAL_MERGE_FIRST`, default ON).
//!
//! With `C ⊑ {a}` and `C ⊑ ∃r.C` (directly or through property domains), the main
//! tableau built each fresh `C` node's successor before merging the node into `a`.
//! The merge moved that successor onto `a`, where it did the same, without end, and
//! `sat`, `subclass`, `explain` and Python `is_subclass_of` never returned on 3- and
//! 4-axiom KBs that classify answers at once (generated cases 1911, 812, 939).
//!
//! The danger in this direction is a wrong `true` from a node that never
//! generated, so most canaries here are UNSATISFIABLE classes whose clash runs
//! through the nominal, the merge, or a successor the merged node must still build.
//! They are checked with the flag on and off. Each query runs on a worker thread
//! under a time limit, so a regression fails instead of hanging the suite.

#![allow(clippy::unwrap_used)]

use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{is_class_satisfiable, is_class_satisfiable_with_timeout, is_subclass_of};

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for its lifetime; callers hold `lock()`.
struct EnvGuard(&'static str);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(k: &'static str, v: &str) -> Self {
        // SAFETY: every env access in this binary holds `lock()`.
        unsafe { std::env::set_var(k, v) };
        Self(k)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above.
        unsafe { std::env::remove_var(self.0) };
    }
}

const NS: &str = "http://ex/#";

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/>\n{DECLS}\n{axioms}\n)"
    );
    let (onto, _) = read_ofn(
        &mut std::io::Cursor::new(src),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    onto
}

const DECLS: &str = "Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E)) \
     Declaration(Class(:F)) Declaration(Class(:G)) Declaration(Class(:C0)) \
     Declaration(Class(:C1)) Declaration(Class(:C2)) Declaration(Class(:C5)) \
     Declaration(Class(:C7)) Declaration(Class(:C3)) Declaration(Class(:C4)) \
     Declaration(Class(:C6)) Declaration(Class(:C8)) Declaration(ObjectProperty(:r2)) \
     Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
     Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1)) \
     Declaration(ObjectProperty(:r3)) \
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
     Declaration(NamedIndividual(:a0)) Declaration(NamedIndividual(:a1))";

/// The issue's 4-axiom shrink of generated case 1911 (seed 20261103). `C7` is
/// satisfiable: `a1 ∈ C7` with `r3(a1, a1)` and three `r1`-successors in `C5`.
const CASE_1911: &str = "ObjectPropertyDomain(:r1 :C7) ObjectPropertyDomain(:r1 :C1) \
     SubClassOf(:C1 ObjectOneOf(:a1)) \
     SubClassOf(:C7 ObjectSomeValuesFrom(:r3 ObjectExactCardinality(3 :r1 :C5)))";

/// The 3-axiom shrink of generated case 812. `C2` is satisfiable: `a0 ∈ C2` with
/// `r0(a0, a0)`, hence `r1(a0, a0)`.
const CASE_812: &str = "SubObjectPropertyOf(ObjectInverseOf(:r0) ObjectInverseOf(:r1)) \
     SubObjectPropertyOf(ObjectPropertyChain(:r1 :r1) :r1) \
     SubClassOf(:C2 ObjectIntersectionOf(ObjectSomeValuesFrom(:r0 :C2) ObjectOneOf(:a0)))";

/// Generated case 1908 (seed 20261103). Every element is `a0` (`⊤ ⊑ C1 ≡ C4 ⊑ C7 ⊑
/// C3 ≡ {a0}`), and `{a0}` reaches a fresh node only through that told chain, a
/// pass or more after its `∃r2`. With the merge-first rule alone the `∃` fired
/// first and the run never finished (main gives up with `NoVerdict`); classify
/// says consistent with no unsatisfiable class.
const CASE_1908: &str = "ObjectPropertyDomain(ObjectInverseOf(:r1) :C8) \
     EquivalentClasses(:C3 ObjectOneOf(:a0)) SubClassOf(owl:Thing :C1) SubClassOf(:C4 :C7) \
     EquivalentClasses(:C8 :C6) SubClassOf(:C7 :C3) EquivalentClasses(:C4 :C1) \
     EquivalentClasses(:C4 ObjectSomeValuesFrom(:r2 \
       ObjectExactCardinality(1 ObjectInverseOf(:r0) :C0)))";

/// Runs `f` on a worker thread and waits at most `secs`. A query that does not
/// return in time fails the test (its thread is left running until exit).
fn within<T: Send + 'static>(secs: u64, what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(secs))
        .unwrap_or_else(|_| panic!("{what}: no answer within {secs} s"))
}

fn sat(name: &str, axioms: &str, class: &str) -> bool {
    // Parsed on the worker: the ontology is not `Send`.
    let axioms = axioms.to_owned();
    let iri = format!("{NS}{class}");
    within(60, name, move || {
        is_class_satisfiable(&parse(&axioms), &iri).expect("decides")
    })
}

/// The reproducers, and satisfiable controls of the same shape: answered `true`
/// (the models are the nominal with a self-loop).
const SATISFIABLE: &[(&str, &str, &str)] = &[
    ("case_1911", CASE_1911, "C7"),
    ("case_812", CASE_812, "C2"),
    ("case_1908_c0", CASE_1908, "C0"),
    ("case_1908_c1", CASE_1908, "C1"),
    (
        "self_loop_on_the_nominal",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :C))) \
         SubClassOf(ObjectOneOf(:a) :D)",
        "C",
    ),
    (
        "seeded_individual_with_a_second_successor",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :C) \
           ObjectSomeValuesFrom(:s :E))) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:s) :F)) ClassAssertion(:G :a)",
        "C",
    ),
    (
        "nominal_or_free_chain",
        "SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:r :C) \
           ObjectUnionOf(ObjectOneOf(:a) :D))) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:C))) \
         ClassAssertion(:G :a)",
        "C",
    ),
];

/// Unsatisfiable classes whose clash needs the nominal, the merge, or a successor
/// the merged node must still build. A deferral that withholds generation for
/// good, or loses a label in the merge, would turn one of these into `true`.
const UNSATISFIABLE: &[(&str, &str, &str)] = &[
    (
        "successor_of_the_nominal_is_excluded",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :C))) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:C)))",
        "C",
    ),
    (
        "label_carried_through_the_merge",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :D))) \
         SubClassOf(:D ObjectIntersectionOf(ObjectOneOf(:a) :E)) \
         SubClassOf(:C ObjectComplementOf(:E))",
        "C",
    ),
    (
        "inverse_universal_back_to_the_nominal",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :D))) \
         SubClassOf(:D ObjectSomeValuesFrom(:r :E)) SubClassOf(:E ObjectOneOf(:a)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(ObjectInverseOf(:r) \
           ObjectComplementOf(:D)))",
        "C",
    ),
    // The query root is not `a`'s canonical node (the seeded individual is), so
    // it waits; its `∃s.E` must still be built on `a`'s node for the clash.
    (
        "seeded_individual_must_still_generate",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :C) \
           ObjectSomeValuesFrom(:s :E))) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:F))) \
         ClassAssertion(:F :a)",
        "C",
    ),
    (
        "two_nominals",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :D))) \
         SubClassOf(:D ObjectIntersectionOf(ObjectOneOf(:b) \
           ObjectSomeValuesFrom(:r ObjectOneOf(:a)))) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D)))",
        "C",
    ),
    (
        "different_individuals_refuse_the_merge",
        "SubClassOf(:C ObjectIntersectionOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r :D))) \
         SubClassOf(:D ObjectIntersectionOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:r :C))) \
         SubClassOf(:D ObjectOneOf(:a)) DifferentIndividuals(:a :b)",
        "C",
    ),
    // `{a}` holds on one branch only; both branches must generate to clash.
    (
        "branch_dependent_nominal",
        "SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:r :C) \
           ObjectUnionOf(ObjectOneOf(:a) :D))) \
         SubClassOf(:D ObjectAllValuesFrom(:r ObjectComplementOf(:C))) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:C))) \
         ClassAssertion(:G :a)",
        "C",
    ),
];

#[test]
fn nominal_bound_classes_with_a_self_successor_are_decided() {
    let _l = lock();
    for &(name, axioms, class) in SATISFIABLE {
        assert!(sat(name, axioms, class), "{name}: {class} is satisfiable");
    }
}

#[test]
fn unsatisfiable_classes_stay_unsatisfiable_with_the_flag_on_and_off() {
    let _l = lock();
    for flag in ["1", "0"] {
        let _g = EnvGuard::set("RUSTDL_NOMINAL_MERGE_FIRST", flag);
        for &(name, axioms, class) in UNSATISFIABLE {
            assert!(
                !sat(name, axioms, class),
                "{name} (RUSTDL_NOMINAL_MERGE_FIRST={flag}): {class} is unsatisfiable"
            );
        }
    }
}

/// `subclass` decides a non-subsumption by the same satisfiability test, so it
/// hung on the same inputs (the issue's `subclass C0 C1`).
#[test]
fn non_subsumptions_on_the_reproducers_are_decided() {
    let _l = lock();
    for (name, axioms, sub, sup) in [
        ("case_1911", CASE_1911, "C7", "C5"),
        ("case_812", CASE_812, "C2", "C0"),
    ] {
        let axioms = axioms.to_owned();
        let (sub_iri, sup_iri) = (format!("{NS}{sub}"), format!("{NS}{sup}"));
        let entailed = within(60, name, move || {
            is_subclass_of(&parse(&axioms), &sub_iri, &sup_iri).expect("decides")
        });
        assert!(!entailed, "{name}: {sub} ⊑ {sup} is not entailed");
    }
}

/// With the flag off the old order returns: the reproducer runs past a short
/// deadline instead of answering.
#[test]
fn flag_off_restores_the_old_rule_order() {
    let _l = lock();
    let _g = EnvGuard::set("RUSTDL_NOMINAL_MERGE_FIRST", "0");
    let onto = parse(CASE_812);
    let verdict =
        is_class_satisfiable_with_timeout(&onto, &format!("{NS}C2"), Duration::from_secs(2))
            .expect("no error");
    assert_eq!(verdict, None, "flag off: the old order does not finish");
}
