//! Individual-level reasoning: instance checks and realization.
//!
//! All three entry points reduce to satisfiability via the standard
//! nominal trick: `KB ⊨ C(a)` iff `{a} ⊓ ¬C` is unsatisfiable in the
//! KB. The test concept seeds a fresh root node carrying both
//! `Nominal(a)` (which the tableau's nominal-assignment rule will
//! merge with the canonical witness for `a`) and `¬C`.
//!
//! Realization computes, for each declared individual, the *most
//! specific* named classes it must belong to in every model. Naive
//! implementation: for every (individual, class) pair, run an
//! instance check; then prune any class that has a strict subclass
//! also in the type set. Phase 6's saturation engine accelerates
//! the dense per-pair loop.

use std::collections::{HashMap, HashSet};

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;

use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{Axiom, ClassId, ConceptExpr, IndividualId, InternalOntology};
use owl_dl_saturation::{Subsumers, saturate};

use crate::PreparedOntology;
use crate::ReasonError;
use crate::classify::{classify_saturation_only_internal, classify_top_down_internal};

/// `(entailed_types, hasse_leaves)` for one individual — returned
/// by the parallel realisation worker so the outer loop can stitch
/// the maps together.
type IndivResult = (Vec<String>, Vec<String>);

/// Whether the pseudo-model realization shortcut is enabled. **Default ON**;
/// set `RUSTDL_PSEUDO_MODEL=0` (or empty) to disable (e.g. for debugging or to
/// A/B the speedup). Matches rustdl's other default-on env-gate convention.
fn pseudo_model_enabled() -> bool {
    std::env::var_os("RUSTDL_PSEUDO_MODEL").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Decide whether `KB ⊨ class_iri(individual_iri)`. Returns `true`
/// iff `individual_iri` is provably an instance of `class_iri` in
/// every model of `ontology`.
///
/// Reduction: build the test concept `{individual_iri} ⊓ ¬class_iri`
/// and run satisfiability — instance-of holds iff *unsatisfiable*.
///
/// # Errors
///
/// See [`ReasonError`]. Unknown class or individual IRI surfaces as
/// [`ReasonError::UnknownClass`] (we reuse the same variant — a
/// dedicated `UnknownIndividual` would be a nice follow-up but
/// isn't load-bearing yet).
pub fn is_instance_of<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
    individual_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_instance_of_internal(&internal, class_iri, individual_iri)
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_instance_of_internal(
    internal: &InternalOntology,
    class_iri: &str,
    individual_iri: &str,
) -> Result<bool, ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let individual_id = internal
        .vocabulary
        .individual_id(individual_iri)
        .ok_or_else(|| ReasonError::UnknownClass(individual_iri.to_owned()))?;
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;
    instance_check_with_closure(
        internal,
        &closure,
        &prepared,
        class_id,
        individual_id,
        None,
        None,
    )
}

/// Saturation-only counterpart of [`is_instance_of`]. Reports
/// `true` iff the closure derived from told class assertions
/// already entails membership; the `{a} ⊓ ¬C` tableau probe is
/// skipped. Sound under-approximation: positive answers are
/// genuinely entailed, but memberships that need tableau
/// reasoning are missed.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_instance_of_saturation_only<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
    individual_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_instance_of_saturation_only_internal(&internal, class_iri, individual_iri)
}

/// Internal entry point for [`is_instance_of_saturation_only`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_instance_of_saturation_only_internal(
    internal: &InternalOntology,
    class_iri: &str,
    individual_iri: &str,
) -> Result<bool, ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let individual_id = internal
        .vocabulary
        .individual_id(individual_iri)
        .ok_or_else(|| ReasonError::UnknownClass(individual_iri.to_owned()))?;
    let closure = saturate(internal);
    Ok(instance_check_closure_only(
        internal,
        &closure,
        class_id,
        individual_id,
    ))
}

