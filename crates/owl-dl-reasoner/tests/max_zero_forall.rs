//! #485: `≤0 R.C` concludes `¬C` on every `R`-neighbour during saturation
//! (`RUSTDL_MAX_ZERO_FORALL`, default ON).
//!
//! Before the change, that conclusion came only from the search's choose rule,
//! which runs after every open disjunction. On `C0 ≡ ≤0 r1.¬C5`,
//! `C5 ≡ ∃r0.(=2 r1.C2)`, a neighbour opened `C5 ⊔ ∀r0.¬X` before the choose rule
//! put `C5` on it. #288's `forall_first` made it try `∀r0.¬X` first, and the
//! subtree under it could only clash, so the search explored it again under every
//! combination of the other choices. `sat C5` never finished.
//!
//! The rule adds labels, so the danger is an unsound positive. Most canaries
//! here are UNSATISFIABLE classes, or satisfiable ones whose only model needs
//! the other branch of a disjunction the `≤0` sits under. Each runs with the
//! flag on and off, and with the wedge on and off: with it off the main tableau
//! must decide. `label_deps_to_a_nominal` puts the clash on a nominal's node,
//! created before the query root, so a conclusion that drops the label's deps is
//! the clash the search reports. `edge_deps_through_an_inverse` does the same for
//! the edge's deps (`edge_deps_to_a_nominal` cannot: the nominal merge adds the
//! decision back). The exact-deps unit test in `owl-dl-tableau` is the primary
//! guard. The flag-off hang is pinned at unit level, not here.

use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{is_class_satisfiable, is_consistent, is_subclass_of};

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
const FLAG: &str = "RUSTDL_MAX_ZERO_FORALL";
const WEDGE: &str = "RUSTDL_HYPERTABLEAU";

const DECLS: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
     Declaration(Class(:D)) Declaration(Class(:D1)) Declaration(Class(:D2)) \
     Declaration(Class(:E)) Declaration(Class(:E1)) Declaration(Class(:E2)) \
     Declaration(Class(:F)) Declaration(Class(:X)) Declaration(Class(:C0)) \
     Declaration(Class(:C2)) Declaration(Class(:C5)) \
     Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
     Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1)) \
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))";

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

fn sat(what: &str, axioms: &str, class: &str) -> bool {
    // Parsed on the worker: the ontology is not `Send`.
    let axioms = axioms.to_owned();
    let iri = format!("{NS}{class}");
    within(60, what, move || {
        is_class_satisfiable(&parse(&axioms), &iri).expect("decides")
    })
}

fn subclass(what: &str, axioms: &str, sub: &str, sup: &str) -> bool {
    let axioms = axioms.to_owned();
    let (sub, sup) = (format!("{NS}{sub}"), format!("{NS}{sup}"));
    within(60, what, move || {
        is_subclass_of(&parse(&axioms), &sub, &sup).expect("decides")
    })
}

fn consistent(what: &str, axioms: &str) -> bool {
    let axioms = axioms.to_owned();
    within(60, what, move || {
        is_consistent(&parse(&axioms)).expect("decides")
    })
}

/// The flag and wedge settings every canary runs under.
const ARMS: &[(&str, &str)] = &[("1", "1"), ("1", "0"), ("0", "1"), ("0", "0")];

fn each_arm(mut f: impl FnMut(&str)) {
    for &(flag, wedge) in ARMS {
        let _l = lock();
        let _f = EnvGuard::set(FLAG, flag);
        let _w = EnvGuard::set(WEDGE, wedge);
        f(&format!("{FLAG}={flag} {WEDGE}={wedge}"));
    }
}

/// The issue's reproducer (case 1349, shrunk). Konclude and `HermiT`: consistent,
/// nothing unsat, no subsumptions.
const MINIMAL: &str = "EquivalentClasses(:C0 ObjectMaxCardinality(0 :r1 ObjectComplementOf(:C5))) \
     EquivalentClasses(:C5 ObjectSomeValuesFrom(:r0 ObjectExactCardinality(2 :r1 :C2)))";

