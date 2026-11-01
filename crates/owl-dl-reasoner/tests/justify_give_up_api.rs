//! #396: the library surface of the justify/repair give-up signal.
//!
//! `RUSTDL_MAX_NODES` is read once per process (`OnceLock`), so this binary
//! holds a SINGLE test, which sets the cap to 1 before any reasoning. The CLI
//! tests (`owl-dl-cli/tests/justify_give_up.rs`) cover the same fixtures with
//! uncapped controls.
//!
//! Run: `cargo test -p owl-dl-reasoner --test justify_give_up_api`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{
    Entailment, Verdict, find_one_justification, find_one_justification_reporting,
};
use std::io::Cursor;

fn parse(src: &str) -> SetOntology<RcStr> {
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

// See `owl-dl-cli/tests/justify_give_up.rs` for why each fixture gives up where
// it does.
const TOP_LEVEL: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/top>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
)
";

const BRANCH: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/branch>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C :X)
  SubClassOf(:X :D)
  SubClassOf(:C :D)
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:r :E))
)
";

const QX: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/qx>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:r :E))
  EquivalentClasses(:C :X)
  EquivalentClasses(:X :D)
)
";

fn c_sub_d() -> Entailment {
    Entailment::SubClassOf {
        sub: "http://ex/#C".into(),
        sup: "http://ex/#D".into(),
    }
}

#[test]
fn give_ups_are_reported_through_the_library_api() {
    // SAFETY: the only test in this binary, run before any reasoning reads the
    // cap; no other thread touches the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_MAX_NODES", "1");
    }
    let q = c_sub_d();

    // Top level: Unknown, not a non-entailment. The legacy form still maps it
    // to `None`.
    let top = parse(TOP_LEVEL);
    let out = find_one_justification_reporting(&top, &q).unwrap();
    assert_eq!(out.status, Verdict::Unknown);
    assert!(out.incomplete);
    assert!(out.justifications.is_empty());
    assert!(find_one_justification(&top, &q).unwrap().is_none());
    let rep = owl_dl_reasoner::find_repairs(&top, &q, 10).unwrap();
    assert!(
        !rep.entailed && rep.incomplete,
        "top-level unknown: {rep:?}"
    );
    assert!(
        !rep.complete,
        "not known complete after a top-level give-up"
    );

    // A QuickXplain give-up: still entailing, minimality no longer guaranteed.
    let out = find_one_justification_reporting(&parse(QX), &q).unwrap();
    assert_eq!(out.status, Verdict::Entailed);
    assert!(out.incomplete);
    assert_eq!(out.justifications.len(), 1);
    assert!(!out.justifications[0].minimal_guaranteed);

    // A branch give-up: the repair set is not known to be complete, and no
    // candidate whose verification gave up is reported.
    let rep = owl_dl_reasoner::find_repairs(&parse(BRANCH), &q, 10).unwrap();
    assert!(rep.entailed);
    assert!(rep.incomplete);
    assert!(!rep.complete, "complete must be cleared after a give-up");
    assert!(rep.repairs.is_empty(), "{rep:?}");
    assert!(rep.unverified_gave_up > 0, "{rep:?}");
}
