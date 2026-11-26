//! #533: classify's wedge is blind to `ABox` facts outside the nominal reach.
//!
//! The classify wedge is given the facts of the individuals in the nominal reach only
//! (individuals named in a `TBox` nominal, closed under `SameIndividual`), and
//! object-property assertions only under `RUSTDL_CLASSIFY_NOMINAL_OPA=1`. Its models
//! are then models of a weaker `KB`, and classify trusted them: label prunes,
//! candidate generation, the unsat probe, per-pair `NotSubsumed`. On each fixture
//! below `subclass` (the `ABox`-seeded main tableau) answered `yes` while the pair was
//! missing from classify's closure, with nothing reported.
//!
//! A completion with no nominal-labelled node can be united disjointly with any model
//! of the full `KB`, so unseen facts cannot invalidate it. Only a trusted model that
//! touches a nominal, while some fact was unfed, is blind:
//!
//! - by default classify reports it (`wedge_abox_blind`, and `completeness_guaranteed`
//!   is false); no answer changes;
//! - with `RUSTDL_CLASSIFY_WEDGE_ABOX_DEMOTE=1` it is demoted to the main tableau,
//!   counted in `wedge_abox_demoted`, and the entailment is found.
//!
//! Every expected closure is adjudicated by `HermiT` and Konclude, which agree on all
//! of them (fixtures and oracle runs in `~/rustdl/sweeps/bugfix/533/`). The `c*`
//! fixtures are the issue's, the `p*` ones the Fable plan review's probes (`X ⊑ ∃R.{a}`
//! with unseen facts about `a`), each with a control that drops the forcing fact.
//! Negatives first. Closures are compared at a non-truncating pair budget (`p2`
//! recovers only there), so a timing cut cannot pass or fail a test.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Classification, classify_with_timeout};

// Tests set `RUSTDL_CLASSIFY_WEDGE_ABOX_DEMOTE` / `RUSTDL_CLASSIFY_NOMINAL_OPA`; every
// test locks, so none reads them mid-change.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const EX: &str = "http://ex/#";
const DEMOTE: &str = "RUSTDL_CLASSIFY_WEDGE_ABOX_DEMOTE";
const OPA: &str = "RUSTDL_CLASSIFY_NOMINAL_OPA";

/// The issue's signature (`c*` fixtures).
const C_DECLS: &str = "Declaration(Class(:C0)) Declaration(Class(:C1)) Declaration(Class(:C2)) \
     Declaration(Class(:C3)) Declaration(Class(:C4)) Declaration(Class(:C7)) \
     Declaration(Class(:C8))\n\
     Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1)) \
     Declaration(ObjectProperty(:r3))\n\
     Declaration(NamedIndividual(:a0)) Declaration(NamedIndividual(:a2)) \
     Declaration(NamedIndividual(:a3)) Declaration(NamedIndividual(:a4))";

/// The probes' signature (`p*` fixtures).
const P_DECLS: &str = "Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E)) \
     Declaration(Class(:C))\n\
     Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))\n\
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
     Declaration(NamedIndividual(:c))";

const C474: &str = "ObjectPropertyAssertion(:r1 :a0 :a2) \
     SubClassOf(ObjectSomeValuesFrom(:r1 ObjectUnionOf(:C8 :C3)) :C7) \
     EquivalentClasses(:C3 ObjectOneOf(:a0)) ObjectPropertyAssertion(:r0 :a0 :a2) \
     SubClassOf(ObjectSomeValuesFrom(ObjectInverseOf(:r0) ObjectOneOf(:a2 :a0)) :C3)";
const C631: &str = "SubObjectPropertyOf(:r0 :r1) InverseFunctionalObjectProperty(:r1) \
     SubClassOf(:C2 ObjectSomeValuesFrom(ObjectInverseOf(:r1) :C0)) ClassAssertion(:C2 :a3) \
     ClassAssertion(ObjectHasValue(ObjectInverseOf(:r0) :a0) :a3) \
     EquivalentClasses(:C4 ObjectIntersectionOf(ObjectOneOf(:a0) :C3))";
const C863: &str = "SubClassOf(:C1 ObjectSomeValuesFrom(ObjectInverseOf(:r0) :C0)) \
     EquivalentClasses(:C4 ObjectSomeValuesFrom(ObjectInverseOf(:r0) owl:Thing)) \
     ObjectPropertyAssertion(:r3 :a4 :a3) \
     EquivalentClasses(:C4 ObjectIntersectionOf(ObjectOneOf(:a0) ObjectIntersectionOf(:C7 :C2))) \
     ObjectPropertyDomain(ObjectInverseOf(:r3) :C1)";

