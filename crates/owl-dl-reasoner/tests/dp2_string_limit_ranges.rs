//! #337 — the DP-2 pre-check counts an individual's string values against a
//! `≤n dp D` limit and reports inconsistency when there are more than `n`. It
//! counted them against ANY string-family `D`, but `xsd:token`, `xsd:NCName`,
//! `xsd:language` and facet-restricted strings are proper subsets of
//! `xsd:string`: `≤1 p.xsd:token` with `"x  y"` and `"z  w"` (not tokens) was
//! reported inconsistent. DP-2 now counts only against a range holding every
//! string: `rdfs:Literal` (unqualified), `xsd:string`, `rdf:PlainLiteral`.
//!
//! `rdf:PlainLiteral` is NEW: it was not counted before, so `≤1 p.rdf:PlainLiteral`
//! with two plain strings was wrongly consistent.
//!
//! ORACLE: `HermiT` 1.4.5. Konclude treats the named derived string types
//! (`xsd:token`, `NCName`, `language`) as `xsd:string` and is not an oracle for
//! them; `HermiT` is off on `≤n` over `rdf:langString` and over `DataUnionOf`, so
//! no test here relies on those.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;

fn consistent(body: &str) -> bool {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)\n\
         Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:C)) Declaration(Class(:A))\n\
         Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n\
         Declaration(NamedIndividual(:a))\n{body}\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("consistent")
}

/// `a : C`, `C ⊑ ≤1 p.{limit}`, and `a` has the two plain string values.
fn limit(limit: &str, v1: &str, v2: &str) -> String {
    format!(
        "SubClassOf(:C DataMaxCardinality(1 :p {limit}))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a {v1}) DataPropertyAssertion(:p :a {v2})"
    )
}

/// The issue's false inconsistencies: neither value lies in the limit's range.
#[test]
fn values_outside_a_narrower_string_limit_are_not_counted() {
    for (range, v1, v2) in [
        ("xsd:token", "\"x  y\"", "\"z  w\""),
        ("xsd:NCName", "\"a b\"", "\"c d\""),
        (
            "xsd:language",
            "\"not a language tag!\"",
            "\"nor this one!\"",
        ),
        ("xsd:Name", "\"a b\"", "\"c d\""),
        ("xsd:NMTOKEN", "\"a b\"", "\"c d\""),
        (
            "DatatypeRestriction(xsd:string xsd:minLength \"5\"^^xsd:integer)",
            "\"ab\"",
            "\"cd\"",
        ),
        (
            "DatatypeRestriction(xsd:string xsd:maxLength \"1\"^^xsd:integer)",
            "\"ab\"",
            "\"cd\"",
        ),
        (
            "DatatypeRestriction(xsd:string xsd:pattern \"[a-z]+\")",
            "\"A1\"",
            "\"B2\"",
        ),
    ] {
        assert!(consistent(&limit(range, v1, v2)), "≤1 p.{range}");
    }
}

/// The same through an exact cardinality, a limit inside an intersection, and
/// values asserted on a sub-property.
#[test]
fn every_route_to_a_narrower_limit_is_guarded() {
    assert!(consistent(
        "SubClassOf(:C DataExactCardinality(1 :p xsd:token)) ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a \"x  y\") DataPropertyAssertion(:p :a \"z  w\")"
    ));
    assert!(consistent(
        "EquivalentClasses(:C ObjectIntersectionOf(:A DataMaxCardinality(1 :p xsd:token)))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a \"x  y\") DataPropertyAssertion(:p :a \"z  w\")"
    ));
    assert!(consistent(
        "SubClassOf(:C DataMaxCardinality(1 :p xsd:token)) SubDataPropertyOf(:q :p)\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:q :a \"x  y\") DataPropertyAssertion(:q :a \"z  w\")"
    ));
}

