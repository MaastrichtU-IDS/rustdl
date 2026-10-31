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

/// `SymmetricObjectProperty(p)` makes `p ≡ p⁻` in the object closure (#389, F1). It
/// is the axiom's own meaning (`p(x,y) → p(y,x)` is `p ⊑ p⁻`, and so `p ≡ p⁻`), so it
/// can only add a relation that holds; with `InverseObjectProperties(p, q)` it gives
/// `p ≡ q`. A symmetric `p⁻` is the same as a symmetric `p`. Built from the horned-owl
/// axiom alone, not from the core's `collect_inverse_pairs` / `mark_symmetric` path.
/// Default **ON**; `RUSTDL_PROPERTY_SYMMETRIC=0` reverts.
#[must_use]
pub fn property_symmetric_enabled() -> bool {
    std::env::var_os("RUSTDL_PROPERTY_SYMMETRIC").is_none_or(|v| v != "0")
}

/// Told property disjointness is passed down the told hierarchy (#389, F2): with
/// `Disjoint(q, r)`, every `p ⊑ q` and `p' ⊑ r` are disjoint, and so are their
/// inverses (`(q ∩ r)⁻ = q⁻ ∩ r⁻`). Members written as `ObjectInverseOf` are no
/// longer skipped. Sound: a sub-property's edges are among its parent's. Default
/// **ON**; `RUSTDL_PROPERTY_DISJOINT_DOWN=0` reverts to told pairs between named
/// members only.
#[must_use]
pub fn property_disjoint_down_enabled() -> bool {
    std::env::var_os("RUSTDL_PROPERTY_DISJOINT_DOWN").is_none_or(|v| v != "0")
}

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
    let symmetric = property_symmetric_enabled();
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
            C::SymmetricObjectProperty(ax) if symmetric => {
                let p = signed(&ax.0).0;
                edges.insert(((p.clone(), false), (p.clone(), true)));
                edges.insert(((p.clone(), true), (p, false)));
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

/// Disjoint named pairs `(a, b)` with `a < b`: every pair below two members of one
/// told disjointness set, through the signed closure `edges`. Two signed nodes of
/// the same polarity give the named pair (for two inverses, `x⁻ ∩ y⁻ = ∅` is
/// `x ∩ y = ∅`). A node below two members is empty, which this `a < b` list
/// cannot express; `property_structural_gaps` reports it. `exclude` names
/// (`owl:top/bottom…`) are dropped.
pub(crate) fn disjoint_down(
    edges: &BTreeSet<(Signed, Signed)>,
    sets: &[Vec<Signed>],
    exclude: &[&str],
) -> Vec<(String, String)> {
    let below = |m: &Signed| -> BTreeSet<Signed> {
        let mut s: BTreeSet<Signed> = edges
            .iter()
            .filter(|(_, y)| y == m)
            .map(|(x, _)| x.clone())
            .collect();
        s.insert(m.clone());
        s
    };
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for set in sets {
        for (i, m1) in set.iter().enumerate() {
            for m2 in &set[i + 1..] {
                let (b1, b2) = (below(m1), below(m2));
                for x in &b1 {
                    for y in &b2 {
                        if x.1 != y.1 || x.0 == y.0 {
                            continue;
                        }
                        if exclude.contains(&x.0.as_str()) || exclude.contains(&y.0.as_str()) {
                            continue;
                        }
                        let (lo, hi) = if x.0 < y.0 {
                            (&x.0, &y.0)
                        } else {
                            (&y.0, &x.0)
                        };
                        out.insert((lo.clone(), hi.clone()));
                    }
                }
            }
        }
    }
    out.into_iter().collect()
}
