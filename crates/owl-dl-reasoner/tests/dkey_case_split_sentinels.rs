//! #589 Step 2 sentinels: reasoning by cases over data keys, deferred.
//!
//! Each test asserts `HermiT`'s answer on one shape that needs the value-level split
//! (or, for an empty range, its emptiness), which no engine makes: data ranges are
//! atomic keys. Step 1 (#639) flags every one of them (`data_case_split_risk`, so no
//! completeness is claimed); the answers stay wrong. Step 2 is deferred on ORE evidence:
//! 132 of the 133 flagged ORE files with an oracle closure miss nothing, and the one that
//! does (`ore_ont_15682`) is a dropped `DataIntersectionOf`, not a case split (see
//! `docs/known-limitations/dkey-enumeration-case-split-and-signed-zero.md`).
//!
//! `#[ignore]`d: they fail today by design. One that starts passing is a finding: run
//! `cargo test -p owl-dl-reasoner --test dkey_case_split_sentinels -- --ignored` and
//! attribute which change made it pass. Fixtures are the #589 probes, each re-adjudicated
//! with `HermiT` for this file (`~/rustdl/sweeps/bugfix/589/step2/oracle/`).
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

const C: &str = "http://ex.org/t#C";
const E: &str = "http://ex.org/t#E";

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    owl_dl_reasoner::classify(&o).unwrap()
}

/// `HermiT`: `C ⊑ E`.
fn entails_c_sub_e(src: &str) {
    let c = classify(src);
    assert!(c.is_subclass(C, E), "C ⊑ E missed");
}

/// `HermiT`: `C` is unsatisfiable.
fn c_is_unsat(src: &str) {
    let c = classify(src);
    assert!(c.unsatisfiable_classes().contains(&C), "C read satisfiable");
}

/// `∃d.{1,2}` against `hasValue(d,1)` and `hasValue(d,2)` (the issue). `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn s_resid() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#,
    );
}

/// the decimal form of the issue. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn s_resid_dec() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1.5"^^xsd:decimal "2.5"^^xsd:decimal)))
SubClassOf(DataHasValue(:d "1.5"^^xsd:decimal) :E)
SubClassOf(DataHasValue(:d "2.5"^^xsd:decimal) :E)
)"#,
    );
}

/// `int[1,2]` against its two points. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn s_resid_range() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#,
    );
}

/// `xsd:boolean` against `true` and `false`. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_bool() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d xsd:boolean))
SubClassOf(DataHasValue(:d "true"^^xsd:boolean) :E)
SubClassOf(DataHasValue(:d "false"^^xsd:boolean) :E)
)"#,
    );
}

/// a string enumeration against its points. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_str() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("a" "b")))
SubClassOf(DataHasValue(:d "a") :E)
SubClassOf(DataHasValue(:d "b") :E)
)"#,
    );
}

/// `int[1,10]` = `[1,5]` ∪ `[6,10]`. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_r10() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "6"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)) :E)
)"#,
    );
}

/// `int[1,10]` = `[2,10]` ∪ the point 1. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_mix() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "2"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)) :E)
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
)"#,
    );
}

/// integer `[1,2]` covered by two decimal ranges. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_intdec() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "0.5"^^xsd:decimal xsd:maxInclusive "1.5"^^xsd:decimal)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "1.5"^^xsd:decimal xsd:maxInclusive "2.5"^^xsd:decimal)) :E)
)"#,
    );
}

/// decimal `[1,3]` covered by two overlapping decimal ranges. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_decov() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "1.0"^^xsd:decimal xsd:maxInclusive "3.0"^^xsd:decimal)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "0.5"^^xsd:decimal xsd:maxInclusive "2.0"^^xsd:decimal)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minExclusive "1.5"^^xsd:decimal xsd:maxInclusive "3.5"^^xsd:decimal)) :E)
)"#,
    );
}

/// `xsd:integer` = `≤0` ∪ `≥1`. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_unb() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d xsd:integer))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:maxInclusive "0"^^xsd:integer)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer)) :E)
)"#,
    );
}

/// `int[1,2]` with the point 1 negated (an EL ⊥-axiom). `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn q_neg2() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(:C ObjectComplementOf(DataHasValue(:d "1"^^xsd:integer)))
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#,
    );
}

