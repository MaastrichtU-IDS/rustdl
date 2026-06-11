//! Class hierarchy computation — naive O(n²) baseline.
//!
//! For each ordered pair `(C, D)` of named classes in the input,
//! decide `C ⊑ D` via the standard satisfiability reduction
//! ([`crate::is_subclass_of_internal`]). The full pairwise matrix is
//! retained; convenience accessors derive equivalence classes, the
//! Hasse-direct super-class relation, and the set of classes
//! equivalent to `⊥` (unsatisfiable).
//!
//! This is *correct* but not fast — every pair triggers a fresh
//! pipeline pass (axiom expansion + NNF + absorption + tableau).
//! Phase 6's consequence-based saturation engine is the planned
//! acceleration.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;

use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{Axiom, ConceptExpr, ConceptId, ConceptPool, InternalOntology, SubRolePath};
use owl_dl_saturation::saturate;

use crate::{PreparedOntology, ReasonError};

/// `(i, j, entailed, used_saturation, timed_out)` — one entry per
/// pairwise subsumption check returned from the parallel work loop.
type PairResult = (usize, usize, bool, bool, bool);

/// Collect the IRIs of every *reportable* named class, in vocabulary
/// interning order. Excludes the synthetic `DKey(range)` filler classes
/// introduced by the integer-facet data lowering
/// ([`owl_dl_core::DKEY_IRI_PREFIX`]): they participate in the internal
/// saturation/tableau reasoning (their told-subsumptions relay datatype
/// containment through the existential machinery) but are NOT user
/// classes, so they must never appear in the classified hierarchy, the
/// unsatisfiable set, or any closure diff.
fn reportable_class_iris(internal: &InternalOntology) -> Vec<String> {
    (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .filter(|iri| !iri.starts_with(owl_dl_core::DKEY_IRI_PREFIX))
        .collect()
}

/// Result of [`classify`]. Holds the complete pairwise subsumption
/// matrix over every declared named class plus the IRIs themselves,
/// keyed by stable insertion order.
#[derive(Debug, Clone)]
pub struct Classification {
    classes: Vec<String>,
    index: HashMap<String, usize>,
    /// `entailed[i][j]` is true iff `classes[i] ⊑ classes[j]` in the
    /// input ontology (including reflexive entries `i == j`). Stored
    /// as a row-major bit-vector via `Vec<bool>`.
    entailed: Vec<Vec<bool>>,
    unsatisfiable_idxs: HashSet<usize>,
    stats: ClassificationStats,
}

/// The expressivity fragment of an ontology, used to surface
/// whether `trust_sat` is sound by construction (EL+ or Horn) or
/// sound by composition (the empirical fragment). See
/// `docs/fragment-completeness.md` for the precise contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FragmentClassification {
    /// Pure EL+ fragment (Kazakov ELK-style). Saturator alone is
    /// complete. `trust_sat` is sound by construction.
    PureEl,
    /// Horn DL-clauses (every clausified axiom has ≤ 1 head atom
    /// and the clausifier handles every axiom). The hyper Horn
    /// fixpoint is complete by construction. `trust_sat` is sound by
    /// construction. Strict superset of `PureEl` by classification,
    /// but tagged separately so users see which engine carries the
    /// guarantee.
    Horn,
    /// The ontology uses disjunctive heads, axioms the clausifier
    /// defers, or other constructs outside the provably-complete
    /// fragment. `trust_sat` is sound by composition (empirically
    /// across the measured corpus) but not formally proven.
    #[default]
    OutOfFragment,
}

impl std::fmt::Display for FragmentClassification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PureEl => f.write_str(
                "pure-EL (trust_sat sound by construction; saturator alone is complete)",
            ),
            Self::Horn => f.write_str(
                "Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)",
            ),
            Self::OutOfFragment => {
                f.write_str("out-of-EL (trust_sat empirically sound; see fragment-completeness.md)")
            }
        }
    }
}

/// Classify the expressivity fragment of an ontology. `PureEl` means
/// the saturator is complete on this input. `Horn` means the
/// clausified form has only Horn clauses (≤ 1 head atom) and no
/// deferred axioms — the hyper Horn fixpoint is complete by
/// construction. Anything else is `OutOfFragment` — the engine remains
/// empirically sound across the measured corpus, but no formal proof
/// of completeness covers it.
///
/// Cost note: when the ontology is not pure-EL this runs the
/// clausifier once to inspect the clause shape histogram
/// (`ClauseStats`). One-shot per `analyze_fragment` call (called once
/// per classify), startup-time only, not in the hot loop.
#[must_use]
pub fn analyze_fragment(internal: &InternalOntology) -> FragmentClassification {
    if is_pure_el(internal) {
        return FragmentClassification::PureEl;
    }
    let (_clauses, stats) = owl_dl_core::clause::clausify_with_stats(internal);
    if stats.disjunctive == 0 && stats.deferred == 0 {
        return FragmentClassification::Horn;
    }
    FragmentClassification::OutOfFragment
}

/// Per-call instrumentation: who decided what during the pairwise
/// classification loop. Useful for understanding when the EL
/// saturation oracle is pulling its weight versus when the tableau
/// is doing the work.
#[derive(Debug, Clone, Default)]
pub struct ClassificationStats {
    /// Pairwise subsumption queries answered `yes` by saturation's
    /// EL closure (no tableau call issued).
    pub saturation_subsumption_hits: usize,
    /// Pairwise subsumption queries that the saturation closure did
    /// not witness, dispatched to the tableau.
    pub tableau_subsumption_calls: usize,
    /// Classes flagged as `⊑ ⊥` by saturation directly (no per-class
    /// tableau probe issued).
    pub saturation_unsat_hits: usize,
    /// Classes that needed a per-class tableau satisfiability probe
    /// (saturation had no opinion).
    pub tableau_unsat_calls: usize,
    /// True iff the entire ontology fits inside the EL fragment
    /// our saturation engine is complete for — in that case the
    /// tableau is never invoked and saturation's `no` answer is
    /// itself the verdict (`no` pairs aren't counted in
    /// `saturation_subsumption_hits`, only the `yes` pairs are).
    pub pure_el_mode: bool,
    /// When the classifier was configured with a per-pair timeout,
    /// the number of pairs that hit it before the tableau returned
    /// a verdict. Those pairs default to `not subsumed` in the
    /// entailment matrix — sound (never reports a false positive),
    /// but may under-report subsumption.
    pub timed_out_pairs: usize,
    /// The `(sub, sup)` class-index pairs whose subsumption probe timed out
    /// (defaulted to "not subsumed"). Parallel to `timed_out_pairs` (the
    /// count); this is the *set*, used to verify the anytime calibration
    /// claim (every miss is a flagged-undecided pair). Populated at the same
    /// sites that bump `timed_out_pairs`.
    pub timed_out_pair_ids: Vec<(u32, u32)>,
    /// Subsumptions recovered by the defined-SUB sweep: a union-defined
    /// `C ≡ D₁ ⊔ … ⊔ Dₙ` ⊑ a primitive sup `X` where every `Dᵢ ⊑ X` holds
    /// in the EL closure (sound by construction). Added directly, no tableau.
    pub defined_sub_sweep_recovered: usize,
    /// Pairs proved subsumed by the H4 hypertableau wedge (sound
    /// `Unsat`), skipping the tableau. Zero unless the wedge is
    /// enabled (`RUSTDL_HYPERTABLEAU`).
    pub hyper_proven_pairs: usize,
    /// HF5: pairs refuted (concluded *not* subsumed) by the hyper
    /// engine's `Sat` verdict, skipping the tableau. Zero unless both
    /// `RUSTDL_HYPERTABLEAU` and `RUSTDL_HYPERTABLEAU_TRUST_SAT` are
    /// enabled — `Sat`-trust is sound only on workloads where the
    /// engine is complete (corpus-validated; off-corpus risky).
    pub hyper_refuted_pairs: usize,
    /// Wedge returned `NotSubsumed` in < `hyper_trust_sat_min_ms()` and
    /// the verdict was therefore distrusted: the tableau was asked
    /// instead. Counts each fall-through, regardless of the tableau's
    /// answer. Zero when [`hyper_trust_sat_min_ms`] returns 0.
    pub hyper_refuted_fast_pairs: u64,
    /// Subset of `hyper_refuted_fast_pairs` where the tableau actually
    /// returned `Subsumed` — the entailment the wedge would have dropped
    /// as MISSED but the slow path recovered. Directly tracks Phase 1's
    /// completeness lever.
    pub hyper_refuted_fast_flipped_pairs: u64,
    /// Per-class label heuristic (Phase 7): pairs where the orchestrator
    /// skipped `subsumes_via_tableau` because D was absent from C's
    /// label cache (sound non-subsumption via counterexample-model).
    pub label_cache_pruned: usize,
    /// Per-class label heuristic: pairs where D was present in C's
    /// label cache and the orchestrator fell through to the existing
    /// per-pair verification (might be coincidence of model).
    pub label_cache_pass_through: usize,
    /// Per-class label heuristic: pairs where the cache was missing
    /// (`NoVerdict` or hyper disabled) and the orchestrator fell through.
    pub label_cache_misses: usize,
    /// Phase 1b snapshot cache: pairs where the snapshot-replay path
    /// was consulted (some verdict returned by `try_replay`, not `None`).
    /// Sum of `*_subsumed + *_not_subsumed + *_aborts + (replay stalls)`.
    pub snapshot_replay_used: usize,
    /// Phase 1b snapshot cache: replay returned `Subsumed` (¬sup
    /// clashed with snapshot — answer used directly).
    pub snapshot_replay_subsumed: usize,
    /// Phase 1b snapshot cache: replay returned `NotSubsumed` and
    /// the orchestrator trusted it (gated by `trust_sat`).
    pub snapshot_replay_not_subsumed: usize,
    /// Phase 1b snapshot cache: replay returned `BackPropAborted`
    /// (either structural Unsafe risk or runtime sentinel fired —
    /// orchestrator fell through to wedge/tableau).
    pub snapshot_replay_aborts: usize,
    /// Phase 1b snapshot cache: pairs where the cache wasn't consulted
    /// or returned no verdict — flag OFF, ontology Unsafe, snapshot
    /// build failed (Unsat/Stalled on `sub`). Orchestrator fell through.
    pub snapshot_cache_falls_through: usize,
    /// Phase 1b.5 recon: per-sub count of pairs reaching
    /// `subsumes_via_tableau`. Keyed by sub `ClassId` index. Used to
    /// derive the pairs-per-sub distribution that determines whether
    /// snapshot caching can amortize on a workload.
    ///
    /// Temporary instrumentation — will be removed or formalized
    /// depending on the recon outcome.
    pub pairs_per_sub: std::collections::HashMap<u32, u32>,
    /// Phase 1b.5 recon: cold-wedge per-call cost histogram, in
    /// milliseconds. Bucket boundaries: 0, 1, 2-4, 5-9, 10-19,
    /// 20-49, 50-99, 100-999, ≥1000. Reset per classify run.
    pub wedge_cost_histogram_ms: [u64; 9],
    /// The expressivity fragment of the input ontology. Diagnostic only:
    /// surfaces whether `trust_sat` is sound by construction (`PureEl`)
    /// or sound by composition (`OutOfFragment`). See
    /// `docs/fragment-completeness.md`.
    pub fragment: FragmentClassification,
    /// Phase 2a recon: cumulative wall time spent in the Phase 7
    /// label cache build (per-class wedge calls). Measured at the
    /// `(0..n).into_par_iter().map(...).collect()` block in
    /// `classify_top_down_internal`. Diagnostic only.
    pub label_cache_build_wall_ms: u64,
    /// Phase 2a recon: cumulative wall time spent building snapshots
    /// in `SnapshotCache::get_or_build_snapshot`. Sum over all subs that
    /// hit the snapshot-build path (cache misses; cache hits cost
    /// near-zero). Diagnostic only.
    pub snapshot_cache_build_wall_ms: u64,
    /// Phase 2a recon: cumulative wall time spent inside
    /// `replay_with_neg_sup` / `replay_with_neg_sup_full_rerun` calls.
    /// Sum over all pairs reaching `subsumes_via_tableau` with the
    /// snapshot path active. Diagnostic only.
    pub snapshot_replay_wall_ms: u64,
    /// Phase 2a recon: top-level classify wall minus the three
    /// component fields above. Captures residual orchestrator
    /// overhead (tier walk, label-cache lookups, wedge calls that
    /// DON'T hit the snapshot path, etc.). Diagnostic only.
    pub tier_walk_wall_ms: u64,
    /// Phase 3a recon: count of classes that the per-class
    /// `BackPropRisk::classify_class` variant would mark Safe.
    /// Diagnostic only; the ontology-wide classifier still gates
    /// the snapshot cache.
    pub per_class_safe_count: usize,
    /// Phase 3a recon: count of classes that the per-class classifier
    /// would mark Unsafe. Diagnostic only.
    pub per_class_unsafe_count: usize,
    /// `ABox` consistency check fired (and the verdict was
    /// `Inconsistent`). When true, every class is unsatisfiable; the
    /// classify result mirrors Konclude's behaviour on inconsistent
    /// input. See `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`.
    pub inconsistent: bool,
}

impl Classification {
    /// Every declared class IRI in insertion order.
    #[must_use]
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// True iff `sub ⊑ sup` is entailed by the ontology.
    /// Returns `false` if either IRI is not a declared class
    /// (callers wanting a hard error should use
    /// [`crate::is_subclass_of`] directly).
    #[must_use]
    pub fn is_subclass(&self, sub: &str, sup: &str) -> bool {
        let (Some(&i), Some(&j)) = (self.index.get(sub), self.index.get(sup)) else {
            return false;
        };
        self.entailed[i][j]
    }

    /// All classes equivalent to `c` (including `c` itself). Empty if
    /// `c` is not declared in the ontology.
    #[must_use]
    pub fn equivalent_classes(&self, c: &str) -> Vec<&str> {
        let Some(&i) = self.index.get(c) else {
            return Vec::new();
        };
        (0..self.classes.len())
            .filter(|&j| self.entailed[i][j] && self.entailed[j][i])
            .map(|j| self.classes[j].as_str())
            .collect()
    }

