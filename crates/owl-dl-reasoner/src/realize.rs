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
use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, InternalOntology,
};
use owl_dl_saturation::{Subsumers, saturate, saturate_for_realize};

use crate::PreparedOntology;
use crate::ReasonError;
use crate::classify::{
    classify_saturation_only_internal, classify_top_down_internal, is_pure_el,
    saturator_complete_fragment, tbox_only_saturator_eligible,
};

/// `(entailed_types, hasse_leaves)` for one individual — returned
/// by the parallel realisation worker so the outer loop can stitch
/// the maps together.
type IndivResult = (Vec<String>, Vec<String>);

/// Same as [`IndivResult`] plus whether any probe for that individual was CUT
/// (deadline / node-cap / depth bail) rather than refuted — the input to
/// `Realization::incomplete`.
type IndivResultReporting = (Vec<String>, Vec<String>, bool);

/// Default per-pair tableau probe deadline (milliseconds) used by the
/// `{a} ⊓ ¬C` instance-check reduction when `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`
/// is unset. Restores (with a sane out-of-the-box bound rather than an
/// opt-in one) the caller-side realize bound removed in 0.3.18: off the
/// saturation fast path, a defined-class + property-domain + nominal input
/// (issue #35 v4) can make a single pair's disjunctive search explode the
/// completion graph and never return. 750ms keeps the issue-#35-v4
/// reproducer's total realize wall to a couple of seconds while staying
/// generous enough not to prematurely cut ordinary pairs (measured: at
/// 500ms wall was ~0.5s; at 750ms wall was still sub-second; both were
/// tried against the reproducer — see `docs/superpowers/sdd/task-B-report.md`).
/// Set `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` to opt out (unbounded).
const DEFAULT_REALIZE_PAIR_TIMEOUT_MS: u64 = 750;

/// Default bounded deadline (milliseconds) for the ONE-OFF pseudo-model
/// witness build `realize_tableau_internal` performs when
/// [`pseudo_model_enabled`] is on, used when `RUSTDL_PSEUDO_MODEL_WITNESS_MS`
/// is unset. Deliberately bounded (never `None`/unbounded) — the witness is a
/// single extra wedge run per `realize` call, not per pair, but an unbounded
/// deadline on an off-fragment `ABox` reintroduces exactly the long-run risk
/// the #35-v4 per-pair timeout was built to bound.
const DEFAULT_PSEUDO_MODEL_WITNESS_MS: u64 = 1000;