/// Unsatisfiable `A`: each clash needs the `¬C` the `≤0` puts on a neighbour
/// (or the choose rule's, with the flag off).
const UNSATISFIABLE: &[(&str, &str)] = &[
    (
        "direct",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r :B) \
         ObjectSomeValuesFrom(:r :B)))",
    ),
    (
        "sub_role",
        "SubObjectPropertyOf(:s :r) SubClassOf(:A ObjectIntersectionOf(\
         ObjectMaxCardinality(0 :r :B) ObjectSomeValuesFrom(:s :B)))",
    ),
    (
        "inverse_to_the_predecessor",
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) \
         ObjectMaxCardinality(0 :r :A)))",
    ),
    (
        "top_filler",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r owl:Thing) \
         ObjectSomeValuesFrom(:r owl:Thing)))",
    ),
    (
        "complemented_filler",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r ObjectComplementOf(:B)) \
         ObjectSomeValuesFrom(:r ObjectComplementOf(:B))))",
    ),
    (
        "max_zero_arrives_after_the_edge",
        "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :B) \
         ObjectSomeValuesFrom(:s :E))) \
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:s) ObjectMaxCardinality(0 :r :B)))",
    ),
    (
        "compound_filler",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r \
         ObjectIntersectionOf(:B :C)) ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :C))))",
    ),
    (
        "union_filler",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r ObjectUnionOf(:B :C)) \
         ObjectSomeValuesFrom(:r :B)))",
    ),
    (
        "inverse_role",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 ObjectInverseOf(:r) :B) \
         ObjectSomeValuesFrom(ObjectInverseOf(:r) :B)))",
    ),
    (
        "inverse_role_back_to_the_owner",
        "SubClassOf(:A ObjectIntersectionOf(:B ObjectSomeValuesFrom(:r \
         ObjectMaxCardinality(0 ObjectInverseOf(:r) :B))))",
    ),
    (
        "nominal_filler",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r ObjectOneOf(:a)) \
         ObjectSomeValuesFrom(:r ObjectOneOf(:a))))",
    ),
    (
        "self_loop",
        "SubClassOf(:A ObjectIntersectionOf(ObjectHasSelf(:r) ObjectMaxCardinality(0 :r :A)))",
    ),
    (
        "symmetric_role",
        "SymmetricObjectProperty(:r) SubClassOf(:A ObjectIntersectionOf(:B \
         ObjectSomeValuesFrom(:r ObjectMaxCardinality(0 :r :B))))",
    ),
];

