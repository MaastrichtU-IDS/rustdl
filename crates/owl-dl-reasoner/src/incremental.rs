//! [`IncrementalSession`] — the public incremental-reasoning API (spec §8).
//!
//! A session owns a horned-owl **mirror** of the current revision, the
//! [`InternalOntology`] lowered from it, and a live
//! [`SaturationState`] (Task 6). [`IncrementalSession::apply`] commits an
//! [`AxiomDelta`] and returns the new [`Revision`]; the classification is
//! recomputed lazily on the next query.
//!
//! # P1 is addition-only
//!
//! Only a **monotone addition** can resume the retained engine. Every other
//! shape of delta — any retraction, any object-property axiom, an exhausted
//! slack gap, or a derivation pass that retracted part of its own overlay —
//! rebuilds. That is the documented P1 contract, not a defect: see
//! [`SessionStats::rebuilds`].
//!
//! # The invariant everything else rests on
//!
//! **`self.internal` never carries a dead axiom.** Nothing downstream of
//! lowering consults [`InternalOntology::live`] — `collect_el_rules`,
//! `saturate`, `PreparedOntology` and the tableau all iterate
//! `internal.axioms` in full. So a tombstone is invisible to the reasoner: an
//! axiom retracted by clearing its live bit would keep firing, a silent FALSE
//! POSITIVE in release. Every commit that would leave a tombstone behind — a
//! retraction, or a `refresh_derived` that killed part of the overlay —
//! instead re-lowers the whole post-delta mirror through
//! [`convert_ontology_seeded`], which produces an all-live axiom vector. The
//! invariant is checked by a `debug_assert` at the end of every commit.
//!
//! Re-lowering is *seeded* with the previous revision's vocabulary and concept
//! pool so entity ids stay stable across a delete. Ids are never recycled, so
//! the vocabulary then names classes no live axiom mentions — which is exactly
//! why every report is filtered through the Task 2 live signature (spec §4a).

use std::collections::HashSet;

use horned_owl::model::{AnnotatedComponent, ForIRI, MutableOntology};
use horned_owl::ontology::set::SetOntology;

use owl_dl_core::convert::{convert_component, convert_ontology, convert_ontology_seeded};
use owl_dl_core::delta::{convert_delta, refresh_derived};
use owl_dl_core::{Axiom, ClassId, ConceptPool, InternalOntology, Vocabulary};
use owl_dl_saturation::state::SaturationState;

use crate::classify::Classification;
use crate::{ReasonError, classify};

/// Reserved class-id headroom handed to the first [`SaturationState`].
///
/// New named classes are interned into the gap `[num_classes, num_classes +
/// slack)`; when it is exhausted the engine rebuilds with double the slack
/// (Task 6). 64 buys a session a few dozen new classes per rebuild at a cost
/// of 64 unused ids in the engine's per-class vectors.
const INITIAL_SLACK: usize = 64;

/// Panic message for the lowering calls that run PAST the commit point.
///
/// Everything `apply` lowers after it has touched the mirror is a subset of
/// what `stage` already lowered successfully against a scratch vocabulary:
/// `commit_addition` re-lowers exactly `staged.added_components`, and
/// `relower` lowers the post-delta mirror, i.e. components that lowered on an
/// earlier revision minus the retracted ones plus those same staged additions.
/// A failure here is therefore not a rejectable delta — the mirror has already
/// moved and §7's "previous revision, completely unmutated" no longer exists
/// to return to. It is a bug in `stage`, which must lower exactly what the
/// commit lowers.
const STAGED_LOWERING_CANNOT_FAIL: &str = "IncrementalSession: a component that lowered during staging failed to lower during the \
     commit. The mirror is already mutated, so this cannot be reported as a rejected delta \
     (spec §7) - `stage` and the commit must lower exactly the same components.";

/// A monotonically increasing revision counter. Advances **only** on a
/// committed [`IncrementalSession::apply`]; a rejected delta leaves it where
/// it was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Revision(pub u64);

/// One transaction against a session: components to add and components to
/// retract. Both are horned-owl components, i.e. the same currency the caller
/// used to build the ontology.
#[derive(Debug, Clone)]
pub struct AxiomDelta<A: ForIRI> {
    pub added: Vec<AnnotatedComponent<A>>,
    pub removed: Vec<AnnotatedComponent<A>>,
}

