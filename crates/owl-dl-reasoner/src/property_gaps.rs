//! Structural gaps of the property surfaces (#389, #390).
//!
//! `property-hierarchy`, the property half of `disjoint`, and the data half of
//! `property-values` are structural closures: they read told axioms and never
//! probe the reasoner. [`property_structural_gaps`] names the constructs present
//! in an ontology that those closures ignore and that can entail a relation the
//! surface would have reported. Each name is one known route to a missed answer.
//!
//! An empty list means none of the enumerated routes occurs. It is NOT a proof of
//! completeness: a property whose domain or range is unsatisfiable outside the EL
//! fragment is empty in every model (so below every property and disjoint from
//! every property) and is not detected; a property used only inside a class
//! expression and never declared is outside the `owl:topObjectProperty` /
//! `owl:bottomObjectProperty` seeding (OWL 2 DL requires the declaration); and
//! routes not enumerated here are not reported. This is a separate signal from `incomplete`, which means only that a
//! deadline cut a probe.
use crate::ReasonError;
use crate::property_closure::{
    DATA_BOT, DATA_TOP, OBJ_BOT, OBJ_TOP, Signed, data_edges, object_edges, signed,
};
use horned_owl::model::{
    ClassExpression as CE, Component as C, DataRange, ForIRI, Individual, Literal,
    PropertyExpression, SubObjectPropertyExpression as SOPE,
};
use horned_owl::ontology::set::SetOntology;
use std::collections::{BTreeMap, BTreeSet};

/// Per-surface structural gaps; see the module doc. Every list is sorted and
/// deduplicated, and every kind is prefixed `object-` or `data-`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PropertyStructuralGaps {
    object_hierarchy: Vec<String>,
    data_hierarchy: Vec<String>,
    object_disjoint: Vec<String>,
    data_disjoint: Vec<String>,
    data_values: Vec<String>,
}

impl PropertyStructuralGaps {
    /// Gaps of `classify_object_property_hierarchy` and
    /// `materialize_subobjectproperty_axioms`.
    #[must_use]
    pub fn object_hierarchy(&self) -> &[String] {
        &self.object_hierarchy
    }
    /// Gaps of `classify_data_property_hierarchy` and
    /// `materialize_subdataproperty_axioms`.
    #[must_use]
    pub fn data_hierarchy(&self) -> &[String] {
        &self.data_hierarchy
    }
    /// Gaps of `disjoint_object_properties`.
    #[must_use]
    pub fn object_disjoint(&self) -> &[String] {
        &self.object_disjoint
    }
    /// Gaps of `disjoint_data_properties`.
    #[must_use]
    pub fn data_disjoint(&self) -> &[String] {
        &self.data_disjoint
    }
    /// Gaps of `inferred_data_property_values` and
    /// `materialize_data_property_assertions`.
    #[must_use]
    pub fn data_values(&self) -> &[String] {
        &self.data_values
    }
    /// Object and data hierarchy gaps together (the `property-hierarchy` command).
    #[must_use]
    pub fn hierarchy(&self) -> Vec<String> {
        merged(&[&self.object_hierarchy, &self.data_hierarchy])
    }
    /// Object and data disjointness gaps together (the property half of `disjoint`).
    #[must_use]
    pub fn disjoint(&self) -> Vec<String> {
        merged(&[&self.object_disjoint, &self.data_disjoint])
    }
}

fn merged(lists: &[&Vec<String>]) -> Vec<String> {
    let set: BTreeSet<String> = lists.iter().flat_map(|l| l.iter().cloned()).collect();
    set.into_iter().collect()
}

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// What a walk over every class expression found.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct CeFacts {
    has_self: bool,
    complement: bool,
    nothing: bool,
    /// `ObjectHasValue`, or `∃s.F` with `F` a nominal enumeration.
    obj_value: bool,
    /// Roles whose every successor is bounded to nominals (`∀r.F`).
    obj_all_oneof: BTreeSet<Signed>,
    /// Roles constrained on all their edges (`∀`, `≤`, `=`, `Self`).
    obj_constrained: BTreeSet<String>,
    /// Roles under an at-most bound (`≤n`, `=n`).
    obj_bounded: BTreeSet<Signed>,
    /// Roles carrying a nominal filler (`∃r.{a}`, `r value a`).
    obj_nominal_filler: BTreeSet<Signed>,
    /// `DataHasValue`, or `∃d.F` / `≥n d.F` with `F` a one-value enumeration.
    data_value: bool,
    data_all_oneof: BTreeSet<String>,
    /// Data properties every instance of some class has a value for
    /// (`∃d.F`, `≥n d.F` / `=n d.F` with `n ≥ 1`).
    data_some: BTreeSet<String>,
    /// Some `∀d.F`: with a value elsewhere, two classes can be disjoint by datatype.
    data_all: bool,
    data_constrained: BTreeSet<String>,
    data_bounded: BTreeSet<String>,
}

