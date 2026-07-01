//! Integration tests for `materialize_data_property_assertions`.

use horned_owl::model::{
    Build, DataPropertyAssertion, DeclareDataProperty, DeclareNamedIndividual, Individual, Literal,
    MutableOntology, SubDataPropertyOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::materialize_data_property_assertions;

type Rc = std::rc::Rc<str>;
const XSD_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";

fn dpa(b: &Build<Rc>, dp: &str, subj: &str, lexical: &str, dt: &str) -> DataPropertyAssertion<Rc> {
    DataPropertyAssertion {
        dp: b.data_property(dp),
        from: Individual::Named(b.named_individual(subj)),
        to: Literal::Datatype {
            literal: lexical.to_string(),
            datatype_iri: b.iri(dt),
        },
    }
}
fn subdp(b: &Build<Rc>, sub: &str, sup: &str) -> SubDataPropertyOf<Rc> {
    SubDataPropertyOf {
        sub: b.data_property(sub),
        sup: b.data_property(sup),
    }
}

#[test]
fn subproperty_data_assertions() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:hasMeasurement")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(subdp(&b, "urn:hasAge", "urn:hasMeasurement"));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    let t = |s: &str, p: &str, l: &str, d: &str, lang: &str| {
        (
            s.to_string(),
            p.to_string(),
            l.to_string(),
            d.to_string(),
            lang.to_string(),
        )
    };
    assert!(
        got.contains(&t("urn:a", "urn:hasMeasurement", "30", XSD_INT, "")),
        "got: {got:?}"
    );
    assert!(got.contains(&t("urn:a", "urn:hasAge", "30", XSD_INT, "")));
    assert!(!got.contains(&t("urn:a", "urn:hasMeasurement", "99", XSD_INT, "")));
}

use horned_owl::model::EquivalentDataProperties;
use owl_dl_reasoner::justify::{Entailment, entails};

#[test]
fn equivalent_data_properties() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:age")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(EquivalentDataProperties(vec![
        b.data_property("urn:hasAge"),
        b.data_property("urn:age"),
    ]));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    assert!(
        got.iter()
            .any(|(s, p, l, _, _)| s == "urn:a" && p == "urn:age" && l == "30"),
        "got: {got:?}"
    );
}

#[test]
fn same_individual_data_folding() {
    // SameIndividual(a,b), hasAge(a,30) ⇒ hasAge(b,30) (and sub-property closure
    // for both). HermiT confirms this (materialize_data_matches_hermit_oracle);
    // this is the docker-free unit guard.
    use horned_owl::model::SameIndividual;
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:hasMeasurement")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:b")));
    o.insert(subdp(&b, "urn:hasAge", "urn:hasMeasurement"));
    o.insert(SameIndividual(vec![
        Individual::Named(b.named_individual("urn:a")),
        Individual::Named(b.named_individual("urn:b")),
    ]));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    let t = |s: &str, p: &str| {
        got.iter()
            .any(|(gs, gp, l, _, _)| gs == s && gp == p && l == "30")
    };
    // Folded onto b via SameIndividual, through the sub-property closure too.
    assert!(
        t("urn:b", "urn:hasAge"),
        "b hasAge 30 (folded); got: {got:?}"
    );
    assert!(
        t("urn:b", "urn:hasMeasurement"),
        "b hasMeasurement 30 (folded + sub); got: {got:?}"
    );
    // The original subject still present.
    assert!(t("urn:a", "urn:hasAge"));
}

#[test]
fn language_tag_round_trips() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:label")));
    o.insert(DeclareDataProperty(b.data_property("urn:name")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(subdp(&b, "urn:label", "urn:name"));
    o.insert(DataPropertyAssertion {
        dp: b.data_property("urn:label"),
        from: Individual::Named(b.named_individual("urn:a")),
        to: Literal::Language {
            literal: "hi".to_string(),
            lang: "en".to_string(),
        },
    });

    let got = materialize_data_property_assertions(&o).expect("materialize");
    assert!(
        got.iter().any(|(s, p, l, d, lang)| s == "urn:a"
            && p == "urn:name"
            && l == "hi"
            && d == "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString"
            && lang == "en"),
        "got: {got:?}"
    );
}

#[test]
fn every_data_triple_is_entailed() {
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    o.insert(DeclareDataProperty(b.data_property("urn:hasAge")));
    o.insert(DeclareDataProperty(b.data_property("urn:hasMeasurement")));
    o.insert(DeclareNamedIndividual(b.named_individual("urn:a")));
    o.insert(subdp(&b, "urn:hasAge", "urn:hasMeasurement"));
    o.insert(dpa(&b, "urn:hasAge", "urn:a", "30", XSD_INT));

    let got = materialize_data_property_assertions(&o).expect("materialize");
    assert!(!got.is_empty());
    for (s, p, lex, dt, lang) in &got {
        if !lang.is_empty() {
            continue;
        }
        let q = Entailment::DataPropertyValue {
            source: s.clone(),
            prop: p.clone(),
            value_lexical: lex.clone(),
            value_datatype: dt.clone(),
        };
        assert!(
            entails(&o, &q).expect("entails"),
            "{s} {p} {lex} must be entailed"
        );
    }
}

#[test]
fn inconsistent_is_error() {
    use horned_owl::model::{ClassAssertion, ClassExpression as CE, DeclareClass, DisjointClasses};
    let b = Build::new_rc();
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareNamedIndividual(b.named_individual("urn:i")));
    o.insert(DisjointClasses(vec![
        CE::Class(b.class("urn:A")),
        CE::Class(b.class("urn:B")),
    ]));
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:A")),
        i: Individual::Named(b.named_individual("urn:i")),
    });
    o.insert(ClassAssertion {
        ce: CE::Class(b.class("urn:B")),
        i: Individual::Named(b.named_individual("urn:i")),
    });

    assert!(materialize_data_property_assertions(&o).is_err());
}
