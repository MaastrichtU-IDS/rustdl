//! Completeness canary: domain expressed as the GCI `∃R.⊤ ⊑ C`.
//!
//! `∃R.⊤ ⊑ C` is semantically identical to `ObjectPropertyDomain(R, C)` (anything
//! with an R-successor is a C). The saturator handled the native
//! `ObjectPropertyDomain` axiom but SILENTLY DROPPED the GCI form: it lowered to
//! an existential trigger whose body is `Top`, and `atomic_or_tseitin_body(Top)`
//! has no representation, so `existential_body_alternatives(Top)` returned `None`
//! and the trigger — and the entire domain inference — was lost. SWEET / OBO
//! ontologies express domains in this GCI form: it cost 323 (ore_ont_13621) +
//! 329 (ore_ont_14450) silent MISSED vs the Konclude∩HermiT oracle (found via the
//! ORE-2015 sweep, 2026-06-20). Fix: route `∃R.⊤ ⊑ C` to the role-domain
//! mechanism. Sound (the domain rule is already complete).
//!
//!   AirPollution ⊑ ∃hasImpactOn.Atmosphere
//!   ∃hasImpactOn.⊤ ⊑ Impact            (domain, GCI form)
//!   Impact ⊑ OrdinalProperty
//! ⟹ AirPollution ⊑ Impact ⊑ OrdinalProperty (pure EL).

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

const ONT: &str = r"Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://t/o>
  Declaration(Class(:AirPollution)) Declaration(Class(:Atmosphere))
  Declaration(Class(:Impact)) Declaration(Class(:OrdinalProperty))
  Declaration(ObjectProperty(:hasImpactOn))
  SubClassOf(:AirPollution ObjectSomeValuesFrom(:hasImpactOn :Atmosphere))
  SubClassOf(ObjectSomeValuesFrom(:hasImpactOn owl:Thing) :Impact)
  SubClassOf(:Impact :OrdinalProperty)
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
fn domain_gci_propagates_to_subject() {
    let o = load();
    assert!(
        sub(&o, "AirPollution", "Impact"),
        "∃hasImpactOn.⊤ ⊑ Impact (domain GCI) + AirPollution ⊑ ∃hasImpactOn.Atmosphere \
         ⟹ AirPollution ⊑ Impact"
    );
    assert!(
        sub(&o, "AirPollution", "OrdinalProperty"),
        "transitively AirPollution ⊑ Impact ⊑ OrdinalProperty"
    );
}

#[test]
fn domain_gci_no_spurious_subsumption() {
    // FP guard: Atmosphere has no hasImpactOn-successor ⟹ NOT an Impact.
    let o = load();
    assert!(!sub(&o, "Atmosphere", "Impact"), "Atmosphere ⋢ Impact (no R-successor)");
}