impl<A: ForIRI> Default for AxiomDelta<A> {
    fn default() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
        }
    }
}

/// How much of a session's work was reused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionStats {
    /// Committed revisions. Rejected deltas do not count.
    pub revisions: u64,
    /// Commits that threw the saturation engine away. In P1 that is every
    /// retraction, every object-property addition, every slack exhaustion and
    /// every commit whose derivation overlay lost an axiom.
    pub rebuilds: u64,
    /// Commits absorbed by [`SaturationState::apply_additions`] without a
    /// rebuild.
    pub additions_reused: u64,
    /// Classifications ANSWERED from the retained saturation closure
    /// ([`classify::classify_from_closure`]) rather than by re-running the
    /// hybrid classifier over the whole ontology.
    ///
    /// This is the only externally visible difference between "the session
    /// reused its engine" and "the session silently re-derived everything and
    /// happened to get the same answer". The two produce a bit-identical
    /// [`Classification`] — same `stats().fragment`, same `pure_el_mode`, same
    /// counters — because `classify_top_down_internal`'s own pure-EL early
    /// return calls the SAME `classify_pure_el`, with a freshly saturated
    /// closure that Task 6 guarantees equals the retained one. So no identity
    /// gate can tell them apart from the `Classification` alone; this counter
    /// is what lets `tests/incremental_identity_gate.rs` assert that the reuse
    /// path was actually taken.
    ///
    /// Counts recomputations only: a `classify()` served from the session's
    /// own cache does not bump it.
    pub closure_answered: u64,
}

/// What a committed delta did to the *logical* content of the ontology. Drives
/// the consistency-verdict retention (spec §10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Zero logical axioms either way: an annotation edit, or a declaration of
    /// an entity that is already in the signature.
    Empty,
    Addition,
    Retraction,
    Mixed,
}

/// Which consistency verdicts survive a commit, given its direction.
///
/// Consistency is monotone in both directions: adding axioms can only remove
/// models, so an INCONSISTENT KB stays inconsistent under a pure addition;
/// removing axioms can only add models, so a CONSISTENT KB stays consistent
/// under a pure retraction. Nothing else is safe to keep — and `None` merely
/// means "recompute", never a wrong answer.
///
/// Derived axioms do not enter the argument: they are entailed by the user
/// axioms, so they cannot change which interpretations are models.
fn retain_consistency(prev: Option<bool>, dir: Direction) -> Option<bool> {
    match (prev, dir) {
        (v, Direction::Empty) => v,
        (Some(true), Direction::Retraction) => Some(true),
        (Some(false), Direction::Addition) => Some(false),
        _ => None,
    }
}

/// An incremental reasoning session over one ontology.
///
/// ```no_run
/// # use horned_owl::model::{Build, MutableOntology, RcStr};
/// # use horned_owl::ontology::set::SetOntology;
/// # use owl_dl_reasoner::incremental::{AxiomDelta, IncrementalSession};
/// # fn main() -> Result<(), owl_dl_reasoner::ReasonError> {
/// let onto: SetOntology<RcStr> = SetOntology::new_rc();
/// let mut session = IncrementalSession::new(&onto)?;
/// session.apply(&AxiomDelta::default())?;
/// let _ = session.classify()?;
/// # Ok(())
/// # }
/// ```
pub struct IncrementalSession<A: ForIRI> {
    /// The horned-owl ontology as of the current revision. Two of the
    /// derivation passes read source components rather than the IR, so
    /// `refresh_derived` needs this to be the POST-delta ontology.
    mirror: SetOntology<A>,
    /// Lowered from `mirror`. Invariant: every axiom in it is live.
    internal: InternalOntology,
    /// The engine that survives across revisions (Task 6).
    saturation: SaturationState,
    /// Dropped whenever a commit changes logical content.
    classification: Option<Classification>,
    /// The last consistency verdict, or `None` when this revision's direction
    /// did not preserve it. See [`retain_consistency`].
    consistency: Option<bool>,
    revision: Revision,
    stats: SessionStats,
}

