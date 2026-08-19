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
    for name in ["pizza", "mie", "paper5"] {
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
