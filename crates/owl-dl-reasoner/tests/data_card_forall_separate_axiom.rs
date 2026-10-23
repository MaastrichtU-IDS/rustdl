//! #327 — a data `∀` from a SEPARATE axiom must reach the cardinality count.
//!
//! `C ⊑ ≥2 p.xsd:string` with `C ⊑ ∀p.DataOneOf("a")` is unsatisfiable: two distinct
//! values, all equal to `"a"`. Written as one `ObjectIntersectionOf` the tableau found
//! it; split into two axioms it said satisfiable with nothing dropped. Absorption
//! turns a told `C ⊑ ∀p.D` into a role rule that fires at `p`-successors, so the `∀`
//! never sat on `C`'s label, which is all `concrete_domain_clash` read. The same held
//! for `DataPropertyRange(p, …)`, an unguarded role rule. `concrete_domain_clash` now
//! also reads the role rules that hold at the node as universal filters.
//!
//! **DIRECTION OF RISK IS A FALSE POSITIVE**, since the fix adds clashes. Each rule
//! read is an axiom (`G ⊑ ∀p.D`, or `⊤ ⊑ ∀p.D`) whose guard is on the node, so the
//! filter is entailed there; the controls carry the weight.
//!
//! ORACLE: `HermiT` 1.4.5 agrees on every verdict below except the two pinned known
//! misses.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, dropped_axioms};
use std::io::Cursor;
use std::time::Duration;

fn onto(body: &str) -> SetOntology<RcStr> {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:G))\n\
         Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn c_unsat(body: &str) -> bool {
    let o = onto(body);
    assert!(
        dropped_axioms(&o).expect("dropped_axioms").is_empty(),
        "nothing may drop: {body}"
    );
    classify_top_down_with_timeout(&o, Duration::from_secs(10))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&"http://ex.org/C")
}

/// The issue's reproducer, string and boolean, against its one-value-more control.
#[test]
fn a_separate_forall_bounds_the_count() {
    let body = |dt: &str, vals: &str| {
        format!(
            "SubClassOf(:C DataMinCardinality(2 :p {dt}))\n\
             SubClassOf(:C DataAllValuesFrom(:p DataOneOf({vals})))"
        )
    };
    assert!(c_unsat(&body("xsd:string", "\"a\"")));
    assert!(!c_unsat(&body("xsd:string", "\"a\" \"b\"")));
    assert!(c_unsat(&body("xsd:boolean", "\"true\"^^xsd:boolean")));
}

/// An integer enumeration and an integer interval as the separate `∀`.
#[test]
fn a_separate_integer_forall_bounds_the_count() {
    let oneof = |allowed: &str| {
        format!(
            "SubClassOf(:C DataMinCardinality(2 :p \
             DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer \"3\"^^xsd:integer)))\n\
             SubClassOf(:C DataAllValuesFrom(:p DataOneOf({allowed})))"
        )
    };
    assert!(c_unsat(&oneof("\"1\"^^xsd:integer")));
    assert!(!c_unsat(&oneof("\"1\"^^xsd:integer \"2\"^^xsd:integer")));
    let interval = |n: u32| {
        format!(
            "SubClassOf(:C DataMinCardinality({n} :p xsd:integer))\n\
             SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer \
             xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"2\"^^xsd:integer)))"
        )
    };
    assert!(c_unsat(&interval(3)), "[1,2] holds two integers, not three");
    assert!(!c_unsat(&interval(2)));
}

/// `DataPropertyRange` is an unguarded `∀` and bounds the count everywhere.
#[test]
fn a_data_range_bounds_the_count() {
    let body = |vals: &str| {
        format!(
            "SubClassOf(:C DataMinCardinality(2 :p xsd:string))\n\
             DataPropertyRange(:p DataOneOf({vals}))"
        )
    };
    assert!(c_unsat(&body("\"a\"")));
    assert!(!c_unsat(&body("\"a\" \"b\"")));
}

/// The `∀` may come from a superclass; it holds wherever its guard does.
#[test]
fn a_forall_inherited_from_a_superclass_bounds_the_count() {
    assert!(c_unsat(
        "SubClassOf(:C :G) SubClassOf(:C DataMinCardinality(2 :p xsd:string))\n\
         SubClassOf(:G DataAllValuesFrom(:p DataOneOf(\"a\")))"
    ));
}

/// FP guards: a `∀` on another property, or one whose guard the node lacks, does
/// not constrain `p` here.
#[test]
fn a_forall_that_does_not_hold_here_is_not_counted() {
    assert!(!c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:string))\n\
         SubClassOf(:C DataAllValuesFrom(:q DataOneOf(\"a\")))"
    ));
    assert!(!c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:string))\n\
         SubClassOf(:G DataAllValuesFrom(:p DataOneOf(\"a\")))"
    ));
    // `C` does carry a guarded rule (on `q`); `G`'s rule on `p` still must not apply.
    assert!(!c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:string))\n\
         SubClassOf(:C DataAllValuesFrom(:q DataOneOf(\"x\")))\n\
         SubClassOf(:G DataAllValuesFrom(:p DataOneOf(\"a\")))"
    ));
}

/// Backjumping: the guard `G` is reached through the `⊒` half of `G ≡ ∀p.{"a"}`,
/// a disjunction; the branch that puts `G` on `C` clashes and the search must
/// backjump to the other one, which only correct deps on the `∀` allow. And an
/// unbounded range (`xsd:string`) bounds nothing.
#[test]
fn a_forall_reached_by_branching_or_unbounded_does_not_clash() {
    assert!(!c_unsat(
        "EquivalentClasses(:G DataAllValuesFrom(:p DataOneOf(\"a\")))\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:string))"
    ));
    assert!(!c_unsat(
        "DataPropertyRange(:p DataOneOf(\"a\"))\n\
         SubClassOf(:C ObjectUnionOf(DataMinCardinality(2 :p xsd:string) :G))"
    ));
    assert!(!c_unsat(
        "DataPropertyRange(:p xsd:string)\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:string))"
    ));
}

/// KNOWN MISS, not #327: the count is keyed on the exact property, so a range on
/// `p` does not bound a demand on a sub-property `q ⊑ p`. `HermiT` says unsat.
/// FLIP when the count follows the property hierarchy. The reverse (range on the
/// sub-property) is genuinely satisfiable.
#[test]
fn a_range_on_a_super_property_is_a_known_miss() {
    assert!(!c_unsat(
        "SubDataPropertyOf(:q :p) DataPropertyRange(:p DataOneOf(\"a\"))\n\
         SubClassOf(:C DataMinCardinality(2 :q xsd:string))"
    ));
    assert!(!c_unsat(
        "SubDataPropertyOf(:q :p) DataPropertyRange(:q DataOneOf(\"a\"))\n\
         SubClassOf(:C DataMinCardinality(2 :p xsd:string))"
    ));
}

/// KNOWN MISS, not #327: an integer RANGE demand (`≥2 p.xsd:integer`) against an
/// integer ENUMERATION filter is not counted together, in one axiom or two; integer
/// ranges and integer enumerations are separate `card_sat` buckets. `HermiT` says
/// unsat. FLIP these when the two buckets are counted together.
#[test]
fn an_integer_range_demand_against_an_enumeration_is_a_known_miss() {
    assert!(!c_unsat(
        "SubClassOf(:C DataMinCardinality(2 :p xsd:integer))\n\
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)))"
    ));
    assert!(!c_unsat(
        "SubClassOf(:C ObjectIntersectionOf(DataMinCardinality(2 :p xsd:integer) \
         DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer))))"
    ));
}
