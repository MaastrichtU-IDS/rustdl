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

use fixedbitset::FixedBitSet;
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;

use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{
    Axiom, ConceptExpr, ConceptId, ConceptPool, InternalOntology, Role, RoleId, SubRolePath,
};
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

/// Row-major subsumption matrix backing [`Classification::entails`].
///
/// `Dense` is the historical `Vec<FixedBitSet>` (n×n bits, O(1)
/// contains) — used for every ontology up to [`dense_max`] classes,
/// keeping the whole curated corpus on the byte-identical dense path
/// and the EL niche fast. `Sparse` stores each row as an
/// ASCENDING-sorted `Vec<u32>` of subsumer ids (O(log k) contains) —
/// used only for giants where the dense n×n bitset is intractable
/// (`ore_ont_868`: 981k classes ⇒ 112 GB dense vs a few hundred MB
/// sparse; the accessors then iterate rows O(k) instead of scanning
/// `0..n`, collapsing the O(n²) hierarchy print).
///
/// In BOTH arms, unsatisfiable classes' rows are ELIDED (left empty) —
/// the trivial "⊥ ⊑ everything" fill (previously `insert_range(..n)`;
/// on 868 a single such row is 122 MB) is reintroduced solely by the
/// [`Classification::entails`] choke-point.
#[derive(Debug, Clone)]
enum EntailmentMatrix {
    Dense(Vec<FixedBitSet>),
    Sparse(Vec<Vec<u32>>),
}

