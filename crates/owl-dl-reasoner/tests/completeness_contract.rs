//! Canary for `Classification::completeness_guaranteed()` — the honest C2 contract.
//!
//! `completeness_guaranteed()` ⟹ `MISSED == 0`. It holds only on `PureEl` with
//! no timed-out pairs. `Horn` was removed from the trusted set in #124: it is a
//! clause-shape property that was being read as an engine property, and a Horn
//! ontology exists on which `classify` misses an oracle-confirmed subsumption. On
//! `OutOfFragment` inputs it returns `false` even when nothing times out, because
//! `trust_sat` can silently miss a subsumption on complement/disjunction structure
//! (the ORE-measured silent-miss hole — see
//! `docs/paper-calibration-decomposition-2026-07-08.md`). This test guards that the
//! flag is honest: `true` only when completeness is provable, never on a fragment
//! where a silent miss is possible.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

#[test]
fn pure_el_guarantees_completeness() {
    // A ⊑ B ⊑ C — pure EL, saturator complete.
    let c = classify(
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         SubClassOf(:A :B) SubClassOf(:B :C)\n)",
    );
    assert!(
        c.completeness_guaranteed(),
        "pure-EL: completeness must be guaranteed"
    );
}

#[test]
fn out_of_fragment_does_not_guarantee_completeness() {
    // A complement-defined class puts the ontology OutOfFragment: trust_sat may
    // silently miss, so completeness is NOT guaranteed even with nothing timed out.
    let c = classify(
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))\n\
         EquivalentClasses(:D ObjectIntersectionOf(:A ObjectComplementOf(:B)))\n\
         SubClassOf(:A :B)\n)",
    );
    assert!(
        !c.completeness_guaranteed(),
        "OutOfFragment: completeness must NOT be guaranteed (trust_sat may silently miss)"
    );
}

#[test]
fn horn_clause_shape_does_not_guarantee_completeness() {
    // #124's reproducer. The clausification is Horn (no disjunctive head, nothing
    // deferred) and `X ⊑ W` IS entailed — Konclude and HermiT both derive it, and
    // rustdl's own `subclass X W` answers `yes`. But `classify` misses it, so the
    // certificate must NOT be handed out. Before #124 this returned `true`, which
    // is the exact shape of a false completeness claim: wrong answer, clean flag.
    //
    // If `classify` ever learns this subsumption, this test still passes (the
    // certificate stays false on `Horn` by construction) — but then consider
    // whether `Horn` deserves a real, engine-backed allowlist rather than being
    // untrusted wholesale.
    let c = classify(
        "Prefix(:=<http://ex.org/>)\nOntology(\n\
         Declaration(Class(:X)) Declaration(Class(:B)) Declaration(Class(:Z)) Declaration(Class(:W))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))\n\
         ObjectPropertyDomain(:r ObjectSomeValuesFrom(:s :Z))\n\
         SubClassOf(ObjectSomeValuesFrom(:s :Z) :W)\n)",
    );
    assert!(
        !c.completeness_guaranteed(),
        "Horn clause shape must NOT guarantee completeness: this very ontology \
         is Horn and classify misses X ⊑ W, which both oracles derive"
    );
}

/// #246: a dropped axiom voids the guarantee. The ontology is pure-EL once the
/// unsupported `DataAllValuesFrom(:dp xsd:hexBinary)` axiom is dropped, and the
/// saturator is complete on what remains, but `A ⊑ ∃r.(B ⊓ U) ⊑ ∃r.B ⊑ E` is
/// entailed by the INPUT and missed. Before the fix this reported
/// `completeness_guaranteed() == true` with `A ⊑ E` absent.
#[test]
fn a_dropped_axiom_voids_the_guarantee() {
    let c = classify(
        "Prefix(:=<http://e#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nOntology(\n\
         Declaration(DataProperty(:dp)) Declaration(ObjectProperty(:r))\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:E))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B \
         DataAllValuesFrom(:dp xsd:hexBinary))))\n\
         SubClassOf(ObjectSomeValuesFrom(:r :B) :E)\n)",
    );
    assert!(
        !c.stats().dropped.is_empty(),
        "precondition: the axiom is dropped"
    );
    assert!(
        !c.is_subclass("http://e#A", "http://e#E"),
        "precondition: the entailed A ⊑ E is missed"
    );
    assert!(
        !c.completeness_guaranteed(),
        "a dropped axiom: completeness must NOT be guaranteed"
    );
}