impl<A: ForIRI> IncrementalSession<A> {
    /// Start a session at revision 0 over `ontology`.
    ///
    /// # Errors
    /// Propagates any lowering failure from [`convert_ontology`].
    pub fn new(ontology: &SetOntology<A>) -> Result<Self, ReasonError> {
        let internal = convert_ontology(ontology)?;
        let saturation = SaturationState::build(&internal, INITIAL_SLACK);
        Ok(Self {
            mirror: ontology.clone(),
            internal,
            saturation,
            classification: None,
            consistency: None,
            revision: Revision(0),
            stats: SessionStats::default(),
        })
    }

    /// The current revision.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Reuse counters for the session so far.
    #[must_use]
    pub fn stats(&self) -> &SessionStats {
        &self.stats
    }

    /// Commit `delta` and return the new revision.
    ///
    /// **Fail-closed (spec §7).** Every component in `delta.removed` is
    /// resolved against the current revision BEFORE anything is mutated; if
    /// any one of them is absent the call returns `Err` and the session is
    /// bit-for-bit the previous revision, revision counter and stats included.
    /// Additions are lowered in the same staging pass, against a scratch copy
    /// of the vocabulary, so an unsupported construct also aborts before any
    /// mutation.
    ///
    /// The mirror is a set and `user_axioms` must stay a multiset-equal image
    /// of it, so a component is never inserted twice: adding one the mirror
    /// already has is a no-op. Naming the same component in BOTH halves of a
    /// delta nets to KEEPING it, whichever order the caller wrote them in —
    /// the commit removes and then re-inserts, so the component survives. (It
    /// still counts as a logical retraction for routing, so such a delta
    /// rebuilds; the result is correct, just not cheap.)
    ///
    /// # Errors
    /// [`ReasonError::AxiomNotPresent`] if a removal names an axiom that is
    /// not in the current revision; [`ReasonError::Conversion`] if an addition
    /// cannot be lowered.
    pub fn apply(&mut self, delta: &AxiomDelta<A>) -> Result<Revision, ReasonError> {
        let staged = self.stage(delta)?;

        // ======================= COMMIT POINT (spec §7) =======================
        // The mirror is mutated on the next line. NOTHING below may be
        // fallible: there is no previous revision left to return to, so a `?`
        // here would leave the session half-committed at a revision that never
        // happened. Every callee below therefore returns `()`, not `Result` —
        // adding a fallible step means changing a signature, which is visible
        // in review rather than silent. See STAGED_LOWERING_CANNOT_FAIL.
        for ac in &delta.removed {
            self.mirror.remove(ac);
        }
        for ac in &staged.added_components {
            self.mirror.insert(ac.clone());
        }

        let dir = staged.direction();
        if staged.removed_axioms.is_empty() {
            self.commit_addition(&staged);
        } else {
            // P1: a retraction is not expressible against the retained engine,
            // and a tombstone is invisible to it (see the module docs), so the
            // whole mirror is re-lowered.
            self.relower(staged.vocabulary, staged.concepts);
            self.stats.rebuilds += 1;
            self.classification = None;
        }

        self.revision.0 += 1;
        self.stats.revisions += 1;
        self.consistency = retain_consistency(self.consistency, dir);
        debug_assert_eq!(
            self.internal.num_live_axioms(),
            self.internal.num_axioms(),
            "IncrementalSession invariant broken: a dead axiom is still in `axioms`, where \
             saturation and the tableau will read it (they never consult `live`)."
        );
        Ok(self.revision)
    }

    /// The classification of the current revision, recomputed if a commit
    /// invalidated the cache.
    ///
    /// Reported classes are sorted by IRI and filtered through the Task 2 live
    /// signature — see [`Classification::restricted_sorted`] for why neither
    /// is optional.
    ///
    /// # Errors
    /// Propagates the classifier's error.
    pub fn classify(&mut self) -> Result<&Classification, ReasonError> {
        if self.classification.is_none() {
            let c = self.recompute_classification()?;
            self.classification = Some(c);
        }
        Ok(self
            .classification
            .as_ref()
            .expect("populated immediately above"))
    }

    /// True iff `sub ⊑ sup` is entailed by the current revision. `false` (not
    /// an error) when either IRI is not a reported class — a retraction can
    /// remove one.
    ///
    /// # Errors
    /// Propagates the classifier's error.
    pub fn is_subclass_of(&mut self, sub: &str, sup: &str) -> Result<bool, ReasonError> {
        Ok(self.classify()?.is_subclass(sub, sup))
    }

