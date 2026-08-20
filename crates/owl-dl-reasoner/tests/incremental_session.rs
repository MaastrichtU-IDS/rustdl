//! Task 7 — the `IncrementalSession` public API (spec §8, §7, §2, §10).
//!
//! Every test here records, in a comment, the mutation of the implementation
//! that makes it fail. A test whose assertion cannot fail is worthless — see
//! the plan's controller ruling C3.
#![allow(clippy::unwrap_used)]
use horned_owl::model::{Build, ClassExpression, MutableOntology, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::incremental::{AxiomDelta, IncrementalSession};

fn hierarchy(c: &owl_dl_reasoner::Classification) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for a in c.classes() {
        for b in c.classes() {
            if a != b && c.is_subclass(a, b) {
                v.push((a.clone(), b.clone()));
            }
        }
    }
    v.sort();
    v
}

fn sub(b: &Build<RcStr>, s: &str, p: &str) -> horned_owl::model::SubClassOf<RcStr> {
    horned_owl::model::SubClassOf {
        sub: b.class(s).into(),
        sup: b.class(p).into(),
    }
}

#[test]
fn session_addition_matches_from_scratch() {
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/A", "http://x/B"));
    let added = sub(&b, "http://x/B", "http://x/C");

    let mut session = IncrementalSession::new(&base).unwrap();
    assert_eq!(session.revision().0, 0);
    let rev = session
        .apply(&AxiomDelta {
            added: vec![added.clone().into()],
            removed: vec![],
        })
        .unwrap();
    assert_eq!(rev.0, 1);

    let mut union = base.clone();
    union.insert(added);
    let expected = owl_dl_reasoner::classify(&union).unwrap();

    assert_eq!(hierarchy(&expected), hierarchy(session.classify().unwrap()));
    // A ⊑ C must now be entailed transitively.
    assert!(session.is_subclass_of("http://x/A", "http://x/C").unwrap());

    // A class-only addition must be absorbed by the retained engine, not by a
    // rebuild. Mutation: route every delta to `SaturationState::build` and the
    // two counters swap.
    assert_eq!(session.stats().rebuilds, 0);
    assert_eq!(session.stats().additions_reused, 1);
    assert_eq!(session.stats().revisions, 1);
}

#[test]
fn consistency_verdict_is_retained_in_the_monotone_direction() {
    // Spec §10: `consistent` survives a delete; `inconsistent` survives an add.
    let b = Build::new_rc();
    let ax = sub(&b, "http://x/A", "http://x/B");
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(ax.clone());

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(session.is_consistent().unwrap());
    // A pure delete cannot make a consistent KB inconsistent.
    session
        .apply(&AxiomDelta {
            added: vec![],
            removed: vec![ax.into()],
        })
        .unwrap();
    assert!(session.is_consistent().unwrap());
}

#[test]
fn removal_forces_a_rebuild_but_stays_correct_in_p1() {
    let b = Build::new_rc();
    let ax = sub(&b, "http://x/A", "http://x/B");
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(ax.clone());

    let mut session = IncrementalSession::new(&base).unwrap();
    session
        .apply(&AxiomDelta {
            added: vec![],
            removed: vec![ax.into()],
        })
        .unwrap();

    assert_eq!(session.stats().rebuilds, 1, "P1 rebuilds on any delete");
    assert!(!session.is_subclass_of("http://x/A", "http://x/B").unwrap());
    // NOTE for the reader: the assertion above holds for the WEAKER reason
    // that A and B left the live signature with the only axiom that mentioned
    // them, so `is_subclass` answers `false` from a missing index rather than
    // from a retracted entailment. The strong version of this test — a
    // retraction whose premise is really gone while its entities are still
    // reported — is
    // `deleting_a_union_lhs_premise_retracts_its_rewritten_consequence`.
    assert!(session.classify().unwrap().classes().is_empty());
}

#[test]
fn a_rejected_delta_leaves_the_revision_untouched() {
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/A", "http://x/B"));
    let mut session = IncrementalSession::new(&base).unwrap();
    let before = hierarchy(session.classify().unwrap()); // owned Vec: borrow ends here
    let rev_before = session.revision();

    // A delta whose removal names an axiom that is not present.
    let bogus = sub(&b, "http://x/NOPE", "http://x/ALSO_NOPE");
    let err = session.apply(&AxiomDelta {
        added: vec![],
        removed: vec![bogus.into()],
    });
    assert!(err.is_err(), "removing an absent axiom is rejected");
    assert_eq!(
        session.revision().0,
        rev_before.0,
        "revision must not advance"
    );
    assert_eq!(before, hierarchy(session.classify().unwrap()));
}

