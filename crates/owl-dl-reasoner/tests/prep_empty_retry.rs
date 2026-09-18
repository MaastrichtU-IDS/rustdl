//! #162 — the `--global-timeout-ms` dead zone: a mid-range budget returned
//! ZERO rows while both smaller and larger budgets returned the complete
//! answer. Mechanism (confirmed by trace on 6 of 20 loaded reps, all naming
//! phase `saturate`): `prep_bounding_active` judges a mid-range budget
//! meetable, bounds prep, the saturation fixpoint is aborted, and the partial
//! closure is read off as the answer — with nothing reportable in it.
//!
//! The fix retries saturation UNBOUNDED when the aborted closure yields
//! nothing a caller could see. Verified statistically on `ore_ont_1043`
//! (the zone wanders with load, so no fixed budget pins it): base binary
//! 6 zeros / 20 reps; fixed binary **0 zeros / 40 reps with 19 rescues**, each
//! rescue printing the `#162 monotonicity fallback` banner and the complete
//! 30,607 rows.
//!
//! # The predicate's own falsification is the load-bearing test here
//!
//! The first version of `closure_yields_something` used
//! `subsumers_count(c) > 1`, which counts SYNTHETIC subsumers (Tseitin
//! markers) — and an aborted early-phase closure is full of exactly those. It
//! answered "yields something" on closures whose read-off printed 0 rows, and
//! the retry never fired (19 zeros / 40 reps, 0 rescues). The
//! `marker_only_subsumers_do_not_count` test below is the regression pin for
//! that exact mistake: it FAILS under the `subsumers_count` formulation.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn closure_probe(ofn: &str) -> bool {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(ofn.to_string()),
        ParserConfiguration::default(),
    )
    .unwrap();
    let internal = owl_dl_core::convert::convert_ontology(&o).unwrap();
    owl_dl_reasoner::closure_yields_something_for_test(&internal)
}

const HEAD: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/p>\n\
Declaration(Class(:A)) Declaration(Class(:B))\nDeclaration(ObjectProperty(:r))\n";

#[test]
fn a_reported_edge_counts() {
    assert!(
        closure_probe(&format!("{HEAD}SubClassOf(:A :B)\n)\n")),
        "A ⊑ B is a reported→reported edge: the closure yields something"
    );
}

#[test]
fn declarations_alone_yield_nothing() {
    assert!(
        !closure_probe(&format!("{HEAD})\n")),
        "reflexive-only closure: nothing a caller could see"
    );
}

#[test]
fn marker_only_subsumers_do_not_count() {
    // THE REGRESSION PIN for the falsified first predicate, and its fixture
    // needed one iteration to actually pin: a bare `A ⊑ ∃r.B` seeds an ∃-FACT,
    // not a marker subsumer, so the complete closure has `subsumers_count(A)=1`
    // and the count formulation survives the mutation check. Marker SUBSUMERS
    // come from ∃-LHS lowering: adding `∃r.B ⊑ ∃s.B` makes CR5 derive the
    // Tseitin marker onto `A` as a subsumer — `subsumers_count(A) > 1` with
    // ZERO reported→reported edges, deterministically, on a COMPLETE closure.
    // The count formulation answers "yields something" here; the read-off would
    // print 0 rows; the #162 retry would never fire (measured on the live dead
    // zone: 19 zeros / 40 reps, 0 rescues, under the count formulation).
    assert!(
        !closure_probe(&format!(
            "{HEAD}Declaration(ObjectProperty(:s))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
             SubClassOf(ObjectSomeValuesFrom(:r :B) ObjectSomeValuesFrom(:s :B))\n)\n"
        )),
        "a marker-only closure yields nothing reportable — counting markers is \
         the bug that made the retry unreachable"
    );
}

// No dedicated test for the predicate's `is_unsatisfiable` arm: deriving a
// reported unsat in the EL closure requires told edges (e.g. `A ⊑ B`, `A ⊑ C`,
// `Disjoint(B, C)`), which are themselves reported→reported edges, so the edge
// arm fires first and the unsat arm is belt-and-braces — kept in the predicate
// because a FUTURE closure source could mark unsat without materialised edges,
// but untestable in isolation today.