/// a boolean range under an untyped filler. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn r_range_bool() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
DataPropertyRange(:d xsd:boolean)
SubClassOf(:C DataSomeValuesFrom(:d rdfs:Literal))
SubClassOf(DataHasValue(:d "true"^^xsd:boolean) :E)
SubClassOf(DataHasValue(:d "false"^^xsd:boolean) :E)
)"#,
    );
}

/// `Range(d,[5,10])` narrows `∃d.[1,5]` to the point 5. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn r_range_inter() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive "5"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
SubClassOf(DataHasValue(:d "5"^^xsd:integer) :E)
)"#,
    );
}

/// `Range(d,[9,20])` narrows `∃d.[1,10]` to the points 9 and 10. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn r_range_split() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive "9"^^xsd:integer xsd:maxInclusive "20"^^xsd:integer))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
SubClassOf(DataHasValue(:d "9"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "10"^^xsd:integer) :E)
)"#,
    );
}

/// `Range(d, nonNegativeInteger)` narrows `∃d.(≤1)` to the points 0 and 1. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn t_nni() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
DataPropertyRange(:d xsd:nonNegativeInteger)
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:maxInclusive "1"^^xsd:integer)))
SubClassOf(DataHasValue(:d "0"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
)"#,
    );
}

/// two adjacent float points cover their closed range. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn t_float_adj() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:float xsd:minInclusive "1.0"^^xsd:float xsd:maxInclusive "1.0000001"^^xsd:float)))
SubClassOf(DataHasValue(:d "1.0"^^xsd:float) :E)
SubClassOf(DataHasValue(:d "1.0000001"^^xsd:float) :E)
)"#,
    );
}

/// the issue's range form beside an unrelated superset key. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn t_superkey() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
SubClassOf(:F DataSomeValuesFrom(:e DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
)"#,
    );
}

/// a dateTime enumeration against its points. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn dt_cover() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("2020-01-01T00:00:00"^^xsd:dateTime "2021-01-01T00:00:00"^^xsd:dateTime)))
SubClassOf(DataHasValue(:d "2020-01-01T00:00:00"^^xsd:dateTime) :E)
SubClassOf(DataHasValue(:d "2021-01-01T00:00:00"^^xsd:dateTime) :E)
)"#,
    );
}

/// an anyURI enumeration against its points. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn uri_cover() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("http://a"^^xsd:anyURI "http://b"^^xsd:anyURI)))
SubClassOf(DataHasValue(:d "http://a"^^xsd:anyURI) :E)
SubClassOf(DataHasValue(:d "http://b"^^xsd:anyURI) :E)
)"#,
    );
}

/// a language-tagged enumeration against its points. `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn lang_cover() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("a"@en "b"@en)))
SubClassOf(DataHasValue(:d "a"@en) :E)
SubClassOf(DataHasValue(:d "b"@en) :E)
)"#,
    );
}

/// a plain string against bare `rdf:langString` (the oracle reading, not RDF 1.1). `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn lg_top2() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataHasValue(:d "a"))
SubClassOf(DataSomeValuesFrom(:d rdf:langString) :E)
)"#,
    );
}

/// `{1e20, 2e20}` (whole decimals past `i64`) against `nonNegativeInteger` (`pBa`). `HermiT`: `C ⊑ E`.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn whole_decimals_past_i64() {
    entails_c_sub_e(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e))
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:M)) SubClassOf(:C :M)
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("100000000000000000000.0"^^xsd:decimal "200000000000000000000.0"^^xsd:decimal)))
SubClassOf(DataSomeValuesFrom(:d xsd:nonNegativeInteger) :E)
)"#,
    );
}

/// float bounds with no `f32` between them: an empty range. `HermiT`: `C` unsatisfiable.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn em_float() {
    c_is_unsat(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:float xsd:minExclusive "1.0"^^xsd:float xsd:maxExclusive "1.0000001"^^xsd:float)))
)"#,
    );
}

/// `decimal(1.0, 1.0]`: an empty range. `HermiT`: `C` unsatisfiable.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn dec_empty() {
    c_is_unsat(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minExclusive "1.0"^^xsd:decimal xsd:maxInclusive "1.0"^^xsd:decimal)))
)"#,
    );
}

/// double `[2.0, 1.0]`: an empty range. `HermiT`: `C` unsatisfiable.
#[test]
#[ignore = "#589 Step 2 deferred: needs the data-key case split"]
fn dbl_empty() {
    c_is_unsat(
        r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:double xsd:minInclusive "2.0"^^xsd:double xsd:maxInclusive "1.0"^^xsd:double)))
)"#,
    );
}
