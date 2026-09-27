//! #177 — inclusions through a declared inverse pair (`a ⊑ b`, `Inverse(b, c)`,
//! `c ⊑ d`) were unrepresentable: the role hierarchy was keyed on polarity-blind
//! `RoleId`s and `role_matches` enforced polarity equality, so a range on `d`
//! never reached the source of an `a`-edge even though the edge, read backwards,
//! is a `d`-edge (`a⁻ ⊑ b⁻ ⊑ c ⊑ d`). The SWEET family (NASA JPL) hits this on
//! 72 ORE ontologies — 49% of the corpus's remaining missed entailments — via
//! `temporalPartOf ⊑ subsetOf`, `Inverse(subsetOf, supersetOf)`,
//! `supersetOf ⊑ setRelation`, `Range(setRelation, Set)`.
//!
//! Fixed by the SIGNED closure on `RoleHierarchy` (#177), consumed by
//! `role_matches`, plus the trigger widening over signed subs and the
//! `has_polarity_crossing` dispatch gate. The Layer A affordability cap also
//! moved 32 → 128 (corpus-censused): SWEET's max closure is 65, and at cap 32
//! Layer A never handed the wedge a hierarchy at all — the fix was invisible on
//! the live family until the cap moved, which is pinned here too.

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

fn classify_holds(ofn: &str, sub: &str, sup: &str) -> bool {
    let h = owl_dl_reasoner::classify(&parse(ofn)).expect("classify");
    h.is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

fn ont(body: &str) -> String {
    format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/sh>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u))\n\
{body}\nSubClassOf(:A ObjectSomeValuesFrom(:r :B))\n)\n"
    )
}

const CHAIN: &str = "SubObjectPropertyOf(:r :s)\n\
InverseObjectProperties(:s :t)\n\
SubObjectPropertyOf(:t :u)\n\
ObjectPropertyRange(:u :C)";

#[test]
fn a_range_reaches_the_source_through_a_polarity_crossing_chain() {
    // THE #177 MECHANISM: r(A,w) is s(A,w) is t(w,A) is u(w,A), so A is the
    // TARGET of a u-edge and Range(u, C) applies to A itself.
    assert!(
        classify_holds(&ont(CHAIN), "A", "C"),
        "A ⊑ C via r ⊑ s, s⁻ ≡ t, t ⊑ u, Range(u, C) — the SWEET mechanism"
    );
}

#[test]
fn without_the_inverse_pair_the_range_stays_forward() {
    // FP GUARD 1: drop the inverse pair and the chain has no polarity crossing —
    // Range(u, C) then says nothing about A (only about u-edge TARGETS, and A has
    // no incoming u-edge). Deriving A ⊑ C here would be reading every sub-role
    // edge backwards unconditionally, the unsound reflex this file exists to
    // forbid.
    let body = "SubObjectPropertyOf(:r :s)\n\
                SubObjectPropertyOf(:t :u)\n\
                ObjectPropertyRange(:u :C)";
    assert!(
        !classify_holds(&ont(body), "A", "C"),
        "no inverse pair: the r-edge is one-way and A is not a u-target — FP if derived"
    );
}

#[test]
fn an_unconnected_inverse_pair_licenses_nothing() {
    // FP GUARD 2: the inverse pair exists but t has no super-role carrying the
    // range — the crossing leads nowhere.
    let body = "SubObjectPropertyOf(:r :s)\n\
                InverseObjectProperties(:s :t)\n\
                ObjectPropertyRange(:u :C)";
    assert!(
        !classify_holds(&ont(body), "A", "C"),
        "t ⊑ u was never asserted: the crossing chain is broken — FP if derived"
    );
}