/// Single instance check that consults the saturation closure first.
///
/// Three saturation fast paths, all of which short-circuit the
/// tableau. For each told class membership of `individual_id`
/// (described below), if its asserted class is a subsumer of
/// `class_id` in the EL closure, the answer is `yes`:
///
/// 1. **`ClassAssertion(D, a)`** — direct told membership.
/// 2. **`ObjectPropertyAssertion(r, a, _)` with `r`'s domain `Dom`**
///    — `a` is in `Dom` via the property domain axiom, transitively
///    via the role hierarchy.
/// 3. **`ObjectPropertyAssertion(r, _, a)` with `r`'s range `Rng`**
///    — `a` is in `Rng` via the property range axiom, transitively
///    via the role hierarchy.
///
/// Falls through to the `{a} ⊓ ¬C` satisfiability reduction otherwise.
fn instance_check_with_closure(
    internal: &InternalOntology,
    closure: &Subsumers,
    prepared: &PreparedOntology,
    class_id: ClassId,
    individual_id: IndividualId,
    per_check_timeout: Option<std::time::Duration>,
    base_types: Option<&std::collections::HashSet<u32>>,
) -> Result<bool, ReasonError> {
    for told in told_classes_of(internal, individual_id) {
        if closure.contains(told, class_id) {
            return Ok(true);
        }
    }

    // Pseudo-model shortcut: `base_types` is this individual's type set in ONE
    // witness model of the ABox (a clash-free completion). If `class_id` is
    // absent there, that model places the individual outside the class, so
    // `KB ⊭ class_id(a)` — not an instance, with no need for a full probe.
    // Sound AND complete-preserving: a genuinely entailed type holds in EVERY
    // model, hence is present here, so it is never skipped.
    if let Some(bt) = base_types
        && !bt.contains(&class_id.index())
    {
        return Ok(false);
    }

    // KB ⊨ C(a) iff `KB ∪ {a : ¬C}` is inconsistent.
    //
    // Primary path: the ABox-seeded hypertableau wedge. It is a terminating
    // and (corpus-verified) complete decision procedure on the
    // nominal + inverse-role + number-restriction fragment (HF2 double
    // blocking + HF3 `≥n`/`≠` + HF4 NN-rule) — precisely the SHOIQ/SROIQ
    // case on which the subset-blocking tableau (`prepared.decide`) does not
    // halt, because the `{a} ⊓ ¬C` root is a nominal and nominal nodes are
    // unblockable. `Unsat` ⟹ entailed instance; `Sat` ⟹ not an instance
    // (sound under HF5 trust-Sat, classify's trust level).
    let deadline = per_check_timeout.map(|budget| std::time::Instant::now() + budget);
    match prepared.instance_check_wedge(class_id, individual_id, deadline) {
        Some(owl_dl_tableau::hyper::HyperResult::Unsat) => return Ok(true),
        // `Sat` ⟹ not an instance (sound under HF5 trust-Sat). `Stalled`
        // (engine cap / deadline elapsed without a verdict) ⟹ not *provably*
        // an instance — a sound under-approximation (no false positives),
        // matching `classify`'s per-pair-timeout semantics. Both decline.
        Some(
            owl_dl_tableau::hyper::HyperResult::Sat | owl_dl_tableau::hyper::HyperResult::Stalled,
        ) => {
            return Ok(false);
        }
        // Wedge unavailable (no ABox, or RUSTDL_WEDGE_CONSISTENCY=0): fall
        // back to the subset-blocking tableau below.
        None => {}
    }

    // Fallback (wedge disabled): `{a} ⊓ ¬C` via the subset-blocking tableau.
    // Bounded by the per-check budget so it cannot hang on the hard fragment;
    // unbounded only when no budget is set (the historical behaviour, and the
    // only path that can loop on nominal-heavy inputs — hence the wedge).
    if let Some(budget) = per_check_timeout {
        let dl = std::time::Instant::now() + budget;
        match prepared.decide_with_deadline(dl, move |pool| {
            let cls = pool.atomic(class_id);
            let not_cls = pool.not(cls);
            let nom = pool.nominal(individual_id);
            pool.and(vec![nom, not_cls])
        })? {
            Some(sat) => Ok(!sat),
            None => Ok(false),
        }
    } else {
        let sat = prepared.decide(move |pool| {
            let cls = pool.atomic(class_id);
            let not_cls = pool.not(cls);
            let nom = pool.nominal(individual_id);
            pool.and(vec![nom, not_cls])
        })?;
        Ok(!sat)
    }
}

