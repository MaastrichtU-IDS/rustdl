//! #473: a node waits to generate successors while it carries an open disjunction
//! with a nominal disjunct (`RUSTDL_NOMINAL_DISJUNCTION_WAIT`, default ON).
//!
//! #460's merge-first rule holds back a node bound to one nominal `{a}`. With
//! `C ⊑ {a} ⊔ {b}` and `C ⊑ ∃r.C` a fresh `C` node is bound to neither until the
//! search picks a disjunct, so it built its successor first; the search then merged
//! it into `a` or `b`, and the moved successor did the same, without end. `sat` and
//! `subclass` never finished on generated case 939, which classify answers.
//!
//! The danger in this direction is a wrong `true` from a node that never
//! generated, so most canaries here are UNSATISFIABLE classes whose clash needs the
//! `∃` / `≥` the waiting node must still build after the merge. They are checked
//! with the flag on and off. Each query runs on a worker thread under a time limit,
//! so a regression fails instead of hanging the suite.

#![allow(clippy::unwrap_used)]

use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    is_class_satisfiable, is_class_satisfiable_with_timeout, is_consistent, is_subclass_of,
};

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
const FLAG: &str = "RUSTDL_NOMINAL_DISJUNCTION_WAIT";

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
     Declaration(Class(:F)) Declaration(Class(:G)) Declaration(Class(:X)) \
     Declaration(Class(:C0)) Declaration(Class(:C1)) Declaration(Class(:C2)) \
     Declaration(Class(:C3)) Declaration(Class(:C4)) \
     Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
     Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1)) \
     Declaration(DataProperty(:d0)) \
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
     Declaration(NamedIndividual(:i)) Declaration(NamedIndividual(:a0)) \
     Declaration(NamedIndividual(:a2))";

/// The 3-axiom shrink of generated case 939 (seed 20261103). Every element is in
/// `C1 = ¬C3 ⊓ {a2, a0}`, so the `Or` reaches a fresh node through an `And` the
/// concept rule adds at once (the materialized branch of the wait). `C1` is
/// satisfiable: `a2 ∈ C1` with `r1(a2, a2)`.
const MIN_939: &str = "EquivalentClasses(:C1 ObjectIntersectionOf(ObjectComplementOf(:C3) \
       ObjectOneOf(:a2 :a0))) \
     EquivalentClasses(:C4 :C1) \
     EquivalentClasses(:C4 ObjectSomeValuesFrom(:r1 :C4))";

/// Generated case 939 itself (14 axioms). Classify and the Konclude/HermiT oracle
/// agree that it is consistent and that `C1`, `C2` are satisfiable.
const CASE_939: &str = "TransitiveObjectProperty(:r1) \
     EquivalentClasses(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0))) \
     SubClassOf(ObjectComplementOf(:C0) :C4) \
     SubClassOf(:C3 ObjectSomeValuesFrom(:r1 ObjectOneOf(:a2))) \
     SubClassOf(ObjectSomeValuesFrom(:r0 ObjectOneOf(:a2)) :C0) \
     DataPropertyDomain(:d0 :C1) \
     EquivalentClasses(:C1 ObjectIntersectionOf(ObjectComplementOf(:C3) ObjectOneOf(:a2 :a0))) \
     EquivalentClasses(:C0 :C4) EquivalentClasses(:C4 :C1) \
     EquivalentClasses(:C4 ObjectSomeValuesFrom(:r1 :C4)) SubClassOf(:C0 :C4) \
     SubClassOf(:C2 ObjectSomeValuesFrom(:r0 ObjectUnionOf(:C1 :C0))) \
     SubClassOf(:C3 ObjectUnionOf(:C4 ObjectExactCardinality(0 :r0 :C2))) \
     InverseFunctionalObjectProperty(:r0)";

/// The `Or` as a concept rule's own conclusion (`C1 ⊑ {a} ⊔ {b}`): it is not on a
/// fresh node until the stable-state sweep materializes it, so this pins the
/// pending branch of the wait. `C1` is satisfiable: `a ∈ C1` with `r1(a, a)`.
const DIRECT_OR: &str = "SubClassOf(:C1 ObjectOneOf(:a :b)) EquivalentClasses(:C4 :C1) \
     EquivalentClasses(:C4 ObjectSomeValuesFrom(:r1 :C4))";

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

fn subsumed(name: &str, axioms: &str, sub: &str, sup: &str) -> bool {
    let axioms = axioms.to_owned();
    let (sub, sup) = (format!("{NS}{sub}"), format!("{NS}{sup}"));
    within(60, name, move || {
        is_subclass_of(&parse(&axioms), &sub, &sup).expect("decides")
    })
}

fn consistent(name: &str, axioms: &str) -> bool {
    let axioms = axioms.to_owned();
    within(60, name, move || {
        is_consistent(&parse(&axioms)).expect("decides")
    })
}

/// The reproducers: answered `true`.
const SATISFIABLE: &[(&str, &str, &str)] = &[
    ("min_939", MIN_939, "C1"),
    ("case_939_c1", CASE_939, "C1"),
    ("case_939_c2", CASE_939, "C2"),
    ("direct_or", DIRECT_OR, "C1"),
];

