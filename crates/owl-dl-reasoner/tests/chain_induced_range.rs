//! #84 — a role chain's range must reach the witness at the end of the chain.
//!
//! ```text
//! Chain(t, u) ⊑ r,  Range(r, F),  C ⊑ ∃t.∃u.A,  ∃t.∃u.F ⊑ D   ⟹   C ⊑ D
//! ```
//!
//! For `x ∈ C`: `t(x,y)`, `u(y,z)`, `z ∈ A`; the chain gives `r(x,z)`, so the range
//! gives `z ∈ F`, so `y ∈ ∃u.F`, so `x ∈ ∃t.∃u.F ⊑ D`. `HermiT` confirms every
//! positive below. **Konclude misses this shape**, which is why the adjudication went
//! to `HermiT` — see `docs/benchmarks/2026-08-29-chain-induced-range-adjudication.md`.
//!
//! # The negatives are the point of this file
//!
//! The fix folds `Range(r)` into the filler of a NESTED `∃t.∃u.X`, where the `t` step
//! is present by construction. Folding it into `effective_ranges[u]` instead would
//! close the same fixture and be **unsound**: a bare `u`-successor with no
//! `t`-predecessor is not an `r`-successor and must not inherit `Range(r)`. The
//! issue says so explicitly, and `a_bare_inner_successor_does_not_inherit_the_chain_range`
//! is what holds the implementation to it. `HermiT` agrees with both negatives.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

fn holds(ofn: &str, sub: &str, sup: &str) -> bool {
    let mut reader = Cursor::new(ofn.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    // The complete path, so a MISS here is the calculus rather than a trust_sat mask.
    let result = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    result.is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
}

fn subsumptions(_ofn: &str) -> &'static str {
    "(see is_subclass)"
}

const HEAD: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/cr>\n\
Declaration(Class(:A)) Declaration(Class(:F)) Declaration(Class(:C)) Declaration(Class(:D))\n\
Declaration(Class(:X))\n\
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u)) Declaration(ObjectProperty(:r))\n";

#[test]
fn a_chain_range_reaches_the_witness_at_the_end_of_the_chain() {
    let ofn = format!(
        "{HEAD}\
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
ObjectPropertyRange(:r :F)\n\
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))\n\
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)\n)\n"
    );
    assert!(
        holds(&ofn, "C", "D"),
        "C ⊑ D is entailed (HermiT confirms) — the chain gives r(x,z) and Range(r) \
         then types the inner witness F. Got {:?}",
        subsumptions(&ofn)
    );
}

#[test]
fn a_bare_inner_successor_does_not_inherit_the_chain_range() {
    // THE FP GUARD. `X ⊑ ∃u.A` with no `t` step: `X`'s `u`-successor is NOT an
    // `r`-successor, so it must not become an `F`, so `X ⊑ D` must NOT hold.
    // Folding the chain range into `effective_ranges[u]` would derive it. HermiT
    // reports nothing here.
    let ofn = format!(
        "{HEAD}\
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
ObjectPropertyRange(:r :F)\n\
SubClassOf(:X ObjectSomeValuesFrom(:u :A))\n\
SubClassOf(ObjectSomeValuesFrom(:u :F) :D)\n)\n"
    );
    assert!(
        !holds(&ofn, "X", "D"),
        "FALSE POSITIVE: a bare u-successor with no t-predecessor is not an \
         r-successor and must not inherit Range(r). Got {:?}",
        subsumptions(&ofn)
    );
}