    /// Whether the current revision has a model.
    ///
    /// Answers from the cached verdict when the commits since it was computed
    /// preserved it (spec §10, [`retain_consistency`]).
    ///
    /// # Errors
    /// Propagates the consistency checker's error.
    pub fn is_consistent(&mut self) -> Result<bool, ReasonError> {
        if let Some(v) = self.consistency {
            return Ok(v);
        }
        let v = crate::is_consistent_internal(self.internal.clone())?;
        self.consistency = Some(v);
        Ok(v)
    }

    // -- internals ---------------------------------------------------------

    /// Absorb a delta that retracts nothing.
    ///
    /// Infallible on purpose — it runs past the commit point. See
    /// [`STAGED_LOWERING_CANNOT_FAIL`].
    fn commit_addition(&mut self, staged: &Staged<A>) {
        let mut new_idxs =
            convert_delta(&mut self.internal, &self.mirror, &staged.added_components)
                .expect(STAGED_LOWERING_CANNOT_FAIL);
        // Requirement of `convert_delta`: the derived overlay is recomputed in
        // the SAME commit. The passes are whole-ontology fixpoints, so an
        // overlay left over from the previous revision is stale by
        // construction.
        //
        // THE SESSION'S LOWERING IS NOT `convert_ontology(mirror)`, AND IS NOT
        // REQUIRED TO BE. `refresh_derived` restores `internal.axioms` from
        // the parked baseline before re-running the passes, so after an
        // incremental add a CONSUMING pass's input survives alongside its
        // output: `(A ⊔ B) ⊑ C` stays in `internal.axioms` next to the
        // `A ⊑ C` / `B ⊑ C` that `split_disjunctive_antecedents` emitted,
        // where a from-scratch lowering holds only the splits. That is
        // deliberate and sound — the contract is that the session's ANSWERS
        // are IRI-identical to `classify()`, never that its IR is. A GCI and
        // the conjunction of its splits have exactly the same models, so
        // retaining both adds and removes nothing. (Observable side effect:
        // the retained union GCI can push the session OUT of the EL fragment
        // on an input that classifies pure-EL from scratch — see
        // `tests/fixtures/incremental/derived-overlay.ofn` and
        // `Fixture::pure_el_in_session` in the identity gate. Different path,
        // same answer.)
        //
        // THIS ARGUMENT IS SPECIFIC TO STRENGTH-PRESERVING REWRITES. If a
        // future consuming pass performs a WEAKENING rewrite — emitting
        // something strictly implied by, rather than equivalent to, its input
        // — then keeping the strong original here would make the session
        // entail more than `classify()` does, and the identity gate would go
        // red with the session as the FALSE-POSITIVE side. A new consuming
        // pass must therefore be checked for equivalence, not just for
        // soundness.
        let diff = refresh_derived(&mut self.internal, &self.mirror);

        if !diff.killed.is_empty() {
            // The overlay lost an axiom. Killing it only cleared a live bit,
            // and saturation never reads `live` — so the retracted derived
            // axiom would keep firing. Re-lower instead.
            self.relower(
                self.internal.vocabulary.clone(),
                self.internal.concepts.clone(),
            );
            self.stats.rebuilds += 1;
            self.classification = None;
            return;
        }

        if staged.logically_empty() && diff.added.is_empty() {
            // Spec §10: an annotation edit (or a re-declaration of a known
            // entity) commits a revision with ZERO invalidation — the engine
            // is not touched and the cached classification stays valid.
            return;
        }

        new_idxs.extend(diff.added);
        let outcome = self.saturation.apply_additions(&self.internal, &new_idxs);
        if outcome.rebuilt {
            self.stats.rebuilds += 1;
        } else {
            self.stats.additions_reused += 1;
        }
        self.classification = None;
    }

    /// Re-lower the whole post-delta mirror into `vocabulary`/`concepts` and
    /// rebuild the engine over the result. Ids survive; dead axioms do not.
    ///
    /// Infallible on purpose — it runs past the commit point. See
    /// [`STAGED_LOWERING_CANNOT_FAIL`].
    fn relower(&mut self, vocabulary: Vocabulary, concepts: ConceptPool) {
        self.internal = convert_ontology_seeded(&self.mirror, vocabulary, concepts)
            .expect(STAGED_LOWERING_CANNOT_FAIL);
        self.saturation = SaturationState::build(&self.internal, self.saturation.slack());
    }

