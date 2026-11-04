//! Layer 2 of the pure-EL guard: does the EL lowering drop anything on an input the
//! saturation fast path certifies complete?
//!
//! [`owl_dl_saturation::unlowered`] records every construct the lowering drops or weakens,
//! on any input. This module adds the gates' knowledge: it keeps the records that
//! contradict a certification, using exactly the gate `classify` dispatches on
//! (`saturation_fast_path_admits`) and the one `realize` does
//! (`realize_saturation_eligible`), and drops records on the axioms those gates admit
//! knowing the lowering ignores them (`admitted_without_lowering`). An empty result means
//! the certification is backed by the rules the saturator actually received. No oracle
//! is needed, so it can be asserted on any input.

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{ConceptExpr, ConceptId, InternalOntology, Role};

use crate::ReasonError;
use crate::classify::{admitted_without_lowering, saturation_fast_path_admits};
use crate::realize::realize_saturation_eligible;

/// One construct the lowering dropped or weakened, on an input whose saturation fast path
/// is certified complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnloweredReport {
    /// Index into the converted ontology's axioms.
    pub axiom: usize,
    /// The `owl_dl_saturation::unlowered` site that recorded it.
    pub site: &'static str,
    /// The dropped construct, rendered in a compact functional syntax (empty for a site
    /// that drops a whole non-concept axiom).
    pub concept: String,
}

/// What [`certified_unlowered`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CertifiedUnlowered {
    /// `classify`'s saturation fast path admits the input (and so reports it complete).
    pub classify_certified: bool,
    /// Drops under that certification; empty unless `classify_certified`.
    pub classify: Vec<UnloweredReport>,
    /// `realize`'s saturation fast path admits the input.
    pub realize_certified: bool,
    /// What `realize`'s `ABox` seeding drops under that certification; empty unless
    /// `realize_certified`. Its `TBox` drops are in `classify`, which checks classify's
    /// gate; realize's admits the tbox-only fragment even with
    /// `RUSTDL_CLASSIFY_TBOX_FRAGMENT=0`, so with that flag off a realize-certified
    /// input's `TBox` goes unchecked here (default on, as in CI).
    pub realize: Vec<UnloweredReport>,
}

/// Run the EL lowering with its `unlowered` recorder and keep what contradicts the
/// fast-path certification. Nothing is classified; this costs one lowering pass.
///
/// # Errors
///
/// [`ReasonError`] if the ontology does not convert, as for `classify`.
pub fn certified_unlowered<A: ForIRI>(
    ontology: &SetOntology<A>,
) -> Result<CertifiedUnlowered, ReasonError> {
    let internal = convert_ontology(ontology)?;
    Ok(certified_unlowered_internal(&internal))
}

pub(crate) fn certified_unlowered_internal(internal: &InternalOntology) -> CertifiedUnlowered {
    let report = |u: owl_dl_saturation::Unlowered| UnloweredReport {
        axiom: u.axiom,
        site: u.site,
        concept: u.concept.map(|c| render(c, internal)).unwrap_or_default(),
    };
    let mut out = CertifiedUnlowered {
        classify_certified: saturation_fast_path_admits(internal),
        realize_certified: realize_saturation_eligible(internal),
        ..CertifiedUnlowered::default()
    };
    if out.classify_certified {
        out.classify = owl_dl_saturation::unlowered_el_constructs(internal)
            .into_iter()
            .filter(|u| !admitted_without_lowering(&internal.axioms[u.axiom], internal))
            .map(report)
            .collect();
    }
    if out.realize_certified {
        out.realize = owl_dl_saturation::unlowered_realize_abox(internal)
            .into_iter()
            .map(report)
            .collect();
    }
    out
}

fn render(c: ConceptId, internal: &InternalOntology) -> String {
    let local = |iri: &str| iri.rsplit(['#', '/']).next().unwrap_or(iri).to_owned();
    let role = |r: &Role| {
        let name = local(internal.vocabulary.role_iri(r.role_id()));
        if r.is_inverse() {
            format!("inv({name})")
        } else {
            name
        }
    };
    let list = |cs: &[ConceptId]| {
        cs.iter()
            .map(|&x| render(x, internal))
            .collect::<Vec<_>>()
            .join(" ")
    };
    match internal.concepts.get(c) {
        ConceptExpr::Top => "⊤".to_owned(),
        ConceptExpr::Bot => "⊥".to_owned(),
        ConceptExpr::Atomic(id) => local(internal.vocabulary.class_iri(*id)),
        ConceptExpr::Nominal(i) => format!("{{{}}}", local(internal.vocabulary.individual_iri(*i))),
        ConceptExpr::SelfRestriction(r) => format!("Self({})", role(r)),
        ConceptExpr::Not(x) => format!("¬{}", render(*x, internal)),
        ConceptExpr::And(xs) => format!("And({})", list(xs)),
        ConceptExpr::Or(xs) => format!("Or({})", list(xs)),
        ConceptExpr::Some(r, x) => format!("∃{}.{}", role(r), render(*x, internal)),
        ConceptExpr::All(r, x) => format!("∀{}.{}", role(r), render(*x, internal)),
        ConceptExpr::Min(n, r, x) => format!("≥{n}{}.{}", role(r), render(*x, internal)),
        ConceptExpr::Max(n, r, x) => format!("≤{n}{}.{}", role(r), render(*x, internal)),
    }
}