#[test]
fn annotation_only_delta_is_logically_empty() {
    // Spec §10: annotation edits lower to zero logical axioms and must commit
    // a revision with zero invalidation.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/A", "http://x/B"));
    let mut session = IncrementalSession::new(&base).unwrap();
    let rebuilds_before = session.stats().rebuilds;
    let reused_before = session.stats().additions_reused;

    let anno = horned_owl::model::AnnotationAssertion {
        subject: b.iri("http://x/A").into(),
        ann: horned_owl::model::Annotation {
            ap: b.annotation_property("http://www.w3.org/2000/01/rdf-schema#comment"),
            av: horned_owl::model::AnnotationValue::Literal(horned_owl::model::Literal::Simple {
                literal: "hi".to_string(),
            }),
        },
    };
    session
        .apply(&AxiomDelta {
            added: vec![anno.into()],
            removed: vec![],
        })
        .unwrap();
    assert_eq!(
        session.stats().rebuilds,
        rebuilds_before,
        "no rebuild for an annotation"
    );
    // ZERO invalidation, not just "no rebuild": the engine must not be touched
    // at all. Mutation: drop the logically-empty short-circuit and route the
    // delta to `apply_additions` — `additions_reused` then goes to 1.
    assert_eq!(session.stats().additions_reused, reused_before);
    assert_eq!(session.revision().0, 1, "the revision still advances");
}

// ---------------------------------------------------------------------------
// The four hard requirements carried from earlier reviews.
// ---------------------------------------------------------------------------

#[test]
fn deleting_a_union_lhs_premise_retracts_its_rewritten_consequence() {
    // HARD REQUIREMENT 1. `(A ⊔ B) ⊑ C` is CONSUMED by
    // `split_disjunctive_antecedents`: `internal.axioms` holds only the
    // rewritten `A ⊑ C` / `B ⊑ C` (both marked `derived`), so the user axiom
    // has NO index in `axioms` and `kill_axiom` can never reach it.
    //
    // Two mutations fail this test, in opposite directions:
    //  * resolve `removed` to an axiom INDEX instead of a value  ⇒ the delete
    //    is rejected as "not present" and the `unwrap()` panics;
    //  * prune only by index (tombstone, keep the incremental path) ⇒ the
    //    premise survives in `user_axioms`, `refresh_derived` re-derives
    //    `A ⊑ C`, and the final assertion sees a FALSE POSITIVE.
    let b = Build::new_rc();
    let union_ax = horned_owl::model::SubClassOf {
        sub: ClassExpression::ObjectUnionOf(vec![
            b.class("http://x/A").into(),
            b.class("http://x/B").into(),
        ]),
        sup: b.class("http://x/C").into(),
    };
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(union_ax.clone());
    // Keep A and C in the live signature after the delete, so the final
    // assertion is about a real absence of entailment and not about an IRI
    // that dropped out of the report.
    base.insert(sub(&b, "http://x/D", "http://x/A"));
    base.insert(sub(&b, "http://x/C", "http://x/E"));

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(
        session.is_subclass_of("http://x/A", "http://x/C").unwrap(),
        "the union premise entails A ⊑ C before the delete"
    );

    session
        .apply(&AxiomDelta {
            added: vec![],
            removed: vec![union_ax.clone().into()],
        })
        .unwrap();

    let mut reduced = base.clone();
    reduced.remove(&union_ax.into());
    let expected = owl_dl_reasoner::classify(&reduced).unwrap();
    assert_eq!(hierarchy(&expected), hierarchy(session.classify().unwrap()));
    assert!(
        !session.is_subclass_of("http://x/A", "http://x/C").unwrap(),
        "retracting the union premise must retract its rewritten consequence"
    );
}

#[test]
fn a_deleted_class_drops_out_of_the_report() {
    // HARD REQUIREMENT 3 (filter). Ids are append-only and never recycled, so
    // after the last axiom mentioning `C` is retracted the vocabulary still
    // names it. Mutation: report `reportable_class_iris` unfiltered and `C`
    // stays in `classes()` forever.
    let b = Build::new_rc();
    let doomed = sub(&b, "http://x/B", "http://x/C");
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/A", "http://x/B"));
    base.insert(doomed.clone());

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(
        session
            .classify()
            .unwrap()
            .classes()
            .contains(&"http://x/C".to_string())
    );

    session
        .apply(&AxiomDelta {
            added: vec![],
            removed: vec![doomed.into()],
        })
        .unwrap();

    let classes = session.classify().unwrap().classes().to_vec();
    assert_eq!(
        classes,
        vec!["http://x/A".to_string(), "http://x/B".to_string()],
        "a class no live axiom mentions must not be reported"
    );
}

