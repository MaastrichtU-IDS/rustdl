//! Issue #110 — a CONJUNCTIVE `ObjectPropertyDomain`/`Range` filler is dropped.
//!
//! `collect_el_rules`' Domain/Range arms handled `Bot` (poison) and `Atomic`
//! (push) and fell through silently on `And`, so `Domain(r, P ⊓ Q)` reached the
//! engine as nothing at all. `is_el_axiom` correctly refused the axiom, routing
//! the ontology to the hybrid path — where the tier walk then never compared
//! `X` against `P`, because dropping the filler left `X` with no EL subsumer and
//! therefore in `P`'s own tier. `classify` returned ZERO rows with
//! `incomplete: false`; Konclude, `HermiT` and KM all derive the pairs.
//!
//! `Domain(r, P ⊓ Q) ≡ Domain(r,P) ∧ Domain(r,Q)` is a logical identity, so the
//! fix decomposes rather than approximates.

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

const DECLS: &str = "Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:X))
     Declaration(Class(:B)) Declaration(Class(:W)) Declaration(Class(:S))
     Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))";

/// #110, domain half. Both conjuncts must be derived, not just the first.
#[test]
fn conjunctive_domain_filler_derives_every_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))"
    );
    assert!(entails(&body, "X", "P"), "X ⊑ P (first conjunct)");
    assert!(entails(&body, "X", "Q"), "X ⊑ Q (second conjunct)");
}

/// #110, range half — the LARGER population (13 of the 14 ORE candidates carry a
/// conjunctive Range against 4 for Domain).
///
/// NB the existential filler is `:B`, NOT `owl:Thing`. `∃r.⊤` lowers to a
/// deliberately subsumer-less ⊤-witness that `Range` is not folded into — the
/// documented `topwitness.ofn` DESIGN DECISION — so a `⊤` fixture fails on the
/// atomic control too and cannot discriminate.
#[test]
fn conjunctive_range_filler_derives_every_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyRange(:r ObjectIntersectionOf(:P :Q))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :W)
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :Q)) :S)"
    );
    // BOTH conjuncts, or a sabotage that pushes only the lowest-id one passes.
    assert!(
        entails(&body, "X", "W"),
        "X ⊑ W via the P side of the folded range"
    );
    assert!(
        entails(&body, "X", "S"),
        "X ⊑ S via the Q side of the folded range"
    );
}

/// CONTROL that must keep passing: the atomic filler was never broken.
#[test]
fn atomic_domain_filler_still_derives_its_subsumption() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r :P)"
    );
    assert!(entails(&body, "X", "P"));
}

/// FP GUARD. Decomposition must never invent a domain the axiom does not state:
/// `Domain(r, P)` alone must NOT yield `X ⊑ Q`.
#[test]
fn decomposition_does_not_invent_an_unstated_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r :P)"
    );
    assert!(!entails(&body, "X", "Q"), "Q is not a domain of r");
}

/// PARTIAL decomposition is SOUND: `P ⊓ ∃s.S` yields the atomic `P`, which is a
/// WEAKER (larger) domain than the axiom states — the safe direction. Task 2
/// pins that the GATE nonetheless refuses this axiom.
#[test]
fn partially_decomposable_filler_still_derives_its_atomic_conjunct() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :S)))"
    );
    assert!(entails(&body, "X", "P"), "the atomic conjunct is entailed");
}

/// A DISJUNCTIVE filler must NOT decompose. `Domain(r, P ⊔ Q)` does not entail
/// `Domain(r,P)`; deriving `X ⊑ P` from it would be a false POSITIVE, which is
/// this change's failure direction.
#[test]
fn a_disjunctive_filler_does_not_decompose() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectUnionOf(:P :Q))"
    );
    assert!(!entails(&body, "X", "P"), "FP: a disjunct is not a domain");
    assert!(!entails(&body, "X", "Q"), "FP: a disjunct is not a domain");
}

/// Step 8 (provenance mirrors): `prove` must attribute the conjunctive-domain
/// subsumption to the `ObjectPropertyDomain` axiom, not return an empty
/// `axiom_refs` — the `domain_axiom_refs` mirror (`collect_el_rules_with_
/// provenance`'s own Domain arm) has to decompose the filler exactly as the
/// real Pass 1 does, or the `.find((role,dom))` lookup at the `DomainSub`/
/// `DomainFact` record sites never matches a decomposed conjunct.
#[test]
fn prove_attributes_the_conjunctive_domain_subsumption_to_its_axiom() {
    use owl_dl_reasoner::{ElRule, ProofNode, ProveEntailmentResult, prove_entailment};

    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))"
    );
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");

    let result =
        prove_entailment(&onto, "http://ex.org/X", "http://ex.org/P").expect("prove_entailment");
    let ProveEntailmentResult::SaturatorProof(data) = result else {
        panic!("expected a step-level saturator proof for an EL-fragment entailment");
    };

    // Walk the proof tree looking for the Domain-rule node and assert it cites
    // an axiom — a divergent mirror would leave `axiom_refs` empty instead.
    fn find_domain_node(node: &ProofNode) -> Option<&ProofNode> {
        if matches!(node.rule, ElRule::DomainSub | ElRule::DomainFact) {
            return Some(node);
        }
        node.premises.iter().find_map(find_domain_node)
    }
    let domain_node = find_domain_node(&data.root)
        .expect("proof tree must contain a DomainSub/DomainFact step for X ⊑ P");
    assert!(
        !domain_node.axiom_refs.is_empty(),
        "conjunctive domain filler must resolve to the ObjectPropertyDomain axiom, not an empty axiom_refs"
    );
}

