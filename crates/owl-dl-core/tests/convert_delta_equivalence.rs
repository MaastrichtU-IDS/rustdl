//! `convert_delta(convert(O), d)` must be IRI-equivalent to `convert(O + d)`.
//! Ids WILL differ (`convert_ontology` sorts the components and then the axiom
//! list) -
//! only the IRI-level axiom multiset is comparable.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::model::{Build, Component, MutableOntology, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::delta;

fn axiom_strings(o: &owl_dl_core::InternalOntology) -> Vec<String> {
    let mut v: Vec<String> = o
        .live_axioms()
        .map(|(_, ax)| owl_dl_core::debug_render_axiom(ax, &o.vocabulary, &o.concepts))
        .collect();
    v.sort();
    v
}

#[test]
fn delta_addition_matches_from_scratch() {
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
    });

    let new_ax = horned_owl::model::SubClassOf {
        sub: b.class("http://x/B").into(),
        sup: b.class("http://x/C").into(),
    };

    let mut union = base.clone();
    union.insert(new_ax.clone());
    let from_scratch = convert_ontology(&union).unwrap();

    let mut incremental = convert_ontology(&base).unwrap();
    let mut mirror = base.clone();
    mirror.insert(new_ax.clone());
    delta::convert_delta(&mut incremental, &mirror, &[new_ax.into()]).unwrap();
    delta::refresh_derived(&mut incremental, &mirror);

    assert_eq!(axiom_strings(&from_scratch), axiom_strings(&incremental));
}

#[test]
fn refresh_derived_retracts_a_stale_derived_axiom() {
    // Functional(dp) + DataMin(2, dp) derives an unsat axiom via derive_data_axioms.
    // Removing Functional(dp) must retract it, or the session reports a
    // false-positive unsatisfiable class. This is the exact B1 failure mode.
    let b = Build::new_rc();
    let dp = b.data_property("http://x/dp");
    let functional = horned_owl::model::FunctionalDataProperty(dp.clone());

    let mut with: SetOntology<RcStr> = SetOntology::new_rc();
    with.insert(functional.clone());
    with.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/C").into(),
        sup: horned_owl::model::ClassExpression::DataMinCardinality {
            n: 2,
            dp: dp.clone(),
            dr: b
                .datatype("http://www.w3.org/2001/XMLSchema#integer")
                .into(),
        },
    });

    let mut internal = convert_ontology(&with).unwrap();
    let derived_before = internal.derived.count_ones(..);
    assert!(derived_before > 0, "fixture must actually derive something");

    // Remove Functional(dp) from the mirror, kill its lowered axiom, refresh.
    let mut without = with.clone();
    without.take(&Component::from(functional.clone()).into());
    let dp_role = internal
        .vocabulary
        .role_id("http://x/dp")
        .expect("data property lowered to a role");
    let func_idx = internal
        .live_axioms()
        .find(|(_, ax)| {
            matches!(ax, owl_dl_core::Axiom::FunctionalRole(owl_dl_core::Role::Named(r))
                if *r == dp_role)
        })
        .map(|(i, _)| i)
        .expect("Functional(dp) must have a lowered user axiom");
    assert!(internal.kill_axiom(func_idx));

    let diff = delta::refresh_derived(&mut internal, &without);

    assert!(
        !diff.killed.is_empty(),
        "the stale derived axiom must be retracted"
    );
    let from_scratch = convert_ontology(&without).unwrap();
    assert_eq!(axiom_strings(&from_scratch), axiom_strings(&internal));
}

#[test]
fn re_adding_the_premise_re_derives_the_retracted_axiom() {
    // Guards the tombstone machinery against a stale value->index cache: a
    // derived axiom killed in one commit must come back LIVE when its premise
    // returns, not stay matched against its dead slot.
    let b = Build::new_rc();
    let dp = b.data_property("http://x/dp");
    let functional = horned_owl::model::FunctionalDataProperty(dp.clone());
    let min2 = horned_owl::model::SubClassOf {
        sub: b.class("http://x/C").into(),
        sup: horned_owl::model::ClassExpression::DataMinCardinality {
            n: 2,
            dp: dp.clone(),
            dr: b
                .datatype("http://www.w3.org/2001/XMLSchema#integer")
                .into(),
        },
    };

    let mut with: SetOntology<RcStr> = SetOntology::new_rc();
    with.insert(functional.clone());
    with.insert(min2.clone());
    let mut internal = convert_ontology(&with).unwrap();

    // Commit 1: retract Functional(dp) -> the derived C ⊑ ⊥ must go.
    let mut without = with.clone();
    without.take(&Component::from(functional.clone()).into());
    let dp_role = internal.vocabulary.role_id("http://x/dp").unwrap();
    let func_idx = internal
        .live_axioms()
        .find(|(_, ax)| {
            matches!(ax, owl_dl_core::Axiom::FunctionalRole(owl_dl_core::Role::Named(r))
                if *r == dp_role)
        })
        .map(|(i, _)| i)
        .unwrap();
    assert!(internal.kill_axiom(func_idx));
    let first = delta::refresh_derived(&mut internal, &without);
    assert!(!first.killed.is_empty());

    // Commit 2: put Functional(dp) back. The overlay must re-derive.
    delta::convert_delta(
        &mut internal,
        &with,
        &[Component::from(functional.clone()).into()],
    )
    .unwrap();
    let second = delta::refresh_derived(&mut internal, &with);
    assert!(
        !second.added.is_empty(),
        "the derived axiom must be re-derived, not matched against its dead slot"
    );

    let from_scratch = convert_ontology(&with).unwrap();
    assert_eq!(axiom_strings(&from_scratch), axiom_strings(&internal));
}

