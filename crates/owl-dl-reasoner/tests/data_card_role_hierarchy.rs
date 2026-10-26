//! #349 — data counting follows the data-property hierarchy.
//!
//! `concrete_domain_clash` counted demands, limits and `∀` filters per EXACT data
//! property, so with `q ⊑ p` a `≤1 p.{1,2}` never saw two `q`-values in `{1,2}`.
//! `q ⊑ p` makes every `q`-value a `p`-value, so at a node:
//! - a demand on `q` is a demand on `p` (lifted UP, narrowed by `q`'s own and
//!   inherited `∀` filters first);
//! - a `∀` on `p` (incl. a `DataPropertyRange`) filters `q` (copied DOWN);
//! - a limit is never copied: `≤m q` does not bound `p`, nor `≤m p` a sibling's
//!   values except through the lifted demand.
//!
//! Both copies are entailed, so the change only adds clashes
//! (`RUSTDL_DATA_COUNTING_ROLE_HIERARCHY`, default ON, `=0` reverts).
//!
//! ORACLE: `HermiT` 1.4.5 agrees on every verdict below. Konclude also agrees except
//! where it does not count interval or singleton capacity (the interval and branch
//! cases, which it calls satisfiable) and on the super-property range, which it
//! under-reports.
//!
//! Every test locks the env mutex, since one test sets the flag.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    classify_top_down_with_timeout, dropped_axioms, is_class_satisfiable, is_consistent,
};
use std::io::Cursor;
use std::time::Duration;

const C: &str = "http://ex.org/C";

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for one test and restores it on drop (the `adaptive_budget.rs`
/// pattern); callers hold [`lock`].
struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one test,
        // serialised via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:G)) Declaration(Class(:P))\n\
         Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n\
         Declaration(DataProperty(:q1)) Declaration(DataProperty(:q2))\n\
         Declaration(DataProperty(:r)) Declaration(NamedIndividual(:a))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `C`'s satisfiability, asserted to agree between `classify` and the per-class
/// probe, with nothing dropped.
fn c_unsat(body: &str) -> bool {
    let o = onto(body);
    assert!(
        dropped_axioms(&o).expect("dropped").is_empty(),
        "nothing may drop: {body}"
    );
    let by_classify = classify_top_down_with_timeout(&o, Duration::from_secs(30))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&C);
    let by_sat = !is_class_satisfiable(&o, C).expect("sat");
    assert_eq!(by_classify, by_sat, "classify and sat disagree on: {body}");
    by_sat
}

const LIMIT_P: &str =
    "SubClassOf(:C DataMaxCardinality(1 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))";
const TWO_Q_VALUES: &str = "SubClassOf(:C DataHasValue(:q \"1\"^^xsd:integer))\n\
     SubClassOf(:C DataHasValue(:q \"2\"^^xsd:integer))";

/// UP: two `q`-values are two `p`-values, so `≤1 p.{1,2}` clashes; also through a
/// two-level chain `r ⊑ q ⊑ p` and through `EquivalentDataProperties`.
#[test]
fn a_limit_on_the_super_property_counts_sub_property_values() {
    let _g = lock();
    assert!(c_unsat(&format!(
        "SubDataPropertyOf(:q :p)\n{LIMIT_P}\n{TWO_Q_VALUES}"
    )));
    assert!(c_unsat(&format!(
        "SubDataPropertyOf(:r :q) SubDataPropertyOf(:q :p)\n{LIMIT_P}\n\
         SubClassOf(:C DataHasValue(:r \"1\"^^xsd:integer))\n\
         SubClassOf(:C DataHasValue(:r \"2\"^^xsd:integer))"
    )));
    assert!(c_unsat(&format!(
        "EquivalentDataProperties(:q :p)\n{LIMIT_P}\n{TWO_Q_VALUES}"
    )));
}

