//! Completeness canary: ObjectHasSelf (`∃R.Self`) + domain/range.
//!
//! The saturator dropped `SelfRestriction(R)` (ObjectHasSelf) entirely. But
//! `X ⊑ ∃R.Self` means X is both the source and target of an R-edge to itself, so
//! `X ⊑ domain(R) ⊓ range(R)`. Without this the whole self-loop → domain/range
//! chain is lost (olia/OBO: cost 43 of ore_ont_4827's self-restriction-driven
//! subsumptions with an atomic domain, e.g. AspectFeature ⊑ ∃hasAspect.Self,
//! domain(hasAspect)=Verb ⟹ AspectFeature ⊑ Verb). Fix routes `X ⊑ ∃R.Self` to
//! `domain(R)`/`range(R)`. Sound under-approximation (the full self-semantics —
//! the successor coincides with X — is otherwise handled by the tableau).
//!
//! (Disjunctive domains — `domain(R) = D1 ⊔ … ⊔ Dn` — are a SEPARATE lever and are
//! not closed by this fix; see ore_ont_4827's residual.)

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const ONT: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/o>
  Declaration(Class(:AspectFeature)) Declaration(Class(:Verb)) Declaration(Class(:Word))
  Declaration(Class(:Animal))
  Declaration(ObjectProperty(:hasAspect))
  SubClassOf(:AspectFeature ObjectHasSelf(:hasAspect))
  ObjectPropertyDomain(:hasAspect :Verb)
  ObjectPropertyRange(:hasAspect :Word)
  SubClassOf(:Verb :Word)
)";

fn load() -> SetOntology<RcStr> {
    let mut r = Cursor::new(ONT.as_bytes().to_vec());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

fn sub(o: &SetOntology<RcStr>, s: &str, t: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(o, &format!("http://t/{s}"), &format!("http://t/{t}"))
        .expect("classify ok")
}

#[test]
fn self_restriction_propagates_domain_and_range() {
    let o = load();
    assert!(
        sub(&o, "AspectFeature", "Verb"),
        "∃hasAspect.Self + domain=Verb ⟹ ⊑ Verb"
    );
    assert!(
        sub(&o, "AspectFeature", "Word"),
        "∃hasAspect.Self + range=Word ⟹ ⊑ Word"
    );
}

#[test]
fn self_restriction_no_spurious_subsumption() {
    // FP guard: Animal has no self-restriction ⟹ not a Verb.
    let o = load();
    assert!(
        !sub(&o, "Animal", "Verb"),
        "Animal ⋢ Verb (no ∃hasAspect.Self)"
    );
}
