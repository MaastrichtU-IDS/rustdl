//! The larger single-query node cap (#363 A2) applies only inside
//! `single_query_node_cap()`, which the CLI and Python `sat`/`subclass`/
//! `explain`/`instance` entry points hold. Library callers (and the multi-probe
//! `justify`/`repair`/`diagnose` built on them) keep the classify-wide 50000.
//!
//! Set `RUSTDL_QUERY_MAX_NODES=1` with `RUSTDL_MAX_NODES` unset: a bare library
//! call must still decide (50000 is plenty for this fixture), and the same call
//! inside the guard must give up. The caps are cached once per process, so this
//! is the only test in its binary.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Out of the EL fragment (the `∀s.E` axiom), and every probe on C needs an
// `:r`-successor, so a cap of 1 trips before a verdict.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/scope>
  Declaration(Class(:C)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
)
";

#[test]
#[allow(unsafe_code)]
fn only_the_guard_raises_the_cap() {
    // SAFETY: set_var/remove_var are unsafe under edition 2024; this is the only
    // test in the binary and runs before anything reads the cached caps.
    unsafe {
        std::env::remove_var("RUSTDL_MAX_NODES");
        std::env::set_var("RUSTDL_QUERY_MAX_NODES", "1");
    }
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(SRC), ParserConfiguration::default()).unwrap();
    let c = "http://ex/#C";

    let (sat, stats) = owl_dl_reasoner::is_class_satisfiable_with_stats(&onto, c).unwrap();
    assert!(
        sat && !stats.incomplete,
        "a library call keeps the 50000 cap and decides"
    );

    let _cap = owl_dl_reasoner::single_query_node_cap();
    let (sat, stats) = owl_dl_reasoner::is_class_satisfiable_with_stats(&onto, c).unwrap();
    assert!(
        sat && stats.incomplete,
        "inside the guard the query cap (1) applies and the probe gives up"
    );
}
