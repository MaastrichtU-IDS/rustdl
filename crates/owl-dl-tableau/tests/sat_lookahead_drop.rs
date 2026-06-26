//! Integration test for `RUSTDL_SAT_LOOKAHEAD` failed-literal drop at ⊔.
//!
//! Fixture: C ⊑ ∃R.A, C ⊑ (B1 ⊔ B2), B1 ⊑ ∃R.D, DisjointClasses(A, D),
//! FunctionalObjectProperty(R).
//!
//! At the disjunction B1⊔B2 for a node labelled C:
//! - Branch B1: node gets C ∧ B1 → {∃R.A, ∃R.D} with R functional → A⊓D merge → clash
//!   (the saturator detects this via the functional-merge rule).
//! - Branch B2: node gets C ∧ B2 → satisfiable (no clash).
//!
//! The clausifier does NOT derive C⊑B2 directly (needs functional-role-merge reasoning
//! that the EL subsumption engine handles, not the clausifier). So the disjunction
//! remains open in the Horn fixpoint. With lookahead ON: `seed_unsat({C,B1},{(R,A),(R,D)})`
//! returns true — drops B1, forces B2 (`lookahead_dropped≥1`, `lookahead_forced_single≥1`).
//! With lookahead OFF: branches both B1 (fails) and B2 (succeeds).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_saturation::seed_sat::build_base;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult, SearchStats};
use std::io::Cursor;
use std::sync::Arc;

const FIXTURE_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/test>
    Declaration(Class(:C))
    Declaration(Class(:A))
    Declaration(Class(:D))
    Declaration(Class(:B1))
    Declaration(Class(:B2))
    Declaration(ObjectProperty(:R))
    FunctionalObjectProperty(:R)
    SubClassOf(:C ObjectSomeValuesFrom(:R :A))
    SubClassOf(:B1 ObjectSomeValuesFrom(:R :D))
    SubClassOf(:C ObjectUnionOf(:B1 :B2))
    DisjointClasses(:A :D)
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

fn run_sat_probe(
    internal: &owl_dl_core::InternalOntology,
    root: owl_dl_core::ir::ClassId,
    lookahead: Option<Arc<owl_dl_saturation::seed_sat::SeedSaturator>>,
) -> (HyperResult, SearchStats) {
    let (clauses, _) = clausify_with_stats(internal);
    let mut eng = HyperEngine::new(&clauses, root);
    if let Some(sat) = lookahead {
        eng = eng.with_sat_lookahead(sat);
    }
    let verdict = eng.decide(64);
    let stats = eng.stats();
    (verdict, stats)
}

#[test]
fn lookahead_drops_dead_disjunct_off_branches_it() {
    let (internal, c_id) = build_fixture();
    let sat = Arc::new(build_base(&internal));

    // OFF: no lookahead
    let (verdict_off, stats_off) = run_sat_probe(&internal, c_id, None);
    // ON: with lookahead
    let (verdict_on, stats_on) = run_sat_probe(&internal, c_id, Some(sat.clone()));

    assert_eq!(verdict_on, verdict_off, "verdict must be invariant");
    assert_eq!(verdict_on, HyperResult::Sat, "control class is satisfiable");
    assert!(
        stats_on.lookahead_dropped >= 1,
        "ON drops the dead disjunct"
    );
    assert!(
        stats_on.lookahead_forced_single >= 1,
        "ON forces the lone survivor"
    );
    assert_eq!(stats_off.lookahead_dropped, 0, "OFF never drops");
}
