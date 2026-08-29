//! Issue #81 — `ObjectPropertyRange` is not folded into the witness of an
//! existential head whose LHS is a CONJUNCTION.
//!
//! `Range(u,G)` means every `u`-successor is in `G`. The EL saturator folds
//! that into the witness class at lowering time via `atomic_existential_rhs`,
//! which passes `effective_ranges[role]` down as `extras`. But
//! `atomic_existential_rhs` is reached only from the `ConceptExpr::Atomic` LHS
//! arm; the `ConceptExpr::And` LHS arm lowers its existential head with plain
//! `atomic_or_tseitin_body`, which takes NO extras. So
//! `And(..) ⊑ ∃u.W` produces a witness carrying `W` but not `G`, and any
//! downstream trigger needing `W ⊓ G` never fires.
//!
//! Konclude derives the entailment. rustdl reported `incomplete: false` —
//! the D10 failure shape.

use owl_dl_reasoner::classify;

fn entails(body: &str, sub: &str, sup: &str) -> bool {
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    let c = classify(&onto).expect("classify");
    c.is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

const DECLS: &str = "Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:F))
     Declaration(Class(:G)) Declaration(Class(:W)) Declaration(Class(:Z)) Declaration(Class(:P))
     Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:u))
     ObjectPropertyRange(:u :G)
     SubClassOf(ObjectIntersectionOf(:W :G) :Z)
     SubClassOf(ObjectSomeValuesFrom(:u :Z) :P)";

/// The issue-#81 `cascade.ofn` mechanism, minimised: the existential head sits
/// under a CONJUNCTIVE left-hand side, so its witness must still carry
/// `Range(u) = G`.
#[test]
fn conjunctive_lhs_existential_head_folds_the_role_range() {
    assert!(
        entails(
            &format!(
                "{DECLS}
                 SubClassOf(:X ObjectSomeValuesFrom(:s :Y))
                 SubClassOf(:X :F)
                 SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:s :Y) :F) \
                   ObjectSomeValuesFrom(:u :W))"
            ),
            "X",
            "P"
        ),
        "X ⊑ ∃u.W and Range(u,G) give a witness in W ⊓ G ⊑ Z, so X ⊑ ∃u.Z ⊑ P"
    );
}

/// Control: the SAME chain with an ATOMIC left-hand side already works, which
/// is what localises the defect to the `And` arm rather than to range folding
/// in general. A regression here means the fix broke the working path.
#[test]
fn atomic_lhs_existential_head_folds_the_role_range() {
    assert!(
        entails(
            &format!("{DECLS} SubClassOf(:X ObjectSomeValuesFrom(:u :W))"),
            "X",
            "P"
        ),
        "atomic-LHS range folding is the pre-existing working path"
    );
}

/// The conjunctive-LHS head may itself be a CONJUNCTION of existentials; that
/// sub-arm has the same missing-extras defect.
#[test]
fn conjunctive_lhs_conjunctive_head_folds_the_role_range() {
    assert!(
        entails(
            &format!(
                "{DECLS} Declaration(Class(:K))
                 SubClassOf(:X ObjectSomeValuesFrom(:s :Y))
                 SubClassOf(:X :F)
                 SubClassOf(ObjectIntersectionOf(ObjectSomeValuesFrom(:s :Y) :F) \
                   ObjectIntersectionOf(:K ObjectSomeValuesFrom(:u :W)))"
            ),
            "X",
            "P"
        ),
        "the And-head sub-arm must fold Range(u,G) into the u-witness too"
    );
}

/// The same defect ONE LEVEL DOWN: `atomic_or_tseitin_body_with_extras`
/// lowers a nested `∃u.W` body to a marker `M ≡ ∃u.W` whose fact targets a
/// bare `W`, because that function never receives `effective_ranges` and so
/// cannot fold the INNER role's own range. Fixing only the outer head leaves
/// this reachable, and it is the same issue-#81 shape.
#[test]
fn nested_inner_role_range_is_folded_into_the_inner_witness() {
    assert!(
        entails(
            "Declaration(Class(:X)) Declaration(Class(:W)) Declaration(Class(:G))
             Declaration(Class(:Z)) Declaration(Class(:P)) Declaration(Class(:FINAL))
             Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:u))
             ObjectPropertyRange(:u :G)
             SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:u :W)))
             SubClassOf(ObjectIntersectionOf(:W :G) :Z)
             SubClassOf(ObjectSomeValuesFrom(:u :Z) :P)
             SubClassOf(ObjectSomeValuesFrom(:r :P) :FINAL)",
            "X",
            "FINAL"
        ),
        "the nested ∃u.W witness must carry Range(u,G), so W ⊓ G ⊑ Z fires"
    );
}
