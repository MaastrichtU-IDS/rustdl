//! Unit tests for `BackPropRisk::classify_ontology`.
//!
//! First-cut ontology-wide risk classifier: any axiom in the
//! ontology that contains an inverse role, a nominal, or a
//! cardinality constraint forces the whole ontology to Unsafe.
//! Per-class refinement is Phase 1b territory.

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ontology::InternalOntology;
use owl_dl_tableau::snapshot::{BackPropRisk, UnsafeReason};

fn lower_ofn(src: &str) -> InternalOntology {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    convert_ontology(&onto).expect("convert_ontology")
}

#[test]
fn pure_horn_classifies_safe() {
    let onto = lower_ofn(
        "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))
)
",
    );
    assert_eq!(BackPropRisk::classify_ontology(&onto), BackPropRisk::Safe);
}

#[test]
fn inverse_role_classifies_unsafe() {
    let onto = lower_ofn(
        "\
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:r_inv))
    InverseObjectProperties(:r :r_inv)
    SubClassOf(:A ObjectSomeValuesFrom(:r owl:Thing))
)
",
    );
    assert_eq!(
        BackPropRisk::classify_ontology(&onto),
        BackPropRisk::Unsafe {
            reason: UnsafeReason::InverseRoleReachable
        },
    );
}

#[test]
fn cardinality_classifies_unsafe() {
    let onto = lower_ofn(
        "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:r))
    SubClassOf(:A ObjectMaxCardinality(2 :r :B))
)
",
    );
    assert_eq!(
        BackPropRisk::classify_ontology(&onto),
        BackPropRisk::Unsafe {
            reason: UnsafeReason::CardinalityReachable
        },
    );
}

#[test]
fn nominal_classifies_unsafe() {
    let onto = lower_ofn(
        "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(NamedIndividual(:a))
    SubClassOf(:A ObjectOneOf(:a))
)
",
    );
    assert_eq!(
        BackPropRisk::classify_ontology(&onto),
        BackPropRisk::Unsafe {
            reason: UnsafeReason::NominalReachable
        },
    );
}