/// Largest class count that still uses the [`EntailmentMatrix::Dense`]
/// arm. The largest curated fixture is go-basic (~52k classes); 60k
/// keeps every curated fixture on the byte-identical dense path
/// (≤ 450 MB) while every ORE giant (≫ 100k classes) goes sparse.
///
/// Test-only override: `RUSTDL_CLASSIFY_DENSE_MAX` — used by the
/// dense-vs-sparse identity gates
/// (`tests/sparse_classification_identity.rs`, and the galen/sio
/// self-diff) to force the sparse arm on small inputs. Not a user knob.
fn dense_max() -> usize {
    std::env::var("RUSTDL_CLASSIFY_DENSE_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(60_000)
}

/// Hoisted Hasse (direct-subsumer) reduction (`RUSTDL_FAST_DIRECT_SUBSUMERS`).
/// **Default OFF** (set `=1` to opt in).
///
/// [`Classification::direct_subsumers`] re-derives the transitive reduction from
/// scratch on every call: `O(k²)` [`Classification::entails`] probes for a class
/// with `k` strict supers, and — because an unsatisfiable subject's row is elided —
/// a full `O(n²)` scan for *each* unsatisfiable class. On `ore_ont_10125`
/// (73 449 classes, 1 111 855 subsumptions, 6 unsatisfiable) reasoning finishes in
/// ~15 s and the CLI then spends >385 s in the `direct` emission loop.
///
/// With the flag on, the shared work is hoisted into a `OnceLock` index built on
/// the first call: each class's strict supers once, plus the set of *maximal*
/// satisfiable classes (which is, by the elided-row semantics, the answer for
/// EVERY unsatisfiable subject — so the `O(n²)` scan happens at most once instead
/// of once per unsatisfiable class). Output — membership AND ascending order — is
/// unchanged; see `direct_subsumers` for the equivalence argument.
fn fast_direct_subsumers_enabled() -> bool {
    // DEFAULT ON since 0.4.7 (`=0` reverts). Verdict-preserving: output membership AND
    // ascending order are unchanged (an ordering sabotage is one of the four canaries),
    // and it takes `ore_ont_10125` from DNF at 900 s to 14.70 s complete.
    std::env::var_os("RUSTDL_FAST_DIRECT_SUBSUMERS").is_none_or(|v| v != "0")
}

/// Hoisted state behind [`fast_direct_subsumers_enabled`]. Built once per
/// [`Classification`], lazily, from the already-materialized entailment matrix.
#[derive(Debug, Clone)]
struct DirectSubsumerIndex {
    /// `strict[i]` = the STRICT supers of `i` (`i ⊑ j`, `j ≠ i`, `j ⋢ i`), ascending.
    /// Empty for unsatisfiable `i` — their rows are elided, so their strict-super
    /// set is not row-derivable and is handled by `minimal_sat` instead.
    strict: Vec<Vec<u32>>,
    /// The satisfiable classes that have no strict satisfiable *sub*class — i.e. the
    /// MINIMAL ones. `⊥` sits at the bottom of the hierarchy, so these are exactly
    /// the Hasse-direct supers of any UNSATISFIABLE subject. Ascending.
    minimal_sat: Vec<u32>,
}

impl EntailmentMatrix {
    /// `Dense` iff `n <= dense_max()`, else `Sparse`. Chosen once per
    /// classification build.
    fn new(n: usize) -> Self {
        if n <= dense_max() {
            Self::Dense((0..n).map(|_| FixedBitSet::with_capacity(n)).collect())
        } else {
            Self::Sparse(vec![Vec::new(); n])
        }
    }

    /// Record `classes[i] ⊑ classes[j]`. Idempotent. Sparse rows stay
    /// ascending: an in-order append is O(1); an out-of-order insert is
    /// O(k) (rows are small — ~16 supers/class on the giants this arm
    /// serves).
    fn insert(&mut self, i: usize, j: usize) {
        match self {
            Self::Dense(rows) => rows[i].insert(j),
            Self::Sparse(rows) => {
                let j = u32::try_from(j).expect("class index fits in u32");
                let row = &mut rows[i];
                match row.last() {
                    Some(&last) if last < j => row.push(j),
                    None => row.push(j),
                    Some(&last) if last == j => {}
                    _ => {
                        if let Err(pos) = row.binary_search(&j) {
                            row.insert(pos, j);
                        }
                    }
                }
            }
        }
    }

    /// True iff row `i` records `j` as a super. UNSAT rows are elided,
    /// so callers other than [`Classification::entails`] are forbidden —
    /// a raw row read would lose "⊥ ⊑ everything".
    fn row_contains(&self, i: usize, j: usize) -> bool {
        match self {
            Self::Dense(rows) => rows[i].contains(j),
            Self::Sparse(rows) => u32::try_from(j).is_ok_and(|j| rows[i].binary_search(&j).is_ok()),
        }
    }

    /// The members of row `i` in ASCENDING id order (the accessors'
    /// output-order contract). Returns a `Vec` rather than an iterator
    /// so both enum arms stay trivial; sparse rows are tiny, and the
    /// dense arm's accessors already paid an O(n) scan per call.
    fn row_ascending(&self, i: usize) -> Vec<usize> {
        match self {
            Self::Dense(rows) => rows[i].ones().collect(),
            Self::Sparse(rows) => rows[i].iter().map(|&j| j as usize).collect(),
        }
    }
}

/// Result of [`classify`]. Holds the complete pairwise subsumption
/// matrix over every declared named class plus the IRIs themselves,
/// keyed by stable insertion order.
#[derive(Debug, Clone)]
pub struct Classification {
    classes: Vec<String>,
    index: HashMap<String, usize>,
    /// Row `i` holds the SATISFIABLE supers of `classes[i]` (including
    /// the reflexive entry `i` for satisfiable classes); rows of
    /// unsatisfiable classes are elided. Read ONLY through
    /// [`Self::entails`] — never touch a raw row (see the invariant
    /// there).
    entailed: EntailmentMatrix,
    unsatisfiable_idxs: HashSet<usize>,
    stats: ClassificationStats,
    /// Memoized transitive reduction, built on first use iff
    /// [`fast_direct_subsumers_enabled`]. Derived purely from the three fields
    /// above, so cloning an empty cell (or a filled one) is always consistent.
    direct_index: std::sync::OnceLock<DirectSubsumerIndex>,
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
    /// Bound-the-tail (`RUSTDL_BOUND_DIVERGED_TAIL`): pairs whose main-tableau
    /// fallthrough was skipped because the wedge diverged (thrashed at saturated
    /// depth). Also counted in `timed_out_pairs`. Diagnostic only.
    pub diverged_tail_skips: usize,
    /// Phase 0 bound-the-tail exploration (diagnostic): a "stall fallthrough" is
    /// a wedge `Unknown`/`UnknownDiverged` (a stall, not a fast-refute/counting
    /// verify) that reached the main tableau. The rescue rate
    /// `fallthrough_subsumed / fallthrough_ran` decides whether skipping the
    /// fallthrough is MISSED-safe; the `_diverged` splits say whether rescues
    /// come from divergence- vs deadline-stalls.
    pub fallthrough_ran: usize,
    pub fallthrough_subsumed: usize,
    pub fallthrough_notsubsumed: usize,
    pub fallthrough_noverdict: usize,
    pub fallthrough_from_diverged: usize,
    pub fallthrough_subsumed_diverged: usize,
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
    /// Subsumptions recovered by the label-cache back-fold (Task 3,
    /// `RUSTDL_CLASSIFY_BACKFOLD`): entailed defined-`∃` names
    /// (`LabelOracle::Sat::derived_sups`) that `HyperEngine::backfold_derived`
    /// proved over the branch-free, merge-enriched `sat(c)` graph. Added
    /// directly, no tableau — mirrors `defined_sub_sweep_recovered`. Zero
    /// unless the flag is on. See
    /// `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md`.
    pub backfold_recovered: usize,
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
    /// Wall time in the TIER WALK proper — the `for tier in &tiers` loop of
    /// `classify_top_down_internal`, MEASURED DIRECTLY. Diagnostic only.
    ///
    /// **This used to be a residual** (`total − label_cache − snapshot_build −
    /// snapshot_replay`), which silently charged every unmeasured phase — the
    /// EL saturation, `from_internal`, the unsat probes, both sweeps, the
    /// entailment-matrix BFS — to the tier walk. On `ore_ont_1028` it reported
    /// `tier_walk = 7198 ms` for a tier walk that actually took 80 ms, and an
    /// earlier taxonomy of the DNF corpus was FALSIFIED because of it: the
    /// instrument pointed at the wrong phase and a whole bucket classification
    /// was built on the reading. Every phase now has its own line item and the
    /// leftover is named [`Self::unattributed_wall_ms`], so a residual can never
    /// masquerade as a phase again.
    pub tier_walk_wall_ms: u64,
    /// Wall time in `owl_dl_saturation::saturate` (the EL closure). Diagnostic only.
    pub saturate_wall_ms: u64,
    /// Wall time in the KB-level inconsistency pre-check plus, on the
    /// saturation fast path, the `abox_check` build+run. Diagnostic only.
    pub precheck_wall_ms: u64,
    /// Wall time in `PreparedOntology::from_internal` plus the `abox_verdict()`
    /// that first forces it. Diagnostic only.
    pub prepare_wall_ms: u64,
    /// Wall time in the per-class unsatisfiability probe loop. Diagnostic only.
    pub unsat_probe_wall_ms: u64,
    /// Wall time in the defined-sup sweep, the defined-SUB sweep and the
    /// label-cache back-fold, together. Diagnostic only.
    pub sweep_wall_ms: u64,
    /// Wall time building the entailment matrix (closure seed + transitive-closure
    /// BFS over `direct_supers`). Diagnostic only.
    pub matrix_wall_ms: u64,
    /// The NAMED leftover: total classify wall minus every phase line item above.
    /// Covers the class-IRI/index build, the fragment analysis, tier grouping and
    /// the `Classification` assembly.
    ///
    /// It exists so the components SUM to the wall without any one phase
    /// absorbing the difference. If this grows large, a phase is missing a timer
    /// — which is exactly the failure the old residual `tier_walk_wall_ms` hid.
    /// Pinned by `phase_components_sum_to_wall` in `crates/owl-dl-cli/tests`.
    ///
    /// NOTE `snapshot_cache_build_wall_ms` / `snapshot_replay_wall_ms` are NESTED
    /// sub-timers of the label-cache and tier-walk phases, so they are
    /// deliberately NOT part of this sum — subtracting them would double-count.
    pub unattributed_wall_ms: u64,
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
    /// Phase 2: subsumption pairs recovered by counting-pair verification —
    /// a wedge `NotSubsumed` that the main-tableau `concrete_domain_clash`
    /// flipped to `Subsumed` because the pair was data-counting-relevant.
    pub counting_verified_pairs: usize,
    /// `RUSTDL_PREP_DEADLINE` fired: the global wall-clock budget expired during
    /// a PREPARATION phase (EL saturation or
    /// `PreparedOntology::from_internal`), so the reported hierarchy is the EL
    /// closure read-off — **sound, and a deliberate under-approximation**.
    /// Always accompanied by a `timed_out_pairs` bump, so
    /// `completeness_guaranteed()` is false and `classify --json` reports
    /// `"incomplete": true`. See [`crate::prep_deadline_enabled`].
    pub prep_timed_out: bool,
}

impl Classification {
    /// Every declared class IRI in insertion order.
    #[must_use]
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// True iff `classes[i] ⊑ classes[j]`.
    ///
    /// INVARIANT (the sparse-row contract): a satisfiable class's row
    /// contains only satisfiable supers — every builder skips
    /// unsatisfiable `j` when filling a satisfiable row `i`. An
    /// unsatisfiable `i` subsumes everything (⊥ ⊑ *); its row is NOT
    /// materialized, so the short-circuit below is the ONLY place that
    /// fact is reintroduced. Every accessor MUST route through here —
    /// no accessor may touch a raw row.
    fn entails(&self, i: usize, j: usize) -> bool {
        self.unsatisfiable_idxs.contains(&i) || self.entailed.row_contains(i, j)
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
        self.entails(i, j)
    }

    /// All classes equivalent to `c` (including `c` itself), in
    /// ascending id order. Empty if `c` is not declared in the
    /// ontology.
    #[must_use]
    pub fn equivalent_classes(&self, c: &str) -> Vec<&str> {
        let Some(&i) = self.index.get(c) else {
            return Vec::new();
        };
        if self.unsatisfiable_idxs.contains(&i) {
            // All unsatisfiable classes are mutually equivalent (≡ ⊥).
            // Their rows are elided, so read the unsat set directly
            // (sorted ascending — the old `(0..n)` scan order).
            let mut idxs: Vec<usize> = self.unsatisfiable_idxs.iter().copied().collect();
            idxs.sort_unstable();
            return idxs.into_iter().map(|j| self.classes[j].as_str()).collect();
        }
        // Satisfiable subject: candidates are i's supers (all
        // satisfiable, by the `entails` invariant) — O(k), not O(n).
        // Merge the reflexive `i` in sorted position so the output
        // stays ascending even if a row ever lacked its reflexive bit.
        let mut candidates = self.entailed.row_ascending(i);
        if let Err(pos) = candidates.binary_search(&i) {
            candidates.insert(pos, i);
        }
        candidates
            .into_iter()
            .filter(|&j| self.entails(i, j) && self.entails(j, i))
            .map(|j| self.classes[j].as_str())
            .collect()
    }

    /// The Hasse-direct super-classes of `c`: every `D` with
    /// `c ⊑ D`, `D ≢ c`, and no intermediate `E ≠ c, D` such that
    /// `c ⊑ E ⊑ D`. Empty if `c` is not declared. Ascending id order.
    #[must_use]
    pub fn direct_subsumers(&self, c: &str) -> Vec<&str> {
        let Some(&i) = self.index.get(c) else {
            return Vec::new();
        };
        if fast_direct_subsumers_enabled() {
            return self.direct_subsumers_fast(i);
        }
        // First: every strict super (i ⊑ j, not j ⊑ i), ascending.
        let strict_supers: Vec<usize> = if self.unsatisfiable_idxs.contains(&i) {
            // Degenerate case: an unsatisfiable subject subsumes
            // everything and its row is elided — keep the full `0..n`
            // scan (rare; correctness over speed here).
            (0..self.classes.len())
                .filter(|&j| j != i && self.entails(i, j) && !self.entails(j, i))
                .collect()
        } else {
            // O(k): i's row lists exactly its supers, ascending.
            self.entailed
                .row_ascending(i)
                .into_iter()
                .filter(|&j| j != i && !self.entails(j, i))
                .collect()
        };
        // Then: prune any `j` for which there is a `k` strictly
        // between i and j (i ⊑ k ⊑ j, neither equivalent).
        strict_supers
            .iter()
            .copied()
            .filter(|&j| {
                !strict_supers
                    .iter()
                    .any(|&k| k != j && self.entails(k, j) && !self.entails(j, k))
            })
            .map(|j| self.classes[j].as_str())
            .collect()
    }

    /// Hoisted equivalent of [`Self::direct_subsumers`]'s body, behind
    /// [`fast_direct_subsumers_enabled`]. Returns the SAME classes in the SAME
    /// (ascending) order — this is a re-association of the same predicate, not an
    /// approximation.
    ///
    /// Write `S(i)` for `i`'s strict supers and `strict(k)` for `k`'s. The slow
    /// path keeps `j ∈ S(i)` iff no `k ∈ S(i)` has `k ≠ j ∧ k ⊑ j ∧ j ⋢ k`. Every
    /// `k ∈ S(i)` is satisfiable (an unsatisfiable `k` has `entails(k, i)` true, so
    /// the `!entails(j, i)` filter already dropped it), hence `entails(k, j)` is
    /// exactly `j ∈ row(k)` — so that condition is precisely `j ∈ strict(k)`.
    /// The predicate is therefore `j ∈ S(i) \ ⋃_{k ∈ S(i)} strict(k)`, and the
    /// `strict(·)` sets are subject-independent: computed once, reused for all `n`
    /// subjects.
    ///
    /// The unsatisfiable subject is the same identity taken to its limit. Its row
    /// is elided, so `entails(i, ·)` is unconditionally true and `entails(j, i)` is
    /// true exactly for the other unsatisfiable `j` — giving `S(i) = ` *all*
    /// satisfiable classes, the same set for every unsatisfiable `i`. Applying the
    /// same `S(i) \ ⋃ strict(k)` predicate leaves the classes that are nobody's
    /// strict super, i.e. the MINIMAL satisfiable ones (`⊥` sits at the bottom).
    /// Computed once as `minimal_sat` rather than by an `O(n²)` rescan per
    /// unsatisfiable class.
    fn direct_subsumers_fast(&self, i: usize) -> Vec<&str> {
        let idx = self.direct_index.get_or_init(|| self.build_direct_index());
        if self.unsatisfiable_idxs.contains(&i) {
            return idx
                .minimal_sat
                .iter()
                .map(|&j| self.classes[j as usize].as_str())
                .collect();
        }
        let supers = &idx.strict[i];
        // `marked` = ⋃ strict(k) over k ∈ S(i). Sorted + deduped rather than a
        // per-call n-bit set: |marked| is bounded by Σ|strict(k)|, which on real
        // hierarchies is tiny next to n.
        let mut marked: Vec<u32> = Vec::new();
        for &k in supers {
            marked.extend_from_slice(&idx.strict[k as usize]);
        }
        marked.sort_unstable();
        marked.dedup();
        supers
            .iter()
            .filter(|j| marked.binary_search(j).is_err())
            .map(|&j| self.classes[j as usize].as_str())
            .collect()
    }

    /// Build [`DirectSubsumerIndex`]. `O(Σ|row|)` plus one `O(n)` maximality scan.
    /// Uses [`Self::entails`] for every membership question, so it cannot diverge
    /// from the slow path's notion of "strict super".
    fn build_direct_index(&self) -> DirectSubsumerIndex {
        let n = self.classes.len();
        let mut strict: Vec<Vec<u32>> = vec![Vec::new(); n];
        // Unsatisfiable rows are elided, so `strict` is left empty for them; their
        // answer comes from `minimal_sat` instead.
        for (i, row) in strict.iter_mut().enumerate() {
            if self.unsatisfiable_idxs.contains(&i) {
                continue;
            }
            *row = self
                .entailed
                .row_ascending(i)
                .into_iter()
                .filter(|&j| j != i && !self.entails(j, i))
                .map(|j| u32::try_from(j).expect("class index fits in u32"))
                .collect();
        }
        // A satisfiable class is MINIMAL iff it is no satisfiable class's strict
        // super. (Only computed once — it is the answer for every unsatisfiable
        // subject, and there may be thousands of those.)
        let mut has_strict_subclass = FixedBitSet::with_capacity(n);
        for row in &strict {
            for &j in row {
                has_strict_subclass.insert(j as usize);
            }
        }
        let minimal_sat: Vec<u32> = (0..n)
            .filter(|i| !self.unsatisfiable_idxs.contains(i) && !has_strict_subclass.contains(*i))
            .map(|i| u32::try_from(i).expect("class index fits in u32"))
            .collect();
        DirectSubsumerIndex {
            strict,
            minimal_sat,
        }
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

    /// `true` iff the reported hierarchy is **guaranteed complete** — no missed
    /// subsumptions, silent or flagged.
    ///
    /// This is the honest calibration contract: `completeness_guaranteed()` ⟹
    /// `MISSED == 0`. It holds **only** on the provably-complete fragments
    /// ([`FragmentClassification::PureEl`] — the saturator is complete; or
    /// [`FragmentClassification::Horn`] — the hyper Horn fixpoint is complete)
    /// **and** when no per-pair probe timed out.
    ///
    /// On `OutOfFragment` inputs it returns `false` even when nothing timed out,
    /// because the classifier relies on the wedge's `trust_sat` verdicts, which
    /// are sound but **not proven complete**: a spurious `Sat` on
    /// complement/disjunction structure can silently miss a subsumption the full
    /// tableau would find (measured on the ORE corpus — see
    /// `docs/paper-calibration-decomposition-2026-07-08.md`). A `false` here does
    /// *not* mean the hierarchy is incomplete — only that completeness is not
    /// *guaranteed*. Callers needing a proof-carrying hierarchy should treat
    /// `false` as "verify externally".
    #[must_use]
    pub fn completeness_guaranteed(&self) -> bool {
        matches!(
            self.stats.fragment,
            FragmentClassification::PureEl | FragmentClassification::Horn
        ) && self.stats.timed_out_pairs == 0
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
    let t0 = Instant::now();
    let internal = convert_ontology(ontology)?;
    classify_top_down_internal(&internal, None, Some(budget_origin(t0) + budget))
}

/// Where a global budget's clock starts.
///
/// Historically it started AFTER `convert_ontology`, so a caller's "1 ms" was
/// really "conversion + 1 ms" — and conversion is itself a multi-second DNF on
/// some ORE inputs (`ore_ont_10926`: 23.8 s). Under
/// [`crate::prep_deadline_enabled`] the clock starts at the call instead, which
/// is what the caller asked for; conversion is still not INTERRUPTIBLE (a known
/// residual), but it no longer silently extends the promise. Flag off ⇒ the
/// pre-existing origin, exactly.
fn budget_origin(call_instant: Instant) -> Instant {
    if crate::prep_deadline_enabled() {
        call_instant
    } else {
        Instant::now()
    }
}

/// Classify with BOTH a per-pair tableau deadline and a global wall-clock
/// budget — the recommended bounded entry point. Each probe is cut at
/// `min(per_pair, remaining global)` (see [`effective_deadline`]); `per_pair`
/// bounds any single pair, `global_budget` bounds the total wall so the run
/// can't grow with the pair count. Either may be `None` (that bound absent;
/// both `None` ⇒ the unbounded top-down classify).
///
/// **Sound** at every setting: a cut probe defaults to "not subsumed"
/// (FP=0; the hierarchy may MISS a real subsumption — inspect
/// [`ClassificationStats`]`::complete` / `timed_out_pairs`).
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_with_budget<A: ForIRI>(
    ontology: &SetOntology<A>,
    per_pair_timeout: Option<std::time::Duration>,
    global_budget: Option<std::time::Duration>,
) -> Result<Classification, ReasonError> {
    let t0 = Instant::now();
    let internal = convert_ontology(ontology)?;
    let global_deadline = global_budget.map(|b| budget_origin(t0) + b);
    classify_top_down_internal(&internal, per_pair_timeout, global_deadline)
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
    Ok(classify_pure_el(
        internal,
        &classes,
        &index,
        &closure,
        analyze_fragment(internal),
    ))
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

    // Sound KB-level inconsistency pre-check (`RUSTDL_CLASSIFY_INCONSISTENCY`,
    // default OFF). Placed BEFORE the fast-path branch so both dispatch arms —
    // the saturation fast path and the hybrid path — are covered by one call
    // site. See `classify_inconsistency_precheck`: it tests that `⊤` is
    // unsatisfiable, NOT that every named class is (the latter is not an
    // inconsistency signal). This is the fix for `classify --json family.ofn`
    // reporting `"consistent": true` where `rustdl consistent` reports
    // `inconsistent`.
    if crate::classify_inconsistency_enabled()
        && crate::classify_inconsistency_precheck(internal, &closure)
    {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("classify: KB inconsistent (pre-check)");
        }
        return Ok(classify_inconsistent(
            classes,
            index,
            analyze_fragment(internal),
        ));
    }

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
        || (crate::classify_tbox_fragment_enabled() && tbox_only_saturator_eligible(internal))
    {
        // Lever 1 admits ABox-bearing ontologies to this fast path, so — like
        // the top-down path — run the sound ABox-driven inconsistency pre-check
        // first (nominal-free ABox is irrelevant to subsumption, but an
        // inconsistent ABox still makes every class unsatisfiable). ABox-free
        // inputs skip it (has_abox_axioms is a microsecond O(n) scan).
        if crate::abox_check_enabled() && has_abox_axioms(internal) {
            // Build ONLY what abox_check reads, reusing the closure the caller
            // already computed — the full `PreparedOntology` built here previously
            // was discarded immediately (this branch either returns
            // `classify_inconsistent` or falls through to `classify_pure_el`, and
            // neither uses it). Measured 0.62 s / 185 MB on `ore_ont_1043`.
            let owned = crate::build_abox_check_inputs(internal);
            let verdict = crate::abox_check::check(&owned.as_inputs(&closure));
            if let crate::abox_check::AboxVerdict::Inconsistent { reason } = &verdict {
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
        return Ok(classify_pure_el(
            internal,
            &classes,
            &index,
            &closure,
            analyze_fragment(internal),
        ));
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
                    .decide_classify_with_deadline(deadline, move |pool| pool.atomic(class_id))?
                    .unwrap_or(true);
                Ok((i, sat, false))
            } else {
                let sat = prepared.decide_classify(move |pool| pool.atomic(class_id))?;
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
    let mut entailed = EntailmentMatrix::new(n);
    let mut work: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        if unsatisfiable_idxs.contains(&i) {
            // Row elided — `Classification::entails` supplies ⊥ ⊑ *.
            continue;
        }
        entailed.insert(i, i);
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
                    let sat = prepared.decide_classify(build)?;
                    Ok((i, j, !sat, false, false))
                }
                Some(timeout) => {
                    // Cooperative deadline: the tableau's search loop
                    // checks `Instant::now()` against this deadline on
                    // every recursion and bails out if exceeded. No
                    // extra threads, no cancellation race — the rayon
                    // worker stays bound to this single decide call.
                    let deadline = Instant::now() + timeout;
                    match prepared.decide_classify_with_deadline(deadline, build)? {
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
        if is_entailed {
            entailed.insert(i, j);
        }
    }
    let _ = satisfiable; // currently informational only
    Ok(Classification {
        classes,
        index,
        entailed,
        unsatisfiable_idxs,
        stats,
        direct_index: std::sync::OnceLock::new(),
    })
}

/// Degrade to the EL closure read-off because the global wall-clock budget
/// expired during a PREPARATION phase (`RUSTDL_PREP_DEADLINE`; `phase` names
/// which one, for `RUSTDL_TRACE`).
///
/// **Sound under-approximation, explicitly flagged incomplete.** The partial
/// closure holds only entailed subsumptions, so the hierarchy can MISS but never
/// gain an edge — the `RUSTDL_MAX_NODES` → `NodeCap` → `Ok(None)` precedent
/// applied to the build instead of the search. Three signals fire so no caller
/// can mistake this for a complete answer:
///
/// * `stats.prep_timed_out = true` (programmatic, unambiguous);
/// * `timed_out_pairs` is bumped, which is what `classify --json` maps to
///   `"incomplete": true` and what `completeness_guaranteed()` reads;
/// * `fragment` is forced to `OutOfFragment` — the conservative value. It is NOT
///   `analyze_fragment(internal)`: that clausifies the entire ontology, which is
///   exactly the sort of unbudgeted work this path exists to avoid, and a
///   `PureEl`/`Horn` verdict here would claim a completeness we just abandoned.
fn classify_prep_timeout(
    internal: &InternalOntology,
    classes: &[String],
    index: &HashMap<String, usize>,
    closure: &owl_dl_saturation::Subsumers,
    phase: &str,
) -> Classification {
    if std::env::var_os("RUSTDL_TRACE").is_some() {
        eprintln!("classify: prep deadline expired in {phase} — partial EL closure returned");
    }
    let mut h = classify_pure_el(
        internal,
        classes,
        index,
        closure,
        FragmentClassification::OutOfFragment,
    );
    h.stats.prep_timed_out = true;
    // Invariant kept by every other timeout site: +1 count, +1 id (so
    // `undecided_pairs()` stays index-safe). With no classes there is no id to
    // record; the count alone still drives the `incomplete` signal.
    h.stats.timed_out_pairs += 1;
    if !classes.is_empty() {
        h.stats.timed_out_pair_ids.push((0, 0));
    }
    h
}

/// Fast-path classifier for ontologies that lie entirely inside our
/// EL saturation fragment. The closure is then *complete* — both
/// subsumption and unsatisfiability decisions reduce to closure
/// lookups, with no tableau calls. Sets `stats.pure_el_mode = true`.
///
/// `fragment` is passed in rather than computed here: [`analyze_fragment`]
/// clausifies the whole ontology, and the `RUSTDL_PREP_DEADLINE` degradation
/// path (which reuses this read-off for a PARTIAL closure) must not pay that
/// cost after its budget has already expired — it supplies the conservative
/// [`FragmentClassification::OutOfFragment`] instead.
fn classify_pure_el(
    internal: &InternalOntology,
    classes: &[String],
    index: &HashMap<String, usize>,
    closure: &owl_dl_saturation::Subsumers,
    fragment: FragmentClassification,
) -> Classification {
    let n = classes.len();
    let mut stats = ClassificationStats {
        pure_el_mode: true,
        fragment,
        ..ClassificationStats::default()
    };

    // Flag KB-level inconsistency so that `classify --json` emits
    // `"consistent": false` and consumers agree with `rustdl consistent` and
    // `rustdl diagnose`.  Two complementary signals from the saturator:
    //
    // 1. `globally_inconsistent()` — set when the KB contains a syntactic
    //    `SubClassOf(owl:Thing, owl:Nothing)` (`⊤ ⊑ ⊥`) axiom.
    //
    // 2. `top_is_unsat()` — set when a named class `C` declared as a
    //    `⊤`-subsumer (i.e. `SubClassOf(owl:Thing, C)`) ended up unsatisfiable
    //    after saturation.  Covers the derived case such as `{⊤ ⊑ E, E ⊑ ⊥}`.
    //    Crucially it does NOT fire for `{A ⊑ ⊥, B ⊑ ⊥}` (consistent KB with
    //    every user class empty but no `⊤ ⊑ …` axiom), avoiding a false positive.
    //    See `Subsumers::top_is_unsat` for the full soundness argument.
    if closure.globally_inconsistent() || closure.top_is_unsat() {
        stats.inconsistent = true;
    }

    // Pass 1 — identify unsatisfiable classes (O(n) via the closure bitset).
    // Build the unsatisfiable bitset directly for O(1) per-bit membership test
    // in the subsumption read-off below.
    let mut unsatisfiable_idxs: HashSet<usize> = HashSet::new();
    let unsat_bs = closure.unsatisfiable_bitset();
    for i in 0..n {
        if i < unsat_bs.len() && unsat_bs.contains(i) {
            unsatisfiable_idxs.insert(i);
            stats.saturation_unsat_hits += 1;
        }
    }

    // Pass 2 — build the entailed rows in one pass over the closure.
    // For an unsat class i: row ELIDED (`Classification::entails`
    //   supplies ⊥ ⊑ * — no per-row O(n) fill; on the ORE giants a
    //   single dense unsat row would be n bits).
    // For a sat class i: copy the closure row for i, restricted to [0,n),
    //   skipping unsat j (unsat classes are ⊑ ⊥, not ⊒ others — the
    //   `entails` invariant), plus the reflexive entry i. Count
    //   non-reflexive, non-unsat-j hits as saturation_subsumption_hits,
    //   matching the original counter semantics.
    let mut entailed = EntailmentMatrix::new(n);
    for i in 0..n {
        let class_id =
            owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        if unsatisfiable_idxs.contains(&i) {
            continue; // row elided
        }
        entailed.insert(i, i); // reflexive
        // `subsumers_of` is ascending by id (as `FixedBitSet::ones()` was), so
        // the `>= n` break still terminates at the first synthetic id.
        for j_id in closure.subsumers_of(class_id) {
            let j = j_id.index() as usize;
            if j >= n {
                break; // synthetic Tseitin/DKey id — outside user vocabulary
            }
            if j == i || unsatisfiable_idxs.contains(&j) {
                continue; // reflexive already set; unsat-j skipped per original
            }
            entailed.insert(i, j);
            stats.saturation_subsumption_hits += 1;
        }
    }
    let _ = internal; // closure was built from this; nothing more to read
    Classification {
        classes: classes.to_vec(),
        index: index.clone(),
        entailed,
        unsatisfiable_idxs,
        stats,
        direct_index: std::sync::OnceLock::new(),
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
    // Every class is unsatisfiable, so every row is elided —
    // `Classification::entails` short-circuits each subject to `true`
    // (the old dense `insert_range(..n)` fill, without materializing
    // n×n bits).
    let entailed = EntailmentMatrix::new(n);
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
        direct_index: std::sync::OnceLock::new(),
    }
}

/// True iff every axiom in `internal` lies inside the EL fragment
/// the saturation engine is complete for. A conservative check: any
/// construct outside the supported shapes (disjunction, complement,
/// cardinality, nominals, inverse roles, role characteristics that
/// expand to cardinality, `ABox` assertions, ...) immediately returns
/// `false`.
pub(crate) fn is_pure_el(internal: &InternalOntology) -> bool {
    is_pure_el_impl(internal, false)
}

/// Backing check for [`is_pure_el`]. `skip_abox` ⟹ ignore `ABox` assertion
/// axioms — Lever 1: a nominal-free `ABox` is irrelevant to class subsumption,
/// so an EL `TBox` carrying a big `ABox` is still classified completely by the
/// saturation fast path.
fn is_pure_el_impl(internal: &InternalOntology, skip_abox: bool) -> bool {
    let bare = BareRoleDecls::analyze(internal);
    internal
        .axioms
        .iter()
        .filter(|ax| !(skip_abox && is_abox_axiom(ax)))
        .all(|ax| is_el_axiom(ax, &internal.concepts, &bare))
}

/// Which `SymmetricObjectProperty` / `InverseObjectProperties` declarations the
/// fragment gates may admit without breaking the "gate ⟹ saturator complete"
/// contract (`RUSTDL_FRAGMENT_BARE_DECL`, **default ON** since 0.4.7; `=0` reverts).
///
/// # The problem
///
/// The EL saturator has **no symmetry and no inverse rule at all** — grep
/// `owl-dl-saturation` for `SymmetricRole` / `InverseObjectProperties`: neither
/// variant is matched anywhere, so both are silently dropped. Admitting them to
/// a fragment gate unconditionally would therefore be a textbook D10 bug (gate
/// certifies the closure complete while the engine drops the axiom): with
/// `Symmetric(r)`, `A ⊑ ∃r.B` and `Range(r, E)` the backward edge `r(y, x)`
/// forces `A ⊑ E`, which the saturator never derives.
///
/// # What is admitted, and why it is sound-complete
///
/// A role id is **observable** when some axiom or concept can *read* its edge
/// set. The saturator's — and OWL's — only edge readers are: an occurrence in
/// any concept (`∃q.C`, `∀q.C`, `≥/≤n q.C`, `Self(q)`), `ObjectPropertyDomain`
/// / `ObjectPropertyRange`, being a *part* of a role chain,
/// `Functional` / `InverseFunctional` / `Reflexive` / `Irreflexive` /
/// `Asymmetric` / `DisjointObjectProperties`, an `ABox` (negative) property
/// assertion, or being **below** an observable role in the property hierarchy
/// (`q ⊑ s`, `s` observable ⟹ `q` observable, closed to a fixpoint; the
/// `EquivalentObjectProperties` members are mutually `⊑`-related).
///
/// `TransitiveRole(q)` is deliberately **not** a read: transitivity only
/// enlarges `q`'s own edge set, which is unobservable unless some genuine
/// reader above also mentions `q`. Neither are the symmetry / inverse
/// declarations under test, nor a role in the *super* position of a chain or of
/// `SubObjectPropertyOf` (that position only *receives* edges).
///
/// **Claim.** If `r` is non-observable, dropping `SymmetricRole(r)` changes no
/// class subsumption. *Proof.* Dropping an axiom only weakens the theory, so no
/// new entailment appears (FP-safe unconditionally). For completeness, take any
/// model `M` of the ontology minus the dropped declarations with `x ∈ C \ D`.
/// Non-observability is closed upward through `⊑` (contrapositive of the
/// hierarchy rule: every super-role of a non-observable role is itself
/// non-observable), so let `M'` extend `M` by closing the edge sets of the
/// non-observable roles under symmetry, under transitivity where declared, and
/// under the `⊑` edges among themselves — this only *adds* edges, and only to
/// non-observable roles. No concept mentions a non-observable role, so every
/// concept extension (hence `x ∈ C \ D`) is unchanged. No axiom is broken
/// either: the axiom shapes that adding `q`-edges could violate — `∀q.C`,
/// `≤n q`, `Functional`, domain/range, `Asymmetric`, `Irreflexive`, disjoint
/// properties, a negative assertion, a chain in which `q` is a part — all make
/// `q` observable by definition, and `SubObjectPropertyOf(t, q)` for an
/// observable `t` stays satisfied because `q` only grew. So `M' ⊨` the full
/// ontology *including* the symmetry declarations, and `C ⊑ D` is not entailed
/// there either. ∎ `InverseObjectProperties(p, q)` is the same argument with
/// `p^{M'}` and `q^{M'}` set to a common edge set and its converse; it requires
/// **both** roles to be non-observable.
///
/// Polarity is ignored throughout (everything is keyed on `RoleId`): symmetry
/// of `r⁻` is symmetry of `r`, and treating `r⁻`'s occurrences as `r`'s can
/// only over-approximate observability, i.e. admit fewer declarations.
#[derive(Debug, Default)]
struct BareRoleDecls {
    /// `false` ⟹ every query answers "not inert", i.e. the exact pre-flag
    /// behaviour (the declarations keep kicking the ontology off the fast path).
    enabled: bool,
    /// Role ids whose edge set some axiom or concept CAN read.
    observable: HashSet<RoleId>,
}

impl BareRoleDecls {
    fn analyze(internal: &InternalOntology) -> Self {
        if !crate::fragment_bare_decl_enabled() {
            return Self::default();
        }
        let mut observable: HashSet<RoleId> = HashSet::new();
        // (1) Every role mentioned by ANY interned concept expression. Scanning
        // the whole pool (rather than only axiom-reachable concepts) is the
        // conservative direction: a stale interning can only mark MORE roles
        // observable, never fewer.
        for (_, expr) in internal.concepts.iter_with_ids() {
            match expr {
                ConceptExpr::Some(r, _)
                | ConceptExpr::All(r, _)
                | ConceptExpr::Min(_, r, _)
                | ConceptExpr::Max(_, r, _)
                | ConceptExpr::SelfRestriction(r) => {
                    observable.insert(r.role_id());
                }
                ConceptExpr::Top
                | ConceptExpr::Bot
                | ConceptExpr::Atomic(_)
                | ConceptExpr::Nominal(_)
                | ConceptExpr::Not(_)
                | ConceptExpr::And(_)
                | ConceptExpr::Or(_) => {}
            }
        }
        // (2) Axiom positions that READ a role's edges, plus the `sub ⊑ sup`
        // pairs the fixpoint in (3) propagates observability along.
        //
        // The match is deliberately EXHAUSTIVE (no `_` arm): a new `Axiom`
        // variant must be classified as reader / non-reader by hand rather than
        // silently defaulting to "harmless".
        let mut sub_sup: Vec<(RoleId, RoleId)> = Vec::new();
        for ax in &internal.axioms {
            match ax {
                Axiom::SubObjectPropertyOf { sub, sup } => match sub {
                    SubRolePath::Role(r) => sub_sup.push((r.role_id(), sup.role_id())),
                    SubRolePath::Chain(parts) => {
                        // A chain PART is matched against existing edges — a
                        // read. The chain's `sup` only receives edges.
                        for p in parts {
                            observable.insert(p.role_id());
                        }
                    }
                },
                Axiom::EquivalentObjectProperties(roles) => {
                    for a in roles {
                        for b in roles {
                            if a.role_id() != b.role_id() {
                                sub_sup.push((a.role_id(), b.role_id()));
                            }
                        }
                    }
                }
                Axiom::ObjectPropertyDomain { role, .. }
                | Axiom::ObjectPropertyRange { role, .. }
                | Axiom::FunctionalRole(role)
                | Axiom::InverseFunctionalRole(role)
                | Axiom::AsymmetricRole(role)
                | Axiom::ReflexiveRole(role)
                | Axiom::IrreflexiveRole(role)
                | Axiom::ObjectPropertyAssertion { role, .. }
                | Axiom::NegativeObjectPropertyAssertion { role, .. } => {
                    observable.insert(role.role_id());
                }
                Axiom::DisjointObjectProperties(roles) => {
                    for r in roles {
                        observable.insert(r.role_id());
                    }
                }
                // NOT readers. `TransitiveRole` only enlarges the role's own
                // edge set; the symmetry/inverse declarations are the axioms
                // under test; the class axioms carry roles only inside concepts,
                // already covered by the pool scan in (1).
                Axiom::TransitiveRole(_)
                | Axiom::SymmetricRole(_)
                | Axiom::InverseObjectProperties(_, _)
                | Axiom::SubClassOf { .. }
                | Axiom::EquivalentClasses(_)
                | Axiom::DisjointClasses(_)
                | Axiom::DisjointUnion { .. }
                | Axiom::ClassAssertion { .. }
                | Axiom::SameIndividual(_)
                | Axiom::DifferentIndividuals(_)
                | Axiom::DeclareClass(_)
                | Axiom::DeclareObjectProperty(_)
                | Axiom::DeclareNamedIndividual(_) => {}
            }
        }
        // (3) Downward closure: `sub ⊑ sup` with an observable `sup` makes
        // `sub` observable too (edges pushed up are then read above).
        loop {
            let mut grew = false;
            for (sub, sup) in &sub_sup {
                if observable.contains(sup) && observable.insert(*sub) {
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        Self {
            enabled: true,
            observable,
        }
    }

    /// True iff `role`'s edge set is provably unread, so a symmetry / inverse
    /// declaration over it is semantically inert for class subsumption.
    fn unread(&self, role: Role) -> bool {
        self.enabled && !self.observable.contains(&role.role_id())
    }
}

/// Shared `SymmetricRole` / `InverseObjectProperties` arm for both fragment
/// gates. See [`BareRoleDecls`] for the soundness argument. Returns `false`
/// (i.e. "kick the ontology off the fast path") whenever the flag is off,
/// preserving the pre-flag verdict exactly.
fn is_inert_bare_role_decl(ax: &Axiom, bare: &BareRoleDecls) -> bool {
    match ax {
        Axiom::SymmetricRole(r) => bare.unread(*r),
        Axiom::InverseObjectProperties(p, q) => bare.unread(*p) && bare.unread(*q),
        _ => false,
    }
}

/// True for the five `ABox` assertion axiom forms (individual-level). Used by
/// Lever 1 ([`tbox_only_saturator_eligible`]) to restrict the fragment gate to
/// the `TBox`. Kept in sync with [`has_abox_axioms`].
fn is_abox_axiom(ax: &Axiom) -> bool {
    matches!(
        ax,
        Axiom::ClassAssertion { .. }
            | Axiom::ObjectPropertyAssertion { .. }
            | Axiom::NegativeObjectPropertyAssertion { .. }
            | Axiom::SameIndividual(_)
            | Axiom::DifferentIndividuals(_)
    )
}

/// True if `c` is `Atomic` in the concept pool.
///
/// This is the predicate that matches what the engine's `disjoint_pairs`
/// collector keeps: members are filtered to `ConceptExpr::Atomic` only
/// (`collect_el_rules`, the `DisjointClasses` arm). Used by the `DisjointClasses`
/// gate arms so the fragment check never admits a member the engine will drop.
fn is_atomic_concept(c: ConceptId, pool: &ConceptPool) -> bool {
    matches!(pool.get(c), ConceptExpr::Atomic(_))
}

/// True if `c` is a concept that the engine handles completely for
/// `ObjectPropertyDomain` / `ObjectPropertyRange` filler positions:
/// - `Atomic` — stored in `role_domains` / `role_ranges` and propagated.
/// - `Bot`    — stored in `poisoned_roles`; any `∃r.*` class becomes unsat.
/// - `Top`    — dropped by the engine, but semantically trivial: `Domain(r, ⊤)`
///   adds no subsumptions, so dropping it is sound (no missed entailment).
///
/// Everything else (`And`, `Some`, `Or`, …) is silently dropped by the engine
/// while potentially entailing real subsumptions — those MUST fall to the hybrid
/// path.  See `collect_el_rules`, the `ObjectPropertyDomain` arm.
fn is_atomic_or_trivial_concept(c: ConceptId, pool: &ConceptPool) -> bool {
    matches!(
        pool.get(c),
        ConceptExpr::Atomic(_) | ConceptExpr::Bot | ConceptExpr::Top
    )
}

fn is_el_axiom(ax: &Axiom, pool: &ConceptPool, bare: &BareRoleDecls) -> bool {
    match ax {
        // Bare (semantically inert) symmetry / inverse declaration — see
        // `BareRoleDecls`. Gated by `RUSTDL_FRAGMENT_BARE_DECL` (default ON since 0.4.7);
        // flag-off `unread` is constant-`false`, so this arm falls through to
        // the pre-flag `_ => false`.
        Axiom::SymmetricRole(_) | Axiom::InverseObjectProperties(_, _)
            if is_inert_bare_role_decl(ax, bare) =>
        {
            true
        }
        Axiom::SubClassOf { sub, sup } => is_el_concept(*sub, pool) && is_el_concept(*sup, pool),
        Axiom::EquivalentClasses(members) => members.iter().all(|c| is_el_concept(*c, pool)),
        // D10 gate tightening (Bug A): `DisjointClasses` members are filtered to
        // `Atomic` by the engine's `disjoint_pairs` collector (see
        // `collect_el_rules`, lines that do `filter_map(|c| match … Atomic(id) =>
        // Some(*id), _ => None)`). A non-atomic member (e.g. `ObjectUnionOf`) is
        // silently dropped, so the engine sees a singleton or empty member list and
        // emits no pairs — a sound-completeness hole when the full disjoint
        // semantics would entail a real unsatisfiability. Require every member
        // to be `Atomic` so the gate matches exactly what the engine keeps.
        // `Bot` and `Top` members are also dropped by the engine; `Bot` is trivial
        // (A ⊓ ⊥ ⊑ ⊥ always); `Top` is non-trivial (`DisjointClasses(A, ⊤)` entails
        // `A ⊑ ⊥`) but the engine drops it too — reject both so the gate stays safe.
        Axiom::DisjointClasses(members) => members.iter().all(|c| is_atomic_concept(*c, pool)),
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
        // D10 gate tightening (Bug B): `role_domains` / `role_ranges` in the engine
        // accept ONLY `Atomic` fillers (or `Bot` via `poisoned_roles`). A conjunctive
        // filler `And(:P :Q)` is silently dropped, so `X ⊑ ∃r.⊤` + `Domain(r)=P⊓Q`
        // misses `X ⊑ P` and `X ⊑ Q` while the gate claims "complete". Restrict to
        // `Atomic`, `Bot` (handled by `poisoned_roles`), and `Top` (trivially `⊤`
        // — `Domain(r, ⊤)` adds no subsumptions, so dropping it is sound).
        Axiom::ObjectPropertyDomain { role, domain } => {
            !role.is_inverse() && is_atomic_or_trivial_concept(*domain, pool)
        }
        Axiom::ObjectPropertyRange { role, range } => {
            !role.is_inverse() && is_atomic_or_trivial_concept(*range, pool)
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
        // Bot (Lever 1b): `⊥` is EL — `X ⊑ ⊥` (unsatisfiability) and `A⊓B ⊑ ⊥`
        // (disjointness) are both reasoned over completely by the saturator's
        // Bot/disjointness machinery. Sound here because `is_pure_el` admits NO
        // functional role (is_el_axiom rejects FunctionalRole), so the
        // disjoint×functional-merge interaction excluded from
        // `saturator_complete_fragment` cannot arise on this arm.
        ConceptExpr::Top | ConceptExpr::Atomic(_) | ConceptExpr::Bot => true,
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
/// back to the sound+complete hybrid path. `DisjointClasses` (and the
/// lowered-`⊥` form `A ⊓ B ⊑ ⊥`) IS admitted when no functional or
/// inverse-functional role is present (`disjoint_ok = true`) — the
/// disjoint×functional-merge interaction is unproven, so when a cardinality
/// role exists both forms fall back to the hybrid path. `DisjointUnion`
/// remains deliberately EXCLUDED (its disjunctive covering `C ≡ ⊔Di` is
/// out-of-fragment). Pure-EL+disjoint takes the separate [`is_pure_el`] arm
/// regardless. GALEN/notgalen (functional, no disjoint, no ∀, no chains>2,
/// no inverse) stay on the fast path — verified by
/// `galen_notgalen_in_saturator_fragment` + the corpus FP/MISSED gate.
pub(crate) fn saturator_complete_fragment(internal: &InternalOntology) -> bool {
    saturator_complete_fragment_impl(internal, false)
}

/// Backing check for [`saturator_complete_fragment`]. `skip_abox` ⟹ evaluate the
/// fragment allowlist over the `TBox` only (Lever 1). The functional-role /
/// disjoint-gating prelude is computed over ALL axioms (harmless — `ABox`
/// assertions declare no role characteristics), so only the final per-axiom
/// allowlist walk is `TBox`-restricted.
fn saturator_complete_fragment_impl(internal: &InternalOntology, skip_abox: bool) -> bool {
    // The set of roles for which conversion emitted a derived `∃R.⊤ ⊑ ≤1 R`
    // GCI: `FunctionalRole(r) → r` (FORWARD only — `derive_functional_max_
    // cardinality` does not emit for inverse-functional).
    //
    // WHY THIS IS SOUND-COMPLETE on the fast path: the saturator is complete on
    // EL+functional via its FunctionalRole BITSET (the D10 allowlist + the
    // role-axiom arm below) — functionality is enforced by the bitset, NOT by
    // this derived GCI, so the GCI is REDUNDANT on the fast path. Recognizing
    // the EXACT derived shape (only when backed by a matching `FunctionalRole`
    // axiom) keeps the fragment verdict identical to the pre-derivation
    // ontology; without it, the `Max` would spuriously kick EL+functional
    // ontologies (GALEN/notgalen) off the fast path. Empirically confirmed:
    // GALEN classifies on the fast path with the GCI present (closure 27997 =
    // Konclude, FP=0/MISSED=0, ~0.5 s) — see the corpus closure-diff gate.
    //
    // FP-CRITICAL: a user-written `≤1 R` with NO functional declaration is NOT
    // in this set ⇒ still rejected (no bitset enforces it; accepting it would
    // be a silently-dropped real `≤1` = unsound completeness, the D10 bug
    // class). Pinned by `saturator_fragment_rejects_user_unqualified_max_
    // without_functional`.
    let functional_roles: HashSet<Role> = internal
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::FunctionalRole(r) => Some(*r),
            _ => None,
        })
        .collect();
    // Disjointness is admitted only when there is no functional / inverse-
    // functional role: the disjoint×functional-merge interaction is unproven
    // (a later increment), so disjoint+functional falls to the hybrid path.
    let has_cardinality_role = functional_roles.iter().next().is_some()
        || internal
            .axioms
            .iter()
            .any(|ax| matches!(ax, Axiom::InverseFunctionalRole(_)));
    let disjoint_ok = !has_cardinality_role;
    let bare = BareRoleDecls::analyze(internal);
    internal
        .axioms
        .iter()
        .filter(|ax| !(skip_abox && is_abox_axiom(ax)))
        .all(|ax| {
            is_saturator_axiom(
                ax,
                &internal.concepts,
                &functional_roles,
                disjoint_ok,
                &bare,
            )
        })
}

/// Lever 1 eligibility: the ontology has an `ABox`, uses NO nominals, and its
/// `TBox` (all non-`ABox` axioms) lies in the saturator's complete fragment
/// (pure-EL, or the EL+functional/hierarchy/chains fragment). When true, the
/// nominal-free `ABox` is provably irrelevant to class subsumption, so the
/// ontology can take the saturation-only fast path instead of the O(n²)
/// per-pair hybrid loop. **Sound by construction** — see
/// [`crate::classify_tbox_fragment_enabled`]. Env gating is the caller's job
/// (mirrors [`saturator_complete_fragment`], a pure predicate).
pub(crate) fn tbox_only_saturator_eligible(internal: &InternalOntology) -> bool {
    has_abox_axioms(internal)
        && !crate::ontology_uses_nominals(internal)
        && (is_pure_el_impl(internal, true)
            || (crate::horn_shortcircuit_enabled()
                && saturator_complete_fragment_impl(internal, true)))
}

/// True iff `c` is exactly the derived functional-enforcement consequent
/// `≤1 role.⊤` (`Max(1, role, Top)` unqualified) AND `role` carries a matching
/// functional axiom (so the saturator's bitset enforces it). Used to accept
/// the `SubClassOf{∃role.⊤, ≤1 role}` GCI emitted by
/// `derive_functional_max_cardinality` without losing the EL fast path.
fn is_derived_functional_max(
    c: ConceptId,
    pool: &ConceptPool,
    functional_roles: &HashSet<Role>,
) -> bool {
    matches!(
        pool.get(c),
        ConceptExpr::Max(1, role, filler)
            if matches!(pool.get(*filler), ConceptExpr::Top)
                && functional_roles.contains(role)
    )
}

fn is_saturator_axiom(
    ax: &Axiom,
    pool: &ConceptPool,
    functional_roles: &HashSet<Role>,
    disjoint_ok: bool,
    bare: &BareRoleDecls,
) -> bool {
    match ax {
        // Bare (semantically inert) symmetry / inverse declaration — see
        // `BareRoleDecls`. Gated by `RUSTDL_FRAGMENT_BARE_DECL` (default ON since 0.4.7);
        // flag-off `unread` is constant-`false`, so this arm falls through to
        // the pre-flag `_ => false`.
        Axiom::SymmetricRole(_) | Axiom::InverseObjectProperties(_, _)
            if is_inert_bare_role_decl(ax, bare) =>
        {
            true
        }
        // Recognize the derived functional-enforcement GCI
        // `∃role.⊤ ⊑ ≤1 role` (role backed by a matching functional axiom) so
        // it does NOT kick the ontology off the fast path. Exact shape only.
        // NOTE: the sub-role and sup-role are not required to be the SAME role
        // — a hypothetical `∃R.⊤ ⊑ ≤1 S` with both R,S functional is still
        // accepted. That is sound: S functional ⇒ the bitset already enforces
        // global `≤1 S` (strictly stronger than the gated form), so the
        // saturator loses nothing. `derive_functional_max_cardinality` only
        // ever emits the same-role shape, so this is a theoretical case.
        Axiom::SubClassOf { sub, sup }
            if is_derived_functional_max(*sup, pool, functional_roles)
                && matches!(
                    pool.get(*sub),
                    ConceptExpr::Some(role, filler)
                        if matches!(pool.get(*filler), ConceptExpr::Top)
                            && functional_roles.contains(role)
                ) =>
        {
            true
        }
        // Lower-`⊥` GCI (Lever 1b): `X ⊑ ⊥`. A CONJUNCTIVE LHS (`A⊓B ⊑ ⊥`) is a
        // disjointness assertion — gate on `disjoint_ok` exactly like a native
        // `DisjointClasses` (the disjoint×functional-merge interaction is
        // unproven, so it must fall back when a functional role is present). A
        // non-conjunctive `A ⊑ ⊥` is a plain single-class unsatisfiability with
        // no such interaction ⇒ always in-fragment. The LHS below `⊥` must still
        // be a saturator concept.
        Axiom::SubClassOf { sub, sup } if matches!(pool.get(*sup), ConceptExpr::Bot) => {
            is_saturator_concept(*sub, pool)
                && (disjoint_ok || !matches!(pool.get(*sub), ConceptExpr::And(_)))
        }
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
        // D10 gate tightening (Bug B): same restriction as `is_el_axiom` — only
        // `Atomic`, `Bot`, and `Top` fillers are handled by the engine; see the
        // comment on `is_el_axiom`'s Domain/Range arms above.
        Axiom::ObjectPropertyDomain { role, domain } => {
            !role.is_inverse() && is_atomic_or_trivial_concept(*domain, pool)
        }
        Axiom::ObjectPropertyRange { role, range } => {
            !role.is_inverse() && is_atomic_or_trivial_concept(*range, pool)
        }
        Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => true,
        // DisjointClasses is complete in the saturator (DisjointnessClash +
        // process_unsat back-prop) on the EL+disjoint-no-functional Horn
        // fragment by construction. Admitted only when no functional /
        // inverse-functional role is present (see saturator_complete_fragment).
        //
        // D10 gate tightening (Bug A): the engine's `disjoint_pairs` collector
        // filters members to `Atomic` only; a non-atomic member is silently dropped,
        // causing missed entailments under the "complete" banner. Require every
        // member to be `Atomic` so the gate matches what the engine actually keeps.
        //
        // DisjointUnion is deliberately EXCLUDED (stays on the hybrid path):
        // (1) DisjointUnion{class, members} entails a disjunctive covering
        //     `class ≡ (member1 ⊔ … ⊔ memberN)` — an Or, which is
        //     out-of-fragment and `is_saturator_concept` rejects.
        // (2) The saturator's rule-builder has no DisjointUnion arm — only
        //     DisjointClasses is registered as disjoint_pairs — so admitting
        //     DisjointUnion would silently drop both the disjointness AND
        //     the covering, causing missed entailments reported as complete
        //     (the D10 unsound-completeness bug class).
        Axiom::DisjointClasses(members) => {
            disjoint_ok && members.iter().all(|c| is_atomic_concept(*c, pool))
        }
        // EXCLUDED ⟹ fall back to the hybrid path. All ABox assertions;
        // InverseObjectProperties decls; Symmetric / Asymmetric / Reflexive /
        // Irreflexive; DisjointObjectProperties; SameIndividual /
        // DifferentIndividuals — none fully reasoned over by the saturator.
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
/// pre-check entirely on `TBox`-only inputs (e.g. GALEN), where
/// building `PreparedOntology` solely to consult `abox_verdict()`
/// is wasted work — the check would early-return `Unknown` on
/// empty `individuals` anyway. Microseconds even on the largest
/// corpus ontologies. See
/// `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`
/// performance contract.
pub(crate) fn has_abox_axioms(internal: &owl_dl_core::ontology::InternalOntology) -> bool {
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

/// Explicit opt-in for an aggregate deadline on the bare per-pair path (see
/// `classify_top_down_internal`): `RUSTDL_AGGREGATE_DEADLINE_MS`, parsed as
/// `u64` milliseconds. When set to a positive value AND the caller supplied
/// no global deadline, it becomes the aggregate wall (`now + value_ms`); `0`
/// or absent leaves the bare path unbounded (the default — no silent cap).
/// Distinct from `RUSTDL_LABEL_CACHE_TIMEOUT_MS`, which scopes only the
/// per-class label-cache build deadline, not this aggregate.
fn aggregate_deadline_env_override() -> Option<u64> {
    std::env::var("RUSTDL_AGGREGATE_DEADLINE_MS")
        .ok()
        .and_then(|s| s.parse().ok())
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
/// Whole milliseconds since `t`, saturating. One place so every phase line
/// item in [`ClassificationStats`] is computed identically.
#[inline]
fn elapsed_ms(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
}

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
    // RSS probe: entry — post-convert_ontology baseline.
    crate::rss_probe::probe("entry");
    let classes: Vec<String> = reportable_class_iris(internal);
    let n = classes.len();
    let index: HashMap<String, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, iri)| (iri.clone(), i))
        .collect();

    // Aggregate-deadline OPT-IN (deadline-honoring fix, 2026-07-12; scoped
    // down in review round 1): the per-pair-only entry points
    // (`classify_top_down_with_timeout` / `classify_with_timeout`) bound each
    // PAIR by `per_pair` but establish no AGGREGATE wall — on a pathological
    // ontology (e.g. `ore_ont_10080`, n=3533) the label-cache build, the tier
    // walk, and the defined-sup sweep can each run unbounded in total,
    // hanging a full-corpus run for 40+ minutes.
    //
    // We deliberately do NOT synthesize an aggregate deadline automatically
    // on this bare path: doing so would silently cap the CLI `classify`
    // default, `owl-dl-bench`, and the closure-diff fixtures, cutting
    // legitimately-slow-but-completable ontologies (e.g. ore-15672, n≈83,
    // historically 29–138 s — an `n × per_pair` estimate of ≈16.6 s would
    // turn its confirmable subsumptions into new MISSED) for a benefit the
    // matrix already gets by explicitly threading `--global-timeout-s`
    // (see `owl-dl-bench` `rustdl_vs_oracle` → `classify_with_budget`).
    // So the BARE path stays exactly as before this branch: unbounded when
    // no global deadline is supplied.
    //
    // `RUSTDL_AGGREGATE_DEADLINE_MS` is an explicit OPT-IN: when set (and
    // non-zero) and no global deadline was supplied by the caller, it becomes
    // the aggregate wall (`now + value_ms`). This makes bounding available
    // without a code change, but never silently on. `0` (or absent) leaves
    // the bare path unbounded. It is intentionally distinct from
    // `RUSTDL_LABEL_CACHE_TIMEOUT_MS`, which scopes only the PER-CLASS
    // label-cache build deadline.
    let global_deadline = match (per_pair_timeout, global_deadline) {
        (Some(_), None) => match aggregate_deadline_env_override() {
            Some(ms) if ms > 0 => Some(classify_start + std::time::Duration::from_millis(ms)),
            _ => None,
        },
        (_, gd) => gd,
    };

    // Deadline-bounded PREP (`RUSTDL_PREP_DEADLINE`, default OFF). Without the
    // flag this is verbatim `saturate(internal)`; with it, and only when a
    // global deadline is actually active, the fixpoint is abandoned at the
    // deadline and the PARTIAL closure is read off as a sound, explicitly
    // INCOMPLETE hierarchy. See `crate::prep_deadline_enabled` for the measured
    // motivation (77 of 252 DNF ontologies burned ≥ 10 s against a 1 ms budget).
    let prep_deadline = if crate::prep_deadline_enabled() {
        global_deadline
    } else {
        None
    };
    let t_saturate = Instant::now();
    let (closure, sat_aborted) = match prep_deadline {
        None => (saturate(internal), false),
        Some(_) => owl_dl_saturation::saturate_with_deadline(internal, prep_deadline),
    };
    // Phase line item, MEASURED (not derived): see the doc comment on
    // `ClassificationStats::tier_walk_wall_ms` for why every phase now carries
    // its own timer instead of one residual absorbing them all.
    let saturate_ms = elapsed_ms(t_saturate);
    // RSS probe: after EL closure / saturation.
    crate::rss_probe::probe("after_saturate");

    if sat_aborted {
        // The fixpoint was abandoned. Every derived edge is still entailed, so
        // read the partial closure off as the answer and flag it incomplete.
        return Ok(classify_prep_timeout(
            internal, &classes, &index, &closure, "saturate",
        ));
    }

    let t_precheck = Instant::now();
    // Sound KB-level inconsistency pre-check (`RUSTDL_CLASSIFY_INCONSISTENCY`,
    // default OFF). Placed BEFORE the fast-path branch so both dispatch arms —
    // the saturation fast path and the hybrid path — are covered by one call
    // site. See `classify_inconsistency_precheck`: it tests that `⊤` is
    // unsatisfiable, NOT that every named class is (the latter is not an
    // inconsistency signal). This is the fix for `classify --json family.ofn`
    // reporting `"consistent": true` where `rustdl consistent` reports
    // `inconsistent`.
    if crate::classify_inconsistency_enabled()
        && crate::classify_inconsistency_precheck(internal, &closure)
    {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("classify: KB inconsistent (pre-check)");
        }
        return Ok(classify_inconsistent(
            classes,
            index,
            analyze_fragment(internal),
        ));
    }

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
        || (crate::classify_tbox_fragment_enabled() && tbox_only_saturator_eligible(internal))
    {
        // Skip the `ABox` check entirely on `ABox`-free inputs — `abox_check`
        // itself would early-return `Unknown` on empty `individuals` (no
        // clash is possible without individuals, by construction). The
        // `has_abox_axioms` scan below is an O(n) walk over the axiom list,
        // microseconds even on GALEN.
        if crate::abox_check_enabled() && has_abox_axioms(internal) {
            // Build ONLY what abox_check reads, reusing the closure the caller
            // already computed — the full `PreparedOntology` built here previously
            // was discarded immediately (this branch either returns
            // `classify_inconsistent` or falls through to `classify_pure_el`, and
            // neither uses it). Measured 0.62 s / 185 MB on `ore_ont_1043`.
            let owned = crate::build_abox_check_inputs(internal);
            let verdict = crate::abox_check::check(&owned.as_inputs(&closure));
            if let crate::abox_check::AboxVerdict::Inconsistent { reason } = &verdict {
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
        let precheck_ms = elapsed_ms(t_precheck);
        let mut h = classify_pure_el(
            internal,
            &classes,
            &index,
            &closure,
            analyze_fragment(internal),
        );
        // The fast path runs only two of the phases, but it must still report
        // them: before this it printed an all-zero breakdown, so a pure-EL run
        // that spent 15 s in saturation looked like it spent nothing anywhere.
        h.stats.saturate_wall_ms = saturate_ms;
        h.stats.precheck_wall_ms = precheck_ms;
        h.stats.unattributed_wall_ms = elapsed_ms(classify_start)
            .saturating_sub(saturate_ms)
            .saturating_sub(precheck_ms);
        return Ok(h);
    }

    let precheck_ms = elapsed_ms(t_precheck);

    // RSS probes: bracket PreparedOntology::from_internal — before/after delta
    // directly answers whether the snapshot is a large allocation.
    crate::rss_probe::probe("before_prepared");
    // Second half of the prep-deadline fix: `from_internal` was the single
    // largest unbudgeted phase (26 of the 252 DNF ontologies never finished it
    // at all). `from_internal_with_deadline(.., None)` is the pre-change call.
    let t_prepare = Instant::now();
    let Some(prepared) =
        PreparedOntology::from_internal_with_deadline(internal.clone(), prep_deadline)?
    else {
        return Ok(classify_prep_timeout(
            internal,
            &classes,
            &index,
            &closure,
            "from_internal",
        ));
    };
    crate::rss_probe::probe("after_prepared");

    // Sound ABox-driven inconsistency pre-check. If it fires, return
    // an every-class-unsatisfiable Classification (mirroring Konclude).
    // `abox_verdict()` is `get_or_init`-lazy, so the FIRST call is where the
    // check actually runs — it belongs inside the prepare line item, which is
    // why `t_prepare` is not stopped until after it.
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

    let prepare_ms = elapsed_ms(t_prepare);

    // Per-class unsat probes — identical to the naive path. Reuse
    // the same parallel pattern.
    let mut stats = ClassificationStats {
        fragment: analyze_fragment(internal),
        saturate_wall_ms: saturate_ms,
        precheck_wall_ms: precheck_ms,
        prepare_wall_ms: prepare_ms,
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
        // Adaptive build-once deadline (2026-06-25): scale to n × per_pair, clamped
        // [1s,30s]; env RUSTDL_LABEL_CACHE_TIMEOUT_MS overrides (0 = unbounded).
        let cache_ms =
            crate::adaptive_label_cache_ms(n, per_pair_timeout, crate::label_cache_env_override());
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
    // RSS probe: after label-cache build.
    crate::rss_probe::probe("after_label_cache");

    let t_unsat_probe = Instant::now();
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
                    Some(crate::LabelOracle::Sat { .. }) => {
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
                    .decide_classify_with_deadline(deadline, move |pool| pool.atomic(class_id))
                {
                    Ok(Some(s)) => s,
                    Ok(None) | Err(crate::ReasonError::NoVerdict) => true,
                    Err(other) => return Err(other),
                };
                Ok((i, sat, false))
            } else {
                let sat = prepared.decide_classify(move |pool| pool.atomic(class_id))?;
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
    stats.unsat_probe_wall_ms = elapsed_ms(t_unsat_probe);

    // Compute closure-subsumer counts once per class (used for sort key and
    // tier grouping). `subsumers_count` is O(1) (no Vec allocation) vs
    // `subsumers_of` (allocates Vec<ClassId> per call). sort_by_key calls the
    // key fn O(n log n) times; pre-computing avoids repeated allocations.
    let subsumer_counts: Vec<usize> = (0..n)
        .map(|i| {
            let id = owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            closure.subsumers_count(id)
        })
        .collect();

    // Sort the satisfiable classes by ascending closure-subsumer
    // count — "most general first". This ordering means when we
    // place class `c`, every class that could be `c`'s parent has
    // already been placed (modulo same-tier siblings, which are
    // handled by the walk's iterative refinement).
    let mut order: Vec<usize> = (0..n).filter(|i| !unsatisfiable_idxs.contains(i)).collect();
    order.sort_by_key(|&i| subsumer_counts[i]);

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
    // Reuse the pre-computed `subsumer_counts` — no additional subsumers_of calls.
    let mut tiers: Vec<Vec<usize>> = Vec::new();
    {
        let mut current: Vec<usize> = Vec::new();
        let mut current_rank: Option<usize> = None;
        for &c in &order {
            let rank = subsumer_counts[c];
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

    // Phase 2: classes whose subsumption pairs must be counting-verified —
    // a data-counting class, or one with a counting subsumer. Empty (the
    // whole corpus) ⇒ the guard in `subsumes_via_tableau` never fires.
    let counting_relevant: std::collections::HashSet<owl_dl_core::ClassId> =
        if !crate::counting_pair_verify_enabled() || prepared.data_counting_classes.is_empty() {
            std::collections::HashSet::new()
        } else {
            (0..n)
                .map(|i| {
                    owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"))
                })
                .filter(|&c| {
                    prepared.data_counting_classes.contains(&c)
                        || closure
                            // `subsumers_of` is the EL closure (under-approximate): a
                            // missing subsumer only SKIPS a counting pair (sound MISS),
                            // never causes a false subsumption.
                            .subsumers_of(c)
                            .iter()
                            .any(|s| prepared.data_counting_classes.contains(s))
                })
                .collect()
        };

    // RSS probe: pair counter for the tier walk.  Incremented in the serial
    // merge loop (post-rayon collect) to avoid interleaved output from rayon
    // worker threads.  We chose the serial-merge site rather than the rayon
    // closure because: (a) output ordering is deterministic, (b) we want a
    // coarse sawtooth that reflects allocated-then-freed per unit of work, and
    // (c) the atomic overhead per class in rayon workers would be visible noise.
    let mut rss_pair_counter: u64 = 0;
    // THE fix for the mis-attribution: `tier_walk_wall_ms` is now this interval,
    // not `total − (label_cache + snapshot_build + snapshot_replay)`.
    let t_tier_walk = Instant::now();
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
                    &counting_relevant,
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
            stats.diverged_tail_skips += sd.diverged_tail_skips;
            stats.fallthrough_ran += sd.fallthrough_ran;
            stats.fallthrough_subsumed += sd.fallthrough_subsumed;
            stats.fallthrough_notsubsumed += sd.fallthrough_notsubsumed;
            stats.fallthrough_noverdict += sd.fallthrough_noverdict;
            stats.fallthrough_from_diverged += sd.fallthrough_from_diverged;
            stats.fallthrough_subsumed_diverged += sd.fallthrough_subsumed_diverged;
            stats.timed_out_pairs += sd.timed_out_pairs;
            stats
                .timed_out_pair_ids
                .extend(sd.timed_out_pair_ids.iter().copied());
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
            stats.counting_verified_pairs += sd.counting_verified_pairs;
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
            // RSS pair probe: emit every RUSTDL_TRACE_RSS_EVERY classes (default
            // 100).  Placed at the end of the serial-merge body so the counter
            // tracks completed classes, not started ones.
            rss_pair_counter += 1;
            crate::rss_probe::probe_pair(rss_pair_counter);
        }
    }
    stats.tier_walk_wall_ms = elapsed_ms(t_tier_walk);

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
    let t_sweeps = Instant::now();
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
    // SP1.1 Layer B: broaden the sweep's sup-side from defined-classes-only to
    // LABEL-DRIVEN. With the classify oracle now hierarchy-aware (Layer A),
    // `labels(cand)` includes inverse/symmetric-domain-derived subsumers, so any
    // such sup appears in some label set. Union those in; the existing
    // label-gated sweep body then tests them (a sup in no label set adds zero
    // oracle calls). Sound: the sweep only ADDS candidate pairs; every recorded
    // subsumption is oracle-confirmed and the label gate only prunes.
    // DEFAULT OFF (RUSTDL_CLASSIFY_SAME_TIER=1 to enable): corpus-invisible gain
    // at ~2× wall cost. When off, sweep_sups == defined_sups (pre-SP1.1 behavior).
    let sweep_sups: Vec<usize> = if crate::classify_same_tier_enabled() {
        let mut set: std::collections::HashSet<usize> = defined_sups.iter().copied().collect();
        for oracle in &label_cache {
            if let crate::LabelOracle::Sat { labels, .. } = oracle {
                for &sup_id in labels {
                    let i = sup_id.index() as usize;
                    if i < n && !unsatisfiable_idxs.contains(&i) {
                        set.insert(i);
                    }
                }
            }
        }
        set.into_iter().collect()
    } else {
        defined_sups.clone()
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
    // Opt-in defined-sup VERIFY mode (RUSTDL_CLASSIFY_DEFINED_SWEEP): for a class
    // defined via a non-EL body, the wedge's label countermodel is an unreliable
    // counterexample, so the label prune drops true `cand ⊑ D`. When on, bypass the
    // label gate for these sups and verify via full tableau (trust_sat=false).
    // Sound (FP=0): only tableau-confirmed edges added. See classify_defined_sweep_enabled.
    let defined_verify_mode = crate::classify_defined_sweep_enabled();
    let defined_set: std::collections::HashSet<usize> = defined_sups.iter().copied().collect();
    for &sup in &sweep_sups {
        // Global-deadline bail (BEFORE the O(n) candidate enumeration below):
        // once the aggregate deadline passes, abandon the rest of the sweep.
        // The old code `continue`d through every remaining sup — enumerating
        // O(n) candidates each (O(n²) total) and materializing up to O(n²)
        // undecided-pair ids into `timed_out_pair_ids` — a multi-minute /
        // multi-GB pathology on large out-of-EL ontologies (DL-approximated
        // SNOMED `ore_ont_3215`: 3.3B pairs / ~26 GB / +227 s past a 90 s
        // budget). Bail instead, recording the unswept sups so the hierarchy is
        // flagged INCOMPLETE (`completeness_guaranteed()` false; the CLI warning
        // fires). `undecided_pairs()` is then a lower bound, not an exhaustive
        // enumeration. Sound: the sweep only ADDS oracle-confirmed edges, so
        // abandoning it only omits subsumptions — a MISS at worst, never an FP.
        if global_deadline.is_some_and(|gd| Instant::now() >= gd) {
            // One marker (invariant-safe: +1 count, +1 id) then bail — do NOT
            // enumerate O(n) candidates per remaining sup.
            let s = u32::try_from(sup).expect("class index fits in u32");
            stats.timed_out_pairs += 1;
            stats.timed_out_pair_ids.push((s, s));
            break;
        }
        let sup_verify = defined_verify_mode && defined_set.contains(&sup);
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
                // Phase 7 label-heuristic gate — same logic as the tier
                // walk's `find_direct_parents_top_down` inner loop.
                // Gate on `labels(cand)` membership of `sup_id`:
                //   Sat(labels) + sup_id ∉ labels → prune (sound
                //     counterexample model; same oracle the wedge built).
                //   Sat(labels) + sup_id ∈ labels → fall through to
                //     subsumes_via_tableau (may be coincidence of model).
                //   Unsat → vacuously subsumed (cand filtered by
                //     unsatisfiable_idxs above, so this branch is
                //     near-dead, but mirror for faithfulness).
                //   NoVerdict | None → fall through (oracle incomplete;
                //     wine's oracle is mostly NoVerdict, so wine's
                //     verdicts are unchanged). RUSTDL_LABEL_HEURISTIC=0
                //     makes the cache all-NoVerdict → full fall-through
                //     (free opt-out, no new flag needed).
                let subsumed = if sup_verify {
                    // VERIFY mode (opt-in): the label countermodel is unreliable for
                    // this non-EL-defined sup, so skip the label prune and verify with
                    // the full tableau (trust_sat=false). Sound — only a tableau `unsat`
                    // yields `true`; a spurious wedge `Sat` times out / returns false
                    // (a MISS, never an FP).
                    local_stats.label_cache_misses += 1;
                    subsumes_via_tableau(
                        &prepared,
                        cand_id,
                        sup_id,
                        Some(sweep_budget),
                        global_deadline,
                        false,
                        &counting_relevant,
                        &mut local_stats,
                    )
                    .ok()
                    .flatten()
                    .unwrap_or(false)
                } else {
                    match label_cache.get(cand) {
                        Some(crate::LabelOracle::Sat { labels, .. }) => {
                            if labels.contains(&sup_id) {
                                // sup_id ∈ labels: might be coincidence of
                                // model; verify via subsumes_via_tableau.
                                local_stats.label_cache_pass_through += 1;
                                subsumes_via_tableau(
                                    &prepared,
                                    cand_id,
                                    sup_id,
                                    Some(sweep_budget),
                                    global_deadline,
                                    true,
                                    &counting_relevant,
                                    &mut local_stats,
                                )
                                .ok()
                                .flatten()
                                .unwrap_or(false)
                            } else {
                                // sup_id ∉ labels: sound non-subsumption.
                                local_stats.label_cache_pruned += 1;
                                false
                            }
                        }
                        Some(crate::LabelOracle::Unsat) => {
                            // cand is unsatisfiable: vacuously subsumed.
                            true
                        }
                        Some(crate::LabelOracle::NoVerdict) | None => {
                            // Oracle incomplete — fall through to per-pair.
                            local_stats.label_cache_misses += 1;
                            subsumes_via_tableau(
                                &prepared,
                                cand_id,
                                sup_id,
                                Some(sweep_budget),
                                global_deadline,
                                true,
                                &counting_relevant,
                                &mut local_stats,
                            )
                            .ok()
                            .flatten()
                            .unwrap_or(false)
                        }
                    }
                };
                (cand, subsumed, local_stats)
            })
            .collect();
        for (cand, subsumed, sd) in probe_results {
            stats.saturation_subsumption_hits += sd.saturation_subsumption_hits;
            stats.tableau_subsumption_calls += sd.tableau_subsumption_calls;
            stats.diverged_tail_skips += sd.diverged_tail_skips;
            stats.fallthrough_ran += sd.fallthrough_ran;
            stats.fallthrough_subsumed += sd.fallthrough_subsumed;
            stats.fallthrough_notsubsumed += sd.fallthrough_notsubsumed;
            stats.fallthrough_noverdict += sd.fallthrough_noverdict;
            stats.fallthrough_from_diverged += sd.fallthrough_from_diverged;
            stats.fallthrough_subsumed_diverged += sd.fallthrough_subsumed_diverged;
            stats.timed_out_pairs += sd.timed_out_pairs;
            stats
                .timed_out_pair_ids
                .extend(sd.timed_out_pair_ids.iter().copied());
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
            stats.counting_verified_pairs += sd.counting_verified_pairs;
            stats.snapshot_replay_used += sd.snapshot_replay_used;
            stats.snapshot_replay_subsumed += sd.snapshot_replay_subsumed;
            stats.snapshot_replay_not_subsumed += sd.snapshot_replay_not_subsumed;
            stats.snapshot_replay_aborts += sd.snapshot_replay_aborts;
            stats.snapshot_cache_falls_through += sd.snapshot_cache_falls_through;
            stats.label_cache_pruned += sd.label_cache_pruned;
            stats.label_cache_pass_through += sd.label_cache_pass_through;
            stats.label_cache_misses += sd.label_cache_misses;
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

    // Label-cache back-fold injection (Task 3, `RUSTDL_CLASSIFY_BACKFOLD`,
    // default OFF): consume `LabelOracle::Sat::derived_sups` — the entailed
    // defined-`∃` names `HyperEngine::backfold_derived` (Task 2) proved over
    // the branch-free, merge-enriched `sat(c)` graph — mirroring the
    // defined-SUB sweep directly above: a direct `direct_supers`/
    // `direct_children` push, **no `subsumes_via_tableau` call**. Must run
    // after `direct_supers`/`direct_children` are populated (tier walk +
    // both sweeps, above) and before the entailment-matrix BFS below, so the
    // BFS transitively propagates the injected edge like any other direct
    // super. See
    // `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md` §4.2/§7.5.
    inject_backfold_derived_sups(
        &label_cache,
        &closure,
        n,
        &mut direct_supers,
        &mut direct_children,
        &mut stats,
    );
    stats.sweep_wall_ms = elapsed_ms(t_sweeps);

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
    let t_matrix = Instant::now();
    let mut entailed = EntailmentMatrix::new(n);
    // Reused BFS-visited buffer with per-row generation stamps: `visited_gen[j]
    // == gen` means "visited this row". Bumping `gen` resets it in O(1), avoiding
    // the O(n²) `vec![false; n]`-per-class allocation (fatal on 55k-class onts).
    let mut visited_gen = vec![0u32; n];
    let mut cur_gen: u32 = 0;
    for i in 0..n {
        if unsatisfiable_idxs.contains(&i) {
            // Row elided — `Classification::entails` supplies ⊥ ⊑ *.
            continue;
        }
        entailed.insert(i, i);
        // Closure seed.
        let i_id = owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        for j_id in closure.subsumers_of(i_id) {
            let j = j_id.index() as usize;
            if j < n {
                entailed.insert(i, j);
            }
        }
        // BFS over direct_supers starting from `i` to pick up the
        // tableau-derived transitive closure. The generation stamp descends
        // through each reached node exactly once — row `i` may already record
        // `j` from the closure seed above, but we still follow `direct_supers[j]`
        // to catch tableau-only ancestors of `j` off `i`'s closure ray.
        cur_gen += 1;
        let mut frontier: Vec<usize> = direct_supers[i].clone();
        while let Some(j) = frontier.pop() {
            if visited_gen[j] == cur_gen {
                continue;
            }
            visited_gen[j] = cur_gen;
            entailed.insert(i, j);
            for &k in &direct_supers[j] {
                if visited_gen[k] != cur_gen {
                    frontier.push(k);
                }
            }
        }
    }

    stats.matrix_wall_ms = elapsed_ms(t_matrix);

    let _ = top_level; // currently informational only

    // Snapshot timers are NESTED inside the label-cache / tier-walk phases (they
    // accumulate inside `decide` calls), so they are reported but deliberately
    // EXCLUDED from the phase sum below — subtracting them, as the old residual
    // `tier_walk_wall_ms` did, double-counted them.
    stats.snapshot_cache_build_wall_ms = prepared.snapshot_cache_build_wall_ms();
    stats.snapshot_replay_wall_ms = prepared.snapshot_cache_replay_wall_ms();
    // Phase 3a recon: per-class BackPropRisk diagnostic counts. Pure
    // instrumentation; does not affect the snapshot cache gate.
    stats.per_class_safe_count = prepared.per_class_safe_count();
    stats.per_class_unsafe_count = prepared.per_class_unsafe_count();
    // The leftover is NAMED (`unattributed_wall_ms`) instead of being folded into
    // a phase. It covers the class-IRI/index build, `analyze_fragment`, tier
    // grouping and the `Classification` assembly. A large value here means a
    // phase is missing a timer — which is precisely what the old residual
    // `tier_walk_wall_ms` concealed (`ore_ont_1028`: 7198 ms reported for an
    // 80 ms tier walk).
    let total_wall = elapsed_ms(classify_start);
    stats.unattributed_wall_ms = total_wall
        .saturating_sub(stats.saturate_wall_ms)
        .saturating_sub(stats.precheck_wall_ms)
        .saturating_sub(stats.prepare_wall_ms)
        .saturating_sub(stats.label_cache_build_wall_ms)
        .saturating_sub(stats.unsat_probe_wall_ms)
        .saturating_sub(stats.tier_walk_wall_ms)
        .saturating_sub(stats.sweep_wall_ms)
        .saturating_sub(stats.matrix_wall_ms);

    Ok(Classification {
        classes,
        index,
        entailed,
        unsatisfiable_idxs,
        stats,
        direct_index: std::sync::OnceLock::new(),
    })
}

/// Task 3 (label-cache back-fold): inject the entailed
/// [`crate::LabelOracle::Sat::derived_sups`] into the class hierarchy.
///
/// `derived_sups(c)` are names `HyperEngine::backfold_derived` (Task 2)
/// proved ENTAILED — not candidates — over `c`'s branch-free,
/// merge-enriched `sat` graph (see
/// `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md`
/// §1.2/§2). So, exactly like the defined-SUB sweep above
/// (`stats.defined_sub_sweep_recovered`), each is injected directly as a
/// `direct_supers`/`direct_children` edge — **no `subsumes_via_tableau`
/// call** — guarded only by dedup (`closure.contains` / already a direct
/// super). Must be called after `direct_supers`/`direct_children` are
/// populated by the tier walk + both sweeps, and before the
/// entailment-matrix transitive-closure BFS, so the BFS propagates the
/// injected edge like any other direct super.
///
/// A no-op when [`crate::classify_backfold_enabled`] is off (opt-out via
/// `RUSTDL_CLASSIFY_BACKFOLD=0`, default is ON since 2026-07-12): the
/// hierarchy is byte-identical to pre-Task-3 behaviour.
fn inject_backfold_derived_sups(
    label_cache: &[crate::LabelOracle],
    closure: &owl_dl_saturation::Subsumers,
    n: usize,
    direct_supers: &mut [Vec<usize>],
    direct_children: &mut [Vec<usize>],
    stats: &mut ClassificationStats,
) {
    if !crate::classify_backfold_enabled() {
        return;
    }
    for (c, oracle) in label_cache.iter().enumerate() {
        // Defensive: `direct_supers`/`direct_children` are indexed by class id in
        // `0..n`; the loop should never see `c >= n` (label_cache.len() == n), but
        // guard so a broken invariant is a skip, never an index panic.
        if c >= n {
            continue;
        }
        let crate::LabelOracle::Sat { derived_sups, .. } = oracle else {
            continue;
        };
        if derived_sups.is_empty() {
            continue;
        }
        let c_id = owl_dl_core::ClassId::new(u32::try_from(c).expect("class index fits in u32"));
        for &d_id in derived_sups {
            let d = d_id.index() as usize;
            if d >= n || d == c {
                continue;
            }
            if closure.contains(c_id, d_id) || direct_supers[c].contains(&d) {
                continue;
            }
            stats.backfold_recovered += 1;
            direct_supers[c].push(d);
            direct_children[d].push(c);
        }
    }
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
    counting_relevant: &std::collections::HashSet<owl_dl_core::ClassId>,
    stats: &mut ClassificationStats,
) -> Result<Vec<usize>, ReasonError> {
    let c_id = owl_dl_core::ClassId::new(u32::try_from(c).expect("class index fits in u32"));
    let n = direct_supers.len();
    // Global-deadline fast-exit BEFORE cloning `top_level`: under a tight budget
    // most classes stay unplaced/top-level, so `top_level` can be ~n; cloning it
    // per class (`to_vec()` below) would be O(n²) even though every post-deadline
    // walk immediately bails. Return one (c,c) marker (invariant-safe: +1 count,
    // +1 id) to flag `c` incomplete. Sound — the partial hierarchy only omits.
    if global_deadline.is_some_and(|gd| Instant::now() >= gd) {
        let cu = u32::try_from(c).expect("class index fits in u32");
        stats.timed_out_pairs += 1;
        stats.timed_out_pair_ids.push((cu, cu));
        return Ok(Vec::new());
    }
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
        // Global-deadline bail: once the budget expires, abandon the REST of
        // `c`'s top-down walk instead of recording every remaining (c,d')
        // candidate. The old `continue` recorded one undecided pair per
        // frontier candidate — O(n) per class × n classes = O(n²) pairs
        // materialized into `timed_out_pair_ids` (ore_ont_3215: 1.3B pairs /
        // 23 GB / minutes past the deadline). Record ONE real undecided pair
        // (c,d) and `break`: bounded to ≤ n markers, keeps the
        // `undecided_pairs().len() == timed_out_pairs` invariant, flags the
        // hierarchy INCOMPLETE. Sound — the partial result only omits.
        if global_deadline.is_some_and(|gd| Instant::now() >= gd) {
            stats.timed_out_pairs += 1;
            stats.timed_out_pair_ids.push((
                u32::try_from(c).expect("class index fits in u32"),
                u32::try_from(d).expect("class index fits in u32"),
            ));
            break;
        }
        let d_id = owl_dl_core::ClassId::new(u32::try_from(d).expect("class index fits in u32"));
        let subsumed = if closure.contains(c_id, d_id) {
            stats.saturation_subsumption_hits += 1;
            true
        } else {
            // Phase 7: per-class label heuristic — check the cache
            // before paying for `subsumes_via_tableau`.
            match label_cache.get(c) {
                Some(crate::LabelOracle::Sat { labels, .. }) => {
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
                            counting_relevant,
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
                        counting_relevant,
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

/// Phase 0 bound-the-tail exploration (diagnostic): record the main-tableau
/// outcome of a wedge STALL fallthrough. `outcome`: `Some(true)` = subsumed
/// (rescue), `Some(false)` = not-subsumed, `None` = no-verdict/timeout.
fn record_fallthrough_outcome(
    stats: &mut ClassificationStats,
    stall_fallthrough: bool,
    stall_diverged: bool,
    outcome: Option<bool>,
) {
    if !stall_fallthrough {
        return;
    }
    match outcome {
        Some(true) => {
            stats.fallthrough_subsumed += 1;
            if stall_diverged {
                stats.fallthrough_subsumed_diverged += 1;
            }
        }
        Some(false) => stats.fallthrough_notsubsumed += 1,
        None => stats.fallthrough_noverdict += 1,
    }
}

/// Helper: ask the tableau whether `sub ⊑ sup`. Counts the call in
/// `stats`, honours `per_pair_timeout`, returns:
/// - `Ok(Some(true))` — subsumption holds
/// - `Ok(Some(false))` — refuted (sat verdict on `sub ⊓ ¬sup`)
/// - `Ok(None)` — timed out (counted as `timed_out_pairs`)
#[allow(clippy::too_many_arguments)]
fn subsumes_via_tableau(
    prepared: &PreparedOntology,
    sub: owl_dl_core::ClassId,
    sup: owl_dl_core::ClassId,
    per_pair_timeout: Option<std::time::Duration>,
    global_deadline: Option<Instant>,
    trust_sat: bool,
    counting_relevant: &std::collections::HashSet<owl_dl_core::ClassId>,
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
    let mut counting_verified = false;
    match verdict {
        crate::HyperVerdict::Subsumed => {
            stats.hyper_proven_pairs += 1;
            return Ok(Some(true));
        }
        crate::HyperVerdict::NotSubsumed if trust_sat && crate::hyper_trust_sat_enabled() => {
            // Phase 2: counting-pair verification. If either side is
            // data-counting-relevant, the wedge `NotSubsumed` is untrusted
            // (the wedge has no `card_sat`); fall through to the main
            // tableau, which runs `concrete_domain_clash`. Sound: only
            // swaps a trusted wedge `Sat` for the complete path.
            if counting_relevant.contains(&sub) || counting_relevant.contains(&sup) {
                counting_verified = true;
                // fall through to the tableau probe (no early return).
            } else {
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
        }
        crate::HyperVerdict::UnknownDiverged if crate::bound_diverged_tail_enabled() => {
            // Bound-the-tail: the wedge diverged (thrashed at saturated depth) on
            // this pair; the main-tableau fallthrough would re-thrash it and
            // default to "not subsumed" anyway. Skip it. Sound: only ever yields
            // "not subsumed" (a MISS at worst — never an FP). Counted as a
            // timed-out pair so the INCOMPLETE banner stays honest.
            stats.diverged_tail_skips += 1;
            stats.timed_out_pairs += 1;
            stats.timed_out_pair_ids.push((sub.index(), sup.index()));
            return Ok(None);
        }
        _ => {}
    }
    // Phase 0 rescue-rate instrumentation (diagnostic): does the main-tableau
    // fallthrough RESCUE a wedge STALL (return Subsumed)? A stall fallthrough is
    // a wedge Unknown/UnknownDiverged reaching the tableau (NOT a fast-refute /
    // counting-verify, which legitimately need the tableau). Split by divergence.
    let stall_fallthrough = matches!(
        verdict,
        crate::HyperVerdict::Unknown | crate::HyperVerdict::UnknownDiverged
    );
    let stall_diverged = matches!(verdict, crate::HyperVerdict::UnknownDiverged);
    if stall_fallthrough {
        stats.fallthrough_ran += 1;
        if stall_diverged {
            stats.fallthrough_from_diverged += 1;
        }
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
            let sat = prepared.decide_classify(build)?;
            stats.tableau_subsumption_calls += 1;
            let subsumed = !sat;
            if was_fast_refuted && subsumed {
                stats.hyper_refuted_fast_flipped_pairs += 1;
            }
            if counting_verified && subsumed {
                stats.counting_verified_pairs += 1;
            }
            record_fallthrough_outcome(stats, stall_fallthrough, stall_diverged, Some(subsumed));
            Ok(Some(subsumed))
        }
        Some(deadline) => {
            // Robustness: a `ReasonError::NoVerdict` (tableau internal
            // cap, e.g. on large workloads like SIO) is treated as a
            // sound timeout — the pair defaults to "not subsumed"
            // (sound under-approximation), counted in `timed_out_pairs`.
            // Crashing classify on a single oversized pair is worse
            // than under-reporting the subsumption.
            match prepared.decide_classify_with_deadline(deadline, build) {
                Ok(Some(sat)) => {
                    stats.tableau_subsumption_calls += 1;
                    let subsumed = !sat;
                    if was_fast_refuted && subsumed {
                        stats.hyper_refuted_fast_flipped_pairs += 1;
                    }
                    if counting_verified && subsumed {
                        stats.counting_verified_pairs += 1;
                    }
                    record_fallthrough_outcome(
                        stats,
                        stall_fallthrough,
                        stall_diverged,
                        Some(subsumed),
                    );
                    Ok(Some(subsumed))
                }
                Ok(None) | Err(crate::ReasonError::NoVerdict) => {
                    stats.timed_out_pairs += 1;
                    stats.timed_out_pair_ids.push((sub.index(), sup.index()));
                    record_fallthrough_outcome(stats, stall_fallthrough, stall_diverged, None);
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

    // ───────────────────────────────────────────────────────────────────────
    // `RUSTDL_FRAGMENT_BARE_DECL` — bare symmetry / inverse declarations.
    //
    // NEGATIVES FIRST. Admitting a declaration whose role IS read anywhere is
    // the D10 unsound-completeness bug (gate certifies the closure complete
    // while the saturator silently drops the axiom), so most of these tests
    // assert REFUSAL. `saturator_drops_symmetry_so_the_gate_must_refuse` is the
    // "why": it exhibits an entailment the saturator provably misses.
    // ───────────────────────────────────────────────────────────────────────

    /// The saturation fast path is taken iff any of the three gates admits.
    fn gate_admits(src: &str) -> bool {
        let onto = parse(src);
        let internal = convert_ontology(&onto).expect("fixture converts");
        is_pure_el(&internal)
            || saturator_complete_fragment(&internal)
            || tbox_only_saturator_eligible(&internal)
    }

    /// WHY the gate must refuse an *observable* symmetric role: the EL
    /// saturator has no symmetry rule, so on `Symmetric(r)` + `Range(r, E)` +
    /// `A ⊑ ∃r.B` + `Disjoint(A, E)` — where the backward edge `r(y, x)` puts
    /// `x` in `E`, making `A` unsatisfiable — the saturation closure reports
    /// nothing while the tableau correctly reports `A` unsat. Admitting such an
    /// ontology to the fast path would publish the saturator's miss as a
    /// complete answer.
    #[test]
    fn saturator_drops_symmetry_so_the_gate_must_refuse() {
        let src = format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:E))\n\
    Declaration(ObjectProperty(:r))\n\
    SymmetricObjectProperty(:r)\n\
    ObjectPropertyRange(:r :E)\n\
    DisjointClasses(:A :E)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
)\n"
        );
        let onto = parse(&src);
        // Ground truth from the complete engine.
        assert!(
            !crate::is_class_satisfiable(&onto, "http://rustdl.test/A")
                .expect("satisfiability check"),
            "fixture is wrong: :A must be unsatisfiable via the symmetric back-edge"
        );
        // The saturator misses it — hence the gate must never certify it.
        let sat_only = classify_saturation_only(&onto).expect("saturation-only");
        assert!(
            sat_only.unsatisfiable_classes().is_empty(),
            "saturator unexpectedly derived the symmetry-driven unsat; \
             if it grew a symmetry rule, revisit BareRoleDecls"
        );
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(
            !gate_admits(&src),
            "D10 BUG: gate admitted an ontology whose symmetric role is read by \
             ObjectPropertyRange and an existential"
        );
    }

    /// Role read by an existential concept ⟹ observable ⟹ refuse.
    #[test]
    fn bare_decl_gate_rejects_symmetric_role_used_in_a_concept() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SymmetricObjectProperty(:r)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
)\n"
        )));
    }

    /// Role read by `ObjectPropertyDomain` ⟹ observable ⟹ refuse (the backward
    /// edge would type the successor into the domain class).
    #[test]
    fn bare_decl_gate_rejects_symmetric_role_with_domain() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    SymmetricObjectProperty(:r)\n\
    ObjectPropertyDomain(:r :D)\n\
)\n"
        )));
    }

    /// Observability propagates DOWN the property hierarchy: `r ⊑ s` with `s`
    /// read (here by a concept) makes `r`'s edges readable through `s`.
    #[test]
    fn bare_decl_gate_rejects_symmetric_role_below_an_observable_superrole() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SymmetricObjectProperty(:r)\n\
    SubObjectPropertyOf(:r :s)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:s :B))\n\
)\n"
        )));
    }

    /// A chain PART matches existing edges ⟹ observable ⟹ refuse.
    #[test]
    fn bare_decl_gate_rejects_symmetric_role_used_as_a_chain_part() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:t))\n\
    Declaration(ObjectProperty(:u))\n\
    SymmetricObjectProperty(:r)\n\
    SubObjectPropertyOf(ObjectPropertyChain(:r :t) :u)\n\
)\n"
        )));
    }

    /// An `ABox` edge is read by the `ABox` machinery ⟹ observable ⟹ refuse.
    #[test]
    fn bare_decl_gate_rejects_symmetric_role_with_an_abox_assertion() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(NamedIndividual(:a))\n\
    Declaration(NamedIndividual(:b))\n\
    Declaration(ObjectProperty(:r))\n\
    SymmetricObjectProperty(:r)\n\
    ObjectPropertyAssertion(:r :a :b)\n\
)\n"
        )));
    }

    /// `InverseObjectProperties(p, q)` needs BOTH sides unread — one used side
    /// makes the equality observable.
    #[test]
    fn bare_decl_gate_rejects_inverse_pair_when_one_side_is_used() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:p))\n\
    Declaration(ObjectProperty(:q))\n\
    InverseObjectProperties(:p :q)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:q :B))\n\
)\n"
        )));
    }

    /// DEFAULT-OFF CONTROL: with the variable UNSET the feature is inert. (The
    /// `=0` control below cannot see a flipped default, so this test is what
    /// pins the default itself.)
    #[test]
    #[allow(unsafe_code)]
    fn fast_direct_subsumers_flag_defaults_on() {
        let _lock = crate::test_env_lock();
        let prev = std::env::var_os("RUSTDL_FAST_DIRECT_SUBSUMERS");
        unsafe { std::env::remove_var("RUSTDL_FAST_DIRECT_SUBSUMERS") };
        let unset = fast_direct_subsumers_enabled();
        unsafe { std::env::set_var("RUSTDL_FAST_DIRECT_SUBSUMERS", "0") };
        let off = fast_direct_subsumers_enabled();
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_FAST_DIRECT_SUBSUMERS", v) },
            None => unsafe { std::env::remove_var("RUSTDL_FAST_DIRECT_SUBSUMERS") },
        }
        assert!(
            unset,
            "RUSTDL_FAST_DIRECT_SUBSUMERS must default ON since 0.4.7"
        );
        assert!(!off, "RUSTDL_FAST_DIRECT_SUBSUMERS=0 must still revert");
    }

    #[test]
    #[allow(unsafe_code)]
    fn bare_decl_flag_defaults_on() {
        let _lock = crate::test_env_lock();
        let prev = std::env::var_os("RUSTDL_FRAGMENT_BARE_DECL");
        unsafe { std::env::remove_var("RUSTDL_FRAGMENT_BARE_DECL") };
        let unset = crate::fragment_bare_decl_enabled();
        unsafe { std::env::set_var("RUSTDL_FRAGMENT_BARE_DECL", "0") };
        let off = crate::fragment_bare_decl_enabled();
        match prev {
            Some(v) => unsafe { std::env::set_var("RUSTDL_FRAGMENT_BARE_DECL", v) },
            None => unsafe { std::env::remove_var("RUSTDL_FRAGMENT_BARE_DECL") },
        }
        // Default flipped ON in 0.4.7. Both halves matter: an UNSET variable must
        // enable, and `=0` must still revert -- a flag whose opt-out silently stopped
        // working would leave no way back to the prior behaviour.
        assert!(
            unset,
            "RUSTDL_FRAGMENT_BARE_DECL must default ON since 0.4.7"
        );
        assert!(!off, "RUSTDL_FRAGMENT_BARE_DECL=0 must still revert");
    }

    /// FLAG-OFF CONTROL: even a provably unread declaration keeps the ontology
    /// off the fast path, i.e. the default path is byte-identical to pre-change.
    #[test]
    fn bare_decl_gate_flag_off_rejects_even_an_unread_symmetric_role() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "0")]);
        assert!(!gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SymmetricObjectProperty(:r)\n\
    SubClassOf(:A :B)\n\
)\n"
        )));
    }

    /// POSITIVE: an EL ontology that merely NAMES a symmetric property.
    #[test]
    fn bare_decl_gate_admits_an_unread_symmetric_role() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SymmetricObjectProperty(:r)\n\
    SubClassOf(:A :B)\n\
)\n"
        )));
    }

    /// POSITIVE: both sides of the inverse pair unread.
    #[test]
    fn bare_decl_gate_admits_an_unread_inverse_pair() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:p))\n\
    Declaration(ObjectProperty(:q))\n\
    InverseObjectProperties(:p :q)\n\
    SubClassOf(:A :B)\n\
)\n"
        )));
    }

    /// POSITIVE: the shape that actually occurs in the ORE corpus
    /// (`ore_ont_8470`): the symmetric role is transitive and sits under a
    /// super-role, but nothing in the whole ontology ever reads either role's
    /// edges. `TransitiveRole` is not a read — it only enlarges `r`'s own edge
    /// set — so the whole unread component stays admissible.
    #[test]
    fn bare_decl_gate_admits_unread_symmetric_transitive_under_unread_superrole() {
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "1")]);
        assert!(gate_admits(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    Declaration(ObjectProperty(:used))\n\
    SymmetricObjectProperty(:r)\n\
    TransitiveObjectProperty(:r)\n\
    SubObjectPropertyOf(:r :s)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:used :B))\n\
)\n"
        )));
    }

    /// GATE PROBE for `RUSTDL_FRAGMENT_BARE_DECL` — reports, per ontology, the
    /// saturation-fast-path gate verdict with the flag OFF and ON.
    ///
    /// Exists because *grep is not the gate*: 205 of the 257 known-DNF ORE
    /// ontologies textually contain a `SymmetricObjectProperty` /
    /// `InverseObjectProperties` line, but only a fraction are blocked by
    /// **only** that. Running `classify` to read the `# mode:` banner costs a
    /// full DNF timeout per refused ontology; this probe costs one conversion.
    ///
    /// ```sh
    /// RUSTDL_GATE_PROBE_LIST=/path/to/paths.txt \
    ///   cargo test -p owl-dl-reasoner --release fragment_bare_decl_gate_probe \
    ///   -- --ignored --nocapture
    /// ```
    ///
    /// Prints `<stem> off=<yes|no> on=<yes|no>`; the recovered set is the rows
    /// with `off=no on=yes`.
    #[test]
    #[ignore = "measurement tool; needs RUSTDL_GATE_PROBE_LIST=<file of ontology paths>"]
    fn fragment_bare_decl_gate_probe() {
        let Some(list) = std::env::var_os("RUSTDL_GATE_PROBE_LIST") else {
            eprintln!("SKIP: set RUSTDL_GATE_PROBE_LIST to a file of ontology paths");
            return;
        };
        let listing = std::fs::read_to_string(&list).expect("probe list is readable");
        for line in listing.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let path = std::path::Path::new(line);
            let Ok(src) = std::fs::read_to_string(path) else {
                println!("{line} READ_ERROR");
                continue;
            };
            let mut reader = Cursor::new(src);
            let Ok((onto, _)) =
                read::<RcStr, SetOntology<RcStr>, _>(&mut reader, ParserConfiguration::default())
            else {
                println!("{line} PARSE_ERROR");
                continue;
            };
            let Ok(internal) = convert_ontology(&onto) else {
                println!("{line} CONVERT_ERROR");
                continue;
            };
            #[allow(unsafe_code)]
            let verdict = |on: bool| {
                // SAFETY: this probe is `#[ignore]`d and run single-threaded on
                // demand; no other thread reads the environment concurrently.
                unsafe {
                    if on {
                        std::env::set_var("RUSTDL_FRAGMENT_BARE_DECL", "1");
                    } else {
                        std::env::remove_var("RUSTDL_FRAGMENT_BARE_DECL");
                    }
                }
                is_pure_el(&internal)
                    || saturator_complete_fragment(&internal)
                    || tbox_only_saturator_eligible(&internal)
            };
            let off = verdict(false);
            let on = verdict(true);
            println!(
                "{line} off={} on={}",
                if off { "yes" } else { "no" },
                if on { "yes" } else { "no" }
            );
        }
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

    /// Task 3 (label-cache back-fold): `inject_backfold_derived_sups` unit
    /// tests. The classify()-level integration fixtures in
    /// `tests/backfold.rs` exercise the flag end-to-end, but — per that
    /// file's doc comment — the two minimal galen-residual repros
    /// (told-filler / merge-derived-filler) are ALREADY closed by the
    /// ordinary label-cache/hierarchy machinery before back-fold ever runs
    /// (confirmed empirically; also see
    /// `docs/known-limitations/galen-defined-class-monotonicity-residual.md`'s
    /// "Follow-up diagnosis" — there is no small OFN fixture that
    /// reproduces the real galen-scale gap). So the injection function
    /// itself is unit-tested directly here, with a hand-built
    /// `label_cache` whose `derived_sups` is the ONLY channel carrying the
    /// edge — this is what actually exercises the new code.
    #[test]
    fn inject_backfold_derived_sups_adds_entailed_edge() {
        let _env = EnvGuard::set(&[("RUSTDL_CLASSIFY_BACKFOLD", "1")]);
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:C))\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let closure = owl_dl_saturation::saturate(&internal);
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        let a = internal.vocabulary.class_id(&iri("A")).expect("A id");
        let c = internal.vocabulary.class_id(&iri("C")).expect("C id");
        // No axioms relate A and C — the EL closure does not carry this
        // edge, so `derived_sups` is the only source.
        assert!(!closure.contains(a, c));
        let n = [a, c]
            .iter()
            .map(|id| id.index() as usize + 1)
            .max()
            .expect("non-empty array");
        let mut direct_supers: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut direct_children: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut label_cache: Vec<crate::LabelOracle> = vec![crate::LabelOracle::NoVerdict; n];
        label_cache[a.index() as usize] = crate::LabelOracle::Sat {
            labels: std::collections::HashSet::new(),
            derived_sups: vec![c],
        };
        let mut stats = ClassificationStats::default();
        inject_backfold_derived_sups(
            &label_cache,
            &closure,
            n,
            &mut direct_supers,
            &mut direct_children,
            &mut stats,
        );
        assert_eq!(stats.backfold_recovered, 1);
        assert!(direct_supers[a.index() as usize].contains(&(c.index() as usize)));
        assert!(direct_children[c.index() as usize].contains(&(a.index() as usize)));

        // Rerunning must not double-count or double-push (the dedup guard
        // is `!direct_supers[c].contains(&d)`, checked fresh each call).
        inject_backfold_derived_sups(
            &label_cache,
            &closure,
            n,
            &mut direct_supers,
            &mut direct_children,
            &mut stats,
        );
        assert_eq!(
            stats.backfold_recovered, 1,
            "dedup guard must prevent double-counting on rerun"
        );
        assert_eq!(
            direct_supers[a.index() as usize]
                .iter()
                .filter(|&&x| x == c.index() as usize)
                .count(),
            1
        );
    }

    #[test]
    fn inject_backfold_derived_sups_noop_when_flag_off() {
        let _env = EnvGuard::set(&[("RUSTDL_CLASSIFY_BACKFOLD", "0")]);
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:C))\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let closure = owl_dl_saturation::saturate(&internal);
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        let a = internal.vocabulary.class_id(&iri("A")).expect("A id");
        let c = internal.vocabulary.class_id(&iri("C")).expect("C id");
        let n = [a, c]
            .iter()
            .map(|id| id.index() as usize + 1)
            .max()
            .expect("non-empty array");
        let mut direct_supers: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut direct_children: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut label_cache: Vec<crate::LabelOracle> = vec![crate::LabelOracle::NoVerdict; n];
        // Even though `derived_sups` carries the edge, the flag is off, so
        // this must be a complete no-op — byte-identical to pre-Task-3.
        label_cache[a.index() as usize] = crate::LabelOracle::Sat {
            labels: std::collections::HashSet::new(),
            derived_sups: vec![c],
        };
        let mut stats = ClassificationStats::default();
        inject_backfold_derived_sups(
            &label_cache,
            &closure,
            n,
            &mut direct_supers,
            &mut direct_children,
            &mut stats,
        );
        assert_eq!(stats.backfold_recovered, 0, "flag off ⇒ zero injections");
        assert!(direct_supers[a.index() as usize].is_empty());
        assert!(direct_children[c.index() as usize].is_empty());
    }

    #[test]
    fn inject_backfold_derived_sups_skips_pair_already_in_closure() {
        let _env = EnvGuard::set(&[("RUSTDL_CLASSIFY_BACKFOLD", "1")]);
        // A ⊑ C is ALREADY an EL-closure fact (asserted `SubClassOf`) — the
        // dedup guard (`closure.contains`) must keep this a no-op; the
        // closure seed already carries the edge into the entailment matrix.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :C)\n\
)\n"
        ));
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let closure = owl_dl_saturation::saturate(&internal);
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        let a = internal.vocabulary.class_id(&iri("A")).expect("A id");
        let c = internal.vocabulary.class_id(&iri("C")).expect("C id");
        assert!(
            closure.contains(a, c),
            "SubClassOf(:A :C) must be an EL-closure fact"
        );
        let n = [a, c]
            .iter()
            .map(|id| id.index() as usize + 1)
            .max()
            .expect("non-empty array");
        let mut direct_supers: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut direct_children: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut label_cache: Vec<crate::LabelOracle> = vec![crate::LabelOracle::NoVerdict; n];
        label_cache[a.index() as usize] = crate::LabelOracle::Sat {
            labels: std::collections::HashSet::new(),
            derived_sups: vec![c],
        };
        let mut stats = ClassificationStats::default();
        inject_backfold_derived_sups(
            &label_cache,
            &closure,
            n,
            &mut direct_supers,
            &mut direct_children,
            &mut stats,
        );
        assert_eq!(stats.backfold_recovered, 0);
        assert!(direct_supers[a.index() as usize].is_empty());
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
        // Pin `RUSTDL_FRAGMENT_BARE_DECL` OFF (no longer the default since 0.4.7): this fixture is a
        // bare inverse-pair declaration over two roles nothing reads, which the
        // flag deliberately admits to the EL fragment. Taking the shared env
        // lock also stops a concurrently-running bare-decl canary from leaking
        // its `=1` into this test.
        let _env = EnvGuard::set(&[("RUSTDL_FRAGMENT_BARE_DECL", "0")]);
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
    fn saturator_fragment_rejects_user_unqualified_max_without_functional() {
        // The FP-CRITICAL guard for the functional-enforcement fragment fix:
        // a user-written UNQUALIFIED `≤1 r` (Max(1,r,Top)) with NO
        // `FunctionalObjectProperty(r)` declaration must STILL reject — the
        // saturator has no bitset to enforce it, so accepting it would be a
        // silently-dropped real `≤1` (the D10 unsound-completeness bug class).
        // Only the conversion-DERIVED shape (backed by FunctionalRole) is
        // accepted.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(ObjectSomeValuesFrom(:r owl:Thing) ObjectMaxCardinality(1 :r))\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "user unqualified ≤1 r WITHOUT FunctionalObjectProperty must reject"
        );
    }

    #[test]
    fn saturator_fragment_accepts_derived_functional_max_gci() {
        // The conversion-derived `∃r.⊤ ⊑ ≤1 r` GCI (emitted by
        // derive_functional_max_cardinality for a FunctionalObjectProperty)
        // must be RECOGNIZED so an EL+functional ontology stays on the fast
        // path. We exercise it through convert_ontology (which emits the GCI),
        // so a plain EL + functional ontology must remain in-fragment — the
        // derived Max GCI it now carries must not kick it out.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n",
        );
        // Sanity: the derived GCI is actually present (Some(r,Top) ⊑ Max(1,r,Top)).
        let has_derived = i.axioms.iter().any(|ax| {
            if let Axiom::SubClassOf { sub, sup } = ax {
                matches!(i.concepts.get(*sup), ConceptExpr::Max(1, _, f)
                    if matches!(i.concepts.get(*f), ConceptExpr::Top))
                    && matches!(i.concepts.get(*sub), ConceptExpr::Some(_, f)
                        if matches!(i.concepts.get(*f), ConceptExpr::Top))
            } else {
                false
            }
        });
        assert!(
            has_derived,
            "conversion must emit the derived ∃r.⊤ ⊑ ≤1 r GCI"
        );
        assert!(
            saturator_complete_fragment(&i),
            "EL + functional with the DERIVED ≤1 GCI must stay in the saturator fragment"
        );
    }

    #[test]
    fn saturator_fragment_accepts_disjoint_without_functional() {
        // No functional/inverse-functional roles ⇒ the disjoint×functional
        // interaction is absent, so DisjointClasses is now in the complete
        // fragment (one-pass fast path). The DisjointnessClash rule + unsat
        // back-prop are complete on EL+disjoint-no-functional by construction.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    DisjointClasses(:A :B)\n",
        );
        assert!(
            saturator_complete_fragment(&i),
            "DisjointClasses with no functional roles must be in the complete fragment"
        );
    }

    #[test]
    fn saturator_fragment_rejects_disjoint_with_functional() {
        // Functional role present ⇒ the disjoint×functional-merge interaction
        // is unproven, so the ontology conservatively falls to the hybrid path.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    DisjointClasses(:A :B)\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "DisjointClasses + a functional role must fall back to the hybrid path"
        );
    }

    #[test]
    fn tbox_fragment_accepts_el_tbox_with_abox() {
        // Lever 1: an EL TBox (A ⊑ B) carrying a nominal-free ABox must be
        // eligible for the saturation fast path — the ABox is irrelevant to
        // class subsumption. Without Lever 1 the ClassAssertion kicks it to the
        // O(n²) hybrid loop (the ORE ore_ont_1043 DNF shape).
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:x))\n\
    SubClassOf(:A :B)\n\
    ClassAssertion(:A :x)\n",
        );
        assert!(
            !is_pure_el(&i),
            "sanity: the full-axiom gate rejects it (ABox present)"
        );
        assert!(
            tbox_only_saturator_eligible(&i),
            "EL TBox + nominal-free ABox must be Lever-1 fast-path eligible"
        );
    }

    #[test]
    fn tbox_fragment_rejects_nominal_abox() {
        // A nominal in the TBox (ObjectHasValue → ConceptExpr::Nominal) makes
        // the ABox subsumption-relevant, so Lever 1 must NOT fire.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:x))\n\
    SubClassOf(:A ObjectHasValue(:r :x))\n\
    ClassAssertion(:A :x)\n",
        );
        assert!(
            !tbox_only_saturator_eligible(&i),
            "an ontology using nominals must be excluded from Lever 1"
        );
    }

    #[test]
    fn tbox_fragment_rejects_forall_tbox() {
        // Out-of-fragment TBox (∀) must reject even with the ABox stripped —
        // the D10 unsound-completeness guard carries into the TBox-only view.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:x))\n\
    SubClassOf(:A ObjectAllValuesFrom(:r :B))\n\
    ClassAssertion(:A :x)\n",
        );
        assert!(
            !tbox_only_saturator_eligible(&i),
            "a ∀ in the TBox must reject even under the TBox-only view"
        );
    }

    #[test]
    fn bot_lowered_disjointness_is_pure_el() {
        // Lever 1b: an explicit `A ⊓ B ⊑ ⊥` (SubClassOf(And, owl:Nothing)) is a
        // sound EL disjointness/unsat the saturator handles completely. With no
        // functional role it must be in the pure-EL fast path. RED before Bot ∈
        // is_el_concept.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)\n",
        );
        assert!(
            is_pure_el(&i),
            "EL + `A⊓B⊑⊥` (no functional) must be pure-EL fast-path eligible"
        );
    }

    #[test]
    fn saturator_fragment_accepts_atomic_unsat() {
        // A plain `A ⊑ ⊥` (single-class unsatisfiability) carries no
        // functional-merge interaction, so it stays in-fragment even with a
        // functional role present.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(:A owl:Nothing)\n",
        );
        assert!(
            saturator_complete_fragment(&i),
            "atomic `A⊑⊥` + functional must stay in the saturator fragment"
        );
    }

    #[test]
    fn saturator_fragment_rejects_conjunctive_bot_with_functional() {
        // FP-CRITICAL: `A⊓B⊑⊥` is disjointness(A,B); combined with a functional
        // role it is the UNPROVEN disjoint×functional-merge interaction the gate
        // deliberately excludes. It must fall back to the hybrid path (same
        // policy as a native DisjointClasses + functional).
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(ObjectIntersectionOf(:A :B) owl:Nothing)\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "conjunctive `A⊓B⊑⊥` + functional (disjoint×functional) must fall back"
        );
    }

    #[test]
    fn tbox_fragment_inert_without_abox() {
        // No ABox ⇒ Lever 1 is inert (the ordinary gate already decides). Guards
        // against changing ABox-free classification behaviour.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n",
        );
        assert!(
            !tbox_only_saturator_eligible(&i),
            "no ABox ⇒ Lever 1 must be inert"
        );
    }

    #[test]
    fn saturator_fragment_rejects_disjoint_union() {
        // DisjointUnion carries a disjunctive covering (class ≡ ⊔members), which
        // is out-of-fragment (Or), and the saturator's rule-builder has no
        // DisjointUnion arm. So it must stay on the hybrid path — even with no
        // functional roles.
        let i = internal_of(
            "    Declaration(Class(:P))\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    DisjointUnion(:P :A :B)\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "DisjointUnion must NOT be in the complete fragment (disjunctive covering + no saturator rule)"
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
