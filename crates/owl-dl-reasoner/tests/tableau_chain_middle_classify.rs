//! #213 on the second surface: `classify` with the wedge off routes to the
//! main tableau, and reported `A` satisfiable with `incomplete: false`.
//!
//! In its own file because it sets an env var: a test binary runs its tests
//! in parallel, and a sibling test would observe it.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

#[test]
#[allow(unsafe_code)]
fn classify_without_the_wedge_derives_forall_over_a_transitive_role() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has one test.
    unsafe { std::env::set_var("RUSTDL_HYPERTABLEAU", "0") };
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n\
Declaration(Class(:A)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
TransitiveObjectProperty(:r) SubClassOf(:A ObjectAllValuesFrom(:r :D))\n\
SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r ObjectComplementOf(:D))))\n)\n";
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&o).expect("classify");
    assert!(c.unsatisfiable_classes().contains(&"http://ex.org/A"));
}
