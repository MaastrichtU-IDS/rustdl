//! #322 — `rdf:PlainLiteral` was dropped as an unsupported data range, so
//! `Range(p, rdf:PlainLiteral)` with an integer value came back satisfiable.
//!
//! `rdf:PlainLiteral` is `xsd:string` ∪ `rdf:langString`. It lowers to one key
//! `P`: every string and langString key is `⊑ P`, and `P` is disjoint from the
//! numeric, dateTime, boolean and anyURI keys. Literals typed `rdf:PlainLiteral`
//! split at the last `@`: `"x@en"` is `"x"@en` and `"x@"` is the string `"x"`.
//!
//! Every verdict below matches `HermiT` 1.4.5 on the same ontology. No ORE
//! ontology uses `rdf:PlainLiteral`, so these tests are the safety net.

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

const C: &str = "http://example.org/C";
const D: &str = "http://example.org/D";
const RANGE: &str = "DataPropertyRange(:p rdf:PlainLiteral)";

fn onto(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://example.org/>)\n\
         Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://example.org/onto>\n\
         Declaration(DataProperty(:p))\n\
         Declaration(Class(:C))\n\
         Declaration(Class(:D))\n\
         {axioms}\n\
         )\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

fn classify(o: &SetOntology<RcStr>) -> owl_dl_reasoner::Classification {
    let dropped = dropped_axioms(o).unwrap();
    assert!(dropped.is_empty(), "axioms dropped: {dropped:?}");
    classify_top_down_with_timeout(o, Duration::from_secs(10)).expect("classify")
}

/// `C`'s satisfiability through both `classify` and `sat`, which must agree.
fn c_satisfiable(axioms: &str) -> bool {
    let o = onto(axioms);
    let classify_unsat = classify(&o).unsatisfiable_classes().contains(&C);
    let sat = is_class_satisfiable(&o, C).unwrap();
    assert_eq!(!classify_unsat, sat, "classify and sat disagree: {axioms}");
    sat
}

fn c_sub_d(axioms: &str) -> bool {
    classify(&onto(axioms)).is_subclass(C, D)
}

/// The issue: an integer is not a plain literal.
#[test]
fn a_plain_literal_range_excludes_integers() {
    assert!(!c_satisfiable(&format!(
        "{RANGE} SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))"
    )));
}

/// Controls: a plain string and a language-tagged string both fit.
#[test]
fn a_plain_literal_range_admits_strings_and_lang_strings() {
    for value in ["\"x\"", "\"x\"@en"] {
        assert!(
            c_satisfiable(&format!("{RANGE} SubClassOf(:C DataHasValue(:p {value}))")),
            "{value}"
        );
    }
}

/// Every other family is excluded.
#[test]
fn a_plain_literal_range_excludes_the_other_families() {
    for value in [
        "\"true\"^^xsd:boolean",
        "\"2020-01-01T00:00:00\"^^xsd:dateTime",
        "\"http://a\"^^xsd:anyURI",
    ] {
        assert!(
            !c_satisfiable(&format!("{RANGE} SubClassOf(:C DataHasValue(:p {value}))")),
            "{value}"
        );
    }
}

#[test]
fn forall_plain_literal_excludes_integers() {
    assert!(!c_satisfiable(
        "SubClassOf(:C DataAllValuesFrom(:p rdf:PlainLiteral)) \
         SubClassOf(:C DataHasValue(:p \"1\"^^xsd:integer))"
    ));
}

/// Strings and language-tagged strings are plain literals.
#[test]
fn strings_and_lang_strings_are_plain_literals() {
    for value in ["\"x\"", "\"x\"@en"] {
        assert!(
            c_sub_d(&format!(
                "SubClassOf(:C DataHasValue(:p {value})) \
                 EquivalentClasses(:D DataSomeValuesFrom(:p rdf:PlainLiteral))"
            )),
            "{value}"
        );
    }
}