    /// The Hasse-direct super-classes of `c`: every `D` with
    /// `c ⊑ D`, `D ≢ c`, and no intermediate `E ≠ c, D` such that
    /// `c ⊑ E ⊑ D`. Empty if `c` is not declared.
    #[must_use]
    pub fn direct_subsumers(&self, c: &str) -> Vec<&str> {
        let Some(&i) = self.index.get(c) else {
            return Vec::new();
        };
        let n = self.classes.len();
        // First: every strict super (i ⊑ j, not j ⊑ i).
        let strict_supers: Vec<usize> = (0..n)
            .filter(|&j| j != i && self.entailed[i][j] && !self.entailed[j][i])
            .collect();
        // Then: prune any `j` for which there is a `k` strictly
        // between i and j (i ⊑ k ⊑ j, neither equivalent).
        strict_supers
            .iter()
            .copied()
            .filter(|&j| {
                !strict_supers
                    .iter()
                    .any(|&k| k != j && self.entailed[k][j] && !self.entailed[j][k])
            })
            .map(|j| self.classes[j].as_str())
            .collect()
    }

    /// Per-call instrumentation for this classification: how many
    /// subsumption queries each engine handled, and how many
    /// unsatisfiable classes each engine flagged.
    #[must_use]
    pub fn stats(&self) -> ClassificationStats {
        self.stats.clone()
    }

    /// All declared classes that are equivalent to `⊥` — i.e. classes
    /// the ontology proves to be empty.
    #[must_use]
    pub fn unsatisfiable_classes(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .unsatisfiable_idxs
            .iter()
            .map(|&i| self.classes[i].as_str())
            .collect();
        out.sort_unstable();
        out
    }

    /// The `(sub, sup)` IRI pairs whose subsumption probe timed out at the
    /// configured deadline — the flagged-undecided set. A timed-out pair is
    /// reported "not subsumed" but recorded here, so a consumer knows
    /// exactly which subsumptions are unverified (the anytime contract).
    #[must_use]
    pub fn undecided_pairs(&self) -> Vec<(&str, &str)> {
        self.stats
            .timed_out_pair_ids
            .iter()
            .map(|&(i, j)| {
                (
                    self.classes[i as usize].as_str(),
                    self.classes[j as usize].as_str(),
                )
            })
            .collect()
    }
}

/// Compute the full subsumption hierarchy of `ontology` over every
/// declared named class. Returns a [`Classification`] from which
/// callers can query subsumption, equivalence, direct super-classes,
/// and the unsatisfiable-class set.
///
/// Uses the top-down traversal of the partial hierarchy
/// (`n × depth × branching` tableau calls). On every real-ontology
/// workload measured (pizza, family, RO, SIO, GO) top-down is
/// faster than the naive `n²` pair sweep; the latter remains
/// available as [`classify_n2`] for benchmarking and regression
/// cross-checks.
///
/// # Errors
///
/// See [`ReasonError`]. Any single subsumption check that errors
/// (e.g. an unsupported role chain) aborts classification with that
/// error — partial results are not surfaced.
pub fn classify<A: ForIRI>(ontology: &SetOntology<A>) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_top_down_internal(&internal, None, None)
}

/// Like [`classify`] but each pairwise tableau query is bounded by
/// `per_pair_timeout`. Pairs that exceed the timeout default to
/// `not subsumed` in the entailment matrix (sound under-approximation)
/// and bump [`ClassificationStats::timed_out_pairs`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    per_pair_timeout: std::time::Duration,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_top_down_internal(&internal, Some(per_pair_timeout), None)
}

/// Classify under a single GLOBAL wall-clock budget: the whole run shares
/// one absolute deadline. Pairs not confirmed by the deadline are reported
/// "not subsumed" and recorded in `undecided_pairs()` (sound
/// under-approximation — nothing is asserted on timeout, only omitted).
///
/// The deadline is shared across all probes in the run. Every probe uses
/// that absolute `Instant` as its `decide_with_deadline` target; a probe
/// reached late has little/no budget → times out → undecided.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_with_global_deadline<A: ForIRI>(
    ontology: &SetOntology<A>,
    budget: std::time::Duration,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let deadline = Instant::now() + budget;
    classify_top_down_internal(&internal, None, Some(deadline))
}

/// Naive `n²` pair-sweep classifier. Kept for benchmarking and
/// regression cross-checks against [`classify`]. On real workloads
/// it is consistently 2× slower than the default top-down path; new
/// code should prefer [`classify`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_n2<A: ForIRI>(ontology: &SetOntology<A>) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_internal(&internal)
}

/// Naive `n²` pair-sweep classifier with a per-pair tableau
/// deadline. Counterpart to [`classify_with_timeout`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_n2_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    per_pair_timeout: std::time::Duration,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_internal_with_timeout(&internal, Some(per_pair_timeout))
}

/// Saturation-only classifier. Skips every tableau probe (both
/// per-class satisfiability and per-pair subsumption) and returns
/// the hierarchy derivable from the EL saturation closure alone.
///
/// The result is a **sound under-approximation** of the true
/// hierarchy:
/// - every reported subsumption is genuinely entailed;
/// - subsumptions that require tableau reasoning to confirm
///   (cardinality, disjunction-with-clash, nominal merges, …)
///   are missed;
/// - classes that are unsatisfiable only via tableau reasoning
///   are reported as satisfiable.
///
/// On hybrid SROIQ workloads where saturation handles ≥ 95% of
/// real subsumptions (e.g. SIO: 10 440 saturation hits vs 5
/// tableau hits, a 0.05% loss) this mode is dramatically faster
/// than the default [`classify`] — the per-pair tableau timeouts
/// that bound the default wall are simply skipped. On SROIQ-heavy
/// workloads (pizza: 19% of subsumptions need tableau) the loss
/// is larger; check the per-ontology trade-off before using.
///
/// `ClassificationStats::pure_el_mode` is `true` regardless of
/// whether the input is structurally pure-EL — it indicates the
/// classifier *behaved* as the pure-EL path, i.e. closure-only.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_saturation_only<A: ForIRI>(
    ontology: &SetOntology<A>,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_saturation_only_internal(&internal)
}

pub(crate) fn classify_saturation_only_internal(
    internal: &InternalOntology,
) -> Result<Classification, ReasonError> {
    let classes: Vec<String> = reportable_class_iris(internal);
    let index: HashMap<String, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, iri)| (iri.clone(), i))
        .collect();
    let closure = saturate(internal);
    Ok(classify_pure_el(internal, &classes, &index, &closure))
}

/// Internal entry point. Useful for tests that hand-build an
/// [`InternalOntology`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_internal(internal: &InternalOntology) -> Result<Classification, ReasonError> {
    classify_internal_with_timeout(internal, None)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn classify_internal_with_timeout(
    internal: &InternalOntology,
    per_pair_timeout: Option<std::time::Duration>,
) -> Result<Classification, ReasonError> {
    // Snapshot the class IRIs before we clone the ontology into each
    // subsumption call. Order is the vocabulary's interning order.
    let classes: Vec<String> = reportable_class_iris(internal);
    let n = classes.len();
    let index: HashMap<String, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, iri)| (iri.clone(), i))
        .collect();

    // Run the EL saturation engine once up-front. Its closure is
    // *sound* (every entry is a genuine entailment, and every
    // `is_unsatisfiable` flag is a real ⊥) but only complete for the
    // EL fragment of the input — so we use it as a fast positive
    // oracle and fall back to the tableau when the closure has
    // nothing to say.
    let closure = saturate(internal);

    // If the entire ontology fits inside the EL fragment our
    // saturation engine recognises, the closure is *also complete*
    // — saturation's `no` answer is itself the verdict, and we
    // never need the tableau. This is the common case for partonomy
    // ontologies like Galen-EL or the SNOMED core fragment.
    //
    // Phase 2b / Phase D10: also dispatch ontologies in the SATURATOR's
    // complete fragment (EL + role-hierarchy/chains/transitivity +
    // functional/inverse-functional merge — e.g. GALEN, notgalen) to the
    // saturation fast path, skipping the redundant per-pair loop (1.86M
    // wasted pair calls on GALEN per Phase 2a recon). NOTE: this is
    // `saturator_complete_fragment`, NOT clausal-Horn — the saturator has no
    // ∀-rule, so the old `analyze_fragment == Horn` gate silently mis-
    // classified Horn-but-not-EL inputs (∀ + disjointness) and reported
    // complete; see `saturator_complete_fragment`. Gated by
    // RUSTDL_HORN_SHORTCIRCUIT (default ON) for A/B isolation.
    if is_pure_el(internal)
        || (crate::horn_shortcircuit_enabled() && saturator_complete_fragment(internal))
    {
        return Ok(classify_pure_el(internal, &classes, &index, &closure));
    }

    // Prepare the tableau-side pipeline once. Every subsequent
    // tableau query reuses the absorbed TBox, role-side metadata,
    // ABox seed, and pool — only the test concept varies.
    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // First pass: which classes are individually unsatisfiable? An
    // unsat class `C` is `⊑ ⊥` and therefore `⊑ D` for every `D` —
    // record that directly. Saturation's bot-detection flags many of
    // these without ever invoking the tableau; the rest fall back to
    // a per-class satisfiability probe. Probes are independent so
    // they run in parallel via rayon.
    let mut stats = ClassificationStats {
        fragment: analyze_fragment(internal),
        per_class_safe_count: prepared.per_class_safe_count(),
        per_class_unsafe_count: prepared.per_class_unsafe_count(),
        ..ClassificationStats::default()
    };
    let unsat_probe_results: Result<Vec<(usize, bool, bool)>, ReasonError> = (0..n)
        .into_par_iter()
        .map(|i| {
            let class_id =
                owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            if closure.is_unsatisfiable(class_id) {
                Ok((i, false, true))
            } else if let Some(timeout) = per_pair_timeout {
                let deadline = Instant::now() + timeout;
                // A timed-out unsat probe defaults to "satisfiable" —
                // sound: if the class actually were unsat the timeout
                // would have flagged it via saturation already, and
                // assuming sat here can never cause us to claim a
                // false subsumption later.
                let sat = prepared
                    .decide_with_deadline(deadline, move |pool| pool.atomic(class_id))?
                    .unwrap_or(true);
                Ok((i, sat, false))
            } else {
                let sat = prepared.decide(move |pool| pool.atomic(class_id))?;
                Ok((i, sat, false))
            }
        })
        .collect();
    let unsat_probe_results = unsat_probe_results?;
    let mut unsatisfiable_idxs: HashSet<usize> = HashSet::new();
    let mut satisfiable: Vec<bool> = vec![false; n];
    for (i, is_sat, used_saturation) in unsat_probe_results {
        if used_saturation {
            stats.saturation_unsat_hits += 1;
        } else {
            stats.tableau_unsat_calls += 1;
        }
        if is_sat {
            satisfiable[i] = true;
        } else {
            unsatisfiable_idxs.insert(i);
        }
    }

    // Second pass: pairwise subsumption. Build the worklist of
    // (i, j) pairs that need a real query (saturation-or-tableau);
    // run them in parallel; reduce into the entailment matrix and
    // stats counters. Skip rows where `i` is unsatisfiable (it
    // subsumes everything trivially — fill the row).
    let mut entailed: Vec<Vec<bool>> = vec![vec![false; n]; n];
    let mut work: Vec<(usize, usize)> = Vec::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        entailed[i][i] = true;
        if unsatisfiable_idxs.contains(&i) {
            entailed[i].iter_mut().take(n).for_each(|v| *v = true);
            continue;
        }
        for j in 0..n {
            if i == j || unsatisfiable_idxs.contains(&j) {
                continue;
            }
            work.push((i, j));
        }
    }
    let pair_results: Result<Vec<PairResult>, ReasonError> = work
        .par_iter()
        .map(|&(i, j)| {
            let sub_class =
                owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            let super_class =
                owl_dl_core::ClassId::new(u32::try_from(j).expect("class index fits in u32"));
            if closure.contains(sub_class, super_class) {
                // (i, j, entailed, used_saturation, timed_out)
                return Ok((i, j, true, true, false));
            }
            let build = move |pool: &mut ConceptPool| {
                let sub_concept = pool.atomic(sub_class);
                let super_concept = pool.atomic(super_class);
                let not_super = pool.not(super_concept);
                pool.and(vec![sub_concept, not_super])
            };
            match per_pair_timeout {
                None => {
                    let sat = prepared.decide(build)?;
                    Ok((i, j, !sat, false, false))
                }
                Some(timeout) => {
                    // Cooperative deadline: the tableau's search loop
                    // checks `Instant::now()` against this deadline on
                    // every recursion and bails out if exceeded. No
                    // extra threads, no cancellation race — the rayon
                    // worker stays bound to this single decide call.
                    let deadline = Instant::now() + timeout;
                    match prepared.decide_with_deadline(deadline, build)? {
                        Some(sat) => Ok((i, j, !sat, false, false)),
                        None => Ok((i, j, false, false, true)),
                    }
                }
            }
        })
        .collect();
    for (i, j, is_entailed, used_saturation, timed_out) in pair_results? {
        if timed_out {
            stats.timed_out_pairs += 1;
            stats.timed_out_pair_ids.push((
                u32::try_from(i).expect("class index fits in u32"),
                u32::try_from(j).expect("class index fits in u32"),
            ));
            // Sound under-approximation: default to "not subsumed".
            // Do not credit either engine — neither produced a verdict.
            continue;
        }
        if used_saturation {
            stats.saturation_subsumption_hits += 1;
        } else {
            stats.tableau_subsumption_calls += 1;
        }
        entailed[i][j] = is_entailed;
    }
    let _ = satisfiable; // currently informational only
    Ok(Classification {
        classes,
        index,
        entailed,
        unsatisfiable_idxs,
        stats,
    })
}

/// Fast-path classifier for ontologies that lie entirely inside our
/// EL saturation fragment. The closure is then *complete* — both
/// subsumption and unsatisfiability decisions reduce to closure
/// lookups, with no tableau calls. Sets `stats.pure_el_mode = true`.
fn classify_pure_el(
    internal: &InternalOntology,
    classes: &[String],
    index: &HashMap<String, usize>,
    closure: &owl_dl_saturation::Subsumers,
) -> Classification {
    let n = classes.len();
    let mut stats = ClassificationStats {
        pure_el_mode: true,
        fragment: analyze_fragment(internal),
        ..ClassificationStats::default()
    };
    let mut unsatisfiable_idxs: HashSet<usize> = HashSet::new();
    let mut entailed: Vec<Vec<bool>> = vec![vec![false; n]; n];
    for (i, row) in entailed.iter_mut().enumerate().take(n) {
        row[i] = true;
        let class_id =
            owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        if closure.is_unsatisfiable(class_id) {
            unsatisfiable_idxs.insert(i);
            stats.saturation_unsat_hits += 1;
            for v in row.iter_mut() {
                *v = true;
            }
        }
    }
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if unsatisfiable_idxs.contains(&i) {
            continue;
        }
        let sub_class =
            owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        #[allow(clippy::needless_range_loop)]
        for j in 0..n {
            if i == j || unsatisfiable_idxs.contains(&j) {
                continue;
            }
            let super_class =
                owl_dl_core::ClassId::new(u32::try_from(j).expect("class index fits in u32"));
            if closure.contains(sub_class, super_class) {
                entailed[i][j] = true;
                stats.saturation_subsumption_hits += 1;
            }
        }
    }
    let _ = internal; // closure was built from this; nothing more to read
    Classification {
        classes: classes.to_vec(),
        index: index.clone(),
        entailed,
        unsatisfiable_idxs,
        stats,
    }
}

