//! #145 — an `InverseObjectProperties` axiom must be read WITH its polarity.
//!
//! The axiom asserts `a ≡ b⁻`. Two consumers used to discard polarity and compare
//! role ids alone — `collect_inverse_pairs` (which feeds the tableau's
//! `declare_inverse_pair`) and the `mark_symmetric` arm of `build_role_hierarchy`.
//! Read that way:
//!
//! | axiom | actually asserts | old reading |
//! |---|---|---|
//! | `InverseObjectProperties(p, p)` | `p ≡ p⁻` — symmetric | symmetric ✔ |
//! | `InverseObjectProperties(p, ObjectInverseOf(p))` | `p ≡ (p⁻)⁻` — **tautology** | symmetric ✘ |
//! | `InverseObjectProperties(p, ObjectInverseOf(q))` | `p ≡ q` — role **equivalence** | `p ≡ q⁻` ✘ |
//!
//! The second row invents a symmetry nobody declared, which is a false-POSITIVE
//! source — and a widening one since #133, because `role_matches` now walks chain
//! legs backwards on a symmetry record. Both sites now require matching polarity.
//!
//! # Why this file mostly asserts a PARSER fact
//!
//! The hazard is **unreachable today**, and the only thing making it unreachable is
//! that horned-owl refuses `ObjectInverseOf` inside `InverseObjectProperties`. Both
//! Konclude and `HermiT` accept the construct, so this is a rustdl-side parser gap,
//! not invalid OWL — i.e. the protection is an accident of a dependency, not a
//! property of the reasoner.
//!
//! That is exactly the kind of protection that evaporates silently: the horned-owl
//! 1.4 → 3.0 upgrade (#126) could have accepted it incidentally and turned a latent
//! FP live with nobody touching the reasoner. It did not — verified — but the next
//! bump might.
//!
//! So this file pins the parser behaviour as the *stated reason* we are safe. If a
//! future parser accepts the construct, `the_parser_still_rejects_inverse_expressions`
//! fails and points whoever sees it at the guards, rather than the protection
//! disappearing unnoticed. The guards themselves are written correctly regardless,
//! so the flip would be a documentation event, not an incident.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn try_parse(ofn: &str) -> Result<SetOntology<RcStr>, String> {
    read_ofn(
        &mut Cursor::new(ofn.to_string()),
        ParserConfiguration::default(),
    )
    .map(|(o, _)| o)
    .map_err(|e| e.to_string())
}

/// `InverseObjectProperties(:p ObjectInverseOf(:p))` — the tautology that the old
/// id-only reading would have recorded as a symmetry declaration.
const TAUTOLOGY: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/g>\n\
Declaration(ObjectProperty(:p)) Declaration(Class(:A)) Declaration(Class(:B))\n\
InverseObjectProperties(:p ObjectInverseOf(:p))\n\
SubClassOf(:A ObjectSomeValuesFrom(:p :B))\n)\n";

#[test]
fn the_parser_still_rejects_inverse_expressions_in_inverse_object_properties() {
    // THE TRIPWIRE. This asserts a horned-owl behaviour, deliberately, because that
    // behaviour is the only reason #145's hazard is unreachable. If it ever flips:
    //
    //   1. Do NOT just delete this test. Check `collect_inverse_pairs` and the
    //      `mark_symmetric` arm of `build_role_hierarchy` still require matching
    //      polarity (they do as of #145 — this is a belt-and-braces tripwire).
    //   2. Then replace this with the real behavioural assertions: the tautology
    //      must NOT make `p` symmetric, and a mixed-polarity pair must NOT be
    //      recorded as an inverse pair.
    //
    // Both Konclude and `HermiT` accept this construct, so a future horned-owl
    // accepting it would be a FIX on their side, not a regression.
    let err = try_parse(TAUTOLOGY)
        .expect_err("if this now PARSES, read the comment above — the guards matter now");
    assert!(
        err.contains("Parsing Error") || err.contains("ObjectInverseOf") || !err.is_empty(),
        "expected a parse rejection naming the position, got: {err}"
    );
}

#[test]
fn a_named_self_inverse_pair_is_still_symmetry() {
    // CONTROL for the guard change, and the case that must keep working:
    // `InverseObjectProperties(p, p)` — matching (named) polarity, same id — really
    // does assert `p ≡ p⁻`, i.e. symmetry. The #145 guard tightened from "ids match"
    // to "ids match AND polarity matches", so this row must be unaffected.
    //
    // Uses the same chain shape as the symmetry tests: with `p` symmetric and
    // `p ∘ q ⊑ q`, `∃p.Y ⊓ ∃q.Z ⊑ ∃p.(Y ⊓ ∃q.Z)`.
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/g2>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Y)) Declaration(Class(:Z))\n\
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
InverseObjectProperties(:p :p)\n\
SubObjectPropertyOf(ObjectPropertyChain(:p :q) :q)\n\
EquivalentClasses(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(:p :Y) ObjectSomeValuesFrom(:q :Z)))\n\
EquivalentClasses(:B ObjectSomeValuesFrom(:p ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:q :Z))))\n)\n";
    let onto = try_parse(ofn).expect("named self-inverse pair must parse");
    assert!(
        owl_dl_reasoner::is_subclass_of(&onto, "http://ex.org/A", "http://ex.org/B")
            .expect("subsumption query"),
        "InverseObjectProperties(p, p) asserts p ≡ p⁻ — symmetry — and must still be \
         honoured after the #145 polarity tightening"
    );
}

#[test]
fn a_distinct_named_inverse_pair_is_still_an_inverse_pair() {
    // CONTROL for the other direction of the same guard: two DIFFERENT roles at
    // matching polarity is an ordinary inverse declaration and must keep working.
    // `s(x,y)` is `co(y,x)`, so `co ∘ p ⊑ p` fires at the `Y` witness.
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/g3>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Y)) Declaration(Class(:Pn))\n\
Declaration(ObjectProperty(:co)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:p))\n\
InverseObjectProperties(:co :s)\n\
SubObjectPropertyOf(ObjectPropertyChain(:co :p) :p)\n\
EquivalentClasses(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(:s :Y) ObjectSomeValuesFrom(:p :Pn)))\n\
EquivalentClasses(:B ObjectSomeValuesFrom(:s ObjectIntersectionOf(\
:Y ObjectSomeValuesFrom(:p :Pn))))\n)\n";
    let onto = try_parse(ofn).expect("distinct named inverse pair must parse");
    assert!(
        owl_dl_reasoner::is_subclass_of(&onto, "http://ex.org/A", "http://ex.org/B")
            .expect("subsumption query"),
        "InverseObjectProperties(co, s) with matching polarity is an ordinary inverse \
         declaration and must survive the #145 tightening"
    );
}