#[test]
fn reported_classes_are_sorted_by_iri_not_by_session_id() {
    // HARD REQUIREMENT 3 (order). The session interns `A` LAST (it arrives in
    // the delta), so id order is B, C, A while IRI order is A, B, C.
    // Mutation: drop the sort in the session's report and the vector comes
    // back in id order.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/B", "http://x/C"));
    let mut session = IncrementalSession::new(&base).unwrap();
    session
        .apply(&AxiomDelta {
            added: vec![sub(&b, "http://x/A", "http://x/B").into()],
            removed: vec![],
        })
        .unwrap();

    assert_eq!(
        session.classify().unwrap().classes().to_vec(),
        vec![
            "http://x/A".to_string(),
            "http://x/B".to_string(),
            "http://x/C".to_string()
        ]
    );
}

#[test]
fn an_unsatisfiable_class_subsumes_everything_in_the_session_report() {
    // HARD REQUIREMENT 4. An unsatisfiable class's row is ELIDED, so a session
    // that re-keys the matrix by copying raw rows loses `⊥ ⊑ *`. Mutation:
    // rebuild the restricted matrix from `row_contains` for unsat subjects too
    // (instead of leaving the row elided) and `U ⊑ A` comes back false.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/A", "http://x/B"));
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/U").into(),
        sup: ClassExpression::ObjectIntersectionOf(vec![
            b.class("http://x/P").into(),
            b.class("http://x/Q").into(),
        ]),
    });
    base.insert(horned_owl::model::DisjointClasses(vec![
        b.class("http://x/P").into(),
        b.class("http://x/Q").into(),
    ]));

    let mut session = IncrementalSession::new(&base).unwrap();
    let expected = owl_dl_reasoner::classify(&base).unwrap();
    assert!(expected.is_subclass("http://x/U", "http://x/A"));
    assert_eq!(hierarchy(&expected), hierarchy(session.classify().unwrap()));
    assert!(
        session.is_subclass_of("http://x/U", "http://x/A").unwrap(),
        "⊥ ⊑ * must survive the session's re-keying of the matrix"
    );
}

#[test]
fn a_partially_invalid_delta_commits_nothing() {
    // Spec §7, fail-closed. The valid removal and the valid addition come
    // BEFORE the bogus removal in the delta. Mutation: validate-as-you-go
    // instead of staging the whole delta first, and the ontology is left
    // half-mutated at a revision that never committed.
    let b = Build::new_rc();
    let live = sub(&b, "http://x/A", "http://x/B");
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(live.clone());
    base.insert(sub(&b, "http://x/B", "http://x/C"));

    let mut session = IncrementalSession::new(&base).unwrap();
    let before = hierarchy(session.classify().unwrap());
    let rev_before = session.revision();
    let stats_before = session.stats().clone();

    let bogus = sub(&b, "http://x/NOPE", "http://x/ALSO_NOPE");
    let err = session.apply(&AxiomDelta {
        added: vec![sub(&b, "http://x/C", "http://x/D").into()],
        removed: vec![live.into(), bogus.into()],
    });
    assert!(err.is_err());
    assert_eq!(session.revision().0, rev_before.0);
    assert_eq!(session.stats(), &stats_before);
    assert_eq!(before, hierarchy(session.classify().unwrap()));

    // ... and the session is still USABLE. A rejection that half-updated the
    // mirror leaves it out of step with `user_axioms`, which only surfaces on
    // the next commit — so commit one and compare against from-scratch.
    let follow_up = sub(&b, "http://x/C", "http://x/D");
    session
        .apply(&AxiomDelta {
            added: vec![follow_up.clone().into()],
            removed: vec![],
        })
        .unwrap();
    let mut union = base.clone();
    union.insert(follow_up);
    let expected = owl_dl_reasoner::classify(&union).unwrap();
    assert_eq!(hierarchy(&expected), hierarchy(session.classify().unwrap()));
}

#[test]
fn a_property_axiom_addition_forces_a_rebuild() {
    // P1 routing: the role hierarchy is frozen into the engine, so any
    // property axiom in the delta is a rebuild. Mutation: report
    // `additions_reused` unconditionally and this flips.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/A", "http://x/B"));
    let mut session = IncrementalSession::new(&base).unwrap();

    session
        .apply(&AxiomDelta {
            added: vec![
                horned_owl::model::SubObjectPropertyOf {
                    sub: horned_owl::model::ObjectPropertyExpression::ObjectProperty(
                        b.object_property("http://x/r"),
                    )
                    .into(),
                    sup: horned_owl::model::ObjectPropertyExpression::ObjectProperty(
                        b.object_property("http://x/s"),
                    ),
                }
                .into(),
            ],
            removed: vec![],
        })
        .unwrap();

    assert_eq!(session.stats().rebuilds, 1);
    assert_eq!(session.stats().additions_reused, 0);
}