const X_TO_A: &str = "SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))";
const P1_TBOX: &str =
    "SubClassOf(ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) owl:Thing)) :D)";
const P1_FACT: &str = "ObjectPropertyAssertion(:S :b :a)";
const P2_REST: &str = "ClassAssertion(ObjectAllValuesFrom(:S :E) :b) \
     SubClassOf(ObjectSomeValuesFrom(:R :E) :D)";
const P3_FACT: &str = "ClassAssertion(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :b)";
const P4_REST: &str = "EquivalentClasses(:C ObjectOneOf(:a)) ObjectPropertyAssertion(:S :c :b) \
     ObjectPropertyRange(:S :C) SubClassOf(ObjectSomeValuesFrom(:R :E) :D)";
const P4_FACT: &str = "ClassAssertion(:E :b)";
const P5_REST: &str = "ClassAssertion(ObjectAllValuesFrom(:S :E) :b) \
     SubClassOf(:X ObjectAllValuesFrom(:R ObjectComplementOf(:E)))";

fn onto(decls: &str, body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n{decls}\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// Classify `body` with the given `(variable, value)` settings, at a pair budget no
/// fixture here comes near.
fn run(decls: &str, body: &str, env: &[(&str, &str)]) -> Classification {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every test in this binary holds `ENV_MUTEX` while the variables are set.
    for (k, v) in env {
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(k, v);
        }
    }
    let c = classify_with_timeout(&onto(decls, body), Duration::from_secs(2));
    for (k, _) in env {
        #[allow(unsafe_code)]
        unsafe {
            std::env::remove_var(k);
        }
    }
    c.expect("classify")
}

fn iri(c: &str) -> String {
    format!("{EX}{c}")
}

/// Every non-trivial entailment between the named classes `names`: `(sub, sup)` with
/// `sub ≠ sup`, plus the unsatisfiable ones as `(c, "Nothing")`.
fn closure(c: &Classification, names: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for &s in names {
        if c.unsatisfiable_classes().iter().any(|u| *u == iri(s)) {
            out.push((s.to_owned(), "Nothing".to_owned()));
            continue;
        }
        for &d in names {
            if s != d && c.is_subclass(&iri(s), &iri(d)) {
                out.push((s.to_owned(), d.to_owned()));
            }
        }
    }
    out.sort();
    out
}

const C_NAMES: &[&str] = &["C0", "C1", "C2", "C3", "C4", "C7", "C8"];
const P_NAMES: &[&str] = &["X", "D", "E", "C"];

fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = p
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
    v.sort();
    v
}

/// The oracle closure must be found with the demotion on, and nothing else; with it
/// off, whatever classify misses must be reported as `wedge_abox_blind`.
fn check(decls: &str, names: &[&str], body: &str, oracle: &[(&str, &str)]) {
    let want = pairs(oracle);

    let off = run(decls, body, &[]);
    let got = closure(&off, names);
    assert!(
        got.iter().all(|p| want.contains(p)),
        "default classify claims a pair the oracles reject: {got:?} vs {want:?}\n{body}"
    );
    let s = off.stats();
    assert_eq!(
        s.wedge_abox_demoted, 0,
        "demotion is off by default\n{body}"
    );
    if got != want {
        assert!(
            s.wedge_abox_blind && !off.completeness_guaranteed(),
            "classify missed {want:?} (got {got:?}) and reported nothing: {s:?}\n{body}"
        );
    }

    let on = run(decls, body, &[(DEMOTE, "1")]);
    assert_eq!(
        closure(&on, names),
        want,
        "with the demotion the closure must be the oracles'\n{body}"
    );
    let s = on.stats();
    assert!(
        !s.wedge_abox_blind,
        "a demoted model is not trusted, so it is not reported: {s:?}"
    );
}

// ── Negatives first: the controls (forcing fact removed), adjudicated empty. ──

#[test]
fn p1_control_without_the_fact_stays_empty() {
    check(P_DECLS, P_NAMES, &format!("{X_TO_A} {P1_TBOX}"), &[]);
}

#[test]
fn p2_control_without_the_edge_stays_empty() {
    check(
        P_DECLS,
        P_NAMES,
        &format!(
            "{X_TO_A} ClassAssertion(ObjectAllValuesFrom(:S :E) :b) \
             SubClassOf(ObjectSomeValuesFrom(:R :E) :D)"
        ),
        &[],
    );
}

#[test]
fn p4_control_without_the_type_stays_empty() {
    check(P_DECLS, P_NAMES, &format!("{X_TO_A} {P4_REST}"), &[]);
}