/// Unsatisfiable classes whose clash needs the successor a waiting node must still
/// build once the search has picked a disjunct. A wait that is never lifted, or
/// that loses the `∃` / `≥`, would turn one of these into `true`.
const UNSATISFIABLE: &[(&str, &str, &str)] = &[
    (
        "both_nominals_exclude_the_successor",
        "SubClassOf(:C ObjectOneOf(:a :b)) SubClassOf(:C ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D))) \
         SubClassOf(ObjectOneOf(:b) ObjectAllValuesFrom(:r ObjectComplementOf(:D)))",
        "C",
    ),
    (
        "mixed_disjunction",
        "SubClassOf(:C ObjectUnionOf(ObjectOneOf(:a) :E)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D))) \
         SubClassOf(:E ObjectAllValuesFrom(:r ObjectComplementOf(:D)))",
        "C",
    ),
    (
        "min_cardinality_after_the_merge",
        "SubClassOf(:C ObjectMinCardinality(2 :r :D)) SubClassOf(:C ObjectOneOf(:a :b)) \
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :r)) \
         SubClassOf(ObjectOneOf(:b) ObjectMaxCardinality(1 :r))",
        "C",
    ),
    // The waiting node is a successor, not the query root.
    (
        "waiting_successor",
        "SubClassOf(:C ObjectSomeValuesFrom(:s :X)) SubClassOf(:X ObjectOneOf(:a :b)) \
         SubClassOf(:X ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D))) \
         SubClassOf(ObjectOneOf(:b) ObjectAllValuesFrom(:r ObjectComplementOf(:D)))",
        "C",
    ),
    // The `Or` is closed by a `∀` from the parent, not by the search.
    (
        "disjunction_closed_by_a_universal",
        "SubClassOf(:C ObjectSomeValuesFrom(:s :X)) \
         SubClassOf(:C ObjectAllValuesFrom(:s ObjectOneOf(:a))) \
         SubClassOf(:X ObjectOneOf(:a :b)) SubClassOf(:X ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D)))",
        "C",
    ),
    // The `Or` comes from `⊤ ⊑ …` (a deferred residual), not a concept rule.
    (
        "disjunction_from_a_universal_gci",
        "SubClassOf(owl:Thing ObjectUnionOf(ObjectOneOf(:a :b) :E)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :D)) SubClassOf(:C ObjectComplementOf(:E)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D))) \
         SubClassOf(ObjectOneOf(:b) ObjectAllValuesFrom(:r ObjectComplementOf(:D)))",
        "C",
    ),
];

#[test]
fn nominal_disjunction_reproducers_are_decided() {
    let _l = lock();
    for &(name, axioms, class) in SATISFIABLE {
        assert!(sat(name, axioms, class), "{name}: {class} is satisfiable");
    }
}

#[test]
fn unsatisfiable_classes_stay_unsatisfiable_with_the_flag_on_and_off() {
    let _l = lock();
    for flag in ["1", "0"] {
        let _g = EnvGuard::set(FLAG, flag);
        for &(name, axioms, class) in UNSATISFIABLE {
            assert!(
                !sat(name, axioms, class),
                "{name} ({FLAG}={flag}): {class} is unsatisfiable"
            );
        }
    }
}

/// A subsumption that needs the waiting node's successor: with `C ⊑ ∃r.D` and
/// `D ⊑ F`, `C ⊑ ∃r.F = G`. A wait that is never lifted shows here as a wrong
/// `false` rather than a hang. The non-subsumption on case 939 hung before.
#[test]
fn subsumptions_through_a_waiting_node_are_decided() {
    let _l = lock();
    let entailing = "SubClassOf(:C ObjectOneOf(:a :b)) SubClassOf(:C ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(:D :F) EquivalentClasses(:G ObjectSomeValuesFrom(:r :F))";
    for flag in ["1", "0"] {
        let _g = EnvGuard::set(FLAG, flag);
        assert!(
            subsumed("successor_entails", entailing, "C", "G"),
            "{FLAG}={flag}: C ⊑ G is entailed"
        );
    }
    assert!(
        !subsumed("case_939", CASE_939, "C1", "C2"),
        "case 939: C1 ⊑ C2 is not entailed"
    );
}

/// `consistent` on an ABox that puts an individual in a class whose only models
/// need the waiting node's successor, on the main tableau and through the wedge.
#[test]
fn consistency_through_a_waiting_node_is_decided() {
    let _l = lock();
    let inconsistent = "SubClassOf(:C ObjectOneOf(:a :b)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:D))) \
         SubClassOf(ObjectOneOf(:b) ObjectAllValuesFrom(:r ObjectComplementOf(:D))) \
         ClassAssertion(:C :i)";
    let consistent_kb = format!("{DIRECT_OR} ClassAssertion(:C1 :i)");
    for wedge in ["1", "0"] {
        let _w = EnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", wedge);
        for flag in ["1", "0"] {
            let _g = EnvGuard::set(FLAG, flag);
            assert!(
                !consistent("inconsistent", inconsistent),
                "RUSTDL_WEDGE_CONSISTENCY={wedge} {FLAG}={flag}: inconsistent"
            );
        }
        assert!(
            consistent("consistent", &consistent_kb),
            "RUSTDL_WEDGE_CONSISTENCY={wedge}: consistent (`i = a`, `r1(a, a)`)"
        );
    }
}

/// With the flag off the old order returns: the reproducers run past a short
/// deadline instead of answering.
///
/// This test pins the #473 defect on the flag-off path. If a later change makes
/// the flag-off path answer here, that is a fix, not a regression: flip this test
/// to assert the answer instead of deleting it.
#[test]
fn flag_off_restores_the_old_rule_order() {
    let _l = lock();
    let _g = EnvGuard::set(FLAG, "0");
    for (name, axioms) in [("min_939", MIN_939), ("direct_or", DIRECT_OR)] {
        let verdict = is_class_satisfiable_with_timeout(
            &parse(axioms),
            &format!("{NS}C1"),
            Duration::from_secs(2),
        )
        .expect("no error");
        assert_eq!(
            verdict, None,
            "{name}, flag off: the old order does not finish"
        );
    }
}