#[test]
fn an_addition_that_changes_the_derived_overlay_is_picked_up() {
    // HARD REQUIREMENT 2: `refresh_derived` runs in the SAME commit as
    // `convert_delta`. The derivation passes are whole-ontology fixpoints, so
    // the overlay of the previous revision is stale the moment the axiom set
    // moves. Here `Functional(dp)` + `C ⊑ ≥2 dp` is the Phase-D4 pattern that
    // derives `C ⊑ ⊥`; the premise arrives in the delta.
    //
    // Mutation: drop the `refresh_derived` call from `commit_addition` and
    // `C ⊑ ⊥` is never derived — the session reports `C` satisfiable while a
    // from-scratch run reports it unsatisfiable.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(sub(&b, "http://x/C", "http://x/Top"));
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/C").into(),
        sup: ClassExpression::DataMinCardinality {
            n: 2,
            dp: b.data_property("http://x/dp"),
            dr: horned_owl::model::DataRange::Datatype(
                b.datatype("http://www.w3.org/2001/XMLSchema#integer"),
            ),
        },
    });
    let functional = horned_owl::model::FunctionalDataProperty(b.data_property("http://x/dp"));

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(
        !session
            .classify()
            .unwrap()
            .unsatisfiable_classes()
            .contains(&"http://x/C"),
        "C is satisfiable until the functionality premise arrives"
    );

    session
        .apply(&AxiomDelta {
            added: vec![functional.clone().into()],
            removed: vec![],
        })
        .unwrap();

    let mut union = base.clone();
    union.insert(functional);
    let expected = owl_dl_reasoner::classify(&union).unwrap();
    assert!(
        expected.unsatisfiable_classes().contains(&"http://x/C"),
        "fixture guard: the from-scratch run must derive C ⊑ ⊥"
    );
    assert_eq!(hierarchy(&expected), hierarchy(session.classify().unwrap()));
    assert!(
        session
            .classify()
            .unwrap()
            .unsatisfiable_classes()
            .contains(&"http://x/C")
    );
}

#[test]
fn an_addition_that_breaks_consistency_is_not_answered_from_the_cache() {
    // Spec §10, the NEGATIVE direction — the half of the retention rule that is
    // observable. `consistent` does NOT survive an addition, so the cached
    // `Some(true)` must be dropped at commit and the verdict recomputed.
    //
    // This is the only cached value in the session that can be flatly WRONG.
    // Mutation: `self.consistency = retain_consistency(self.consistency,
    // Direction::Empty)` in `apply` (i.e. always retain) — or any
    // `Staged::direction()` that reports `Empty`/`Retraction` for a pure
    // addition — and the session keeps answering `true` for a KB that now has
    // no model at all.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::ClassAssertion {
        ce: b.class("http://x/C").into(),
        i: b.named_individual("http://x/a").into(),
    });
    base.insert(horned_owl::model::ClassAssertion {
        ce: b.class("http://x/D").into(),
        i: b.named_individual("http://x/a").into(),
    });

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(
        session.is_consistent().unwrap(),
        "fixture guard: C(a) ∧ D(a) alone has a model"
    );

    let disjoint = horned_owl::model::DisjointClasses(vec![
        b.class("http://x/C").into(),
        b.class("http://x/D").into(),
    ]);
    session
        .apply(&AxiomDelta {
            added: vec![disjoint.clone().into()],
            removed: vec![],
        })
        .unwrap();

    let mut union = base.clone();
    union.insert(disjoint);
    assert!(
        !owl_dl_reasoner::is_consistent(&union).unwrap(),
        "fixture guard: the from-scratch run must call the union inconsistent"
    );
    assert!(
        !session.is_consistent().unwrap(),
        "a delete-only retention rule must not survive an addition"
    );
}

