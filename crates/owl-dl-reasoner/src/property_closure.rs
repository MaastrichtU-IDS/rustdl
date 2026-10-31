//! Told property closures shared by the property-hierarchy, property-disjointness,
//! materialize and structural-gap surfaces (#389). Structural only: no
//! entailment probe.
use horned_owl::model::{
    Component as C, ForIRI, ObjectPropertyExpression as OPE, SubObjectPropertyExpression as SOPE,
};
use horned_owl::ontology::set::SetOntology;
use std::collections::BTreeSet;

pub(crate) const OBJ_TOP: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
pub(crate) const OBJ_BOT: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";
pub(crate) const DATA_TOP: &str = "http://www.w3.org/2002/07/owl#topDataProperty";
pub(crate) const DATA_BOT: &str = "http://www.w3.org/2002/07/owl#bottomDataProperty";

/// `(property IRI, is_inverse)`.
pub(crate) type Signed = (String, bool);

pub(crate) fn signed<A: ForIRI>(ope: &OPE<A>) -> Signed {
    match ope {
        OPE::ObjectProperty(op) => (op.0.as_ref().to_string(), false),
        OPE::InverseObjectProperty(op) => (op.0.as_ref().to_string(), true),
    }
}

/// The told object-property subsumption edges over signed properties (told
/// sub-property, equivalence, declared inverses), closed under inversion and
/// transitivity. `(a, b)` means `a ⊑ b`.
pub(crate) fn object_edges<A: ForIRI>(onto: &SetOntology<A>) -> BTreeSet<(Signed, Signed)> {
    let mut edges: BTreeSet<(Signed, Signed)> = BTreeSet::new();
    for ac in onto {
        match &ac.component {
            C::SubObjectPropertyOf(ax) => {
                if let SOPE::ObjectPropertyExpression(sub_ope) = &ax.sub {
                    edges.insert((signed(sub_ope), signed(&ax.sup)));
                }
                // ObjectPropertyChain sub → complex subsumption, skipped.
            }
            C::EquivalentObjectProperties(ax) => {
                let ss: Vec<Signed> = ax.0.iter().map(signed).collect();
                for (i, si) in ss.iter().enumerate() {
                    for (j, sj) in ss.iter().enumerate() {
                        if i != j {
                            edges.insert((si.clone(), sj.clone()));
                        }
                    }
                }
            }
            C::InverseObjectProperties(ax) => {
                let p = ax.0.0.as_ref().to_string();
                let q = ax.1.0.as_ref().to_string();
                // (p,false) ≡ (q,true) ; (q,false) ≡ (p,true)
                edges.insert(((p.clone(), false), (q.clone(), true)));
                edges.insert(((q.clone(), true), (p.clone(), false)));
                edges.insert(((q.clone(), false), (p.clone(), true)));
                edges.insert(((p.clone(), true), (q.clone(), false)));
            }
            _ => {}
        }
    }
    close_signed(&mut edges);
    edges
}

/// Inverse propagation + transitive closure to fixpoint.
fn close_signed(edges: &mut BTreeSet<(Signed, Signed)>) {
    loop {
        let mut new: Vec<(Signed, Signed)> = Vec::new();
        for ((an, af), (bn, bf)) in edges.iter() {
            let cand = ((an.clone(), !*af), (bn.clone(), !*bf));
            if !edges.contains(&cand) {
                new.push(cand);
            }
        }
        for (a, b) in edges.iter() {
            for (b2, c) in edges.iter() {
                if b == b2 {
                    let cand = (a.clone(), c.clone());
                    if !edges.contains(&cand) {
                        new.push(cand);
                    }
                }
            }
        }
        if new.is_empty() {
            break;
        }
        for e in new {
            edges.insert(e);
        }
    }
}

/// The told data-property subsumption edges (told sub-property, equivalence),
/// transitively closed. `(a, b)` means `a ⊑ b`.
pub(crate) fn data_edges<A: ForIRI>(onto: &SetOntology<A>) -> BTreeSet<(String, String)> {
    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for ac in onto {
        match &ac.component {
            C::SubDataPropertyOf(ax) => {
                edges.insert((ax.sub.0.as_ref().to_string(), ax.sup.0.as_ref().to_string()));
            }
            C::EquivalentDataProperties(ax) => {
                let ds: Vec<String> = ax.0.iter().map(|d| d.0.as_ref().to_string()).collect();
                for (i, di) in ds.iter().enumerate() {
                    for (j, dj) in ds.iter().enumerate() {
                        if i != j {
                            edges.insert((di.clone(), dj.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    close_plain(&mut edges);
    edges
}

fn close_plain(edges: &mut BTreeSet<(String, String)>) {
    loop {
        let mut new: Vec<(String, String)> = Vec::new();
        for (a, b) in edges.iter() {
            for (b2, c) in edges.iter() {
                if b == b2 {
                    let cand = (a.clone(), c.clone());
                    if !edges.contains(&cand) {
                        new.push(cand);
                    }
                }
            }
        }
        if new.is_empty() {
            break;
        }
        for e in new {
            edges.insert(e);
        }
    }
}

/// Named `(sub, sup)` rows of the object closure, without `owl:top/bottom`.
pub(crate) fn object_named_pairs(edges: &BTreeSet<(Signed, Signed)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = edges
        .iter()
        .filter(|((_, af), (_, bf))| !af && !bf)
        .map(|((a, _), (b, _))| (a.clone(), b.clone()))
        .filter(|(a, b)| a != b && a != OBJ_TOP && a != OBJ_BOT && b != OBJ_TOP && b != OBJ_BOT)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Named `(sub, sup)` rows of the data closure, without `owl:top/bottom`.
pub(crate) fn data_named_pairs(edges: &BTreeSet<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = edges
        .iter()
        .filter(|(a, b)| a != b && a != DATA_TOP && a != DATA_BOT && b != DATA_TOP && b != DATA_BOT)
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}
