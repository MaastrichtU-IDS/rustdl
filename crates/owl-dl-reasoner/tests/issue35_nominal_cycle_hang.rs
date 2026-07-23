//! Regression test for the issue-#35 **nominal-anchored ∃-cycle** hang (the
//! `hang_v3` core, reported against 0.3.34).
//!
//! Distinct from both prior #35 fixes:
//!   - the 0.3.31 realize saturation fast-path (keeps realize off the tableau
//!     on the EL/Horn fragment), and
//!   - the 0.3.34 deep-cap fix (removes the 256-deep recursion limit on the
//!     deadline-free tableau paths so a bounded-graph ⊔-search can complete).
//!
//! Here the completion graph itself grows near-unbounded: a `{a} ⊓ ¬C`
//! instance probe over `Person ⊑ ∃hasMother.Woman` + `Woman ⊑ Person` builds an
//! infinite maternal ∃-cycle **anchored at a nominal root** (introduced by the
//! `ABox` edge `isMotherOf(a,b)`). Ancestor-scoped pair-blocking cannot block that
//! chain — the pairwise parent-subset condition never holds near the nominal
//! anchor — so each probe explores a huge graph (~10 s on the tiny core here;
//! realize/`materialize_inferred_class_assertions`, one probe per
//! individual×class, aggregates that into a multi-minute "hang" on the source
//! KG; ancestor-only realize on this core takes ~70 s).
//!
//! The fix enables **anywhere-blocking** (Motik/Shearer/Horrocks) on the
//! deadline-free query paths, which blocks the maternal chain against any
//! earlier non-nominal node and bounds the graph. With it, every probe is
//! sub-second and the verdicts match `HermiT`: the ontology is consistent, and
//! neither `a` nor `b` is an instance of any defined class (both are just
//! `owl:Thing`).
//!
//! `RUSTDL_ANYWHERE_BLOCKING=0` forces the pre-fix ancestor-only behaviour
//! (this test would then be very slow but still eventually assert the same
//! verdicts) — so this guards correctness; the fix guards the speed.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{is_class_satisfiable, is_consistent, is_instance_of, realize};
use std::fs;
use std::io::Cursor;
use std::path::Path;

const NS: &str = "http://example.org/hang3#";

fn load() -> SetOntology<RcStr> {
    let path = Path::new("tests/fixtures/regression/issue35_nominal_cycle_hang.ofn");
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read fixture: {e}"));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

#[test]
fn issue35_v3_is_consistent() {
    assert!(
        is_consistent(&load()).expect("is_consistent must return, not hang"),
        "hang3 core is consistent (HermiT agrees)"
    );
}

#[test]
fn issue35_v3_defined_classes_satisfiable() {
    let onto = load();
    for cls in ["Person", "Man", "Woman", "Male", "Female"] {
        let iri = format!("{NS}{cls}");
        assert!(
            is_class_satisfiable(&onto, &iri).expect("must return, not hang"),
            "{cls} is satisfiable"
        );
    }
}

#[test]
fn issue35_v3_individuals_have_no_defined_type() {
    // HermiT: a, b are just owl:Thing — instances of none of the defined
    // classes. This is the `{x} ⊓ ¬C` probe path that grew unbounded pre-fix.
    let onto = load();
    for ind in ["a", "b"] {
        for cls in ["Person", "Man", "Woman", "Male", "Female"] {
            let cls_iri = format!("{NS}{cls}");
            let ind_iri = format!("{NS}{ind}");
            assert!(
                !is_instance_of(&onto, &cls_iri, &ind_iri).expect("must return, not hang"),
                "{ind} must NOT be an instance of {cls}"
            );
        }
    }
}

#[test]
fn issue35_v3_realize_terminates() {
    // realize runs a probe per (individual, class); pre-fix this aggregated
    // into a multi-second-to-minutes stall. It must return with a, b carrying
    // no non-trivial named type.
    let realization = realize(&load()).expect("realize must return, not hang");
    for ind in realization.individuals() {
        let named: Vec<_> = realization
            .entailed_types(ind)
            .iter()
            .filter(|t| t.contains("hang3#"))
            .collect();
        assert!(
            named.is_empty(),
            "individual {ind} should have no defined-class type, got {named:?}"
        );
    }
}
