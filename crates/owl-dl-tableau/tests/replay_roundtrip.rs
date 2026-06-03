//! Round-trip test for snapshot capture + engine reconstruction.
//!
//! Phase 1b T1: a snapshot captured from a Sat verdict must, when
//! used to reconstruct a fresh engine, also return Sat when `decide`
//! runs again. (Replay-with-neg-sup is Task 2; this test exercises
//! only the seeded state.)

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
use std::io::Cursor;

#[test]
fn snapshot_seeded_engine_round_trips_to_sat() {
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;
    let a_id = internal
        .vocabulary
        .class_id("http://t/A")
        .expect("A exists");

    // Capture: run decide, snapshot.
    let mut eng = HyperEngine::new(&clauses, a_id);
    assert_eq!(eng.decide(64), HyperResult::Sat);
    let snap = eng.satisfiability_snapshot(a_id).expect("snapshot built");
    // Phase 1b.5 invariant: at capture time, pre_capture_labels == labels
    // for every node. They diverge only during replay.
    for i in 0..snap.node_count() {
        assert_eq!(
            snap.labels_at(i),
            snap.pre_capture_labels_at(i),
            "Phase 1b.5: pre_capture_labels must equal labels at capture (node {i})"
        );
    }
    let original_node_count = snap.node_count();
    let original_root_labels: Vec<_> = snap.root_labels().to_vec();

    // Reconstruct + verify: seeded engine returns Sat too.
    let mut eng2 = HyperEngine::from_snapshot(&clauses, &snap);
    assert_eq!(eng2.decide(64), HyperResult::Sat);

    // Idempotence: snapshot-of-seeded should equal the original up to ordering.
    let snap2 = eng2
        .satisfiability_snapshot(a_id)
        .expect("re-snapshot built");
    assert_eq!(snap2.node_count(), original_node_count);
    let mut sorted_a = original_root_labels.clone();
    sorted_a.sort();
    let mut sorted_b: Vec<_> = snap2.root_labels().to_vec();
    sorted_b.sort();
    assert_eq!(sorted_a, sorted_b);
}
