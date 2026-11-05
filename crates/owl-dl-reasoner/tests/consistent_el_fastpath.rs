//! #459: `consistent` answers an input that classify's saturation fast path admits
//! and that names no individual from the EL saturator, as classify does.
//!
//! Before, such an input reached the unbounded main-tableau `decide(Top)`; a
//! `⊤ ⊑ ∃r.B` GCI with cyclic existential definitions kept it busy for minutes
//! (12 of 1000 `el_top` generator cases past 30 s), while classify answered in
//! under a second.
//!
//! The danger in this direction is a wrong `true`, so most canaries here are
//! INCONSISTENT inputs the shortcut must still call inconsistent, each checked
//! through `consistent`, `classify`, the bounded API and the flag-off path, and
//! each confirmed to be gate-admitted with nothing the EL lowering drops
//! (`certified_unlowered`). The scope tests pin that inputs with individuals,
//! and inputs outside the gate, keep their old route.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    certified_unlowered, classify, is_consistent_with_stats, is_consistent_with_timeout,
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

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex/>\n{axioms}\n)"
    );
    let (onto, _) = read_ofn(
        &mut std::io::Cursor::new(src),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    onto
}

const DECLS: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
     Declaration(Class(:D)) \
     Declaration(Class(:X)) Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
     Declaration(ObjectProperty(:t)) Declaration(DataProperty(:p))";

/// Individual-free inputs classify's fast path admits: `(name, axioms, consistent)`.
const ADMITTED: &[(&str, &str, bool)] = &[
    ("top_sub_bottom", "SubClassOf(owl:Thing owl:Nothing)", false),
    (
        "top_sub_unsat_atom",
        "SubClassOf(owl:Thing :A) SubClassOf(:A owl:Nothing)",
        false,
    ),
    (
        "top_needs_unsat_filler",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) SubClassOf(:B owl:Nothing)",
        false,
    ),
    (
        "top_needs_disjoint_conjunction",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A :B))) \
         DisjointClasses(:A :B)",
        false,
    ),
    (
        "range_unsat",
        "ObjectPropertyRange(:r :X) SubClassOf(:X owl:Nothing) \
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r owl:Thing))",
        false,
    ),
    (
        "super_role_range_unsat",
        "SubObjectPropertyOf(:s :r) ObjectPropertyRange(:r :X) SubClassOf(:X owl:Nothing) \
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:s owl:Thing))",
        false,
    ),
    (
        "domain_bottom",
        "ObjectPropertyDomain(:r owl:Nothing) \
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r owl:Thing))",
        false,
    ),
    (
        "thing_equivalent_to_unsat",
        "EquivalentClasses(owl:Thing :C) SubClassOf(:C owl:Nothing)",
        false,
    ),
    (
        "nested_existential_unsat",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B))) \
         SubClassOf(:B owl:Nothing)",
        false,
    ),
    (
        "functional_role_unsat_filler",
        "FunctionalObjectProperty(:r) SubClassOf(owl:Thing ObjectIntersectionOf(\
         ObjectSomeValuesFrom(:r :A) ObjectSomeValuesFrom(:r :B))) SubClassOf(:A owl:Nothing)",
        false,
    ),
    (
        "transitive_role_reaches_unsat",
        "TransitiveObjectProperty(:t) SubClassOf(owl:Thing ObjectSomeValuesFrom(:t :A)) \
         SubClassOf(:A ObjectSomeValuesFrom(:t :B)) \
         SubClassOf(ObjectSomeValuesFrom(:t :B) owl:Nothing)",
        false,
    ),
    (
        "top_needs_empty_integer_range",
        "SubClassOf(owl:Thing DataSomeValuesFrom(:p DataIntersectionOf(\
           DatatypeRestriction(xsd:integer xsd:minExclusive \"5\"^^xsd:integer) \
           DatatypeRestriction(xsd:integer xsd:maxExclusive \"3\"^^xsd:integer))))",
        false,
    ),
    (
        "domain_unsat",
        "ObjectPropertyDomain(:r :X) SubClassOf(:X owl:Nothing) \
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r owl:Thing))",
        false,
    ),
    (
        "role_chain_reaches_unsat",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :A))) \
         SubClassOf(ObjectSomeValuesFrom(:t :A) owl:Nothing)",
        false,
    ),
    (
        "data_value_outside_range",
        "DataPropertyRange(:p xsd:integer) SubClassOf(owl:Thing DataHasValue(:p \"a\"))",
        false,
    ),
    // The #459 reproducer (6-axiom shrink of el_top case 74).
    (
        "reproducer_min_00074",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :C)) \
         SubClassOf(ObjectSomeValuesFrom(:s owl:Thing) ObjectIntersectionOf(:X \
           ObjectSomeValuesFrom(:r :B))) \
         EquivalentClasses(:A ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s :X))) \
         EquivalentClasses(:B ObjectSomeValuesFrom(:r :C)) \
         EquivalentClasses(:B ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s owl:Thing))) \
         EquivalentClasses(:C ObjectSomeValuesFrom(:r :D))",
        true,
    ),
    (
        "disjoint_fillers_of_different_roles",
        "DisjointClasses(:A :B) SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :A)) \
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:s :B))",
        true,
    ),
    (
        "unsat_classes_alone",
        "SubClassOf(:A owl:Nothing) SubClassOf(:B owl:Nothing)",
        true,
    ),
];

