//! #42 — qualified data cardinality over a UNION of enumerations.
//!
//! `≥3 p.({1} ⊔ {2})` is unsatisfiable, and rustdl used to report the class
//! satisfiable while dropping the axiom (visibly, via `dropped`). OWL 2 Direct
//! Semantics:
//!
//! ```text
//! DataMinCardinality( n DPE DR ) = { x | #{ y | (x,y) ∈ (DPE)^DP and y ∈ (DR)^DT } ≥ n }
//! DataOneOf( lt1 ... ltn )       = { (lt1)^LT , ... , (ltn)^LT }
//! ```
//!
//! `#` counts DISTINCT values and `{1} ⊔ {2}` denotes the 2-element set `{1,2}`, so
//! `≥ 3` cannot be met and the class is empty.
//!
//! # The peers split, and the majority is on the wrong side
//!
//! **The peers split DIFFERENTLY for `≥` than for `≤`/`=`, so name the operator
//! before citing one.** On `≥n` over a union `HermiT` is right and Konclude and
//! `JFact` under-report (Konclude's ninth recorded instance, `JFact`'s first). On
//! `≤n`/`=n` it REVERSES: Konclude and `JFact` are right and **`HermiT` answers
//! satisfiable** — the first `HermiT` under-report recorded in this project.
//! The discriminating control is that `HermiT` decides the identical
//! hand-written `DataOneOf` correctly, so it can do the reasoning and simply
//! does not see through `DataUnionOf` in a `≤`/`=` position. Kobayashi-MaRust
//! v1.3.0 also says satisfiable, but its answer carries no weight in that direction:
//! it misses an unsat that all four other reasoners agree on
//! (`∀p.(≥1 ⊓ ≤10)` with `∃p.{50}`), so its `satisfiable` is uninformative while its
//! `unsat` is not. **Do not resolve this by counting reasoners** — the semantics
//! decide it, and the boundary pair below is what shows `HermiT` is reasoning about
//! the set SIZE rather than refusing the construct.
//!
//! The fix reuses `flatten_union_of_oneofs`, the same helper (and the same
//! all-or-drop discipline) the `∀`/range path already uses, so the two cannot drift.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