/// Saturation-only counterpart to [`instance_check_with_closure`].
/// Reports `true` iff a told class of `individual_id` already has
/// `class_id` in its EL-closure subsumer set. Skips the
/// `{a} ⊓ ¬C` tableau probe entirely — sound under-approximation
/// matching [`crate::classify_saturation_only`].
fn instance_check_closure_only(
    internal: &InternalOntology,
    closure: &Subsumers,
    class_id: ClassId,
    individual_id: IndividualId,
) -> bool {
    told_classes_of(internal, individual_id)
        .into_iter()
        .any(|told| closure.contains(told, class_id))
}

/// Collect every atomic class that `individual_id` is *told* to
/// belong to:
///
/// - Direct: every `ClassAssertion(D, individual)` with `D` atomic.
/// - Via domain: every `ObjectPropertyAssertion(r, individual, _)`
///   where some `ObjectPropertyDomain(r', Dom)` axiom applies for
///   `r ⊑ r'` (named-role-only; `r'` matches when `r` and `r'`
///   share an underlying `RoleId`).
/// - Via range: every `ObjectPropertyAssertion(r, _, individual)`
///   where some `ObjectPropertyRange(r', Rng)` axiom applies under
///   the same conditions.
fn told_classes_of(internal: &InternalOntology, individual_id: IndividualId) -> Vec<ClassId> {
    let mut out = Vec::new();
    for axiom in &internal.axioms {
        match axiom {
            Axiom::ClassAssertion { class, individual } if *individual == individual_id => {
                if let ConceptExpr::Atomic(id) = internal.concepts.get(*class) {
                    out.push(*id);
                }
            }
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                // Inverse-role property assertions: the converter
                // swaps subject/object so the stored role is always
                // named; we don't try to second-guess that here and
                // simply use `role.role_id()`.
                let used_role_id = role.role_id();
                if *subject == individual_id {
                    for dom in domains_for_role(internal, used_role_id) {
                        out.push(dom);
                    }
                }
                if *object == individual_id {
                    for rng in ranges_for_role(internal, used_role_id) {
                        out.push(rng);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn domains_for_role(internal: &InternalOntology, role_id: owl_dl_core::RoleId) -> Vec<ClassId> {
    let mut out = Vec::new();
    for axiom in &internal.axioms {
        if let Axiom::ObjectPropertyDomain { role, domain } = axiom
            && !role.is_inverse()
            && role.role_id() == role_id
            && let ConceptExpr::Atomic(id) = internal.concepts.get(*domain)
        {
            out.push(*id);
        }
    }
    out
}

fn ranges_for_role(internal: &InternalOntology, role_id: owl_dl_core::RoleId) -> Vec<ClassId> {
    let mut out = Vec::new();
    for axiom in &internal.axioms {
        if let Axiom::ObjectPropertyRange { role, range } = axiom
            && !role.is_inverse()
            && role.role_id() == role_id
            && let ConceptExpr::Atomic(id) = internal.concepts.get(*range)
        {
            out.push(*id);
        }
    }
    out
}

/// All declared individuals that `KB` provably places in `class_iri`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn instances_of<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<Vec<String>, ReasonError> {
    let internal = convert_ontology(ontology)?;
    instances_of_internal(&internal, class_iri)
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn instances_of_internal(
    internal: &InternalOntology,
    class_iri: &str,
) -> Result<Vec<String>, ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;
    let mut out = Vec::new();
    for idx in 0..internal.vocabulary.num_individuals() {
        let individual_id =
            IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
        if instance_check_with_closure(
            internal,
            &closure,
            &prepared,
            class_id,
            individual_id,
            None,
            None,
        )? {
            out.push(internal.vocabulary.individual_iri(individual_id).to_owned());
        }
    }
    Ok(out)
}

/// Saturation-only counterpart of [`instances_of`]. Returns the
/// list of individuals provably in `class_iri` via the EL closure
/// alone — every tableau probe is skipped. Sound
/// under-approximation; large `ABox` queries that do not finish under the
/// default path remain tractable here.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn instances_of_saturation_only<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<Vec<String>, ReasonError> {
    let internal = convert_ontology(ontology)?;
    instances_of_saturation_only_internal(&internal, class_iri)
}

/// Internal entry point for [`instances_of_saturation_only`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn instances_of_saturation_only_internal(
    internal: &InternalOntology,
    class_iri: &str,
) -> Result<Vec<String>, ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let closure = saturate(internal);
    let mut out = Vec::new();
    for idx in 0..internal.vocabulary.num_individuals() {
        let individual_id =
            IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
        if instance_check_closure_only(internal, &closure, class_id, individual_id) {
            out.push(internal.vocabulary.individual_iri(individual_id).to_owned());
        }
    }
    Ok(out)
}

/// Per-individual realization: every entailed type plus the
/// most-specific named classes (the leaves of the subclass relation
/// restricted to the entailed types).
#[derive(Debug, Clone, Default)]
pub struct Realization {
    /// All declared individual IRIs that the realization examined.
    individuals: Vec<String>,
    /// individual → all named classes entailed at that individual
    /// (full set; not the Hasse leaves).
    entailed_types: HashMap<String, Vec<String>>,
    /// individual → the most-specific entailed classes (Hasse leaves
    /// of the entailed set under the KB's subclass relation).
    most_specific_types: HashMap<String, Vec<String>>,
}

impl Realization {
    #[must_use]
    pub fn individuals(&self) -> &[String] {
        &self.individuals
    }

    #[must_use]
    pub fn entailed_types(&self, individual_iri: &str) -> &[String] {
        static EMPTY: Vec<String> = Vec::new();
        self.entailed_types
            .get(individual_iri)
            .map_or(EMPTY.as_slice(), Vec::as_slice)
    }

    #[must_use]
    pub fn most_specific_types(&self, individual_iri: &str) -> &[String] {
        static EMPTY: Vec<String> = Vec::new();
        self.most_specific_types
            .get(individual_iri)
            .map_or(EMPTY.as_slice(), Vec::as_slice)
    }
}

/// Realize every declared individual: compute entailed types and the
/// most-specific named classes per individual.
///
/// Algorithm (naive):
/// 1. Classify the ontology once to obtain the subclass matrix.
/// 2. For each individual, run an instance check against every
///    (satisfiable) class.
/// 3. From each individual's entailed-type set, prune classes that
///    have a strict subclass also in the set — leaving only the
///    Hasse leaves.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize<A: ForIRI>(ontology: &SetOntology<A>) -> Result<Realization, ReasonError> {
    let internal = convert_ontology(ontology)?;
    realize_internal(&internal)
}