/// Build a `Classification` representing an inconsistent ontology:
/// every class is unsatisfiable and therefore a subclass of every
/// other class (the trivial entailment under inconsistency). Mirrors
/// Konclude's behaviour. Used when the `ABox` consistency pre-check
/// fires.
fn classify_inconsistent(
    classes: Vec<String>,
    index: HashMap<String, usize>,
    fragment: FragmentClassification,
) -> Classification {
    let n = classes.len();
    let entailed = vec![vec![true; n]; n];
    let unsatisfiable_idxs: HashSet<usize> = (0..n).collect();
    let stats = ClassificationStats {
        inconsistent: true,
        fragment,
        ..ClassificationStats::default()
    };
    Classification {
        classes,
        index,
        entailed,
        unsatisfiable_idxs,
        stats,
    }
}

/// True iff every axiom in `internal` lies inside the EL fragment
/// the saturation engine is complete for. A conservative check: any
/// construct outside the supported shapes (disjunction, complement,
/// cardinality, nominals, inverse roles, role characteristics that
/// expand to cardinality, `ABox` assertions, ...) immediately returns
/// `false`.
pub(crate) fn is_pure_el(internal: &InternalOntology) -> bool {
    internal
        .axioms
        .iter()
        .all(|ax| is_el_axiom(ax, &internal.concepts))
}

fn is_el_axiom(ax: &Axiom, pool: &ConceptPool) -> bool {
    match ax {
        Axiom::SubClassOf { sub, sup } => is_el_concept(*sub, pool) && is_el_concept(*sup, pool),
        Axiom::EquivalentClasses(members) => members.iter().all(|c| is_el_concept(*c, pool)),
        Axiom::DisjointClasses(members) => members.iter().all(|c| is_el_concept(*c, pool)),
        Axiom::SubObjectPropertyOf { sub, sup } => {
            if sup.is_inverse() {
                return false;
            }
            match sub {
                SubRolePath::Role(r) => !r.is_inverse(),
                SubRolePath::Chain(parts) => {
                    parts.len() == 2 && parts.iter().all(|r| !r.is_inverse())
                }
            }
        }
        Axiom::EquivalentObjectProperties(roles) => roles.iter().all(|r| !r.is_inverse()),
        Axiom::TransitiveRole(role) => !role.is_inverse(),
        Axiom::ObjectPropertyDomain { role, domain } => {
            !role.is_inverse() && is_el_concept(*domain, pool)
        }
        Axiom::ObjectPropertyRange { role, range } => {
            !role.is_inverse() && is_el_concept(*range, pool)
        }
        Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => true,
        // Everything else (ABox assertions, role characteristics that
        // expand to cardinality, disjoint object properties, ...) is
        // outside the saturation fragment.
        _ => false,
    }
}

fn is_el_concept(c: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(c) {
        ConceptExpr::Top | ConceptExpr::Atomic(_) => true,
        ConceptExpr::And(ops) => ops.iter().all(|op| is_el_concept(*op, pool)),
        ConceptExpr::Some(role, body) => !role.is_inverse() && is_el_concept(*body, pool),
        _ => false,
    }
}

/// The fragment on which the **EL saturator is COMPLETE** — the sound gate
/// for the Horn-shortcircuit (Phase D10, 2026-06-09). A *clausal*-Horn
/// ontology is NOT enough: the saturator is complete on EL plus the
/// extensions its rules actually run (role hierarchy, length-≤2 chains,
/// transitivity, functional / inverse-functional witness-merge, domain,
/// range), but it has **no ∀-rule and no qualified-cardinality / general
/// disjunction reasoning**. So `∀`, `≤n`, `⊔`, nominals, inverse-role *use*,
/// etc. can make it silently MISS entailments while the closure reports
/// "complete" — proven by `∃p.K3 ⊓ ∀p.K1020` + `K3 ⊓ K1020 ⊑ ⊥`, which is
/// clausal-Horn yet the saturator reports C satisfiable. (Earlier the
/// shortcircuit keyed on `analyze_fragment == Horn`, which is exactly this
/// unsound clausal test.)
///
/// This is a STRICT allowlist anchored to the constructs the saturator's
/// rules genuinely process (the D9 fragment map: COMPLETE = Atomic / ⊓ / ∃ /
/// the listed role axioms); anything outside ⟹ `false` ⟹ the caller falls
/// back to the sound+complete hybrid path. Deliberately conservative:
/// `DisjointClasses` is EXCLUDED here even though [`is_pure_el`] permits it,
/// because disjointness combined with the functional witness-merge is an
/// unproven interaction — functional+disjoint ontologies fall back rather
/// than risk a silent miss; pure-EL+disjoint still takes the separate
/// `is_pure_el` arm. GALEN/notgalen (functional, no disjoint, no ∀, no
/// chains>2, no inverse) stay on the fast path — verified by
/// `galen_notgalen_in_saturator_fragment` + the corpus FP/MISSED gate.
pub(crate) fn saturator_complete_fragment(internal: &InternalOntology) -> bool {
    internal
        .axioms
        .iter()
        .all(|ax| is_saturator_axiom(ax, &internal.concepts))
}

fn is_saturator_axiom(ax: &Axiom, pool: &ConceptPool) -> bool {
    match ax {
        Axiom::SubClassOf { sub, sup } => {
            is_saturator_concept(*sub, pool) && is_saturator_concept(*sup, pool)
        }
        Axiom::EquivalentClasses(members) => members.iter().all(|c| is_saturator_concept(*c, pool)),
        Axiom::SubObjectPropertyOf { sub, sup } => {
            !sup.is_inverse()
                && match sub {
                    SubRolePath::Role(r) => !r.is_inverse(),
                    SubRolePath::Chain(parts) => {
                        parts.len() == 2 && parts.iter().all(|r| !r.is_inverse())
                    }
                }
        }
        Axiom::EquivalentObjectProperties(roles) => roles.iter().all(|r| !r.is_inverse()),
        // The saturator fully processes these role axioms: transitivity +
        // length-2 chains (CR-chain), CR9 hierarchy, and the Phase-2
        // functional / inverse-functional witness-merge.
        Axiom::TransitiveRole(role)
        | Axiom::FunctionalRole(role)
        | Axiom::InverseFunctionalRole(role) => !role.is_inverse(),
        Axiom::ObjectPropertyDomain { role, domain } => {
            !role.is_inverse() && is_saturator_concept(*domain, pool)
        }
        Axiom::ObjectPropertyRange { role, range } => {
            !role.is_inverse() && is_saturator_concept(*range, pool)
        }
        Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => true,
        // EXCLUDED ⟹ fall back to the hybrid path. DisjointClasses /
        // DisjointUnion (disjoint×functional-merge unproven); all ABox
        // assertions; InverseObjectProperties decls; Symmetric / Asymmetric
        // / Reflexive / Irreflexive; DisjointObjectProperties;
        // SameIndividual / DifferentIndividuals — none fully reasoned over
        // by the saturator.
        _ => false,
    }
}

/// Concept fragment the saturator reasons over completely: EL
/// (`Top` / `Atomic` / `⊓` / `∃` over forward roles). `Min(n≥1)` is a sound
/// existential under-approximation for subsumption but is EXCLUDED here
/// (conservative — `Min(≥2)` + functional is a cardinality interaction); and
/// `All` / `Max` / `Or` / `Not` / `Nominal` / `Bot`-filler all ⟹ `false`.
fn is_saturator_concept(c: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(c) {
        ConceptExpr::Top | ConceptExpr::Atomic(_) => true,
        ConceptExpr::And(ops) => ops.iter().all(|op| is_saturator_concept(*op, pool)),
        ConceptExpr::Some(role, body) => !role.is_inverse() && is_saturator_concept(*body, pool),
        _ => false,
    }
}

// ─── Top-down classification ─────────────────────────────────────
//
// The naive [`classify_internal_with_timeout`] tests `n²` ordered
// pairs. On hierarchies dominated by "this class is *not* subsumed
// by that one" pairs — the typical real-ontology shape — most
// queries return `false` after a full tableau call. Top-down
// classification (Tsarkov & Horrocks 2005) walks the partial
// hierarchy built so far, only testing candidates whose closure +
// already-confirmed subsumptions don't already settle the question.
//
// For an ontology of depth `d` and branching factor `b`, top-down
// does roughly `n × d × b` tableau calls instead of `n²` — a real
// reduction once `n` exceeds a few hundred. For SULO at `n = 17`
// the savings are modest; for SIO at `n = 1585` it's the difference
// between feasibility and not.
//
// This commit ships `classify_top_down_internal` + a public
// `classify_top_down` wrapper. The CLI doesn't surface it yet
// (intentional — perf comparison vs. the naive path happens in a
// follow-up). Tests confirm bit-identical `Classification` output
// on the existing in-tree test ontologies.

/// Top-down counterpart to [`classify`]. Tests pairs against an
/// incrementally-built partial hierarchy instead of the full
/// `n × n` matrix. See the module-level comment above this function
/// for the algorithmic shape.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_top_down<A: ForIRI>(
    ontology: &SetOntology<A>,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_top_down_internal(&internal, None, None)
}

/// Top-down classifier with an optional per-pair tableau timeout
/// (same semantics as [`classify_with_timeout`]).
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_top_down_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    per_pair_timeout: std::time::Duration,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_top_down_internal(&internal, Some(per_pair_timeout), None)
}

/// Returns true iff the ontology contains any `ABox` axiom. Cheap
/// scan over `internal.axioms` used to skip the `ABox` consistency
/// pre-check entirely on TBox-only inputs (e.g. GALEN), where
/// building `PreparedOntology` solely to consult `abox_verdict()`
/// is wasted work — the check would early-return `Unknown` on
/// empty `individuals` anyway. Microseconds even on the largest
/// corpus ontologies. See
/// `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`
/// performance contract.
fn has_abox_axioms(internal: &owl_dl_core::ontology::InternalOntology) -> bool {
    internal.axioms.iter().any(|ax| {
        matches!(
            ax,
            owl_dl_core::ontology::Axiom::ClassAssertion { .. }
                | owl_dl_core::ontology::Axiom::ObjectPropertyAssertion { .. }
                | owl_dl_core::ontology::Axiom::NegativeObjectPropertyAssertion { .. }
                | owl_dl_core::ontology::Axiom::SameIndividual(_)
                | owl_dl_core::ontology::Axiom::DifferentIndividuals(_)
        )
    })
}

