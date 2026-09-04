//! #42 item 2 — a `DataOneOf` enumeration under `∀` must clash with a
//! `DataHasValue`/facet range whose value lies OUTSIDE it.
//!
//! `DataHasValue` and facet restrictions lower into a RANGE bucket; `DataOneOf`
//! lowers into a separate ENUMERATION bucket. Disjointness was seeded within each
//! bucket and never across, so `∃p.{5} ⊓ ∀p.{1,2}` did not clash.
//!
//! **`xsd:string` is the discriminating control and it always passed**: both its
//! forms lower to one `StrSet` in the `str:` bucket, so `StrSet::disjoint` already
//! compared them. That asymmetry is what identified the defect — an
//! integer probe missing while the structurally identical string probe hit.
//!
//! **The corpus cannot validate this area.** `datatype_value_membership.rs` says so
//! itself ("the corpus has NO such clash, so these canaries are the ENTIRE safety
//! net"), and these probes are that net plus the `HermiT` adjudication recorded in
//! `docs/known-limitations/` — a green FP=0 net here shows non-regression only.
//!
//! Oracle: `HermiT` confirms every positive below. **Konclude does NOT confirm the
//! `xsd:date` one** — it reports `A ⊑ owl:Thing` with no unsatisfiability — which is
//! a further instance of the under-reporting this repo already records, not a
//! disagreement about the semantics. The NEGATIVE controls are what make the
//! positives meaningful: each is one value away from its positive twin.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

fn unsat_classes(ofn: &str) -> Vec<String> {
    let mut reader = Cursor::new(ofn.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    // Force the slow, complete path so a MISS here is calculus, not a
    // trust_sat mask.
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    let mut out: Vec<String> = result
        .unsatisfiable_classes()
        .iter()
        .map(std::string::ToString::to_string)
        .filter(|c| !c.contains("Nothing"))
        .collect();
    out.sort();
    out
}

fn fixture(value_axiom: &str, oneof: &str) -> String {
    format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:A))\n\
         Declaration(DataProperty(:p))\n\
         SubClassOf(:A {value_axiom})\n\
         SubClassOf(:A DataAllValuesFrom(:p DataOneOf({oneof})))\n\
         )\n"
    )
}

fn assert_unsat(what: &str, value_axiom: &str, oneof: &str) {
    let got = unsat_classes(&fixture(value_axiom, oneof));
    assert_eq!(
        got,
        vec!["http://ex.org/A".to_string()],
        "{what}: the value is OUTSIDE the enumeration, so A is unsatisfiable \
         (HermiT agrees). Got {got:?}. If this regressed, check that \
         `seed_range_oneof_disjoint` still pairs this datatype's range bucket \
         with its enumeration bucket in `convert.rs`."
    );
}

fn assert_sat(what: &str, value_axiom: &str, oneof: &str) {
    let got = unsat_classes(&fixture(value_axiom, oneof));
    assert!(
        got.is_empty(),
        "{what}: the value IS IN the enumeration, so A must stay satisfiable. \
         Reporting it unsatisfiable is a FALSE POSITIVE — the seeding emitted \
         `DisjointClasses` for a pair that shares a value. Got {got:?}."
    );
}

#[test]
fn integer_value_outside_a_oneof_under_forall_clashes() {
    assert_unsat(
        "xsd:integer",
        "DataHasValue(:p \"5\"^^xsd:integer)",
        "\"1\"^^xsd:integer \"2\"^^xsd:integer",
    );
}

#[test]
fn integer_value_inside_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:integer",
        "DataHasValue(:p \"2\"^^xsd:integer)",
        "\"1\"^^xsd:integer \"2\"^^xsd:integer",
    );
}

#[test]
fn integer_facet_range_disjoint_from_a_oneof_clashes() {
    // The range side as a FACET rather than a point value: the seeding must
    // compare a genuine interval against the set, not just singletons.
    assert_unsat(
        "xsd:integer facet",
        "DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive \"10\"^^xsd:integer))",
        "\"1\"^^xsd:integer \"2\"^^xsd:integer",
    );
}

#[test]
fn integer_facet_range_overlapping_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:integer facet",
        "DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive \"2\"^^xsd:integer))",
        "\"1\"^^xsd:integer \"2\"^^xsd:integer",
    );
}

#[test]
fn double_value_outside_a_oneof_under_forall_clashes() {
    assert_unsat(
        "xsd:double",
        "DataSomeValuesFrom(:p DatatypeRestriction(xsd:double xsd:minInclusive \"10.0\"^^xsd:double))",
        "\"1.0\"^^xsd:double \"2.0\"^^xsd:double",
    );
}

#[test]
fn double_value_inside_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:double",
        "DataSomeValuesFrom(:p DatatypeRestriction(xsd:double xsd:minInclusive \"1.0\"^^xsd:double))",
        "\"1.0\"^^xsd:double \"2.0\"^^xsd:double",
    );
}