#[test]
fn a_nested_pair_whose_outer_role_is_not_the_chain_head_does_not_fire() {
    // **The FP guard that actually exercises the fold.** `∃w.∃u.A` is NESTED, so it
    // DOES reach `nested_extras` — but `w ∘ u` is not a declared chain, so no range
    // may be folded and `X ⊑ D` must not hold.
    //
    // Its sibling `a_bare_inner_successor_does_not_inherit_the_chain_range` uses a
    // TOP-LEVEL `∃u.A`, which `atomic_existential_rhs` handles without ever calling
    // `nested_extras`. That test therefore guards the "put it in
    // `effective_ranges[u]`" implementation the issue warns against, and CANNOT see
    // an over-broad `nested_extras`. Sabotaging the lookup to match any pair whose
    // SECOND leg is `u` left it green; this one fails. Keep both — they cover
    // different implementations of the same mistake.
    let ofn = format!(
        "{HEAD}\
Declaration(ObjectProperty(:w))\n\
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
ObjectPropertyRange(:r :F)\n\
SubClassOf(:X ObjectSomeValuesFrom(:w ObjectSomeValuesFrom(:u :A)))\n\
SubClassOf(ObjectSomeValuesFrom(:w ObjectSomeValuesFrom(:u :F)) :D)\n)\n"
    );
    assert!(
        !holds(&ofn, "X", "D"),
        "FALSE POSITIVE: w ∘ u is not the declared chain t ∘ u, so no r-edge exists \
         and Range(r) must not reach the inner witness."
    );
}

#[test]
fn the_chain_pair_is_ordered_so_the_reverse_nesting_does_not_fire() {
    // `∃u.∃t.A` composes as `u ∘ t`, which is NOT the declared chain. Pins that
    // `chain_ranges` is keyed on the ORDERED pair.
    let ofn = format!(
        "{HEAD}\
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
ObjectPropertyRange(:r :F)\n\
SubClassOf(:X ObjectSomeValuesFrom(:u ObjectSomeValuesFrom(:t :A)))\n\
SubClassOf(ObjectSomeValuesFrom(:u ObjectSomeValuesFrom(:t :F)) :D)\n)\n"
    );
    assert!(
        !holds(&ofn, "X", "D"),
        "FALSE POSITIVE: u ∘ t is not the declared chain t ∘ u. Got {:?}",
        subsumptions(&ofn)
    );
}

#[test]
fn a_range_on_a_super_role_of_the_chain_head_also_reaches_the_witness() {
    // `chain_ranges` reads `effective_ranges[sup]`, already closed over `sup`'s own
    // super-roles, so `Range(q)` with `r ⊑ q` applies. Reading `role_ranges[sup]`
    // instead would miss this. HermiT confirms.
    let ofn = format!(
        "{HEAD}\
Declaration(ObjectProperty(:q))\n\
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
SubObjectPropertyOf(:r :q)\n\
ObjectPropertyRange(:q :F)\n\
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))\n\
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)\n)\n"
    );
    assert!(
        holds(&ofn, "C", "D"),
        "a range on a SUPER-role of the chain head applies to the chain's \
         successors too (HermiT confirms). Got {:?}",
        subsumptions(&ofn)
    );
}

#[test]
fn no_chain_means_no_fold() {
    // The plain control: same shape, chain axiom removed. Nothing should be derived,
    // and this is what shows the chain axiom is load-bearing rather than the range
    // alone doing the work.
    let ofn = format!(
        "{HEAD}\
ObjectPropertyRange(:r :F)\n\
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))\n\
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)\n)\n"
    );
    assert!(
        !holds(&ofn, "C", "D"),
        "without the chain axiom there is no r-edge, so no range applies. Got {:?}",
        subsumptions(&ofn)
    );
}

#[test]
fn the_fix_does_not_disturb_a_plain_nested_range() {
    // #81's fold (a role's OWN range into a nested witness) must keep working —
    // `nested_extras` starts from `range_extras` and only ADDS to it.
    let ofn = format!(
        "{HEAD}\
ObjectPropertyRange(:u :F)\n\
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))\n\
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :F)) :D)\n)\n"
    );
    assert!(
        holds(&ofn, "C", "D"),
        "#81's plain nested-range fold regressed. Got {:?}",
        subsumptions(&ofn)
    );
}
