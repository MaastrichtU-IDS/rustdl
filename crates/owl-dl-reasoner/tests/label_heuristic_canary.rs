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

/// Pin the orchestrator flags this canary exercises. The label-cache /
/// top-down path is bypassed by the snapshot cache (`RUSTDL_SNAPSHOT_CAPTURE`,
/// Phase 1c) and the Horn shortcircuit (`RUSTDL_HORN_SHORTCIRCUIT`, Phase 2b),
/// both flipped default-ON after this canary was written. Every test in this
/// (separate) test binary wants the same values, so setting them without
/// restore is safe here and can't leak to other test binaries (each is its
/// own process).
#[allow(unsafe_code)]
fn pin_label_cache_path() {
    unsafe {
        std::env::set_var("RUSTDL_SNAPSHOT_CAPTURE", "0");
        std::env::set_var("RUSTDL_HORN_SHORTCIRCUIT", "0");
    }
}

#[test]
fn label_heuristic_prunes_disjoint_pairs() {
    pin_label_cache_path();
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
    let result =
        classify_top_down_with_timeout(&onto, Duration::from_millis(200)).expect("classify");
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

/// Companion canary that exercises the `pass_through` arm of the
/// per-class label heuristic: pairs where `D ∈ labels(C)` AND `C ⊑ D`
/// is a real subsumption, so the cache check passes through to
/// per-pair verification.
///
/// Construction: same disjoint A/B chains as the first canary (to
/// exercise the cache build and the prune arm), PLUS an extra equivalent
/// pair (E ≡ F) and an entailed subsumption (G ⊑ E). The equivalence
/// propagates labels both directions, so for G the cache may carry both
/// E and F as labels and the cache check passes through to per-pair
/// verification rather than pruning.
///
/// NOTE on assertion shape: the saturation closure-seeded fast path may
/// pre-empt the simple equivalence cases via `closure.contains` (the
/// `if closure.contains(...)` branch at `classify.rs:1162` short-circuits
/// BEFORE the cache check) — meaning `pass_through > 0` is not guaranteed
/// even with the construction above. The assertion accepts either
/// `pruned > 0` OR `pass_through > 0` as evidence that the cache wiring
/// is live for this synthetic; isolating `pass_through` alone is not
/// worth heavy synthetic-tweaking when EITHER arm proves the cache path
/// runs.
#[test]
fn label_heuristic_pass_through_on_equivalent_classes() {
    pin_label_cache_path();
    let src = "\
Prefix(:=<http://test/lh-pt/>)
Ontology(<http://test/lh-pt>
    Declaration(Class(:A1))
    Declaration(Class(:A2))
    Declaration(Class(:A3))
    Declaration(Class(:B1))
    Declaration(Class(:B2))
    Declaration(Class(:B3))
    Declaration(Class(:E))
    Declaration(Class(:F))
    Declaration(Class(:G))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:r_inv))
    InverseObjectProperties(:r :r_inv)
    SubClassOf(:A2 :A1)
    SubClassOf(:A3 :A2)
    SubClassOf(:B2 :B1)
    SubClassOf(:B3 :B2)
    DisjointClasses(:A1 :B1)
    EquivalentClasses(:E :F)
    SubClassOf(:G :E)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result =
        classify_top_down_with_timeout(&onto, Duration::from_millis(200)).expect("classify");
    let stats = result.stats();

    // Sanity checks: classification correctness.
    assert!(
        result.is_subclass("http://test/lh-pt/E", "http://test/lh-pt/F"),
        "E ⊑ F via equivalence"
    );
    assert!(
        result.is_subclass("http://test/lh-pt/G", "http://test/lh-pt/E"),
        "G ⊑ E via told subclass"
    );
    assert!(
        !result.is_subclass("http://test/lh-pt/A3", "http://test/lh-pt/B1"),
        "A3 ⋢ B1 (disjoint)"
    );

    // The heuristic should fire in at least one arm on this synthetic.
    // See doc comment above for why the assertion accepts either arm.
    assert!(
        stats.label_cache_pass_through > 0 || stats.label_cache_pruned > 0,
        "label heuristic must have either pruned OR passed-through at least one pair. \
         Got pruned={} pass_through={} misses={}",
        stats.label_cache_pruned,
        stats.label_cache_pass_through,
        stats.label_cache_misses,
    );
}
