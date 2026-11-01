//! #401 `RUSTDL_QUERY_TBOX_ONLY` (default ON): the single queries drop the `ABox`
//! seed when the ontology (with the query's probe axiom) has no nominal and the KB
//! is consistent, because then the `ABox` cannot change a class-level answer.
//!
//! What these pin, each with a sabotage that fails it:
//! - answers on a consistent nominal-free KB equal the seeded answers (`=0`);
//! - an inconsistent KB still answers "unsatisfiable" / "subsumed" (the skip must
//!   not run without the consistency check);
//! - a nominal in the `TBox` (incl. #400's `Range(r, {a})`) or in the query keeps
//!   the `ABox`;
//! - an undetermined consistency keeps the `ABox`;
//! - `relies_on_trusted_consistency` is set exactly when the skip rested on a
//!   trusted wedge `Sat`;
//! - the seeded probe runs first (`RUSTDL_QUERY_SEEDED_FIRST_MS`): a verdict it
//!   reaches is returned as is, a give-up falls through to the skip, and an
//!   inherited "consistent" verdict skips the seeded attempt.
//!
//! Every test locks `ENV_MUTEX`, including the read-only ones, so none observes
//! another test's setting. `lock()` also sets `RUSTDL_QUERY_SEEDED_FIRST_MS=0`,
//! so the tests of the consistency step reach it on these tiny fixtures (the
//! seeded probe would answer them first); the seeded-first tests override it.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, Individual, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{QueryStats, ReasonError};
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The env lock, plus `RUSTDL_QUERY_SEEDED_FIRST_MS=0` (see the module doc).
/// Fields drop in order: the env is restored before the lock is released.
struct Locked {
    _seeded_first: SetEnvGuard,
    _lock: std::sync::MutexGuard<'static, ()>,
}

fn lock() -> Locked {
    let lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Locked {
        _seeded_first: SetEnvGuard::set("RUSTDL_QUERY_SEEDED_FIRST_MS", "0"),
        _lock: lock,
    }
}

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
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const NS: &str = "http://t/";

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(<http://t>\n{axioms}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

fn sat(o: &SetOntology<RcStr>, c: &str) -> (bool, QueryStats) {
    owl_dl_reasoner::is_class_satisfiable_with_stats(o, &iri(c)).unwrap()
}

fn sub(o: &SetOntology<RcStr>, a: &str, b: &str) -> (bool, QueryStats) {
    owl_dl_reasoner::is_subclass_of_with_stats(o, &iri(a), &iri(b)).unwrap()
}

/// Off pure EL (the `ObjectUnionOf`), with an `ABox` and no nominal. `A ⊑ C ⊔ D`
/// keeps `A ⊑ C` and `A ⊑ D` for the tableau, so the queries below reach it.
const CONSISTENT: &str = "\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
Declaration(Class(:E)) Declaration(ObjectProperty(:r))
Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
SubClassOf(:A ObjectUnionOf(:C :D))
SubClassOf(:C :B) SubClassOf(:D :B)
SubClassOf(:E ObjectIntersectionOf(:C ObjectAllValuesFrom(:r ObjectComplementOf(:C))))
SubClassOf(:E ObjectSomeValuesFrom(:r :C))
ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b) ClassAssertion(:C :b)";

/// (query, expected) on `CONSISTENT`; `E` is unsatisfiable.
fn consistent_answers(o: &SetOntology<RcStr>) -> Vec<(&'static str, bool, bool)> {
    vec![
        ("sat A", sat(o, "A").0, true),
        ("sat E", sat(o, "E").0, false),
        ("A ⊑ C", sub(o, "A", "C").0, false),
        ("A ⊑ B", sub(o, "A", "B").0, true),
        ("C ⊑ A", sub(o, "C", "A").0, false),
    ]
}

#[test]
fn a_consistent_nominal_free_kb_answers_as_the_seeded_path_does() {
    let _l = lock();
    let o = parse(CONSISTENT);
    for (q, got, want) in consistent_answers(&o) {
        assert_eq!(got, want, "{q} with the ABox skipped");
    }
    let _off = SetEnvGuard::set("RUSTDL_QUERY_TBOX_ONLY", "0");
    for (q, got, want) in consistent_answers(&o) {
        assert_eq!(got, want, "{q} with the ABox seeded");
    }
}

/// The skip happened, and says what it rests on: the KB's consistency came from
/// a wedge `Sat`. With `=0` the seeded path runs and nothing is reported.
#[test]
fn the_skip_reports_its_trusted_consistency() {
    let _l = lock();
    let o = parse(CONSISTENT);
    let (_, st) = sat(&o, "A");
    assert!(
        !st.answered_by_saturation,
        "the tableau must decide this query"
    );
    assert!(
        st.relies_on_trusted_consistency,
        "skip on a wedge Sat is reported"
    );
    assert!(!st.incomplete, "a trusted consistency is not a give-up");
    let _off = SetEnvGuard::set("RUSTDL_QUERY_TBOX_ONLY", "0");
    let (_, st) = sat(&o, "A");
    assert!(
        !st.relies_on_trusted_consistency,
        "the seeded path relies on nothing"
    );
}

