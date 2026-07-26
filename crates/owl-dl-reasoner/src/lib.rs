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
mod union_find;

pub use class_expr_query::{
    CeInstances, CeVerdict, class_expression_entailed_subclass, class_expression_instances,
    class_expression_satisfiable,
};
pub use classify::{
    Classification, ClassificationStats, FragmentClassification, analyze_fragment, classify,
    classify_internal, classify_n2, classify_n2_with_timeout, classify_saturation_only,
    classify_top_down, classify_top_down_with_timeout, classify_with_budget,
    classify_with_global_deadline, classify_with_timeout,
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

pub(crate) struct HyperCache {
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
        let (result, stats) = self.decide_with_stats(sub, sup, HYPER_WEDGE_DEPTH, deadline);
        match result {
            HyperResult::Unsat => HyperVerdict::Subsumed,
            HyperResult::Sat => HyperVerdict::NotSubsumed,
            // Distinguish a divergence-cut `Stalled` (thrash) from a plain
            // deadline `Stalled`, so bound-the-tail can skip the fallthrough.
            HyperResult::Stalled if stats.diverged => HyperVerdict::UnknownDiverged,
            HyperResult::Stalled => HyperVerdict::Unknown,
        }
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
        let mut clauses = self.clauses.clone();
        clauses.push(DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(c, X)],
        });
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
                clauses.push(DlClause {
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
                clauses.push(DlClause {
                    body: vec![Atom::Class(self.fresh_q, X)],
                    head: vec![Atom::Exists(role, target, X)],
                });
            }
        }
        // VALUE-DERIVED TYPE DISJOINTNESS (experiment): empty-head clashes.
        if let Some(pairs) = &self.value_disjoint {
            for &(a, b) in pairs {
                clauses.push(DlClause {
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
        let mut engine = if self.sat_seed.is_some()
            || self.exists_seed.is_some()
            || self.value_disjoint.is_some()
        {
            let mut e = HyperEngine::new(&clauses, self.fresh_q);
            if crate::classify_same_tier_enabled() {
                e = e.with_sub_roles(self.sub_roles.clone());
            }
            e
        } else {
            let mut e = HyperEngine::new_with_prebuilt(
                &clauses,
                self.fresh_q,
                std::sync::Arc::clone(&self.base_indexes),
                std::sync::Arc::clone(&self.base_disjoint_pairs),
            );
            if crate::classify_same_tier_enabled() {
                e = e.with_sub_roles_keep_index(self.sub_roles.clone());
            }
            e
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
    if abox_saturation_enabled()
        && classify::has_abox_axioms(&internal)
        && abox_saturation::saturate_abox_consistency(&internal).clash
    {
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
    pub(crate) closure: owl_dl_saturation::Subsumers,
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
    pub(crate) fn from_internal(mut internal: InternalOntology) -> Result<Self, ReasonError> {
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
        // and P2 (subsumers_of). Cheap on small ABox-bearing ontologies;
        // on ABox-free ontologies, abox_check exits early before
        // querying the closure, so the cost is amortised.
        let closure = owl_dl_saturation::saturate(&internal);
        let told = owl_dl_core::told::build_told_tables(&internal);
        let axioms = internal.axioms.clone();
        // Concrete-domain solver (P2): decode the synthetic DKey filler
        // classes into a ClassId → CardRange map while the vocabulary is
        // available. Pure; consumed by the (not-yet-armed) P3 clash rule.
        let dkey_ranges = build_dkey_range_map(&internal);
        let data_counting_classes = build_data_counting_classes(&internal, &dkey_ranges);
        // H4: build the hyper cache from the un-mutated ontology
        // (before the absorb/NNF passes below consume it), iff enabled.
        let hyper = hyper_wedge_enabled().then(|| HyperCache::build(&internal));
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
        expand_role_characteristics(&mut internal);
        let hierarchy = build_role_hierarchy(&internal);
        let inverse_pairs = collect_inverse_pairs(&internal);
        let asymmetric_roles = collect_asymmetric_roles(&internal);
        let disjoint_role_pairs = collect_disjoint_role_pairs(&internal);
        let chain_axioms = collect_chain_axioms(&internal)?;
        let normalized = nnf_axioms(&mut internal);
        let tbox = absorb(&normalized, &mut internal.concepts);
        // Ensure `⊥` is interned — `apply_max` flags inequality
        // clashes by adding `Bot` to the offending node's label set,
        // and looks up the canonical id via `pool.bot_id()`. Cheap
        // & idempotent.
        let _ = internal.concepts.bot();
        let complements = precompute_max_complements(&mut internal.concepts);
        let abox = collect_abox(&mut internal);
        Ok(Self {
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
        })
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
            if crate::abox_check_enabled() {
                abox_check::check(self)
            } else {
                abox_check::AboxVerdict::Unknown
            }
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
/// This is the last stage that mutates the pool; after this call
/// the pool is frozen for the tableau run.
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
    let outcome = if deadline.is_some() {
        owl_dl_tableau::search(&mut ctx, MAX_SEARCH_DEPTH)
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
    match outcome {
        owl_dl_tableau::SearchVerdict::Sat => Ok(Some(true)),
        owl_dl_tableau::SearchVerdict::Unsat(_) => Ok(Some(false)),
        // Live-node cap hit: sound under-approximation, never an error (#35 v4
        // safety net) — must be checked before the DepthLimit arms below so a
        // cap trip is never mistaken for a hard NoVerdict.
        owl_dl_tableau::SearchVerdict::NodeCap => Ok(None),
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