/// Compute the effective deadline for a single probe from the two
/// deadline sources. Either source may be absent:
/// - If both are set, use the earlier (min) of `global` and
///   `now() + per_pair`.
/// - If only one is set, use it.
/// - If neither is set, return `None` (unbounded).
///
/// The per-pair term is re-evaluated at call time (`Instant::now()`)
/// so each probe gets a fresh budget even when called sequentially
/// (matches the existing `Instant::now() + timeout` pattern).
#[inline]
fn effective_deadline(
    global: Option<Instant>,
    per_pair: Option<std::time::Duration>,
) -> Option<Instant> {
    match (global, per_pair) {
        (Some(gd), Some(t)) => Some(gd.min(Instant::now() + t)),
        (Some(gd), None) => Some(gd),
        (None, Some(t)) => Some(Instant::now() + t),
        (None, None) => None,
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn classify_top_down_internal(
    internal: &InternalOntology,
    per_pair_timeout: Option<std::time::Duration>,
    global_deadline: Option<Instant>,
) -> Result<Classification, ReasonError> {
    // Phase 2a recon: top-level classify wall, used to derive
    // tier_walk_wall_ms = total - (label_cache + snapshot_build + replay).
    let classify_start = std::time::Instant::now();
    let classes: Vec<String> = reportable_class_iris(internal);
    let n = classes.len();
    let index: HashMap<String, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, iri)| (iri.clone(), i))
        .collect();

    let closure = saturate(internal);

    // Pure-EL path: the closure is complete; reuse the naive
    // classifier's fast path. Top-down only earns its complexity on
    // hybrid inputs where the tableau actually runs.
    //
    // Phase 2b / Phase D10: dispatch ontologies in the SATURATOR's complete
    // fragment to the saturation fast path (see `saturator_complete_fragment`
    // — NOT clausal-Horn; the latter silently mis-classified Horn-but-not-EL
    // ∀ inputs). See spec §5 + docs/phase2a-recon.md. Gated by
    // RUSTDL_HORN_SHORTCIRCUIT.
    if is_pure_el(internal)
        || (crate::horn_shortcircuit_enabled() && saturator_complete_fragment(internal))
    {
        // Skip the ABox check entirely on ABox-free inputs — building
        // PreparedOntology costs ~1.5 s on GALEN-sized TBoxes (NNF +
        // absorb + role-hierarchy + closure), and abox_check itself
        // would early-return `Unknown` on empty `individuals`. The
        // inline scan below is an O(n) walk over the axiom list,
        // microseconds even on GALEN.
        if crate::abox_check_enabled() && has_abox_axioms(internal) {
            let prepared = PreparedOntology::from_internal(internal.clone())?;
            if let crate::abox_check::AboxVerdict::Inconsistent { reason } = prepared.abox_verdict()
            {
                if std::env::var_os("RUSTDL_TRACE").is_some() {
                    eprintln!("abox_check: inconsistent — {reason:?}");
                }
                return Ok(classify_inconsistent(
                    classes,
                    index,
                    analyze_fragment(internal),
                ));
            }
        }
        return Ok(classify_pure_el(internal, &classes, &index, &closure));
    }

    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // Sound ABox-driven inconsistency pre-check. If it fires, return
    // an every-class-unsatisfiable Classification (mirroring Konclude).
    if let crate::abox_check::AboxVerdict::Inconsistent { reason } = prepared.abox_verdict() {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("abox_check: inconsistent — {reason:?}");
        }
        return Ok(classify_inconsistent(
            classes,
            index,
            analyze_fragment(internal),
        ));
    }

    // Per-class unsat probes — identical to the naive path. Reuse
    // the same parallel pattern.
    let mut stats = ClassificationStats {
        fragment: analyze_fragment(internal),
        ..ClassificationStats::default()
    };

    // Phase 7: per-class label heuristic. Run wedge satisfiability per
    // named class ONCE; cache the root-node labels as a sound
    // non-subsumption pruner. Parallel via rayon — independent calls,
    // ~0.5-2 ms each (Horn case) + occasional slower disjunctive
    // cases. Consulted by find_direct_parents_top_down. See
    // docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md.
    //
    // Disabled via `RUSTDL_LABEL_HEURISTIC=0`: every slot becomes
    // `NoVerdict`, so the walk falls through to the wedge/tableau
    // path uniformly (used by tests that exercise the wedge directly).
    let label_cache_start = Instant::now();
    let label_cache: Vec<crate::LabelOracle> = if crate::label_heuristic_enabled() {
        // Phase 8: cache-build deadline is independent of per_pair_timeout.
        // The per-pair budget (typically 200 ms) is too tight for the ~5%
        // SROIQ classes that need a few hundred ms of wedge satisfiability;
        // cutting them off at NoVerdict bloats the tier walk's cache-miss
        // bucket. See `docs/phase8-recon.md`. Default 5000 ms; set
        // `RUSTDL_LABEL_CACHE_TIMEOUT_MS=0` for unbounded.
        //
        // Global-deadline cap: if a global wall-clock budget is active, each
        // per-class label deadline is capped at `global_deadline` via
        // `effective_deadline`. This ensures `classify_with_global_deadline`
        // actually returns near its promised deadline even when many label
        // classes stall (e.g. wine). The per-pair budget is still deliberately
        // NOT used here — the Phase-8 independence is preserved.
        let cache_ms = crate::label_cache_timeout_ms();
        let per_class_cache_dur = if cache_ms == 0 {
            None
        } else {
            Some(std::time::Duration::from_millis(cache_ms))
        };
        (0..n)
            .into_par_iter()
            .map(|i| {
                // Skip entirely once the global deadline has passed: there is no
                // point paying for a per-class wedge call that will instant-timeout
                // anyway. `NoVerdict` is sound — it makes the unsat-probe and
                // tier-walk fall through to the already-gd-bounded probe path.
                if global_deadline.is_some_and(|gd| Instant::now() >= gd) {
                    return crate::LabelOracle::NoVerdict;
                }
                let class_id =
                    owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
                // Effective deadline: earlier of the global deadline and the
                // per-class cache budget. When global_deadline is None, this
                // reproduces the pre-fix behaviour exactly.
                let deadline = effective_deadline(global_deadline, per_class_cache_dur);
                prepared.classify_labels(class_id, deadline)
            })
            .collect()
    } else {
        vec![crate::LabelOracle::NoVerdict; n]
    };
    stats.label_cache_build_wall_ms =
        u64::try_from(label_cache_start.elapsed().as_millis()).unwrap_or(u64::MAX);

    let unsat_probe_results: Result<Vec<(usize, bool, bool)>, ReasonError> = (0..n)
        .into_par_iter()
        .map(|i| {
            let class_id =
                owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            if closure.is_unsatisfiable(class_id) {
                return Ok((i, false, true));
            }
            // Perf (unsat-probe de-redundancy, 2026-06-10): the Phase-7 label
            // cache (the WEDGE) already decided satisfiability for most classes
            // during its build — reuse that verdict instead of re-running the
            // MAIN TABLEAU once per class (profiled as the dominant classify
            // wall: ~6 s alehif / ~22 s ore-10908; see the global-model spec's
            // TIER-WALK PROFILE). Soundness: `LabelOracle::Unsat` is a wedge
            // `Unsat` — sound for any ontology (the trusted direction) and
            // already trusted in `find_direct_parents_top_down`; `Sat` matches
            // the trust_sat model the label cache + pruning already rely on.
            // `NoVerdict`/absent (heuristic off, or build deadline) falls
            // through to the main-tableau probe unchanged. Third tuple field is
            // the "used_saturation" stat flag — `true` = "decided without a
            // tableau call" (wedge), keeping `tableau_unsat_calls` honest.
            if crate::unsat_via_labels_enabled() {
                match label_cache.get(i) {
                    Some(crate::LabelOracle::Unsat) => return Ok((i, false, true)),
                    Some(crate::LabelOracle::Sat(_)) => {
                        // Concrete-domain verify: the wedge has no `card_sat`
                        // and does not materialise DKey cardinality, so it
                        // reports a counting-clash class `Sat`. For a class
                        // carrying a `Min`/`Max`-over-DKey constraint (or a
                        // saturation-subclass of one), don't trust that `Sat`
                        // — fall through to the main tableau (which runs
                        // `concrete_domain_clash`). Sound: only swaps a wedge
                        // `Sat` for the complete path. Empty set ⇒ no overhead.
                        let needs_verify = !prepared.data_counting_classes.is_empty()
                            && (prepared.data_counting_classes.contains(&class_id)
                                || closure
                                    .subsumers_of(class_id)
                                    .iter()
                                    .any(|s| prepared.data_counting_classes.contains(s)));
                        if !needs_verify {
                            return Ok((i, true, true));
                        }
                        // else: fall through to the main-tableau probe below.
                    }
                    Some(crate::LabelOracle::NoVerdict) | None => {}
                }
            }
            // Use effective_deadline so that a global wall-clock budget
            // bounds the unsat probe just as it bounds pair probes.
            if let Some(deadline) = effective_deadline(global_deadline, per_pair_timeout) {
                // Robustness: a `NoVerdict` (tableau internal cap, hit
                // on large workloads like SIO) is treated as "possibly
                // satisfiable" — the class survives the unsat probe,
                // sound under-approximation. Crashing classify on a
                // single oversized class is worse.
                let sat = match prepared
                    .decide_with_deadline(deadline, move |pool| pool.atomic(class_id))
                {
                    Ok(Some(s)) => s,
                    Ok(None) | Err(crate::ReasonError::NoVerdict) => true,
                    Err(other) => return Err(other),
                };
                Ok((i, sat, false))
            } else {
                let sat = prepared.decide(move |pool| pool.atomic(class_id))?;
                Ok((i, sat, false))
            }
        })
        .collect();
    let unsat_probe_results = unsat_probe_results?;
    let mut unsatisfiable_idxs: HashSet<usize> = HashSet::new();
    for (i, is_sat, used_saturation) in unsat_probe_results {
        if used_saturation {
            stats.saturation_unsat_hits += 1;
        } else {
            stats.tableau_unsat_calls += 1;
        }
        if !is_sat {
            unsatisfiable_idxs.insert(i);
        }
    }

    // Sort the satisfiable classes by ascending closure-subsumer
    // count — "most general first". This ordering means when we
    // place class `c`, every class that could be `c`'s parent has
    // already been placed (modulo same-tier siblings, which are
    // handled by the walk's iterative refinement).
    let mut order: Vec<usize> = (0..n).filter(|i| !unsatisfiable_idxs.contains(i)).collect();
    order.sort_by_key(|&i| {
        closure
            .subsumers_of(owl_dl_core::ClassId::new(
                u32::try_from(i).expect("class index fits in u32"),
            ))
            .len()
    });

    // `direct_supers[i]` = direct super-classes of `i` placed so
    // far. The hierarchy is built tier-by-tier: a tier is the set
    // of classes that share a closure-subsumer count. Within a
    // tier, classes are independent of each other w.r.t. the
    // hierarchy walk (none has been placed yet; they don't appear
    // in any frontier), so the tier processes in parallel via
    // rayon. Cross-tier subsumption that the walk can't see is
    // recovered by the closure-seed step in the entailment-matrix
    // builder below.
    let mut direct_supers: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut direct_children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut top_level: Vec<usize> = Vec::new();

    // Group `order` into tiers of equal closure-subsumer count.
    let mut tiers: Vec<Vec<usize>> = Vec::new();
    {
        let mut current: Vec<usize> = Vec::new();
        let mut current_rank: Option<usize> = None;
        for &c in &order {
            let rank = closure
                .subsumers_of(owl_dl_core::ClassId::new(
                    u32::try_from(c).expect("class index fits in u32"),
                ))
                .len();
            if current_rank.is_some_and(|r| r != rank) {
                tiers.push(std::mem::take(&mut current));
            }
            current_rank = Some(rank);
            current.push(c);
        }
        if !current.is_empty() {
            tiers.push(current);
        }
    }

    for tier in &tiers {
        // Each tier member walks the snapshot of `direct_children`
        // + `top_level` as of tier entry and returns its
        // `direct_parents` + a stats delta. Parallel since none of
        // them read or write each other's slot.
        let tier_results: Result<Vec<(usize, Vec<usize>, ClassificationStats)>, ReasonError> = tier
            .par_iter()
            .map(|&c| {
                let mut local_stats = ClassificationStats::default();
                let parents = find_direct_parents_top_down(
                    c,
                    &closure,
                    &prepared,
                    &direct_supers,
                    &direct_children,
                    &top_level,
                    per_pair_timeout,
                    global_deadline,
                    &label_cache,
                    &mut local_stats,
                )?;
                Ok((c, parents, local_stats))
            })
            .collect();
        let tier_results = tier_results?;
        // Serial merge of the tier's results into the global state.
        for (c, parents, sd) in tier_results {
            stats.saturation_subsumption_hits += sd.saturation_subsumption_hits;
            stats.tableau_subsumption_calls += sd.tableau_subsumption_calls;
            stats.timed_out_pairs += sd.timed_out_pairs;
            stats
                .timed_out_pair_ids
                .extend(sd.timed_out_pair_ids.iter().copied());
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
            stats.label_cache_pruned += sd.label_cache_pruned;
            stats.label_cache_pass_through += sd.label_cache_pass_through;
            stats.label_cache_misses += sd.label_cache_misses;
            stats.snapshot_replay_used += sd.snapshot_replay_used;
            stats.snapshot_replay_subsumed += sd.snapshot_replay_subsumed;
            stats.snapshot_replay_not_subsumed += sd.snapshot_replay_not_subsumed;
            stats.snapshot_replay_aborts += sd.snapshot_replay_aborts;
            stats.snapshot_cache_falls_through += sd.snapshot_cache_falls_through;
            for (k, v) in sd.pairs_per_sub {
                *stats.pairs_per_sub.entry(k).or_insert(0) += v;
            }
            for (i, cnt) in sd.wedge_cost_histogram_ms.iter().enumerate() {
                stats.wedge_cost_histogram_ms[i] += cnt;
            }
            for &p in &parents {
                direct_children[p].push(c);
            }
            if parents.is_empty() {
                top_level.push(c);
            }
            direct_supers[c] = parents;
        }
    }

    // Defined-sup sweep: same-tier inferred subsumptions are missed by
    // the parallel walk above ("two same-tier classes don't see each
    // other"). Empirically on pizza, **every** such missed sup is a
    // class with an `EquivalentClasses(Name, ComplexExpr)` axiom
    // (definitions like `VegetarianTopping ≡ Topping ⊓ ¬(Meat ⊔ Fish)`
    // or `SpicyPizza ≡ Pizza ⊓ ∃hT.SpicyTopping`), and the gap closes
    // when we test each candidate sub against those defined sups
    // directly. A naive within-tier `n²` is intractable
    // (> 24 min on pizza); restricting the sup side to defined classes
    // cuts the cost to `defined_count × all_classes` (~1.5 k pairs on
    // pizza), tightening the per-pair budget to 200 ms (most wedge
    // calls finish in < 100 ms; the slow tail times out as "not
    // subsumed" — sound under-approximation), parallel via rayon.
    let defined_sups: Vec<usize> = {
        let mut set: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for ax in &internal.axioms {
            if let owl_dl_core::ontology::Axiom::EquivalentClasses(ids) = ax {
                let has_complex = ids.iter().any(|c| {
                    !matches!(
                        internal.concepts.get(*c),
                        owl_dl_core::ir::ConceptExpr::Atomic(_)
                    )
                });
                if has_complex {
                    for c in ids {
                        if let owl_dl_core::ir::ConceptExpr::Atomic(cls) = internal.concepts.get(*c)
                        {
                            let i = cls.index() as usize;
                            if i < n && !unsatisfiable_idxs.contains(&i) {
                                set.insert(i);
                            }
                        }
                    }
                }
            }
        }
        set.into_iter().collect()
    };
    // Sweep budget: honour the caller's per_pair_timeout so that
    // pairs requiring more than the default 200 ms (e.g. ones that
    // need the hyper wedge but converge in 1–5 s) aren't silently
    // dropped to "not subsumed". Before this fix, the sweep
    // hardcoded 200 ms regardless of the caller's request, which
    // caused MISSED entailments on GALEN's PathologicalCondition
    // pattern and SIO/notgalen residuals that the wedge proves
    // sub-second via direct probe but exceed 200 ms under the
    // top-down classifier's tier-parallel load.
    let sweep_budget = per_pair_timeout.unwrap_or(std::time::Duration::from_millis(200));
    for &sup in &defined_sups {
        let sup_id =
            owl_dl_core::ClassId::new(u32::try_from(sup).expect("class index fits in u32"));
        // Parallel test of candidate subs. Skip pairs already known via
        // closure or the existing direct-supers transitive closure.
        let already_known: std::collections::HashSet<usize> = {
            let mut s: std::collections::HashSet<usize> = std::collections::HashSet::new();
            // BFS down from `sup` to collect already-known subs.
            let mut frontier = direct_children[sup].clone();
            while let Some(c) = frontier.pop() {
                if s.insert(c) {
                    frontier.extend(direct_children[c].iter().copied());
                }
            }
            s
        };
        let candidates: Vec<usize> = (0..n)
            .filter(|&cand| cand != sup && !unsatisfiable_idxs.contains(&cand))
            .filter(|&cand| !already_known.contains(&cand))
            .filter(|&cand| {
                let cand_id = owl_dl_core::ClassId::new(
                    u32::try_from(cand).expect("class index fits in u32"),
                );
                !closure.contains(cand_id, sup_id)
            })
            .collect();
        let probe_results: Vec<(usize, bool, ClassificationStats)> = candidates
            .par_iter()
            .map(|&cand| {
                let cand_id = owl_dl_core::ClassId::new(
                    u32::try_from(cand).expect("class index fits in u32"),
                );
                let mut local_stats = ClassificationStats::default();
                // Defined-sup sweep. The `trust_sat` parameter is
                // available here for future selective verification
                // (the wedge's `NotSubsumed` is incomplete on
                // functional-role + ≥n-with-disjointness, undercounting
                // real entailments on GALEN/notgalen-style ontologies).
                // Set to `false` here to disregard the wedge's Sat
                // verdicts; we currently default to `true` because
                // GALEN's 699 defined classes × ~2700 candidates =
                // ~1.9M pair-tests is too expensive at any per-pair
                // budget that's actually useful. Users who need full
                // completeness can opt out of trust-Sat globally with
                // `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` (slow, but recovers
                // the ~109 GALEN / ~27 notgalen MISSED). Future work
                // can wire a selective verification heuristic here.
                let subsumed = subsumes_via_tableau(
                    &prepared,
                    cand_id,
                    sup_id,
                    Some(sweep_budget),
                    global_deadline,
                    true,
                    &mut local_stats,
                )
                .ok()
                .flatten()
                .unwrap_or(false);
                (cand, subsumed, local_stats)
            })
            .collect();
        for (cand, subsumed, sd) in probe_results {
            stats.saturation_subsumption_hits += sd.saturation_subsumption_hits;
            stats.tableau_subsumption_calls += sd.tableau_subsumption_calls;
            stats.timed_out_pairs += sd.timed_out_pairs;
            stats
                .timed_out_pair_ids
                .extend(sd.timed_out_pair_ids.iter().copied());
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
            stats.snapshot_replay_used += sd.snapshot_replay_used;
            stats.snapshot_replay_subsumed += sd.snapshot_replay_subsumed;
            stats.snapshot_replay_not_subsumed += sd.snapshot_replay_not_subsumed;
            stats.snapshot_replay_aborts += sd.snapshot_replay_aborts;
            stats.snapshot_cache_falls_through += sd.snapshot_cache_falls_through;
            for (k, v) in sd.pairs_per_sub {
                *stats.pairs_per_sub.entry(k).or_insert(0) += v;
            }
            for (i, cnt) in sd.wedge_cost_histogram_ms.iter().enumerate() {
                stats.wedge_cost_histogram_ms[i] += cnt;
            }
            if subsumed && !direct_supers[cand].contains(&sup) {
                direct_supers[cand].push(sup);
                direct_children[sup].push(cand);
            }
        }
    }

    // Defined-SUB sweep (cluster A; wine residual-31, 2026-06-07). The
    // defined-sup sweep above only tests pairs whose SUP is a defined class.
    // A union/covering-defined SUB `C ≡ D₁ ⊔ … ⊔ Dₙ` ⊑ a *primitive* sup X
    // (e.g. `Fruit ≡ NonSweetFruit ⊔ SweetFruit ⊑ EdibleThing`, where
    // `EdibleThing` is `SubClassOf`-only) is missed by BOTH the tier-walk (the
    // covering subsumption isn't in the EL closure) AND the defined-sup sweep
    // (X is primitive). Recover it soundly *by construction*: if the sound EL
    // closure has `Dᵢ ⊑ X` for EVERY disjunct, then `C ⊑ ⊔Dᵢ ⊑ X`. So the
    // candidate sups are exactly the common closure-supersumers of the
    // disjuncts (`∩ᵢ subsumers(Dᵢ)`); each is a genuine entailment — added
    // directly, no tableau/wedge call (hence no per-pair-budget timeout risk).
    // See docs/classify-recovery-scope-2026-06-07.md.
    for ax in &internal.axioms {
        let owl_dl_core::ontology::Axiom::EquivalentClasses(ids) = ax else {
            continue;
        };
        // Identify the named class `C` (an Atomic operand) and a union
        // operand whose disjuncts are all atomic.
        let mut name: Option<usize> = None;
        let mut disjuncts: Option<Vec<usize>> = None;
        for cid in ids {
            match internal.concepts.get(*cid) {
                owl_dl_core::ir::ConceptExpr::Atomic(cls) => name = Some(cls.index() as usize),
                owl_dl_core::ir::ConceptExpr::Or(ds) => {
                    let atoms: Option<Vec<usize>> = ds
                        .iter()
                        .map(|d| match internal.concepts.get(*d) {
                            owl_dl_core::ir::ConceptExpr::Atomic(dc) => Some(dc.index() as usize),
                            _ => None,
                        })
                        .collect();
                    if let Some(a) = atoms {
                        disjuncts = Some(a);
                    }
                }
                _ => {}
            }
        }
        let (Some(c), Some(ds)) = (name, disjuncts) else {
            continue;
        };
        if c >= n || ds.is_empty() || unsatisfiable_idxs.contains(&c) {
            continue;
        }
        // Candidate sups = intersection of the disjuncts' closure-subsumers.
        let mut cand: Option<std::collections::HashSet<usize>> = None;
        for &d in &ds {
            let d_id =
                owl_dl_core::ClassId::new(u32::try_from(d).expect("class index fits in u32"));
            let subs: std::collections::HashSet<usize> = closure
                .subsumers_of(d_id)
                .into_iter()
                .map(|s| s.index() as usize)
                .filter(|&j| j < n)
                .collect();
            cand = Some(match cand {
                None => subs,
                Some(prev) => prev.intersection(&subs).copied().collect(),
            });
        }
        let c_id = owl_dl_core::ClassId::new(u32::try_from(c).expect("class index fits in u32"));
        for x in cand.unwrap_or_default() {
            if x == c || unsatisfiable_idxs.contains(&x) {
                continue;
            }
            let x_id =
                owl_dl_core::ClassId::new(u32::try_from(x).expect("class index fits in u32"));
            // Skip subsumptions already on `C`'s closure ray (the entailment
            // matrix seeds those) or already recorded.
            if closure.contains(c_id, x_id) || direct_supers[c].contains(&x) {
                continue;
            }
            stats.defined_sub_sweep_recovered += 1;
            direct_supers[c].push(x);
            direct_children[x].push(c);
        }
    }

    // Build the full entailment matrix. Three sources contribute:
    //
    // 1. **Closure seed.** Every saturation-derived subsumption is
    //    sound, so we copy `closure` straight in. This catches
    //    *same-tier* equivalences (e.g., `EquivalentClasses(A, B)`
    //    where both ranks tie at 2) that the top-down walk above
    //    misses by construction — the walk only looks at *placed*
    //    classes, and two same-tier classes don't see each other.
    // 2. **Reflexive + unsat-row trivial fill.**
    // 3. **Tableau-derived direct supers** from the top-down walk,
    //    transitively closed via BFS over `direct_supers`.
    let mut entailed: Vec<Vec<bool>> = vec![vec![false; n]; n];
    for i in 0..n {
        entailed[i][i] = true;
        if unsatisfiable_idxs.contains(&i) {
            entailed[i].iter_mut().take(n).for_each(|v| *v = true);
            continue;
        }
        // Closure seed.
        let i_id = owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        for sup in closure.subsumers_of(i_id) {
            let j = sup.index() as usize;
            if j < n {
                entailed[i][j] = true;
            }
        }
        // BFS over direct_supers starting from `i` to pick up the
        // tableau-derived transitive closure. Tracked via a
        // `visited` set so we descend through every reached node
        // exactly once — `entailed[i][j]` may already be true from
        // the closure seed above, but we still need to follow
        // `direct_supers[j]` to catch tableau-only ancestors of `j`
        // that aren't on `i`'s closure ray.
        let mut visited = vec![false; n];
        let mut frontier: Vec<usize> = direct_supers[i].clone();
        while let Some(j) = frontier.pop() {
            if visited[j] {
                continue;
            }
            visited[j] = true;
            entailed[i][j] = true;
            for &k in &direct_supers[j] {
                if !visited[k] {
                    frontier.push(k);
                }
            }
        }
    }

    let _ = top_level; // currently informational only

    // Phase 2a recon: pull AtomicU64 snapshot timers and derive
    // tier_walk_wall_ms = total - (label_cache + snapshot_build + replay).
    stats.snapshot_cache_build_wall_ms = prepared.snapshot_cache_build_wall_ms();
    stats.snapshot_replay_wall_ms = prepared.snapshot_cache_replay_wall_ms();
    // Phase 3a recon: per-class BackPropRisk diagnostic counts. Pure
    // instrumentation; does not affect the snapshot cache gate.
    stats.per_class_safe_count = prepared.per_class_safe_count();
    stats.per_class_unsafe_count = prepared.per_class_unsafe_count();
    let total_wall = u64::try_from(classify_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    stats.tier_walk_wall_ms = total_wall
        .saturating_sub(stats.label_cache_build_wall_ms)
        .saturating_sub(stats.snapshot_cache_build_wall_ms)
        .saturating_sub(stats.snapshot_replay_wall_ms);

    Ok(Classification {
        classes,
        index,
        entailed,
        unsatisfiable_idxs,
        stats,
    })
}