#[test]
fn p5_control_without_the_edge_keeps_x_satisfiable() {
    check(
        P_DECLS,
        P_NAMES,
        &format!(
            "{X_TO_A} ClassAssertion(ObjectAllValuesFrom(:S :E) :b) \
             SubClassOf(:X ObjectAllValuesFrom(:R ObjectComplementOf(:E)))"
        ),
        &[],
    );
}

/// The no-nominal argument's negative side: an `ABox` fact is unfed (`b` is outside
/// the reach of the only nominal, `{a}`) but no class's model touches a nominal
/// (`{a}` occurs only on the left), so every trusted model is a valid counterexample.
/// The signal must stay off and the closure must be the oracles' (`Y ⊑ W` only;
/// `X ⋢ D` although `b : Z` and `∃R.Z ⊑ D`).
#[test]
fn unfed_facts_without_a_nominal_in_the_model_raise_nothing() {
    let decls = "Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:Z)) \
         Declaration(Class(:W)) Declaration(Class(:D)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:R))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))";
    let body = "SubClassOf(ObjectOneOf(:a) :Q) ClassAssertion(:Z :b) \
         SubClassOf(:X ObjectSomeValuesFrom(:R :Y)) \
         SubClassOf(ObjectSomeValuesFrom(:R :Z) :D) SubClassOf(:Y :W)";
    let names = &["X", "Y", "Z", "W", "D", "Q"];
    for env in [&[][..], &[(DEMOTE, "1")][..]] {
        let c = run(decls, body, env);
        assert_eq!(closure(&c, names), pairs(&[("Y", "W")]), "{env:?}");
        let s = c.stats();
        assert!(!s.wedge_abox_blind, "{env:?}: {s:?}");
        assert_eq!(s.wedge_abox_blind_models, 0, "{env:?}: {s:?}");
        assert_eq!(s.wedge_abox_demoted, 0, "{env:?}: {s:?}");
    }
}

/// Every `ABox` fact fed: `c474`'s facts are object-property assertions on `a0`, which
/// is in the reach, so with `RUSTDL_CLASSIFY_NOMINAL_OPA=1` the wedge sees them all.
/// Its models touch `{a0}`, but nothing is unfed: no signal, and the pair is found.
#[test]
fn a_fully_fed_abox_raises_nothing() {
    let c = run(C_DECLS, C474, &[(OPA, "1")]);
    assert_eq!(closure(&c, C_NAMES), pairs(&[("C3", "C7")]));
    let s = c.stats();
    assert!(!s.wedge_abox_blind, "{s:?}");
    assert_eq!(s.wedge_abox_blind_models, 0, "{s:?}");
}

// ── The positives. ──

#[test]
fn c474_property_assertions_on_the_nominal() {
    check(C_DECLS, C_NAMES, C474, &[("C3", "C7")]);
}

#[test]
fn c631_class_assertions_outside_the_reach() {
    check(C_DECLS, C_NAMES, C631, &[("C4", "C0"), ("C4", "C3")]);
}

#[test]
fn c863_an_equality_only_type_reasoning_derives() {
    check(
        C_DECLS,
        C_NAMES,
        C863,
        &[
            ("C1", "C2"),
            ("C1", "C4"),
            ("C1", "C7"),
            ("C4", "C1"),
            ("C4", "C2"),
            ("C4", "C7"),
        ],
    );
}

#[test]
fn p1_an_inverse_edge_into_the_nominal() {
    check(
        P_DECLS,
        P_NAMES,
        &format!("{X_TO_A} {P1_FACT} {P1_TBOX}"),
        &[("X", "D")],
    );
}

#[test]
fn p2_a_universal_through_an_unfed_edge() {
    check(
        P_DECLS,
        P_NAMES,
        &format!("{X_TO_A} {P1_FACT} {P2_REST}"),
        &[("X", "D")],
    );
}

#[test]
fn p3_an_existential_fact_outside_the_reach() {
    check(
        P_DECLS,
        P_NAMES,
        &format!("{X_TO_A} {P3_FACT} {P1_TBOX}"),
        &[("X", "D")],
    );
}

#[test]
fn p4_an_equality_forced_by_a_range() {
    check(
        P_DECLS,
        P_NAMES,
        &format!("{X_TO_A} {P4_REST} {P4_FACT}"),
        &[("C", "E"), ("X", "D")],
    );
}

#[test]
fn p5_an_unsatisfiable_class_the_label_model_hid() {
    check(
        P_DECLS,
        P_NAMES,
        &format!("{X_TO_A} {P1_FACT} {P5_REST}"),
        &[("X", "Nothing")],
    );
}
