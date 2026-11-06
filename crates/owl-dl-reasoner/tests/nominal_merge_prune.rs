//! #474: a node that merges into a nominal's canonical node has its blockable
//! subtree pruned first (`RUSTDL_NOMINAL_MERGE_PRUNE`, default ON).
//!
//! #460 and #473 make a node merge before it generates when the nominal is on it
//! in time. In `D ⊑ ∃r.E`, `E ⊑ ∀r⁻.{a}`, `D ⊑ ∃s.D` the nominal reaches a `D`
//! node only from its own `r`-successor, after it has generated. The merge then
//! moved its `s`-successor onto `a`, which had generated too and did the same,
//! until the node cap: `sat D` gave up with no verdict.
//!
//! Pruning removes facts, and the danger in that direction is an unsound
//! positive, so most canaries here are UNSATISFIABLE classes (or an inconsistent
//! ABox) whose clash runs through the pruned subtree, the re-derived subtree, the
//! merge deps, or a branch the prune must be undone for. Each is checked with the
//! flag on and off. Each query runs on a worker thread under a time limit, so a
//! regression fails instead of hanging the suite.

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
const FLAG: &str = "RUSTDL_NOMINAL_MERGE_PRUNE";

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
     Declaration(Class(:F)) Declaration(Class(:G)) Declaration(Class(:H)) \
     Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X)) \
     Declaration(Class(:Y)) \
     Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
     Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:q)) \
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
     Declaration(NamedIndividual(:i))";

/// The issue's shape A. `D` is satisfiable: `a ∈ D` with `r(a, e)`, `e ∈ E`, and
/// `s(a, a)`.
const SHAPE_A: &str = "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
     SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
     SubClassOf(:D ObjectSomeValuesFrom(:s :D))";

/// Shape A with the nominal coming back as `{a, b}`. Satisfiable as shape A.
const TWO_NOMINALS: &str = "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
     SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a :b))) \
     SubClassOf(:D ObjectSomeValuesFrom(:s :D))";

/// Shape A with `≥2 r.E`. Satisfiable: `a` with two `r`-successors in `E`.
const MIN_TWO: &str = "SubClassOf(:D ObjectMinCardinality(2 :r :E)) \
     SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
     SubClassOf(:D ObjectSomeValuesFrom(:s :D))";

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

fn consistent(name: &str, axioms: &str) -> bool {
    let axioms = axioms.to_owned();
    within(60, name, move || {
        is_consistent(&parse(&axioms)).expect("decides")
    })
}

/// The reproducers: answered `true`.
const SATISFIABLE: &[(&str, &str, &str)] = &[
    ("shape_a", SHAPE_A, "D"),
    ("two_nominals", TWO_NOMINALS, "D"),
    ("min_two", MIN_TWO, "D"),
];

/// Satisfiable only on the right branch: the clash on the first branch must not
/// leak past the branch point (the merge deps reach the clash), and the prune
/// done on it must not survive into the second branch.
const SATISFIABLE_AFTER_A_BRANCH: &[(&str, &str, &str)] = &[
    // `{a} ⊑ ¬D` clashes on a merged `D` node only through the merge deps; were
    // they dropped, the clash would backjump past the `X ⊔ Y` that avoids it.
    (
        "merge_deps_reach_the_clash",
        "SubClassOf(:D ObjectSomeValuesFrom(:t ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:r :E)) SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(:D ObjectUnionOf(:X :Y)) SubClassOf(:X ObjectAllValuesFrom(:r :F)) \
         SubClassOf(:F ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(ObjectOneOf(:a) ObjectComplementOf(:D))",
        "D",
    ),
    // The `Or` frame sits on the `r`-successor the prune removes.
    (
        "or_frame_on_a_pruned_node",
        "SubClassOf(:D ObjectSomeValuesFrom(:t ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:r :E)) SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(:E ObjectUnionOf(:X :Y)) \
         SubClassOf(:X ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(ObjectOneOf(:a) ObjectComplementOf(:D))",
        "D",
    ),
];