/// `refresh_derived` over an UNCHANGED mirror must be a no-op: it is run at
/// every commit, so any disagreement between the overlay `convert_ontology`
/// marked and the overlay the passes recompute would churn axiom indices (and,
/// worse, retract something real) on every single revision. Run over tracked
/// fixtures with a non-trivial derived overlay - `/ontologies/` is gitignored,
/// so only `bench-corpus/` is usable here.
#[test]
fn refresh_derived_is_a_no_op_on_an_unchanged_ontology() {
    // `(fixture, does it have a derived overlay at all?)`. MEASURED, not
    // guessed: pizza 22 of 327 axioms, mie 69 of 994, paper5 0 of 94.
    //
    // The flag is what stops this test from passing vacuously. Every
    // assertion below is trivially satisfied by an EMPTY overlay - there is
    // nothing to re-derive and nothing to retract - so without pinning which
    // fixtures actually carry one, the whole test could decay into three
    // no-ops and stay green. Pinned as an equality rather than a `> 0` so it
    // also fires if paper5 ever GAINS an overlay, at which point it starts
    // carrying real weight and the comment above goes stale.
    for (name, has_overlay) in [("pizza", true), ("mie", true), ("paper5", false)] {
        let path = format!(
            "{}/../../bench-corpus/{name}.ofn",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut r = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let (set, _) = horned_owl::io::ofn::reader::read::<String, SetOntology<String>, _>(
            &mut r,
            ParserConfiguration::default(),
        )
        .unwrap();

        let mut internal = convert_ontology(&set).unwrap();
        let before = axiom_strings(&internal);
        let derived = internal.derived.count_ones(..);
        assert!(
            derived < internal.axioms.len(),
            "{name}: everything marked derived - the user baseline is wrong"
        );
        assert_eq!(
            derived > 0,
            has_overlay,
            "{name}: derived-overlay presence changed ({derived} derived) - see the \
             comment on this loop"
        );

        let diff = delta::refresh_derived(&mut internal, &set);
        assert!(
            diff.added.is_empty(),
            "{name}: re-derived {} spurious axioms",
            diff.added.len()
        );
        assert!(
            diff.killed.is_empty(),
            "{name}: retracted {} live axioms",
            diff.killed.len()
        );
        assert_eq!(before, axiom_strings(&internal), "{name}: live set changed");
    }
}

/// `refresh_derived` over an unchanged mirror must be a no-op for the two
/// REWRITING passes too. `split_disjunctive_antecedents` CONSUMES
/// `(A ⊔ B) ⊑ C` and emits `A ⊑ C`, `B ⊑ C` in its place, so the original is
/// absent from `axioms` entirely: if the pass input is reconstructed from the
/// post-pass state it cannot reproduce those two, and the reconcile deletes
/// them. `bench-corpus` carries neither construct, so this case has to be
/// synthetic.
#[test]
fn refresh_derived_is_a_no_op_on_a_union_lhs_axiom() {
    let b = Build::new_rc();
    let mut o: SetOntology<RcStr> = SetOntology::new_rc();
    o.insert(horned_owl::model::SubClassOf {
        sub: horned_owl::model::ClassExpression::ObjectUnionOf(vec![
            b.class("http://x/A").into(),
            b.class("http://x/B").into(),
        ]),
        sup: b.class("http://x/C").into(),
    });

    let mut internal = convert_ontology(&o).unwrap();
    let before = axiom_strings(&internal);
    assert!(!before.is_empty(), "fixture must lower to something");

    let diff = delta::refresh_derived(&mut internal, &o);
    assert!(
        diff.killed.is_empty(),
        "union-LHS split must not be retracted"
    );
    assert!(
        diff.added.is_empty(),
        "union-LHS split must not be duplicated"
    );
    assert_eq!(before, axiom_strings(&internal));
}

/// Same, for `decompose_long_chains`: it CONSUMES `r1 ∘ r2 ∘ r3 ⊑ s` and emits
/// a 2-leg cascade through a fresh auxiliary role.
#[test]
fn refresh_derived_is_a_no_op_on_a_long_role_chain() {
    let b = Build::new_rc();
    let mut o: SetOntology<RcStr> = SetOntology::new_rc();
    o.insert(horned_owl::model::SubObjectPropertyOf {
        sub: horned_owl::model::SubObjectPropertyExpression::ObjectPropertyChain(vec![
            b.object_property("http://x/r1").into(),
            b.object_property("http://x/r2").into(),
            b.object_property("http://x/r3").into(),
        ]),
        sup: b.object_property("http://x/s").into(),
    });

    let mut internal = convert_ontology(&o).unwrap();
    let before = axiom_strings(&internal);
    assert!(!before.is_empty(), "fixture must lower to something");

    let diff = delta::refresh_derived(&mut internal, &o);
    assert!(
        diff.killed.is_empty(),
        "chain cascade must not be retracted"
    );
    assert!(
        diff.added.is_empty(),
        "chain cascade must not be duplicated"
    );
    assert_eq!(before, axiom_strings(&internal));
}

/// `derive_data_axioms` reads the MIRROR's source components, not the IR. A
/// caller that retracts a user axiom in the IR but passes a mirror that still
/// carries the component gets the stale `C ⊑ ⊥` re-derived - the false
/// positive this module exists to prevent, reintroduced from the caller side.
/// Only a debug assertion stands between us and that, so pin that it fires.
#[test]
#[should_panic(expected = "mirror and IR baseline disagree")]
// The guard it pins is a `debug_assert!`, so in a `--release` test build
// `refresh_derived` returns normally and `should_panic` FAILS. Skip there
// rather than let a release run go red on a test that cannot hold.
#[cfg_attr(not(debug_assertions), ignore = "guard is a debug_assert")]
fn refresh_derived_rejects_a_stale_mirror() {
    let b = Build::new_rc();
    let dp = b.data_property("http://x/dp");
    let mut with: SetOntology<RcStr> = SetOntology::new_rc();
    with.insert(horned_owl::model::FunctionalDataProperty(dp.clone()));
    with.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/C").into(),
        sup: horned_owl::model::ClassExpression::DataMinCardinality {
            n: 2,
            dp: dp.clone(),
            dr: b
                .datatype("http://www.w3.org/2001/XMLSchema#integer")
                .into(),
        },
    });

    let mut internal = convert_ontology(&with).unwrap();
    let dp_role = internal.vocabulary.role_id("http://x/dp").unwrap();
    let func_idx = internal
        .live_axioms()
        .find(|(_, ax)| {
            matches!(ax, owl_dl_core::Axiom::FunctionalRole(owl_dl_core::Role::Named(r))
                if *r == dp_role)
        })
        .map(|(i, _)| i)
        .unwrap();
    assert!(internal.kill_axiom(func_idx));

    // The mirror still contains FunctionalDataProperty(dp) - a desync.
    delta::refresh_derived(&mut internal, &with);
}