#[test]
fn an_exclusive_lower_bound_excludes_its_own_endpoint() {
    // FP-CRITICAL for the real datatypes: the `±1` normalization that makes
    // integer bounds simple is INVALID here, so `contains` must compare an
    // exclusive bound STRICTLY. `(2.0, ∞)` shares no value with {1.0, 2.0}.
    assert_unsat(
        "xsd:double minExclusive",
        "DataSomeValuesFrom(:p DatatypeRestriction(xsd:double xsd:minExclusive \"2.0\"^^xsd:double))",
        "\"1.0\"^^xsd:double \"2.0\"^^xsd:double",
    );
}

#[test]
fn an_inclusive_lower_bound_admits_its_own_endpoint() {
    // The twin of the above, one facet keyword apart. If `contains` ignored the
    // inclusivity flags, exactly one of this pair would break — which is the
    // whole point of running them together.
    assert_sat(
        "xsd:double minInclusive",
        "DataSomeValuesFrom(:p DatatypeRestriction(xsd:double xsd:minInclusive \"2.0\"^^xsd:double))",
        "\"1.0\"^^xsd:double \"2.0\"^^xsd:double",
    );
}

#[test]
fn float_value_outside_a_oneof_under_forall_clashes() {
    // `xsd:float` keeps its OWN buckets (`f:` / `fo:`), both f32-parsed then
    // widened — pairing them is exact. Pairing a float range against a DOUBLE
    // enumeration would re-introduce the f32/f64 mismatch and is not done.
    assert_unsat(
        "xsd:float",
        "DataHasValue(:p \"5.0\"^^xsd:float)",
        "\"1.0\"^^xsd:float \"2.0\"^^xsd:float",
    );
}

#[test]
fn float_value_inside_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:float",
        "DataHasValue(:p \"2.0\"^^xsd:float)",
        "\"1.0\"^^xsd:float \"2.0\"^^xsd:float",
    );
}

#[test]
fn decimal_value_outside_a_oneof_under_forall_clashes() {
    assert_unsat(
        "xsd:decimal",
        "DataHasValue(:p \"5.5\"^^xsd:decimal)",
        "\"1.5\"^^xsd:decimal \"2.5\"^^xsd:decimal",
    );
}

#[test]
fn decimal_value_inside_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:decimal",
        "DataHasValue(:p \"2.5\"^^xsd:decimal)",
        "\"1.5\"^^xsd:decimal \"2.5\"^^xsd:decimal",
    );
}

#[test]
fn date_value_outside_a_oneof_under_forall_clashes() {
    // The one HermiT confirms and Konclude does not — see the module header.
    assert_unsat(
        "xsd:date",
        "DataHasValue(:p \"2020-05-05\"^^xsd:date)",
        "\"2001-01-01\"^^xsd:date \"2002-02-02\"^^xsd:date",
    );
}

#[test]
fn date_value_inside_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:date",
        "DataHasValue(:p \"2002-02-02\"^^xsd:date)",
        "\"2001-01-01\"^^xsd:date \"2002-02-02\"^^xsd:date",
    );
}

#[test]
fn datetime_value_outside_a_oneof_under_forall_clashes() {
    assert_unsat(
        "xsd:dateTime",
        "DataHasValue(:p \"2020-05-05T00:00:00\"^^xsd:dateTime)",
        "\"2001-01-01T00:00:00\"^^xsd:dateTime \"2002-02-02T00:00:00\"^^xsd:dateTime",
    );
}

#[test]
fn datetime_value_inside_the_oneof_stays_satisfiable() {
    assert_sat(
        "xsd:dateTime",
        "DataHasValue(:p \"2002-02-02T00:00:00\"^^xsd:dateTime)",
        "\"2001-01-01T00:00:00\"^^xsd:dateTime \"2002-02-02T00:00:00\"^^xsd:dateTime",
    );
}

#[test]
fn string_control_was_never_broken_and_must_stay_working() {
    // The DISCRIMINATING CONTROL. Strings put both forms in one bucket, so this
    // passed before the fix. If it ever fails, the diagnosis in the module header
    // is wrong and the whole framing of #42 item 2 needs revisiting.
    assert_unsat(
        "xsd:string",
        "DataHasValue(:p \"e\"^^xsd:string)",
        "\"a\"^^xsd:string \"b\"^^xsd:string",
    );
}

#[test]
fn a_cross_datatype_pair_is_not_seeded_and_stays_a_sound_miss() {
    // `∃p.{5}^^integer ⊓ ∀p.{1.0,2.0}^^double` IS unsatisfiable (both HermiT and
    // Konclude say so) because the two value spaces are disjoint. rustdl misses
    // it: the range×enumeration seeding is same-datatype only, and #86's
    // cross-datatype seeding covers range×range only.
    //
    // Pinned so the MISS is deliberate rather than forgotten. A future fix should
    // FLIP this assertion — never delete it.
    assert_sat(
        "integer range vs double enumeration",
        "DataHasValue(:p \"5\"^^xsd:integer)",
        "\"1.0\"^^xsd:double \"2.0\"^^xsd:double",
    );
}
