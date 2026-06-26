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
use owl_dl_core::clause::{Atom, DlClause, X};
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ir::ClassId;
use owl_dl_tableau::hyper::{AboxSeed, HyperEngine, HyperResult, SearchStats};
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

/// NN-merge taint-recovery test: proves the shadow layer recovers the precise
/// dep-set that `nn_tainted` discards.
///
/// Fixture (low-level clause harness — no OFN parsing needed):
///
/// ```text
/// Classes:  q=0, k=1, p=2  (ordinary)
///           nom0=3, nom1=4  (nominals; nominal_base=3, 2 individuals)
/// Seeded:   node 0 ↔ individual 0, starts with {nom0}
///           node 1 ↔ individual 1, starts with {nom1}
/// Horn:     nom0 → k    (node 0 gets label k, dep=EMPTY)
///           nom1 → q    (node 1 gets label q, dep=EMPTY)
/// Decision D (level 0):
///           q → {nom0} ⊔ p
/// Clash clause:  k ⊓ q → ⊥
/// Branch-2 clash (makes the whole graph Unsat):
///           p ⊓ q → ⊥
/// ```
///
/// Branch 1 (D → nom0):
///   - node 1 gains `nom0` with dep `{D}`.
///   - NN-rule fires: both nodes carry `nom0` → `merge_with_cause(node0, node1, cause={D})`.
///   - `k` is folded onto the survivor with dep `{D}` (cause folded in).
///   - `nn_tainted = true` on survivor.
///   - `shadow_merge_cause = {D}` on survivor.
///   - Clash `k ⊓ q → ⊥`: `body_deps = dep(k)∪dep(q) = {D}∪EMPTY = {D}`.
///   - Real dep = `DepSet::ALL` (`nn_tainted` guard), shadow dep = `{D}`.
///
/// Branch 2 (D → p):
///   - node 1 gains `p` with dep `{D}`.
///   - Clash `p ⊓ q → ⊥`: `body_deps = {D}`.
///   - Real dep = `{D}` (node 1 is NOT tainted here), shadow dep = `{D}`.
///
/// Both branches Unsat → overall Unsat.
///
/// The test asserts the read-only invariant AND checks that at least one
/// recorded clash has `real=ALL` while `shadow` is a precise (non-ALL)
/// dep-set — proving the taint-recovery threading works end-to-end.
#[test]
fn shadow_probe_nn_taint_recovery() {
    // Class ids: q=0, k=1, p=2; nom0=3 ({individual 0}), nom1=4 ({individual 1}).
    let (q, k, p) = (ClassId::new(0), ClassId::new(1), ClassId::new(2));
    let (nom0, nom1) = (ClassId::new(3), ClassId::new(4));

    let clauses = vec![
        // nom0 → k  (node 0 picks up k, dep=EMPTY)
        DlClause {
            body: vec![Atom::Class(nom0, X)],
            head: vec![Atom::Class(k, X)],
        },
        // nom1 → q  (node 1 picks up q, dep=EMPTY)
        DlClause {
            body: vec![Atom::Class(nom1, X)],
            head: vec![Atom::Class(q, X)],
        },
        // Disjunctive decision D at q: q → {nom0} ⊔ p
        DlClause {
            body: vec![Atom::Class(q, X)],
            head: vec![Atom::Class(nom0, X), Atom::Class(p, X)],
        },
        // Clash clause (branch 1 path): k ⊓ q → ⊥
        DlClause {
            body: vec![Atom::Class(k, X), Atom::Class(q, X)],
            head: vec![],
        },
        // Clash clause (branch 2 path): p ⊓ q → ⊥
        DlClause {
            body: vec![Atom::Class(p, X), Atom::Class(q, X)],
            head: vec![],
        },
    ];

    let seed = AboxSeed {
        num_individuals: 2,
        nominal_base: 3,
        property_assertions: vec![],
        same_pairs: vec![],
    };

    // Build an engine with the same production-path builder flags plus the
    // shadow probe. Both off and on runs use identical flags except the probe.
    let build = |probe: bool| {
        let mut eng = HyperEngine::new_seeded(&clauses, &seed)
            .with_nominals(3, 2)
            .with_precise_card_deps()
            .with_double_blocking();
        if probe {
            eng = eng.with_shadow_dep_probe(true);
        }
        eng
    };

    let off_verdict = build(false).decide(64);
    let mut eng_on = build(true);
    let on_verdict = eng_on.decide(64);
    let on_stats = eng_on.stats();

    // ── Verdict ──────────────────────────────────────────────────────────────
    // Both branches of D clash → the whole graph is Unsat.
    assert_eq!(
        off_verdict,
        HyperResult::Unsat,
        "NN-merge fixture must be Unsat (both D-branches clash)"
    );

    // ── Read-only invariant ──────────────────────────────────────────────────
    assert_eq!(
        on_verdict, off_verdict,
        "nn-taint: shadow probe must not change the verdict"
    );

    // ── Probe non-vacuousness ────────────────────────────────────────────────
    assert!(
        !on_stats.clash_records.is_empty(),
        "nn-taint: flag-on must record clashes"
    );

    // Print all records for --nocapture inspection.
    println!(
        "nn-taint fixture: {} clash records",
        on_stats.clash_records.len()
    );
    for (i, rec) in on_stats.clash_records.iter().enumerate() {
        println!(
            "  record[{}]: branch_depth={}, real=(highest={:?}, count={}), \
             shadow=(highest={:?}, count={}, levels={:?})",
            i,
            rec.branch_depth,
            rec.real.highest,
            rec.real.count,
            rec.shadow.highest,
            rec.shadow.count,
            rec.shadow.levels,
        );
    }

    // ── Core assertion: taint-recovery ──────────────────────────────────────
    // The NN-merge clash (branch 1) must produce a record where the REAL
    // dep-set is ALL (nn_tainted forced it), but the SHADOW dep-set is a
    // precise non-ALL set carrying at least the disjunction-decision dep.
    //
    // ALL is encoded as: highest == Some(127) && count == 0 (overflow bit, no
    // actual bits set).  A precise set has count >= 1 and highest < 127.
    let taint_recovery_record = on_stats.clash_records.iter().find(|rec| {
        let real_is_all = rec.real.highest == Some(127) && rec.real.count == 0;
        let shadow_is_not_all = !(rec.shadow.highest == Some(127) && rec.shadow.count == 0);
        let shadow_is_precise = rec.shadow.count >= 1;
        real_is_all && shadow_is_not_all && shadow_is_precise
    });

    let rec = taint_recovery_record.unwrap_or_else(|| {
        panic!(
            "nn-taint: expected at least one ClashRecord with real=ALL and shadow precise/non-ALL \
             (count>=1), but none found.\n\
             Records: {:?}",
            on_stats.clash_records
        )
    });

    // Shadow must be precise (below the ALL sentinel of 127).
    assert!(
        rec.shadow.highest.is_some_and(|h| h < 127),
        "nn-taint: shadow highest must be < 127 (precise, not ALL sentinel), got {:?}",
        rec.shadow.highest
    );

    // The shadow dep set must have at least one decision level.
    assert!(
        rec.shadow.count >= 1,
        "nn-taint: shadow.count must be >= 1 (carries the disjunction-decision dep), got {}",
        rec.shadow.count
    );
}