/// FP guard: an integer is not a plain literal.
#[test]
fn an_integer_is_not_a_plain_literal() {
    assert!(!c_sub_d(
        "SubClassOf(:C DataHasValue(:p \"1\"^^xsd:integer)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p rdf:PlainLiteral))"
    ));
}

/// FP guard: a plain literal need not be a string (it may be language-tagged).
#[test]
fn a_plain_literal_need_not_be_a_string() {
    assert!(!c_sub_d(
        "SubClassOf(:C DataSomeValuesFrom(:p rdf:PlainLiteral)) \
         EquivalentClasses(:D DataSomeValuesFrom(:p xsd:string))"
    ));
}

/// `"x@en"^^rdf:PlainLiteral` is `"x"@en`: inside `{"x"@en}`, outside `{"y"@en}`.
#[test]
fn a_typed_plain_literal_with_a_tag_is_a_lang_string() {
    let axioms = |allowed: &str| {
        format!(
            "SubClassOf(:C DataHasValue(:p \"x@en\"^^rdf:PlainLiteral)) \
             SubClassOf(:C DataAllValuesFrom(:p DataOneOf({allowed})))"
        )
    };
    assert!(c_satisfiable(&axioms("\"x\"@en")));
    assert!(!c_satisfiable(&axioms("\"y\"@en")));
}

/// The text may itself contain `@`: the split is at the LAST one, so
/// `"a@b@en"^^rdf:PlainLiteral` is `"a@b"@en`.
#[test]
fn a_typed_plain_literal_splits_at_the_last_at_sign() {
    assert!(c_satisfiable(
        "SubClassOf(:C DataHasValue(:p \"a@b@en\"^^rdf:PlainLiteral)) \
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"a@b\"@en)))"
    ));
}

/// `"x@"^^rdf:PlainLiteral` (empty tag) is the string `"x"`.
#[test]
fn a_typed_plain_literal_with_an_empty_tag_is_a_string() {
    assert!(c_satisfiable(
        "SubClassOf(:C DataHasValue(:p \"x@\"^^rdf:PlainLiteral)) \
         SubClassOf(:C DataAllValuesFrom(:p DataOneOf(\"x\")))"
    ));
}

/// Its complement contains no string.
#[test]
fn the_complement_contains_no_string() {
    assert!(!c_satisfiable(
        "SubClassOf(:C DataSomeValuesFrom(:p DataComplementOf(rdf:PlainLiteral))) \
         SubClassOf(:C DataAllValuesFrom(:p xsd:string))"
    ));
}

/// An `ABox` assertion: an asserted `"x@"^^rdf:PlainLiteral` is the string `"x"`, so it
/// violates a `{"y"}` range and satisfies an `{"x"}` one.
#[test]
fn an_asserted_typed_plain_literal_is_checked_against_the_range() {
    let kb = |allowed: &str| {
        onto(&format!(
            "Declaration(NamedIndividual(:a)) \
             DataPropertyRange(:p DataOneOf({allowed})) \
             DataPropertyAssertion(:p :a \"x@\"^^rdf:PlainLiteral)"
        ))
    };
    assert!(!is_consistent(&kb("\"y\"")).unwrap());
    assert!(is_consistent(&kb("\"x\"")).unwrap());
}

/// A literal typed `rdf:PlainLiteral` with no `@` is ill-formed and drops,
/// visibly.
#[test]
fn a_typed_plain_literal_without_an_at_sign_drops_visibly() {
    let o = onto("SubClassOf(:C DataHasValue(:p \"x\"^^rdf:PlainLiteral))");
    assert!(!dropped_axioms(&o).unwrap().is_empty());
}

/// `HermiT` reads a bare `rdf:langString` range as admitting plain strings, and
/// #315 keeps that key out of the langString family, so a plain string typed
/// `rdf:PlainLiteral` stays satisfiable under it.
#[test]
fn a_bare_lang_string_range_admits_a_typed_plain_string() {
    assert!(c_satisfiable(
        "DataPropertyRange(:p rdf:langString) \
         SubClassOf(:C DataHasValue(:p \"x@\"^^rdf:PlainLiteral))"
    ));
}