fn unsat(body: &str) -> bool {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/c>\n\
         Declaration(Class(:A)) Declaration(DataProperty(:p))\n\
         {body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    result
        .unsatisfiable_classes()
        .iter()
        .any(|c| c.ends_with("/A"))
}

/// Two DISTINCT witnesses, as `DataSomeValuesFrom(p, DataOneOf(v))` — see
/// `max_cardinality_below_the_union_size_is_unsatisfiable` for why `DataHasValue`
/// cannot be used here.
const WITNESSES: &str = "SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)))\n\
     SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"2\"^^xsd:integer)))";

const UNION2: &str = "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) DataOneOf(\"2\"^^xsd:integer))";

#[test]
fn min_cardinality_above_the_union_size_is_unsatisfiable() {
    assert!(
        unsat(&format!("SubClassOf(:A DataMinCardinality(3 :p {UNION2}))")),
        "≥3 over a 2-value union is unsatisfiable by the OWL 2 semantics (HermiT agrees)"
    );
}

#[test]
fn min_cardinality_at_the_union_size_stays_satisfiable() {
    // THE BOUNDARY CONTROL, one integer from its twin above. Without it, "≥n is
    // unsat" would be satisfied by an implementation that calls every qualified
    // min-cardinality unsatisfiable. HermiT gives the same discriminating pair,
    // which is what shows it reasons about the set SIZE.
    assert!(
        !unsat(&format!("SubClassOf(:A DataMinCardinality(2 :p {UNION2}))")),
        "≥2 over a 2-value union is satisfiable — FALSE POSITIVE if this trips"
    );
}

#[test]
fn the_plain_enumeration_form_still_works() {
    // Worked before the union flattening reached the cardinality path; pins that it
    // was not disturbed.
    assert!(unsat(
        "SubClassOf(:A DataMinCardinality(3 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
    ));
}

#[test]
fn a_nested_union_flattens_too() {
    // `⊔` is associative; the helper recurses.
    let nested = "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) \
                  DataUnionOf(DataOneOf(\"2\"^^xsd:integer) DataOneOf(\"3\"^^xsd:integer)))";
    assert!(
        unsat(&format!("SubClassOf(:A DataMinCardinality(4 :p {nested}))")),
        "≥4 over a 3-value nested union is unsatisfiable"
    );
    assert!(
        !unsat(&format!("SubClassOf(:A DataMinCardinality(3 :p {nested}))")),
        "≥3 over a 3-value nested union is satisfiable"
    );
}

#[test]
fn duplicate_values_across_the_union_are_counted_once() {
    // `{1,2} ⊔ {2,3}` has THREE distinct values, not four — dedup is by VALUE.
    // Over-counting here would make `≥4` satisfiable (a MISS); under-counting would
    // make `≥3` unsatisfiable (a FALSE POSITIVE). Both directions are pinned.
    let overlap = "DataUnionOf(DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer) \
                   DataOneOf(\"2\"^^xsd:integer \"3\"^^xsd:integer))";
    assert!(
        unsat(&format!(
            "SubClassOf(:A DataMinCardinality(4 :p {overlap}))"
        )),
        "the union holds 3 distinct values, so ≥4 is unsatisfiable"
    );
    assert!(
        !unsat(&format!(
            "SubClassOf(:A DataMinCardinality(3 :p {overlap}))"
        )),
        "≥3 is satisfiable over 3 distinct values — FALSE POSITIVE if this trips"
    );
}

#[test]
fn max_cardinality_below_the_union_size_is_unsatisfiable() {
    // CLOSES SABOTAGE SURVIVOR 1: reverting the flatten in the `Max`+`Exact` arms
    // ONLY left all 21 tests across three files green, because every canary here
    // was `DataMinCardinality`.
    //
    // WITNESSES MUST BE `DataSomeValuesFrom(p, DataOneOf(v))`, NOT `DataHasValue`.
    // `DataHasValue` lowers to the RANGE bucket `[v,v]` while `≤n`-over-enumeration
    // lands in the `io:` bucket, and `concrete_domain_clash` checks buckets
    // independently — so the `DataHasValue` spelling is SATISFIABLE on correct code
    // (a pre-existing SILENT miss: `incomplete: false`, `dropped: {}`, while all
    // three peers say unsat). Writing this canary the obvious way would fail against
    // a correct engine and invite "fixing" the implementation to match.
    //
    // Oracle: Konclude and JFact. `HermiT` says SATISFIABLE here — see the header.
    assert!(
        unsat(&format!(
            "SubClassOf(:A DataMaxCardinality(1 :p {UNION2}))\n{WITNESSES}"
        )),
        "two distinct values both in a 2-value union cannot satisfy ≤1"
    );
}

#[test]
fn max_cardinality_at_the_union_size_stays_satisfiable() {
    // The boundary twin, one integer away — without it the test above is satisfied
    // by an implementation that calls every `≤n` unsatisfiable.
    assert!(
        !unsat(&format!(
            "SubClassOf(:A DataMaxCardinality(2 :p {UNION2}))\n{WITNESSES}"
        )),
        "≤2 with two witnesses is satisfiable — FALSE POSITIVE if this trips"
    );
}

#[test]
fn exact_cardinality_over_a_union_is_flattened_in_both_directions() {
    // `=n` is `≥n ⊓ ≤n`, so it must fail for BOTH reasons. `=1` with two distinct
    // witnesses violates the `≤` half; `=3` over a 2-value union violates the `≥`
    // half; `=2` satisfies both and pins that the arm is not simply always-unsat.
    assert!(unsat(&format!(
        "SubClassOf(:A DataExactCardinality(1 :p {UNION2}))\n{WITNESSES}"
    )));
    assert!(unsat(&format!(
        "SubClassOf(:A DataExactCardinality(3 :p {UNION2}))"
    )));
    assert!(!unsat(&format!(
        "SubClassOf(:A DataExactCardinality(2 :p {UNION2}))"
    )));
}

#[test]
fn a_flat_three_member_union_flattens() {
    // CLOSES SABOTAGE SURVIVOR 2: a `members.len() != 2 => None` guard survived the
    // entire suite, because EVERY union in all three test files — and the unit pin
    // in `convert.rs` — has exactly two top-level members. `a_nested_union_flattens_too`
    // does not cover this: it is 2-member at the top and nests.
    let u3 = "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) DataOneOf(\"2\"^^xsd:integer) \
              DataOneOf(\"3\"^^xsd:integer))";
    assert!(
        unsat(&format!("SubClassOf(:A DataMinCardinality(4 :p {u3}))")),
        "≥4 over a flat 3-value union is unsatisfiable"
    );
    assert!(
        !unsat(&format!("SubClassOf(:A DataMinCardinality(3 :p {u3}))")),
        "≥3 over a flat 3-value union is satisfiable"
    );
}

#[test]
fn the_identity_is_not_integer_specific() {
    // Makes this file SELF-SUFFICIENT. Filtering `collect` to `xsd:integer` literals
    // survived every test here and was caught only by a neighbouring file, for an
    // unrelated reason — so the coverage was incidental and would vanish silently if
    // that neighbour were ever retargeted.
    let us = "DataUnionOf(DataOneOf(\"a\"^^xsd:string) DataOneOf(\"b\"^^xsd:string))";
    assert!(
        unsat(&format!("SubClassOf(:A DataMinCardinality(3 :p {us}))")),
        "≥3 over a 2-value STRING union is unsatisfiable"
    );
    assert!(
        !unsat(&format!("SubClassOf(:A DataMinCardinality(2 :p {us}))")),
        "≥2 over a 2-value STRING union is satisfiable"
    );
}

#[test]
fn a_union_carrying_a_non_enumeration_member_still_drops() {
    // ALL-OR-DROP, inherited from `flatten_union_of_oneofs`. A partial flatten would
    // be a strictly weaker range, which is unsound in the sufficient direction. The
    // class stays satisfiable (a sound MISS, and a VISIBLE one — `dropped` is set).
    let mixed = "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) \
                 DatatypeRestriction(xsd:integer xsd:minInclusive \"100\"^^xsd:integer))";
    assert!(
        !unsat(&format!("SubClassOf(:A DataMinCardinality(3 :p {mixed}))")),
        "a union with a non-enumeration member must drop whole, not flatten partially"
    );
}