#[test]
fn the_mirror_direction_works_too() {
    // The same chain entered from the OTHER polarity: A ⊑ ∃t.B with t ⊑ u is the
    // forward read (Range(u,C) puts C on the WITNESS); the signed content here is
    // that r's backward read composes — so assert the forward control also holds
    // (w gets C) via the derived subsumption ∃t-witness … observable as B ⊑ C?
    // No: B is the witness's LABEL, not a subclass. Instead pin the DOMAIN dual:
    // Domain(u, C) with the same chain puts C on the u-edge SOURCE = w, and A
    // must NOT gain C (the crossing sends A to the TARGET position only).
    let body = "SubObjectPropertyOf(:r :s)\n\
                InverseObjectProperties(:s :t)\n\
                SubObjectPropertyOf(:t :u)\n\
                ObjectPropertyDomain(:u :C)";
    assert!(
        !classify_holds(&ont(body), "A", "C"),
        "under the crossing, A is the u-edge TARGET; Domain constrains the SOURCE \
         (the witness) — putting C on A would be a polarity bookkeeping FP"
    );
}

#[test]
fn the_live_sweet_spelling_is_pinned() {
    // The real SWEET module axioms (full IRIs — the prefixed spelling with '#'
    // inside a local name is not valid OFN and parses to different identities,
    // which cost this investigation one false negative).
    let s = "http://sweet.jpl.nasa.gov/2.3";
    let ofn = format!(
        "Ontology(<http://ex.org/ms2>\n\
Declaration(Class(<{s}/stateTime.owl#Age>)) Declaration(Class(<{s}/stateTime.owl#Epoch>))\n\
Declaration(Class(<{s}/reprMath.owl#Set>))\n\
Declaration(ObjectProperty(<{s}/relaTime.owl#temporalPartOf>))\n\
Declaration(ObjectProperty(<{s}/relaMath.owl#subsetOf>))\n\
Declaration(ObjectProperty(<{s}/relaMath.owl#supersetOf>))\n\
Declaration(ObjectProperty(<{s}/relaMath.owl#setRelation>))\n\
SubClassOf(<{s}/stateTime.owl#Age> ObjectSomeValuesFrom(<{s}/relaTime.owl#temporalPartOf> <{s}/stateTime.owl#Epoch>))\n\
SubObjectPropertyOf(<{s}/relaTime.owl#temporalPartOf> <{s}/relaMath.owl#subsetOf>)\n\
InverseObjectProperties(<{s}/relaMath.owl#subsetOf> <{s}/relaMath.owl#supersetOf>)\n\
SubObjectPropertyOf(<{s}/relaMath.owl#supersetOf> <{s}/relaMath.owl#setRelation>)\n\
ObjectPropertyRange(<{s}/relaMath.owl#setRelation> <{s}/reprMath.owl#Set>)\n)\n"
    );
    let h = owl_dl_reasoner::classify(&parse(&ofn)).expect("classify");
    assert!(
        h.is_subclass(
            &format!("{s}/stateTime.owl#Age"),
            &format!("{s}/reprMath.owl#Set"),
        ),
        "the live SWEET chain: Age ⊑ Set"
    );
}

#[test]
fn a_second_inverse_pair_on_one_role_still_crosses() {
    // THE `add_inverse_pair` DISCRIMINATOR. `build_inverse_canon` is first-wins:
    // with `Inverse(s, t)` mapped, a second pair `Inverse(t, w)` is SKIPPED
    // (`contains_key(t)`), so no canon rewrite connects `w` to the `s`/`t`
    // family — only the explicit signed pair edges do. Without them, the chain
    // `r ⊑ t`, `Inverse(t, w)`, `w ⊑ u`, `Range(u, C)` (semantically:
    // r(A,x) ⟹ t(A,x) ⟹ w(x,A) ⟹ u(x,A) ⟹ C(A)) is severed at the skipped
    // pair. The plain-chain positives above pass WITHOUT the pair arm (the
    // canon carries them), which is exactly why this fixture exists — the
    // first mutation run proved the suite blind to it.
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/dp>\n\
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:w)) Declaration(ObjectProperty(:u))\n\
SubObjectPropertyOf(:r :t)\n\
InverseObjectProperties(:s :t)\n\
InverseObjectProperties(:t :w)\n\
SubObjectPropertyOf(:w :u)\n\
ObjectPropertyRange(:u :C)\n\
SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n)\n";
    let h = owl_dl_reasoner::classify(&parse(ofn)).expect("classify");
    assert!(
        h.is_subclass("http://ex.org/A", "http://ex.org/C"),
        "the second inverse pair must connect w to the family even though the \
         canon skipped it (#177)"
    );
}
