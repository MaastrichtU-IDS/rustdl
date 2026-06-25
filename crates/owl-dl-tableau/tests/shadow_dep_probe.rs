//! Integration test for the read-only shadow precise-dependency probe
//! (`RUSTDL_SHADOW_DEP_PROBE`).
//!
//! Fixture: C ⊑ ∃R.A ⊓ ∃R.B ⊓ ∃R.D ⊓ ≤1 R, with A⊓B⊓D ⊑ ⊥.
//!
//! The `≤1 R` forces `solve_at_most` to enumerate partitions (only one
//! valid partition: all three merged into one group). The merge runs,
//! then the label-clash clause A⊓B⊓D⊑⊥ fires on the survivor. No pairwise
//! `DisjointClasses` is declared, so `forced_distinct_exceeds` does NOT
//! short-circuit — `partition_rec` branches and increments `branches_taken`.
//!
//! Read-only invariant tested: verdict, `branches_taken`, `restores`,
//! and `max_branch_depth` are byte-identical flag-off vs flag-on.
//!
//! The probe is wired with the same builder chain used by the production
//! path (`with_precise_card_deps` + `with_double_blocking`), matching
//! what `decide_with_stats` / `sat_only_with_stats` in `owl-dl-reasoner`
//! apply.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult, SearchStats};
use std::io::Cursor;

// Three existential successors (A, B, D) with ≤1 R constraint.
// Their conjunction is Bottom but without pairwise DisjointClasses, so
// forced_distinct_exceeds stays false and partition_rec is entered.
const FIXTURE_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/test>
    Declaration(Class(:C))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:R))
    SubClassOf(:C ObjectSomeValuesFrom(:R :A))
    SubClassOf(:C ObjectSomeValuesFrom(:R :B))
    SubClassOf(:C ObjectSomeValuesFrom(:R :D))
    SubClassOf(:C ObjectMaxCardinality(1 :R))
    SubClassOf(ObjectIntersectionOf(:A :B :D) owl:Nothing)
)
";

fn build_fixture() -> (owl_dl_core::InternalOntology, owl_dl_core::ir::ClassId) {
    let mut reader = Cursor::new(FIXTURE_SRC);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let c_id = internal
        .vocabulary
        .class_id("http://rustdl.test/C")
        .expect("C declared");
    (internal, c_id)
}

struct ProbeResult {
    verdict: HyperResult,
    stats: SearchStats,
}

/// Construct the engine with the same builder chain the production path uses
/// (`decide_with_stats` / `sat_only_with_stats` in `owl-dl-reasoner`).
fn run_probe(
    internal: &owl_dl_core::InternalOntology,
    root: owl_dl_core::ir::ClassId,
    probe: bool,
) -> ProbeResult {
    let (clauses, _) = clausify_with_stats(internal);
    let mut eng = HyperEngine::new(&clauses, root)
        .with_precise_card_deps()
        .with_double_blocking();
    if probe {
        eng = eng.with_shadow_dep_probe(true);
    }
    let verdict = eng.decide(64);
    let stats = eng.stats();
    ProbeResult { verdict, stats }
}

#[test]
fn shadow_probe_is_read_only_and_records() {
    let (internal, root) = build_fixture();
    let off = run_probe(&internal, root, false);
    let on = run_probe(&internal, root, true);

    // The fixture is Unsat: ≤1 R forces merging the three disjoint-conjunction
    // successors, and the merged node satisfies A⊓B⊓D⊑⊥.
    assert_eq!(
        off.verdict,
        HyperResult::Unsat,
        "fixture must be Unsat (≤1 R + A⊓B⊓D⊑⊥)"
    );

    // ── Read-only invariant ──────────────────────────────────────────────────
    // The shadow probe must NEVER influence any search decision, merge, edge,
    // or verdict. All search counters are byte-identical flag-on vs flag-off.
    assert_eq!(on.verdict, off.verdict, "read-only: verdict invariant");
    assert_eq!(
        on.stats.branches_taken, off.stats.branches_taken,
        "read-only: branches invariant"
    );
    assert_eq!(
        on.stats.restores, off.stats.restores,
        "read-only: restores invariant"
    );
    assert_eq!(
        on.stats.max_branch_depth, off.stats.max_branch_depth,
        "read-only: max_branch_depth invariant"
    );

    // ── Non-vacuousness ─────────────────────────────────────────────────────
    // partition_rec increments `branches_taken` once per partition attempted.
    // This confirms the test exercises actual branching (not just a precheck
    // short-circuit), so the read-only invariant is tested under save/restore.
    assert!(
        on.stats.branches_taken >= 1,
        "fixture must branch (branches_taken={}, expected ≥1): the test is non-vacuous",
        on.stats.branches_taken
    );

    // ── Probe accumulation ───────────────────────────────────────────────────
    assert!(
        off.stats.clash_records.is_empty(),
        "flag-off records nothing"
    );
    assert!(
        !on.stats.clash_records.is_empty(),
        "flag-on records clashes (got {})",
        on.stats.clash_records.len()
    );

    // ── Structural consistency of ClashRecords ───────────────────────────────
    // Each record: branch_depth ≥ 0 (trivially true for u32).
    // When real is not ALL (not overflow), shadow must have at least as many
    // dep levels as real — shadow never collapses precision below the real
    // path (it is a superset, or at worst equal, to the real dep-set).
    // Note: ALL is encoded as `overflow` flag, visible as
    //   `highest == Some(127) && count == 0` (bits = 0 but overflow set).
    for rec in &on.stats.clash_records {
        let real_is_all = rec.real.highest == Some(127) && rec.real.count == 0;
        if !real_is_all {
            assert!(
                rec.shadow.count >= rec.real.count,
                "shadow must have ≥ dep levels as real when real is not ALL \
                 (shadow.count={}, real.count={}, depth={})",
                rec.shadow.count,
                rec.real.count,
                rec.branch_depth
            );
        }
    }
}
