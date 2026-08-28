//! Tests for `VerifiedModel::still_holds_after` (Task 11): checking an
//! ontology EDIT against an already-verified model, instead of re-running
//! the reasoner.

use owl_dl_core::{Axiom, InternalOntology, Role, RoleId, SubRolePath};
use owl_dl_verify::{Bounds, Interpretation, UnresolvedReason, Verdict, verify};

mod common;

const SUBCLASS_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/sc>
Declaration(Class(:A)) Declaration(Class(:B))
SubClassOf(:A :B)
)
";

// A and B are deliberately UNRELATED: no axiom connects them, so the model
// built from this fixture has no reason to place B in A's label.
const INDEPENDENT_CLASSES_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/indep>
Declaration(Class(:A)) Declaration(Class(:B))
)
";

const ROLE_ONLY_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/role>
Declaration(ObjectProperty(:p))
)
";

// p and q are two INDEPENDENT roles; C's only existential is under p, so the
// model carries a p-edge with no matching q-edge — the shape needed to make
// an added `SubObjectPropertyOf(p, q)` a GENUINE (non-vacuous) check.
const TWO_ROLES_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/tworoles>
Declaration(Class(:C)) Declaration(Class(:E))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
SubClassOf(:C ObjectSomeValuesFrom(:p :E))
)
";

/// Builds and verifies `internal`, asserting the run is a clean `Verified`
/// (no build reasons, no violations, no unresolved forms) so that whatever
/// `still_holds_after` reports afterward is attributable to the ADDED axiom,
/// not to a pre-existing gap in the base model.
fn verified_model(internal: &InternalOntology) -> (owl_dl_verify::VerifiedModel, usize) {
    let (m, build_reasons) =
        owl_dl_verify::build_model(internal, &Bounds::default()).expect("builds");
    assert!(build_reasons.is_empty(), "{build_reasons:?}");
    let domain_size = m.domain_size();
    let (verdict, model_out) = verify(m, internal, None);
    assert!(
        matches!(verdict, Verdict::Verified { .. }),
        "base ontology must verify cleanly: {verdict:?}"
    );
    (
        model_out.expect("Verified always hands back Some(VerifiedModel)"),
        domain_size,
    )
}

#[test]
fn delta_that_holds_in_the_model_is_verified() {
    let mut internal = common::load(SUBCLASS_FIXTURE);
    let (vm, domain_size) = verified_model(&internal);

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let a_expr = internal.concepts.atomic(a);
    let b_expr = internal.concepts.atomic(b);
    // Re-asserting the very axiom the base ontology already carries: it
    // already holds in this model, so the edit must verify.
    let added = vec![Axiom::SubClassOf {
        sub: a_expr,
        sup: b_expr,
    }];

    match vm.still_holds_after(&internal.concepts, &added, None) {
        Verdict::Verified {
            axioms_checked,
            domain_size: reported_domain,
        } => {
            assert_eq!(axioms_checked, 1);
            assert_eq!(reported_domain, domain_size);
        }
        other => panic!("a delta that holds must verify: {other:?}"),
    }
}

#[test]
fn delta_that_genuinely_changes_the_classification_is_violated() {
    // WITHOUT this test, a `still_holds_after` that returns `Verified`
    // unconditionally passes every other test in this file.
    let mut internal = common::load(INDEPENDENT_CLASSES_FIXTURE);
    let (vm, _) = verified_model(&internal);

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let a_expr = internal.concepts.atomic(a);
    let b_expr = internal.concepts.atomic(b);
    // A and B are unrelated in the base model: this genuinely changes what
    // the classification would say, and must be caught.
    let added = vec![Axiom::SubClassOf {
        sub: a_expr,
        sup: b_expr,
    }];

    match vm.still_holds_after(&internal.concepts, &added, None) {
        Verdict::Violated { violations, .. } => {
            assert_eq!(violations.len(), 1);
            assert_eq!(violations[0].axiom_index, 0);
        }
        other => panic!("a delta that changes the classification must be Violated: {other:?}"),
    }
}