/// Same `TBox`, but `a` is asserted into two disjoint classes. Every class is then
/// unsatisfiable and every subsumption holds; dropping the `ABox` without the
/// consistency check would answer "satisfiable" here.
const INCONSISTENT: &str = "\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
Declaration(Class(:E)) Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))
SubClassOf(:A ObjectUnionOf(:C :D))
SubClassOf(:C :B) SubClassOf(:D :B)
DisjointClasses(:A :E)
ClassAssertion(:A :a) ClassAssertion(:E :a)";

#[test]
fn an_inconsistent_kb_answers_unsatisfiable_and_subsumed() {
    let _l = lock();
    let o = parse(INCONSISTENT);
    assert!(!sat(&o, "B").0, "every class is unsatisfiable");
    assert!(sub(&o, "C", "A").0, "every subsumption holds");
    let x: ClassExpression<RcStr> = Build::<RcStr>::new().class(iri("B")).into();
    assert!(matches!(
        owl_dl_reasoner::class_expression_satisfiable(&o, &x),
        Err(ReasonError::Inconsistent)
    ));
}

// The nominal tests use `sat` on a class whose unsatisfiability needs a `∀` (out
// of the EL saturator's reach) together with the `ABox`, reached through a
// nominal. `sat` has no wedge step, so only the F1 path can decide them; a
// `subclass` form was answered earlier by the subclass wedge and could not see
// a broken nominal check.

/// #400's shape: every `r`-successor is `a`, and `a` is a `B`, so
/// `K ≡ ∃r.⊤ ⊓ ∀r.¬B` is unsatisfiable, but only with the `ABox`.
#[test]
fn a_nominal_in_a_range_keeps_the_abox() {
    let _l = lock();
    let o = parse(
        "Declaration(Class(:B)) Declaration(Class(:K))
Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))
ObjectPropertyRange(:r ObjectOneOf(:a)) ClassAssertion(:B :a)
EquivalentClasses(:K ObjectIntersectionOf(ObjectSomeValuesFrom(:r owl:Thing)
    ObjectAllValuesFrom(:r ObjectComplementOf(:B))))",
    );
    let (s, st) = sat(&o, "K");
    assert!(!s, "K is unsatisfiable through the range nominal and a : B");
    assert!(!st.relies_on_trusted_consistency, "no skip with a nominal");
}

/// `ObjectHasValue` in the `TBox`: `K ≡ ∃p.{a} ⊓ ∀p.¬P` with `a : P` is unsatisfiable.
#[test]
fn a_has_value_keeps_the_abox() {
    let _l = lock();
    let o = parse(
        "Declaration(Class(:P)) Declaration(Class(:K))
Declaration(ObjectProperty(:p)) Declaration(NamedIndividual(:a))
EquivalentClasses(:K ObjectIntersectionOf(ObjectHasValue(:p :a)
    ObjectAllValuesFrom(:p ObjectComplementOf(:P))))
ClassAssertion(:P :a)",
    );
    assert!(!sat(&o, "K").0, "K is unsatisfiable: a : P is asserted");
}

/// A nominal only in the query: `{a} ⊓ ∀r.¬C` is unsatisfiable because `a` has
/// the `r`-successor `b` and `b : C` is asserted. The probe axiom carries the
/// nominal, so the `ABox` is kept although the ontology itself has none.
#[test]
fn a_nominal_in_the_query_keeps_the_abox() {
    let _l = lock();
    let o = parse(CONSISTENT);
    let b = Build::<RcStr>::new();
    let ce = ClassExpression::ObjectIntersectionOf(vec![
        ClassExpression::ObjectOneOf(vec![Individual::Named(b.named_individual(iri("a")))]),
        ClassExpression::ObjectAllValuesFrom {
            ope: b.object_property(iri("r")).into(),
            bce: Box::new(ClassExpression::ObjectComplementOf(Box::new(
                b.class(iri("C")).into(),
            ))),
        },
    ]);
    let v = owl_dl_reasoner::class_expression_satisfiable(&o, &ce).unwrap();
    assert!(
        !v.holds(),
        "{{a}} ⊓ ∀r.¬C is unsatisfiable: r(a, b) and b : C"
    );
    assert!(!v.relies_on_trusted_consistency());
}