/// Like [`realize`] but bounds each per-individual instance check by
/// `per_check_timeout_ms`. A pair whose tableau probe does not finish in
/// time is recorded as "not an instance" — a sound under-approximation
/// (no spurious types), symmetric to `classify`'s `per_pair_timeout_ms`.
/// `None` or `Some(0)` means unbounded (identical to [`realize`]).
///
/// This is the entry the Python `materialize_inferred_class_assertions`
/// binding uses: the `{a} ⊓ ¬C` reduction can otherwise hang on
/// nominal-heavy SHOIQ/SROIQ `ABoxes` (nominal nodes are unblockable).
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    per_check_timeout_ms: Option<u64>,
) -> Result<Realization, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let per_check = per_check_timeout_ms
        .filter(|&ms| ms > 0)
        .map(std::time::Duration::from_millis);
    realize_internal_with_timeout(&internal, per_check)
}

/// Saturation-only realization. Skips every tableau probe (both
/// during classification and during per-individual type inference)
/// and reports only the type assignments derivable from the EL
/// closure and told class assertions.
///
/// Returns a sound under-approximation of [`realize`]: every
/// `(individual, class)` pair reported is genuinely entailed, but
/// memberships that need tableau reasoning to confirm
/// (cardinality, disjunction-with-clash, …) are missed. On large
/// mostly-EL workloads this is dramatically faster than [`realize`]
/// — symmetric to [`crate::classify_saturation_only`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_saturation_only<A: ForIRI>(
    ontology: &SetOntology<A>,
) -> Result<Realization, ReasonError> {
    let internal = convert_ontology(ontology)?;
    realize_saturation_only_internal(&internal)
}

