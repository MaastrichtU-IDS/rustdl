#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn probe(path: &str) -> Option<bool> {
    let s = std::fs::read_to_string(path).ok()?;
    let o: SetOntology<RcStr> = read_ofn(
        &mut Cursor::new(s.into_bytes()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0;
    Some(owl_dl_reasoner::abox_sat_inconsistent(&o).unwrap())
}

#[test]
#[ignore = "FP guard; needs fixtures"]
fn no_fp_on_consistent_fixtures() {
    // All of these are CONSISTENT per the oracle. The saturator must return false.
    for (name, path) in [
        ("wine", "../../ontologies/real/wine.ofn"),
        ("pizza", "../../ontologies/real/pizza.ofn"),
        ("sio", "../../ontologies/real/sio.ofn"),
        ("ro", "../../ontologies/real/ro.ofn"),
        ("sulo", "../../ontologies/real/sulo.ofn"),
        ("bibtex", "../../ontologies/real/bibtex.ofn"),
    ] {
        match probe(path) {
            Some(inc) => {
                println!("FP {name}: abox_sat_inconsistent={inc}");
                assert!(
                    !inc,
                    "FALSE POSITIVE: {name} is consistent but saturator flagged inconsistent"
                );
            }
            None => println!("FP {name}: (missing)"),
        }
    }
}