fn is_oneofish<A: ForIRI>(ce: &CE<A>) -> bool {
    match ce {
        CE::ObjectOneOf(_) => true,
        CE::ObjectUnionOf(v) => !v.is_empty() && v.iter().all(is_oneofish),
        _ => false,
    }
}

fn is_data_oneofish<A: ForIRI>(dr: &DataRange<A>) -> bool {
    match dr {
        DataRange::DataOneOf(_) => true,
        DataRange::DataUnionOf(v) => !v.is_empty() && v.iter().all(is_data_oneofish),
        _ => false,
    }
}

fn is_single_value<A: ForIRI>(dr: &DataRange<A>) -> bool {
    matches!(dr, DataRange::DataOneOf(v) if v.len() == 1)
}

fn walk<A: ForIRI>(ce: &CE<A>, f: &mut CeFacts) {
    match ce {
        CE::Class(c) => {
            if c.0.as_ref() == NOTHING {
                f.nothing = true;
            }
        }
        CE::ObjectIntersectionOf(v) | CE::ObjectUnionOf(v) => v.iter().for_each(|c| walk(c, f)),
        CE::ObjectComplementOf(b) => {
            f.complement = true;
            walk(b, f);
        }
        CE::ObjectOneOf(_) => {}
        CE::ObjectSomeValuesFrom { ope, bce } => {
            if is_oneofish(bce) {
                f.obj_value = true;
                f.obj_nominal_filler.insert(signed(ope));
            }
            walk(bce, f);
        }
        CE::ObjectAllValuesFrom { ope, bce } => {
            if is_oneofish(bce) {
                f.obj_all_oneof.insert(signed(ope));
            }
            f.obj_constrained.insert(signed(ope).0);
            walk(bce, f);
        }
        CE::ObjectHasValue { ope, .. } => {
            f.obj_value = true;
            f.obj_nominal_filler.insert(signed(ope));
        }
        CE::ObjectHasSelf(ope) => {
            f.has_self = true;
            f.obj_constrained.insert(signed(ope).0);
        }
        CE::ObjectMinCardinality { ope, bce, .. } => {
            if is_oneofish(bce) {
                f.obj_nominal_filler.insert(signed(ope));
            }
            walk(bce, f);
        }
        CE::ObjectMaxCardinality { ope, bce, .. } | CE::ObjectExactCardinality { ope, bce, .. } => {
            f.obj_constrained.insert(signed(ope).0);
            f.obj_bounded.insert(signed(ope));
            if is_oneofish(bce) {
                f.obj_nominal_filler.insert(signed(ope));
            }
            walk(bce, f);
        }
        CE::DataSomeValuesFrom { dp, dr } => {
            f.data_some.insert(dp.0.as_ref().to_string());
            if is_single_value(dr) {
                f.data_value = true;
            }
        }
        CE::DataMinCardinality { n, dp, dr } => {
            if *n >= 1 {
                f.data_some.insert(dp.0.as_ref().to_string());
            }
            if is_single_value(dr) {
                f.data_value = true;
            }
        }
        CE::DataAllValuesFrom { dp, dr } => {
            f.data_all = true;
            if is_data_oneofish(dr) {
                f.data_all_oneof.insert(dp.0.as_ref().to_string());
            }
            f.data_constrained.insert(dp.0.as_ref().to_string());
        }
        CE::DataHasValue { .. } => f.data_value = true,
        CE::DataExactCardinality { n, dp, .. } if *n >= 1 => {
            f.data_some.insert(dp.0.as_ref().to_string());
            f.data_constrained.insert(dp.0.as_ref().to_string());
            f.data_bounded.insert(dp.0.as_ref().to_string());
        }
        CE::DataMaxCardinality { dp, .. } | CE::DataExactCardinality { dp, .. } => {
            f.data_constrained.insert(dp.0.as_ref().to_string());
            f.data_bounded.insert(dp.0.as_ref().to_string());
        }
    }
}

