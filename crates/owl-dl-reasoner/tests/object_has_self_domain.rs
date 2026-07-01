//! Completeness canary: ObjectHasSelf (`∃R.Self`) + domain/range, asserted at the
//! SATURATION-CLOSURE layer (`owl_dl_saturation::saturate`), NOT via
//! `is_subclass_of` — the wedge/tableau closes these entailments regardless, so a
//! full-classifier probe cannot isolate (or guard) the saturator rule.
//!
//! `X ⊑ ∃R.Self` means X is both the source and target of an R-edge to itself, so
//! `X ⊑ domain(R) ⊓ range(R)`. Moreover the self-loop `(x,x) ∈ R` is an `S`-edge
//! for every super-role `R ⊑* S`, so `X ⊑ range(S)` for the *effective*
//! (super-role-closed) range too — this closes the ore_ont_4827 pattern
//! (`ClusivityFeature ⊑ ∃hasClusivity.Self`, `hasClusivity ⊑ hasFeature`,
//! `range(hasFeature)=Feature` ⟹ `ClusivityFeature ⊑ Feature`; 79 silent MISSED →
//! 0, FP=0). Sound: the successor coincides with X, so the range obligation lands
//! on X itself (unlike general range propagation, which is deliberately omitted —
//! there the range target is a distinct existential body).

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const ONT: &str = r"Prefix(:=<http://t/>)
Ontology(<http://t/o>
  Declaration(Class(:AspectFeature)) Declaration(Class(:Verb)) Declaration(Class(:Word))
  Declaration(Class(:Animal)) Declaration(Class(:Feature)) Declaration(Class(:Bogus))
  Declaration(ObjectProperty(:hasAspect)) Declaration(ObjectProperty(:hasFeature))
  Declaration(ObjectProperty(:unrelated))
  SubClassOf(:AspectFeature ObjectHasSelf(:hasAspect))
  ObjectPropertyDomain(:hasAspect :Verb)
  ObjectPropertyRange(:hasAspect :Word)
  SubClassOf(:Verb :Word)
  SubObjectPropertyOf(:hasAspect :hasFeature)
  ObjectPropertyRange(:hasFeature :Feature)
  ObjectPropertyRange(:unrelated :Bogus)
)";

fn load() -> SetOntology<RcStr> {
    let mut r = Cursor::new(ONT.as_bytes().to_vec());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

/// `true` iff the SATURATOR's subsumer closure contains `X ⊑ Y` (isolates the
/// saturation rule from the wedge/tableau).
fn sat_sub(o: &SetOntology<RcStr>, x: &str, y: &str) -> bool {
    let internal = convert_ontology(o).expect("lower to IR");
    let subsumers = owl_dl_saturation::saturate(&internal);
    let xid = internal
        .vocabulary
        .class_id(&format!("http://t/{x}"))
        .expect("x declared");
    let yid = internal
        .vocabulary
        .class_id(&format!("http://t/{y}"))
        .expect("y declared");
    subsumers.contains(xid, yid)
}

#[test]
fn self_restriction_propagates_domain_and_range() {
    let o = load();
    assert!(
        sat_sub(&o, "AspectFeature", "Verb"),
        "∃hasAspect.Self + domain=Verb ⟹ ⊑ Verb (saturation)"
    );
    assert!(
        sat_sub(&o, "AspectFeature", "Word"),
        "∃hasAspect.Self + range=Word ⟹ ⊑ Word (saturation)"
    );
}

#[test]
fn self_restriction_propagates_super_role_range() {
    // ore_ont_4827 pattern: `X ⊑ ∃R.Self`, `R ⊑* S`, `range(S)=C` ⟹ `X ⊑ C`.
    // The self-loop (x,x)∈R ⊆ S puts x in range(S). The direct-range rule only
    // read range(hasAspect); this exercises the super-role range hasFeature and
    // fails at the saturation layer without the `effective_ranges` fix.
    let o = load();
    assert!(
        sat_sub(&o, "AspectFeature", "Feature"),
        "∃hasAspect.Self + hasAspect⊑hasFeature + range(hasFeature)=Feature ⟹ ⊑ Feature (saturation)"
    );
}

#[test]
fn self_restriction_no_spurious_subsumption() {
    // FP guard: Animal has no self-restriction ⟹ not a Verb, not a Feature.
    let o = load();
    assert!(
        !sat_sub(&o, "Animal", "Verb"),
        "Animal ⋢ Verb (no ∃hasAspect.Self)"
    );
    assert!(
        !sat_sub(&o, "Animal", "Feature"),
        "Animal ⋢ Feature (no ∃hasAspect.Self)"
    );
    // FP guard: `unrelated` is NOT a super-role of hasAspect, so its range (Bogus)
    // must not leak into AspectFeature's effective range (only R⊑*S ranges apply).
    assert!(
        !sat_sub(&o, "AspectFeature", "Bogus"),
        "AspectFeature ⋢ Bogus (unrelated is not a super-role of hasAspect)"
    );
}
