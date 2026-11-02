//! #388: a cut check of two co-listed most-specific types keeps both and sets
//! `Realization::direct_types_unverified`, without touching `incomplete`. Its own
//! file because `RUSTDL_MAX_NODES` is cached once per process.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use std::io::Cursor;

#[test]
#[allow(unsafe_code)]
fn a_cut_check_keeps_both_and_reports_it() {
    // SAFETY: the only test in this process; set before any reasoning reads it.
    unsafe { std::env::set_var("RUSTDL_MAX_NODES", "1") };
    // `X ⊑ ∃r.D` makes the `X ⊓ ¬Y` check generate a successor, which trips a
    // node cap of 1. The asserted types need no probe, and the instance probes for
    // C and D are skipped by the pseudo-model prune (`RUSTDL_PSEUDO_MODEL`, default
    // ON), so nothing else is cut. With the prune off those probes trip the cap too.
    let src = "Prefix(:=<http://ex.org/>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C ObjectAllValuesFrom(:r :D))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:r :D))\n\
         ClassAssertion(:X :a)\nClassAssertion(:Y :a)\n)";
    let (o, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    let r = owl_dl_reasoner::realize(&o).unwrap();
    let mut direct = r.most_specific_types("http://ex.org/a").to_vec();
    direct.sort();
    assert_eq!(direct, ["http://ex.org/X", "http://ex.org/Y"]);
    assert!(r.direct_types_unverified(), "the X/Y check was cut");
    assert!(!r.incomplete(), "no instance probe was cut");
}