/// An inconsistency only the `ABox`-seeded consistency wedge finds (`a : ∃r.E`
/// with `E ⊑ ⊥`; neither the saturator nor `abox_check` sees it).
const WEDGE_ONLY_INCONSISTENT: &str = "\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
Declaration(Class(:E)) Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))
SubClassOf(:A ObjectUnionOf(:C :D))
SubClassOf(:E owl:Nothing)
ClassAssertion(ObjectSomeValuesFrom(:r :E) :a)";

/// The wedge-`Unsat` arm: the KB is inconsistent, so `B` is unsatisfiable.
/// Dropping the `ABox` without settling consistency would answer "satisfiable".
#[test]
fn a_wedge_only_inconsistency_answers_unsatisfiable() {
    let _l = lock();
    let o = parse(WEDGE_ONLY_INCONSISTENT);
    let (s, st) = sat(&o, "B");
    assert!(!s, "every class is unsatisfiable in an inconsistent KB");
    assert!(!st.relies_on_trusted_consistency);
}

/// No bounded route decides consistency (the consistency wedge is off, so
/// [`owl_dl_reasoner`] reports it undetermined): the seeded path runs, so the
/// answers stay right and nothing claims a skip.
#[test]
fn an_undetermined_consistency_keeps_the_abox() {
    let _l = lock();
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    // With the wedge off nothing bounded sees this inconsistency, so the verdict
    // is undetermined; only the seeded tableau finds it. Treating undetermined
    // as consistent would drop the ABox and answer "satisfiable".
    let o = parse(WEDGE_ONLY_INCONSISTENT);
    assert!(!sat(&o, "B").0, "seeded path still sees the inconsistency");
    let o = parse(CONSISTENT);
    let (s, st) = sat(&o, "A");
    assert!(s);
    assert!(
        !st.relies_on_trusted_consistency,
        "no skip when undetermined"
    );
}

/// The #401 control: the same `TBox` with no `ABox` at all takes the ordinary
/// path and reports nothing.
#[test]
fn an_abox_free_kb_reports_no_skip() {
    let _l = lock();
    let tbox: String = CONSISTENT
        .lines()
        .filter(|l| !l.starts_with("ClassAssertion"))
        .collect::<Vec<_>>()
        .join("\n");
    let o = parse(&tbox);
    let (s, st) = sat(&o, "A");
    assert!(s);
    assert!(!st.relies_on_trusted_consistency);
}

/// Release-only coverage of the reported hang (gitignored fixture, so it is
/// `#[ignore]`d and only counts when run with `--ignored`): `sat :Pizza` on this
/// host's pizza ran past 120 CPU-s on every build before #401.
#[test]
#[ignore = "needs the gitignored ontologies/real/pizza.ofn and a release build"]
fn pizza_sat_answers_quickly() {
    let _l = lock();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ontologies/real/pizza.ofn"
    );
    let src = std::fs::read_to_string(path).expect("ontologies/real/pizza.ofn is missing");
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0;
    let start = std::time::Instant::now();
    let (s, st) = owl_dl_reasoner::is_class_satisfiable_with_stats(
        &o,
        "https://w3id.org/ontostart/pizza/Pizza",
    )
    .unwrap();
    assert!(s && !st.incomplete);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    // At the default seeded-first budget the seeded probe runs out its budget
    // first (it would not finish), then the skip answers.
    let default_ms = owl_dl_reasoner::QUERY_SEEDED_FIRST_DEFAULT_MS.to_string();
    let _budget = SetEnvGuard::set("RUSTDL_QUERY_SEEDED_FIRST_MS", &default_ms);
    let start = std::time::Instant::now();
    let (s, st) = owl_dl_reasoner::is_class_satisfiable_with_stats(
        &o,
        "https://w3id.org/ontostart/pizza/Pizza",
    )
    .unwrap();
    assert!(s && !st.incomplete && st.relies_on_trusted_consistency);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

/// The subset hook: settle the parent once, then probes inherit it. The trust
/// bit is inherited too, and an inconsistent parent yields no verdict to inherit.
#[test]
fn a_parent_verdict_is_settled_once_and_inherited() {
    let _l = lock();
    assert_eq!(
        owl_dl_reasoner::query_consistency_verdict(&parse(CONSISTENT)).unwrap(),
        Some(true),
        "consistent by a wedge Sat"
    );
    assert_eq!(
        owl_dl_reasoner::query_consistency_verdict(&parse(INCONSISTENT)).unwrap(),
        None
    );
    let o = parse(CONSISTENT);
    {
        let _g = owl_dl_reasoner::assume_query_consistency(false);
        let (s, st) = sat(&o, "A");
        assert!(s);
        assert!(
            !st.relies_on_trusted_consistency,
            "inherits trusted = false"
        );
    }
    {
        let _g = owl_dl_reasoner::assume_query_consistency(true);
        assert!(
            sat(&o, "A").1.relies_on_trusted_consistency,
            "inherits trusted = true"
        );
    }
    assert!(
        sat(&o, "A").1.relies_on_trusted_consistency,
        "guard restored on drop"
    );
}

