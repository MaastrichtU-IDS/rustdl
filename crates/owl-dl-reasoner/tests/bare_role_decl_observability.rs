//! `RUSTDL_FRAGMENT_BARE_DECL` (default ON) lets a `SymmetricObjectProperty` or
//! `InverseObjectProperties` **declaration** stay on the saturation fast path when the
//! role's edge set is *provably unread*. This file pins the "provably" part.
//!
//! # Why it needs a canary
//!
//! The saturator never reads `Axiom::SymmetricRole` or
//! `Axiom::InverseObjectProperties` — `grep` finds no match in
//! `crates/owl-dl-saturation`. So admitting one of these axioms means **dropping its
//! semantics** while reporting a complete answer, which is sound only if the role's
//! edges genuinely cannot be observed. That is the D10 bug class if the "unread"
//! judgement is ever wrong, and the flag is default-ON and changed dispatch for **44
//! ORE ontologies**, so the judgement carries weight.
//!
//! Before this file, the only tests touching the flag pinned it *off* for unrelated
//! reasons (`label_heuristic_canary`, `label_cache_total_budget`,
//! `snapshot_phase0_canary`); nothing exercised the analysis itself.
//!
//! # The subtle case
//!
//! `BareRoleDecls::analyze` marks a role *observable* when it appears in a concept
//! (`∃r`, `∀r`, `≥n r`, `≤n r`, `Self(r)`), in a role chain, in domain/range, in a
//! role characteristic, or in an `ABox` assertion — and then applies a **downward
//! closure**: `r ⊑ s` with observable `s` makes `r` observable, because `r`-edges are
//! `s`-edges and `s` is read.
//!
//! That closure is the load-bearing step. A symmetric role used **nowhere directly**
//! can still be observable purely through a super-role, and missing it would drop
//! symmetry on an ontology certified complete.

use owl_dl_reasoner::classify;

fn mode_is_pure_el(src: &str) -> bool {
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut std::io::Cursor::new(src.to_string()),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse");
    classify(&onto).expect("classify").stats().pure_el_mode
}

/// `r` is symmetric and appears in NO concept — but `r ⊑ s` and `∃s` is read, so
/// `r`-edges are observable through `s`. Dropping `r`'s symmetry would be a silent
/// completeness loss on a fast-path (complete-certified) answer.
///
/// **This is the test that fails if the downward closure in `BareRoleDecls::analyze`
/// is removed.**
#[test]
fn symmetric_role_observable_only_via_a_super_role_leaves_the_fast_path() {
    let src = "\
Prefix(:=<http://t/>)\n\
Ontology(<http://t/ind>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
SymmetricObjectProperty(:r)\n\
SubObjectPropertyOf(:r :s)\n\
SubClassOf(:A ObjectSomeValuesFrom(:s :B))\n\
SubClassOf(:B :C)\n\
)";
    assert!(
        !mode_is_pure_el(src),
        "`r` is symmetric and r ⊑ s with ∃s read, so r's edges ARE observable and the \
         saturator (which never reads SymmetricRole) must not be trusted with this \
         ontology. Reaching the fast path here means symmetry was dropped while the \
         result was reported complete — the D10 bug class."
    );
}

/// The control that stops the test above from being vacuous: the same ontology with
/// the sub-role edge removed, so `r` really is unread and the declaration really is
/// inert. This one MUST stay on the fast path — otherwise the flag is doing nothing
/// and its 44 recoveries would be lost.
#[test]
fn genuinely_unread_symmetric_role_stays_on_the_fast_path() {
    let src = "\
Prefix(:=<http://t/>)\n\
Ontology(<http://t/inert>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
SymmetricObjectProperty(:r)\n\
SubClassOf(:A ObjectSomeValuesFrom(:s :B))\n\
SubClassOf(:B :C)\n\
)";
    assert!(
        mode_is_pure_el(src),
        "`r` appears in no concept, no chain, no characteristic and no assertion, so the \
         symmetry declaration is inert and this must take the fast path. If it does not, \
         RUSTDL_FRAGMENT_BARE_DECL has stopped working and its 44 ORE recoveries are gone."
    );
}

/// Direct use is the easy case, included so a regression that breaks the simple path
/// is not mistaken for a subtlety in the closure.
#[test]
fn directly_used_symmetric_role_leaves_the_fast_path() {
    let src = "\
Prefix(:=<http://t/>)\n\
Ontology(<http://t/direct>\n\
Declaration(Class(:A)) Declaration(Class(:B))\n\
Declaration(ObjectProperty(:r))\n\
SymmetricObjectProperty(:r)\n\
SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
)";
    assert!(
        !mode_is_pure_el(src),
        "∃r is read, so symmetry of r is observable and the ontology must leave the fast path"
    );
}

/// `InverseObjectProperties` requires BOTH roles unread (`unread(p) && unread(q)`),
/// which is deliberately stricter than necessary. Pinned so a future "optimisation"
/// to a single-sided check is a visible decision rather than a silent relaxation.
#[test]
fn inverse_pair_needs_both_roles_unread() {
    let one_side_read = "\
Prefix(:=<http://t/>)\n\
Ontology(<http://t/inv>\n\
Declaration(Class(:A)) Declaration(Class(:B))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
InverseObjectProperties(:p :q)\n\
SubClassOf(:A ObjectSomeValuesFrom(:p :B))\n\
)";
    assert!(
        !mode_is_pure_el(one_side_read),
        "∃p is read, so the p⁻ = q declaration is not inert and the ontology must leave \
         the fast path even though q itself is unread"
    );
    let neither_read = "\
Prefix(:=<http://t/>)\n\
Ontology(<http://t/inv2>\n\
Declaration(Class(:A)) Declaration(Class(:B))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:s))\n\
InverseObjectProperties(:p :q)\n\
SubClassOf(:A ObjectSomeValuesFrom(:s :B))\n\
)";
    assert!(
        mode_is_pure_el(neither_read),
        "neither p nor q is read anywhere, so the inverse declaration is inert"
    );
}
