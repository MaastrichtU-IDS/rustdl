//! #591: an `xsd:integer` facet with an `xsd:decimal` literal was dropped, so
//! `{2} ⊑ xsd:integer[≥ "2.0"^^xsd:decimal]` was missed. The integer value space is
//! part of the decimal one, so the literal denotes a decimal and the facet bounds the
//! integers by it: a whole value is that integer, and a fractional one bounds by its
//! ceiling (min) or floor (max).
//!
//! Oracles: `HermiT` and Konclude 0.7 agree on every case
//! (`~/rustdl/sweeps/wB/591/oracle.out`), including that both accept fractional
//! decimal facets on `xsd:integer`. Negative controls come first; each is one value
//! away from a positive twin.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const EX: &str = "http://ex.org/t#";

/// `C ⊑ sub`, `∃d.integer[facet] ⊑ E`: is `C ⊑ E`?
fn entails(sub: &str, facet: &str) -> bool {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\n\
         Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))\n\
         SubClassOf(:C {sub})\n\
         SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer {facet})) :E)\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    owl_dl_reasoner::is_subclass_of(&o, &format!("{EX}C"), &format!("{EX}E")).unwrap()
}

fn has(v: &str) -> String {
    format!(r#"DataHasValue(:d "{v}"^^xsd:integer)"#)
}

#[test]
fn values_outside_the_decimal_bound_are_not_subsumed() {
    for (what, sub, facet) in [
        (
            "1 < 2.0",
            r#"DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer))"#.to_owned(),
            r#"xsd:minInclusive "2.0"^^xsd:decimal"#,
        ),
        (
            "3 is not < 3.0",
            has("3"),
            r#"xsd:maxExclusive "3.0"^^xsd:decimal"#,
        ),
        (
            "2 < 2.5",
            has("2"),
            r#"xsd:minInclusive "2.5"^^xsd:decimal"#,
        ),
        (
            "2 is not > 2.5",
            has("2"),
            r#"xsd:minExclusive "2.5"^^xsd:decimal"#,
        ),
        (
            "-2 > -2.5",
            has("-2"),
            r#"xsd:maxInclusive "-2.5"^^xsd:decimal"#,
        ),
        (
            "-2 is not < -2.0",
            has("-2"),
            r#"xsd:maxExclusive "-2.0"^^xsd:decimal"#,
        ),
    ] {
        assert!(
            !entails(&sub, facet),
            "{what}: not subsumed (HermiT, Konclude)"
        );
    }
}

#[test]
fn values_inside_the_decimal_bound_are_subsumed() {
    for (what, sub, facet) in [
        (
            "the issue: {2} ≥ 2.0",
            r#"DataSomeValuesFrom(:d DataOneOf("2"^^xsd:integer))"#.to_owned(),
            r#"xsd:minInclusive "2.0"^^xsd:decimal"#,
        ),
        ("5 ≥ 2.00", has("5"), r#"xsd:minInclusive "2.00"^^xsd:decimal"#),
        ("3 < 4.0", has("3"), r#"xsd:maxExclusive "4.0"^^xsd:decimal"#),
        (
            "[3, ∞) ⊆ [2.0, ∞)",
            r#"DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "3"^^xsd:integer))"#
                .to_owned(),
            r#"xsd:minInclusive "2.0"^^xsd:decimal"#,
        ),
        ("3 ≥ 2.5", has("3"), r#"xsd:minInclusive "2.5"^^xsd:decimal"#),
        ("3 > 2.5", has("3"), r#"xsd:minExclusive "2.5"^^xsd:decimal"#),
        ("-3 ≤ -2.5", has("-3"), r#"xsd:maxInclusive "-2.5"^^xsd:decimal"#),
        ("-3 < -2.0", has("-3"), r#"xsd:maxExclusive "-2.0"^^xsd:decimal"#),
    ] {
        assert!(entails(&sub, facet), "{what}: subsumed (HermiT, Konclude)");
    }
}