/// Unsatisfiable classes whose clash needs the pruned subtree, the subtree the
/// canonical node re-derives, the merge deps, or the facts the prune removed on a
/// branch that was undone. A prune that lost a fact for good, or was not undone
/// on backtracking, would turn one of these into `true`.
const UNSATISFIABLE: &[(&str, &str, &str)] = &[
    // The clash needs the `r`-successor the canonical node rebuilds.
    (
        "regenerated_successor",
        "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:E)))",
        "D",
    ),
    // The clash runs through the `s` chain the prune cuts.
    (
        "s_chain",
        "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:s ObjectComplementOf(:D)))",
        "D",
    ),
    // A label the pruned subtree had passed up (`G`) must survive the prune.
    (
        "label_from_the_subtree",
        "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) \
           ObjectIntersectionOf(ObjectOneOf(:a) :G))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) SubClassOf(:G ObjectAllValuesFrom(:s :H)) \
         SubClassOf(:D ObjectComplementOf(:H))",
        "D",
    ),
    // A clash already in the subtree when the prune runs must be found again.
    (
        "clash_in_the_subtree",
        "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:s \
           ObjectAllValuesFrom(:s ObjectComplementOf(:D))))",
        "D",
    ),
    // The canonical node is `source`'s child: it is re-rooted, not pruned.
    (
        "survivor_re_rooted_from_the_subtree",
        "SubClassOf(:C ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :E))) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:C ObjectSomeValuesFrom(:s :C)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:s ObjectComplementOf(:C)))",
        "C",
    ),
    // As above, two levels down.
    (
        "survivor_re_rooted_two_levels_down",
        "SubClassOf(:C ObjectSomeValuesFrom(:r :P)) \
         SubClassOf(:P ObjectSomeValuesFrom(:q ObjectIntersectionOf(ObjectOneOf(:a) :E))) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:q) :G)) \
         SubClassOf(:G ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:C ObjectSomeValuesFrom(:s :C)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:s ObjectComplementOf(:C)))",
        "C",
    ),
    // `source` is itself `b`'s canonical node.
    (
        "source_is_a_nominal",
        "SubClassOf(:D ObjectOneOf(:b)) SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:s ObjectComplementOf(:D))) \
         ClassAssertion(:Q :a)",
        "D",
    ),
    // A distinct pair: the merge is refused.
    (
        "refused_merge",
        "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) SubClassOf(:D ObjectOneOf(:b)) \
         DifferentIndividuals(:a :b)",
        "D",
    ),
    // Branch `X` merges and prunes, then clashes; branch `Y` clashes only through
    // the `s`-successor that branch pruned, so the prune must be undone.
    (
        "prune_undone_on_backtrack",
        "SubClassOf(:D ObjectSomeValuesFrom(:t ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:r :E)) SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(:D ObjectUnionOf(:X :Y)) SubClassOf(:X ObjectAllValuesFrom(:r :F)) \
         SubClassOf(:F ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(ObjectOneOf(:a) ObjectComplementOf(:X)) \
         SubClassOf(:Y ObjectAllValuesFrom(:s ObjectComplementOf(:D)))",
        "D",
    ),
    // As above, but branch `Y` needs a restored node to fire its own `∃` again.
    (
        "restored_node_fires_again",
        "SubClassOf(:D ObjectSomeValuesFrom(:t ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:r :E)) SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(:D ObjectUnionOf(:X :Y)) SubClassOf(:X ObjectAllValuesFrom(:r :F)) \
         SubClassOf(:F ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(ObjectOneOf(:a) ObjectComplementOf(:X)) \
         SubClassOf(:Y ObjectAllValuesFrom(:s :G)) SubClassOf(:G ObjectSomeValuesFrom(:q :H)) \
         SubClassOf(:H ObjectAllValuesFrom(ObjectInverseOf(:q) ObjectComplementOf(:D)))",
        "D",
    ),
    // Both disjuncts on the pruned `r`-successor lead to the clash.
    (
        "or_frame_on_a_pruned_node",
        "SubClassOf(:D ObjectSomeValuesFrom(:t ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:r :E)) SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(:E ObjectUnionOf(:X :Y)) \
         SubClassOf(:X ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:Y ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(ObjectOneOf(:a) ObjectComplementOf(:D))",
        "D",
    ),
];