fn check_admitted(name: &str, axioms: &str, consistent: bool) {
    let onto = parse(&format!("{DECLS} {axioms}"));
    let certified = certified_unlowered(&onto).expect("converts");
    assert!(
        certified.classify_certified,
        "{name}: classify's fast path admits it"
    );
    assert!(
        certified.classify.is_empty(),
        "{name}: the EL lowering drops nothing: {:?}",
        certified.classify
    );

    let (verdict, stats) = is_consistent_with_stats(&onto).expect("decides");
    assert_eq!(verdict, consistent, "{name}: consistent");
    assert!(
        stats.answered_by_saturation && stats.pure_el_mode,
        "{name}: the EL saturator decided ({stats:?})"
    );
    assert!(!stats.incomplete, "{name}");

    let c = classify(&onto).expect("classifies");
    assert_eq!(
        !c.stats().inconsistent,
        consistent,
        "{name}: classify agrees"
    );

    assert_eq!(
        is_consistent_with_timeout(&onto, Duration::from_secs(60)).expect("decides"),
        Some(consistent),
        "{name}: bounded API"
    );
}

#[test]
fn admitted_individual_free_inputs_are_decided_by_the_saturator() {
    let _l = lock();
    for &(name, axioms, consistent) in ADMITTED {
        check_admitted(name, axioms, consistent);
    }
}

/// `RUSTDL_CONSISTENT_EL_FASTPATH=0` restores the old route, which reaches the
/// same verdict on the inconsistent canaries (the tableau finds their clash at
/// once; the consistent reproducer is what it could not finish).
#[test]
fn flag_off_restores_the_old_route_with_the_same_verdicts() {
    let _l = lock();
    let _off = EnvGuard::set("RUSTDL_CONSISTENT_EL_FASTPATH", "0");
    for &(name, axioms, consistent) in ADMITTED.iter().filter(|c| !c.2) {
        let onto = parse(&format!("{DECLS} {axioms}"));
        let (verdict, stats) = is_consistent_with_stats(&onto).expect("decides");
        assert_eq!(verdict, consistent, "{name}: flag off");
        assert!(
            !stats.pure_el_mode,
            "{name}: flag off takes the old route ({stats:?})"
        );
    }
}

/// With `RUSTDL_SAT_INCONSISTENCY_ALL_SURFACES=0` the saturator's verdict is read
/// nowhere, so the shortcut must not run (it would answer `true` blindly).
#[test]
fn shortcut_needs_the_saturator_verdict_surface() {
    let _l = lock();
    let _off = EnvGuard::set("RUSTDL_SAT_INCONSISTENCY_ALL_SURFACES", "0");
    let onto = parse(&format!(
        "{DECLS} SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) SubClassOf(:B owl:Nothing)"
    ));
    let (verdict, stats) = is_consistent_with_stats(&onto).expect("decides");
    assert!(!verdict, "still inconsistent, by another route");
    assert!(!stats.pure_el_mode, "{stats:?}");
}

/// Inputs with individuals keep their old route, including with the consistency
/// wedge off: the saturator never sees an `ABox` the way the `ABox` checks do.
#[test]
fn inputs_with_individuals_keep_their_route() {
    let _l = lock();
    let cases: &[(&str, &str, bool)] = &[
        (
            "class_assertion",
            "Declaration(NamedIndividual(:a)) ClassAssertion(:A :a) \
             SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))",
            true,
        ),
        (
            "negative_property_assertion",
            "Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
             ObjectPropertyAssertion(:r :a :b) NegativeObjectPropertyAssertion(:r :a :b)",
            false,
        ),
        (
            "functional_merge_of_different_individuals",
            "Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
             Declaration(NamedIndividual(:c)) FunctionalObjectProperty(:r) \
             ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :a :c) \
             DifferentIndividuals(:b :c)",
            false,
        ),
        (
            "tbox_nominal",
            "Declaration(NamedIndividual(:a)) SubClassOf(ObjectOneOf(:a) owl:Nothing)",
            false,
        ),
    ];
    for wedge_off in [false, true] {
        let _w = wedge_off.then(|| EnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0"));
        for &(name, axioms, consistent) in cases {
            let onto = parse(&format!("{DECLS} {axioms}"));
            let (verdict, stats) = is_consistent_with_stats(&onto).expect("decides");
            assert_eq!(verdict, consistent, "{name} (wedge off: {wedge_off})");
            assert!(
                !stats.pure_el_mode,
                "{name} (wedge off: {wedge_off}): not the individual-free shortcut ({stats:?})"
            );
        }
    }
}

/// Outside the gate the shortcut does not apply, whatever the saturator says.
///
/// The second input pins that a functional data role with two clashing values
/// stays off the shortcut: its DKey value-space disjointness is seeded as
/// `DisjointClasses`, which the gate rejects beside a functional role. That is the
/// one place where the tableau's datatype reasoning could otherwise go beyond the
/// saturator's.
#[test]
fn inputs_outside_the_gate_keep_their_route() {
    let _l = lock();
    for (name, axioms) in [
        (
            "top_sub_union_of_unsat",
            "SubClassOf(owl:Thing ObjectUnionOf(:A :B)) SubClassOf(:A owl:Nothing) \
             SubClassOf(:B owl:Nothing)",
        ),
        (
            "functional_data_role_two_values",
            "FunctionalDataProperty(:p) SubClassOf(owl:Thing ObjectIntersectionOf(\
             DataHasValue(:p \"1\"^^xsd:integer) DataHasValue(:p \"2\"^^xsd:integer)))",
        ),
    ] {
        let onto = parse(&format!("{DECLS} {axioms}"));
        assert!(
            !certified_unlowered(&onto).unwrap().classify_certified,
            "{name}: must be outside the gate"
        );
        let (verdict, stats) = is_consistent_with_stats(&onto).expect("decides");
        assert!(!verdict, "{name}: inconsistent");
        assert!(!stats.pure_el_mode, "{name}: {stats:?}");
    }
}
