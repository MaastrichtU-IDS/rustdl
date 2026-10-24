//! #344 — `consistent` trusted the hypertableau wedge's `Sat`, and the wedge does no
//! data-value counting: `a : P`, `P ⊑ ≤1 p.DataOneOf(1 2)`, `p(a,1)`, `p(a,2)` was
//! reported consistent. When the ontology has a `≥n`/`≤n` over a data range anywhere,
//! a wedge `Sat` is now verified on the main tableau, whose `concrete_domain_clash`
//! counts (`RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY`, default ON), bounded by its own
//! budget; a give-up keeps the wedge's `consistent`. Classify's consistency probe
//! does the same.
//!
//! ORACLE: `HermiT` 1.4.5 agrees on every verdict below except the pinned misses.
//! No `xsd:date` case: `HermiT` does not decide those.
//!
//! Every test locks the env mutex, since one test sets the flag.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    classify_top_down_with_timeout, is_consistent_with_stats, is_consistent_with_timeout,
};
use std::io::Cursor;
use std::time::Duration;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:P)) Declaration(Class(:Q))\n\
         Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n\
         Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// Consistency through the unbounded and the bounded API, asserted to agree, with
/// the unbounded verdict decided rather than a give-up.
fn consistent(body: &str) -> bool {
    let o = onto(body);
    let (verdict, stats) = is_consistent_with_stats(&o).expect("consistent");
    assert!(!stats.incomplete, "a give-up, not a verdict: {body}");
    assert_eq!(
        is_consistent_with_timeout(&o, Duration::from_secs(60)).expect("bounded"),
        Some(verdict),
        "bounded and unbounded disagree on: {body}"
    );
    verdict
}

const LIMIT: &str = "DataMaxCardinality(1 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer))";
const TWO_VALUES: &str = "DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n\
                          DataPropertyAssertion(:p :a \"2\"^^xsd:integer)";

/// The issue's reproducer, and the routes by which the limit reaches `a`.
#[test]
fn two_values_against_a_one_value_limit_are_inconsistent() {
    let _g = lock();
    for typing in [
        "SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)",
        "SubClassOf(:P {LIMIT}) SubClassOf(:Q :P) ClassAssertion(:Q :a)",
        "SubClassOf(:P {LIMIT}) ObjectPropertyDomain(:r :P) ObjectPropertyAssertion(:r :a :b)",
        "SubClassOf(:P {LIMIT}) EquivalentClasses(:P ObjectSomeValuesFrom(:r :Q))\n\
         ClassAssertion(:Q :b) ObjectPropertyAssertion(:r :a :b)",
        "ClassAssertion({LIMIT} :a)",
        "ObjectPropertyDomain(:r {LIMIT}) ObjectPropertyAssertion(:r :a :b)",
        "SubClassOf(ObjectSomeValuesFrom(:r :Q) {LIMIT})\n\
         ClassAssertion(:Q :b) ObjectPropertyAssertion(:r :a :b)",
    ] {
        let body = format!("{}\n{TWO_VALUES}", typing.replace("{LIMIT}", LIMIT));
        assert!(!consistent(&body), "{typing}");
    }
}

/// Other forms of the limit and of the values.
#[test]
fn other_counting_shapes_are_inconsistent() {
    let _g = lock();
    for body in [
        // Exact, and ≤2 with three values.
        format!(
            "SubClassOf(:P DataExactCardinality(1 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
             ClassAssertion(:P :a)\n{TWO_VALUES}"
        ),
        "SubClassOf(:P DataMaxCardinality(2 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer \"3\"^^xsd:integer)))\n\
         ClassAssertion(:P :a) DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n\
         DataPropertyAssertion(:p :a \"2\"^^xsd:integer) DataPropertyAssertion(:p :a \"3\"^^xsd:integer)"
            .to_string(),
        // Values as DataHasValue class assertions, and split across SameIndividual.
        format!(
            "SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n\
             ClassAssertion(DataHasValue(:p \"1\"^^xsd:integer) :a)\n\
             ClassAssertion(DataHasValue(:p \"2\"^^xsd:integer) :a)"
        ),
        format!(
            "SubClassOf(:P {LIMIT}) ClassAssertion(:P :a) SameIndividual(:a :b)\n\
             DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n\
             DataPropertyAssertion(:p :b \"2\"^^xsd:integer)"
        ),
        // A demand of two in [0,10] and a limit of two integers, plus a third value.
        "SubClassOf(:P DataMinCardinality(2 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))\n\
         SubClassOf(:Q DataMaxCardinality(2 :p xsd:integer))\n\
         ClassAssertion(:P :a) ClassAssertion(:Q :a)\n\
         DataPropertyAssertion(:p :a \"50\"^^xsd:integer)"
            .to_string(),
        // Booleans: two values exist; three cannot.
        "SubClassOf(:P DataMaxCardinality(1 :p xsd:boolean)) ClassAssertion(:P :a)\n\
         DataPropertyAssertion(:p :a \"true\"^^xsd:boolean)\n\
         DataPropertyAssertion(:p :a \"false\"^^xsd:boolean)"
            .to_string(),
        "SubClassOf(:P DataMinCardinality(3 :p xsd:boolean)) ClassAssertion(:P :a)".to_string(),
        // A demand against a range enumeration; a class made unsatisfiable by counting.
        "SubClassOf(:P DataMinCardinality(3 :p xsd:integer))\n\
         DataPropertyRange(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer))\n\
         ClassAssertion(:P :a)"
            .to_string(),
        format!(
            "SubClassOf(:P {LIMIT}) SubClassOf(:P DataHasValue(:p \"1\"^^xsd:integer))\n\
             SubClassOf(:P DataHasValue(:p \"2\"^^xsd:integer)) ClassAssertion(:P :a)"
        ),
        // Strings, with an inferred type.
        "SubClassOf(:P DataMaxCardinality(1 :p xsd:string))\n\
         ObjectPropertyDomain(:r :P) ObjectPropertyAssertion(:r :a :b)\n\
         DataPropertyAssertion(:p :a \"x\"^^xsd:string) DataPropertyAssertion(:p :a \"y\"^^xsd:string)"
            .to_string(),
    ] {
        assert!(!consistent(&body), "{body}");
    }
}