/// UP, other datatypes: a string range on `p` bounds `≥2 q.xsd:string` (the
/// `DataPropertyRange` role-rule path, #327), and a decimal limit counts decimal
/// enumeration witnesses on `q`.
#[test]
fn the_lift_works_for_strings_and_decimals() {
    let _g = lock();
    assert!(c_unsat(
        "SubDataPropertyOf(:q :p) DataPropertyRange(:p DataOneOf(\"a\"))\n\
         SubClassOf(:C DataMinCardinality(2 :q xsd:string))"
    ));
    assert!(c_unsat(
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataMaxCardinality(1 :p DataOneOf(\"1.5\"^^xsd:decimal \"2.5\"^^xsd:decimal)))\n\
         SubClassOf(:C DataSomeValuesFrom(:q DataOneOf(\"1.5\"^^xsd:decimal)))\n\
         SubClassOf(:C DataSomeValuesFrom(:q DataOneOf(\"2.5\"^^xsd:decimal)))"
    ));
}

/// The lifted demand is NARROWED by the sub-property's filter: `≥2 q.xsd:integer`
/// under `∀q.{1,2}` is two values in `{1,2}`, which `≤1 p.{1,2}` forbids. Without
/// the narrowing `p` sees `≥2 xsd:integer`, which it can meet outside `{1,2}`.
/// Likewise two siblings each forced to one value of `{1,2}`.
#[test]
fn a_lifted_demand_is_narrowed_by_the_sub_property_filter() {
    let _g = lock();
    assert!(c_unsat(&format!(
        "SubDataPropertyOf(:q :p)\n{LIMIT_P}\n\
         SubClassOf(:C DataAllValuesFrom(:q DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubClassOf(:C DataMinCardinality(2 :q xsd:integer))"
    )));
    assert!(c_unsat(&format!(
        "SubDataPropertyOf(:q1 :p) SubDataPropertyOf(:q2 :p)\n{LIMIT_P}\n\
         SubClassOf(:C DataAllValuesFrom(:q1 DataOneOf(\"1\"^^xsd:integer)))\n\
         SubClassOf(:C DataMinCardinality(1 :q1 xsd:integer))\n\
         SubClassOf(:C DataAllValuesFrom(:q2 DataOneOf(\"2\"^^xsd:integer)))\n\
         SubClassOf(:C DataMinCardinality(1 :q2 xsd:integer))"
    )));
    assert!(c_unsat(
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataMaxCardinality(1 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer \"3\"^^xsd:integer)))\n\
         SubClassOf(:C DataMinCardinality(2 :q DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
    ));
}

/// The `≥` half of an `Exact` on `q` lifts; its `≤` half does not.
#[test]
fn an_exact_on_the_sub_property_lifts_its_minimum_only() {
    let _g = lock();
    let enum3 = "DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer \"3\"^^xsd:integer)";
    assert!(c_unsat(&format!(
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataExactCardinality(2 :q {enum3}))\n\
         SubClassOf(:C DataMaxCardinality(1 :p {enum3}))"
    )));
    assert!(!c_unsat(&format!(
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataExactCardinality(1 :q {enum3}))\n\
         SubClassOf(:C DataMinCardinality(2 :p {enum3}))"
    )));
}

/// DOWN: a `∀` on `p` filters `q`, so `≥2 q.xsd:integer` must find two values in
/// `{1,2}`, which `≤1 q.{1,2}` forbids. As a class axiom, a `DataPropertyRange`, an
/// integer interval (a different integer bucket from the enumeration), a string
/// enumeration and an interval union.
#[test]
fn a_filter_on_the_super_property_narrows_sub_property_demands() {
    let _g = lock();
    let rest = "SubClassOf(:C DataMinCardinality(2 :q xsd:integer))\n\
         SubClassOf(:C DataMaxCardinality(1 :q DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))";
    for filter in [
        "SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))",
        "DataPropertyRange(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer))",
        "SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"2\"^^xsd:integer)))",
    ] {
        let body = format!("SubDataPropertyOf(:q :p)\n{filter}\n{rest}");
        assert!(c_unsat(&body), "{body}");
    }
    assert!(c_unsat(
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"a\" \"b\")))\n\
         SubClassOf(:C DataMinCardinality(2 :q xsd:string))\n\
         SubClassOf(:C DataMaxCardinality(1 :q DataOneOf(\"a\" \"b\")))"
    ));
    let union = |n: u32| {
        format!(
            "SubDataPropertyOf(:q :p)\n\
             SubClassOf(:C DataAllValuesFrom(:p DataUnionOf(\
             DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer) \
             DatatypeRestriction(xsd:integer xsd:minInclusive \"5\"^^xsd:integer xsd:maxInclusive \"5\"^^xsd:integer))))\n\
             SubClassOf(:C DataMinCardinality({n} :q xsd:integer))"
        )
    };
    assert!(c_unsat(&union(4)));
    assert!(!c_unsat(&union(3)));
}

/// A branched `∀` on the super-property: both disjuncts clash with `≥2 q.integer`,
/// so `C` is unsat; with a free second disjunct it is satisfiable. Exercises the
/// clash's dependency set under backjumping.
#[test]
fn a_branched_super_property_filter_backjumps() {
    let _g = lock();
    let demand = "SubDataPropertyOf(:q :p) SubClassOf(:C DataMinCardinality(2 :q xsd:integer))";
    assert!(c_unsat(&format!(
        "{demand}\nSubClassOf(:C ObjectUnionOf(\
         DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)) \
         DataAllValuesFrom(:p DataOneOf(\"2\"^^xsd:integer))))"
    )));
    assert!(!c_unsat(&format!(
        "{demand}\nSubClassOf(:C ObjectUnionOf(\
         DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)) :G))"
    )));
}

/// Controls, all satisfiable: a limit on `q` with values on `p`; a `∀` on `q` with a
/// demand on `p` (the `∀` does not filter `p`); a limit on one sibling with values on
/// the other; siblings that may share a value; values inside the limit.
#[test]
fn limits_and_sub_property_filters_are_not_copied() {
    let _g = lock();
    for body in [
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataMaxCardinality(1 :q DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubClassOf(:C DataHasValue(:p \"1\"^^xsd:integer))\n\
         SubClassOf(:C DataHasValue(:p \"2\"^^xsd:integer))"
            .to_string(),
        "SubDataPropertyOf(:q :p)\n\
         SubClassOf(:C DataAllValuesFrom(:q DataOneOf(\"1\"^^xsd:integer)))\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:integer))"
            .to_string(),
        "SubDataPropertyOf(:q1 :p) SubDataPropertyOf(:q2 :p)\n\
         SubClassOf(:C DataMaxCardinality(1 :q1 DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubClassOf(:C DataHasValue(:q2 \"1\"^^xsd:integer))\n\
         SubClassOf(:C DataHasValue(:q2 \"2\"^^xsd:integer))"
            .to_string(),
        format!(
            "SubDataPropertyOf(:q1 :p) SubDataPropertyOf(:q2 :p)\n{LIMIT_P}\n\
             SubClassOf(:C DataMinCardinality(1 :q1 DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
             SubClassOf(:C DataMinCardinality(1 :q2 DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
        ),
        format!(
            "SubDataPropertyOf(:q :p)\n\
             SubClassOf(:C DataMaxCardinality(2 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
             {TWO_Q_VALUES}"
        ),
    ] {
        assert!(!c_unsat(&body), "{body}");
    }
}

/// The issue's `ABox` reproducer is inconsistent; with the values on `p` and the
/// limit on `q` it is consistent.
#[test]
fn the_abox_reproducer_is_inconsistent() {
    let _g = lock();
    let abox = |limit_on: &str, values_on: &str| {
        format!(
            "SubDataPropertyOf(:q :p)\n\
             SubClassOf(:P DataMaxCardinality(1 :{limit_on} DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
             ClassAssertion(:P :a)\n\
             DataPropertyAssertion(:{values_on} :a \"1\"^^xsd:integer)\n\
             DataPropertyAssertion(:{values_on} :a \"2\"^^xsd:integer)"
        )
    };
    assert!(!is_consistent(&onto(&abox("p", "q"))).expect("consistent"));
    assert!(is_consistent(&onto(&abox("q", "p"))).expect("consistent"));
}

/// `=0` reverts to per-property counting.
#[test]
fn the_flag_off_reverts_to_exact_property_counting() {
    let _g = lock();
    let body = format!("SubDataPropertyOf(:q :p)\n{LIMIT_P}\n{TWO_Q_VALUES}");
    let _env = SetEnvGuard::set("RUSTDL_DATA_COUNTING_ROLE_HIERARCHY", "0");
    assert!(!c_unsat(&body));
}
