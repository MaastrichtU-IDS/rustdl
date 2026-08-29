//! Issue #80 — nested existential monotonicity on the pure-EL path.
//!
//! `C ⊑ ∃t.∃u.A`, `A ⊑ F`, `∃t.∃u.F ⊑ D` entails `C ⊑ D` by plain EL
//! monotonicity: `A ⊑ F` gives `∃u.A ⊑ ∃u.F`, hence `∃t.∃u.A ⊑ ∃t.∃u.F ⊑ D`.
//! Konclude derives it. rustdl reported only `(A,F)` with `incomplete: false`,
//! i.e. a wrong answer presented as complete — the D10 failure shape.
//!
//! ROOT CAUSE: `atomic_or_tseitin_body_with_extras`'s `Some`/`Min` arms lowered
//! a bare nested existential body via `introduce_existential_marker` (ONE-WAY:
//! emits only `∃S.X ⊑ M`), so the witness `M` had no `S`-edge of its own and
//! nothing about `X` could propagate through it. Its sibling
//! `atomic_classes_with_existential_markers` already used the two-way
//! `introduce_equivalent_existential_marker` for the same shape inside an `And`.
//! The one-way flavour is for LHS-trigger sites; this is an RHS body position.

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

/// The issue-#80 reproducer. Konclude derives both rows.
#[test]
fn nested_existential_body_inherits_its_own_subsumers() {
    assert!(
        entails(
            "Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))
             Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
             SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
             SubClassOf(:A :F)
             SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)",
            "C", "D",
        ),
        "C sub D is entailed by EL monotonicity through the NESTED existential \
         (A sub F implies exists-u.A sub exists-u.F implies exists-t.exists-u.A sub D); \
         Konclude derives it."
    );
}

/// Same bug, `Min` arm: `C ⊑ ∃t.(≥1 u.A)`, `A ⊑ F`, `∃t.(≥1 u.F) ⊑ D` ⟹ `C ⊑ D`,
/// by the same EL-monotonicity argument with the inner existential written as a
/// `≥1` cardinality restriction instead of `ObjectSomeValuesFrom`. Exercises the
/// `ConceptExpr::Min` arm of `atomic_or_tseitin_body_with_extras`, which had the
/// identical one-way-marker bug and got the identical fix.
#[test]
fn nested_min_cardinality_body_inherits_its_own_subsumers() {
    assert!(
        entails(
            "Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))
             Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))
             SubClassOf(:C ObjectSomeValuesFrom(:t ObjectMinCardinality(1 :u :A)))
             SubClassOf(:A :F)
             SubClassOf(ObjectSomeValuesFrom(:t ObjectMinCardinality(1 :u :F)) :D)",
            "C", "D",
        ),
        "C sub D is entailed by EL monotonicity through the NESTED ≥1 cardinality body \
         (A sub F implies >=1 u.A sub >=1 u.F implies exists-t.(>=1 u.A) sub D); \
         Konclude derives it."
    );
}

/// Discriminating control: the ONE-LEVEL form was always handled correctly, so a
/// failure here would mean the fix broke the working case rather than the broken one.
#[test]
fn flat_existential_body_still_inherits_its_own_subsumers() {
    assert!(
        entails(
            "Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))
             Declaration(ObjectProperty(:u))
             SubClassOf(:C ObjectSomeValuesFrom(:u :A))
             SubClassOf(:A :F)
             SubClassOf(ObjectSomeValuesFrom(:u :F) :D)",
            "C", "D",
        ),
        "the flat case is the control and was never broken"
    );
}