/// Step 8 (provenance mirrors), RANGE half. There is no `RangeSub`/`RangeFact`
/// rule — range provenance is never attributed directly (unlike Domain), it only
/// shapes the mini Pass-1 simulation's per-axiom rule-slot delta cursors
/// (`mini_effective`, feeding `lower_sub_class_of`). A divergence there does not
/// merely empty one node's `axiom_refs` — the reviewer's traced failure mode is
/// that it MISALIGNS every later axiom's attribution, so proofs get cited to the
/// WRONG axiom, which looks correct and is worse than a missing proof.
///
/// Empirically (a throwaway probe dumping the proof tree for this exact
/// fixture): reverting *only* the mini range mirror to `Atomic`-only turns the
/// two `ToldSubsumer` leaves that fold `Range(r, P⊓Q)` into X's existential
/// witness (`Sub(synthetic, B)` / `Sub(synthetic, P)`) from `axiom_refs=[AxiomRef(2)]`
/// to `axiom_refs=[]`, while every ancestor of the root stays `[AxiomRef(1)]`
/// unchanged — the corruption is localized exactly where the range fold enters
/// the tree, not at the entailment's own top-level step. So the discriminating
/// check is a `ToldSubsumer` node concluding `Sub(_, P)`, not the root.
#[test]
fn prove_attributes_the_conjunctive_range_folded_subsumer_to_its_axiom() {
    use owl_dl_reasoner::{
        DerivedFact, ElRule, ProofNode, ProveEntailmentResult, prove_entailment,
    };

    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyRange(:r ObjectIntersectionOf(:P :Q))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :P)) :W)
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :Q)) :S)"
    );
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");

    // Look up P's ClassId independently (via the same conversion prove_entailment
    // uses internally) so the test can find the P-typed leaf without guessing IDs.
    let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
    let p_id = internal
        .vocabulary
        .class_id("http://ex.org/P")
        .expect("P declared");

    let result =
        prove_entailment(&onto, "http://ex.org/X", "http://ex.org/W").expect("prove_entailment");
    let ProveEntailmentResult::SaturatorProof(data) = result else {
        panic!("expected a step-level saturator proof for an EL-fragment entailment");
    };

    // Walk the whole tree for the ToldSubsumer leaf that folds Range's P
    // conjunct onto X's existential witness.
    fn find_p_subsumer(node: &ProofNode, p_id: owl_dl_core::ClassId) -> Option<&ProofNode> {
        if node.rule == ElRule::ToldSubsumer
            && matches!(node.conclusion, DerivedFact::Sub(_, sup) if sup == p_id)
        {
            return Some(node);
        }
        node.premises.iter().find_map(|p| find_p_subsumer(p, p_id))
    }
    let p_node = find_p_subsumer(&data.root, p_id)
        .expect("proof tree must contain a ToldSubsumer step folding Range's P conjunct");
    assert!(
        !p_node.axiom_refs.is_empty(),
        "the range-folded P subsumer must resolve to the ObjectPropertyRange axiom, \
         not an empty axiom_refs — a divergent mini range mirror desyncs the \
         per-axiom rule-slot cursor and silently drops this attribution"
    );
}

use owl_dl_core::convert::convert_ontology;
use owl_dl_reasoner::{FragmentClassification as FC, analyze_fragment};

fn fragment_of(body: &str) -> FC {
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    analyze_fragment(&convert_ontology(&onto).expect("convert"))
}

/// The gate must move WITH the engine: a fully-decomposable filler is now
/// processed, so the ontology belongs on the pure-EL fast path.
#[test]
fn a_fully_decomposable_filler_is_admitted_to_pure_el() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q))"
    );
    assert_eq!(fragment_of(&body), FC::PureEl);
}

/// THE LOAD-BEARING NEGATIVE. A partially-decomposable filler leaves `∃s.S`
/// unprocessed by the engine, so admitting it to a complete-certified fragment
/// would be a FRESH D10 — the exact bug class this fix exists to close.
#[test]
fn a_partially_decomposable_filler_is_refused_by_the_gate() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectIntersectionOf(:P ObjectSomeValuesFrom(:s :S)))"
    );
    assert_ne!(fragment_of(&body), FC::PureEl);
}

/// A disjunctive filler is not decomposable and must stay out of the fragment.
#[test]
fn a_disjunctive_filler_is_refused_by_the_gate() {
    let body = format!(
        "{DECLS}
         SubClassOf(:X ObjectSomeValuesFrom(:r :B))
         ObjectPropertyDomain(:r ObjectUnionOf(:P :Q))"
    );
    assert_ne!(fragment_of(&body), FC::PureEl);
}
