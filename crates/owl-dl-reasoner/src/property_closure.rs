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

/// `AsymmetricObjectProperty(p)` is read as `Disjoint(p, p⁻)` by the property
/// disjointness surface (#389, F3), which is exactly its meaning (`p(x,y)` and
/// `p(y,x)` never both hold). It then passes down the hierarchy like any told
/// disjointness, so it needs `RUSTDL_PROPERTY_DISJOINT_DOWN` and is inert without
/// it. Default **ON**; `RUSTDL_PROPERTY_ASYMMETRIC_DISJOINT=0` reverts.
#[must_use]
pub fn property_asymmetric_disjoint_enabled() -> bool {
    std::env::var_os("RUSTDL_PROPERTY_ASYMMETRIC_DISJOINT").is_none_or(|v| v != "0")
}

/// Seed `owl:topObjectProperty` / `owl:bottomObjectProperty` (and the data pair)
/// into the told closures (#389): every property is below top and above bottom,
/// both are their own inverse. So `top ⊑ p` makes every property a sub-property of
/// `p`, and `p ⊑ bottom` makes `p` a sub-property of every property. Done only when
/// a told hierarchy axiom mentions top or bottom, so the closure of an ordinary
/// ontology is unchanged; top and bottom stay out of the reported rows. Default
/// **ON**; `RUSTDL_PROPERTY_TOP_BOTTOM=0` reverts.
#[must_use]
pub fn property_top_bottom_enabled() -> bool {
    std::env::var_os("RUSTDL_PROPERTY_TOP_BOTTOM").is_none_or(|v| v != "0")
}

/// A one-leg `ObjectPropertyChain([p]) ⊑ s` is the plain `p ⊑ s` and joins the told
/// closure (#389). OFN cannot write one (its grammar wants two legs); other syntaxes
/// and the API can. Default **ON**; `RUSTDL_PROPERTY_UNIT_CHAIN=0` reverts.
#[must_use]
pub fn property_unit_chain_enabled() -> bool {
    std::env::var_os("RUSTDL_PROPERTY_UNIT_CHAIN").is_none_or(|v| v != "0")
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
    let unit_chain = property_unit_chain_enabled();
    let mut edges: BTreeSet<(Signed, Signed)> = BTreeSet::new();
    for ac in onto {
        match &ac.component {
            C::SubObjectPropertyOf(ax) => {
                match &ax.sub {
                    SOPE::ObjectPropertyExpression(sub_ope) => {
                        edges.insert((signed(sub_ope), signed(&ax.sup)));
                    }
                    SOPE::ObjectPropertyChain(legs) if unit_chain && legs.len() == 1 => {
                        edges.insert((signed(&legs[0]), signed(&ax.sup)));
                    }
                    // A longer chain is complex subsumption, skipped.
                    SOPE::ObjectPropertyChain(_) => {}
                }
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
    let mentions = |n: &str| n == OBJ_TOP || n == OBJ_BOT;
    if property_top_bottom_enabled() && edges.iter().any(|(a, b)| mentions(&a.0) || mentions(&b.0))
    {
        let mut names: BTreeSet<String> = object_signature(onto);
        names.extend(edges.iter().flat_map(|(a, b)| [a.0.clone(), b.0.clone()]));
        for n in names {
            for pol in [false, true] {
                edges.insert(((n.clone(), pol), (OBJ_TOP.to_string(), false)));
                edges.insert(((OBJ_BOT.to_string(), false), (n.clone(), pol)));
            }
        }
        for t in [OBJ_TOP, OBJ_BOT] {
            edges.insert(((t.to_string(), false), (t.to_string(), true)));
            edges.insert(((t.to_string(), true), (t.to_string(), false)));
        }
    }
    close_signed(&mut edges);
    edges
}

/// Object properties declared or used at the top level of a property axiom or
/// assertion. Nested class-expression uses of an undeclared property are not
/// collected; missing one only loses rows for that property.
fn object_signature<A: ForIRI>(onto: &SetOntology<A>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut add = |o: &OPE<A>| {
        out.insert(signed(o).0);
    };
    for ac in onto {
        match &ac.component {
            C::DeclareObjectProperty(d) => add(&OPE::ObjectProperty(d.0.clone())),
            C::ObjectPropertyDomain(ax) => add(&ax.ope),
            C::ObjectPropertyRange(ax) => add(&ax.ope),
            C::DisjointObjectProperties(ax) => ax.0.iter().for_each(&mut add),
            C::FunctionalObjectProperty(ax) => add(&ax.0),
            C::InverseFunctionalObjectProperty(ax) => add(&ax.0),
            C::SymmetricObjectProperty(ax) => add(&ax.0),
            C::AsymmetricObjectProperty(ax) => add(&ax.0),
            C::ReflexiveObjectProperty(ax) => add(&ax.0),
            C::IrreflexiveObjectProperty(ax) => add(&ax.0),
            C::TransitiveObjectProperty(ax) => add(&ax.0),
            C::ObjectPropertyAssertion(ax) => add(&ax.ope),
            C::NegativeObjectPropertyAssertion(ax) => add(&ax.ope),
            _ => {}
        }
    }
    out
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
    let mentions = |n: &str| n == DATA_TOP || n == DATA_BOT;
    if property_top_bottom_enabled() && edges.iter().any(|(a, b)| mentions(a) || mentions(b)) {
        let mut names: BTreeSet<String> = onto
            .iter()
            .filter_map(|ac| match &ac.component {
                C::DeclareDataProperty(d) => Some(d.0.0.as_ref().to_string()),
                C::DataPropertyDomain(ax) => Some(ax.dp.0.as_ref().to_string()),
                C::DataPropertyRange(ax) => Some(ax.dp.0.as_ref().to_string()),
                C::FunctionalDataProperty(ax) => Some(ax.0.0.as_ref().to_string()),
                C::DataPropertyAssertion(ax) => Some(ax.dp.0.as_ref().to_string()),
                _ => None,
            })
            .collect();
        names.extend(edges.iter().flat_map(|(a, b)| [a.clone(), b.clone()]));
        for n in names {
            edges.insert((n.clone(), DATA_TOP.to_string()));
            edges.insert((DATA_BOT.to_string(), n));
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