/// Internal entry point for [`realize_saturation_only`]. Skips
/// every tableau probe.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_saturation_only_internal(
    internal: &InternalOntology,
) -> Result<Realization, ReasonError> {
    let hierarchy = classify_saturation_only_internal(internal)?;
    let class_iris: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
    let unsat: HashSet<&str> = hierarchy.unsatisfiable_classes().into_iter().collect();
    let satisfiable: Vec<(usize, &str)> = class_iris
        .iter()
        .enumerate()
        .filter(|(_, iri)| !unsat.contains(iri.as_str()))
        .map(|(i, iri)| (i, iri.as_str()))
        .collect();

    let individual_iris: Vec<String> = (0..internal.vocabulary.num_individuals())
        .map(|i| {
            internal
                .vocabulary
                .individual_iri(IndividualId::new(
                    u32::try_from(i).expect("individual count fits in u32"),
                ))
                .to_owned()
        })
        .collect();

    let closure = saturate(internal);

    let per_individual: Vec<IndivResult> = individual_iris
        .par_iter()
        .enumerate()
        .map(|(idx, _iri)| {
            let individual_id =
                IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
            let mut types: Vec<&str> = Vec::new();
            for (class_idx, class_iri) in &satisfiable {
                let class_id = ClassId::new(u32::try_from(*class_idx).expect("class fits in u32"));
                if instance_check_closure_only(internal, &closure, class_id, individual_id) {
                    types.push(class_iri);
                }
            }
            let leaves: Vec<String> = types
                .iter()
                .filter(|&&cls| {
                    !types.iter().any(|&other| {
                        other != cls
                            && hierarchy.is_subclass(other, cls)
                            && !hierarchy.is_subclass(cls, other)
                    })
                })
                .map(|s| (*s).to_owned())
                .collect();
            let types_owned: Vec<String> = types.into_iter().map(str::to_owned).collect();
            (types_owned, leaves)
        })
        .collect();
    let mut entailed_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut most_specific_types: HashMap<String, Vec<String>> = HashMap::new();
    for (iri, (types_owned, leaves)) in individual_iris.iter().zip(per_individual) {
        entailed_types.insert(iri.clone(), types_owned);
        most_specific_types.insert(iri.clone(), leaves);
    }
    Ok(Realization {
        individuals: individual_iris,
        entailed_types,
        most_specific_types,
    })
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_internal(internal: &InternalOntology) -> Result<Realization, ReasonError> {
    realize_internal_with_timeout(internal, None)
}

/// Internal entry point for [`realize_with_timeout`]. `per_check`
/// bounds each per-individual `{a} ⊓ ¬C` tableau probe; `None` is
/// unbounded (the historical [`realize_internal`] behaviour).
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_internal_with_timeout(
    internal: &InternalOntology,
    per_check: Option<std::time::Duration>,
) -> Result<Realization, ReasonError> {
    // Use the top-down classifier — the same default as the public
    // `classify` entry. The N² pair-sweep that this previously
    // called (`classify_internal`) DNFs on real ontologies and
    // forced any realize call on SIO-scale inputs to time out
    // before per-individual probing ever started.
    let hierarchy = classify_top_down_internal(internal, None, None)?;
    let class_iris: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
    // Unsatisfiable classes are entailed by every individual under
    // any inconsistent slice — skip the per-individual check there;
    // we test only satisfiable classes.
    let unsat: HashSet<&str> = hierarchy.unsatisfiable_classes().into_iter().collect();
    let satisfiable: Vec<(usize, &str)> = class_iris
        .iter()
        .enumerate()
        .filter(|(_, iri)| !unsat.contains(iri.as_str()))
        .map(|(i, iri)| (i, iri.as_str()))
        .collect();

    let individual_iris: Vec<String> = (0..internal.vocabulary.num_individuals())
        .map(|i| {
            internal
                .vocabulary
                .individual_iri(IndividualId::new(
                    u32::try_from(i).expect("individual count fits in u32"),
                ))
                .to_owned()
        })
        .collect();

    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // Candidate restriction (completeness-preserving): only instance-check
    // classes that are positively *derivable* (occur in some clause head).
    // A class in no head can only be a member if asserted, and assertions
    // become `{a} ⊑ C` clauses whose `C` is on a head — so it is still a
    // candidate. Non-candidates (e.g. primitive leaf classes) can never be
    // newly entailed, so dropping them is sound and complete, and it avoids
    // the expensive tableau probe that would only ever return "not a member".
    // When the wedge is unavailable, keep every satisfiable class.
    let satisfiable: Vec<(usize, &str)> = match prepared.realize_candidate_classes() {
        Some(candidates) => satisfiable
            .into_iter()
            .filter(|(i, _)| candidates.contains(&(u32::try_from(*i).unwrap_or(u32::MAX))))
            .collect(),
        None => satisfiable,
    };
    if std::env::var_os("RUSTDL_TRACE").is_some() {
        eprintln!(
            "realize: {} individuals × {} candidate classes (of {} satisfiable) = {} probes",
            individual_iris.len(),
            satisfiable.len(),
            class_iris.len(),
            individual_iris.len() * satisfiable.len()
        );
    }
    // Pseudo-model shortcut (DEFAULT ON; disable with RUSTDL_PSEUDO_MODEL=0):
    // compute ONE witness model of the ABox; per individual its type set lets
    // us refute most (individual, class) pairs without a full wedge probe —
    // sound and completeness-preserving (a genuinely entailed type holds in
    // every model, hence is present in this one and is never skipped).
    // Validated == HermiT on closure-derived (MIE) and tableau-derived
    // (disjunction / ∀-propagation / cardinality-merge) instances; ~630× faster
    // complete realization on MIE.
    let base_model = if pseudo_model_enabled() {
        prepared.realize_base_model_types(None)
    } else {
        None
    };

    // Per-individual realization is independent across individuals
    // (each builds a fresh tableau context per class probe via
    // `prepared.decide`). Parallelise the outer loop with rayon; the
    // hierarchy / closure / prepared snapshot is shared read-only.
    let per_individual: Result<Vec<IndivResult>, ReasonError> = individual_iris
        .par_iter()
        .enumerate()
        .map(|(idx, _iri)| {
            let individual_id =
                IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
            let base_types = base_model.as_ref().map(|m| &m[idx]);
            let mut types: Vec<&str> = Vec::new();
            for (class_idx, class_iri) in &satisfiable {
                let class_id = ClassId::new(u32::try_from(*class_idx).expect("class fits in u32"));
                if instance_check_with_closure(
                    internal,
                    &closure,
                    &prepared,
                    class_id,
                    individual_id,
                    per_check,
                    base_types,
                )? {
                    types.push(class_iri);
                }
            }
            // Filter to Hasse leaves under the classification relation.
            let leaves: Vec<String> = types
                .iter()
                .filter(|&&cls| {
                    !types.iter().any(|&other| {
                        other != cls
                            && hierarchy.is_subclass(other, cls)
                            && !hierarchy.is_subclass(cls, other)
                    })
                })
                .map(|s| (*s).to_owned())
                .collect();
            let types_owned: Vec<String> = types.into_iter().map(str::to_owned).collect();
            Ok((types_owned, leaves))
        })
        .collect();
    let per_individual = per_individual?;
    let mut entailed_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut most_specific_types: HashMap<String, Vec<String>> = HashMap::new();
    for (iri, (types_owned, leaves)) in individual_iris.iter().zip(per_individual) {
        entailed_types.insert(iri.clone(), types_owned);
        most_specific_types.insert(iri.clone(), leaves);
    }


    Ok(Realization {
        individuals: individual_iris,
        entailed_types,
        most_specific_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
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

    #[test]
    fn class_assertion_is_an_entailed_instance() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        assert!(
            is_instance_of(&onto, "http://rustdl.test/A", "http://rustdl.test/alice")
                .expect("verdict")
        );
    }

    #[test]
    fn instance_via_subsumption_chain() {
        // A ⊑ B; ClassAssertion(:A :alice) ⇒ alice : B
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:A :B)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        assert!(
            is_instance_of(&onto, "http://rustdl.test/B", "http://rustdl.test/alice")
                .expect("verdict")
        );
    }

    #[test]
    fn non_instance_is_rejected() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        assert!(
            !is_instance_of(&onto, "http://rustdl.test/B", "http://rustdl.test/alice")
                .expect("verdict")
        );
    }

    #[test]
    fn instances_of_returns_all_known_members() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    Declaration(NamedIndividual(:carol))\n\
    ClassAssertion(:A :alice)\n\
    ClassAssertion(:A :bob)\n\
)\n"
        ));
        let mut members = instances_of(&onto, "http://rustdl.test/A").expect("verdict");
        members.sort();
        assert_eq!(
            members,
            vec![
                "http://rustdl.test/alice".to_owned(),
                "http://rustdl.test/bob".to_owned(),
            ]
        );
    }

    #[test]
    fn instance_check_via_property_domain() {
        // ObjectPropertyDomain(hasParent, Person);
        // ObjectPropertyAssertion(hasParent, alice, bob) ⇒
        // alice is a Person (subject of an r-edge, r's domain is
        // Person). bob is also a Person via the *range* axiom in
        // the next test.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    ObjectPropertyDomain(:hasParent :Person)\n\
    ObjectPropertyAssertion(:hasParent :alice :bob)\n\
)\n"
        ));
        assert!(
            is_instance_of(
                &onto,
                "http://rustdl.test/Person",
                "http://rustdl.test/alice"
            )
            .expect("verdict")
        );
    }

    #[test]
    fn instance_check_via_property_range() {
        // ObjectPropertyRange(hasParent, Person);
        // hasParent(alice, bob) ⇒ bob is a Person.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    ObjectPropertyRange(:hasParent :Person)\n\
    ObjectPropertyAssertion(:hasParent :alice :bob)\n\
)\n"
        ));
        assert!(
            is_instance_of(&onto, "http://rustdl.test/Person", "http://rustdl.test/bob")
                .expect("verdict")
        );
    }

    #[test]
    fn realize_filters_to_most_specific() {
        // alice : A; A ⊑ B; alice should realize as A (the leaf),
        // with B in entailed_types but not in most_specific.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:A :B)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        let r = realize(&onto).expect("realization");
        let alice = "http://rustdl.test/alice";
        let entailed = r.entailed_types(alice);
        assert!(entailed.iter().any(|c| c == "http://rustdl.test/A"));
        assert!(entailed.iter().any(|c| c == "http://rustdl.test/B"));
        let leaves = r.most_specific_types(alice);
        assert_eq!(leaves, vec!["http://rustdl.test/A".to_owned()]);
    }

    /// `realize_saturation_only` is a sound under-approximation of
    /// `realize`: every reported `(individual, class)` pair must
    /// hold under the full realization. On a pure-told-types
    /// scenario (alice : A; A ⊑ B) both agree exactly.
    #[test]
    fn realize_saturation_only_matches_full_on_told_chain() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:A :B)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        let full = realize(&onto).expect("full realization");
        let sat = realize_saturation_only(&onto).expect("saturation-only");
        let alice = "http://rustdl.test/alice";
        let full_types: HashSet<&str> = full
            .entailed_types(alice)
            .iter()
            .map(String::as_str)
            .collect();
        let sat_types: HashSet<&str> = sat
            .entailed_types(alice)
            .iter()
            .map(String::as_str)
            .collect();
        // Soundness: sat-only ⊆ full.
        for t in &sat_types {
            assert!(
                full_types.contains(t),
                "saturation-only reported {t} but full did not — soundness violated",
            );
        }
        // On a pure-told chain both modes should agree exactly.
        assert_eq!(full_types, sat_types);
        assert_eq!(
            sat.most_specific_types(alice),
            vec!["http://rustdl.test/A".to_owned()],
        );
    }
}
