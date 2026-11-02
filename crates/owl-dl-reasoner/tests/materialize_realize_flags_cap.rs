//! #391: a cut instance probe under `materialize_existential_successors` is
//! reported. Its own file because `RUSTDL_MAX_NODES` is cached once per process;
//! the uncapped control is in `materialize_realize_flags.rs`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use std::io::Cursor;

/// #395's fixture: every class generates an `:r`-successor, so a probe needs a
/// node beyond the root and trips a cap of 1, and `∀s.E` keeps it off the
/// saturation fast path. `C ⊑ ∃r.C` is the told existential the rows join on.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/querygiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
";

#[test]
#[allow(unsafe_code)]
fn a_cut_probe_is_reported_and_the_rows_stay() {
    // SAFETY: the only test in this process; set before any reasoning reads them.
    // The prune would answer the D probe without building it, so turn it off.
    unsafe {
        std::env::set_var("RUSTDL_MAX_NODES", "1");
        std::env::set_var("RUSTDL_PSEUDO_MODEL", "0");
    }
    let (o, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = read_ofn(&mut Cursor::new(SRC), ParserConfiguration::default()).unwrap();
    let (rows, flags) = owl_dl_reasoner::materialize_existential_successors_reporting(&o).unwrap();
    assert!(flags.incomplete, "the D probe trips the cap: {flags:?}");
    assert!(!flags.witness_prune_active);
    assert_eq!(
        rows,
        [(
            "http://ex/#a".to_owned(),
            "http://ex/#r".to_owned(),
            "_:b0".to_owned(),
            "http://ex/#C".to_owned()
        )],
        "a : C is told, so its row survives the cut"
    );
}
