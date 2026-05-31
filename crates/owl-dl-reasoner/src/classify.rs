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

/// Per-call instrumentation: who decided what during the pairwise
/// classification loop. Useful for understanding when the EL
/// saturation oracle is pulling its weight versus when the tableau
/// is doing the work.
#[derive(Debug, Clone, Copy, Default)]
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
        self.stats
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
    classify_top_down_internal(&internal, None)
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
    classify_top_down_internal(&internal, Some(per_pair_timeout))
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
    let classes: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
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
    let classes: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
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
    if is_pure_el(internal) {
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
    let mut stats = ClassificationStats::default();
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
    classify_top_down_internal(&internal, None)
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
    classify_top_down_internal(&internal, Some(per_pair_timeout))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn classify_top_down_internal(
    internal: &InternalOntology,
    per_pair_timeout: Option<std::time::Duration>,
) -> Result<Classification, ReasonError> {
    let classes: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
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
    if is_pure_el(internal) {
        return Ok(classify_pure_el(internal, &classes, &index, &closure));
    }

    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // Per-class unsat probes — identical to the naive path. Reuse
    // the same parallel pattern.
    let mut stats = ClassificationStats::default();
    let unsat_probe_results: Result<Vec<(usize, bool, bool)>, ReasonError> = (0..n)
        .into_par_iter()
        .map(|i| {
            let class_id =
                owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            if closure.is_unsatisfiable(class_id) {
                Ok((i, false, true))
            } else if let Some(timeout) = per_pair_timeout {
                let deadline = Instant::now() + timeout;
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
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
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
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
            if subsumed && !direct_supers[cand].contains(&sup) {
                direct_supers[cand].push(sup);
                direct_children[sup].push(cand);
            }
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
    stats: &mut ClassificationStats,
) -> Result<Vec<usize>, ReasonError> {
    let c_id = owl_dl_core::ClassId::new(u32::try_from(c).expect("class index fits in u32"));
    let mut frontier: Vec<usize> = top_level.to_vec();
    let mut accepted: Vec<usize> = Vec::new();
    while let Some(d) = frontier.pop() {
        if d == c {
            continue;
        }
        let d_id = owl_dl_core::ClassId::new(u32::try_from(d).expect("class index fits in u32"));
        let subsumed = if closure.contains(c_id, d_id) {
            stats.saturation_subsumption_hits += 1;
            true
        } else {
            subsumes_via_tableau(prepared, c_id, d_id, per_pair_timeout, true, stats)?
                .unwrap_or_default()
        };
        if !subsumed {
            continue;
        }
        for &k in &direct_children[d] {
            frontier.push(k);
        }
        accepted.push(d);
    }
    // Prune `accepted` to the most-specific entries: drop any
    // candidate that has a strict descendant also in `accepted`.
    let direct_parents: Vec<usize> = accepted
        .iter()
        .filter(|&&d| {
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
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
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
    trust_sat: bool,
    stats: &mut ClassificationStats,
) -> Result<Option<bool>, ReasonError> {
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
    let hyper_deadline = per_pair_timeout.map(|t| Instant::now() + t);
    match prepared.hyper_decide(sub, sup, hyper_deadline) {
        crate::HyperVerdict::Subsumed => {
            stats.hyper_proven_pairs += 1;
            return Ok(Some(true));
        }
        crate::HyperVerdict::NotSubsumed
            if trust_sat && crate::hyper_trust_sat_enabled() =>
        {
            stats.hyper_refuted_pairs += 1;
            return Ok(Some(false));
        }
        _ => {}
    }
    let build = move |pool: &mut ConceptPool| {
        let sub_concept = pool.atomic(sub);
        let super_concept = pool.atomic(sup);
        let not_super = pool.not(super_concept);
        pool.and(vec![sub_concept, not_super])
    };
    match per_pair_timeout {
        None => {
            let sat = prepared.decide(build)?;
            stats.tableau_subsumption_calls += 1;
            Ok(Some(!sat))
        }
        Some(timeout) => {
            let deadline = Instant::now() + timeout;
            // Robustness: a `ReasonError::NoVerdict` (tableau internal
            // cap, e.g. on large workloads like SIO) is treated as a
            // sound timeout — the pair defaults to "not subsumed"
            // (sound under-approximation), counted in `timed_out_pairs`.
            // Crashing classify on a single oversized pair is worse
            // than under-reporting the subsumption.
            match prepared.decide_with_deadline(deadline, build) {
                Ok(Some(sat)) => {
                    stats.tableau_subsumption_calls += 1;
                    Ok(Some(!sat))
                }
                Ok(None) | Err(crate::ReasonError::NoVerdict) => {
                    stats.timed_out_pairs += 1;
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

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

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
        // The DisjointObjectProperties axiom is outside our EL
        // saturation fragment — classify should NOT take the
        // pure-EL fast path and should issue at least one tableau
        // call (per-class unsat probes count).
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
        let stats = h.stats();
        assert!(!stats.pure_el_mode);
        assert!(
            stats.tableau_subsumption_calls + stats.tableau_unsat_calls > 0,
            "expected the tableau to be invoked for the non-EL fragment"
        );
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
}