/// A user axiom CONSUMED by a rewriting pass has no index in `axioms`, so it
/// cannot be retracted through `kill_axiom` - only by dropping it from the
/// `user_axioms` baseline, which needs a source-level retraction API that is
/// Task 7's job. Until then the hazard must be LOUD, not silent: pin that the
/// mirror guard catches the desync instead of letting the passes re-derive
/// `A ⊑ C`, `B ⊑ C` from a premise the user already removed.
#[test]
#[should_panic(expected = "mirror and IR baseline disagree")]
// The guard it pins is a `debug_assert!`, so in a `--release` test build
// `refresh_derived` returns normally and `should_panic` FAILS. Skip there
// rather than let a release run go red on a test that cannot hold.
#[cfg_attr(not(debug_assertions), ignore = "guard is a debug_assert")]
fn retracting_a_consumed_original_is_caught_not_silently_ignored() {
    let b = Build::new_rc();
    let ax = horned_owl::model::SubClassOf {
        sub: horned_owl::model::ClassExpression::ObjectUnionOf(vec![
            b.class("http://x/A").into(),
            b.class("http://x/B").into(),
        ]),
        sup: b.class("http://x/C").into(),
    };
    let mut o: SetOntology<RcStr> = SetOntology::new_rc();
    o.insert(ax.clone());

    let mut internal = convert_ontology(&o).unwrap();
    let mut without = o.clone();
    without.take(&Component::from(ax).into());
    delta::refresh_derived(&mut internal, &without);
}
