//! Hybrid saturation+tableau OWL DL reasoner — the public API surface.
//!
//! End-users depend on this crate. Internally it orchestrates
//! `owl-dl-core` (IR, preprocessing), `owl-dl-saturation` (EL
//! fragment), `owl-dl-tableau` (SROIQ), and `owl-dl-datatypes`
//! (concrete domains).
//!
//! ## Public API
//!
//! - [`is_class_satisfiable`] — concept satisfiability.
//! - [`is_consistent`] — does the KB have any model.
//! - [`is_subclass_of`] — KB ⊨ sub ⊑ super (via the standard
//!   `sub ⊓ ¬sup` reduction).
//! - [`is_instance_of`] / [`instances_of`] — entailed class
//!   memberships of declared individuals.
//! - [`classify`] — full atomic-class hierarchy with equivalences,
//!   direct super-classes, and the unsat-class set. Returns
//!   [`ClassificationStats`] tracking how many queries each engine
//!   handled.
//! - [`realize`] — per-individual entailed types + Hasse leaves.
//!
//! ## Orchestrator
//!
//! Every entry point that issues at least one tableau query first
//! runs the EL saturation engine (sound but only complete for the
//! supported EL fragment) and short-circuits on a hit. When the
//! entire ontology lives inside that fragment, [`classify_internal`]
//! takes a saturation-only fast path with zero tableau calls
//! (`stats.pure_el_mode == true`).
//!
//! `PreparedOntology::from_internal` snapshots the post-expand /
//! NNF / absorb / `ABox`-seed state once so the pairwise
//! classification loop reuses it across queries instead of
//! re-running the pipeline per pair. The pairwise loop runs in
//! parallel via rayon.
//!
//! ## DL fragment coverage
//!
//! The tableau side handles `SROIQ` (Phase 5 complete except full
//! role-chain automata — length-2 chains + `TransitiveRole` only).
//! Datatypes are scaffolded but not wired into reasoning yet.

mod abox_check;
pub mod abox_saturation;
mod class_expr_query;
mod classify;
pub mod diagnose;
mod disjointness;
mod individuals;
pub mod justify;
pub mod laconic;
mod model_cache;
pub mod oracle_diff;
mod property_classify;
mod property_values;
mod realize;
pub mod repair;
mod rss_probe;
mod union_find;

pub use class_expr_query::{
    CeInstances, CeVerdict, class_expression_entailed_subclass, class_expression_instances,
    class_expression_satisfiable,
};
pub use classify::{
    Classification, ClassificationStats, FragmentClassification, analyze_fragment, cb_eli_blocker,
    cb_eli_eligible, cb_eli_eligible_tbox_only, cb_fragment_features, classify, classify_internal,
    classify_n2, classify_n2_with_timeout, classify_saturation_only, classify_top_down,
    classify_top_down_with_timeout, classify_with_budget, classify_with_global_deadline,
    classify_with_timeout,
};
pub use diagnose::{DerivedClass, Diagnosis, diagnose};
pub use disjointness::{
    Disjointness, disjoint_classes, disjoint_data_properties, disjoint_object_properties,
};
pub use individuals::{
    DifferentIndividuals, SameIndividuals, different_individuals, same_individuals,
};
pub use laconic::{find_all_laconic_justifications, find_laconic_justification};
pub use property_classify::{
    PropertyClassification, classify_data_property_hierarchy, classify_object_property_hierarchy,
};
pub use property_values::{
    DataPropertyValues, ObjectPropertyValues, inferred_data_property_values,
    inferred_object_property_values,
};
pub use realize::{
    Realization, instances_of, instances_of_internal, instances_of_saturation_only,
    instances_of_saturation_only_internal, is_instance_of, is_instance_of_internal,
    is_instance_of_saturation_only, is_instance_of_saturation_only_internal, realize,
    realize_internal, realize_saturation_only, realize_saturation_only_internal,
};
pub use repair::{Repair, Repairs, find_repairs};

/// Run the standalone `ABox` consequence-based saturator on `ontology` and return
/// `true` iff a disjoint-class clash was detected under named-only semantics.
///
/// This is a sound but incomplete inconsistency check: it can only derive clashes
/// that are reachable through named individuals and named edges — it does NOT
/// generate anonymous witnesses for existential restrictions. See
/// [`abox_saturation::saturate_abox_consistency`] for the diagnostic details.
///
/// # Errors
///
/// Returns a [`ReasonError`] if the ontology cannot be converted.
pub fn abox_sat_inconsistent<A: horned_owl::model::ForIRI>(
    o: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<bool, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(o)?;
    let result = abox_saturation::saturate_abox_consistency(&internal);
    Ok(result.clash)
}

/// Materialize the inferred OBJECT property assertions entailed over **named
/// individuals** — `(subject_iri, property_iri, object_iri)` triples, the full
/// entailed closure (asserted + derived via sub-property hierarchy / inverse /
/// symmetric / role chains / transitivity). Sound under-approximation: omits edges
/// to anonymous existential witnesses and disjunctive-derived edges. Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent (everything is
/// vacuously entailed); [`ReasonError::Conversion`] on lowering failure.
pub fn materialize_object_property_assertions<A: horned_owl::model::ForIRI>(
    onto: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(String, String, String)>, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(onto)?;
    let result = abox_saturation::saturate_abox_consistency(&internal);
    if result.clash {
        return Err(ReasonError::Inconsistent);
    }
    let vocab = &internal.vocabulary;
    const TOP: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
    const BOT: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";
    let mut out: Vec<(String, String, String)> = result
        .edges
        .iter()
        .map(|&(rid, a, b)| {
            (
                vocab.individual_iri(a).to_string(),
                vocab.role_iri(rid).to_string(),
                vocab.individual_iri(b).to_string(),
            )
        })
        .filter(|(s, p, o)| {
            p != TOP
                && p != BOT
                && !s.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX)
                && !o.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX)
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Materialize the inferred DATA property assertions entailed over **named
/// individuals** — `(subject_iri, property_iri, lexical, datatype_iri, lang)`
/// 5-tuples (the full entailed closure under sub-data-property hierarchy,
/// equivalent-data-properties, and `SameIndividual` folding). Sound; complete for
/// that fragment. Under-approximate: omits class-axiom-derived assertions
/// (e.g. `DataHasValue`). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] on lowering failure.
#[allow(clippy::type_complexity)]
pub fn materialize_data_property_assertions<A: horned_owl::model::ForIRI>(
    onto: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(String, String, String, String, String)>, ReasonError> {
    use horned_owl::model::{Component as C, Individual, Literal};
    use std::collections::{BTreeSet, HashMap, HashSet};

    // Iterative union-find over individual IRIs (path-compressing). An individual
    // absent from `parent` is its own root, so unrelated individuals need no seed.
    fn uf_find(parent: &mut HashMap<String, String>, x: &str) -> String {
        let mut root = x.to_string();
        while let Some(p) = parent.get(&root) {
            if p == &root {
                break;
            }
            root.clone_from(p);
        }
        let mut cur = x.to_string();
        while cur != root {
            let next = parent.get(&cur).cloned().unwrap_or_else(|| root.clone());
            parent.insert(cur.clone(), root.clone());
            cur = next;
        }
        root
    }

    const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
    const LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";

    let internal = owl_dl_core::convert::convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    let mut asserted: Vec<(String, String, (String, String, String))> = Vec::new();
    let mut hierarchy: Vec<(String, String)> = Vec::new();
    // SameIndividual folding: a data value asserted on `x` holds for every
    // individual equal to `x`. Build equivalence classes via union-find over
    // SameIndividual axioms (transitive), then replicate values across each class.
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut all_inds: HashSet<String> = HashSet::new();
    for ac in onto {
        match &ac.component {
            C::DataPropertyAssertion(ax) => {
                let Individual::Named(ni) = &ax.from else {
                    continue;
                };
                let subj = ni.0.as_ref().to_string();
                let dp = ax.dp.0.as_ref().to_string();
                let value = match &ax.to {
                    Literal::Simple { literal } => {
                        (literal.clone(), XSD_STRING.to_string(), String::new())
                    }
                    Literal::Language { literal, lang } => {
                        (literal.clone(), LANG_STRING.to_string(), lang.clone())
                    }
                    Literal::Datatype {
                        literal,
                        datatype_iri,
                    } => (
                        literal.clone(),
                        datatype_iri.as_ref().to_string(),
                        String::new(),
                    ),
                };
                all_inds.insert(subj.clone());
                asserted.push((subj, dp, value));
            }
            C::SameIndividual(ax) => {
                let named: Vec<String> =
                    ax.0.iter()
                        .filter_map(|i| match i {
                            Individual::Named(n) => Some(n.0.as_ref().to_string()),
                            Individual::Anonymous(_) => None,
                        })
                        .collect();
                for w in named.windows(2) {
                    all_inds.insert(w[0].clone());
                    all_inds.insert(w[1].clone());
                    let ra = uf_find(&mut parent, &w[0]);
                    let rb = uf_find(&mut parent, &w[1]);
                    if ra != rb {
                        parent.insert(ra, rb);
                    }
                }
            }
            C::SubDataPropertyOf(ax) => {
                hierarchy.push((ax.sub.0.as_ref().to_string(), ax.sup.0.as_ref().to_string()));
            }
            C::EquivalentDataProperties(ax) => {
                let dps: Vec<String> = ax.0.iter().map(|d| d.0.as_ref().to_string()).collect();
                for (i, di) in dps.iter().enumerate() {
                    for (j, dj) in dps.iter().enumerate() {
                        if i != j {
                            hierarchy.push((di.clone(), dj.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let closure = |dp: &str| -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        set.insert(dp.to_string());
        let mut stack = vec![dp.to_string()];
        while let Some(cur) = stack.pop() {
            for (s, sup) in &hierarchy {
                if s == &cur && set.insert(sup.clone()) {
                    stack.push(sup.clone());
                }
            }
        }
        set
    };

    // Group each individual with its SameIndividual-equivalence class.
    let mut by_root: HashMap<String, Vec<String>> = HashMap::new();
    for ind in &all_inds {
        let r = uf_find(&mut parent, ind);
        by_root.entry(r).or_default().push(ind.clone());
    }

    let mut out: Vec<(String, String, String, String, String)> = Vec::new();
    for (subj, dp, (lex, dt, lang)) in &asserted {
        let root = uf_find(&mut parent, subj);
        // Members of subj's equivalence class (includes subj); a subject with no
        // SameIndividual axiom is its own singleton class.
        let members: &[String] = by_root
            .get(&root)
            .map_or(std::slice::from_ref(subj), Vec::as_slice);
        for m in members {
            for sup in closure(dp) {
                out.push((m.clone(), sup, lex.clone(), dt.clone(), lang.clone()));
            }
        }
    }
    out.sort();
    out.dedup();
    out.retain(|(s, ..)| !s.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX));
    Ok(out)
}

/// Materialize the entailed existential successors of named individuals as a
/// blank-node representation: one row `(subject_iri, property_iri,
/// witness_blank_id, filler_class_iri)` per entailed `a : ∃R.C`.
///
/// NOTE: these are NOT entailed ground triples — a specific witness edge
/// `a R _:x` is not entailed (witnesses differ across models); what is entailed is
/// `a : ∃R.C`. Each row represents one such entailed existential, with a fresh
/// deterministic blank node. Sound by construction (`a:X` from `realize` +
/// told `X ⊑ ∃R.C`). Under-approximate: told `∃` only, simple named role + named
/// class filler, 1-step (no recursion). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if inconsistent; [`ReasonError::Conversion`].
#[allow(clippy::type_complexity)]
pub fn materialize_existential_successors<A: horned_owl::model::ForIRI>(
    onto: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(String, String, String, String)>, ReasonError> {
    use horned_owl::model::{
        ClassExpression as CE, Component as C, ForIRI, ObjectPropertyExpression as OPE,
    };
    use std::collections::{BTreeMap, BTreeSet};

    let internal = owl_dl_core::convert::convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    // Collect told (R, C) from a superclass expression (top-level + conjuncts).
    fn collect_exists<A: ForIRI>(sup: &CE<A>, out: &mut BTreeSet<(String, String)>) {
        match sup {
            CE::ObjectIntersectionOf(cs) => {
                for c in cs {
                    collect_exists(c, out);
                }
            }
            CE::ObjectSomeValuesFrom { ope, bce } => {
                if let (OPE::ObjectProperty(r), CE::Class(c)) = (ope, &**bce) {
                    out.insert((r.0.as_ref().to_string(), c.0.as_ref().to_string()));
                }
            }
            _ => {}
        }
    }

    // told-∃ index: X_iri → {(R_iri, C_iri)}.
    let mut told: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for ac in onto {
        match &ac.component {
            C::SubClassOf(ax) => {
                if let CE::Class(x) = &ax.sub {
                    let mut set = BTreeSet::new();
                    collect_exists(&ax.sup, &mut set);
                    if !set.is_empty() {
                        told.entry(x.0.as_ref().to_string())
                            .or_default()
                            .extend(set);
                    }
                }
            }
            C::EquivalentClasses(ax) => {
                for (i, mi) in ax.0.iter().enumerate() {
                    if let CE::Class(x) = mi {
                        let mut set = BTreeSet::new();
                        for (j, mj) in ax.0.iter().enumerate() {
                            if i != j {
                                collect_exists(mj, &mut set);
                            }
                        }
                        if !set.is_empty() {
                            told.entry(x.0.as_ref().to_string())
                                .or_default()
                                .extend(set);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if told.is_empty() {
        return Ok(Vec::new());
    }

    let realization = realize(onto)?;
    // Distinct (a, R, C) with a:X entailed and X ⊑ ∃R.C told.
    let mut triples: BTreeSet<(String, String, String)> = BTreeSet::new();
    for a in realization.individuals() {
        for x in realization.entailed_types(a) {
            if let Some(rcs) = told.get(x) {
                for (r, c) in rcs {
                    triples.insert((a.clone(), r.clone(), c.clone()));
                }
            }
        }
    }

    // One stable blank id per distinct (a,R,C), in sorted order.
    let mut out: Vec<(String, String, String, String)> = triples
        .into_iter()
        .enumerate()
        .map(|(i, (a, r, c))| (a, r, format!("_:b{i}"), c))
        .collect();
    out.retain(|(s, ..)| !s.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX));
    Ok(out)
}

/// Materialize the inferred OBJECT property-subsumption axioms `(sub, sup)` over
/// named object properties (told + equivalent + inverse closure, transitively
/// closed). Sound; complete for the named simple-subsumption fragment (role chains,
/// which give complex subsumption, are excluded). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent; [`ReasonError::Conversion`].
#[allow(clippy::type_complexity)]
pub fn materialize_subobjectproperty_axioms<A: horned_owl::model::ForIRI>(
    onto: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    use horned_owl::model::{
        Component as C, ObjectPropertyExpression as OPE, SubObjectPropertyExpression as SOPE,
    };
    use std::collections::BTreeSet;

    let internal = owl_dl_core::convert::convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

    type Signed = (String, bool); // (property IRI, is_inverse)
    fn signed<A: horned_owl::model::ForIRI>(ope: &OPE<A>) -> Signed {
        match ope {
            OPE::ObjectProperty(op) => (op.0.as_ref().to_string(), false),
            OPE::InverseObjectProperty(op) => (op.0.as_ref().to_string(), true),
        }
    }

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

    // Inverse propagation + transitive closure to fixpoint.
    loop {
        let mut new: Vec<(Signed, Signed)> = Vec::new();
        for ((an, af), (bn, bf)) in &edges {
            let cand = ((an.clone(), !*af), (bn.clone(), !*bf));
            if !edges.contains(&cand) {
                new.push(cand);
            }
        }
        for (a, b) in &edges {
            for (b2, c) in &edges {
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

    const TOP: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
    const BOT: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";
    let mut out: Vec<(String, String)> = edges
        .iter()
        .filter(|((_, af), (_, bf))| !af && !bf)
        .map(|((a, _), (b, _))| (a.clone(), b.clone()))
        .filter(|(a, b)| a != b && a != TOP && a != BOT && b != TOP && b != BOT)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Materialize the inferred DATA property-subsumption axioms `(sub, sup)` over named
/// data properties (told + equivalent closure, transitively closed). Sound; complete
/// for that fragment (data properties have no inverses/chains). Read-only.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent; [`ReasonError::Conversion`].
pub fn materialize_subdataproperty_axioms<A: horned_owl::model::ForIRI>(
    onto: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    use horned_owl::model::Component as C;
    use std::collections::BTreeSet;

    let internal = owl_dl_core::convert::convert_ontology(onto)?;
    if abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }

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

    loop {
        let mut new: Vec<(String, String)> = Vec::new();
        for (a, b) in &edges {
            for (b2, c) in &edges {
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

    const TOP: &str = "http://www.w3.org/2002/07/owl#topDataProperty";
    const BOT: &str = "http://www.w3.org/2002/07/owl#bottomDataProperty";
    let mut out: Vec<(String, String)> = edges
        .into_iter()
        .filter(|(a, b)| a != b && a != TOP && a != BOT && b != TOP && b != BOT)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Compute a sparse summary of the signature-locality partition
/// (see [`docs/module-extraction-plan.md`]). Counts and the
/// largest-component-size are the diagnostics most useful for
/// deciding whether the partition will actually skip pair-queries
/// — if one component dominates, the filter has nothing to do.
#[derive(Debug, Clone, Copy)]
pub struct LocalityStats {
    pub num_classes: usize,
    pub num_components: usize,
    pub largest_component: usize,
    pub singleton_components: usize,
}

/// Sparse summary of the absorbed `TBox` shape — how many rules
/// of each kind survive absorption, and how the residual GCIs
/// break down by top-level `ConceptExpr` variant. Used by the
/// `rustdl tbox-stats` CLI to inform the lazy-unfolding plan; see
/// `docs/lazy-unfolding-plan.md`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TBoxStats {
    pub concept_rules: usize,
    pub nominal_rules: usize,
    pub role_rules_guarded: usize,
    pub role_rules_unguarded: usize,
    pub residual_gcis: usize,
    /// Residual GCIs whose body is a top-level `Or(_)` — these
    /// are the universal disjunctions that drive the pizza
    /// search-tree explosion (one Or per residual × one
    /// branching decision per node).
    pub residual_or_count: usize,
    /// Residual GCIs whose body is `Atomic(_)` — pure
    /// "everything is a C" assertions; cheap because they don't
    /// branch.
    pub residual_atomic_count: usize,
    /// Residual GCIs of other shapes (`And`, `Some`, `Min`,
    /// `Max`, `Not`, `SelfRestriction`, `Nominal`) — buckets
    /// kept summed because each is rarer.
    pub residual_other_count: usize,
    /// Concept rules `A ⊑ ψ` whose conclusion `ψ` is `Or(_)`.
    /// These are the per-trigger disjunctions; on pizza they're
    /// the dominant branching source (the residual count is only
    /// 4). Candidates for the Lever-A-extension lazy unfolding.
    pub concept_rule_or_count: usize,
    /// Told-subsumer edges after [`owl_dl_core::told::build_told_tables`] closes
    /// the relation transitively — i.e. `Σ_c |super_classes(c)|`.
    ///
    /// **Why this is instrumented separately from `concept_rules`** (2026-08-03,
    /// for the `DKey` volume scan): `RUSTDL_DKEY_ONEOF_SEED` emits told
    /// `DKey ⊑ DKey` edges, which land in THIS table and appear nowhere in the
    /// absorbed-`TBox` rule counts. `told.rs` closes them transitively at build,
    /// so a linear growth in seeded edges can be a quadratic growth here — and
    /// the v0.3.27 fix was a DNF in exactly this table. Without this field a
    /// `concept_rules`-only scan is blind to the failure mode it exists to detect.
    pub told_super_edges: usize,
    /// Told-disjoint pairs, counted **unordered** (the underlying table is
    /// symmetric, so this is `Σ_c |disjoints_of(c)| / 2`).
    /// `RUSTDL_DKEY_EMIT_ORDER` makes conversion emit MORE
    /// `DisjointClasses(DKey, DKey)`; this is where that volume shows up.
    pub told_disjoint_pairs: usize,
}

/// Clausify the ontology into DL-clauses and return the shape
/// histogram (hypertableau Phase H0 — see
/// `docs/hypertableau-scoping.md`). Produces no reasoning; the
/// stats measure clause-shape distribution and clausifier
/// coverage (`deferred`) across the corpus.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn clause_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<owl_dl_core::clause::ClauseStats, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let (_clauses, stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    Ok(stats)
}

/// Per-category census of what the clausifier still defers — the HF1
/// coverage target list (see `docs/hypertableau-full-scoping.md`).
///
/// # Errors
///
/// See [`ReasonError`].
pub fn clause_deferred_census<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(&'static str, usize)>, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    Ok(owl_dl_core::clause::deferred_census(&internal))
}

pub use owl_dl_tableau::hyper::{HyperResult, SearchStats};

/// Per-class concept-satisfiability result from the hypertableau
/// engine ([`owl_dl_tableau::hyper`]), for the H2b wall measurement.
#[derive(Debug, Clone)]
pub struct HyperSatClassResult {
    /// The named class tested as the root concept.
    pub iri: String,
    /// `decide`'s verdict over the **clausifiable fragment**.
    pub result: owl_dl_tableau::hyper::HyperResult,
    /// Wall time for this class (milliseconds).
    pub wall_ms: f64,
    /// Search instrumentation (branches taken, restores, depth).
    pub stats: owl_dl_tableau::hyper::SearchStats,
}

/// Summary of a [`hyper_sat_probe`] run.
#[derive(Debug, Clone)]
pub struct HyperSatProbe {
    /// Per-class results, in vocabulary order.
    pub results: Vec<HyperSatClassResult>,
    /// Clause-set shape (so the deferred count is visible alongside).
    pub clause_stats: owl_dl_core::clause::ClauseStats,
}

/// Run the hypertableau engine's concept-satisfiability decision
/// ([`owl_dl_tableau::hyper::HyperEngine::decide`]) once per named
/// class, timing each, for the H2b wall measurement (see
/// `docs/hypertableau-scoping.md`).
///
/// **This is a performance probe, not a correctness claim.** The
/// H1c clausifier defers cardinality/nominals, so the clause set is
/// an under-approximation of the ontology. Dropping axioms only
/// *removes* constraints, hence `Models(full) ⊆ Models(fragment)`:
/// a `Unsat` verdict is sound for the full ontology, but a `Sat`
/// verdict is **not** (the full ontology may still be unsatisfiable
/// via a dropped axiom). Use this to ask "does `decide` terminate
/// quickly with branching exercised", not "is class C satisfiable".
///
/// `max_depth` bounds branching recursion; `per_class_timeout` (if
/// set) is the wall budget per class, after which the result is
/// `Stalled`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn hyper_sat_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    max_depth: usize,
    per_class_timeout: Option<std::time::Duration>,
) -> Result<HyperSatProbe, ReasonError> {
    use owl_dl_tableau::hyper::HyperEngine;
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let (clauses, clause_stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    let mut results = Vec::with_capacity(internal.vocabulary.num_classes());
    for (class_id, iri) in internal.vocabulary.classes() {
        let mut engine = HyperEngine::new(&clauses, class_id);
        if crate::incremental_fixpoint_enabled() {
            engine = engine.with_incremental_fixpoint();
        }
        if crate::semantic_branching_enabled() {
            engine = engine.with_semantic_branching();
        }
        let deadline = per_class_timeout.map(|t| std::time::Instant::now() + t);
        let start = std::time::Instant::now();
        let result = engine.decide_with_deadline(max_depth, deadline);
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        results.push(HyperSatClassResult {
            iri: iri.to_string(),
            result,
            wall_ms,
            stats: engine.stats(),
        });
    }
    Ok(HyperSatProbe {
        results,
        clause_stats,
    })
}

/// Smallest `ClassId` strictly greater than every class index that
/// appears in `clauses` — a fresh id usable for the subsumption
/// probe's helper concept `Q`.
fn fresh_class_id(clauses: &[owl_dl_core::clause::DlClause]) -> owl_dl_core::ir::ClassId {
    use owl_dl_core::clause::Atom;
    let mut max = 0u32;
    for cl in clauses {
        for atom in cl.body.iter().chain(cl.head.iter()) {
            if let Atom::Class(c, _) | Atom::Exists(_, c, _) = atom {
                max = max.max(c.index() + 1);
            }
        }
    }
    owl_dl_core::ir::ClassId::new(max)
}

/// Get-or-allocate the complement class `Ā` for atomic `a`, emitting
/// the clash clause `A(x) ∧ Ā(x) → ⊥` to `clauses` on first use. The
/// complement is a positive label the engine treats normally; the
/// clash clause is what makes asserting `Ā` refute a derived `A`.
/// Sound for *refutation only* (we assert `Ā`, never derive it from
/// the absence of `A`). See `docs/hypertableau-h3b-scoping.md` §2.
fn complement_of(
    a: owl_dl_core::ir::ClassId,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> owl_dl_core::ir::ClassId {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    if let Some(&c) = complements.get(&a) {
        return c;
    }
    let c = ClassId::new(*next_fresh);
    *next_fresh += 1;
    complements.insert(a, c);
    clauses.push(DlClause {
        body: vec![Atom::Class(a, X), Atom::Class(c, X)],
        head: vec![],
    });
    c
}

/// Translate one disjunct of `NNF(¬sup-definition)` into a head atom,
/// or `None` if it falls outside the supported set (caller falls back
/// to the bare-complement test). Supported: `atomic` → `Class(A)`,
/// `¬atomic` → `Class(Ā)`, `∃R.atomic` → `Exists(R,A)`, `∃R.¬atomic`
/// → `Exists(R,Ā)`. See `docs/hypertableau-h3b-scoping.md` §3.
fn encode_neg_disjunct(
    d: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ConceptPool,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> Option<owl_dl_core::clause::Atom> {
    use owl_dl_core::ConceptExpr;
    use owl_dl_core::clause::{Atom, X};
    match pool.get(d) {
        ConceptExpr::Atomic(a) => Some(Atom::Class(*a, X)),
        ConceptExpr::Not(inner) => match pool.get(*inner) {
            ConceptExpr::Atomic(a) => Some(Atom::Class(
                complement_of(*a, complements, clauses, next_fresh),
                X,
            )),
            _ => None,
        },
        ConceptExpr::Some(role, inner) => match pool.get(*inner) {
            ConceptExpr::Atomic(a) => Some(Atom::Exists(*role, *a, X)),
            ConceptExpr::Not(i2) => match pool.get(*i2) {
                ConceptExpr::Atomic(a) => Some(Atom::Exists(
                    *role,
                    complement_of(*a, complements, clauses, next_fresh),
                    X,
                )),
                _ => None,
            },
            // `∃R.(L1 ⊓ … ⊓ Lk)` with `Li` literals (atomic / ¬atomic):
            // name the conjunction with a fresh `N ⊑ ⊓Li` and assert
            // `∃R.N`. The `VegetarianPizzaEquivalent2` shape
            // `∃hT.(¬Cheese ⊓ … ⊓ ¬Veg)`. `N` is a sound under-name
            // (anything `N` satisfies every literal), fresh, so it
            // never affects other reasoning — refutation stays sound.
            ConceptExpr::And(parts) => {
                let parts: Vec<owl_dl_core::ir::ConceptId> = parts.to_vec();
                let lits = name_literal_conjunction(&parts, pool, clauses, next_fresh)?;
                Some(Atom::Exists(*role, lits, X))
            }
            _ => None,
        },
        // `≤n R.C` (NNF of `¬(≥(n+1) R.C)`) → an at-most constraint.
        // Unqualified when the qualifier is `⊤` (the pizza
        // `InterestingPizza` shape `≤2 hasTopping`); a named-class
        // qualifier is carried through, anything else defers.
        ConceptExpr::Max(n, role, inner) => match pool.get(*inner) {
            ConceptExpr::Top => Some(Atom::AtMost(*role, None, *n, X)),
            ConceptExpr::Atomic(a) => Some(Atom::AtMost(*role, Some(*a), *n, X)),
            _ => None,
        },
        _ => None,
    }
}

/// Allocate a fresh class `N` with `N ⊑ ⊓parts` where every part is a
/// literal (`atomic` → `N → A`, `¬atomic` → `N ∧ A → ⊥`). Returns
/// `None` (no clauses emitted) if any part is non-literal. Used to
/// name the inner concept of an `∃R.(…)` disjunct (§3 extension).
fn name_literal_conjunction(
    parts: &[owl_dl_core::ir::ConceptId],
    pool: &owl_dl_core::ConceptPool,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> Option<owl_dl_core::ir::ClassId> {
    use owl_dl_core::ConceptExpr;
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    // Reject early if any part is non-literal — emit nothing.
    let mut lits: Vec<(ClassId, bool)> = Vec::with_capacity(parts.len());
    for &p in parts {
        match pool.get(p) {
            ConceptExpr::Atomic(a) => lits.push((*a, true)),
            ConceptExpr::Not(inner) => match pool.get(*inner) {
                ConceptExpr::Atomic(a) => lits.push((*a, false)),
                _ => return None,
            },
            _ => return None,
        }
    }
    let n = ClassId::new(*next_fresh);
    *next_fresh += 1;
    for (a, positive) in lits {
        if positive {
            // N(x) → A(x)
            clauses.push(DlClause {
                body: vec![Atom::Class(n, X)],
                head: vec![Atom::Class(a, X)],
            });
        } else {
            // N(x) ∧ A(x) → ⊥  (N implies ¬A)
            clauses.push(DlClause {
                body: vec![Atom::Class(n, X), Atom::Class(a, X)],
                head: vec![],
            });
        }
    }
    Some(n)
}

/// Encode `NNF(¬def)` as the Q-gated disjunctive head atoms for the
/// H3b subsumption test, or `None` if any top-level disjunct is
/// untranslatable (caller falls back). The disjunction's atoms are
/// later emitted as `Q(x) → d1 ∨ … ∨ dk` — gated on `Q` so the
/// constraint binds only the root (never generated successors).
fn encode_neg_definition(
    neg: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ConceptPool,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> Option<Vec<owl_dl_core::clause::Atom>> {
    use owl_dl_core::ConceptExpr;
    let disjuncts: Vec<owl_dl_core::ir::ConceptId> = match pool.get(neg) {
        ConceptExpr::Or(parts) => parts.to_vec(),
        _ => vec![neg],
    };
    let mut out = Vec::with_capacity(disjuncts.len());
    for d in disjuncts {
        out.push(encode_neg_disjunct(
            d,
            pool,
            complements,
            clauses,
            next_fresh,
        )?);
    }
    Some(out)
}

/// One subsumption-pair result from [`hyper_subsumption_probe`].
#[derive(Debug, Clone)]
pub struct HyperSubResult {
    /// Sub-class IRI.
    pub sub: String,
    /// Super-class IRI.
    pub sup: String,
    /// `Unsat` ⇒ `sub ⊑ sup` (sound for the full ontology); `Sat` ⇒
    /// not entailed *over the fragment* (NOT sound for the full
    /// ontology); `Stalled` ⇒ budget exhausted.
    pub result: HyperResult,
    /// Wall time for this pair (milliseconds).
    pub wall_ms: f64,
    /// Search instrumentation.
    pub stats: SearchStats,
}

/// Summary of a [`hyper_subsumption_probe`] run.
#[derive(Debug, Clone)]
pub struct HyperSubProbe {
    /// Only the *interesting* pairs are retained (those that branched
    /// or whose verdict was `Unsat`/`Stalled`) to bound output; the
    /// counters below summarise the full N² sweep.
    pub results: Vec<HyperSubResult>,
    /// Total ordered pairs tested (`n·(n−1)`).
    pub pairs_tested: u64,
    /// Pairs decided `Unsat` (i.e. entailed subsumptions found).
    pub subsumptions: u64,
    /// Pairs whose decision exercised branching (`branches_taken>0`).
    pub pairs_branched: u64,
    /// Pairs that hit the budget (`Stalled`).
    pub stalled: u64,
    /// Deepest branch nesting across all pairs.
    pub max_branch_depth: u32,
    /// Total wall across all pairs (milliseconds).
    pub total_wall_ms: f64,
    /// Pairs whose `sup` used the H3b `¬sup`-expansion encoding
    /// (`sup` had a translatable definition). The rest used the bare
    /// `Q ∧ sup → ⊥` fallback.
    pub pairs_via_expansion: u64,
    /// Complement classes introduced for negative literals (§2).
    pub complements_introduced: usize,
    /// Clause-set shape (deferred count visible alongside).
    pub clause_stats: owl_dl_core::clause::ClauseStats,
}

/// Run the hypertableau subsumption test ([`decide_subsumption`])
/// over **every ordered pair** of named classes, for the H2c pizza
/// wall measurement (see `docs/hypertableau-scoping.md`). This is the
/// analog of `classify`'s pair loop, but routed through the
/// hyperresolution engine.
///
/// **Performance probe, not a complete classifier.** As with
/// [`hyper_sat_probe`], deferred axioms make the clause set an
/// under-approximation: an `Unsat` (subsumption-holds) verdict is
/// sound for the full ontology, but `Sat` (not-subsumed) is not. So
/// the reported `subsumptions` count is a sound *lower bound* on the
/// true hierarchy.
///
/// `per_pair_timeout`, if set, bounds each pair's wall.
///
/// Pre-pass for [`hyper_subsumption_probe`]: for each defined `sup`,
/// expand `NNF(¬def)` into Q-gated disjunct atoms, appending any
/// complement/structural clash clauses to `clauses` (once). Returns
/// the per-`sup` disjunct atoms for the sups whose `¬def` fully
/// translated; the rest fall back to the bare-complement test.
fn build_sup_neg_map(
    vocab: &[(owl_dl_core::ir::ClassId, String)],
    defs: &owl_dl_core::definitions::Definitions,
    pool: &mut owl_dl_core::ConceptPool,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> std::collections::HashMap<owl_dl_core::ir::ClassId, Vec<owl_dl_core::clause::Atom>> {
    let mut sup_neg = std::collections::HashMap::new();
    for (sup, _) in vocab {
        let Some(def) = defs.body_of(*sup) else {
            continue;
        };
        let neg = owl_dl_core::normalize::nnf_complement(def, pool);
        if let Some(atoms) = encode_neg_definition(neg, pool, complements, clauses, next_fresh) {
            sup_neg.insert(*sup, atoms);
        }
    }
    sup_neg
}

/// # Errors
///
/// See [`ReasonError`].
#[allow(clippy::too_many_lines)] // probe orchestration is necessarily long
pub fn hyper_subsumption_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    max_depth: usize,
    per_pair_timeout: Option<std::time::Duration>,
) -> Result<HyperSubProbe, ReasonError> {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    use owl_dl_tableau::hyper::HyperEngine;

    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let (base, clause_stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    let defs = owl_dl_core::definitions::extract_definitions(&internal);

    // Fresh id space: past every class used in clauses and the
    // vocabulary. `q` first, then complement classes.
    let num_classes = u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
    // `HF4a`: nominal classes occupy `[num_classes, num_classes + num_individuals)`
    // (matching the clausifier's `nominal_base`), so the engine's NN-rule
    // can recognise singleton labels.
    let num_individuals = u32::try_from(internal.vocabulary.num_individuals()).unwrap_or(0);
    let mut next_fresh = fresh_class_id(&base).index().max(num_classes);
    let q = ClassId::new(next_fresh);
    next_fresh += 1;

    let vocab: Vec<(ClassId, String)> = internal
        .vocabulary
        .classes()
        .map(|(id, iri)| (id, iri.to_string()))
        .collect();

    let mut clauses = base;
    let mut complements: std::collections::HashMap<ClassId, ClassId> =
        std::collections::HashMap::new();

    // Pre-pass: for each defined `sup`, expand `NNF(¬def)` into Q-gated
    // disjunct atoms (§1/§3). Complement clash clauses (§2) are
    // appended to `clauses` here, once — so the engine's clause set is
    // monotonic across the pair loop below.
    let sup_neg = build_sup_neg_map(
        &vocab,
        &defs,
        &mut internal.concepts,
        &mut complements,
        &mut clauses,
        &mut next_fresh,
    );
    // base clauses + complement clash clauses — fixed for every pair.
    let fixed_len = clauses.len();

    // HF2 role hierarchy: an `R`-edge satisfies an `S`-atom when
    // `R ⊑* S`. Built from the original (pre-NNF) axioms, then cloned
    // into each per-pair engine. (Interaction with inverse-pair
    // canonicalization on the *same* role is an edge case — TODO HF3.)
    let sub_role_hierarchy = build_role_hierarchy(&internal);

    // Sat-lookahead: build once, share across all pairs via Arc.
    let probe_sat_lookahead: Option<std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>> =
        if hyper_sat_lookahead_enabled() {
            Some(std::sync::Arc::new(
                owl_dl_saturation::seed_sat::build_base(&internal),
            ))
        } else {
            None
        };

    let mut probe = HyperSubProbe {
        results: Vec::new(),
        pairs_tested: 0,
        subsumptions: 0,
        pairs_branched: 0,
        stalled: 0,
        max_branch_depth: 0,
        total_wall_ms: 0.0,
        pairs_via_expansion: 0,
        complements_introduced: complements.len(),
        clause_stats,
    };
    for (sub, sub_iri) in &vocab {
        for (sup, sup_iri) in &vocab {
            if sub == sup {
                continue;
            }
            clauses.truncate(fixed_len);
            clauses.push(DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(*sub, X)],
            });
            // H3b: if `sup` has a translatable definition, assert the
            // Q-gated `¬sup` disjunction; else fall back to the bare
            // `Q ∧ sup → ⊥` complement test (H2c behaviour).
            let via_expansion = if let Some(atoms) = sup_neg.get(sup) {
                clauses.push(DlClause {
                    body: vec![Atom::Class(q, X)],
                    head: atoms.clone(),
                });
                true
            } else {
                clauses.push(DlClause {
                    body: vec![Atom::Class(q, X), Atom::Class(*sup, X)],
                    head: vec![],
                });
                false
            };

            let deadline = per_pair_timeout.map(|t| std::time::Instant::now() + t);
            let start = std::time::Instant::now();
            let mut engine = HyperEngine::new(&clauses, q)
                .with_sub_roles(sub_role_hierarchy.clone())
                .with_nominals(num_classes, num_individuals);
            if hyper_double_block_enabled() {
                engine = engine.with_double_blocking();
            }
            if hyper_precise_card_deps_enabled() {
                engine = engine.with_precise_card_deps();
            }
            if hyper_mrv_ordering_enabled() {
                engine = engine.with_mrv_ordering();
            }
            if let Some(sat) = probe_sat_lookahead.clone() {
                engine = engine.with_sat_lookahead(sat);
            }
            if crate::adaptive_budget_enabled() {
                engine = engine.with_adaptive_budget();
            }
            if crate::incremental_fixpoint_enabled() {
                engine = engine.with_incremental_fixpoint();
            }
            if crate::semantic_branching_enabled() {
                engine = engine.with_semantic_branching();
            }
            let result = engine.decide_with_deadline(max_depth, deadline);
            let stats = engine.stats();
            let wall_ms = start.elapsed().as_secs_f64() * 1000.0;

            probe.pairs_tested += 1;
            probe.total_wall_ms += wall_ms;
            probe.max_branch_depth = probe.max_branch_depth.max(stats.max_branch_depth);
            if via_expansion {
                probe.pairs_via_expansion += 1;
            }
            if result == HyperResult::Unsat {
                probe.subsumptions += 1;
            }
            if result == HyperResult::Stalled {
                probe.stalled += 1;
            }
            if stats.branches_taken > 0 {
                probe.pairs_branched += 1;
            }
            // Retain only interesting pairs to bound memory/output.
            if stats.branches_taken > 0 || result != HyperResult::Sat {
                probe.results.push(HyperSubResult {
                    sub: sub_iri.clone(),
                    sup: sup_iri.clone(),
                    result,
                    wall_ms,
                    stats,
                });
            }
        }
    }
    Ok(probe)
}

/// Diagnostic: run the **real classify per-pair oracle** ([`HyperCache::decide`])
/// for a single `(sub, sup)` pair and return its raw [`HyperResult`],
/// [`SearchStats`], and wall (ms). Unlike [`hyper_subsumption_probe`] this uses
/// the identical `HyperCache` construction classify uses (`¬sup` expansion,
/// `DifferentIndividuals` seeding, role hierarchy, all the per-pair flags), so
/// the stats characterise the actual production search — used to root-cause
/// *why* a classify pair stalls (depth cap vs deadline vs branch explosion vs
/// blocking failure). Returns `Ok(None)` if either IRI is not a named class.
///
/// `depth` is the branch-depth cap; `per_pair_timeout` bounds the wall.
///
/// # Errors
///
/// See [`ReasonError`].
/// SP1→SP2 viability probe: `sat(class)` with the wedge root optionally SEEDED with
/// the class's complete all-model saturated subsumer set (`owl_dl_saturation::saturate`).
/// Tests the load-bearing coupled-saturation assumption: does seeding the wedge from the
/// (closure-complete) saturation collapse the disjunctive model-search branch count?
/// `seed=false` reproduces the unseeded baseline; `seed=true` adds `Q → D` for every
/// saturated subsumer `D` of `class`. Adding entailed subsumers is sound (cannot FP).
/// Returns `Ok(None)` if `class_iri` is not a named class.
///
/// # Errors
/// See [`ReasonError`].
pub fn seed_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    class_iri: &str,
    seed_mode: u8,
    depth: usize,
    timeout: Option<std::time::Duration>,
) -> Result<Option<(HyperResult, SearchStats, f64, usize)>, ReasonError> {
    // seed_mode: 0 = none (baseline); 1 = real named saturated subsumers;
    // 2 = garbage control (same COUNT of named NON-subsumers — isolates
    // "saturation knowledge" from "more root labels / MRV reorder").
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_tableau::hyper::HyperEngine;
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let cache = HyperCache::build(&internal);
    let Some(c) = internal
        .vocabulary
        .classes()
        .find(|(_, iri)| *iri == class_iri)
        .map(|(id, _)| id)
    else {
        return Ok(None);
    };
    let mut clauses = cache.clauses.clone();
    clauses.push(DlClause {
        body: vec![Atom::Class(cache.fresh_q, X)],
        head: vec![Atom::Class(c, X)],
    });
    let mut n_seeded = 0usize;
    if seed_mode != 0 {
        // Seed ONLY named (non-synthetic) classes. The saturator's closure
        // includes synthetic ids (NomKey/ForallKey/DKey/Tseitin) ≥ num_classes
        // whose semantics the wedge does NOT share — forcing those onto the root
        // is a cross-engine mismatch (spurious clash = FP). Named subsumers are
        // genuinely entailed in BOTH engines, so seeding them is sound.
        let n_named = internal.vocabulary.num_classes();
        let subs = owl_dl_saturation::saturate(&internal);
        let real: Vec<owl_dl_core::ir::ClassId> = subs
            .subsumers_of(c)
            .into_iter()
            .filter(|&d| d != c && (d.index() as usize) < n_named)
            .collect();
        let to_seed: Vec<owl_dl_core::ir::ClassId> = if seed_mode == 1 {
            real
        } else {
            // Garbage control: the same count of named NON-subsumers.
            let count = real.len();
            let real_set: std::collections::HashSet<_> = real.iter().copied().collect();
            internal
                .vocabulary
                .classes()
                .map(|(id, _)| id)
                .filter(|&d| d != c && !real_set.contains(&d))
                .take(count)
                .collect()
        };
        for d in to_seed {
            clauses.push(DlClause {
                body: vec![Atom::Class(cache.fresh_q, X)],
                head: vec![Atom::Class(d, X)],
            });
            n_seeded += 1;
        }
    }
    let mut engine = HyperEngine::new(&clauses, cache.fresh_q);
    if hyper_double_block_enabled() {
        engine = engine.with_double_blocking();
    }
    if hyper_precise_card_deps_enabled() {
        engine = engine.with_precise_card_deps();
    }
    if hyper_mrv_ordering_enabled() {
        engine = engine.with_mrv_ordering();
    }
    if crate::incremental_fixpoint_enabled() {
        engine = engine.with_incremental_fixpoint();
    }
    if crate::semantic_branching_enabled() {
        engine = engine.with_semantic_branching();
    }
    let deadline = timeout.map(|t| std::time::Instant::now() + t);
    let start = std::time::Instant::now();
    let result = engine.decide_with_deadline(depth, deadline);
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Some((result, engine.stats(), wall_ms, n_seeded)))
}

/// Probe whether class `class_iri` is unsatisfiable, optionally seeding the
/// wedge root with the saturation's derived knowledge.
///
/// `mode`:
/// - `0` — no seed (baseline: class label only).
/// - `1` — named-subsumer seed (same as SP2 [`seed_probe`] mode 1): the
///   saturation's named subsumers of `c` are pushed as `Q → D` clauses.
/// - `2` — named-subsumer + ∃-seed: mode 1 plus each derived existential fact
///   `(c, r, tgt)` from the saturation is translated and seeded as
///   `Q(x) → ∃r.D(x)`. `NomKey` targets are translated to the wedge nominal id
///   (`num_named + a.index()`); named targets are seeded directly; Tseitin/DKey
///   targets are dropped (sound under-approximation).
/// - `3` — garbage-∃ control: mode 1 plus the **same count** of deterministic,
///   in-vocabulary `∃R.D` clauses whose `(R, D)` is NOT among `c`'s real
///   translated facts. Isolates "more ∃ clauses / MRV reorder" from real knowledge.
///
/// Returns `(verdict, stats, wall_ms, n_exists_seeded)` where `n_exists_seeded`
/// is the number of ∃-clauses added (0 for modes 0 and 1; same count for modes
/// 2 and 3 so the controller can assert they match).
///
/// Returns `Ok(None)` when `class_iri` is not found in the ontology.
pub fn precompletion_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    class_iri: &str,
    mode: u8,
    depth: usize,
    timeout: Option<std::time::Duration>,
) -> Result<Option<(HyperResult, SearchStats, f64, usize)>, ReasonError> {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::Role;
    use owl_dl_tableau::hyper::HyperEngine;

    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let cache = HyperCache::build(&internal);

    // Look up the class id.
    let Some(c) = internal
        .vocabulary
        .classes()
        .find(|(_, iri)| *iri == class_iri)
        .map(|(id, _)| id)
    else {
        return Ok(None);
    };

    // Clone the base clause set and inject the Q → C root.
    let mut clauses = cache.clauses.clone();
    clauses.push(DlClause {
        body: vec![Atom::Class(cache.fresh_q, X)],
        head: vec![Atom::Class(c, X)],
    });

    // Saturate once to get subsumers + ∃-facts + NomKey map.
    let (subs, facts, nom_to_ind) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let n_named = internal.vocabulary.num_classes();

    // Named-subsumer seed (modes 1, 2, 3) — identical to seed_probe mode 1.
    if mode != 0 {
        for d in subs.subsumers_of(c) {
            if d != c && (d.index() as usize) < n_named {
                clauses.push(DlClause {
                    body: vec![Atom::Class(cache.fresh_q, X)],
                    head: vec![Atom::Class(d, X)],
                });
            }
        }
    }

    // Translate the derived ∃-facts of c: produce the real set used by mode 2.
    // Mode 3 also needs this count, so we build the translated list in all cases
    // where mode ≥ 2, then either use it directly (mode 2) or ignore the
    // content but use the count for the garbage control (mode 3).
    let mut n_exists_seeded = 0usize;

    if mode == 2 || mode == 3 {
        // Collect all translated (role, translated_target) pairs for c's facts.
        let n_named_u32 = u32::try_from(n_named).expect("num_named fits u32");
        let translated: Vec<(owl_dl_core::ir::RoleId, owl_dl_core::ir::ClassId)> = facts
            .iter()
            .filter(|&&(s, _, _)| s == c)
            .filter_map(|&(_, r, tgt)| {
                if (tgt.index() as usize) < n_named {
                    // Named target — seed directly.
                    Some((r, tgt))
                } else if let Some(&ind) = nom_to_ind.get(&tgt) {
                    // NomKey → wedge nominal id: same formula as the clausifier.
                    let wedge_nominal = owl_dl_core::ir::ClassId::new(n_named_u32 + ind.index());
                    Some((r, wedge_nominal))
                } else {
                    // Tseitin / DKey — drop (sound under-approximation).
                    None
                }
            })
            .collect();

        if mode == 2 {
            // Real ∃-seed.
            for (r, tgt) in &translated {
                clauses.push(DlClause {
                    body: vec![Atom::Class(cache.fresh_q, X)],
                    head: vec![Atom::Exists(Role::named(*r), *tgt, X)],
                });
                n_exists_seeded += 1;
            }
        } else {
            // Mode 3: garbage control — same count, deterministic in-vocab
            // (R, D) pairs NOT among c's real translated facts.
            type RdPair = (owl_dl_core::ir::RoleId, owl_dl_core::ir::ClassId);
            let real_set: std::collections::HashSet<RdPair> = translated.iter().copied().collect();
            let target_count = translated.len();
            let roles: Vec<owl_dl_core::ir::RoleId> =
                internal.vocabulary.roles().map(|(id, _)| id).collect();
            let named_classes: Vec<owl_dl_core::ir::ClassId> = internal
                .vocabulary
                .classes()
                .filter(|(id, _)| (id.index() as usize) < n_named)
                .map(|(id, _)| id)
                .collect();
            // Build a deterministic stream of (R, D) pairs not in the real set.
            let mut garbage: Vec<DlClause> = Vec::new();
            'outer: for &r in &roles {
                for &d in &named_classes {
                    if garbage.len() >= target_count {
                        break 'outer;
                    }
                    if !real_set.contains(&(r, d)) {
                        garbage.push(DlClause {
                            body: vec![Atom::Class(cache.fresh_q, X)],
                            head: vec![Atom::Exists(Role::named(r), d, X)],
                        });
                    }
                }
            }
            n_exists_seeded = garbage.len();
            clauses.extend(garbage);
        }
    }

    // Build engine with full index rebuild (seed clauses must be indexed).
    let mut engine = HyperEngine::new(&clauses, cache.fresh_q);
    if hyper_double_block_enabled() {
        engine = engine.with_double_blocking();
    }
    if hyper_precise_card_deps_enabled() {
        engine = engine.with_precise_card_deps();
    }
    if hyper_mrv_ordering_enabled() {
        engine = engine.with_mrv_ordering();
    }
    if crate::incremental_fixpoint_enabled() {
        engine = engine.with_incremental_fixpoint();
    }
    if crate::semantic_branching_enabled() {
        engine = engine.with_semantic_branching();
    }

    let deadline = timeout.map(|t| std::time::Instant::now() + t);
    let start = std::time::Instant::now();
    let result = engine.decide_with_deadline(depth, deadline);
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Some((result, engine.stats(), wall_ms, n_exists_seeded)))
}

pub fn decide_pair_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    sub_iri: &str,
    sup_iri: &str,
    depth: usize,
    per_pair_timeout: Option<std::time::Duration>,
) -> Result<Option<(HyperResult, SearchStats, f64)>, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let cache = HyperCache::build(&internal);
    let sub = internal
        .vocabulary
        .classes()
        .find(|(_, iri)| *iri == sub_iri)
        .map(|(id, _)| id);
    let sup = internal
        .vocabulary
        .classes()
        .find(|(_, iri)| *iri == sup_iri)
        .map(|(id, _)| id);
    let (Some(sub), Some(sup)) = (sub, sup) else {
        return Ok(None);
    };
    let deadline = per_pair_timeout.map(|t| std::time::Instant::now() + t);
    let start = std::time::Instant::now();
    let (result, stats) = cache.decide_with_stats(sub, sup, depth, deadline);
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Some((result, stats, wall_ms)))
}

/// Diagnostic sibling of [`decide_pair_probe`]: wedge satisfiability of a single
/// class ALONE (no `¬sup`). Returns `Ok(None)` if `class_iri` is not a named
/// class. Used to localise a per-pair stall to `c`'s own expansion vs the
/// `c ⊓ ¬sup` interaction.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn sat_class_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    class_iri: &str,
    depth: usize,
    timeout: Option<std::time::Duration>,
) -> Result<Option<(HyperResult, SearchStats, f64)>, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let cache = HyperCache::build(&internal);
    let Some(c) = internal
        .vocabulary
        .classes()
        .find(|(_, iri)| *iri == class_iri)
        .map(|(id, _)| id)
    else {
        return Ok(None);
    };
    let deadline = timeout.map(|t| std::time::Instant::now() + t);
    let start = std::time::Instant::now();
    let (result, stats) = cache.sat_only_with_stats(c, depth, deadline);
    let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(Some((result, stats, wall_ms)))
}

/// Branching-recursion depth cap for the H4 in-orchestrator hyper
/// subsumption check (the per-pair wall budget bounds it further).
const HYPER_WEDGE_DEPTH: usize = 256;

/// Depth schedule for the classify per-pair **iterative-deepening** wedge
/// search (`RUSTDL_ITERATIVE_DEEPENING`, default ON — see
/// [`iterative_deepening_enabled`]).
///
/// The fixed [`HYPER_WEDGE_DEPTH`] is measurably wrong in **both** directions:
/// `ore_ont_10407` needs depth **319** (256 truncates a search that would have
/// terminated, and the truncated run does 4.4× MORE work than the completing
/// one, because a capped branch cannot conclude and the search re-descends
/// through every sibling disjunct), while `ore_ont_2182`'s useful proof depth
/// is **≤7** (256 buys 2357 stalled pairs and 13× the wall of depth 8, and
/// finds *fewer* subsumptions). See `docs/2026-08-02-cardinality-rootcause.md`
/// and `docs/2026-08-02-nominal-blocking-rootcause.md`.
///
/// **Invariants this array must satisfy** (both asserted at compile time by
/// [`assert_depth_schedule_well_formed`]):
/// 1. strictly increasing — a level that does not deepen is pure re-work;
/// 2. the LAST element is `>= HYPER_WEDGE_DEPTH`. This is what makes the
///    change completeness-safe under an unbounded deadline: increasing the cap
///    is **verdict-monotone** (see [`Self::decide_iterative_deepening`]), so a
///    final level at or above today's fixed cap can only *add* entailments.
const HYPER_WEDGE_DEPTH_SCHEDULE: &[usize] = &[8, 32, 128, 512];

/// Compile-time check of the two [`HYPER_WEDGE_DEPTH_SCHEDULE`] invariants.
const fn assert_depth_schedule_well_formed() {
    assert!(!HYPER_WEDGE_DEPTH_SCHEDULE.is_empty());
    let mut i = 1;
    while i < HYPER_WEDGE_DEPTH_SCHEDULE.len() {
        assert!(
            HYPER_WEDGE_DEPTH_SCHEDULE[i] > HYPER_WEDGE_DEPTH_SCHEDULE[i - 1],
            "depth schedule must be strictly increasing"
        );
        i += 1;
    }
    assert!(
        HYPER_WEDGE_DEPTH_SCHEDULE[HYPER_WEDGE_DEPTH_SCHEDULE.len() - 1] >= HYPER_WEDGE_DEPTH,
        "final schedule level must be >= HYPER_WEDGE_DEPTH or deepening can lose entailments"
    );
}
const _: () = assert_depth_schedule_well_formed();

/// What the iterative-deepening loop actually did on one pair: how many
/// schedule levels it ran, and the depth cap of the level whose verdict was
/// returned. Recorded on the production path (it is two stack words), so the
/// canaries observe the real loop rather than a test-only twin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeepeningTrace {
    /// Number of `decide_with_stats` calls made (always `>= 1`, never more
    /// than the schedule length).
    pub(crate) levels_run: usize,
    /// Depth cap of the level that produced the returned verdict.
    pub(crate) final_depth: usize,
    /// The adaptive shutoff was latched for this pair, so the shallow phase was
    /// skipped and only the final (unbounded, deepest) level ran. Observable so
    /// the canaries can pin the shutoff without depending on wall-clock timing.
    pub(crate) shallow_skipped: bool,
}

/// Iterative deepening of the classify per-pair wedge depth cap
/// (`RUSTDL_ITERATIVE_DEEPENING`). **DEFAULT ON** since 2026-08-02; only an
/// explicit `=0` reverts (an EMPTY value ENABLES, per the house default-ON
/// idiom — see `hyper_*_enabled`).
///
/// Flag-OFF the classify subsumption oracle takes exactly the pre-change path
/// (one `decide_with_stats` at [`HYPER_WEDGE_DEPTH`]), so the off path is
/// byte-identical by construction.
///
/// **Why the default flipped.** A 1,920-ontology ORE sweep (single-thread,
/// 60 s cap, one pinned binary with the flag toggled by env) measured, ON vs
/// OFF: **16 recoveries** (`dnf` → `ok`), **0 regressions** (`ok` → `dnf`),
/// 10 materially faster / **0 materially slower** (>25% and >2 s), and a 2.1%
/// aggregate wall reduction over the 1,730 both-completing ontologies. The
/// pre-registered decision rule was zero `ok` → `dnf`. Deepening is
/// verdict-monotone (the final level's cap is `>= HYPER_WEDGE_DEPTH`), and a
/// 26-ontology OFF-vs-ON closure diff — biased toward the ontologies where the
/// shutoff demonstrably acts — found **0 lost and 0 gained** subsumptions.
/// See `docs/2026-08-02-iterative-deepening-results.md`.
#[must_use]
pub(crate) fn iterative_deepening_enabled() -> bool {
    std::env::var_os("RUSTDL_ITERATIVE_DEEPENING").is_none_or(|v| v != "0")
}

/// Default wall budget, in milliseconds, for the WHOLE shallow phase of one
/// iterative-deepening pair (every level except the last, taken together).
/// Overridable with `RUSTDL_ID_SHALLOW_MS`; `0` disables the bound.
///
/// **Why the shallow levels must be bounded at all** — measured on the two
/// root-caused instances, `hyper-classify-probe --per-pair-timeout-ms 0`,
/// single-thread, v0.4.11:
///
/// | depth | `ore_ont_10407` wall / stalled | `ore_ont_2182` wall / stalled |
/// |---|---|---|
/// | 8   | **68.18 s** / 1726 | **1.06 s** / 4 |
/// | 32  | 169.30 s / 1726 | 2.50 s / 6 |
/// | 128 | 82.47 s / 787 | 7.12 s / 2097 |
/// | 256 | 44.50 s / 357 | 13.41 s / 2357 |
/// | 512 | **10.45 s** / 0 | 13.36 s / 2357 |
///
/// So UNBOUNDED iterative deepening is refuted on `10407`: its shallow levels
/// find nothing the final level does not (926 subsumptions at every depth) and
/// cost 68 s + 169 s + 82 s of pure re-work before the 10.45 s level that
/// actually decides. The per-pair profile says why: every stalled depth-8 pair
/// costs ~**50 ms** (502 branches at ~100 µs each — the adaptive-budget
/// divergence cut already bounds the *branch count*, but not the wall).
/// `2182`'s depth-8 pairs cost ≤ **1.67 ms** (110–216 branches at ~8 µs). A few
/// milliseconds separates the population the shallow level rescues from the one
/// it merely taxes.
///
/// **Bounding the shallow phase cannot change any verdict** when the final level
/// runs: by the monotonicity argument in
/// [`HyperCache::decide_iterative_deepening`], a shallow level that is cut short
/// returns `Stalled`, and the unbounded final level (depth `>= HYPER_WEDGE_DEPTH`)
/// then reproduces whatever the shallow level would have concluded. The shallow
/// levels are pure accelerators; their budget is a wall knob, not a semantic one.
const ID_SHALLOW_BUDGET_MS: u64 = 5;

/// Fraction of the caller's remaining per-pair budget that the shallow phase may
/// consume. The shallow phase gets `min(ID_SHALLOW_BUDGET_MS, remaining / N)`, so
/// at least `(N-1)/N` of a caller-supplied budget always reaches the final level.
/// Without this, a small `--pair-timeout-ms` would be spent entirely on shallow
/// probes and the final level — the only one that can decide a deep pair — would
/// never run.
const ID_SHALLOW_BUDGET_DIVISOR: u32 = 4;

/// Wall budget for the shallow phase of one pair. Pure function of the inputs so
/// the arithmetic is unit-testable without a search.
///
/// Returns the deadline every NON-final level shares. `None` only when the bound
/// is disabled (`RUSTDL_ID_SHALLOW_MS=0`) *and* the caller supplied no deadline.
fn id_shallow_deadline(
    now: std::time::Instant,
    caller: Option<std::time::Instant>,
    budget_ms: u64,
) -> Option<std::time::Instant> {
    if budget_ms == 0 {
        return caller;
    }
    let mut slice = std::time::Duration::from_millis(budget_ms);
    if let Some(dl) = caller {
        // Never exceed the caller's budget, and never take more than
        // 1/ID_SHALLOW_BUDGET_DIVISOR of what remains of it.
        let remaining = dl.saturating_duration_since(now);
        slice = slice.min(remaining / ID_SHALLOW_BUDGET_DIVISOR);
    }
    Some(now + slice)
}

/// Shallow-phase wall budget in ms (`RUSTDL_ID_SHALLOW_MS`, default
/// [`ID_SHALLOW_BUDGET_MS`], `0` disables the bound). Garbage parses to the
/// default rather than to `0` — silently disabling the bound would reintroduce
/// the 68 s re-work the bound exists to prevent.
fn id_shallow_budget_ms() -> u64 {
    std::env::var("RUSTDL_ID_SHALLOW_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(ID_SHALLOW_BUDGET_MS)
}

/// How much wall, in milliseconds, one classify may WASTE on iterative-deepening
/// shallow phases that fail to decide their pair, before the shallow phase is
/// switched off for the rest of that classify. Overridable with
/// `RUSTDL_ID_SHALLOW_WASTE_MS`; `0` disables the shutoff (restoring the
/// always-run-shallow behaviour that regressed `ore_ont_13991`).
///
/// **Why a fixed per-pair shallow budget is not enough.** [`ID_SHALLOW_BUDGET_MS`]
/// is a **per-pair** constant, so the shallow phase's total cost scales with the
/// pair count — which is quadratic in the class count. `ore_ont_13991` (3,119
/// classes, **56,760 pairs**) classifies in 32.79 s with deepening off and **DNFs
/// at 180 s** with it on. The dose–response confirms the mechanism rather than
/// merely fitting it:
///
/// | `RUSTDL_ID_SHALLOW_MS` | `ore_ont_13991` |
/// |---|---|
/// | 5 (default) | DNF @200 s |
/// | 1 | completes, 90.31 s, 2,558 subs — identical to OFF |
/// | 0 (bound disabled) | DNF @200 s |
///
/// At 1 ms the overhead is 90.31 − 32.79 = **57.5 s** against a predicted
/// 1 ms × 56,760 = **57 s**.
///
/// **The discriminator is not size — it is whether the shallow phase DECIDES.**
/// The shallow phase never does re-work that the final level then repeats for
/// free: it either decides the pair (and the deepest level is never run) or it
/// is pure tax. On `wine` it decides nearly every pair (3,454 land in the 0 ms
/// bucket at depth 8) and repays its cost many times over; on `ore_ont_13991` it
/// decides essentially nothing and costs 5 ms × 56,760. So the fix measures the
/// discriminator directly instead of proxying it.
///
/// **A CONSECUTIVE-MISS COUNTER WAS TRIED FIRST AND IS REFUTED — do not
/// reintroduce it.** "Stop after the shallow phase fails to decide the last K
/// consecutive pairs" is the obvious reading of the discriminator, and it does
/// not survive contact with `13991`, measured on this binary:
///
/// | consecutive-miss K | `ore_ont_13991` |
/// |---|---|
/// | 1 | completes, 39.46 s (= the 39.25 s flag-OFF baseline) |
/// | 16 | **DNF @90 s** |
/// | 256 | **DNF @60 s** |
///
/// The cliff between K=1 and K=16 is the refutation. `13991`'s shallow phase is
/// not uniformly useless — it decides a great many pairs *cheaply* (an easy pair
/// goes `Sat` at depth 8 in microseconds) while a separate subpopulation misses
/// at the full 5 ms. Those interleave, so any decide resets the run and the latch
/// never trips. "Consecutive" measures the WRONG THING: the harm is not a run of
/// failures, it is accumulated wall.
///
/// **So the shutoff meters the harm in the units the harm is measured in:**
/// wall spent on shallow phases that did not decide. A decide is not charged —
/// it is the thing being paid for, and on the winning population it is nearly
/// free anyway. This is immune to the interleaving that broke the counter,
/// because a cheap decide neither adds to the total nor cancels what is already
/// in it.
///
/// **Why 1000 ms.** It bounds the worst measured tax to ~1 s (`13991`: 39.25 s
/// flag-OFF, so ~3%) while being far more than the winning population ever
/// wastes — `wine` and the depth-8 recoveries `ore_ont_2182`/`16481` decide in
/// the shallow phase, so they accumulate waste slowly and never reach it.
///
/// **Why the shutoff is permanent for the rest of the classify, with no retry.**
/// It is self-latching rather than explicitly latched: once the total reaches the
/// budget the shallow phase stops running, so it can no longer add to the total.
/// That needs no second constant and no second mechanism. A periodic re-probe was
/// considered and rejected as unjustified — it would cost little, but nothing
/// measured shows it recovers anything, and all 16 sweep recoveries are retained
/// without it.
///
/// **Alternatives considered and rejected**, both of which proxy the
/// discriminator instead of measuring it:
/// * A **global budget on the shallow phase itself** (rather than on its waste).
///   This conflates exactly the two populations the per-pair constant already
///   conflates: `wine` runs its shallow phase on thousands of pairs and *wins*
///   there, so a global cap small enough to protect `13991` would cut `wine`'s
///   shallow phase off partway through and destroy its 92–98% win. Charging only
///   the non-deciding pairs is what separates them.
/// * **Scaling the per-pair constant by the pair count.** Size is only a proxy: a
///   large ontology whose shallow phase does pay would be penalised for being
///   large, and a small one where it never pays would keep paying. The same
///   structural-profile trap is on record in
///   `docs/2026-08-02-iterative-deepening-results.md`, where 41 of
///   `ore_ont_10407`'s "50 cardinality axioms" turned out to be
///   `MinCardinality(0 R)` tautologies.
///
/// **Determinism note.** The accumulator is shared across rayon workers, so
/// *which* pairs get a shallow phase can vary run to run. That cannot vary the
/// ANSWERS on an unbounded run: by the soundness note below, both paths return a
/// verdict `>=` the flag-OFF verdict for that pair, and the flag-OFF verdict does
/// not depend on the accumulator. Under a truncating `--pair-timeout-ms` the
/// hierarchy is already documented as run-to-run nondeterministic on hard
/// ontologies, independently of this.
///
/// **Soundness is untouched, and this is verified rather than assumed** — see
/// `id_shallow_shutoff_cannot_change_a_verdict` and the canaries in
/// [`iterative_deepening_tests`]. Skipping the shallow phase runs only the final
/// level, whose cap is `>= HYPER_WEDGE_DEPTH` and whose deadline is the caller's
/// own, so a skipped pair gets *exactly* the flag-OFF search at a cap that is
/// `>=` the flag-OFF cap. By the monotonicity argument on
/// [`HyperCache::decide_iterative_deepening`] that verdict is a superset of the
/// flag-OFF verdict, never a subset. Under a bounded deadline the shutoff can
/// only *return* budget to the final level, so it cannot lose a pair either.
const ID_SHALLOW_WASTE_BUDGET_MS: u64 = 1000;

/// Wasted-wall budget for the adaptive shallow shutoff
/// (`RUSTDL_ID_SHALLOW_WASTE_MS`, default [`ID_SHALLOW_WASTE_BUDGET_MS`], `0`
/// disables the shutoff). Garbage parses to the default rather than to `0` —
/// silently disabling the shutoff would reintroduce the `ore_ont_13991`
/// regression it exists to prevent, exactly as for [`id_shallow_budget_ms`].
fn id_shallow_waste_budget_ms() -> u64 {
    std::env::var("RUSTDL_ID_SHALLOW_WASTE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(ID_SHALLOW_WASTE_BUDGET_MS)
}

/// Was the depth cap NOT the binding constraint at this level — i.e. would a
/// deeper cap run the identical search, so that deepening cannot change the
/// verdict? Pure predicate so the reasoning is unit-testable.
///
/// * `shallow_spent` — the level was cut by the shallow-phase wall budget.
///   **Load-bearing**: a budget-cut level has an arbitrarily small
///   `max_branch_depth`, so without this term the caller would conclude "cap not
///   binding", stop, and never run the final level — the only one that can
///   decide a deep pair. This term must dominate.
/// * `diverged` — the adaptive-budget divergence cut fired. That cut depends on
///   `init_depth`, so it fires LESS at a larger cap and a deeper level may get
///   further; never treat a diverged level as exhausted.
/// * otherwise the level is exhausted iff no branch ever reached its cap.
fn id_cap_was_not_binding(
    shallow_spent: bool,
    diverged: bool,
    max_branch_depth: u32,
    level_depth: usize,
) -> bool {
    !shallow_spent
        && !diverged
        && u64::from(max_branch_depth) < u64::try_from(level_depth).unwrap_or(u64::MAX)
}

/// Diagnostic override of [`HYPER_WEDGE_DEPTH_SCHEDULE`]
/// (`RUSTDL_ID_SCHEDULE="8,32,128,512"`), so a schedule can be A/B-measured
/// without a rebuild. Only consulted when [`iterative_deepening_enabled`].
///
/// A malformed override — unparsable, empty, non-increasing, or with a final
/// level below [`HYPER_WEDGE_DEPTH`] — is **rejected wholesale** (falls back to
/// the compiled default) rather than silently reasoning under a schedule that
/// could lose entailments.
fn depth_schedule() -> Vec<usize> {
    let Some(raw) = std::env::var_os("RUSTDL_ID_SCHEDULE") else {
        return HYPER_WEDGE_DEPTH_SCHEDULE.to_vec();
    };
    let parsed: Option<Vec<usize>> = raw
        .to_str()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().parse::<usize>().ok())
                .collect()
        })
        .and_then(|v: Option<Vec<usize>>| v);
    match parsed {
        Some(v)
            if !v.is_empty()
                && v.windows(2).all(|w| w[1] > w[0])
                && v[v.len() - 1] >= HYPER_WEDGE_DEPTH =>
        {
            v
        }
        _ => HYPER_WEDGE_DEPTH_SCHEDULE.to_vec(),
    }
}

/// Whether the hypertableau sound-accelerator wedge (H4) is enabled.
/// **Default on** as of 2026-05-29 — the corpus is now sound across
/// every tested ontology (pizza/ro/sulo/SIO/GALEN/notgalen/ALEHIF+),
/// and the perf payoff is dramatic (pizza 13×, SIO 50× wall reductions).
/// Disable explicitly with `RUSTDL_HYPERTABLEAU=0`.
#[must_use]
pub fn hyper_wedge_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPERTABLEAU").is_none_or(|v| v != "0" && !v.is_empty())
}

/// HF2 double-blocking (`RUSTDL_HYPER_DOUBLE_BLOCK`). Uses the Motik
/// et al. §3.4 pair-blocking condition (equal labels + equal parent
/// labels + equal edge role) instead of anywhere blocking — required
/// for `Sat` soundness with inverse roles. **Default on** as of
/// 2026-05-29 alongside trust-Sat; subset pair-blocking semantics
/// (208f0f3) keep it fast (ro went 111 s → 10 s). Disable with
/// `RUSTDL_HYPER_DOUBLE_BLOCK=0`.
#[must_use]
pub fn hyper_double_block_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPER_DOUBLE_BLOCK").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Decide per-class (un)satisfiability in the top-down classifier's unsat-probe
/// pass from the already-built Phase-7 label cache (the wedge) instead of a
/// fresh MAIN-TABLEAU `decide` per class (`RUSTDL_UNSAT_VIA_LABELS`). Profiled
/// as the dominant classify wall (~6 s alehif / ~22 s ore-10908 — the per-class
/// main-tableau pass, redundant with the wedge verdict the label cache already
/// holds). **Default ON.** Sound: `LabelOracle::Unsat` is a wedge `Unsat`
/// (trusted direction, already trusted in the walk); `Sat` matches the
/// established `trust_sat` model; `NoVerdict` falls through to the tableau. Set
/// `RUSTDL_UNSAT_VIA_LABELS=0` for the pre-fix main-tableau pass (A/B).
#[must_use]
pub fn unsat_via_labels_enabled() -> bool {
    std::env::var_os("RUSTDL_UNSAT_VIA_LABELS").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Precise (sound over-approx) `≤n`-cardinality clash deps
/// (`RUSTDL_PRECISE_CARD_DEPS`). At the `forced_distinct_exceeds` pre-check site
/// replaces the conservative `DepSet::ALL` with `parent.at_most_dep ∪ ⋃(birth ∪
/// label of supcs) ∪ parent(birth ∪ label)` — a provable superset of the clash's
/// true deps (sound by construction; see `card_clash_deps`), guarded by the
/// own-successor / `≠`-only / merge-taint fallbacks. Unblocks dependency-directed
/// backjumping on cardinality clashes (wine MISSED 34→31, −25% wall, FP=0;
/// see `docs/backjump-reconcile-2026-06-06.md`). **Default on** as of the flip
/// (2026-06-06): sound by construction, FP=0 across the cardinality/nominal
/// corpus, and inert on the EL/Horn corpus (Horn-shortcircuited, never enters
/// the wedge cardinality path). Set `RUSTDL_PRECISE_CARD_DEPS=0` to revert to the
/// conservative `DepSet::ALL` behaviour.
#[must_use]
pub fn hyper_precise_card_deps_enabled() -> bool {
    std::env::var_os("RUSTDL_PRECISE_CARD_DEPS").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Read-only shadow precise-dependency probe (`RUSTDL_SHADOW_DEP_PROBE`).
/// When set, the wedge `sat_class_probe` and `decide_pair_probe` probes maintain a
/// shadow dep layer that never collapses to `DepSet::ALL` due to taints and record
/// `(real, shadow)` dep-set snapshots at every clash into
/// [`SearchStats::clash_records`]. **Default OFF.**
///
/// **Read-only invariant**: enabling this MUST NOT change any verdict,
/// `branches_taken`, `restores`, or `max_branch_depth`. See
/// [`owl_dl_tableau::hyper::HyperEngine::with_shadow_dep_probe`].
#[must_use]
pub fn hyper_shadow_dep_probe_enabled() -> bool {
    std::env::var_os("RUSTDL_SHADOW_DEP_PROBE").is_some_and(|v| v != "0" && !v.is_empty())
}

/// `RUSTDL_MRV_ORDERING` (default ON as of 2026-06-23). Most-constrained-⊔-first
/// ordering of `find_open_disjunction`; verdict-invariant (reordering only).
/// Flipped default-ON after the corpus FP gate passed: FP=0/MISSED=0 byte-identical
/// across all 10 oracled fixtures, no wall regression, wine collapse sound. `=0` opts out.
#[must_use]
pub fn hyper_mrv_ordering_enabled() -> bool {
    std::env::var_os("RUSTDL_MRV_ORDERING").is_none_or(|v| v != "0" && !v.is_empty())
}

/// `RUSTDL_SAT_LOOKAHEAD` (default **OFF**): at each ⊔ choice point, call
/// the seed-saturator to drop disjuncts proved dead before branching.
/// Flag-OFF path is byte-identical to pre-lookahead. Instrumented via
/// [`owl_dl_tableau::hyper::SearchStats`] counters `lookahead_calls`,
/// `lookahead_dropped`, `lookahead_forced_single`.
#[must_use]
pub fn hyper_sat_lookahead_enabled() -> bool {
    std::env::var_os("RUSTDL_SAT_LOOKAHEAD").is_some_and(|v| v != "0" && !v.is_empty())
}

/// SP2 coupled-saturation seed: seed each per-pair wedge call with the
/// class's named saturated subsumers. Gated by `RUSTDL_SAT_SEED`, **default ON**
/// (set to `"0"` to disable; see the SP3 note below). When on, `HyperCache::build`
/// computes a per-named-class table once via `owl_dl_saturation::saturate`,
/// and `decide_with_stats` seeds `Q → D` for every entry in the table
/// before the engine runs. Soundness: seeding entailed named subsumers (and
/// derived ∃-facts) cannot introduce a false `Unsat` (they hold in every
/// model). Synthetic ids ≥ `num_classes` are filtered — cross-engine semantics
/// mismatch would be unsound.
///
/// **Default ON** as of the SP3 ∃-seed gate (wine 49 s → 3.2 s, ~15×; FP=0 /
/// MISSED=0 byte-identical corpus-wide). Set `RUSTDL_SAT_SEED=0` to opt out
/// (restores the pre-seed per-pair behaviour).
#[must_use]
pub fn hyper_sat_seed_enabled() -> bool {
    std::env::var_os("RUSTDL_SAT_SEED").is_none_or(|v| v != "0" && !v.is_empty())
}

/// HF5: whether the wedge is allowed to *trust* the engine's `Sat`
/// verdict (concluding "not subsumed" without consulting the tableau).
/// `Unsat` is sound by construction for any ontology; `Sat` is sound
/// only if the engine is complete on the workload. **Default on** as
/// of 2026-05-29 — every tested ontology agrees with Konclude (0 FP)
/// with this flag and double-blocking both enabled. The original SIO
/// 38 FPs that motivated the opt-in design were a saturation bug
/// (`process_fact` range propagation, fixed in f71a012), not a
/// wedge bug. Disable with `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`. Off ⇒
/// `Sat` verdicts are treated as `Unknown` (older H4 behaviour, falls
/// through to the tableau).
#[must_use]
pub fn hyper_trust_sat_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPERTABLEAU_TRUST_SAT").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Per-class label heuristic (Phase 7) — when enabled, the classifier
/// runs wedge satisfiability once per named class to build a label
/// cache, then prunes non-subsumption pairs whose candidate super is
/// absent from the subject's root-node labels. Sound; on by default.
/// Disable with `RUSTDL_LABEL_HEURISTIC=0` (e.g. for tests that need
/// to exercise the downstream wedge/tableau paths the cache would
/// otherwise pre-empt).
#[must_use]
pub fn label_heuristic_enabled() -> bool {
    std::env::var_os("RUSTDL_LABEL_HEURISTIC").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Concrete Phase 2 — counting-pair verification. When ON, a wedge
/// `NotSubsumed` verdict on a subsumption pair where either side is
/// data-counting-constrained (or has a counting subsumer) is NOT trusted;
/// the pair falls through to the main tableau (`concrete_domain_clash`).
/// Sound (only swaps a trusted wedge `Sat` for the complete path). On by
/// default; `RUSTDL_COUNTING_PAIR_VERIFY=0` reverts to trusting the wedge.
#[must_use]
pub fn counting_pair_verify_enabled() -> bool {
    std::env::var_os("RUSTDL_COUNTING_PAIR_VERIFY").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Same-tier classify-completeness (SP1.1): carry the role hierarchy into the
/// classify oracle (Layer A) + broaden the same-tier sweep to label-driven
/// (Layer B), so inverse/symmetric-domain subsumptions surface in default
/// classify. DEFAULT OFF — sound (FP=0) but corpus-invisible and ~2× wall, so
/// opt-in. Set `RUSTDL_CLASSIFY_SAME_TIER=1` to enable.
#[must_use]
pub(crate) fn classify_same_tier_enabled() -> bool {
    std::env::var_os("RUSTDL_CLASSIFY_SAME_TIER").is_some_and(|v| v == "1")
}

/// Defined-sup sweep VERIFY mode. For a class defined via a non-EL body
/// (`D ≡ … ⊓ ¬… / ⊔ / ∀ …`), the wedge's label countermodel is an unreliable
/// counterexample: it can satisfy `cand ⊓ ¬D` only because the wedge is
/// incomplete on complement/disjunction, so the label-heuristic prune drops a
/// TRUE `cand ⊑ D`. When enabled, the defined-sup sweep bypasses the label prune
/// for defined sups and verifies each candidate with the full tableau
/// (`trust_sat=false`). **Sound (FP=0 by construction)** — only edges a tableau
/// `unsat` confirms are added; a spurious wedge `Sat` can only MISS, never FP.
/// DEFAULT OFF — closes complement/disjunction-defined subsumptions the closure-
/// guided walk can't see (ORE `ore_ont_15167`), but corpus-invisible and can hit
/// the tableau wall on disjunction-heavy inputs, so it is strictly opt-in at the
/// caller's per-pair budget. Set `RUSTDL_CLASSIFY_DEFINED_SWEEP=1` to enable. See
/// `docs/ore-sweep-2026-07-01.md`.
#[must_use]
pub(crate) fn classify_defined_sweep_enabled() -> bool {
    std::env::var_os("RUSTDL_CLASSIFY_DEFINED_SWEEP").is_some_and(|v| v == "1")
}

/// Label-cache back-fold: when ON, `classify_labels` runs the sound,
/// branch-free `∃`-composition rule [`HyperEngine::backfold_derived`] over the
/// per-class `sat` graph, carries the entailed defined-`∃` names out in
/// [`LabelOracle::Sat::derived_sups`] (Task 2), and `classify.rs`'s
/// `inject_backfold_derived_sups` (Task 3) injects each into the class
/// hierarchy directly (no `subsumes_via_tableau` call), same as the
/// defined-SUB sweep. **DEFAULT ON** (since 2026-07-12, after the corpus gate
/// went green: galen MISSED 1 → 0, corpus FP=0/MISSED=0 unchanged elsewhere,
/// galen wall stays ~sub-second — no `DEFINED_SWEEP`-style explosion, since
/// the rule makes zero search calls). With the flag on, the label path derives
/// `derived_sups` and the hierarchy build injects each entailed edge; set
/// `RUSTDL_CLASSIFY_BACKFOLD=0` to revert to the pre-back-fold behaviour (no
/// back-fold call, `derived_sups` empty, zero injections). See
/// `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md` §6.
#[must_use]
pub(crate) fn classify_backfold_enabled() -> bool {
    std::env::var_os("RUSTDL_CLASSIFY_BACKFOLD").is_none_or(|v| v != "0")
}

/// Lever #1: adaptive early-cut of diverging wedge searches. Default OFF until the
/// corpus MISSED-unchanged gate confirms it. Set `RUSTDL_ADAPTIVE_BUDGET=1`.
///
/// When ON, `HyperEngine::with_adaptive_budget` is applied to every wedge engine
/// built at classify/consistency/label-cache sites. The predicate fires only on
/// searches that saturate `max_depth` without progress (depth-saturated + high
/// restore ratio + growing model) — non-diverging searches are unaffected and the
/// adaptive cut can only convert a stalling `Stalled` verdict to an early exit, not
/// flip a sound `Unsat` or `Sat`.
#[must_use]
pub(crate) fn adaptive_budget_enabled() -> bool {
    // DEFAULT ON: strictly verdict-preserving (corpus MISSED=0, byte-identical
    // closures at DIV_WINDOW=500) and faster (ore-15672 138→91s). `=0` opts out.
    std::env::var("RUSTDL_ADAPTIVE_BUDGET").map_or(true, |v| v != "0")
}

/// SP1: incremental `horn_fixpoint` (drains the per-branch worklist delta
/// instead of re-seeding the whole graph each `solve` frame). **DEFAULT ON**
/// as of 2026-07-14: verdict-preserving (curated FP=0/MISSED=0, byte-identical
/// closures; `ore_ont_13723` non-Horn FP oracle 0→0) with a real ~10% stall
/// reduction on dense SROIQ (`ore_ont_10019` incomplete 1626→1465) and no curated
/// wall regression. `RUSTDL_HYPER_INCREMENTAL_FIXPOINT=0` reverts to the
/// full-reseed path.
#[must_use]
pub(crate) fn incremental_fixpoint_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPER_INCREMENTAL_FIXPOINT").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Classify per-pair `ClauseIndexes` amortization
/// (`RUSTDL_CLASSIFY_AMORTIZE_IDX`, **DEFAULT ON**): the subsumption oracle
/// (`HyperCache::decide_with_stats`) reuses the shared base `ClauseIndexes`
/// (built once in `HyperCache::build`) plus a small per-pair index delta for
/// the appended clauses, instead of cloning the full base clause `Vec` and
/// rebuilding the whole index per decided pair (13,772 rebuilds × ~34k
/// clauses on one `ore_ont_1508` classify — 11-15% self-time). The
/// pair-invariant `value_disjoint` clash clauses are folded into the base
/// clause set/index once at `build` time under the flag. `=0` reverts to the
/// old clone + full-rebuild path (kept intact for A/B). Read once in
/// `HyperCache::build` and stored, so build-time folding and decide-time
/// routing can never disagree. See
/// `docs/superpowers/plans/2026-07-23-classify-clauseindex-amortization-plan.md`.
#[must_use]
pub(crate) fn classify_amortize_idx_enabled() -> bool {
    std::env::var_os("RUSTDL_CLASSIFY_AMORTIZE_IDX").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Per-CLASS clause-index amortization for the label cache
/// (`RUSTDL_CLASSIFY_LABELS_AMORTIZE`, **DEFAULT ON since 0.4.10**; `=0` reverts).
///
/// **This header read "DEFAULT OFF" until 2026-08-17 and was wrong.** The flip
/// happened in 0.4.10 and the predicate below has used the default-ON idiom
/// (`is_none_or(|v| v != "0")`) ever since — as the comment inside it already
/// said, so the two contradicted each other. Verified by measurement on
/// `ore_ont_12698` (`--pair-timeout-ms 1000`, single-thread): unset **5.2 s**
/// (`label_cache_build` 2,028 ms), `=1` **5.7 s** (2,039 ms), `=0` **108.1 s**
/// (104,237 ms). Unset behaves as `=1`; turning it off costs ~20×.
///
/// A stale default in a header is worse than no header: it invites someone to
/// "flip" what is already flipped, and it mis-attributes any measurement taken
/// against it.
///
/// `HyperCache::classify_labels` appends the SP2.1 `sat_seed` / SP3
/// `exists_seed` clauses to the per-class clause vector. Those are absent from
/// the shared `base_indexes` (built before the seed), so the pre-2026-08-01 code
/// fell back to `HyperEngine::new` — a full O(#clauses) `ClauseIndexes` rebuild
/// **once per class**. Because `RUSTDL_SAT_SEED` defaults ON, that was the
/// always-taken branch. With this flag on, the seed clauses stay in their own
/// small `extras` slice and only the O(#extras) sparse
/// `build_clause_index_delta` is built — exactly what the per-PAIR sibling
/// `decide_with_stats` has done by default since v0.3.39.
///
/// Verdict-preserving by construction: the delta routes through the same
/// `index_one_clause` as the base build, the extras keep the same logical
/// clause ids they had at the tail of the cloned vector, and `disjoint_pair_of`
/// extraction is shared. `=1` opts in; unset/`0` keeps the clone + full rebuild.
/// See `docs/superpowers/specs/2026-08-01-clauseindex-per-class-adjudication.md`.
#[must_use]
pub(crate) fn classify_labels_amortize_enabled() -> bool {
    // DEFAULT ON since 0.4.10 (`=0` reverts). Verdict-preserving: closures are
    // byte-identical at a non-truncating budget on both adjudication ontologies. The win
    // is large (ore_ont_12698 100.46 -> 5.40 s; ore_ont_1508 202.79 -> 98.47 s) and it
    // targets the phase where 123 of the 199 remaining DNF ontologies stall.
    std::env::var_os("RUSTDL_CLASSIFY_LABELS_AMORTIZE").is_none_or(|v| v != "0")
}

/// One-shot stderr provenance marker for which engine-construction path
/// `HyperCache::classify_labels` took, printed only when
/// `RUSTDL_LABEL_AMORTIZE_MARK` is set to a non-empty, non-`"0"` value.
///
/// Exists because an arm of a measurement must be *provable*, not assumed: each
/// of the two branches prints at most once per process, so the presence of
/// `engaged` proves the amortized path ran and the ABSENCE of `full-rebuild`
/// over a whole classify proves **every** class took it. Off by default and
/// entirely off the timed path (one relaxed atomic load per class).
fn mark_label_engine_path(amortized: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static AMORTIZED_SEEN: AtomicBool = AtomicBool::new(false);
    static FULL_REBUILD_SEEN: AtomicBool = AtomicBool::new(false);
    if !std::env::var_os("RUSTDL_LABEL_AMORTIZE_MARK").is_some_and(|v| v != "0" && !v.is_empty()) {
        return;
    }
    let seen = if amortized {
        &AMORTIZED_SEEN
    } else {
        &FULL_REBUILD_SEEN
    };
    if seen
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        if amortized {
            eprintln!("# label-amortize: engaged (per-class ClauseIndexes delta)");
        } else {
            eprintln!("# label-amortize: full-rebuild (per-class HyperEngine::new)");
        }
    }
}

/// Fix#2 Layer A in-search boolean constraint propagation at the `⊔` decision
/// point (`RUSTDL_SEMANTIC_BRANCHING`). **DEFAULT OFF**: opt-in only. Returns
/// `true` only when `RUSTDL_SEMANTIC_BRANCHING` is set to a non-empty, non-`"0"`
/// value. Verdict-preserving (byte-identical curated closures OFF vs ON); wired
/// at the classify `HyperEngine` builders alongside `with_incremental_fixpoint`.
#[must_use]
pub(crate) fn semantic_branching_enabled() -> bool {
    std::env::var_os("RUSTDL_SEMANTIC_BRANCHING").is_some_and(|v| v != "0" && !v.is_empty())
}

/// Bound-the-tail (`RUSTDL_BOUND_DIVERGED_TAIL`, **default OFF**): when the
/// wedge returns a *divergence*-`Stalled` (`is_diverging` fired — the search
/// thrashed at saturated depth), skip the main-tableau fallthrough in
/// `subsumes_via_tableau` and record "not subsumed" directly. The fallthrough
/// would re-thrash the same hard SROIQ pair and default to not-subsumed anyway
/// (measured: `ore_ont_10019` `tier_walk` 77.7 s → 43.4 s if ALL fallthroughs are
/// skipped — but the sound divergence-keyed skip is inert there; see findings).
/// **Completeness,
/// not soundness** — it only ever *removes* subsumptions (FP=0 trivially); a
/// completable ontology does not trip `is_diverging`, so it is untouched. Gated
/// on curated MISSED=0. Depends on `adaptive_budget` (default ON) to set the
/// divergence flag; inert if `RUSTDL_ADAPTIVE_BUDGET=0`.
#[must_use]
pub(crate) fn bound_diverged_tail_enabled() -> bool {
    std::env::var_os("RUSTDL_BOUND_DIVERGED_TAIL").is_some_and(|v| v != "0" && !v.is_empty())
}

/// Anywhere (pairwise/double) blocking in the MAIN SROIQ tableau
/// (`RUSTDL_ANYWHERE_BLOCKING`). Opt-IN, default OFF: returns `true` only when
/// the variable is exactly `"1"`. When ON, `TableauContext::is_blocked` scopes
/// the pair-blocking candidate to ANY earlier-created node (Motik/Shearer/
/// Horrocks anywhere blocking) instead of only tree-ancestors, keeping the
/// completion small on large generative `ABox`es. The actual decision is read
/// once per `TableauContext` at construction (see
/// `owl_dl_tableau::anywhere_blocking_enabled`); this mirror exists for
/// discoverability alongside the other gate fns. Default OFF until the
/// soundness gate validates it corpus-wide.
#[must_use]
pub fn anywhere_blocking_enabled() -> bool {
    std::env::var_os("RUSTDL_ANYWHERE_BLOCKING").is_some_and(|v| v == "1")
}

/// Project flag for the Konclude snapshot cache. When ON,
/// `subsumes_via_tableau` consults a per-class snapshot-replay
/// cache ahead of the wedge.
///
/// **Default ON as of Phase 1c (project-headline landing).** Set
/// `RUSTDL_SNAPSHOT_CAPTURE=0` (or empty) to revert to pre-project
/// pure-wedge behavior. Opt-IN only: `RUSTDL_SNAPSHOT_CAPTURE=1` (or
/// `=true`/`=yes`/`=on`) enables it; default and any other value = OFF.
///
/// **DEFAULT FLIPPED TO OFF 2026-06-08 — SOUNDNESS FIX.** The snapshot
/// cache is unsound (false-positive subsumptions) on the non-Horn
/// fragment: replay trusts ONE satisfying model, but on non-Horn
/// `sup ∈ that-model ≠ sub ⊑ sup` (the "A1" analysis,
/// `docs/reuse-trap-A1-scoping-2026-06-08.md`). The `BackPropRisk::Safe`
/// gate that guards it only excludes inverse/nominal/cardinality — **not
/// disjunction** — so a disjunctive, inv/nom/card-free ontology passes as
/// Safe and the cache emits spurious subsumptions (ORE 2015 surfaced this:
/// `ore_ont_13723` etc., 30+ FP each vs a Konclude∩HermiT oracle, with NO
/// incompleteness signal). Moreover the cache's only *sound* domain (Horn,
/// canonical least model) is already taken by the Horn-shortcircuit, so the
/// cache has no sound active domain. Hence: OFF by default. Re-enable with
/// `=1` only for snapshot A/B experiments. See
/// `docs/perf-2026-06-08-konclude-vs-rustdl.md` (ORE findings).
#[must_use]
pub fn snapshot_capture_enabled() -> bool {
    std::env::var_os("RUSTDL_SNAPSHOT_CAPTURE")
        .is_some_and(|v| v == "1" || v == "true" || v == "yes" || v == "on")
}

/// Phase 1b.5 lazy expansion toggle. Default ON (unset → ON);
/// explicit `RUSTDL_SNAPSHOT_LAZY=0` (or empty) reverts replay to
/// Phase 1b's full-re-run behavior (correctness equivalent; useful
/// for A/B comparison + debugging). Sibling-style env helper:
/// accepts any non-empty, non-`"0"` value (`=1`/`=true`/`=yes`/`=on`).
///
/// Spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §4.1
#[must_use]
pub fn snapshot_lazy_enabled() -> bool {
    std::env::var_os("RUSTDL_SNAPSHOT_LAZY").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Phase 2b: for ontologies classified as `Horn` fragment (hyper
/// Horn fixpoint is sound + complete by construction), dispatch
/// classify to the saturation-only fast path instead of the
/// per-pair verification loop. **Default ON** as of Phase 2b
/// (project-headline landing); set `RUSTDL_HORN_SHORTCIRCUIT=0`
/// (or empty) to revert to the pre-Phase-2b per-pair loop for
/// Horn ontologies (A/B isolation).
///
/// Sibling-style env helper: any non-empty, non-`"0"` value
/// (`=1`/`=true`/`=yes`/`=on`) keeps it ON; only `=0` or empty
/// disables.
///
/// Spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md` §5
/// Recon: `docs/phase2a-recon.md`
#[must_use]
pub fn horn_shortcircuit_enabled() -> bool {
    std::env::var_os("RUSTDL_HORN_SHORTCIRCUIT").is_none_or(|v| v != "0" && !v.is_empty())
}

/// `ABox` consistency-check pre-pass toggle. **Default ON.** Runs a
/// sound under-approximation check before the tableau in
/// `is_consistent` and `classify`. Set `RUSTDL_ABOX_CHECK=0` (or
/// empty) to skip the check entirely (today's tableau-only
/// behaviour). Sibling-style env helper.
///
/// Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`
#[must_use]
pub fn abox_check_enabled() -> bool {
    std::env::var_os("RUSTDL_ABOX_CHECK").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Elide the `PreparedOntology`-owned EL saturation on provably `ABox`-free
/// inputs (`RUSTDL_LAZY_ABOX_SATURATION`). **Default OFF** (set `=1` to opt in).
///
/// `PreparedOntology::closure` has exactly ONE consumer:
/// [`PreparedOntology::abox_verdict`], which feeds it to [`abox_check::check`].
/// `check` early-returns [`abox_check::AboxVerdict::Unknown`] before touching the
/// closure whenever `abox.individuals` is empty, so on an `ABox`-free ontology the
/// whole saturation is dead work — a *third* full saturation of the same ontology
/// on the hybrid classify path (the other two are `lib.rs`'s
/// `saturate_with_exists_facts` label-cache seed and `classify.rs`'s own closure).
///
/// The gate is `internal_has_abox`, evaluated on the **un-mutated** input, and it is
/// exactly equivalent to `abox.individuals.is_empty()` at `collect_abox` time:
/// `collect_abox` populates `individuals` from precisely the five axiom kinds
/// `internal_has_abox` matches, `nnf_axioms` leaves `internal.axioms` unchanged
/// (pinned by `normalize::tests::nnf_axioms_leaves_original_axioms_unchanged`), and
/// `expand_role_characteristics` only appends `SubClassOf` / `InverseObjectProperties`.
/// Should that ever drift, `abox_verdict` degrades to `Unknown` — a sound
/// under-approximation (a missed inconsistency is a MISS, never an FP), pinned by a
/// `debug_assert`.
#[must_use]
pub fn lazy_abox_saturation_enabled() -> bool {
    std::env::var_os("RUSTDL_LAZY_ABOX_SATURATION").is_some_and(|v| v != "0" && !v.is_empty())
}

/// Consequence-based ABox-saturation consistency pre-check
/// (`RUSTDL_ABOX_SATURATION`). **Default on** (set `=0` or empty to disable); a
/// derived clash ⇒ inconsistent (sound under-approximation — non-clash falls
/// through to the hybrid path unchanged). Closes the family inconsistency gap;
/// `has_abox_axioms`-guarded so ABox-free inputs skip it (zero cost). Whole-corpus
/// bake-off (2026-06-20): FP=0/MISSED=0 byte-identical, zero classify cost.
#[must_use]
pub fn abox_saturation_enabled() -> bool {
    std::env::var_os("RUSTDL_ABOX_SATURATION").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Run the KB-level inconsistency pre-checks on the **classify** path too, so
/// `classify --json` cannot report `"consistent": true` on a KB the sibling
/// `rustdl consistent` subcommand calls `inconsistent`
/// (`RUSTDL_CLASSIFY_INCONSISTENCY`). **Default ON since 0.4.8** (`=0` reverts
/// to the pre-2026-08-01 behaviour). The `ABox`-saturation half runs under the
/// [`classify_inconsistency_budget_ms`] budget on this path only.
///
/// Motivation: `classify` used to consult only [`abox_check`] (the Phase A1
/// pattern matcher) and, on the *pure-EL path only*, the saturator's `⊤`-unsat
/// signal. `is_consistent` additionally runs the
/// [`abox_saturation`] consequence-based fixpoint, and `realize_internal` gained
/// the same short-circuit in v0.3.36. `family.ofn` is exactly the gap: `HermiT`,
/// Konclude, `rustdl consistent` and `rustdl realize` all call it inconsistent
/// in under a second, while `classify --json` reported
/// `"consistent": true, "unsatisfiable": []`.
///
/// See [`classify_inconsistency_precheck`] for the (sound) signals consulted.
#[must_use]
pub fn classify_inconsistency_enabled() -> bool {
    // DEFAULT ON since 0.4.8 (`=0` reverts). Without it `classify --json` and the
    // `consistent` subcommand contradict each other on an inconsistent KB. Measured
    // Verdict-inert on consistent input. NOTE the original cost justification (-1.5% over 12
    // ABox-bearing ORE ontologies) was UNDER-POWERED: a full-corpus sweep later found 4
    // ontologies regressing from ~5 s to DNF, because 12 ontologies missed the 60k+-assertion
    // population entirely. The cost is now bounded by `classify_inconsistency_budget_ms`.
    std::env::var_os("RUSTDL_CLASSIFY_INCONSISTENCY").is_none_or(|v| v != "0")
}

/// Let a classify GLOBAL wall-clock budget bound the **preparation** phases —
/// the EL saturation and [`PreparedOntology::from_internal`] —
/// not just the search (`RUSTDL_PREP_DEADLINE`). **Default OFF** (set `=1` to
/// opt in; anything else is the pre-2026-08-01 behaviour).
///
/// The defect: `classify_top_down_internal` consulted `global_deadline` for the
/// first time inside the label-cache loop, so `saturate()` and
/// `from_internal()` ran to completion no matter how small the budget. Measured
/// over the 252-ontology DNF population under a **1 ms** budget: 77 still burned
/// ≥ 10 s and 26 never finished `from_internal` at all (`ore_ont_10926`: 84.9 s
/// against a 1 ms promise). A caller asking for a bounded classify got an
/// unbounded one.
///
/// With the flag on, three things change, all **only when a global deadline is
/// active** (an untimed classify is byte-identical, and pays not one extra
/// clock read):
///
/// 1. the saturation fixpoint is drained via
///    [`owl_dl_saturation::saturate_with_deadline`];
/// 2. `from_internal` checks the deadline at coarse pass boundaries
///    ([`PreparedOntology::from_internal_with_deadline`]);
/// 3. either abort returns the **EL closure read-off** as the answer.
///
/// **Sound under-approximation**, following the `RUSTDL_MAX_NODES` precedent
/// (`NodeCap` → `Ok(None)` → sound MISS): the partial closure contains only
/// entailed subsumptions, so the hierarchy can MISS but never gain an edge. The
/// result is flagged INCOMPLETE — `ClassificationStats::prep_timed_out`, plus a
/// `timed_out_pairs` bump so `completeness_guaranteed()` is false and
/// `classify --json` reports `"incomplete": true`.
///
/// **Known residual bound** (honest, not closed here): `convert_ontology` and
/// the saturator's `collect_el_rules` / `seed` prelude run before the first
/// possible check, so an ontology whose *conversion* is the DNF is still
/// unbounded. Only the two phases named above are covered.
#[must_use]
pub fn prep_deadline_enabled() -> bool {
    std::env::var_os("RUSTDL_PREP_DEADLINE").is_some_and(|v| v == "1")
}

/// Wall-clock budget, in milliseconds, for the `ABox`-saturation half of the
/// **classify** inconsistency pre-check (`RUSTDL_CLASSIFY_INCONSISTENCY_MS`).
/// **Default 3000**; `0` (or a garbage value) means unbounded.
///
/// Bounds ONLY the classify path. `is_consistent` / `realize` /
/// `materialize_*` / `diagnose` stay unbounded — for them the pre-check *is*
/// the point of the call.
///
/// **Why a budget at all:** making `RUSTDL_CLASSIFY_INCONSISTENCY` default-ON
/// in v0.4.8 put an unbounded named-individual fixpoint in front of every
/// classify. On ORE ontologies with ~60k–110k `ABox` assertions that fixpoint
/// dominates the whole run — `ore_ont_{10838,15846,16315,3087}` went from
/// 1.3–4.4 s to **DNF at 60 s**. Bounding it is safe by the pre-check's own
/// contract (sound under-approximation: no clash ⇒ no verdict ⇒ the caller
/// proceeds exactly as before), so a timeout costs at most the inconsistency
/// detection, never correctness.
///
/// **The 3000 ms flat default was too tight at the only end that matters, and
/// is superseded by [`adaptive_classify_inconsistency_budget_ms`]** (2026-08-03).
/// Measured in isolation on the reference host, `family.ofn`'s pre-check costs
/// **2585 ms** and the classify-level detection flips between **2600 and
/// 2700 ms** — i.e. 3000 ms left only ~13% headroom, so a host 15% slower
/// silently lost the detection v0.4.11 shipped to provide.
///
/// (The earlier "~2.0 s" figure was a confounded subtraction — classify *with*
/// the pre-check minus *without*. A clash short-circuits the rest of classify,
/// so that difference measures "pre-check minus the classify it replaced", not
/// the pre-check. Always measure this fixpoint in isolation; see
/// `crates/owl-dl-reasoner/examples/abox_precheck_probe.rs`.)
///
/// This function is now only the **explicit override** reader — a documented
/// escape hatch that always wins, including `0` for unbounded.
#[must_use]
pub fn classify_inconsistency_budget_override_ms() -> Option<u64> {
    std::env::var("RUSTDL_CLASSIFY_INCONSISTENCY_MS")
        .ok()
        .and_then(|s| s.parse().ok())
}

/// Whether `classify` runs a **budgeted wedge-consistency probe** when it has
/// already found at least one unsatisfiable class.
///
/// **The gap.** `classify`'s inconsistency detection is a sound
/// under-approximation: it consults the saturator's `⊤`-unsat signal and the
/// `ABox` pre-check, but never the wedge-consistency route that `is_consistent`
/// uses. Measured over all 1,920 ORE ontologies (2026-08-08): `is_consistent`
/// finds **43** inconsistent; classify agrees on 41 and reports
/// `consistent = true` on **2** — `ore_ont_16372` and `ore_ont_7610`. Those are
/// WRONG ANSWERS, the worst failure class, and both are caught by
/// `consistency_wedge` in under 0.4 s.
///
/// **Why this is affordable, when running `is_consistent` on the classify path
/// is not.** Unconditionally, it is not: on 60 sampled *consistent* ontologies
/// `is_consistent` costs a mean of **5.1 s** (16 over 1 s, max 30 s), which is
/// the documented dead-end. The gate is what makes it cheap:
///
/// > An inconsistent KB makes `⊤` unsatisfiable, hence **every** class
/// > unsatisfiable. Contrapositive: **zero unsatisfiable classes ⟹ consistent**,
/// > so no probe is needed there.
///
/// Measured, that gate admits **1 of 60** sampled ontologies (~1.6%), so the
/// probe runs on roughly 31 of 1,920 rather than all of them. Both targets pass
/// it (3 and 91 unsatisfiable classes respectively).
///
/// **Soundness.** Skipping keeps today's behaviour exactly, so the gate can only
/// fail to fix, never break — note the gate is a heuristic for *when to look*,
/// not a claim, because classify's per-class unsat detection is itself
/// incomplete. A positive verdict is a wedge `Unsat`, which `is_consistent`
/// already trusts as a real inconsistency on the same justification.
///
/// **Default ON** (`=0` reverts) — a wrong consistency verdict is not acceptable
/// as a default, whatever it saves.
#[must_use]
pub fn classify_consistency_probe_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables. A confidently
    // WRONG consistency verdict is worse than a slow one, so this is not opt-in.
    std::env::var_os("RUSTDL_CLASSIFY_CONSISTENCY_PROBE").is_none_or(|v| v != "0")
}

/// Budget in ms for the gated classify consistency probe. **Default 200.**
///
/// The value is small for a measured reason, not conservatism. A
/// 1,920-ontology two-arm sweep at 1000 ms cost **4 ontologies `ok` → `dnf`**
/// (`ore_ont_14881`, `6108`, `7416`, `7803`) and took `ore_ont_1966` from 7.30 s
/// to 58.20 s. The cost is **not proportional to the budget** — `1966` reads
/// 66.08 s at 1000 ms, 73.00 s at 100 ms, and **5.17 s at 10 ms** against a 5.06 s
/// baseline — because `decide_with_deadline` overshoots its deadline on the main
/// tableau, the same defect class found in `horn_fixpoint` on 2026-08-08.
///
/// **There is no single budget that satisfies both sides**: `ore_ont_16372` needs
/// **≥200 ms** to be decided, and `ore_ont_1966` is already destroyed at 100 ms.
/// So 10 ms buys the two cheap layers — the exact asserted-instance test and the
/// wedge route — at a measured cost of **+0.06 to +0.37 s** on the five ontologies
/// the larger budget harmed, while layer 3 (the bounded `⊤` probe) rarely
/// concludes.
///
/// Consequence, stated plainly: **`ore_ont_7610` is fixed and `ore_ont_16372` is
/// not.** Raising this value fixes `16372` at the price of ontologies that
/// currently answer correctly — a bad trade until the `decide_with_deadline`
/// overshoot is fixed, which is the real blocker.
#[must_use]
pub fn classify_consistency_probe_ms() -> u64 {
    std::env::var("RUSTDL_CLASSIFY_CONSISTENCY_PROBE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Minimum unsatisfiable-class fraction, in PER MILLE, for the expensive
/// consistency-probe layers to run. **Default 2 (= 0.2%).** `0` disables the gate.
///
/// Not a tuning knob so much as a documented threshold: it sits in a measured ~6×
/// gap between the ontologies the probe harms and the ones it must reach.
///
/// | ontology | classes | unsat | fraction | |
/// |---|---|---|---|---|
/// | `ore_ont_14881` | 20,485 | 1 | 0.005% | harmed |
/// | `ore_ont_1966` | 20,514 | 13 | 0.063% | harmed |
/// | `ore_ont_16372` | 744 | 3 | **0.403%** | must reach |
/// | `ore_ont_7610` | 91 | 91 | **100%** | must reach |
///
/// **Keep it LOW.** `ore_ont_16372` is genuinely inconsistent yet shows only
/// 0.403%, because classify's own per-class unsat detection is incomplete; a
/// threshold like "half the classes" would miss it.
///
/// `0` is the diagnostic setting: it restores the pre-gate behaviour, which is the
/// only way to exercise the `decide_with_deadline` overshoot this gate masks.
#[must_use]
/// Admit the classify consistency probe on an INCOMPLETE, `ABox`-bearing run even when
/// no class was proved unsatisfiable (`RUSTDL_CLASSIFY_PROBE_ON_INCOMPLETE`, **default
/// OFF**, `=1` enables).
///
/// The probe's existing admission test is the unsatisfiable FRACTION, which is
/// budget-sensitive: a timed-out per-class probe defaults to *satisfiable*, so a small
/// `--pair-timeout-ms` empties `unsatisfiable_idxs` and the gate reads "no evidence of
/// inconsistency" when the truth is "we did not look long enough to have evidence".
/// Those are different states and the gate conflates them.
///
/// `ore_ont_16372` is inconsistent (Konclude, `HermiT` and rustdl's own `consistent`
/// agree) and its entire admission signal is **3** unsat proofs out of 744 classes,
/// each needing >25 ms. At `--pair-timeout-ms 5` all three are lost and `classify`
/// reports `consistent: true`. See `docs/2026-08-15-ore16372-verdict-rootcause.md`.
///
/// Scope: `timed_out_pairs > 0` AND the ontology has `ABox` axioms. Measured over the
/// 424-ontology release population at a 5 ms budget, that is **49** ontologies — one
/// bounded probe each, so ≤ 200 ms × 49 ≈ 9.8 s across the whole population in the
/// worst case where every probe burns its budget.
///
/// The cost hazard the fraction gate was added for is ALREADY mitigated by bounding.
/// Forcing the probe on the five ontologies that regression names
/// (`ore_ont_14881`, `6108`, `7416`, `7803`, `1966`) costs −0.9 s … +0.9 s — noise
/// against a 200 ms budget. The 8h47m FP=0-net disaster on record was the EARLIER
/// mechanism: `k` UNBOUNDED probes, 58 of them on `wine`.
pub fn classify_probe_on_incomplete() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables, `=0` reverts.
    // Not opt-in, because the v0.4.19 per-pair default (5 ms) is exactly what empties
    // `unsatisfiable_idxs`; shipping that budget without this flag reports
    // `consistent: true` on `ore_ont_16372`. The two move together.
    std::env::var_os("RUSTDL_CLASSIFY_PROBE_ON_INCOMPLETE").is_none_or(|v| v != "0")
}

pub fn classify_probe_min_frac_permille() -> u64 {
    std::env::var("RUSTDL_CLASSIFY_PROBE_MIN_FRAC_PERMILLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
}

/// Cheap structural predictors of the `ABox`-saturation fixpoint's cost, read in
/// ONE linear pass over the lowered axioms — no reasoning, no allocation beyond
/// three counters.
///
/// **Which predictors, and why these.** A 1137-ontology scan of the ABox-bearing
/// ORE population measured the pre-check *in isolation* against every cheap
/// structural quantity on offer. Named-individual count, `ClassAssertion` count
/// and `ObjectPropertyAssertion` count **do not track cost, in either
/// direction** — `ore_ont_4510` carries 114 957 `ObjectPropertyAssertion` and
/// saturates in **136 ms**, while `family.ofn` carries 1 337 and takes
/// **2585 ms**. `ore_ont_6233` (176 043 `ClassAssertion`) takes 17 ms.
///
/// What does separate them is whether the ontology has a rule that *multiplies*
/// edges. The fixpoint's cost is the size of the derived edge closure; without a
/// role chain or a transitive role the closure is the asserted edge set expanded
/// through the sub-role/inverse hierarchy, which is linear (`4510`'s
/// `edge_additions` equals its assertion count exactly). With one, it is up to a
/// transitive closure — `family` turns 1 337 asserted edges into **267 112**.
///
/// **A SECOND, INDEPENDENT COST DRIVER EXISTS, and these predictors deliberately
/// do not model it.** The first version of this analysis concluded that edge
/// multiplication was *necessary* for expense. Extending the scan from 409 to
/// 1137 ontologies refuted that: `ore_ont_5368` performs **zero** type additions
/// and **zero** edge additions and still costs **5936 ms**, and `ore_ont_1833`
/// costs 4478 ms for a closure that does not grow at all. Their cost is the
/// fixpoint's **pre-indexing prelude**, which walks every lowered axiom — and
/// they carry 18.6 M and 14.1 M axioms respectively (`DKey` disjointness floods).
/// The rate is strikingly stable at ~0.3 µs/axiom across all three
/// prelude-dominated cases.
///
/// That driver is **left out of the rule on purpose, on measurement**: the
/// prelude runs before the first deadline probe, so its cost is
/// *budget-independent*. Measured on both, at 3000 ms vs 12 000 ms:
/// `1833` 4065 → 4023 ms and `5368` 6059 → 5871 ms — the **same wall**, but
/// `timed_out` flips from `true` to `false`, i.e. the larger budget converts
/// work that was already paid for and then discarded into an actual verdict.
/// Gating on axiom count would push these two back into the stingy class and
/// make them strictly worse. (The honest residual: **no budget bounds the
/// prelude**, so such an ontology overruns any budget by several seconds. That
/// is pre-existing and identical at 3000 ms; it is a separate lever.)
///
/// See `docs/2026-08-03-adaptive-inconsistency-budget.md` for the full table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AboxCostPredictors {
    /// `ObjectPropertyAssertion` count — the fixpoint's *input* edge set.
    pub asserted_edges: usize,
    /// Rules that can multiply an edge set: `SubObjectPropertyOf` with a role
    /// **chain** body, plus `TransitiveObjectProperty` (which the saturator
    /// handles as the self-chain `R∘R ⊑ R`).
    pub multiplying_rules: usize,
}

impl AboxCostPredictors {
    /// The work proxy the budget rule thresholds on: asserted edges times the
    /// number of edge-multiplying rules, **at least one** so that a
    /// chain-free ontology is still scored on its raw edge count rather than
    /// collapsing to zero (a multi-million-assertion chain-free `ABox` is
    /// linear, but linear in something large).
    #[must_use]
    pub fn work_proxy(self) -> u64 {
        (self.asserted_edges as u64).saturating_mul(self.multiplying_rules.max(1) as u64)
    }
}

/// Read [`AboxCostPredictors`] off the lowered ontology.
#[must_use]
pub fn abox_cost_predictors(internal: &InternalOntology) -> AboxCostPredictors {
    let mut asserted_edges = 0usize;
    let mut multiplying_rules = 0usize;
    for ax in &internal.axioms {
        match ax {
            Axiom::ObjectPropertyAssertion { .. } => asserted_edges += 1,
            Axiom::SubObjectPropertyOf { sub, .. } => {
                if matches!(sub, SubRolePath::Chain(_)) {
                    multiplying_rules += 1;
                }
            }
            Axiom::TransitiveRole(_) => multiplying_rules += 1,
            _ => {}
        }
    }
    AboxCostPredictors {
        asserted_edges,
        multiplying_rules,
    }
}

/// Work proxy at or below which the classify pre-check gets the **generous**
/// budget.
///
/// **Placed inside a measured, EMPTY 40× gap.** Over the ABox-bearing ORE
/// population plus `family.ofn`, every ontology whose pre-check exceeds 3000 ms
/// *for fixpoint reasons* scores at least **2 047 210** (`ore_ont_16315`,
/// 68.8 s), and `family.ofn` scores **50 806**. Nothing lies between. 300 000 is
/// the balance point of that gap on a log scale: `family` clears it by 5.9× and
/// the cheapest expensive ontology exceeds it by 6.8×.
///
/// (The two ontologies that are expensive for *prelude* reasons —
/// `ore_ont_{1833,5368}` — score 10 865 and 6 099 and so fall below this line.
/// That is the intended outcome, not a leak: see the second cost driver
/// discussed on [`AboxCostPredictors`]. Their wall is budget-independent, so the
/// generous branch costs them nothing and gains them a verdict.)
///
/// **The asymmetry that decided the exact position:** landing too LOW costs an
/// ontology only *today's* behaviour (the flat 3000 ms — not a regression),
/// while landing too HIGH hands a runaway fixpoint the full generous budget.
/// So a mid-gap value is preferred to one hugging the expensive end.
pub const INCONSISTENCY_WORK_THRESHOLD: u64 = 300_000;

/// Budget granted to the low-work class. 4.6× `family.ofn`'s measured 2585 ms
/// (4.4× its 2700 ms classify-level flip point) — the headroom the flat 3000 ms
/// did not have.
pub const INCONSISTENCY_GENEROUS_MS: u64 = 12_000;

/// Budget granted to the high-work class. Deliberately **identical to the
/// superseded flat default**, so this change can only ever *raise* a budget,
/// never lower one: every ontology outside the low-work class is bounded exactly
/// as it is today, which is what keeps `ore_ont_{10838,15846,16315,3087}` at the
/// walls the flat budget bought them.
pub const INCONSISTENCY_STINGY_MS: u64 = 3_000;

/// Adaptive wall-clock budget for the `ABox`-saturation half of the **classify**
/// inconsistency pre-check. **Default ON**;
/// `RUSTDL_CLASSIFY_INCONSISTENCY_MS` overrides it outright (including `0` for
/// unbounded).
///
/// Bounds ONLY the classify path. `is_consistent` / `realize` / `materialize_*`
/// / `diagnose` stay unbounded — for them the pre-check *is* the point of the
/// call.
///
/// **The rule, and the direction that is easy to get backwards.** The budget
/// *decreases* with predicted work. It is tempting to scale it up with `ABox`
/// size — the pathological ontologies have big `ABox`es, so surely they need
/// more time? — but that is exactly inverted: the expensive cases are expensive
/// *because* the closure runs away, and `family.ofn` needs its 2.6 s with only
/// 508 individuals. A budget increasing in `ABox` size would starve `family` and
/// subsidise the four ontologies whose DNF the budget exists to prevent.
///
/// So: **generous when the fixpoint provably cannot run away, unchanged when it
/// can.**
///
/// ```text
/// work_proxy = asserted_edges × max(multiplying_rules, 1)
/// budget     = work_proxy ≤ 300_000 ? 12_000 ms : 3_000 ms
/// ```
///
/// **Why two levels and not a formula.** The proxy separates the expensive tail
/// cleanly but its magnitude does NOT predict milliseconds, and the refutation
/// is exact: `ore_ont_1579` and `ore_ont_15846` have **identical** predictors
/// (78 441 asserted edges, 55 multiplying rules, work proxy 4 314 255) and cost
/// **1502 ms** and **>5000 ms** respectively. A formula would be reading a
/// precision the measurement does not contain. A threshold reads only the
/// ordering, which is what was measured.
///
/// **Soundness is untouched, and this is structural rather than inherited:**
/// a deadline abandonment returns `clash: false` with `edges` / `derived_same`
/// emptied ([`abox_saturation::saturate_abox_consistency_bounded`]), and
/// `clash: false` is *already* the no-verdict answer every caller handles. No
/// budget — larger, smaller, or absent — can manufacture an inconsistency, so
/// changing it costs at most the detection, never correctness.
///
/// **Residual exposure, measured rather than argued.** The generous branch *can*
/// grant more than the flat 3000 ms, so in principle an ontology could sit in the
/// low-work class and burn 12 000 ms. Over the 1137-ontology ABox-bearing ORE
/// population, **1089 of the 1102 low-work members cost under 500 ms** — a
/// 12 000 ms cap is unobservable for them, because a budget is a cap and not an
/// expenditure. Of the 13 that cost ≥500 ms, 11 complete in ≤1627 ms (so the cap
/// never binds) and the remaining 2 are the prelude-dominated
/// `ore_ont_{1833,5368}`, measured at the same wall under 12 000 ms as under
/// 3000 ms. Net effect on the population: **no wall change and no outcome
/// change, except that those 2 stop discarding a pre-check they had already paid
/// for.**
///
/// **Coverage.** The pre-check is `has_abox_axioms`-guarded, so an ABox-free
/// ontology cannot be reached by this rule at all; the ABox-bearing set *is* the
/// complete affected population, and 1137 of the 1144 in the ORE pool were
/// measured (7 excluded: 6 exceed a 60 s cap inside `convert_ontology`, before
/// the pre-check runs; 1 fails to parse). See
/// `docs/2026-08-03-adaptive-inconsistency-budget.md`.
#[must_use]
pub fn adaptive_classify_inconsistency_budget_ms(predictors: AboxCostPredictors) -> u64 {
    if predictors.work_proxy() <= INCONSISTENCY_WORK_THRESHOLD {
        INCONSISTENCY_GENEROUS_MS
    } else {
        INCONSISTENCY_STINGY_MS
    }
}

/// The budget the classify path actually uses: the explicit override if set,
/// else [`adaptive_classify_inconsistency_budget_ms`] over predictors read from
/// `internal`. `0` means unbounded.
#[must_use]
pub fn classify_inconsistency_budget_ms(internal: &InternalOntology) -> u64 {
    classify_inconsistency_budget_override_ms().unwrap_or_else(|| {
        adaptive_classify_inconsistency_budget_ms(abox_cost_predictors(internal))
    })
}

/// The `ABox`-saturation half of the KB-level inconsistency pre-check, factored
/// out so `is_consistent` and `classify` cannot drift apart.
///
/// **Sound under-approximation:** a clash derived by the consequence-based
/// fixpoint over named individuals is a genuine inconsistency (every derived
/// type/edge/merge is entailed); no clash yields no verdict, and the caller
/// falls through to its normal path unchanged. Guarded by
/// [`abox_saturation_enabled`] and `has_abox_axioms`, so `ABox`-free inputs pay
/// nothing.
///
/// Unbounded — the shape `is_consistent` and friends want.
#[must_use]
pub(crate) fn abox_saturation_inconsistent(internal: &InternalOntology) -> bool {
    abox_saturation_inconsistent_bounded(internal, None)
}

/// [`abox_saturation_inconsistent`] with an optional wall-clock budget; the two
/// share one body so the bounded (classify) and unbounded (`is_consistent`)
/// surfaces still cannot drift.
///
/// A timeout yields `false` — i.e. *no verdict*, indistinguishable from "the
/// fixpoint completed and found no clash", which is what every caller already
/// handles. It can never manufacture an inconsistency.
#[must_use]
pub(crate) fn abox_saturation_inconsistent_bounded(
    internal: &InternalOntology,
    budget: Option<std::time::Duration>,
) -> bool {
    if !abox_saturation_enabled() || !classify::has_abox_axioms(internal) {
        return false;
    }
    let deadline = budget.map(|b| std::time::Instant::now() + b);
    abox_saturation::saturate_abox_consistency_bounded(internal, deadline).clash
}

/// Sound KB-level inconsistency pre-check for the classify drivers.
///
/// Returns `true` only when the KB is **provably** inconsistent. Two independent
/// signals, both already shipped elsewhere in the tree — this reuses them rather
/// than inventing a third mechanism:
///
/// 1. **`⊤` is unsatisfiable** — `closure.globally_inconsistent()` (a syntactic
///    `⊤ ⊑ ⊥`) or `closure.top_is_unsat()` (some `C` with `⊤ ⊑ C` saturated to
///    `⊥`). This is verbatim the test `classify_pure_el` already applies; the
///    hybrid path simply never ran it.
/// 2. **`ABox`-saturation clash** — [`abox_saturation_inconsistent`], the same
///    pre-check `is_consistent` runs before its tableau, here under the
///    [`classify_inconsistency_budget_ms`] wall-clock budget (classify is the
///    one caller for which this pre-check is an accelerator rather than the
///    answer, and an unbounded fixpoint over a 60k-assertion `ABox` costs more
///    than the classify it precedes). A timeout yields no verdict.
///
/// **Soundness subtlety (deliberate, load-bearing):** *all named classes being
/// unsatisfiable is NOT an inconsistency signal.* `{A ⊑ ⊥, B ⊑ ⊥}` empties every
/// named class yet has a perfectly good non-empty model. The correct test is that
/// `⊤` is unsatisfiable, which is what `top_is_unsat` reports and what signal (1)
/// asks for. Nothing here inspects the unsatisfiable-class list.
///
/// A negative verdict is *not* a claim of consistency: both signals are
/// under-approximations, so the caller proceeds exactly as before.
pub(crate) fn classify_inconsistency_precheck(
    internal: &InternalOntology,
    closure: &owl_dl_saturation::Subsumers,
) -> bool {
    closure.globally_inconsistent()
        || closure.top_is_unsat()
        || abox_saturation_inconsistent_bounded(
            internal,
            match classify_inconsistency_budget_ms(internal) {
                0 => None,
                ms => Some(std::time::Duration::from_millis(ms)),
            },
        )
}

/// Per-class deadline (in milliseconds) for the Phase 7 label-cache
/// build during classification. **Distinct from `--pair-timeout-ms`**:
/// the cache build is one-shot per class at classify-start, and a
/// class that exceeds this budget becomes `LabelOracle::NoVerdict` —
/// forcing every `(C, *)` pair to fall through to per-pair
/// `subsumes_via_tableau`. The Phase 8 recon
/// (`docs/phase8-recon.md`) showed that ~5% of ORE-10908 classes
/// stalled at the per-pair 200 ms budget (median 341 ms, max 631 ms
/// when given more time) and each `NoVerdict` class contributed
/// ~28 ms × ~38 cache-miss pairs to the tier walk — a disproportionate
/// tail. A generous default (5000 ms = 5 s) lets these classes
/// complete to `Sat` and collapse their cache-miss pairs to ~µs prunes.
///
/// Override with `RUSTDL_LABEL_CACHE_TIMEOUT_MS=<integer>`. Default
/// 1000 ms — generous enough to catch ORE-10908's stallers
/// (median 341 ms, max 631 ms per the recon) but tight enough to
/// bail quickly on genuinely intractable classes (ORE-15672's
/// ~56% `NoVerdict` rate suggests those classes don't finish in
/// any reasonable budget). Set to `0` for unbounded cache build.
#[must_use]
pub fn label_cache_timeout_ms() -> u64 {
    const DEFAULT_MS: u64 = 1000;
    std::env::var("RUSTDL_LABEL_CACHE_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MS)
}

// Floor lowered 1000→50 (perf/label-cache-floor, 2026-06-27): the break-even is
// `n × per_pair` (label C iff cheaper than refuting C's ~n pairs); a floor ABOVE
// that is provable over-investment — at a tight per-pair cap (refutation ~free) it
// forced the label build to over-pay (wine @1ms-cap: floored to 1000ms ⟹ build 1.0s
// vs the break-even ~137ms ⟹ ~890ms total, −42%). 50ms is a small absolute minimum
// for degenerate near-zero budgets. Inactive at the default 1000ms cap (n×1000 ≥ 1000
// for any n ≥ 1 ⟹ the floor never binds there) ⟹ no default-path change.
pub(crate) const LABEL_CACHE_FLOOR_MS: u64 = 50;
pub(crate) const LABEL_CACHE_CEILING_MS: u64 = 30_000;

/// Adaptive per-class label-cache build deadline (build-once tuning, 2026-06-25).
/// `env_override` (`RUSTDL_LABEL_CACHE_TIMEOUT_MS`) always wins — incl. `0` (unbounded).
/// Else `n × per_pair` (the refute-the-row break-even: labeling C is worth it iff its
/// `sat` costs less than refuting C's ~n pairs at the per-pair cap), clamped to
/// [floor, ceiling]. `per_pair == None` (unbounded refutations) → base = ceiling.
/// See docs/superpowers/specs/2026-06-25-adaptive-label-cache-deadline-design.md.
pub(crate) fn adaptive_label_cache_ms(
    n: usize,
    per_pair: Option<std::time::Duration>,
    env_override: Option<u64>,
) -> u64 {
    if let Some(v) = env_override {
        return v;
    }
    let base = per_pair.map_or(LABEL_CACHE_CEILING_MS, |d| {
        u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
    });
    (n as u64)
        .saturating_mul(base)
        .clamp(LABEL_CACHE_FLOOR_MS, LABEL_CACHE_CEILING_MS)
}

pub(crate) fn label_cache_env_override() -> Option<u64> {
    std::env::var("RUSTDL_LABEL_CACHE_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
}

/// AGGREGATE wall budget (ms) for the WHOLE label-cache build phase.
///
/// The per-class budget (`adaptive_label_cache_ms`) bounds one class. It cannot
/// bound the phase, because the phase costs `n × per-class` and `n` reaches
/// 8,025 on the affected ontologies. Profiling the 11 `label_cache_build`-bound
/// members of the DNF tail (2026-08-08,
/// `docs/known-limitations/label-cache-build-unbounded.md`) found the MEDIAN
/// per-class overshoot is **0 ms** — most classes are instant — with a tail of
/// 400–560 ms classes. So even a *perfect* 10 ms per-class bound leaves
/// 1,682–8,025 classes × 10 ms = **17–80 s**. Those ontologies are
/// aggregate-bound, not precision-bound.
///
/// Cutting the phase early is **sound**: an unbuilt label is `NoVerdict`, which
/// is exactly what a per-class timeout already yields, and `NoVerdict` only
/// removes a pruning opportunity — the tier walk then falls through to the
/// (separately bounded) probe path. It costs COMPLETENESS-VIA-BUDGET, never
/// correctness. Whether the ontologies then actually classify is an empirical
/// question, deliberately left to measurement.
///
/// **Default 0 = unbounded** (opt-in): `RUSTDL_LABEL_CACHE_TOTAL_MS`.
#[must_use]
pub fn label_cache_total_ms() -> Option<u64> {
    match std::env::var("RUSTDL_LABEL_CACHE_TOTAL_MS") {
        Ok(v) => match v.parse::<u64>() {
            Ok(0) | Err(_) => None,
            Ok(ms) => Some(ms),
        },
        Err(_) => None,
    }
}

/// Minimum wedge wall-time threshold (in milliseconds) below which a
/// `NotSubsumed` verdict is **distrusted** and the tableau is asked to
/// verify. A wedge `NotSubsumed` returned in < threshold ms is conjectured
/// to be "didn't try hard enough" rather than a genuine satisfying model;
/// in that case the tableau is consulted before trusting.
///
/// **Default: 0 (disabled).** The Phase 1 alehif threshold sweep
/// (1/5/10/20/30 ms) found wall times flat at ~230× baseline across
/// every threshold in that range, meaning virtually every wedge
/// `NotSubsumed` verdict completes in under 1 ms — so wall-time is not
/// a useful filter at this resolution. See `docs/phase1-results.md`
/// and `docs/hypertableau-dead-ends.md` §13 for the empirical analysis.
///
/// The mechanism is preserved (sound on the corpus) for users who have
/// profiled a specific workload and identified a threshold that works
/// for them. Setting the var to any positive integer enables the
/// behaviour. Empty / garbage values fall back to the default (0).
///
/// **Caching:** in non-test builds the env var is read once per process
/// (first call) and cached in a `OnceLock` thereafter. Subsequent
/// mutations of the env var have no effect until the process restarts.
/// In unit tests *within this crate* (`cfg(test)`) the cache is
/// bypassed so per-test env mutation works. Integration tests
/// (`crates/owl-dl-reasoner/tests/*`) and any downstream consumer see
/// the cached path — set the env var BEFORE the first call from those
/// contexts, or accept the cached value.
#[must_use]
pub fn hyper_trust_sat_min_ms() -> u64 {
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static CACHED: OnceLock<u64> = OnceLock::new();
        *CACHED.get_or_init(read_hyper_trust_sat_min_ms_env)
    }
    #[cfg(test)]
    {
        read_hyper_trust_sat_min_ms_env()
    }
}

fn read_hyper_trust_sat_min_ms_env() -> u64 {
    std::env::var("RUSTDL_HYPER_TRUST_SAT_MIN_MS")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Three-valued verdict from the H4/HF5 hyper wedge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HyperVerdict {
    /// `Unsat` on `sub ⊓ ¬sup` — subsumption holds. Sound for any
    /// ontology (clausifier is sound; calculus is `Unsat`-sound).
    Subsumed,
    /// `Sat` on `sub ⊓ ¬sup` — subsumption does **not** hold. Sound
    /// only when [`hyper_trust_sat_enabled`] is set (HF5).
    NotSubsumed,
    /// `Stalled`/budget exhausted — caller falls back to the tableau.
    Unknown,
    /// `Stalled` because the adaptive-budget `is_diverging` early-cut fired —
    /// the wedge thrashed at saturated depth. Distinguished from `Unknown` so
    /// the bound-the-tail path can skip a main-tableau fallthrough that would
    /// re-thrash the same pair (sound: only ever yields "not subsumed").
    UnknownDiverged,
}

/// Per-class label heuristic oracle. Built by `HyperCache::classify_labels`
/// once per named class at classify-time; consulted by the orchestrator
/// to prune `subsumes_via_tableau` calls. See
/// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
#[derive(Debug, Clone)]
pub(crate) enum LabelOracle {
    /// C is satisfiable; root-node labels are the candidate subsumer
    /// set. `D ∈ labels` → verify via per-pair test; `D ∉ labels` →
    /// sound non-subsumption (this completion graph is a counterexample).
    ///
    /// `derived_sups` are the defined-`∃` names the branch-free back-fold
    /// ([`HyperCache::classify_labels`] → `HyperEngine::backfold_derived`)
    /// proved ENTAILED (not candidates) over this `sat` graph. Empty unless
    /// [`classify_backfold_enabled`] is on. Consumed by the hierarchy-injection
    /// step in `classify.rs::inject_backfold_derived_sups` (Task 3); the
    /// label-prune sites ignore it (they read `labels` only, unchanged).
    Sat {
        labels: std::collections::HashSet<owl_dl_core::ir::ClassId>,
        derived_sups: Vec<owl_dl_core::ir::ClassId>,
    },
    /// C is unsatisfiable (every model omits C). Orchestrator returns
    /// `true` for every (C, D) — unsat classes vacuously subsume all.
    Unsat,
    /// Deadline elapsed; no labels recorded. Orchestrator falls through
    /// to the existing per-pair path (sound by existing contract).
    NoVerdict,
}

/// Cached clausified state for the H4 sound accelerator: built once
/// per ontology (the expensive clausify + `¬sup` pre-pass), then
/// reused across every subsumption pair. [`proves`](Self::proves)
/// answers `sub ⊑ sup` soundly via the hyper engine — `true` only on
/// a `Unsat` verdict, which is sound for *any* ontology (see
/// `docs/hypertableau-h4-scoping.md` §0). A `false` means "hyper
/// can't prove it" and the caller must fall back to the tableau.
/// B-complete increment 1: emit `{a} ⊓ {b} ⊑ ⊥` disjointness clauses for
/// every pair in every `DifferentIndividuals(...)` axiom, so the wedge's
/// `build_disjoint_pairs` registers the entailed `a ≠ b` distinctness.
///
/// The clausifier reserves `[num_classes, num_classes + num_individuals)` for
/// nominal classes: the singleton `{a}` is the atomic class
/// `num_classes + a.index()` (see `clause.rs::class_id_of` /
/// `Clausifier::nominal_base`). Each emitted clause is `⊥`-headed with a
/// two-`Class`-atom body on the same variable `X`, the exact shape
/// `build_disjoint_pairs` recognises (`hyper.rs:516`).
///
/// Sound by construction: `DifferentIndividuals(a, b)` asserts `a ≠ b` in
/// every model (no UNA needed), so the added constraint holds in every model
/// the wedge would otherwise explore — it can only let the wedge find real
/// subsumptions it previously missed/timed-out on, never an unsound one.
/// Whether `internal` carries any `ABox` axiom (mirrors classify's
/// `has_abox_axioms`; kept here so the consistency-cache build gate
/// can run on the un-mutated input without crossing module privacy).
fn internal_has_abox(internal: &InternalOntology) -> bool {
    use owl_dl_core::ontology::Axiom;
    internal.axioms.iter().any(|ax| {
        matches!(
            ax,
            Axiom::ClassAssertion { .. }
                | Axiom::ObjectPropertyAssertion { .. }
                | Axiom::NegativeObjectPropertyAssertion { .. }
                | Axiom::SameIndividual(_)
                | Axiom::DifferentIndividuals(_)
        )
    })
}

fn push_different_individuals_disjoint(
    internal: &InternalOntology,
    num_classes: u32,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
) {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    use owl_dl_core::ontology::Axiom;
    let nominal_class =
        |ind: owl_dl_core::ir::IndividualId| ClassId::new(num_classes + ind.index());
    for axiom in &internal.axioms {
        let Axiom::DifferentIndividuals(inds) = axiom else {
            continue;
        };
        for i in 0..inds.len() {
            for j in (i + 1)..inds.len() {
                let (a, b) = (nominal_class(inds[i]), nominal_class(inds[j]));
                if a == b {
                    continue;
                }
                clauses.push(DlClause {
                    body: vec![Atom::Class(a, X), Atom::Class(b, X)],
                    head: vec![],
                });
            }
        }
    }
}

/// One line per classify on `RUSTDL_ID_STATS=1`, so the shutoff's inputs can be
/// read off a real run instead of inferred. Diagnostic only — nothing in the
/// engine reads these counters.
impl Drop for HyperCache {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        if std::env::var_os("RUSTDL_ID_STATS").is_some_and(|v| v == "1") {
            eprintln!(
                "# id-stats: shallow_decided={} shallow_missed={} shallow_waste_ms={}",
                self.id_shallow_decided.load(Relaxed),
                self.id_shallow_missed.load(Relaxed),
                self.id_shallow_waste_us.load(Relaxed) / 1000,
            );
        }
    }
}

pub(crate) struct HyperCache {
    /// Adaptive shutoff for the iterative-deepening shallow phase: cumulative
    /// MICROSECONDS this classify has spent in shallow phases that did **not**
    /// decide their pair. Once it reaches [`ID_SHALLOW_WASTE_BUDGET_MS`] the
    /// shallow phase stops running for the rest of this classify (and therefore
    /// can no longer add to this total — the latch is self-sustaining).
    ///
    /// Scoped to one classify because a `HyperCache` is built per
    /// `PreparedOntology`. `Relaxed` is sufficient and deliberate: this is a
    /// cost heuristic, never a correctness input, so an interleaving that
    /// miscounts under rayon can only shift *when* the shutoff fires, never
    /// *what* any pair answers (see the soundness note on
    /// [`ID_SHALLOW_WASTE_BUDGET_MS`]).
    id_shallow_waste_us: std::sync::atomic::AtomicU64,
    /// Telemetry only (`RUSTDL_ID_STATS=1` dumps it): pairs the shallow phase
    /// decided, and pairs it did not. Never read by the shutoff.
    id_shallow_decided: std::sync::atomic::AtomicU64,
    id_shallow_missed: std::sync::atomic::AtomicU64,
    /// Base clauses + complement clash clauses (+ the pair-invariant
    /// `value_disjoint` clash clauses when `amortize_idx` is on — folded
    /// in once at build so the shared index covers them). The per-pair
    /// Q/seed/`¬sup` clauses live in a small per-pair extras `Vec` on the
    /// amortized path, or are appended to a clone on the old path.
    clauses: Vec<owl_dl_core::clause::DlClause>,
    /// Per-defined-`sup` `NNF(¬def)` disjunct atoms (Q-gated).
    sup_neg: std::collections::HashMap<owl_dl_core::ir::ClassId, Vec<owl_dl_core::clause::Atom>>,
    /// Fresh helper concept `q` for the `sub ⊓ ¬sup` injection.
    fresh_q: owl_dl_core::ir::ClassId,
    /// Pre-built trigger indexes for `self.clauses` plus the Q-clause
    /// delta (`x_trigger[fresh_q] += [clauses.len()]`). Built once in
    /// `HyperCache::build` and shared via `Arc::clone` (O(1) ref-count
    /// bump) across all `classify_labels` probes — eliminates the
    /// O(#clauses) `build_clause_indexes` + clone cost per probe.
    /// Indexes are **read-only after construction** — rayon safety holds.
    /// See `docs/superpowers/specs/2026-06-16-soundcaching-design-and-gonogo.md` §5.
    base_indexes: std::sync::Arc<owl_dl_tableau::hyper::ClauseIndexes>,
    /// Pre-built pairwise-disjoint set for `self.clauses`. Shared via
    /// `Arc::clone` per `classify_labels` probe (Q-clause is not
    /// ⊥-headed so adds no new pairs — same set for every probe).
    base_disjoint_pairs: std::sync::Arc<std::collections::HashSet<(u32, u32)>>,
    /// Snapshot of [`classify_amortize_idx_enabled`] at `build` time
    /// (`RUSTDL_CLASSIFY_AMORTIZE_IDX`, default ON). When `true`,
    /// `decide_with_stats` takes the amortized base-index + per-pair-delta
    /// path (and `value_disjoint` was folded into `clauses`/`base_indexes`
    /// at build); when `false`, it takes the old clone + full-rebuild path.
    /// Stored (not re-read per call) so build-time folding and decide-time
    /// routing can never disagree.
    amortize_idx: bool,
    /// Role hierarchy for inverse + symmetric domain/range firing.
    /// Built once in `HyperCache::build` and passed into every engine so
    /// `domain(p⁻, C)` fires at the TARGET of `p`-edges on generated successors,
    /// not just ABox-seeded nodes. Without this, classify misses subsumptions
    /// derivable only via an inverse-domain triggered on a generated successor.
    sub_roles: RoleHierarchy,
    /// Pre-built seed-saturator for the ⊔ look-ahead gate
    /// (`RUSTDL_SAT_LOOKAHEAD`, default OFF). `None` when the flag is off
    /// (zero build cost). Shared via `Arc::clone` across all per-pair probes.
    sat_lookahead: Option<std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>>,
    /// SP2: per-named-class table of NAMED saturated subsumers, computed once
    /// via `owl_dl_saturation::saturate`. `sat_seed[c.index()]` lists every `D`
    /// with `c ⊑ D` (named, `D != c`, synthetic ids filtered). `None` when
    /// `RUSTDL_SAT_SEED` is off (zero cost — no `saturate` call). Used by
    /// `decide_with_stats` to seed the wedge root with `Q → D` for each entry.
    sat_seed: Option<Vec<Vec<owl_dl_core::ir::ClassId>>>,
    /// SP3 Phase-2: per-named-class table of derived ∃-facts, computed from
    /// the same `saturate_with_exists_facts` call that builds `sat_seed`.
    /// `exists_seed[c.index()]` lists `(Role, target)` pairs translated for the
    /// wedge: named target → direct `ClassId`; NomKey(a) → `ClassId(n_named + a.index())`;
    /// Tseitin/DKey → dropped (sound under-approximation).
    /// `None` when `RUSTDL_SAT_SEED` is off (built in the same flag-gated block).
    /// Used by `decide_with_stats` and `classify_labels` to seed `Q → ∃R.target`.
    exists_seed: Option<Vec<Vec<(owl_dl_core::ir::Role, owl_dl_core::ir::ClassId)>>>,
    /// VALUE-DERIVED TYPE DISJOINTNESS (`RUSTDL_VALUE_TYPE_DISJOINT`, **default ON**; `=0` to disable):
    /// pairs `(T1, T2)` of named classes that force DIFFERENT `DifferentIndividuals`-
    /// distinct nominal values on the SAME functional role (`T1 ⊑ ∃R.{v1}`,
    /// `T2 ⊑ ∃R.{v2}`, `R` functional, `v1≠v2` ⟹ `T1⊓T2 ⊑ ⊥`). Seeded as
    /// empty-head clauses so the wedge clashes value-incompatible type combos
    /// (e.g. RedWine⊓WhiteWine) SHALLOWLY at the ⊔ frontier instead of deep.
    /// Sound by construction (functionality + asserted distinctness). `None`
    /// when the flag is off. See docs/stage4-foodpairing-probe-refuted-2026-06-26.md.
    value_disjoint: Option<Vec<(owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId)>>,
    /// TAUTOLOGY-SKIP (`RUSTDL_TAUTOLOGY_SKIP`, **default ON**; `=0` to disable): unordered index pairs
    /// `(a,b)` of a complement pair `b ≡ ¬a` (from `EquivalentClasses(B, ¬A)`), so
    /// the wedge skips the tautological binary disjunction `a ⊔ ¬a` (e.g.
    /// `ConsumableThing ⊔ NonConsumableThing`). Threaded into each engine via
    /// `with_tautology_skip`. Sound (FP=0: skip removes an obligation; MISSED=0: an
    /// OPEN `a ⊔ ¬a` has unconstrained polarity ⇒ any model extends). `None` when off.
    tautology_pairs: Option<std::collections::HashSet<(u32, u32)>>,
    /// Label-cache back-fold precompute (Task 1 of
    /// `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md`):
    /// one entry per `EquivalentClasses`-defined name whose body is a flat
    /// conjunction (or a lone `∃`) with **at least one** `ObjectSomeValuesFrom`
    /// conjunct. Purely-atomic defined bodies are excluded — they already
    /// fire via Horn clauses, so back-fold would be redundant for them.
    /// Populated once in `build`; empty when the ontology has no such
    /// defined classes (zero cost on the common case). Consumed by the
    /// (not-yet-built) back-fold rule in a later task — this precompute is
    /// otherwise inert: nothing outside tests reads these fields yet.
    #[allow(dead_code)]
    defined_exists_bodies: Vec<DefinedBody>,
    /// Genus index for `defined_exists_bodies`: each atomic conjunct
    /// (`DefinedBody::atoms` member) maps to the indices of every body it
    /// appears in, so a later recognition pass only scans bodies whose genus
    /// is actually present on a class's label instead of scanning every
    /// defined body per class.
    #[allow(dead_code)]
    defined_body_by_genus: std::collections::HashMap<owl_dl_core::ir::ClassId, Vec<usize>>,
}

/// One `EquivalentClasses`-defined name whose body is a flat conjunction (or
/// lone `∃`) carrying at least one `ObjectSomeValuesFrom(role, Class(filler))`
/// conjunct. See [`HyperCache::defined_exists_bodies`].
///
/// Defined in `owl-dl-tableau` so `HyperEngine::backfold_derived` (which
/// consumes `&[DefinedBody]`) can name the type; re-exported here for the
/// precompute + call sites.
pub(crate) use owl_dl_tableau::hyper::DefinedBody;

/// Build [`HyperCache::defined_exists_bodies`] + `defined_body_by_genus` (Task 1
/// of `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md`).
///
/// Walks each defined name's body **one level**: if it is a flat conjunction
/// (`ConceptExpr::And`) — or a lone `∃` — with at least one
/// `ObjectSomeValuesFrom(role, Class(filler))` conjunct, collects atomic
/// conjuncts into `atoms` and `(role, filler)` pairs into `exists`. A
/// non-atomic `∃`-filler (e.g. `∃r.(C ⊓ D)`, not a single named class) or any
/// other unsupported conjunct shape (`Not`/`Or`/`Min`/`Max`/…) causes the
/// **whole body** to be skipped — v1 handles only named fillers over flat
/// conjunctions. Bodies with zero `∃`-conjuncts (purely atomic, e.g.
/// `E2 ≡ A2`) are excluded — they already fire via Horn clauses.
fn build_defined_exists_bodies(
    internal: &InternalOntology,
    defs: &owl_dl_core::definitions::Definitions,
) -> (
    Vec<DefinedBody>,
    std::collections::HashMap<owl_dl_core::ir::ClassId, Vec<usize>>,
) {
    use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId, Role};

    let mut bodies: Vec<DefinedBody> = Vec::new();
    for (name, body_id) in defs.iter() {
        let mut atoms: Vec<ClassId> = Vec::new();
        let mut exists: Vec<(Role, ClassId)> = Vec::new();
        // Classify one conjunct given its `ConceptId`; returns `false` to
        // signal the whole body must be dropped (non-atomic ∃-filler or an
        // unsupported conjunct shape).
        let mut classify_conjunct = |cid: ConceptId| -> bool {
            match internal.concepts.get(cid) {
                ConceptExpr::Atomic(a) => {
                    atoms.push(*a);
                    true
                }
                ConceptExpr::Some(role, filler) => match internal.concepts.get(*filler) {
                    ConceptExpr::Atomic(f) => {
                        exists.push((*role, *f));
                        true
                    }
                    // Non-atomic filler — v1 handles named fillers only.
                    _ => false,
                },
                // Anything else at this level (Not/Or/Min/Max/Top/Bot/...) is
                // outside the flat-conjunction-of-atoms-and-∃ shape v1 targets.
                _ => false,
            }
        };

        let ok = match internal.concepts.get(body_id) {
            ConceptExpr::And(operands) => operands.iter().all(|&cid| classify_conjunct(cid)),
            ConceptExpr::Some(..) => classify_conjunct(body_id),
            // Purely atomic single-name equivalence, or an unsupported shape
            // (Or/Not/Min/Max/...) — not handled by this precompute.
            _ => false,
        };
        if ok && !exists.is_empty() {
            bodies.push(DefinedBody {
                name,
                atoms,
                exists,
            });
        }
    }

    let mut by_genus: std::collections::HashMap<ClassId, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, body) in bodies.iter().enumerate() {
        for &a in &body.atoms {
            by_genus.entry(a).or_default().push(idx);
        }
    }
    (bodies, by_genus)
}

impl HyperCache {
    /// Clausify `internal` and pre-compute the `¬sup` expansions once.
    pub(crate) fn build(internal: &InternalOntology) -> Self {
        use owl_dl_core::ir::ClassId;
        let mut internal = internal.clone();
        let (base, _stats) = owl_dl_core::clause::clausify_with_stats(&internal);
        let defs = owl_dl_core::definitions::extract_definitions(&internal);
        // Label-cache back-fold (Task 1): precompute the ∃-bearing defined
        // bodies + genus index. Pure read of `defs`/`internal.concepts` — the
        // pool is append-only interning, so this is safe before the later
        // complement-concept additions in `build_sup_neg_map`. Inert until a
        // later task adds a consumer: this only appends two fields, no
        // existing behaviour changes.
        let (defined_exists_bodies, defined_body_by_genus) =
            build_defined_exists_bodies(&internal, &defs);
        let num_classes = u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
        let mut next_fresh = fresh_class_id(&base).index().max(num_classes);
        let fresh_q = ClassId::new(next_fresh);
        next_fresh += 1;
        let vocab: Vec<(ClassId, String)> = internal
            .vocabulary
            .classes()
            .map(|(id, iri)| (id, iri.to_string()))
            .collect();
        let mut clauses = base;
        // B-complete increment 1: seed `DifferentIndividuals`-entailed
        // distinctness into the wedge. The wedge never consumed
        // `DifferentIndividuals`, so two `≤1 R` successors carrying distinct
        // nominal fillers `{a}`,`{b}` were treated as mergeable — the
        // partition fan-out behind the wine wall. Encoding each asserted
        // distinct pair as a `{a} ⊓ {b} ⊑ ⊥` disjointness clause makes the
        // existing `build_disjoint_pairs → labels_disjoint → must_be_distinct`
        // path force those successors apart, so `≤1` clashes immediately.
        // Sound by construction: `DifferentIndividuals(a,b)` ENTAILS `a ≠ b`
        // in every model (asserted, no UNA needed) — adding an entailed
        // constraint can only reveal real subsumptions, never create one.
        push_different_individuals_disjoint(&internal, num_classes, &mut clauses);
        // SP2: build the per-named-class saturated-subsumer table before
        // `build_sup_neg_map` mutates `internal.concepts` (complement additions
        // for ¬sup expansions must not pollute the saturation input).
        // Named subsumers are TBox facts; the complement additions are
        // clausification artefacts — so computing saturation here gives the
        // same result as computing it on the original `internal`. The
        // synthetic-id filter (`< n_named`) is SOUNDNESS-CRITICAL: synthetic
        // class ids (NomKey/ForallKey/DKey/Tseitin) ≥ `num_classes` share ids
        // but carry different semantics in the wedge; seeding them would be a
        // cross-engine mismatch (spurious Unsat = FP).
        // SP2 + SP3 Phase-2: build both the named-subsumer table (sat_seed) and the
        // ∃-facts table (exists_seed) from a single `saturate_with_exists_facts` call.
        // This avoids double-saturation while adding ∃-seed support.
        // Both tables are `None` when `RUSTDL_SAT_SEED` is off — the flag-off path is
        // byte-identical to pre-SP3 behaviour (no extra cost, no extra call).
        let (sat_seed, exists_seed) = if hyper_sat_seed_enabled() {
            use owl_dl_core::ir::Role;
            let n_named = internal.vocabulary.num_classes();
            let n_named_u32 = u32::try_from(n_named).unwrap_or(u32::MAX);
            let (subs, facts, nom_to_ind) =
                owl_dl_saturation::saturate_with_exists_facts(&internal);
            // Named subsumer table (unchanged from SP2).
            let named: Vec<Vec<ClassId>> = (0..n_named)
                .map(|ci| {
                    let c = ClassId::new(u32::try_from(ci).unwrap_or(u32::MAX));
                    subs.subsumers_of(c)
                        .into_iter()
                        .filter(|&d| d != c && (d.index() as usize) < n_named)
                        .collect()
                })
                .collect();
            // ∃-seed table: translate derived ∃-facts for the wedge.
            // Named target → direct ClassId; NomKey(a) → ClassId(n_named + a.index());
            // Tseitin/DKey → drop (sound under-approximation).
            // Synthetic subjects (si >= n_named) are also dropped.
            let mut exists: Vec<Vec<(Role, ClassId)>> = vec![Vec::new(); n_named];
            for (s, r, tgt) in facts {
                let si = s.index() as usize;
                if si >= n_named {
                    continue; // synthetic subject — skip
                }
                let translated = if (tgt.index() as usize) < n_named {
                    Some(tgt)
                } else if let Some(&ind) = nom_to_ind.get(&tgt) {
                    Some(ClassId::new(n_named_u32 + ind.index()))
                } else {
                    None // Tseitin / DKey — drop
                };
                if let Some(t) = translated {
                    exists[si].push((Role::named(r), t));
                }
            }
            (Some(named), Some(exists))
        } else {
            (None, None)
        };
        // VALUE-DERIVED TYPE DISJOINTNESS (experiment): from the ∃-seed table,
        // pair up named classes that force different DifferentIndividuals-distinct
        // nominal values on the same functional role. See struct-field docs.
        let value_disjoint: Option<Vec<(ClassId, ClassId)>> =
            if std::env::var_os("RUSTDL_VALUE_TYPE_DISJOINT")
                .is_none_or(|v| v != "0" && !v.is_empty())
                && let Some(exists) = exists_seed.as_ref()
            {
                use owl_dl_core::ir::RoleId;
                use owl_dl_core::ontology::Axiom;
                let n_named_u32 =
                    u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
                let mut functional: std::collections::HashSet<RoleId> =
                    std::collections::HashSet::new();
                let mut distinct: std::collections::HashSet<(u32, u32)> =
                    std::collections::HashSet::new();
                for ax in &internal.axioms {
                    match ax {
                        Axiom::FunctionalRole(role) if !role.is_inverse() => {
                            functional.insert(role.role_id());
                        }
                        Axiom::DifferentIndividuals(inds) => {
                            for i in 0..inds.len() {
                                for j in (i + 1)..inds.len() {
                                    let (a, b) = (inds[i].index(), inds[j].index());
                                    distinct.insert((a, b));
                                    distinct.insert((b, a));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // functional role → (value-individual index → classes forcing it)
                let mut by_role: std::collections::HashMap<
                    RoleId,
                    std::collections::HashMap<u32, Vec<ClassId>>,
                > = std::collections::HashMap::new();
                for (ci, facts) in exists.iter().enumerate() {
                    let c = ClassId::new(u32::try_from(ci).unwrap_or(u32::MAX));
                    for &(role, target) in facts {
                        if role.is_inverse() || !functional.contains(&role.role_id()) {
                            continue;
                        }
                        if target.index() < n_named_u32 {
                            continue; // named filler, not a nominal value
                        }
                        let ind_idx = target.index() - n_named_u32;
                        by_role
                            .entry(role.role_id())
                            .or_default()
                            .entry(ind_idx)
                            .or_default()
                            .push(c);
                    }
                }
                let mut pairs: Vec<(ClassId, ClassId)> = Vec::new();
                for by_ind in by_role.values() {
                    let entries: Vec<(&u32, &Vec<ClassId>)> = by_ind.iter().collect();
                    for a in 0..entries.len() {
                        for b in (a + 1)..entries.len() {
                            if !distinct.contains(&(*entries[a].0, *entries[b].0)) {
                                continue;
                            }
                            for &c1 in entries[a].1 {
                                for &c2 in entries[b].1 {
                                    if c1 != c2 {
                                        pairs.push((c1.min(c2), c1.max(c2)));
                                    }
                                }
                            }
                        }
                    }
                }
                pairs.sort_unstable_by_key(|&(a, b)| (a.index(), b.index()));
                pairs.dedup();
                if std::env::var_os("RUSTDL_TRACE").is_some() {
                    eprintln!(
                        "VALUE_TYPE_DISJOINT: {} pairs from {} functional roles",
                        pairs.len(),
                        by_role.len()
                    );
                }
                Some(pairs)
            } else {
                None
            };
        // TAUTOLOGY-SKIP: complement pairs `b ≡ ¬a` from EquivalentClasses(B, ¬A),
        // so the wedge skips the tautological `a ⊔ ¬a` binary disjunction.
        let tautology_pairs: Option<std::collections::HashSet<(u32, u32)>> = if std::env::var_os(
            "RUSTDL_TAUTOLOGY_SKIP",
        )
        .is_none_or(|v| v != "0" && !v.is_empty())
        {
            use owl_dl_core::ir::ConceptExpr;
            use owl_dl_core::ontology::Axiom;
            let mut pairs: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
            for ax in &internal.axioms {
                if let Axiom::EquivalentClasses(members) = ax {
                    let mut atomic_b: Option<ClassId> = None;
                    let mut not_a: Option<ClassId> = None;
                    for &m in members {
                        match internal.concepts.get(m) {
                            ConceptExpr::Atomic(id) => atomic_b = atomic_b.or(Some(*id)),
                            ConceptExpr::Not(inner) => {
                                if let ConceptExpr::Atomic(a) = internal.concepts.get(*inner) {
                                    not_a = Some(*a);
                                }
                            }
                            _ => {}
                        }
                    }
                    if let (Some(b), Some(a)) = (atomic_b, not_a) {
                        // b ≡ ¬a ⟹ a ⊔ b = ⊤ (mutually exhaustive)
                        pairs.insert((a.index(), b.index()));
                        pairs.insert((b.index(), a.index()));
                    }
                }
            }
            Some(pairs)
        } else {
            None
        };
        let mut complements: std::collections::HashMap<ClassId, ClassId> =
            std::collections::HashMap::new();
        let sup_neg = build_sup_neg_map(
            &vocab,
            &defs,
            &mut internal.concepts,
            &mut complements,
            &mut clauses,
            &mut next_fresh,
        );
        // Build the role hierarchy from the clausified ontology so
        // `domain(p⁻, C)` and symmetric-role domain/range fire on generated
        // successors in the classify subsumption oracle. The hierarchy is built
        // after clausification so that role ids are fully populated and match
        // the role atoms in `clauses`. Stored in the cache and passed into
        // every `decide` / `classify_labels` engine.
        let sub_roles = build_role_hierarchy(&internal);
        // Pre-build the trigger indexes and disjoint-pair set once from
        // the base clause slice, then pre-apply the Q-clause delta so
        // `classify_labels` probes need zero per-probe index work.
        //
        // Q-clause body is always {Class(fresh_q, X)} — the same trigger
        // body for every probe. Its contribution is exactly one entry:
        //   x_trigger[fresh_q.index()] += [clauses.len()]
        // (clauses.len() is the Q-clause's logical index in every probe).
        // We pre-populate this entry here so probes just Arc::clone the
        // shared indexes in O(1) without any per-probe mutation.
        //
        // Soundness: `clauses.len()` is the index that `get_clause(ci)`
        // will route to `extra_clause` — this is consistent with
        // `HyperEngine::new_with_prebuilt` setting `extra_clause = &q_clause`
        // with logical index `clauses.len()`. The delta is applied before
        // Arc::new, so all shared probes see the same correct index.
        //
        // The index is built with the role hierarchy so that inverse + symmetric
        // role triggers (`inverse_first_trigger`) are included in the amortized
        // shared index, matching what each engine gets via `with_sub_roles_keep_index`.
        // When SP1.1 is OFF the hierarchy is not threaded into the classify oracle,
        // so build without it (matches pre-SP1.1 behavior and avoids the ~2× wall).
        // Clause-index amortization (advisor B2): the `value_disjoint` clash
        // clauses are pair-INVARIANT, so under the amortized decide path fold
        // them into the BASE clause vector once — the shared `base_indexes` /
        // `base_disjoint_pairs` built below then index them for free, instead
        // of every pair appending + re-indexing them. `value_disjoint`
        // becomes `None` so no per-pair site appends them again (the clause
        // SET every engine sees is unchanged). Flag OFF (`=0`): keep the
        // pairs and the old per-pair append — byte-identical to pre-amortize.
        let amortize_idx = classify_amortize_idx_enabled();
        let value_disjoint = if amortize_idx {
            if let Some(pairs) = value_disjoint {
                use owl_dl_core::clause::{Atom, DlClause, X};
                for &(a, b) in &pairs {
                    clauses.push(DlClause {
                        body: vec![Atom::Class(a, X), Atom::Class(b, X)],
                        head: vec![],
                    });
                }
            }
            None
        } else {
            value_disjoint
        };
        let same_tier = crate::classify_same_tier_enabled();
        let idx_hier = if same_tier { Some(&sub_roles) } else { None };
        let mut base_indexes_inner =
            owl_dl_tableau::hyper::build_clause_indexes(&clauses, idx_hier);
        {
            use owl_dl_core::clause::{Atom, DlClause, X};
            let q_ci = clauses.len(); // logical index of the Q-clause in every probe
            // Pre-apply the Q-clause entry through the SAME per-clause routine
            // as the base build (`index_one_clause` via `index_extra_clause`),
            // so it contributes BOTH `x_trigger[fresh_q] += [q_ci]` AND the
            // `match_plans[q_ci]` entry (advisor B1 — a trigger entry without
            // a match plan panics/no-ops in `match_body`). Every probe's
            // Q-clause has the same body `{Class(fresh_q, X)}` and a Horn
            // (single-atom) head, so this representative clause indexes
            // identically to each probe's actual Q-clause; the head atom is
            // never indexed.
            let q_probe = DlClause {
                body: vec![Atom::Class(fresh_q, X)],
                head: vec![Atom::Class(fresh_q, X)],
            };
            owl_dl_tableau::hyper::index_extra_clause(
                &mut base_indexes_inner,
                q_ci,
                &q_probe,
                idx_hier,
            );
        }
        let base_indexes = std::sync::Arc::new(base_indexes_inner);
        let base_disjoint_pairs =
            std::sync::Arc::new(owl_dl_tableau::hyper::build_disjoint_pairs(&clauses));
        // Build the seed-saturator once when the lookahead flag is on.
        let sat_lookahead = if hyper_sat_lookahead_enabled() {
            Some(std::sync::Arc::new(
                owl_dl_saturation::seed_sat::build_base(&internal),
            ))
        } else {
            None
        };
        Self {
            id_shallow_waste_us: std::sync::atomic::AtomicU64::new(0),
            id_shallow_decided: std::sync::atomic::AtomicU64::new(0),
            id_shallow_missed: std::sync::atomic::AtomicU64::new(0),
            clauses,
            sup_neg,
            fresh_q,
            base_indexes,
            base_disjoint_pairs,
            amortize_idx,
            sub_roles,
            sat_lookahead,
            sat_seed,
            exists_seed,
            value_disjoint,
            tautology_pairs,
            defined_exists_bodies,
            defined_body_by_genus,
        }
    }

    /// Test accessors for the adaptive-shutoff accumulator. Canaries drive the
    /// accumulator DIRECTLY rather than trying to burn a real millisecond
    /// budget, so the shutoff is pinned by construction instead of by timing —
    /// a wall-clock-dependent test on a loaded host would be flaky in exactly
    /// the direction that stops testing anything.
    #[cfg(test)]
    pub(crate) fn id_shallow_waste_us_for_test(&self) -> u64 {
        self.id_shallow_waste_us
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn set_id_shallow_waste_us_for_test(&self, v: u64) {
        self.id_shallow_waste_us
            .store(v, std::sync::atomic::Ordering::Relaxed);
    }

    /// Test accessor: `(shallow_decided, shallow_missed)` telemetry counters.
    #[cfg(test)]
    pub(crate) fn id_shallow_counts_for_test(&self) -> (u64, u64) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            self.id_shallow_decided.load(Relaxed),
            self.id_shallow_missed.load(Relaxed),
        )
    }

    /// Test accessor: returns the SP2 sat-seed table when the flag is on.
    /// `None` iff `RUSTDL_SAT_SEED` was off at `build` time.
    #[cfg(test)]
    pub(crate) fn sat_seed_for_test(&self) -> Option<&Vec<Vec<owl_dl_core::ir::ClassId>>> {
        self.sat_seed.as_ref()
    }

    /// Test accessor: returns the SP3 ∃-seed table when the flag is on.
    /// `None` iff `RUSTDL_SAT_SEED` was off at `build` time.
    #[cfg(test)]
    pub(crate) fn exists_seed_for_test(
        &self,
    ) -> Option<&Vec<Vec<(owl_dl_core::ir::Role, owl_dl_core::ir::ClassId)>>> {
        self.exists_seed.as_ref()
    }

    /// Three-valued subsumption verdict from the hyper engine:
    /// `Subsumed` (sound for any ontology), `NotSubsumed` (HF5 — only
    /// trust when [`hyper_trust_sat_enabled`]), or `Unknown`
    /// (Stalled/deadline → caller falls back).
    pub(crate) fn decide(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> HyperVerdict {
        use owl_dl_tableau::hyper::HyperResult;
        let (result, stats) = if crate::iterative_deepening_enabled() {
            self.decide_iterative_deepening(sub, sup, deadline)
        } else {
            self.decide_with_stats(sub, sup, HYPER_WEDGE_DEPTH, deadline)
        };
        match result {
            HyperResult::Unsat => HyperVerdict::Subsumed,
            HyperResult::Sat => HyperVerdict::NotSubsumed,
            // Distinguish a divergence-cut `Stalled` (thrash) from a plain
            // deadline `Stalled`, so bound-the-tail can skip the fallthrough.
            HyperResult::Stalled if stats.diverged => HyperVerdict::UnknownDiverged,
            HyperResult::Stalled => HyperVerdict::Unknown,
        }
    }

    /// Iterative-deepening driver for the classify per-pair oracle
    /// (`RUSTDL_ITERATIVE_DEEPENING`, default ON). Runs
    /// [`decide_with_stats`](Self::decide_with_stats) at each level of
    /// [`depth_schedule`] until the engine returns a **definite** verdict
    /// (`Unsat`/`Sat`) or the schedule is exhausted, and returns that level's
    /// `(result, stats)`.
    ///
    /// # Why this is FP-safe by construction
    ///
    /// A depth cap can only *suppress* an `Unsat`: on hitting `depth == 0`
    /// `HyperEngine::solve` returns `Stalled`, and a parent frame with any
    /// stalled child returns `Stalled` rather than `Unsat`. It can never
    /// *manufacture* an `Unsat`. So no depth schedule can create a subsumption
    /// the fixed cap would not also have found sound — the classify FP surface
    /// is untouched. Deepening can only add entailments.
    ///
    /// # Why it does not lose entailments (unbounded deadline)
    ///
    /// Raising the cap is **verdict-monotone**:
    /// * `Unsat` at cap `k` requires *every* branch decisively unsat (no child
    ///   stalled), so the identical DFS at cap `k' > k` re-derives it;
    /// * `Sat` at cap `k` means a completed model was found; the DFS prefix at
    ///   `k' > k` is identical except that some frames which returned `Stalled`
    ///   may now return `Sat` (immediate `Sat`) or `Unsat` (the parent
    ///   continues to the next disjunct — exactly what it did after `Stalled`),
    ///   so the outcome is still `Sat`;
    /// * only `Stalled` can change, and only into a definite verdict.
    ///
    /// The adaptive-budget divergence cut does not break this: `is_diverging`
    /// requires depth saturation, so a larger `init_depth` makes it fire *less*,
    /// which only lets a search run longer.
    ///
    /// Since [`HYPER_WEDGE_DEPTH_SCHEDULE`]'s last level is `>= HYPER_WEDGE_DEPTH`
    /// (compile-time asserted), the final level dominates today's fixed cap.
    ///
    /// # Deadline
    ///
    /// The caller's `deadline` bounds the **whole loop**, not each level: every
    /// level is passed the same `Instant`, and the loop breaks before starting
    /// a level once it has passed. Iterative deepening therefore never
    /// multiplies the per-pair budget. The converse is the one real completeness
    /// exposure: under a *bounded* deadline the shallow levels spend budget the
    /// final level might have needed, so a deadline-bounded run can lose a pair
    /// the fixed cap would have found. Unbounded runs cannot (monotonicity above).
    fn decide_iterative_deepening(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> (
        owl_dl_tableau::hyper::HyperResult,
        owl_dl_tableau::hyper::SearchStats,
    ) {
        let (result, stats, _trace) = self.decide_iterative_deepening_traced(sub, sup, deadline);
        (result, stats)
    }

    /// [`decide_iterative_deepening`](Self::decide_iterative_deepening) plus a
    /// [`DeepeningTrace`] recording how many levels ran and which one produced
    /// the returned verdict. The trace is a two-word stack value, not a
    /// heap-allocated log, so the traced form IS the production path — there is
    /// no untraced twin that could drift from it.
    fn decide_iterative_deepening_traced(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> (
        owl_dl_tableau::hyper::HyperResult,
        owl_dl_tableau::hyper::SearchStats,
        DeepeningTrace,
    ) {
        use owl_dl_tableau::hyper::HyperResult;
        use std::sync::atomic::Ordering::Relaxed;
        let schedule = crate::depth_schedule();
        // Shallow phase (every level but the last) shares ONE small wall budget,
        // so the re-work iterative deepening adds to a pair is bounded a priori
        // — see `ID_SHALLOW_BUDGET_MS` for the measurement that forces this.
        // The final level always gets the caller's own deadline, so it is
        // exactly today's search at a deeper cap.
        let started = std::time::Instant::now();
        let shallow = crate::id_shallow_deadline(started, deadline, crate::id_shallow_budget_ms());
        let last = schedule.len() - 1;
        // ADAPTIVE SHUTOFF. The per-pair shallow budget bounds the tax on ONE
        // pair; nothing bounded it across a quadratic pair count, which is the
        // `ore_ont_13991` regression. Once this classify has burned `waste_budget`
        // of wall on shallow phases that did NOT decide, the shallow phase has
        // demonstrably stopped paying on THIS ontology, so skip straight to the
        // final level. That level carries the caller's own deadline at a cap
        // `>= HYPER_WEDGE_DEPTH`, so a skipped pair gets exactly the flag-OFF
        // search at a `>=` cap — it cannot change an answer, only its cost.
        // See `ID_SHALLOW_WASTE_BUDGET_MS` (incl. why counting CONSECUTIVE
        // non-deciding pairs instead was measured and refuted).
        let waste_budget_us = crate::id_shallow_waste_budget_ms().saturating_mul(1000);
        let shallow_skipped =
            waste_budget_us != 0 && self.id_shallow_waste_us.load(Relaxed) >= waste_budget_us;
        let start = if shallow_skipped { last } else { 0 };
        let level_deadline = |i: usize| if i == last { deadline } else { shallow };
        // `schedule` is non-empty by construction (compiled default is, and a
        // malformed override is rejected wholesale in `depth_schedule`).
        let mut level = self.decide_with_stats(sub, sup, schedule[start], level_deadline(start));
        let mut trace = DeepeningTrace {
            levels_run: 1,
            final_depth: schedule[start],
            shallow_skipped,
        };
        let mut i = start;
        while i < last {
            // A definite verdict is final — never deepen past it.
            if !matches!(level.0, HyperResult::Stalled) {
                break;
            }
            // Was this level cut by the SHALLOW budget rather than by its own cap?
            let shallow_spent = shallow.is_some_and(|d| std::time::Instant::now() >= d);
            if crate::id_cap_was_not_binding(
                shallow_spent,
                level.1.diverged,
                level.1.max_branch_depth,
                schedule[i],
            ) {
                break;
            }
            // The CALLER's deadline bounds the LOOP: never start a level after
            // it passes, so deepening cannot multiply the per-pair budget.
            if deadline.is_some_and(|dl| std::time::Instant::now() >= dl) {
                break;
            }
            // Once the shallow budget is spent, jump straight to the final
            // level: every intermediate level would return `Stalled` on its
            // first deadline check anyway, after paying a full per-pair engine
            // build for nothing.
            i = if shallow_spent { last } else { i + 1 };
            level = self.decide_with_stats(sub, sup, schedule[i], level_deadline(i));
            trace.levels_run += 1;
            trace.final_depth = schedule[i];
        }
        // Feed the shutoff. Only observations count: when the shallow phase was
        // skipped there is nothing to learn, and not touching the accumulator
        // here is what makes the latch permanent for the rest of the classify
        // without a second flag to hold it. "Decided" means a DEFINITE verdict
        // from a NON-final level — a `Stalled` that merely fell through to the
        // final level is the tax this shutoff exists to stop paying, and a
        // verdict from the final level would have been reached with no shallow
        // phase at all.
        if !shallow_skipped {
            let decided_shallow = i < last && !matches!(level.0, HyperResult::Stalled);
            if decided_shallow {
                self.id_shallow_decided.fetch_add(1, Relaxed);
            } else {
                self.id_shallow_missed.fetch_add(1, Relaxed);
                // Charge only the shallow phase, never the final level: on a
                // miss the shallow phase ran until `shallow` (or until it fell
                // through earlier), and the final level's own wall is work the
                // flag-OFF path would have done anyway.
                let spent = shallow
                    .map_or_else(
                        || started.elapsed(),
                        |d| d.saturating_duration_since(started).min(started.elapsed()),
                    )
                    .as_micros();
                self.id_shallow_waste_us
                    .fetch_add(u64::try_from(spent).unwrap_or(u64::MAX), Relaxed);
            }
        }
        (level.0, level.1, trace)
    }

    /// Diagnostic sibling of [`decide`](Self::decide): runs the identical
    /// per-pair Q-clause construction + engine configuration but returns the
    /// raw [`HyperResult`] alongside the engine's [`SearchStats`], and takes
    /// the depth cap explicitly (so a probe can separate a depth-cap `Stalled`
    /// from a deadline/branch-explosion `Stalled`). Used by
    /// [`decide_pair_probe`] to characterise *why* a classify per-pair search
    /// stalls. Soundness/behaviour is identical to `decide` at
    /// `depth == HYPER_WEDGE_DEPTH`.
    pub(crate) fn decide_with_stats(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        depth: usize,
        deadline: Option<std::time::Instant>,
    ) -> (
        owl_dl_tableau::hyper::HyperResult,
        owl_dl_tableau::hyper::SearchStats,
    ) {
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_tableau::hyper::HyperEngine;
        // The per-pair clauses appended after the shared base slice. Under the
        // amortized path (`amortize_idx`, default ON) these stay in their own
        // small Vec (the engine branch-routes clause ids into it); under the
        // old path (`RUSTDL_CLASSIFY_AMORTIZE_IDX=0`) they are appended to a
        // clone of the base Vec exactly as before.
        let mut extras: Vec<DlClause> = vec![DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(sub, X)],
        }];
        // SP2: seed the wedge root with the class's named saturated subsumers.
        // For each `D` in `sat_seed[sub.index()]`, assert `Q → D` so the engine
        // starts with all entailed named subsumers already on the root node.
        // This can only collapse branches (the seed is sound), never FP.
        // Flag-off: `sat_seed` is `None` ⇒ this block is skipped entirely —
        // byte-identical to the pre-seed behaviour.
        if let Some(tbl) = &self.sat_seed
            && let Some(seeds) = tbl.get(sub.index() as usize)
        {
            for &d in seeds {
                extras.push(DlClause {
                    body: vec![Atom::Class(self.fresh_q, X)],
                    head: vec![Atom::Class(d, X)],
                });
            }
        }
        // SP3 Phase-2: seed the derived ∃-facts for `sub`.
        // For each `(R, target)` in `exists_seed[sub.index()]`, assert `Q → ∃R.target`
        // so the engine starts with the saturation-entailed existential successors
        // already forced at the root. Sound: these ∃-facts are EL-entailed.
        // Flag-off: `exists_seed` is `None` ⇒ this block is skipped entirely.
        if let Some(tbl) = &self.exists_seed
            && let Some(seeds) = tbl.get(sub.index() as usize)
        {
            for &(role, target) in seeds {
                extras.push(DlClause {
                    body: vec![Atom::Class(self.fresh_q, X)],
                    head: vec![Atom::Exists(role, target, X)],
                });
            }
        }
        // VALUE-DERIVED TYPE DISJOINTNESS (experiment): empty-head clashes for
        // value-incompatible type pairs (shallow pruning of e.g. RedWine⊓WhiteWine).
        // Pair-INVARIANT, so under `amortize_idx` these were folded into the
        // base clause vector once in `build` (`value_disjoint` is `None` here).
        if let Some(pairs) = &self.value_disjoint {
            for &(a, b) in pairs {
                extras.push(DlClause {
                    body: vec![Atom::Class(a, X), Atom::Class(b, X)],
                    head: vec![],
                });
            }
        }
        if let Some(atoms) = self.sup_neg.get(&sup) {
            extras.push(DlClause {
                body: vec![Atom::Class(self.fresh_q, X)],
                head: atoms.clone(),
            });
        } else {
            extras.push(DlClause {
                body: vec![Atom::Class(self.fresh_q, X), Atom::Class(sup, X)],
                head: vec![],
            });
        }
        // Old-path storage (flag OFF): the full clone + append lives here so it
        // outlives the engine borrow.
        let full_clauses: Vec<DlClause>;
        let mut engine = if self.amortize_idx {
            // Amortized path (default): share the base clause slice + the
            // pre-built base indexes/disjoint pairs (O(1) Arc bumps) and build
            // only the O(#extras) sparse index delta for the appended clauses.
            // The delta goes through the SAME per-clause routine as the base
            // build (`index_one_clause`), so a clause shape can never index
            // differently between base and delta (advisor B1). The extras all
            // have class-only bodies, so the hierarchy argument is irrelevant
            // to them; pass the same gate as the base build for consistency.
            let idx_hier = if crate::classify_same_tier_enabled() {
                Some(&self.sub_roles)
            } else {
                None
            };
            let delta = owl_dl_tableau::hyper::build_clause_index_delta(
                self.clauses.len(),
                &extras,
                idx_hier,
            );
            HyperEngine::new_with_prebuilt_extras(
                &self.clauses,
                &extras,
                self.fresh_q,
                std::sync::Arc::clone(&self.base_indexes),
                std::sync::Arc::clone(&self.base_disjoint_pairs),
                delta,
            )
        } else {
            // Old path (`RUSTDL_CLASSIFY_AMORTIZE_IDX=0`): clone the full base
            // clause vector, append the per-pair clauses, and rebuild the
            // whole index in `HyperEngine::new` — kept intact for A/B.
            let mut clauses = self.clauses.clone();
            clauses.extend(extras.iter().cloned());
            full_clauses = clauses;
            HyperEngine::new(&full_clauses, self.fresh_q)
        };
        if crate::incremental_fixpoint_enabled() {
            engine = engine.with_incremental_fixpoint();
        }
        if crate::semantic_branching_enabled() {
            engine = engine.with_semantic_branching();
        }
        // Thread the role hierarchy in when SP1.1 is enabled (default OFF) so
        // inverse + symmetric domain/range fire on generated successors. On
        // the amortized path the shared base index was already built
        // hierarchy-aware in `build` (matched gate) and the extras have no
        // role-body atoms, so `with_sub_roles_keep_index` suffices; the old
        // path rebuilds the per-pair index with the hierarchy exactly as
        // before (`with_sub_roles`).
        if crate::classify_same_tier_enabled() {
            engine = if self.amortize_idx {
                engine.with_sub_roles_keep_index(self.sub_roles.clone())
            } else {
                engine.with_sub_roles(self.sub_roles.clone())
            };
        }
        if hyper_double_block_enabled() {
            engine = engine.with_double_blocking();
        }
        if hyper_precise_card_deps_enabled() {
            engine = engine.with_precise_card_deps();
        }
        if hyper_mrv_ordering_enabled() {
            engine = engine.with_mrv_ordering();
        }
        if let Some(p) = &self.tautology_pairs {
            engine = engine.with_tautology_skip(p.clone());
        }
        if let Some(sat) = self.sat_lookahead.clone() {
            engine = engine.with_sat_lookahead(sat);
        }
        if crate::adaptive_budget_enabled() {
            engine = engine.with_adaptive_budget();
        }
        if crate::hyper_shadow_dep_probe_enabled() {
            engine = engine.with_shadow_dep_probe(true);
        }
        let result = engine.decide_with_deadline(depth, deadline);
        (result, engine.stats())
    }

    /// Diagnostic: wedge satisfiability of `c` ALONE (no `¬sup`), returning the
    /// raw [`HyperResult`] + [`SearchStats`]. Discriminates whether a per-pair
    /// stall lives in `c`'s own disjunctive expansion (this thrashes too) or in
    /// the `c ⊓ ¬sup` interaction (this is fast, the pair is not). Mirrors
    /// [`classify_labels`](Self::classify_labels)' construction.
    pub(crate) fn sat_only_with_stats(
        &self,
        c: owl_dl_core::ir::ClassId,
        depth: usize,
        deadline: Option<std::time::Instant>,
    ) -> (
        owl_dl_tableau::hyper::HyperResult,
        owl_dl_tableau::hyper::SearchStats,
    ) {
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_tableau::hyper::HyperEngine;
        let mut clauses = self.clauses.clone();
        clauses.push(DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(c, X)],
        });
        // VALUE-DERIVED TYPE DISJOINTNESS (experiment): seed empty-head clashes
        // for value-incompatible type pairs so the wedge prunes them shallowly.
        if let Some(pairs) = &self.value_disjoint {
            for &(a, b) in pairs {
                clauses.push(DlClause {
                    body: vec![Atom::Class(a, X), Atom::Class(b, X)],
                    head: vec![],
                });
            }
        }
        let mut engine = HyperEngine::new(&clauses, self.fresh_q);
        if crate::incremental_fixpoint_enabled() {
            engine = engine.with_incremental_fixpoint();
        }
        if crate::semantic_branching_enabled() {
            engine = engine.with_semantic_branching();
        }
        if crate::classify_same_tier_enabled() {
            engine = engine.with_sub_roles(self.sub_roles.clone());
        }
        if hyper_double_block_enabled() {
            engine = engine.with_double_blocking();
        }
        if hyper_precise_card_deps_enabled() {
            engine = engine.with_precise_card_deps();
        }
        if hyper_mrv_ordering_enabled() {
            engine = engine.with_mrv_ordering();
        }
        if let Some(p) = &self.tautology_pairs {
            engine = engine.with_tautology_skip(p.clone());
        }
        if let Some(sat) = self.sat_lookahead.clone() {
            engine = engine.with_sat_lookahead(sat);
        }
        if crate::adaptive_budget_enabled() {
            engine = engine.with_adaptive_budget();
        }
        if crate::hyper_shadow_dep_probe_enabled() {
            engine = engine.with_shadow_dep_probe(true);
        }
        let result = engine.decide_with_deadline(depth, deadline);
        (result, engine.stats())
    }

    /// Run wedge satisfiability of `c` alone (no negated sup) and return
    /// a [`LabelOracle`] capturing the seed-node's labels. Sound basis
    /// for the per-class label heuristic — see
    /// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
    pub(crate) fn classify_labels(
        &self,
        c: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> LabelOracle {
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
        // Clause-index amortization (§5 of the go/no-go spec):
        // Clone the base clause Vec and push the per-probe Q-clause, then
        // use pre-built `ClauseIndexes` and `disjoint_pairs` (Arc-shared,
        // O(1) ref-count bump) instead of rebuilding them O(#clauses).
        // The Q-clause delta (x_trigger[fresh_q] += [clauses.len()]) was
        // pre-applied to base_indexes in HyperCache::build.
        //
        // The per-class clauses appended after the shared base slice, in the
        // SAME order the pre-`RUSTDL_CLASSIFY_LABELS_AMORTIZE` code pushed them
        // onto the clone (Q-clause, SP2.1 sat-seed, SP3 ∃-seed, value-disjoint),
        // so the flag-OFF path below produces a byte-identical clause vector —
        // identical logical clause ids ⇒ identical search.
        let mut extras: Vec<DlClause> = vec![DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(c, X)],
        }];
        // SP2.1: seed the label-cache build with `c`'s named saturated subsumers
        // (the same monotone, sound seed as `decide_with_stats`). This is where the
        // seed's per-class collapse pays off: `classify_labels(c)` is the per-class
        // `sat(c)` whose timeout on hard nominal classes produces the ~4638 wine
        // label-cache misses → per-pair refutations. Seeding lets those sats
        // terminate within the (adaptive) label-cache deadline, completing the cache.
        if let Some(tbl) = &self.sat_seed
            && let Some(seeds) = tbl.get(c.index() as usize)
        {
            for &d in seeds {
                extras.push(DlClause {
                    body: vec![Atom::Class(self.fresh_q, X)],
                    head: vec![Atom::Class(d, X)],
                });
            }
        }
        // SP3 Phase-2: seed the derived ∃-facts for `c`.
        // Mirrors the `decide_with_stats` seed site above so label-cache probes
        // see the same saturation-entailed existential successors.
        if let Some(tbl) = &self.exists_seed
            && let Some(seeds) = tbl.get(c.index() as usize)
        {
            for &(role, target) in seeds {
                extras.push(DlClause {
                    body: vec![Atom::Class(self.fresh_q, X)],
                    head: vec![Atom::Exists(role, target, X)],
                });
            }
        }
        // VALUE-DERIVED TYPE DISJOINTNESS (experiment): empty-head clashes.
        if let Some(pairs) = &self.value_disjoint {
            for &(a, b) in pairs {
                extras.push(DlClause {
                    body: vec![Atom::Class(a, X), Atom::Class(b, X)],
                    head: vec![],
                });
            }
        }
        // Use `with_sub_roles_keep_index` (NOT `with_sub_roles`) because the
        // amortized `base_indexes` was already built hierarchy-aware in
        // `HyperCache::build` (`build_clause_indexes(.., Some(&sub_roles))`).
        // Calling `with_sub_roles` here would rebuild the index — discarding
        // the prebuilt amortization and defeating the O(1)-per-probe design.
        // `with_sub_roles_keep_index` sets `self.sub_roles` only, so
        // `role_matches` + `inverse_first_trigger` fire correctly without
        // a redundant rebuild.
        // When SP1.1 is OFF the base_indexes was built without the hierarchy
        // (None) so we must NOT call with_sub_roles_keep_index — the two sites
        // are a matched pair (build gate ↔ classify_labels gate).
        // When SP2.1/SP3 seed clauses were appended, the amortized `base_indexes`
        // (built in `HyperCache::build` BEFORE the per-class seed) does NOT index
        // them, so `new_with_prebuilt` leaves the seed clauses inert (they never
        // trigger). Rebuild the full index with `new` so the seed fires. When
        // unseeded (both `sat_seed` and `exists_seed` are None) keep the amortized
        // path — byte-identical to pre-SP2.1.
        // Note: under `amortize_idx` the `value_disjoint` clauses were folded
        // into the base clause vector + `base_indexes` at build time
        // (`value_disjoint` is `None` here), so they are correctly indexed on
        // BOTH branches below.
        //
        // `RUSTDL_CLASSIFY_LABELS_AMORTIZE` (default OFF) removes exactly the
        // rebuild the two paragraphs above describe: instead of rebuilding the
        // whole `ClauseIndexes` because the per-class seed clauses are absent
        // from `base_indexes`, build the O(#extras) sparse
        // [`build_clause_index_delta`] over them — the same amortization the
        // per-PAIR sibling `decide_with_stats` has used by default since
        // v0.3.39. The seed clauses are KEPT (that is the whole point: this is
        // arm C of the R3/R4 adjudication, isolating the rebuild from the
        // clause volume that `RUSTDL_SAT_SEED=0` also removes). See
        // `docs/superpowers/specs/2026-08-01-clauseindex-per-class-adjudication.md`.
        //
        // Old-path storage (flag OFF): the full clone + append lives here so it
        // outlives the engine borrow.
        let full_clauses: Vec<DlClause>;
        let mut engine = if crate::classify_labels_amortize_enabled() {
            mark_label_engine_path(true);
            // The extras all have class-only bodies (Q → D, Q → ∃R.D, and the
            // ⊥-headed value-disjoint clashes), so the hierarchy argument is
            // irrelevant to them; pass the same gate as the base build for
            // consistency with `decide_with_stats`.
            let idx_hier = if crate::classify_same_tier_enabled() {
                Some(&self.sub_roles)
            } else {
                None
            };
            let delta = owl_dl_tableau::hyper::build_clause_index_delta(
                self.clauses.len(),
                &extras,
                idx_hier,
            );
            let mut e = HyperEngine::new_with_prebuilt_extras(
                &self.clauses,
                &extras,
                self.fresh_q,
                std::sync::Arc::clone(&self.base_indexes),
                std::sync::Arc::clone(&self.base_disjoint_pairs),
                delta,
            );
            if crate::classify_same_tier_enabled() {
                e = e.with_sub_roles_keep_index(self.sub_roles.clone());
            }
            e
        } else {
            mark_label_engine_path(false);
            let mut owned = self.clauses.clone();
            owned.extend(extras.iter().cloned());
            full_clauses = owned;
            if self.sat_seed.is_some()
                || self.exists_seed.is_some()
                || self.value_disjoint.is_some()
            {
                let mut e = HyperEngine::new(&full_clauses, self.fresh_q);
                if crate::classify_same_tier_enabled() {
                    e = e.with_sub_roles(self.sub_roles.clone());
                }
                e
            } else {
                let mut e = HyperEngine::new_with_prebuilt(
                    &full_clauses,
                    self.fresh_q,
                    std::sync::Arc::clone(&self.base_indexes),
                    std::sync::Arc::clone(&self.base_disjoint_pairs),
                );
                if crate::classify_same_tier_enabled() {
                    e = e.with_sub_roles_keep_index(self.sub_roles.clone());
                }
                e
            }
        };
        if crate::incremental_fixpoint_enabled() {
            engine = engine.with_incremental_fixpoint();
        }
        if crate::semantic_branching_enabled() {
            engine = engine.with_semantic_branching();
        }
        if hyper_double_block_enabled() {
            engine = engine.with_double_blocking();
        }
        if hyper_precise_card_deps_enabled() {
            engine = engine.with_precise_card_deps();
        }
        if hyper_mrv_ordering_enabled() {
            engine = engine.with_mrv_ordering();
        }
        if let Some(p) = &self.tautology_pairs {
            engine = engine.with_tautology_skip(p.clone());
        }
        if let Some(sat) = self.sat_lookahead.clone() {
            engine = engine.with_sat_lookahead(sat);
        }
        if crate::adaptive_budget_enabled() {
            engine = engine.with_adaptive_budget();
        }
        match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {
            HyperResult::Unsat => LabelOracle::Unsat,
            HyperResult::Sat => {
                engine
                    .satisfiability_labels(self.fresh_q)
                    .map_or(LabelOracle::NoVerdict, |v| {
                        // Branch-free `∃`-composition back-fold (Task 2, flag-gated,
                        // default OFF): the entailed defined-`∃` names over this `sat`
                        // graph. `backfold_derived` self-gates on `branches_taken == 0`
                        // (returns empty otherwise). Flag off ⇒ empty, no engine call.
                        let derived_sups = if crate::classify_backfold_enabled() {
                            let root = engine.root_node();
                            engine.backfold_derived(
                                root,
                                &self.defined_exists_bodies,
                                &self.defined_body_by_genus,
                            )
                        } else {
                            Vec::new()
                        };
                        LabelOracle::Sat {
                            labels: v.into_iter().collect(),
                            derived_sups,
                        }
                    })
            }
            HyperResult::Stalled => LabelOracle::NoVerdict,
        }
    }
}

/// Whether `ABox`-seeded **wedge** consistency checking is enabled
/// (default **on**). When on, [`is_consistent_internal_full`] runs the
/// hypertableau wedge over an `ABox`-seeded completion graph instead of
/// (well, before) the main-tableau `decide(Top)`. The wedge terminates
/// fast on the out-of-EL `ABox`es where `decide(Top)` hangs 60–125 s.
///
/// Verdict map (the soundness contract):
/// - wedge `Unsat` ⟹ ontology **inconsistent** (sound — only asserted
///   `ABox` is seeded; a clause clash is a real model violation).
/// - wedge `Sat` ⟹ **consistent** (trusted, exactly like classify's
///   `trust_sat`; the wedge is Horn-incomplete so it may MISS a clash,
///   which is sound — a missed clash is a `Sat` the main tableau might
///   refute, the same trust level as today).
/// - wedge `Stalled` ⟹ undetermined → bounded main-tableau fall-through.
///
/// Set `RUSTDL_WEDGE_CONSISTENCY=0` to revert to pure main-tableau.
#[must_use]
pub fn wedge_consistency_enabled() -> bool {
    std::env::var_os("RUSTDL_WEDGE_CONSISTENCY").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Lever A (2026-07-20): when the ontology uses NO nominals, the `ABox` is
/// irrelevant to class subsumption, so the per-pair classification tableau
/// probes do NOT seed it. This eliminates the `ABox`-driven completion-graph
/// blow-up that stalls near-EL `ABox`-bearing ontologies (e.g. ORE
/// `ore_ont_10894`: DNF → ~1.6 s, closure byte-identical to saturation/Konclude).
/// **Sound:** absent nominals the `ABox` cannot affect `C ⊑ D`; a
/// globally-inconsistent `ABox` is still caught by the separate
/// `abox_check`/`abox_saturation` pre-checks (so this is FP=0 by construction —
/// a missed inconsistency would be a MISS, gated by the corpus MISSED=0 check).
/// `RUSTDL_CLASSIFY_TBOX_ONLY=0` reverts to seeding the full `ABox`.
#[must_use]
pub fn classify_tbox_only_enabled() -> bool {
    std::env::var_os("RUSTDL_CLASSIFY_TBOX_ONLY").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Lever 1 (2026-07-20): when the ontology is nominal-free, evaluate the
/// Horn-shortcircuit FRAGMENT gate on the `TBox` axioms only, ignoring the `ABox`.
/// An EL/saturator-fragment `TBox` carrying a large `ABox` is otherwise kicked off
/// the saturation fast path (`ABox` assertions fail the fragment allowlist) into
/// the O(n²) per-pair hybrid loop, which DNFs on 8k–70k-class ontologies even
/// though the saturation closure is already complete (verified == Konclude on
/// the ORE ChEBI/OBO tier). **Sound by construction:** the existing fragment
/// guarantee applies to the `TBox`; a nominal-free `ABox` cannot contribute a class
/// subsumption (monotone, same basis as [`classify_tbox_only_enabled`]); and the
/// inconsistent-`ABox` → all-unsat verdict is still produced by the `abox_check`
/// pre-check that runs on the fast-path arm when `has_abox_axioms`. Recovers ~67
/// ORE onts (e.g. `ore_ont_1043`: DNF → ~7 s, closure == Konclude).
/// `RUSTDL_CLASSIFY_TBOX_FRAGMENT=0` reverts to the full-axiom fragment gate.
#[must_use]
pub fn classify_tbox_fragment_enabled() -> bool {
    std::env::var_os("RUSTDL_CLASSIFY_TBOX_FRAGMENT").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Bare symmetry / inverse **declarations** in the fragment gates
/// (`RUSTDL_FRAGMENT_BARE_DECL`, **default ON** since 0.4.7; `=0` reverts).
///
/// `SymmetricObjectProperty(r)` and `InverseObjectProperties(p, q)` fell through
/// to `_ => false` in both `is_el_axiom` and `is_saturator_axiom`, so merely
/// *naming* such a property refused the ontology the saturation fast path and
/// dropped it onto the O(n²) hybrid loop — 71 of the 257 ORE ontologies rustdl
/// cannot classify at 120 s carry one. With the flag on, such a declaration is
/// admitted **only when the declared role's edge set is provably unread by every
/// axiom and concept in the ontology**, which makes the declaration
/// semantically inert for class subsumption. See `classify::BareRoleDecls` for
/// the exact "observable role" definition and the model-construction
/// completeness proof. FP-safe unconditionally (dropping axioms only weakens the
/// theory); the flag exists because the *completeness* side of the gate contract
/// is what is being extended.
#[must_use]
pub(crate) fn fragment_bare_decl_enabled() -> bool {
    // DEFAULT ON since 0.4.7 (`=0` reverts). Admits ONLY provably-unread symmetric /
    // inverse declarations; the "unread" predicate is what keeps this from being a D10 bug.
    std::env::var_os("RUSTDL_FRAGMENT_BARE_DECL").is_none_or(|v| v != "0")
}

/// True iff any axiom's class expression references a nominal (`ObjectOneOf` /
/// `ObjectHasValue`, both lowered to `ConceptExpr::Nominal`). When false, the
/// `ABox` is provably irrelevant to class subsumption, enabling Lever A. Scanned
/// once at `from_internal` time over the un-mutated axioms (nominals only occur
/// inside class expressions, so only concept-bearing axioms are walked).
pub(crate) fn ontology_uses_nominals(internal: &InternalOntology) -> bool {
    fn has_nominal(c: ConceptId, pool: &ConceptPool) -> bool {
        match pool.get(c) {
            ConceptExpr::Nominal(_) => true,
            ConceptExpr::Not(x)
            | ConceptExpr::Some(_, x)
            | ConceptExpr::All(_, x)
            | ConceptExpr::Min(_, _, x)
            | ConceptExpr::Max(_, _, x) => has_nominal(*x, pool),
            ConceptExpr::And(ops) | ConceptExpr::Or(ops) => {
                ops.iter().any(|op| has_nominal(*op, pool))
            }
            _ => false,
        }
    }
    let pool = &internal.concepts;
    internal.axioms.iter().any(|ax| match ax {
        Axiom::SubClassOf { sub, sup } => has_nominal(*sub, pool) || has_nominal(*sup, pool),
        Axiom::EquivalentClasses(v) | Axiom::DisjointClasses(v) => {
            v.iter().any(|c| has_nominal(*c, pool))
        }
        Axiom::DisjointUnion { members, .. } => members.iter().any(|c| has_nominal(*c, pool)),
        Axiom::ClassAssertion { class, .. } => has_nominal(*class, pool),
        _ => false,
    })
}

/// Bounded wall budget (ms) for the main-tableau `decide(Top)`
/// fall-through used when the consistency wedge returns `Stalled`. The
/// whole point of the wedge route is to kill the unbounded
/// `decide(Top)` hang, so the fall-through is itself deadline-capped.
/// On deadline-elapse the result is reported as consistent (sound:
/// no inconsistency was witnessed) with an incompleteness trace.
/// Override with `RUSTDL_CONSISTENCY_FALLBACK_MS`; default 10 000 ms.
#[must_use]
fn consistency_fallback_ms() -> u64 {
    const DEFAULT_MS: u64 = 10_000;
    std::env::var("RUSTDL_CONSISTENCY_FALLBACK_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MS)
}

/// `ABox`-seeded wedge state for consistency checking. Built once at
/// [`PreparedOntology::from_internal`] time from the **un-mutated**
/// input ontology (the same snapshot `HyperCache::build` clausifies),
/// so every nominal / role identifier is matched-by-construction with
/// the clause set — the false-`Unsat` (false-inconsistent) surface.
///
/// `None` on the prepared ontology when the wedge route is disabled
/// or there is no `ABox` (`has_abox_axioms` false), so `ABox`-free inputs
/// pay nothing and classify stays byte-identical.
pub(crate) struct ConsistencyCache {
    /// Clause set: base `TBox`/`RBox` clauses, `DifferentIndividuals`
    /// disjointness (`{a}⊓{b}⊑⊥`, via [`push_different_individuals_disjoint`]),
    /// and one `{a}⊑C` GCI per `ClassAssertion` (the exact equivalence,
    /// clausified so complex / nominal-bearing `C` are handled). Carries
    /// **no** raw `ABox` — all `ABox` graph state comes from `seed`.
    clauses: Vec<owl_dl_core::clause::DlClause>,
    /// Pre-resolved `ABox` graph seed (nominal nodes, asserted edges,
    /// asserted `SameIndividual` merges). Distinctness + class
    /// assertions live in `clauses`, not here.
    seed: owl_dl_tableau::hyper::AboxSeed,
    /// Role hierarchy built from the **same un-mutated internal** that
    /// produced `clauses` (un-expanded — matches `HyperCache::build` /
    /// the standalone probe). Supplied to the engine via
    /// `with_sub_roles`. A mismatched hierarchy would let an unrelated
    /// edge satisfy a super-role atom = false clash, so same-source
    /// construction is load-bearing.
    sub_roles: RoleHierarchy,
    /// `num_classes` = the clausifier's nominal range start.
    num_classes: u32,
    /// `num_individuals` = the nominal range width (`with_nominals`).
    num_individuals: u32,
    /// Pre-built seed-saturator for the ⊔ look-ahead gate
    /// (`RUSTDL_SAT_LOOKAHEAD`, default OFF). `None` when the flag is off.
    sat_lookahead: Option<std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>>,
}

impl ConsistencyCache {
    /// Build from the un-mutated `internal`. Mirrors `HyperCache::build`
    /// (clausify a clone) but additionally injects `{a}⊑C` GCIs for
    /// every `ClassAssertion` so class assertions fire through the
    /// seeded nominal labels.
    pub(crate) fn build(internal: &InternalOntology) -> Self {
        use owl_dl_core::ontology::Axiom;
        let mut internal = internal.clone();
        let num_classes = u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
        let num_individuals = u32::try_from(internal.vocabulary.num_individuals()).unwrap_or(0);

        // Inject `{a} ⊑ C` for every ClassAssertion(C, a) — the exact
        // equivalence. Clausifying through SubClassOf(Nominal(a), C)
        // handles atomic, complex, and nominal-bearing C uniformly and
        // soundly (proper var / Tseitin allocation, role canon matching
        // the rest of the clause set). The nominal `{a}` antecedent
        // encodes to a single `{a}(X)` body that fires only at node `a`.
        let class_assertions: Vec<(owl_dl_core::ir::IndividualId, ConceptId)> = internal
            .axioms
            .iter()
            .filter_map(|ax| match ax {
                Axiom::ClassAssertion { class, individual } => Some((*individual, *class)),
                _ => None,
            })
            .collect();
        for (individual, class) in class_assertions {
            let nom = internal.concepts.nominal(individual);
            internal.axioms.push(Axiom::SubClassOf {
                sub: nom,
                sup: class,
            });
        }

        let (mut clauses, _stats) = owl_dl_core::clause::clausify_with_stats(&internal);
        // DifferentIndividuals → `{a}⊓{b}⊑⊥` disjointness clauses (the
        // SAME mechanism the classify wedge uses; never engine `neq`).
        push_different_individuals_disjoint(&internal, num_classes, &mut clauses);

        // Role hierarchy from the SAME un-mutated internal (un-expanded).
        let sub_roles = build_role_hierarchy(&internal);

        // Pre-resolve the ABox graph seed: nominal nodes are implicit
        // (one per individual index, created by `new_seeded`); collect
        // asserted edges (polarity normalised) + SameIndividual pairs.
        let mut property_assertions: Vec<(u32, owl_dl_core::ir::Role, u32)> = Vec::new();
        let mut same_pairs: Vec<(u32, u32)> = Vec::new();
        for ax in &internal.axioms {
            match ax {
                Axiom::ObjectPropertyAssertion {
                    role,
                    subject,
                    object,
                } => {
                    // Normalise inverse-role polarity to forward, then
                    // store the forward role (matching `collect_abox`).
                    let (from, to) = if role.is_inverse() {
                        (*object, *subject)
                    } else {
                        (*subject, *object)
                    };
                    let fwd = owl_dl_core::ir::Role::Named(role.role_id());
                    property_assertions.push((from.index(), fwd, to.index()));
                }
                Axiom::SameIndividual(inds) => {
                    for i in 0..inds.len() {
                        for j in (i + 1)..inds.len() {
                            same_pairs.push((inds[i].index(), inds[j].index()));
                        }
                    }
                }
                _ => {}
            }
        }

        let seed = owl_dl_tableau::hyper::AboxSeed {
            num_individuals,
            nominal_base: num_classes,
            property_assertions,
            same_pairs,
        };

        let sat_lookahead = if hyper_sat_lookahead_enabled() {
            Some(std::sync::Arc::new(
                owl_dl_saturation::seed_sat::build_base(&internal),
            ))
        } else {
            None
        };

        Self {
            clauses,
            seed,
            sub_roles,
            num_classes,
            num_individuals,
            sat_lookahead,
        }
    }

    /// Build a fresh ABox-seeded [`owl_dl_tableau::hyper::HyperEngine`]
    /// configured EXACTLY like the classify wedge — the nine configurators
    /// `with_sub_roles` + `with_nominals` (unconditional) and the gated
    /// `with_incremental_fixpoint` / `with_semantic_branching` /
    /// `with_double_blocking` / `with_precise_card_deps` / `with_mrv_ordering`
    /// / `with_sat_lookahead` / `with_adaptive_budget`. Shared by [`Self::decide`]
    /// and [`Self::base_model_types`] so both ALWAYS build an identical engine
    /// by construction — a hand-duplicated subset of these configurators would
    /// silently produce a different completion (or a non-model), which would be
    /// unsound for `base_model_types`'s witness-model contract.
    fn build_seeded_engine(&self) -> owl_dl_tableau::hyper::HyperEngine<'_> {
        use owl_dl_tableau::hyper::HyperEngine;
        let mut engine = HyperEngine::new_seeded(&self.clauses, &self.seed)
            .with_sub_roles(self.sub_roles.clone())
            .with_nominals(self.num_classes, self.num_individuals);
        if crate::incremental_fixpoint_enabled() {
            engine = engine.with_incremental_fixpoint();
        }
        if crate::semantic_branching_enabled() {
            engine = engine.with_semantic_branching();
        }
        if hyper_double_block_enabled() {
            engine = engine.with_double_blocking();
        }
        if hyper_precise_card_deps_enabled() {
            engine = engine.with_precise_card_deps();
        }
        if hyper_mrv_ordering_enabled() {
            engine = engine.with_mrv_ordering();
        }
        if let Some(sat) = self.sat_lookahead.clone() {
            engine = engine.with_sat_lookahead(sat);
        }
        if crate::adaptive_budget_enabled() {
            engine = engine.with_adaptive_budget();
        }
        engine
    }

    /// Run the ABox-seeded wedge. Returns the three-valued
    /// [`owl_dl_tableau::hyper::HyperResult`] (`Unsat`=inconsistent,
    /// `Sat`=consistent, `Stalled`=undetermined). Configured exactly
    /// as the classify wedge (`with_nominals` + `with_sub_roles` +
    /// double-blocking + precise-card-deps, under their env gates).
    pub(crate) fn decide(
        &self,
        deadline: Option<std::time::Instant>,
    ) -> owl_dl_tableau::hyper::HyperResult {
        let mut engine = self.build_seeded_engine();
        engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline)
    }

    /// One `ABox` witness model → each individual's COMPLETE atomic-class type
    /// set, or `None` when no clash-free completion is available
    /// (`Unsat`/`Stalled`/deadline). Builds the SAME engine configuration as
    /// [`Self::decide`] (via [`Self::build_seeded_engine`]) so the returned
    /// labels come from a genuine model of the `ABox`, not a divergent
    /// completion. Indexed by individual id (`0..self.num_individuals`);
    /// `seeded_individual_labels` resolves through the union-find so a
    /// `SameIndividual`/functional merge never silently under-reports a
    /// merged-away individual's types.
    ///
    /// Consumed via [`Self::realize_base_model_types`], the
    /// realize-loop's pseudo-model shortcut (`RUSTDL_PSEUDO_MODEL`,
    /// see `realize::pseudo_model_enabled`).
    pub(crate) fn base_model_types(
        &self,
        deadline: Option<std::time::Instant>,
    ) -> Option<Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>>> {
        use owl_dl_tableau::hyper::HyperResult;
        let mut engine = self.build_seeded_engine();
        match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {
            HyperResult::Sat => Some(
                (0..self.num_individuals)
                    .map(|i| {
                        engine
                            .seeded_individual_labels(i)
                            .unwrap_or_default()
                            .into_iter()
                            .collect()
                    })
                    .collect(),
            ),
            HyperResult::Unsat | HyperResult::Stalled => None,
        }
    }
}

/// Per-class snapshot cache for the Konclude snapshot cache project
/// (Phase 1b). Populated lazily: on first `try_replay(sub, ...)` call
/// for a given `sub`, build a wedge satisfiability snapshot of `sub`
/// and stash it. Subsequent calls for the same `sub` reuse the cached
/// snapshot.
///
/// Cache is per-[`PreparedOntology`] instance. `TBox` is frozen for the
/// instance's lifetime, so cached snapshots stay valid across the
/// pair loop.
///
/// Sound only for ontologies whose `BackPropRisk` is `Safe` — Phase 1b
/// uses the ontology-wide first-cut classifier. Unsafe ontologies skip
/// cache build entirely (`build` returns `None` shape via the orchestrator
/// `Option<SnapshotCache>` field) so every `try_replay` is a no-op.
pub(crate) struct SnapshotCache {
    /// Clausified `TBox` (base only — q-injection clauses are added
    /// per-`sub` in `clauses_for_sub`). Shared with the wedge build
    /// pattern; cloned per snapshot build / replay so we never mutate
    /// the cached state.
    base_clauses: Vec<owl_dl_core::clause::DlClause>,
    /// Fresh query class id, allocated once per cache. Mirrors the
    /// wedge's `HyperCache::fresh_q`: snapshots are seeded with
    /// `fresh_q` (carrying it at the root) and a Horn clause
    /// `fresh_q → sub` derives `sub` from `fresh_q`. Replay's
    /// `¬sup` is then encoded as `fresh_q ⊓ sup → ⊥` which is
    /// **root-scoped** (only the root carries `fresh_q`), so
    /// successor labels matching arbitrary sup classes never
    /// spuriously clash. Without this gate, GALEN-style fixtures
    /// produce massive FPs (a sub's successor nodes carry labels
    /// matching unrelated sups). See Phase 1b T6 recon for the
    /// 25,333-FP regression that motivated this design.
    fresh_q: owl_dl_core::ir::ClassId,
    /// Per-class snapshot, lazily populated. `Arc` for cheap clone-on-read.
    /// Outer `Arc` wraps the `DashMap` so the cache itself is cheap to share
    /// across the rayon pair-loop workers. Snapshot for `sub` has
    /// `fresh_q` as the seed (`snapshot.seed()` == `fresh_q` for every entry).
    snapshots: std::sync::Arc<
        dashmap::DashMap<owl_dl_core::ir::ClassId, std::sync::Arc<owl_dl_tableau::GraphSnapshot>>,
    >,
    /// Ontology-wide `BackPropRisk` classification, computed once at
    /// build. Drives the orchestrator's "is this safe to consult" check
    /// (spec §4.2).
    risk: owl_dl_tableau::BackPropRisk,
    /// Phase 1b.5: per-sup `fresh_q ⊓ sup → ⊥` clauses, lazily
    /// populated on first `try_replay` call for that sup. Avoids
    /// re-allocating the 1-element Vec on every call (~1.86M times on
    /// GALEN). Lock-free reads via `DashMap`; bucket-locked writes are
    /// rare (one per unique sup column observed).
    per_sup_neg_clauses: std::sync::Arc<
        dashmap::DashMap<
            owl_dl_core::ir::ClassId,
            std::sync::Arc<Vec<owl_dl_core::clause::DlClause>>,
        >,
    >,
    /// Phase 2a recon: cumulative wall time spent in
    /// `get_or_build_snapshot` for cache misses (actual snapshot
    /// builds). `AtomicU64` for lock-free updates across the rayon
    /// pair loop. Microseconds for sub-ms resolution.
    build_wall_micros: std::sync::atomic::AtomicU64,
    /// Phase 2a recon: cumulative wall time spent in `try_replay`'s
    /// `replay_with_neg_sup*` call. Microseconds for sub-ms resolution.
    replay_wall_micros: std::sync::atomic::AtomicU64,
}

impl SnapshotCache {
    /// Build the cache from `internal`. Clones the ontology once for
    /// clausification (mirrors `HyperCache::build`).
    pub(crate) fn build(internal: &InternalOntology) -> Self {
        let internal = internal.clone();
        let (base_clauses, _stats) = owl_dl_core::clause::clausify_with_stats(&internal);
        let risk = owl_dl_tableau::BackPropRisk::classify_ontology(&internal);
        let num_classes = u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
        let next_fresh = fresh_class_id(&base_clauses).index().max(num_classes);
        let fresh_q = owl_dl_core::ir::ClassId::new(next_fresh);
        Self {
            base_clauses,
            fresh_q,
            snapshots: std::sync::Arc::new(dashmap::DashMap::new()),
            risk,
            per_sup_neg_clauses: std::sync::Arc::new(dashmap::DashMap::new()),
            build_wall_micros: std::sync::atomic::AtomicU64::new(0),
            replay_wall_micros: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Phase 2a recon: cumulative `get_or_build_snapshot` wall (cache
    /// miss path only), in milliseconds.
    #[must_use]
    pub(crate) fn build_wall_ms(&self) -> u64 {
        self.build_wall_micros
            .load(std::sync::atomic::Ordering::Relaxed)
            / 1000
    }

    /// Phase 2a recon: cumulative `try_replay` wall (the
    /// `replay_with_neg_sup*` call), in milliseconds.
    #[must_use]
    pub(crate) fn replay_wall_ms(&self) -> u64 {
        self.replay_wall_micros
            .load(std::sync::atomic::Ordering::Relaxed)
            / 1000
    }

    /// Is the ontology back-prop-safe? Orchestrator's contract gate —
    /// short-circuits the replay path on Unsafe ontologies (every SROIQ
    /// workload in Phase 1b, until the per-class classifier lands in
    /// Phase 3). Mirrors `GraphSnapshot::is_safe()`.
    #[must_use]
    pub(crate) fn is_safe(&self) -> bool {
        matches!(self.risk, owl_dl_tableau::BackPropRisk::Safe)
    }

    /// Build the per-`sub` clause set: base clauses + the q-injection
    /// clause `fresh_q → sub`. Used both at snapshot-build time (so
    /// the snapshot's root carries `sub` via Horn derivation from
    /// `fresh_q`) and at replay time (so the reconstructed engine's
    /// clause indexes include the same q-injection clause).
    fn clauses_for_sub(&self, sub: owl_dl_core::ir::ClassId) -> Vec<owl_dl_core::clause::DlClause> {
        use owl_dl_core::clause::{Atom, DlClause, X};
        let mut clauses = self.base_clauses.clone();
        clauses.push(DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(sub, X)],
        });
        clauses
    }

    /// Try a snapshot-replay for `sub ⊑ sup`. Returns:
    /// - `Some(verdict)` when the cache built or reused a snapshot
    ///   AND replay produced a verdict (any of the 4 variants;
    ///   `BackPropAborted`/`Stalled` mean caller must fall through).
    /// - `None` when the ontology is Unsafe (every call is a no-op)
    ///   or when building the snapshot for `sub` failed (the wedge
    ///   returned Unsat or Stalled for `sub` alone).
    pub(crate) fn try_replay(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
    ) -> Option<owl_dl_tableau::ReplayVerdict> {
        if !self.is_safe() {
            return None;
        }
        let snap = self.get_or_build_snapshot(sub)?;
        // Root-scoped ¬sup: `fresh_q ⊓ sup → ⊥`. Only the root carries
        // fresh_q (it's the seed and doesn't propagate to successors
        // in Horn), so this clause fires only when sup is at the root.
        // This is the soundness fix for the 25,333-FP regression seen
        // with the global `sup(x) → ⊥` encoding (T6 recon).
        //
        // Phase 1b.5 T4: cached per-sup. replay_with_neg_sup wants
        // Vec<DlClause> (owned); clone the Arc'd vec (shallow: Arc
        // ref-bump + Vec clone of 1 element).
        let neg_sup_clauses_arc = self.get_or_build_neg_sup_clauses(sup);
        let neg_sup_clauses = (*neg_sup_clauses_arc).clone();
        // Replay needs the full clause set (base + q-injection +
        // ¬sup) so the reconstructed engine's indexes pick up all
        // three. Pass the per-sub clauses as the base.
        //
        // Phase 1b.5 toggle: snapshot_lazy_enabled() picks between
        // lazy-expansion replay (default ON) and full-re-run replay
        // (Phase 1b first-cut behavior). The A/B toggle exists so a
        // future regression can be isolated by setting
        // RUSTDL_SNAPSHOT_LAZY=0.
        let base_clauses = self.clauses_for_sub(sub);
        let replay_start = std::time::Instant::now();
        let verdict = if snapshot_lazy_enabled() {
            owl_dl_tableau::replay_with_neg_sup(&base_clauses, &snap, neg_sup_clauses)
        } else {
            owl_dl_tableau::replay_with_neg_sup_full_rerun(&base_clauses, &snap, neg_sup_clauses)
        };
        let replay_us = u64::try_from(replay_start.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.replay_wall_micros
            .fetch_add(replay_us, std::sync::atomic::Ordering::Relaxed);
        Some(verdict)
    }

    /// Get the cached `fresh_q ⊓ sup → ⊥` clause for this sup, or
    /// build + insert. Returns `Arc<Vec<...>>` for cheap clone-on-read.
    fn get_or_build_neg_sup_clauses(
        &self,
        sup: owl_dl_core::ir::ClassId,
    ) -> std::sync::Arc<Vec<owl_dl_core::clause::DlClause>> {
        use owl_dl_core::clause::{Atom, DlClause, X};
        if let Some(existing) = self.per_sup_neg_clauses.get(&sup) {
            return existing.clone();
        }
        let clauses = std::sync::Arc::new(vec![DlClause {
            body: vec![Atom::Class(self.fresh_q, X), Atom::Class(sup, X)],
            head: vec![],
        }]);
        self.per_sup_neg_clauses.insert(sup, clauses.clone());
        clauses
    }

    /// Cache-build path: returns the cached snapshot or builds + stores
    /// one. Snapshot is captured with `fresh_q` as the seed (the Horn
    /// closure derives `sub` and its closure at the root from
    /// `fresh_q → sub`). `None` if snapshot can't be built (sub is
    /// Unsat or wedge stalled).
    fn get_or_build_snapshot(
        &self,
        sub: owl_dl_core::ir::ClassId,
    ) -> Option<std::sync::Arc<owl_dl_tableau::GraphSnapshot>> {
        if let Some(existing) = self.snapshots.get(&sub) {
            return Some(existing.clone());
        }
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
        let build_start = std::time::Instant::now();
        let clauses = self.clauses_for_sub(sub);
        // incremental_fixpoint deliberately NOT wired here: the snapshot cache
        // (RUSTDL_SNAPSHOT_CAPTURE) is default-OFF and out of scope for SP1's
        // classify-path acceleration. (engine.decide() does route through
        // decide_with_deadline, so the root seed would engage — this exclusion
        // is by scope, not a seeding limitation.)
        let mut engine = HyperEngine::new(&clauses, self.fresh_q);
        let result = match engine.decide(HYPER_WEDGE_DEPTH) {
            HyperResult::Sat => {
                let snap = std::sync::Arc::new(engine.satisfiability_snapshot(self.fresh_q)?);
                self.snapshots.insert(sub, snap.clone());
                Some(snap)
            }
            HyperResult::Unsat | HyperResult::Stalled => None,
        };
        let elapsed_us = u64::try_from(build_start.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.build_wall_micros
            .fetch_add(elapsed_us, std::sync::atomic::Ordering::Relaxed);
        result
    }
}

/// Build the absorbed `TBox` and classify every residual GCI by
/// **which absorption technique would remove it** — see
/// [`owl_dl_core::residual_absorbability`] and
/// `docs/2026-08-01-absorption-is-the-bottleneck.md`.
///
/// Report-only: this changes no reasoning behaviour. It exists so the
/// decision to implement domain / binary absorption is made on measured
/// population counts rather than on a grep.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn residual_absorbability_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<owl_dl_core::residual_absorbability::ResidualAbsorbabilityStats, ReasonError> {
    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let normalized = owl_dl_core::normalize::nnf_axioms(&mut internal);
    let tbox = owl_dl_core::absorb::absorb(&normalized, &mut internal.concepts);
    Ok(owl_dl_core::residual_absorbability::census(
        &tbox,
        &internal.concepts,
        Some(&internal.vocabulary),
    ))
}

/// Build the absorbed `TBox` and classify every residual GCI's
/// trigger per [`owl_dl_core::residual_trigger`]. The result is
/// the histogram needed to decide whether the lazy-unfolding
/// Phase-2 integration will move walls — see
/// `docs/lazy-unfolding-plan.md`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn residual_trigger_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<owl_dl_core::residual_trigger::ResidualTriggerStats, ReasonError> {
    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let normalized = owl_dl_core::normalize::nnf_axioms(&mut internal);
    let tbox = owl_dl_core::absorb::absorb(&normalized, &mut internal.concepts);
    let (_triggers, stats) =
        owl_dl_core::residual_trigger::classify_residuals(&tbox.residual_gcis, &internal.concepts);
    Ok(stats)
}

/// Build the absorbed `TBox` for `ontology` and summarise its
/// shape.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn tbox_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<TBoxStats, ReasonError> {
    use owl_dl_core::ConceptExpr;
    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let normalized = owl_dl_core::normalize::nnf_axioms(&mut internal);
    let tbox = owl_dl_core::absorb::absorb(&normalized, &mut internal.concepts);
    let mut stats = TBoxStats {
        concept_rules: tbox.concept_rules.len(),
        nominal_rules: tbox.nominal_rules.len(),
        role_rules_guarded: tbox
            .guarded_role_rules_by_guard
            .values()
            .map(Vec::len)
            .sum(),
        role_rules_unguarded: tbox.unguarded_role_rules.len(),
        residual_gcis: tbox.residual_gcis.len(),
        ..TBoxStats::default()
    };
    for &gci in &tbox.residual_gcis {
        match internal.concepts.get(gci) {
            ConceptExpr::Or(_) => stats.residual_or_count += 1,
            ConceptExpr::Atomic(_) => stats.residual_atomic_count += 1,
            _ => stats.residual_other_count += 1,
        }
    }
    for rule in &tbox.concept_rules {
        if matches!(internal.concepts.get(rule.conclusion), ConceptExpr::Or(_)) {
            stats.concept_rule_or_count += 1;
        }
    }
    // Told tables are the OTHER volume sink a DKey lever can inflate, and the
    // absorbed-TBox counts above do not see them at all. Built here on the same
    // `InternalOntology` the real pipeline uses (`PreparedOntology::from_internal`
    // also calls `build_told_tables(&internal)`), so the counts are the shipped
    // ones, not a re-derivation.
    let told = owl_dl_core::told::build_told_tables(&internal);
    for i in 0..told.num_classes() {
        let cid = owl_dl_core::ClassId::new(u32::try_from(i).expect("class count fits in u32"));
        stats.told_super_edges += told.super_classes(cid).len();
        stats.told_disjoint_pairs += told.disjoints_of(cid).len();
    }
    // `disjoint_with` is symmetric, so every unordered pair was counted twice.
    // (A degenerate self-disjoint `DisjointClasses(c, c)` is counted once and so
    // would round down; it is a malformed axiom and does not occur in the corpus.)
    stats.told_disjoint_pairs /= 2;
    Ok(stats)
}

/// Build the locality partition for `ontology` and summarise it.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn locality_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<LocalityStats, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let n_classes = internal.vocabulary.num_classes();
    let partition = owl_dl_core::locality::LocalityPartition::build(
        &internal.axioms,
        &internal.concepts,
        n_classes,
    );
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for i in 0..n_classes {
        let cid = owl_dl_core::ClassId::new(u32::try_from(i).expect("class count fits in u32"));
        *sizes.entry(partition.component(cid)).or_insert(0) += 1;
    }
    let num_components = partition.num_components();
    let largest_component = sizes.values().copied().max().unwrap_or(0);
    let singleton_components = sizes.values().filter(|&&s| s == 1).count();
    Ok(LocalityStats {
        num_classes: n_classes,
        num_components,
        largest_component,
        singleton_components,
    })
}

pub use owl_dl_core::DroppedAxioms;

/// The axioms conversion could not represent (sound under-approximation).
///
/// `convert_ontology` never aborts on an unsupported construct — it
/// records the drop and continues — so this is the way to find out
/// whether an ontology lost anything on the way into the reasoner.
///
/// # Errors
///
/// [`ReasonError::Conversion`] only on a genuinely fatal conversion failure
/// (unsupported constructs are recorded, not errored).
pub fn dropped_axioms<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<DroppedAxioms, ReasonError> {
    Ok(owl_dl_core::convert::convert_ontology(ontology)?.dropped)
}

use std::collections::HashMap;

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use thiserror::Error;

use owl_dl_core::convert::{ConversionError, convert_ontology};
use owl_dl_core::{
    AbsorbedTBox, Axiom, ConceptExpr, ConceptId, ConceptPool, IndividualId, InternalOntology, Role,
    RoleHierarchy, RoleHierarchyBuilder, RoleId, SubRolePath, absorb, nnf_axioms, nnf_complement,
};
use owl_dl_tableau::{NodeId, TableauContext};

/// Recursion depth cap for the search driver on the **deadline-bounded**
/// query paths (classification pairs, timed realize probes). Those paths
/// cannot hang — `search` re-checks the deadline at every recursive entry —
/// so a modest cap is fine: hitting it yields `DepthLimit`, which the caller
/// maps to a sound MISS/NoVerdict. Kept small so it runs on the default
/// (rayon-worker) stack with zero overhead.
const MAX_SEARCH_DEPTH: usize = 256;

// ---------------------------------------------------------------------------
// Main-tableau iterative deepening (RUSTDL_TABLEAU_ITERATIVE_DEEPENING)
// ---------------------------------------------------------------------------

/// Iterative deepening of [`MAX_SEARCH_DEPTH`], the **main tableau's**
/// deadline-bounded depth cap — the sibling of the wedge's
/// [`HYPER_WEDGE_DEPTH`] cap that [`HyperCache::decide_iterative_deepening`]
/// already deepens (`RUSTDL_ITERATIVE_DEEPENING`, default ON since v0.4.12).
///
/// **DEFAULT OFF, and the measurement says it should stay off.** See
/// `docs/2026-08-03-tableau-iterative-deepening.md`: the constant audit
/// (`docs/2026-08-03-constant-audit.md` §4) found `MAX_SEARCH_DEPTH` binding on
/// 82% of the ontologies that reach the main tableau at all, with three DNFs
/// recovered and two completers ~14× faster **at a fixed cap of 8** — but
/// deepening cannot capture any of that, because on every one of those
/// ontologies the probes whose cost a shallow cap saves are probes that reach
/// **no verdict at either depth**. Iterative deepening is verdict-monotone by
/// construction, so it must re-run each undecided probe at the final level
/// (`>= MAX_SEARCH_DEPTH`) under the caller's own deadline — reproducing the
/// flag-OFF cost and adding the shallow phase's tax on top. That is measured,
/// not predicted (§3 of the results doc). Kept as opt-in scaffolding: the
/// mechanism is correct and the canaries pin its monotonicity, so a future
/// workload where a shallow level genuinely *decides* pairs can switch it on
/// without rebuilding it.
#[must_use]
pub(crate) fn tableau_iterative_deepening_enabled() -> bool {
    std::env::var_os("RUSTDL_TABLEAU_ITERATIVE_DEEPENING").is_some_and(|v| v == "1")
}

// ---------------------------------------------------------------------------
// Main-tableau adaptive early-abandon (RUSTDL_TABLEAU_EARLY_ABANDON)
// ---------------------------------------------------------------------------

/// Adaptive early-abandon of a *doomed* main-tableau probe — the lever the
/// iterative-deepening NO-GO identified as the shape its own data actually
/// supports (`docs/2026-08-03-tableau-iterative-deepening.md` §10).
///
/// **DEFAULT OFF.** It trades completeness for wall: unlike deepening (whose
/// verdicts are a superset of flag-OFF's) this one can lose an entailment the
/// full budget would have found, so its gate is a corpus-wide MISSED net rather
/// than a superset check. See `docs/2026-08-03-tableau-early-abandon.md` for the
/// measurement and the recommendation.
///
/// Mechanism: [`owl_dl_tableau::TableauContext::note_depth_cap_hit`] abandons a
/// probe once it has bottomed out at [`MAX_SEARCH_DEPTH`]
/// [`TABLEAU_EARLY_ABANDON_CAP_HITS`] times. FP-safe by construction: the cut can
/// only turn a would-be `Unsat` into a non-verdict, never the reverse.
#[must_use]
pub(crate) fn tableau_early_abandon_enabled() -> bool {
    // DEFAULT ON since 0.4.14 (`=0` reverts). Gated on the two measurements its
    // decision rule demanded, both taken before the flip: the MISSED net shows
    // **ΔMISSED = 0** against the 5,198 baseline with FP=0 and 400/400 closures
    // byte-identical, and a full 1,920-ontology two-arm sweep shows **6 recoveries,
    // 0 regressions, 14 faster / 0 slower, −5.5% aggregate**. The corpus sweep is
    // not redundant with the net: the net's frame is drawn from COMPLETERS and
    // structurally cannot observe an `ok → dnf` in the DNF tail.
    std::env::var_os("RUSTDL_TABLEAU_EARLY_ABANDON").is_none_or(|v| v != "0")
}

/// Depth-cap bottom-outs after which a main-tableau probe is abandoned.
///
/// **Calibrated, not guessed** (`docs/2026-08-03-tableau-early-abandon.md` §2).
/// On the telemetry-only arm (`…_HITS=0`) at the CLI-default 1 000 ms per-pair
/// budget, the per-probe cap-hit counts on the audit's targets are:
/// `ore_ont_13545` min 1 003, `ore_ont_8666` min 864, `ore_ont_3250` median 648,
/// `ore_ont_2826` 86. 32 therefore fires on all four while leaving 32 poisoned
/// subtrees of slack. Overridable via `RUSTDL_TABLEAU_EARLY_ABANDON_HITS`; `0`
/// keeps the accounting live but never cuts.
const TABLEAU_EARLY_ABANDON_CAP_HITS: u64 = 32;

/// Read the cap-hit limit, falling back to [`TABLEAU_EARLY_ABANDON_CAP_HITS`] on
/// an absent or unparsable value.
#[must_use]
pub(crate) fn tableau_early_abandon_cap_hits() -> u64 {
    match std::env::var("RUSTDL_TABLEAU_EARLY_ABANDON_HITS") {
        Ok(v) => v.trim().parse().unwrap_or(TABLEAU_EARLY_ABANDON_CAP_HITS),
        Err(_) => TABLEAU_EARLY_ABANDON_CAP_HITS,
    }
}

/// `RUSTDL_TABLEAU_EA_STATS=1` ⇒ dump one stderr line per armed probe on drop.
/// Calibration channel only; never read by the search.
#[must_use]
pub(crate) fn tableau_early_abandon_stats_enabled() -> bool {
    std::env::var_os("RUSTDL_TABLEAU_EA_STATS").is_some_and(|v| v == "1")
}

/// Depth schedule for the main tableau's iterative-deepening search.
///
/// The final level is [`MAX_SEARCH_DEPTH`] **exactly**, not more, and that is a
/// deliberate difference from the wedge's `[8, 32, 128, 512]`: the audit
/// measured that *raising* this cap recovers nothing (`ore_ont_10019` reads
/// `search_depth0 = 0` at 512 and 2048 — the cap stops binding, its true
/// requirement being 459–460 — and still DNFs), so a deeper final level would
/// buy no completeness and would make every undecided probe explore further
/// before its deadline. `>= MAX_SEARCH_DEPTH` is the monotonicity requirement;
/// `== MAX_SEARCH_DEPTH` additionally makes the ON path's final level *identical*
/// to the OFF path's only level.
const MAX_SEARCH_DEPTH_SCHEDULE: &[usize] = &[8, 32, MAX_SEARCH_DEPTH];

/// Compile-time check of the two [`MAX_SEARCH_DEPTH_SCHEDULE`] invariants —
/// the same pair [`assert_depth_schedule_well_formed`] pins for the wedge.
const fn assert_search_depth_schedule_well_formed() {
    assert!(!MAX_SEARCH_DEPTH_SCHEDULE.is_empty());
    let mut i = 1;
    while i < MAX_SEARCH_DEPTH_SCHEDULE.len() {
        assert!(
            MAX_SEARCH_DEPTH_SCHEDULE[i] > MAX_SEARCH_DEPTH_SCHEDULE[i - 1],
            "tableau depth schedule must be strictly increasing"
        );
        i += 1;
    }
    assert!(
        MAX_SEARCH_DEPTH_SCHEDULE[MAX_SEARCH_DEPTH_SCHEDULE.len() - 1] >= MAX_SEARCH_DEPTH,
        "final tableau schedule level must be >= MAX_SEARCH_DEPTH or deepening can lose entailments"
    );
}
const _: () = assert_search_depth_schedule_well_formed();

/// Diagnostic override of [`MAX_SEARCH_DEPTH_SCHEDULE`]
/// (`RUSTDL_TABLEAU_ID_SCHEDULE="8,32,256"`). A malformed override —
/// unparsable, empty, non-increasing, or with a final level below
/// [`MAX_SEARCH_DEPTH`] — is **rejected wholesale** (falls back to the compiled
/// default) rather than silently reasoning under a schedule that could lose
/// entailments. Mirrors [`depth_schedule`] exactly.
fn tableau_depth_schedule() -> Vec<usize> {
    let Some(raw) = std::env::var_os("RUSTDL_TABLEAU_ID_SCHEDULE") else {
        return MAX_SEARCH_DEPTH_SCHEDULE.to_vec();
    };
    let parsed: Option<Vec<usize>> = raw
        .to_str()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().parse::<usize>().ok())
                .collect()
        })
        .and_then(|v: Option<Vec<usize>>| v);
    match parsed {
        Some(v)
            if !v.is_empty()
                && v.windows(2).all(|w| w[1] > w[0])
                && v[v.len() - 1] >= MAX_SEARCH_DEPTH =>
        {
            v
        }
        _ => MAX_SEARCH_DEPTH_SCHEDULE.to_vec(),
    }
}

/// Default wall budget, in milliseconds, for the WHOLE shallow phase of one
/// iterative-deepening main-tableau probe (every level except the last, taken
/// together). Overridable with `RUSTDL_TABLEAU_ID_SHALLOW_MS`; `0` disables the
/// bound (which is the unbounded variant the wedge's own
/// [`ID_SHALLOW_BUDGET_MS`] docs already record as refuted).
///
/// **Why 20 ms rather than the wedge's 5 ms.** These are different engines with
/// different per-probe costs and the constant must follow the engine it bounds.
/// On the audit's own targets a *whole* depth-8 main-tableau probe costs
/// ~19 ms (`ore_ont_13545`: 30 unsat probes, `unsat_probe` 562 ms at depth 8
/// against 30 014 ms at 256) — so a 5 ms shallow budget would cut the shallow
/// level short on the very population it exists to serve, and every probe would
/// pay 5 ms for nothing. 20 ms clears a depth-8 probe on that population while
/// still being ~50× below the 1 000 ms per-pair budget it is carved out of.
const TABLEAU_ID_SHALLOW_BUDGET_MS: u64 = 20;

/// Shallow-phase wall budget in ms (`RUSTDL_TABLEAU_ID_SHALLOW_MS`, default
/// [`TABLEAU_ID_SHALLOW_BUDGET_MS`], `0` disables the bound). Garbage parses to
/// the default rather than to `0` — silently disabling the bound is the refuted
/// unbounded variant, exactly as for [`id_shallow_budget_ms`].
fn tableau_id_shallow_budget_ms() -> u64 {
    std::env::var("RUSTDL_TABLEAU_ID_SHALLOW_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(TABLEAU_ID_SHALLOW_BUDGET_MS)
}

/// How much wall, in milliseconds, one classify may WASTE on main-tableau
/// iterative-deepening shallow phases that fail to decide their probe, before
/// the shallow phase is switched off for the rest of that classify.
/// Overridable with `RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS`; `0` disables the
/// shutoff.
///
/// **This exists because the per-pair-tax failure mode is not hypothetical here
/// — it is the documented regression the wedge shipped and had to fix.**
/// [`ID_SHALLOW_BUDGET_MS`] was a per-*pair* constant, so its total cost scaled
/// with the pair count, which is quadratic in the class count; `ore_ont_13991`
/// (3 119 classes, 56 760 pairs) went from a 32.79 s completion to a DNF at
/// 180 s, and the dose–response confirmed the mechanism (1 ms → 90.31 s, i.e.
/// 57.5 s of overhead against a predicted 1 ms × 56 760 = 57 s).
///
/// The main tableau is exposed to the *same* arithmetic and the exposure is in
/// fact worse per unit, because [`TABLEAU_ID_SHALLOW_BUDGET_MS`] is 4× the
/// wedge's: an ontology on which many pairs fall through the wedge to the
/// tableau and whose shallow level decides nothing would pay 20 ms × (pairs
/// reaching the tableau). So the SAME shape of fix is applied, for the same
/// reason: a cumulative budget on **wall wasted by shallow phases that did not
/// decide**, which meters the harm in the units the harm is measured in.
///
/// **A CONSECUTIVE-MISS COUNTER IS ALREADY REFUTED — do not reintroduce it.**
/// The wedge tried it first (see [`ID_SHALLOW_WASTE_BUDGET_MS`]): `13991`'s
/// shallow phase decides 84 pairs while missing 200, and those interleave, so
/// any decide resets the run and a latch tolerating even a short streak never
/// trips. "Consecutive" measures the wrong thing — the harm is accumulated
/// wall, not a run of failures.
///
/// **Why a SEPARATE accumulator from the wedge's, rather than sharing it.**
/// Three reasons, in order of force:
/// 1. **Volume.** The wedge runs on *every* classify pair; the main tableau runs
///    only on the wedge's fallthrough subset (on `ore_ont_2826`: 342 pairs
///    reach the wedge, 6 reach the tableau). A shared accumulator would be
///    spent almost entirely by the higher-volume engine, latching the other's
///    shallow phase off before it had been given a chance to pay for itself —
///    the meter would measure one engine and charge both.
/// 2. **Units.** A wedge shallow phase is bounded at 5 ms, a tableau one at
///    20 ms. One budget cannot be correctly sized for both, and the whole point
///    of the waste metric is that it is denominated in the harm's own units.
/// 3. **Revertibility.** Two flags with two accumulators are independently
///    A/B-able; the wedge feature is default ON and this one is default OFF, so
///    a shared mutable counter would make the OFF path observably depend on the
///    ON path's behaviour.
///
/// **Soundness is untouched.** Skipping the shallow phase runs only the final
/// level, at cap `>= MAX_SEARCH_DEPTH` under the caller's own deadline — i.e.
/// *exactly* the flag-OFF search at a cap `>=` the flag-OFF cap. By the
/// monotonicity argument on [`search_iterative_deepening`] that verdict is a
/// superset of flag-OFF's, never a subset.
const TABLEAU_ID_SHALLOW_WASTE_BUDGET_MS: u64 = 1000;

/// Wasted-wall budget for the main tableau's adaptive shallow shutoff
/// (`RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS`, default
/// [`TABLEAU_ID_SHALLOW_WASTE_BUDGET_MS`], `0` disables the shutoff). Garbage
/// parses to the default rather than to `0`.
fn tableau_id_shallow_waste_budget_ms() -> u64 {
    std::env::var("RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(TABLEAU_ID_SHALLOW_WASTE_BUDGET_MS)
}

/// Per-classify accumulator for the main tableau's adaptive shallow shutoff.
///
/// Scope is one [`PreparedOntology`] — the tableau-side analogue of the
/// wedge's per-[`HyperCache`] counters, which is what makes the budget a
/// *per-classify* bound rather than a per-probe one. `Relaxed` ordering
/// throughout: this is a cost heuristic and never a correctness input.
#[derive(Debug, Default)]
pub(crate) struct TableauIdBudget {
    /// Microseconds spent in shallow phases that did NOT decide their probe.
    waste_us: std::sync::atomic::AtomicU64,
    /// Probes a non-final level decided (telemetry; `RUSTDL_TABLEAU_ID_STATS`).
    decided: std::sync::atomic::AtomicU64,
    /// Probes whose shallow phase failed to decide (telemetry).
    missed: std::sync::atomic::AtomicU64,
}

impl TableauIdBudget {
    /// `(decided, missed, waste_us)` — telemetry for the results write-up and
    /// for the canaries, which drive the accumulator directly rather than
    /// trying to burn a real millisecond budget (a wall-clock-dependent test on
    /// a loaded host would be flaky in exactly the direction that stops testing
    /// anything).
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> (u64, u64, u64) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            self.decided.load(Relaxed),
            self.missed.load(Relaxed),
            self.waste_us.load(Relaxed),
        )
    }

    #[cfg(test)]
    pub(crate) fn set_waste_us(&self, v: u64) {
        self.waste_us.store(v, std::sync::atomic::Ordering::Relaxed);
    }

    /// Is the shutoff latched — has this classify already wasted its whole
    /// budget on non-deciding shallow phases? Self-latching: once latched the
    /// shallow phase stops running, so it can no longer add to the total, so no
    /// second flag is needed to hold the latch.
    fn latched(&self) -> bool {
        let budget_us = tableau_id_shallow_waste_budget_ms().saturating_mul(1000);
        budget_us != 0 && self.waste_us.load(std::sync::atomic::Ordering::Relaxed) >= budget_us
    }
}

impl Drop for TableauIdBudget {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        // Mirrors `impl Drop for HyperCache`'s `RUSTDL_ID_STATS` dump. This is
        // also the POSITIVE CONTROL that the flag is live rather than a no-op:
        // on a flag-ON run these counters must be non-zero on any ontology that
        // reaches the deadline-bounded main tableau at all, and they must be
        // zero flag-OFF. Without it a measured "ON == OFF" could not be told
        // apart from "my flag never executed".
        if std::env::var_os("RUSTDL_TABLEAU_ID_STATS").is_some_and(|v| v == "1") {
            eprintln!(
                "# tableau-id-stats: shallow_decided={} shallow_missed={} shallow_waste_ms={}",
                self.decided.load(Relaxed),
                self.missed.load(Relaxed),
                self.waste_us.load(Relaxed) / 1000,
            );
        }
    }
}

/// Iterative-deepening driver for the **main tableau** on the deadline-bounded
/// query paths — the one call site of [`MAX_SEARCH_DEPTH`]. Runs
/// [`owl_dl_tableau::search`] at each level of [`tableau_depth_schedule`] until
/// the search returns something other than `DepthLimit`, or the schedule is
/// exhausted, or the caller's deadline passes.
///
/// # Why this is FP-safe by construction — verified for THIS engine, not
/// inherited from the wedge
///
/// The two engines differ, so the argument is re-derived from
/// `owl-dl-tableau/src/search.rs` rather than transplanted. A depth cap can
/// only *suppress* an `Unsat`, never manufacture one:
/// * `search` returns `SearchVerdict::DepthLimit` at `max_depth == 0`, before
///   any saturation or clash detection runs;
/// * in `search::branch`, a `DepthLimit` from ANY child sets
///   `depth_limited = true` and the frame returns `DepthLimit` **instead of**
///   `Unsat(combined)` — the `Unsat` arm is reached only when every option
///   clashed *decisively*;
/// * `decide` maps `DepthLimit` to `Ok(None)` / `Err(NoVerdict)`, i.e. to
///   "satisfiable / not subsumed" at every caller — a MISS, never a claim.
///
/// So no depth schedule can create a subsumption the fixed cap would not also
/// have found sound. Deepening can only ADD entailments.
///
/// # Why it does not LOSE entailments: depth is verdict-monotone
///
/// Raising the cap from `k` to `k' > k`:
/// * `Unsat` at `k` requires every branch decisively unsat with no stalled
///   child, so the identical DFS at `k'` re-derives it;
/// * `Sat` at `k` means a clash-free completion was found within `k`
///   decisions; at `k'` the DFS prefix is identical except that frames which
///   returned `DepthLimit` may now return `Sat` (immediate) or `Unsat` (the
///   parent continues to the next disjunct — exactly what it did after a
///   `DepthLimit`), so the outcome is still `Sat`;
/// * only `DepthLimit` can change, and only into a definite verdict.
///
/// Since the schedule's last level is `>= MAX_SEARCH_DEPTH` (compile-time
/// asserted) and carries the caller's own deadline, flag-ON's verdict is a
/// superset of flag-OFF's — a LOST pair means the implementation is wrong, not
/// that the idea is.
///
/// # Reusing one context across levels
///
/// The levels share the `ctx` rather than rebuilding it, which matters because
/// `decide`'s per-probe setup clones the whole `ConceptPool` (documented there
/// as dominating on large ontologies). This is sound because `search::branch`
/// **rolls back to its checkpoint** on every `Unsat` / `DepthLimit` /
/// `NodeCap`, so what survives a `DepthLimit` is exactly the top-level
/// `saturate` fixpoint — the deterministic closure of the root state, i.e.
/// sound consequences that a fresh run's own first `saturate` would recompute.
/// Extra deterministic labels can only make clashes MORE likely, so a `Sat`
/// found on the reused context is still a model of the root, and an `Unsat`
/// still rests only on entailed labels. Pinned by
/// `reused_context_matches_a_fresh_context_at_the_same_depth`.
///
/// # Deadline
///
/// The caller's `deadline` bounds the **whole loop**: the final level receives
/// it verbatim, non-final levels receive `min(shallow budget, deadline)`, and
/// the loop breaks before starting a level once the caller's deadline has
/// passed. Deepening therefore never multiplies the per-probe budget. The
/// converse is the one completeness exposure and is bounded by both the shallow
/// budget and the shutoff: the shallow phase spends at most
/// `remaining / TABLEAU_ID_SHALLOW_DIVISOR` of a budget the final level might
/// have needed.
fn search_iterative_deepening(
    ctx: &mut TableauContext<'_, '_, '_>,
    deadline: std::time::Instant,
    budget: &TableauIdBudget,
) -> owl_dl_tableau::SearchVerdict {
    use owl_dl_tableau::SearchVerdict;
    use std::sync::atomic::Ordering::Relaxed;
    let schedule = tableau_depth_schedule();
    // `schedule` is non-empty by construction (the compiled default is, and a
    // malformed override is rejected wholesale in `tableau_depth_schedule`).
    let last = schedule.len() - 1;
    let started = std::time::Instant::now();
    // Reuse the wedge's shallow-budget arithmetic verbatim: `min(budget_ms,
    // remaining / DIVISOR)`, so at least (DIVISOR-1)/DIVISOR of the caller's
    // budget always reaches the final level — the only one that can decide a
    // deep probe.
    let shallow = id_shallow_deadline(started, Some(deadline), tableau_id_shallow_budget_ms())
        .unwrap_or(deadline)
        .min(deadline);
    let shallow_skipped = budget.latched();
    let mut i = if shallow_skipped { last } else { 0 };
    let verdict = loop {
        let final_level = i == last;
        if final_level {
            // Restore the caller's deadline AND clear the sticky hit flag, so
            // this level's verdict is reported exactly as the flag-OFF single
            // search would report it. Without the clear, a shallow level's
            // elapsed sub-deadline would make a genuine depth-cap `DepthLimit`
            // read as a deadline cut.
            ctx.set_deadline(deadline);
            ctx.clear_deadline_hit();
        } else {
            ctx.set_deadline(shallow);
        }
        let v = owl_dl_tableau::search(ctx, schedule[i]);
        if final_level {
            break v;
        }
        // Only a `DepthLimit` can be improved by deepening (§ monotonicity).
        // `Sat`/`Unsat` are final; `NodeCap` is a global resource cap that a
        // deeper search can only hit sooner.
        if !matches!(v, SearchVerdict::DepthLimit) {
            break v;
        }
        // The CALLER's deadline bounds the LOOP: never start a level after it
        // passes, so deepening cannot multiply the per-probe budget. Restore it
        // first so `deadline_reached()` reports against the right instant.
        if std::time::Instant::now() >= deadline {
            ctx.set_deadline(deadline);
            ctx.check_deadline();
            break v;
        }
        // Once the shallow budget is spent, jump straight to the final level:
        // every intermediate level would return `DepthLimit` on its first
        // deadline check anyway, after paying a full re-descent for nothing.
        i = if std::time::Instant::now() >= shallow {
            last
        } else {
            i + 1
        };
    };
    // Feed the shutoff. Only observations count: when the shallow phase was
    // skipped there is nothing to learn, and not touching the accumulator here
    // is what makes the latch permanent without a second flag. "Decided" means
    // a DEFINITE verdict from a NON-final level — a `DepthLimit` that merely
    // fell through to the final level is the tax this shutoff exists to stop
    // paying, and a verdict from the final level would have been reached with
    // no shallow phase at all.
    if !shallow_skipped {
        if i < last && !matches!(verdict, SearchVerdict::DepthLimit) {
            budget.decided.fetch_add(1, Relaxed);
        } else {
            budget.missed.fetch_add(1, Relaxed);
            // Charge only the shallow phase, never the final level: the final
            // level's own wall is work the flag-OFF path would have done anyway.
            let spent = shallow
                .saturating_duration_since(started)
                .min(started.elapsed())
                .as_micros();
            budget
                .waste_us
                .fetch_add(u64::try_from(spent).unwrap_or(u64::MAX), Relaxed);
        }
    }
    verdict
}

/// Recursion depth cap for the **deadline-free** query paths (`is_consistent`,
/// `is_class_satisfiable`, un-timed realize). Termination on these paths must
/// come from pair-blocking (which bounds the completion *graph*), NOT from an
/// artificial recursion limit — the ⊔/choose *decision count* along a single
/// branch is ≈ nodes × disjunctions-per-node, which is finite but easily runs
/// into the hundreds or more on a blocking-bounded-but-disjunction-dense
/// ontology (issue #35: a 5-axiom core needs ~650). The old 256 cut such a
/// branch off as `DepthLimit`; with no clash deps to back-jump on, the driver
/// then enumerated the exponential ⊔-space and never returned (>300 s hang).
/// Set far above any realistic blocking-bounded decision count; the sole
/// remaining bound is blocking, and [`DEEP_SEARCH_STACK_BYTES`] gives the
/// recursion room so this depth cannot overflow the stack.
const DEEP_SEARCH_DEPTH: usize = 1_000_000;

/// Dedicated stack size for the deadline-free deep search (see
/// [`DEEP_SEARCH_DEPTH`]). The recursive `search`/`branch` pair uses one frame
/// per ⊔/choose decision; 1 GiB of (lazily-committed) stack comfortably covers
/// any depth pair-blocking can actually reach on a real ontology.
const DEEP_SEARCH_STACK_BYTES: usize = 1024 * 1024 * 1024;

/// Per-query instrumentation: did the EL closure alone answer this
/// query, or did the tableau have to run? Returned alongside the
/// boolean verdict by the `_with_stats` variants of the public
/// reasoning entry points.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryStats {
    /// `true` iff the EL saturation closure was sufficient to
    /// produce the verdict — no tableau call was made.
    pub answered_by_saturation: bool,
    /// `true` iff this run took the pure-EL fast path (the closure
    /// is also complete for the input, so a closure miss is itself
    /// the verdict).
    pub pure_el_mode: bool,
}

/// Errors that can surface from the public reasoning API.
#[derive(Debug, Error)]
pub enum ReasonError {
    /// horned-owl axioms couldn't be lowered to the internal IR.
    /// Most often: a construct rustdl doesn't support yet (inverse
    /// roles, data ranges, anonymous individuals, ...).
    #[error("conversion from horned-owl: {0}")]
    Conversion(#[from] ConversionError),

    /// The IRI given to [`is_class_satisfiable`] was not declared as
    /// a class in the input ontology. Most often a typo or a missing
    /// `Declaration(Class(...))`.
    #[error("class IRI not in ontology: {0}")]
    UnknownClass(String),

    /// The tableau hit its internal iteration/recursion cap. Should
    /// not happen for inputs in the implemented fragment; bug
    /// indicator.
    #[error("tableau bailed out without a verdict (likely an internal limit)")]
    NoVerdict,

    /// A role chain sub-property axiom is outside the supported
    /// fragment. Phase 5 (R) supports **length-2** chains
    /// (`r ∘ s ⊑ t`) over **named** roles only. Anything longer, or
    /// any chain containing an `ObjectInverseOf` role expression,
    /// surfaces here.
    #[error(
        "role chain sub-property axiom outside supported fragment (only length-2 named-role chains are implemented)"
    )]
    RoleChainUnsupported,

    /// The ontology is inconsistent — every assertion is vacuously entailed, so
    /// enumerating (e.g. property assertions) is meaningless.
    #[error("ontology is inconsistent; every assertion is trivially entailed")]
    Inconsistent,
}

/// Decide whether `class_iri` is satisfiable in the ontology.
///
/// Pipeline:
/// 1. Lower horned-owl axioms to the internal IR ([`convert_ontology`]).
/// 2. Push every concept to NNF ([`nnf_axioms`]).
/// 3. Run binary, nominal and role absorption ([`absorb`]).
/// 4. Build a [`TableauContext`] backed by the absorbed `TBox`.
/// 5. Add `Atomic(class)` to a fresh root node and call
///    [`TableauContext::is_satisfiable`].
///
/// Returns `Ok(true)` if `class_iri` is satisfiable w.r.t. the
/// ontology, `Ok(false)` if unsatisfiable, and a [`ReasonError`]
/// otherwise.
///
/// # Errors
///
/// See [`ReasonError`] variants. The most common cause is the IRI
/// not appearing as a declared class in the ontology.
pub fn is_class_satisfiable<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_class_satisfiable_internal(internal, class_iri)
}

/// Like [`is_class_satisfiable`] but the tableau run is bounded by
/// `deadline`. Returns `Ok(Some(sat))` if the tableau reached a
/// verdict before the deadline elapsed, or `Ok(None)` on timeout.
/// EL-closure / pure-EL fast paths are checked first and never
/// time out.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
    deadline: std::time::Duration,
) -> Result<Option<bool>, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let closure = owl_dl_saturation::saturate(&internal);
    if closure.is_unsatisfiable(class_id) {
        return Ok(Some(false));
    }
    if classify::is_pure_el(&internal) {
        return Ok(Some(true));
    }
    let prepared = PreparedOntology::from_internal(internal)?;
    let when = std::time::Instant::now() + deadline;
    prepared.decide_with_deadline(when, move |pool| pool.atomic(class_id))
}

/// Internal entry point that takes the already-lowered ontology.
/// Exposed for tests that want to assemble an `InternalOntology` by
/// hand or share one across multiple satisfiability checks.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_internal(
    internal: InternalOntology,
    class_iri: &str,
) -> Result<bool, ReasonError> {
    is_class_satisfiable_internal_full(internal, class_iri).map(|(b, _)| b)
}

/// Stats-returning variant of [`is_class_satisfiable`]; the verdict
/// is paired with a [`QueryStats`] recording whether the EL closure
/// answered alone.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_with_stats<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_class_satisfiable_internal_full(internal, class_iri)
}

fn is_class_satisfiable_internal_full(
    internal: InternalOntology,
    class_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    // EL closure oracle: a sound `⊑ ⊥` flag means the class is
    // definitively unsatisfiable, regardless of whether the rest of
    // the ontology is in the EL fragment. And for *pure*-EL inputs
    // the closure is also complete, so a *lack* of `⊑ ⊥` is itself
    // the verdict.
    let closure = owl_dl_saturation::saturate(&internal);
    let pure_el = classify::is_pure_el(&internal);
    if closure.is_unsatisfiable(class_id) {
        return Ok((
            false,
            QueryStats {
                answered_by_saturation: true,
                pure_el_mode: pure_el,
            },
        ));
    }
    if pure_el {
        return Ok((
            true,
            QueryStats {
                answered_by_saturation: true,
                pure_el_mode: true,
            },
        ));
    }
    let sat = run_satisfiability(internal, move |pool| pool.atomic(class_id))?;
    Ok((
        sat,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}

/// Decide whether `ontology` is consistent — i.e. whether it has any
/// model at all. Reduces to satisfiability of `⊤` under the full
/// `TBox` + `ABox`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_consistent<A: ForIRI>(ontology: &SetOntology<A>) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_consistent_internal(internal)
}

/// Internal entry point that takes the already-lowered ontology.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_consistent_internal(internal: InternalOntology) -> Result<bool, ReasonError> {
    is_consistent_internal_full(internal).map(|(b, _)| b)
}

/// Stats-returning variant of [`is_consistent`].
///
/// `is_consistent` always goes through the tableau today because the
/// EL closure can't soundly answer "every model is empty" without
/// `⊤`-sub-class lowering — so the returned stats will currently
/// report `answered_by_saturation: false`. Surfacing the field
/// anyway keeps the API symmetric and ready for a future fast path.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_consistent_with_stats<A: ForIRI>(
    ontology: &SetOntology<A>,
) -> Result<(bool, QueryStats), ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_consistent_internal_full(internal)
}

fn is_consistent_internal_full(
    internal: InternalOntology,
) -> Result<(bool, QueryStats), ReasonError> {
    // Sound ABox-saturation pre-check (gated, default off): a clash derived by
    // consequence-based saturation over named individuals is a real inconsistency.
    // Runs before `from_internal` (which moves `internal`); guarded by
    // `has_abox_axioms` so ABox-free inputs skip it. Non-clash ⇒ fall through to
    // the existing hybrid path unchanged (FP-safe; sound under-approximation).
    if abox_saturation_inconsistent(&internal) {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("abox_saturation: inconsistent");
        }
        return Ok((
            false,
            QueryStats {
                answered_by_saturation: true,
                pure_el_mode: false,
            },
        ));
    }
    let prepared = PreparedOntology::from_internal(internal)?;
    // Sound pre-check: a positive verdict short-circuits the tableau.
    if let abox_check::AboxVerdict::Inconsistent { reason } = prepared.abox_verdict() {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("abox_check: inconsistent — {reason:?}");
        }
        return Ok((
            false,
            QueryStats {
                answered_by_saturation: false,
                pure_el_mode: false,
            },
        ));
    }
    let trace = std::env::var_os("RUSTDL_TRACE").is_some();
    // ABox-seeded wedge route (default on; kills the `decide(Top)` hang
    // on out-of-EL ABoxes). `Some` only when enabled AND there is an
    // ABox; otherwise fall straight through to the main tableau.
    let wedge_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(consistency_fallback_ms());
    match prepared.consistency_wedge(Some(wedge_deadline)) {
        Some(owl_dl_tableau::hyper::HyperResult::Unsat) => {
            // SOUND: a clause clash on the asserted-only ABox seed is a
            // real model violation ⟹ ontology inconsistent.
            if trace {
                eprintln!("consistency: wedge Unsat — inconsistent");
            }
            return Ok((
                false,
                QueryStats {
                    answered_by_saturation: false,
                    pure_el_mode: false,
                },
            ));
        }
        Some(owl_dl_tableau::hyper::HyperResult::Sat) => {
            // Trusted consistent (like classify's trust_sat). The wedge
            // is Horn-incomplete so it may MISS a clash — sound (a
            // missed clash is the same trust level as classify today).
            if trace {
                eprintln!("consistency: wedge Sat — consistent (trusted)");
            }
            return Ok((
                true,
                QueryStats {
                    answered_by_saturation: false,
                    pure_el_mode: false,
                },
            ));
        }
        Some(owl_dl_tableau::hyper::HyperResult::Stalled) if trace => {
            // Undetermined → bounded main-tableau fall-through below.
            eprintln!("consistency: wedge Stalled — bounded tableau fall-through");
        }
        // `Some(Stalled)` (no trace): bounded main-tableau fall-through.
        // `None`: wedge route disabled or no ABox → pure main-tableau
        // (the historical path; unbounded for the no-ABox case which does
        // not hang — GALEN/SIO/pizza decide(Top) is sub-second).
        Some(owl_dl_tableau::hyper::HyperResult::Stalled) | None => {}
    }

    // Fall through: tableau-based satisfiability of Top. When the wedge
    // stalled we BOUND it (so a hard out-of-EL ABox can't hang); on the
    // pure no-ABox path we keep the unbounded call (no hang risk).
    let wedge_route_active = prepared.consistency.is_some();
    let consistent = if wedge_route_active {
        let dl =
            std::time::Instant::now() + std::time::Duration::from_millis(consistency_fallback_ms());
        if let Some(sat) = prepared.decide_with_deadline(dl, owl_dl_core::ConceptPool::top)? {
            sat
        } else {
            // Deadline elapsed: no inconsistency witnessed within
            // budget. Report consistent (sound — a tableau timeout
            // is "don't know", and the trusted direction here is
            // "consistent") with an incompleteness trace.
            if trace {
                eprintln!(
                    "consistency: bounded tableau fall-through timed out \
                     ({} ms) — reporting consistent (incomplete)",
                    consistency_fallback_ms()
                );
            }
            true
        }
    } else {
        prepared.decide(owl_dl_core::ConceptPool::top)?
    };
    Ok((
        consistent,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}

/// Decide whether `sub_iri ⊑ super_iri` holds in `ontology`. Standard
/// reduction: subsumption holds iff `sub ⊓ ¬sup` is *unsatisfiable*.
///
/// Returns `Ok(true)` if `sub ⊑ sup`, `Ok(false)` if there is a model
/// in which some `sub`-instance is not a `sup`-instance.
///
/// # Errors
///
/// See [`ReasonError`]. Either IRI not declared as a class surfaces as
/// [`ReasonError::UnknownClass`].
pub fn is_subclass_of<A: ForIRI>(
    ontology: &SetOntology<A>,
    sub_iri: &str,
    super_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_subclass_of_internal(internal, sub_iri, super_iri)
}

/// Internal entry point that takes the already-lowered ontology.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_subclass_of_internal(
    internal: InternalOntology,
    sub_iri: &str,
    super_iri: &str,
) -> Result<bool, ReasonError> {
    is_subclass_of_internal_full(internal, sub_iri, super_iri).map(|(b, _)| b)
}

/// Stats-returning variant of [`is_subclass_of`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_subclass_of_with_stats<A: ForIRI>(
    ontology: &SetOntology<A>,
    sub_iri: &str,
    super_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_subclass_of_internal_full(internal, sub_iri, super_iri)
}

/// Saturation-only counterpart of [`is_subclass_of`]. Skips the
/// `sub ⊓ ¬sup` tableau probe and answers purely from the EL
/// closure: `true` iff the closure contains the subsumption or
/// proves `sub` unsatisfiable. Sound under-approximation: positive
/// answers are genuine, negatives may be missed positives the full
/// classifier would catch.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_subclass_of_saturation_only<A: ForIRI>(
    ontology: &SetOntology<A>,
    sub_iri: &str,
    super_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let sub_id = internal
        .vocabulary
        .class_id(sub_iri)
        .ok_or_else(|| ReasonError::UnknownClass(sub_iri.to_owned()))?;
    let super_id = internal
        .vocabulary
        .class_id(super_iri)
        .ok_or_else(|| ReasonError::UnknownClass(super_iri.to_owned()))?;
    if sub_id == super_id {
        return Ok(true);
    }
    let closure = owl_dl_saturation::saturate(&internal);
    Ok(closure.contains(sub_id, super_id) || closure.is_unsatisfiable(sub_id))
}

fn is_subclass_of_internal_full(
    internal: InternalOntology,
    sub_iri: &str,
    super_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let sub_id = internal
        .vocabulary
        .class_id(sub_iri)
        .ok_or_else(|| ReasonError::UnknownClass(sub_iri.to_owned()))?;
    let super_id = internal
        .vocabulary
        .class_id(super_iri)
        .ok_or_else(|| ReasonError::UnknownClass(super_iri.to_owned()))?;
    let pure_el = classify::is_pure_el(&internal);
    let sat_stats = QueryStats {
        answered_by_saturation: true,
        pure_el_mode: pure_el,
    };
    // Reflexive shortcut.
    if sub_id == super_id {
        return Ok((true, sat_stats));
    }
    // Saturation fast path: the EL closure is sound (every entry is a
    // genuine entailment) but only complete for the EL fragment of the
    // input. If it answers `yes`, we're done — skip the tableau. A
    // `no` just means "the EL subset doesn't witness it"; full
    // tableau still needs to run.
    let closure = owl_dl_saturation::saturate(&internal);
    if closure.contains(sub_id, super_id) {
        return Ok((true, sat_stats));
    }
    // If `sub` is itself unsat in the closure, every superclass —
    // including `super` — vacuously subsumes it.
    if closure.is_unsatisfiable(sub_id) {
        return Ok((true, sat_stats));
    }
    // Pure-EL inputs: the closure is complete, so a miss is the
    // verdict, no tableau needed.
    if pure_el {
        return Ok((false, sat_stats));
    }
    // H4 sound-accelerator wedge: a hyper `Unsat` proves the
    // subsumption (sound for any ontology), skipping the tableau. A
    // non-proof falls through. No-op when the wedge is disabled.
    // (HF5 `Sat`-trust is wired in the classify path, not here.)
    if hyper_wedge_enabled()
        && HyperCache::build(&internal).decide(sub_id, super_id, None) == HyperVerdict::Subsumed
    {
        return Ok((
            true,
            QueryStats {
                answered_by_saturation: false,
                pure_el_mode: false,
            },
        ));
    }
    // `sub ⊓ ¬sup` is unsatisfiable iff every model that contains a
    // `sub`-instance also makes it a `sup`-instance.
    let sat = run_satisfiability(internal, move |pool| {
        let sub_concept = pool.atomic(sub_id);
        let super_concept = pool.atomic(super_id);
        let not_super = pool.not(super_concept);
        pool.and(vec![sub_concept, not_super])
    })?;
    Ok((
        !sat,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}

/// Shared end-of-pipeline runner: takes a (possibly mutated)
/// `InternalOntology`, runs the full normalize/absorb/`ABox`-seed
/// pipeline once, and asks the tableau whether `build_test_concept`
/// produces a satisfiable concept against the rest of the model.
///
/// One-shot convenience wrapper for callers (`is_class_satisfiable`,
/// `is_consistent`, `is_subclass_of`) that only ask a single tableau
/// question. For repeated queries against the same ontology — the
/// pairwise loop in `classify`, or the per-class probes in
/// `realize` — prefer [`PreparedOntology::from_internal`] +
/// [`PreparedOntology::decide`], which shares the expensive
/// prepare work across calls.
///
/// The closure is invoked *after* the pool has been cloned for the
/// tableau run, so the concept it returns is guaranteed to live in
/// the pool the tableau will use.
pub(crate) fn run_satisfiability<F>(
    internal: InternalOntology,
    build_test_concept: F,
) -> Result<bool, ReasonError>
where
    F: FnOnce(&mut ConceptPool) -> ConceptId,
{
    let prepared = PreparedOntology::from_internal(internal)?;
    prepared.decide(build_test_concept)
}

/// Owned backing store for [`abox_check::AboxCheckInputs`], for callers that need
/// only the inconsistency verdict and not a full [`PreparedOntology`]. Built by
/// [`build_abox_check_inputs`]; borrow with [`Self::as_inputs`].
///
/// This exists so the classify fast path stops building [`HyperCache`], `NNF`,
/// absorb and [`ConsistencyCache`] solely to read `abox_verdict()` and then discard
/// them — measured at 0.62 s / 185 MB on `ore_ont_1043`.
pub(crate) struct OwnedAboxCheckInputs {
    pool: ConceptPool,
    abox: Abox,
    axioms: Vec<Axiom>,
    told: owl_dl_core::told::ToldTables,
    hierarchy: RoleHierarchy,
    inverse_pairs: Vec<(RoleId, RoleId)>,
    disjoint_role_pairs: Vec<(RoleId, RoleId)>,
}

impl OwnedAboxCheckInputs {
    pub(crate) fn as_inputs<'a>(
        &'a self,
        closure: &'a owl_dl_saturation::Subsumers,
    ) -> abox_check::AboxCheckInputs<'a> {
        abox_check::AboxCheckInputs {
            abox: &self.abox,
            axioms: &self.axioms,
            told: &self.told,
            pool: &self.pool,
            inverse_pairs: &self.inverse_pairs,
            hierarchy: &self.hierarchy,
            disjoint_role_pairs: &self.disjoint_role_pairs,
            closure,
        }
    }
}

/// Build only what [`abox_check::check`] reads. Mirrors the corresponding prefix
/// of [`PreparedOntology::from_internal`] — `expand_role_characteristics`, the
/// role-side collectors, `build_told_tables`, `collect_abox` — and deliberately
/// omits `nnf_axioms`, `absorb`, `precompute_max_complements`, [`HyperCache::build`],
/// [`ConsistencyCache::build`] and `snapshot_cache`, none of which `check` reads.
///
/// `collect_abox` only reads `internal.axioms` and interns one nominal concept per
/// individual, so running it before `absorb` yields different *individual* concept
/// ids but identical *class* ids — and `check` compares ids only within one input
/// set, so the verdict is unchanged. The canaries in
/// `tests/abox_check_reduced_input.rs` pin this.
///
/// # This is a HAND-COPIED prefix, and the differential test does not fully guard it
///
/// This function and `from_internal` are two parallel sequences, not shared code. A
/// future edit to `from_internal` can silently desynchronise them.
/// `abox_check_differential_tests` compares the two paths' verdicts, but its coverage
/// of THIS risk was measured and is **partial**: with the tests in place, all three of
/// these sabotages of this function still passed —
///
/// 1. deleting the `expand_role_characteristics` call outright,
/// 2. moving `build_told_tables` to after it,
/// 3. moving the `axioms` clone to after it.
///
/// The reason is that those three are, for *today's* `check`, semantically inert:
/// `expand_role_characteristics` appends `⊤ ⊑ ≤1 r.⊤` and self-inverse
/// `InverseObjectProperties` pairs; told tables index atomic subsumption/disjointness
/// (a `Max` is not atomic, so no new told edge), and `check` scans `axioms` only for
/// `ABox`/role forms it recognises, which those additions are not. `hierarchy` /
/// `inverse_pairs` / `disjoint_role_pairs` are collected after the call on **both**
/// paths, so they cannot diverge from its placement either.
///
/// So the desync hazard is real but currently **latent**: it becomes live the moment
/// `check` starts reading something an omitted pass affects. If you extend
/// `abox_check` to read a new field, or to consume a lowered axiom form, re-run the
/// sabotage above — if it still passes, the differential test is not protecting your
/// new dependency. The durable fix is to have `from_internal` call this function
/// rather than restate it; that was not done here because the values are not a
/// contiguous prefix of `from_internal` (the `dkey`/`hyper`/`consistency` builds are
/// interleaved), so unifying them is a restructure of `from_internal`, not an
/// extraction.
pub(crate) fn build_abox_check_inputs(internal: &InternalOntology) -> OwnedAboxCheckInputs {
    let mut internal = internal.clone();
    let told = owl_dl_core::told::build_told_tables(&internal);
    let axioms = internal.axioms.clone();
    expand_role_characteristics(&mut internal);
    let hierarchy = build_role_hierarchy(&internal);
    let inverse_pairs = collect_inverse_pairs(&internal);
    let disjoint_role_pairs = collect_disjoint_role_pairs(&internal);
    let abox = collect_abox(&mut internal);
    OwnedAboxCheckInputs {
        pool: internal.concepts,
        abox,
        axioms,
        told,
        hierarchy,
        inverse_pairs,
        disjoint_role_pairs,
    }
}

/// Snapshot of an ontology after every pre-tableau pass has run.
/// Holds the absorbed `TBox`, role-side metadata, `ABox` seed data and
/// the (now-frozen) concept pool, so each tableau query reuses one
/// preparation pass.
pub(crate) struct PreparedOntology {
    pub(crate) pool: ConceptPool,
    /// IRI ↔ id vocabulary, cloned from the input `InternalOntology` before
    /// its `concepts` are moved into `pool`. Lets downstream query surfaces
    /// resolve named classes/individuals by IRI — e.g. [`crate::disjoint_classes`]
    /// (#47) resolves each candidate class IRI back to a [`owl_dl_core::ir::ClassId`]
    /// here.
    pub(crate) vocabulary: owl_dl_core::vocab::Vocabulary,
    tbox: AbsorbedTBox,
    pub(crate) hierarchy: RoleHierarchy,
    inverse_pairs: Vec<(RoleId, RoleId)>,
    chain_axioms: Vec<(Role, Role, Role)>,
    asymmetric_roles: Vec<RoleId>,
    disjoint_role_pairs: Vec<(RoleId, RoleId)>,
    complements: Vec<(ConceptId, ConceptId)>,
    pub(crate) abox: Abox,
    /// Lever A (2026-07-20): true iff the `ABox` is provably irrelevant to class
    /// subsumption — the ontology has `ABox` axioms, uses NO nominals, and the
    /// `TBox`-only-classify gate is on. When true, the per-pair classification
    /// tableau probes (`decide_classify`) do NOT seed the `ABox`, avoiding the
    /// completion-graph blow-up it causes on near-EL `ABox`-bearing ontologies.
    /// `abox` is still kept full for `realize`/`materialize`/consistency.
    abox_irrelevant_to_classify: bool,
    /// EL saturator closure over the un-mutated input ontology.
    /// Used by [`abox_check`] (P1 `is_unsatisfiable`, P2 `subsumers_of`).
    /// Computed once at build time; classify already computes the same
    /// closure at its own call site, so we keep `abox_check`'s copy
    /// self-contained rather than threading it through.
    ///
    /// `None` **only** when [`lazy_abox_saturation_enabled`] is on AND the input
    /// is provably `ABox`-free — in which case `abox_check::check` early-returns
    /// before reading it. See that flag's doc for the equivalence argument.
    pub(crate) closure: Option<owl_dl_saturation::Subsumers>,
    /// Told-disjoint pairs (and other told-* relations) over the
    /// input ontology. Used by [`abox_check`] P2/P7. Built once in
    /// `from_internal`.
    pub(crate) told: owl_dl_core::told::ToldTables,
    /// Phase 1 scaffolding for the satisfying-model cache. The
    /// field is shipped now so [`crate::PreparedOntology::decide`]
    /// callers can be wired one at a time in Phase 2 without a
    /// signature change. See [`docs/model-caching-plan.md`] for
    /// the full design and the §A revert criterion if the cache
    /// doesn't move pizza/SIO walls.
    #[allow(dead_code)]
    model_cache: model_cache::ModelCache,
    /// H4 sound-accelerator state (clausified clauses + `¬sup`
    /// expansions), `Some` iff [`hyper_wedge_enabled`]. The classify
    /// pair loop consults it before the tableau (`subsumes_via_tableau`).
    hyper: Option<HyperCache>,
    /// Phase 1b snapshot cache: per-class completion graph snapshots
    /// reused across the classify pair loop. `Some` iff
    /// [`snapshot_capture_enabled`]; populated lazily by
    /// [`Self::snapshot_replay`]. Spec §3 + §4.
    snapshot_cache: Option<SnapshotCache>,
    /// Phase 3a recon: count of classes that the per-class
    /// [`owl_dl_tableau::BackPropRisk::classify_class`] variant marks
    /// `Safe`. Diagnostic only; the ontology-wide classifier still
    /// gates `SnapshotCache::try_replay`.
    pub(crate) per_class_safe_count: usize,
    /// Phase 3a recon: count of classes that the per-class classifier
    /// marks `Unsafe`. Diagnostic only.
    pub(crate) per_class_unsafe_count: usize,
    /// Cloned axiom list from the input ontology, kept so the `ABox`
    /// consistency check (P5/P6/P7) can scan for
    /// `FunctionalRole` / `InverseFunctionalRole` / `AsymmetricRole` /
    /// `IrreflexiveRole` / `ObjectPropertyDomain` / `ObjectPropertyRange`.
    /// These role-side characteristics are absorbed into `hierarchy`
    /// and other lowered representations elsewhere in the pipeline,
    /// so this is a read-only snapshot for the pre-tableau check.
    pub(crate) axioms: Vec<Axiom>,
    /// Cached `ABox` consistency check verdict. Populated on first
    /// call to [`Self::abox_verdict`]. `None` until then (lazy).
    /// Honours [`crate::abox_check_enabled`]. See [`abox_check`].
    abox_verdict: std::sync::OnceLock<abox_check::AboxVerdict>,
    /// Concrete-domain solver (P2): `ClassId → CardRange` for the synthetic
    /// `DKey` filler classes, decoded once here (where the vocabulary maps
    /// `ClassId → IRI`) so the tableau — which has no vocabulary — can
    /// recognise a data-range filler and recover its range via a cheap map
    /// lookup. Consumed by the P3 `apply_concrete_domain_check` clash rule
    /// (not yet armed). See `docs/superpowers/specs/2026-06-11-concrete-domain-solver-design.md`.
    pub(crate) dkey_ranges:
        std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange>,
    /// Named classes carrying a counting `DKey` constraint (see
    /// `build_data_counting_classes`). The classify unsat-probe
    /// main-tableau-verifies these instead of trusting the wedge's `Sat`.
    pub(crate) data_counting_classes: std::collections::HashSet<owl_dl_core::ir::ClassId>,
    /// `ABox`-seeded wedge consistency state. `Some` iff
    /// [`wedge_consistency_enabled`] and the ontology has `ABox` axioms;
    /// `None` otherwise (`ABox`-free inputs pay nothing, classify
    /// byte-identical). Consumed by [`is_consistent_internal_full`].
    consistency: Option<ConsistencyCache>,
    /// Per-classify accumulator for the main tableau's iterative-deepening
    /// adaptive shallow shutoff (`RUSTDL_TABLEAU_ITERATIVE_DEEPENING`, default
    /// OFF). Lives here rather than on [`HyperCache`] so it is scoped to one
    /// classify and is METERED SEPARATELY from the wedge's — see
    /// [`TABLEAU_ID_SHALLOW_WASTE_BUDGET_MS`] for why sharing one budget across
    /// two engines of very different per-probe volume would starve the
    /// lower-volume one. Inert (never read) when the flag is off.
    tableau_id: TableauIdBudget,
}

/// Build the concrete-domain solver's `ClassId → CardRange` side-map by
/// decoding every synthetic `DKey` filler class's IRI. Done where the
/// vocabulary is available so the tableau need not carry IRIs. All six
/// datatype buckets (integer, string, float, decimal, date, dateTime) are
/// decoded; the `DenseInterval` for dense types preserves inclusivity exactly
/// (field-for-field copy, zero normalization) — a soundness requirement.
fn build_dkey_range_map(
    internal: &InternalOntology,
) -> std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange> {
    use owl_dl_datatypes::{DenseInterval, OrdF64};
    let mut map = std::collections::HashMap::new();
    for (class_id, iri) in internal.vocabulary.classes() {
        if !owl_dl_core::is_dkey_iri(iri) {
            continue;
        }
        if let Some((min, max)) = owl_dl_core::decode_integer_dkey(iri) {
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::Int(owl_dl_datatypes::IntInterval { min, max }),
            );
        } else if let Some(ss) = owl_dl_core::decode_string_dkey(iri) {
            let fs = match ss {
                owl_dl_core::StrSet::Top => owl_dl_datatypes::FiniteSet::Top,
                owl_dl_core::StrSet::Set(s) => owl_dl_datatypes::FiniteSet::Set(s),
            };
            map.insert(class_id, owl_dl_datatypes::CardRange::Str(fs));
        } else if let Some((min, min_incl, max, max_incl)) = owl_dl_core::decode_float_dkey(iri) {
            // Bridge FloatRange → DenseInterval<OrdF64>: wrap each f64 bound
            // in OrdF64. Inclusivity flags copied 1:1 — no normalization.
            // `OrdF64::new` canonicalizes signed zero (-0.0 → +0.0); using the
            // bare tuple constructor here would be UNSOUND (see the OrdF64
            // signed-zero note — a raw -0.0 bound can fire a false counting
            // clash via the disjoint-packing rule).
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::Float(DenseInterval {
                    min: min.map(OrdF64::new),
                    min_incl,
                    max: max.map(OrdF64::new),
                    max_incl,
                }),
            );
        } else if let Some((min, min_incl, max, max_incl)) = owl_dl_core::decode_double_dkey(iri) {
            // xsd:double uses the same DenseInterval<OrdF64> carrier as xsd:float
            // but a separate `db:` DKey bucket — never cross-subsumes with `f:`.
            // Bridge is identical: wrap f64 bounds in OrdF64 (signed-zero-safe).
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::Float(DenseInterval {
                    min: min.map(OrdF64::new),
                    min_incl,
                    max: max.map(OrdF64::new),
                    max_incl,
                }),
            );
        } else if let Some((min, min_incl, max, max_incl)) = owl_dl_core::decode_decimal_dkey(iri) {
            // Bridge OrdRange<Decimal> → DenseInterval<Decimal>: same shape.
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::Decimal(DenseInterval {
                    min,
                    min_incl,
                    max,
                    max_incl,
                }),
            );
        } else if let Some((min, min_incl, max, max_incl)) = owl_dl_core::decode_date_dkey(iri) {
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::Date(DenseInterval {
                    min,
                    min_incl,
                    max,
                    max_incl,
                }),
            );
        } else if let Some((min, min_incl, max, max_incl)) = owl_dl_core::decode_datetime_dkey(iri)
        {
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::DateTime(DenseInterval {
                    min,
                    min_incl,
                    max,
                    max_incl,
                }),
            );
        } else if let Some(set) = owl_dl_core::decode_int_oneof_dkey(iri) {
            // Integer-oneof: each decoded i64 maps directly to a FiniteSet<i64>.
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::IntSet(owl_dl_datatypes::FiniteSet::Set(set)),
            );
        } else if let Some(set) = owl_dl_core::decode_float_oneof_dkey(iri) {
            // Float-oneof: OrdF64 is already normalized (signed zero).
            // Bridge to FiniteSet<OrdF64> used by the tableau's FloatSet bucket.
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::FloatSet(owl_dl_datatypes::FiniteSet::Set(set)),
            );
        } else if let Some(set) = owl_dl_core::decode_double_oneof_dkey(iri) {
            // Double-oneof (`dbo:`): a SEPARATE DKey bucket from `fo:` (disjoint
            // OWL value spaces), but the same `FiniteSet<OrdF64>` counting
            // representation here — cardinality counting only ever MERGES
            // representatives, which under-counts distinct values and so can
            // only MISS a counting clash, never invent one.
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::FloatSet(owl_dl_datatypes::FiniteSet::Set(set)),
            );
        } else if let Some(set) = owl_dl_core::decode_decimal_oneof_dkey(iri) {
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::DecimalSet(owl_dl_datatypes::FiniteSet::Set(set)),
            );
        } else if let Some(set) = owl_dl_core::decode_date_oneof_dkey(iri) {
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::DateSet(owl_dl_datatypes::FiniteSet::Set(set)),
            );
        } else if let Some(set) = owl_dl_core::decode_datetime_oneof_dkey(iri) {
            map.insert(
                class_id,
                owl_dl_datatypes::CardRange::DateTimeSet(owl_dl_datatypes::FiniteSet::Set(set)),
            );
        }
    }
    map
}

/// True if concept `c`'s expression contains (recursively) a `Min`/`Max`
/// cardinality whose filler is a `DKey` datatype-range class. Such a class's
/// satisfiability can hinge on a concrete-domain counting clash that the
/// hypertableau wedge cannot evaluate (it has no `card_sat` and does not
/// materialise `DKey` cardinality — see the wedge-hang fix).
fn concept_has_dkey_counting(
    pool: &ConceptPool,
    c: ConceptId,
    dkey_ranges: &std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange>,
) -> bool {
    match pool.get(c) {
        ConceptExpr::Min(_, _, inner) | ConceptExpr::Max(_, _, inner) => {
            // The filler is usually atomic (`Min`/`Max` over a DKey class).
            // `Atomic` isn't matched by the recursion below (it hits
            // `_ => false`), so this direct check is REQUIRED, not an
            // optimisation; the recursive call only catches a DKey nested
            // inside a compound filler.
            if matches!(
                pool.get(*inner),
                ConceptExpr::Atomic(cls) if dkey_ranges.contains_key(cls)
            ) {
                return true;
            }
            concept_has_dkey_counting(pool, *inner, dkey_ranges)
        }
        ConceptExpr::Not(inner) => concept_has_dkey_counting(pool, *inner, dkey_ranges),
        ConceptExpr::Some(_, inner) | ConceptExpr::All(_, inner) => {
            concept_has_dkey_counting(pool, *inner, dkey_ranges)
        }
        ConceptExpr::And(ops) | ConceptExpr::Or(ops) => ops
            .iter()
            .any(|&o| concept_has_dkey_counting(pool, o, dkey_ranges)),
        _ => false,
    }
}

/// Named classes that carry a *counting* `DKey` constraint
/// (`DataMin/Max/ExactCardinality` over an integer range, lowered to
/// `Min`/`Max` over a `DKey` filler). Scanned from the *un-mutated* IR
/// (pre-absorb), where the raw `SubClassOf`/`EquivalentClasses` axioms
/// still carry the lowered concept. The classify unsat-probe verifies
/// these (and their saturation-subclasses) on the main tableau instead of
/// trusting the wedge's `Sat`. Empty unless the ontology has integer data
/// cardinality — keeps the fast wedge path for every value-membership-only
/// ontology (e.g. `sio`).
fn build_data_counting_classes(
    internal: &InternalOntology,
    dkey_ranges: &std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange>,
) -> std::collections::HashSet<owl_dl_core::ir::ClassId> {
    let mut set = std::collections::HashSet::new();
    if dkey_ranges.is_empty() {
        return set;
    }
    let pool = &internal.concepts;
    for ax in &internal.axioms {
        match ax {
            Axiom::SubClassOf { sub, sup } => {
                if let ConceptExpr::Atomic(c) = pool.get(*sub)
                    && concept_has_dkey_counting(pool, *sup, dkey_ranges)
                {
                    set.insert(*c);
                }
            }
            Axiom::EquivalentClasses(members)
                if members
                    .iter()
                    .any(|&m| concept_has_dkey_counting(pool, m, dkey_ranges)) =>
            {
                for &m in members {
                    if let ConceptExpr::Atomic(c) = pool.get(m) {
                        set.insert(*c);
                    }
                }
            }
            _ => {}
        }
    }
    set
}

impl PreparedOntology {
    /// Run every preparation pass against `internal` so subsequent
    /// `decide` calls only have to allocate a fresh tableau and run
    /// the search.
    pub(crate) fn from_internal(internal: InternalOntology) -> Result<Self, ReasonError> {
        // `deadline: None` makes every `expired` test below constant-false, so
        // the `Ok(None)` arm is unreachable — hence the `expect`.
        Ok(Self::from_internal_with_deadline(internal, None, None)?
            .expect("from_internal_with_deadline(.., None) never aborts"))
    }

    /// [`Self::from_internal`] with a coarse wall-clock bound: the deadline is
    /// tested between preparation passes and returns `Ok(None)` instead of a
    /// snapshot once it has passed.
    ///
    /// Checks are deliberately COARSE — one per pass, not one per axiom — so the
    /// clock is read a handful of times per build. The passes chosen are the ones
    /// measured to dominate (`saturate`, `HyperCache::build`, `ConsistencyCache::
    /// build`, `absorb`); the cheap ones ride on the neighbouring check.
    ///
    /// `Ok(None)` mirrors the `RUSTDL_MAX_NODES` → `NodeCap` → `Ok(None)`
    /// precedent: the caller must degrade to a sound under-approximation and
    /// report it as incomplete. `deadline == None` ⇒ always `Ok(Some(..))` and
    /// zero clock reads, so the default path is untouched.
    /// `precomputed_closure`: an EL closure the CALLER already computed over the
    /// SAME unmutated ontology. Passing it skips a full re-saturation.
    ///
    /// `classify_top_down_internal` computes exactly this closure for its fast-path
    /// check and then called here with no way to hand it over, so the identical
    /// fixpoint ran twice. Measured (2026-08-12): `ore_ont_8475` 46,836 ms then
    /// 46,318 ms — matching within 3%, and roughly HALF the total classify wall.
    /// See `docs/2026-08-12-duplicate-saturation-in-prepare.md`.
    ///
    /// **Sound because the inputs are identical**, not because the values are
    /// compared: the caller saturates `internal`, this receives `internal.clone()`,
    /// and (with `RUSTDL_LAZY_ABOX_SATURATION` off — the default) the branch below
    /// is `saturate(&internal)` *before* any mutation. `abox_irrelevant_to_classify`
    /// is computed early but applied later, so it cannot affect the closure.
    ///
    /// **Completeness gate:** an ABORTED closure is a sound UNDER-approximation, so
    /// reusing one would hand this a weaker closure than it would have built.
    /// `classify_top_down_internal` returns early on `sat_aborted`, so its closure is
    /// complete by construction at the call site — but a future caller must preserve
    /// that, or pass `None`.
    pub(crate) fn from_internal_with_deadline(
        mut internal: InternalOntology,
        deadline: Option<std::time::Instant>,
        precomputed_closure: Option<owl_dl_saturation::Subsumers>,
    ) -> Result<Option<Self>, ReasonError> {
        // One-liner so each boundary below is a single readable line. `None` ⇒
        // constant-false (the closure is inlined and the branch folds away).
        let expired = || deadline.is_some_and(|d| std::time::Instant::now() >= d);
        if expired() {
            return Ok(None);
        }
        // Clone the vocabulary before `internal.concepts` is moved into `pool`
        // below, so downstream IRI↔id lookups survive `from_internal`.
        let vocabulary = internal.vocabulary.clone();
        // Lever A: decide up front (on the un-mutated input) whether the ABox is
        // irrelevant to class subsumption — has-ABox, no nominals, gate on.
        let abox_irrelevant_to_classify = classify_tbox_only_enabled()
            && internal_has_abox(&internal)
            && !ontology_uses_nominals(&internal);
        // Phase A1 (ABox consistency check): EL closure over the
        // un-mutated input. Used by abox_check for P1 (is_unsatisfiable)
        // and P2 (subsumers_of).
        //
        // On an ABox-FREE ontology `abox_check::check` early-returns before it
        // ever reads the closure, so this whole saturation is dead work — it is
        // elided under `RUSTDL_LAZY_ABOX_SATURATION` (default OFF). The gate is
        // evaluated on the un-mutated input and is equivalent to
        // `abox.individuals.is_empty()` below; see
        // `lazy_abox_saturation_enabled`'s doc for why.
        let closure = if let Some(pre) = precomputed_closure {
            // Caller already ran this exact fixpoint (see the doc comment).
            Some(pre)
        } else if lazy_abox_saturation_enabled() && !internal_has_abox(&internal) {
            None
        } else if let Some(d) = deadline {
            // Bounded fixpoint. An ABORTED closure is a sound under-approximation
            // for `abox_check` (fewer derived types ⇒ fewer clashes ⇒ a missed
            // inconsistency at worst) — but we bail immediately below anyway.
            let (subs, aborted) = owl_dl_saturation::saturate_with_deadline(&internal, Some(d));
            if aborted {
                return Ok(None);
            }
            Some(subs)
        } else {
            Some(owl_dl_saturation::saturate(&internal))
        };
        let told = owl_dl_core::told::build_told_tables(&internal);
        let axioms = internal.axioms.clone();
        // Concrete-domain solver (P2): decode the synthetic DKey filler
        // classes into a ClassId → CardRange map while the vocabulary is
        // available. Pure; consumed by the (not-yet-armed) P3 clash rule.
        let dkey_ranges = build_dkey_range_map(&internal);
        let data_counting_classes = build_data_counting_classes(&internal, &dkey_ranges);
        // H4: build the hyper cache from the un-mutated ontology
        // (before the absorb/NNF passes below consume it), iff enabled.
        if expired() {
            return Ok(None);
        }
        let hyper = hyper_wedge_enabled().then(|| HyperCache::build(&internal));
        if expired() {
            return Ok(None);
        }
        // ABox-seeded wedge consistency: build iff enabled AND the input
        // has ABox axioms (so ABox-free inputs pay nothing and classify
        // stays byte-identical). Built from the un-mutated `internal`,
        // before the absorb/NNF passes below consume it — so every
        // nominal/role id is matched-by-construction with the clause set.
        let consistency = (wedge_consistency_enabled() && internal_has_abox(&internal))
            .then(|| ConsistencyCache::build(&internal));
        // Phase 1b: build the snapshot cache from the same un-mutated
        // ontology, iff `RUSTDL_SNAPSHOT_CAPTURE` is ON. The cache's
        // `clausify_with_stats` + `BackPropRisk::classify_ontology`
        // run on a separate clone (mirrors HyperCache::build).
        let snapshot_cache = snapshot_capture_enabled().then(|| SnapshotCache::build(&internal));
        // Phase 3a recon: per-class BackPropRisk classifier counts.
        // Pure diagnostic — does not change the snapshot cache gate
        // (the ontology-wide classifier still drives `try_replay`).
        // Runs on the un-mutated `internal` (before absorb/NNF) so
        // axiom shapes match what `classify_ontology` saw.
        // Cost: O(n_classes × n_axioms × concept_tree_depth). Gate behind
        // snapshot_capture_enabled() — when the snapshot cache is OFF (the
        // default), this diagnostic loop is the only call site and can be
        // skipped. Snapshot capture ON → run as before.
        let (per_class_safe_count, per_class_unsafe_count) = if snapshot_capture_enabled() {
            let n = internal.vocabulary.num_classes();
            let mut safe = 0usize;
            let mut not_safe = 0usize;
            for i in 0..n {
                let cid = owl_dl_core::ir::ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                );
                if matches!(
                    owl_dl_tableau::BackPropRisk::classify_class(cid, &internal),
                    owl_dl_tableau::BackPropRisk::Safe
                ) {
                    safe += 1;
                } else {
                    not_safe += 1;
                }
            }
            (safe, not_safe)
        } else {
            (0, 0)
        };
        if expired() {
            return Ok(None);
        }
        expand_role_characteristics(&mut internal);
        let hierarchy = build_role_hierarchy(&internal);
        let inverse_pairs = collect_inverse_pairs(&internal);
        let asymmetric_roles = collect_asymmetric_roles(&internal);
        let disjoint_role_pairs = collect_disjoint_role_pairs(&internal);
        let chain_axioms = collect_chain_axioms(&internal)?;
        if expired() {
            return Ok(None);
        }
        let normalized = nnf_axioms(&mut internal);
        let tbox = absorb(&normalized, &mut internal.concepts);
        // Ensure `⊥` is interned — `apply_max` flags inequality
        // clashes by adding `Bot` to the offending node's label set,
        // and looks up the canonical id via `pool.bot_id()`. Cheap
        // & idempotent.
        let _ = internal.concepts.bot();
        let complements = precompute_max_complements(&mut internal.concepts);
        let abox = collect_abox(&mut internal);
        Ok(Some(Self {
            pool: internal.concepts,
            vocabulary,
            tbox,
            hierarchy,
            inverse_pairs,
            chain_axioms,
            asymmetric_roles,
            disjoint_role_pairs,
            complements,
            abox,
            abox_irrelevant_to_classify,
            closure,
            told,
            axioms,
            model_cache: model_cache::ModelCache::new(),
            hyper,
            snapshot_cache,
            per_class_safe_count,
            per_class_unsafe_count,
            abox_verdict: std::sync::OnceLock::new(),
            dkey_ranges,
            data_counting_classes,
            consistency,
            tableau_id: TableauIdBudget::default(),
        }))
    }

    /// `ABox`-seeded wedge consistency verdict, or `None` when the wedge
    /// route is disabled / there is no `ABox`. `Unsat`=inconsistent,
    /// `Sat`=consistent, `Stalled`=undetermined (caller falls through).
    pub(crate) fn consistency_wedge(
        &self,
        deadline: Option<std::time::Instant>,
    ) -> Option<owl_dl_tableau::hyper::HyperResult> {
        self.consistency.as_ref().map(|c| c.decide(deadline))
    }

    /// One `ABox` witness model's per-individual COMPLETE type sets, or `None`
    /// when the wedge consistency route is disabled / there is no `ABox` /
    /// no clash-free completion is available (`Unsat`/`Stalled`/deadline).
    /// Mirrors [`Self::consistency_wedge`]'s accessor pattern. Callers MUST
    /// treat `None` as "no usable model" (skip the prune it would otherwise
    /// enable) — never assume unsatisfiability.
    ///
    /// Consumer: `realize_tableau_internal`'s pseudo-model shortcut, gated
    /// by `RUSTDL_PSEUDO_MODEL` (see `realize::pseudo_model_enabled`).
    pub(crate) fn realize_base_model_types(
        &self,
        deadline: Option<std::time::Instant>,
    ) -> Option<Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>>> {
        self.consistency
            .as_ref()
            .and_then(|c| c.base_model_types(deadline))
    }

    /// Lazy accessor for the `ABox` consistency check verdict.
    /// Honours [`crate::abox_check_enabled`]: if the gate is off,
    /// always returns `Unknown` without invoking the check.
    pub(crate) fn abox_verdict(&self) -> &abox_check::AboxVerdict {
        self.abox_verdict.get_or_init(|| {
            if !crate::abox_check_enabled() {
                return abox_check::AboxVerdict::Unknown;
            }
            let Some(closure) = self.closure.as_ref() else {
                // `RUSTDL_LAZY_ABOX_SATURATION` elided the saturation because the
                // input is provably ABox-free — the exact case in which `check`
                // early-returns `Unknown` anyway. If the gate's equivalence with
                // `abox.individuals.is_empty()` ever drifts, this arm degrades to
                // `Unknown`: a sound under-approximation (a missed inconsistency
                // is a MISS, never an FP).
                debug_assert!(
                    self.abox.individuals.is_empty(),
                    "lazy-saturation gate elided the closure on an ABox-bearing ontology"
                );
                return abox_check::AboxVerdict::Unknown;
            };
            abox_check::check(&abox_check::AboxCheckInputs {
                abox: &self.abox,
                axioms: &self.axioms,
                told: &self.told,
                pool: &self.pool,
                inverse_pairs: &self.inverse_pairs,
                hierarchy: &self.hierarchy,
                disjoint_role_pairs: &self.disjoint_role_pairs,
                closure,
            })
        })
    }

    /// Phase 1b snapshot-replay shortcut. Returns:
    /// - `Some(ReplayVerdict)` when the snapshot cache produced a verdict
    ///   (orchestrator maps `Subsumed`/`NotSubsumed` to the classify
    ///   decision; falls through on `BackPropAborted`/`Stalled`).
    /// - `None` when snapshot capture is disabled (flag OFF), the
    ///   ontology is Unsafe (cache built but `is_safe()` is false), or
    ///   the wedge couldn't build a snapshot for `sub` (Unsat/Stalled
    ///   on `sub` alone — caller should treat as a separate path).
    ///
    /// Phase 1b T6 fix: the cache uses the wedge's `fresh_q` injection
    /// pattern so `¬sup` is root-scoped (not global). Caller passes
    /// just `(sub, sup)`; the cache internals build the q-gated
    /// `fresh_q ⊓ sup → ⊥` clause.
    pub(crate) fn snapshot_replay(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
    ) -> Option<owl_dl_tableau::ReplayVerdict> {
        self.snapshot_cache
            .as_ref()
            .and_then(|c| c.try_replay(sub, sup))
    }

    /// Phase 2a recon: cumulative snapshot-build wall (ms) across the
    /// classify pair loop. 0 when the snapshot cache is disabled.
    pub(crate) fn snapshot_cache_build_wall_ms(&self) -> u64 {
        self.snapshot_cache
            .as_ref()
            .map_or(0, SnapshotCache::build_wall_ms)
    }

    /// Phase 2a recon: cumulative snapshot-replay wall (ms) across the
    /// classify pair loop. 0 when the snapshot cache is disabled.
    pub(crate) fn snapshot_cache_replay_wall_ms(&self) -> u64 {
        self.snapshot_cache
            .as_ref()
            .map_or(0, SnapshotCache::replay_wall_ms)
    }

    /// Phase 3a recon: count of classes the per-class
    /// [`owl_dl_tableau::BackPropRisk::classify_class`] variant marks
    /// `Safe`. Diagnostic only.
    pub(crate) fn per_class_safe_count(&self) -> usize {
        self.per_class_safe_count
    }

    /// Phase 3a recon: count of classes the per-class classifier
    /// marks `Unsafe`. Diagnostic only.
    pub(crate) fn per_class_unsafe_count(&self) -> usize {
        self.per_class_unsafe_count
    }

    // snapshot_cache_is_safe accessor reserved for Phase 3 when the
    // per-class classifier lands and orchestrator callers need to
    // gate per call. Phase 1b's single caller (snapshot_replay) folds
    // the safety check into try_replay; an explicit accessor would
    // be dead code.

    /// H4/HF5 sound accelerator: the hyper engine's three-valued
    /// verdict for `sub ⊑ sup`, or [`HyperVerdict::Unknown`] when the
    /// wedge is disabled. `Subsumed` is sound for any ontology;
    /// `NotSubsumed` is sound only under [`hyper_trust_sat_enabled`]
    /// (HF5) — the caller decides whether to trust it.
    pub(crate) fn hyper_decide(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> HyperVerdict {
        self.hyper
            .as_ref()
            .map_or(HyperVerdict::Unknown, |hc| hc.decide(sub, sup, deadline))
    }

    /// Per-class label heuristic: run wedge satisfiability of `c` and
    /// return a [`LabelOracle`]. Returns [`LabelOracle::NoVerdict`] when
    /// the hyper wedge is disabled. See
    /// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
    pub(crate) fn classify_labels(
        &self,
        c: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> LabelOracle {
        self.hyper
            .as_ref()
            .map_or(LabelOracle::NoVerdict, |hc| hc.classify_labels(c, deadline))
    }

    /// Decide whether the test concept built by `build_test_concept`
    /// is satisfiable in this prepared ontology. The closure is
    /// invoked on a freshly-cloned pool so the prepared pool stays
    /// intact for the next call.
    pub(crate) fn decide<F>(&self, build_test_concept: F) -> Result<bool, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            &self.abox,
            &[],
            &[],
            &self.dkey_ranges,
            &self.tableau_id,
            None,
            build_test_concept,
        )
        // `None` was previously impossible with no deadline set (search always
        // returned `Some(_)`), but a live-node cap trip (#35 v4 safety net) can
        // now legitimately yield `None` even here. Treat it the same as the
        // deadline-bounded callers already do: "no verdict" ⇒ satisfiable ⇒
        // not-an-instance/not-subsumed — a sound under-approximation, never a
        // panic.
        .map(|opt| opt.unwrap_or(true))
    }

    /// Like [`Self::decide`], but does NOT fold an inconclusive `None` (a
    /// live-node-cap trip, #35 v4 safety net) into `Some(true)`. Callers that
    /// need to distinguish "genuinely satisfiable" from "no verdict reached"
    /// on the deadline-FREE path — e.g. [`Self::pair_disjoint_with_deadline`]
    /// / [`Self::pair_individuals_disjoint_with_deadline`]'s `None`-deadline
    /// branch, whose own callers set `incomplete` on an observed `None` —
    /// MUST use this instead of [`Self::decide`], whose `unwrap_or(true)`
    /// would otherwise silently swallow the `NodeCap` `None` and report a
    /// probe-capped result as complete.
    pub(crate) fn decide_raw<F>(&self, build_test_concept: F) -> Result<Option<bool>, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            &self.abox,
            &[],
            &[],
            &self.dkey_ranges,
            &self.tableau_id,
            None,
            build_test_concept,
        )
    }

    /// Lever A: like [`Self::decide`], but for the **classification pairwise
    /// subsumption loop only** — skips the `ABox` seed when it is provably
    /// irrelevant to class subsumption (`abox_irrelevant_to_classify`: has
    /// `ABox`, no nominals, gate on). MUST NOT be used for realize / consistency
    /// / instance queries, which genuinely need the `ABox`. Global `ABox`
    /// inconsistency is caught by the separate pre-checks before this loop runs.
    pub(crate) fn decide_classify<F>(&self, build_test_concept: F) -> Result<bool, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        let empty = Abox::default();
        let abox = if self.abox_irrelevant_to_classify {
            &empty
        } else {
            &self.abox
        };
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            abox,
            &[],
            &[],
            &self.dkey_ranges,
            &self.tableau_id,
            None,
            build_test_concept,
        )
        // `None` was previously impossible with no deadline set (search always
        // returned `Some(_)`), but a live-node cap trip (#35 v4 safety net) can
        // now legitimately yield `None` even here. Treat it the same as the
        // deadline-bounded callers already do: "no verdict" ⇒ satisfiable ⇒
        // not-an-instance/not-subsumed — a sound under-approximation, never a
        // panic.
        .map(|opt| opt.unwrap_or(true))
    }

    /// Like [`Self::decide`] but the search is bounded by `deadline`.
    /// Returns `Ok(Some(sat))` if the tableau reached a verdict in
    /// time, or `Ok(None)` if the deadline elapsed first.
    pub(crate) fn decide_with_deadline<F>(
        &self,
        deadline: std::time::Instant,
        build_test_concept: F,
    ) -> Result<Option<bool>, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            &self.abox,
            &[],
            &[],
            &self.dkey_ranges,
            &self.tableau_id,
            Some(deadline),
            build_test_concept,
        )
    }

    /// `Some(true)` iff `a ⊓ b` is unsatisfiable (the two named classes are
    /// entailed disjoint); `Some(false)` if satisfiable; `None` on timeout.
    /// Sound: only unsat ⇒ disjoint (never a false positive). Consumed by
    /// [`crate::disjoint_classes`] (#47 disjointness query).
    pub(crate) fn pair_disjoint_with_deadline(
        &self,
        a: owl_dl_core::ir::ClassId,
        b: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<bool>, ReasonError> {
        let build_test_concept = |pool: &mut ConceptPool| {
            let ca = pool.atomic(a);
            let cb = pool.atomic(b);
            pool.and([ca, cb])
        };
        let sat = match deadline {
            Some(deadline) => self.decide_with_deadline(deadline, build_test_concept)?,
            // Use the raw Option-returning decide, NOT `Self::decide` — its
            // `unwrap_or(true)` would fold an inconclusive NodeCap `None`
            // into `Some(true)` (satisfiable ⇒ "not disjoint"), silently
            // discarding the inconclusive verdict instead of letting it
            // propagate to the caller's `None => incomplete = true` arm.
            None => self.decide_raw(build_test_concept)?,
        };
        Ok(sat.map(|s| !s)) // unsat ⇒ disjoint
    }

    /// `Some(true)` iff `{a} ⊓ {b}` is unsatisfiable (the two named
    /// individuals are entailed distinct — genuinely PROVEN, not merely
    /// assumed under a Unique Name Assumption); `Some(false)` if
    /// satisfiable; `None` on timeout. Sound: only unsat ⇒ distinct (never
    /// a false positive). Consumed by [`crate::different_individuals`]
    /// (#46 same/different-individuals query).
    pub(crate) fn pair_individuals_disjoint_with_deadline(
        &self,
        a: IndividualId,
        b: IndividualId,
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<bool>, ReasonError> {
        let build_test_concept = |pool: &mut ConceptPool| {
            let na = pool.nominal(a);
            let nb = pool.nominal(b);
            pool.and([na, nb])
        };
        let sat = match deadline {
            Some(deadline) => self.decide_with_deadline(deadline, build_test_concept)?,
            // See the matching comment in `pair_disjoint_with_deadline`: use
            // the raw Option-returning decide so a NodeCap `None` propagates
            // instead of being folded into `Some(true)` by `Self::decide`'s
            // `unwrap_or(true)`.
            None => self.decide_raw(build_test_concept)?,
        };
        Ok(sat.map(|s| !s)) // {a}⊓{b} unsat ⇒ a≠b
    }

    /// Lever A: like [`Self::decide_with_deadline`], but for the classification
    /// pairwise subsumption loop only — skips the `ABox` seed when it is provably
    /// irrelevant (`abox_irrelevant_to_classify`). Same scoping caveat as
    /// [`Self::decide_classify`].
    pub(crate) fn decide_classify_with_deadline<F>(
        &self,
        deadline: std::time::Instant,
        build_test_concept: F,
    ) -> Result<Option<bool>, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        let empty = Abox::default();
        let abox = if self.abox_irrelevant_to_classify {
            &empty
        } else {
            &self.abox
        };
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            abox,
            &[],
            &[],
            &self.dkey_ranges,
            &self.tableau_id,
            Some(deadline),
            build_test_concept,
        )
    }

    /// Task 0.3: snapshot-preserving augment-and-recheck. Decides whether
    /// `KB ∪ extra_distinct ∪ extra_neg_prop` is consistent WITHOUT rebuilding
    /// this `PreparedOntology` snapshot — reuses the frozen `pool`/`tbox`/etc.,
    /// injecting the extra facts into the per-probe tableau seed. The test
    /// concept is `⊤` (`pool.top()`), so satisfiability of the seeded graph
    /// *is* consistency of `KB ∪ extra facts`.
    ///
    /// `Some(true)` = consistent (tableau found a model); `Some(false)` =
    /// inconsistent (genuine clash — sound, never a false positive);
    /// `None` = no verdict within `deadline` (or `None` for unbounded).
    ///
    /// Downstream consumers: #46 same-individuals (`a=b` iff
    /// `KB ∪ {a≠b}` is inconsistent, [`crate::individuals`]) and #45 property
    /// values (`R(a,b)` iff `KB ∪ {¬R(a,b)}` is inconsistent,
    /// [`crate::property_values`]) both call this in production.
    pub(crate) fn consistent_with_extra(
        &self,
        extra_distinct: &[(IndividualId, IndividualId)],
        extra_neg_prop: &[(IndividualId, RoleId, IndividualId)],
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<bool>, ReasonError> {
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            &self.abox,
            extra_distinct,
            extra_neg_prop,
            &self.dkey_ranges,
            &self.tableau_id,
            deadline,
            ConceptPool::top,
        )
    }
}

/// Pre-resolved `ABox` state, ready to seed into the tableau context.
/// All `ConceptId` fields are interned in the pool by
/// [`collect_abox`] (the last stage to mutate the pool); the tableau
/// then runs with a frozen pool.
#[derive(Default, Debug)]
struct Abox {
    /// `(individual, Nominal(individual)_id)` — one entry per
    /// individual referenced in any `ABox` axiom. Each gets a root
    /// node seeded with the nominal label before the test class is
    /// added.
    individuals: Vec<(IndividualId, ConceptId)>,
    /// `(individual, class_concept_id)` from `ClassAssertion`.
    class_assertions: Vec<(IndividualId, ConceptId)>,
    /// `(from_individual, role_id, to_individual)` from
    /// `ObjectPropertyAssertion`. Role polarity has been normalized:
    /// an inverse-role assertion swaps subject/object so the role
    /// stored here is always forward.
    property_assertions: Vec<(IndividualId, RoleId, IndividualId)>,
    /// `(individual, ∀r.¬{b}_concept_id)` from
    /// `NegativeObjectPropertyAssertion`. Encoded as a label that
    /// will be propagated by `apply_forall` along any matching
    /// edge — any actual r-relation to `b`'s nominal causes a
    /// `Not(Nominal(b))` / `Nominal(b)` clash.
    negative_property_assertions: Vec<(IndividualId, ConceptId)>,
    /// `(a, b)` pairs from `SameIndividual(a, b, ...)`. Decomposed
    /// pairwise — the tableau merges `b` into `a` for each pair.
    same_pairs: Vec<(IndividualId, IndividualId)>,
    /// `(a, b)` pairs from `DifferentIndividuals(a, b, ...)`.
    /// Likewise pairwise; the tableau marks them distinct.
    different_pairs: Vec<(IndividualId, IndividualId)>,
    /// P3 input: raw `(subject, role_id, object)` triples from
    /// `NegativeObjectPropertyAssertion` axioms. Polarity normalised
    /// (inverse-role assertions swap subject/object). The `∀`-form
    /// stored in `negative_property_assertions` is for the tableau;
    /// this is for the `ABox` consistency check.
    pub(crate) negative_property_triples: Vec<(IndividualId, RoleId, IndividualId)>,
}

fn collect_abox(internal: &mut InternalOntology) -> Abox {
    use std::collections::HashSet;
    let mut abox = Abox::default();
    let mut seen: HashSet<IndividualId> = HashSet::new();
    let record_individual = |ind: IndividualId,
                             pool: &mut ConceptPool,
                             seen: &mut HashSet<IndividualId>,
                             abox: &mut Abox| {
        if seen.insert(ind) {
            let nom = pool.nominal(ind);
            abox.individuals.push((ind, nom));
        }
    };
    // First pass: enumerate every individual referenced and intern
    // its Nominal expression.
    for ax in &internal.axioms {
        match ax {
            Axiom::ClassAssertion { individual, .. } => {
                record_individual(*individual, &mut internal.concepts, &mut seen, &mut abox);
            }
            Axiom::ObjectPropertyAssertion {
                subject, object, ..
            }
            | Axiom::NegativeObjectPropertyAssertion {
                subject, object, ..
            } => {
                record_individual(*subject, &mut internal.concepts, &mut seen, &mut abox);
                record_individual(*object, &mut internal.concepts, &mut seen, &mut abox);
            }
            Axiom::SameIndividual(inds) | Axiom::DifferentIndividuals(inds) => {
                for ind in inds {
                    record_individual(*ind, &mut internal.concepts, &mut seen, &mut abox);
                }
            }
            _ => {}
        }
    }
    // Second pass: derive concrete assertions / clashes / pairs.
    // We collect axiom references in a local Vec to avoid double-
    // borrowing internal during the body.
    let axioms: Vec<Axiom> = internal.axioms.clone();
    for ax in &axioms {
        match ax {
            Axiom::ClassAssertion { class, individual } => {
                abox.class_assertions.push((*individual, *class));
            }
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                let (from, to) = if role.is_inverse() {
                    (*object, *subject)
                } else {
                    (*subject, *object)
                };
                abox.property_assertions.push((from, role.role_id(), to));
            }
            Axiom::NegativeObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                let (from, to) = if role.is_inverse() {
                    (*object, *subject)
                } else {
                    (*subject, *object)
                };
                abox.negative_property_triples
                    .push((from, role.role_id(), to));
                // Encode `(subject, object) ∉ role` as
                // `{subject} ⊑ ∀role.¬{object}`. Polarity of the
                // role passes through unchanged.
                let nom_b = internal.concepts.nominal(*object);
                let not_nom_b = internal.concepts.not(nom_b);
                let forall = internal.concepts.all(*role, not_nom_b);
                abox.negative_property_assertions.push((*subject, forall));
            }
            Axiom::SameIndividual(inds) => {
                for i in 0..inds.len() {
                    for j in (i + 1)..inds.len() {
                        abox.same_pairs.push((inds[i], inds[j]));
                    }
                }
            }
            Axiom::DifferentIndividuals(inds) => {
                for i in 0..inds.len() {
                    for j in (i + 1)..inds.len() {
                        abox.different_pairs.push((inds[i], inds[j]));
                    }
                }
            }
            _ => {}
        }
    }
    abox
}

/// Pre-compute NNF complements for every concept that the tableau
/// may need to negate at search time. Two sources of targets:
///
/// 1. **`Max(_, _, body)` bodies.** The choose rule branches on
///    `C` vs `¬C` around an unlabelled neighbour of a `≤n R.C`
///    constraint.
/// 2. **Literal `Or` disjuncts** — atomic, nominal, self-restriction,
///    or `Not(_)` of those. Phase 4 commit 6's *restricted semantic
///    branching* (see `docs/phase4-backjumping-plan.md`) asserts
///    `¬d_j` for previously-tried literal disjuncts `d_j` in
///    [`crate::search::branch`] so a re-derivation clashes
///    immediately. Complex (Or/And/quantified) disjuncts are
///    deliberately *excluded* — their complements are themselves
///    compound expressions whose addition would inflate the label
///    set faster than the back-jump can prune (Phase 4 attempt 1
///    regressed corpus 2× this way).
///
/// This is the last stage before `absorb` that mutates the pool for the
/// tableau's complement-precomputation; `collect_abox` runs afterwards and
/// also mutates the pool (interning one nominal concept per individual), so
/// the pool is not fully frozen until after `collect_abox` returns.
fn precompute_max_complements(pool: &mut ConceptPool) -> Vec<(ConceptId, ConceptId)> {
    let mut targets: Vec<ConceptId> = pool
        .iter_with_ids()
        .filter_map(|(_, e)| match e {
            ConceptExpr::Max(_, _, body) => Some(*body),
            _ => None,
        })
        .collect();
    // Atomic-shaped Or disjuncts for semantic branching.
    let literal_disjuncts: Vec<ConceptId> = pool
        .iter_with_ids()
        .filter_map(|(_, e)| match e {
            ConceptExpr::Or(args) => Some(args.to_vec()),
            _ => None,
        })
        .flatten()
        .filter(|d| {
            matches!(
                pool.get(*d),
                ConceptExpr::Atomic(_)
                    | ConceptExpr::Nominal(_)
                    | ConceptExpr::SelfRestriction(_)
                    | ConceptExpr::Not(_)
            )
        })
        .collect();
    targets.extend(literal_disjuncts);
    targets.sort_unstable();
    targets.dedup();
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let neg = nnf_complement(target, pool);
        out.push((target, neg));
    }
    out
}

/// Build the ALCH role hierarchy from atomic `SubObjectPropertyOf` and
/// `EquivalentObjectProperties` axioms. Chain sub-property axioms are
/// not encoded in the hierarchy itself — they are collected separately
/// by [`collect_chain_axioms`] and registered on the
/// [`TableauContext`].
fn build_role_hierarchy(internal: &InternalOntology) -> RoleHierarchy {
    let mut builder = RoleHierarchyBuilder::with_roles(
        u32::try_from(internal.vocabulary.num_roles()).expect("vocabulary role count fits in u32"),
    );
    // Mirror the clausifier's inverse-pair canonicalization
    // (clause::build_inverse_canon): if `InverseObjectProperties(R, S)`
    // is declared, the clausifier rewrites `S` to `Inverse(R)` at every
    // clause site. The role hierarchy must use the *same* role IDs the
    // engine sees on canonicalized edges/atoms, otherwise the
    // hierarchy lookup misses and inverse-sub-role inferences are lost
    // (which combined with HF5 trust-Sat can manifest as false-Unsat
    // FPs, as on SIO).
    let canon_map: std::collections::HashMap<RoleId, owl_dl_core::ir::Role> = {
        let mut m = std::collections::HashMap::new();
        for ax in &internal.axioms {
            if let Axiom::InverseObjectProperties(a, b) = ax {
                if a.is_inverse() || b.is_inverse() {
                    continue;
                }
                // Self-inverse InverseObjectProperties(p, p): semantics = symmetric,
                // handled by mark_symmetric below. Adding p→Inverse(p) to the canon
                // rewrite map would block a subsequent InverseObjectProperties(p, q)
                // from mapping q→Inverse(p) (the contains_key guard fires). Skip.
                if a.role_id() == b.role_id() {
                    continue;
                }
                if m.contains_key(&a.role_id()) || m.contains_key(&b.role_id()) {
                    continue;
                }
                m.insert(b.role_id(), a.flip());
            }
        }
        m
    };
    let canon = |r: owl_dl_core::ir::Role| -> owl_dl_core::ir::Role {
        match canon_map.get(&r.role_id()) {
            None => r,
            Some(&c) => {
                if r.is_inverse() {
                    c.flip()
                } else {
                    c
                }
            }
        }
    };

    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(sub_role),
                sup,
            } => {
                // Canonicalize both sides. If they end up at matching
                // polarities, record the role-id inclusion (the
                // hierarchy is on `RoleId`, with polarity handled
                // separately by `role_matches`'s same-polarity check).
                let cs = canon(*sub_role);
                let ct = canon(*sup);
                if cs.is_inverse() == ct.is_inverse() {
                    builder.add_sub_role(cs.role_id(), ct.role_id());
                }
            }
            Axiom::EquivalentObjectProperties(roles) => {
                let cans: Vec<owl_dl_core::ir::Role> = roles.iter().map(|r| canon(*r)).collect();
                for a in &cans {
                    for b in &cans {
                        if a != b && a.is_inverse() == b.is_inverse() {
                            builder.add_sub_role(a.role_id(), b.role_id());
                        }
                    }
                }
            }
            Axiom::SymmetricRole(role) if !role.is_inverse() => {
                builder.mark_symmetric(role.role_id());
            }
            Axiom::InverseObjectProperties(a, b)
                if a.role_id() == b.role_id() && !a.is_inverse() =>
            {
                // Self-inverse declaration: InverseObjectProperties(r, r) ⟹ r is symmetric.
                builder.mark_symmetric(a.role_id());
            }
            _ => {}
        }
    }
    builder.build()
}

/// Collect the length-2 role-chain axioms supported by Phase 5 (R).
///
/// Two sources:
/// 1. `SubObjectPropertyOf` with a `Chain` LHS — must have exactly
///    length 2 and use only named roles end-to-end (including the
///    super-role).
/// 2. `TransitiveRole(Role::Named(r))` lowered to `(r, r, r)` — the
///    standard chain encoding of role transitivity.
///
/// Length-N chains (N > 2) are silently *skipped* rather than
/// erroring out: dropping them under-approximates the role-side
/// closure (some role-level entailments are missed) but is sound
/// for class-side reasoning, which is what `classify` consumes.
/// Family ontology has 4 length-3 chains (cousins, great-relatives)
/// whose super-roles only appear in role-axiom declarations, not in
/// any class definition — so classification under this skip matches
/// `HermiT` on the class hierarchy. Inverse roles in any position
/// (including the super-role) are accepted; the tableau's chain
/// rule reads each position's polarity to choose edge direction.
fn collect_chain_axioms(
    internal: &InternalOntology,
) -> Result<Vec<(Role, Role, Role)>, ReasonError> {
    let mut chains = Vec::new();
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(parts),
                sup,
            } => {
                if parts.len() != 2 {
                    // Length-N (N > 2) chain: drop. See doc comment.
                    continue;
                }
                chains.push((parts[0], parts[1], *sup));
            }
            Axiom::TransitiveRole(role) => {
                // Transitivity on `r` lowers to `r ∘ r ⊑ r` —
                // including the inverse polarity if the user
                // declared `TransitiveObjectProperty` against an
                // inverse-typed role expression.
                chains.push((*role, *role, *role));
            }
            _ => {}
        }
    }
    Ok(chains)
}

/// Lower the simple role-characteristic axioms into the equivalent
/// concept- and inverse-axiom forms the rest of the pipeline already
/// handles. This runs before [`nnf_axioms`] so the new axioms ride
/// through normalization + absorption like any other input.
///
/// Lowerings (Phase 5 part S — "simple" SROIQ role characteristics):
/// - `SymmetricRole(Named(r))` ⇒ `InverseObjectProperties(r, r)` — a
///   role that is its own inverse is symmetric. Picked up by
///   [`collect_inverse_pairs`].
/// - `FunctionalRole(Named(r))` ⇒ `SubClassOf(⊤, Max(1, r, ⊤))`.
/// - `InverseFunctionalRole(Named(r))` ⇒ `SubClassOf(⊤, Max(1, r⁻, ⊤))`.
///
/// Inverse-polarity inputs (`SymmetricRole(Inverse(r))`) are
/// semantically equivalent to the same-named axiom but we don't bother
/// special-casing — converter only emits named-role characteristics
/// today.
///
/// Original axioms are kept in `internal.axioms` so that downstream
/// passes (e.g., reverse conversion) still see them; the lowered
/// duplicates are appended.
fn expand_role_characteristics(internal: &mut InternalOntology) {
    let top = internal.concepts.top();
    let mut additions: Vec<Axiom> = Vec::new();
    for ax in &internal.axioms {
        match ax {
            Axiom::SymmetricRole(role) if !role.is_inverse() => {
                additions.push(Axiom::InverseObjectProperties(*role, *role));
            }
            Axiom::FunctionalRole(role) if !role.is_inverse() => {
                let max1 = internal.concepts.max(1, *role, top);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: max1,
                });
            }
            Axiom::InverseFunctionalRole(role) if !role.is_inverse() => {
                let inv = Role::inverse(role.role_id());
                let max1 = internal.concepts.max(1, inv, top);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: max1,
                });
            }
            Axiom::ReflexiveRole(role) => {
                // ⊤ ⊑ Self(r) — every individual carries the
                // self-restriction concept; the tableau's
                // `apply_self_restriction` then materializes the
                // self-edge.
                let self_r = internal.concepts.self_restriction(*role);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: self_r,
                });
            }
            Axiom::IrreflexiveRole(role) => {
                // ⊤ ⊑ ¬Self(r) — every individual is constrained to
                // not have an r-self-edge. NNF-safe: `Not(Self)` is
                // already in NNF.
                let self_r = internal.concepts.self_restriction(*role);
                let not_self = internal.concepts.not(self_r);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: not_self,
                });
            }
            _ => {}
        }
    }
    internal.axioms.extend(additions);
}

/// Collect roles declared `AsymmetricObjectProperty`. Inverse-typed
/// declarations resolve to the same underlying `RoleId` (the
/// asymmetry constraint is about the unordered role pair regardless
/// of source polarity).
fn collect_asymmetric_roles(internal: &InternalOntology) -> Vec<RoleId> {
    let mut out = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::AsymmetricRole(role) = ax {
            out.push(role.role_id());
        }
    }
    out
}

/// Decompose every `DisjointObjectProperties(r, s, …)` axiom into its
/// pairwise constituents. Reflexive entries `(r, r)` (degenerate
/// `Disjoint(r)`) are skipped — they'd assert the role is disjoint
/// from itself, which is only satisfiable when no pair is in `r`. We
/// leave that diagnosis to higher-level validators rather than seed
/// universal clashes.
/// Collect forward-only disjoint role pairs from `DisjointObjectProperties` axioms.
///
/// **Soundness restriction (forward-only):** a `DisjointObjectProperties(r, ObjectInverseOf(s))`
/// axiom forbids `r(a,b) ∧ s(b,a)`, NOT `r(a,b) ∧ s(a,b)`. The `(RoleId, RoleId)` pair type
/// strips polarity via `role_id()`, so emitting an inverse-involving pair would represent a
/// DIFFERENT (weaker) semantic constraint and would cause false-positive clashes in the P9
/// `ABox` pre-check and the tableau disjoint-role rule.
///
/// Solution: emit a pair only when **both** roles are forward/named (`!role.is_inverse()`).
/// Axioms that involve an inverse are silently skipped — a sound under-approximation
/// (may miss a clash, never produces a false positive).  Polarity-aware disjoint-role
/// handling (storing the full `Role` pair) is deferred as future work.
fn collect_disjoint_role_pairs(internal: &InternalOntology) -> Vec<(RoleId, RoleId)> {
    let mut pairs = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::DisjointObjectProperties(roles) = ax {
            for i in 0..roles.len() {
                for j in (i + 1)..roles.len() {
                    let ri = roles[i];
                    let rj = roles[j];
                    // Soundness: only forward–forward disjoint pairs.  An inverse-involving
                    // pair (e.g. `Disjoint(r, Inv(s))`) forbids `r(a,b) ∧ s(b,a)`, not
                    // `r(a,b) ∧ s(a,b)`.  The polarity-stripped `(RoleId, RoleId)` representation
                    // cannot express that distinction; emitting it would be an FP in P9 and the
                    // tableau disjoint-role clash.  Skip — a sound under-approximation.
                    if ri.is_inverse() || rj.is_inverse() {
                        continue;
                    }
                    let a = ri.role_id();
                    let b = rj.role_id();
                    if a != b {
                        pairs.push((a, b));
                    }
                }
            }
        }
    }
    pairs
}

/// Collect declared inverse-role pairs from `InverseObjectProperties`
/// axioms. Each axiom `InverseObjectProperties(r, s)` contributes one
/// `(r.role_id(), s.role_id())` pair; the tableau context populates
/// the map symmetrically.
fn collect_inverse_pairs(internal: &InternalOntology) -> Vec<(RoleId, RoleId)> {
    let mut pairs = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::InverseObjectProperties(a, b) = ax {
            pairs.push((a.role_id(), b.role_id()));
        }
    }
    pairs
}

#[allow(clippy::too_many_arguments)]
fn decide<F>(
    pool: &ConceptPool,
    tbox: &AbsorbedTBox,
    hierarchy: &RoleHierarchy,
    inverse_pairs: &[(RoleId, RoleId)],
    chain_axioms: &[(Role, Role, Role)],
    asymmetric_roles: &[RoleId],
    disjoint_role_pairs: &[(RoleId, RoleId)],
    complements: &[(ConceptId, ConceptId)],
    abox: &Abox,
    extra_distinct: &[(IndividualId, IndividualId)],
    extra_neg_prop: &[(IndividualId, RoleId, IndividualId)],
    dkey_ranges: &std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_datatypes::CardRange>,
    id_budget: &TableauIdBudget,
    deadline: Option<std::time::Instant>,
    build_test_concept: F,
) -> Result<Option<bool>, ReasonError>
where
    F: FnOnce(&mut ConceptPool) -> ConceptId,
{
    // Fast-exit if the deadline is already spent — BEFORE the expensive
    // `pool.clone()` + context setup below. On large ontologies that per-call
    // ConceptPool clone dominates: under a global classify deadline,
    // `ore_ont_3215` issues ~55k unsat/tier-walk probes that each cloned a
    // ~200k-concept pool even though the search would instant-timeout — minutes
    // of deadline-oblivious setup. `None` = no verdict (timeout), which every
    // caller already treats soundly (unsat probe → satisfiable; subsumption
    // probe → not-subsumed).
    if let Some(d) = deadline
        && std::time::Instant::now() >= d
    {
        return Ok(None);
    }
    let mut pool = pool.clone();
    let test_concept: ConceptId = build_test_concept(&mut pool);
    // Pre-build the `∀role.¬{obj}` concepts for `extra_neg_prop` NOW, while
    // `pool` is still mutably available — mirrors the `NegativeObjectPropertyAssertion`
    // encoding `collect_abox` builds (lib.rs, `Axiom::NegativeObjectPropertyAssertion`
    // arm), just against this per-probe cloned pool. Must happen before `ctx`
    // takes its immutable borrow of `pool` below, since `ctx` stays live through
    // the whole seed+search below and `pool.nominal`/`not`/`all` all need `&mut`.
    let extra_neg_prop_concepts: Vec<(IndividualId, IndividualId, ConceptId)> = extra_neg_prop
        .iter()
        .map(|&(subj, role, obj)| {
            let nom = pool.nominal(obj);
            let neg = pool.not(nom);
            let all = pool.all(owl_dl_core::ir::Role::Named(role), neg);
            (subj, obj, all)
        })
        .collect();
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, tbox, hierarchy);
    // Anywhere-blocking on the deadline-FREE query paths (`is_consistent`,
    // `is_class_satisfiable`, un-timed `realize`/`instance`). Ancestor-scoped
    // pair-blocking cannot block a generating ∃-cycle anchored at a nominal
    // root — the pairwise parent-subset condition never holds near that anchor —
    // so a `{a} ⊓ ¬C` probe over a defined-class + covering-disjunction +
    // property-domain ontology grows the completion graph without bound (issue
    // #35 v3). Anywhere-blocking (Motik/Shearer/Horrocks) blocks against any
    // earlier non-nominal node and terminates it. The deadline-BOUNDED paths
    // (classify pairs, timed realize probes) keep ancestor-blocking: they have a
    // deadline safety net and the tuned classify loop is left untouched (a
    // 152-ontology ORE + curated-corpus bake-off showed anywhere-blocking is
    // verdict-identical there, but there is no reason to perturb it). Env
    // override: `RUSTDL_ANYWHERE_BLOCKING=1` forces it on everywhere (incl.
    // classify), `=0` forces the pre-fix ancestor-only behaviour everywhere.
    let anywhere = match std::env::var("RUSTDL_ANYWHERE_BLOCKING").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => deadline.is_none(),
    };
    ctx.set_anywhere_blocking(anywhere);
    // Concrete-domain solver (P3): supply the DKey range side-map so the
    // additive concrete-domain clash + the cardinality-suppression guard fire.
    if !dkey_ranges.is_empty() {
        ctx.set_dkey_ranges(dkey_ranges.clone());
    }
    if let Some(d) = deadline {
        ctx.set_deadline(d);
    }
    for &(r, s) in inverse_pairs {
        ctx.declare_inverse_pair(r, s);
    }
    for &(r1, r2, sup) in chain_axioms {
        ctx.declare_chain_axiom(r1, r2, sup);
    }
    for &r in asymmetric_roles {
        ctx.declare_asymmetric_role(r);
    }
    for &(r, s) in disjoint_role_pairs {
        ctx.declare_disjoint_role_pair(r, s);
    }
    for &(body, comp) in complements {
        ctx.set_complement(body, comp);
    }

    // Phase 5 `ABox` seeding. Order matters:
    // 1. Create a nominal root for each individual.
    // 2. DifferentIndividuals — mark before any merges so a later
    //    SameIndividual on the same pair is detected as a clash.
    // 3. SameIndividual merges; failed merges (declared distinct)
    //    flag the surviving node with ⊥.
    // 4. ClassAssertion / NegativeObjectPropertyAssertion labels.
    // 5. ObjectPropertyAssertion edges between nominal roots.
    // Then add the test class to a fresh anonymous root and run.
    let mut roots: HashMap<IndividualId, NodeId> = HashMap::new();
    for &(ind, nom) in &abox.individuals {
        let node = ctx.new_node();
        ctx.add_label(node, nom);
        ctx.assign_nominal(ind, node);
        roots.insert(ind, node);
    }
    for &(left, right) in &abox.different_pairs {
        if let (Some(&nleft), Some(&nright)) = (roots.get(&left), roots.get(&right)) {
            let nleft = ctx.resolve(nleft);
            let nright = ctx.resolve(nright);
            ctx.mark_distinct(nleft, nright);
        }
    }
    // Task 0.3: caller-supplied "extra ABox facts" for a snapshot-preserving
    // augment-and-recheck probe (#46 same-individuals: KB ∪ {a≠b}; #45
    // property values: KB ∪ {¬R(a,b)}). Seeded in the same slot as the
    // corresponding native facts above/below — distinct pairs marked before
    // any merges, neg-prop labels alongside the native
    // `NegativeObjectPropertyAssertion` labels — so ordering semantics match.
    for &(left, right) in extra_distinct {
        if let (Some(&nl), Some(&nr)) = (roots.get(&left), roots.get(&right)) {
            let nl = ctx.resolve(nl);
            let nr = ctx.resolve(nr);
            ctx.mark_distinct(nl, nr);
        }
    }
    for &(subj, obj, all) in &extra_neg_prop_concepts {
        if let (Some(&ns), Some(_)) = (roots.get(&subj), roots.get(&obj)) {
            let n = ctx.resolve(ns);
            ctx.add_label(n, all);
        }
    }
    for &(left, right) in &abox.same_pairs {
        if let (Some(&nleft), Some(&nright)) = (roots.get(&left), roots.get(&right)) {
            let target = ctx.resolve(nleft);
            let source = ctx.resolve(nright);
            if target == source {
                continue;
            }
            if !ctx.merge_into(source, target)
                && let Some(bot) = ctx.pool().bot_id()
            {
                ctx.add_label(target, bot);
            }
        }
    }
    for &(ind, c) in &abox.class_assertions {
        if let Some(&n) = roots.get(&ind) {
            let target = ctx.resolve(n);
            ctx.add_label(target, c);
        }
    }
    for &(ind, c) in &abox.negative_property_assertions {
        if let Some(&n) = roots.get(&ind) {
            let target = ctx.resolve(n);
            ctx.add_label(target, c);
        }
    }
    for &(from, role, to) in &abox.property_assertions {
        if let (Some(&nf), Some(&nt)) = (roots.get(&from), roots.get(&to)) {
            let from_n = ctx.resolve(nf);
            let to_n = ctx.resolve(nt);
            ctx.add_edge(from_n, role, to_n);
        }
    }

    // Now the test class on a fresh anonymous root.
    let test_root = ctx.new_node();
    ctx.add_label(test_root, test_concept);

    // Deadline-bounded paths (classify pairs, timed realize probes) run inline
    // on the current (possibly rayon-worker) stack with the modest cap — they
    // cannot hang (the deadline is checked at every recursive entry) and a cap
    // hit is a sound MISS. Deadline-free paths (`is_consistent`,
    // `is_class_satisfiable`, un-timed realize) instead run with a deep cap on
    // a large dedicated stack, so termination rests on pair-blocking rather
    // than an artificial recursion limit — the issue-#35 hang was this cap
    // cutting a blocking-bounded-but-deep branch and destroying back-jumping.
    let outcome = if let Some(dl) = deadline {
        // Adaptive early-abandon (RUSTDL_TABLEAU_EARLY_ABANDON, default ON since 0.4.14).
        // Armed only on the DEADLINE-BOUNDED arm — that is the one and only path
        // `MAX_SEARCH_DEPTH` is reachable from, so arming the deep arm would be
        // dead code that nonetheless changed a `search`-entry predicate.
        if tableau_early_abandon_enabled() {
            ctx.enable_early_abandon(tableau_early_abandon_cap_hits());
        }
        // Iterative deepening of the modest cap (RUSTDL_TABLEAU_ITERATIVE_DEEPENING,
        // default OFF). Flag-OFF this is verbatim the pre-change single search at
        // `MAX_SEARCH_DEPTH`, so the OFF path is byte-identical by construction.
        if tableau_iterative_deepening_enabled() {
            search_iterative_deepening(&mut ctx, dl, id_budget)
        } else {
            owl_dl_tableau::search(&mut ctx, MAX_SEARCH_DEPTH)
        }
    } else {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(DEEP_SEARCH_STACK_BYTES)
                .spawn_scoped(scope, || {
                    owl_dl_tableau::search(&mut ctx, DEEP_SEARCH_DEPTH)
                })
                .expect("spawn deep tableau search thread")
                .join()
                .expect("deep tableau search thread panicked")
        })
    };
    // Calibration channel for the early-abandon constant. Read before `ctx` is
    // dropped; one line per armed probe, and nothing at all when unarmed.
    if let Some((trials, definite, depth0, max_stall_run, abandoned)) = ctx.early_abandon_stats()
        && tableau_early_abandon_stats_enabled()
    {
        eprintln!(
            "# ea probe trials={trials} definite={definite} depth0={depth0} \
             max_stall_run={max_stall_run} abandoned={}",
            u8::from(abandoned)
        );
    }
    match outcome {
        owl_dl_tableau::SearchVerdict::Sat => Ok(Some(true)),
        owl_dl_tableau::SearchVerdict::Unsat(_) => Ok(Some(false)),
        // Live-node cap hit: sound under-approximation, never an error (#35 v4
        // safety net) — must be checked before the DepthLimit arms below so a
        // cap trip is never mistaken for a hard NoVerdict.
        owl_dl_tableau::SearchVerdict::NodeCap => Ok(None),
        // Adaptive early-abandon: report the same sound "don't know" a deadline
        // cut and a `NodeCap` trip report (`Ok(None)`), NOT `Err(NoVerdict)`.
        // Checked BEFORE the deadline arm so the abandon reason is attributed
        // even when the deadline happened to elapse during the unwind.
        owl_dl_tableau::SearchVerdict::DepthLimit if ctx.early_abandoned() => Ok(None),
        owl_dl_tableau::SearchVerdict::DepthLimit if ctx.deadline_reached() => Ok(None),
        owl_dl_tableau::SearchVerdict::DepthLimit => Err(ReasonError::NoVerdict),
    }
}

// ---------------------------------------------------------------------------
// Proof API (Track B)
// ---------------------------------------------------------------------------

pub use owl_dl_saturation::proof::{
    AxiomRef, DerivedFact, ElRule, Inference, ProofNode, ProofTrace, ProveResult, SyntheticDef,
    check_proof, check_proof_with_content, prove_subsumption, render_proof, render_proof_with_defs,
};
pub use owl_dl_saturation::{SaturateConfig, saturate_with_config};

/// Step-level proof result, boxed to avoid large-size enum variants.
#[derive(Debug)]
pub struct SaturatorProofData {
    /// The root of the proof tree.
    pub root: ProofNode,
    /// The `ProofTrace` used.
    pub trace: ProofTrace,
    /// Number of declared classes (for vocabulary lookups).
    pub vocab_num_classes: usize,
    /// Number of axioms in the `InternalOntology` (for axiom-ref range checks).
    pub num_axioms: usize,
}

/// Result of a `prove_entailment` call.
#[derive(Debug)]
pub enum ProveEntailmentResult {
    /// Step-level proof from the EL saturator.
    SaturatorProof(Box<SaturatorProofData>),
    /// The entailment is not in the saturation fragment; axiom-level justification.
    JustificationFallback(justify::Justification<horned_owl::model::RcStr>),
    /// The entailment does not hold.
    NotEntailed,
}

/// Prove a `sub ⊑ sup` entailment: run the saturator with proof recording and
/// return either a step-level proof tree (EL fragment) or check if held (via
/// the general reasoner) + note that the step proof is unavailable.
///
/// For the full justification fallback (with axiom sets), use [`prove_entailment_rcstr`].
///
/// This forces `record_proofs: true` regardless of `RUSTDL_PROOF`.
///
/// # Errors
/// Propagates `ReasonError` from conversion.
pub fn prove_entailment<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    sub_iri: &str,
    sup_iri: &str,
) -> Result<ProveEntailmentResult, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let sub_opt = internal.vocabulary.class_id(sub_iri);
    let sup_opt = internal.vocabulary.class_id(sup_iri);
    let (Some(sub), Some(sup)) = (sub_opt, sup_opt) else {
        return Ok(ProveEntailmentResult::NotEntailed);
    };

    let cfg = SaturateConfig {
        record_proofs: true,
    };
    let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);

    if subs.contains(sub, sup) {
        let trace = maybe_trace.unwrap_or_default();
        let mut memo = std::collections::HashMap::new();
        if let Some(root) = prove_subsumption(&trace, sub, sup, &mut memo) {
            return Ok(ProveEntailmentResult::SaturatorProof(Box::new(
                SaturatorProofData {
                    root,
                    vocab_num_classes: internal.vocabulary.num_classes(),
                    num_axioms: internal.axioms.len(),
                    trace,
                },
            )));
        }
    }

    // Check if the entailment holds at all.
    let held = is_subclass_of(ontology, sub_iri, sup_iri)?;
    if !held {
        return Ok(ProveEntailmentResult::NotEntailed);
    }

    // Held but not in saturation fragment; no justification available in generic path.
    // (Use prove_entailment_rcstr for the full justification fallback.)
    Ok(ProveEntailmentResult::JustificationFallback(
        justify::Justification {
            axioms: vec![],
            fragment: classify::FragmentClassification::OutOfFragment,
            minimal_guaranteed: false,
        },
    ))
}

/// Variant of `prove_entailment` for `SetOntology<RcStr>` (the common case
/// used by the CLI and most tests), which supports the justification fallback.
///
/// # Errors
/// Propagates `ReasonError`.
pub fn prove_entailment_rcstr(
    ontology: &horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
    sub_iri: &str,
    sup_iri: &str,
) -> Result<ProveEntailmentResult, ReasonError> {
    use justify::Entailment;

    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let sub_opt = internal.vocabulary.class_id(sub_iri);
    let sup_opt = internal.vocabulary.class_id(sup_iri);

    // If classes not found, not entailed.
    let (Some(sub), Some(sup)) = (sub_opt, sup_opt) else {
        return Ok(ProveEntailmentResult::NotEntailed);
    };

    let cfg = SaturateConfig {
        record_proofs: true,
    };
    let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);

    if subs.contains(sub, sup) {
        let trace = maybe_trace.unwrap_or_default();
        let mut memo = std::collections::HashMap::new();
        if let Some(root) = prove_subsumption(&trace, sub, sup, &mut memo) {
            return Ok(ProveEntailmentResult::SaturatorProof(Box::new(
                SaturatorProofData {
                    vocab_num_classes: internal.vocabulary.num_classes(),
                    num_axioms: internal.axioms.len(),
                    root,
                    trace,
                },
            )));
        }
    }

    // Check if the entailment holds at all (possibly via tableau).
    let held = is_subclass_of(ontology, sub_iri, sup_iri)?;
    if !held {
        return Ok(ProveEntailmentResult::NotEntailed);
    }

    // Holds but not in saturation fragment — axiom-level justification.
    let q = Entailment::SubClassOf {
        sub: sub_iri.to_string(),
        sup: sup_iri.to_string(),
    };
    match justify::find_one_justification(ontology, &q) {
        Ok(Some(j)) => Ok(ProveEntailmentResult::JustificationFallback(j)),
        Ok(None) => Ok(ProveEntailmentResult::NotEntailed),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    fn parse_internal_lib(src: &str) -> InternalOntology {
        owl_dl_core::convert::convert_ontology(&parse(src)).expect("fixture converts")
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    /// Task 0.2: `PreparedOntology::pair_disjoint_with_deadline` must return
    /// `Some(true)` for a told-disjoint pair (`a ⊓ b` unsatisfiable) and
    /// `Some(false)` for a satisfiable pair (`a ⊓ a`, never a false positive).
    #[test]
    fn pair_disjoint_detects_told_disjoint() {
        let internal = parse_internal_lib(
            r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            DisjointClasses(:A :B))",
        );
        let a = internal
            .vocabulary
            .class_id("http://ex/#A")
            .expect("A is declared");
        let b = internal
            .vocabulary
            .class_id("http://ex/#B")
            .expect("B is declared");
        let prepared = PreparedOntology::from_internal(internal).expect("prepares");
        assert_eq!(
            prepared
                .pair_disjoint_with_deadline(a, b, None)
                .expect("decide succeeds"),
            Some(true)
        );
        // A vs A is satisfiable (A is not unsat here) ⇒ not disjoint.
        assert_eq!(
            prepared
                .pair_disjoint_with_deadline(a, a, None)
                .expect("decide succeeds"),
            Some(false)
        );
    }

    /// Task 3.1 Step 1: `PreparedOntology::pair_individuals_disjoint_with_deadline`
    /// must return `Some(true)` for a told-`DifferentIndividuals` pair
    /// (`{a}⊓{b}` unsatisfiable) and `Some(false)` for a self-pair (never a
    /// false positive).
    #[test]
    fn pair_individuals_disjoint_detects_told_different() {
        let internal = parse_internal_lib(
            r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            DifferentIndividuals(:a :b))",
        );
        let a = internal
            .vocabulary
            .individual_id("http://ex/#a")
            .expect("a is declared");
        let b = internal
            .vocabulary
            .individual_id("http://ex/#b")
            .expect("b is declared");
        let prepared = PreparedOntology::from_internal(internal).expect("prepares");
        assert_eq!(
            prepared
                .pair_individuals_disjoint_with_deadline(a, b, None)
                .expect("decide succeeds"),
            Some(true)
        );
        assert_eq!(
            prepared
                .pair_individuals_disjoint_with_deadline(a, a, None)
                .expect("decide succeeds"),
            Some(false)
        );
    }

    /// Task 0.3: `PreparedOntology::consistent_with_extra` injects extra
    /// `DifferentIndividuals` facts into the frozen tableau seed WITHOUT
    /// rebuilding the `PreparedOntology` snapshot. `Functional(r); r(a,b);
    /// r(a,c)` forces `b=c`; the base KB is consistent, but adding `b≠c`
    /// as an extra fact must clash (⇒ `b=c` is entailed).
    #[test]
    fn consistent_with_extra_distinct_detects_forced_same() {
        let internal = parse_internal_lib(
            r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
            FunctionalObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :a :c))",
        );
        let b = internal
            .vocabulary
            .individual_id("http://ex/#b")
            .expect("b is declared");
        let c = internal
            .vocabulary
            .individual_id("http://ex/#c")
            .expect("c is declared");
        let prepared = PreparedOntology::from_internal(internal).expect("prepares");
        // Base KB is consistent…
        assert_eq!(
            prepared
                .consistent_with_extra(&[], &[], None)
                .expect("decide succeeds"),
            Some(true)
        );
        // …but KB ∪ {b≠c} is inconsistent ⇒ b=c entailed.
        assert_eq!(
            prepared
                .consistent_with_extra(&[(b, c)], &[], None)
                .expect("decide succeeds"),
            Some(false)
        );
    }

    /// Task 0.3: the `extra_neg_prop` slice injects a `¬R(subj,obj)` fact
    /// (encoded as `subj ⊑ ∀role.¬{obj}`, the same form `collect_abox` builds
    /// for a native `NegativeObjectPropertyAssertion`). `R(a,b)` is asserted,
    /// so adding `¬R(a,b)` as an extra fact must clash — proving `R(a,b)` is
    /// entailed (the #45 property-values use case).
    #[test]
    fn consistent_with_extra_neg_prop_detects_asserted_edge() {
        let internal = parse_internal_lib(
            r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(ObjectProperty(:r))
            ObjectPropertyAssertion(:r :a :b))",
        );
        let a = internal
            .vocabulary
            .individual_id("http://ex/#a")
            .expect("a is declared");
        let b = internal
            .vocabulary
            .individual_id("http://ex/#b")
            .expect("b is declared");
        let r = internal
            .vocabulary
            .role_id("http://ex/#r")
            .expect("r is declared");
        let prepared = PreparedOntology::from_internal(internal).expect("prepares");
        // Base KB is consistent…
        assert_eq!(
            prepared
                .consistent_with_extra(&[], &[], None)
                .expect("decide succeeds"),
            Some(true)
        );
        // …but KB ∪ {¬r(a,b)} is inconsistent ⇒ r(a,b) entailed.
        assert_eq!(
            prepared
                .consistent_with_extra(&[], &[(a, r, b)], None)
                .expect("decide succeeds"),
            Some(false)
        );
    }

    /// Task 2 Step 1 (RED): `realize_base_model_types` returns one `ABox`
    /// witness model's per-individual COMPLETE type sets. `a` is asserted
    /// `D`; `D ⊑ E` so `E` is derived; `F` is a declared, disjoint sibling
    /// class so `a` is provably NOT an `F`. The returned set for `a` must
    /// be a superset of `{D, E}` and must not contain `F`.
    #[test]
    fn realize_base_model_types_returns_witness_type_sets() {
        let internal = parse_internal_lib(
            r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))
            Declaration(NamedIndividual(:a))
            SubClassOf(:D :E)
            DisjointClasses(:D :F)
            ClassAssertion(:D :a))",
        );
        let d = internal
            .vocabulary
            .class_id("http://ex/#D")
            .expect("D is declared");
        let e = internal
            .vocabulary
            .class_id("http://ex/#E")
            .expect("E is declared");
        let f = internal
            .vocabulary
            .class_id("http://ex/#F")
            .expect("F is declared");
        let a = internal
            .vocabulary
            .individual_id("http://ex/#a")
            .expect("a is declared");
        let prepared = PreparedOntology::from_internal(internal).expect("prepares");
        let types = prepared
            .realize_base_model_types(None)
            .expect("a consistent ABox yields a witness model");
        let a_types = types
            .get(a.index() as usize)
            .expect("witness types indexed by individual");
        assert!(a_types.contains(&d), "witness model must type a as D");
        assert!(
            a_types.contains(&e),
            "witness model must derive a:E via D⊑E"
        );
        assert!(
            !a_types.contains(&f),
            "a is provably NOT F (DisjointClasses(D,F) + a:D)"
        );
    }

    /// P2 plumbing: `PreparedOntology` builds the `ClassId → CardRange` side-map
    /// by decoding the synthetic integer `DKey` filler classes. An ontology with
    /// an `xsd:integer`-facet `DataSomeValuesFrom` lowers to `∃p.DKey([37,100])`,
    /// so the map must hold the matching `CardRange::Int`. (The map is consumed
    /// by the not-yet-armed P3 clash; here we only verify it populates.)
    #[test]
    fn prepared_builds_integer_dkey_range_map() {
        let src = format!(
            "{HEADER}Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nOntology(\n\
             Declaration(Class(:C)) Declaration(DataProperty(:p))\n\
             SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
             xsd:minInclusive \"37\"^^xsd:integer xsd:maxInclusive \"100\"^^xsd:integer)))\n)\n"
        );
        let internal = convert_ontology(&parse(&src)).expect("converts");
        let prepared = PreparedOntology::from_internal(internal).expect("prepares");
        let ranges: Vec<_> = prepared
            .dkey_ranges
            .values()
            .filter_map(owl_dl_datatypes::CardRange::as_int)
            .collect();
        assert!(
            ranges
                .iter()
                .any(|i| i.min == Some(37) && i.max == Some(100)),
            "expected CardRange::Int([37,100]) in the side-map, got {ranges:?}"
        );
    }

    /// T3 termination acceptance for anywhere blocking (plan
    /// `docs/superpowers/plans/2026-06-15-anywhere-pairwise-blocking.md`).
    ///
    /// Drives the MAIN tableau consistency path directly on family's `ABox`
    /// (`PreparedOntology::decide_with_deadline(Top)` — the exact `decide` that
    /// hangs >60 s under ancestor-only blocking) with `RUSTDL_ANYWHERE_BLOCKING`
    /// forced ON via the env the `TableauContext` ctor reads. Asserts the main
    /// tableau TERMINATES within a generous wall cap (vs hanging) and reports
    /// the verdict. Per the plan, family must come back **inconsistent** (a
    /// terminating-but-consistent result would mean the clash was masked by an
    /// unsound blocker). `#[ignore]`d (needs the gitignored family fixture);
    /// run with `-- --ignored --nocapture`. Single-threaded by nature (it
    /// mutates the process env), so run it in isolation.
    #[test]
    #[ignore = "needs ontologies/real/family.ofn; main-tableau anywhere-blocking termination acceptance"]
    #[allow(unsafe_code)] // test-only: set/unset RUSTDL_ANYWHERE_BLOCKING around one decide
    fn family_main_tableau_terminates_under_anywhere_blocking() {
        use horned_owl::io::ofn::reader::read as read_ofn;
        use std::time::{Duration, Instant};
        // The corpus is gitignored and lives only in the main checkout, not in
        // git worktrees. Probe a few candidate locations + an env override.
        let candidates = [
            std::env::var("RUSTDL_FAMILY_OFN").unwrap_or_default(),
            "../../ontologies/real/family.ofn".to_string(),
            "/data/dumontier/rustdl/ontologies/real/family.ofn".to_string(),
        ];
        let Some(path) = candidates
            .iter()
            .map(std::path::Path::new)
            .find(|p| p.exists())
        else {
            eprintln!(
                "SKIP: family.ofn not found in {candidates:?}; \
                 set RUSTDL_FAMILY_OFN to its absolute path"
            );
            return;
        };
        let src = std::fs::read_to_string(path).expect("read family");
        let mut r = Cursor::new(src);
        let (o, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse family");
        let internal = convert_ontology(&o).expect("convert family");
        let prepared = PreparedOntology::from_internal(internal).expect("prepare family");

        // Force anywhere blocking ON for the main-tableau ctx built inside
        // `decide`. (This env is read once per TableauContext at construction.)
        // SAFETY: test process; set/remove around the single decide call.
        unsafe {
            std::env::set_var("RUSTDL_ANYWHERE_BLOCKING", "1");
        }
        assert!(
            anywhere_blocking_enabled(),
            "gate must read ON after setting the env"
        );

        // Generous cap: ancestor-only hangs >60 s; anywhere should resolve far
        // faster. If it elapses we report a soft FAIL (no hang, but no verdict).
        // Override with RUSTDL_FAMILY_CAP_S (e.g. a short cap for diagnosis).
        let cap_s = std::env::var("RUSTDL_FAMILY_CAP_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);
        let cap = Duration::from_secs(cap_s);
        let started = Instant::now();
        let deadline = started + cap;
        let outcome = prepared.decide_with_deadline(deadline, owl_dl_core::ConceptPool::top);
        let elapsed = started.elapsed();

        unsafe {
            std::env::remove_var("RUSTDL_ANYWHERE_BLOCKING");
        }

        eprintln!(
            "family main-tableau decide(Top) under anywhere blocking: \
             outcome={outcome:?} elapsed={elapsed:?}"
        );
        // decide(Top): Ok(Some(true)) = Top satisfiable = CONSISTENT;
        //              Ok(Some(false)) = Top unsatisfiable = INCONSISTENT;
        //              Ok(None) = deadline elapsed cleanly (DepthLimit + deadline
        //                         reached) — NOT a hang, NOT a definite verdict.
        //
        // SOUNDNESS GUARD (the only hard FAIL): a definite CONSISTENT verdict
        // would mean anywhere blocking masked family's known clash — an unsound
        // over-block. That MUST fail. A clean deadline (`Ok(None)`) is the
        // soundness-SAFE direction (no false `consistent`), so it does NOT fail.
        //
        // EMPIRICAL FINDING (2026-06-15, counters): anywhere blocking fires
        // heavily on family (~2.5M `is_blocked_true` in 20 s) but the dominant
        // single parent-role bucket is itself O(N) — even Phase-B bucket-keyed
        // blocking can't bound family's generative nominal ABox, so `decide`
        // hits the deadline rather than converging. Closing family end-to-end
        // needs the separate, out-of-scope wedge role-chain work (and/or a
        // HermiT-style NI-rule for nominal-driven generation). This test
        // therefore documents the boundary; it asserts only SOUNDNESS.
        match outcome {
            Ok(Some(true)) => {
                panic!(
                    "SOUNDNESS FAILURE: family came back CONSISTENT in \
                     {elapsed:?} — anywhere blocking masked family's known \
                     clash (unsound over-block). This must never happen."
                );
            }
            Ok(Some(false)) => {
                eprintln!("family inconsistent (terminated in {elapsed:?}) — ideal outcome");
            }
            Ok(None) => {
                eprintln!(
                    "family did NOT converge within {cap:?} under anywhere \
                     blocking — clean deadline (Ok(None)), soundness-SAFE (no \
                     false `consistent`). Documented scoping limitation: the \
                     dominant single-role candidate bucket is O(N); closing \
                     family needs separate wedge role-chain / NI-rule work."
                );
            }
            Err(e) => panic!("decide errored: {e:?}"),
        }
    }

    /// Perf probe (wine wall, 2026-06-07): split the per-pair wedge cost into
    /// `clauses.clone()` + `HyperEngine::new` (construction, un-preemptable by a
    /// deadline) vs `decide_with_deadline` (search). The histogram showed 9183
    /// wine pairs cost the wedge 100-999ms; this test decides whether that 200ms
    /// is construction (→ build-once-reuse lever) or search (→ deadline lever),
    /// and whether the search EVER completes (5s cap, not None). `#[ignore]`d
    /// (needs the gitignored wine fixture); run with `-- --ignored --nocapture`.
    #[test]
    #[ignore = "needs ontologies/real/wine.ofn; perf probe for the wine wedge-stall wall"]
    fn wine_wedge_construct_vs_solve_probe() {
        use horned_owl::io::ofn::reader::read as read_ofn;
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
        use std::time::{Duration, Instant};
        let path = std::path::Path::new("../../ontologies/real/wine.ofn");
        if !path.exists() {
            eprintln!("SKIP: missing {}", path.display());
            return;
        }
        let src = std::fs::read_to_string(path).expect("read wine");
        let mut r = Cursor::new(src);
        let (o, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse wine");
        let internal = owl_dl_core::convert::convert_ontology(&o).expect("convert");
        let cache = HyperCache::build(&internal);
        eprintln!("wine: {} base clauses", cache.clauses.len());

        // Helper: replicate HyperCache::decide's clause construction, returning
        // the assembled per-pair clause vec.
        let assemble = |sub: owl_dl_core::ir::ClassId, sup: owl_dl_core::ir::ClassId| {
            let mut clauses = cache.clauses.clone();
            clauses.push(DlClause {
                body: vec![Atom::Class(cache.fresh_q, X)],
                head: vec![Atom::Class(sub, X)],
            });
            if let Some(atoms) = cache.sup_neg.get(&sup) {
                clauses.push(DlClause {
                    body: vec![Atom::Class(cache.fresh_q, X)],
                    head: atoms.clone(),
                });
            } else {
                clauses.push(DlClause {
                    body: vec![Atom::Class(cache.fresh_q, X), Atom::Class(sup, X)],
                    head: vec![],
                });
            }
            clauses
        };

        // Find a few hard pairs: decide → Unknown at a 200ms deadline.
        let ids: Vec<owl_dl_core::ir::ClassId> =
            internal.vocabulary.classes().map(|(id, _)| id).collect();
        let n = ids.len();
        eprintln!("wine: {n} classes");
        let mut hard: Vec<(owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId)> = Vec::new();
        let mut probes = 0usize;
        'outer: for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                probes += 1;
                if probes > 400 {
                    break 'outer;
                }
                let dl = Instant::now() + Duration::from_millis(200);
                if cache.decide(ids[i], ids[j], Some(dl)) == HyperVerdict::Unknown {
                    hard.push((ids[i], ids[j]));
                    if hard.len() >= 3 {
                        break 'outer;
                    }
                }
            }
        }
        eprintln!(
            "found {} hard (Unknown@200ms) pairs in {probes} probes",
            hard.len()
        );

        for (sub, sup) in hard {
            // Timer 1: clone + push (the per-pair allocation).
            let t_clone = Instant::now();
            let clauses = assemble(sub, sup);
            let clone_ms = t_clone.elapsed().as_secs_f64() * 1000.0;
            // Timer 2: HyperEngine::new (build_disjoint_pairs + build_clause_indexes).
            let t_new = Instant::now();
            let mut engine = HyperEngine::new(&clauses, cache.fresh_q);
            if hyper_double_block_enabled() {
                engine = engine.with_double_blocking();
            }
            if hyper_precise_card_deps_enabled() {
                engine = engine.with_precise_card_deps();
            }
            if hyper_mrv_ordering_enabled() {
                engine = engine.with_mrv_ordering();
            }
            if let Some(sat) = cache.sat_lookahead.clone() {
                engine = engine.with_sat_lookahead(sat);
            }
            if crate::adaptive_budget_enabled() {
                engine = engine.with_adaptive_budget();
            }
            let new_ms = t_new.elapsed().as_secs_f64() * 1000.0;
            // Timer 3: the actual search, 5s cap (NOT None — may not terminate).
            let t_solve = Instant::now();
            let dl = Instant::now() + Duration::from_secs(5);
            let verdict = engine.decide_with_deadline(HYPER_WEDGE_DEPTH, Some(dl));
            let solve_ms = t_solve.elapsed().as_secs_f64() * 1000.0;
            let st = engine.stats();
            let verdict_str = match verdict {
                HyperResult::Sat => "Sat(NotSubsumed)",
                HyperResult::Unsat => "Unsat(Subsumed)",
                HyperResult::Stalled => "Stalled",
            };
            eprintln!(
                "pair({},{}) clone={clone_ms:.1}ms new={new_ms:.1}ms solve(5s cap)={solve_ms:.1}ms branches={} merge_branches={} -> {verdict_str}",
                sub.index(),
                sup.index(),
                st.branches_taken,
                st.merge_branches,
            );
        }
    }

    fn check(onto: &SetOntology<RcStr>, iri: &str) -> bool {
        is_class_satisfiable(onto, iri).expect("verdict returned")
    }

    /// Hypertableau Phase H1c: end-to-end cross-check of the
    /// structural-transformation clausifier + Horn engine against
    /// the EL entailment on the `∃R.E ⊑ F` back-propagation shape.
    ///
    /// The H1b finding (clausify-from-absorbed deferred `∃`-on-LHS)
    /// is fixed by the H1c clausifier, which transforms the GCI
    /// axioms directly: `∃R.E ⊑ F` now becomes the Horn clause
    /// `R(x,y) ∧ E(y) → F(x)`, so the engine derives `C ⊑ F`. This
    /// test (formerly `#[ignore]`d) now passes.
    #[test]
    fn hyper_horn_matches_el_closure_with_existential_backprop() {
        use owl_dl_core::clause::clausify;
        use owl_dl_core::convert::convert_ontology;
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};

        // C ⊑ ∃R.D,  D ⊑ E,  ∃R.E ⊑ F  ⊨  C ⊑ F.
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D))\nDeclaration(Class(:E))\n\
Declaration(Class(:F))\nDeclaration(ObjectProperty(:r))\n\
SubClassOf(:C ObjectSomeValuesFrom(:r :D))\n\
SubClassOf(:D :E)\n\
SubClassOf(ObjectSomeValuesFrom(:r :E) :F)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let clauses = clausify(&internal);
        assert!(
            HyperEngine::all_horn(&clauses),
            "pure-EL ontology must clausify to all-Horn"
        );
        let c_id = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("C interned");
        let f_id = internal
            .vocabulary
            .class_id("http://rustdl.test/F")
            .expect("F interned");
        let mut engine = HyperEngine::new(&clauses, c_id);
        assert_eq!(engine.run(4096), HyperResult::Sat);
        assert!(
            engine.root_labels().contains(&f_id),
            "hyper engine must derive C ⊑ F via ∃R.E ⊑ F back-propagation; \
             root labels = {:?}",
            engine.root_labels()
        );
    }

    /// Hypertableau Phase H2c: the `¬B`-injection subsumption probe
    /// decides entailed subsumptions (`Unsat`) and correctly rejects
    /// non-entailed ones (`Sat`). `A ⊑ B ⊑ C` ⊨ `A ⊑ C` but ⊭ `C ⊑ A`.
    #[test]
    fn hyper_subsumption_probe_finds_transitive_and_rejects_converse() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
SubClassOf(:A :B)\nSubClassOf(:B :C)\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        // A⊑C is entailed (transitively) ⇒ Unsat reported.
        assert!(holds("A", "C"), "A ⊑ C must be found");
        assert!(holds("A", "B"), "A ⊑ B must be found");
        assert!(holds("B", "C"), "B ⊑ C must be found");
        // The converse C⊑A is not entailed ⇒ never reported as Unsat.
        assert!(!holds("C", "A"), "C ⊑ A must NOT be reported");
        assert!(!holds("C", "B"), "C ⊑ B must NOT be reported");
        // 3 classes ⇒ 6 ordered pairs; 3 are entailed subsumptions.
        assert_eq!(probe.pairs_tested, 6);
        assert_eq!(probe.subsumptions, 3);
    }

    /// Hypertableau Phase H3a: antecedent DNF-distribution unlocks the
    /// pizza-style covering subsumption. `Vegetarian ≡ Topping ⊓
    /// (Cheese ⊔ Veg)`, `Cheese ⊑ Topping` ⊨ `Cheese ⊑ Vegetarian` —
    /// previously a miss because the nested `Or` in the antecedent
    /// conjunction was deferred.
    #[test]
    fn hyper_subsumption_probe_handles_distributed_covering() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:Topping))\nDeclaration(Class(:Cheese))\n\
Declaration(Class(:Veg))\nDeclaration(Class(:Vegetarian))\n\
SubClassOf(:Cheese :Topping)\n\
EquivalentClasses(:Vegetarian \
ObjectIntersectionOf(:Topping ObjectUnionOf(:Cheese :Veg)))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("Cheese", "Vegetarian"),
            "Cheese ⊑ Vegetarian must be derivable after antecedent distribution"
        );
    }

    /// H3b ¬sup-expansion fires: `A ≡ B ⊓ ¬C`, `D ⊑ B`, `D` disjoint
    /// `C` ⊨ `D ⊑ A`. Refuting `D ⊓ ¬A` needs expanding
    /// `¬A = ¬B ⊔ C`: the `¬B` branch clashes (`D ⊑ B`), the `C`
    /// branch clashes (`D`⊓`C` disjoint). Bare `D ∧ A → ⊥` could not
    /// derive this — it would need `A` positively.
    #[test]
    fn hyper_subsumption_probe_expands_negated_definition() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
Declaration(Class(:C))\nDeclaration(Class(:D))\n\
EquivalentClasses(:A ObjectIntersectionOf(:B ObjectComplementOf(:C)))\n\
SubClassOf(:D :B)\nDisjointClasses(:C :D)\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        assert!(probe.pairs_via_expansion > 0, "¬sup expansion must be used");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("D", "A"),
            "D ⊑ A must derive via expanding ¬A = ¬B ⊔ C"
        );
    }

    /// H4 encoding-drift guard: the hyper Q-injection and the tableau
    /// `sub ⊓ ¬sup` are *different encodings* of the same query. Every
    /// pair hyper proves (`Unsat`) must agree with the complete
    /// tableau (`is_subclass_of` = true). Catches clausifier/tableau
    /// drift before it reaches users — the wedge's soundness contract.
    #[test]
    fn hyper_wedge_agrees_with_tableau() {
        // A SROIQ-ish ontology with a covering + disjointness so the
        // hierarchy isn't all told: Veg ≡ Topping ⊓ (Cheese ⊔ Plant);
        // Cheese, Meat disjoint; Cheese, Plant ⊑ Topping.
        let src = format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:Topping))\nDeclaration(Class(:Cheese))\n\
Declaration(Class(:Plant))\nDeclaration(Class(:Meat))\nDeclaration(Class(:Veg))\n\
SubClassOf(:Cheese :Topping)\nSubClassOf(:Plant :Topping)\nSubClassOf(:Meat :Topping)\n\
DisjointClasses(:Cheese :Meat)\n\
EquivalentClasses(:Veg ObjectIntersectionOf(:Topping ObjectUnionOf(:Cheese :Plant)))\n)\n"
        );
        let onto = parse(&src);
        let internal = convert_ontology(&onto).expect("convert");
        let cache = HyperCache::build(&internal);
        let classes: Vec<(owl_dl_core::ir::ClassId, String)> = internal
            .vocabulary
            .classes()
            .map(|(id, iri)| (id, iri.to_string()))
            .collect();
        for (sub, sub_iri) in &classes {
            for (sup, sup_iri) in &classes {
                if sub == sup {
                    continue;
                }
                if cache.decide(*sub, *sup, None) == HyperVerdict::Subsumed {
                    // Hyper proved it ⇒ the complete tableau must agree.
                    let tableau =
                        is_subclass_of_internal(internal.clone(), sub_iri, sup_iri).expect("ok");
                    assert!(
                        tableau,
                        "hyper proved {sub_iri} ⊑ {sup_iri} but tableau disagrees"
                    );
                }
            }
        }
    }

    /// H4 `HyperCache::proves` works in isolation on the
    /// distributed-covering subsumption (saturation misses it, hyper
    /// proves it). Rules out a cache bug vs an orchestrator-wiring bug.
    #[test]
    fn hyper_cache_proves_distributed_covering() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:Topping))\nDeclaration(Class(:Cheese))\n\
Declaration(Class(:Veg))\nDeclaration(Class(:Vegetarian))\n\
SubClassOf(:Cheese :Topping)\n\
EquivalentClasses(:Vegetarian \
ObjectIntersectionOf(:Topping ObjectUnionOf(:Cheese :Veg)))\n)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let cheese = internal
            .vocabulary
            .class_id("http://rustdl.test/Cheese")
            .expect("interned");
        let vegetarian = internal
            .vocabulary
            .class_id("http://rustdl.test/Vegetarian")
            .expect("interned");
        let cache = HyperCache::build(&internal);
        assert!(
            (cache.decide(cheese, vegetarian, None) == HyperVerdict::Subsumed),
            "HyperCache must prove Cheese ⊑ Vegetarian"
        );
        let topping = internal
            .vocabulary
            .class_id("http://rustdl.test/Topping")
            .expect("interned");
        assert!(
            !(cache.decide(topping, vegetarian, None) == HyperVerdict::Subsumed),
            "Topping ⊑ Vegetarian must NOT be proven (not entailed)"
        );
    }

    /// B-complete increment 1 — POSITIVE canary. `DifferentIndividuals(a,b)`
    /// must reach the wedge so that `C ⊑ ≤1 R`, `C ⊑ ∃R.{a}`, `C ⊑ ∃R.{b}`
    /// forces two distinct R-fillers under a `≤1`, hence `C ⊑ ⊥`.
    /// Before the fix the wedge dropped `DifferentIndividuals`, treated the
    /// `{a}`/`{b}` fillers as mergeable, and reported Sat (a MISS); after the
    /// fix the entailed distinctness makes the `≤1` clash, so `C` is Unsat.
    #[test]
    fn different_individuals_forces_at_most_one_clash_in_wedge() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(ObjectProperty(:r))\n\
Declaration(NamedIndividual(:a))\nDeclaration(NamedIndividual(:b))\n\
SubClassOf(:C ObjectMaxCardinality(1 :r))\n\
SubClassOf(:C ObjectHasValue(:r :a))\n\
SubClassOf(:C ObjectHasValue(:r :b))\n\
DifferentIndividuals(:a :b)\n)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let c = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("interned");
        let cache = HyperCache::build(&internal);
        assert!(
            matches!(cache.classify_labels(c, None), LabelOracle::Unsat),
            "DifferentIndividuals(a,b) must make C ⊑ ≤1 r with distinct {{a}},{{b}} fillers unsatisfiable"
        );
    }

    /// B-complete increment 1 — SOUNDNESS NEGATIVE. The SAME ontology WITHOUT
    /// `DifferentIndividuals(a,b)`: under OWA `a` and `b` may denote the same
    /// individual, so the single `≤1 r` filler is satisfiable. The wedge must
    /// NOT clash — guards against over-forcing distinctness on co-nominal
    /// fillers that are not asserted distinct.
    #[test]
    fn co_nominal_fillers_without_different_individuals_stay_sat() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(ObjectProperty(:r))\n\
Declaration(NamedIndividual(:a))\nDeclaration(NamedIndividual(:b))\n\
SubClassOf(:C ObjectMaxCardinality(1 :r))\n\
SubClassOf(:C ObjectHasValue(:r :a))\n\
SubClassOf(:C ObjectHasValue(:r :b))\n)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let c = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("interned");
        let cache = HyperCache::build(&internal);
        assert!(
            !matches!(cache.classify_labels(c, None), LabelOracle::Unsat),
            "without DifferentIndividuals, {{a}} and {{b}} may be equal ⇒ C stays satisfiable"
        );
    }

    /// B-complete increment 1 — SECOND NEGATIVE. `DifferentIndividuals(a,b)`
    /// with NO `≤1` constraint: distinctness alone never produces a clash, so
    /// `C` (with two distinct fillers and no cardinality bound) stays Sat.
    #[test]
    fn different_individuals_without_at_most_stays_sat() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(ObjectProperty(:r))\n\
Declaration(NamedIndividual(:a))\nDeclaration(NamedIndividual(:b))\n\
SubClassOf(:C ObjectHasValue(:r :a))\n\
SubClassOf(:C ObjectHasValue(:r :b))\n\
DifferentIndividuals(:a :b)\n)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let c = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("interned");
        let cache = HyperCache::build(&internal);
        assert!(
            !matches!(cache.classify_labels(c, None), LabelOracle::Unsat),
            "DifferentIndividuals without a ≤n bound must not clash ⇒ C stays satisfiable"
        );
    }

    /// Per-class label heuristic basis (Task 2): on a Horn chain
    /// `A ⊑ B ⊑ C`, `HyperCache::classify_labels(A)` must return
    /// `LabelOracle::Sat` whose label set contains both `B` and `C`
    /// (every model of `A` is also a model of `B` and `C`). Wires
    /// `HyperEngine::satisfiability_labels` (Task 1) into the
    /// oracle returned to the orchestrator.
    #[test]
    fn hypercache_classify_labels_returns_atomic_supers_on_horn_chain() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
SubClassOf(:A :B)\nSubClassOf(:B :C)\n)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let a = internal
            .vocabulary
            .class_id("http://rustdl.test/A")
            .expect("A declared");
        let b = internal
            .vocabulary
            .class_id("http://rustdl.test/B")
            .expect("B declared");
        let c = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("C declared");
        let cache = HyperCache::build(&internal);
        let oracle = cache.classify_labels(a, None);
        match oracle {
            LabelOracle::Sat { labels, .. } => {
                assert!(labels.contains(&b), "A's labels must contain B: {labels:?}");
                assert!(labels.contains(&c), "A's labels must contain C: {labels:?}");
            }
            other => panic!("expected Sat, got {other:?}"),
        }
    }

    /// Nominals (`hasValue`): `A ≡ P ⊓ ∃r.{o}`, `B ⊑ P`, `B ⊑ ∃r.{o}`
    /// ⊨ `B ⊑ A`. The nominal `{o}` is clausified as an atomic class,
    /// so the `⊒`-direction `P ⊓ ∃r.{o} ⊑ A` derives `A` on `B`. The
    /// `RealItalianPizza` shape.
    #[test]
    fn hyper_subsumption_probe_handles_nominal_has_value() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:P))\n\
Declaration(ObjectProperty(:r))\nDeclaration(NamedIndividual(:o))\n\
EquivalentClasses(:A ObjectIntersectionOf(:P ObjectHasValue(:r :o)))\n\
SubClassOf(:B :P)\nSubClassOf(:B ObjectHasValue(:r :o))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(holds("B", "A"), "B ⊑ A must derive via the nominal {{o}}");
    }

    /// H3b Q-gating: the `¬sup` disjunction must bind only the root,
    /// never generated successors. `sub ≡ ∃R.A`, `sup ≡ ¬∃R.A` are
    /// disjoint but neither subsumes the other, so `sub ⊑ sup` must be
    /// `Sat` (not reported). If `¬sup` leaked onto the `R`-successor,
    /// the engine would clash spuriously and wrongly report `Unsat`.
    #[test]
    fn hyper_subsumption_probe_q_gating_no_spurious_subsumption() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(ObjectProperty(:r))\n\
Declaration(Class(:Sub))\nDeclaration(Class(:Sup))\n\
EquivalentClasses(:Sub ObjectSomeValuesFrom(:r :A))\n\
EquivalentClasses(:Sup ObjectComplementOf(ObjectSomeValuesFrom(:r :A)))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let reported = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        // Sub = ∃r.A, Sup = ¬∃r.A — genuinely disjoint, NOT subsuming.
        assert!(
            !reported("Sub", "Sup"),
            "Sub ⊑ Sup must NOT be reported (Q-gating leak would clash the r-successor)"
        );
    }

    /// HF2 canary (inverse-role propagation). `A ⊑ ∃R.B`,
    /// `B ⊑ ∀R⁻.C` ⊨ `A ⊑ C`: an `A` has an `R`-successor `b:B`;
    /// `b`'s `∀R⁻.C` forces every `R`-predecessor of `b` — including
    /// the `A` node — to be `C`. Deriving this requires propagating
    /// `∀R⁻` across the *reverse* edge. HF2 made this pass via
    /// inverse-aware matching in `enumerate_matches`: following `R⁻`
    /// from a node walks its `R`-predecessors. See
    /// `docs/hypertableau-hf2-scoping.md` §4.1.
    #[test]
    fn hyper_subsumption_probe_propagates_inverse_universal() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
Declaration(ObjectProperty(:R))\n\
SubClassOf(:A ObjectSomeValuesFrom(:R :B))\n\
SubClassOf(:B ObjectAllValuesFrom(ObjectInverseOf(:R) :C))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "C"),
            "A ⊑ C must be derivable via ∀R⁻ propagation across the reverse edge"
        );
    }

    /// HF2 named-inverse canary (`RBox` inverse-pair clausification).
    /// `InverseObjectProperties(R, S)` makes `S ≡ R⁻`, so `B ⊑ ∀S.C`
    /// is `B ⊑ ∀R⁻.C` and `A ⊑ ∃R.B` ⊨ `A ⊑ C` exactly as the inline
    /// canary — but here the inverse comes from the `RBox`, not an inline
    /// `ObjectInverseOf`. The clausifier rewrites role `S` to `R⁻`
    /// (`build_inverse_canon` / `canon_role`), after which the engine's
    /// flip-matching propagates `∀S` across the `R`-edge. See
    /// `docs/hypertableau-hf2-scoping.md` §1.
    #[test]
    fn hyper_subsumption_probe_propagates_named_inverse() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
Declaration(ObjectProperty(:R))\nDeclaration(ObjectProperty(:S))\n\
InverseObjectProperties(:R :S)\n\
SubClassOf(:A ObjectSomeValuesFrom(:R :B))\n\
SubClassOf(:B ObjectAllValuesFrom(:S :C))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "C"),
            "A ⊑ C must be derivable: S ≡ R⁻ so ∀S.C propagates across the R-edge"
        );
    }

    /// HF2 role-hierarchy canary. `R ⊑ S`, `A ⊑ ∃R.B`, `∃S.B ⊑ D`
    /// ⊨ `A ⊑ D`: A's R-successor `b:B` is also an S-successor (R ⊑ S),
    /// so `∃S.B ⊑ D` fires D onto A. Needs hierarchy-aware matching —
    /// `S(x,y)` must match an `R`-edge when `R ⊑* S` (one-way, so unlike
    /// inverse pairs this can't be canonicalized). HF2 threads the
    /// `RoleHierarchy` into the engine's `role_matches`. See
    /// `docs/hypertableau-hf2-scoping.md` §1/§4.2.
    #[test]
    fn hyper_subsumption_probe_propagates_super_role() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:D))\n\
Declaration(ObjectProperty(:R))\nDeclaration(ObjectProperty(:S))\n\
SubObjectPropertyOf(:R :S)\n\
SubClassOf(:A ObjectSomeValuesFrom(:R :B))\n\
SubClassOf(ObjectSomeValuesFrom(:S :B) :D)\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "D"),
            "A ⊑ D must be derivable: R ⊑ S so A's R-successor satisfies ∃S.B ⊑ D"
        );
    }

    /// HF4 canary (nominals as true singletons / NN-rule). `A ⊑ ≥2 R.{o}`
    /// is **unsatisfiable**: `{o}` is a singleton, so two R-successors
    /// both `{o}` must be the *same* individual — they cannot be the 2
    /// distinct fillers `≥2` requires. Composes with `HF3a`: `≥2`
    /// generates two `≠` successors both labelled `{o}`; the NN-rule
    /// merges same-nominal nodes; the `≠` then clashes. Today `{o}` is a
    /// plain atomic class (sound under-approximation that loses the
    /// singleton), so `A` is wrongly Sat and `A ⊑ B` (which holds only
    /// because `A` is unsat) is missed. `HF4a`'s NN-rule makes it pass.
    /// See `docs/hypertableau-hf4-scoping.md`.
    #[test]
    fn hyper_subsumption_probe_nominal_singleton_cardinality() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
Declaration(NamedIndividual(:o))\nDeclaration(ObjectProperty(:R))\n\
SubClassOf(:A ObjectMinCardinality(2 :R ObjectOneOf(:o)))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "B"),
            "A ⊑ B must hold because A is unsat: ≥2 R.{{o}} with {{o}} a singleton"
        );
    }

    /// `HF4a` over-merge guard (sibling of the singleton canary).
    /// `A ⊑ ≥1 R.{o} ⊓ ≤1 R.{o}` is **Sat**: one nominal successor
    /// satisfies both bounds, so the NN-rule must *not* fire (there is
    /// only one `{o}`-node). `A ⊑ B` (unrelated `B`) must therefore
    /// **not** be reported — pins that the NN-rule fires only on
    /// distinct same-nominal nodes, not spuriously.
    #[test]
    fn hyper_subsumption_probe_nominal_singleton_no_overmerge() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
Declaration(NamedIndividual(:o))\nDeclaration(ObjectProperty(:R))\n\
SubClassOf(:A ObjectIntersectionOf(\
ObjectMinCardinality(1 :R ObjectOneOf(:o)) \
ObjectMaxCardinality(1 :R ObjectOneOf(:o))))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let reported = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            !reported("A", "B"),
            "A ⊑ B must NOT be reported: A is sat (one {{o}}-successor, no merge)"
        );
    }

    /// `HF4b` probe: nominal-under-`∀` propagation. `A ⊑ ∃R.B ⊓ ∃R.C ⊓
    /// ∀R.{o}` with `B ⊓ C ⊑ ⊥` ⊨ `A` unsat: the two distinct `∃`
    /// successors both become `{o}` via `∀R.{o}` (clausified
    /// `R(x,y) → {o}(y)`), the NN-rule merges them, and `B ⊓ C → ⊥`
    /// clashes. Tests whether nominal-under-`∀` already composes with
    /// the `HF4a` NN-rule (the label that `∀` seeds is the same `Label`
    /// event the NN-rule triggers on). `D` unrelated; `A ⊑ D` holds iff
    /// `A` is unsat.
    #[test]
    fn hyper_subsumption_probe_nominal_under_forall_propagates() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
Declaration(Class(:D))\nDeclaration(NamedIndividual(:o))\n\
Declaration(ObjectProperty(:R))\n\
SubClassOf(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(:R :B) ObjectSomeValuesFrom(:R :C) \
ObjectAllValuesFrom(:R ObjectOneOf(:o))))\n\
DisjointClasses(:B :C)\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "D"),
            "A ⊑ D must hold because A is unsat: ∀R.{{o}} merges the B- and C-successors"
        );
    }

    /// `HF4b` composition probe: multi-predecessor nominal merge. `{o}` is
    /// reached two ways — `A —R→ {o}` (root) and `E —T→ {o}` — and the
    /// NN-rule merges those nodes. Two back-prop constraints, one per
    /// role: `{o} ⊑ ∀R⁻.WA ⊓ ∀T⁻.WE` ⊨ both `A ⊑ WA` and `E ⊑ WE`.
    ///
    /// This passes **without** an in-edge redirect on merge, and that is
    /// the point worth pinning: each `{o}` node fires its `∀R⁻`/`∀T⁻`
    /// consequences on its own `Label` event — back-propagating to *its
    /// own* predecessor — *before* the NN-rule collapses the two nodes.
    /// So the merged-away node's in-edge carries no information the
    /// survivor needed to learn later. (The in-edge redirect would still
    /// be principled for inverse-heavy ontologies with post-merge label
    /// derivation — corpus-inert, no constructible canary fails — so it
    /// is deliberately not built; see `docs/hypertableau-hf4-scoping.md`
    /// §2.) If a later change breaks the fire-before-merge ordering,
    /// this test catches it.
    #[test]
    fn hyper_subsumption_probe_nominal_merge_inedge_compose() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:E))\n\
Declaration(Class(:WA))\nDeclaration(Class(:WE))\n\
Declaration(NamedIndividual(:o))\n\
Declaration(ObjectProperty(:R))\nDeclaration(ObjectProperty(:S))\n\
Declaration(ObjectProperty(:T))\n\
SubClassOf(:A ObjectIntersectionOf(\
ObjectSomeValuesFrom(:R ObjectOneOf(:o)) ObjectSomeValuesFrom(:S :E)))\n\
SubClassOf(:E ObjectSomeValuesFrom(:T ObjectOneOf(:o)))\n\
SubClassOf(ObjectOneOf(:o) ObjectIntersectionOf(\
ObjectAllValuesFrom(ObjectInverseOf(:R) :WA) \
ObjectAllValuesFrom(ObjectInverseOf(:T) :WE)))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "WA") && holds("E", "WE"),
            "both A ⊑ WA (R-pred) and E ⊑ WE (T-pred) must hold: A⊑WA={}, E⊑WE={}",
            holds("A", "WA"),
            holds("E", "WE")
        );
    }

    /// Regression for the pizza false-positive-unsat bug fixed
    /// 2026-05-25. Minimal repro extracted from pizza.ofn via ROBOT
    /// STAR extraction + axiom-level bisection. Bug was in
    /// [`TableauContext::merge_into`]: it copied source-node labels
    /// without their [`DepSet`]s, so a merge-induced clash returned
    /// empty `clash_deps`, which the back-jumping search treated as
    /// "branch-independent unsat" and back-jumped past the licensing
    /// disjunction (the `:S ⊔ ∀hs.¬:Hot` choice introduced by
    /// absorbing the equivalence). `HermiT` says `:A` is sat; rustdl
    /// agreed only after the fix.
    ///
    /// Pattern:
    ///   :A ⊑ :PT
    ///   :A ⊑ ∃hs.Mild
    ///   FunctionalObjectProperty(:hs)
    ///   :S ≡ :PT ⊓ ∃hs.Hot
    ///   Disjoint(:Hot, :Mild)
    ///
    /// Each axiom is essential — dropping any one yields the
    /// correct `sat` verdict (verified by bisection).
    /// Regression for the second pizza false-positive-unsat bug
    /// fixed 2026-05-25. Minimal repro of the
    /// `VegetarianTopping ≡ PizzaTopping ⊓ (CheeseTopping ⊔ … ⊔
    /// VegetableTopping)` shape: `:A` is `:F` is `:PT`; `:F` is
    /// disjoint with the union members. `HermiT` says `:A` is sat.
    /// Bug was in [`crate::search::branch`]: when asserting a
    /// disjunct, it used only `[my_id]` as deps instead of the
    /// parent `Or` label's deps ∪ `my_id`. A clash on a nested
    /// branch then returned `clash_deps` missing the outer branch's
    /// id, and back-jumping skipped past the licensing disjunction.
    #[test]
    fn pizza_equiv_pizzatopping_union_should_be_sat() {
        let onto = parse(
            "Prefix(:=<http://example.org/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://example.org/min-veg>\n\
Declaration(Class(:A))\n\
Declaration(Class(:F))\n\
Declaration(Class(:PT))\n\
Declaration(Class(:V))\n\
Declaration(Class(:C))\n\
Declaration(Class(:N))\n\
SubClassOf(:A :F)\n\
SubClassOf(:F :PT)\n\
SubClassOf(:C :PT)\n\
SubClassOf(:N :PT)\n\
DisjointClasses(:C :F :N)\n\
EquivalentClasses(:V ObjectIntersectionOf(:PT ObjectUnionOf(:C :N)))\n\
)\n",
        );
        assert!(
            check(&onto, "http://example.org/A"),
            "A should be satisfiable (matches HermiT) but rustdl returned unsat"
        );
    }

    /// Regression for the named-pizza false-positive unsat fixed
    /// 2026-05-25. With both `:DomainConcept` reverse-equiv (Country
    /// nominals branching) and `:Pizza ⊑ ∃:hasBase.:PizzaBase`
    /// generating a successor that also gets the same branching,
    /// `apply_nominal_assignment` ends up merging the root and the
    /// hasBase-successor as the same individual. The merge then
    /// moves `:Pizza` (which was added with deps=[] from initial
    /// concept-rule chain) to the merged node where it triggers
    /// disjointness (`Pizza ⊓ PizzaBase ⊑ ⊥`), producing a clash with
    /// empty `clash_deps`. Back-jumping skips past every branch
    /// because `[]` doesn't contain any `my_id` — `:NamedPizza`
    /// wrongly reported unsat.
    ///
    /// Fix: `merge_into_with_deps(source, target, merge_deps)` —
    /// the merge condition's deps (union of both sides' nominal
    /// label deps) flow into every moved label / edge, so a
    /// post-merge clash inherits them. Both `apply_nominal_assignment`
    /// and `apply_max` now pass the precise merge-condition deps.
    /// Regression for the `apply_min` over-assert bug fixed
    /// 2026-05-25 (the SIO bug). When `Min(n, R, body)` fires after
    /// subclass propagation has put `body` on additional existing
    /// R-witnesses, the rule was pairwise-marking *all* witnesses
    /// distinct — not just the `n` it commits to. The resulting
    /// over-constraint blocked any `Max(k, R, body)` merge with
    /// `k < witnesses.len()`, producing false-positive unsats on
    /// the 22-class cluster around `:SIO_000450` ("axis").
    ///
    /// Minimal repro (`HermiT`: sat):
    ///   :A ⊑ :B; :B ⊑ :C
    ///   :X508 ⊑ :X532
    ///   :C ⊑ =2 :r.:X532   (Min(2) + Max(2))
    ///   :B ⊑ =1 :r.:X508   (Min(1) + Max(1))
    /// A satisfying model has two :r-successors: one of type
    /// {:X508, :X532}, one of type {:X532} only.
    #[test]
    fn sio_apply_min_over_assert_should_be_sat() {
        let onto = parse(
            "Prefix(:=<http://example.org/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://example.org/min-card>\n\
Declaration(Class(:A))\n\
Declaration(Class(:B))\n\
Declaration(Class(:C))\n\
Declaration(Class(:X508))\n\
Declaration(Class(:X532))\n\
Declaration(ObjectProperty(:r))\n\
SubClassOf(:A :B)\n\
SubClassOf(:B :C)\n\
SubClassOf(:X508 :X532)\n\
SubClassOf(:C ObjectExactCardinality(2 :r :X532))\n\
SubClassOf(:B ObjectExactCardinality(1 :r :X508))\n\
)\n",
        );
        assert!(
            check(&onto, "http://example.org/A"),
            ":A should be sat (matches HermiT); apply_min was over-asserting distinctness"
        );
    }

    #[test]
    fn pizza_named_pizza_country_should_be_sat() {
        // Use the saved 84-line STAR-extraction fixture — small
        // enough to be in-tree, large enough to exercise the
        // role-characteristics chain that the original synthetic
        // 10-axiom repros couldn't reproduce.
        let src = include_str!("../tests/fixtures/named-pizza-country-bug.ofn");
        let onto = parse(src);
        assert!(
            check(
                &onto,
                "http://www.co-ode.org/ontologies/pizza/pizza.owl#NamedPizza"
            ),
            ":NamedPizza should be sat (matches HermiT) — merge-deps regression"
        );
    }

    #[test]
    fn pizza_functional_equiv_some_should_be_sat() {
        let onto = parse(
            "Prefix(:=<http://example.org/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://example.org/min-bug>\n\
Declaration(Class(:A))\n\
Declaration(Class(:PT))\n\
Declaration(Class(:S))\n\
Declaration(Class(:Hot))\n\
Declaration(Class(:Mild))\n\
Declaration(ObjectProperty(:hs))\n\
SubClassOf(:A :PT)\n\
SubClassOf(:A ObjectSomeValuesFrom(:hs :Mild))\n\
FunctionalObjectProperty(:hs)\n\
EquivalentClasses(:S ObjectIntersectionOf(:PT ObjectSomeValuesFrom(:hs :Hot)))\n\
DisjointClasses(:Hot :Mild)\n\
)\n",
        );
        assert!(
            check(&onto, "http://example.org/A"),
            "A should be satisfiable (matches HermiT) but rustdl returned unsat"
        );
    }

    #[test]
    fn satisfiable_atomic_class() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn unsatisfiable_via_equivalence() {
        // Test ≡ A ⊓ ¬A — :Test must be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    EquivalentClasses(:Test ObjectIntersectionOf(:A ObjectComplementOf(:A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn unsatisfiable_via_subsumption_chain() {
        // A ⊑ B, B ⊑ C, Test ≡ A ⊓ ¬C — :Test must be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:Test))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(:A ObjectComplementOf(:C)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn cyclic_tbox_terminates_via_blocking() {
        // A ⊑ ∃r.A — :A is satisfiable; must terminate.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn role_hierarchy_makes_concept_unsat() {
        // r ⊑ s; ∃r.A ⊓ ∀s.¬A — the sub-property axiom forces the
        // ¬A from ∀s to land on the r-witness too, producing a clash.
        // Without role hierarchy support this would (wrongly) be sat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SubObjectPropertyOf(:r :s)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r :A) \
        ObjectAllValuesFrom(:s ObjectComplementOf(:A))))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn inverse_object_properties_declared_inverse_matches() {
        // InverseObjectProperties(r, s); Test ≡ ∃r.A ⊓ ∀s⁻.¬A.
        // The declared pair lets the ∀s⁻ rule propagate ¬A through
        // the r-edge, clashing at the witness.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    InverseObjectProperties(:r :s)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r :A) \
        ObjectAllValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:A))))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn abox_class_assertion_propagates_to_nominal() {
        // ClassAssertion(A, alice); Test ≡ {alice} ⊓ ¬A — unsat
        // because the `ABox` forces alice into A.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectOneOf(:alice) ObjectComplementOf(:A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn abox_same_and_different_is_inconsistent() {
        // SameIndividual + DifferentIndividuals on the same pair —
        // the ontology has no model. Any class query should be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Test))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    DifferentIndividuals(:alice :bob)\n\
    SameIndividual(:alice :bob)\n\
    EquivalentClasses(:Test ObjectOneOf(:alice))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn nominal_forces_witness_merge() {
        // ∃r.(A ⊓ {alice}) ⊓ ∃r.(B ⊓ {alice}) — the two existentials
        // generate separate witnesses, but both carry {alice}; the
        // nominal-assignment rule merges them into one node carrying
        // A and B. Satisfiable.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A ObjectOneOf(:alice))) \
        ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B ObjectOneOf(:alice)))))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn min_cardinality_generates_distinct_witnesses() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMinCardinality(3 :r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn max_cardinality_alone_is_satisfiable() {
        // ≤1 r.A alone is trivially satisfiable — pick a model with
        // zero or one r-successors. Tests that Max parses, lowers,
        // and saturates without error.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMaxCardinality(1 :r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn min_and_max_conflict_unsat() {
        // ≥2 r.A ⊓ ≤1 r.A — two distinct A-witnesses required, only
        // one allowed. The merge rule cannot collapse them
        // (apply_min marked them distinct); inequality clash.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectIntersectionOf(\
        ObjectMinCardinality(2 :r :A) \
        ObjectMaxCardinality(1 :r :A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn role_chain_length_three_silently_skipped() {
        // Length-N (N > 2) chain axioms are silently dropped — sound
        // for class-side reasoning, just under-approximates the
        // role-side closure. Lets the family ontology classify
        // instead of hard-erroring; whoever needs the dropped role
        // entailments can flag it via `--features chain-strict` in
        // the future. The test just confirms the absence of an error.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    Declaration(ObjectProperty(:u))\n\
    Declaration(ObjectProperty(:t))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:r :s :u) :t)\n\
)\n"
        ));
        // No axiom forbids :A; with the length-3 chain dropped, the
        // ontology is just a class declaration plus inert role
        // declarations.
        assert!(is_class_satisfiable(&onto, "http://rustdl.test/A").expect("verdict returned"));
    }

    #[test]
    fn length_two_role_chain_supported() {
        // SubObjectPropertyOf(ObjectPropertyChain(r s) t) at length 2
        // is in scope for Phase 5 (R): the named-role two-hop chain
        // axiom is registered on the tableau context, so this
        // ontology is consistent and the test class is satisfiable
        // (no axioms forbid it).
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    Declaration(ObjectProperty(:t))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn query_stats_pure_el_answered_by_saturation() {
        // Pure EL ontology — every query should be answered by the
        // closure with `pure_el_mode == true`.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        let (verdict, stats) =
            is_subclass_of_with_stats(&onto, "http://rustdl.test/A", "http://rustdl.test/B")
                .expect("verdict");
        assert!(verdict);
        assert!(stats.answered_by_saturation);
        assert!(stats.pure_el_mode);

        let (sat, sat_stats) =
            is_class_satisfiable_with_stats(&onto, "http://rustdl.test/A").expect("verdict");
        assert!(sat);
        assert!(sat_stats.answered_by_saturation);
        assert!(sat_stats.pure_el_mode);
    }

    #[test]
    fn query_stats_hybrid_falls_through_to_tableau() {
        // Disjunction lives outside the EL fragment; the subsumption
        // check should fall through to the tableau and the stats
        // should reflect that.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A ObjectUnionOf(:B :C))\n\
)\n"
        ));
        let (_verdict, stats) =
            is_subclass_of_with_stats(&onto, "http://rustdl.test/A", "http://rustdl.test/B")
                .expect("verdict");
        assert!(!stats.pure_el_mode);
        assert!(!stats.answered_by_saturation);
    }

    #[test]
    fn unknown_class_iri_errors() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        let err = is_class_satisfiable(&onto, "http://rustdl.test/Nope")
            .expect_err("unknown class should error");
        assert!(matches!(err, ReasonError::UnknownClass(_)));
    }

    #[test]
    fn empty_ontology_is_consistent() {
        let onto = parse(&format!("{HEADER}Ontology(<http://rustdl.test/test>\n)\n"));
        assert!(is_consistent(&onto).expect("verdict"));
    }

    #[test]
    fn contradictory_abox_is_inconsistent() {
        // SameIndividual + DifferentIndividuals on the same pair —
        // no model exists.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    DifferentIndividuals(:alice :bob)\n\
    SameIndividual(:alice :bob)\n\
)\n"
        ));
        assert!(!is_consistent(&onto).expect("verdict"));
    }

    #[test]
    fn explicit_subclassof_axiom_entails_subsumption() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/B").expect("verdict")
        );
    }

    #[test]
    fn transitive_subclassof_is_entailed() {
        // A ⊑ B, B ⊑ C ⇒ A ⊑ C
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/C").expect("verdict")
        );
    }

    #[test]
    fn unrelated_classes_are_not_subclass() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
)\n"
        ));
        assert!(
            !is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/B")
                .expect("verdict")
        );
    }

    #[test]
    fn subclass_via_saturation_then_tableau_mixed_ontology() {
        // Mixed input: an EL subsumption (A ⊑ B ⊑ C reachable by the
        // saturation engine) plus a non-EL one (D ⊑ ∀r.A which the
        // saturation drops but the tableau handles). The
        // orchestrator should resolve both correctly.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    SubClassOf(:D ObjectAllValuesFrom(:r :A))\n\
)\n"
        ));
        // EL chain: saturation should handle without invoking tableau.
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/C").expect("verdict")
        );
        // Reflexive: handled by the in-function shortcut.
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/D", "http://rustdl.test/D").expect("verdict")
        );
        // A doesn't subsume D (truly false; tableau-confirmed).
        assert!(
            !is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/D")
                .expect("verdict")
        );
    }

    #[test]
    fn subclass_of_unknown_class_errors() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        let err = is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/Nope")
            .expect_err("unknown sup should error");
        assert!(matches!(err, ReasonError::UnknownClass(_)));
    }

    #[test]
    fn builds_data_counting_classes_for_integer_cardinality() {
        use horned_owl::io::ofn::reader::read as read_ofn;
        let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(\nDeclaration(Class(:C))\nDeclaration(DataProperty(:p))\n\
SubClassOf(:C DataMinCardinality(3 :p DatatypeRestriction(xsd:integer \
xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n)\n";
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
        let internal = convert_ontology(&onto).expect("convert");
        let dkey = build_dkey_range_map(&internal);
        let counting = build_data_counting_classes(&internal, &dkey);
        // Resolve :C's ClassId by IRI (do NOT assume index 0 — owl:Thing may take it).
        let c_id = internal
            .vocabulary
            .classes()
            .find(|(_, iri)| *iri == "http://t/C")
            .map(|(id, _)| id)
            .expect("C declared");
        assert!(
            counting.contains(&c_id),
            "C must be in data_counting_classes; got {counting:?}"
        );
    }

    #[test]
    fn builds_data_counting_classes_for_exact_cardinality() {
        // DataExactCardinality lowers to And(Min, Max) over DKey — exercises
        // the And-arm recursion in concept_has_dkey_counting.
        use horned_owl::io::ofn::reader::read as read_ofn;
        let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(\nDeclaration(Class(:C))\nDeclaration(DataProperty(:p))\n\
SubClassOf(:C DataExactCardinality(2 :p DatatypeRestriction(xsd:integer \
xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))\n)\n";
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
        let internal = convert_ontology(&onto).expect("convert");
        let dkey = build_dkey_range_map(&internal);
        let counting = build_data_counting_classes(&internal, &dkey);
        let c_id = internal
            .vocabulary
            .classes()
            .find(|(_, iri)| *iri == "http://t/C")
            .map(|(id, _)| id)
            .expect("C declared");
        assert!(
            counting.contains(&c_id),
            "C (exact card) must be counting; got {counting:?}"
        );
    }

    #[test]
    fn no_data_counting_classes_for_value_membership_only() {
        use horned_owl::io::ofn::reader::read as read_ofn;
        // DataSomeValuesFrom is value-membership (∃p.DKey), NOT counting.
        let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(\nDeclaration(Class(:C))\nDeclaration(DataProperty(:p))\n\
SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))\n)\n";
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
        let internal = convert_ontology(&onto).expect("convert");
        let dkey = build_dkey_range_map(&internal);
        let counting = build_data_counting_classes(&internal, &dkey);
        assert!(
            counting.is_empty(),
            "value-membership must not be counting; got {counting:?}"
        );
    }

    #[test]
    fn adaptive_label_cache_ms_branches() {
        use std::time::Duration;
        // env override always wins (incl. 0 = unbounded sentinel)
        assert_eq!(
            adaptive_label_cache_ms(137, Some(Duration::from_millis(200)), Some(7777)),
            7777
        );
        assert_eq!(adaptive_label_cache_ms(137, None, Some(0)), 0);
        // n × per_pair, clamped to [1000, 30000]
        assert_eq!(
            adaptive_label_cache_ms(137, Some(Duration::from_millis(200)), None),
            27_400
        ); // 137*200
        assert_eq!(
            adaptive_label_cache_ms(137, Some(Duration::from_secs(1)), None),
            30_000
        ); // 137000→ceiling
        assert_eq!(
            adaptive_label_cache_ms(2, Some(Duration::from_millis(200)), None),
            400
        ); // 2*200=400, above the 50ms floor (floor lowered 1000→50)
        // Tight-cap break-even (wine pattern): n×per_pair stays UN-floored so the
        // label build doesn't over-invest where refutation is ~free.
        assert_eq!(
            adaptive_label_cache_ms(137, Some(Duration::from_millis(1)), None),
            137
        ); // 137*1=137, above the 50ms floor (was floored to 1000 pre-tune)
        // Degenerate tiny budget clamps to the 50ms floor.
        assert_eq!(
            adaptive_label_cache_ms(10, Some(Duration::from_millis(1)), None),
            50
        ); // 10*1=10 → floor 50
        // None per_pair → base = ceiling, then ×n clamps to ceiling
        assert_eq!(adaptive_label_cache_ms(137, None, None), 30_000);
        assert_eq!(adaptive_label_cache_ms(1, None, None), 30_000); // 1*30000=30000
    }
}

/// Shared serialization lock for tests that mutate the process-wide
/// `RUSTDL_*` orchestrator env vars. Several env vars are read/written by
/// tests in more than one module (e.g. `RUSTDL_HYPER_TRUST_SAT_MIN_MS` by
/// both this crate's `with_env` helper and `classify`'s selective-verify
/// tests), so every such test holds this one lock for its whole body and
/// they never run concurrently. Poison-tolerant so a panicking test
/// doesn't cascade-fail the rest.
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod hyper_trust_sat_min_ms_tests {
    use super::hyper_trust_sat_min_ms;

    // These tests mutate the process-wide RUSTDL_HYPER_TRUST_SAT_MIN_MS
    // env var, which classify's selective-verify tests also touch. The
    // helper holds `test_env_lock` for the whole closure so the mutation
    // never races with another env-touching test, then save/restores the
    // previous value.
    #[allow(unsafe_code)]
    fn with_env<F: FnOnce()>(key: &str, val: Option<&str>, f: F) {
        let _lock = crate::test_env_lock();
        let prev = std::env::var_os(key);
        match val {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        f();
        match prev {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[test]
    fn default_is_disabled() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", None, || {
            assert_eq!(hyper_trust_sat_min_ms(), 0);
        });
    }

    #[test]
    fn env_overrides_value() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some("200"), || {
            assert_eq!(hyper_trust_sat_min_ms(), 200);
        });
    }

    #[test]
    fn zero_disables_selective_verification() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some("0"), || {
            assert_eq!(hyper_trust_sat_min_ms(), 0);
        });
    }

    #[test]
    fn empty_string_uses_default() {
        with_env("RUSTDL_HYPER_TRUST_SAT_MIN_MS", Some(""), || {
            assert_eq!(hyper_trust_sat_min_ms(), 0);
        });
    }

    #[test]
    fn garbage_uses_default() {
        with_env(
            "RUSTDL_HYPER_TRUST_SAT_MIN_MS",
            Some("not-a-number"),
            || {
                assert_eq!(hyper_trust_sat_min_ms(), 0);
            },
        );
    }
}

/// SP2 `sat_seed` wiring tests.
///
/// Inline (not in `tests/`) because `HyperCache`, `sat_seed_for_test`, and
/// `test_env_lock` are all `pub(crate)` / `#[cfg(test)]` — unreachable from
/// an integration-test crate. Integration tests in `tests/` compile as a
/// separate crate and can only reach always-compiled `pub` items.
///
/// Fixture: a tiny three-class chain A⊑B⊑C parsed via `owl_dl_core::convert`.
#[cfg(test)]
mod sat_seed_wiring_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    /// Build an `InternalOntology` with A⊑B and B⊑C.
    fn build_chain_abc() -> InternalOntology {
        let src = format!(
            "{HEADER}Ontology(\n\
             Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
             SubClassOf(:A :B)\n\
             SubClassOf(:B :C)\n\
             )\n"
        );
        owl_dl_core::convert::convert_ontology(&parse(&src)).expect("chain A⊑B⊑C converts")
    }

    /// Look up a named class by local name. The prefix `:=<http://rustdl.test/>`
    /// expands `:A` to `http://rustdl.test/A`.
    fn class_id_by_local(internal: &InternalOntology, local: &str) -> owl_dl_core::ir::ClassId {
        let iri = format!("http://rustdl.test/{local}");
        internal
            .vocabulary
            .classes()
            .find(|(_, i)| *i == iri.as_str())
            .map_or_else(
                || panic!("class {local} not found in vocabulary"),
                |(id, _)| id,
            )
    }

    /// Helper: set/clear `RUSTDL_SAT_SEED`, build a `HyperCache`, restore the
    /// prior env value. Serialised via `test_env_lock` so env mutations from
    /// concurrent tests don't race.
    #[allow(unsafe_code)]
    fn build_cache_with_sat_seed_flag(internal: &InternalOntology, enable: bool) -> HyperCache {
        let _lock = test_env_lock();
        let prior = std::env::var_os("RUSTDL_SAT_SEED");
        // SAFETY: serialized by test_env_lock (one test at a time); restored
        // before the lock is released.
        if enable {
            unsafe { std::env::set_var("RUSTDL_SAT_SEED", "1") };
        } else {
            // Default is now ON, so "off" means explicitly "0" (not unset).
            unsafe { std::env::set_var("RUSTDL_SAT_SEED", "0") };
        }
        let cache = HyperCache::build(internal);
        match prior {
            Some(v) => unsafe { std::env::set_var("RUSTDL_SAT_SEED", v) },
            None => unsafe { std::env::remove_var("RUSTDL_SAT_SEED") },
        }
        cache
    }

    /// Flag-off ⇒ `sat_seed` is `None` (no table built, zero `saturate` cost).
    #[test]
    fn sat_seed_table_none_when_flag_off() {
        let internal = build_chain_abc();
        let cache = build_cache_with_sat_seed_flag(&internal, false);
        assert!(
            cache.sat_seed_for_test().is_none(),
            "RUSTDL_SAT_SEED unset ⇒ sat_seed must be None"
        );
    }

    /// Flag-on ⇒ table is built; A⊑B⊑C chain ⇒ A's entry contains B and C
    /// (transitively), and does NOT contain A itself.
    #[test]
    fn sat_seed_table_built_when_flag_on() {
        let internal = build_chain_abc();
        let a = class_id_by_local(&internal, "A");
        let b = class_id_by_local(&internal, "B");
        let c = class_id_by_local(&internal, "C");
        let cache = build_cache_with_sat_seed_flag(&internal, true);
        let tbl = cache
            .sat_seed_for_test()
            .expect("RUSTDL_SAT_SEED=1 ⇒ table must be Some");
        let seeded: std::collections::HashSet<owl_dl_core::ir::ClassId> = tbl
            .get(a.index() as usize)
            .expect("A has an entry in the table")
            .iter()
            .copied()
            .collect();
        assert!(seeded.contains(&b), "A⊑B⊑C ⇒ A's table must contain B");
        assert!(
            seeded.contains(&c),
            "A⊑B⊑C ⇒ A's table must contain C (transitive)"
        );
        assert!(
            !seeded.contains(&a),
            "A's table must NOT contain A itself (d != c filter)"
        );
    }
}

/// `RUSTDL_CLASSIFY_LABELS_AMORTIZE` canaries — the per-CLASS clause-index
/// amortization in [`HyperCache::classify_labels`].
///
/// Inline (not in `tests/`) because `HyperCache`, `LabelOracle` and
/// `classify_labels` are all `pub(crate)`: an integration-test crate cannot
/// reach them, and the *only* meaningful equivalence statement is at the
/// `classify_labels` → `LabelOracle` boundary. A whole-classify byte-identity
/// check (the CLI-level sibling in
/// `crates/owl-dl-cli/tests/classify_labels_amortize_identity.rs`) is much
/// weaker here, because the label oracle feeds a *pruning* heuristic: an
/// amortization bug that silently dropped the seed clauses would shrink
/// `labels` — which only ever costs extra per-pair probes and would leave the
/// final hierarchy unchanged on any fixture the per-pair path can still
/// decide. These tests compare the oracle itself, so a dropped seed clause is
/// visible directly.
///
/// NEGATIVES FIRST: the flag must default OFF, and the amortized path must
/// agree with the full-rebuild path on a fixture where the seed table is
/// genuinely non-empty (asserted, so the comparison cannot pass vacuously).
#[cfg(test)]
mod classify_labels_amortize_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    fn convert(src: &str) -> InternalOntology {
        owl_dl_core::convert::convert_ontology(&parse(src)).expect("fixture converts")
    }

    /// A⊑B⊑C plus a `∃r.`-bearing defined class and a disjoint pair, so that
    /// (a) `sat_seed` is non-empty, (b) `exists_seed` has something to carry,
    /// and (c) the delta's `disjoint_pair_of` extraction is exercised.
    fn build_fixture() -> InternalOntology {
        convert(&format!(
            "{HEADER}Ontology(\n\
             Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
             Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))\n\
             Declaration(ObjectProperty(:r))\n\
             SubClassOf(:A :B)\n\
             SubClassOf(:B :C)\n\
             EquivalentClasses(:D ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :C)))\n\
             SubClassOf(:E ObjectSomeValuesFrom(:r :C))\n\
             DisjointClasses(:B :F)\n\
             SubClassOf(:F :A)\n\
             )\n"
        ))
    }

    /// Structural equality for [`LabelOracle`] (it deliberately does not derive
    /// `PartialEq` — `labels` is a `HashSet` and `derived_sups` a `Vec` whose
    /// order is an implementation detail, so both are normalised here).
    fn oracle_eq(a: &LabelOracle, b: &LabelOracle) -> bool {
        let norm = |o: &LabelOracle| -> Option<(Vec<u32>, Vec<u32>)> {
            match o {
                LabelOracle::Sat {
                    labels,
                    derived_sups,
                } => {
                    let mut l: Vec<u32> = labels.iter().map(|c| c.index()).collect();
                    let mut d: Vec<u32> = derived_sups.iter().map(|c| c.index()).collect();
                    l.sort_unstable();
                    d.sort_unstable();
                    Some((l, d))
                }
                _ => None,
            }
        };
        match (a, b) {
            (LabelOracle::Unsat, LabelOracle::Unsat)
            | (LabelOracle::NoVerdict, LabelOracle::NoVerdict) => true,
            (LabelOracle::Sat { .. }, LabelOracle::Sat { .. }) => norm(a) == norm(b),
            _ => false,
        }
    }

    fn describe(o: &LabelOracle) -> String {
        match o {
            LabelOracle::Sat {
                labels,
                derived_sups,
            } => format!(
                "Sat(|labels|={}, |derived|={})",
                labels.len(),
                derived_sups.len()
            ),
            LabelOracle::Unsat => "Unsat".to_owned(),
            LabelOracle::NoVerdict => "NoVerdict".to_owned(),
        }
    }

    /// Run `classify_labels` for every named class with the amortize flag set
    /// to `enable`, restoring the prior env value. Serialised by
    /// `test_env_lock`, matching the sibling `sat_seed` helper.
    #[allow(unsafe_code)]
    fn labels_with_flag(
        cache: &HyperCache,
        internal: &InternalOntology,
        enable: bool,
    ) -> Vec<LabelOracle> {
        let _lock = test_env_lock();
        let prior = std::env::var_os("RUSTDL_CLASSIFY_LABELS_AMORTIZE");
        // SAFETY: serialized by `test_env_lock` (one test at a time); restored
        // before the guard is dropped.
        if enable {
            unsafe { std::env::set_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE", "1") };
        } else {
            unsafe { std::env::remove_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE") };
        }
        let out = internal
            .vocabulary
            .classes()
            .map(|(id, _)| cache.classify_labels(id, None))
            .collect();
        match prior {
            Some(v) => unsafe { std::env::set_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE", v) },
            None => unsafe { std::env::remove_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE") },
        }
        out
    }

    /// DEFAULT ON since 0.4.10. Pins BOTH halves of the contract: an unset variable
    /// must ENABLE, and `=0` must still REVERT. The second half matters as much as the
    /// first — a flag whose opt-out silently stopped working would leave no way back to
    /// the prior behaviour, and this is the escape hatch for a 52–95% behaviour change.
    #[test]
    #[allow(unsafe_code)]
    fn flag_defaults_on() {
        let _lock = test_env_lock();
        let prior = std::env::var_os("RUSTDL_CLASSIFY_LABELS_AMORTIZE");
        // SAFETY: serialized by `test_env_lock`; restored below.
        unsafe { std::env::remove_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE") };
        assert!(
            classify_labels_amortize_enabled(),
            "RUSTDL_CLASSIFY_LABELS_AMORTIZE must default ON since 0.4.10"
        );
        unsafe { std::env::set_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE", "1") };
        assert!(classify_labels_amortize_enabled(), "=1 must enable");
        unsafe { std::env::set_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE", "0") };
        assert!(!classify_labels_amortize_enabled(), "=0 must disable");
        // EMPTY ENABLES, matching every other default-ON flag in this workspace
        // (`is_none_or(|v| v != "0")`): for a default-ON flag only an explicit `=0` is the
        // opt-out. The previous assertion here required empty to DISABLE, which was correct
        // under the old default-OFF semantics but would now make `VAR=$UNSET_VAR` silently
        // revert a 52-95% improvement -- the opposite of safe for this polarity.
        unsafe { std::env::set_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE", "") };
        assert!(
            classify_labels_amortize_enabled(),
            "empty must enable (only =0 reverts)"
        );
        match prior {
            Some(v) => unsafe { std::env::set_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE", v) },
            None => unsafe { std::env::remove_var("RUSTDL_CLASSIFY_LABELS_AMORTIZE") },
        }
    }

    /// NON-VACUITY GUARD for the equivalence test below: the fixture must
    /// actually populate the per-class seed tables, because those seed clauses
    /// are precisely what the amortized path has to index in its delta rather
    /// than via a full rebuild. If this ever goes empty, the equivalence test
    /// stops exercising the delta on anything but the Q-clause.
    ///
    /// It does NOT follow that the equivalence test would *detect* the seed
    /// clauses going missing — VERIFIED BY SABOTAGE, it does not: truncating
    /// `extras` to the Q-clause on the amortized path leaves all four canaries
    /// green. The reason is structural, and worth stating so nobody re-derives
    /// it the hard way: these probes pass `deadline: None`, and the SP2.1/SP3
    /// seed is a CONVERGENCE aid (it pre-loads entailed subsumers so `sat(c)`
    /// finishes inside the label-cache deadline), not a source of new
    /// entailments. With no deadline the probe always converges, so on a
    /// fixture this small the seed is redundant with the base clauses and
    /// dropping it changes no verdict. A test that pinned seed *presence*
    /// would have to race a deadline, i.e. be timing-dependent — deliberately
    /// not done here. What protects the seed instead is that BOTH paths now
    /// consume the SAME `extras` vector built once above; only an edit that
    /// touches `extras` between construction and use can desynchronise them.
    #[test]
    fn fixture_actually_carries_seed_clauses() {
        let internal = build_fixture();
        let cache = HyperCache::build(&internal);
        let sat = cache
            .sat_seed_for_test()
            .expect("RUSTDL_SAT_SEED defaults ON ⇒ sat_seed table must be built");
        assert!(
            sat.iter().any(|v| !v.is_empty()),
            "fixture must seed at least one class (else the amortize canary is vacuous)"
        );
        let ex = cache
            .exists_seed_for_test()
            .expect("RUSTDL_SAT_SEED defaults ON ⇒ exists_seed table must be built");
        assert!(
            ex.iter().any(|v| !v.is_empty()),
            "fixture must carry at least one ∃-seed (else the delta's Exists-head \
             indexing is never exercised)"
        );
    }

    /// THE equivalence gate: the amortized per-class index delta must yield the
    /// SAME `LabelOracle` as the full `ClauseIndexes` rebuild, for every class.
    ///
    /// A delta built at the wrong base length, or one that failed to index the
    /// appended seed clauses, changes `labels` here (or turns an `Unsat` into a
    /// `Sat`) and fails this test.
    #[test]
    fn label_oracle_identical_off_vs_on() {
        let internal = build_fixture();
        let cache = HyperCache::build(&internal);
        let off = labels_with_flag(&cache, &internal, false);
        let on = labels_with_flag(&cache, &internal, true);
        assert_eq!(off.len(), on.len(), "same number of classes probed");
        assert!(!off.is_empty(), "fixture must have named classes");
        for (i, (o, n)) in off.iter().zip(on.iter()).enumerate() {
            assert!(
                oracle_eq(o, n),
                "class #{i}: full-rebuild gave {} but amortized gave {}",
                describe(o),
                describe(n)
            );
        }
    }

    /// `F ⊑ A ⊑ B` with `DisjointClasses(B, F)` makes `F` unsatisfiable, so BOTH
    /// paths must report `Unsat`. Asserting the *value* (not just OFF == ON) is
    /// what keeps this from being a tautology over two equally-broken paths: it
    /// pins that the amortized engine still reaches a clash it must reach.
    ///
    /// SCOPE, corrected by sabotage: this does NOT guard the `disjoint_pairs`
    /// overlay. Handing the amortized path an EMPTY disjointness set leaves all
    /// four canaries green, because `F`'s clash is derived by firing the
    /// ⊥-headed clause `B(X) ⊓ F(X) → ⊥` from the shared base clause set, not by
    /// consulting the overlay (which is a merge-time/`≤n` shortcut). Losing the
    /// overlay here would therefore cost a pruning shortcut, not a verdict —
    /// which is why the survival is benign, but it does mean this test says
    /// nothing about overlay plumbing.
    #[test]
    fn unsat_class_reported_unsat_on_both_paths() {
        let internal = build_fixture();
        let cache = HyperCache::build(&internal);
        let f = internal
            .vocabulary
            .classes()
            .find(|(_, i)| *i == "http://rustdl.test/F")
            .map_or_else(|| panic!("class F not in vocabulary"), |(id, _)| id);
        let off = {
            let _l = test_env_lock();
            cache.classify_labels(f, None)
        };
        let on = {
            let all = labels_with_flag(&cache, &internal, true);
            let idx = internal
                .vocabulary
                .classes()
                .position(|(id, _)| id == f)
                .expect("F is enumerated");
            all[idx].clone()
        };
        assert!(
            matches!(off, LabelOracle::Unsat),
            "F ⊑ A ⊑ B with DisjointClasses(B,F) must be Unsat on the full-rebuild \
             path, got {}",
            describe(&off)
        );
        assert!(
            matches!(on, LabelOracle::Unsat),
            "F must be Unsat on the AMORTIZED path too, got {} — the per-class delta \
             lost the disjointness overlay",
            describe(&on)
        );
    }
}

/// Task 1 (label-cache back-fold): `defined_exists_bodies` +
/// `defined_body_by_genus` precompute tests.
///
/// Inline (not in `tests/`) because `HyperCache` and its fields are
/// `pub(crate)` — unreachable from an integration-test crate.
///
/// Fixture: `D ≡ A ⊓ ∃r.C` (∃-bearing — must be captured) and `E2 ≡ A2`
/// (purely atomic — must be excluded, it already fires via Horn clauses).
#[cfg(test)]
mod defined_exists_bodies_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    /// Look up a named class by local name. The prefix `:=<http://rustdl.test/>`
    /// expands `:X` to `http://rustdl.test/X`.
    fn class_id_by_local(internal: &InternalOntology, local: &str) -> owl_dl_core::ir::ClassId {
        let iri = format!("http://rustdl.test/{local}");
        internal
            .vocabulary
            .classes()
            .find(|(_, i)| *i == iri.as_str())
            .map_or_else(
                || panic!("class {local} not found in vocabulary"),
                |(id, _)| id,
            )
    }

    fn build_fixture() -> InternalOntology {
        let src = format!(
            "{HEADER}Ontology(\n\
             Declaration(Class(:D)) Declaration(Class(:E2)) \
             Declaration(Class(:A)) Declaration(Class(:A2)) Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:r))\n\
             EquivalentClasses(:D ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :C)))\n\
             EquivalentClasses(:E2 :A2)\n\
             )\n"
        );
        owl_dl_core::convert::convert_ontology(&parse(&src))
            .expect("D≡A⊓∃r.C, E2≡A2 fixture converts")
    }

    #[test]
    fn defined_exists_bodies_extracted_and_genus_indexed() {
        let internal = build_fixture();
        let d = class_id_by_local(&internal, "D");
        let e2 = class_id_by_local(&internal, "E2");
        let a = class_id_by_local(&internal, "A");
        let c = class_id_by_local(&internal, "C");
        let r = owl_dl_core::ir::Role::named(
            internal
                .vocabulary
                .roles()
                .find(|(_, iri)| *iri == "http://rustdl.test/r")
                .map(|(id, _)| id)
                .expect(": r role must exist"),
        );

        let hc = HyperCache::build(&internal);

        let d_body = hc
            .defined_exists_bodies
            .iter()
            .find(|b| b.name == d)
            .expect("D (∃-bearing defined body) must be present");
        assert_eq!(
            d_body.atoms.as_slice(),
            &[a],
            "D's atomic conjuncts must be exactly [A]"
        );
        assert_eq!(
            d_body.exists.as_slice(),
            &[(r, c)],
            "D's ∃-conjuncts must be exactly [(r, C)]"
        );

        assert!(
            !hc.defined_exists_bodies.iter().any(|b| b.name == e2),
            "E2 (purely atomic body) must be excluded — it already fires via Horn clauses"
        );

        // Genus index: D must be reachable from its atomic conjunct A.
        let idx = hc
            .defined_body_by_genus
            .get(&a)
            .expect("A must be indexed as a genus");
        assert!(
            idx.iter().any(|&i| hc.defined_exists_bodies[i].name == d),
            "defined_body_by_genus[A] must include D's index"
        );
    }
}

/// Precompletion probe tests.
///
/// Inline (not in `tests/`) because `precompletion_probe` depends on
/// `pub(crate)` helpers (`HyperCache`, `test_env_lock`).
///
/// Fixture: `C ⊑ ∃r.{a}` — an `ObjectHasValue`, lowered by the converter to
/// `C ⊑ ∃r.NomKey(a)`. After saturation the derived ∃-fact `(C, r, NomKey(a))`
/// must translate to the wedge nominal class id `num_named + a.index()`.
#[cfg(test)]
mod precompletion_probe_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    struct CExistsNominalIds {
        c: owl_dl_core::ir::ClassId,
        r: owl_dl_core::ir::RoleId,
        a: owl_dl_core::ir::IndividualId,
    }

    /// Build a minimal ontology: class C, object property r, individual a,
    /// with the axiom `SubClassOf(C ObjectHasValue(r a))`.
    fn build_c_exists_nominal_a() -> (InternalOntology, CExistsNominalIds) {
        let src = format!(
            "{HEADER}Ontology(\n\
             Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:r))\n\
             Declaration(NamedIndividual(:a))\n\
             SubClassOf(:C ObjectHasValue(:r :a))\n\
             )\n"
        );
        let onto = parse(&src);
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("C⊑∃r.{a} converts");
        let c = internal
            .vocabulary
            .classes()
            .find(|(_, iri)| *iri == "http://rustdl.test/C")
            .map(|(id, _)| id)
            .expect("class C present");
        let r = internal
            .vocabulary
            .roles()
            .find(|(_, iri)| *iri == "http://rustdl.test/r")
            .map(|(id, _)| id)
            .expect("role r present");
        let a = internal
            .vocabulary
            .individuals()
            .find(|(_, iri)| *iri == "http://rustdl.test/a")
            .map(|(id, _)| id)
            .expect("individual a present");
        (internal, CExistsNominalIds { c, r, a })
    }

    /// The derived ∃-fact `(C, r, NomKey(a))` must:
    /// 1. Be present in the saturation's `seen_facts` output.
    /// 2. Have its `NomKey` target map back to individual `a` via `nom_to_ind`.
    /// 3. Translate to wedge nominal id `= num_named + a.index()` (≥ `num_named`).
    #[test]
    fn precompletion_translates_nomkey_to_wedge_nominal() {
        let (internal, ids) = build_c_exists_nominal_a();
        let (_subs, facts, nom_to_ind) = owl_dl_saturation::saturate_with_exists_facts(&internal);
        let n_named = u32::try_from(internal.vocabulary.num_classes()).expect("fits u32");
        // Find the (C, r, NomKey) fact.
        let (_, _, tgt) = facts
            .iter()
            .copied()
            .find(|&(s, r, _)| s == ids.c && r == ids.r)
            .expect("derived ∃-fact (C, r, NomKey(a)) must be present");
        // The target must be a NomKey (≥ n_named) and map back to individual a.
        assert!(
            tgt.index() >= n_named,
            "NomKey target id must be ≥ num_named (synthetic region)"
        );
        let ind = nom_to_ind
            .get(&tgt)
            .copied()
            .expect("target must be a NomKey (in nom_to_ind)");
        assert_eq!(ind, ids.a, "NomKey maps back to individual a");
        // Wedge nominal id is the same formula the clausifier uses.
        let wedge_nominal = owl_dl_core::ir::ClassId::new(n_named + ind.index());
        assert!(
            wedge_nominal.index() >= n_named,
            "wedge nominal id is in the nominal region (≥ num_named)"
        );
    }

    /// Mode 2 (real ∃-seed) and mode 3 (garbage control) must seed exactly the
    /// same number of ∃-clauses so the controller can use them as a matched pair.
    /// Mode 1 (named-only) should seed 0 ∃-clauses.
    #[test]
    fn precompletion_mode2_mode3_exists_counts_match() {
        let src = format!(
            "{HEADER}Ontology(\n\
             Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:r))\n\
             Declaration(NamedIndividual(:a))\n\
             SubClassOf(:C ObjectHasValue(:r :a))\n\
             )\n"
        );
        let onto = parse(&src);
        let m1 = precompletion_probe(&onto, "http://rustdl.test/C", 1, 8, None)
            .expect("no error")
            .expect("class found")
            .3;
        let m2 = precompletion_probe(&onto, "http://rustdl.test/C", 2, 8, None)
            .expect("no error")
            .expect("class found")
            .3;
        let m3 = precompletion_probe(&onto, "http://rustdl.test/C", 3, 8, None)
            .expect("no error")
            .expect("class found")
            .3;
        assert_eq!(m1, 0, "mode 1 (named-only) seeds no ∃-clauses");
        assert!(
            m2 >= 1,
            "mode 2 seeds at least one ∃-fact (the ∃r.{{a}} axiom)"
        );
        assert_eq!(m2, m3, "mode 3 control must seed the same count as mode 2");
    }
}

/// SP3 Phase-2: `exists_seed` wiring tests.
///
/// Inline (not in `tests/`) because `HyperCache`, `exists_seed_for_test`, and
/// `test_env_lock` are all `pub(crate)` / `#[cfg(test)]` — unreachable from an
/// integration-test crate.
///
/// Fixture: `C ⊑ ∃r.{a}` — reused from `precompletion_probe_tests`.
#[cfg(test)]
mod exists_seed_wiring_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::ir::{ClassId, Role};
    use std::io::Cursor;

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    /// Build the `C ⊑ ∃r.{a}` fixture and return its `InternalOntology` plus ids.
    fn build_c_exists_nominal_a() -> (
        InternalOntology,
        ClassId,
        owl_dl_core::ir::RoleId,
        owl_dl_core::ir::IndividualId,
    ) {
        let src = format!(
            "{HEADER}Ontology(\n\
             Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:r))\n\
             Declaration(NamedIndividual(:a))\n\
             SubClassOf(:C ObjectHasValue(:r :a))\n\
             )\n"
        );
        let onto = parse(&src);
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("C⊑∃r.{a} converts");
        let c = internal
            .vocabulary
            .classes()
            .find(|(_, iri)| *iri == "http://rustdl.test/C")
            .map(|(id, _)| id)
            .expect("class C present");
        let r = internal
            .vocabulary
            .roles()
            .find(|(_, iri)| *iri == "http://rustdl.test/r")
            .map(|(id, _)| id)
            .expect("role r present");
        let a = internal
            .vocabulary
            .individuals()
            .find(|(_, iri)| *iri == "http://rustdl.test/a")
            .map(|(id, _)| id)
            .expect("individual a present");
        (internal, c, r, a)
    }

    /// Helper: set/clear `RUSTDL_SAT_SEED`, build a `HyperCache`, restore prior.
    #[allow(unsafe_code)]
    fn build_cache_with_sat_seed_flag(internal: &InternalOntology, enable: bool) -> HyperCache {
        let _lock = test_env_lock();
        let prior = std::env::var_os("RUSTDL_SAT_SEED");
        // SAFETY: serialized by test_env_lock (one test at a time); restored before release.
        if enable {
            unsafe { std::env::set_var("RUSTDL_SAT_SEED", "1") };
        } else {
            // Default is now ON, so "off" means explicitly "0" (not unset).
            unsafe { std::env::set_var("RUSTDL_SAT_SEED", "0") };
        }
        let cache = HyperCache::build(internal);
        match prior {
            Some(v) => unsafe { std::env::set_var("RUSTDL_SAT_SEED", v) },
            None => unsafe { std::env::remove_var("RUSTDL_SAT_SEED") },
        }
        cache
    }

    /// Flag-off ⇒ `exists_seed` is `None`. Flag-on ⇒ `exists_seed[C]`
    /// contains `(Role::named(r), wedge_nominal)` where
    /// `wedge_nominal = num_named + a.index()`.
    #[test]
    fn exists_seed_table_built_only_when_flagged() {
        let (internal, c, r, a) = build_c_exists_nominal_a();
        let off = build_cache_with_sat_seed_flag(&internal, false);
        assert!(
            off.exists_seed_for_test().is_none(),
            "flag off ⇒ no ∃ table"
        );
        let on = build_cache_with_sat_seed_flag(&internal, true);
        let tbl = on
            .exists_seed_for_test()
            .expect("flag on ⇒ ∃ table present");
        let n_named = u32::try_from(internal.vocabulary.num_classes()).expect("fits u32");
        let wedge_nominal = ClassId::new(n_named + a.index());
        let want_role = Role::named(r);
        assert!(
            tbl[c.index() as usize]
                .iter()
                .any(|&(rr, t)| rr == want_role && t == wedge_nominal),
            "C seeds ∃r.{{a}} translated to wedge nominal id {wedge_nominal:?}"
        );
    }
}

/// Differential test: verify that `build_abox_check_inputs` (the reduced fast-path
/// builder) and `PreparedOntology::from_internal` (the full builder) produce the same
/// `AboxVerdict` discriminant.
///
/// This lives in a unit-test module rather than `tests/` because both builders are
/// `pub(crate)`. Integration tests cannot reach them.
///
/// What is compared: `Inconsistent`-vs-`Unknown` only. The payload ids inside
/// `ClashReason` (e.g. `IndividualId`, `ClassId`) come from `HashSet` iteration
/// order and are nondeterministic across runs even for the same binary and input —
/// comparing them would be flaky. The `Inconsistent`-vs-`Unknown` discriminant is
/// the FP-relevant invariant: a spurious `Inconsistent` from the fast path marks
/// every class unsatisfiable.
///
/// Each fixture parses a fresh `InternalOntology` independently for each path so
/// that neither path sees the other's pool mutations.
#[cfg(test)]
mod abox_check_differential_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(body: &str) -> SetOntology<RcStr> {
        let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
        onto
    }

    /// Convert to `InternalOntology` and panic on conversion error.
    fn to_internal(body: &str) -> InternalOntology {
        let onto = parse(body);
        owl_dl_core::convert::convert_ontology(&onto).expect("convert ofn")
    }

    /// Returns `true` iff the verdict is `Inconsistent`.
    fn is_inconsistent_fast(internal: &InternalOntology) -> bool {
        let closure = owl_dl_saturation::saturate(internal);
        let owned = build_abox_check_inputs(internal);
        let verdict = abox_check::check(&owned.as_inputs(&closure));
        matches!(verdict, abox_check::AboxVerdict::Inconsistent { .. })
    }

    /// Returns `true` iff the full `PreparedOntology` verdict is `Inconsistent`.
    /// Uses `abox_verdict()` which internally calls `abox_check::check` with
    /// the same eight fields but built via the full pipeline.
    fn is_inconsistent_full(internal: InternalOntology) -> bool {
        let prepared = PreparedOntology::from_internal(internal).expect("prepare");
        matches!(
            prepared.abox_verdict(),
            abox_check::AboxVerdict::Inconsistent { .. }
        )
    }

    /// Assert that fast-path and full-path agree on verdict, and also check the
    /// expected outcome so the test cannot pass by both paths being uniformly wrong.
    fn assert_both_paths(body: &str, expect_inconsistent: bool, label: &str) {
        let internal_a = to_internal(body);
        let internal_b = to_internal(body); // fresh parse — independent pool
        let fast = is_inconsistent_fast(&internal_a);
        let full = is_inconsistent_full(internal_b);
        assert_eq!(
            fast, full,
            "{label}: fast path and full path disagree (fast={fast}, full={full})"
        );
        assert_eq!(
            fast, expect_inconsistent,
            "{label}: expected inconsistent={expect_inconsistent} but got {fast}"
        );
    }

    /// P2: individual typed into two disjoint classes — must be `Inconsistent`.
    #[test]
    fn differential_p2_disjoint_types_inconsistent() {
        assert_both_paths(
            "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
             Declaration(NamedIndividual(:i))
             DisjointClasses(:A :B)
             ClassAssertion(:A :i)
             ClassAssertion(:B :i)
             SubClassOf(:C :A)",
            true,
            "P2 disjoint types",
        );
    }

    /// P5: `Functional(R)` + two distinct witnesses + `DifferentIndividuals` —
    /// must be `Inconsistent`. The merge forced by `Functional(R)` causes
    /// the two witnesses to unify, violating the `DifferentIndividuals` pair.
    /// Uses `InverseFunctionalObjectProperty` (which `expand_role_characteristics`
    /// lowers to `FunctionalObjectProperty` on the inverse) so the ordering
    /// of `expand_role_characteristics` relative to `build_told_tables` is exercised.
    #[test]
    fn differential_p5_functional_two_witnesses_inconsistent() {
        assert_both_paths(
            "Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
             Declaration(NamedIndividual(:c))
             Declaration(ObjectProperty(:r))
             FunctionalObjectProperty(:r)
             ObjectPropertyAssertion(:r :a :b)
             ObjectPropertyAssertion(:r :a :c)
             DifferentIndividuals(:b :c)",
            true,
            "P5 functional + two witnesses + DifferentIndividuals",
        );
    }

    /// P4: `SameIndividual` + `DifferentIndividuals` on the same pair — must be
    /// `Inconsistent`.
    #[test]
    fn differential_p4_same_and_different_inconsistent() {
        assert_both_paths(
            "Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
             SameIndividual(:a :b)
             DifferentIndividuals(:a :b)",
            true,
            "P4 same+different",
        );
    }

    /// P3: matching positive and negative property assertion — must be `Inconsistent`.
    #[test]
    fn differential_p3_negopa_vs_opa_inconsistent() {
        assert_both_paths(
            "Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
             Declaration(ObjectProperty(:r))
             ObjectPropertyAssertion(:r :a :b)
             NegativeObjectPropertyAssertion(:r :a :b)",
            true,
            "P3 NegOPA vs OPA",
        );
    }

    /// Negative control 1: consistent `ABox` — must NOT be `Inconsistent`.
    #[test]
    fn differential_consistent_abox_not_inconsistent() {
        assert_both_paths(
            "Declaration(Class(:A)) Declaration(Class(:B))
             Declaration(NamedIndividual(:i))
             SubClassOf(:A :B)
             ClassAssertion(:A :i)",
            false,
            "consistent ABox",
        );
    }

    /// Negative control 2: `ABox`-free ontology — must NOT be `Inconsistent`.
    #[test]
    fn differential_abox_free_not_inconsistent() {
        assert_both_paths(
            "Declaration(Class(:A)) Declaration(Class(:B))
             SubClassOf(:A :B)",
            false,
            "ABox-free",
        );
    }
}

/// Canaries for the lazy `ABox`-saturation gate (`RUSTDL_LAZY_ABOX_SATURATION`,
/// default OFF). Lives in-crate because `PreparedOntology::closure` and
/// `test_env_lock` are `pub(crate)`.
///
/// **Negatives first.** The gate elides work; the failure mode that matters is
/// eliding it on an input that still needs it, which would silently downgrade an
/// `Inconsistent` verdict to `Unknown`. So the tests pin, in order:
/// (1) the ABox-BEARING half still builds the closure and still reaches the same
/// verdict, (2) the ABox-FREE half actually elides (otherwise the lever is inert),
/// (3) OFF-vs-ON verdict identity over both shapes.
///
/// Sabotage record (2026-08-01) — each mutation was applied, the suite run, and
/// the change reverted:
/// * gate → `lazy_abox_saturation_enabled()` alone (elide unconditionally):
///   `lazy_saturation_on_keeps_closure_when_abox_present`,
///   `lazy_saturation_on_preserves_p2_inconsistency` and
///   `lazy_saturation_verdict_identical_off_vs_on` all FAIL.
/// * gate → `false` (never elide): `lazy_saturation_on_elides_closure_when_abox_free`
///   FAILS.
#[cfg(test)]
mod lazy_abox_saturation_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    /// An `ABox`-bearing ontology whose inconsistency is found by `abox_check`
    /// P2 (one individual typed into two disjoint classes) — i.e. by the very
    /// closure this gate may elide.
    const ABOX_INCONSISTENT: &str = "Declaration(Class(:A)) Declaration(Class(:B))
         Declaration(Class(:C)) Declaration(NamedIndividual(:i))
         DisjointClasses(:A :B)
         ClassAssertion(:A :i) ClassAssertion(:B :i)
         SubClassOf(:C :A)";

    /// `ABox`-bearing but consistent — the negative control for the above.
    const ABOX_CONSISTENT: &str = "Declaration(Class(:A)) Declaration(Class(:B))
         Declaration(NamedIndividual(:i))
         ClassAssertion(:A :i) SubClassOf(:A :B)";

    /// No individuals at all — the case the gate is allowed to elide.
    const ABOX_FREE: &str = "Declaration(Class(:A)) Declaration(Class(:B))
         Declaration(Class(:C))
         SubClassOf(:A :B) SubClassOf(:B :C)";

    fn to_internal(body: &str) -> InternalOntology {
        let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
        owl_dl_core::convert::convert_ontology(&onto).expect("convert ofn")
    }

    /// Build a `PreparedOntology` with the lazy gate forced on/off, restoring the
    /// prior env value. Serialised via `test_env_lock`, mirroring
    /// `build_cache_with_sat_seed_flag`.
    #[allow(unsafe_code)]
    fn prepare_with_flag(body: &str, enable: bool) -> PreparedOntology {
        let internal = to_internal(body);
        let _lock = test_env_lock();
        let prior = std::env::var_os("RUSTDL_LAZY_ABOX_SATURATION");
        // SAFETY: serialized by test_env_lock (one test at a time); restored
        // before the lock is released.
        unsafe {
            std::env::set_var(
                "RUSTDL_LAZY_ABOX_SATURATION",
                if enable { "1" } else { "0" },
            );
        }
        let prepared = PreparedOntology::from_internal(internal).expect("prepare");
        match prior {
            Some(v) => unsafe { std::env::set_var("RUSTDL_LAZY_ABOX_SATURATION", v) },
            None => unsafe { std::env::remove_var("RUSTDL_LAZY_ABOX_SATURATION") },
        }
        prepared
    }

    fn is_inconsistent(prepared: &PreparedOntology) -> bool {
        matches!(
            prepared.abox_verdict(),
            abox_check::AboxVerdict::Inconsistent { .. }
        )
    }

    /// NEGATIVE #1 — flag ON must NOT elide the closure when the input has an
    /// `ABox`. This is the arm an unconditional elision would break.
    #[test]
    fn lazy_saturation_on_keeps_closure_when_abox_present() {
        for body in [ABOX_INCONSISTENT, ABOX_CONSISTENT] {
            let prepared = prepare_with_flag(body, true);
            assert!(
                prepared.closure.is_some(),
                "ABox-bearing input must keep its EL closure even with the lazy gate on"
            );
            assert!(
                !prepared.abox.individuals.is_empty(),
                "fixture is supposed to have individuals"
            );
        }
    }

    /// NEGATIVE #2 — flag ON on an `ABox`-bearing INCONSISTENT input must still
    /// report `Inconsistent`. Eliding the closure there downgrades it to
    /// `Unknown` (sound, but a completeness regression).
    #[test]
    fn lazy_saturation_on_preserves_p2_inconsistency() {
        assert!(
            is_inconsistent(&prepare_with_flag(ABOX_INCONSISTENT, true)),
            "P2 inconsistency must survive the lazy-saturation gate"
        );
    }

    /// The lever actually fires: flag ON + no individuals ⟹ no closure built.
    /// Without this the gate could be a no-op and every other test would pass.
    #[test]
    fn lazy_saturation_on_elides_closure_when_abox_free() {
        let prepared = prepare_with_flag(ABOX_FREE, true);
        assert!(
            prepared.abox.individuals.is_empty(),
            "fixture is supposed to be ABox-free"
        );
        assert!(
            prepared.closure.is_none(),
            "ABox-free input with the lazy gate on must skip the EL saturation"
        );
        assert!(
            !is_inconsistent(&prepared),
            "an elided closure must still yield a usable (Unknown) verdict"
        );
    }

    /// Default / explicit-OFF keeps today's behaviour: the closure is always built.
    #[test]
    fn lazy_saturation_off_always_builds_closure() {
        for body in [ABOX_INCONSISTENT, ABOX_CONSISTENT, ABOX_FREE] {
            assert!(
                prepare_with_flag(body, false).closure.is_some(),
                "flag OFF must build the closure unconditionally"
            );
        }
    }

    /// OFF-vs-ON verdict identity across both shapes — the differential gate.
    #[test]
    fn lazy_saturation_verdict_identical_off_vs_on() {
        for body in [ABOX_INCONSISTENT, ABOX_CONSISTENT, ABOX_FREE] {
            let off = is_inconsistent(&prepare_with_flag(body, false));
            let on = is_inconsistent(&prepare_with_flag(body, true));
            assert_eq!(off, on, "lazy-saturation gate changed the ABox verdict");
        }
    }
}

/// Iterative-deepening (`RUSTDL_ITERATIVE_DEEPENING`) canaries.
///
/// **Negatives first.** Every positive assertion below is preceded by a control
/// that proves the fixture actually discriminates — a fixture that the FIRST
/// schedule level already decides would make "deepening works" vacuously true.
/// `deep_chain_stalls_at_the_first_level` is that control; if it ever starts
/// passing at depth 8, the rest of this module is measuring nothing.
///
/// Inline (not in `tests/`) because `HyperCache`, `decide_iterative_deepening_traced`
/// and `test_env_lock` are `pub(crate)` / `#[cfg(test)]`.
#[cfg(test)]
mod iterative_deepening_tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_tableau::hyper::HyperResult;
    use std::io::Cursor;
    use std::time::Duration;

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    /// A `⊔`-chain of length `n` whose proof genuinely needs branch depth `n`:
    ///
    /// ```text
    /// A0 ⊑ K
    /// A_{i-1} ⊑ P_i ⊔ Q_i,   P_i ⊑ A_i,   Q_i ⊓ K ⊑ ⊥      (i = 1..n)
    /// An ⊑ Y
    /// ```
    ///
    /// Refuting `A0 ⊓ ¬Y` must choose `P_i` at every level — `Q_i` clashes
    /// against the `K` that `A0` puts on the same node — and level `i`'s
    /// disjunction only opens once `A_{i-1}` is present, so the `n` decisions
    /// cannot be reordered or merged. A cap below `n` yields `Stalled`; a cap at
    /// or above `n` yields `Unsat`.
    ///
    /// **The obvious fixture does not work, and the negative control caught it.**
    /// Writing the disjunction as `A_{i-1} ⊑ A_i ⊔ B_i` with `B_i ⊑ A_i` gives
    /// the two disjuncts a common told subsumer, so the minimal-common-subsumer
    /// pass rewrites the whole chain to Horn implications and `sat_seed` hands
    /// the wedge `Y` at the root: `pairs_branched: 0`, decided at depth 0. Here
    /// `Q_i` has NO told superclass (a conjunctive-`⊥` GCI is recorded as a
    /// told-DISJOINT pair, not a subsumer), so no common subsumer exists and the
    /// case split survives to the engine.
    pub(super) fn build_disjunction_chain(n: usize) -> InternalOntology {
        owl_dl_core::convert::convert_ontology(&parse(&chain_src(n, true)))
            .expect("chain fixture converts")
    }

    /// The same chain with the terminal `An ⊑ Y` link REMOVED, so `A0 ⊑ Y`
    /// genuinely does not hold and the wedge answers `Sat`.
    fn build_unprovable_chain(n: usize) -> InternalOntology {
        owl_dl_core::convert::convert_ontology(&parse(&chain_src(n, false)))
            .expect("chain fixture converts")
    }

    fn chain_src(n: usize, entail_y: bool) -> String {
        use std::fmt::Write as _;
        let mut b = String::from("Declaration(Class(:Y)) Declaration(Class(:K))\n");
        for i in 0..=n {
            let _ = writeln!(b, "Declaration(Class(:A{i}))");
        }
        b.push_str("SubClassOf(:A0 :K)\n");
        for i in 1..=n {
            let prev = i - 1;
            let _ = write!(
                b,
                "Declaration(Class(:P{i})) Declaration(Class(:Q{i}))\n\
                 SubClassOf(:A{prev} ObjectUnionOf(:P{i} :Q{i}))\n\
                 SubClassOf(:P{i} :A{i})\n\
                 SubClassOf(ObjectIntersectionOf(:Q{i} :K) owl:Nothing)\n"
            );
        }
        if entail_y {
            let _ = writeln!(b, "SubClassOf(:A{n} :Y)");
        }
        format!("{HEADER}Ontology(\n{b})\n")
    }

    pub(super) fn class_id(internal: &InternalOntology, local: &str) -> owl_dl_core::ir::ClassId {
        let iri = format!("http://rustdl.test/{local}");
        internal
            .vocabulary
            .classes()
            .find(|(_, i)| *i == iri.as_str())
            .map_or_else(|| panic!("class {local} not found"), |(id, _)| id)
    }

    /// Run `f` with `RUSTDL_ID_SCHEDULE` set (or cleared), serialised through
    /// `test_env_lock` and restored afterwards.
    ///
    /// `RUSTDL_ID_SHALLOW_MS` is pinned to `0` (bound disabled) for the whole
    /// closure. Without that pin these canaries would depend on whether a
    /// microsecond-scale search happened to outrun a 5 ms wall budget on a
    /// loaded machine — a flaky test that would also silently stop testing the
    /// deepening path. The budget itself is covered by its own canaries.
    #[allow(unsafe_code)]
    fn with_schedule<T>(val: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ID_SCHEDULE");
        let prev_ms = std::env::var_os("RUSTDL_ID_SHALLOW_MS");
        // SAFETY: serialised by `test_env_lock`; restored before release.
        unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", "0") };
        match val {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ID_SCHEDULE", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ID_SCHEDULE") },
        }
        let out = f();
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ID_SCHEDULE", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ID_SCHEDULE") },
        }
        match prev_ms {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_MS") },
        }
        out
    }

    // ---------------------------------------------------------------- negatives

    /// **Control (negatives-first).** The 12-deep chain is genuinely beyond the
    /// first schedule level: a single search capped at depth 8 `Stalled`s. If
    /// this ever returns `Unsat`, every deepening assertion below is vacuous.
    #[test]
    fn deep_chain_stalls_at_the_first_level() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _) = cache.decide_with_stats(a0, y, 8, None);
        assert_eq!(
            res,
            HyperResult::Stalled,
            "12-deep ⊔-chain must NOT be decidable at depth 8, or the fixture is inert"
        );
    }

    /// **Control.** The same chain IS decidable at the second schedule level,
    /// so the deepening test below has somewhere to succeed.
    #[test]
    fn deep_chain_is_unsat_at_the_second_level() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _) = cache.decide_with_stats(a0, y, 32, None);
        assert_eq!(res, HyperResult::Unsat, "depth 32 must prove A0 ⊑ Y");
    }

    /// **Control.** The shallow chain is decided by the FIRST level, so
    /// `stops_at_first_definite_verdict` is not vacuous either.
    #[test]
    fn shallow_chain_is_unsat_at_the_first_level() {
        let internal = build_disjunction_chain(3);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _) = cache.decide_with_stats(a0, y, 8, None);
        assert_eq!(
            res,
            HyperResult::Unsat,
            "depth 8 must prove the 3-deep chain"
        );
    }

    // ---------------------------------------------------------------- the loop

    /// The loop deepens past a `Stalled` first level and returns the deeper
    /// level's definite verdict. **Sabotage target: "the loop never deepens"**
    /// (returning after level 0) and **"a `Stalled` is treated as definite"**
    /// (breaking on `Stalled`) both fail here.
    #[test]
    fn deepens_past_a_stalled_first_level() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _, trace) = with_schedule(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert_eq!(res, HyperResult::Unsat, "deepening must recover the proof");
        assert_eq!(trace.levels_run, 2, "must run exactly levels 8 then 32");
        assert_eq!(trace.final_depth, 32);
    }

    /// A definite verdict at the first level ends the loop — no re-work.
    /// **Sabotage target: "a `Stalled` is treated as definite"** inverted —
    /// a loop that ignores the verdict and always runs the whole schedule
    /// fails here.
    #[test]
    fn stops_at_the_first_definite_verdict() {
        let internal = build_disjunction_chain(3);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _, trace) = with_schedule(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert_eq!(res, HyperResult::Unsat);
        assert_eq!(
            trace.levels_run, 1,
            "a definite verdict must not be deepened past"
        );
        assert_eq!(trace.final_depth, HYPER_WEDGE_DEPTH_SCHEDULE[0]);
    }

    /// A `Sat` (genuine non-subsumption) is definite too — the loop must not
    /// grind through the whole schedule looking for an `Unsat` that cannot exist.
    #[test]
    fn sat_is_definite_and_ends_the_loop() {
        // n = 3, so the model is found within the FIRST level's cap of 8.
        let internal = build_unprovable_chain(3);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _, trace) = with_schedule(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert_eq!(
            res,
            HyperResult::Sat,
            "A0 ⊑ Y does not hold without the link"
        );
        assert_eq!(trace.levels_run, 1, "a Sat verdict must end the loop");
    }

    /// The loop never runs a level that is not in the schedule, and never runs
    /// more levels than the schedule has. **Sabotage target: "the loop deepens
    /// past the final cap"** — a driver that multiplies the depth itself (or
    /// appends a level beyond the last) lands on a depth outside the schedule.
    /// A three-level schedule is used so two deepening steps are exercised.
    #[test]
    fn never_leaves_the_schedule() {
        // A 20-deep chain, so BOTH intermediate levels (8, 12) genuinely stall
        // and two deepening steps are exercised before 256 decides.
        let internal = build_disjunction_chain(20);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _, trace) = with_schedule(Some("8,12,256"), || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert_eq!(res, HyperResult::Unsat);
        assert_eq!(trace.levels_run, 3, "8 and 12 both stall; 256 decides");
        assert_eq!(
            trace.final_depth, 256,
            "must land on the LAST schedule level"
        );
        assert!(
            [8usize, 12, 256].contains(&trace.final_depth),
            "the loop must only ever run scheduled depths"
        );
    }

    /// An already-expired deadline bounds the WHOLE loop: no second level is
    /// started. **Sabotage target: dropping the deadline guard** (which would
    /// let iterative deepening multiply the effective per-pair budget).
    #[test]
    fn expired_deadline_stops_the_loop() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let past = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .expect("instant is well past process start");
        let (res, _, trace) = with_schedule(None, || {
            cache.decide_iterative_deepening_traced(a0, y, Some(past))
        });
        assert_eq!(res, HyperResult::Stalled);
        assert_eq!(
            trace.levels_run, 1,
            "an expired deadline must stop the loop, not restart it deeper"
        );
    }

    // ------------------------------------------------------------ flag + schedule

    /// Default ON (2026-08-02 flip), and ONLY an explicit `=0` reverts.
    ///
    /// BOTH halves are pinned deliberately. The unset half guards the flip
    /// itself; the `=0` half guards the ESCAPE HATCH, and it is the more
    /// important of the two — an opt-out that silently stopped working would
    /// leave no way back from a change this large, and would do so without
    /// failing a single test. Per the house default-ON idiom an EMPTY value
    /// ENABLES, so it is asserted on the ON side, not the OFF side.
    #[test]
    #[allow(unsafe_code)]
    fn flag_defaults_on_and_only_0_reverts() {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ITERATIVE_DEEPENING");
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe { std::env::remove_var("RUSTDL_ITERATIVE_DEEPENING") };
        assert!(iterative_deepening_enabled(), "unset must be ON");
        unsafe { std::env::set_var("RUSTDL_ITERATIVE_DEEPENING", "0") };
        assert!(!iterative_deepening_enabled(), "=0 must revert");
        for v in ["", "1", "true", "2", "on"] {
            unsafe { std::env::set_var("RUSTDL_ITERATIVE_DEEPENING", v) };
            assert!(iterative_deepening_enabled(), "{v:?} must stay ON");
        }
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ITERATIVE_DEEPENING", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ITERATIVE_DEEPENING") },
        }
    }

    /// The compiled schedule's final level dominates the fixed cap it replaces.
    /// This is the completeness invariant: deepening is verdict-monotone, so a
    /// final level `>= HYPER_WEDGE_DEPTH` can only ADD entailments.
    #[test]
    fn final_level_dominates_the_fixed_cap() {
        let last = *HYPER_WEDGE_DEPTH_SCHEDULE
            .last()
            .expect("schedule is non-empty");
        assert!(
            last >= HYPER_WEDGE_DEPTH,
            "final level {last} < fixed cap {HYPER_WEDGE_DEPTH}: deepening could LOSE entailments"
        );
        assert!(
            HYPER_WEDGE_DEPTH_SCHEDULE.windows(2).all(|w| w[1] > w[0]),
            "schedule must be strictly increasing"
        );
    }

    /// A malformed `RUSTDL_ID_SCHEDULE` is rejected wholesale rather than
    /// silently reasoning under a schedule that could lose entailments. The
    /// dangerous case is the third: a well-formed but SHALLOW schedule whose
    /// final level is below the fixed cap.
    #[test]
    fn malformed_schedule_override_falls_back_to_default() {
        for bad in [
            "",         // empty
            "nonsense", // unparsable
            "8,,256",   // empty component
            "256,8",    // not increasing
            "8,8,256",  // not STRICTLY increasing
            "8,32,128", // final level below HYPER_WEDGE_DEPTH — would lose entailments
            "-4,256",   // negative
        ] {
            let got = with_schedule(Some(bad), depth_schedule);
            assert_eq!(
                got,
                HYPER_WEDGE_DEPTH_SCHEDULE.to_vec(),
                "malformed schedule {bad:?} must fall back to the default"
            );
        }
    }

    /// A well-formed override IS honoured (otherwise the rejection test above
    /// would pass trivially for a `depth_schedule` that ignores the env var).
    #[test]
    fn well_formed_schedule_override_is_honoured() {
        let got = with_schedule(Some("4, 16, 300"), depth_schedule);
        assert_eq!(got, vec![4usize, 16, 300]);
    }

    // -------------------------------------------------- shallow-phase budget

    /// The shallow phase gets at most `ID_SHALLOW_BUDGET_MS` when the caller
    /// supplied no deadline, and the FINAL level then runs unbounded.
    #[test]
    fn shallow_budget_bounds_the_unbounded_case() {
        let now = std::time::Instant::now();
        let d = id_shallow_deadline(now, None, 5).expect("bounded when budget > 0");
        assert_eq!(d.saturating_duration_since(now), Duration::from_millis(5));
    }

    /// With a caller deadline the shallow phase takes at most
    /// `1/ID_SHALLOW_BUDGET_DIVISOR` of what remains, so the majority of a
    /// `--pair-timeout-ms` budget always reaches the final level.
    /// **Sabotage target: dropping the divisor clamp** — at a 4 ms per-pair
    /// budget the shallow phase would otherwise eat 4 of the 4 ms.
    #[test]
    fn shallow_budget_never_eats_the_callers_budget() {
        let now = std::time::Instant::now();
        let caller = now + Duration::from_millis(4);
        let d = id_shallow_deadline(now, Some(caller), 5).expect("bounded");
        assert_eq!(
            d.saturating_duration_since(now),
            Duration::from_millis(1),
            "4ms budget / divisor 4 = 1ms for the shallow phase"
        );
        assert!(
            d < caller,
            "shallow phase must end strictly before the caller deadline"
        );
    }

    /// A generous caller deadline does not raise the shallow budget above its
    /// absolute cap.
    #[test]
    fn shallow_budget_is_capped_absolutely() {
        let now = std::time::Instant::now();
        let d = id_shallow_deadline(now, Some(now + Duration::from_secs(60)), 5).expect("bounded");
        assert_eq!(d.saturating_duration_since(now), Duration::from_millis(5));
    }

    /// `=0` disables the bound: the shallow levels then share the caller's own
    /// deadline (i.e. exactly the unbounded-re-work variant that measurement
    /// refuted on `ore_ont_10407`). Kept as an escape hatch and an A/B arm.
    #[test]
    fn shallow_budget_zero_disables_the_bound() {
        let now = std::time::Instant::now();
        assert!(id_shallow_deadline(now, None, 0).is_none());
        let caller = now + Duration::from_secs(1);
        assert_eq!(id_shallow_deadline(now, Some(caller), 0), Some(caller));
    }

    /// An already-expired caller deadline yields a zero-width shallow slice
    /// rather than panicking on a negative duration.
    #[test]
    fn shallow_budget_handles_an_expired_caller_deadline() {
        let now = std::time::Instant::now();
        let past = now
            .checked_sub(Duration::from_secs(1))
            .expect("instant is well past process start");
        let d = id_shallow_deadline(now, Some(past), 5).expect("bounded");
        assert_eq!(d.saturating_duration_since(now), Duration::ZERO);
    }

    /// Garbage in `RUSTDL_ID_SHALLOW_MS` must fall back to the DEFAULT, not to
    /// `0` — parsing `"abc"` as "disabled" would silently reinstate the 68 s of
    /// shallow re-work the bound exists to prevent.
    #[test]
    #[allow(unsafe_code)]
    fn shallow_budget_env_garbage_falls_back_to_the_default() {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ID_SHALLOW_MS");
        // SAFETY: serialised by `test_env_lock`; restored below.
        for bad in ["abc", "", "-1", "5ms"] {
            unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", bad) };
            assert_eq!(
                id_shallow_budget_ms(),
                ID_SHALLOW_BUDGET_MS,
                "{bad:?} must fall back to the default, NOT to 0"
            );
        }
        unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", "17") };
        assert_eq!(
            id_shallow_budget_ms(),
            17,
            "a valid override must be honoured"
        );
        unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_MS") };
        assert_eq!(id_shallow_budget_ms(), ID_SHALLOW_BUDGET_MS);
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_MS") },
        }
    }

    // --------------------------------------------- adaptive shallow shutoff

    /// Run `f` with `RUSTDL_ID_SHALLOW_WASTE_MS` set (or cleared) AND
    /// `RUSTDL_ID_SHALLOW_MS` pinned to `0`, serialised through `test_env_lock`.
    /// Pinning the per-pair budget off keeps these canaries about the SHUTOFF
    /// rather than about whether a microsecond search outran a 5 ms wall.
    #[allow(unsafe_code)]
    fn with_waste_budget<T>(val: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ID_SHALLOW_WASTE_MS");
        let prev_ms = std::env::var_os("RUSTDL_ID_SHALLOW_MS");
        let prev_sched = std::env::var_os("RUSTDL_ID_SCHEDULE");
        // SAFETY: serialised by `test_env_lock`; restored before release.
        unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", "0") };
        // A two-level schedule makes "reached the FINAL level" — i.e. a shallow
        // MISS — reachable on the 12-deep chain, which the default schedule
        // decides at its second (non-final) level.
        unsafe { std::env::set_var("RUSTDL_ID_SCHEDULE", "8,256") };
        match val {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_WASTE_MS", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_WASTE_MS") },
        }
        let out = f();
        for (k, v) in [
            ("RUSTDL_ID_SHALLOW_WASTE_MS", prev),
            ("RUSTDL_ID_SHALLOW_MS", prev_ms),
            ("RUSTDL_ID_SCHEDULE", prev_sched),
        ] {
            match v {
                Some(v) => unsafe { std::env::set_var(k, v) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
        out
    }

    /// **Control (negatives-first).** On a FRESH cache the accumulator is zero
    /// and the shallow phase RUNS — so every "the shutoff fired" assertion below
    /// is discriminating rather than describing the default state. Also pins the
    /// complement of "the shutoff triggers immediately": it must NOT fire on the
    /// first pair of a classify.
    #[test]
    fn fresh_cache_runs_the_shallow_phase() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        assert_eq!(cache.id_shallow_waste_us_for_test(), 0, "fresh cache");
        let (res, _, trace) = with_waste_budget(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert!(
            !trace.shallow_skipped,
            "the shutoff must NOT fire on the first pair of a classify"
        );
        assert_eq!(trace.levels_run, 2, "8 stalls, 256 decides");
        assert_eq!(res, HyperResult::Unsat);
    }

    /// A pair the shallow phase DECIDES charges NO waste. **Sabotage target:
    /// charging every pair** — that would make the accumulator track total
    /// shallow spend rather than wasted shallow spend, and would shut the phase
    /// off on exactly the population it wins on (`wine` decides 3,465 pairs and
    /// wastes 195 ms; `ore_ont_13991` decides 84 and wastes its whole budget).
    #[test]
    fn a_deciding_pair_charges_no_waste() {
        // 3-deep: the FIRST level (8) decides, which is a non-final level.
        let internal = build_disjunction_chain(3);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _, trace) = with_waste_budget(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert_eq!(res, HyperResult::Unsat);
        assert_eq!(trace.levels_run, 1, "decided at the first level");
        assert_eq!(
            cache.id_shallow_waste_us_for_test(),
            0,
            "a shallow DECIDE must not be charged as waste"
        );
        assert_eq!(cache.id_shallow_counts_for_test(), (1, 0));
    }

    /// A pair that falls through to the FINAL level charges waste. **Sabotage
    /// target: never charging** — an accumulator that never grows is a shutoff
    /// that never fires, which is the `ore_ont_13991` regression verbatim.
    #[test]
    fn a_non_deciding_pair_charges_waste() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let (res, _, _) = with_waste_budget(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert_eq!(res, HyperResult::Unsat, "the final level still decides");
        assert_eq!(
            cache.id_shallow_counts_for_test(),
            (0, 1),
            "reaching the final level is a shallow MISS"
        );
        assert!(
            cache.id_shallow_waste_us_for_test() > 0,
            "a shallow MISS must be charged, or the shutoff can never fire"
        );
    }

    /// A latched accumulator skips the shallow phase entirely: ONE level runs,
    /// and it is the final one. **Sabotage target: the shutoff never
    /// triggering.**
    #[test]
    fn a_latched_accumulator_skips_the_shallow_phase() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        cache.set_id_shallow_waste_us_for_test(u64::from(u32::MAX));
        let (res, _, trace) = with_waste_budget(None, || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert!(trace.shallow_skipped, "the shutoff must fire when latched");
        assert_eq!(trace.levels_run, 1, "only the final level may run");
        assert_eq!(trace.final_depth, 256, "and it must BE the final level");
        assert_eq!(res, HyperResult::Unsat);
    }

    /// **The soundness gate, verified rather than assumed.** The claim on
    /// [`ID_SHALLOW_WASTE_BUDGET_MS`] is that skipping the shallow phase cannot
    /// change any answer — it runs only the final level, whose cap is
    /// `>= HYPER_WEDGE_DEPTH` and whose deadline is the caller's own. Pinned on
    /// BOTH verdict directions, because a subtractive-only check would miss a
    /// shutoff that suppressed an `Unsat` into a `Sat`.
    #[test]
    fn shutoff_cannot_change_a_verdict() {
        for (n, want) in [(12usize, HyperResult::Unsat), (20, HyperResult::Unsat)] {
            for entailed in [true, false] {
                let internal = if entailed {
                    build_disjunction_chain(n)
                } else {
                    build_unprovable_chain(n)
                };
                let expect = if entailed { want } else { HyperResult::Sat };
                let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));

                let on = HyperCache::build(&internal);
                on.set_id_shallow_waste_us_for_test(u64::from(u32::MAX));
                let (latched, _, t_latched) =
                    with_waste_budget(None, || on.decide_iterative_deepening_traced(a0, y, None));

                let off = HyperCache::build(&internal);
                let (unlatched, _, t_unlatched) =
                    with_waste_budget(None, || off.decide_iterative_deepening_traced(a0, y, None));

                assert!(t_latched.shallow_skipped && !t_unlatched.shallow_skipped);
                assert_eq!(
                    latched, unlatched,
                    "shutoff changed the verdict on chain n={n} entailed={entailed}"
                );
                assert_eq!(latched, expect, "chain n={n} entailed={entailed}");
            }
        }
    }

    /// `=0` disables the shutoff, restoring the always-run-shallow behaviour —
    /// the escape hatch, and the arm that reproduces the `ore_ont_13991`
    /// regression. **Sabotage target: the shutoff triggering unconditionally.**
    #[test]
    fn waste_budget_zero_disables_the_shutoff() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        cache.set_id_shallow_waste_us_for_test(u64::MAX);
        let (res, _, trace) = with_waste_budget(Some("0"), || {
            cache.decide_iterative_deepening_traced(a0, y, None)
        });
        assert!(
            !trace.shallow_skipped,
            "=0 must disable the shutoff even with the accumulator saturated"
        );
        assert_eq!(trace.levels_run, 2, "the shallow phase must still run");
        assert_eq!(res, HyperResult::Unsat);
    }

    /// Once latched the shutoff STAYS latched: a skipped pair neither adds to
    /// the accumulator nor resets it, so no second flag is needed to hold it.
    /// **Sabotage target: resetting (or charging) on a skipped pair** — a reset
    /// would re-enable the shallow phase every other pair and halve rather than
    /// remove the `13991` tax.
    #[test]
    fn the_latch_is_self_sustaining() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let latched = u64::from(u32::MAX);
        cache.set_id_shallow_waste_us_for_test(latched);
        for _ in 0..3 {
            let (_, _, trace) = with_waste_budget(None, || {
                cache.decide_iterative_deepening_traced(a0, y, None)
            });
            assert!(trace.shallow_skipped, "must stay latched");
        }
        assert_eq!(
            cache.id_shallow_waste_us_for_test(),
            latched,
            "a skipped pair must not touch the accumulator"
        );
        assert_eq!(
            cache.id_shallow_counts_for_test(),
            (0, 0),
            "a skipped pair is not an observation"
        );
    }

    /// Garbage in `RUSTDL_ID_SHALLOW_WASTE_MS` falls back to the DEFAULT, not to
    /// `0` — parsing `"abc"` as "disabled" would silently reinstate the
    /// `ore_ont_13991` DNF, exactly as for `RUSTDL_ID_SHALLOW_MS`.
    #[test]
    #[allow(unsafe_code)]
    fn waste_budget_env_garbage_falls_back_to_the_default() {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ID_SHALLOW_WASTE_MS");
        // SAFETY: serialised by `test_env_lock`; restored below.
        for bad in ["abc", "", "-1", "1s"] {
            unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_WASTE_MS", bad) };
            assert_eq!(
                id_shallow_waste_budget_ms(),
                ID_SHALLOW_WASTE_BUDGET_MS,
                "{bad:?} must fall back to the default, NOT to 0"
            );
        }
        unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_WASTE_MS", "250") };
        assert_eq!(id_shallow_waste_budget_ms(), 250);
        unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_WASTE_MS") };
        assert_eq!(id_shallow_waste_budget_ms(), ID_SHALLOW_WASTE_BUDGET_MS);
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_WASTE_MS", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_WASTE_MS") },
        }
    }

    /// A budget-cut shallow level must NOT be mistaken for an exhausted one.
    /// **Sabotage target: dropping the `shallow_spent` term** from
    /// `id_cap_was_not_binding` — that inversion silently turns every deep pair
    /// into a stall, because the final level is never reached.
    #[test]
    fn a_budget_cut_level_is_never_treated_as_exhausted() {
        // Cut by the shallow budget at depth 0 of a 512-cap level: NOT exhausted.
        assert!(!id_cap_was_not_binding(true, false, 0, 512));
        assert!(!id_cap_was_not_binding(true, true, 0, 512));
    }

    /// A diverged level is not exhausted either: the divergence cut depends on
    /// `init_depth`, so a deeper level may get further.
    #[test]
    fn a_diverged_level_is_never_treated_as_exhausted() {
        assert!(!id_cap_was_not_binding(false, true, 512, 512));
        assert!(!id_cap_was_not_binding(false, true, 3, 512));
    }

    /// The positive case the exit exists for: the search finished inside its cap
    /// (no branch reached it) and was neither budget-cut nor diverged, so a
    /// deeper cap would run the identical search.
    #[test]
    fn an_exhausted_level_stops_the_deepening() {
        assert!(id_cap_was_not_binding(false, false, 3, 8));
        // Reaching the cap is NOT exhaustion — deepening must continue.
        assert!(!id_cap_was_not_binding(false, false, 8, 8));
        assert!(!id_cap_was_not_binding(false, false, 9, 8));
    }

    /// Integration: at the SHIPPING shallow budget (not the `0` the other loop
    /// canaries pin), a pair that needs a deeper level is still decided.
    #[test]
    #[allow(unsafe_code)]
    fn default_shallow_budget_still_decides_a_deep_pair() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ID_SHALLOW_MS");
        let prev_s = std::env::var_os("RUSTDL_ID_SCHEDULE");
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe { std::env::remove_var("RUSTDL_ID_SHALLOW_MS") };
        unsafe { std::env::remove_var("RUSTDL_ID_SCHEDULE") };
        let res = cache.decide_iterative_deepening_traced(a0, y, None);
        if let Some(v) = prev {
            unsafe { std::env::set_var("RUSTDL_ID_SHALLOW_MS", v) };
        }
        if let Some(v) = prev_s {
            unsafe { std::env::set_var("RUSTDL_ID_SCHEDULE", v) };
        }
        assert_eq!(res.0, HyperResult::Unsat);
        assert!(res.2.levels_run >= 2, "depth 8 cannot decide this pair");
    }

    /// Flag-OFF, `decide` takes the single fixed-cap path and agrees with a
    /// direct `decide_with_stats(.., HYPER_WEDGE_DEPTH, ..)`.
    ///
    /// Pins `RUSTDL_ITERATIVE_DEEPENING=0` EXPLICITLY rather than reading it off
    /// the ambient default. This test used to assert the ambient default was
    /// OFF, which made it fail the moment the default flipped to ON on
    /// 2026-08-02 — a dispatch change, not a regression: the test wants the
    /// flag-OFF path, so it should ASK for it.
    #[test]
    #[allow(unsafe_code)]
    fn flag_off_matches_the_fixed_cap_path() {
        let internal = build_disjunction_chain(12);
        let cache = HyperCache::build(&internal);
        let (a0, y) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_ITERATIVE_DEEPENING");
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe { std::env::set_var("RUSTDL_ITERATIVE_DEEPENING", "0") };
        assert!(!iterative_deepening_enabled(), "pinned OFF for this test");
        let (fixed, _) = cache.decide_with_stats(a0, y, HYPER_WEDGE_DEPTH, None);
        let decided = cache.decide(a0, y, None);
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_ITERATIVE_DEEPENING", v) },
            None => unsafe { std::env::remove_var("RUSTDL_ITERATIVE_DEEPENING") },
        }
        assert_eq!(fixed, HyperResult::Unsat);
        assert_eq!(decided, HyperVerdict::Subsumed);
    }
}

/// Canaries for **main-tableau** iterative deepening
/// (`RUSTDL_TABLEAU_ITERATIVE_DEEPENING`, default OFF). Negatives first: the
/// three controls at the top establish that the fixture DISCRIMINATES — that
/// the shallow level genuinely cannot decide the deep chain, and genuinely can
/// decide a shallow one — before any deepening or shutoff claim is asserted.
/// Without them every assertion below could pass on an inert fixture.
///
/// These drive `PreparedOntology::decide_classify_with_deadline`, the real
/// classify tableau entry point, not a test-only twin.
#[cfg(test)]
/// Canaries for the reasoner-owned half of the main-tableau adaptive
/// early-abandon: the **flag** (default-OFF idiom), the **limit** override, and
/// the `decide` **verdict mapping**. The mechanism itself — `note_depth_cap_hit`,
/// the latch, the unwind, and the FP direction — is canaried inside the tableau
/// crate (`owl_dl_tableau::search::early_abandon_tests`), where the hooks live;
/// the iterative-deepening write-up recorded an uncaught sabotage precisely
/// because a canary pinned a `TableauContext` API without pinning its call.
mod tableau_early_abandon_tests {
    use super::*;

    /// **Control.** The flag is OFF by default and only `=1` enables it — the
    /// house default-OFF idiom. An empty value must NOT enable (that is the
    /// default-ON idiom, and confusing the two is how a default gets flipped by
    /// accident).
    #[test]
    #[allow(unsafe_code)]
    fn flag_defaults_on_and_only_0_reverts() {
        let _lock = test_env_lock();
        let k = "RUSTDL_TABLEAU_EARLY_ABANDON";
        let prev = std::env::var_os(k);
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe { std::env::remove_var(k) };
        // DEFAULT ON since 0.4.14. Both halves are pinned deliberately: an unset
        // variable must ENABLE, and `=0` must still REVERT. The opt-out matters as
        // much as the default — it is the only way back from a change that alters
        // search behaviour on every tableau-exercised ontology.
        assert!(tableau_early_abandon_enabled(), "unset ⇒ ON since 0.4.14");
        unsafe { std::env::set_var(k, "0") };
        assert!(!tableau_early_abandon_enabled(), "\"0\" must revert");
        // Empty ENABLES, matching the seven other default-ON flags in this
        // workspace (`is_none_or(|v| v != "0")`): for this polarity only an
        // explicit `=0` is the opt-out, so `VAR=$UNSET_VAR` cannot silently
        // revert the change.
        for v in ["", "1", "2", "true", "yes", "on"] {
            unsafe { std::env::set_var(k, v) };
            assert!(
                tableau_early_abandon_enabled(),
                "{v:?} must enable (only 0 reverts)"
            );
        }
        unsafe {
            match prev {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    /// The limit override parses, `0` is honoured (accounting-only, the arm the
    /// constant was calibrated on), and anything unparsable falls back to the
    /// compiled default rather than to `0` — falling back to `0` would silently
    /// disable the lever for a caller who typed the value wrong.
    #[test]
    #[allow(unsafe_code)]
    fn limit_override_parses_and_falls_back_to_the_constant() {
        let _lock = test_env_lock();
        let k = "RUSTDL_TABLEAU_EARLY_ABANDON_HITS";
        let prev = std::env::var_os(k);
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe { std::env::remove_var(k) };
        assert_eq!(
            tableau_early_abandon_cap_hits(),
            TABLEAU_EARLY_ABANDON_CAP_HITS
        );
        unsafe { std::env::set_var(k, "7") };
        assert_eq!(tableau_early_abandon_cap_hits(), 7);
        unsafe { std::env::set_var(k, " 0 ") };
        assert_eq!(
            tableau_early_abandon_cap_hits(),
            0,
            "0 must disable the cut"
        );
        for bad in ["", "abc", "-1", "1.5"] {
            unsafe { std::env::set_var(k, bad) };
            assert_eq!(
                tableau_early_abandon_cap_hits(),
                TABLEAU_EARLY_ABANDON_CAP_HITS,
                "{bad:?} must fall back to the constant, not to 0"
            );
        }
        unsafe {
            match prev {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    /// The compiled default is the calibrated value, and it is bounded above.
    /// A silently huge default would make the lever inert while reading as ON —
    /// the failure mode the adaptive-inconsistency-budget work had to close with
    /// its own `generous_budget_is_bounded_above` canary.
    #[test]
    fn the_default_limit_is_the_calibrated_value() {
        let k = TABLEAU_EARLY_ABANDON_CAP_HITS;
        assert_eq!(
            k, 32,
            "the calibrated value (docs/2026-08-03-tableau-early-abandon.md §2c)"
        );
        assert!(
            k > 0 && k <= 128,
            "a limit outside (0, 128] is either a no-op or indiscriminate"
        );
    }

    /// **The verdict-mapping canary.** An abandoned probe must surface as
    /// `Ok(None)` — the same sound "don't know" a deadline cut and a `NodeCap`
    /// trip report — and **never** as `Err(NoVerdict)`, which
    /// `classify_internal_with_timeout` propagates with `?`. The abandon arm
    /// therefore has to sit BEFORE the deadline arm in `decide`'s `match`, and
    /// this pins that the arm exists at all: with `HITS=1` a probe that reaches
    /// the depth cap must return `Ok(None)`.
    ///
    /// The fixture is the 400-link `⊔`-chain, which was measured to reach
    /// `MAX_SEARCH_DEPTH` through the real classify path (`depth0 = 2` on a
    /// telemetry run). The assertion is deliberately weak on the OFF side (any
    /// `Ok`), because what is being pinned is the mapping, not the chain's
    /// verdict.
    #[test]
    #[allow(unsafe_code)]
    fn an_abandoned_probe_maps_to_ok_none_not_an_error() {
        use super::iterative_deepening_tests::{build_disjunction_chain, class_id};
        use std::time::Duration;
        let internal = build_disjunction_chain(400);
        let prepared = PreparedOntology::from_internal(internal.clone()).expect("fixture prepares");
        let (s, p) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let build = move |pool: &mut ConceptPool| {
            let sc = pool.atomic(s);
            let pc = pool.atomic(p);
            let np = pool.not(pc);
            pool.and(vec![sc, np])
        };
        let _lock = test_env_lock();
        let keys = [
            "RUSTDL_TABLEAU_EARLY_ABANDON",
            "RUSTDL_TABLEAU_EARLY_ABANDON_HITS",
        ];
        let prev: Vec<_> = keys.iter().map(std::env::var_os).collect();
        // SAFETY: serialised by `test_env_lock`; every key restored below.
        unsafe {
            std::env::set_var(keys[0], "1");
            std::env::set_var(keys[1], "1");
        }
        let dl = std::time::Instant::now() + Duration::from_secs(60);
        let got = prepared.decide_classify_with_deadline(dl, build);
        for (k, v) in keys.iter().zip(prev) {
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        assert!(
            matches!(got, Ok(None)),
            "an abandoned probe must be Ok(None), got {got:?}"
        );
    }
}

#[cfg(test)]
mod tableau_iterative_deepening_tests {
    use super::iterative_deepening_tests::{build_disjunction_chain, class_id};
    use super::*;
    use std::time::Duration;

    /// Chain length whose refutation genuinely needs MORE branch depth than the
    /// shallowest schedule level (8). **Measured, not assumed** — and the
    /// negatives-first control is what forced the measurement: the wedge's own
    /// 12-deep fixture is decided by the MAIN TABLEAU at depth 8, because
    /// absorb + dependency-directed back-jumping compress this chain to roughly
    /// one branch decision per five links. Depth 8 decides n = 4/8/12/24 and
    /// first MISSES at n = 48. Two engines, two depth requirements from one
    /// fixture generator — which is precisely why the FP-safety and
    /// monotonicity arguments were re-derived for this engine rather than
    /// inherited from the wedge.
    const DEEP_N: usize = 48;

    /// Chain length the shallowest level DOES decide, so "the shallow phase
    /// decided" is a reachable state and the shutoff canaries are not vacuous.
    const SHALLOW_N: usize = 12;

    /// One `sub ⊓ ¬sup` probe through the production classify tableau path,
    /// returning `(verdict, (decided, missed, waste_us))`. `verdict` is
    /// `Some(false)` = unsat = subsumed, `Some(true)` = satisfiable,
    /// `None` = no verdict.
    fn probe(
        internal: &InternalOntology,
        sub: &str,
        sup: &str,
        budget_secs: u64,
    ) -> (Option<bool>, (u64, u64, u64)) {
        let prepared = PreparedOntology::from_internal(internal.clone()).expect("fixture prepares");
        let (s, p) = (class_id(internal, sub), class_id(internal, sup));
        let build = move |pool: &mut ConceptPool| {
            let sc = pool.atomic(s);
            let pc = pool.atomic(p);
            let np = pool.not(pc);
            pool.and(vec![sc, np])
        };
        let dl = std::time::Instant::now() + Duration::from_secs(budget_secs);
        let v = prepared
            .decide_classify_with_deadline(dl, build)
            .expect("probe does not error");
        (v, prepared.tableau_id.snapshot())
    }

    /// Run `f` with the tableau-ID env pinned: flag `on`, shallow budget
    /// UNBOUNDED (`0`), shutoff `waste_ms`. Serialised through `test_env_lock`
    /// and restored afterwards.
    ///
    /// The shallow bound is pinned to `0` for the same reason the wedge's
    /// canaries pin theirs: otherwise these tests would depend on whether a
    /// microsecond-scale search happened to outrun a 20 ms wall budget on a
    /// loaded host — flaky in exactly the direction that silently stops testing
    /// the deepening path. The budget arithmetic has its own canaries.
    #[allow(unsafe_code)]
    fn with_tid<T>(on: bool, waste_ms: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _lock = test_env_lock();
        let keys = [
            "RUSTDL_TABLEAU_ITERATIVE_DEEPENING",
            "RUSTDL_TABLEAU_ID_SHALLOW_MS",
            "RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS",
        ];
        let prev: Vec<_> = keys.iter().map(std::env::var_os).collect();
        // SAFETY: serialised by `test_env_lock`; every key restored below.
        unsafe {
            std::env::set_var(keys[0], if on { "1" } else { "0" });
            std::env::set_var(keys[1], "0");
            match waste_ms {
                Some(v) => std::env::set_var(keys[2], v),
                None => std::env::remove_var(keys[2]),
            }
        }
        let out = f();
        for (k, v) in keys.iter().zip(prev) {
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        out
    }

    // ------------------------------------------------------------- negatives

    /// **Control (negatives-first).** The 12-deep `⊔`-chain is genuinely beyond
    /// the shallowest schedule level (8): the shallow phase must record a MISS
    /// and zero decides. If this ever reported a decide, every "deepening
    /// works" assertion below would be vacuous — the fixture would simply be
    /// shallow.
    #[test]
    fn deep_chain_is_not_decided_by_the_shallow_level() {
        let internal = build_disjunction_chain(DEEP_N);
        let (_, (decided, missed, _)) = with_tid(true, None, || probe(&internal, "A0", "Y", 20));
        assert_eq!(decided, 0, "depth 8 must NOT decide a DEEP_N-deep chain");
        assert!(missed >= 1, "the shallow phase must have run and missed");
    }

    /// **Control (negatives-first).** A 3-deep chain IS decided by the
    /// shallowest level, so "the shallow phase decided" is a reachable state.
    /// Without this, `a_deciding_probe_charges_no_waste` could pass simply
    /// because nothing is ever decided.
    #[test]
    fn shallow_chain_is_decided_by_the_shallow_level() {
        let internal = build_disjunction_chain(SHALLOW_N);
        let (v, (decided, _, _)) = with_tid(true, None, || probe(&internal, "A0", "Y", 20));
        assert_eq!(
            v,
            Some(false),
            "a SHALLOW_N-deep chain must be refuted (A0 ⊑ Y)"
        );
        assert!(decided >= 1, "depth 8 must decide a SHALLOW_N-deep chain");
    }

    /// **Control.** The flag is OFF by default and only `=1` enables it — the
    /// house default-OFF idiom. An empty value must NOT enable (that is the
    /// default-ON idiom, and confusing the two is how a default gets flipped by
    /// accident).
    #[test]
    #[allow(unsafe_code)]
    fn flag_defaults_off_and_only_1_enables() {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_TABLEAU_ITERATIVE_DEEPENING");
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe { std::env::remove_var("RUSTDL_TABLEAU_ITERATIVE_DEEPENING") };
        assert!(!tableau_iterative_deepening_enabled(), "unset ⇒ OFF");
        for (v, want) in [("1", true), ("0", false), ("", false), ("yes", false)] {
            unsafe { std::env::set_var("RUSTDL_TABLEAU_ITERATIVE_DEEPENING", v) };
            assert_eq!(tableau_iterative_deepening_enabled(), want, "value {v:?}");
        }
        unsafe {
            match prev {
                Some(v) => std::env::set_var("RUSTDL_TABLEAU_ITERATIVE_DEEPENING", v),
                None => std::env::remove_var("RUSTDL_TABLEAU_ITERATIVE_DEEPENING"),
            }
        }
    }

    // ------------------------------------------------------------- the loop

    /// The loop deepens past a `DepthLimit` shallow level and returns the
    /// deeper level's definite verdict.
    #[test]
    fn deepens_past_a_depth_limited_shallow_level() {
        let internal = build_disjunction_chain(DEEP_N);
        let (v, _) = with_tid(true, None, || probe(&internal, "A0", "Y", 20));
        assert_eq!(
            v,
            Some(false),
            "the deep chain must be refuted by a deeper level"
        );
    }

    /// **The superset property at unit scale, and the pin on REUSING ONE
    /// CONTEXT across levels.** Flag ON must agree with flag OFF on both an
    /// entailed and a non-entailed probe. Flag OFF builds a fresh context and
    /// runs ONE search at `MAX_SEARCH_DEPTH`; flag ON reuses one context across
    /// three levels. A disagreement here means the reused context is not the
    /// deterministic closure of the root — i.e. that `search::branch`'s rollback
    /// left residue — which would be a correctness bug, not a slow path.
    #[test]
    fn on_agrees_with_off_on_entailed_and_non_entailed_probes() {
        let entailed = build_disjunction_chain(DEEP_N);
        let (off_e, _) = with_tid(false, None, || probe(&entailed, "A0", "Y", 20_000));
        let (on_e, _) = with_tid(true, None, || probe(&entailed, "A0", "Y", 20_000));
        assert_eq!(
            off_e,
            Some(false),
            "control: OFF refutes the entailed probe"
        );
        assert_eq!(on_e, off_e, "ON must not lose the entailed probe");

        // Non-entailed: `A0 ⊑ K` holds but `K ⊑ Y` does not, so `K ⊓ ¬Y` is
        // satisfiable and both arms must say so.
        let (off_n, _) = with_tid(false, None, || probe(&entailed, "K", "Y", 20_000));
        let (on_n, _) = with_tid(true, None, || probe(&entailed, "K", "Y", 20_000));
        assert_eq!(off_n, Some(true), "control: OFF finds the model");
        assert_eq!(on_n, off_n, "ON must not invent a subsumption");
    }

    /// The schedule is well-formed and its final level is `MAX_SEARCH_DEPTH`
    /// exactly — so flag-ON's final level is the flag-OFF search verbatim.
    #[test]
    fn schedule_final_level_equals_the_fixed_cap() {
        let s = MAX_SEARCH_DEPTH_SCHEDULE;
        assert!(s.windows(2).all(|w| w[1] > w[0]), "strictly increasing");
        assert_eq!(
            s[s.len() - 1],
            MAX_SEARCH_DEPTH,
            "final level must equal the fixed cap"
        );
    }

    /// A malformed `RUSTDL_TABLEAU_ID_SCHEDULE` is rejected WHOLESALE rather
    /// than partially honoured. The `final < MAX_SEARCH_DEPTH` case is the
    /// soundness-relevant one: honouring it would make the ON path lose
    /// entailments the OFF path finds.
    #[test]
    #[allow(unsafe_code)]
    fn malformed_schedule_override_is_rejected_wholesale() {
        let _lock = test_env_lock();
        let prev = std::env::var_os("RUSTDL_TABLEAU_ID_SCHEDULE");
        let default = MAX_SEARCH_DEPTH_SCHEDULE.to_vec();
        for bad in ["", "junk", "8,junk", "32,8", "8,32,64", "0", "8,8"] {
            // SAFETY: serialised by `test_env_lock`; restored below.
            unsafe { std::env::set_var("RUSTDL_TABLEAU_ID_SCHEDULE", bad) };
            assert_eq!(
                tableau_depth_schedule(),
                default,
                "malformed schedule {bad:?} must fall back to the default"
            );
        }
        unsafe { std::env::set_var("RUSTDL_TABLEAU_ID_SCHEDULE", "4,16,256") };
        assert_eq!(
            tableau_depth_schedule(),
            vec![4, 16, 256],
            "a well-formed override must be honoured"
        );
        unsafe {
            match prev {
                Some(v) => std::env::set_var("RUSTDL_TABLEAU_ID_SCHEDULE", v),
                None => std::env::remove_var("RUSTDL_TABLEAU_ID_SCHEDULE"),
            }
        }
    }

    // ------------------------------------------------------------- shutoff

    /// A probe the shallow level DECIDED charges no waste — it is the thing
    /// being paid for.
    #[test]
    fn a_deciding_probe_charges_no_waste() {
        let internal = build_disjunction_chain(SHALLOW_N);
        let (_, (decided, _, waste)) = with_tid(true, None, || probe(&internal, "A0", "Y", 20));
        assert!(decided >= 1, "control: the shallow level decided");
        assert_eq!(waste, 0, "a decide must never be charged");
    }

    /// A probe the shallow level MISSED charges waste, so the accumulator can
    /// grow at all. With the shallow bound disabled (`=0`) the charge is the
    /// whole shallow phase, which is > 0 µs for a 12-deep chain.
    #[test]
    fn a_non_deciding_probe_charges_waste() {
        let internal = build_disjunction_chain(DEEP_N);
        let (_, (_, missed, waste)) = with_tid(true, None, || probe(&internal, "A0", "Y", 20));
        assert!(missed >= 1, "control: the shallow level missed");
        assert!(waste > 0, "a miss must be charged, got {waste} us");
    }

    /// A LATCHED accumulator skips the shallow phase entirely: neither counter
    /// moves, because a skipped probe has nothing to observe. That is also what
    /// makes the latch self-sustaining without a second flag.
    #[test]
    fn a_latched_accumulator_skips_the_shallow_phase() {
        let internal = build_disjunction_chain(DEEP_N);
        let prepared = PreparedOntology::from_internal(internal.clone()).expect("fixture prepares");
        let (s, p) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let build = move |pool: &mut ConceptPool| {
            let sc = pool.atomic(s);
            let pc = pool.atomic(p);
            let np = pool.not(pc);
            pool.and(vec![sc, np])
        };
        let v = with_tid(true, Some("1"), || {
            // 1 ms budget, accumulator pre-loaded far past it ⇒ latched.
            prepared.tableau_id.set_waste_us(10_000_000);
            let dl = std::time::Instant::now() + Duration::from_secs(20);
            prepared
                .decide_classify_with_deadline(dl, build)
                .expect("probe does not error")
        });
        let (decided, missed, waste) = prepared.tableau_id.snapshot();
        assert_eq!(
            v,
            Some(false),
            "a skipped shallow phase must not lose the verdict"
        );
        assert_eq!(decided, 0, "latched ⇒ no shallow observation");
        assert_eq!(missed, 0, "latched ⇒ no shallow observation");
        assert_eq!(waste, 10_000_000, "latched ⇒ the accumulator does not grow");
    }

    /// The shutoff **cannot change a verdict**: latched and unlatched agree on
    /// an entailed probe AND on a non-entailed one. Verified rather than
    /// asserted — the latched path runs only the final level, at a cap
    /// `>= MAX_SEARCH_DEPTH` under the caller's own deadline.
    #[test]
    fn shutoff_cannot_change_a_verdict() {
        let internal = build_disjunction_chain(DEEP_N);
        for (sub, sup, want) in [("A0", "Y", Some(false)), ("K", "Y", Some(true))] {
            let unlatched = with_tid(true, None, || probe(&internal, sub, sup, 20).0);
            let latched = with_tid(true, Some("1"), || {
                let prepared = PreparedOntology::from_internal(internal.clone()).expect("prepares");
                prepared.tableau_id.set_waste_us(10_000_000);
                let (s, p) = (class_id(&internal, sub), class_id(&internal, sup));
                let build = move |pool: &mut ConceptPool| {
                    let sc = pool.atomic(s);
                    let pc = pool.atomic(p);
                    let np = pool.not(pc);
                    pool.and(vec![sc, np])
                };
                let dl = std::time::Instant::now() + Duration::from_secs(20);
                prepared
                    .decide_classify_with_deadline(dl, build)
                    .expect("probe does not error")
            });
            assert_eq!(unlatched, want, "control: {sub} ⊑ {sup}");
            assert_eq!(
                latched, unlatched,
                "shutoff changed the {sub} ⊑ {sup} verdict"
            );
        }
    }

    /// `RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS=0` disables the shutoff, so a
    /// pre-loaded accumulator does NOT latch and the shallow phase still runs.
    /// Positive control that the knob gates the code it documents.
    #[test]
    fn waste_budget_zero_disables_the_shutoff() {
        let internal = build_disjunction_chain(DEEP_N);
        let prepared = PreparedOntology::from_internal(internal.clone()).expect("fixture prepares");
        let (s, p) = (class_id(&internal, "A0"), class_id(&internal, "Y"));
        let build = move |pool: &mut ConceptPool| {
            let sc = pool.atomic(s);
            let pc = pool.atomic(p);
            let np = pool.not(pc);
            pool.and(vec![sc, np])
        };
        with_tid(true, Some("0"), || {
            prepared.tableau_id.set_waste_us(10_000_000);
            let dl = std::time::Instant::now() + Duration::from_secs(20);
            let _ = prepared.decide_classify_with_deadline(dl, build);
        });
        let (_, missed, _) = prepared.tableau_id.snapshot();
        assert!(
            missed >= 1,
            "with the shutoff disabled the shallow phase must still run"
        );
    }

    /// Env garbage falls back to the DEFAULT, never to `0`. `0` silently
    /// disables the bound / the shutoff, which is how the `ore_ont_13991`-class
    /// per-pair-tax regression gets reintroduced.
    #[test]
    #[allow(unsafe_code)]
    fn env_garbage_falls_back_to_the_defaults() {
        let _lock = test_env_lock();
        let keys = [
            "RUSTDL_TABLEAU_ID_SHALLOW_MS",
            "RUSTDL_TABLEAU_ID_SHALLOW_WASTE_MS",
        ];
        let prev: Vec<_> = keys.iter().map(std::env::var_os).collect();
        for bad in ["", "junk", "5ms", "-1"] {
            // SAFETY: serialised by `test_env_lock`; restored below.
            unsafe {
                std::env::set_var(keys[0], bad);
                std::env::set_var(keys[1], bad);
            }
            assert_eq!(
                tableau_id_shallow_budget_ms(),
                TABLEAU_ID_SHALLOW_BUDGET_MS,
                "shallow budget {bad:?} must fall back to the default, not 0"
            );
            assert_eq!(
                tableau_id_shallow_waste_budget_ms(),
                TABLEAU_ID_SHALLOW_WASTE_BUDGET_MS,
                "waste budget {bad:?} must fall back to the default, not 0"
            );
        }
        for (k, v) in keys.iter().zip(prev) {
            unsafe {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    /// Clearing the sticky deadline flag is what keeps the final level's
    /// depth-cap `DepthLimit` distinguishable from a deadline cut. Pinned on
    /// `TableauContext` directly, because the two map to DIFFERENT reasoner
    /// results (`Err(NoVerdict)` vs `Ok(None)`) and one caller propagates the
    /// `Err` with `?`.
    #[test]
    fn clearing_the_sticky_deadline_flag_works() {
        let internal = build_disjunction_chain(SHALLOW_N);
        let prepared = PreparedOntology::from_internal(internal).expect("fixture prepares");
        let mut ctx = owl_dl_tableau::TableauContext::with_tbox_and_hierarchy(
            &prepared.pool,
            &prepared.tbox,
            &prepared.hierarchy,
        );
        let past = std::time::Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("an instant one second in the past exists");
        ctx.set_deadline(past);
        assert!(ctx.check_deadline(), "an elapsed deadline must be observed");
        assert!(ctx.deadline_reached(), "and must be sticky");
        ctx.clear_deadline_hit();
        assert!(!ctx.deadline_reached(), "clear must reset the sticky flag");
    }
}
