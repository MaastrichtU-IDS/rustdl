//! #603: `data_axioms::domain_inference_premises`, the source-side replay `prove` uses to
//! cite the premises of `data_axioms`' derived `C ⊑ D` (Pattern 3: a data-property
//! domain; Pattern 4: the same through `SubDataPropertyOf`, or an unrestricted
//! `∃e.rdfs:Literal` / `≥1 e` super side). It records facts with the pass's own scanner,
//! so it can never cite premises for an inference the pass no longer makes: #632 (#626)
//! narrowed Pattern 4's super side, and its false positives `s1`–`s6` get no premises.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const EX: &str = "http://ex.org/t#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
         Ontology(<http://ex.org/t>\n\
         Declaration(DataProperty(:d)) Declaration(DataProperty(:e))\n\
         Declaration(Class(:C)) Declaration(Class(:E))\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// The premises' components, as `{:?}` strings (stable enough to search).
fn premises(body: &str) -> Option<Vec<String>> {
    let o = onto(body);
    owl_dl_core::data_axioms::domain_inference_premises(&o, &format!("{EX}C"), &format!("{EX}E"))
        .map(|cs| {
            cs.into_iter()
                .map(|ac| format!("{:?}", ac.component))
                .collect()
        })
}

fn kinds(ps: &[String]) -> Vec<&str> {
    let mut k: Vec<&str> = ps
        .iter()
        .map(|p| p.split(['(', '{', ' ']).next().unwrap())
        .collect();
    k.sort_unstable();
    k
}

#[test]
fn a_data_property_domain_gives_the_restriction_and_the_domain() {
    // OAEI's shape: `Manual ⊑ =1 title`, `domain(title) = Reference`.
    let ps = premises(
        "DataPropertyDomain(:d :E)
         SubClassOf(:C DataExactCardinality(1 :d))",
    )
    .expect("Pattern 3");
    assert_eq!(kinds(&ps), ["DataPropertyDomain", "SubClassOf"], "{ps:?}");
    // SWEET's shape: a value.
    let ps = premises(
        r#"DataPropertyDomain(:d :E)
           SubClassOf(:C DataHasValue(:d "0"^^xsd:integer))"#,
    )
    .expect("Pattern 3, hasValue");
    assert_eq!(ps.len(), 2, "{ps:?}");
}

#[test]
fn a_domain_on_the_super_property_cites_the_chain() {
    let ps = premises(
        "SubDataPropertyOf(:d :e)
         DataPropertyDomain(:e :E)
         SubClassOf(:C DataSomeValuesFrom(:d xsd:string))",
    )
    .expect("Pattern 4, domain on the super-property");
    assert_eq!(
        kinds(&ps),
        ["DataPropertyDomain", "SubClassOf", "SubDataPropertyOf"],
        "{ps:?}"
    );
}

#[test]
fn an_unrestricted_super_side_cites_the_chain() {
    let ps = premises(
        r#"SubDataPropertyOf(:d :e)
           SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
           SubClassOf(DataSomeValuesFrom(:e rdfs:Literal) :E)"#,
    )
    .expect("Pattern 4, ∃e.rdfs:Literal ⊑ E");
    assert_eq!(ps.len(), 3, "{ps:?}");
    assert!(ps.iter().any(|p| p.contains("SubDataPropertyOf")), "{ps:?}");
}

/// The manager's canary: #632's false positives (HermiT rejects each). The pass no
/// longer derives `C ⊑ E` from them, and the replay finds no premises either.
#[test]
fn the_narrowed_pattern_4_false_positives_get_no_premises() {
    for (what, body) in [
        (
            "7 against 1",
            r#"SubDataPropertyOf(:d :e)
               SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataHasValue(:e "1"^^xsd:integer) :E)"#,
        ),
        (
            "s1",
            r#"SubDataPropertyOf(:d :e)
               SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataMinCardinality(2 :e) :E)"#,
        ),
        (
            "s2",
            r#"SubDataPropertyOf(:d :e)
               SubClassOf(:C DataHasValue(:d "7"^^xsd:integer))
               SubClassOf(DataExactCardinality(1 :e) :E)"#,
        ),
        (
            "s3",
            "SubDataPropertyOf(:d :e)
             SubClassOf(:C DataSomeValuesFrom(:d rdfs:Literal))
             SubClassOf(DataExactCardinality(2 :e) :E)",
        ),
        (
            "s4",
            "SubDataPropertyOf(:d :e)
             SubClassOf(:C DataMinCardinality(1 :d))
             SubClassOf(DataMinCardinality(3 :e rdfs:Literal) :E)",
        ),
        (
            "s5",
            r#"SubClassOf(:C DataHasValue(:e "7"^^xsd:integer))
               SubClassOf(DataExactCardinality(1 :e) :E)"#,
        ),
        (
            "s6",
            "SubDataPropertyOf(:d :e)
             SubClassOf(:C DataExactCardinality(1 :d))
             SubClassOf(DataExactCardinality(1 :e) :E)",
        ),
    ] {
        assert!(premises(body).is_none(), "{what}: {:?}", premises(body));
    }
}

/// Only the components the pass reads: an unrelated axiom is never cited.
#[test]
fn unrelated_components_are_not_cited() {
    let o = onto(
        "DataPropertyDomain(:d :E)
         SubClassOf(:C DataSomeValuesFrom(:d xsd:integer))
         Declaration(Class(:Z)) SubClassOf(:Z :E)",
    );
    let ps = owl_dl_core::data_axioms::domain_inference_premises(
        &o,
        &format!("{EX}C"),
        &format!("{EX}E"),
    )
    .unwrap();
    assert_eq!(ps.len(), 2);
    assert!(
        ps.iter()
            .all(|ac| !matches!(&ac.component, Component::SubClassOf(s) if format!("{s:?}").contains("t#Z"))),
        "{ps:?}"
    );
}
