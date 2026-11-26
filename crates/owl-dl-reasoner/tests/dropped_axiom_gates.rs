//! #579, at the API level, one surface at a time: the CLI's `property-values
//! --json` ORs the object and data halves, so a CLI test cannot tell which one reads
//! `dropped`. Each gate is shown to read `dropped` itself, not one drop kind: a
//! SWRL rule, an unresolved `Import`, `HasKey`, and a salvaged axiom (kept in part,
//! still counted).
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    different_individuals, inferred_object_property_values, property_structural_gaps,
    same_individuals,
};
use std::io::Cursor;

fn onto(head: &str, body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://e#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://e/o>\n{head}\n\
         Declaration(Class(:A)) Declaration(Class(:B))\n\
         Declaration(ObjectProperty(:r)) Declaration(DataProperty(:dp))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         ClassAssertion(:A :a) ClassAssertion(:B :b)\n{body}\n)"
    );
    read(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// `(tag, head, body)`: each drops exactly one axiom and is otherwise inside the
/// object gate's whitelist, with no seed edge (so no extension probe runs).
fn dropping() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "rule",
            "",
            "DLSafeRule(Body(ClassAtom(:A Variable(<urn:x>)) ClassAtom(:B Variable(<urn:y>))) \
             Head(ObjectPropertyAtom(:r Variable(<urn:x>) Variable(<urn:y>))))",
        ),
        ("import", "Import(<http://example.org/not-loaded>)", ""),
        ("haskey", "", "HasKey(:A (:r) ())"),
        (
            "salvaged",
            "",
            "SubClassOf(:A ObjectIntersectionOf(:B DataAllValuesFrom(:dp xsd:hexBinary)))",
        ),
    ]
}

#[test]
fn the_object_gate_reads_dropped() {
    for (tag, head, body) in dropping() {
        let o = onto(head, body);
        assert_eq!(
            owl_dl_reasoner::dropped_axioms(&o).unwrap().total(),
            1,
            "{tag}: precondition"
        );
        let v = inferred_object_property_values(&o, None).unwrap();
        assert!(v.incomplete(), "{tag}: object half claims completeness");
    }
    let v = inferred_object_property_values(&onto("", ""), None).unwrap();
    assert!(!v.incomplete(), "control");
}

#[test]
fn the_data_half_reports_dropped_as_a_gap() {
    for (tag, head, body) in dropping() {
        let gaps = property_structural_gaps(&onto(head, body)).unwrap();
        for (surface, list) in [
            ("data_values", gaps.data_values().to_vec()),
            ("hierarchy", gaps.hierarchy()),
            ("disjoint", gaps.disjoint()),
        ] {
            assert!(
                list.iter().any(|k| k.ends_with("-dropped-axioms")),
                "{tag}/{surface}: {list:?}"
            );
        }
    }
    let gaps = property_structural_gaps(&onto("", "")).unwrap();
    assert!(
        gaps.data_values().is_empty() && gaps.hierarchy().is_empty(),
        "control"
    );
}

/// `same_individuals`' seed is complete only for converted axioms, so a drop sets
/// `incomplete`. `different_individuals`' `incomplete` means only a cut (its doc), and
/// stays false: the CLI's `dropped` block carries the drop there. No individual is
/// declared, so no probe runs and only the drop can set `incomplete`.
#[test]
fn individuals_read_dropped_where_they_claim_completeness() {
    let doc = |head: &str| -> SetOntology<RcStr> {
        let src = format!(
            "Prefix(:=<http://e#>)\nOntology(<http://e/o>\n{head}\n\
             Declaration(Class(:A)) SubClassOf(:A :A)\n)"
        );
        read(&mut Cursor::new(src), ParserConfiguration::default())
            .unwrap()
            .0
    };
    let o = doc("Import(<http://example.org/not-loaded>)");
    assert!(same_individuals(&o, None).unwrap().incomplete());
    assert!(!different_individuals(&o, None).unwrap().incomplete());
    assert!(
        !same_individuals(&doc(""), None).unwrap().incomplete(),
        "control"
    );
}