#[test]
fn delta_with_an_unhandled_form_is_unresolved_never_verified() {
    let internal = common::load(ROLE_ONLY_FIXTURE);
    let (vm, _) = verified_model(&internal);

    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    // FunctionalRole has no evaluator at all (eval.rs's doc: 12 variants with
    // no evaluator planned) — must never be silently accepted as holding.
    let added = vec![Axiom::FunctionalRole(Role::named(p))];

    match vm.still_holds_after(&internal.concepts, &added, None) {
        Verdict::Unresolved { reasons, .. } => {
            assert!(
                reasons.contains(&UnresolvedReason::UnhandledAxiom {
                    axiom_index: 0,
                    variant: "FunctionalRole",
                }),
                "{reasons:?}"
            );
        }
        other @ Verdict::Verified { .. } => {
            panic!("an unhandled axiom form must never read as Verified: {other:?}")
        }
        other => panic!("expected Unresolved: {other:?}"),
    }
}

#[test]
fn delta_naming_a_fresh_role_does_not_panic() {
    // RoleHierarchy::{super,sub}_roles panic out of range, and "the edit
    // introduces a role" is the normal case for this API — `hierarchy_sub_
    // roles` (model.rs) is what must guard this, so pin the guard here
    // rather than trusting it stays correct under refactoring.
    let internal = common::load(ROLE_ONLY_FIXTURE);
    let (vm, domain_size) = verified_model(&internal);

    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let num_roles = u32::try_from(internal.vocabulary.num_roles()).expect("small role count");
    let fresh_role = RoleId::new(num_roles + 41);
    let added = vec![Axiom::SubObjectPropertyOf {
        sub: SubRolePath::Role(Role::named(fresh_role)),
        sup: Role::named(p),
    }];

    // Must not panic. A role with no edges in the (unaugmented) hierarchy
    // reads as an empty extension, so this holds vacuously.
    match vm.still_holds_after(&internal.concepts, &added, None) {
        Verdict::Verified {
            axioms_checked,
            domain_size: reported_domain,
        } => {
            assert_eq!(axioms_checked, 1);
            assert_eq!(reported_domain, domain_size);
        }
        other => panic!("a fresh role with no edges must hold vacuously: {other:?}"),
    }
}

#[test]
fn empty_delta_is_verified() {
    let internal = common::load(SUBCLASS_FIXTURE);
    let (vm, domain_size) = verified_model(&internal);

    match vm.still_holds_after(&internal.concepts, &[], None) {
        Verdict::Verified {
            axioms_checked,
            domain_size: reported_domain,
        } => {
            assert_eq!(axioms_checked, 0);
            assert_eq!(reported_domain, domain_size);
        }
        other => panic!("an empty delta must verify: {other:?}"),
    }
}

#[test]
fn added_subobjectpropertyof_the_old_model_does_not_satisfy_is_violated() {
    // `eval::check_axiom`'s doc argues `SubObjectPropertyOf(Role)` is true by
    // construction when checking a FRESHLY BUILT model against its own
    // ontology — because `build_role_hierarchy` already folded the axiom
    // under test into the very hierarchy the check reads back. That argument
    // does not apply here: `still_holds_after` checks the ADDED axiom
    // against the EXISTING, unchanged hierarchy, which has never heard of
    // `p ⊑ q`. This is the sharpest evidence the incremental path does real
    // (non-vacuous) work, not just replay a foregone conclusion.
    let internal = common::load(TWO_ROLES_FIXTURE);
    let (vm, _) = verified_model(&internal);

    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let q = internal.vocabulary.role_id("http://ex.org/q").expect("q");
    let added = vec![Axiom::SubObjectPropertyOf {
        sub: SubRolePath::Role(Role::named(p)),
        sup: Role::named(q),
    }];

    match vm.still_holds_after(&internal.concepts, &added, None) {
        Verdict::Violated { violations, .. } => {
            assert_eq!(violations.len(), 1);
        }
        other => panic!(
            "the old model has a p-edge with no matching q-edge, so the \
             added p ⊑ q must be Violated: {other:?}"
        ),
    }
}