/// A singleton nominal at the top level of `ce` (or of an intersection), which
/// equates whatever it classifies with that individual.
fn has_singleton_nominal<A: ForIRI>(ce: &CE<A>) -> bool {
    match ce {
        CE::ObjectOneOf(v) => v.len() == 1,
        CE::ObjectIntersectionOf(v) => v.iter().any(has_singleton_nominal),
        _ => false,
    }
}

fn ind_key<A: ForIRI>(i: &Individual<A>) -> String {
    match i {
        Individual::Named(n) => n.0.as_ref().to_string(),
        Individual::Anonymous(a) => format!("_:{}", a.0.as_ref()),
    }
}

fn lit_key<A: ForIRI>(l: &Literal<A>) -> String {
    format!("{l:?}")
}

/// Strict sub-properties exist below `t`: some `x` with `x ⊑ t` and not `t ⊑ x`.
/// `skip` excludes the seeded top/bottom nodes: `owl:bottomObjectProperty ⊑ t`
/// holds for every `t` and is no route to a miss.
fn has_strict_sub<T: Ord + Clone>(
    edges: &BTreeSet<(T, T)>,
    t: &T,
    skip: impl Fn(&T) -> bool,
) -> bool {
    edges
        .iter()
        .any(|(x, y)| y == t && x != t && !skip(x) && !edges.contains(&(t.clone(), x.clone())))
}

fn obj_top_or_bottom(s: &Signed) -> bool {
    s.0 == OBJ_TOP || s.0 == OBJ_BOT
}

fn data_top_or_bottom(d: &String) -> bool {
    d == DATA_TOP || d == DATA_BOT
}

/// See the module doc.
///
/// # Errors
/// [`ReasonError::Conversion`] if the ontology cannot be lowered (the EL
/// saturation used for the `empty-property` check needs it).
pub fn property_structural_gaps<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<PropertyStructuralGaps, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(onto)?;
    let closure = owl_dl_saturation::saturate(&internal);
    Ok(gaps_with(onto, &internal, &closure))
}