/// Satisfiable `A`. The first group has a model only through a branch the `≤0`
/// is not on (a conclusion whose deps miss the branch would close it too); the
/// rest are satisfiable outright.
const SATISFIABLE: &[(&str, &str)] = &[
    (
        "label_deps",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
    ),
    (
        "edge_deps",
        "SubClassOf(:A ObjectMaxCardinality(0 :r :B)) SubClassOf(:A ObjectUnionOf(:E1 :E2)) \
         SubClassOf(:E1 ObjectSomeValuesFrom(:r :B))",
    ),
    (
        "edge_deps_to_a_nominal",
        "SubClassOf(:A ObjectMaxCardinality(0 :r :B)) SubClassOf(:A ObjectUnionOf(:E1 :E2)) \
         SubClassOf(:E1 ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
    ),
    (
        "label_deps_to_a_nominal",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
    ),
    (
        // The `≤0` sits on the successor with no decision in its deps; its `¬A` goes
        // back to the root (a lower index) through the edge, which carries `E1`. The
        // only end-to-end canary for dropped edge deps: in `edge_deps_to_a_nominal`
        // the nominal merge adds `E1` back.
        "edge_deps_through_an_inverse",
        "SubClassOf(owl:Thing ObjectMaxCardinality(0 ObjectInverseOf(:r) :A)) \
         SubClassOf(:A ObjectUnionOf(:E1 :E2)) SubClassOf(:E1 ObjectSomeValuesFrom(:r :F))",
    ),
    (
        "nested_disjunct_through_an_inverse",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectSomeValuesFrom(:s \
         ObjectMaxCardinality(0 ObjectInverseOf(:s) :A))) SubClassOf(:D2 :C)",
    ),
    (
        "compound_filler_half_met",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r \
         ObjectIntersectionOf(:B :C)) ObjectSomeValuesFrom(:r :B)))",
    ),
    (
        "cycle",
        "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
         ObjectMaxCardinality(0 :r :B)))",
    ),
    (
        "cycle_complemented",
        "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
         ObjectMaxCardinality(0 :r ObjectComplementOf(:A))))",
    ),
    (
        "top_filler_other_role",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r owl:Thing) \
         ObjectSomeValuesFrom(:s owl:Thing)))",
    ),
    (
        "nominal_filler_other_nominal",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r ObjectOneOf(:a)) \
         ObjectSomeValuesFrom(:r ObjectOneOf(:b))))",
    ),
    (
        "union_filler_other_class",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r ObjectUnionOf(:B :C)) \
         ObjectSomeValuesFrom(:r :D)))",
    ),
    (
        // An `s`-edge is not an `r`-edge when only `r ⊑ s`: the rule must not
        // treat a super-role's edge as `R`'s.
        "super_role_edge",
        "SubObjectPropertyOf(:r :s) SubClassOf(:A ObjectIntersectionOf(\
         ObjectMaxCardinality(0 :r :B) ObjectSomeValuesFrom(:s :B)))",
    ),
    (
        // `A`'s `r`-successor is not its `r⁻`-neighbour: the rule must keep the
        // edge's direction.
        "wrong_polarity",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 ObjectInverseOf(:r) :B) \
         ObjectSomeValuesFrom(:r :B)))",
    ),
    (
        // A sabotage that adds `C` instead of `¬C` makes this unsat.
        "undecided_neighbour",
        "SubClassOf(:A ObjectIntersectionOf(ObjectMaxCardinality(0 :r :B) \
         ObjectSomeValuesFrom(:r :C)))",
    ),
];

/// `(name, axioms, sub, sup, expected)`: the disjunct the `≤0` sits on is refuted,
/// so `A` is in the other one; it is in no unrelated class.
const SUBSUMPTIONS: &[(&str, &str, &str, &str, bool)] = &[
    (
        "edge_deps_through_an_inverse",
        "SubClassOf(owl:Thing ObjectMaxCardinality(0 ObjectInverseOf(:r) :A)) \
         SubClassOf(:A ObjectUnionOf(:E1 :E2)) SubClassOf(:E1 ObjectSomeValuesFrom(:r :F))",
        "A",
        "E2",
        true,
    ),
    (
        "edge_deps_through_an_inverse",
        "SubClassOf(owl:Thing ObjectMaxCardinality(0 ObjectInverseOf(:r) :A)) \
         SubClassOf(:A ObjectUnionOf(:E1 :E2)) SubClassOf(:E1 ObjectSomeValuesFrom(:r :F))",
        "A",
        "E1",
        false,
    ),
    (
        "edge_deps_through_an_inverse",
        "SubClassOf(owl:Thing ObjectMaxCardinality(0 ObjectInverseOf(:r) :A)) \
         SubClassOf(:A ObjectUnionOf(:E1 :E2)) SubClassOf(:E1 ObjectSomeValuesFrom(:r :F))",
        "A",
        "X",
        false,
    ),
    (
        "label_deps",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
        "A",
        "D2",
        true,
    ),
    (
        "label_deps",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
        "A",
        "X",
        false,
    ),
    (
        "edge_deps_to_a_nominal",
        "SubClassOf(:A ObjectMaxCardinality(0 :r :B)) SubClassOf(:A ObjectUnionOf(:E1 :E2)) \
         SubClassOf(:E1 ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
        "A",
        "E2",
        true,
    ),
    (
        "edge_deps_to_a_nominal",
        "SubClassOf(:A ObjectMaxCardinality(0 :r :B)) SubClassOf(:A ObjectUnionOf(:E1 :E2)) \
         SubClassOf(:E1 ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
        "A",
        "E1",
        false,
    ),
    (
        "edge_deps_to_a_nominal",
        "SubClassOf(:A ObjectMaxCardinality(0 :r :B)) SubClassOf(:A ObjectUnionOf(:E1 :E2)) \
         SubClassOf(:E1 ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
        "A",
        "X",
        false,
    ),
    (
        "label_deps_to_a_nominal",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
        "A",
        "D2",
        true,
    ),
    (
        "label_deps_to_a_nominal",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
        "A",
        "D1",
        false,
    ),
    (
        "label_deps_to_a_nominal",
        "SubClassOf(:A ObjectUnionOf(:D1 :D2)) SubClassOf(:D1 ObjectMaxCardinality(0 :r :B)) \
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectOneOf(:b))) ClassAssertion(:B :b)",
        "A",
        "X",
        false,
    ),
];