#[test]
fn late_nominal_reproducers_are_decided() {
    let _l = lock();
    for &(name, axioms, class) in SATISFIABLE {
        assert!(sat(name, axioms, class), "{name}: {class} is satisfiable");
    }
}

#[test]
fn classes_satisfiable_on_one_branch_stay_satisfiable_with_the_flag_on_and_off() {
    let _l = lock();
    for flag in ["1", "0"] {
        let _g = EnvGuard::set(FLAG, flag);
        for &(name, axioms, class) in SATISFIABLE_AFTER_A_BRANCH {
            assert!(
                sat(name, axioms, class),
                "{name} ({FLAG}={flag}): {class} is satisfiable"
            );
        }
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

/// A subsumption through the subtree the canonical node re-derives: with `E ⊑ F`,
/// `D ⊑ ∃r.F = G`.
#[test]
fn subsumption_through_the_rebuilt_subtree_is_decided() {
    let _l = lock();
    let axioms =
        format!("{SHAPE_A} SubClassOf(:E :F) EquivalentClasses(:G ObjectSomeValuesFrom(:r :F))");
    for flag in ["1", "0"] {
        let _g = EnvGuard::set(FLAG, flag);
        let axioms = axioms.clone();
        let (sub, sup) = (format!("{NS}D"), format!("{NS}G"));
        let entailed = within(60, "subsumption", move || {
            is_subclass_of(&parse(&axioms), &sub, &sup).expect("decides")
        });
        assert!(entailed, "{FLAG}={flag}: D ⊑ G is entailed");
    }
}

/// `consistent` on ABoxes whose individual gains a nominal late, on the main
/// tableau and through the wedge.
#[test]
fn consistency_with_a_late_nominal_is_decided() {
    let _l = lock();
    let regenerated = "SubClassOf(:D ObjectSomeValuesFrom(:r :E)) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))) \
         SubClassOf(:D ObjectSomeValuesFrom(:s :D)) \
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectComplementOf(:E))) \
         ClassAssertion(:D :i)";
    let late = format!("{SHAPE_A} ClassAssertion(:D :i) ClassAssertion(ObjectComplementOf(:D) :a)");
    let fine = format!("{SHAPE_A} ClassAssertion(:D :i)");
    for wedge in ["1", "0"] {
        let _w = EnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", wedge);
        for flag in ["1", "0"] {
            let _g = EnvGuard::set(FLAG, flag);
            assert!(
                !consistent("regenerated_successor", regenerated),
                "RUSTDL_WEDGE_CONSISTENCY={wedge} {FLAG}={flag}: inconsistent"
            );
            assert!(
                !consistent("abox_late_nominal", &late),
                "RUSTDL_WEDGE_CONSISTENCY={wedge} {FLAG}={flag}: inconsistent (i = a)"
            );
        }
        assert!(
            consistent("abox_shape_a", &fine),
            "RUSTDL_WEDGE_CONSISTENCY={wedge}: consistent (i = a, r(a, e), s(a, a))"
        );
    }
}

/// With the flag off the old merge returns: the reproducer runs past a short
/// deadline instead of answering.
///
/// This test pins the #474 defect on the flag-off path. If a later change makes
/// the flag-off path answer here, that is a fix, not a regression: flip this test
/// to assert the answer instead of deleting it.
#[test]
fn flag_off_restores_the_old_merge() {
    let _l = lock();
    let _g = EnvGuard::set(FLAG, "0");
    for (name, axioms) in [("shape_a", SHAPE_A), ("min_two", MIN_TWO)] {
        // Under `within` too, so a deadline that failed to fire fails the test
        // instead of hanging the suite.
        let verdict = within(30, name, move || {
            is_class_satisfiable_with_timeout(
                &parse(axioms),
                &format!("{NS}D"),
                Duration::from_secs(2),
            )
            .expect("no error")
        });
        assert_eq!(
            verdict, None,
            "{name}, flag off: the old merge does not finish"
        );
    }
}