/// Negative controls: the values fit, are on another property, or the individual
/// is not under the limit; a limit on a sub-property does not count its super's
/// values; `"1"` and `"01"` are one value.
#[test]
fn values_the_limit_does_not_count_are_consistent() {
    let _g = lock();
    for body in [
        format!(
            "SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n\
             DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n\
             DataPropertyAssertion(:p :a \"7\"^^xsd:integer)"
        ),
        format!(
            "SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n\
             DataPropertyAssertion(:q :a \"1\"^^xsd:integer)\n\
             DataPropertyAssertion(:q :a \"2\"^^xsd:integer)"
        ),
        format!("SubClassOf(:P {LIMIT}) ClassAssertion(:Q :a)\n{TWO_VALUES}"),
        "SubClassOf(:P DataMaxCardinality(1 :q DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubDataPropertyOf(:q :p) ClassAssertion(:P :a)\n\
         DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n\
         DataPropertyAssertion(:p :a \"2\"^^xsd:integer)"
            .to_string(),
        format!(
            "SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n\
             DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n\
             DataPropertyAssertion(:p :a \"01\"^^xsd:integer)"
        ),
    ] {
        assert!(consistent(&body), "{body}");
    }
}

/// `=0` restores the trusted wedge `Sat`, so the reproducer reads consistent.
#[test]
fn the_flag_reverts() {
    let _g = lock();
    let body = format!("SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n{TWO_VALUES}");
    // SAFETY: the env mutex serialises every test in this file.
    unsafe { std::env::set_var("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY", "0") };
    let reverted = is_consistent_with_stats(&onto(&body))
        .expect("consistent")
        .0;
    unsafe { std::env::remove_var("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY") };
    assert!(reverted, "=0 must restore the trusted wedge Sat");
    assert!(!consistent(&body));
}

/// A verification that gives up keeps the wedge's answer rather than reporting
/// `unknown`: with a zero budget the reproducer reads consistent, as on `main`, and
/// is not marked incomplete.
#[test]
fn a_verification_give_up_keeps_the_old_answer() {
    let _g = lock();
    let body = format!("SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n{TWO_VALUES}");
    // SAFETY: the env mutex serialises every test in this file.
    unsafe { std::env::set_var("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY_MS", "0") };
    let o = onto(&body);
    let (verdict, stats) = is_consistent_with_stats(&o).expect("consistent");
    let bounded = is_consistent_with_timeout(&o, Duration::from_secs(60)).expect("bounded");
    unsafe { std::env::remove_var("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY_MS") };
    assert!(verdict && !stats.incomplete, "a give-up keeps `consistent`");
    assert_eq!(
        bounded,
        Some(true),
        "a give-up keeps `consistent` on the bounded path"
    );
}

/// Classify agrees: the reproducer is inconsistent there too.
#[test]
fn classify_reports_the_inconsistency() {
    let _g = lock();
    let body = format!("SubClassOf(:P {LIMIT}) ClassAssertion(:P :a)\n{TWO_VALUES}");
    let stats = classify_top_down_with_timeout(&onto(&body), Duration::from_secs(10))
        .expect("classify")
        .stats();
    assert!(
        stats.inconsistent,
        "classify must report the KB inconsistent"
    );
}

/// KNOWN MISSES, sound: the main tableau does not count these either. `HermiT` says
/// inconsistent on each. FLIP when handled.
/// - values on a sub-property (`concrete_domain_clash` keys on the exact property);
/// - a decimal, double or dateTime enumeration limit against asserted values (only
///   the integer range and enumeration buckets are counted together, #334);
/// - an unqualified `≥n` against a range enumeration (no data range to count).
#[test]
fn shapes_the_tableau_does_not_count_are_known_misses() {
    let _g = lock();
    let enumeration = |dt: &str, v1: &str, v2: &str| {
        format!(
            "SubClassOf(:P DataMaxCardinality(1 :p DataOneOf(\"{v1}\"^^xsd:{dt} \"{v2}\"^^xsd:{dt})))\n\
             ClassAssertion(:P :a)\n\
             DataPropertyAssertion(:p :a \"{v1}\"^^xsd:{dt})\n\
             DataPropertyAssertion(:p :a \"{v2}\"^^xsd:{dt})"
        )
    };
    for body in [
        format!(
            "SubClassOf(:P {LIMIT}) SubDataPropertyOf(:q :p) ClassAssertion(:P :a)\n\
             DataPropertyAssertion(:q :a \"1\"^^xsd:integer)\n\
             DataPropertyAssertion(:q :a \"2\"^^xsd:integer)"
        ),
        enumeration("decimal", "1.5", "2.5"),
        enumeration("double", "1.5", "2.5"),
        enumeration("dateTime", "2020-01-01T00:00:00", "2021-01-01T00:00:00"),
        "SubClassOf(:P DataMinCardinality(3 :p))\n\
         DataPropertyRange(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer))\n\
         ClassAssertion(:P :a)"
            .to_string(),
    ] {
        assert!(consistent(&body), "{body}");
    }
}
