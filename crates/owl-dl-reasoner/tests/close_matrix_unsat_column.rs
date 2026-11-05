//! #448 (Fable's PR review): the transitive closure of the classification matrix
//! panicked ("index out of bounds: the len is 0") when a satisfiable row held an
//! unsat column. The top-down driver's closure seed records a told unsat super of a
//! class whose own unsat probe was cut; with the wedge off, a node cap does that.
//!
//! In its own file because `RUSTDL_MAX_NODES` is read once per process and
//! `RUSTDL_HYPERTABLEAU` is read per call: both must be set before the first
//! classify, and no sibling test may observe them.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

/// `B` is unsatisfiable (∃r.C ⊓ ∀r.¬C) and `A ⊑ B` is told, so `A` is unsat too;
/// `A`'s chain of four `s`-successors makes its probe outgrow a 3-node cap.
const OFN: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
SubClassOf(:A :B)\n\
SubClassOf(:A ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:s \
ObjectSomeValuesFrom(:s :D)))))\n\
SubClassOf(:B ObjectIntersectionOf(ObjectSomeValuesFrom(:r :C) \
ObjectAllValuesFrom(:r ObjectComplementOf(:C))))\n)\n";

#[test]
#[allow(unsafe_code)]
fn an_unsat_column_in_a_satisfiable_row_does_not_break_the_closure() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has one test.
    unsafe {
        std::env::set_var("RUSTDL_HYPERTABLEAU", "0");
        std::env::set_var("RUSTDL_MAX_NODES", "3");
        std::env::set_var("RUSTDL_CLASSIFY_CLOSE_MATRIX", "1");
    }
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(OFN), ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&o).expect("classify");
    let (a, b) = ("http://ex.org/A", "http://ex.org/B");
    let unsat = c.unsatisfiable_classes();
    assert!(unsat.contains(&b), "B is unsat: {unsat:?}");
    assert!(
        !unsat.contains(&a),
        "precondition: the node cap must stop A's probe (else this test is vacuous)"
    );
    assert!(
        c.undecided_pairs().contains(&(a, a)),
        "{:?}",
        c.undecided_pairs()
    );
    // The told (sound) A ⊑ B survives the closure.
    assert!(c.is_subclass(a, b));
}
