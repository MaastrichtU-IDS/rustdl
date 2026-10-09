//! #218: classify's per-class unsat probe, when CUT by its deadline (or bailing
//! with `NoVerdict`), keeps the class as satisfiable. That is sound, but the
//! class may be unsatisfiable, and nothing recorded the cut, so a genuinely
//! unsat class could be reported satisfiable with `incomplete: false`. Each cut
//! now adds a `(c, c)` undecided marker.
//!
//! The wedge is switched off so the probe is decided by the main tableau, and
//! a 1 ns per-pair budget makes the cut deterministic. In its own file because
//! it sets an env var, which sibling tests in one binary would observe.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

/// `A` is unsatisfiable (`HermiT`, Konclude), and only the tableau sees it
/// once the wedge is off: `∀r.D` must reach the second hop of a transitive `r`.
const OFN: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n\
Declaration(Class(:A)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
TransitiveObjectProperty(:r) SubClassOf(:A ObjectAllValuesFrom(:r :D))\n\
SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r ObjectComplementOf(:D))))\n)\n";

#[test]
#[allow(unsafe_code)]
fn a_cut_unsat_probe_flags_the_class_undecided() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has one test.
    unsafe { std::env::set_var("RUSTDL_HYPERTABLEAU", "0") };
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(OFN), ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify_with_timeout(&o, std::time::Duration::from_nanos(1))
        .expect("classify");
    let a = "http://ex.org/A";
    let undecided = c.undecided_pairs();
    assert_eq!(
        undecided.len(),
        c.stats().timed_out_pairs,
        "undecided/count invariant"
    );
    assert!(
        c.unsatisfiable_classes().contains(&a) || undecided.contains(&(a, a)),
        "A is unsat: it must be reported unsat, or its cut probe flagged undecided \
         (unsat={:?}, undecided={undecided:?})",
        c.unsatisfiable_classes()
    );
    assert!(!c.completeness_guaranteed());
}
