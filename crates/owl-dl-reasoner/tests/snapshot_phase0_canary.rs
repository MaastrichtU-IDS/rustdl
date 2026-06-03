//! Phase 0 canary for the Konclude snapshot cache project.
//!
//! Invariant: the snapshot-capture machinery exists in the
//! tableau crate AND is gated behind `RUSTDL_SNAPSHOT_CAPTURE`
//! (default OFF) on the reasoner side, so default classify
//! has zero behavior change while the project is in flight.
//!
//! This test extends through every phase — Phase 1b will add
//! flag-ON assertions; Phase 1c flips the default and asserts
//! the new path matches verdicts of the old path.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, snapshot_capture_enabled};
use std::io::Cursor;
use std::time::Duration;

#[test]
fn snapshot_capture_flag_defaults_off() {
    // Phase 0 invariant: env flag defaults OFF.
    // Test guard: SAFE_TO_UNSET — assumes RUSTDL_SNAPSHOT_CAPTURE
    // is not set in the test process env. If a developer manually
    // exports it for debugging, this test correctly fails to remind
    // them.
    assert!(
        std::env::var("RUSTDL_SNAPSHOT_CAPTURE").is_err(),
        "Phase 0 canary: RUSTDL_SNAPSHOT_CAPTURE must not be set in the test env"
    );
    assert!(!snapshot_capture_enabled(), "default must be OFF (Phase 0)");
}

#[test]
fn classify_unchanged_with_flag_off() {
    // Sanity: a tiny ontology classifies to the same result as
    // it did pre-project, regardless of the Phase 1a code merging.
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result =
        classify_top_down_with_timeout(&onto, Duration::from_millis(200)).expect("classify");
    assert!(result.is_subclass("http://t/A", "http://t/B"));
    assert!(!result.is_subclass("http://t/B", "http://t/A"));
}
