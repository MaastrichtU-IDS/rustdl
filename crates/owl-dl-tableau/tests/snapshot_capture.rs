//! Snapshot capture test: build a tiny Horn ontology, run `hyper.decide`
//! to Sat, then call `satisfiability_snapshot` and assert the captured
//! structure looks right.
//!
//! Phase 1a invariant: snapshot's root labels are a superset of the
//! seed (the seed was asserted at root) and the `snapshot.seed` field
//! matches.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::HyperEngine;
use std::io::Cursor;

#[test]
fn snapshot_captures_root_labels_on_horn_sat() {
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

    // Class IRI → ClassId via Vocabulary::class_id (Option).
    let a_id = internal
        .vocabulary
        .class_id("http://t/A")
        .expect("A exists");

    let mut eng = HyperEngine::new(&clauses, a_id);
    let result = eng.decide(64);
    assert_eq!(result, owl_dl_tableau::hyper::HyperResult::Sat);

    let snapshot = eng.satisfiability_snapshot(a_id).expect("snapshot built");
    assert_eq!(snapshot.seed(), a_id);
    assert!(snapshot.is_safe()); // pure Horn → ontology-wide Safe (placeholder, Phase 1b stamps real risk)
    assert!(snapshot.node_count() >= 1);
    // Root carries the seed plus its told-subsumer closure.
    let root_labels = snapshot.root_labels();
    assert!(root_labels.contains(&a_id), "root must carry seed");
}

#[test]
fn snapshot_seed_field_matches() {
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:X))
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;
    let x = internal
        .vocabulary
        .class_id("http://t/X")
        .expect("X exists");

    let mut eng = HyperEngine::new(&clauses, x);
    let _ = eng.decide(64);
    let snap = eng.satisfiability_snapshot(x).expect("sat");
    assert_eq!(snap.seed(), x);
}