    fn recompute_classification(&mut self) -> Result<Classification, ReasonError> {
        let full = if classify::is_pure_el(&self.internal) {
            // Bumped INSIDE the branch on purpose: reading `is_pure_el` again
            // at the call site would keep counting even if this arm were
            // deleted, which is precisely the mutation the counter exists to
            // catch.
            self.stats.closure_answered += 1;
            // The retained engine's closure IS the answer on this fragment —
            // that is what the session exists for. `is_pure_el` also excludes
            // every ABox axiom, so the ABox inconsistency pre-check that
            // `classify_internal` runs on its fast path cannot apply here.
            classify::classify_from_closure(&self.internal, self.saturation.subsumers())
        } else {
            // Off the saturator's complete fragment the closure is only a
            // sound oracle, so fall back to the full hybrid classifier.
            //
            // This MUST be the same entry point as the public
            // [`crate::classify`] — i.e. the TOP-DOWN walk, not the `n²` pair
            // sweep that `classify_internal` (and its public alias
            // `classify_n2`) runs. The two are not verdict-equivalent off the
            // EL fragment: on `bench-corpus/paper5.ofn` the sweep reports 37
            // subsumptions and the top-down walk 20, because the top-down
            // label heuristic prunes pairs the sweep probes. A session that
            // answered from the sweep would give a user DIFFERENT answers from
            // the ones `classify` gives them, purely for having edited their
            // way to the axiom set — which is exactly what
            // `tests/incremental_identity_gate.rs` exists to forbid. (It is
            // also the slower of the two by ~2× on real inputs.)
            classify::classify_top_down_internal(&self.internal, None, None)?
        };
        Ok(full.restricted_sorted(&self.live_class_iris()))
    }

    /// The IRIs of classes still mentioned by a live axiom (Task 2).
    fn live_class_iris(&self) -> HashSet<String> {
        let sig = owl_dl_core::signature::compute(&self.internal);
        (0..self.internal.vocabulary.num_classes())
            .map(|i| ClassId::new(u32::try_from(i).expect("class count fits in u32")))
            .filter(|&id| sig.has_class(id))
            .map(|id| self.internal.vocabulary.class_iri(id).to_owned())
            .collect()
    }

    /// Resolve `delta` against the current revision without mutating anything.
    fn stage(&self, delta: &AxiomDelta<A>) -> Result<Staged<A>, ReasonError> {
        // Lower into throwaway copies so an id allocated while validating is
        // never visible to a rejected transaction. The copies are handed to
        // `relower` on the retraction path, so the clone is not wasted.
        let mut vocabulary = self.internal.vocabulary.clone();
        let mut concepts = self.internal.concepts.clone();

        // --- removals. Resolved BY VALUE against `user_axioms`, the pre-pass
        // --- baseline, NOT by index into `axioms`: `split_disjunctive_
        // --- antecedents` and `decompose_long_chains` CONSUME their input, so
        // --- a union-LHS or long-chain user axiom has no index at all. An
        // --- index-based lookup would reject a legitimate delete; an
        // --- index-based *prune* would leave the premise in the baseline for
        // --- `refresh_derived` to re-derive from — a false positive.
        let mut baseline: Vec<Axiom> = self.internal.user_axioms.clone();
        let mut removed_axioms: Vec<Axiom> = Vec::new();
        let mut removed_components: HashSet<AnnotatedComponent<A>> = HashSet::new();
        for ac in &delta.removed {
            if !self.mirror.i().contains(ac) || !removed_components.insert(ac.clone()) {
                return Err(ReasonError::AxiomNotPresent(format!("{:?}", ac.component)));
            }
            if let Some(ax) = convert_component(&ac.component, &mut vocabulary, &mut concepts)? {
                let Some(pos) = baseline.iter().position(|u| *u == ax) else {
                    return Err(ReasonError::AxiomNotPresent(format!("{:?}", ac.component)));
                };
                baseline.swap_remove(pos);
                removed_axioms.push(ax);
            }
        }

        // --- additions. A component the mirror already has (and this delta is
        // --- not retracting) is dropped: pushing it again would leave
        // --- `user_axioms` a strict superset of the mirror, which
        // --- `refresh_derived` reads as a different ontology.
        let mut added_components: Vec<AnnotatedComponent<A>> = Vec::new();
        let mut added_axioms: Vec<Axiom> = Vec::new();
        let mut seen_added: HashSet<AnnotatedComponent<A>> = HashSet::new();
        for ac in &delta.added {
            let present = self.mirror.i().contains(ac) && !removed_components.contains(ac);
            if present || !seen_added.insert(ac.clone()) {
                continue;
            }
            if let Some(ax) = convert_component(&ac.component, &mut vocabulary, &mut concepts)? {
                added_axioms.push(ax);
            }
            added_components.push(ac.clone());
        }

        let signature = owl_dl_core::signature::compute(&self.internal);
        Ok(Staged {
            vocabulary,
            concepts,
            added_components,
            added_axioms,
            removed_axioms,
            signature,
        })
    }
}