/// Is the pseudo-model realize shortcut enabled?
///
/// When on, `realize_tableau_internal` computes one `ABox` witness model
/// (via [`crate::PreparedOntology::realize_base_model_types`]) ONCE per
/// `realize` call, under a bounded deadline
/// (`RUSTDL_PSEUDO_MODEL_WITNESS_MS`, default
/// [`DEFAULT_PSEUDO_MODEL_WITNESS_MS`]), and threads each individual's
/// witness type set into [`instance_check_with_closure`] as a subtractive
/// prune: `class ∉ witness_types(individual) ⇒ Ok(false)`, skipping the
/// per-pair `{a} ⊓ ¬C` tableau probe entirely. The prune only ever returns
/// `Ok(false)` and only fires AFTER the told-closure `Ok(true)` fast path, so
/// it is verdict-identical to the flag being off (completeness-preserving) —
/// see the module's soundness note at the shortcut's call site.
///
/// **Default ON (Task 4, 2026-07-26 — assessment passed).** A custom
/// nominal-`ABox` fixture (`ObjectOneOf` + `ObjectPropertyDomain` + a defined
/// class + `DisjointClasses` + assertions,
/// `tests/fixtures/pseudo_model/nominal_abox.ofn`) and a 40-decoy-class scaled
/// variant (`nominal_abox_scaled.ofn`) both showed: (1) `realize --json`
/// byte-identical ON vs OFF (completeness-preserving); (2) the prune fires and
/// wins (5.45ms → 3.44ms median of 9, 1.59× on the scaled fixture — real
/// MIE-scale wins are PR #23's 110–630×); (3) a `HermiT` oracle
/// (`robot reason --reasoner hermit --axiom-generators ClassAssertion
/// --include-indirect true`, output committed as
/// `tests/fixtures/pseudo_model/nominal_abox-hermit.ofn`) matched rustdl ON on
/// every named type, FP=0 (the only diff was `owl:Thing`, which rustdl
/// conventionally omits — not a miss). See
/// `docs/2026-07-26-pseudo-model-assessment.md`. The ORE-tier
/// verdict-identity bake-off could not run in-sandbox (corpus fetch is
/// macOS-broken there) and remains the recommended CI/Linux confirmation;
/// default-ON additionally rests on the shortcut being sound by construction
/// (subtractive-only prune — an entailed type is in every model, hence in the
/// witness, hence never pruned) and on the same direction as the shipped
/// default-ON Phase-7 label heuristic.
///
/// # THAT SOUNDNESS-BY-CONSTRUCTION ARGUMENT IS FALSIFIED (2026-08-18)
///
/// The argument needs the witness to BE a model of the KB. It is not, on
/// inverse-functional `ABox`es — the witness applies FUNCTIONAL merges but not
/// INVERSE-functional ones, so an entailed type can be absent from it and therefore
/// pruned.
///
/// Reproducer (`tests/fixtures/realize_derived_same/inverse-functional.ofn`):
/// `InverseFunctional(r)`, `r(x,z)`, `r(y,z)` ⊨ `x = y`, with `x : A` and `y : B`.
///
/// | | result |
/// |---|---|
/// | `RUSTDL_PSEUDO_MODEL=1` (default) | `x : A`, `y : B` — **entailed types PRUNED** |
/// | `RUSTDL_PSEUDO_MODEL=0` | `x : A, B` and `y : A, B` — correct |
/// | `rustdl justify … instance x B` | proves it, 4-axiom minimal justification |
///
/// So the engine derives the entailment; this prune discards it. The FUNCTIONAL
/// analogue (`Functional(r)`, `r(x,y)`, `r(x,z)` ⊨ `y = z`) is CORRECT at the default,
/// which is what localises the gap to inverse-functional merging in the witness build.
///
/// **Direction of harm:** subtractive, so FP=0 is intact — this costs entailments, not
/// soundness. But the "sound by construction" clause above is the stated basis for
/// shipping this default-ON *without* the ORE verdict-identity bake-off that the same
/// paragraph calls "the recommended CI/Linux confirmation". That basis does not hold in
/// general, so the bake-off is now load-bearing rather than optional.
///
/// **Fix belongs in the witness build**, not here: the `ABox`-seeded wedge consistency
/// completion must apply inverse-functional merges, as it already does functional ones.
/// Until then `RUSTDL_PSEUDO_MODEL=0` is the workaround. Pinned by
/// `pseudo_model_prunes_entailed_type` in
/// `crates/owl-dl-reasoner/tests/realize_derived_same.rs`;
/// `docs/known-limitations/realize-drops-derived-individual-equality.md` carries the
/// full record.
///
/// **Coupling to [`crate::PreparedOntology::realize_base_model_types`]:** it
/// returns `None` whenever the `ABox`-seeded wedge consistency cache is
/// unavailable — i.e. when `RUSTDL_WEDGE_CONSISTENCY=0`, or the input has no
/// `ABox` at all. So this shortcut (on by default, or explicitly
/// `RUSTDL_PSEUDO_MODEL=1`) combined with either of those silently no-ops:
/// every pair falls through to the normal per-pair probe, which is safe (a
/// missing witness can only skip the prune, never change a verdict) but means
/// the flag has no effect in that configuration. Set `RUSTDL_PSEUDO_MODEL=0`
/// to revert to the pre-Task-3 per-pair-only behaviour.
fn pseudo_model_enabled() -> bool {
    std::env::var_os("RUSTDL_PSEUDO_MODEL").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Reads `RUSTDL_PSEUDO_MODEL_WITNESS_MS`, returning the bounded deadline to
/// use for the one-off pseudo-model witness build. Unset or unparsable ⟹
/// [`DEFAULT_PSEUDO_MODEL_WITNESS_MS`]. Note that (unlike
/// `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, where `0` means unbounded) `0` here yields
/// an immediate deadline ⟹ the witness build `Stalled`s ⟹ no witness ⟹ the
/// shortcut safely no-ops for that call; there is no "unbounded witness" option.
fn pseudo_model_witness_deadline_from_env() -> std::time::Instant {
    let ms = std::env::var("RUSTDL_PSEUDO_MODEL_WITNESS_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_PSEUDO_MODEL_WITNESS_MS);
    std::time::Instant::now() + std::time::Duration::from_millis(ms)
}

/// Reads `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, returning the per-pair deadline
/// in milliseconds to apply. Unset ⟹ [`DEFAULT_REALIZE_PAIR_TIMEOUT_MS`];
/// set to a positive integer ⟹ that value; set to `0` ⟹ `None` (explicit
/// opt-out — unbounded).
fn realize_pair_timeout_ms_from_env() -> Option<u64> {
    match std::env::var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS") {
        Ok(v) => match v.parse::<u64>() {
            Ok(0) => None,
            Ok(ms) => Some(ms),
            Err(_) => Some(DEFAULT_REALIZE_PAIR_TIMEOUT_MS),
        },
        Err(_) => Some(DEFAULT_REALIZE_PAIR_TIMEOUT_MS),
    }
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
    // Fast path: on the saturator-complete fragment answer completely via
    // saturation — `a : C` iff `C` subsumes `a`'s nominal class. Avoids the
    // `{a} ⊓ ¬C` tableau probe that explodes on issue-#35-style inputs.
    if realize_saturation_eligible(internal) {
        let (subsumers, nominal_by_ind) = saturate_for_realize(internal);
        return Ok(nominal_by_ind
            .get(&individual_id)
            .is_some_and(|&nom| subsumers.contains(nom, class_id)));
    }
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;
    let pair_deadline = realize_pair_timeout_ms_from_env()
        .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
    // Single-pair path — the witness is a realize-loop optimization (one
    // witness amortized across the whole per-individual, per-class loop);
    // it isn't worth building for a lone instance check.
    instance_check_with_closure(
        internal,
        &closure,
        &prepared,
        class_id,
        individual_id,
        pair_deadline,
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
///
/// `base_types`, when `Some`, is one `ABox` witness model's COMPLETE type set
/// for `individual_id` (see [`crate::PreparedOntology::realize_base_model_types`]
/// / [`pseudo_model_enabled`]): a subtractive prune, checked AFTER the
/// told-closure `Ok(true)` loop and BEFORE the `{a} ⊓ ¬C` probe is built —
/// `class_id ∉ base_types ⇒ Ok(false)`, skipping the probe entirely. Sound
/// and verdict-identical to `base_types: None` PROVIDED `base_types` is a
/// genuine witness model's type set (every told/derived membership already
/// returned `true` above, so it is never pruned; a class the individual
/// genuinely has is always present in a real witness model's label). `None`
/// (no witness available) takes the unchanged normal path.
fn instance_check_with_closure(
    internal: &InternalOntology,
    closure: &Subsumers,
    prepared: &PreparedOntology,
    class_id: ClassId,
    individual_id: IndividualId,
    pair_deadline: Option<std::time::Instant>,
    base_types: Option<&HashSet<ClassId>>,
) -> Result<bool, ReasonError> {
    instance_check_reporting(
        internal,
        closure,
        prepared,
        class_id,
        individual_id,
        pair_deadline,
        base_types,
    )
    .map(|(is_instance, _truncated)| is_instance)
}

/// `(is_instance, truncated)` — [`instance_check_with_closure`] plus WHETHER the
/// negative answer came from a real refutation or from a cut probe.
///
/// **Why this exists.** A deadline expiry, a `RUSTDL_MAX_NODES` trip and a
/// depth-limit bail all collapse to `Ok(false)` = "not an instance". That is a sound
/// under-approximation, and the comment below has always said so — but the
/// information was **discarded at the source**, so `Realization` had no way to tell a
/// consumer that a type set might be short. `realize --json` consequently shipped
/// with no `incomplete` field at all, unlike `classify --json`.
///
/// Same shape as `classify::prep_bounding_decision`: keep the reason, report it, change
/// no verdict. `truncated` is advisory only — every `false` it accompanies is still a
/// sound "not proven to be an instance".
fn instance_check_reporting(
    internal: &InternalOntology,
    closure: &Subsumers,
    prepared: &PreparedOntology,
    class_id: ClassId,
    individual_id: IndividualId,
    pair_deadline: Option<std::time::Instant>,
    base_types: Option<&HashSet<ClassId>>,
) -> Result<(bool, bool), ReasonError> {
    for told in told_classes_of(internal, individual_id) {
        if closure.contains(told, class_id) {
            return Ok((true, false));
        }
    }
    if let Some(bt) = base_types
        && !bt.contains(&class_id)
    {
        return Ok((false, false));
    }
    // KB ⊨ C(a) iff `{a} ⊓ ¬C` is unsatisfiable.
    let build = move |pool: &mut ConceptPool| {
        let cls = pool.atomic(class_id);
        let not_cls = pool.not(cls);
        let nom = pool.nominal(individual_id);
        pool.and(vec![nom, not_cls])
    };
    match pair_deadline {
        // Bounded probe: ANY non-verdict within the budget — a deadline hit
        // (`Ok(None)`), a node-cap trip (`Ok(None)`), or a depth-limit bail
        // (`Err(NoVerdict)`) — is treated as "not an instance": a SOUND
        // under-approximation (a MISS at worst, never a false membership,
        // since we could not prove `{a} ⊓ ¬C` unsatisfiable). A per-pair
        // non-verdict must NOT fail the whole `realize` — it degrades to a
        // partial result, mirroring classify's `subsumes_via_tableau`
        // handling. The #57 pseudo-model shortcut avoids the tableau for
        // eligible pairs; this makes the tableau FALLBACK graceful for
        // shortcut-ineligible pairs (e.g. `ore_ont_7988`), which otherwise
        // errored the whole realize on a single depth-limit bail.
        Some(deadline) => match prepared.decide_with_deadline(deadline, build) {
            // `sat == None` is a deadline expiry or a node-cap trip: no verdict, so the
            // sound answer is "not an instance" AND the result is truncated.
            Ok(sat) => Ok((!sat.unwrap_or(true), sat.is_none())),
            // Depth-limit bail — also no verdict, also truncated.
            Err(ReasonError::NoVerdict) => Ok((false, true)),
            Err(e) => Err(e),
        },
        // No deadline: the probe ran to a verdict, so nothing was truncated.
        None => Ok((!prepared.decide(build)?, false)),
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
    // Fast path: on the saturator-complete fragment, `a ∈ C` iff `C` subsumes
    // `a`'s nominal class (complete + terminating; no tableau).
    if realize_saturation_eligible(internal) {
        let (subsumers, nominal_by_ind) = saturate_for_realize(internal);
        let mut out = Vec::new();
        for idx in 0..internal.vocabulary.num_individuals() {
            let ind = IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
            if nominal_by_ind
                .get(&ind)
                .is_some_and(|&nom| subsumers.contains(nom, class_id))
            {
                let iri = internal.vocabulary.individual_iri(ind);
                if !iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
                    out.push(iri.to_owned());
                }
            }
        }
        return Ok(out);
    }
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;
    let pair_deadline_ms: Option<u64> = realize_pair_timeout_ms_from_env();
    let mut out = Vec::new();
    for idx in 0..internal.vocabulary.num_individuals() {
        let individual_id =
            IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
        let pair_deadline = pair_deadline_ms
            .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
        // Single-pair-per-individual path — same rationale as
        // `is_instance_of_internal`: no shared witness to amortize here.
        if instance_check_with_closure(
            internal,
            &closure,
            &prepared,
            class_id,
            individual_id,
            pair_deadline,
            None,
        )? {
            let iri = internal.vocabulary.individual_iri(individual_id);
            if !iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
                out.push(iri.to_owned());
            }
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
            let iri = internal.vocabulary.individual_iri(individual_id);
            if !iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
                out.push(iri.to_owned());
            }
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
    /// At least one instance probe was CUT — deadline expiry, `RUSTDL_MAX_NODES`
    /// trip, or depth-limit bail — rather than refuted, so a reported type set may be
    /// SHORT.
    ///
    /// **Why this field exists.** Those three outcomes all collapse to "not an
    /// instance", which is a sound under-approximation but was previously discarded at
    /// the source, so `realize --json` shipped with no completeness signal at all
    /// while `classify --json` has had one for months. A consumer could not
    /// distinguish "these are all the types" from "some were dropped" — and the
    /// per-pair budget defaults to 750 ms, so truncation is reachable on real inputs.
    ///
    /// Sound in one direction only, like classify's flag: `false` means nothing was
    /// cut on the path taken; it does NOT certify that no entailment is missed by
    /// other mechanisms (e.g. the derived-equality gap in
    /// `docs/known-limitations/realize-drops-derived-individual-equality.md`).
    incomplete: bool,
}

impl Realization {
    /// See [`Self::incomplete`] — `true` iff at least one instance probe was cut
    /// rather than refuted, so a type set may be short.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

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
    let mut named_individual_iris: Vec<String> = Vec::new();
    for (iri, (types_owned, leaves)) in individual_iris.iter().zip(per_individual) {
        if iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
            continue;
        }
        named_individual_iris.push(iri.clone());
        entailed_types.insert(iri.clone(), types_owned);
        most_specific_types.insert(iri.clone(), leaves);
    }
    Ok(Realization {
        individuals: named_individual_iris,
        entailed_types,
        most_specific_types,
        // Saturation-only path: closure lookups only, no probe to cut.
        incomplete: false,
    })
}

/// True iff `class` (a `ClassAssertion` body) is captured EXACTLY by
/// [`saturate_for_realize`]'s atomic-operand seeding — i.e. it is atomic,
/// `⊤`, or a conjunction thereof. A body carrying `∃`/`∀`/nominal/cardinality/
/// `¬` would be under-captured (the seeding drops the non-atomic operands), so
/// such inputs must fall back to the complete tableau path.
fn class_body_realize_safe(class: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(class) {
        ConceptExpr::Atomic(_) | ConceptExpr::Top => true,
        ConceptExpr::And(ops) => ops.iter().all(|op| class_body_realize_safe(*op, pool)),
        _ => false,
    }
}

/// Is `internal` realized COMPLETELY (== the tableau) by the saturation fast
/// path? Two conditions:
///
/// 1. The `TBox` is in the saturator-complete fragment — reusing `classify`'s
///    proven gates (`is_pure_el` / `saturator_complete_fragment` for the no-`ABox`
///    case, or the `ABox`-admitting Lever-1 `tbox_only_saturator_eligible`, which
///    also excludes nominals).
/// 2. Every `ABox` axiom is a shape [`saturate_for_realize`] captures exactly:
///    atomic/⊓ `ClassAssertion` and non-inverse `ObjectPropertyAssertion`.
///    `NegativeObjectPropertyAssertion` / `DifferentIndividuals` add no types
///    (an inconsistency they cause is caught by the classify/abox pre-checks),
///    so they are permitted. `SameIndividual` (needs type-merge across
///    individuals) and inverse-role assertions are NOT captured ⟹ fall back.
///
/// Unlike `classify`, realize CANNOT admit arbitrary `ABox` shapes: the `ABox` is
/// load-bearing for individual types, whereas it is irrelevant to class
/// subsumption.
fn realize_saturation_eligible(internal: &InternalOntology) -> bool {
    // RUSTDL_REALIZE_SATURATION=0 forces the tableau path (A/B isolation).
    if std::env::var_os("RUSTDL_REALIZE_SATURATION").is_some_and(|v| v == "0") {
        return false;
    }
    let tbox_ok = is_pure_el(internal)
        || (crate::horn_shortcircuit_enabled() && saturator_complete_fragment(internal))
        || tbox_only_saturator_eligible(internal);
    if !tbox_ok {
        return false;
    }
    // A cardinality-style role characteristic can FORCE two named individuals to be
    // equal, and this fast path has no equality folding whatsoever — so it would
    // report each individual's own types and silently omit its twin's.
    //
    // Measured before this guard existed
    // (`docs/known-limitations/realize-drops-derived-individual-equality.md`):
    // `Functional(r) + r(x,y) + r(x,z) ⊨ y = z` gave `y : A` and `z : B` instead of
    // both types for both, while `rustdl individuals` on the SAME file derived
    // `same_groups: [["y","z"]]` — two surfaces of one binary disagreeing, with no
    // `incomplete` field in `realize --json` to signal it.
    //
    // Handled exactly as `SameIndividual` already is, three lines below: refuse the
    // ontology and let the tableau path realize it, since the tableau DOES fold
    // functional-forced equality (verified: it returns both types for both
    // individuals). This is the whole mechanism behind the working asserted case.
    //
    // RESIDUAL, deliberate: the tableau does NOT fold INVERSE-functional-forced
    // equality, so those ontologies stay incomplete — but now consistently on both
    // paths instead of differing between them. That is a separate gap in a different
    // engine (`RUSTDL_INVERSE_FUNC_MERGE` is default-ON in `owl-dl-tableau` yet does
    // not reach these probes) and is tracked in the limitation doc rather than
    // blocking this fix.
    //
    // Narrow by design: the characteristic alone is harmless without ground edges to
    // merge, so this only fires when the ontology ALSO has an
    // `ObjectPropertyAssertion`. Ontologies with a functional role and no ABox edges
    // keep the fast path.
    let has_merge_forcing_characteristic = internal.axioms.iter().any(|ax| {
        matches!(
            ax,
            Axiom::FunctionalRole(_) | Axiom::InverseFunctionalRole(_)
        )
    });
    if has_merge_forcing_characteristic
        && internal
            .axioms
            .iter()
            .any(|ax| matches!(ax, Axiom::ObjectPropertyAssertion { .. }))
    {
        return false;
    }
    internal.axioms.iter().all(|ax| match ax {
        Axiom::ClassAssertion { class, .. } => class_body_realize_safe(*class, &internal.concepts),
        Axiom::ObjectPropertyAssertion { role, .. } => !role.is_inverse(),
        Axiom::SameIndividual(_) => false,
        // Everything else is either a type-irrelevant ABox form
        // (NegativeObjectPropertyAssertion / DifferentIndividuals) or a TBox
        // axiom already vetted by `tbox_ok`.
        _ => true,
    })
}

/// Saturation fast-path realization — complete == the tableau on the
/// [`realize_saturation_eligible`] fragment, and TERMINATING (no tableau, no
/// disjunctive search). Materializes each named individual as a nominal class
/// via [`saturate_for_realize`] and reads its entailed named types off the
/// subsumer closure. This is the fix for the issue-#35 realize hang: the
/// exploding `{a} ⊓ ¬C` tableau probe is never invoked on the EL/Horn fragment.
///
/// # Errors
///
/// See [`ReasonError`].
pub(crate) fn realize_via_saturation_internal(
    internal: &InternalOntology,
) -> Result<Realization, ReasonError> {
    // Complete class hierarchy on this fragment (for Hasse-leaf pruning and the
    // unsatisfiable-class filter).
    let hierarchy = classify_saturation_only_internal(internal)?;
    let unsat: HashSet<&str> = hierarchy.unsatisfiable_classes().into_iter().collect();
    let num_user_classes = internal.vocabulary.num_classes();

    let (subsumers, nominal_by_ind) = saturate_for_realize(internal);

    let mut entailed_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut most_specific_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut named_individual_iris: Vec<String> = Vec::new();

    for idx in 0..internal.vocabulary.num_individuals() {
        let ind = IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
        let iri = internal.vocabulary.individual_iri(ind).to_owned();
        if iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
            continue;
        }
        // Entailed named types = subsumers of the individual's nominal class,
        // restricted to declared user classes (synthetic Tseitin / nominal ids
        // are ≥ num_user_classes) and to satisfiable classes.
        let types: Vec<String> = nominal_by_ind
            .get(&ind)
            .map(|&nom| {
                subsumers
                    .subsumers_of(nom)
                    .into_iter()
                    .filter(|c| (c.index() as usize) < num_user_classes)
                    .map(|c| internal.vocabulary.class_iri(c).to_owned())
                    .filter(|iri| !unsat.contains(iri.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        // Filter to Hasse leaves under the classification relation.
        let leaves: Vec<String> = types
            .iter()
            .filter(|cls| {
                !types.iter().any(|other| {
                    other != *cls
                        && hierarchy.is_subclass(other, cls)
                        && !hierarchy.is_subclass(cls, other)
                })
            })
            .cloned()
            .collect();
        named_individual_iris.push(iri.clone());
        entailed_types.insert(iri.clone(), types);
        most_specific_types.insert(iri, leaves);
    }

    Ok(Realization {
        individuals: named_individual_iris,
        entailed_types,
        most_specific_types,
        // Saturation path: no tableau probe runs, so nothing can be cut.
        incomplete: false,
    })
}

/// True iff the internal ontology contains the axiom `SubClassOf(owl:Thing,
/// owl:Nothing)` (`⊤ ⊑ ⊥`), which makes the entire domain empty and every
/// named class unsatisfiable. O(n) axiom scan; cheap in practice.
fn has_top_subclass_bot(internal: &InternalOntology) -> bool {
    let pool = &internal.concepts;
    internal.axioms.iter().any(|ax| {
        if let Axiom::SubClassOf { sub, sup } = ax {
            matches!(pool.get(*sub), ConceptExpr::Top) && matches!(pool.get(*sup), ConceptExpr::Bot)
        } else {
            false
        }
    })
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_internal(internal: &InternalOntology) -> Result<Realization, ReasonError> {
    // Inconsistency short-circuit. On an inconsistent ontology every individual
    // is vacuously an instance of every class, so the per-(individual, class)
    // `{a} ⊓ ¬C` probe loop below is both wrong to report as a meaningful
    // realization AND, on a *deep* inconsistency, a pathological stall: the
    // sound ABox-saturation pre-check catches such clashes cheaply (it is what
    // makes `is_consistent` fast on `family.ofn`), but classify's own pattern
    // checks do not, so realize would otherwise classify it as "consistent",
    // see a large satisfiable-class set, and grind slow probes over an ABox that
    // never cheaply clashes. Erroring here matches the sibling
    // `materialize_{object,data}_property_assertions` convention. The check is a
    // sound under-approximation — `clash` is only ever set when the ontology is
    // genuinely inconsistent — so a consistent ontology falls straight through
    // to the unchanged realization paths below.
    if crate::abox_saturation::saturate_abox_consistency(internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    // TBox-level global inconsistency (`⊤ ⊑ ⊥`): the ABox-saturation check
    // above handles ABox-sourced clashes; this cheap O(n) scan catches the
    // pure-TBox empty-domain axiom that `abox_saturation` misses (it only
    // processes atomic-LHS SubClassOf). Without this, realize silently returns
    // an empty realization instead of the `Err(Inconsistent)` convention.
    if has_top_subclass_bot(internal) {
        return Err(ReasonError::Inconsistent);
    }
    // Fast path: on the saturator-complete fragment (EL/Horn TBox + simple
    // ABox) realize completely via saturation, never touching the tableau —
    // whose `{a} ⊓ ¬C` disjunctive search can explode on defined-class +
    // property-domain + property-assertion inputs (issue #35).
    if realize_saturation_eligible(internal) {
        return realize_via_saturation_internal(internal);
    }
    realize_tableau_internal(internal)
}

/// The sound+complete tableau realization path — one `{a} ⊓ ¬C` satisfiability
/// probe per (individual, satisfiable-class) pair. Used off the saturation
/// fast-path fragment (and directly in the fast-path-vs-tableau identity test).
/// Bounds each per-pair probe via `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`
/// (default [`DEFAULT_REALIZE_PAIR_TIMEOUT_MS`]) so a genuinely-hard SROIQ
/// input degrades to a sound under-approximation instead of hanging (restores
/// the caller-side bound removed in 0.3.18). Set the env var to `0` to opt out
/// (unbounded).
///
/// # Errors
///
/// See [`ReasonError`].
pub(crate) fn realize_tableau_internal(
    internal: &InternalOntology,
) -> Result<Realization, ReasonError> {
    let pair_deadline_ms: Option<u64> = realize_pair_timeout_ms_from_env();

    // Use the top-down classifier — the same default as the public
    // `classify` entry. The N² pair-sweep that this previously
    // called (`classify_internal`) DNFs on real ontologies and
    // forced any realize call on SIO-scale inputs to time out
    // before per-individual probing ever started.
    // Bound the classify step with the same per-pair budget that realize
    // already applies to its own probe loop.  An unbounded classify could hang
    // here on one hard class-subsumption pair before the per-individual loop
    // ever starts (verified: ore_ont_14379 DNF → completes with this bound).
    // A cut pair is a sound under-approximation (MISS, never a false
    // subsumption); at worst it makes `most_specific_types` less minimal, but
    // every reported type is still a genuine type. `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`
    // ⟹ `pair_deadline_ms = None` ⟹ unbounded (opt-out preserved).
    let hierarchy = classify_top_down_internal(
        internal,
        pair_deadline_ms.map(std::time::Duration::from_millis),
        None,
    )?;
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

    // Pseudo-model shortcut (Task 3): compute ONE `ABox` witness model,
    // ONCE, under a bounded deadline (never unbounded — see
    // `pseudo_model_witness_deadline_from_env`'s doc). `None` (flag off,
    // `Stalled`/deadline hit, or no wedge-consistency cache — see
    // `pseudo_model_enabled`'s coupling note) ⇒ every pair below takes the
    // unchanged normal path, so this can only ever skip probes, never
    // change a verdict.
    let base_model: Option<Vec<HashSet<ClassId>>> = if pseudo_model_enabled() {
        let witness_deadline = pseudo_model_witness_deadline_from_env();
        prepared.realize_base_model_types(Some(witness_deadline))
    } else {
        None
    };
    if let Some(ref m) = base_model {
        debug_assert_eq!(
            m.len(),
            individual_iris.len(),
            "witness model must carry one type set per individual",
        );
    }

    // Per-individual realization is independent across individuals
    // (each builds a fresh tableau context per class probe via
    // `prepared.decide`). Parallelise the outer loop with rayon; the
    // hierarchy / closure / prepared snapshot is shared read-only.
    let per_individual: Result<Vec<IndivResultReporting>, ReasonError> = individual_iris
        .par_iter()
        .enumerate()
        .map(|(idx, _iri)| {
            let individual_id =
                IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
            let base_types = base_model.as_ref().and_then(|m| m.get(idx));
            let mut types: Vec<&str> = Vec::new();
            // Any cut probe for this individual makes its type set possibly short.
            let mut truncated = false;
            for (class_idx, class_iri) in &satisfiable {
                let class_id = ClassId::new(u32::try_from(*class_idx).expect("class fits in u32"));
                let pair_deadline = pair_deadline_ms
                    .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
                let (is_instance, was_cut) = instance_check_reporting(
                    internal,
                    &closure,
                    &prepared,
                    class_id,
                    individual_id,
                    pair_deadline,
                    base_types,
                )?;
                truncated |= was_cut;
                if is_instance {
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
            Ok((types_owned, leaves, truncated))
        })
        .collect();
    let per_individual = per_individual?;
    let mut entailed_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut most_specific_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut named_individual_iris: Vec<String> = Vec::new();
    // Any cut probe anywhere makes the whole realization possibly short.
    let mut incomplete = false;
    for (iri, (types_owned, leaves, truncated)) in individual_iris.iter().zip(per_individual) {
        if iri.starts_with(owl_dl_core::convert::ANON_IRI_PREFIX) {
            continue;
        }
        incomplete |= truncated;
        named_individual_iris.push(iri.clone());
        entailed_types.insert(iri.clone(), types_owned);
        most_specific_types.insert(iri.clone(), leaves);
    }

    Ok(Realization {
        individuals: named_individual_iris,
        entailed_types,
        most_specific_types,
        incomplete,
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

    /// White-box canary (Task 3, RED before `base_types` exists — this test
    /// only compiles once `instance_check_with_closure` gains the param):
    /// the `base_types` prune is a genuine short-circuit that takes
    /// precedence over the `{a} ⊓ ¬C` tableau probe, not a no-op parameter.
    ///
    /// `a : (A ⊔ B)`, `A ⊑ E`, `B ⊑ E` ⇒ `a : E` is entailed by case-split
    /// tableau reasoning, but NOT by the told-closure fast path
    /// (`told_classes_of` only captures atomic `ClassAssertion` bodies, and
    /// here the asserted body is a disjunction). So with `base_types: None`
    /// the function must reach the tableau probe and correctly return
    /// `Ok(true)`. Feeding a deliberately-wrong `base_types` (empty, i.e.
    /// claiming `a` has no `ABox` witness type at all) that EXCLUDES `E`
    /// must flip the answer to `Ok(false)` — proving the prune is consulted
    /// and dominates. (Production callers only ever pass a genuine witness
    /// model's type set, which — per the soundness note on
    /// `instance_check_with_closure` — never excludes a genuinely-entailed
    /// class; this test isolates the mechanism with a synthetic input.)
    #[test]
    fn pseudo_model_prune_short_circuits_before_tableau_probe() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:E))\n\
    Declaration(NamedIndividual(:a))\n\
    SubClassOf(:A :E)\n\
    SubClassOf(:B :E)\n\
    ClassAssertion(ObjectUnionOf(:A :B) :a)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let e_id = internal
            .vocabulary
            .class_id("http://rustdl.test/E")
            .expect("E declared");
        let a_id = internal
            .vocabulary
            .individual_id("http://rustdl.test/a")
            .expect("a declared");
        let closure = saturate(&internal);
        let prepared = PreparedOntology::from_internal(internal.clone()).expect("prepares");

        // Baseline: no witness ⇒ the tableau probe alone correctly derives
        // a:E via the disjunctive case split.
        assert!(
            instance_check_with_closure(&internal, &closure, &prepared, e_id, a_id, None, None)
                .expect("verdict"),
            "a:E must be entailed via case-split tableau reasoning",
        );

        // A witness claiming `a` has no types at all (E excluded) must
        // short-circuit to `Ok(false)`, overriding the (correct) tableau
        // answer — proof the prune actually fires.
        let empty: HashSet<ClassId> = HashSet::new();
        assert!(
            !instance_check_with_closure(
                &internal,
                &closure,
                &prepared,
                e_id,
                a_id,
                None,
                Some(&empty),
            )
            .expect("verdict"),
            "base_types excluding E must short-circuit to Ok(false)",
        );

        // A witness that DOES carry E falls through normally and still
        // gives the correct answer.
        let with_e: HashSet<ClassId> = HashSet::from([e_id]);
        assert!(
            instance_check_with_closure(
                &internal,
                &closure,
                &prepared,
                e_id,
                a_id,
                None,
                Some(&with_e),
            )
            .expect("verdict"),
            "base_types containing E must not block the correct Ok(true)",
        );
    }

    /// Soundness ordering invariant: the told-closure `Ok(true)` fast path
    /// runs BEFORE the `base_types` prune is even consulted, so a told/
    /// derived membership is never pruned — even a deliberately-wrong
    /// (empty) `base_types` cannot override it. `alice : A`, `A ⊑ C` ⇒
    /// `alice : C` is told-closure-derivable.
    #[test]
    fn pseudo_model_prune_never_overrides_told_closure() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A)) Declaration(Class(:C))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:A :C)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let c_id = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("C declared");
        let alice_id = internal
            .vocabulary
            .individual_id("http://rustdl.test/alice")
            .expect("alice declared");
        let closure = saturate(&internal);
        let prepared = PreparedOntology::from_internal(internal.clone()).expect("prepares");

        let empty: HashSet<ClassId> = HashSet::new();
        assert!(
            instance_check_with_closure(
                &internal,
                &closure,
                &prepared,
                c_id,
                alice_id,
                None,
                Some(&empty),
            )
            .expect("verdict"),
            "told/derived membership must win before base_types is consulted",
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

    /// Tier B fast-path realize must be COMPLETE on the EL fragment,
    /// including conjunctive-LHS instance entailments that the old
    /// closure-only `realize_saturation_only` drops:
    /// `x:D1, x:D2, D1 ⊓ D2 ⊑ E ⊨ x:E`.
    #[test]
    fn saturation_realize_derives_conjunctive_lhs() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(Class(:E))\n\
    Declaration(NamedIndividual(:x))\n\
    SubClassOf(ObjectIntersectionOf(:D1 :D2) :E)\n\
    ClassAssertion(:D1 :x)\n\
    ClassAssertion(:D2 :x)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let r = realize_via_saturation_internal(&internal).expect("realize");
        let x = "http://rustdl.test/x";
        let types: HashSet<&str> = r.entailed_types(x).iter().map(String::as_str).collect();
        assert!(types.contains("http://rustdl.test/D1"));
        assert!(types.contains("http://rustdl.test/D2"));
        assert!(
            types.contains("http://rustdl.test/E"),
            "conjunctive-LHS entailment x:E missed; got {types:?}",
        );
    }

    /// Tier B fast-path realize must handle existential-filler LHS
    /// (the family shape): `Person(a), hasSex(a,b), Male(b),
    /// Person ⊓ ∃hasSex.Male ⊑ Man ⊨ a:Man` — the pattern that makes
    /// the tableau path diverge on issue #35.
    #[test]
    fn saturation_realize_derives_existential_lhs() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person)) Declaration(Class(:Man)) Declaration(Class(:Male))\n\
    Declaration(ObjectProperty(:hasSex))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    EquivalentClasses(:Man ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Male)))\n\
    ClassAssertion(:Person :a)\n\
    ClassAssertion(:Male :b)\n\
    ObjectPropertyAssertion(:hasSex :a :b)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let r = realize_via_saturation_internal(&internal).expect("realize");
        let a = "http://rustdl.test/a";
        let types: HashSet<&str> = r.entailed_types(a).iter().map(String::as_str).collect();
        assert!(types.contains("http://rustdl.test/Person"));
        assert!(
            types.contains("http://rustdl.test/Man"),
            "existential-LHS entailment a:Man missed; got {types:?}",
        );
    }

    /// Tier B fast-path realize derives domain (subject) and range
    /// (object) types from a property assertion, matching the tableau.
    #[test]
    fn saturation_realize_domain_and_range() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Parent)) Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(NamedIndividual(:alice)) Declaration(NamedIndividual(:bob))\n\
    ObjectPropertyDomain(:hasParent :Parent)\n\
    ObjectPropertyRange(:hasParent :Person)\n\
    ObjectPropertyAssertion(:hasParent :alice :bob)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let r = realize_via_saturation_internal(&internal).expect("realize");
        let alice: HashSet<&str> = r
            .entailed_types("http://rustdl.test/alice")
            .iter()
            .map(String::as_str)
            .collect();
        let bob: HashSet<&str> = r
            .entailed_types("http://rustdl.test/bob")
            .iter()
            .map(String::as_str)
            .collect();
        assert!(
            alice.contains("http://rustdl.test/Parent"),
            "domain type missed on subject; got {alice:?}",
        );
        assert!(
            bob.contains("http://rustdl.test/Person"),
            "range type missed on object; got {bob:?}",
        );
    }

    /// Correctness gate: on a fixture where BOTH paths terminate (no
    /// defined-class `∃` to explode the tableau), the saturation fast path
    /// must produce byte-identical entailed types to the sound+complete
    /// tableau path. Exercises conjunction, subsumption chain, domain and
    /// range across three individuals. Calls the two internals directly to
    /// avoid a global-env race with the parallel test runner.
    #[test]
    fn fast_path_matches_tableau_on_terminating_fixture() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(Class(:E)) Declaration(Class(:F))\n\
    Declaration(Class(:Parent)) Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    SubClassOf(ObjectIntersectionOf(:D1 :D2) :E)\n\
    SubClassOf(:E :F)\n\
    ObjectPropertyDomain(:hasParent :Parent)\n\
    ObjectPropertyRange(:hasParent :Person)\n\
    ClassAssertion(:D1 :x)\n\
    ClassAssertion(:D2 :x)\n\
    ObjectPropertyAssertion(:hasParent :alice :bob)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        assert!(
            realize_saturation_eligible(&internal),
            "fixture must be fast-path eligible for this A/B to be meaningful",
        );
        let fast = realize_via_saturation_internal(&internal).expect("fast");
        let slow = realize_tableau_internal(&internal).expect("tableau");
        for ind in ["x", "alice", "bob"] {
            let iri = format!("http://rustdl.test/{ind}");
            let f: HashSet<&str> = fast
                .entailed_types(&iri)
                .iter()
                .map(String::as_str)
                .collect();
            let s: HashSet<&str> = slow
                .entailed_types(&iri)
                .iter()
                .map(String::as_str)
                .collect();
            assert_eq!(
                f, s,
                "entailed types differ for {ind}: fast={f:?} tableau={s:?}"
            );
        }
    }

    /// Issue #35 regression (single-query path): `is_instance_of` must
    /// TERMINATE on the reproducer and answer `no` for `a:Male` (a is not
    /// entailed to be Male).
    #[test]
    fn is_instance_of_terminates_on_issue35_reproducer() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person)) Declaration(Class(:Man)) Declaration(Class(:Woman))\n\
    Declaration(Class(:Male)) Declaration(Class(:Female))\n\
    Declaration(ObjectProperty(:hasSex)) Declaration(ObjectProperty(:hasParent))\n\
    Declaration(ObjectProperty(:isMotherOf))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    EquivalentClasses(:Man ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Male)))\n\
    EquivalentClasses(:Woman ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Female)))\n\
    ObjectPropertyDomain(:hasParent :Person)\n\
    ObjectPropertyAssertion(:isMotherOf :a :b)\n\
)\n"
        ));
        assert!(
            !is_instance_of(&onto, "http://rustdl.test/Male", "http://rustdl.test/a")
                .expect("verdict terminates")
        );
    }

    /// Issue #35 regression: `realize` must TERMINATE on the 4-axiom
    /// reproducer (previously a >300s hang via the exploding tableau).
    /// Neither individual has any entailed named type.
    #[test]
    fn realize_terminates_on_issue35_reproducer() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person)) Declaration(Class(:Man)) Declaration(Class(:Woman))\n\
    Declaration(Class(:Male)) Declaration(Class(:Female))\n\
    Declaration(ObjectProperty(:hasSex)) Declaration(ObjectProperty(:hasParent))\n\
    Declaration(ObjectProperty(:isMotherOf))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    EquivalentClasses(:Man ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Male)))\n\
    EquivalentClasses(:Woman ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Female)))\n\
    ObjectPropertyDomain(:hasParent :Person)\n\
    ObjectPropertyAssertion(:isMotherOf :a :b)\n\
)\n"
        ));
        let r = realize(&onto).expect("realization terminates");
        assert!(r.entailed_types("http://rustdl.test/a").is_empty());
        assert!(r.entailed_types("http://rustdl.test/b").is_empty());
    }

    /// Verdict-preservation guard for the bounded-classify fix.
    ///
    /// `realize_tableau_internal` bounds its internal `classify_top_down_internal`
    /// call with `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` (default 750 ms).  On any
    /// fixture whose classify completes well within that budget the two modes
    /// (bounded default vs unbounded `=0`) must produce identical output.
    ///
    /// The fixture uses `EquivalentClasses(:Nom ObjectOneOf(:a))` to push the
    /// ontology out of the EL/saturation-eligible fragment so the call
    /// actually exercises `realize_tableau_internal` (not the saturation fast
    /// path).  It also includes `SubClassOf` + `ClassAssertion` axioms so
    /// individual `:alice` acquires ≥1 most-specific type (non-vacuity).
    #[test]
    fn bounded_classify_verdict_preserved() {
        // Fixture: Nom ≡ {a} (nominal), A ⊑ B ⊑ C, alice : A.
        // alice's most-specific type must be A (leaf under A ⊑ B ⊑ C).
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
    Declaration(Class(:Nom))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:alice))\n\
    EquivalentClasses(:Nom ObjectOneOf(:a))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");

        // The nominal makes the ontology off-fragment; verify so the A/B comparison
        // is exercising `realize_tableau_internal` in both branches.
        assert!(
            !realize_saturation_eligible(&internal),
            "fixture must NOT be saturation-eligible so realize_tableau_internal is the live path",
        );

        // Save/restore RUSTDL_REALIZE_PAIR_TIMEOUT_MS under the global env lock.
        // Bounded run: use default (unset env ⟹ 750 ms).
        // Unbounded run: set env to "0".
        let bounded = {
            let _lock = crate::test_env_lock();
            #[allow(unsafe_code)]
            unsafe {
                std::env::remove_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS");
            }
            realize_tableau_internal(&internal).expect("bounded realize")
        };
        let unbounded = {
            let _lock = crate::test_env_lock();
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0");
            }
            let r = realize_tableau_internal(&internal).expect("unbounded realize");
            #[allow(unsafe_code)]
            unsafe {
                std::env::remove_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS");
            }
            r
        };

        let alice = "http://rustdl.test/alice";
        let bounded_types: std::collections::HashSet<&str> = bounded
            .entailed_types(alice)
            .iter()
            .map(String::as_str)
            .collect();
        let unbounded_types: std::collections::HashSet<&str> = unbounded
            .entailed_types(alice)
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(
            bounded_types, unbounded_types,
            "entailed_types differ: bounded={bounded_types:?} unbounded={unbounded_types:?}",
        );
        // Clone into owned Vec so we can sort without holding a borrow.
        let mut bounded_leaves: Vec<String> = bounded.most_specific_types(alice).to_vec();
        let mut unbounded_leaves: Vec<String> = unbounded.most_specific_types(alice).to_vec();
        bounded_leaves.sort();
        unbounded_leaves.sort();
        assert_eq!(
            bounded_leaves, unbounded_leaves,
            "most_specific_types differ: bounded={bounded_leaves:?} unbounded={unbounded_leaves:?}",
        );

        // Non-vacuity: alice must appear as A (which is a leaf under A ⊑ B ⊑ C).
        assert!(
            bounded_types.contains("http://rustdl.test/A"),
            "alice must be typed as A; got {bounded_types:?}",
        );
        assert_eq!(
            bounded_leaves,
            vec!["http://rustdl.test/A".to_owned()],
            "A must be alice's sole most-specific type; got {bounded_leaves:?}",
        );
    }
}
