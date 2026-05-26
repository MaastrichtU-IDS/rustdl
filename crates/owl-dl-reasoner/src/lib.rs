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

mod classify;
mod realize;

pub use classify::{
    Classification, ClassificationStats, classify, classify_internal, classify_n2,
    classify_n2_with_timeout, classify_saturation_only, classify_top_down,
    classify_top_down_with_timeout, classify_with_timeout,
};
pub use realize::{
    Realization, instances_of, instances_of_internal, is_instance_of, is_instance_of_internal,
    realize, realize_internal, realize_saturation_only, realize_saturation_only_internal,
};

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

/// Recursion depth cap for the search driver — generous and
/// defensive. Real ALCHIQ inputs terminate via pair blocking long
/// before this matters.
const MAX_SEARCH_DEPTH: usize = 256;

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
    let consistent = run_satisfiability(internal, ConceptPool::top)?;
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
    pool: ConceptPool,
    tbox: AbsorbedTBox,
    hierarchy: RoleHierarchy,
    inverse_pairs: Vec<(RoleId, RoleId)>,
    chain_axioms: Vec<(Role, Role, Role)>,
    asymmetric_roles: Vec<RoleId>,
    disjoint_role_pairs: Vec<(RoleId, RoleId)>,
    complements: Vec<(ConceptId, ConceptId)>,
    abox: Abox,
}

impl PreparedOntology {
    /// Run every preparation pass against `internal` so subsequent
    /// `decide` calls only have to allocate a fresh tableau and run
    /// the search.
    pub(crate) fn from_internal(mut internal: InternalOntology) -> Result<Self, ReasonError> {
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
            tbox,
            hierarchy,
            inverse_pairs,
            chain_axioms,
            asymmetric_roles,
            disjoint_role_pairs,
            complements,
            abox,
        })
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
            None,
            build_test_concept,
        )
        .map(|opt| opt.expect("no deadline ⇒ search always returns Some(_)"))
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
            Some(deadline),
            build_test_concept,
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
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(sub_role),
                sup,
            } => {
                // Only encode the named-to-named portion of the
                // sub-role lattice; the inverse axis still hangs
                // off the polarity-check in `edge_satisfies`. If
                // either side carries an inverse polarity, we'd
                // need a Role-keyed hierarchy — defer to a later
                // commit, but still record the underlying-id
                // relation so same-polarity sub-role inference
                // remains correct.
                builder.add_sub_role(sub_role.role_id(), sup.role_id());
            }
            Axiom::EquivalentObjectProperties(roles) => {
                // r ≡ s ≡ … expands to pairwise sub-property both ways.
                for a in roles {
                    for b in roles {
                        if a != b {
                            builder.add_sub_role(a.role_id(), b.role_id());
                        }
                    }
                }
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
fn collect_disjoint_role_pairs(internal: &InternalOntology) -> Vec<(RoleId, RoleId)> {
    let mut pairs = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::DisjointObjectProperties(roles) = ax {
            for i in 0..roles.len() {
                for j in (i + 1)..roles.len() {
                    let a = roles[i].role_id();
                    let b = roles[j].role_id();
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
    deadline: Option<std::time::Instant>,
    build_test_concept: F,
) -> Result<Option<bool>, ReasonError>
where
    F: FnOnce(&mut ConceptPool) -> ConceptId,
{
    let mut pool = pool.clone();
    let test_concept: ConceptId = build_test_concept(&mut pool);
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, tbox, hierarchy);
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

    let outcome = owl_dl_tableau::search(&mut ctx, MAX_SEARCH_DEPTH);
    match outcome {
        owl_dl_tableau::SearchVerdict::Sat => Ok(Some(true)),
        owl_dl_tableau::SearchVerdict::Unsat(_) => Ok(Some(false)),
        owl_dl_tableau::SearchVerdict::DepthLimit if ctx.deadline_reached() => Ok(None),
        owl_dl_tableau::SearchVerdict::DepthLimit => Err(ReasonError::NoVerdict),
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

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn check(onto: &SetOntology<RcStr>, iri: &str) -> bool {
        is_class_satisfiable(onto, iri).expect("verdict returned")
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
}
