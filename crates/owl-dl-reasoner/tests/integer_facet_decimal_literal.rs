//! #591: an `xsd:integer` facet with an `xsd:decimal` literal was dropped, so
//! `{2} ⊑ xsd:integer[≥ "2.0"^^xsd:decimal]` was missed. The integer value space is
//! part of the decimal one, so the literal denotes a decimal and the facet bounds the
//! integers by it: a whole value is that integer, and a fractional one bounds by its
//! ceiling (min) or floor (max).
//!
//! Oracles: `HermiT` and Konclude 0.7 agree on the first 14 cases
//! (`~/rustdl/sweeps/wB/591/oracle.out`), including that both accept fractional
//! decimal facets on `xsd:integer`. The review cases (`maxExclusive 2.5`,
//! `maxInclusive -0.5`, crossing facets, mistyped facet literals) are adjudicated by
//! `HermiT` alone; the `i64::MIN` case follows from the semantics. The per-case
//! "(HermiT, Konclude)" messages name the oracles of the first 14. Negative controls
//! come first; each is one value away from a positive twin.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const EX: &str = "http://ex.org/t#";

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// `C ⊑ sub`, `∃d.integer[facet] ⊑ E`: is `C ⊑ E`?
fn entails(sub: &str, facet: &str) -> bool {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\n\
         Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))\n\
         SubClassOf(:C {sub})\n\
         SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer {facet})) :E)\n)\n"
    );
    let o = parse(&src);
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
        (
            "3 is not < 2.5",
            has("3"),
            r#"xsd:maxExclusive "2.5"^^xsd:decimal"#,
        ),
        (
            "0 > -0.5 (the sign only in the literal)",
            has("0"),
            r#"xsd:maxInclusive "-0.5"^^xsd:decimal"#,
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
        ("2 < 2.5", has("2"), r#"xsd:maxExclusive "2.5"^^xsd:decimal"#),
        ("-1 ≤ -0.5", has("-1"), r#"xsd:maxInclusive "-0.5"^^xsd:decimal"#),
        (
            "i64::MIN as a whole decimal",
            has("-9223372036854775808"),
            r#"xsd:maxInclusive "-9223372036854775808"^^xsd:decimal"#,
        ),
    ] {
        assert!(entails(&sub, facet), "{what}: subsumed (HermiT, Konclude)");
    }
}

/// #635 review: facets that cross describe no integer, so a class demanding such a
/// value is unsat (`HermiT`). `[≥ 2.5, ≤ 2.7]` is `[3, 2]` after rounding, and
/// `[≥ 3, ≤ 2]` is empty as written. Each read satisfiable, with nothing dropped.
#[test]
fn crossing_facets_make_the_class_unsat() {
    for (what, facets) in [
        (
            "[≥ 2.5, ≤ 2.7] (c3)",
            r#"xsd:minInclusive "2.5"^^xsd:decimal xsd:maxInclusive "2.7"^^xsd:decimal"#,
        ),
        (
            "[≥ 3, ≤ 2] (d1)",
            r#"xsd:minInclusive "3"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer"#,
        ),
    ] {
        let o = parse(&format!(
            "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
             Ontology(<http://ex.org/t>\n\
             Declaration(DataProperty(:d)) Declaration(Class(:C))\n\
             SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer {facets})))\n)\n"
        ));
        assert!(
            !owl_dl_reasoner::is_class_satisfiable(&o, &format!("{EX}C")).unwrap(),
            "{what}: C is unsat"
        );
        let c = owl_dl_reasoner::classify(&o).unwrap();
        assert!(
            c.unsatisfiable_classes()
                .contains(&format!("{EX}C").as_str()),
            "{what}: classify"
        );
    }
}

/// #635 review: a facet literal that is not of the integers' value space
/// (`"2"^^xsd:string`, `"2.0"^^xsd:double`) put the restriction outside OWL 2 DL, and
/// `HermiT` rejects it. Its lexical form was read as the integer 2, so `{2}` entered
/// the range and gave `C ⊑ E`. The range is now dropped, and visibly so.
#[test]
fn a_facet_literal_outside_the_integers_drops_the_range() {
    for (what, facet) in [
        ("a string facet (c8)", r#"xsd:minInclusive "2"^^xsd:string"#),
        (
            "a double facet (c7)",
            r#"xsd:minInclusive "2.0"^^xsd:double"#,
        ),
        (
            "a decimal beyond i64",
            r#"xsd:minInclusive "99999999999999999999.5"^^xsd:decimal"#,
        ),
    ] {
        assert!(!entails(&has("2"), facet), "{what}: no subsumption");
        let src = format!(
            "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
             Ontology(<http://ex.org/t>\n\
             Declaration(DataProperty(:d)) Declaration(Class(:E))\n\
             SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer {facet})) :E)\n)\n"
        );
        let dropped = owl_dl_reasoner::dropped_axioms(&parse(&src)).unwrap();
        assert!(!dropped.is_empty(), "{what}: recorded in dropped");
    }
}
