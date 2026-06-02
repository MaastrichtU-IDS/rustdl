//! Structural canary for the per-class label heuristic.
//!
//! Constructs a tiny synthetic ontology where many `C ⊑ D` queries
//! must succeed via the wedge closure AND many `C ⊑ D'` non-queries
//! must be pruned by `D' ∉ labels(C)`. Asserts that
//! `ClassificationStats::label_cache_pruned > 0`.
//!
//! Failure mode: the heuristic isn't firing (cache wiring broken,
//! or wedge labels are missing the disjoint-class atoms).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

#[test]
fn label_heuristic_prunes_disjoint_pairs() {
    // 6 classes in two disjoint A/B chains, plus an inverse-role
    // declaration to push the ontology *out* of pure-EL so the
    // top-down classifier (which owns the label cache) runs
    // instead of the saturation-only fast path. Many pair queries
    // (Ai ⊑ Bj) will have D ∉ labels(C) and should be pruned.
    let src = "\
Prefix(:=<http://test/lh/>)
Ontology(<http://test/lh>
    Declaration(Class(:A1))
    Declaration(Class(:A2))
    Declaration(Class(:A3))
    Declaration(Class(:B1))
    Declaration(Class(:B2))
    Declaration(Class(:B3))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:r_inv))
    InverseObjectProperties(:r :r_inv)
    SubClassOf(:A2 :A1)
    SubClassOf(:A3 :A2)
    SubClassOf(:B2 :B1)
    SubClassOf(:B3 :B2)
    DisjointClasses(:A1 :B1)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result = classify_top_down_with_timeout(&onto, Duration::from_millis(200))
        .expect("classify");
    let stats = result.stats();

    // Sanity checks: classification correctness.
    assert!(
        result.is_subclass("http://test/lh/A3", "http://test/lh/A1"),
        "A3 ⊑ A1 via chain"
    );
    assert!(
        !result.is_subclass("http://test/lh/A3", "http://test/lh/B1"),
        "A3 ⋢ B1 (disjoint)"
    );

    // Label heuristic must have fired at least once. The disjoint
    // class atoms ensure pruning is exercised: querying Ai ⊑ Bj
    // sees D=Bj absent from labels(Ai) and prunes.
    assert!(
        stats.label_cache_pruned > 0,
        "Phase 7 label heuristic must prune at least one pair on this synthetic. \
         Got pruned={} pass_through={} misses={}",
        stats.label_cache_pruned,
        stats.label_cache_pass_through,
        stats.label_cache_misses,
    );
}