/// Walk the partial hierarchy top-down to find class `c`'s direct
/// super-classes among the already-placed classes. Free function so
/// rayon workers can invoke it in parallel within a closure-rank
/// tier (the tier's members don't appear in each other's frontier
/// — `top_level` + `direct_children` are snapshots from before the
/// tier started).
///
/// Returns the set of most-specific placed classes that subsume `c`.
/// Mutates `stats` in place (the caller treats it as a delta and
/// merges into a global accumulator).
#[allow(clippy::too_many_arguments)]
fn find_direct_parents_top_down(
    c: usize,
    closure: &owl_dl_saturation::Subsumers,
    prepared: &PreparedOntology,
    direct_supers: &[Vec<usize>],
    direct_children: &[Vec<usize>],
    top_level: &[usize],
    per_pair_timeout: Option<std::time::Duration>,
    global_deadline: Option<Instant>,
    label_cache: &[crate::LabelOracle],
    stats: &mut ClassificationStats,
) -> Result<Vec<usize>, ReasonError> {
    let c_id = owl_dl_core::ClassId::new(u32::try_from(c).expect("class index fits in u32"));
    let n = direct_supers.len();
    let mut frontier: Vec<usize> = top_level.to_vec();
    // Phase 6: dedupe the walk. Dense subsumer lattices (e.g. GALEN's
    // 2748-class hierarchy) reach the same candidate via many parent
    // paths; without `visited`, each duplicate redoes closure.contains
    // + pushes its children + appends to accepted. The de-dup at the
    // bottom (`accepted.collect::<HashSet>()`) covered correctness but
    // not the redundant walk-time work, which the Phase 5 T3b probe
    // localized as 97% of GALEN classify wall
    // (`docs/phase5-downstream-probe.md`).
    let mut visited: Vec<bool> = vec![false; n];
    let mut accepted: Vec<usize> = Vec::new();
    while let Some(d) = frontier.pop() {
        if d == c || visited[d] {
            continue;
        }
        visited[d] = true;
        // Global deadline short-circuit: if the budget has expired, flag
        // this candidate undecided rather than paying for a probe that would
        // instant-timeout anyway. Mark visited first so duplicate entries in
        // the frontier don't double-count.
        if global_deadline.is_some_and(|gd| Instant::now() >= gd) {
            stats.timed_out_pairs += 1;
            stats.timed_out_pair_ids.push((
                u32::try_from(c).expect("class index fits in u32"),
                u32::try_from(d).expect("class index fits in u32"),
            ));
            continue;
        }
        let d_id = owl_dl_core::ClassId::new(u32::try_from(d).expect("class index fits in u32"));
        let subsumed = if closure.contains(c_id, d_id) {
            stats.saturation_subsumption_hits += 1;
            true
        } else {
            // Phase 7: per-class label heuristic — check the cache
            // before paying for `subsumes_via_tableau`.
            match label_cache.get(c) {
                Some(crate::LabelOracle::Sat(labels)) => {
                    if labels.contains(&d_id) {
                        // D ∈ C's labels: might be coincidence-of-model;
                        // verify via the existing per-pair path.
                        stats.label_cache_pass_through += 1;
                        subsumes_via_tableau(
                            prepared,
                            c_id,
                            d_id,
                            per_pair_timeout,
                            global_deadline,
                            true,
                            stats,
                        )?
                        .unwrap_or_default()
                    } else {
                        // D ∉ C's labels: this completion graph is a
                        // counterexample model. Sound non-subsumption.
                        stats.label_cache_pruned += 1;
                        false
                    }
                }
                Some(crate::LabelOracle::Unsat) => {
                    // C is unsatisfiable: vacuously subsumes every D.
                    true
                }
                Some(crate::LabelOracle::NoVerdict) | None => {
                    // Cache missing — fall through to per-pair.
                    stats.label_cache_misses += 1;
                    subsumes_via_tableau(
                        prepared,
                        c_id,
                        d_id,
                        per_pair_timeout,
                        global_deadline,
                        true,
                        stats,
                    )?
                    .unwrap_or_default()
                }
            }
        };
        if !subsumed {
            continue;
        }
        for &k in &direct_children[d] {
            if !visited[k] {
                frontier.push(k);
            }
        }
        accepted.push(d);
    }
    // Prune `accepted` to the most-specific entries: drop any
    // candidate that has a strict descendant also in `accepted`.
    // `visited` guarantees `accepted` has no duplicates, so we skip
    // the final HashSet-dedup ceremony that the pre-Phase-6 path used.
    let direct_parents: Vec<usize> = accepted
        .iter()
        .copied()
        .filter(|&d| {
            !accepted.iter().any(|&e| {
                e != d
                    && (closure.contains(
                        owl_dl_core::ClassId::new(
                            u32::try_from(e).expect("class index fits in u32"),
                        ),
                        owl_dl_core::ClassId::new(
                            u32::try_from(d).expect("class index fits in u32"),
                        ),
                    ) || direct_supers[e].contains(&d))
            })
        })
        .collect();
    Ok(direct_parents)
}

