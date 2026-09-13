//! #138 — role canonicalisation must not silently drop a sub-role axiom.
//!
//! `build_role_hierarchy` canonicalises roles before recording sub-role edges:
//! given `InverseObjectProperties(f, p)` it rewrites `p` to `f⁻`. `add_sub_role`
//! then records the edge only when the two canonicalised sides agree in polarity —
//! so a plain `SubObjectPropertyOf(p, t)` becomes `f⁻ ⊑ t`, fails that test, and
//! **vanishes without a trace**, while `SubObjectPropertyOf(f, t)` (untouched by
//! the rewrite) survives.
//!
//! The fix records the inclusion AS WRITTEN in addition to the canonical one,
//! whenever the axiom's own two roles already agree in polarity. That is exactly
//! what the ontology asserts, and it is the form the clause set uses — only the
//! hierarchy is canonicalised, so the raw edge is the one the matcher can act on.
//!
//! # The asymmetry is the test
//!
//! With `m ∘ t ⊑ m`, `f ⊑ t`, `p ⊑ t` and `f ≡ p⁻`, both of
//!
//! ```text
//! A ≡ ∃m.(Pn ⊓ ∃f.Dc)        B ≡ ∃m.(Dc ⊓ ∃p.Pn)
//! ```
//!
//! subsume each other: `f(a,d)` is `p(d,a)`, and whichever of the two roles carries
//! the edge, `⊑ t` lets the chain re-root `m` on the other endpoint. `HermiT` agrees.
//!
//! Before the fix rustdl derived **one direction only** — and `which` direction was
//! decided by which role the `InverseOf` was written on. That is the discriminator
//! this file is built around: `mirrored_declaration_loses_the_mirror_image_direction`
//! pins that swapping the declaration must NOT swap which entailment is found. A
//! test asserting only one ontology's two directions would pass on a fix that merely
//! moved the bug to the other role.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(ofn: &str) -> SetOntology<RcStr> {
    let (o, _) = read_ofn(
        &mut Cursor::new(ofn.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    o
}

fn holds(ofn: &str, sub: &str, sup: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(
        &parse(ofn),
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
    .expect("subsumption query")
}

/// The #138 shape. `inverse_on` selects WHICH role carries the `InverseOf`
/// declaration — the axiom means the same thing either way (`f ≡ p⁻` ⟺ `p ≡ f⁻`),
/// which is what makes it a control rather than a second fixture.
fn shape(inverse_on_f: bool) -> String {
    let inv = if inverse_on_f {
        "InverseObjectProperties(:f :p)"
    } else {
        "InverseObjectProperties(:p :f)"
    };
    format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/c>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Pn)) Declaration(Class(:Dc))\n\
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:f))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:m))\n\
{inv}\n\
SubObjectPropertyOf(:f :t)\n\
SubObjectPropertyOf(:p :t)\n\
SubObjectPropertyOf(ObjectPropertyChain(:m :t) :m)\n\
EquivalentClasses(:A ObjectSomeValuesFrom(:m ObjectIntersectionOf(\
:Pn ObjectSomeValuesFrom(:f :Dc))))\n\
EquivalentClasses(:B ObjectSomeValuesFrom(:m ObjectIntersectionOf(\
:Dc ObjectSomeValuesFrom(:p :Pn))))\n)\n"
    )
}

#[test]
fn both_directions_hold_with_the_inverse_declared_on_f() {
    let ofn = shape(true);
    assert!(
        holds(&ofn, "A", "B"),
        "A ⊑ B is entailed: f(a,d) gives t(a,d) via f ⊑ t, so m ∘ t ⊑ m re-roots m"
    );
    assert!(
        holds(&ofn, "B", "A"),
        "B ⊑ A is entailed by the mirror argument via p ⊑ t — this is the direction \
         #138 lost, because canonicalisation rewrote p to f⁻ and the sub-role edge \
         was then dropped for polarity mismatch"
    );
}

#[test]
fn mirrored_declaration_loses_the_mirror_image_direction() {
    // THE DISCRIMINATOR. `InverseObjectProperties(p, f)` asserts exactly what
    // `InverseObjectProperties(f, p)` does, so both ontologies must give the same
    // two answers. Before the fix each derived one direction and MISSED the other,
    // with the missing one determined by which role was written first — so a fix
    // that merely moved the rewrite to the other role would still pass a test that
    // only checked one spelling.
    let on_f = shape(true);
    let on_p = shape(false);
    assert_eq!(
        (holds(&on_f, "A", "B"), holds(&on_f, "B", "A")),
        (holds(&on_p, "A", "B"), holds(&on_p, "B", "A")),
        "InverseObjectProperties(f, p) and InverseObjectProperties(p, f) assert the \
         same thing; which one is written must not decide which entailment is found"
    );
    assert!(
        holds(&on_p, "A", "B") && holds(&on_p, "B", "A"),
        "and both directions must actually hold, not merely agree"
    );
}

#[test]
fn the_chain_is_still_required() {
    // FP GUARD. Drop the chain axiom and neither direction is entailed: without
    // `m ∘ t ⊑ m` there is nothing to re-root `m` on the far endpoint, whatever the
    // sub-role edges say. A fix that recorded spurious inclusions would derive it.
    let ofn = shape(true).replace("SubObjectPropertyOf(ObjectPropertyChain(:m :t) :m)\n", "");
    assert!(
        !holds(&ofn, "A", "B"),
        "A ⊑ B must NOT hold without the chain axiom"
    );
    assert!(
        !holds(&ofn, "B", "A"),
        "B ⊑ A must NOT hold without the chain axiom"
    );
}

#[test]
fn the_subrole_edges_are_still_required() {
    // FP GUARD on the other ingredient: keep the chain, drop `p ⊑ t` and `f ⊑ t`.
    // The chain's second leg wants `t`, nothing supplies it, so neither direction
    // holds. This is what would break if the fix recorded a raw edge that the
    // ontology did not assert.
    let ofn = shape(true)
        .replace("SubObjectPropertyOf(:f :t)\n", "")
        .replace("SubObjectPropertyOf(:p :t)\n", "");
    assert!(
        !holds(&ofn, "A", "B"),
        "A ⊑ B must NOT hold without the sub-role edges feeding the chain's t leg"
    );
    assert!(
        !holds(&ofn, "B", "A"),
        "B ⊑ A must NOT hold without the sub-role edges feeding the chain's t leg"
    );
}
