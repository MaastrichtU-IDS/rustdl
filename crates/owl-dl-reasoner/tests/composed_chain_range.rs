//! #108 — a range reached only through a COMPOSED chain must not be certified
//! saturator-complete.
//!
//! `t∘u ⊑ r`, `r∘v ⊑ s`, `Range(s, F)`: the `t∘u∘v` path is an `s`-edge, so its
//! endpoint is an `F`. The saturator's #84 range fold is keyed on the ordered pairs
//! chain axioms DECLARE — it sees `(t,u)` and `(r,v)` and never forms `(t,u,v) ⊑ s`,
//! so the entailment is absent from the closure. Before this fix the fragment gate
//! nevertheless certified the closure complete (`pure-EL`), so every path trusted it:
//! wrong answer, `incomplete: false` — the D10 shape.
//!
//! The fix is a SET-level gate rejection (`has_composed_chain_range`), not a saturator
//! fix: the tableau proves these shapes (verified via `sat-expr` before writing this),
//! so de-certifying routes the pair to an engine that gets it right. The predicate was
//! mapped empirically first — single chains, SELF-recursive chains, composed+DOMAIN,
//! and composed supers in ∃-LHS positions are all complete in the saturator and MUST
//! stay on the fast path (ORE census: naive "any composition" would de-certify 16% of
//! the pool including corpus-verified `ro`; the narrowed predicate hits 2.2%).
//!
//! # Mutation guard
//!
//! The positives below hold ONLY because the gate rejects: stub
//! `has_composed_chain_range` to `false` and the orchestrator trusts the saturator's
//! closure again, flipping `composed_chain_range_is_derived` to `no`. Verified.
//!
//! # What this file does NOT cover
//!
//! Default `classify` on these ontologies still omits `C ⊑ D` — that residual is
//! #160's same-tier gap (`RUSTDL_CLASSIFY_SAME_TIER=1` recovers it; verified), a
//! separate, already-tracked orchestration issue. The fixture
//! `owl-dl-verify/tests/fixtures/chaincompose.ofn` is a minimal reproducer for it.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn holds(body: &str, sub: &str, sup: &str) -> bool {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/p>\n\
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:F))\n\
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u)) Declaration(ObjectProperty(:v))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:w))\n\
{body}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse");
    owl_dl_reasoner::is_subclass_of(
        &o,
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
    .expect("query")
}

const COMPOSED: &str = "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
SubObjectPropertyOf(ObjectPropertyChain(:r :v) :s)\n\
ObjectPropertyRange(:s :F)\n\
SubClassOf(:F :D)\n\
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u ObjectSomeValuesFrom(:v :A))))\n\
SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u ObjectSomeValuesFrom(:v :D))) :D)";

#[test]
fn composed_chain_range_is_derived() {
    // THE #108 SHAPE. Fails (`no`) if the gate stops rejecting — the saturator's
    // closure lacks the entailment and would be trusted again.
    assert!(
        holds(COMPOSED, "C", "D"),
        "C ⊑ D via t∘u∘v ⇒ s-edge ⇒ Range(s,F) ⇒ F ⊑ D — the #108 entailment"
    );
}

#[test]
fn inherited_range_on_a_composed_super_is_derived() {
    // The P7 shape: the range sits on `w` with `s ⊑ w`. The gate must use
    // EFFECTIVE ranges, or this reproduces #108 one hierarchy hop away.
    let body = COMPOSED.replace(
        "ObjectPropertyRange(:s :F)",
        "SubObjectPropertyOf(:s :w)\nObjectPropertyRange(:w :F)",
    );
    assert!(
        holds(&body, "C", "D"),
        "range inherited via s ⊑ w must also be found"
    );
}

#[test]
fn negative_control_no_range_no_entailment() {
    // FP GUARD: drop the range and C ⊑ D must NOT hold — a gate fix that somehow
    // over-derived would fail here.
    let body = COMPOSED.replace("ObjectPropertyRange(:s :F)\n", "");
    assert!(
        !holds(&body, "C", "D"),
        "without the range there is no entailment"
    );
}

/// The admitted shapes must KEEP working — they are complete in the saturator and
/// the census says over-rejecting them costs 16% of the ORE pool its fast path.
/// These assert the ANSWER stays right; the fast-path routing itself is asserted
/// in the CLI banner test (`composed_chain_fragment.rs`, owl-dl-cli).
#[test]
fn admitted_shapes_still_derive() {
    // single chain + range (#84's case)
    assert!(holds(
        "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
         ObjectPropertyRange(:r :F)\nSubClassOf(:F :D)\n\
         SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))\n\
         SubClassOf(ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :D)) :D)",
        "C",
        "D"
    ));
    // self-recursive chain + range
    assert!(holds(
        "SubObjectPropertyOf(ObjectPropertyChain(:r :v) :r)\n\
         ObjectPropertyRange(:r :F)\nSubClassOf(:F :D)\n\
         SubClassOf(:C ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:v :A)))\n\
         SubClassOf(ObjectSomeValuesFrom(:r :D) :D)",
        "C",
        "D"
    ));
    // composed chains + DOMAIN (domain side is complete; stays admitted)
    assert!(holds(
        "SubObjectPropertyOf(ObjectPropertyChain(:t :u) :r)\n\
         SubObjectPropertyOf(ObjectPropertyChain(:r :v) :s)\n\
         ObjectPropertyDomain(:s :F)\nSubClassOf(:F :D)\n\
         SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u ObjectSomeValuesFrom(:v :A))))",
        "C",
        "D"
    ));
}
