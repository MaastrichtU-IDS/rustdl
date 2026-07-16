//! Integration test for `RUSTDL_SEMANTIC_BRANCHING` Layer A: in-search
//! disjoint-pruning + unit-forcing at the `⊔` decision (Fix #2).
//!
//! Layer A is **verdict-preserving**: with the flag ON the engine drops each
//! disjunct that is told-disjoint with a label already on the node (it would
//! clash on the very next `horn_fixpoint` pass anyway) and, if exactly one
//! `Atom::Class` disjunct survives, unit-forces it without opening a decision
//! level. So the Sat/Unsat verdict must be identical to the flag-OFF path.
//!
//! Fixtures (no `sat_lookahead` — Layer A's own filter is exercised):
//!
//! * `Start ⊑ A`, `Start ⊑ E`, `A ⊑ B ⊔ C ⊔ D`, `Disjoint(E,B)`, `Disjoint(E,C)`.
//!   Node seeded (via `Start`) with `{A,E}`; the `A ⊑ B⊔C⊔D` disjunction is
//!   OPEN (none of B/C/D on the node). Layer A prunes B and C (each disjoint
//!   with E on the node), leaving D the lone survivor → unit-forced. Result
//!   Sat, `semantic_prunes >= 2`, `semantic_unit_forces >= 1`. OFF branches
//!   B (clash), C (clash), D (Sat) → also Sat.
//!
//! * `Start2 ⊑ A2`, `Start2 ⊑ E2`, `A2 ⊑ B2 ⊔ C2`, `Disjoint(E2,B2)`,
//!   `Disjoint(E2,C2)`. Every disjunct is pruned → Unsat directly. OFF
//!   branches both (each clashes) → also Unsat. `semantic_prunes >= 2`.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult, SearchStats};
use std::io::Cursor;

const UNIT_FORCE_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Ontology(<http://rustdl.test/unitforce>
    Declaration(Class(:Start))
    Declaration(Class(:A))
    Declaration(Class(:E))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    SubClassOf(:Start :A)
    SubClassOf(:Start :E)
    SubClassOf(:A ObjectUnionOf(:B :C :D))
    DisjointClasses(:E :B)
    DisjointClasses(:E :C)
)
";

const ALL_PRUNED_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Ontology(<http://rustdl.test/allpruned>
    Declaration(Class(:Start2))
    Declaration(Class(:A2))
    Declaration(Class(:E2))
    Declaration(Class(:B2))
    Declaration(Class(:C2))
    SubClassOf(:Start2 :A2)
    SubClassOf(:Start2 :E2)
    SubClassOf(:A2 ObjectUnionOf(:B2 :C2))
    DisjointClasses(:E2 :B2)
    DisjointClasses(:E2 :C2)
)
";

fn build(src: &str, root_iri: &str) -> (owl_dl_core::InternalOntology, owl_dl_core::ir::ClassId) {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let root = internal
        .vocabulary
        .class_id(root_iri)
        .expect("root declared");
    (internal, root)
}

fn run(
    internal: &owl_dl_core::InternalOntology,
    root: owl_dl_core::ir::ClassId,
    semantic: bool,
) -> (HyperResult, SearchStats) {
    let (clauses, _) = clausify_with_stats(internal);
    let mut eng = HyperEngine::new(&clauses, root);
    if semantic {
        eng = eng.with_semantic_branching();
    }
    let verdict = eng.decide(64);
    (verdict, eng.stats())
}

#[test]
fn semantic_branching_prunes_and_unit_forces_verdict_identical() {
    let (internal, root) = build(UNIT_FORCE_SRC, "http://rustdl.test/Start");

    let (verdict_off, stats_off) = run(&internal, root, false);
    let (verdict_on, stats_on) = run(&internal, root, true);

    assert_eq!(
        verdict_on, verdict_off,
        "verdict must be invariant to the flag"
    );
    assert_eq!(
        verdict_on,
        HyperResult::Sat,
        "lone survivor D is satisfiable"
    );

    // Non-vacuous: the disjoint-prune actually fired (B and C both dropped),
    // and the lone survivor was unit-forced (no decision level).
    assert!(
        stats_on.semantic_prunes >= 2,
        "ON prunes the two disjoint disjuncts (got {})",
        stats_on.semantic_prunes
    );
    assert!(
        stats_on.semantic_unit_forces >= 1,
        "ON unit-forces the lone survivor (got {})",
        stats_on.semantic_unit_forces
    );
    assert_eq!(stats_off.semantic_prunes, 0, "OFF never prunes");
    assert_eq!(stats_off.semantic_unit_forces, 0, "OFF never unit-forces");
}

#[test]
fn semantic_branching_all_pruned_is_unsat_verdict_identical() {
    let (internal, root) = build(ALL_PRUNED_SRC, "http://rustdl.test/Start2");

    let (verdict_off, stats_off) = run(&internal, root, false);
    let (verdict_on, stats_on) = run(&internal, root, true);

    assert_eq!(
        verdict_on, verdict_off,
        "verdict must be invariant to the flag"
    );
    assert_eq!(
        verdict_on,
        HyperResult::Unsat,
        "every disjunct is disjoint with E2 → Unsat"
    );
    assert!(
        stats_on.semantic_prunes >= 2,
        "ON prunes both disjuncts (got {})",
        stats_on.semantic_prunes
    );
    assert_eq!(stats_off.semantic_prunes, 0, "OFF never prunes");
}
