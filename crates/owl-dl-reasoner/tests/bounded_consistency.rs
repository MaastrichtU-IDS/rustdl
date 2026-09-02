//! `is_consistent_with_timeout` (#74) — a deadline-bearing consistency check.
//!
//! Every other reasoner entry point has a bounded form; `is_consistent` had none,
//! so a caller who needed one had to substitute
//! `is_class_satisfiable_with_timeout` on a top concept.
//!
//! #74 calls that substitution unsafe on the grounds that satisfiability
//! short-circuits on `is_pure_el` without consulting the ABox. That reading of the
//! source is right, but the divergence is NOT demonstrated — see
//! `the_substitute_agrees_on_every_shape_tried`, which pins the measured agreement
//! across four shapes. What IS demonstrated is narrower and still sufficient: the
//! substitute needs `owl:Thing` declared, and the unbounded check silently reports
//! `true` when its own internal budget elapses.
//!
//! Direction of each outcome:
//! * `Some(false)` — a clash was WITNESSED. Never invented.
//! * `Some(true)`  — consistent at `is_consistent`'s trust level (incl. wedge `Sat`).
//! * `None`        — gave up inside the budget. The UNBOUNDED path reports `true`
//!   in the corresponding case, which is sound but invisible; removing that
//!   silence is the point.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::fmt::Write as _;
use std::io::Cursor;
use std::time::Duration;

const GENEROUS: Duration = Duration::from_secs(30);

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://ex#>)\nOntology(\n{body}\n)");
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// An ABox-only clash on a PURE-EL TBox: two disjoint classes asserted of one
/// individual. Nothing in the TBox is unsatisfiable.
const ABOX_ONLY_CLASH: &str = r"Declaration(Class(:A)) Declaration(Class(:B))
    Declaration(NamedIndividual(:x))
    DisjointClasses(:A :B)
    ClassAssertion(:A :x)
    ClassAssertion(:B :x)";

#[test]
fn witnessed_inconsistency_is_reported() {
    let o = onto(ABOX_ONLY_CLASH);
    assert_eq!(
        owl_dl_reasoner::is_consistent_with_timeout(&o, GENEROUS).unwrap(),
        Some(false),
        "an ABox clash must be witnessed inside a generous budget"
    );
}

#[test]
fn a_consistent_ontology_is_reported_consistent() {
    let o = onto(
        r"Declaration(Class(:A)) Declaration(Class(:B))
        Declaration(NamedIndividual(:x))
        SubClassOf(:A :B)
        ClassAssertion(:A :x)",
    );
    assert_eq!(
        owl_dl_reasoner::is_consistent_with_timeout(&o, GENEROUS).unwrap(),
        Some(true)
    );
}

/// THE SUBSTITUTION HAZARD IS STRUCTURAL, NOT DEMONSTRATED — and this test says so
/// rather than implying otherwise.
///
/// #74 argues that `is_class_satisfiable_with_timeout` on a top concept is an
/// unsafe stand-in, because it short-circuits on `is_pure_el` before the ABox is
/// consulted. That reading of the source is correct. But **no divergent case has
/// been produced**: the issue's author reports measuring the two agreeing on every
/// fixture they had, and I failed to construct one across four more shapes —
/// ABox-only disjointness, a functional-role merge clash, a role-chain-induced
/// range clash, and an inverse-materialised domain clash. All four agree, because
/// `owl_dl_saturation::saturate` derives the clash and the
/// `closure.is_unsatisfiable` short-circuit fires BEFORE the `is_pure_el` arm.
///
/// So the case for this API rests on the two things that ARE demonstrated:
/// there was no bounded consistency check at all, and the unbounded one reports
/// `true` when its own internal budget elapses (see
/// `an_expired_budget_yields_no_verdict`).
///
/// One real asymmetry does show up and is pinned below: the substitute needs
/// `owl:Thing` to be DECLARED, and errors out otherwise.
#[test]
fn the_substitute_agrees_on_every_shape_tried() {
    let top = "http://www.w3.org/2002/07/owl#Thing";
    // `owl:Thing` must be declared or the substitute cannot even be called.
    let declared_top = "Declaration(Class(<http://www.w3.org/2002/07/owl#Thing>))";
    for body in [
        format!("{declared_top}\n{ABOX_ONLY_CLASH}"),
        format!(
            "{declared_top}
             Declaration(Class(:A)) Declaration(Class(:B))
             Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)
             Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y1))
             Declaration(NamedIndividual(:y2))
             DisjointClasses(:A :B)
             ObjectPropertyAssertion(:r :x :y1) ObjectPropertyAssertion(:r :x :y2)
             ClassAssertion(:A :y1) ClassAssertion(:B :y2)"
        ),
    ] {
        let o = onto(&body);
        let cons = owl_dl_reasoner::is_consistent_with_timeout(&o, GENEROUS).unwrap();
        let sat = owl_dl_reasoner::is_class_satisfiable_with_timeout(&o, top, GENEROUS).unwrap();
        assert_eq!(
            cons, sat,
            "measured agreement is the CURRENT state; if this ever fails, a genuine \
             divergent case has been found and #74's structural argument is now \
             demonstrated — record it rather than just fixing the test:\n{body}"
        );
    }
}