/// A delta resolved against the current revision, before any mutation.
struct Staged<A: ForIRI> {
    vocabulary: Vocabulary,
    concepts: ConceptPool,
    /// `delta.added` minus the components the mirror already had.
    added_components: Vec<AnnotatedComponent<A>>,
    /// What those components lower to. Shorter than `added_components`
    /// whenever a component carries no logical content.
    added_axioms: Vec<Axiom>,
    /// What `delta.removed` lowers to, each one confirmed present in the
    /// pre-pass baseline.
    removed_axioms: Vec<Axiom>,
    /// The live signature of the PRE-delta revision — used to recognise a
    /// declaration that re-declares an entity the ontology already has.
    signature: owl_dl_core::signature::LiveSignature,
}

impl<A: ForIRI> Staged<A> {
    /// True iff nothing this delta adds changes the logical content: every
    /// component either lowered to nothing (annotations, imports, ontology
    /// metadata) or re-declares an entity that is already in the signature.
    fn additions_are_inert(&self) -> bool {
        self.added_axioms
            .iter()
            .all(|ax| is_known_declaration(ax, &self.signature))
    }

    fn logically_empty(&self) -> bool {
        self.removed_axioms.is_empty() && self.additions_are_inert()
    }

    fn direction(&self) -> Direction {
        match (!self.additions_are_inert(), !self.removed_axioms.is_empty()) {
            (false, false) => Direction::Empty,
            (true, false) => Direction::Addition,
            (false, true) => Direction::Retraction,
            (true, true) => Direction::Mixed,
        }
    }
}

/// True iff `ax` declares an entity the ontology already mentions. Such a
/// declaration is logically inert: it neither introduces a class the reasoner
/// must consider nor keeps one reportable that was not already.
fn is_known_declaration(ax: &Axiom, sig: &owl_dl_core::signature::LiveSignature) -> bool {
    match ax {
        Axiom::DeclareClass(c) => sig.has_class(*c),
        Axiom::DeclareObjectProperty(r) => sig.has_role(*r),
        Axiom::DeclareNamedIndividual(i) => sig.has_individual(*i),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, retain_consistency};

    /// Spec §10, exhaustively. The two `Some(_)` retentions are the whole
    /// optimisation; every other cell MUST be `None`, because keeping a
    /// verdict there would report a stale answer as fact.
    #[test]
    fn only_the_monotone_direction_retains_a_verdict() {
        // consistent survives a delete, and a logically-empty commit.
        assert_eq!(
            retain_consistency(Some(true), Direction::Retraction),
            Some(true)
        );
        assert_eq!(retain_consistency(Some(true), Direction::Empty), Some(true));
        // inconsistent survives an add, and a logically-empty commit.
        assert_eq!(
            retain_consistency(Some(false), Direction::Addition),
            Some(false)
        );
        assert_eq!(
            retain_consistency(Some(false), Direction::Empty),
            Some(false)
        );
        // ... and nothing else is retained.
        assert_eq!(retain_consistency(Some(true), Direction::Addition), None);
        assert_eq!(retain_consistency(Some(false), Direction::Retraction), None);
        assert_eq!(retain_consistency(Some(true), Direction::Mixed), None);
        assert_eq!(retain_consistency(Some(false), Direction::Mixed), None);
        assert_eq!(retain_consistency(None, Direction::Addition), None);
        assert_eq!(retain_consistency(None, Direction::Retraction), None);
        assert_eq!(retain_consistency(None, Direction::Mixed), None);
        assert_eq!(retain_consistency(None, Direction::Empty), None);
    }
}