/// Helper: ask the tableau whether `sub ⊑ sup`. Counts the call in
/// `stats`, honours `per_pair_timeout`, returns:
/// - `Ok(Some(true))` — subsumption holds
/// - `Ok(Some(false))` — refuted (sat verdict on `sub ⊓ ¬sup`)
/// - `Ok(None)` — timed out (counted as `timed_out_pairs`)
fn subsumes_via_tableau(
    prepared: &PreparedOntology,
    sub: owl_dl_core::ClassId,
    sup: owl_dl_core::ClassId,
    per_pair_timeout: Option<std::time::Duration>,
    global_deadline: Option<Instant>,
    trust_sat: bool,
    stats: &mut ClassificationStats,
) -> Result<Option<bool>, ReasonError> {
    // Phase 1b snapshot-replay shortcut. When RUSTDL_SNAPSHOT_CAPTURE
    // is ON AND the ontology is BackPropRisk::Safe, consult the per-class
    // snapshot cache before the wedge. A snapshot for `sub` is built on
    // first query and reused across all subsequent (sub, *) probes; the
    // replay re-runs `decide` on the seeded engine state with `¬sup`
    // injected, returning Subsumed/NotSubsumed/BackPropAborted/Stalled.
    //
    // Phase 1b ships full-re-run (no rule-firing skip) — correctness
    // equivalent to the wedge; perf wins wait for Phase 1b.5 lazy
    // expansion. Sound by spec §4.2 Inv-1 + the runtime sentinel at
    // §4.3. Flag-OFF or Unsafe-ontology: try_replay returns None and
    // execution falls through to the wedge unchanged.
    if crate::snapshot_capture_enabled() {
        // Snapshot replay uses the wedge's fresh_q injection pattern
        // (root-scoped ¬sup: `fresh_q ⊓ sup → ⊥`). Caller passes just
        // (sub, sup); the SnapshotCache internals build the q-gated
        // clause. T6 recon: the global `sup(x) → ⊥` encoding triggered
        // 25,333 FPs on GALEN because successor labels matched arbitrary
        // sups. Defined-sup support is Phase 1b.5 / Phase 1c work.
        if let Some(verdict) = prepared.snapshot_replay(sub, sup) {
            stats.snapshot_replay_used += 1;
            match verdict {
                owl_dl_tableau::ReplayVerdict::Subsumed => {
                    stats.snapshot_replay_subsumed += 1;
                    return Ok(Some(true));
                }
                owl_dl_tableau::ReplayVerdict::NotSubsumed
                    if trust_sat && crate::hyper_trust_sat_enabled() =>
                {
                    stats.snapshot_replay_not_subsumed += 1;
                    return Ok(Some(false));
                }
                owl_dl_tableau::ReplayVerdict::BackPropAborted => {
                    stats.snapshot_replay_aborts += 1;
                    // fall through to wedge
                }
                _ => {
                    // NotSubsumed without trust_sat, or Stalled — fall through.
                    stats.snapshot_cache_falls_through += 1;
                }
            }
        } else {
            // Flag ON but cache returned None: Unsafe ontology OR snapshot
            // build failed for `sub` (Unsat/Stalled on `sub` alone).
            stats.snapshot_cache_falls_through += 1;
        }
    }

    // H4 sound-accelerator wedge: try the hyper engine first. An
    // `Unsat` (subsumption-holds) verdict is sound for any ontology
    // (see docs/hypertableau-h4-scoping.md §0), so trust it and skip
    // the (slow, sometimes timing-out) tableau. HF5 extends this with
    // `Sat`→not-subsumed under `RUSTDL_HYPERTABLEAU_TRUST_SAT` — sound
    // only when the engine is complete on the workload (corpus-verified
    // both-direction Konclude agreement; off-corpus risky). A non-proof
    // / `Stalled` falls through to the tableau. No-op when the wedge
    // is off.
    //
    // The `trust_sat` parameter is a per-call override of the global
    // `RUSTDL_HYPERTABLEAU_TRUST_SAT` flag. The main top-down walk
    // passes `true` (fast classify of the regular hierarchy). The
    // defined-sup sweep passes `false`: the wedge is incomplete on the
    // functional-role + ≥n-with-disjointness patterns that defined
    // classes (`EquivalentClasses(Name, ComplexExpr)`) exercise, so
    // its `NotSubsumed` would silently drop real entailments (109
    // MISSED on GALEN, 27 on notgalen all traced to this).
    // Compute effective deadline for the wedge: honours both the
    // per-pair timeout and any global wall-clock deadline.
    let hyper_deadline = effective_deadline(global_deadline, per_pair_timeout);
    let wedge_start = Instant::now();
    let verdict = prepared.hyper_decide(sub, sup, hyper_deadline);
    let wedge_elapsed_ms = u64::try_from(wedge_start.elapsed().as_millis()).unwrap_or(u64::MAX);
    *stats.pairs_per_sub.entry(sub.index()).or_insert(0) += 1;
    let bucket = match wedge_elapsed_ms {
        0 => 0,
        1 => 1,
        2..=4 => 2,
        5..=9 => 3,
        10..=19 => 4,
        20..=49 => 5,
        50..=99 => 6,
        100..=999 => 7,
        _ => 8,
    };
    stats.wedge_cost_histogram_ms[bucket] += 1;
    let mut was_fast_refuted = false;
    match verdict {
        crate::HyperVerdict::Subsumed => {
            stats.hyper_proven_pairs += 1;
            return Ok(Some(true));
        }
        crate::HyperVerdict::NotSubsumed if trust_sat && crate::hyper_trust_sat_enabled() => {
            // Phase 1 selective verification: a wedge `NotSubsumed`
            // returned in < `RUSTDL_HYPER_TRUST_SAT_MIN_MS` is more
            // likely "didn't try hard enough" than a genuine satisfying
            // model. Fall through to the tableau in that case; trust
            // the verdict only when the wedge took at least the
            // threshold. Setting the env var to 0 restores pre-Phase-1
            // behaviour (trust every NotSubsumed verdict).
            let threshold = crate::hyper_trust_sat_min_ms();
            if threshold == 0 || wedge_elapsed_ms >= threshold {
                stats.hyper_refuted_pairs += 1;
                return Ok(Some(false));
            }
            stats.hyper_refuted_fast_pairs += 1;
            was_fast_refuted = true;
            // fall through to the tableau probe below; if the tableau
            // returns Subsumed, bump hyper_refuted_fast_flipped_pairs.
        }
        _ => {}
    }
    let build = move |pool: &mut ConceptPool| {
        let sub_concept = pool.atomic(sub);
        let super_concept = pool.atomic(sup);
        let not_super = pool.not(super_concept);
        pool.and(vec![sub_concept, not_super])
    };
    // Use effective_deadline so that a global wall-clock budget bounds the
    // tableau probe even when per_pair_timeout is None (global-only mode).
    match effective_deadline(global_deadline, per_pair_timeout) {
        None => {
            let sat = prepared.decide(build)?;
            stats.tableau_subsumption_calls += 1;
            let subsumed = !sat;
            if was_fast_refuted && subsumed {
                stats.hyper_refuted_fast_flipped_pairs += 1;
            }
            Ok(Some(subsumed))
        }
        Some(deadline) => {
            // Robustness: a `ReasonError::NoVerdict` (tableau internal
            // cap, e.g. on large workloads like SIO) is treated as a
            // sound timeout — the pair defaults to "not subsumed"
            // (sound under-approximation), counted in `timed_out_pairs`.
            // Crashing classify on a single oversized pair is worse
            // than under-reporting the subsumption.
            match prepared.decide_with_deadline(deadline, build) {
                Ok(Some(sat)) => {
                    stats.tableau_subsumption_calls += 1;
                    let subsumed = !sat;
                    if was_fast_refuted && subsumed {
                        stats.hyper_refuted_fast_flipped_pairs += 1;
                    }
                    Ok(Some(subsumed))
                }
                Ok(None) | Err(crate::ReasonError::NoVerdict) => {
                    stats.timed_out_pairs += 1;
                    stats.timed_out_pair_ids.push((sub.index(), sup.index()));
                    Ok(None)
                }
                Err(other) => Err(other),
            }
        }
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

    /// Diagnostic probe (wine residual-31, cluster A): why does classify miss
    /// `food#Fruit ⊑ food#EdibleThing` when `is_subclass_of` proves it in 0.01s?
    /// Compares the fresh tableau (`is_subclass_of_internal`) against the
    /// classify-path `PreparedOntology::decide` (the ABox-seeded snapshot) on the
    /// exact query `Fruit ⊓ ¬EdibleThing`, unbounded and at the 200ms classify
    /// budget. Settles timeout-vs-wrong-verdict. See
    /// `docs/wine-residual-31-diagnosis-2026-06-07.md`. `#[ignore]`d (needs the
    /// gitignored wine fixture); run with `-- --ignored --nocapture`.
    #[test]
    #[ignore = "needs ontologies/real/wine.ofn; diagnostic for the Fruit cluster-A classify miss"]
    fn wine_fruit_prepared_vs_fresh_probe() {
        use horned_owl::io::ofn::reader::read as read_ofn;
        let path = std::path::Path::new("../../ontologies/real/wine.ofn");
        if !path.exists() {
            eprintln!("SKIP: missing {}", path.display());
            return;
        }
        let f = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/food#Fruit";
        let e = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/food#EdibleThing";
        let src = std::fs::read_to_string(path).expect("read wine");
        let parse_onto = || {
            let mut r = Cursor::new(src.clone());
            let (o, _): (SetOntology<RcStr>, _) =
                read_ofn(&mut r, ParserConfiguration::default()).expect("parse wine");
            o
        };

        // Fresh path (what subclass / explain use).
        let fresh = crate::is_subclass_of(&parse_onto(), f, e).expect("fresh is_subclass_of");
        eprintln!("FRESH is_subclass_of(Fruit, EdibleThing) = {fresh}");

        // Classify-path: PreparedOntology::decide on `Fruit ⊓ ¬EdibleThing`.
        let internal = owl_dl_core::convert::convert_ontology(&parse_onto()).expect("convert");
        let cons = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/food#ConsumableThing";
        let fid = internal.vocabulary.class_id(f).expect("Fruit id");
        let eid = internal.vocabulary.class_id(e).expect("EdibleThing id");
        let cid = internal
            .vocabulary
            .class_id(cons)
            .expect("ConsumableThing id");
        // EL closure witness?
        let closure = owl_dl_saturation::saturate(&internal);
        eprintln!(
            "CLOSURE.contains(Fruit, EdibleThing) = {}",
            closure.contains(fid, eid)
        );
        let prepared = PreparedOntology::from_internal(internal).expect("prepare");
        // The classify walk tries the WEDGE first (hyper_decide), only falling
        // to the tableau on a non-proof. Measure both deadlines.
        let tw = std::time::Instant::now();
        let wedge_unbounded = prepared.hyper_decide(fid, eid, None);
        eprintln!(
            "WEDGE prepared.hyper_decide(None) = {wedge_unbounded:?} in {} ms",
            tw.elapsed().as_millis()
        );
        let tw2 = std::time::Instant::now();
        let wdl = std::time::Instant::now() + std::time::Duration::from_millis(200);
        let wedge_200 = prepared.hyper_decide(fid, eid, Some(wdl));
        eprintln!(
            "WEDGE prepared.hyper_decide(Fruit,EdibleThing,200ms) = {wedge_200:?} in {} ms",
            tw2.elapsed().as_millis()
        );
        // The descent GATE: EdibleThing ⊑ ConsumableThing (top-level), so the
        // walk reaches EdibleThing only by first accepting ConsumableThing.
        let tc = std::time::Instant::now();
        let cdl = std::time::Instant::now() + std::time::Duration::from_millis(200);
        let wedge_cons = prepared.hyper_decide(fid, cid, Some(cdl));
        eprintln!(
            "WEDGE prepared.hyper_decide(Fruit,ConsumableThing,200ms) = {wedge_cons:?} in {} ms  [descent gate]",
            tc.elapsed().as_millis()
        );
        // SEPARATE finding (NOT cluster A's cause — the WEDGE proves Fruit ⊑
        // EdibleThing in 0 ms above, so the tableau is never reached for this
        // pair in classify). The ABox/nominal-seeded `prepared.decide` is
        // pathologically slow / non-terminating: a 5 s deadline times out, vs the
        // fresh path's 0.01 s (unbounded does not return in 150 s — do NOT call
        // it). This matters for the B/C/D pairs (whose wedge does NOT prove them
        // → tableau fallback). Cluster A's actual cause is the defined-sup sweep
        // coverage gap; see docs/classify-recovery-scope-2026-06-07.md.
        let build = |pool: &mut owl_dl_core::ir::ConceptPool| {
            let fc = pool.atomic(fid);
            let ec = pool.atomic(eid);
            let nec = pool.not(ec);
            pool.and(vec![fc, nec])
        };
        let t0 = std::time::Instant::now();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let bounded = prepared
            .decide_with_deadline(deadline, build)
            .expect("prepared.decide_with_deadline");
        let ms = t0.elapsed().as_millis();
        eprintln!("PREPARED.decide_with_deadline(5s) = {bounded:?} (None=timeout) in {ms} ms");

        eprintln!(
            "VERDICT: {}",
            match (fresh, bounded) {
                (true, None) =>
                    "wedge proves it in 0ms (cluster A = defined-sup-sweep gap); \
                     SEPARATELY the ABox-seeded prepared.decide tableau times out even at 5s \
                     (non-termination, affects B/C/D)",
                (true, Some(false)) =>
                    "prepared agrees (subsumed) within 5s ⇒ the miss is only the 200ms budget",
                (true, Some(true)) =>
                    "prepared returns WRONG Sat ⇒ PreparedOntology completeness bug",
                _ => "fresh disagrees — re-examine",
            }
        );
        // Pin the established finding: fresh proves it; prepared cannot in 5s.
        assert!(fresh, "fresh is_subclass_of must prove Fruit ⊑ EdibleThing");
        assert_eq!(
            bounded, None,
            "regression: prepared.decide now finishes in 5s — the ABox-seeding \
             pathology may be fixed; update docs/wine-residual-31-diagnosis"
        );
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    /// Test-only RAII env-var guard. Sets each `(key, val)` on
    /// construction and restores the prior values (or removes them) on
    /// drop. Used to pin the orchestrator flags a test was written
    /// against — notably `RUSTDL_HORN_SHORTCIRCUIT` (Phase 2b) and
    /// `RUSTDL_SNAPSHOT_CAPTURE` (Phase 1c), both of which flipped to
    /// default-ON after several of these tests were written and otherwise
    /// bypass the per-pair loop / `pure_el_mode` path under test.
    ///
    /// The orchestrator reads these vars from the process-global
    /// environment, so the guard also holds a module-wide mutex for its
    /// whole lifetime: any test that pins a flag — or classifies a
    /// Horn-but-non-EL ontology whose verdict depends on one — must build
    /// exactly one guard and hold it for the whole test body, so such
    /// tests never run concurrently and never observe each other's
    /// transient values. One guard per test (not nested): each guard
    /// takes the lock once. The mutex is poison-tolerant so a panicking
    /// test doesn't cascade-fail the rest.
    #[allow(unsafe_code)]
    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        #[allow(unsafe_code)]
        fn set(vars: &[(&'static str, &str)]) -> Self {
            let lock = crate::test_env_lock();
            let mut prev = Vec::with_capacity(vars.len());
            for &(k, v) in vars {
                prev.push((k, std::env::var_os(k)));
                unsafe { std::env::set_var(k, v) };
            }
            Self { _lock: lock, prev }
        }
    }

    impl Drop for EnvGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            for (k, v) in &self.prev {
                match v {
                    Some(val) => unsafe { std::env::set_var(k, val) },
                    None => unsafe { std::env::remove_var(k) },
                }
            }
        }
    }

    #[test]
    fn classify_picks_up_explicit_chain() {
        // A ⊑ B ⊑ C — classification should yield both direct edges.
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
        let h = classify(&onto).expect("classification");
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        assert!(h.is_subclass(&iri("A"), &iri("B")));
        assert!(h.is_subclass(&iri("B"), &iri("C")));
        assert!(h.is_subclass(&iri("A"), &iri("C")));
        assert!(!h.is_subclass(&iri("C"), &iri("A")));
        let direct = h.direct_subsumers(&iri("A"));
        assert_eq!(direct, vec![iri("B")]);
    }

    /// Regression for the defined-SUB sweep (cluster A; wine residual-31,
    /// 2026-06-07). A union/covering-defined sub `C ≡ A ⊔ B` ⊑ a PRIMITIVE sup
    /// `X` (every disjunct `⊑ X`) is missed by both the tier-walk (the covering
    /// subsumption isn't in the EL closure) and the defined-sup sweep (`X` is
    /// primitive). The companion defined-SUB sweep recovers it soundly by
    /// construction. Mirrors wine's `Fruit ≡ NonSweetFruit ⊔ SweetFruit ⊑
    /// EdibleThing`. See docs/classify-recovery-scope-2026-06-07.md.
    #[test]
    fn defined_union_sub_under_primitive_sup() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:X))\n\
    EquivalentClasses(:C ObjectUnionOf(:A :B))\n\
    SubClassOf(:A :X)\n\
    SubClassOf(:B :X)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        // C ≡ A ⊔ B, A ⊑ X, B ⊑ X ⟹ C ⊑ X (every model element of C is in
        // A or B, both ⊑ X). `X` is primitive, so only the defined-SUB sweep
        // recovers this.
        assert!(
            h.is_subclass(&iri("C"), &iri("X")),
            "defined-SUB sweep must place C ⊑ X"
        );
        // Disjuncts and the union are mutually subsumed by X but not vice versa.
        assert!(h.is_subclass(&iri("A"), &iri("X")));
        assert!(!h.is_subclass(&iri("X"), &iri("C")));
    }

    #[test]
    fn classify_groups_equivalents() {
        // EquivalentClasses(A, B) — they should appear as each
        // other's equivalents.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    EquivalentClasses(:A :B)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let iri_a = "http://rustdl.test/A".to_string();
        let iri_b = "http://rustdl.test/B".to_string();
        let equiv_a: Vec<String> = h
            .equivalent_classes(&iri_a)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert!(equiv_a.contains(&iri_a));
        assert!(equiv_a.contains(&iri_b));
    }

    #[test]
    fn classify_flags_unsatisfiable() {
        // Pin the per-pair path: the Horn-shortcircuit fast path
        // (default ON) routes this Horn input to the EL saturation
        // closure, which drops the ¬B clash and misses A ⊑ ⊥.
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        // A ⊑ B ⊓ ¬B — A is empty, equivalent to ⊥.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A ObjectIntersectionOf(:B ObjectComplementOf(:B)))\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        assert!(h.unsatisfiable_classes().contains(&"http://rustdl.test/A"));
    }

    #[test]
    fn classify_stats_show_saturation_carries_pure_el() {
        // Pure EL: A ⊑ B ⊑ C ⊑ D. Saturation should handle every
        // (non-reflexive, non-self) pairwise subsumption query
        // without dispatching to the tableau.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    SubClassOf(:C :D)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let stats = h.stats();
        // Pure EL ⇒ tableau is never invoked, saturation alone is
        // both sound and complete here.
        assert!(stats.pure_el_mode);
        assert_eq!(stats.tableau_subsumption_calls, 0);
        assert_eq!(stats.tableau_unsat_calls, 0);
        // Entailed pairs are A⊑B, A⊑C, A⊑D, B⊑C, B⊑D, C⊑D = 6.
        assert_eq!(stats.saturation_subsumption_hits, 6);
    }

    #[test]
    fn classify_with_timeout_matches_untimed_for_simple_input() {
        // A → B → C (pure EL) — even with a tiny timeout, all pairs
        // get answered by saturation (the closure path bypasses the
        // tableau entirely) so the timed and untimed runs agree.
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
        let baseline = classify(&onto).expect("baseline");
        let timed = super::classify_with_timeout(&onto, std::time::Duration::from_millis(50))
            .expect("timed classification");
        assert_eq!(baseline.stats().timed_out_pairs, 0);
        assert_eq!(timed.stats().timed_out_pairs, 0);
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        assert!(timed.is_subclass(&iri("A"), &iri("C")));
        assert_eq!(
            baseline.unsatisfiable_classes(),
            timed.unsatisfiable_classes()
        );
    }

    #[test]
    fn classify_drops_to_tableau_when_axioms_leave_el() {
        // This test exercises the drop-to-tableau path, which the Horn
        // shortcircuit (default ON) bypasses for Horn-but-non-EL inputs.
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        // The DisjointObjectProperties axiom is outside our EL
        // saturation fragment — classify should NOT take the pure-EL
        // fast path; it takes the hybrid path and still produces the
        // correct hierarchy. (Pre-2026-06-10 this asserted a tableau
        // call count > 0; the unsat-probe-via-label-cache optimization
        // now decides the trivially-satisfiable classes via the wedge
        // without a main-tableau call, so the documented intent is the
        // fragment routing + verdict correctness, not the call count.)
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        assert!(
            !h.stats().pure_el_mode,
            "non-EL fragment must not take the pure-EL path"
        );
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        assert!(
            h.is_subclass(&iri("A"), &iri("B")),
            "A ⊑ B in the hybrid path"
        );
        assert!(!h.is_subclass(&iri("B"), &iri("A")), "B ⋢ A");
    }

    #[test]
    fn stats_carry_selective_verify_counters_by_default() {
        let s = ClassificationStats::default();
        assert_eq!(s.hyper_refuted_fast_pairs, 0);
        assert_eq!(s.hyper_refuted_fast_flipped_pairs, 0);
    }

    /// Helper for the top-down ↔ naive cross-check: compare the
    /// entailment matrix and unsat set under both classifiers. We
    /// don't compare `ClassificationStats` — the call-count breakdown
    /// is expected to differ by construction.
    fn assert_top_down_matches_naive(onto: &SetOntology<RcStr>) {
        // Compares the naive vs top-down *walk* strategies, both bypassed
        // by the Horn shortcircuit. Callers pin RUSTDL_HORN_SHORTCIRCUIT=0
        // and hold the EnvGuard lock; this helper stays lock-free so it
        // doesn't re-enter the (non-reentrant) mutex.
        let naive = classify_n2(onto).expect("naive classify");
        let td = classify_top_down(onto).expect("top-down classify");
        assert_eq!(
            naive.classes(),
            td.classes(),
            "class list disagrees: naive {:?} vs top-down {:?}",
            naive.classes(),
            td.classes(),
        );
        let unsat_naive: std::collections::BTreeSet<&str> =
            naive.unsatisfiable_classes().into_iter().collect();
        let unsat_td: std::collections::BTreeSet<&str> =
            td.unsatisfiable_classes().into_iter().collect();
        assert_eq!(unsat_naive, unsat_td, "unsat set disagrees");
        for sub in naive.classes() {
            for sup in naive.classes() {
                assert_eq!(
                    naive.is_subclass(sub, sup),
                    td.is_subclass(sub, sup),
                    "subsumption verdict diverges for {sub} ⊑ {sup}",
                );
            }
        }
    }

    #[test]
    fn classify_top_down_matches_naive_on_explicit_chain() {
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        // A ⊑ B ⊑ C: 3-class tower used in `classify_picks_up_
        // explicit_chain` — top-down should report the same matrix.
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
        assert_top_down_matches_naive(&onto);
    }

    #[test]
    fn classify_top_down_matches_naive_on_equivalent_classes() {
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        // EquivalentClasses(A, B) — equivalence pairs are a subtle
        // case for the top-down hierarchy walk.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    EquivalentClasses(:A :B)\n\
)\n"
        ));
        assert_top_down_matches_naive(&onto);
    }

    #[test]
    fn classify_top_down_matches_naive_on_unsatisfiable_class() {
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        // A ⊑ B ⊓ ¬B — A is unsat. Top-down's unsat-row trivial
        // fill should match naive.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A ObjectIntersectionOf(:B ObjectComplementOf(:B)))\n\
)\n"
        ));
        assert_top_down_matches_naive(&onto);
    }

    #[test]
    fn classify_top_down_matches_naive_on_hybrid_fragment() {
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        // The DisjointObjectProperties axiom forces hybrid mode.
        // Top-down's hybrid path must agree with naive.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        assert_top_down_matches_naive(&onto);
    }

    #[test]
    fn classify_top_down_handles_equivalent_classes_in_hybrid_mode() {
        // Regression test: the top-down walk only inspects *placed*
        // classes. When A and B sit at the same closure-rank
        // (`EquivalentClasses(A, B)` ⇒ both have 2 subsumers), the
        // walk for whichever class is processed first sees an empty
        // frontier and misses the equivalence in `direct_supers`.
        // The closure-seed step in the entailment-matrix builder
        // restores it. The pure-EL counterpart goes through
        // `classify_pure_el` and was never affected; we force the
        // hybrid path here with a `DisjointObjectProperties` axiom.
        // Horn shortcircuit (default ON) would route this Horn input to
        // the saturation fast path (`pure_el_mode`), so pin it off.
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    EquivalentClasses(:A :B)\n\
)\n"
        ));
        assert_top_down_matches_naive(&onto);
        let h = classify_top_down(&onto).expect("td classify");
        assert!(
            !h.stats().pure_el_mode,
            "expected hybrid mode for this fixture"
        );
        let iri_a = "http://rustdl.test/A";
        let iri_b = "http://rustdl.test/B";
        assert!(h.is_subclass(iri_a, iri_b), "A ⊑ B should hold");
        assert!(h.is_subclass(iri_b, iri_a), "B ⊑ A should hold");
    }

    #[test]
    fn classify_top_down_issues_fewer_tableau_calls_than_naive() {
        // Constructed shape: 6 classes A..F with two told subsumptions
        // (A ⊑ B, C ⊑ D), plus DisjointObjectProperties forcing the
        // hybrid path. With saturation handling the told edges, the
        // naive path still tableau-tests every remaining pair (6×5 =
        // 30 pairs, minus closure hits and unsat-row fills). The
        // top-down path walks the partial hierarchy and only probes
        // candidates encountered during descent — should issue
        // strictly fewer subsumption calls.
        //
        // This is a regression-test against accidental degradation of
        // the top-down algorithm into "test every pair anyway."
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(Class(:E))\n\
    Declaration(Class(:F))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:C :D)\n\
)\n"
        ));
        // Pin the walk path: the Horn shortcircuit (default ON) bypasses
        // both classify_n2 and classify_top_down, zeroing tableau calls.
        let _env = EnvGuard::set(&[("RUSTDL_HORN_SHORTCIRCUIT", "0")]);
        let naive = classify_n2(&onto).expect("naive");
        let td = classify_top_down(&onto).expect("top-down");
        let naive_calls = naive.stats().tableau_subsumption_calls;
        let td_calls = td.stats().tableau_subsumption_calls;
        assert!(
            td_calls < naive_calls,
            "top-down should issue fewer tableau subsumption calls than naive — \
             naive={naive_calls} top-down={td_calls}",
        );
        // Sanity: outputs still match.
        assert_top_down_matches_naive(&onto);
    }

    /// Saturation-only is a sound under-approximation: every
    /// subsumption it reports must hold in the full hierarchy.
    /// A pure-EL chain is the easy case — both classifiers agree
    /// exactly because no tableau reasoning is needed.
    #[test]
    fn classify_saturation_only_matches_full_on_pure_el_chain() {
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
        let full = classify(&onto).expect("full classify");
        let sat = classify_saturation_only(&onto).expect("saturation-only");
        assert_eq!(full.classes(), sat.classes());
        for sub in full.classes() {
            for sup in full.classes() {
                let full_v = full.is_subclass(sub, sup);
                let sat_v = sat.is_subclass(sub, sup);
                if sat_v {
                    assert!(
                        full_v,
                        "saturation-only reported {sub} ⊑ {sup} but full did not — soundness violated",
                    );
                }
                assert_eq!(
                    full_v, sat_v,
                    "on a pure-EL chain both classifiers must agree exactly — {sub} ⊑ {sup}: full={full_v} sat={sat_v}",
                );
            }
        }
        assert!(sat.stats().pure_el_mode);
        assert_eq!(sat.stats().tableau_subsumption_calls, 0);
        assert_eq!(sat.stats().tableau_unsat_calls, 0);
    }

    /// With `RUSTDL_HYPER_TRUST_SAT_MIN_MS=100000` (~100 s, far above any
    /// realistic wedge call), every wedge `NotSubsumed` should be
    /// distrusted and the tableau should be asked. Verified via stats:
    /// `hyper_refuted_fast_pairs > 0` proves the new code path was taken.
    ///
    /// The ontology includes an isolated class D (no `SubClassOf` axioms
    /// linking it to A/B/C). The top-down walk places B and C first,
    /// then when it processes D it cannot find D⊑B or D⊑C in the
    /// saturation closure, so it calls `subsumes_via_tableau` for those
    /// pairs — both produce a wedge `NotSubsumed`. With the threshold
    /// set to 100 000 ms every such verdict is fast-refuted, exercising
    /// the new code path.
    ///
    /// SAFETY: env-var mutation; tests in this module that mutate
    /// `RUSTDL_HYPER_TRUST_SAT_MIN_MS` must run with --test-threads=1.
    /// Also disables `RUSTDL_LABEL_HEURISTIC` so the per-class label
    /// cache (Phase 7) doesn't prune the D⊑B/D⊑C non-subsumptions
    /// before they reach the wedge — the cache would soundly catch
    /// them, but that bypasses the selective-verify path under test.
    #[test]
    #[allow(unsafe_code)]
    fn selective_verify_triggers_when_threshold_high() {
        // The wedge per-pair path must be reached: disable the snapshot
        // cache (Phase 1c) and Horn shortcircuit (Phase 2b), both
        // default-ON and both intercepting these pairs before the wedge.
        let _env = EnvGuard::set(&[
            ("RUSTDL_SNAPSHOT_CAPTURE", "0"),
            ("RUSTDL_HORN_SHORTCIRCUIT", "0"),
        ]);
        let key = "RUSTDL_HYPER_TRUST_SAT_MIN_MS";
        let prev = std::env::var_os(key);
        unsafe { std::env::set_var(key, "100000") };
        let label_key = "RUSTDL_LABEL_HEURISTIC";
        let prev_label = std::env::var_os(label_key);
        unsafe { std::env::set_var(label_key, "0") };

        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let h = classify(&onto).expect("classify");
        let stats = h.stats();

        match prev {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        match prev_label {
            Some(v) => unsafe { std::env::set_var(label_key, v) },
            None => unsafe { std::env::remove_var(label_key) },
        }

        assert!(
            stats.hyper_refuted_fast_pairs > 0,
            "selective verify path never fired; stats = {stats:?}"
        );
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        assert!(h.is_subclass(&iri("A"), &iri("B")));
        assert!(h.is_subclass(&iri("A"), &iri("C")));
        assert!(h.is_subclass(&iri("B"), &iri("C")));
        assert!(!h.is_subclass(&iri("C"), &iri("A")));
        assert!(!h.is_subclass(&iri("D"), &iri("A")));
    }

    /// With `RUSTDL_HYPER_TRUST_SAT_MIN_MS=0`, selective verification is
    /// disabled — `hyper_refuted_fast_pairs` stays at zero.
    ///
    /// Uses the same 4-class ontology as the threshold-high test so that
    /// the wedge is exercised (D⊑C? and D⊑B? are probed), but the
    /// `NotSubsumed` verdicts are trusted immediately (threshold=0 means
    /// "always trust"), so the fast-refuted counter stays at zero.
    ///
    /// SAFETY: same env-var mutation as above; --test-threads=1.
    /// Also disables `RUSTDL_LABEL_HEURISTIC` for the same reason as
    /// the threshold-high test: the cache would prune D⊑B/D⊑C before
    /// they reach the wedge, bypassing the path under test.
    #[test]
    #[allow(unsafe_code)]
    fn selective_verify_disabled_when_threshold_zero() {
        // The wedge per-pair path must be reached: disable the snapshot
        // cache (Phase 1c) and Horn shortcircuit (Phase 2b), both
        // default-ON and both intercepting these pairs before the wedge.
        let _env = EnvGuard::set(&[
            ("RUSTDL_SNAPSHOT_CAPTURE", "0"),
            ("RUSTDL_HORN_SHORTCIRCUIT", "0"),
        ]);
        let key = "RUSTDL_HYPER_TRUST_SAT_MIN_MS";
        let prev = std::env::var_os(key);
        unsafe { std::env::set_var(key, "0") };
        let label_key = "RUSTDL_LABEL_HEURISTIC";
        let prev_label = std::env::var_os(label_key);
        unsafe { std::env::set_var(label_key, "0") };

        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let h = classify(&onto).expect("classify");
        let stats = h.stats();

        match prev {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        match prev_label {
            Some(v) => unsafe { std::env::set_var(label_key, v) },
            None => unsafe { std::env::remove_var(label_key) },
        }

        assert_eq!(
            stats.hyper_refuted_fast_pairs, 0,
            "selective verify fired despite threshold=0; stats = {stats:?}"
        );
        assert!(
            stats.hyper_refuted_pairs > 0,
            "wedge was never exercised — test ontology doesn't reach the trusted-NotSubsumed arm; stats = {stats:?}"
        );
    }

    /// Saturation-only on a hybrid input: every reported
    /// subsumption must be entailed by the full classifier, but
    /// some subsumptions may be missed (the under-approximation
    /// semantics). Pizza's `:Pizza` ⊑ `:Thing` chain is the easy
    /// affirmative case; the negative side is implicit in the
    /// "sound under-approximation" framing.
    #[test]
    fn classify_saturation_only_is_sound_subset_of_full_on_hybrid() {
        // DisjointObjectProperties forces hybrid mode.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let full = classify(&onto).expect("full classify");
        let sat = classify_saturation_only(&onto).expect("saturation-only");
        for sub in full.classes() {
            for sup in full.classes() {
                if sat.is_subclass(sub, sup) {
                    assert!(
                        full.is_subclass(sub, sup),
                        "saturation-only reported {sub} ⊑ {sup} but full did not — soundness violated",
                    );
                }
            }
        }
        // The reported `pure_el_mode` is True regardless of whether
        // the input is structurally pure-EL — it indicates the
        // classifier *behaved* as the pure-EL path.
        assert!(sat.stats().pure_el_mode);
        assert_eq!(sat.stats().tableau_subsumption_calls, 0);
        assert_eq!(sat.stats().tableau_unsat_calls, 0);
    }

    #[test]
    fn analyze_fragment_returns_pure_el_on_minimal_el_ontology() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        assert_eq!(analyze_fragment(&internal), FragmentClassification::PureEl);
    }

    #[test]
    fn analyze_fragment_returns_out_of_fragment_on_disjunction() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A ObjectUnionOf(:B :C))\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        assert_eq!(
            analyze_fragment(&internal),
            FragmentClassification::OutOfFragment
        );
    }

    #[test]
    fn analyze_fragment_returns_out_of_fragment_on_inverse_role() {
        // InverseObjectProperties — clearly outside EL+. Phase 4b
        // shipped before Horn detection landed; the test name carries
        // that history. Phase 4c re-targets the assertion to accept
        // either Horn or OutOfFragment (depending on the clausifier's
        // behaviour on this minimal shape) — the test's purpose is to
        // confirm we don't regress to `PureEl` on a non-EL input.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:r_inv))\n\
    InverseObjectProperties(:r :r_inv)\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let result = analyze_fragment(&internal);
        assert_ne!(
            result,
            FragmentClassification::PureEl,
            "InverseObjectProperties is non-EL — must not classify as PureEl",
        );
    }

    // ── Phase D10: saturator_complete_fragment gate (the sound
    // Horn-shortcircuit trigger). NEGATIVES carry the soundness weight: a
    // construct the saturator can't fully reason over must NOT pass, or the
    // shortcircuit silently misses entailments and reports complete.

    fn internal_of(body: &str) -> InternalOntology {
        let onto = parse(&format!(
            "{HEADER}Ontology(<http://rustdl.test/t>\n{body}\n)\n"
        ));
        owl_dl_core::convert::convert_ontology(&onto).expect("convert")
    }

    #[test]
    fn saturator_fragment_accepts_el_plus_functional() {
        // EL concepts (∃, ⊓) + a Functional role characteristic — the
        // GALEN/notgalen shape. Must stay on the fast path.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    TransitiveObjectProperty(:r)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :A)))\n",
        );
        assert!(
            saturator_complete_fragment(&i),
            "EL + functional/transitive must be in the saturator fragment"
        );
    }

    #[test]
    fn saturator_fragment_rejects_forall() {
        // The proven silent-miss shape: ∀ + disjointness is clausal-Horn but
        // the saturator has no ∀-rule. Must FALL BACK (predicate false).
        let i = internal_of(
            "    Declaration(Class(:C))\n\
    Declaration(Class(:K3))\n\
    Declaration(Class(:K1020))\n\
    Declaration(ObjectProperty(:p))\n\
    SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:p :K3) ObjectAllValuesFrom(:p :K1020)))\n\
    DisjointClasses(:K3 :K1020)\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "∀ (ObjectAllValuesFrom) must drop out of the saturator fragment"
        );
    }

    #[test]
    fn saturator_fragment_rejects_max_cardinality() {
        // ≤n is only handled in the narrow unqualified+functional path — not
        // a general subsumption rule. Conservatively reject (the advisor's
        // 'you suspect ≤n' — pinned).
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectMaxCardinality(1 :r :B))\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "≤n cardinality must drop out of the saturator fragment"
        );
    }

    #[test]
    fn saturator_fragment_rejects_disjoint_classes() {
        // Conservative exclusion: disjoint×functional-merge is an unproven
        // interaction, so DisjointClasses falls back (pure-EL+disjoint still
        // takes the separate is_pure_el arm).
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    DisjointClasses(:A :B)\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "DisjointClasses is excluded from the saturator fragment (conservative)"
        );
    }

    #[test]
    fn analyze_fragment_returns_horn_on_inverse_role_subclass() {
        // Non-EL (inverse role in the SubClassOf RHS) but Horn-
        // shaped: a single sub-implies-single-head subsumption. The
        // clausifier should emit Horn clauses with no deferred
        // axioms, putting the ontology in the Horn fragment.
        //
        // If the clausifier happens to defer this exact shape the
        // result will land as OutOfFragment instead. The assertion
        // accepts either Horn-or-OutOfFragment; the test still rules
        // out a spurious PureEl. The Horn-positive case is verified
        // empirically on the corpus check in step 5 of the Phase 4c
        // plan.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:r) :B))\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let result = analyze_fragment(&internal);
        assert_ne!(
            result,
            FragmentClassification::PureEl,
            "inverse role in RHS is non-EL — must not classify as PureEl",
        );
        assert!(
            matches!(
                result,
                FragmentClassification::Horn | FragmentClassification::OutOfFragment
            ),
            "expected Horn or OutOfFragment, got {result:?}",
        );
    }

    #[test]
    fn analyze_fragment_returns_out_of_fragment_on_disjunctive_axiom() {
        // ObjectUnionOf in SubClassOf RHS forces a disjunctive head
        // in the clausified form — stats.disjunctive > 0 ⇒
        // OutOfFragment. Distinct from the
        // `analyze_fragment_returns_out_of_fragment_on_disjunction`
        // test above (which exercises the same shape) — this one
        // documents the precise Phase 4c detection contract.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A ObjectUnionOf(:B :C))\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        assert_eq!(
            analyze_fragment(&internal),
            FragmentClassification::OutOfFragment,
        );
    }

    /// The `undecided_pairs()` set length must equal `timed_out_pairs`
    /// count — consistency invariant for the anytime calibration contract.
    /// Uses a tiny out-of-EL ontology (`∀`-axiom + existential) that
    /// falls through to the per-pair tableau path, with a 1 ms deadline
    /// so at least some pairs may time out in CI; even if none do, the
    /// invariant `len == count` must hold.
    #[test]
    fn undecided_pairs_reports_timed_out_subsumptions() {
        let src = "Prefix(:=<http://t/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(\n\
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(ObjectProperty(:r))\n\
  SubClassOf(:A ObjectAllValuesFrom(:r :B))\n\
  SubClassOf(:A ObjectSomeValuesFrom(:r owl:Thing))\n)\n";
        let onto = parse(src);
        let h =
            classify_with_timeout(&onto, std::time::Duration::from_millis(1)).expect("classify");
        // The set length must equal the count stat (consistency), regardless of
        // whether this tiny ontology actually times out.
        assert_eq!(h.undecided_pairs().len(), h.stats().timed_out_pairs);
    }

    /// Global wall-clock deadline is sound and bounded.
    ///
    /// Uses a tiny pure-EL ontology to check the told-subsumption path
    /// (A ⊑ B survives any budget via saturation, never probed) plus an
    /// out-of-EL ontology (∀+∃, falls through to the tableau path) to
    /// confirm the deadline actually bounds the wall and that
    /// `undecided_pairs().len() == stats().timed_out_pairs` (anytime
    /// invariant).
    #[test]
    fn global_deadline_is_sound_and_bounded() {
        use std::time::{Duration, Instant};

        // Tiny pure-EL ontology: A ⊑ B is a told subsumption, decided by the
        // saturation closure before any probe is issued. Survives even a near-zero budget.
        let src_el = "Prefix(:=<http://t/>)\n\
Ontology(\n  Declaration(Class(:A)) Declaration(Class(:B))\n  SubClassOf(:A :B)\n)\n";
        let onto_el = parse(src_el);
        let t0 = Instant::now();
        let h_el = classify_with_global_deadline(&onto_el, Duration::from_millis(50))
            .expect("classify pure-EL");
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "global deadline must bound the wall (pure-EL path)"
        );
        // A ⊑ B is told/saturator-decided (not probe-gated), so it survives a tiny budget.
        assert!(
            h_el.is_subclass("http://t/A", "http://t/B"),
            "told subsumption A ⊑ B must survive global deadline"
        );
        // Anytime invariant holds even when nothing times out.
        assert_eq!(h_el.undecided_pairs().len(), h_el.stats().timed_out_pairs);

        // Out-of-EL ontology (∀ + ∃): forces the tableau path, exercising the
        // actual deadline threading. A 1 ms global budget means most pairs will
        // time out; that's fine — we only check the wall bound and the invariant.
        let src_oe = "Prefix(:=<http://t/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(\n\
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(ObjectProperty(:r))\n\
  SubClassOf(:A ObjectAllValuesFrom(:r :B))\n\
  SubClassOf(:A ObjectSomeValuesFrom(:r owl:Thing))\n)\n";
        let onto_oe = parse(src_oe);
        let t1 = Instant::now();
        let h_oe = classify_with_global_deadline(&onto_oe, Duration::from_millis(1))
            .expect("classify out-of-EL");
        assert!(
            t1.elapsed() < Duration::from_secs(5),
            "global deadline must bound the wall (out-of-EL path)"
        );
        // Anytime invariant: every timed-out pair is recorded in undecided_pairs().
        assert_eq!(
            h_oe.undecided_pairs().len(),
            h_oe.stats().timed_out_pairs,
            "undecided_pairs() must mirror timed_out_pairs count"
        );
    }

    /// Global deadline must bound the label-cache build phase, not just
    /// the per-pair probe phase. Without the fix, each label-build call
    /// uses a fresh 5000 ms per-class budget regardless of the global
    /// deadline — on a stalling ontology (∃R.B ⊓ ∀R.C with B⊓C⊑⊥) the
    /// label-cache build alone could run for many seconds. With the fix the
    /// per-class deadline is capped at the global deadline, so the whole
    /// classify call returns well within the budget.
    #[test]
    fn global_deadline_bounds_label_cache_build() {
        use std::time::{Duration, Instant};
        // Out-of-EL input that forces the hybrid path and exercises the
        // label-cache build (∃R + ∀R + disjointness clash → ¬pure-EL).
        // The `owl:Nothing` sink makes A unsatisfiable so the label build
        // must run the wedge per class.
        let src = "Prefix(:=<http://t/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(\n  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n  \
Declaration(ObjectProperty(:r))\n  \
SubClassOf(:A ObjectSomeValuesFrom(:r :B)) SubClassOf(:A ObjectAllValuesFrom(:r :C))\n  \
SubClassOf(ObjectIntersectionOf(:B :C) owl:Nothing)\n)\n";
        let onto = parse(src);
        let t0 = Instant::now();
        let _h = classify_with_global_deadline(&onto, Duration::from_millis(100))
            .expect("classify with global deadline");
        assert!(
            t0.elapsed() < Duration::from_secs(3),
            "global deadline must bound label-cache build: {:?}",
            t0.elapsed()
        );
    }
}
