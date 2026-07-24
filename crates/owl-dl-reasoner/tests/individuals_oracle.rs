//! Integration tests for `same_individuals` / `different_individuals`
//! (issue #46).
#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

#[test]
fn same_from_functional_role() {
    // Functional(r); r(a,b); r(a,c) ⇒ b=c.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
            FunctionalObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b)
            ObjectPropertyAssertion(:r :a :c))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(
        s.groups()
            .iter()
            .any(|g| g.contains(&"http://ex/#b".to_string())
                && g.contains(&"http://ex/#c".to_string())),
        "expected {{b,c}} group, got {:?}",
        s.groups()
    );
}

#[test]
fn different_from_disjoint_types() {
    // A,B disjoint; a:A, b:B ⇒ a≠b.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :b))",
    );
    let d = owl_dl_reasoner::different_individuals(&o, None).unwrap();
    assert!(
        d.pairs()
            .iter()
            .any(|(x, y)| (x == "http://ex/#a" && y == "http://ex/#b")
                || (x == "http://ex/#b" && y == "http://ex/#a")),
        "expected a≠b, got {:?}",
        d.pairs()
    );
}

#[test]
fn same_individuals_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        owl_dl_reasoner::same_individuals(&o, None),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

#[test]
fn different_individuals_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        owl_dl_reasoner::different_individuals(&o, None),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

#[test]
fn same_individuals_told_seed_only_is_complete() {
    // Only one asserted SameIndividual pair, no other named individuals to
    // probe against ⇒ the seed alone resolves everything ⇒ incomplete=false.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            SameIndividual(:a :b))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(!s.incomplete(), "seed-only case must be complete");
    assert!(
        s.groups()
            .iter()
            .any(|g| g.contains(&"http://ex/#a".to_string())
                && g.contains(&"http://ex/#b".to_string())),
        "expected {{a,b}} group, got {:?}",
        s.groups()
    );
}

#[test]
fn same_individuals_probe_sets_incomplete() {
    // Two unrelated named individuals with nothing forcing them same or
    // different ⇒ a probe is consulted ⇒ incomplete=true (per this query's
    // conservative policy), and they must NOT be reported same.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(
        s.incomplete(),
        "an extension probe must have been consulted"
    );
    assert!(
        !s.groups()
            .iter()
            .any(|g| g.contains(&"http://ex/#a".to_string())),
        "unrelated individuals must not be merged, got {:?}",
        s.groups()
    );
}

#[test]
fn anonymous_individuals_are_skipped() {
    // A blank-node individual should never appear in same/different output.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A))
            ClassAssertion(:A _:anon))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(
        s.groups()
            .iter()
            .all(|g| g.iter().all(|i| !i.contains("_:")))
    );
    let d = owl_dl_reasoner::different_individuals(&o, None).unwrap();
    assert!(d.pairs().is_empty());
}
