//! Gate 2 of the sparse-`entailed`-matrix spec
//! (`docs/superpowers/specs/2026-07-21-sparse-classification-entailed-matrix-spec.md` §6):
//! dense-vs-sparse semantic identity.
//!
//! Builds ONE small `Classification` twice — once at the default dense
//! threshold (Dense arm) and once with `RUSTDL_CLASSIFY_DENSE_MAX=0`
//! (forced Sparse arm) — and asserts the three accessors
//! (`is_subclass`, `equivalent_classes`, `direct_subsumers`) agree as
//! ORDERED outputs for every pair/class. The fixture deliberately
//! contains the three risky shapes the spec mandates:
//!   (a) an unsatisfiable class `U` (its sparse row is ELIDED — the
//!       "⊥ ⊑ *" fact is reintroduced only by the `entails` choke-point),
//!   (b) an equivalence pair `E1 ≡ E2` (exercises the mutual-subsumption
//!       filter + reflexive-merge ordering),
//!   (c) a 3-level chain `A ⊑ B ⊑ C` (exercises the Hasse prune in
//!       `direct_subsumers` — `C` must be pruned as indirect for `A`).
//!
//! Everything lives in a single `#[test]` because the dense/sparse arm is
//! selected via a process-global env var — no intra-binary parallelism may
//! race it.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const FIXTURE: &str = "Prefix(:=<http://e#>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(\n\
 Declaration(Class(:U)) Declaration(Class(:A)) Declaration(Class(:B))\n\
 Declaration(Class(:C)) Declaration(Class(:E1)) Declaration(Class(:E2))\n\
 SubClassOf(:U owl:Nothing)\n\
 SubClassOf(:A :B) SubClassOf(:B :C)\n\
 EquivalentClasses(:E1 :E2)\n\
)\n";

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

#[test]
fn dense_and_sparse_classifications_are_semantically_identical() {
    // --- Build the Dense arm (default threshold; 6 classes ≪ 60k). ---
    // SAFETY: single-test binary — no other thread reads/writes env here.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_CLASSIFY_DENSE_MAX");
    }
    let dense = classify(FIXTURE);

    // --- Build the Sparse arm (forced via the test-only override). ---
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_CLASSIFY_DENSE_MAX", "0");
    }
    let sparse = classify(FIXTURE);
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_CLASSIFY_DENSE_MAX");
    }

    // The override must actually engage each arm — otherwise this test
    // silently compares Dense with Dense and validates nothing. The
    // `EntailmentMatrix` enum's derived Debug carries the arm name.
    let dense_repr = format!("{dense:?}");
    let sparse_repr = format!("{sparse:?}");
    assert!(
        dense_repr.contains("Dense"),
        "default threshold did not build the Dense arm"
    );
    assert!(
        sparse_repr.contains("Sparse"),
        "RUSTDL_CLASSIFY_DENSE_MAX=0 did not force the Sparse arm"
    );

    // Same vocabulary, same order.
    let classes: Vec<String> = dense.classes().to_vec();
    assert_eq!(classes, sparse.classes().to_vec(), "class list must agree");

    // Fixture sanity: the three mandated shapes are actually present
    // (a trivial fixture would pass while every risky path ships untested).
    assert!(
        dense
            .unsatisfiable_classes()
            .iter()
            .any(|c| c.ends_with("#U")),
        "fixture must contain the unsatisfiable class U"
    );
    assert!(
        dense.is_subclass("http://e#A", "http://e#C"),
        "fixture must contain the transitive chain A ⊑ B ⊑ C"
    );
    assert!(
        dense.is_subclass("http://e#E1", "http://e#E2")
            && dense.is_subclass("http://e#E2", "http://e#E1"),
        "fixture must contain the equivalence pair E1 ≡ E2"
    );

    // (1) `is_subclass` agrees for EVERY ordered pair, incl. unsat subjects.
    for x in &classes {
        for y in &classes {
            assert_eq!(
                dense.is_subclass(x, y),
                sparse.is_subclass(x, y),
                "is_subclass({x}, {y}) diverged between Dense and Sparse"
            );
        }
    }

    // (2)+(3) `equivalent_classes` / `direct_subsumers` agree as ORDERED
    // Vecs for every class (ascending-id order is part of the contract).
    for x in &classes {
        assert_eq!(
            dense.equivalent_classes(x),
            sparse.equivalent_classes(x),
            "equivalent_classes({x}) diverged (ordered comparison)"
        );
        assert_eq!(
            dense.direct_subsumers(x),
            sparse.direct_subsumers(x),
            "direct_subsumers({x}) diverged (ordered comparison)"
        );
    }

    // Unsat subject specifics: U's equivalence class is exactly the unsat
    // set, and the unsat sets agree.
    assert_eq!(
        dense.unsatisfiable_classes(),
        sparse.unsatisfiable_classes(),
        "unsatisfiable class sets diverged"
    );
    assert_eq!(
        dense.equivalent_classes("http://e#U"),
        sparse.equivalent_classes("http://e#U"),
        "equivalent_classes of the unsat subject diverged"
    );

    // Hasse-prune spot check on the chain: A's direct supers must be
    // exactly [B] (C pruned as indirect) in BOTH arms.
    assert_eq!(dense.direct_subsumers("http://e#A"), vec!["http://e#B"]);
    assert_eq!(sparse.direct_subsumers("http://e#A"), vec!["http://e#B"]);
}