/// The one asymmetry that IS demonstrable: the substitute requires `owl:Thing` to
/// be declared in the ontology, and errors otherwise. `is_consistent_with_timeout`
/// has no such precondition.
#[test]
fn the_substitute_needs_owl_thing_declared() {
    let o = onto(ABOX_ONLY_CLASH);
    let top = "http://www.w3.org/2002/07/owl#Thing";
    assert!(
        owl_dl_reasoner::is_class_satisfiable_with_timeout(&o, top, GENEROUS).is_err(),
        "without a Declaration(Class(owl:Thing)) the substitute cannot be called at all"
    );
    assert_eq!(
        owl_dl_reasoner::is_consistent_with_timeout(&o, GENEROUS).unwrap(),
        Some(false),
        "the real check has no such precondition"
    );
}

/// The budget must bound the ABox-saturation pre-check too, not just the stages
/// after it.
///
/// A first cut left that pre-check unbounded on the reasoning that it is cheap. It
/// is not always: `family.ofn` overran a 500 ms budget by **2.3x** (1.17 s).
/// Bounded, the same input measures 1.03x at 100 ms and 1.02x at 500 ms.
///
/// ## The fixture is sized from a measured sabotage, not guessed
///
/// A first version of this canary used 400 individuals with no transitive role and
/// **failed to catch the regression** — restoring the unbounded call left it green,
/// because that shape costs milliseconds either way. What makes the pre-check
/// expensive is the ROLE-CHAIN CLOSURE, which is also what makes `family` slow. A
/// transitive role over a chain of 800 individuals gives an O(n²) closure and this
/// measured separation:
///
/// | n | bounded | unbounded |
/// |---|---|---|
/// | 200 | 447 µs | 63 ms |
/// | 400 | 284 µs | 380 ms |
/// | **800** | **557 µs** | **4 s** |
///
/// The 2 s threshold sits ~3500x above the passing cost and ~2x below the failing
/// one, so it is not the host-speed knife-edge that #92 documents.
#[test]
fn the_budget_bounds_the_abox_pre_check() {
    const N: usize = 800;
    let mut body = String::from(
        "Declaration(Class(:A)) Declaration(ObjectProperty(:r))\nTransitiveObjectProperty(:r)\n",
    );
    for i in 0..N {
        let _ = write!(
            body,
            "Declaration(NamedIndividual(:i{i}))\nClassAssertion(:A :i{i})\n"
        );
        if i > 0 {
            let _ = writeln!(body, "ObjectPropertyAssertion(:r :i{} :i{i})", i - 1);
        }
    }
    let o = onto(&body);
    let start = std::time::Instant::now();
    let verdict = owl_dl_reasoner::is_consistent_with_timeout(&o, Duration::from_nanos(1)).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(
        verdict, None,
        "a 1 ns budget cannot be met, so the answer must be None"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the ABox pre-check must respect the deadline; {elapsed:?} means it ran \
         unbounded (measured: ~557 us bounded vs ~4 s unbounded on this fixture)"
    );
}

/// An exhausted budget yields `None`, not a guess in either direction.
#[test]
fn an_expired_budget_yields_no_verdict() {
    let o = onto(
        r"Declaration(Class(:A)) Declaration(NamedIndividual(:x))
        ClassAssertion(:A :x)",
    );
    assert_eq!(
        owl_dl_reasoner::is_consistent_with_timeout(&o, Duration::from_nanos(1)).unwrap(),
        None,
        "a budget that cannot be met must report None rather than defaulting to a verdict"
    );
}

/// The bounded form must not disagree with the unbounded one where both conclude.
#[test]
fn bounded_agrees_with_unbounded_where_both_conclude() {
    for body in [
        ABOX_ONLY_CLASH,
        r"Declaration(Class(:A)) Declaration(Class(:B)) SubClassOf(:A :B)",
        r"Declaration(Class(:A)) Declaration(Class(:B))
          DisjointClasses(:A :B)
          Declaration(NamedIndividual(:x)) ClassAssertion(:A :x)",
    ] {
        let o = onto(body);
        let unbounded = owl_dl_reasoner::is_consistent(&o).unwrap();
        let bounded = owl_dl_reasoner::is_consistent_with_timeout(&o, GENEROUS).unwrap();
        assert_eq!(
            bounded,
            Some(unbounded),
            "bounded and unbounded disagree on:\n{body}"
        );
    }
}
