//! Unsatisfiable classes must not appear as SUBJECTS of taxonomy direct edges.
//!
//! An unsatisfiable class denotes `⊥`, so it is subsumed by everything and
//! `direct_subsumers` correctly returns every MINIMAL satisfiable class. As
//! taxonomy output that is a spurious parent set — the same argument
//! `build_classify_json` already records for keeping unsatisfiable classes out
//! of `equivalent_groups`. The equivalence half was excluded; the direct-edge
//! half was not.
//!
//! Scale of the discrepancy, measured on `ore_ont_12567` (232,084 classes,
//! 4,202 unsatisfiable): each unsatisfiable class emitted 180,328 direct edges,
//! so 4,202 × 180,328 = 757,738,256 rows — 99.97% of a 758-million-row, ~21 GB
//! output — against 256,785 rows for every satisfiable subject combined, and
//! 513,554 `SubClassOf` axioms in Konclude's taxonomy for the same file.
//!
//! NOTE ON EVIDENCE: the FP=0 closure-diff net CANNOT see this change.
//! `oracle_diff::aligned_closures` excludes `verdict.unsat ∪ rustdl_unsat` from
//! both sides before diffing, so unsat-subject edges never entered the
//! comparison. A green net is therefore not evidence here — these tests are.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("fixture parses")
        .0
}

/// `:Bad` is unsatisfiable (disjoint parents); `:Mid`/`:Top`/`:Other` are not.
const FIXTURE: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:Top))
    Declaration(Class(:Mid))
    Declaration(Class(:Other))
    Declaration(Class(:Bad))
    SubClassOf(:Mid :Top)
    SubClassOf(:Other :Top)
    DisjointClasses(:Mid :Other)
    SubClassOf(:Bad :Mid)
    SubClassOf(:Bad :Other)
)
";

#[test]
fn unsatisfiable_subject_has_no_taxonomy_direct_edges() {
    let h = owl_dl_reasoner::classify(&parse(FIXTURE)).expect("classify");
    let unsat = h.unsatisfiable_classes();
    assert!(
        unsat
            .iter()
            .any(|c| c.ends_with("#Bad") || c.ends_with("/Bad")),
        ":Bad must be unsatisfiable for this fixture to test anything; unsat={unsat:?}"
    );
    for c in unsat {
        assert!(
            h.taxonomy_direct_subsumers(c).is_empty(),
            "unsatisfiable {c} must contribute no taxonomy direct edges, got {:?}",
            h.taxonomy_direct_subsumers(c)
        );
    }
}

/// The mathematical answer is UNCHANGED — this is an output convention, not a
/// change to what the reasoner entails. Without this the fix could have been
/// implemented by breaking `direct_subsumers` itself.
#[test]
fn direct_subsumers_still_reports_the_mathematical_answer() {
    let h = owl_dl_reasoner::classify(&parse(FIXTURE)).expect("classify");
    let unsat = h.unsatisfiable_classes();
    let bad = unsat
        .iter()
        .find(|c| c.ends_with("#Bad") || c.ends_with("/Bad"))
        .expect("Bad is unsatisfiable");
    assert!(
        !h.direct_subsumers(bad).is_empty(),
        "`direct_subsumers` must keep returning the minimal satisfiable classes \
         for an unsatisfiable subject; only the TAXONOMY view filters them"
    );
}

/// Satisfiable subjects are untouched: `:Mid ⊑ :Top` must still be emitted.
#[test]
fn satisfiable_subjects_keep_their_direct_edges() {
    let h = owl_dl_reasoner::classify(&parse(FIXTURE)).expect("classify");
    let mid = h
        .classes()
        .iter()
        .find(|c| c.ends_with("#Mid") || c.ends_with("/Mid"))
        .expect(":Mid is declared")
        .clone();
    let directs = h.taxonomy_direct_subsumers(&mid);
    assert!(
        directs
            .iter()
            .any(|s| s.ends_with("#Top") || s.ends_with("/Top")),
        ":Mid must still report :Top as a direct subsumer, got {directs:?}"
    );
    assert_eq!(
        directs,
        h.direct_subsumers(&mid),
        "a satisfiable subject's taxonomy edges must equal its direct subsumers exactly"
    );
}

/// An ontology with NO unsatisfiable class must be completely unaffected — the
/// filter must not perturb the common case.
#[test]
fn consistent_ontology_is_unaffected() {
    let h = owl_dl_reasoner::classify(&parse(
        r"Prefix(:=<http://t/>)
Ontology(<http://t/x>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
",
    ))
    .expect("classify");
    assert!(
        h.unsatisfiable_classes().is_empty(),
        "control fixture must have no unsatisfiable class"
    );
    for c in h.classes() {
        assert_eq!(
            h.taxonomy_direct_subsumers(c),
            h.direct_subsumers(c),
            "with nothing unsatisfiable the two views must agree for {c}"
        );
    }
}