#[test]
fn the_reproducer_is_decided() {
    // Flag on only, set explicitly: with it off the search runs on (pinned at unit
    // level), so an ambient `=0` would make this a 60 s hang.
    for wedge in ["1", "0"] {
        let _l = lock();
        let _f = EnvGuard::set(FLAG, "1");
        let _w = EnvGuard::set(WEDGE, wedge);
        let arm = format!("{WEDGE}={wedge}");
        for class in ["C5", "C0", "C2"] {
            assert!(
                sat(&format!("sat {class} ({arm})"), MINIMAL, class),
                "{class} is satisfiable ({arm})"
            );
        }
        for (sub, sup) in [("C5", "C0"), ("C0", "C5"), ("C5", "C2"), ("C2", "C5")] {
            assert!(
                !subclass(&format!("{sub} ⊑ {sup} ({arm})"), MINIMAL, sub, sup),
                "{sub} ⊑ {sup} does not hold ({arm})"
            );
        }
        assert!(consistent(&format!("consistent ({arm})"), MINIMAL));
    }
}

#[test]
fn unsatisfiable_classes_stay_unsatisfiable() {
    each_arm(|arm| {
        for &(name, axioms) in UNSATISFIABLE {
            assert!(
                !sat(&format!("{name} ({arm})"), axioms, "A"),
                "{name} ({arm}): A is unsatisfiable"
            );
        }
    });
}

#[test]
fn satisfiable_classes_stay_satisfiable() {
    each_arm(|arm| {
        for &(name, axioms) in SATISFIABLE {
            assert!(
                sat(&format!("{name} ({arm})"), axioms, "A"),
                "{name} ({arm}): A is satisfiable"
            );
        }
    });
}

#[test]
fn subsumptions_through_the_refuted_disjunct() {
    each_arm(|arm| {
        for &(name, axioms, sub, sup, expected) in SUBSUMPTIONS {
            assert_eq!(
                subclass(&format!("{name} {sub} ⊑ {sup} ({arm})"), axioms, sub, sup),
                expected,
                "{name} ({arm}): {sub} ⊑ {sup}"
            );
        }
    });
}

#[test]
fn abox_consistency() {
    let inconsistent = "ClassAssertion(ObjectMaxCardinality(0 :r :B) :a) \
         ObjectPropertyAssertion(:r :a :b) ClassAssertion(:B :b)";
    let consistent_kb = "ClassAssertion(ObjectMaxCardinality(0 :r :B) :a) \
         ObjectPropertyAssertion(:r :a :b) ClassAssertion(:C :b)";
    each_arm(|arm| {
        assert!(
            !consistent(&format!("inconsistent ({arm})"), inconsistent),
            "({arm}) `≤0 r.B` at a with r(a, b), B(b) is inconsistent"
        );
        assert!(
            consistent(&format!("consistent ({arm})"), consistent_kb),
            "({arm}) with C(b) instead it is consistent"
        );
    });
}