/// Control: the same shape with no unsupported conjunct drops nothing, derives
/// `A ⊑ E`, and keeps the guarantee, so the test above isolates the drop.
#[test]
fn the_same_shape_without_a_drop_keeps_the_guarantee() {
    let c = classify(
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(ObjectProperty(:r))\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:E))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
         SubClassOf(ObjectSomeValuesFrom(:r :B) :E)\n)",
    );
    assert!(c.stats().dropped.is_empty());
    assert!(c.is_subclass("http://e#A", "http://e#E"));
    assert!(c.completeness_guaranteed());
}

/// Each condition of `ClassificationStats::completeness_guaranteed` on its own
/// voids the guarantee. The base is the one state that guarantees completeness
/// (PureEl, nothing cut, nothing refused, nothing dropped); every row flips
/// exactly one field away from it.
///
/// This is also the sabotage test for the guard: deleting any conjunct from the
/// predicate makes exactly its row fail. A new completeness signal needs a row
/// here AND a row in `docs/assurance/completeness-guarantee-ledger.md`.
#[test]
fn each_condition_alone_voids_the_guarantee() {
    use owl_dl_reasoner::{ClassificationStats, FragmentClassification};

    let base = || ClassificationStats {
        fragment: FragmentClassification::PureEl,
        ..ClassificationStats::default()
    };
    assert!(
        base().completeness_guaranteed(),
        "base state (PureEl, nothing cut or dropped) must guarantee completeness"
    );

    type Flip = fn(&mut ClassificationStats);
    let rows: [(&str, Flip); 8] = [
        ("fragment Horn (#124)", |s| {
            s.fragment = FragmentClassification::Horn;
        }),
        ("fragment OutOfFragment", |s| {
            s.fragment = FragmentClassification::OutOfFragment;
        }),
        ("timed_out_pairs", |s| s.timed_out_pairs = 1),
        ("wedge_hierarchy_blind (#214)", |s| {
            s.wedge_hierarchy_blind = true;
        }),
        ("wedge_clauses_refused (#353)", |s| {
            s.wedge_clauses_refused = 1;
        }),
        ("derived_same_undecided (#354)", |s| {
            s.derived_same_undecided = 1;
        }),
        ("consistency_undetermined (#162)", |s| {
            s.consistency_undetermined = true;
        }),
        ("dropped (#246)", |s| {
            s.dropped.record("TestKind".to_owned());
        }),
    ];
    // Seven conditions; the fragment condition has two failing values. Keep this
    // count in step with the predicate and the ledger.
    assert_eq!(rows.len(), 8);

    for (name, flip) in rows {
        let mut s = base();
        flip(&mut s);
        assert!(
            !s.completeness_guaranteed(),
            "{name}: this condition alone must void the guarantee"
        );
    }
}

/// `Classification::completeness_guaranteed` and the stats predicate are the
/// same answer, so the CLI and Python surfaces (which read the former) cannot
/// drift from the table test above.
#[test]
fn the_classification_delegates_to_its_stats() {
    for src in [
        // PureEl, guaranteed.
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:B))\nSubClassOf(:A :B)\n)",
        // OutOfFragment, not guaranteed.
        "Prefix(:=<http://e#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))\n\
         EquivalentClasses(:D ObjectIntersectionOf(:A ObjectComplementOf(:B)))\n)",
    ] {
        let c = classify(src);
        assert_eq!(
            c.completeness_guaranteed(),
            c.stats().completeness_guaranteed()
        );
    }
}