/// [`property_structural_gaps`] over an already converted ontology and its EL
/// closure, for callers that built both anyway.
#[allow(clippy::too_many_lines)]
pub(crate) fn gaps_with<A: ForIRI>(
    onto: &SetOntology<A>,
    internal: &owl_dl_core::ontology::InternalOntology,
    closure: &owl_dl_saturation::Subsumers,
) -> PropertyStructuralGaps {
    let oedges = object_edges(onto);
    let dedges = data_edges(onto);
    let mut f = CeFacts::default();

    let mut symmetric: BTreeSet<String> = BTreeSet::new();
    let mut asymmetric: BTreeSet<String> = BTreeSet::new();
    let mut reflexive = false;
    let mut chain_long = false;
    let mut chain_top_leg = false;
    let mut chain_unit = false;
    let mut obj_top_bottom = false;
    let mut data_top_bottom = false;
    let mut obj_disjoint_sets: Vec<Vec<Signed>> = Vec::new();
    let mut data_disjoint_sets: Vec<Vec<String>> = Vec::new();
    let mut disjoint_source = false;
    let mut obj_domain_range_nothing: BTreeSet<String> = BTreeSet::new();
    let mut obj_domain_range_class: Vec<(String, String)> = Vec::new();
    let mut data_domain_class: Vec<(String, String)> = Vec::new();
    let mut top_is_nominal = false;
    let mut data_range_props: BTreeSet<String> = BTreeSet::new();
    let mut abox = false;
    let mut dpa = false;
    let mut anon_individual = false;
    let mut singleton_nominal_axiom = false;
    let mut haskey_props: Vec<Vec<String>> = Vec::new();
    // Told successors per (subject, signed role), named objects only.
    let mut successors: BTreeMap<(String, Signed), BTreeSet<String>> = BTreeMap::new();
    // Told values per (property, value) -> subjects, for the `HasKey` check.
    let mut key_values: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();

    let mut bounded_obj: BTreeSet<Signed> = BTreeSet::new();
    let mut bounded_data: BTreeSet<String> = BTreeSet::new();

    for ac in onto {
        match &ac.component {
            C::SubClassOf(ax) => {
                walk(&ax.sub, &mut f);
                walk(&ax.sup, &mut f);
                if matches!(&ax.sub, CE::Class(c) if c.0.as_ref() == THING) && is_oneofish(&ax.sup)
                {
                    top_is_nominal = true;
                }
                if matches!(&ax.sup, CE::Class(c) if c.0.as_ref() == NOTHING) {
                    disjoint_source = true;
                }
                if has_singleton_nominal(&ax.sup) {
                    singleton_nominal_axiom = true;
                }
            }
            C::EquivalentClasses(ax) => {
                ax.0.iter().for_each(|c| walk(c, &mut f));
                let thing =
                    ax.0.iter()
                        .any(|c| matches!(c, CE::Class(k) if k.0.as_ref() == THING));
                if thing && ax.0.iter().any(is_oneofish) {
                    top_is_nominal = true;
                }
                if ax.0.iter().any(has_singleton_nominal) {
                    singleton_nominal_axiom = true;
                }
            }
            C::DisjointClasses(ax) => {
                disjoint_source = true;
                ax.0.iter().for_each(|c| walk(c, &mut f));
            }
            C::DisjointUnion(ax) => {
                disjoint_source = true;
                ax.1.iter().for_each(|c| walk(c, &mut f));
            }
            C::DifferentIndividuals(_) => {
                disjoint_source = true;
                abox = true;
            }
            C::SubObjectPropertyOf(ax) => {
                let sup = signed(&ax.sup);
                if sup.0 == OBJ_TOP || sup.0 == OBJ_BOT {
                    obj_top_bottom = true;
                }
                match &ax.sub {
                    SOPE::ObjectPropertyExpression(o) => {
                        let s = signed(o);
                        if s.0 == OBJ_TOP || s.0 == OBJ_BOT {
                            obj_top_bottom = true;
                        }
                    }
                    SOPE::ObjectPropertyChain(legs) => {
                        if legs.len() == 1 {
                            chain_unit = true;
                        } else {
                            chain_long = true;
                        }
                        if legs.iter().any(|l| signed(l).0 == OBJ_TOP) {
                            chain_top_leg = true;
                        }
                    }
                }
            }
            C::EquivalentObjectProperties(ax)
                if ax.0.iter().any(|o| {
                    let s = signed(o);
                    s.0 == OBJ_TOP || s.0 == OBJ_BOT
                }) =>
            {
                obj_top_bottom = true;
            }
            C::SubDataPropertyOf(ax) => {
                for d in [&ax.sub, &ax.sup] {
                    if d.0.as_ref() == DATA_TOP || d.0.as_ref() == DATA_BOT {
                        data_top_bottom = true;
                    }
                }
            }
            C::EquivalentDataProperties(ax)
                if ax
                    .0
                    .iter()
                    .any(|d| d.0.as_ref() == DATA_TOP || d.0.as_ref() == DATA_BOT) =>
            {
                data_top_bottom = true;
            }
            C::DisjointObjectProperties(ax) => {
                obj_disjoint_sets.push(ax.0.iter().map(signed).collect());
            }
            C::DisjointDataProperties(ax) => {
                data_disjoint_sets.push(ax.0.iter().map(|d| d.0.as_ref().to_string()).collect());
            }
            C::SymmetricObjectProperty(ax) => {
                let o = &ax.0;
                symmetric.insert(signed(o).0);
            }
            C::AsymmetricObjectProperty(ax) => {
                let o = &ax.0;
                asymmetric.insert(signed(o).0);
                f.obj_constrained.insert(signed(o).0);
            }
            C::ReflexiveObjectProperty(_) => reflexive = true,
            C::IrreflexiveObjectProperty(ax) => {
                let o = &ax.0;
                f.obj_constrained.insert(signed(o).0);
            }
            C::FunctionalObjectProperty(ax) => {
                let o = &ax.0;
                bounded_obj.insert(signed(o));
                f.obj_constrained.insert(signed(o).0);
            }
            C::InverseFunctionalObjectProperty(ax) => {
                let o = &ax.0;
                let (n, inv) = signed(o);
                f.obj_constrained.insert(n.clone());
                bounded_obj.insert((n, !inv));
            }
            C::ObjectPropertyDomain(ax) => {
                walk(&ax.ce, &mut f);
                note_domain_range(
                    &ax.ce,
                    &signed(&ax.ope).0,
                    &mut obj_domain_range_nothing,
                    &mut obj_domain_range_class,
                );
                f.obj_constrained.insert(signed(&ax.ope).0);
            }
            C::ObjectPropertyRange(ax) => {
                walk(&ax.ce, &mut f);
                if is_oneofish(&ax.ce) {
                    f.obj_all_oneof.insert(signed(&ax.ope));
                }
                note_domain_range(
                    &ax.ce,
                    &signed(&ax.ope).0,
                    &mut obj_domain_range_nothing,
                    &mut obj_domain_range_class,
                );
                f.obj_constrained.insert(signed(&ax.ope).0);
            }
            C::DataPropertyDomain(ax) => {
                walk(&ax.ce, &mut f);
                let dp = ax.dp.0.as_ref().to_string();
                if let CE::Class(c) = &ax.ce {
                    data_domain_class.push((dp.clone(), c.0.as_ref().to_string()));
                }
                f.data_constrained.insert(dp);
            }
            C::DataPropertyRange(ax) => {
                let dp = ax.dp.0.as_ref().to_string();
                if is_data_oneofish(&ax.dr) {
                    f.data_all_oneof.insert(dp.clone());
                }
                data_range_props.insert(dp.clone());
                f.data_constrained.insert(dp);
            }
            C::FunctionalDataProperty(ax) => {
                let d = &ax.0;
                bounded_data.insert(d.0.as_ref().to_string());
                f.data_constrained.insert(d.0.as_ref().to_string());
            }
            C::HasKey(ax) => {
                walk(&ax.ce, &mut f);
                haskey_props.push(
                    ax.vpe
                        .iter()
                        .filter_map(|p| match p {
                            PropertyExpression::ObjectPropertyExpression(o) => Some(signed(o).0),
                            PropertyExpression::DataProperty(d) => Some(d.0.as_ref().to_string()),
                            PropertyExpression::AnnotationProperty(_) => None,
                        })
                        .collect(),
                );
            }
            C::ClassAssertion(ax) => {
                abox = true;
                walk(&ax.ce, &mut f);
                if has_singleton_nominal(&ax.ce) {
                    singleton_nominal_axiom = true;
                }
            }
            C::ObjectPropertyAssertion(ax) => {
                abox = true;
                let (s, o) = (ind_key(&ax.from), ind_key(&ax.to));
                let r = signed(&ax.ope);
                key_values
                    .entry((r.0.clone(), o.clone()))
                    .or_default()
                    .insert(s.clone());
                successors
                    .entry((s.clone(), r.clone()))
                    .or_default()
                    .insert(o.clone());
                successors.entry((o, (r.0, !r.1))).or_default().insert(s);
            }
            C::DataPropertyAssertion(ax) => {
                abox = true;
                dpa = true;
                if matches!(ax.from, Individual::Anonymous(_)) {
                    anon_individual = true;
                }
                key_values
                    .entry((ax.dp.0.as_ref().to_string(), lit_key(&ax.to)))
                    .or_default()
                    .insert(ind_key(&ax.from));
            }
            C::SameIndividual(ax) => {
                abox = true;
                if ax.0.iter().any(|i| matches!(i, Individual::Anonymous(_))) {
                    anon_individual = true;
                }
            }
            C::NegativeObjectPropertyAssertion(_) | C::NegativeDataPropertyAssertion(_) => {
                abox = true;
            }
            _ => {}
        }
    }
    bounded_obj.extend(f.obj_bounded.iter().cloned());
    bounded_data.extend(f.data_bounded.iter().cloned());

    // ── empty properties ─────────────────────────────────────────────────────
    let unsat_class = |iri: &str| {
        iri == NOTHING
            || internal
                .vocabulary
                .class_id(iri)
                .is_some_and(|c| closure.is_unsatisfiable(c))
    };
    let obj_empty = obj_domain_range_nothing.iter().next().is_some()
        || obj_domain_range_class.iter().any(|(_, c)| unsat_class(c))
        || symmetric.iter().any(|p| asymmetric.contains(p))
        || asymmetric
            .iter()
            .any(|p| oedges.contains(&((p.clone(), false), (p.clone(), true))))
        || oedges.iter().any(|(x, y)| y.0 == OBJ_BOT && x.0 != OBJ_BOT)
        || derives_self_disjoint(&oedges, &obj_disjoint_sets, obj_top_or_bottom);
    let data_empty = data_domain_class
        .iter()
        .any(|(_, c)| c == NOTHING || unsat_class(c))
        || dedges.iter().any(|(x, y)| y == DATA_BOT && x != DATA_BOT)
        || derives_self_disjoint(&dedges, &data_disjoint_sets, data_top_or_bottom);

    // Constructs that can make two classes disjoint: told disjointness, `⊥`,
    // complements, an at-most bound (`≤0 r` against `≥1 r`), or a data constraint
    // (`∀d.xsd:string` against a value of `xsd:integer`).
    let class_disjoint_source = disjoint_source
        || f.complement
        || f.nothing
        || !bounded_obj.is_empty()
        || !bounded_data.is_empty()
        || f.data_all;

    // ── object hierarchy ─────────────────────────────────────────────────────
    let mut oh: BTreeSet<&str> = BTreeSet::new();
    if !symmetric.is_empty() && !crate::property_symmetric_enabled() {
        oh.insert("object-symmetric");
    }
    if obj_top_bottom && !crate::property_top_bottom_enabled() {
        oh.insert("object-top-bottom");
    }
    if chain_unit && !crate::property_unit_chain_enabled() {
        oh.insert("object-unit-chain");
    }
    if chain_long && (reflexive || f.has_self || chain_top_leg) {
        oh.insert("object-chain-reflexive");
    }
    if (!f.obj_all_oneof.is_empty() && f.obj_value) || top_is_nominal {
        oh.insert("object-nominal-range");
    }
    if bounded_obj
        .iter()
        .any(|t| has_strict_sub(&oedges, t, obj_top_or_bottom))
    {
        oh.insert("object-counting");
    }
    if obj_empty {
        oh.insert("object-empty-property");
    }

    // ── data hierarchy ───────────────────────────────────────────────────────
    let mut dh: BTreeSet<&str> = BTreeSet::new();
    if data_top_bottom && !crate::property_top_bottom_enabled() {
        dh.insert("data-top-bottom");
    }
    if bounded_data
        .iter()
        .any(|t| has_strict_sub(&dedges, t, data_top_or_bottom))
    {
        dh.insert("data-counting");
    }
    if !f.data_all_oneof.is_empty() && f.data_value {
        dh.insert("data-value-range");
    }
    if data_empty {
        dh.insert("data-empty-property");
    }

    // ── object disjointness ──────────────────────────────────────────────────
    let mut od: BTreeSet<&str> = BTreeSet::new();
    let obj_told_disjoint = !obj_disjoint_sets.is_empty() || !asymmetric.is_empty();
    if obj_told_disjoint {
        od.extend(oh.iter().copied().filter(|k| *k != "object-empty-property"));
    }
    if obj_empty {
        od.insert("object-empty-property");
    }
    let down = crate::property_disjoint_down_enabled();
    if !down
        && obj_disjoint_sets
            .iter()
            .flatten()
            .any(|m| oedges.iter().any(|(x, y)| y == m && x != m))
    {
        od.insert("object-disjoint-inheritance");
    }
    if !down && obj_disjoint_sets.iter().flatten().any(|m| m.1) {
        od.insert("object-disjoint-inverse-member");
    }
    let asymmetric_covered = down && crate::property_asymmetric_disjoint_enabled();
    if !asymmetric.is_empty() && !asymmetric_covered {
        od.insert("object-asymmetric");
    }
    let obj_constrained = f
        .obj_constrained
        .iter()
        .filter(|p| p.as_str() != OBJ_TOP && p.as_str() != OBJ_BOT)
        .count();
    if class_disjoint_source && obj_constrained >= 2 {
        od.insert("object-class-constraint");
    }

    // ── data disjointness ────────────────────────────────────────────────────
    let mut dd: BTreeSet<&str> = BTreeSet::new();
    if !data_disjoint_sets.is_empty() {
        dd.extend(dh.iter().copied().filter(|k| *k != "data-empty-property"));
    }
    if data_empty {
        dd.insert("data-empty-property");
    }
    if !down
        && data_disjoint_sets
            .iter()
            .flatten()
            .any(|m| dedges.iter().any(|(x, y)| y == m && x != m))
    {
        dd.insert("data-disjoint-inheritance");
    }
    let data_ranged = data_range_props.len() + f.data_all_oneof.len();
    if data_range_props.len() >= 2
        || data_ranged >= 2
        || (class_disjoint_source && f.data_constrained.len() >= 2)
    {
        dd.insert("data-class-constraint");
    }

    // ── data values ──────────────────────────────────────────────────────────
    let mut dv: BTreeSet<&str> = BTreeSet::new();
    // A one-value range or `∀d.{v}` on `d` (or a super-property of `d`), plus a
    // class that forces a `d` value, gives that value to every instance.
    let value_via_range = f.data_some.iter().any(|d| {
        f.data_all_oneof.contains(d)
            || f.data_all_oneof
                .iter()
                .any(|e| dedges.contains(&(d.clone(), e.clone())))
    });
    if (f.data_value || value_via_range) && abox {
        dv.insert("data-value-class-axiom");
    }
    if dpa {
        // Successors of one subject over every role at or below a bounded `t`
        // (sub-roles and declared inverses included), counted together.
        let mut per_bound: BTreeMap<(&String, &Signed), BTreeSet<&String>> = BTreeMap::new();
        for ((subj, r), objs) in &successors {
            for t in &bounded_obj {
                if t == r || oedges.contains(&(r.clone(), t.clone())) {
                    per_bound.entry((subj, t)).or_default().extend(objs);
                }
            }
        }
        let bounded_successors = per_bound.values().any(|objs| objs.len() >= 2);
        let nominal_on_bounded = f.obj_nominal_filler.iter().any(|r| {
            bounded_obj
                .iter()
                .any(|t| t == r || oedges.contains(&(r.clone(), t.clone())))
        });
        let shared_key = haskey_props.iter().any(|props| {
            props.first().is_some_and(|p| {
                key_values
                    .iter()
                    .any(|((kp, _), subjects)| kp == p && subjects.len() >= 2)
            })
        });
        if bounded_successors || nominal_on_bounded || shared_key || singleton_nominal_axiom {
            dv.insert("data-derived-equality");
        }
        if anon_individual && !crate::data_values_anonymous_enabled() {
            dv.insert("data-anonymous-individual");
        }
        dv.extend(dh.iter().copied().filter(|k| *k != "data-empty-property"));
    }

    let to_vec = |s: BTreeSet<&str>| s.into_iter().map(str::to_string).collect::<Vec<_>>();
    PropertyStructuralGaps {
        object_hierarchy: to_vec(oh),
        data_hierarchy: to_vec(dh),
        object_disjoint: to_vec(od),
        data_disjoint: to_vec(dd),
        data_values: to_vec(dv),
    }
}