/// Seeded first: on a KB where the seeded probe finishes in its budget, the
/// answer comes from it (so nothing rests on a trusted consistency), and equals
/// the skip's answer. Under an inherited "consistent" verdict the seeded attempt
/// is skipped, since the consistency step is then free.
#[test]
fn the_seeded_probe_answers_first_when_it_finishes_in_time() {
    let _l = lock();
    let _budget = SetEnvGuard::set("RUSTDL_QUERY_SEEDED_FIRST_MS", "60000");
    let o = parse(CONSISTENT);
    for (q, got, want) in consistent_answers(&o) {
        assert_eq!(got, want, "{q} seeded first");
    }
    let (s, st) = sat(&o, "A");
    assert!(s);
    assert!(
        !st.relies_on_trusted_consistency,
        "answered by the seeded probe, which relies on nothing"
    );
    assert!(!st.incomplete);
    let _g = owl_dl_reasoner::assume_query_consistency(true);
    assert!(
        sat(&o, "A").1.relies_on_trusted_consistency,
        "an inherited verdict goes straight to the skip"
    );
}

/// Seeded first: a seeded probe that gives up (here the node cap, which the
/// individuals' nominal roots exceed and the `ABox`-free probe does not) is not
/// an answer; the query falls through to the consistency step and the skip. With
/// the skip off, the same cap leaves the query cut.
#[test]
fn a_seeded_give_up_falls_through_to_the_skip() {
    let _l = lock();
    let _budget = SetEnvGuard::set("RUSTDL_QUERY_SEEDED_FIRST_MS", "60000");
    let o = parse(CONSISTENT);
    let _cap = owl_dl_tableau::NodeCapOverride::enter(Some(SEEDED_GIVE_UP_CAP));
    let (s, st) = sat(&o, "A");
    assert!(s);
    assert!(!st.incomplete, "the skip answered after the seeded give-up");
    assert!(st.relies_on_trusted_consistency, "answered by the skip");
    let _off = SetEnvGuard::set("RUSTDL_QUERY_TBOX_ONLY", "0");
    assert!(
        sat(&o, "A").1.incomplete,
        "control: the seeded probe alone gives up under this cap"
    );
}

/// A live-node cap the seeded probe on `CONSISTENT` exceeds (nominal roots for
/// `a` and `b`, plus the test root) and the `ABox`-free probe does not.
const SEEDED_GIVE_UP_CAP: usize = 2;

/// Under the guard a nominal still keeps the `ABox`: the inherited verdict
/// replaces only the consistency step, never the nominal precondition.
#[test]
fn an_inherited_verdict_does_not_bypass_the_nominal_check() {
    let _l = lock();
    let o = parse(
        "Declaration(Class(:B)) Declaration(Class(:K))
Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))
ObjectPropertyRange(:r ObjectOneOf(:a)) ClassAssertion(:B :a)
EquivalentClasses(:K ObjectIntersectionOf(ObjectSomeValuesFrom(:r owl:Thing)
    ObjectAllValuesFrom(:r ObjectComplementOf(:B))))",
    );
    let _g = owl_dl_reasoner::assume_query_consistency(true);
    assert!(!sat(&o, "K").0);
}

fn justification(o: &SetOntology<RcStr>, sub: &str, sup: &str) -> Option<Vec<String>> {
    let q = owl_dl_reasoner::justify::Entailment::SubClassOf {
        sub: iri(sub),
        sup: iri(sup),
    };
    owl_dl_reasoner::justify::find_one_justification(o, &q)
        .unwrap()
        .map(|j| {
            let mut v: Vec<String> = j.axioms.iter().map(|a| format!("{a:?}")).collect();
            v.sort();
            v
        })
}

/// justify's subset probes inherit the parent's verdict and drop the `ABox`; the
/// justification found is the one the seeded probes find. On the inconsistent
/// KB there is no verdict to inherit, so every probe settles its own.
#[test]
fn justify_finds_the_same_justification_with_the_abox_dropped() {
    let _l = lock();
    let consistent = parse(CONSISTENT);
    let inconsistent = parse(INCONSISTENT);
    let on = (
        justification(&consistent, "A", "B"),
        justification(&inconsistent, "C", "A"),
    );
    assert!(on.0.is_some(), "A ⊑ B is entailed");
    assert!(on.1.is_some(), "the inconsistent KB entails C ⊑ A");
    let _off = SetEnvGuard::set("RUSTDL_QUERY_TBOX_ONLY", "0");
    let off = (
        justification(&consistent, "A", "B"),
        justification(&inconsistent, "C", "A"),
    );
    assert_eq!(on, off);
}
