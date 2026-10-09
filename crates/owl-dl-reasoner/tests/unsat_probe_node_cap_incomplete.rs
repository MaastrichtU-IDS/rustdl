//! #218, the UNBOUNDED probe arm: with no deadline at all (library
//! `classify()`, CLI `--pair-timeout-ms 0`, Python's complete mode), a
//! `RUSTDL_MAX_NODES` trip still leaves the unsat probe without a verdict, and
//! it used to fold silently into "satisfiable" with `incomplete: false`.
//!
//! In its own file because `RUSTDL_MAX_NODES` is read once per process and
//! `RUSTDL_HYPERTABLEAU` is read per call: both must be set before the first
//! classify, and no sibling test may observe them.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

/// `A` is unsatisfiable (`HermiT`, Konclude); with the wedge off only the main
/// tableau can show it, and a 2-node cap stops it first.
const OFN: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n\
Declaration(Class(:A)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
TransitiveObjectProperty(:r) SubClassOf(:A ObjectAllValuesFrom(:r :D))\n\
SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r ObjectComplementOf(:D))))\n)\n";

#[test]
#[allow(unsafe_code)]
fn a_node_capped_unbounded_probe_flags_the_class_undecided() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has one test.
    unsafe {
        std::env::set_var("RUSTDL_HYPERTABLEAU", "0");
        std::env::set_var("RUSTDL_MAX_NODES", "2");
    }
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(OFN), ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&o).expect("classify");
    let a = "http://ex.org/A";
    assert!(
        !c.unsatisfiable_classes().contains(&a),
        "precondition: the 2-node cap must stop the probe (else this test is vacuous)"
    );
    assert!(
        c.undecided_pairs().contains(&(a, a)),
        "{:?}",
        c.undecided_pairs()
    );
    assert_eq!(c.undecided_pairs().len(), c.stats().timed_out_pairs);
}