fn note_domain_range<A: ForIRI>(
    ce: &CE<A>,
    p: &str,
    nothing: &mut BTreeSet<String>,
    named: &mut Vec<(String, String)>,
) {
    if let CE::Class(c) = ce {
        if c.0.as_ref() == NOTHING {
            nothing.insert(p.to_string());
        } else {
            named.push((p.to_string(), c.0.as_ref().to_string()));
        }
    }
}

/// A node below two members of one told disjointness set (or listed twice in one)
/// is disjoint from itself, i.e. empty.
/// `skip` excludes the seeded top/bottom nodes, which sit below everything; a
/// property told below `owl:bottomObjectProperty` is caught separately.
fn derives_self_disjoint<T: Ord + Clone>(
    edges: &BTreeSet<(T, T)>,
    sets: &[Vec<T>],
    skip: impl Fn(&T) -> bool,
) -> bool {
    let below = |m: &T| -> BTreeSet<T> {
        let mut s: BTreeSet<T> = edges
            .iter()
            .filter(|(x, y)| y == m && !skip(x))
            .map(|(x, _)| x.clone())
            .collect();
        s.insert(m.clone());
        s
    };
    sets.iter().any(|set| {
        for (i, a) in set.iter().enumerate() {
            for b in &set[i + 1..] {
                if a == b || !below(a).is_disjoint(&below(b)) {
                    return true;
                }
            }
        }
        false
    })
}