#[test]
fn retracting_a_component_that_lowers_to_nothing_drops_the_inconsistent_verdict() {
    // WHOLE-BRANCH REVIEW C1 — the Task 4 / Task 7 seam.
    //
    // `derive_data_axioms` reads the horned-owl SOURCE mirror, so a component
    // the IR never sees can still put `⊤ ⊑ ⊥` in the derived overlay. Here a
    // LANGUAGE-TAGGED `DataPropertyAssertion` is rejected by
    // `exact_string_literal`, so `convert_component` returns `Ok(None)` and
    // `Staged::removed_axioms` is EMPTY — yet `literal_family` maps it to
    // `DtFamily::LangString`, which disagrees with the recorded
    // `DataPropertyRange(xsd:integer)` and makes the base KB inconsistent.
    //
    // Retracting it therefore makes the KB CONSISTENT while `direction()`
    // reports `Empty`, and `retain_consistency(Some(false), Empty)` keeps the
    // stale `false`. "Inconsistent" is the maximally-entailing answer, so a
    // stale one is a FALSE POSITIVE — the one thing this reasoner may never do.
    //
    // Mutation: derive `Direction` from what `convert_component` LOWERED
    // (`removed_axioms` / `added_axioms`) instead of from what moved in the
    // MIRROR, and the last assertion sees `false` for a KB with a model.
    let b = Build::new_rc();
    let dpa = horned_owl::model::DataPropertyAssertion {
        dp: b.data_property("http://x/dp"),
        from: horned_owl::model::Individual::Named(b.named_individual("http://x/a")),
        to: horned_owl::model::Literal::Language {
            literal: "hello".to_string(),
            lang: "en".to_string(),
        },
    };
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::DataPropertyRange {
        dp: b.data_property("http://x/dp"),
        dr: horned_owl::model::DataRange::Datatype(horned_owl::model::Datatype(
            b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        )),
    });
    base.insert(dpa.clone());
    base.insert(horned_owl::model::ClassAssertion {
        ce: b.class("http://x/C").into(),
        i: b.named_individual("http://x/a").into(),
    });

    assert!(
        !owl_dl_reasoner::is_consistent(&base).unwrap(),
        "fixture guard: a langString value under an xsd:integer range is a data-range violation"
    );

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(
        !session.is_consistent().unwrap(),
        "fixture guard: the session must agree, and CACHE `Some(false)`"
    );

    session
        .apply(&AxiomDelta {
            added: vec![],
            removed: vec![dpa.clone().into()],
        })
        .unwrap();

    let mut after = base.clone();
    after.remove(&dpa.into());
    assert!(
        owl_dl_reasoner::is_consistent(&after).unwrap(),
        "fixture guard: without the offending assertion the KB has a model"
    );
    assert!(
        session.is_consistent().unwrap(),
        "STALE `inconsistent` retained across a retraction the IR never saw — false positive"
    );
}

#[test]
fn adding_a_component_that_lowers_to_nothing_drops_the_consistent_verdict() {
    // WHOLE-BRANCH REVIEW C1, the mirror image — the MISS direction of the same
    // seam. The langString `DataPropertyAssertion` lowers to nothing on the way
    // IN too, so `added_axioms` was empty, `additions_are_inert()` was true and
    // `retain_consistency(Some(true), Empty)` kept a `consistent` verdict for a
    // KB the derived overlay had just closed with `⊤ ⊑ ⊥`.
    //
    // Not a false positive, so not the Critical — but the same defect, and the
    // reason `additions_are_inert()` now treats "lowered to nothing" as NOT
    // inert rather than adding one more shape to an allowlist.
    //
    // Mutation: `additions_are_inert()` over the `Some(_)` lowerings only
    // (`.flatten()`), and the session keeps answering `true`.
    let b = Build::new_rc();
    let dpa = horned_owl::model::DataPropertyAssertion {
        dp: b.data_property("http://x/dp"),
        from: horned_owl::model::Individual::Named(b.named_individual("http://x/a")),
        to: horned_owl::model::Literal::Language {
            literal: "hello".to_string(),
            lang: "en".to_string(),
        },
    };
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::DataPropertyRange {
        dp: b.data_property("http://x/dp"),
        dr: horned_owl::model::DataRange::Datatype(horned_owl::model::Datatype(
            b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        )),
    });
    base.insert(horned_owl::model::ClassAssertion {
        ce: b.class("http://x/C").into(),
        i: b.named_individual("http://x/a").into(),
    });

    let mut union = base.clone();
    union.insert(dpa.clone());
    assert!(
        owl_dl_reasoner::is_consistent(&base).unwrap(),
        "fixture guard: the range alone has a model"
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&union).unwrap(),
        "fixture guard: adding the langString value violates the xsd:integer range"
    );

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(
        session.is_consistent().unwrap(),
        "fixture guard: the session must agree, and CACHE `Some(true)`"
    );
    session
        .apply(&AxiomDelta {
            added: vec![dpa.into()],
            removed: vec![],
        })
        .unwrap();
    assert!(
        !session.is_consistent().unwrap(),
        "STALE `consistent` retained across an addition the IR never saw"
    );
}