/// Controls: a limit holding every string counts both values.
#[test]
fn a_limit_holding_every_string_counts_them() {
    for range in ["xsd:string", "rdfs:Literal"] {
        assert!(
            !consistent(&limit(range, "\"ab\"", "\"cd\"")),
            "≤1 p.{range}"
        );
    }
    assert!(!consistent(&limit(
        "xsd:string",
        "\"ab\"^^xsd:string",
        "\"cd\"^^xsd:string"
    )));
    assert!(!consistent(
        "SubClassOf(:C DataMaxCardinality(1 :p)) ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a \"ab\") DataPropertyAssertion(:p :a \"cd\")"
    ));
    assert!(!consistent(
        "SubClassOf(:C DataMaxCardinality(1 :p xsd:string)) SubDataPropertyOf(:q :p)\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:q :a \"ab\") DataPropertyAssertion(:q :a \"cd\")"
    ));
}

/// NEW behaviour (`main` said consistent): every string is an `rdf:PlainLiteral`,
/// so two plain strings break `≤1 p.rdf:PlainLiteral`. `HermiT` and Konclude agree.
#[test]
fn a_plain_literal_limit_counts_plain_strings() {
    assert!(!consistent(&limit("rdf:PlainLiteral", "\"ab\"", "\"cd\"")));
}

/// KNOWN MISSES, sound: values that DO lie in a narrower range are not counted
/// against it, since DP-2 has no lexical check for those types. `HermiT` says
/// inconsistent. FLIP when a membership check exists. Also: typed `xsd:token`
/// literals are not read as strings, so they are not counted against an
/// `xsd:string` limit either.
#[test]
fn values_inside_a_narrower_limit_are_a_known_miss() {
    assert!(consistent(&limit("xsd:token", "\"ab\"", "\"cd\"")));
    assert!(consistent(&limit(
        "DatatypeRestriction(xsd:string xsd:minLength \"5\"^^xsd:integer)",
        "\"abcdef\"",
        "\"ghijkl\""
    )));
    assert!(consistent(&limit(
        "xsd:string",
        "\"ab\"^^xsd:token",
        "\"cd\"^^xsd:token"
    )));
}

/// An enumeration limit inside an intersection. DP-2b reads only direct
/// `SubClassOf` limits, so this was a miss; since #344 the main tableau verifies
/// the consistency answer of an ontology that counts data values, and catches it.
/// `HermiT` and Konclude say inconsistent. The direct form is counted by DP-2b.
#[test]
fn an_enumeration_limit_inside_an_intersection_is_counted() {
    assert!(!consistent(&limit(
        "DataOneOf(\"a\" \"b\")",
        "\"a\"",
        "\"b\""
    )));
    assert!(!consistent(
        "EquivalentClasses(:C ObjectIntersectionOf(:A DataMaxCardinality(1 :p DataOneOf(\"a\" \"b\"))))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a \"a\") DataPropertyAssertion(:p :a \"b\")"
    ));
}

/// FP guards for the `rdf:PlainLiteral` row: one value written three ways is one
/// value, and a language-tagged `rdf:PlainLiteral` is not read as an `xsd:string`.
#[test]
fn plain_literal_spellings_are_one_value_and_tags_are_not_strings() {
    assert!(consistent(
        "SubClassOf(:C DataMaxCardinality(1 :p rdf:PlainLiteral)) ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a \"ab\") DataPropertyAssertion(:p :a \"ab\"^^xsd:string)\n\
         DataPropertyAssertion(:p :a \"ab@\"^^rdf:PlainLiteral)"
    ));
    assert!(consistent(&limit(
        "xsd:string",
        "\"ab\"",
        "\"cd@en\"^^rdf:PlainLiteral"
    )));
}

/// KNOWN MISS, sound: language-tagged values are not read as strings, so they are
/// not counted against an `rdf:PlainLiteral` limit, which they do fall under.
/// `HermiT` says inconsistent. FLIP when DP-2 counts tagged values.
#[test]
fn tagged_values_against_a_plain_literal_limit_are_a_known_miss() {
    assert!(consistent(&limit(
        "rdf:PlainLiteral",
        "\"ab\"",
        "\"cd\"@en"
    )));
}
