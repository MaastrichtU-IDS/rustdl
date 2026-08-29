//! Issue #78 — the wedge reported `Sat` for a subsumed pair.
//!
//! The clausifier names a `¬A` occurring as a DISJUNCT with a fresh class `Q`
//! plus the auxiliary clause `Q ⊓ A → ⊥`. That clause states a UNIVERSAL
//! property of `Q`, but it was emitted on the LOCAL variable. Inside a `∀`
//! body the local variable is the successor `y`, so the clause had no `X` atom
//! and no `Role` atom — nothing for the matcher to anchor it to and no join to
//! bind `y` through. It could never fire, so a branch whose only route to
//! closure was that clash stayed open and the pair came back `Sat`.
//!
//! THE BISECTION IS THE POINT, and each case below is load-bearing:
//!   * disjunction WITHOUT negation  -> no `¬A` naming at all          -> was OK
//!   * negation WITHOUT disjunction  -> `emit_head`'s `Not` arm appends to the
//!     enclosing body (already carrying `Class(C,X)` + `Role(p,X,y)`) -> was OK
//!   * BOTH                          -> the only route to closure is the
//!     unanchored clause                                              -> BROKE
//!
//! All four subsumptions below are confirmed by Konclude.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const T: &str = "http://ex#T";
const S: &str = "http://ex#S";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex#>)\nOntology(\n\
         Declaration(Class(:A)) Declaration(Class(:V)) Declaration(Class(:R))\n\
         Declaration(Class(:Z)) Declaration(Class(:W))\n\
         Declaration(Class(:S)) Declaration(Class(:T))\n\
         Declaration(ObjectProperty(:p))\n{body}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// `classify` must contain `T ⊑ S`. This is the assertion that failed: the
/// per-pair `subclass` path always answered correctly, so only the classify
/// hierarchy exposes the wedge's wrong `Sat`.
fn classify_has_t_sub_s(body: &str) -> bool {
    let o = onto(body);
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        owl_dl_reasoner::is_subclass_of(&o, T, S).unwrap(),
        "fixture is wrong: the per-pair oracle must agree the subsumption holds"
    );
    c.is_subclass(T, S)
}

/// THE BUG. `T`'s only axiom IS `S`'s definition, so `T ⊑ S` is immediate —
/// and the wedge answered `Sat`. `¬S` expands to `∃p.(R ⊓ Z)`, and the branch
/// from `T`'s own `∀` can only close through the unanchored clash clause.
#[test]
fn disjunctive_forall_with_negated_atomics_is_classified() {
    assert!(classify_has_t_sub_s(
        "EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(:R) ObjectComplementOf(:Z))))\n\
         SubClassOf(:T ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(:R) ObjectComplementOf(:Z))))"
    ));
}

/// The original issue-#66 shape: one negated atomic, and the entailment needs
/// `A ⊑ V` to relate the positive disjuncts.
#[test]
fn disjunctive_forall_mixed_polarity_is_classified() {
    assert!(classify_has_t_sub_s(
        "SubClassOf(:A :V)\n\
         EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectUnionOf(:V ObjectComplementOf(:R))))\n\
         SubClassOf(:T ObjectAllValuesFrom(:p ObjectUnionOf(:A ObjectComplementOf(:R))))"
    ));
}

/// CONTROL — disjunction, no negation. Worked before the fix; must still work.
/// Guards against a "fix" that merely disabled the naming path.
#[test]
fn disjunctive_forall_without_negation_still_classified() {
    assert!(classify_has_t_sub_s(
        "SubClassOf(:A :V)\n\
         EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectUnionOf(:V :Z)))\n\
         SubClassOf(:T ObjectAllValuesFrom(:p ObjectUnionOf(:A :Z)))"
    ));
}

/// CONTROL — negation, no disjunction. Takes `emit_head`'s `Not` arm, which
/// was always anchored. Worked before; must still work.
#[test]
fn single_negated_forall_still_classified() {
    assert!(classify_has_t_sub_s(
        "EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectComplementOf(:R)))\n\
         SubClassOf(:T ObjectAllValuesFrom(:p ObjectComplementOf(:R)))"
    ));
}

/// NEGATIVE CONTROL — a non-entailment must stay non-entailed. The fix makes
/// the wedge derive MORE, so its risk direction is a false POSITIVE; without
/// this, every test above would pass on an engine that simply asserted
/// everything.
#[test]
fn unrelated_forall_bodies_are_not_subsumed() {
    let o = onto(
        "EquivalentClasses(:S ObjectAllValuesFrom(:p ObjectUnionOf(ObjectComplementOf(:R) ObjectComplementOf(:Z))))\n\
         SubClassOf(:T ObjectAllValuesFrom(:p ObjectUnionOf(:V :W)))",
    );
    assert!(
        !owl_dl_reasoner::is_subclass_of(&o, T, S).unwrap(),
        "unrelated ∀ bodies must NOT subsume"
    );
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        !c.is_subclass(T, S),
        "classify must not invent the subsumption"
    );
}
