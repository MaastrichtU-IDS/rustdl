//! Consequence-based saturation engine for the EL fragment.
//!
//! Algorithm follows Kazakov, Krötzsch, Simančík (JAR 2014) "The Incredible
//! ELK". The Rust crate `whelk-rs` is the working reference implementation;
//! we re-implement against our own IR (see `owl-dl-core`) to avoid IR-boundary
//! copies in the hot loop.
//!
//! ## Opt-in proof recording
//!
//! Set `RUSTDL_PROOF=1` or use [`saturate_with_config`] with
//! `record_proofs: true` to enable proof recording. This populates a
//! [`proof::ProofTrace`] side-table mapping each derived fact to the
//! rule that produced it. Zero-cost when off: a single `bool` check per
//! derivation, no allocation.
//!
//! After saturation, [`proof::prove_subsumption`] walks the trace backward
//! to the axiom leaves and returns a [`proof::ProofNode`] tree.
//!
//! ## What this engine covers
//!
//! Subsumer closure over the atomic-class subset of the input
//! ontology, with the supporting EL rules wired into one fixed-point
//! loop:
//!
//! - Atomic `SubClassOf(A, B)` — told subsumption.
//! - `SubClassOf(A, ObjectIntersectionOf([B₁, …, Bₙ]))` distributes
//!   to `A ⊑ Bᵢ` for each atomic operand.
//! - `SubClassOf(ObjectIntersectionOf([B₁, …, Bₙ]), C)` — conjunctive
//!   trigger; any class with all `Bᵢ` as subsumers gains `C`.
//! - `EquivalentClasses(A₁, …, Aₙ)` — decomposed pairwise.
//! - **CR5 existential propagation** for `∃r.Y` on either side of a
//!   `SubClassOf`; the chain rule grows the existential-fact set
//!   in-loop so further hops compose. Facts are indexed by subject
//!   class so the chain inner loop is `O(|subsumers(target)| ·
//!   |facts_per_sub|)` rather than `O(|facts|)`.
//! - **Tseitin introduction** for compound existential bodies
//!   `∃r.(B₁ ⊓ … ⊓ Bₙ)`: a synthetic atomic stand-in is allocated
//!   above the user vocabulary, paired with `F ≡ B₁ ⊓ … ⊓ Bₙ`
//!   clauses, so the rewritten `∃r.F` rides the same CR5 path.
//! - **CR9 role hierarchy** — sub-role / equivalent-role decls + a
//!   reflexive-transitive closure built once, consulted in CR5 and
//!   chain rules.
//! - **Length-2 role chains + `TransitiveObjectProperty`** materialise
//!   derived `(A, sup, C)` existential facts; longer chains and
//!   inverse-role chains are out of scope (rejected upstream).
//! - **`ObjectPropertyDomain` / `Range`** propagate to subject /
//!   target classes through the cached super-role closure.
//! - **`DisjointClasses` → Bot detection** flags classes equivalent
//!   to `⊥`.
//! - Closure under transitivity at every round.
//!
//! Still outside the engine (the orchestrator falls back to the
//! tableau for these): disjunction, complement, cardinality,
//! nominals, inverse roles in any position, role characteristics
//! that expand to cardinality (`Functional`, `InverseFunctional`,
//! etc.), `ABox` assertions, role chains of length ≠ 2.
//!
//! Axioms outside the supported fragment are silently dropped; the
//! reasoner orchestrator decides whether to take the saturation-only
//! fast path (when *every* axiom is in scope) or fall through to
//! tableau on the misses.

pub mod proof;
pub mod seed_sat;

use std::collections::{HashMap, HashSet, VecDeque};

use fixedbitset::FixedBitSet;
use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, InternalOntology, Role,
    RoleId, SubRolePath,
};

use proof::{AxiomRef, DerivedFact, ElRule, Inference, ProofTrace};

/// Read `RUSTDL_PROOF` once and cache it.  Default OFF.
fn proof_enabled() -> bool {
    std::env::var("RUSTDL_PROOF").as_deref() == Ok("1")
}

/// Configuration for [`saturate_with_config`].
#[derive(Debug, Clone)]
pub struct SaturateConfig {
    /// Whether to record proof steps. Also controlled by the `RUSTDL_PROOF`
    /// environment variable (default OFF). Setting this to `true` overrides
    /// the env var (e.g. for the `rustdl prove` subcommand).
    pub record_proofs: bool,
}

impl Default for SaturateConfig {
    fn default() -> Self {
        Self {
            record_proofs: proof_enabled(),
        }
    }
}

/// Compute the subsumer closure over the EL-fragment subset of
/// `internal`. The result maps every declared `ClassId` to the set
/// of named classes that subsume it (including itself).
///
/// Implementation: worklist-driven (ELK-style). Each newly-derived
/// fact (new subsumer, new existential edge, or new unsat flag) is
/// pushed onto a worklist; the loop pops and fires *only* the rules
/// that depend on that specific fact. Replaces the previous
/// full-table sweep on each fixed-point iteration.
#[must_use]
pub fn saturate(internal: &InternalOntology) -> Subsumers {
    saturate_with_config(internal, &SaturateConfig::default()).0
}

/// Like [`saturate`] but also returns every derived existential fact
/// `(sub, role, target)` and the `NomKey → individual` reverse map, so a
/// caller can seed a tableau with the saturation's deterministic ∃-structure.
///
/// Sorted by `(sub, role, target)` for determinism.
/// The `nominal_to_ind` map lets the caller convert a `NomKey` synthetic
/// target back to the original [`IndividualId`], which in turn gives the
/// wedge's nominal class id as `ClassId::new(num_named + ind.index())`.
#[must_use]
#[allow(clippy::type_complexity)]
pub fn saturate_with_exists_facts(
    internal: &InternalOntology,
) -> (
    Subsumers,
    Vec<(ClassId, RoleId, ClassId)>,
    std::collections::HashMap<ClassId, IndividualId>,
) {
    let n = internal.vocabulary.num_classes();
    let role_super_map = build_role_super(internal);
    let (rules, tseitin, num_total_classes, maybe_trace) =
        collect_el_rules_with_provenance(internal, &role_super_map, false);
    let role_super = freeze_role_super(&role_super_map);
    let mut engine = WorklistEngine::new(
        n,
        num_total_classes,
        rules,
        tseitin,
        role_super,
        false,
        maybe_trace,
    );
    engine.seed(internal);
    engine.run();
    let mut facts: Vec<(ClassId, RoleId, ClassId)> = engine.seen_facts.iter().copied().collect();
    facts.sort_unstable_by_key(|&(s, r, t)| (s.index(), r.index(), t.index()));
    let nom = engine.rules.nominal_to_ind.clone();
    (engine.subsumers, facts, nom)
}

/// Like [`saturate`] but also supports optional proof recording.
///
/// Returns `(Subsumers, Some(ProofTrace))` when `cfg.record_proofs` is `true`,
/// or `(Subsumers, None)` when proof recording is off (zero extra cost).
///
/// The `ProofTrace` maps each derived fact to the inference step that first
/// produced it. Use [`proof::prove_subsumption`] to extract a backward proof
/// tree.
#[must_use]
pub fn saturate_with_config(
    internal: &InternalOntology,
    cfg: &SaturateConfig,
) -> (Subsumers, Option<ProofTrace>) {
    let n = internal.vocabulary.num_classes();
    let role_super_map = build_role_super(internal);
    let (rules, tseitin, num_total_classes, maybe_trace) =
        collect_el_rules_with_provenance(internal, &role_super_map, cfg.record_proofs);
    let role_super = freeze_role_super(&role_super_map);
    let mut engine = WorklistEngine::new(
        n,
        num_total_classes,
        rules,
        tseitin,
        role_super,
        cfg.record_proofs,
        maybe_trace,
    );
    engine.seed(internal);
    engine.run();
    let trace = engine.proof_trace;
    (engine.subsumers, trace)
}

/// Build and fully run the base saturation engine, reserving one extra synthetic
/// class id `X` (above the Tseitin universe) that carries no axioms in the base.
///
/// Returns `(engine, X)`.  The returned engine is the fully-saturated base; the
/// caller can clone it, inject seed facts for `X` into the clone, run it to
/// fixpoint, and read `is_unsat_class(X)` for a per-call "is ⊓seed unsat?"
/// query without mutating the base.
///
/// **Reserved-id approach:** `collect_el_rules` returns
/// `tseitin.next_id == num_total_classes`, so a naïve `reserved_x = ClassId::new(
/// num_total_classes)` would alias the *first runtime synthetic* allocated inside
/// the engine (e.g. by Phase-2a functional-role witness-merge).  We bump
/// `tseitin.next_id` by 1 before handing it to `WorklistEngine::new`, so `X` =
/// `old_next_id` and any runtime synthetics start at `old_next_id + 1`.  The
/// engine is sized at `num_total_classes + 1` so `X`'s index is within bounds of
/// every per-class Vec and bitset.
pub(crate) fn build_run_engine_with_reserved(
    internal: &InternalOntology,
) -> (WorklistEngine, ClassId) {
    let n = internal.vocabulary.num_classes();
    let role_super_map = build_role_super(internal);
    let (rules, mut tseitin, num_total_classes, maybe_trace) =
        collect_el_rules_with_provenance(internal, &role_super_map, false);
    let role_super = freeze_role_super(&role_super_map);
    // Reserve one id above the static Tseitin universe for the seed query class X.
    // Bumping next_id ensures runtime synthetics start above X (no aliasing).
    let reserved_x = ClassId::new(u32::try_from(num_total_classes).expect("fits u32"));
    tseitin.next_id = u32::try_from(num_total_classes)
        .expect("fits u32")
        .checked_add(1)
        .expect("synthetic id overflow");
    let mut engine = WorklistEngine::new(
        n,
        num_total_classes + 1, // size all per-class Vecs/bitsets to include X
        rules,
        tseitin,
        role_super,
        false,
        maybe_trace,
    );
    engine.seed(internal);
    engine.run();
    (engine, reserved_x)
}

/// Worklist-driven saturation engine. Maintains the running closure
/// plus three event queues; each iteration pops one event, derives
/// its direct consequents, and pushes new events for anything that
/// became newly applicable. Terminates when all three queues are
/// empty.
///
/// Indices the engine maintains for O(1) rule lookup:
/// - `subsumed_by[D] = {C : C ⊑ D}` — reverse of `subsumers`.
///   Used by unsat propagation and trigger firing.
/// - `facts_by_sub[A]` / `facts_by_target[T]` — per-side fact
///   indices, so chain-rule and trigger lookups walk only relevant
///   facts.
/// - `conjunctive_by_body[B]` / `existential_triggers_by_body[B]`
///   — trigger lookup keyed on the body class, so a new subsumer
///   only re-checks the triggers that could possibly fire.
/// - `disjoints_by_class[A] = {B : (A,B) or (B,A) is disjoint}`
///   — disjoint-pair lookup keyed on either operand.
#[derive(Clone)]
struct WorklistEngine {
    subsumers: Subsumers,
    /// Reverse index: `subsumed_by[D]` is the bitset of classes
    /// `C` such that `C ⊑ D` is in the closure. Maintained pairwise
    /// with `subsumers.subsumers` (every `(C, D)` pair lives in
    /// both).
    subsumed_by: Vec<FixedBitSet>,

    facts: Vec<ExistentialFact>,
    seen_facts: HashSet<(ClassId, RoleId, ClassId)>,
    /// `facts_by_sub[class_idx]` → indices into `facts`. Dense
    /// `Vec<Vec<_>>` keyed by class id, replacing the previous
    /// `HashMap<ClassId, Vec<usize>>` for cache- and dispatch-
    /// friendliness on the hot lookups.
    facts_by_sub: Vec<Vec<usize>>,
    facts_by_target: Vec<Vec<usize>>,

    todo_subsumer: VecDeque<(ClassId, ClassId)>,
    todo_fact: VecDeque<usize>,
    todo_unsat: VecDeque<ClassId>,

    rules: ElRules,
    /// Dense reflexive-transitive super-role closure indexed by `RoleId::index()`.
    /// `role_super[r.index()]` is the sorted slice of all roles `s` with `r ⊑ s`
    /// (including `r` itself). Built once from `build_role_super` via
    /// `freeze_role_super`; enables O(1) Vec indexing in the hot saturation loop.
    role_super: Vec<Box<[RoleId]>>,
    /// Bitset version of `role_super` for O(1) `is_sub_role(r, s)` tests.
    /// `role_super_bitset[r.index()].contains(s.index())` iff `r ⊑ s` (reflexive).
    /// Built once alongside `role_super`; eliminates the O(k) `slice::contains`
    /// call that dominated the profile post-fix1 at ~21% for galen.
    role_super_bitset: Vec<FixedBitSet>,
    /// Dense per-class indices into `rules.conjunctive_triggers`.
    conjunctive_by_body: Vec<Vec<usize>>,
    /// Dense per-class indices into `rules.existential_triggers`.
    existential_triggers_by_body: Vec<Vec<usize>>,
    /// Dense per-class list of classes disjoint from each class.
    disjoints_by_class: Vec<Vec<ClassId>>,

    /// Number of *user-declared* classes (excluding Tseitin
    /// synthetics). The seeder iterates only this range for
    /// reflexive `C ⊑ C` so synthetic classes get their reflexivity
    /// implicitly via the rules that introduce them.
    num_user_classes: usize,
    /// Total class-id universe size (user + Tseitin). Used to size
    /// the bitsets.
    num_total_classes: usize,
    /// Runtime Tseitin allocator for synthetic class IDs introduced
    /// by the Phase 2a functional-role witness-merge rule. Seeded
    /// from (and sharing the `by_body` dedup map of) the
    /// collection-time allocator returned by `collect_el_rules`, so
    /// runtime and static synthetics produced for the same body
    /// `{A, B}` map to the same class id. Pairs `{target_i, target_j}`
    /// are deduplicated by sorted body, just like the static path.
    tseitin_runtime: TseitinAllocator,
    /// Phase 2a EL++ witness-merge — per-`(sub, R_f)` FLAT SET of
    /// atomic class IDs that have been accumulated into a single
    /// `R_f`-witness. Monotonically grows; bounded by the atomic
    /// vocabulary, so the merge rule terminates regardless of how
    /// many sub-property facts feed in. Replaces T4's synthetic-id
    /// tracking which non-terminated on 3+ sub-property fan-in (see
    /// T4.5 commit message + docs/phase2a-results.md when written).
    merged_atom_sets: HashMap<(ClassId, RoleId), std::collections::BTreeSet<ClassId>>,
    /// Atomic-content map for every allocated synthetic (static AND
    /// runtime). For a synthetic `F` with body `{a, b, ...}` where each
    /// operand may itself be a synthetic, `atomic_content_of[F]` is the
    /// transitive flattening into the original atomic vocabulary.
    /// For non-synthetic class IDs, callers default to `{id}`.
    atomic_content_of: HashMap<ClassId, std::collections::BTreeSet<ClassId>>,
    /// Phase 2d: count of facts materialized via subsumer inheritance.
    /// Bumped each time `push_fact` (or its inherit-from-subsumer call
    /// in `process_subsumer`) creates a fact whose `(sub, role, target)`
    /// triple wasn't in `seen_facts` AND whose `sub` differs from the
    /// originating fact's `sub`. Diagnostic only; not gated by a feature.
    /// See `docs/phase2d-design.md`.
    phase2d_facts_inherited: u64,
    /// Phase 2c-redux: number of sub-role propagations emitted by the
    /// Phase 2c inner loop in `process_fact` (one bump per successful
    /// `push_fact` of a `(X, R_k, synthetic)` emission where `R_k ⊑ R_f`
    /// and X already had a fact on `R_k`). Used by structural canaries /
    /// diagnostics; not consumed by the reasoner output.
    phase2c_sub_role_propagations: u64,

    /// SP-B2a: per-disjunction state for the synthetic-conjunction forced-disjunct.
    /// Each entry holds the class `C`, its atomic disjuncts `Dᵢ`, and the
    /// `Sᵢ = C⊓Dᵢ` synthetics created at seed; `fired` guards re-forcing. The hook
    /// in `process_unsat` recomputes survivors (disjuncts whose `Sᵢ` is not unsat)
    /// when any `Sᵢ` becomes unsat — one survivor ⟹ force `C⊑Dₖ`, none ⟹ `C⊑⊥`.
    /// Empty ⇒ no-op (EL/Horn corpus). Complements B1's cheaper told/derived-disjoint
    /// check; catches deeper `C⊓Dᵢ` unsat (functional-merge, existential, domain, …).
    b2_disjunctions: Vec<B2Disjunction>,
    /// Reverse index: synthetic `Sᵢ` class id → its `b2_disjunctions` entry index.
    b2_synth_to_disj: HashMap<ClassId, usize>,

    // --- Proof recording (zero-cost when off) ---
    /// Whether proof recording is active. When `false`, every proof-recording
    /// branch is skipped and `proof_trace` stays `None`.
    record_proofs: bool,
    /// The proof trace, populated only when `record_proofs` is `true`.
    proof_trace: Option<ProofTrace>,
}

/// SP-B2a per-disjunction state (see `WorklistEngine::b2_disjunctions`).
#[derive(Clone)]
struct B2Disjunction {
    class: ClassId,
    disjuncts: Box<[ClassId]>,
    synthetics: Box<[ClassId]>,
    fired: bool,
}

impl WorklistEngine {
    #[allow(clippy::too_many_arguments)]
    fn new(
        num_user_classes: usize,
        num_total_classes: usize,
        rules: ElRules,
        tseitin: TseitinAllocator,
        role_super: Vec<Box<[RoleId]>>,
        record_proofs: bool,
        proof_trace: Option<ProofTrace>,
    ) -> Self {
        let mut conjunctive_by_body: Vec<Vec<usize>> = vec![Vec::new(); num_total_classes];
        for (idx, trigger) in rules.conjunctive_triggers.iter().enumerate() {
            for &body in &trigger.bodies {
                conjunctive_by_body[body.index() as usize].push(idx);
            }
        }
        let mut existential_triggers_by_body: Vec<Vec<usize>> = vec![Vec::new(); num_total_classes];
        for (idx, trigger) in rules.existential_triggers.iter().enumerate() {
            existential_triggers_by_body[trigger.body.index() as usize].push(idx);
        }
        let mut disjoints_by_class: Vec<Vec<ClassId>> = vec![Vec::new(); num_total_classes];
        for &(a, b) in &rules.disjoint_pairs {
            disjoints_by_class[a.index() as usize].push(b);
            disjoints_by_class[b.index() as usize].push(a);
        }
        let mut subsumed_by = Vec::with_capacity(num_total_classes);
        for _ in 0..num_total_classes {
            subsumed_by.push(FixedBitSet::with_capacity(num_total_classes));
        }
        // Populate atomic_content_of for all static Tseitin synthetics.
        // The bodies in tseitin.by_body are sorted Vec<ClassId>; we treat
        // each body operand as atomic (the bodies contain only user-class IDs
        // and existential-marker IDs from introduce_existential_marker, which
        // are themselves above the user vocabulary but bounded).
        let mut atomic_content_of: HashMap<ClassId, std::collections::BTreeSet<ClassId>> =
            HashMap::new();
        for (body, &synthetic) in &tseitin.by_body {
            let atoms: std::collections::BTreeSet<ClassId> = body.iter().copied().collect();
            atomic_content_of.insert(synthetic, atoms);
        }
        // Build the role-super bitset from the already-built dense Vec.
        // `role_super_bitset[r].contains(s)` iff `r ⊑ s` (including reflexive).
        // Used for O(1) sub-role tests in the hot loop, replacing the O(k)
        // `slice_contains` that was ~21% of galen wall post-fix1.
        let num_roles = role_super.len();
        let mut role_super_bitset: Vec<FixedBitSet> =
            vec![FixedBitSet::with_capacity(num_roles); num_roles];
        for (r_idx, supers) in role_super.iter().enumerate() {
            for s in supers {
                let si = s.index() as usize;
                if si < num_roles {
                    role_super_bitset[r_idx].insert(si);
                }
            }
        }
        Self {
            subsumers: Subsumers::with_capacity(num_total_classes),
            subsumed_by,
            facts: Vec::new(),
            seen_facts: HashSet::new(),
            facts_by_sub: vec![Vec::new(); num_total_classes],
            facts_by_target: vec![Vec::new(); num_total_classes],
            todo_subsumer: VecDeque::new(),
            todo_fact: VecDeque::new(),
            todo_unsat: VecDeque::new(),
            rules,
            role_super,
            role_super_bitset,
            conjunctive_by_body,
            existential_triggers_by_body,
            disjoints_by_class,
            num_user_classes,
            num_total_classes,
            tseitin_runtime: tseitin,
            merged_atom_sets: HashMap::new(),
            atomic_content_of,
            phase2d_facts_inherited: 0,
            phase2c_sub_role_propagations: 0,
            b2_disjunctions: Vec::new(),
            b2_synth_to_disj: HashMap::new(),
            record_proofs,
            proof_trace,
        }
    }

    /// O(1) sub-role test: returns `true` iff `r ⊑ s` in the reflexive-transitive
    /// super-role closure. Uses `role_super_bitset` so it avoids the linear slice
    /// scan that `supers_of(role_super, r).contains(&s)` performed.
    #[inline]
    fn is_sub_role(&self, r: RoleId, s: RoleId) -> bool {
        let ri = r.index() as usize;
        let si = s.index() as usize;
        self.role_super_bitset
            .get(ri)
            .is_some_and(|bs| si < bs.len() && bs.contains(si))
    }

    /// Snapshot the bitset at `subsumers.subsumers[c.index()]` as a
    /// `Vec<ClassId>`. Used at points where the borrow into the
    /// bitset would conflict with subsequent mutation.
    fn supers_of_class(&self, c: ClassId) -> Vec<ClassId> {
        let ci = c.index() as usize;
        self.subsumers
            .subsumers
            .get(ci)
            .map(|bs| {
                bs.ones()
                    .map(|i| ClassId::new(u32::try_from(i).expect("class id fits in u32")))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Snapshot the reverse bitset at `subsumed_by[c.index()]` as a
    /// `Vec<ClassId>`.
    fn subs_of_class(&self, c: ClassId) -> Vec<ClassId> {
        let ci = c.index() as usize;
        self.subsumed_by
            .get(ci)
            .map(|bs| {
                bs.ones()
                    .map(|i| ClassId::new(u32::try_from(i).expect("class id fits in u32")))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return the transitive atomic content of class `c`. For synthetics
    /// tracked in `atomic_content_of`, returns the stored set. For any
    /// class not in the map (i.e., a user-vocabulary atomic or an
    /// existential marker that wasn't given an explicit entry), returns
    /// the singleton `{c}`.
    fn atomic_content_of_or_self(&self, c: ClassId) -> std::collections::BTreeSet<ClassId> {
        if let Some(set) = self.atomic_content_of.get(&c) {
            set.clone()
        } else {
            let mut s = std::collections::BTreeSet::new();
            s.insert(c);
            s
        }
    }

    /// Introduce a runtime Tseitin synthetic for the conjunction of the
    /// body's atomic classes. Returns the synthetic class id
    /// (deduplicated — passing the same sorted body twice returns the
    /// same id without allocating a new one).
    ///
    /// Beyond `TseitinAllocator::introduce` (which only mutates
    /// `self.rules`), this method ALSO:
    /// - Grows `self.subsumers`/`self.subsumed_by` bitsets and the
    ///   per-class index Vecs to fit the new id.
    /// - Indexes the new conjunctive trigger into `conjunctive_by_body`.
    /// - Enqueues `synthetic ⊑ body[i]` subsumptions so the standard
    ///   rules pick them up.
    ///
    /// Because `tseitin_runtime` is seeded from the same `by_body` map
    /// as the collection-time allocator, a body `{A, B}` that was
    /// already Tseitin-introduced statically (e.g. for `∃r.(A⊓B) ⊑ T`)
    /// will return the SAME synthetic id here, so the runtime fact
    /// `(sub, r_func, F)` chains correctly into the existing existential
    /// trigger.
    fn introduce_runtime_synthetic(&mut self, body: Vec<ClassId>) -> ClassId {
        let before_atomic = self.rules.atomic_subsumptions.len();
        let before_conjunctive = self.rules.conjunctive_triggers.len();
        // Capture a clone of the body before `introduce` consumes it (sorts
        // and stores it) so we can compute atomic_content_of for the new
        // synthetic. On the dedup path we skip this.
        let body_clone = body.clone();
        let synthetic = self.tseitin_runtime.introduce(body, &mut self.rules);
        let s_idx = synthetic.index() as usize;
        let added_atomic = self.rules.atomic_subsumptions.len() - before_atomic;
        let added_conjunctive = self.rules.conjunctive_triggers.len() - before_conjunctive;
        if added_atomic == 0 && added_conjunctive == 0 {
            // Dedup hit — synthetic already exists; atomic_content_of already
            // has an entry for it (populated when first allocated).
            return synthetic;
        }
        // Track atomic content: flatten each body operand transitively into
        // the original-vocabulary atomic class IDs. The result is a flat
        // BTreeSet so the merge rule can use set operations directly.
        let mut atoms = std::collections::BTreeSet::new();
        for b in &body_clone {
            atoms.extend(self.atomic_content_of_or_self(*b));
        }
        self.atomic_content_of.insert(synthetic, atoms);
        // Grow per-class state if the synthetic id exceeds current capacity.
        let needed = s_idx + 1;
        if needed > self.num_total_classes {
            for bs in &mut self.subsumers.subsumers {
                bs.grow(needed);
            }
            self.subsumers.unsatisfiable.grow(needed);
            for bs in &mut self.subsumed_by {
                bs.grow(needed);
            }
            while self.subsumers.subsumers.len() < needed {
                self.subsumers
                    .subsumers
                    .push(FixedBitSet::with_capacity(needed));
            }
            while self.subsumed_by.len() < needed {
                self.subsumed_by.push(FixedBitSet::with_capacity(needed));
            }
            while self.facts_by_sub.len() < needed {
                self.facts_by_sub.push(Vec::new());
            }
            while self.facts_by_target.len() < needed {
                self.facts_by_target.push(Vec::new());
            }
            while self.conjunctive_by_body.len() < needed {
                self.conjunctive_by_body.push(Vec::new());
            }
            while self.existential_triggers_by_body.len() < needed {
                self.existential_triggers_by_body.push(Vec::new());
            }
            while self.disjoints_by_class.len() < needed {
                self.disjoints_by_class.push(Vec::new());
            }
            self.num_total_classes = needed;
        }
        // Index any new conjunctive triggers into conjunctive_by_body.
        for added_idx in before_conjunctive..self.rules.conjunctive_triggers.len() {
            let bodies = self.rules.conjunctive_triggers[added_idx].bodies.clone();
            for b in bodies {
                self.conjunctive_by_body[b.index() as usize].push(added_idx);
            }
        }
        // Enqueue the F ⊑ Bi atomic subsumptions so existing rules fire on them.
        for added_idx in before_atomic..self.rules.atomic_subsumptions.len() {
            let sub_ax = self.rules.atomic_subsumptions[added_idx];
            self.todo_subsumer.push_back((sub_ax.sub, sub_ax.sup));
        }
        synthetic
    }

    /// Seed the worklist from told axioms + reflexivity.
    fn seed(&mut self, internal: &InternalOntology) {
        // Reflexive `C ⊑ C` for every declared class. Synthetic
        // Tseitin classes get their reflexive entry implicitly via
        // the conjunctive-trigger / atomic-subsumption rules that
        // introduced them.
        for i in 0..self.num_user_classes {
            let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
            self.todo_subsumer.push_back((id, id));
        }
        // Synthetic Tseitin classes need explicit reflexivity too —
        // they don't appear in the user vocabulary but the engine
        // still derives `F ⊑ F` for them via told rules. Push them
        // up-front to keep behaviour matched with the previous
        // HashSet implementation.
        for i in self.num_user_classes..self.num_total_classes {
            let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
            self.todo_subsumer.push_back((id, id));
        }
        // Told atomic subsumers. When proof recording is on, tag each
        // seeded subsumption with the axiom that produced it.
        if self.record_proofs {
            let axiom_refs: Vec<Option<usize>> = self
                .proof_trace
                .as_ref()
                .map(|t| t.atomic_sub_axiom.clone())
                .unwrap_or_default();
            for (idx, rule) in self.rules.atomic_subsumptions.iter().enumerate() {
                self.todo_subsumer.push_back((rule.sub, rule.sup));
                // Pre-record ToldSubsumer so process_subsumer finds it after
                // record_subsumer; we use the probe in record_subsumer_with_rule.
                let ax_ref = axiom_refs.get(idx).copied().flatten().map(AxiomRef);
                let inf = Inference {
                    rule: ElRule::ToldSubsumer,
                    premise_facts: vec![],
                    axiom_refs: ax_ref.into_iter().collect(),
                };
                if let Some(t) = self.proof_trace.as_mut() {
                    t.record(DerivedFact::Sub(rule.sub, rule.sup), inf);
                }
            }
        } else {
            for rule in &self.rules.atomic_subsumptions {
                self.todo_subsumer.push_back((rule.sub, rule.sup));
            }
        }
        // `⊤ ⊑ C` broadcast: C is Top-equivalent ⟹ every named class ⊑ C. Seed
        // `(X, C)` for every named class X and every top subsumer C; the fixpoint
        // then closes transitively (X ⊑ C ⊑ D ⟹ X ⊑ D) and fires C's downstream
        // rules on each X. No-op unless the ontology has a `⊤ ⊑ NamedClass` axiom.
        if !self.rules.top_subsumers.is_empty() {
            let tops = self.rules.top_subsumers.clone();
            for i in 0..self.num_user_classes {
                let x = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
                for &c in &tops {
                    self.todo_subsumer.push_back((x, c));
                }
            }
        }
        // Told existential facts (snapshot first to release the
        // borrow into `self.rules`).
        let told: Vec<ExistentialFact> = self.rules.existential_facts.clone();
        if self.record_proofs {
            let fact_axioms: Vec<Option<usize>> = self
                .proof_trace
                .as_ref()
                .map(|t| t.existential_fact_axiom.clone())
                .unwrap_or_default();
            for (idx, fact) in told.into_iter().enumerate() {
                let ax_ref = fact_axioms.get(idx).copied().flatten().map(AxiomRef);
                let df = DerivedFact::Exist(fact.sub, fact.role, fact.target);
                let inf = Inference {
                    rule: ElRule::ToldFact,
                    premise_facts: vec![],
                    axiom_refs: ax_ref.into_iter().collect(),
                };
                if let Some(t) = self.proof_trace.as_mut() {
                    t.record(df, inf);
                }
                self.push_fact(fact);
            }
        } else {
            for fact in told {
                self.push_fact(fact);
            }
        }
        // Phase D4: classes told directly to be unsatisfiable via
        // `SubClassOf(Atomic, Bot)` (data-axiom preprocessing clash
        // emission). enqueue_unsat queues them; process_unsat
        // propagates to subclasses + fact targets via the standard
        // rules.
        let directly_unsat: Vec<ClassId> = self.rules.directly_unsat.clone();
        if self.record_proofs {
            let unsat_axioms: Vec<Option<usize>> = self
                .proof_trace
                .as_ref()
                .map(|t| t.directly_unsat_axiom.clone())
                .unwrap_or_default();
            for (idx, c) in directly_unsat.into_iter().enumerate() {
                let ax_ref = unsat_axioms.get(idx).copied().flatten().map(AxiomRef);
                let inf = Inference {
                    rule: ElRule::ToldUnsat,
                    premise_facts: vec![],
                    axiom_refs: ax_ref.into_iter().collect(),
                };
                if let Some(t) = self.proof_trace.as_mut() {
                    t.record(DerivedFact::Unsat(c), inf);
                }
                self.enqueue_unsat(c);
            }
        } else {
            for c in directly_unsat {
                self.enqueue_unsat(c);
            }
        }
        // Reflexivity proof records: record Sub(C,C) for every user class
        // (the told-subsumer loop will override if there's also an axiom, but
        // first-writer-wins so reflexivity lands unless we record told first).
        // We do this AFTER told so told wins if both apply (Sub(C,C) may be
        // derived both by reflexivity and by a told axiom C ⊑ C).
        // In practice the seeding queues Sub(C,C) before atomic_subsumptions,
        // so we record reflexivity here after the told seeds.
        if self.record_proofs {
            for i in 0..internal.vocabulary.num_classes() {
                let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
                let df = DerivedFact::Sub(id, id);
                let inf = Inference {
                    rule: ElRule::Reflexivity,
                    premise_facts: vec![],
                    axiom_refs: vec![],
                };
                if let Some(t) = self.proof_trace.as_mut() {
                    t.record(df, inf);
                }
            }
        }
        // SP-B2a: create `Sᵢ = C⊓Dᵢ` synthetics for each atomic disjunction (ingested
        // by B1 into `disjunctions_by_class`) and register the reverse map. The
        // fixpoint determines each `Sᵢ`'s satisfiability via the existing rules; the
        // `process_unsat` hook forces the disjunction when an `Sᵢ` becomes unsat.
        if !self.rules.disjunctions_by_class.is_empty() {
            let disjunctions: Vec<(ClassId, Vec<Box<[ClassId]>>)> = self
                .rules
                .disjunctions_by_class
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect();
            for (c, disjs) in disjunctions {
                for disj in disjs {
                    let synthetics: Vec<ClassId> = disj
                        .iter()
                        .map(|&di| self.introduce_runtime_synthetic(vec![c, di]))
                        .collect();
                    let idx = self.b2_disjunctions.len();
                    for &s in &synthetics {
                        self.b2_synth_to_disj.insert(s, idx);
                    }
                    self.b2_disjunctions.push(B2Disjunction {
                        class: c,
                        disjuncts: disj,
                        synthetics: synthetics.into_boxed_slice(),
                        fired: false,
                    });
                }
            }
        }
    }

    /// Drain queues until all three are empty.
    fn run(&mut self) {
        loop {
            if let Some((c, d)) = self.todo_subsumer.pop_front() {
                self.process_subsumer(c, d);
            } else if let Some(idx) = self.todo_fact.pop_front() {
                self.process_fact(idx);
            } else if let Some(c) = self.todo_unsat.pop_front() {
                self.process_unsat(c);
            } else {
                break;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Seed-sat API hooks (used by `seed_sat.rs`).
    // -----------------------------------------------------------------------

    /// Enqueue `c ⊑ d` as a starting fact, exactly as `seed` does for told
    /// subsumptions.  Used by the seed-sat API.
    pub(crate) fn inject_subsumer(&mut self, c: ClassId, d: ClassId) {
        self.todo_subsumer.push_back((c, d));
    }

    /// Enqueue `c ⊑ ∃role.target` as a starting existential fact.
    pub(crate) fn inject_existential(&mut self, c: ClassId, role: RoleId, target: ClassId) {
        self.push_fact(ExistentialFact {
            sub: c,
            role,
            target,
        });
    }

    /// True iff saturation has proved `c ⊑ ⊥`.
    pub(crate) fn is_unsat_class(&self, c: ClassId) -> bool {
        self.subsumers.is_unsatisfiable(c)
    }

    /// Insert a derived `(C, D)` subsumer edge — no-op if already
    /// present. Returns whether the insert was new.
    fn record_subsumer(&mut self, c: ClassId, d: ClassId) -> bool {
        let ci = c.index() as usize;
        let di = d.index() as usize;
        let added = self.subsumers.subsumers[ci].put(di);
        if !added {
            // `put` returns true iff the bit was already set; we want
            // the opposite semantic here ("newly inserted").
            self.subsumed_by[di].insert(ci);
            return true;
        }
        false
    }

    /// Push `(c, d)` onto the subsumer worklist if not yet asserted.
    fn enqueue_subsumer(&mut self, c: ClassId, d: ClassId) {
        if !self.subsumers.contains(c, d) {
            self.todo_subsumer.push_back((c, d));
        }
    }

    /// Cluster-B path (b) core: seed `sub ⊑ ForallKey(role, S)` for every
    /// `∀role.OneOf(S)` target with `ind ∈ S`. The caller has established the
    /// gate (`role` functional, or `sub ⊑ ≤1 role`) under which `∃role.{ind}`
    /// forces the unique `role`-filler to be `ind ∈ S`.
    fn enqueue_forall_targets(&mut self, sub: ClassId, role: RoleId, ind: IndividualId) {
        if let Some(keys) = self.rules.forall_key_targets.get(&(role, ind)) {
            for key in keys.clone() {
                self.enqueue_subsumer(sub, key);
            }
        }
    }

    /// Push a class onto the unsat worklist if not yet flagged.
    fn enqueue_unsat(&mut self, c: ClassId) {
        let ci = c.index() as usize;
        if !self.subsumers.unsatisfiable.contains(ci) {
            self.todo_unsat.push_back(c);
        }
    }

    /// Insert a new existential fact and enqueue it for processing.
    /// Returns the index assigned to the fact, or `None` if it was
    /// already known.
    ///
    /// Phase 2d: after the new fact is inserted, propagate it to every
    /// subclass of `fact.sub` by recursively calling `push_fact` for
    /// `(subclass, fact.role, fact.target)`. Sound by the standard
    /// subsumer-driven existential propagation argument: if
    /// `subclass ⊑ fact.sub` and `(fact.sub, role, target)` is a sound
    /// existential commitment, then every `subclass`-instance is a
    /// `fact.sub`-instance with the same role witness, so
    /// `(subclass, role, target)` holds.
    ///
    /// Termination: bounded by `seen_facts` dedup over the finite
    /// `(sub, role, target)` triple space. The recursion at each
    /// subclass either short-circuits (triple already seen) or inserts
    /// a fresh triple; total fresh insertions ≤ number of distinct
    /// triples in the closure.
    ///
    /// See `docs/phase2d-design.md`.
    fn push_fact(&mut self, fact: ExistentialFact) -> Option<usize> {
        // `parent_fact` carries the fact we are inheriting FROM when this is a
        // Phase-2d recursive inheritance call.  `None` for top-level inserts.
        self.push_fact_impl(fact, None)
    }

    /// Internal implementation for `push_fact` that accepts an optional
    /// parent fact for Phase-2d proof recording.  When `parent_fact` is
    /// `Some(p)` the new fact was inherited from `p` via a `Sub(c, p.sub)`
    /// edge and we record a `FactInheritance` step.  All other callers pass
    /// `None` (the top-level insert).
    fn push_fact_impl(
        &mut self,
        fact: ExistentialFact,
        parent_fact: Option<&DerivedFact>,
    ) -> Option<usize> {
        if !self.seen_facts.insert((fact.sub, fact.role, fact.target)) {
            return None;
        }
        // Record the Phase-2d FactInheritance step when we have a parent fact.
        if self.record_proofs
            && let Some(pf) = parent_fact
        {
            // Sub(fact.sub, parent.sub) + Exist(parent) ⟹ Exist(fact.sub, role, target)
            let df = DerivedFact::Exist(fact.sub, fact.role, fact.target);
            let sub_edge = match pf {
                DerivedFact::Exist(parent_sub, _, _) => DerivedFact::Sub(fact.sub, *parent_sub),
                _ => DerivedFact::Sub(fact.sub, fact.sub), // fallback (should not occur)
            };
            let inf = Inference {
                rule: ElRule::FactInheritance,
                premise_facts: vec![sub_edge, pf.clone()],
                axiom_refs: vec![],
            };
            if let Some(t) = self.proof_trace.as_mut() {
                t.record(df, inf);
            }
        }
        let idx = self.facts.len();
        self.facts.push(fact);
        self.facts_by_sub[fact.sub.index() as usize].push(idx);
        self.facts_by_target[fact.target.index() as usize].push(idx);
        self.todo_fact.push_back(idx);
        // Phase 2d: propagate the fact to every subclass of fact.sub. subs_of_class
        // returns an owned Vec, so no borrow conflict with the recursive
        // push_fact_impl mutable borrow.
        //
        // NOTE: gating this on `has_functional_roles` is UNSOUND — although Phase 2d
        // was introduced to feed the functional witness-merge, the copied facts are
        // also consumed by the general conjunction/∃ rules on subclasses, so skipping
        // it drops real EL subsumptions on non-functional TBoxes (e.g. ORE
        // ore_ont_11522: 522 vs whelk's 1490). go-basic's byte-identical result was a
        // false reassurance (its structure happens not to need the copies). Do not gate.
        let subs = self.subs_of_class(fact.sub);
        let parent_df = DerivedFact::Exist(fact.sub, fact.role, fact.target);
        for c in subs {
            if c == fact.sub {
                continue;
            }
            let inherited = ExistentialFact {
                sub: c,
                role: fact.role,
                target: fact.target,
            };
            // Pass parent_df so the recursive call records FactInheritance.
            if self.push_fact_impl(inherited, Some(&parent_df)).is_some() {
                self.phase2d_facts_inherited += 1;
            }
        }
        Some(idx)
    }

    /// Fire all rules triggered by a freshly-derived `(C, D)` edge.
    #[allow(clippy::too_many_lines)]
    fn process_subsumer(&mut self, c: ClassId, d: ClassId) {
        if !self.record_subsumer(c, d) {
            return;
        }
        // Cluster-B path (b), ≤1-driven symmetric direction: if D is a
        // `MaxKey(1, R)` marker (C now known `⊑ ≤1 R`), every existing
        // `∃R.{a}` fact of C forces the unique R-filler to `a` — seed
        // `C ⊑ ForallKey(R,S)` for targets with `a ∈ S`. The fact-first order
        // is handled in `process_fact`; this covers subsumer-first.
        if let Some(&role) = self.rules.max1_role_by_key.get(&d) {
            let fact_idxs = self.facts_by_sub[c.index() as usize].clone();
            for fi in fact_idxs {
                let f = self.facts[fi];
                if f.role == role
                    && let Some(&ind) = self.rules.nominal_to_ind.get(&f.target)
                {
                    self.enqueue_forall_targets(c, role, ind);
                }
            }
        }
        // Transitivity (forward): anything D ⊑ is also a subsumer
        // of C.
        let supers_of_d = self.supers_of_class(d);
        for e in supers_of_d {
            // Proof: Sub(C,D) + Sub(D,E) ⟹ Sub(C,E)
            if self.record_proofs && !self.subsumers.contains(c, e) {
                let inf = Inference {
                    rule: ElRule::SubsumerTransitivityFwd,
                    premise_facts: vec![DerivedFact::Sub(c, d), DerivedFact::Sub(d, e)],
                    axiom_refs: vec![],
                };
                if let Some(t) = self.proof_trace.as_mut() {
                    t.record(DerivedFact::Sub(c, e), inf);
                }
            }
            self.enqueue_subsumer(c, e);
        }
        // Transitivity (backward): anything that had C as a
        // subsumer now also has D as a subsumer.
        let subs_of_c = self.subs_of_class(c);
        for x in subs_of_c {
            // Proof: Sub(X,C) + Sub(C,D) ⟹ Sub(X,D)
            if self.record_proofs && !self.subsumers.contains(x, d) {
                let inf = Inference {
                    rule: ElRule::SubsumerTransitivityBwd,
                    premise_facts: vec![DerivedFact::Sub(x, c), DerivedFact::Sub(c, d)],
                    axiom_refs: vec![],
                };
                if let Some(t) = self.proof_trace.as_mut() {
                    t.record(DerivedFact::Sub(x, d), inf);
                }
            }
            self.enqueue_subsumer(x, d);
        }
        // Unsat propagation: if D is unsat, C is unsat too.
        if self.subsumers.is_unsatisfiable(d) {
            // Proof: Sub(C,D) + Unsat(D) ⟹ Unsat(C)
            if self.record_proofs {
                let ci = c.index() as usize;
                if !self.subsumers.unsatisfiable.contains(ci) {
                    let inf = Inference {
                        rule: ElRule::UnsatSubsumer,
                        premise_facts: vec![DerivedFact::Sub(c, d), DerivedFact::Unsat(d)],
                        axiom_refs: vec![],
                    };
                    if let Some(t) = self.proof_trace.as_mut() {
                        t.record(DerivedFact::Unsat(c), inf);
                    }
                }
            }
            self.enqueue_unsat(c);
        }
        // Conjunctive triggers: every trigger with D in its body
        // list may now fire on C if C has all the other bodies too.
        if let Some(trigger_idxs) = Some(self.conjunctive_by_body[d.index() as usize].clone()) {
            for tidx in trigger_idxs {
                let trigger = &self.rules.conjunctive_triggers[tidx];
                if trigger
                    .bodies
                    .iter()
                    .all(|b| self.subsumers.contains(c, *b))
                {
                    let head = trigger.head;
                    // Proof: Sub(C,Bi) for each body ⟹ Sub(C,head)
                    if self.record_proofs && !self.subsumers.contains(c, head) {
                        let premises: Vec<DerivedFact> = trigger
                            .bodies
                            .iter()
                            .map(|&b| DerivedFact::Sub(c, b))
                            .collect();
                        let ax_ref = self
                            .proof_trace
                            .as_ref()
                            .and_then(|t| t.conjunctive_trigger_axiom.get(tidx).copied().flatten())
                            .map(AxiomRef);
                        let inf = Inference {
                            rule: ElRule::ConjunctiveTrigger,
                            premise_facts: premises,
                            axiom_refs: ax_ref.into_iter().collect(),
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Sub(c, head), inf);
                        }
                    }
                    self.enqueue_subsumer(c, head);
                }
            }
        }
        // Disjointness: if any class disjoint from D is already a
        // subsumer of C, C is unsat.
        if let Some(others) = Some(self.disjoints_by_class[d.index() as usize].clone()) {
            for &other in &others {
                if self.subsumers.contains(c, other) {
                    // Proof: Sub(C,D) + Sub(C,other) + Disjoint(D,other) ⟹ Unsat(C)
                    if self.record_proofs {
                        let ci = c.index() as usize;
                        if !self.subsumers.unsatisfiable.contains(ci) {
                            // Find axiom ref for the disjoint pair (use the first one).
                            let ax_ref = self.proof_trace.as_ref().and_then(|t| {
                                t.disjoint_pair_axiom.iter().find_map(|ax| ax.map(AxiomRef))
                            });
                            let inf = Inference {
                                rule: ElRule::DisjointnessClash,
                                premise_facts: vec![
                                    DerivedFact::Sub(c, d),
                                    DerivedFact::Sub(c, other),
                                ],
                                axiom_refs: ax_ref.into_iter().collect(),
                            };
                            if let Some(t) = self.proof_trace.as_mut() {
                                t.record(DerivedFact::Unsat(c), inf);
                            }
                        }
                    }
                    self.enqueue_unsat(c);
                    break; // one disjoint clash is enough
                }
            }
        }
        // SP-B1: derived-closure forced-disjunct. `c` gained subsumer `d`; recheck
        // `c`'s effective disjunctions (declared on `c` or on any subsumer of `c`,
        // since `c⊑g⊑⊔Dᵢ ⟹ c⊑⊔Dᵢ`). A disjunct `Dᵢ` is excluded iff some current
        // subsumer of `c` is disjoint from it; one survivor ⟹ force it, none ⟹ `c`
        // unsat. Uses the DERIVED subsumer closure (vs SP-A's told-only) and fires
        // inside the fixpoint, so forcing one disjunct can force the next. Decisions
        // are collected first so the immutable borrows (`rules`/`disjoints_by_class`/
        // `subsumers`) are released before the mutating enqueues. Sound by
        // construction; no-op when no atomic disjunction exists.
        if !self.rules.disjunctions_by_class.is_empty() {
            let mut force_unsat = false;
            let mut force_subs: Vec<ClassId> = Vec::new();
            for g in self.supers_of_class(c) {
                let Some(disjs) = self.rules.disjunctions_by_class.get(&g) else {
                    continue;
                };
                for disj in disjs {
                    let disj: &[ClassId] = disj;
                    let mut surv: Option<ClassId> = None;
                    let mut count = 0u32;
                    for &di in disj {
                        let excluded = self.disjoints_by_class[di.index() as usize]
                            .iter()
                            .any(|&gg| self.subsumers.contains(c, gg));
                        if !excluded {
                            count += 1;
                            surv = Some(di);
                        }
                    }
                    match (count, surv) {
                        (0, _) => force_unsat = true,
                        (1, Some(s)) => force_subs.push(s),
                        _ => {}
                    }
                }
            }
            if force_unsat {
                self.enqueue_unsat(c);
            }
            for s in force_subs {
                self.enqueue_subsumer(c, s);
            }
        }
        // Existential trigger firing — target side: for facts whose
        // target is C, a new subsumer D may match a trigger body.
        if let Some(fact_idxs) = Some(self.facts_by_target[c.index() as usize].clone())
            && let Some(trigger_idxs) =
                Some(self.existential_triggers_by_body[d.index() as usize].clone())
        {
            for fidx in fact_idxs {
                let fact = self.facts[fidx];
                for tidx in &trigger_idxs {
                    let trigger = self.rules.existential_triggers[*tidx];
                    if !self.is_sub_role(fact.role, trigger.role) {
                        continue;
                    }
                    // Every Y with fact.sub ∈ subsumers(Y) gains
                    // trigger.head — walk subsumed_by.
                    let head = trigger.head;
                    let candidates = self.subs_of_class(fact.sub);
                    for y in candidates {
                        if self.record_proofs && !self.subsumers.contains(y, head) {
                            let ax_ref = self
                                .proof_trace
                                .as_ref()
                                .and_then(|t| {
                                    t.existential_trigger_axiom.get(*tidx).copied().flatten()
                                })
                                .map(AxiomRef);
                            let inf = Inference {
                                rule: ElRule::ExistentialTriggerTarget,
                                premise_facts: vec![
                                    DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                    DerivedFact::Sub(fact.target, d),
                                    DerivedFact::Sub(y, fact.sub),
                                ],
                                axiom_refs: ax_ref.into_iter().collect(),
                            };
                            if let Some(t) = self.proof_trace.as_mut() {
                                t.record(DerivedFact::Sub(y, head), inf);
                            }
                        }
                        self.enqueue_subsumer(y, head);
                    }
                    if self.record_proofs && !self.subsumers.contains(fact.sub, head) {
                        let ax_ref = self
                            .proof_trace
                            .as_ref()
                            .and_then(|t| t.existential_trigger_axiom.get(*tidx).copied().flatten())
                            .map(AxiomRef);
                        let inf = Inference {
                            rule: ElRule::ExistentialTriggerTarget,
                            premise_facts: vec![
                                DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                DerivedFact::Sub(fact.target, d),
                            ],
                            axiom_refs: ax_ref.into_iter().collect(),
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Sub(fact.sub, head), inf);
                        }
                    }
                    // fact.sub itself always has fact.sub ∈ subsumers(sub).
                    self.enqueue_subsumer(fact.sub, head);
                }
            }
        }
        // Existential trigger firing — sub side: when C newly has
        // subsumer D, and D itself has an existential fact, then
        // C inherits that fact's trigger effect for every trigger
        // whose body is already in subsumers(fact.target).
        if let Some(fact_idxs) = Some(self.facts_by_sub[d.index() as usize].clone()) {
            for fidx in fact_idxs {
                let fact = self.facts[fidx];
                let target_subsumers = self.supers_of_class(fact.target);
                for sub in target_subsumers {
                    if let Some(trigger_idxs) =
                        Some(self.existential_triggers_by_body[sub.index() as usize].clone())
                    {
                        for tidx in trigger_idxs {
                            let trigger = self.rules.existential_triggers[tidx];
                            if !self.is_sub_role(fact.role, trigger.role) {
                                continue;
                            }
                            let head = trigger.head;
                            if self.record_proofs && !self.subsumers.contains(c, head) {
                                let ax_ref = self
                                    .proof_trace
                                    .as_ref()
                                    .and_then(|t| {
                                        t.existential_trigger_axiom.get(tidx).copied().flatten()
                                    })
                                    .map(AxiomRef);
                                let inf = Inference {
                                    rule: ElRule::ExistentialTriggerSub,
                                    premise_facts: vec![
                                        DerivedFact::Sub(c, d),
                                        DerivedFact::Exist(d, fact.role, fact.target),
                                        DerivedFact::Sub(fact.target, sub),
                                    ],
                                    axiom_refs: ax_ref.into_iter().collect(),
                                };
                                if let Some(t) = self.proof_trace.as_mut() {
                                    t.record(DerivedFact::Sub(c, head), inf);
                                }
                            }
                            self.enqueue_subsumer(c, trigger.head);
                        }
                    }
                }
                // Domain axiom: if there's a domain for any super
                // of fact.role, C now gets that domain.
                // Snapshot the super-role slice to release the immutable borrow
                // on `self.role_super` before the `enqueue_subsumer` mutable call.
                let fact_role_supers_snap: Vec<RoleId> =
                    supers_of(&self.role_super, fact.role).to_vec();
                for super_role in &fact_role_supers_snap {
                    let doms: Vec<ClassId> = self
                        .rules
                        .role_domains
                        .get(super_role)
                        .cloned()
                        .unwrap_or_default();
                    for dom in doms {
                        if self.record_proofs && !self.subsumers.contains(c, dom) {
                            let ax_ref = self.proof_trace.as_ref().and_then(|t| {
                                t.domain_axiom_refs
                                    .iter()
                                    .find(|(r, d_, _)| r == super_role && *d_ == dom)
                                    .map(|(_, _, idx)| AxiomRef(*idx))
                            });
                            let inf = Inference {
                                rule: ElRule::DomainSub,
                                premise_facts: vec![
                                    DerivedFact::Sub(c, d),
                                    DerivedFact::Exist(d, fact.role, fact.target),
                                ],
                                axiom_refs: ax_ref.into_iter().collect(),
                            };
                            if let Some(t) = self.proof_trace.as_mut() {
                                t.record(DerivedFact::Sub(c, dom), inf);
                            }
                        }
                        self.enqueue_subsumer(c, dom);
                    }
                }
            }
        }
        // Phase 2d: materialize D's existential facts on C in
        // facts_by_sub[c]. When C newly has D as subsumer, every
        // existential fact on D represents a witness that C-instances
        // also have (model-theoretically: C ⊑ D ⇒ every C-instance is a
        // D-instance with the same role witness). Sound by the standard
        // ELK existential-propagation argument; the existing sub-side
        // trigger-firing above (lines 525-557) already exploits this
        // semantically — Phase 2d materializes the fact explicitly so
        // fact-time rules (Phase 2a witness-merge, future Phase 2c-redux,
        // chain rule) can see it on `facts_by_sub[c]`.
        //
        // See docs/phase2d-design.md for soundness + termination.
        let inherit_fact_idxs = self.facts_by_sub[d.index() as usize].clone();
        for fidx in inherit_fact_idxs {
            let fact = self.facts[fidx];
            let inherited = ExistentialFact {
                sub: c,
                role: fact.role,
                target: fact.target,
            };
            // Proof for fact inheritance: Sub(C,D) + Exist(D,r,T) ⟹ Exist(C,r,T)
            if self.record_proofs {
                let df = DerivedFact::Exist(c, fact.role, fact.target);
                if !self.seen_facts.contains(&(c, fact.role, fact.target)) {
                    let inf = Inference {
                        rule: ElRule::FactInheritance,
                        premise_facts: vec![
                            DerivedFact::Sub(c, d),
                            DerivedFact::Exist(d, fact.role, fact.target),
                        ],
                        axiom_refs: vec![],
                    };
                    if let Some(t) = self.proof_trace.as_mut() {
                        t.record(df, inf);
                    }
                }
            }
            if self.push_fact(inherited).is_some() {
                self.phase2d_facts_inherited += 1;
            }
        }
        // Chain rule — `c` is fact1.target side: when a new subsumer
        // `d` lands on `c`, for every fact1 = (A, r1', c) with the
        // chain's r1 in r1's super-roles, and every fact2 = (d, r2',
        // T) whose sub is the new subsumer `d`, derive (A, sup, T)
        // when the chain matches.
        if !self.rules.chain_axioms.is_empty() {
            let head_facts: Vec<ExistentialFact> = self.facts_by_target[c.index() as usize]
                .iter()
                .map(|&i| self.facts[i])
                .collect();
            let tail_facts: Vec<ExistentialFact> = self.facts_by_sub[d.index() as usize]
                .iter()
                .map(|&i| self.facts[i])
                .collect();
            for chain_idx in 0..self.rules.chain_axioms.len() {
                let (r1, r2, sup) = self.rules.chain_axioms[chain_idx];
                for head in &head_facts {
                    if !self.is_sub_role(head.role, r1) {
                        continue;
                    }
                    for tail in &tail_facts {
                        if !self.is_sub_role(tail.role, r2) {
                            continue;
                        }
                        // Proof: Exist(A,r1,C) + Sub(C,D) + Exist(D,r2,T) ⟹ Exist(A,sup,T)
                        if self.record_proofs
                            && !self.seen_facts.contains(&(head.sub, sup, tail.target))
                        {
                            let ax_ref = self
                                .proof_trace
                                .as_ref()
                                .and_then(|t| t.chain_axiom_axiom.get(chain_idx).copied().flatten())
                                .map(AxiomRef);
                            let inf = Inference {
                                rule: ElRule::RoleChainSubsumer,
                                premise_facts: vec![
                                    DerivedFact::Exist(head.sub, head.role, head.target),
                                    DerivedFact::Sub(c, d),
                                    DerivedFact::Exist(d, tail.role, tail.target),
                                ],
                                axiom_refs: ax_ref.into_iter().collect(),
                            };
                            if let Some(t) = self.proof_trace.as_mut() {
                                t.record(DerivedFact::Exist(head.sub, sup, tail.target), inf);
                            }
                        }
                        self.push_fact(ExistentialFact {
                            sub: head.sub,
                            role: sup,
                            target: tail.target,
                        });
                    }
                }
            }
        }
    }

    /// Fire all rules triggered by a freshly-added existential fact.
    #[allow(clippy::too_many_lines)]
    fn process_fact(&mut self, idx: usize) {
        let fact = self.facts[idx];
        // Nominal/ABox transitive propagation: if `fact` is
        // `X ⊑ ∃R.{a}` (target is a NomKey) and `R` is transitive with
        // `a R⁺ b` in the ABox, derive `X ⊑ ∃R.{b}`. Sound: `X R a`,
        // `a R⁺ b`, `R` transitive ⟹ `X R b`. See `build_abox_nominal_reach`.
        if !self.rules.abox_nominal_reach.is_empty()
            && let Some(reach) = self.rules.abox_nominal_reach.get(&(fact.role, fact.target))
        {
            let derived: Vec<ClassId> = reach.clone();
            for b_key in derived {
                // Proof (R15, coarse): premise is the triggering Exist;
                // ABox path axioms are included when abox_path is available.
                if self.record_proofs && !self.seen_facts.contains(&(fact.sub, fact.role, b_key)) {
                    let ax_refs: Vec<AxiomRef> = self
                        .proof_trace
                        .as_ref()
                        .and_then(|t| t.abox_path.get(&(fact.role, fact.target, b_key)).cloned())
                        .unwrap_or_default();
                    let inf = Inference {
                        rule: ElRule::NominalTransitiveProp,
                        premise_facts: vec![DerivedFact::Exist(fact.sub, fact.role, fact.target)],
                        axiom_refs: ax_refs,
                    };
                    if let Some(t) = self.proof_trace.as_mut() {
                        t.record(DerivedFact::Exist(fact.sub, fact.role, b_key), inf);
                    }
                }
                self.push_fact(ExistentialFact {
                    sub: fact.sub,
                    role: fact.role,
                    target: b_key,
                });
            }
        }
        // Cluster-B path (b): functional R + `∃R.{a}` (a ∈ S) ⟹
        // `C ⊑ ForallKey(R,S)`. The unique R-filler (functionality) is the
        // nominal `a`; if `a ∈ S` then `∀R.OneOf(S)` holds. Fire on the fact's
        // own role if functional, and on each functional super-role `R''`
        // (`R ⊑ R''` ⟹ `∃R''.{a}`, functional `R''` ⟹ unique filler `a`).
        // Sound; if `C` also had `∃R.{b}` with b ∉ S, functionality forces a=b →
        // `C` unsat → vacuously ⊑ everything.
        if !self.rules.forall_key_targets.is_empty()
            && let Some(&ind) = self.rules.nominal_to_ind.get(&fact.target)
        {
            // Functional roles among {R} ∪ functional super-roles.
            let mut froles: Vec<RoleId> = Vec::new();
            if self.rules.is_functional(fact.role) {
                froles.push(fact.role);
            }
            froles.extend_from_slice(self.rules.functional_supers_of(fact.role));
            for fr in froles {
                self.enqueue_forall_targets(fact.sub, fr, ind);
            }
            // ≤1-driven: if `C ⊑ ≤1 R` (the `MaxKey(1,R)` marker is a current
            // subsumer of C), the same "unique R-filler is `a`" reasoning applies
            // on R itself even when R is not globally functional. The symmetric
            // direction (subsumer arrives after the fact) is handled in
            // `process_subsumer`.
            if let Some(&mk) = self.rules.max1_key_by_role.get(&fact.role)
                && self.subsumers.contains(fact.sub, mk)
            {
                self.enqueue_forall_targets(fact.sub, fact.role, ind);
            }
        }
        // NOTE: range propagation deliberately omitted.
        //
        // `ObjectPropertyRange(R, C)` is sound for instance reasoning:
        // every actual R-successor is in C. But it does NOT entail that
        // the TYPE used as the existential's target is itself ⊑ C —
        // only the specific instances that *are* R-successors are.
        // From `A ⊑ ∃R.B` + `Range(R) = C`, deriving `B ⊑ C` is
        // unsound (a `B` that isn't anyone's R-successor escapes the
        // range obligation). The prior code emitted exactly that
        // derivation and was the source of the 38 SIO FPs (e.g.
        // `SIO_010085 ⊑ ∃SIO_000225.SIO_000395` + `Range(SIO_000225)
        // = SIO_000017` was producing the false `SIO_000395 ⊑
        // SIO_000017`). A sound range encoding would substitute the
        // existential body with a Tseitin synthetic `B ⊓ C` —
        // future work; safe to drop for now (the orchestrator's
        // tableau path still handles range correctly via its own
        // clausifier).
        // Snapshot the super-role slice to release the immutable borrow on
        // `self.role_super` before the `enqueue_subsumer` mutable calls inside.
        let role_supers_snap: Vec<RoleId> = supers_of(&self.role_super, fact.role).to_vec();
        for super_role in &role_supers_snap {
            // Domain axiom: every class with fact.sub as a subsumer
            // (including fact.sub itself) gains the domain.
            let domains: Vec<ClassId> = self
                .rules
                .role_domains
                .get(super_role)
                .cloned()
                .unwrap_or_default();
            if !domains.is_empty() {
                let candidates = self.subs_of_class(fact.sub);
                for dom in domains {
                    if self.record_proofs && !self.subsumers.contains(fact.sub, dom) {
                        let ax_ref = self.proof_trace.as_ref().and_then(|t| {
                            t.domain_axiom_refs
                                .iter()
                                .find(|(r, d_, _)| r == super_role && *d_ == dom)
                                .map(|(_, _, idx)| AxiomRef(*idx))
                        });
                        let inf = Inference {
                            rule: ElRule::DomainFact,
                            premise_facts: vec![DerivedFact::Exist(
                                fact.sub,
                                fact.role,
                                fact.target,
                            )],
                            axiom_refs: ax_ref.into_iter().collect(),
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Sub(fact.sub, dom), inf);
                        }
                    }
                    self.enqueue_subsumer(fact.sub, dom);
                    for y in &candidates {
                        if self.record_proofs && !self.subsumers.contains(*y, dom) {
                            let ax_ref = self.proof_trace.as_ref().and_then(|t| {
                                t.domain_axiom_refs
                                    .iter()
                                    .find(|(r, d_, _)| r == super_role && *d_ == dom)
                                    .map(|(_, _, idx)| AxiomRef(*idx))
                            });
                            let inf = Inference {
                                rule: ElRule::DomainFact,
                                premise_facts: vec![
                                    DerivedFact::Sub(*y, fact.sub),
                                    DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                ],
                                axiom_refs: ax_ref.into_iter().collect(),
                            };
                            if let Some(t) = self.proof_trace.as_mut() {
                                t.record(DerivedFact::Sub(*y, dom), inf);
                            }
                        }
                        self.enqueue_subsumer(*y, dom);
                    }
                }
            }
        }
        // Unsat propagation: if the target is unsat, the source is
        // unsat (an A-instance would need an r-successor in an
        // empty class).
        if self.subsumers.is_unsatisfiable(fact.target) {
            if self.record_proofs {
                let ci = fact.sub.index() as usize;
                if !self.subsumers.unsatisfiable.contains(ci) {
                    let inf = Inference {
                        rule: ElRule::UnsatTarget,
                        premise_facts: vec![
                            DerivedFact::Exist(fact.sub, fact.role, fact.target),
                            DerivedFact::Unsat(fact.target),
                        ],
                        axiom_refs: vec![],
                    };
                    if let Some(t) = self.proof_trace.as_mut() {
                        t.record(DerivedFact::Unsat(fact.sub), inf);
                    }
                }
            }
            self.enqueue_unsat(fact.sub);
        }
        // Existential triggers (fact side): for each trigger
        // (r', body, head) with fact.role ⊑ r' and body in
        // subsumers(target), every class with fact.sub as a subsumer
        // gains head.
        let target_subsumers = self.supers_of_class(fact.target);
        let candidates = self.subs_of_class(fact.sub);
        for sub in &target_subsumers {
            if let Some(trigger_idxs) =
                Some(self.existential_triggers_by_body[sub.index() as usize].clone())
            {
                for tidx in trigger_idxs {
                    let trigger = self.rules.existential_triggers[tidx];
                    if !self.is_sub_role(fact.role, trigger.role) {
                        continue;
                    }
                    let head = trigger.head;
                    if self.record_proofs && !self.subsumers.contains(fact.sub, head) {
                        let ax_ref = self
                            .proof_trace
                            .as_ref()
                            .and_then(|t| t.existential_trigger_axiom.get(tidx).copied().flatten())
                            .map(AxiomRef);
                        let inf = Inference {
                            rule: ElRule::ExistentialTriggerFact,
                            premise_facts: vec![
                                DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                DerivedFact::Sub(fact.target, *sub),
                            ],
                            axiom_refs: ax_ref.into_iter().collect(),
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Sub(fact.sub, head), inf);
                        }
                    }
                    self.enqueue_subsumer(fact.sub, head);
                    for y in &candidates {
                        if self.record_proofs && !self.subsumers.contains(*y, head) {
                            let ax_ref = self
                                .proof_trace
                                .as_ref()
                                .and_then(|t| {
                                    t.existential_trigger_axiom.get(tidx).copied().flatten()
                                })
                                .map(AxiomRef);
                            let inf = Inference {
                                rule: ElRule::ExistentialTriggerFact,
                                premise_facts: vec![
                                    DerivedFact::Sub(*y, fact.sub),
                                    DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                    DerivedFact::Sub(fact.target, *sub),
                                ],
                                axiom_refs: ax_ref.into_iter().collect(),
                            };
                            if let Some(t) = self.proof_trace.as_mut() {
                                t.record(DerivedFact::Sub(*y, head), inf);
                            }
                        }
                        self.enqueue_subsumer(*y, head);
                    }
                }
            }
        }
        // Chain rule: pair with existing facts.
        for chain_idx in 0..self.rules.chain_axioms.len() {
            let (r1, r2, sup) = self.rules.chain_axioms[chain_idx];
            let role_in_r1 = self.is_sub_role(fact.role, r1);
            let role_in_r2 = self.is_sub_role(fact.role, r2);
            if role_in_r1 {
                // This fact is the head; pair with tails whose sub
                // is a subsumer of fact.target.
                let target_subs = target_subsumers.clone();
                for sub in &target_subs {
                    let tail_idxs = self.facts_by_sub[sub.index() as usize].clone();
                    for tidx in tail_idxs {
                        let tail = self.facts[tidx];
                        if self.is_sub_role(tail.role, r2) {
                            if self.record_proofs
                                && !self.seen_facts.contains(&(fact.sub, sup, tail.target))
                            {
                                let ax_ref = self
                                    .proof_trace
                                    .as_ref()
                                    .and_then(|t| {
                                        t.chain_axiom_axiom.get(chain_idx).copied().flatten()
                                    })
                                    .map(AxiomRef);
                                let inf = Inference {
                                    rule: ElRule::RoleChainFact,
                                    premise_facts: vec![
                                        DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                        DerivedFact::Exist(tail.sub, tail.role, tail.target),
                                    ],
                                    axiom_refs: ax_ref.into_iter().collect(),
                                };
                                if let Some(t) = self.proof_trace.as_mut() {
                                    t.record(DerivedFact::Exist(fact.sub, sup, tail.target), inf);
                                }
                            }
                            self.push_fact(ExistentialFact {
                                sub: fact.sub,
                                role: sup,
                                target: tail.target,
                            });
                        }
                    }
                }
            }
            if role_in_r2 {
                // This fact is the tail; pair with heads whose
                // target has fact.sub as a subsumer.
                let candidates = candidates.clone();
                let mut head_targets: Vec<ClassId> = candidates;
                head_targets.push(fact.sub);
                for cand in head_targets {
                    let head_idxs = self.facts_by_target[cand.index() as usize].clone();
                    for hidx in head_idxs {
                        let head_fact = self.facts[hidx];
                        if self.is_sub_role(head_fact.role, r1) {
                            if self.record_proofs
                                && !self.seen_facts.contains(&(head_fact.sub, sup, fact.target))
                            {
                                let ax_ref = self
                                    .proof_trace
                                    .as_ref()
                                    .and_then(|t| {
                                        t.chain_axiom_axiom.get(chain_idx).copied().flatten()
                                    })
                                    .map(AxiomRef);
                                let inf = Inference {
                                    rule: ElRule::RoleChainFact,
                                    premise_facts: vec![
                                        DerivedFact::Exist(
                                            head_fact.sub,
                                            head_fact.role,
                                            head_fact.target,
                                        ),
                                        DerivedFact::Exist(fact.sub, fact.role, fact.target),
                                    ],
                                    axiom_refs: ax_ref.into_iter().collect(),
                                };
                                if let Some(t) = self.proof_trace.as_mut() {
                                    t.record(
                                        DerivedFact::Exist(head_fact.sub, sup, fact.target),
                                        inf,
                                    );
                                }
                            }
                            self.push_fact(ExistentialFact {
                                sub: head_fact.sub,
                                role: sup,
                                target: fact.target,
                            });
                        }
                    }
                }
            }
        }
        // Phase 2a EL++ functional-role witness-merge rule (T4.5
        // atom-set redesign). For each functional super-role R_f of
        // `fact.role`, accumulate `fact.target`'s atomic content into
        // the (sub, R_f) atom set; if it grew (and it isn't the first
        // arrival), allocate a synthetic for the FLAT set and emit a
        // new fact (sub, R_f, synthetic). Termination: the atom set
        // is monotonically bounded by the atomic vocabulary, so per
        // (sub, R_f) the rule fires at most |atomic_vocabulary| times.
        let funcs: Vec<RoleId> = self.rules.functional_supers_of(fact.role).to_vec();
        if !funcs.is_empty() {
            let new_atoms = self.atomic_content_of_or_self(fact.target);
            for rf in funcs {
                let key = (fact.sub, rf);
                // R21: update merge_contributors before we borrow prev_set
                if self.record_proofs {
                    let contrib = self.proof_trace.as_mut().map(|t| &mut t.merge_contributors);
                    if let Some(c) = contrib {
                        c.entry(key).or_default().push(DerivedFact::Exist(
                            fact.sub,
                            fact.role,
                            fact.target,
                        ));
                    }
                }
                let prev_set = self.merged_atom_sets.entry(key).or_default();
                let was_first = prev_set.is_empty();
                let grew = !new_atoms.is_subset(prev_set);
                if grew {
                    prev_set.extend(&new_atoms);
                }
                if was_first || !grew {
                    // First-arrival is mute, non-growing is no-op.
                    //
                    // Soundness rationale for was_first: a SINGLE
                    // sub-role fact `(sub, R_i, A)` with R_i ⊑ R_f
                    // doesn't yet exercise functionality — it just
                    // asserts an R_f-witness exists in A. CR9 role-
                    // hierarchy propagation already emits the derived
                    // `(sub, R_f, A)` fact, so no merge synthetic is
                    // needed to recover the entailment. The witness-
                    // merge rule's payoff only starts when a SECOND
                    // sub-role fact arrives, forcing the two witnesses
                    // to coincide by functionality.
                    continue;
                }
                // Snapshot the now-grown set as a sorted Vec to pass
                // to the allocator (which sorts+dedups internally, but
                // we already have it sorted via BTreeSet).
                let body: Vec<ClassId> = prev_set.iter().copied().collect();
                let synthetic = self.introduce_runtime_synthetic(body);
                let new_fact = ExistentialFact {
                    sub: fact.sub,
                    role: rf,
                    target: synthetic,
                };
                let dedup_key = (new_fact.sub, new_fact.role, new_fact.target);
                if self.seen_facts.insert(dedup_key) {
                    // R21 proof: all contributing facts + functional role axiom.
                    if self.record_proofs {
                        let premises: Vec<DerivedFact> = self
                            .proof_trace
                            .as_ref()
                            .and_then(|t| t.merge_contributors.get(&key).cloned())
                            .unwrap_or_default();
                        let ax_ref = self
                            .proof_trace
                            .as_ref()
                            .and_then(|t| t.functional_role_axiom.get(&rf).copied())
                            .map(AxiomRef);
                        let inf = Inference {
                            rule: ElRule::FunctionalMerge,
                            premise_facts: premises,
                            axiom_refs: ax_ref.into_iter().collect(),
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Exist(fact.sub, rf, synthetic), inf);
                        }
                    }
                    let new_idx = self.facts.len();
                    self.facts.push(new_fact);
                    self.facts_by_sub[new_fact.sub.index() as usize].push(new_idx);
                    self.facts_by_target[new_fact.target.index() as usize].push(new_idx);
                    self.todo_fact.push_back(new_idx);
                }
                // Phase 2c-redux (restored on top of Phase 2d): propagate
                // the merged synthetic back to sub-roles X has facts on.
                // With Phase 2d, `facts_by_sub[X]` now includes inherited
                // facts from X's super-classes, so this loop has the
                // preconditions to fire even when X doesn't directly
                // assert the existential.
                //
                // Soundness (witness-coincidence): any existing
                // `(X, R_k, _)` fact has its R_k-witness coinciding with
                // the R_f-witness by functionality of R_f, so X already
                // has the merged atom-set content via R_k. Inherited
                // facts preserve the model-theoretic witness existence
                // (C ⊑ D ⇒ every C-instance is a D-instance with the
                // same witness — see docs/phase2d-design.md §Soundness;
                // docs/phase2c-fix-target.md §"Rule design" for the
                // original argument).
                //
                // Phase 2e: we DO emit on the merge-triggering role
                // (`other.role == fact.role`) too. Pre-2e skipped it,
                // reasoning CR9 hierarchy propagation already covered
                // R_arr — but CR9 only propagates the *original* witness
                // `target` UP to the super-role `R_f`; it does NOT push
                // the merged *synthetic* DOWN to R_arr. When the
                // existential body lives on R_arr itself (notgalen IPBP:
                // `∃hasIntrinsicPathologicalStatus.pathological`), the
                // merged filler must land on R_arr or the fold never
                // fires — an order-dependent miss (whichever sub-role's
                // fact was processed second triggered the merge and was
                // then the only role NOT to receive the synthetic). See
                // `functional_role_merge_body_on_sub_role`.
                //
                // Soundness: by functionality of `R_f`, EVERY sub-role
                // witness (including R_arr's) coincides with the single
                // `R_f`-successor that carries the full merged atom set,
                // so `(sub, R_arr, synthetic)` holds in every model.
                //
                // Re-using `push_fact` here (vs the manual insertion
                // pattern Phase 2c originally used) means each emitted
                // `(X, R_k, synthetic)` also recursively inherits to X's
                // subclasses via Phase 2d — sound by the same witness-
                // inheritance argument. We snapshot `facts_by_sub[fact.sub]`
                // before iterating because `push_fact` writes into it.
                let facts_snapshot = self.facts_by_sub[fact.sub.index() as usize].clone();
                for other_idx in facts_snapshot {
                    let other = self.facts[other_idx];
                    if !self.is_sub_role(other.role, rf) || !self.rules.is_functional(rf) {
                        continue;
                    }
                    // R22: back-prop
                    if self.record_proofs
                        && !self.seen_facts.contains(&(fact.sub, other.role, synthetic))
                    {
                        let ax_ref = self
                            .proof_trace
                            .as_ref()
                            .and_then(|t| t.functional_role_axiom.get(&rf).copied())
                            .map(AxiomRef);
                        let inf = Inference {
                            rule: ElRule::FunctionalMergeSubRole,
                            premise_facts: vec![
                                DerivedFact::Exist(fact.sub, other.role, other.target),
                                DerivedFact::Exist(fact.sub, rf, synthetic),
                            ],
                            axiom_refs: ax_ref.into_iter().collect(),
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Exist(fact.sub, other.role, synthetic), inf);
                        }
                    }
                    let prop_fact = ExistentialFact {
                        sub: fact.sub,
                        role: other.role,
                        target: synthetic,
                    };
                    if self.push_fact(prop_fact).is_some() {
                        self.phase2c_sub_role_propagations += 1;
                    }
                }
            }
        }
    }

    /// Fire all rules triggered by `c` becoming unsatisfiable.
    fn process_unsat(&mut self, c: ClassId) {
        let ci = c.index() as usize;
        if self.subsumers.unsatisfiable.put(ci) {
            // already flagged
            return;
        }
        // Every class with c as a subsumer is also unsat.
        let dependents = self.subs_of_class(c);
        for d in dependents {
            // Proof (R23): D ⊑ C, C ⊑ ⊥ ⟹ D ⊑ ⊥
            if self.record_proofs {
                let di = d.index() as usize;
                if !self.subsumers.unsatisfiable.contains(di) {
                    let inf = Inference {
                        rule: ElRule::UnsatSubclass,
                        premise_facts: vec![DerivedFact::Unsat(c), DerivedFact::Sub(d, c)],
                        axiom_refs: vec![],
                    };
                    if let Some(t) = self.proof_trace.as_mut() {
                        t.record(DerivedFact::Unsat(d), inf);
                    }
                }
            }
            self.enqueue_unsat(d);
        }
        // Every fact with c as its target makes its source unsat.
        if let Some(fact_idxs) = Some(self.facts_by_target[c.index() as usize].clone()) {
            for fidx in fact_idxs {
                let fact = self.facts[fidx];
                // Proof (R24): (D,r,C), C ⊑ ⊥ ⟹ D ⊑ ⊥
                if self.record_proofs {
                    let di = fact.sub.index() as usize;
                    if !self.subsumers.unsatisfiable.contains(di) {
                        let inf = Inference {
                            rule: ElRule::UnsatFactSource,
                            premise_facts: vec![
                                DerivedFact::Unsat(c),
                                DerivedFact::Exist(fact.sub, fact.role, fact.target),
                            ],
                            axiom_refs: vec![],
                        };
                        if let Some(t) = self.proof_trace.as_mut() {
                            t.record(DerivedFact::Unsat(fact.sub), inf);
                        }
                    }
                }
                self.enqueue_unsat(fact.sub);
            }
        }
        // SP-B2a: if `c` is a `Sᵢ = C⊓Dᵢ` synthetic that just became unsat, recompute
        // its disjunction's survivors (disjuncts whose synthetic is not unsat) and
        // force. `c` is already flagged unsat above, so it counts as excluded here.
        // Clone the entry's slices first to release the borrow before enqueuing.
        if let Some(&di) = self.b2_synth_to_disj.get(&c)
            && !self.b2_disjunctions[di].fired
        {
            let synth = self.b2_disjunctions[di].synthetics.clone();
            let disjuncts = self.b2_disjunctions[di].disjuncts.clone();
            let class = self.b2_disjunctions[di].class;
            let mut surv: Option<ClassId> = None;
            let mut count = 0u32;
            for (k, &s) in synth.iter().enumerate() {
                if !self.subsumers.is_unsatisfiable(s) {
                    count += 1;
                    surv = disjuncts.get(k).copied();
                }
            }
            match (count, surv) {
                (0, _) => {
                    self.b2_disjunctions[di].fired = true;
                    self.enqueue_unsat(class);
                }
                (1, Some(s)) => {
                    self.b2_disjunctions[di].fired = true;
                    self.enqueue_subsumer(class, s);
                }
                _ => {}
            }
        }
    }
}

/// Look up the reflexive-transitive super-role closure for `r`.
///
/// Returns a zero-alloc `&[RoleId]` slice from the dense Vec indexed by
/// `r.index()`. Returns `&[]` for any out-of-bounds index (unreachable
/// for vocabulary roles, which all lie in `0..role_super.len()`).
fn supers_of(role_super: &[Box<[RoleId]>], r: RoleId) -> &[RoleId] {
    role_super
        .get(r.index() as usize)
        .map_or(&[], |b| b.as_ref())
}

/// Subsumer closure: for each class `C`, the set of named classes
/// `D` such that `C ⊑ D` is entailed by the EL-fragment subset of
/// the input ontology.
///
/// **Soundness:** every entry is a genuine entailment.
/// **Completeness:** only complete *for the EL fragment of the
/// input*. Axioms outside EL (union, complement, cardinality,
/// nominals) are not consulted; if a subsumption depends on those,
/// the table will miss it.
#[derive(Debug, Clone)]
pub struct Subsumers {
    /// One `FixedBitSet` per class — `subsumers[i].contains(j)` is
    /// true iff `class_i ⊑ class_j`. Each bitset is sized for the
    /// full class universe (including Tseitin synthetic classes
    /// allocated above the user vocabulary). Dense representation
    /// gives O(1) `contains` and avoids the per-class
    /// `HashSet<ClassId>` allocation overhead the previous
    /// implementation paid.
    subsumers: Vec<FixedBitSet>,
    /// Bit i set iff `class_i ⊑ ⊥`.
    unsatisfiable: FixedBitSet,
}

impl Default for Subsumers {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

impl Subsumers {
    fn with_capacity(n: usize) -> Self {
        let mut subsumers = Vec::with_capacity(n);
        for _ in 0..n {
            subsumers.push(FixedBitSet::with_capacity(n));
        }
        Self {
            subsumers,
            unsatisfiable: FixedBitSet::with_capacity(n),
        }
    }

    fn class_index(c: ClassId) -> usize {
        c.index() as usize
    }

    /// True iff the closure contains `sub ⊑ sup`.
    #[must_use]
    pub fn contains(&self, sub: ClassId, sup: ClassId) -> bool {
        let si = Self::class_index(sub);
        let pi = Self::class_index(sup);
        self.subsumers
            .get(si)
            .is_some_and(|bs| pi < bs.len() && bs.contains(pi))
    }

    /// Every entailed subsumer of `c` (including `c` itself).
    #[must_use]
    pub fn subsumers_of(&self, c: ClassId) -> Vec<ClassId> {
        let ci = Self::class_index(c);
        self.subsumers
            .get(ci)
            .map(|bs| {
                bs.ones()
                    .map(|i| ClassId::new(u32::try_from(i).expect("class id fits in u32")))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A reference to the raw subsumer bitset for class `c`.
    /// The bitset may be wider than the user class vocabulary (it
    /// includes Tseitin / `DKey` synthetic IDs ≥ n). Callers that want
    /// only user-vocabulary subsumers must restrict to `[0, n)`.
    #[must_use]
    pub fn subsumers_bitset(&self, c: ClassId) -> Option<&FixedBitSet> {
        self.subsumers.get(Self::class_index(c))
    }

    /// A reference to the raw unsatisfiable bitset.
    /// Bit `i` set iff `class_i ⊑ ⊥` according to saturation.
    #[must_use]
    pub fn unsatisfiable_bitset(&self) -> &FixedBitSet {
        &self.unsatisfiable
    }

    /// True iff saturation proved `c` is empty in every model (i.e.
    /// `c ⊑ ⊥`).
    #[must_use]
    pub fn is_unsatisfiable(&self, c: ClassId) -> bool {
        let ci = Self::class_index(c);
        ci < self.unsatisfiable.len() && self.unsatisfiable.contains(ci)
    }

    /// Every class flagged as `⊑ ⊥` by the saturation pass.
    #[must_use]
    pub fn unsatisfiable_classes(&self) -> Vec<ClassId> {
        self.unsatisfiable
            .ones()
            .map(|i| ClassId::new(u32::try_from(i).expect("class id fits in u32")))
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
struct ElRules {
    /// Direct named-to-named `A ⊑ B` facts.
    atomic_subsumptions: Vec<AtomicSubsumption>,
    /// Atomic subsumers of `⊤` (from `SubClassOf(owl:Thing, C)` / `⊤ ⊑ C⊓…`).
    /// Every named class ⊑ each of these (C is Top-equivalent). Broadcast to all
    /// classes in `seed`; empty unless the ontology has a `⊤ ⊑ NamedClass` axiom.
    top_subsumers: Vec<ClassId>,
    /// Conjunctive triggers: when a class accumulates every `body`
    /// among its subsumers, it gains `head`.
    conjunctive_triggers: Vec<ConjunctiveTrigger>,
    /// Existential facts from `SubClassOf(sub, ∃role.target)` over
    /// atomic-named-atomic shapes. Read as "every `sub`-instance has
    /// some `role`-successor whose subsumers include `target`."
    existential_facts: Vec<ExistentialFact>,
    /// Existential triggers from `SubClassOf(∃role.body, head)` over
    /// atomic-named-atomic shapes. Read as "any class with an
    /// existential `role`-successor in `body` is also in `head`."
    existential_triggers: Vec<ExistentialTrigger>,
    /// Pairwise disjoint atomic-class pairs, decomposed from n-ary
    /// `DisjointClasses` axioms. Read as `A ⊓ B ⊑ ⊥`.
    disjoint_pairs: Vec<(ClassId, ClassId)>,
    /// Nominal-reasoning support (wine region cluster). For a
    /// **transitive** role `R` and a nominal-key class `NomKey(a)`
    /// (synthetic stand-in for the singleton `{a}`),
    /// `abox_nominal_reach[(R, NomKey(a))]` lists `NomKey(b)` for every
    /// individual `b` reachable from `a` via the transitive closure of
    /// `R` over the named-individual `ABox`. Lets a fact
    /// `X ⊑ ∃R.{a}` derive `X ⊑ ∃R.{b}` (sound: `X R a`, `a R⁺ b`,
    /// `R` transitive ⟹ `X R b`). Empty unless the ontology has both
    /// nominal existential bodies and transitive-role `ABox` edges.
    abox_nominal_reach: std::collections::HashMap<(RoleId, ClassId), Vec<ClassId>>,
    /// Cluster-B path (b): `ForallKey` targets indexed by `(role, member)`.
    /// `forall_key_targets[(R, a)]` lists the `ForallKey(R,S)` synthetic class
    /// ids with `a ∈ S`. When a fact `C ⊑ ∃R.{a}` is processed and `R` (or a
    /// functional super-role) is functional, each such key is enqueued as a
    /// subsumer of `C` — sound: functional + `∃R.{a}` ⟹ the unique R-filler is
    /// `a ∈ S` ⟹ `C ⊑ ∀R.OneOf(S)`. Empty unless the ontology has both
    /// `∀R.OneOf` defined-class conjuncts and functional roles.
    forall_key_targets: std::collections::HashMap<(RoleId, IndividualId), Vec<ClassId>>,
    /// Reverse of `TseitinAllocator::nominal_by_ind`: a `NomKey` synthetic class
    /// id back to its individual, so `process_fact` can recover `a` from a
    /// `∃R.{a}` fact's target. Used only by the path-(b) rule above.
    nominal_to_ind: std::collections::HashMap<ClassId, IndividualId>,
    /// `MaxKey(1, R)` synthetic ids by role — the `≤1 R` markers. Path (b)'s
    /// `≤1`-driven variant: a told/derived `C ⊑ ≤1 R` (per-class, R need NOT be
    /// globally functional) + `∃R.{a}` (a∈S) ⟹ `∀R.OneOf(S)` (the unique R-filler
    /// is `a`). `process_fact` checks `MaxKey(1,R) ∈ supers(C)`; `process_subsumer`
    /// fires the symmetric direction when the `≤1 R` subsumer arrives.
    max1_key_by_role: std::collections::HashMap<RoleId, ClassId>,
    /// Reverse of `max1_key_by_role` (the `MaxKey(1,R)` class id back to its role),
    /// so `process_subsumer` can detect a `≤1 R` subsumer and fire path (b).
    max1_role_by_key: std::collections::HashMap<ClassId, RoleId>,
    /// Atomic classes told directly to be unsatisfiable via
    /// `SubClassOf(Atomic(C), Bot)`. Seeded into the unsat worklist
    /// at `seed` time so the standard `process_unsat` propagation
    /// rules fire (subclass + fact-target-of-c → also unsat).
    /// Phase D4 (2026-06-03): added to support the data-axiom
    /// preprocessing pass's emitted `C ⊑ Bot` axioms (Functional + ≥n
    /// clash; `DataMin` > `DataMax` clash).
    directly_unsat: Vec<ClassId>,
    /// Per-role domain classes: `role_domains[r]` holds the atomic
    /// classes `C` such that any `r`-source belongs to `C`. Lowered
    /// from `ObjectPropertyDomain(r, C)` with named `r` and atomic
    /// `C`. Equivalent to `∃r.⊤ ⊑ C`.
    role_domains: HashMap<RoleId, Vec<ClassId>>,
    /// Per-role range classes: `role_ranges[r]` holds the atomic
    /// classes `C` such that any `r`-target belongs to `C`. Lowered
    /// from `ObjectPropertyRange(r, C)` with named `r` and atomic
    /// `C`. Equivalent to `⊤ ⊑ ∀r.C`; in EL we only consult it on
    /// edges that actually appear (the existential-fact targets).
    role_ranges: HashMap<RoleId, Vec<ClassId>>,
    /// Role chain axioms `r₁ ∘ r₂ ⊑ sup`. Lowered from
    /// `SubObjectPropertyOf(ObjectPropertyChain(r₁ r₂), sup)` with
    /// length-2 named roles end-to-end, and from
    /// `TransitiveObjectProperty(r)` as `(r, r, r)`. Longer chains
    /// and inverse-role chains are dropped — those stay in the
    /// tableau's lane.
    chain_axioms: Vec<(RoleId, RoleId, RoleId)>,
    /// Roles declared `FunctionalObjectProperty(...)`. Indexed by role
    /// id (dense bitset for O(1) lookup). Phase 2a EL++ rule input.
    functional_roles: FixedBitSet,
    /// Per-role precomputed list of FUNCTIONAL super-roles in the
    /// transitive closure: `functional_supers_of[r]` lists every
    /// functional role `R_f` such that `r ⊑ R_f` (reflexive: r itself
    /// if functional). Precomputed once at collection time so the
    /// runtime worklist rule doesn't re-walk `role_super` on every new
    /// existential fact. Empty for roles with no functional ancestor.
    functional_supers_of: Vec<Vec<RoleId>>,
    /// SP-B1: per-class atomic disjunctions on the RHS. `disjunctions_by_class[C]`
    /// holds each `[D₁,…,Dₙ]` from a `C ⊑ D₁⊔…⊔Dₙ` axiom whose disjuncts are ALL
    /// atomic (≥2). The derived-closure forced-disjunct rule (`process_subsumer`)
    /// excludes any disjunct disjoint with a current subsumer of the class; one
    /// survivor ⟹ force it, none ⟹ unsat. Sound by construction; atomic-only
    /// (nominal `⊔` deferred to B3). Empty ⇒ the rule is a no-op (EL/Horn corpus).
    disjunctions_by_class: HashMap<ClassId, Vec<Box<[ClassId]>>>,
}

impl ElRules {
    /// True if `r` is declared `FunctionalObjectProperty`.
    fn is_functional(&self, r: RoleId) -> bool {
        let i = r.index() as usize;
        i < self.functional_roles.len() && self.functional_roles.contains(i)
    }

    /// Precomputed: every functional role `R_f` with `r ⊑ R_f`.
    /// Empty slice if `r` has no functional ancestor.
    fn functional_supers_of(&self, r: RoleId) -> &[RoleId] {
        let i = r.index() as usize;
        if let Some(v) = self.functional_supers_of.get(i) {
            v.as_slice()
        } else {
            &[]
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct AtomicSubsumption {
    sub: ClassId,
    sup: ClassId,
}

#[derive(Debug, Clone)]
struct ConjunctiveTrigger {
    bodies: Vec<ClassId>,
    head: ClassId,
}

#[derive(Debug, Copy, Clone)]
struct ExistentialFact {
    sub: ClassId,
    role: RoleId,
    target: ClassId,
}

#[derive(Debug, Copy, Clone)]
struct ExistentialTrigger {
    role: RoleId,
    body: ClassId,
    head: ClassId,
}

/// Tseitin-style allocator for synthetic atomic classes that stand
/// in for compound `And(of atomics)` bodies inside existential
/// positions.
///
/// When the lowerer sees `∃r.(B₁ ⊓ … ⊓ Bₙ)` (where every `Bᵢ` is
/// atomic) it requests a synthetic `F` from this allocator. The
/// allocator returns a stable id for that body and, on first
/// introduction, emits two paired clauses into the EL rule set:
///
/// - `F ⊑ Bᵢ` for each operand (so anything provably-`F` inherits
///   each operand as a subsumer);
/// - `B₁ ⊓ … ⊓ Bₙ ⊑ F` (a conjunctive trigger, so anything that
///   has all of the operands as subsumers also has `F`).
///
/// Together those clauses define `F ≡ B₁ ⊓ … ⊓ Bₙ`, so the existing
/// CR5 propagation over `∃r.F` produces exactly the same closure as
/// it would on `∃r.(B₁ ⊓ … ⊓ Bₙ)`. Synthetic class ids start at
/// `num_original_classes` and never collide with user-declared
/// class ids; they don't leak into the public `Subsumers` API
/// because callers iterate over `0..num_classes` only.
#[derive(Debug, Clone)]
struct TseitinAllocator {
    next_id: u32,
    by_body: HashMap<Vec<ClassId>, ClassId>,
    /// Cache for existential markers used to lower LHS conjunctions
    /// containing existential operands (e.g. `∃R.B ⊓ A ⊑ C`). Keyed
    /// by `(role, body_class_id)` so repeated occurrences of the same
    /// `∃R.B` shape across different conjunctions share one marker.
    by_existential: HashMap<(RoleId, ClassId), ClassId>,
    /// Cache for markers standing in for a *disjunctive* existential body
    /// `∃R.(C1 ⊔ … ⊔ Cn)` used as an LHS conjunct. Keyed by `(role, sorted
    /// alternatives)`. CRITICAL: this is a SEPARATE namespace from
    /// `by_existential` — a union marker must never be the same synthetic as a
    /// singleton `∃R.Ci` marker, or emitting the other alternatives' triggers
    /// onto it would corrupt that singleton's meaning (an unsound FP — a class
    /// with `∃R.Cj` would spuriously gain the `∃R.Ci` marker).
    by_union_existential: HashMap<(RoleId, Vec<ClassId>), ClassId>,
    /// Stable synthetic atomic class per individual used as a nominal
    /// (`{a}`) in an existential body. Treated as an opaque atom
    /// (no subsumers, no triggers) — a 1:1 structural stand-in so the
    /// EL fold of `C ≡ D ⊓ ∃R.{a}` fires on the same key the fact
    /// `X ⊑ ∃R.{a}` produced. Injective, so no two individuals merge.
    nominal_by_ind: HashMap<IndividualId, ClassId>,
    /// Stable synthetic atomic class per unqualified `≤n R` cardinality
    /// restriction (`(n, R)`), used as a structural stand-in (cluster-C lever,
    /// wine residual-29). Like `nominal_by_ind`: an opaque atom. `C ⊑ MaxKey(n,R)`
    /// is seeded iff `C ⊑ ≤n R` is told, and a defined class's `≤n R` conjunct
    /// lowers to the SAME key, so the conjunctive trigger for that definition
    /// fires only when the cardinality conjunct genuinely holds. Sound: keyed on
    /// `(n, R)` identity; exact match only (no `≤m ⊑ ≤n` cross-`n`), qualifier
    /// must be `⊤` (unqualified). See `docs/classify-recovery-scope-2026-06-07.md`.
    max_key_by_role: HashMap<(u32, RoleId), ClassId>,
    /// Stable synthetic atomic class per `∀R.OneOf(S)` universal-over-nominal-set
    /// restriction (`(R, sorted S)`), used as a structural stand-in (cluster-B
    /// lever, wine residual-9). Same opaque-atom discipline as `max_key_by_role`:
    /// `C ⊑ ForallKey(R,S)` is seeded iff `C ⊑ ∀R.OneOf(S)` is told, and a
    /// defined class's `∀R.OneOf(S)` conjunct lowers to the SAME key, so its
    /// conjunctive trigger fires only when the universal conjunct genuinely
    /// holds. Sound: keyed on `(R, exactly-S)` identity (no subset `∀R.S' ⊑
    /// ∀R.S` lattice — under-approximation), non-inverse, `OneOf`-of-nominals.
    forall_key_by_role: HashMap<(RoleId, Vec<IndividualId>), ClassId>,
    /// SP-B2b: opaque synthetic class per `∀R.Atomic(K)` (the general-∀ analog of
    /// `forall_key_by_role`). `C ⊑ ForallAtomicKey(R,K)` iff `C ⊑ ∀R.K` is told /
    /// subsumption-propagated; a defined class's `∀R.K` conjunct lowers to the SAME
    /// key. Told monotonicity edges `ForallAtomicKey(R,K) ⊑ ForallAtomicKey(R,L)` for
    /// `K ⊑ L` give `∀R.K ⊑ ∀R.L` (sound, non-inverse). Keyed on `(R, K)` identity.
    forall_atomic_key_by_role: HashMap<(RoleId, ClassId), ClassId>,
    /// Stable synthetic ⊤-witness class per role, used as the fact target for a
    /// `∃R.⊤` existential (`ObjectSomeValuesFrom(R, owl:Thing)` on the RHS). ⊤ has
    /// no `atomic_or_tseitin_body` representation, so `A ⊑ ∃R.⊤` previously created
    /// no fact — A never got an R-marker, so the domain rule (`∃R.⊤ ⊑ C`, i.e.
    /// domain(R)=C) never fired on A. The witness is an opaque atom (no subsumers →
    /// ⊤-equivalent), so it only ever triggers domain(R); sound. Keyed per role so
    /// all `∃R.⊤` facts share one witness (they mean the same thing).
    top_witness_by_role: HashMap<RoleId, ClassId>,
}

impl TseitinAllocator {
    fn new(num_original_classes: usize) -> Self {
        Self {
            next_id: u32::try_from(num_original_classes).expect("class count fits in u32"),
            by_body: HashMap::new(),
            by_existential: HashMap::new(),
            by_union_existential: HashMap::new(),
            nominal_by_ind: HashMap::new(),
            max_key_by_role: HashMap::new(),
            forall_key_by_role: HashMap::new(),
            forall_atomic_key_by_role: HashMap::new(),
            top_witness_by_role: HashMap::new(),
        }
    }

    /// Get-or-allocate the opaque ⊤-witness synthetic for `∃R.⊤`. See
    /// `top_witness_by_role`.
    fn introduce_top_witness(&mut self, role: RoleId) -> ClassId {
        if let Some(&existing) = self.top_witness_by_role.get(&role) {
            return existing;
        }
        let synthetic = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        self.top_witness_by_role.insert(role, synthetic);
        synthetic
    }

    /// SP-B2b: get-or-allocate the opaque synthetic class for `∀R.Atomic(K)`.
    /// Keyed on `(R, K)`; mirror of `introduce_forall_key`.
    fn introduce_forall_atomic_key(&mut self, role: RoleId, k: ClassId) -> ClassId {
        if let Some(&existing) = self.forall_atomic_key_by_role.get(&(role, k)) {
            return existing;
        }
        let synthetic = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        self.forall_atomic_key_by_role.insert((role, k), synthetic);
        synthetic
    }

    /// Get-or-allocate the opaque synthetic atomic class standing in for
    /// the nominal `{ind}`. Sound: matching is by individual identity
    /// (structural), so `∃R.{a}` folds only against the same `a`.
    fn introduce_nominal(&mut self, ind: IndividualId) -> ClassId {
        if let Some(&existing) = self.nominal_by_ind.get(&ind) {
            return existing;
        }
        let synthetic = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        self.nominal_by_ind.insert(ind, synthetic);
        synthetic
    }

    /// Get-or-allocate the opaque synthetic class standing in for an
    /// unqualified `≤n R`. See `max_key_by_role`.
    fn introduce_max_key(&mut self, n: u32, role: RoleId) -> ClassId {
        if let Some(&existing) = self.max_key_by_role.get(&(n, role)) {
            return existing;
        }
        let synthetic = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        self.max_key_by_role.insert((n, role), synthetic);
        synthetic
    }

    /// Get-or-allocate the opaque synthetic class for `∀R.OneOf(S)`. `members`
    /// is sorted+deduped for a canonical key. See `forall_key_by_role`.
    fn introduce_forall_key(&mut self, role: RoleId, mut members: Vec<IndividualId>) -> ClassId {
        members.sort();
        members.dedup();
        if let Some(&existing) = self.forall_key_by_role.get(&(role, members.clone())) {
            return existing;
        }
        let synthetic = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        self.forall_key_by_role.insert((role, members), synthetic);
        synthetic
    }

    fn introduce(&mut self, mut body: Vec<ClassId>, rules: &mut ElRules) -> ClassId {
        body.sort();
        body.dedup();
        if let Some(&existing) = self.by_body.get(&body) {
            return existing;
        }
        let synthetic = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        for &b in &body {
            rules.atomic_subsumptions.push(AtomicSubsumption {
                sub: synthetic,
                sup: b,
            });
        }
        rules.conjunctive_triggers.push(ConjunctiveTrigger {
            bodies: body.clone(),
            head: synthetic,
        });
        self.by_body.insert(body, synthetic);
        synthetic
    }

    /// Allocate (or reuse) a one-way marker class `F` for `∃R.B` used
    /// inside an LHS conjunction. Emits the trigger `∃R.B ⊑ F`. **Does
    /// not** emit the reverse `F ⊑ ∃R.B`: F is a marker meaning "has an
    /// R-edge to a B", not equivalent to the existential.
    fn introduce_existential_marker(
        &mut self,
        role: RoleId,
        body: ClassId,
        rules: &mut ElRules,
    ) -> ClassId {
        if let Some(&existing) = self.by_existential.get(&(role, body)) {
            return existing;
        }
        let marker = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        rules.existential_triggers.push(ExistentialTrigger {
            role,
            body,
            head: marker,
        });
        self.by_existential.insert((role, body), marker);
        marker
    }

    /// Get-or-allocate a FRESH synthetic marker for a *disjunctive*
    /// existential body `∃R.(C1 ⊔ … ⊔ Cn)` appearing as an LHS conjunct.
    /// Emits `∃R.Ci ⊑ marker` for every alternative, so any single
    /// alternative satisfies the marker (sound: `∃R.Ci ⊑ ∃R.(C1 ⊔ … ⊔ Cn)`).
    ///
    /// CRITICAL — soundness: the marker is allocated in `by_union_existential`,
    /// a namespace DISJOINT from the singleton `by_existential` markers. It must
    /// never coincide with a singleton `∃R.Ci` marker: doing so (the prior bug)
    /// added the other alternatives' triggers onto a singleton marker that
    /// genuine `X ≡ ∃R.Ci ⊓ …` axioms also key on, so a class carrying only
    /// `∃R.Cj` spuriously gained the `∃R.Ci` marker and the unentailed
    /// subsumption `…∃R.Cj… ⊑ …∃R.Ci…` was derived. Memoized by
    /// `(role, sorted alternatives)` so identical unions still share one marker.
    fn introduce_union_existential_marker(
        &mut self,
        role: RoleId,
        mut body_ids: Vec<ClassId>,
        rules: &mut ElRules,
    ) -> ClassId {
        body_ids.sort_unstable();
        body_ids.dedup();
        if let Some(&existing) = self.by_union_existential.get(&(role, body_ids.clone())) {
            return existing;
        }
        let marker = ClassId::new(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("synthetic id overflow");
        for &body in &body_ids {
            rules.existential_triggers.push(ExistentialTrigger {
                role,
                body,
                head: marker,
            });
        }
        self.by_union_existential.insert((role, body_ids), marker);
        marker
    }

    /// Like `introduce_existential_marker`, but ALSO emits the
    /// existential fact `(marker, role, body)` so the marker
    /// behaves equivalent to `∃R.B` in the closure — not just
    /// one-way.
    ///
    /// Used by `atomic_classes_with_existential_markers` where the
    /// marker is consumed as a body operand inside a Tseitin
    /// synthetic that requires full equivalence semantics: the
    /// outer synthetic's closure needs to drive CR5/CR9 propagation
    /// through the inner existential (e.g., sub-property + sub-class
    /// chains through the inner existential), which requires the marker
    /// to have an existential fact about itself.
    ///
    /// LHS-trigger call sites (where the marker semantics ARE
    /// correctly asymmetric — "X has an R-edge to a B" without
    /// also asserting "F has an R-witness in B") continue to use
    /// `introduce_existential_marker`.
    ///
    /// Soundness: the marker is defined by the surrounding Tseitin
    /// synthetic to be ≡ `∃R.B`, so the new fact `(F, R, B)` is just
    /// the definition restated. See `docs/phase2b-trace.md`.
    fn introduce_equivalent_existential_marker(
        &mut self,
        role: RoleId,
        body: ClassId,
        rules: &mut ElRules,
    ) -> ClassId {
        let marker = self.introduce_existential_marker(role, body, rules);
        rules.existential_facts.push(ExistentialFact {
            sub: marker,
            role,
            target: body,
        });
        marker
    }
}

/// Wrapper around `collect_el_rules` that optionally builds a `ProofTrace`
/// with axiom provenance tables filled in. When `record_proofs` is false, returns
/// `(rules, tseitin, total_classes, None)` at zero extra cost.
fn collect_el_rules_with_provenance(
    internal: &InternalOntology,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
    record_proofs: bool,
) -> (ElRules, TseitinAllocator, usize, Option<ProofTrace>) {
    let (rules, tseitin, total_classes) = collect_el_rules(internal, role_super);
    if !record_proofs {
        return (rules, tseitin, total_classes, None);
    }

    // Build provenance tables by re-scanning `internal.axioms` and matching
    // the rule entries that were produced.
    //
    // Strategy: re-walk axioms in the same order as `collect_el_rules` and
    // track `before`/`after` counts to assign ranges of rule slots to their
    // source axiom.  This is the same range-snapshotting pattern used in
    // `introduce_runtime_synthetic`.
    //
    // We need parallel provenance Vecs of the same length as each rule Vec.

    let num_atomic = rules.atomic_subsumptions.len();
    let num_conj = rules.conjunctive_triggers.len();
    let num_facts = rules.existential_facts.len();
    let num_trigs = rules.existential_triggers.len();
    let num_disjt = rules.disjoint_pairs.len();
    let num_chain = rules.chain_axioms.len();
    let num_unsat = rules.directly_unsat.len();

    let mut atomic_sub_axiom: Vec<Option<usize>> = vec![None; num_atomic];
    let mut conjunctive_trigger_axiom: Vec<Option<usize>> = vec![None; num_conj];
    let mut existential_fact_axiom: Vec<Option<usize>> = vec![None; num_facts];
    let mut existential_trigger_axiom: Vec<Option<usize>> = vec![None; num_trigs];
    let mut disjoint_pair_axiom: Vec<Option<usize>> = vec![None; num_disjt];
    let mut chain_axiom_axiom: Vec<Option<usize>> = vec![None; num_chain];
    let mut directly_unsat_axiom: Vec<Option<usize>> = vec![None; num_unsat];
    let mut domain_axiom_refs: Vec<(RoleId, ClassId, usize)> = Vec::new();
    let mut functional_role_axiom: HashMap<RoleId, usize> = HashMap::new();

    // We re-simulate the axiom-to-rule mapping by tracking counters.
    // Simulated counters for Pass 1 (DisjointClasses, Domain, Chain, Transitive):
    let mut disjt_cur = 0usize;
    let mut chain_cur = 0usize;

    // Pass 1 (same order as collect_el_rules Pass 1):
    for (ax_idx, ax) in internal.axioms.iter().enumerate() {
        match ax {
            Axiom::DisjointClasses(members) => {
                let atomics: Vec<ClassId> = members
                    .iter()
                    .filter_map(|c| match internal.concepts.get(*c) {
                        ConceptExpr::Atomic(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                let count = atomics.len() * atomics.len().saturating_sub(1) / 2;
                for slot in disjt_cur..disjt_cur + count {
                    if let Some(entry) = disjoint_pair_axiom.get_mut(slot) {
                        *entry = Some(ax_idx);
                    }
                }
                disjt_cur += count;
            }
            Axiom::ObjectPropertyDomain { role, domain } => {
                if !role.is_inverse()
                    && let ConceptExpr::Atomic(id) = internal.concepts.get(*domain)
                {
                    domain_axiom_refs.push((role.role_id(), *id, ax_idx));
                }
            }
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(parts),
                sup,
            } if parts.len() == 2
                && !parts[0].is_inverse()
                && !parts[1].is_inverse()
                && !sup.is_inverse() =>
            {
                if let Some(entry) = chain_axiom_axiom.get_mut(chain_cur) {
                    *entry = Some(ax_idx);
                }
                chain_cur += 1;
            }
            Axiom::TransitiveRole(role) if !role.is_inverse() => {
                if let Some(entry) = chain_axiom_axiom.get_mut(chain_cur) {
                    *entry = Some(ax_idx);
                }
                chain_cur += 1;
            }
            Axiom::FunctionalRole(role) if !role.is_inverse() => {
                functional_role_axiom.insert(role.role_id(), ax_idx);
            }
            _ => {}
        }
    }

    // Pass 2: SubClassOf / EquivalentClasses — atomic_sub, existential_fact,
    // conjunctive_trigger, existential_trigger, directly_unsat.
    // We simulate lower_sub_class_of by counting, not re-running.
    // Use the counts from the already-built rules to attribute by axiom.
    //
    // Because lower_sub_class_of processes axioms in source order and appends
    // to the rule Vecs, the range [before..after] for each axiom is
    // monotonically increasing. We track cursors per Vec.
    let num_classes = internal.vocabulary.num_classes();
    // `tseitin.next_id` is used only to verify total_classes; no action needed here.
    let mut atomic_cur = 0usize;
    let mut conj_cur = 0usize;
    let mut facts_cur = 0usize;
    let mut trigs_cur = 0usize;
    let mut unsat_cur = 0usize;

    // We need a minimal simulation of what lower_sub_class_of does to know
    // how many rule slots each axiom produced. Rather than fully re-running,
    // we build a mini-version that counts only.
    //
    // For simplicity: run collect_el_rules a SECOND time on each individual
    // axiom (we already have the results, so we just need to attribute them).
    // This is O(n_axioms * rules_per_axiom) and only happens when record_proofs=true.
    {
        use std::collections::HashMap as SMap;
        let mut mini_rules = ElRules::default();
        let mut mini_tseitin = TseitinAllocator::new(num_classes);
        // Pass 1 mini: just range (needed for effective_ranges).
        for ax in &internal.axioms {
            if let Axiom::ObjectPropertyRange { role, range } = ax
                && !role.is_inverse()
                && let ConceptExpr::Atomic(id) = internal.concepts.get(*range)
            {
                mini_rules
                    .role_ranges
                    .entry(role.role_id())
                    .or_default()
                    .push(*id);
            }
        }
        let mut mini_effective: SMap<RoleId, Vec<ClassId>> = SMap::new();
        for (&r, supers) in role_super {
            let mut union: Vec<ClassId> = supers
                .iter()
                .flat_map(|s| mini_rules.role_ranges.get(s).into_iter().flatten().copied())
                .collect();
            union.sort();
            union.dedup();
            if !union.is_empty() {
                mini_effective.insert(r, union);
            }
        }
        for (ax_idx, ax) in internal.axioms.iter().enumerate() {
            match ax {
                Axiom::SubClassOf { sub, sup } => {
                    let b_a = mini_rules.atomic_subsumptions.len();
                    let b_c = mini_rules.conjunctive_triggers.len();
                    let b_f = mini_rules.existential_facts.len();
                    let b_t = mini_rules.existential_triggers.len();
                    let b_u = mini_rules.directly_unsat.len();
                    lower_sub_class_of(
                        *sub,
                        *sup,
                        &internal.concepts,
                        &mut mini_rules,
                        &mut mini_tseitin,
                        &mini_effective,
                    );
                    let a_a = mini_rules.atomic_subsumptions.len();
                    let a_c = mini_rules.conjunctive_triggers.len();
                    let a_f = mini_rules.existential_facts.len();
                    let a_t = mini_rules.existential_triggers.len();
                    let a_u = mini_rules.directly_unsat.len();
                    atomic_sub_axiom[atomic_cur..atomic_cur + (a_a - b_a)].fill(Some(ax_idx));
                    atomic_cur += a_a - b_a;
                    conjunctive_trigger_axiom[conj_cur..conj_cur + (a_c - b_c)].fill(Some(ax_idx));
                    conj_cur += a_c - b_c;
                    existential_fact_axiom[facts_cur..facts_cur + (a_f - b_f)].fill(Some(ax_idx));
                    facts_cur += a_f - b_f;
                    existential_trigger_axiom[trigs_cur..trigs_cur + (a_t - b_t)]
                        .fill(Some(ax_idx));
                    trigs_cur += a_t - b_t;
                    directly_unsat_axiom[unsat_cur..unsat_cur + (a_u - b_u)].fill(Some(ax_idx));
                    unsat_cur += a_u - b_u;
                }
                Axiom::EquivalentClasses(members) => {
                    for i in 0..members.len() {
                        for j in 0..members.len() {
                            if i != j {
                                let b_a = mini_rules.atomic_subsumptions.len();
                                let b_c = mini_rules.conjunctive_triggers.len();
                                let b_f = mini_rules.existential_facts.len();
                                let b_t = mini_rules.existential_triggers.len();
                                let b_u = mini_rules.directly_unsat.len();
                                lower_sub_class_of(
                                    members[i],
                                    members[j],
                                    &internal.concepts,
                                    &mut mini_rules,
                                    &mut mini_tseitin,
                                    &mini_effective,
                                );
                                let a_a = mini_rules.atomic_subsumptions.len();
                                let a_c = mini_rules.conjunctive_triggers.len();
                                let a_f = mini_rules.existential_facts.len();
                                let a_t = mini_rules.existential_triggers.len();
                                let a_u = mini_rules.directly_unsat.len();
                                atomic_sub_axiom[atomic_cur..atomic_cur + (a_a - b_a)]
                                    .fill(Some(ax_idx));
                                atomic_cur += a_a - b_a;
                                conjunctive_trigger_axiom[conj_cur..conj_cur + (a_c - b_c)]
                                    .fill(Some(ax_idx));
                                conj_cur += a_c - b_c;
                                existential_fact_axiom[facts_cur..facts_cur + (a_f - b_f)]
                                    .fill(Some(ax_idx));
                                facts_cur += a_f - b_f;
                                existential_trigger_axiom[trigs_cur..trigs_cur + (a_t - b_t)]
                                    .fill(Some(ax_idx));
                                trigs_cur += a_t - b_t;
                                directly_unsat_axiom[unsat_cur..unsat_cur + (a_u - b_u)]
                                    .fill(Some(ax_idx));
                                unsat_cur += a_u - b_u;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Build synthetic_defs from the tseitin allocator maps so rendering can
    // expand synthetic class ids into their defining expressions.
    let mut synthetic_defs: HashMap<ClassId, proof::SyntheticDef> = HashMap::new();
    // Tseitin conjunctions: F ≡ B₁ ⊓ … ⊓ Bₙ
    for (body_vec, &synthetic) in &tseitin.by_body {
        synthetic_defs.insert(
            synthetic,
            proof::SyntheticDef::TseitinConj(body_vec.clone()),
        );
    }
    // Existential markers (one-way: ∃R.B ⊑ M, or two-way: M ≡ ∃R.B).
    // `by_existential` keyed by (role, body) → marker. Check whether the
    // marker also has an existential fact (body, role) which would make it
    // equivalent; otherwise it's one-way.
    for (&(role, body), &marker) in &tseitin.by_existential {
        // Determine if this marker has an equivalent existential fact
        // (i.e. `introduce_equivalent_existential_marker` was used).
        let is_equiv = rules
            .existential_facts
            .iter()
            .any(|f| f.sub == marker && f.role == role && f.target == body);
        let def = if is_equiv {
            proof::SyntheticDef::ExistMarkerEquiv { role, body }
        } else {
            proof::SyntheticDef::ExistMarkerOneWay { role, body }
        };
        synthetic_defs.insert(marker, def);
    }
    // Nominal keys: NomKey(ind) → stand-in for {ind}
    for (&ind, &nomkey) in &tseitin.nominal_by_ind {
        synthetic_defs.insert(nomkey, proof::SyntheticDef::NominalKey(ind));
    }
    // MaxKey: MaxKey(n, role) → stand-in for ≤n R
    for (&(n, role), &key) in &tseitin.max_key_by_role {
        synthetic_defs.insert(key, proof::SyntheticDef::MaxKey { n, role });
    }
    // ForallKey: ForallKey(role, S) → stand-in for ∀role.OneOf(S)
    for ((role, members), &key) in &tseitin.forall_key_by_role {
        synthetic_defs.insert(
            key,
            proof::SyntheticDef::ForallKey {
                role: *role,
                members: members.clone(),
            },
        );
    }

    let trace = ProofTrace {
        steps: HashMap::new(),
        synthetic_defs,
        atomic_sub_axiom,
        existential_fact_axiom,
        conjunctive_trigger_axiom,
        existential_trigger_axiom,
        disjoint_pair_axiom,
        chain_axiom_axiom,
        directly_unsat_axiom,
        domain_axiom_refs,
        functional_role_axiom,
        merge_contributors: HashMap::new(),
        abox_path: HashMap::new(),
    };

    (rules, tseitin, total_classes, Some(trace))
}

fn collect_el_rules(
    internal: &InternalOntology,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
) -> (ElRules, TseitinAllocator, usize) {
    let mut rules = ElRules::default();
    let mut tseitin = TseitinAllocator::new(internal.vocabulary.num_classes());
    // Pass 1: metadata that the SubClassOf lowering needs to see — in
    // particular `role_ranges`, used below to fold range constraints
    // into RHS existential bodies via Tseitin synthetics.
    for ax in &internal.axioms {
        match ax {
            Axiom::DisjointClasses(members) => {
                let atomics: Vec<ClassId> = members
                    .iter()
                    .filter_map(|c| match internal.concepts.get(*c) {
                        ConceptExpr::Atomic(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                for i in 0..atomics.len() {
                    for j in (i + 1)..atomics.len() {
                        rules.disjoint_pairs.push((atomics[i], atomics[j]));
                    }
                }
            }
            Axiom::ObjectPropertyDomain { role, domain } => {
                if !role.is_inverse()
                    && let ConceptExpr::Atomic(id) = internal.concepts.get(*domain)
                {
                    rules
                        .role_domains
                        .entry(role.role_id())
                        .or_default()
                        .push(*id);
                }
            }
            Axiom::ObjectPropertyRange { role, range } => {
                if !role.is_inverse()
                    && let ConceptExpr::Atomic(id) = internal.concepts.get(*range)
                {
                    rules
                        .role_ranges
                        .entry(role.role_id())
                        .or_default()
                        .push(*id);
                }
            }
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(parts),
                sup,
            } if parts.len() == 2
                && !parts[0].is_inverse()
                && !parts[1].is_inverse()
                && !sup.is_inverse() =>
            {
                rules
                    .chain_axioms
                    .push((parts[0].role_id(), parts[1].role_id(), sup.role_id()));
            }
            Axiom::TransitiveRole(role) if !role.is_inverse() => {
                let r = role.role_id();
                rules.chain_axioms.push((r, r, r));
            }
            _ => {}
        }
    }
    // Build `effective_ranges[r]` = ⋃ { role_ranges[s] : r ⊑ s } using
    // the role-super closure. A range on a super-role applies to every
    // sub-role's successors (the witness of an r-existential is also
    // an s-successor when r ⊑ s, so it inherits Range(s) too).
    let mut effective_ranges: HashMap<RoleId, Vec<ClassId>> = HashMap::new();
    for (&r, supers) in role_super {
        let mut union: Vec<ClassId> = supers
            .iter()
            .flat_map(|s| rules.role_ranges.get(s).into_iter().flatten().copied())
            .collect();
        union.sort();
        union.dedup();
        if !union.is_empty() {
            effective_ranges.insert(r, union);
        }
    }
    // Pass 2: lower SubClassOf / EquivalentClasses with effective_ranges
    // available so RHS existential bodies can be Tseitin-folded with
    // their role's range constraint.
    for ax in &internal.axioms {
        match ax {
            Axiom::SubClassOf { sub, sup } => {
                lower_sub_class_of(
                    *sub,
                    *sup,
                    &internal.concepts,
                    &mut rules,
                    &mut tseitin,
                    &effective_ranges,
                );
            }
            Axiom::EquivalentClasses(members) => {
                // Decompose pairwise as mutual `SubClassOf` and route
                // each direction through `lower_sub_class_of`. That
                // handles compound members (e.g. `Test ≡ ∃r.(A⊓B)`)
                // through the same path that processes told
                // SubClassOf axioms, including the Tseitin allocator
                // for compound existential bodies.
                for i in 0..members.len() {
                    for j in 0..members.len() {
                        if i != j {
                            lower_sub_class_of(
                                members[i],
                                members[j],
                                &internal.concepts,
                                &mut rules,
                                &mut tseitin,
                                &effective_ranges,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Nominal/ABox transitive propagation (wine region cluster).
    // Allocates NomKeys for ABox individuals, so it must run before
    // `total_classes` is captured.
    //
    // GATE (2026-07-18): only run this if the TBox actually uses a nominal
    // (`∃R.{a}` / `ObjectHasValue` / `ObjectOneOf`), which — since it is the
    // only source of a TBox nominal — is exactly the case where
    // `tseitin.nominal_by_ind` is non-empty at this point (TBox nominals are
    // introduced during the axiom loop above; ABox NomKeys are introduced only
    // *by* this function). `abox_nominal_reach` is consulted only when a
    // processed fact TARGETS a NomKey, which cannot happen without a TBox
    // nominal — so when there is none, this pass is provably inert and merely
    // allocates a NomKey per ABox individual, inflating `num_total_classes` and
    // the O(num_total_classes²) subsumer matrix (ore_ont_3914: 570K GAZ
    // individuals ⇒ ~84 GB of dead matrix). Skipping is verdict-identical.
    if !tseitin.nominal_by_ind.is_empty() {
        build_abox_nominal_reach(internal, &mut tseitin, &mut rules);
    }

    // Cluster-B path (b) setup: index ForallKey targets by (role, member) and
    // build the NomKey→individual reverse, so `process_fact` can fire
    // `functional R + ∃R.{a}(a∈S) ⟹ C ⊑ ForallKey(R,S)`.
    for ((role, members), &key) in &tseitin.forall_key_by_role {
        for &a in members {
            rules
                .forall_key_targets
                .entry((*role, a))
                .or_default()
                .push(key);
        }
    }
    for (&ind, &nomkey) in &tseitin.nominal_by_ind {
        rules.nominal_to_ind.insert(nomkey, ind);
    }
    // Path (b) ≤1-driven variant: index the `MaxKey(1, R)` markers by role.
    for (&(n, role), &key) in &tseitin.max_key_by_role {
        if n == 1 {
            rules.max1_key_by_role.insert(role, key);
            rules.max1_role_by_key.insert(key, role);
        }
    }

    // SP-B2b: monotonicity edges `ForallAtomicKey(R,K) ⊑ ForallAtomicKey(R,L)` for
    // every DIRECT told `K ⊑ L` where both `(R,K)` and `(R,L)` are keys (`∀R.K ⊑
    // ∀R.L`, sound for non-inverse `R`). Direct edges suffice — the saturator's
    // subsumer transitivity closes chains (`BlandFish⊑Fish⊑Seafood`). Connect
    // existing keys only (the target marker must be a defined-class body atom to
    // matter). Sound (told ⊆ entailment); FP=0 by construction.
    if !tseitin.forall_atomic_key_by_role.is_empty() {
        let keys = tseitin.forall_atomic_key_by_role.clone();
        let told_direct: Vec<(ClassId, ClassId)> = rules
            .atomic_subsumptions
            .iter()
            .map(|s| (s.sub, s.sup))
            .collect();
        let mut edges: Vec<AtomicSubsumption> = Vec::new();
        for (k, l) in told_direct {
            if k == l {
                continue;
            }
            for (&(r, kk), &kc) in &keys {
                if kk == k
                    && let Some(&lc) = keys.get(&(r, l))
                {
                    edges.push(AtomicSubsumption { sub: kc, sup: lc });
                }
            }
        }
        rules.atomic_subsumptions.extend(edges);
    }

    let total_classes = tseitin.next_id as usize;

    // Phase 2a: collect functional-role declarations and precompute
    // the per-role list of functional super-roles (the index the
    // runtime witness-merge rule consults on every new existential
    // fact arrival).
    let num_roles = internal.vocabulary.num_roles();
    rules.functional_roles = FixedBitSet::with_capacity(num_roles);
    for ax in &internal.axioms {
        if let Axiom::FunctionalRole(role) = ax
            && !role.is_inverse()
        {
            let idx = role.role_id().index() as usize;
            if idx < num_roles {
                rules.functional_roles.insert(idx);
            }
        }
    }
    rules.functional_supers_of = vec![Vec::new(); num_roles];
    for r_idx in 0..num_roles {
        let r = RoleId::new(u32::try_from(r_idx).expect("role id fits in u32"));
        let mut supers: Vec<RoleId> = role_super
            .get(&r)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        supers.retain(|s| rules.is_functional(*s));
        supers.sort_unstable_by_key(|r| r.index());
        rules.functional_supers_of[r_idx] = supers;
    }

    // NOMINAL-FILLER TYPING (RUSTDL_NOMINAL_TYPING, **default ON**; `=0` to disable):
    // seed `NomKey(a) ⊑ C` for every `ClassAssertion(C, a)` whose individual `a` is
    // used as a nominal filler (`∃R.{a}`). Then CR5 derives `∃R.{a} ⊑ ∃R.C`
    // (object value-membership — the analog of the data D6 lever) — e.g. DMOP's
    // `C4.5… ⊑ ∃hasHypothesisStructure.{UnivariateDecisionTree}` +
    // `UnivariateDecisionTree : DecisionTree` ⟹ `… ⊑ ∃hasHypothesisStructure.
    // DecisionTree`. SOUND: `a:C ⟹ {a}⊑C ⟹ NomKey(a)⊑C` (only adds ENTAILED
    // subsumptions — FP=0 by construction). Lookup-only (no new ids); subclass
    // closure inherits via the atomic-subsumption fixpoint. Pure completeness gain
    // (closes DMOP 31→0; corpus FP=0/MISSED=0 byte-identical).
    if std::env::var_os("RUSTDL_NOMINAL_TYPING").is_none_or(|v| v != "0" && !v.is_empty()) {
        for ax in &internal.axioms {
            if let Axiom::ClassAssertion { class, individual } = ax
                && let Some(&nomkey) = tseitin.nominal_by_ind.get(individual)
            {
                for sup in atomic_operands_on_right(*class, &internal.concepts) {
                    rules
                        .atomic_subsumptions
                        .push(AtomicSubsumption { sub: nomkey, sup });
                }
            }
        }
    }

    // LHS-NOMINAL-DISJUNCTION COMMON-SUBSUMER (RUSTDL_ONEOF_SUBSUMER, **default ON**;
    // `=0` to disable): for an enumerated class `X ⊑ ObjectOneOf(a₁…aₙ)` (lowered to
    // `X ⊑ Or(Nominal(aᵢ))`), seed `X ⊑ C` for every atomic `C` that EVERY member `aᵢ`
    // is asserted to (`ClassAssertion(C, aᵢ)`). SOUND: `Xᴵ ⊆ {a₁ᴵ…aₙᴵ}` and each
    // `aᵢᴵ ∈ Cᴵ` ⟹ `Xᴵ ⊆ Cᴵ` (only ENTAILED subsumptions — FP=0 by construction; this
    // is LHS `⊔`-elimination, the complement of NOMINAL-FILLER TYPING above). Told-
    // subsumption closure cascades the rest — e.g. ORE 5107 `Anytime ≡ {h01…h24}`,
    // each `hᵢ : Hours` ⟹ `Anytime ⊑ Hours`, then `LeisureTime ⊑ Anytime ⊑ Hours`.
    // FP LANDMINE: the intersection MUST be over ALL n members — a member with NO
    // asserted type is unconstrained (could be a non-`C`), so it contributes the EMPTY
    // set and collapses the intersection to ∅ (seed nothing). Guards: all disjuncts
    // nominal (else `X` is not bounded by the enumeration), n ≥ 1. Under-approx: only
    // ASSERTED member types (not the derived NomKey closure) — sufficient for the
    // enumeration pattern; the derived version is deferred.
    if std::env::var_os("RUSTDL_ONEOF_SUBSUMER").is_none_or(|v| v != "0" && !v.is_empty()) {
        let pool = &internal.concepts;
        // member individual → set of atomic asserted types
        let mut asserted: HashMap<IndividualId, std::collections::HashSet<ClassId>> =
            HashMap::new();
        for ax in &internal.axioms {
            if let Axiom::ClassAssertion { class, individual } = ax {
                let entry = asserted.entry(*individual).or_default();
                for sup in atomic_operands_on_right(*class, pool) {
                    entry.insert(sup);
                }
            }
        }
        // Recover the nominal members of `sup` iff it is `Or(Nominal …)` (all nominal).
        let oneof_members = |sup: ConceptId| -> Option<Vec<IndividualId>> {
            let ConceptExpr::Or(disjuncts) = pool.get(sup) else {
                return None;
            };
            if disjuncts.is_empty() {
                return None;
            }
            let mut inds = Vec::with_capacity(disjuncts.len());
            for &d in disjuncts {
                let ConceptExpr::Nominal(ind) = pool.get(d) else {
                    return None;
                };
                inds.push(*ind);
            }
            Some(inds)
        };
        // For `X ⊑ Or(Nominal …)` seed `X ⊑ c` for every `c` common to all members.
        let seed_for = |sub: ConceptId, sup: ConceptId, rules: &mut ElRules| {
            let ConceptExpr::Atomic(x) = pool.get(sub) else {
                return;
            };
            let Some(members) = oneof_members(sup) else {
                return;
            };
            // Intersect asserted types over ALL members (absent member ⟹ ∅).
            let mut common: Option<std::collections::HashSet<ClassId>> = None;
            for ind in &members {
                let types = asserted.get(ind).cloned().unwrap_or_default();
                common = Some(match common.take() {
                    None => types,
                    Some(prev) => prev.intersection(&types).copied().collect(),
                });
                if common
                    .as_ref()
                    .is_some_and(std::collections::HashSet::is_empty)
                {
                    return;
                }
            }
            if let Some(common) = common {
                for sup in common {
                    rules
                        .atomic_subsumptions
                        .push(AtomicSubsumption { sub: *x, sup });
                }
            }
        };
        for ax in &internal.axioms {
            match ax {
                Axiom::SubClassOf { sub, sup } => seed_for(*sub, *sup, &mut rules),
                Axiom::EquivalentClasses(members) => {
                    for i in 0..members.len() {
                        for j in 0..members.len() {
                            if i != j {
                                seed_for(members[i], members[j], &mut rules);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    (rules, tseitin, total_classes)
}

/// Lower a single `SubClassOf(sub, sup)` axiom into atomic facts
/// and conjunctive triggers. Anything that doesn't fit (existentials,
/// disjunction, complement, cardinality, ...) is silently dropped —
/// the orchestrator handles those via tableau fallback.
fn lower_sub_class_of(
    sub: ConceptId,
    sup: ConceptId,
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
    effective_ranges: &HashMap<RoleId, Vec<ClassId>>,
) {
    // SP-B1: register an atomic disjunction `C ⊑ D₁⊔…⊔Dₙ` (all `Dᵢ` atomic, n≥2)
    // for the derived-closure forced-disjunct rule fired in `process_subsumer`.
    // Runs for both SubClassOf and the EquivalentClasses `P ⊑ Or` direction.
    // Non-atomic disjunct ⟹ skip the whole disjunction (nominal `⊔` = B3 scope).
    if let ConceptExpr::Atomic(c) = pool.get(sub)
        && let ConceptExpr::Or(disjuncts) = pool.get(sup)
    {
        let mut atomic: Vec<ClassId> = Vec::with_capacity(disjuncts.len());
        let mut all_atomic = true;
        for &d in disjuncts {
            if let ConceptExpr::Atomic(did) = pool.get(d) {
                atomic.push(*did);
            } else {
                all_atomic = false;
                break;
            }
        }
        if all_atomic && atomic.len() >= 2 {
            rules
                .disjunctions_by_class
                .entry(*c)
                .or_default()
                .push(atomic.into_boxed_slice());
        }
    }
    match pool.get(sub) {
        ConceptExpr::Atomic(sub_id) => {
            // Phase D4 (2026-06-03): `Atomic(C) ⊑ Bot` directly marks
            // C unsatisfiable. Without this branch the saturator's
            // `atomic_operands_on_right(Bot, _)` returns empty and the
            // axiom is silently lost — the data-axiom preprocessing
            // pass's emitted clash axioms (Functional + ≥n, DataMin >
            // DataMax) wouldn't be picked up. See
            // `crates/owl-dl-core/src/data_axioms.rs`.
            if matches!(pool.get(sup), ConceptExpr::Bot) {
                rules.directly_unsat.push(*sub_id);
                return;
            }
            for atomic_sup in atomic_operands_on_right(sup, pool) {
                rules.atomic_subsumptions.push(AtomicSubsumption {
                    sub: *sub_id,
                    sup: atomic_sup,
                });
            }
            // `X ⊑ ∃R.Self` (ObjectHasSelf): X is both source and target of an
            // R-edge to itself ⟹ `X ⊑ domain(R) ⊓ range(R)`. Without this the
            // saturator drops SelfRestriction entirely, missing the whole
            // self-loop → domain/range chain (olia/OBO: cost 461 MISSED on
            // ore_ont_4827, e.g. AspectFeature ⊑ ∃hasAspect.Self, domain(hasAspect)
            // = Verb ⟹ AspectFeature ⊑ Verb). Sound under-approximation. Domain /
            // range tables are fully populated in Pass 1, before this Pass-2
            // lowering, so reading them here is complete. Subclasses inherit via
            // the atomic-subsumption closure (Y ⊑ X ⟹ Y ⊑ domain(R)).
            for r in self_restriction_roles_on_right(sup, pool) {
                // The self-loop `(x,x) ∈ R` is also an `S`-edge for every super-role
                // `R ⊑* S`, so `x` lies in `range(S)` for all such `S` — i.e. the
                // *effective* (super-role-closed) range, not just `range(R)`. Using
                // `effective_ranges` here closes the ore_ont_4827 pattern
                // (`ClusivityFeature ⊑ ∃hasClusivity.Self`, `hasClusivity ⊑ hasFeature`,
                // `range(hasFeature)=Feature` ⟹ `ClusivityFeature ⊑ Feature`). Sound:
                // the successor coincides with `x`, so the range obligation lands on
                // `x` itself (unlike general range propagation, deliberately omitted
                // above — there the range target is a distinct existential body).
                let heads: Vec<ClassId> = rules
                    .role_domains
                    .get(&r)
                    .into_iter()
                    .chain(effective_ranges.get(&r))
                    .flatten()
                    .copied()
                    .collect();
                for head in heads {
                    rules.atomic_subsumptions.push(AtomicSubsumption {
                        sub: *sub_id,
                        sup: head,
                    });
                }
            }
            // Cluster-C lever: a told unqualified `≤n R` (top-level or an And
            // operand of the RHS) seeds `sub ⊑ MaxKey(n,R)` — the same opaque
            // key a defined class's `≤n R` conjunct lowers to, so the
            // defined-class conjunctive trigger requires the cardinality
            // conjunct soundly (fires only when an identical told `≤n R` holds).
            for (n, role) in unqualified_max_operands_on_right(sup, pool) {
                let key = tseitin.introduce_max_key(n, role);
                rules.atomic_subsumptions.push(AtomicSubsumption {
                    sub: *sub_id,
                    sup: key,
                });
            }
            // Cluster-B lever: a told `∀R.OneOf(S)` (top-level or And operand)
            // seeds `sub ⊑ ForallKey(R,S)` — the same opaque key a defined
            // class's `∀R.OneOf(S)` conjunct lowers to. Sound: `C ⊑ ∀R.OneOf(S)`
            // is a genuine told (or subsumption-propagated) fact, exact-`S` match.
            for (role, members) in forall_oneof_operands_on_right(sup, pool) {
                let key = tseitin.introduce_forall_key(role, members);
                rules.atomic_subsumptions.push(AtomicSubsumption {
                    sub: *sub_id,
                    sup: key,
                });
            }
            // SP-B2b: a told `∀R.Atomic(K)` (top-level or And operand) seeds
            // `sub ⊑ ForallAtomicKey(R,K)` — the same opaque key a defined class's
            // `∀R.K` conjunct lowers to. Sound; monotonicity edges (seeded below)
            // give `∀R.K ⊑ ∀R.L` for told `K ⊑ L`.
            for (role, k) in forall_atomic_operands_on_right(sup, pool) {
                let key = tseitin.introduce_forall_atomic_key(role, k);
                rules.atomic_subsumptions.push(AtomicSubsumption {
                    sub: *sub_id,
                    sup: key,
                });
            }
            // `Atomic(X) ⊑ ¬Atomic(Y)` (directly, or as an operand of a
            // top-level `And` on the right) means `X ⊓ Y ⊑ ⊥`, i.e.
            // `disjoint(X, Y)`. The saturator otherwise drops the `¬Y`
            // (a negated atomic is not an atomic subsumer), missing the
            // unsatisfiability it induces — e.g. `A ⊑ B ⊓ ¬B ⇒ A ⊑ ⊥`.
            // Register the pair so the existing disjointness→unsat
            // propagation fires (reflexive `X ⊑ X` is seeded, so the
            // check at `process_subsumer` triggers). Sound and
            // monotonic: `X ⊑ ¬Y ⟺ disjoint(X, Y)`, so this only ever
            // adds a genuine clash, never a false subsumption.
            for y in not_atomic_operands_on_right(sup, pool) {
                rules.disjoint_pairs.push((*sub_id, y));
            }
            // Atomic ⊑ ∃r.Y: existential fact. Tseitin introduces a
            // synthetic atomic if the body is a compound And, OR if
            // r has a range constraint that needs to be folded in.
            if let Some((role, target)) =
                atomic_existential_rhs(sup, pool, rules, tseitin, effective_ranges)
            {
                rules.existential_facts.push(ExistentialFact {
                    sub: *sub_id,
                    role,
                    target,
                });
            }
            // Atomic ⊑ (∃r.Y₁ ⊓ ∃r.Y₂ ⊓ …): pick up each existential
            // operand of a top-level And on the right.
            if let ConceptExpr::And(operands) = pool.get(sup) {
                for op in operands {
                    if let Some((role, target)) =
                        atomic_existential_rhs(*op, pool, rules, tseitin, effective_ranges)
                    {
                        rules.existential_facts.push(ExistentialFact {
                            sub: *sub_id,
                            role,
                            target,
                        });
                    }
                }
            }
        }
        ConceptExpr::And(operands) => {
            // EL+ existential-in-conjunction lowering: each `∃R.B`
            // operand is replaced by a Tseitin marker `F` with
            // `∃R.B ⊑ F` emitted as an existential trigger, and `F` is
            // added to the conjunctive body alongside the atomic
            // operands. If *any* operand is neither atomic nor a
            // named-role existential with an atomic-or-And body, drop
            // the whole trigger (partial lowering would be unsound:
            // the trigger would fire when only some of the required
            // operands are present).
            let mut bodies: Vec<ClassId> = Vec::with_capacity(operands.len());
            let mut salvageable = true;
            for &op in operands {
                match pool.get(op) {
                    ConceptExpr::Atomic(id) => bodies.push(*id),
                    ConceptExpr::Some(role, body) if !role.is_inverse() => {
                        let Some(body_ids) =
                            existential_body_alternatives(*body, pool, rules, tseitin)
                        else {
                            salvageable = false;
                            break;
                        };
                        // Allocate one marker for this existential operand.
                        // A singleton body `∃R.C` shares the singleton
                        // `by_existential` marker (correct — every `∃R.C`
                        // genuinely means the same thing). A disjunctive body
                        // `∃R.(C1 ⊔ … ⊔ Cn)` gets a FRESH dedicated union marker
                        // (`by_union_existential`) with `∃R.Ci ⊑ marker` for each
                        // alternative — it must NOT reuse a singleton `∃R.Ci`
                        // marker, or the other alternatives' triggers would
                        // corrupt that singleton (unsound FP). See
                        // `introduce_union_existential_marker`.
                        let marker = if body_ids.len() == 1 {
                            tseitin.introduce_existential_marker(role.role_id(), body_ids[0], rules)
                        } else {
                            tseitin.introduce_union_existential_marker(
                                role.role_id(),
                                body_ids,
                                rules,
                            )
                        };
                        bodies.push(marker);
                    }
                    // Cluster-C lever: an unqualified `≤n R` conjunct of a
                    // defined class lowers to the opaque `MaxKey(n,R)` body,
                    // matched by the told-`≤n R` seed (`unqualified_max_operands_
                    // on_right`). Qualified / inverse stay un-salvageable.
                    ConceptExpr::Max(n, role, inner)
                        if !role.is_inverse() && matches!(pool.get(*inner), ConceptExpr::Top) =>
                    {
                        bodies.push(tseitin.introduce_max_key(*n, role.role_id()));
                    }
                    // Cluster-B lever: a `∀R.OneOf(S)` conjunct lowers to the
                    // opaque `ForallKey(R,S)` body (matched by the told-`∀` seed
                    // in `forall_oneof_operands_on_right`).
                    _ if forall_oneof_members(op, pool).is_some() => {
                        let (role, members) =
                            forall_oneof_members(op, pool).expect("just checked Some");
                        bodies.push(tseitin.introduce_forall_key(role, members));
                    }
                    // SP-B2b: a `∀R.Atomic(K)` conjunct of a defined class lowers to
                    // the opaque `ForallAtomicKey(R,K)` body (matched by the told-`∀`
                    // seed in `forall_atomic_operands_on_right` + the monotonicity edges).
                    _ if forall_atomic_member(op, pool).is_some() => {
                        let (role, k) = forall_atomic_member(op, pool).expect("just checked Some");
                        bodies.push(tseitin.introduce_forall_atomic_key(role, k));
                    }
                    _ => {
                        salvageable = false;
                        break;
                    }
                }
            }
            if !salvageable {
                return;
            }
            // The existing atomic-operand loop: any atomic class on
            // the right (or atomic operand of an `And` on the right)
            // becomes a head of the conjunctive trigger.
            for head in atomic_operands_on_right(sup, pool) {
                rules.conjunctive_triggers.push(ConjunctiveTrigger {
                    bodies: bodies.clone(),
                    head,
                });
            }
            // Phase 2b.5: a non-atomic `∃R.B` on the right (or as an
            // operand of an `And` on the right) also produces a trigger.
            // Allocate a two-way marker via `introduce_equivalent_existential_marker`
            // and push a conjunctive trigger `{bodies} ⊑ marker`. Without
            // this, axioms of shape `And(...) ⊑ ∃R.B` are silently dropped
            // because `atomic_operands_on_right` returns [] for `Some`.
            // One-way would consume an R-witness rather than create one, so
            // the chain `Y ⊑ {bodies} → Y ⊑ marker → ... → Y has R-witness`
            // requires the marker to emit the fact (M, R, body).
            // See docs/phase2b-trace2.md for the diagnostic.
            let sup_existentials: Vec<(RoleId, ClassId)> = match pool.get(sup) {
                ConceptExpr::Some(role, body) if !role.is_inverse() => {
                    atomic_or_tseitin_body(*body, pool, rules, tseitin)
                        .map(|body_id| vec![(role.role_id(), body_id)])
                        .unwrap_or_default()
                }
                ConceptExpr::And(operands) => operands
                    .iter()
                    .filter_map(|&op| match pool.get(op) {
                        ConceptExpr::Some(role, body) if !role.is_inverse() => {
                            atomic_or_tseitin_body(*body, pool, rules, tseitin)
                                .map(|body_id| (role.role_id(), body_id))
                        }
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            for (role, body_id) in sup_existentials {
                // Use the two-way (equivalent) marker: M ≡ ∃R.B, so
                // both `∃R.B ⊑ M` and the existential fact `(M, R, B)`
                // are emitted. The conjunctive trigger gives the
                // conjunction M as a subsumer; the existential fact on M
                // then propagates the R-witness to any class that gains M,
                // completing the chain `{bodies} ⊑ M ⊑ ∃R.B ⊑ T`. A
                // one-way marker would not complete: Y gains M but never
                // gets the R-witness needed for downstream triggers.
                let marker = tseitin.introduce_equivalent_existential_marker(role, body_id, rules);
                rules.conjunctive_triggers.push(ConjunctiveTrigger {
                    bodies: bodies.clone(),
                    head: marker,
                });
            }
        }
        ConceptExpr::Some(role, body) => {
            // ∃r.B ⊑ C: existential trigger. Named role only; the
            // body may be atomic, an `And` of atomics (Tseitin-folded),
            // or an `Or(C1, ..., Cn)` (one trigger emitted per
            // operand; sound because `∃r.Ci ⊑ ∃r.(C1 ⊔ ... ⊔ Cn)`).
            // Range constraints are NOT folded here: trigger bodies
            // are matched against witness subsumers, and user classes
            // aren't marked as subsumers of Range(R) — folding the
            // range in would make the trigger never fire.
            if role.is_inverse() {
                return;
            }
            // `∃R.⊤ ⊑ C` is exactly the domain axiom domain(R) ⊒ C (semantically
            // identical to `ObjectPropertyDomain(R, C)`): anything with an
            // R-successor is a C. Route it to the role-domain mechanism (which
            // fires on any R-marker/edge), NOT the existential-trigger path: a
            // `Top` body has no `atomic_or_tseitin_body` representation, so
            // `existential_body_alternatives(Top)` returns `None` and the trigger
            // — and the entire domain inference — was silently DROPPED (SWEET /
            // OBO express domains in this GCI form; cost 300+ MISSED on
            // ore_ont_13621/14450). Sound EL completeness fix: the domain rule is
            // already complete (cf. the `ObjectPropertyDomain` arm).
            if matches!(pool.get(*body), ConceptExpr::Top) {
                for head in atomic_operands_on_right(sup, pool) {
                    rules
                        .role_domains
                        .entry(role.role_id())
                        .or_default()
                        .push(head);
                }
                return;
            }
            let Some(body_ids) = existential_body_alternatives(*body, pool, rules, tseitin) else {
                return;
            };
            for head in atomic_operands_on_right(sup, pool) {
                for &body_id in &body_ids {
                    rules.existential_triggers.push(ExistentialTrigger {
                        role: role.role_id(),
                        body: body_id,
                        head,
                    });
                }
            }
        }
        // `⊤ ⊑ C` (a named class equivalent to owl:Thing): C subsumes EVERYTHING,
        // so every named class ⊑ C. Record each atomic sup as a "top subsumer";
        // `seed` broadcasts these to all classes and the fixpoint closes them
        // transitively. Without this the axiom was silently dropped here — a real
        // EL-incompleteness (ORE ore_ont_11522: 522 vs whelk's complete 1490).
        ConceptExpr::Top => {
            for c in atomic_operands_on_right(sup, pool) {
                rules.top_subsumers.push(c);
            }
        }
        _ => {}
    }
}

/// Extract `(role_id, target_class_id)` from `∃<named-role>.<body>`
/// in **RHS** position (i.e., `A ⊑ ∃R.body`). Folds any
/// `effective_ranges[role]` into the body via Tseitin: the witness of
/// an R-existential is in `body ⊓ Range(R)`, so a synthetic
/// `F ≡ body ⊓ Range(R)` stands in for the body. This is sound (the
/// witness is constrained, not the type symbol `body` itself).
///
/// Returns `None` for inverse roles, non-atomic bodies, or any other
/// shape (those are dropped from the EL fragment; the tableau path
/// still handles them).
fn atomic_existential_rhs(
    c: ConceptId,
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
    effective_ranges: &HashMap<RoleId, Vec<ClassId>>,
) -> Option<(RoleId, ClassId)> {
    // Accept both `∃R.body` (Some) and `≥n R.body` (Min with n ≥ 1).
    // Min(n, R, C) implies ∃R.C for n ≥ 1, so lowering Min as Some is
    // a sound under-approximation: the saturator picks up an
    // existential fact, the precise cardinality is left to the
    // tableau path. Min(0, ...) is trivially true and contributes
    // nothing — skip.
    let (role, body) = match pool.get(c) {
        ConceptExpr::Some(role, body) => (role, body),
        ConceptExpr::Min(n, role, body) if *n >= 1 => (role, body),
        _ => return None,
    };
    if role.is_inverse() {
        return None;
    }
    // Nominal body `∃R.{a}`: emit the bare per-individual NomKey, NOT a
    // range-wrapped synthetic. The wrap (`NomKey ⊓ Range(R)`) would make
    // the fact target a fresh synthetic, defeating the `abox_nominal_reach`
    // lookup in `process_fact` (keyed on the bare NomKey). Dropping the
    // range-typing of the witness is a sound under-approximation — the
    // nominal fold needs only the NomKey identity.
    if let ConceptExpr::Nominal(ind) = pool.get(*body) {
        return Some((role.role_id(), tseitin.introduce_nominal(*ind)));
    }
    // `∃R.⊤` (ObjectSomeValuesFrom(R, owl:Thing)): the ⊤ filler has no
    // atomic_or_tseitin body, so previously this made no fact and the subject never
    // got an R-marker — the domain rule (`∃R.⊤ ⊑ C` = domain(R)=C) then never fired
    // on it. Emit a fact to an opaque ⊤-witness so the domain inference fires (e.g.
    // `A ≡ ∃R.⊤`, `B ≡ ∃R.⊤` ⟹ A ≡ B via domain(R) ⊒ {A,B}). Sound: the witness has
    // no subsumers (⊤-equivalent), so it only ever triggers domain(R).
    if matches!(pool.get(*body), ConceptExpr::Top) {
        return Some((
            role.role_id(),
            tseitin.introduce_top_witness(role.role_id()),
        ));
    }
    let extras = effective_ranges
        .get(&role.role_id())
        .map_or(&[][..], Vec::as_slice);
    let body_id = atomic_or_tseitin_body_with_extras(*body, extras, pool, rules, tseitin)?;
    Some((role.role_id(), body_id))
}

/// Lower a concept used as an existential's body to a single atomic
/// class id: if it's already atomic, return it; if it's an `And` of
/// all-atomic operands, Tseitin-introduce a synthetic class that's
/// equivalent to the intersection and return that.
fn atomic_or_tseitin_body(
    body: ConceptId,
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
) -> Option<ClassId> {
    atomic_or_tseitin_body_with_extras(body, &[], pool, rules, tseitin)
}

/// Populate [`ElRules::abox_nominal_reach`]: for each **transitive**
/// named role `R`, compute the transitive closure of `R` over named
/// individuals (`ObjectPropertyAssertion`s) and map each source's
/// `NomKey` to the `NomKey`s of all reachable individuals. Enables the
/// sound `X ⊑ ∃R.{a}`, `a R⁺ b` ⟹ `X ⊑ ∃R.{b}` propagation in
/// `process_fact`. No-op unless the ontology has transitive roles with
/// `ABox` edges.
fn build_abox_nominal_reach(
    internal: &InternalOntology,
    tseitin: &mut TseitinAllocator,
    rules: &mut ElRules,
) {
    use std::collections::BTreeSet;
    let mut transitive: HashSet<RoleId> = HashSet::new();
    for ax in &internal.axioms {
        if let Axiom::TransitiveRole(role) = ax
            && !role.is_inverse()
        {
            transitive.insert(role.role_id());
        }
    }
    if transitive.is_empty() {
        return;
    }
    // Direct R-successor graph over individuals (named, transitive R only).
    let mut direct: HashMap<RoleId, HashMap<IndividualId, BTreeSet<IndividualId>>> = HashMap::new();
    for ax in &internal.axioms {
        if let Axiom::ObjectPropertyAssertion {
            role,
            subject,
            object,
        } = ax
            && !role.is_inverse()
            && transitive.contains(&role.role_id())
        {
            direct
                .entry(role.role_id())
                .or_default()
                .entry(*subject)
                .or_default()
                .insert(*object);
        }
    }
    for (role, graph) in &direct {
        // Naive transitive-closure fixpoint (ABoxes here are tiny).
        let mut closure = graph.clone();
        let mut changed = true;
        while changed {
            changed = false;
            let sources: Vec<IndividualId> = closure.keys().copied().collect();
            for a in sources {
                let mids: Vec<IndividualId> = closure
                    .get(&a)
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                let mut additions: Vec<IndividualId> = Vec::new();
                for m in mids {
                    if let Some(ms) = graph.get(&m) {
                        additions.extend(ms.iter().copied());
                    }
                }
                if let Some(reach) = closure.get_mut(&a) {
                    for t in additions {
                        if t != a && reach.insert(t) {
                            changed = true;
                        }
                    }
                }
            }
        }
        for (a, reach) in &closure {
            if reach.is_empty() {
                continue;
            }
            let a_key = tseitin.introduce_nominal(*a);
            let targets: Vec<ClassId> = reach
                .iter()
                .map(|&b| tseitin.introduce_nominal(b))
                .collect();
            rules.abox_nominal_reach.insert((*role, a_key), targets);
        }
    }
}

/// Return the list of alternative body class ids for an existential
/// trigger's body. For `Atomic` / `And` returns one element. For
/// `Or(C1, ..., Cn)` returns one element per operand (each itself
/// lowered via `atomic_or_tseitin_body`). Used when lowering trigger
/// LHS existentials so that `∃R.Or(C1, C2) ⊑ Head` becomes
/// `∃R.C1 ⊑ Head` plus `∃R.C2 ⊑ Head` — sound because
/// `∃R.Ci ⊑ ∃R.(C1 ⊔ C2)`. Returns `None` if any operand can't be
/// lowered (drops the whole trigger, since partial coverage would
/// fire too eagerly on some pathological shapes).
fn existential_body_alternatives(
    body: ConceptId,
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
) -> Option<Vec<ClassId>> {
    match pool.get(body) {
        ConceptExpr::Or(operands) => {
            let mut out = Vec::with_capacity(operands.len());
            for &op in operands {
                let id = atomic_or_tseitin_body(op, pool, rules, tseitin)?;
                out.push(id);
            }
            Some(out)
        }
        _ => atomic_or_tseitin_body(body, pool, rules, tseitin).map(|id| vec![id]),
    }
}

/// Like `atomic_or_tseitin_body`, but additionally folds `extras`
/// (atomic class ids) into the synthetic body. When `extras` is
/// non-empty, always allocates a Tseitin synthetic `F ≡ body ⊓
/// extras…` even if `body` is itself atomic. Used at RHS existential
/// sites to fold in `Range(R)` constraints, so the witness of an
/// R-existential is correctly typed.
fn atomic_or_tseitin_body_with_extras(
    body: ConceptId,
    extras: &[ClassId],
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
) -> Option<ClassId> {
    let body_atomics: Vec<ClassId> = match pool.get(body) {
        ConceptExpr::Atomic(id) => vec![*id],
        // Nominal `{a}` body (`∃R.{a}`, i.e. ObjectHasValue): use an
        // opaque per-individual synthetic class as a structural
        // stand-in so the EL fold of `C ≡ D ⊓ ∃R.{a}` matches the
        // `X ⊑ ∃R.{a}` fact. Sound (1:1 individual identity); the
        // singleton/cardinality semantics of `{a}` are deliberately
        // not modeled (under-approximation — the tableau handles those).
        ConceptExpr::Nominal(ind) => vec![tseitin.introduce_nominal(*ind)],
        ConceptExpr::And(operands) => {
            atomic_classes_with_existential_markers(operands, pool, rules, tseitin)?
        }
        ConceptExpr::Some(role, inner_body) if !role.is_inverse() => {
            // Top-level nested existential as the outer body:
            // `∃R.∃S.X` style. Introduce a marker for the inner
            // existential and use it as the single-class body.
            let inner_id = atomic_or_tseitin_body(*inner_body, pool, rules, tseitin)?;
            let marker = tseitin.introduce_existential_marker(role.role_id(), inner_id, rules);
            vec![marker]
        }
        ConceptExpr::Min(n, role, inner_body) if *n >= 1 && !role.is_inverse() => {
            // `≥n R.X` as a nested body — sound underapproximation
            // to ∃R.X (same lowering as `atomic_existential_rhs`).
            let inner_id = atomic_or_tseitin_body(*inner_body, pool, rules, tseitin)?;
            let marker = tseitin.introduce_existential_marker(role.role_id(), inner_id, rules);
            vec![marker]
        }
        _ => return None,
    };
    if extras.is_empty() && body_atomics.len() == 1 {
        return Some(body_atomics[0]);
    }
    let mut combined: Vec<ClassId> = body_atomics;
    combined.extend_from_slice(extras);
    // `TseitinAllocator::introduce` sort+dedups; identical bodies map
    // to the same synthetic, so two existentials A ⊑ ∃R.B and
    // A' ⊑ ∃R.B with the same Range(R) share one synthetic F.
    Some(tseitin.introduce(combined, rules))
}

/// Like `atomic_classes`, but also accepts `∃R.body` and `≥n R.body`
/// operands by introducing existential markers. Used inside the body
/// of an existential when the body's And contains nested existentials
/// (e.g. `∃R.(B ⊓ ∃S.C)` — the inner `∃S.C` is replaced by a marker M
/// with `∃S.C ⊑ M`, then the outer body becomes the And of atomic
/// operands ∪ {M}). Returns None if any operand can't be reduced to
/// an atomic id this way.
fn atomic_classes_with_existential_markers(
    ids: &[ConceptId],
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
) -> Option<Vec<ClassId>> {
    let mut out = Vec::with_capacity(ids.len());
    for &c in ids {
        match pool.get(c) {
            ConceptExpr::Atomic(id) => out.push(*id),
            ConceptExpr::Some(role, inner_body) if !role.is_inverse() => {
                let inner_id = atomic_or_tseitin_body(*inner_body, pool, rules, tseitin)?;
                let marker = tseitin.introduce_equivalent_existential_marker(
                    role.role_id(),
                    inner_id,
                    rules,
                );
                out.push(marker);
            }
            ConceptExpr::Min(n, role, inner_body) if *n >= 1 && !role.is_inverse() => {
                let inner_id = atomic_or_tseitin_body(*inner_body, pool, rules, tseitin)?;
                let marker = tseitin.introduce_equivalent_existential_marker(
                    role.role_id(),
                    inner_id,
                    rules,
                );
                out.push(marker);
            }
            // NB: `≤n R` conjuncts are deliberately NOT lowered here — this
            // function lowers an existential *body* (the filler's type), where a
            // `MaxKey` would assert the filler's cardinality, a different (and
            // un-modeled) fact than the subject's own `≤n R`. The cluster-C lever
            // lives only in the conjunctive-trigger builder + the told-`≤n` seed.
            _ => return None,
        }
    }
    Some(out)
}

/// Unqualified `≤n R` restrictions that are `c` itself or a top-level `And`
/// operand of `c` — each `(n, R)` seeds the cluster-C `MaxKey` subsumer.
/// Only `inner = ⊤` (unqualified) and non-inverse roles are recognised (a sound
/// under-approximation; qualified / inverse stay dropped). Mirrors the `Max`
/// arm of `atomic_classes_with_existential_markers` so the seed key and the
/// defined-class trigger key coincide.
fn unqualified_max_operands_on_right(c: ConceptId, pool: &ConceptPool) -> Vec<(u32, RoleId)> {
    let one = |cid: ConceptId| -> Option<(u32, RoleId)> {
        match pool.get(cid) {
            ConceptExpr::Max(n, role, inner)
                if !role.is_inverse() && matches!(pool.get(*inner), ConceptExpr::Top) =>
            {
                Some((*n, role.role_id()))
            }
            _ => None,
        }
    };
    match pool.get(c) {
        ConceptExpr::And(operands) => operands.iter().filter_map(|&op| one(op)).collect(),
        _ => one(c).into_iter().collect(),
    }
}

/// If `c` is `∀R.OneOf(S)` — an `All(R, inner)` where `inner` is a single
/// `Nominal` or an `Or` of `Nominal`s and `R` is non-inverse — return
/// `(R, members)`. The cluster-B `ForallKey` recogniser; anything else → None.
fn forall_oneof_members(c: ConceptId, pool: &ConceptPool) -> Option<(RoleId, Vec<IndividualId>)> {
    let ConceptExpr::All(role, inner) = pool.get(c) else {
        return None;
    };
    if role.is_inverse() {
        return None;
    }
    let mut members = Vec::new();
    match pool.get(*inner) {
        ConceptExpr::Nominal(ind) => members.push(*ind),
        ConceptExpr::Or(ops) => {
            for &op in ops {
                match pool.get(op) {
                    ConceptExpr::Nominal(ind) => members.push(*ind),
                    _ => return None,
                }
            }
        }
        _ => return None,
    }
    if members.is_empty() {
        return None;
    }
    Some((role.role_id(), members))
}

/// `∀R.OneOf(S)` restrictions that are `c` itself or a top-level `And` operand
/// of `c` — each `(R, S)` seeds the cluster-B `ForallKey` subsumer. Mirrors
/// `unqualified_max_operands_on_right` (told side) and the trigger-builder arm.
fn forall_oneof_operands_on_right(
    c: ConceptId,
    pool: &ConceptPool,
) -> Vec<(RoleId, Vec<IndividualId>)> {
    match pool.get(c) {
        ConceptExpr::And(operands) => operands
            .iter()
            .filter_map(|&op| forall_oneof_members(op, pool))
            .collect(),
        _ => forall_oneof_members(c, pool).into_iter().collect(),
    }
}

/// SP-B2b: `∀R.Atomic(K)` (non-inverse `R`) — the general-∀ filler the saturator
/// otherwise drops. Mirror of `forall_oneof_members` for an atomic filler.
fn forall_atomic_member(c: ConceptId, pool: &ConceptPool) -> Option<(RoleId, ClassId)> {
    let ConceptExpr::All(role, inner) = pool.get(c) else {
        return None;
    };
    if role.is_inverse() {
        return None;
    }
    if let ConceptExpr::Atomic(k) = pool.get(*inner) {
        Some((role.role_id(), *k))
    } else {
        None
    }
}

/// SP-B2b: `∀R.Atomic(K)` restrictions that are `c` itself or a top-level `And`
/// operand of `c`. Mirror of `forall_oneof_operands_on_right`.
fn forall_atomic_operands_on_right(c: ConceptId, pool: &ConceptPool) -> Vec<(RoleId, ClassId)> {
    match pool.get(c) {
        ConceptExpr::And(operands) => operands
            .iter()
            .filter_map(|&op| forall_atomic_member(op, pool))
            .collect(),
        _ => forall_atomic_member(c, pool).into_iter().collect(),
    }
}

fn atomic_operands_on_right(c: ConceptId, pool: &ConceptPool) -> Vec<ClassId> {
    match pool.get(c) {
        ConceptExpr::Atomic(id) => vec![*id],
        ConceptExpr::And(operands) => operands
            .iter()
            .filter_map(|&op| match pool.get(op) {
                ConceptExpr::Atomic(id) => Some(*id),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Named roles `R` such that `SelfRestriction(R)` (`∃R.Self`, i.e.
/// `ObjectHasSelf`) is `c` itself or a top-level `And` operand of `c`. Each
/// witnesses `subject ⊑ ∃R.Self`: the subject has an R-edge to ITSELF, so it is
/// both the source and the target of an R-edge ⟹ `subject ⊑ domain(R) ⊓ range(R)`.
/// Inverse self-restrictions are skipped (the saturator's domain/range tables are
/// keyed on forward roles). A sound under-approximation — the full `∃R.Self`
/// semantics (the successor coincides with the node, so it also satisfies `∃R.C`
/// for every `C` the node has) is otherwise dropped; the tableau handles those.
fn self_restriction_roles_on_right(c: ConceptId, pool: &ConceptPool) -> Vec<RoleId> {
    let role_of = |cid: ConceptId| -> Option<RoleId> {
        match pool.get(cid) {
            ConceptExpr::SelfRestriction(r) if !r.is_inverse() => Some(r.role_id()),
            _ => None,
        }
    };
    match pool.get(c) {
        ConceptExpr::SelfRestriction(_) => role_of(c).into_iter().collect(),
        ConceptExpr::And(operands) => operands.iter().filter_map(|&op| role_of(op)).collect(),
        _ => Vec::new(),
    }
}

/// The `Y`s such that `Not(Atomic(Y))` is `c` itself or a top-level
/// `And` operand of `c`. Each witnesses `subject ⊑ ¬Y`, i.e.
/// `disjoint(subject, Y)`. Only literal negated atomics are recognised
/// (a sound under-approximation — anything else stays dropped).
fn not_atomic_operands_on_right(c: ConceptId, pool: &ConceptPool) -> Vec<ClassId> {
    let negated_atomic = |cid: ConceptId| -> Option<ClassId> {
        match pool.get(cid) {
            ConceptExpr::Not(inner) => match pool.get(*inner) {
                ConceptExpr::Atomic(y) => Some(*y),
                _ => None,
            },
            _ => None,
        }
    };
    match pool.get(c) {
        ConceptExpr::And(operands) => operands
            .iter()
            .filter_map(|&op| negated_atomic(op))
            .collect(),
        _ => negated_atomic(c).into_iter().collect(),
    }
}

/// Convert the `HashMap` closure produced by `build_role_super` into a
/// dense `Vec<Box<[RoleId]>>` indexed by `RoleId::index()`.
///
/// Each slot holds the sorted super-role slice for that role
/// (including reflexive self), enabling O(1) `Vec` indexing in the
/// hot saturation loop instead of `SipHash`-keyed `HashMap` lookups.
///
/// All vocabulary roles lie in `0..num_roles` by construction
/// (dense sequential assignment), so every lookup is in-bounds.
pub(crate) fn freeze_role_super(closure: &HashMap<RoleId, HashSet<RoleId>>) -> Vec<Box<[RoleId]>> {
    let num_roles = closure.len();
    let mut dense: Vec<Box<[RoleId]>> = vec![Box::new([]) as Box<[RoleId]>; num_roles];
    for (&r, supers) in closure {
        let idx = r.index() as usize;
        if idx < num_roles {
            let mut v: Vec<RoleId> = supers.iter().copied().collect();
            v.sort_unstable_by_key(|x| x.index());
            dense[idx] = v.into_boxed_slice();
        }
    }
    dense
}

/// Build the reflexive-transitive closure of the named-role
/// sub-property relation. `result[r]` is the set of named roles `s`
/// such that `r ⊑ s` (including `r` itself).
///
/// Sources:
/// - `SubObjectPropertyOf(r, s)` with both sides named.
/// - `EquivalentObjectProperties(rs)` decomposed pairwise.
///
/// Inverse-role sub-properties are ignored — Phase 6's EL scope is
/// named-roles only. Role chain LHS sub-properties are likewise
/// ignored: chain semantics belong to the tableau path.
fn build_role_super(internal: &InternalOntology) -> HashMap<RoleId, HashSet<RoleId>> {
    let num_roles = internal.vocabulary.num_roles();
    // Fast path: build the direct edge matrix as bitsets (densely indexed by
    // RoleId::index()), then run Floyd-Warshall's transitive closure in one pass
    // using bitwise OR — O(n³/64) instead of the old HashMap-iteration O(n³).
    // For galen/notgalen (few hundred roles) the bitset pass is ~microseconds;
    // for large EL ontologies with many roles (go-basic) it's still O(n²/64)
    // per iteration.  After the bitset closure is computed, we convert to the
    // caller's expected `HashMap<RoleId, HashSet<RoleId>>` shape.
    let mut bs: Vec<FixedBitSet> = vec![FixedBitSet::with_capacity(num_roles); num_roles];
    // Reflexive edges.
    for (i, row) in bs.iter_mut().enumerate() {
        row.insert(i);
    }
    let edge = |role: &Role| -> Option<RoleId> {
        if role.is_inverse() {
            None
        } else {
            Some(role.role_id())
        }
    };
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(sub_role),
                sup,
            } => {
                if let (Some(a), Some(b)) = (edge(sub_role), edge(sup)) {
                    let (ai, bi) = (a.index() as usize, b.index() as usize);
                    if ai < num_roles && bi < num_roles {
                        bs[ai].insert(bi);
                    }
                }
            }
            Axiom::EquivalentObjectProperties(members) => {
                let named: Vec<usize> = members
                    .iter()
                    .filter_map(edge)
                    .map(|r| r.index() as usize)
                    .filter(|&i| i < num_roles)
                    .collect();
                for &ai in &named {
                    for &bi in &named {
                        if ai != bi {
                            bs[ai].insert(bi);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Floyd-Warshall transitive closure on the bitset matrix:
    // for each intermediate node k, for each source i, if i reaches k,
    // union row i with row k (reaching everything k reaches).
    // Avoids snapshots and per-element HashMap insertions.
    for k in 0..num_roles {
        // Work on a cloned snapshot of row k to avoid aliasing.
        let row_k = bs[k].clone();
        for row in &mut bs {
            if row.contains(k) {
                row.union_with(&row_k);
            }
        }
    }
    // Convert to the expected HashMap output.
    let mut closure: HashMap<RoleId, HashSet<RoleId>> = HashMap::with_capacity(num_roles);
    for (i, row) in bs.iter().enumerate() {
        let r = RoleId::new(u32::try_from(i).expect("role count fits in u32"));
        let set: HashSet<RoleId> = row
            .ones()
            .map(|j| RoleId::new(u32::try_from(j).expect("role count fits in u32")))
            .collect();
        closure.insert(r, set);
    }
    closure
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    fn parse_internal(src: &str) -> InternalOntology {
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("ofn parses");
        convert_ontology(&onto).expect("conversion")
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn class(internal: &InternalOntology, local: &str) -> ClassId {
        internal
            .vocabulary
            .class_id(&format!("http://rustdl.test/{local}"))
            .expect("class declared")
    }

    /// NOMINAL-FILLER TYPING canary (`RUSTDL_NOMINAL_TYPING`, default-ON): a nominal
    /// filler typed by `ClassAssertion` lifts the existential — `X ⊑ ∃r.{a}`, `a:C`,
    /// `D ≡ ∃r.C` ⟹ `X ⊑ D` (via `NomKey(a) ⊑ C` + CR5). The DMOP gap pattern.
    #[test]
    fn nominal_filler_typing_lifts_existential() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/nt>\n\
    Declaration(Class(:X)) Declaration(Class(:C)) Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:a))\n\
    ClassAssertion(:C :a)\n\
    SubClassOf(:X ObjectHasValue(:r :a))\n\
    EquivalentClasses(:D ObjectSomeValuesFrom(:r :C))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "D")),
            "X ⊑ ∃r.{{a}} + a:C + D≡∃r.C ⟹ X ⊑ D (nominal-filler typing)"
        );
    }

    /// ONEOF-SUBSUMER positive (`RUSTDL_ONEOF_SUBSUMER`, default-ON): every member of
    /// an enumerated class is typed `C` ⟹ the class is `⊑ C`. The ORE 5107 pattern.
    #[test]
    fn oneof_subsumer_all_members_typed() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/oo>\n\
    Declaration(Class(:X)) Declaration(Class(:C))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    ClassAssertion(:C :a) ClassAssertion(:C :b)\n\
    EquivalentClasses(:X ObjectOneOf(:a :b))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "C")),
            "X ≡ {{a,b}} + a:C + b:C ⟹ X ⊑ C (oneof-subsumer)"
        );
    }

    /// ONEOF-SUBSUMER cascade: a told subclass of the enumerated class inherits the
    /// seeded subsumption via told-closure (ORE 5107 `LeisureTime ⊑ Anytime ⊑ Hours`).
    #[test]
    fn oneof_subsumer_cascades_via_told() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/oo>\n\
    Declaration(Class(:X)) Declaration(Class(:C)) Declaration(Class(:Y))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    ClassAssertion(:C :a) ClassAssertion(:C :b)\n\
    EquivalentClasses(:X ObjectOneOf(:a :b))\n\
    SubClassOf(:Y :X)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "Y"), class(&internal, "C")),
            "Y ⊑ X ≡ {{a,b}} + a,b:C ⟹ Y ⊑ C (cascade)"
        );
    }

    /// ONEOF-SUBSUMER NEGATIVE (FP landmine): a member with NO asserted type is
    /// unconstrained ⟹ the intersection collapses to ∅ ⟹ NO subsumption seeded.
    #[test]
    fn oneof_subsumer_typeless_member_no_seed() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/oo>\n\
    Declaration(Class(:X)) Declaration(Class(:C))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    ClassAssertion(:C :a)\n\
    EquivalentClasses(:X ObjectOneOf(:a :b))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.contains(class(&internal, "X"), class(&internal, "C")),
            "X ≡ {{a,b}} + a:C only (b typeless) ⟹ X ⊄ C (no FP)"
        );
    }

    /// ONEOF-SUBSUMER NEGATIVE: members disagree on type ⟹ NO common subsumer.
    #[test]
    fn oneof_subsumer_disagreeing_members_no_seed() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/oo>\n\
    Declaration(Class(:X)) Declaration(Class(:C)) Declaration(Class(:D))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    ClassAssertion(:C :a) ClassAssertion(:D :b)\n\
    EquivalentClasses(:X ObjectOneOf(:a :b))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.contains(class(&internal, "X"), class(&internal, "C")),
            "X ≡ {{a,b}} + a:C + b:D ⟹ X ⊄ C (no FP)"
        );
    }

    /// ONEOF-SUBSUMER NEGATIVE: a non-nominal disjunct means `X` is NOT bounded by the
    /// enumeration ⟹ the all-nominal guard must reject ⟹ NO subsumption seeded.
    #[test]
    fn oneof_subsumer_non_nominal_disjunct_no_seed() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/oo>\n\
    Declaration(Class(:X)) Declaration(Class(:C)) Declaration(Class(:E))\n\
    Declaration(NamedIndividual(:a))\n\
    ClassAssertion(:C :a)\n\
    SubClassOf(:X ObjectUnionOf(ObjectOneOf(:a) :E))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.contains(class(&internal, "X"), class(&internal, "C")),
            "X ⊑ {{a}} ⊔ E + a:C ⟹ X ⊄ C (non-nominal disjunct, no FP)"
        );
    }

    #[test]
    fn transitive_subsumption_closes() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "A"), class(&internal, "B")));
        assert!(subs.contains(class(&internal, "B"), class(&internal, "C")));
        assert!(subs.contains(class(&internal, "A"), class(&internal, "C")));
    }

    #[test]
    fn and_right_distributes() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A ObjectIntersectionOf(:B :C))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "A"), class(&internal, "B")));
        assert!(subs.contains(class(&internal, "A"), class(&internal, "C")));
    }

    #[test]
    fn and_left_conjunctive_trigger_fires() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:X))\n\
    SubClassOf(:X :A)\n\
    SubClassOf(:X :B)\n\
    SubClassOf(ObjectIntersectionOf(:A :B) :C)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "X"), class(&internal, "C")));
    }

    #[test]
    fn equivalent_classes_both_directions() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    EquivalentClasses(:A :B)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "A"), class(&internal, "B")));
        assert!(subs.contains(class(&internal, "B"), class(&internal, "A")));
    }

    #[test]
    fn existential_propagation_pizza_food() {
        // Classic EL pattern:
        //   Pizza        ⊑ ∃hasTopping.Topping
        //   Topping      ⊑ EdibleThing
        //   ∃hasTopping.EdibleThing ⊑ FoodItem
        // ⇒ Pizza ⊑ FoodItem.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Pizza))\n\
    Declaration(Class(:Topping))\n\
    Declaration(Class(:EdibleThing))\n\
    Declaration(Class(:FoodItem))\n\
    Declaration(ObjectProperty(:hasTopping))\n\
    SubClassOf(:Pizza ObjectSomeValuesFrom(:hasTopping :Topping))\n\
    SubClassOf(:Topping :EdibleThing)\n\
    SubClassOf(ObjectSomeValuesFrom(:hasTopping :EdibleThing) :FoodItem)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Pizza"), class(&internal, "FoodItem")));
    }

    #[test]
    fn role_hierarchy_propagates_through_existential() {
        // SubObjectPropertyOf(hasOwner, hasContact); a—hasOwner→...
        // existential on the right; ∃hasContact-trigger on the left.
        // The fact's role (hasOwner) is a sub-role of the trigger's
        // (hasContact) — saturation should fire across the hierarchy.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Pet))\n\
    Declaration(Class(:Person))\n\
    Declaration(Class(:Reachable))\n\
    Declaration(ObjectProperty(:hasOwner))\n\
    Declaration(ObjectProperty(:hasContact))\n\
    SubObjectPropertyOf(:hasOwner :hasContact)\n\
    SubClassOf(:Pet ObjectSomeValuesFrom(:hasOwner :Person))\n\
    SubClassOf(ObjectSomeValuesFrom(:hasContact :Person) :Reachable)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Pet"), class(&internal, "Reachable")));
    }

    #[test]
    fn role_chain_propagates_through_two_existentials() {
        // SubObjectPropertyOf(ObjectPropertyChain(hasParent, hasBrother), hasUncle).
        // Niece ⊑ ∃hasParent.Parent.
        // Parent ⊑ ∃hasBrother.Man.
        // ∃hasUncle.Man ⊑ HasUncle.
        // ⇒ Niece ⊑ HasUncle.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Niece))\n\
    Declaration(Class(:Parent))\n\
    Declaration(Class(:Man))\n\
    Declaration(Class(:HasUncle))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(ObjectProperty(:hasBrother))\n\
    Declaration(ObjectProperty(:hasUncle))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:hasParent :hasBrother) :hasUncle)\n\
    SubClassOf(:Niece ObjectSomeValuesFrom(:hasParent :Parent))\n\
    SubClassOf(:Parent ObjectSomeValuesFrom(:hasBrother :Man))\n\
    SubClassOf(ObjectSomeValuesFrom(:hasUncle :Man) :HasUncle)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Niece"), class(&internal, "HasUncle")));
    }

    #[test]
    fn transitive_role_chains_two_existentials() {
        // TransitiveObjectProperty(partOf) ≡ partOf ∘ partOf ⊑ partOf.
        // Finger ⊑ ∃partOf.Hand.
        // Hand ⊑ ∃partOf.Arm.
        // ∃partOf.Arm ⊑ HasArmRoot.
        // ⇒ Finger ⊑ HasArmRoot.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Finger))\n\
    Declaration(Class(:Hand))\n\
    Declaration(Class(:Arm))\n\
    Declaration(Class(:HasArmRoot))\n\
    Declaration(ObjectProperty(:partOf))\n\
    TransitiveObjectProperty(:partOf)\n\
    SubClassOf(:Finger ObjectSomeValuesFrom(:partOf :Hand))\n\
    SubClassOf(:Hand ObjectSomeValuesFrom(:partOf :Arm))\n\
    SubClassOf(ObjectSomeValuesFrom(:partOf :Arm) :HasArmRoot)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Finger"), class(&internal, "HasArmRoot")));
    }

    #[test]
    fn transitive_role_chains_three_hops() {
        // TransitiveObjectProperty(partOf); Finger ⊑ ∃partOf.Hand,
        // Hand ⊑ ∃partOf.Arm, Arm ⊑ ∃partOf.Body. With derived
        // existentials, the closure should reach Finger ⊑ ∃partOf.Body
        // (3 hops). The trigger ∃partOf.Body ⊑ BodyPart then fires
        // on Finger, Hand, and Arm.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Finger))\n\
    Declaration(Class(:Hand))\n\
    Declaration(Class(:Arm))\n\
    Declaration(Class(:Body))\n\
    Declaration(Class(:BodyPart))\n\
    Declaration(ObjectProperty(:partOf))\n\
    TransitiveObjectProperty(:partOf)\n\
    SubClassOf(:Finger ObjectSomeValuesFrom(:partOf :Hand))\n\
    SubClassOf(:Hand ObjectSomeValuesFrom(:partOf :Arm))\n\
    SubClassOf(:Arm ObjectSomeValuesFrom(:partOf :Body))\n\
    SubClassOf(ObjectSomeValuesFrom(:partOf :Body) :BodyPart)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Arm"), class(&internal, "BodyPart")));
        assert!(subs.contains(class(&internal, "Hand"), class(&internal, "BodyPart")));
        assert!(subs.contains(class(&internal, "Finger"), class(&internal, "BodyPart")));
    }

    #[test]
    fn property_domain_propagates_to_subjects() {
        // ObjectPropertyDomain(hasOwner, Person); Pet ⊑ ∃hasOwner.Dog
        // ⇒ Pet ⊑ Person (anything with a hasOwner-edge is a Person).
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Pet))\n\
    Declaration(Class(:Dog))\n\
    Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasOwner))\n\
    ObjectPropertyDomain(:hasOwner :Person)\n\
    SubClassOf(:Pet ObjectSomeValuesFrom(:hasOwner :Dog))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Pet"), class(&internal, "Person")));
    }

    #[test]
    fn property_range_does_not_force_target_type_subsumption() {
        // ObjectPropertyRange(hasOwner, Person); Pet ⊑ ∃hasOwner.Dog
        // does **not** entail Dog ⊑ Person — the range applies to
        // *instances* that happen to be R-successors, not to the type
        // used as the existential's target. A `Dog` that's nobody's
        // pet escapes the range obligation. Konclude agrees: classify
        // this ontology and you get `Dog ⊑ Thing`, `Person ⊑ Thing`,
        // no `Dog ⊑ Person`. The previous test asserted the opposite
        // and was the latent encoding of the 38 SIO FPs traced
        // 2026-05-28; the unsound derivation was removed from
        // `process_fact`.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Pet))\n\
    Declaration(Class(:Dog))\n\
    Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasOwner))\n\
    ObjectPropertyRange(:hasOwner :Person)\n\
    SubClassOf(:Pet ObjectSomeValuesFrom(:hasOwner :Dog))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(!subs.contains(class(&internal, "Dog"), class(&internal, "Person")));
    }

    #[test]
    fn property_range_constrains_synthetic_witness_via_tseitin() {
        // Sound counterpart of the unsound `Dog ⊑ Person` derivation:
        // ObjectPropertyRange(hasOwner, Person) + Pet ⊑ ∃hasOwner.Dog
        // means the hasOwner-witness of a Pet is in Dog ⊓ Person —
        // even though Dog itself isn't subsumed by Person. The Tseitin
        // encoding lowers the existential body to a synthetic F with
        // F ⊑ Dog and F ⊑ Person, so the trigger
        // `∃hasOwner.Person ⊑ HasHumanOwner` fires on Pet via F.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Pet))\n\
    Declaration(Class(:Dog))\n\
    Declaration(Class(:Person))\n\
    Declaration(Class(:HasHumanOwner))\n\
    Declaration(ObjectProperty(:hasOwner))\n\
    ObjectPropertyRange(:hasOwner :Person)\n\
    SubClassOf(:Pet ObjectSomeValuesFrom(:hasOwner :Dog))\n\
    SubClassOf(ObjectSomeValuesFrom(:hasOwner :Person) :HasHumanOwner)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Pet"), class(&internal, "HasHumanOwner")));
        // The unsound class-level Dog ⊑ Person must still NOT hold.
        assert!(!subs.contains(class(&internal, "Dog"), class(&internal, "Person")));
    }

    #[test]
    fn property_range_via_super_role_constrains_witness() {
        // Sub-role inherits its super-role's range: SubProperty(r, s),
        // Range(s, C). A hasOwner-witness (via r) is also an s-witness,
        // so it must be in C. The Tseitin fold should look up the
        // super-role's range when lowering the r-existential.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:Has))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SubObjectPropertyOf(:r :s)\n\
    ObjectPropertyRange(:s :C)\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
    SubClassOf(ObjectSomeValuesFrom(:r :C) :Has)\n\
)\n"
        ));
        let subs = saturate(&internal);
        // The r-witness for A is in B ⊓ C (via Range(s)); the trigger
        // `∃r.C ⊑ Has` fires.
        assert!(subs.contains(class(&internal, "A"), class(&internal, "Has")));
    }

    #[test]
    fn lhs_conjunction_with_existential_operand_fires() {
        // EL+ pattern from SIO: hypernym/synonym are both defined as a
        // conjunction of an atomic class plus an existential. With sub-
        // role relations linking the existentials' roles, one is ⊑ the
        // other. The previous EL lowering dropped any LHS conjunction
        // containing an existential operand and missed this entirely.
        //
        // - Synonym ≡ Word ⊓ ∃refersTo.Concept
        // - Hypernym ≡ Word ⊓ ∃refersToBroader.Concept
        // - refersToBroader ⊑ refersTo
        // Then Hypernym ⊑ Synonym (a hypernym's referent witnesses
        // satisfy the synonym's existential via the super-role).
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Synonym))\n\
    Declaration(Class(:Hypernym))\n\
    Declaration(Class(:Word))\n\
    Declaration(Class(:Concept))\n\
    Declaration(ObjectProperty(:refersTo))\n\
    Declaration(ObjectProperty(:refersToBroader))\n\
    SubObjectPropertyOf(:refersToBroader :refersTo)\n\
    EquivalentClasses(:Synonym ObjectIntersectionOf(:Word ObjectSomeValuesFrom(:refersTo :Concept)))\n\
    EquivalentClasses(:Hypernym ObjectIntersectionOf(:Word ObjectSomeValuesFrom(:refersToBroader :Concept)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "Hypernym"), class(&internal, "Synonym")),
            "Hypernym ⊑ Synonym should hold via LHS-conjunctive-existential lowering"
        );
    }

    #[test]
    fn lhs_conjunction_existential_marker_is_shared_across_conjunctions() {
        // Two distinct conjunctions reference the same `∃r.B` shape.
        // The Tseitin existential-marker cache should reuse one marker
        // F so the trigger `∃r.B ⊑ F` fires once and both conjunctive
        // triggers fire when an A picks up F as a subsumer.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:C1))\n\
    Declaration(Class(:C2))\n\
    Declaration(Class(:A1))\n\
    Declaration(Class(:A2))\n\
    Declaration(Class(:Target))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(ObjectIntersectionOf(:A1 ObjectSomeValuesFrom(:r :B)) :C1)\n\
    SubClassOf(ObjectIntersectionOf(:A2 ObjectSomeValuesFrom(:r :B)) :C2)\n\
    SubClassOf(:Target :A1)\n\
    SubClassOf(:Target :A2)\n\
    SubClassOf(:Target ObjectSomeValuesFrom(:r :B))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "Target"), class(&internal, "C1")));
        assert!(subs.contains(class(&internal, "Target"), class(&internal, "C2")));
    }

    #[test]
    fn lhs_conjunction_with_unsupported_operand_is_dropped() {
        // If the LHS conjunction contains an operand neither atomic
        // nor a named-role existential (here: a top-level disjunction),
        // the whole trigger must be dropped — partial lowering would
        // fire when only the lowerable operands match. The hypertableau
        // path still handles the dropped axiom.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(Class(:Sink))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(ObjectIntersectionOf(:A ObjectUnionOf(:B :C) ObjectSomeValuesFrom(:r :D)) :Sink)\n\
    SubClassOf(:A :A)\n\
)\n"
        ));
        let subs = saturate(&internal);
        // Sanity: ordinary subsumption still works after the drop.
        assert!(!subs.contains(class(&internal, "A"), class(&internal, "Sink")));
    }

    #[test]
    fn min_cardinality_on_rhs_lowers_to_existential() {
        // The SIO_010008 ⊑ biopolymer pattern (smaller form): a class
        // with `≥n R.C` on the RHS should be treated as having
        // `∃R.C` for EL closure purposes. Sound under-approximation:
        // `≥n R.C` implies `∃R.C` for n ≥ 1, and the EL pass then
        // fires `∃R.C ⊑ Head` triggers correctly.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:Head))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectMinCardinality(2 :r :B))\n\
    SubClassOf(ObjectSomeValuesFrom(:r :B) :Head)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "A"), class(&internal, "Head")),
            "≥2 R.B on RHS should fire ∃R.B trigger"
        );
    }

    #[test]
    fn existential_with_union_body_on_trigger_lhs_fires_per_operand() {
        // `∃R.Or(B, C) ⊑ Head` should fire when X has ∃R.B OR ∃R.C —
        // sound because ∃R.B ⊑ ∃R.(B ⊔ C). The trigger lowering emits
        // one ExistentialTrigger per Or operand, all sharing the head.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X1))\n\
    Declaration(Class(:X2))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:Head))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C)) :Head)\n\
    SubClassOf(:X1 ObjectSomeValuesFrom(:r :B))\n\
    SubClassOf(:X2 ObjectSomeValuesFrom(:r :C))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "X1"), class(&internal, "Head")));
        assert!(subs.contains(class(&internal, "X2"), class(&internal, "Head")));
    }

    #[test]
    fn lhs_conjunction_with_union_existential_body_fires() {
        // The SIO biopolymer pattern: `∃R.Or(...) ⊓ A ⊑ Target`. The
        // Tseitin marker covers all operands; the conjunctive trigger
        // fires when any operand's existential plus the atomic A both
        // hold on a class.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Target))\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:X))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r ObjectUnionOf(:B :C))) :Target)\n\
    SubClassOf(:X :A)\n\
    SubClassOf(:X ObjectSomeValuesFrom(:r :B))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "Target")),
            "X has A and ∃r.B (a Union-operand), so X ⊑ Target via the LHS-conjunctive-Or-body rule"
        );
    }

    #[test]
    fn min_cardinality_with_super_role_chains_through_union() {
        // Combined exercise of all new features: SIO_010008-style
        // pattern. SubClassOf(A, ≥2 r.C); SubObjectPropertyOf(r, s);
        // SubClassOf(∃s.Or(C, D), Head). Need: ≥n → ∃, super-role
        // propagation, Or-on-trigger-LHS-body.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(Class(:Head))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SubObjectPropertyOf(:r :s)\n\
    SubClassOf(:A ObjectMinCardinality(2 :r :C))\n\
    SubClassOf(ObjectSomeValuesFrom(:s ObjectUnionOf(:C :D)) :Head)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "A"), class(&internal, "Head")),
            "A ⊑ Head via ≥2r.C → ∃r.C → ∃s.C (super-role) → ∃s.Or(C,D) → Head"
        );
    }

    #[test]
    fn nested_existential_in_outer_body_lowers_via_marker() {
        // SIO SIO_010038 / SIO_010410 shape: outer existential's body
        // is `B ⊓ ∃R'.C`. With nested-existential lowering, the inner
        // `∃R'.C` becomes a marker `M` (via `∃R'.C ⊑ M`), the outer
        // body becomes Tseitin(`B ⊓ M`), and CR5 propagation can fire
        // triggers on the synthetic.
        //
        // Setup: A ⊑ ∃r.(B ⊓ ∃s.C); B ⊑ Q; ∃r.Q ⊑ Head.
        // The outer body's Tseitin F has Q as subsumer (via F ⊑ B ⊑ Q),
        // and the trigger ∃r.Q ⊑ Head fires on A.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:Q))\n\
    Declaration(Class(:Head))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B ObjectSomeValuesFrom(:s :C))))\n\
    SubClassOf(:B :Q)\n\
    SubClassOf(ObjectSomeValuesFrom(:r :Q) :Head)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "A"), class(&internal, "Head")),
            "A ⊑ Head via nested-existential body lowering"
        );
    }

    #[test]
    fn existential_with_unsat_body_propagates_to_source() {
        // DisjointClasses(A, B); Y ⊑ A; Y ⊑ B (Y is unsat);
        // Test ≡ ∃r.(A ⊓ B ⊓ Y).
        // The Tseitin synthetic for the body has A and B as
        // subsumers and is thus unsat. The existential fact
        // (Test, r, synth) then propagates unsat back to Test.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Test))\n\
    Declaration(Class(:Y))\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    DisjointClasses(:A :B)\n\
    SubClassOf(:Y :A)\n\
    SubClassOf(:Y :B)\n\
    EquivalentClasses(:Test ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A :B :Y)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.is_unsatisfiable(class(&internal, "Y")),
            "Y ⊑ A ⊓ B must be unsat"
        );
        assert!(
            subs.is_unsatisfiable(class(&internal, "Test")),
            "Test ≡ ∃r.<unsat> must itself be unsat"
        );
    }

    #[test]
    fn equivalent_classes_with_compound_existential_decomposes() {
        // Test ≡ ∃r.B; X ⊑ ∃r.B  ⇒  X ⊑ Test should hold via the
        // existential trigger introduced by the equivalence's
        // backward direction (∃r.B ⊑ Test).
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Test))\n\
    Declaration(Class(:X))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    EquivalentClasses(:Test ObjectSomeValuesFrom(:r :B))\n\
    SubClassOf(:X ObjectSomeValuesFrom(:r :B))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "X"), class(&internal, "Test")));
    }

    #[test]
    fn disjoint_classes_makes_intersection_unsat() {
        // DisjointClasses(A, B); X ⊑ A; X ⊑ B ⇒ X is unsat.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:X))\n\
    DisjointClasses(:A :B)\n\
    SubClassOf(:X :A)\n\
    SubClassOf(:X :B)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.is_unsatisfiable(class(&internal, "X")));
        assert!(!subs.is_unsatisfiable(class(&internal, "A")));
        assert!(!subs.is_unsatisfiable(class(&internal, "B")));
    }

    #[test]
    fn subclass_of_complement_conjunct_makes_class_unsat() {
        // `A ⊑ B ⊓ ¬B` ⇒ A ⊑ ⊥. The `¬B` conjunct is `A ⊑ ¬B`, i.e.
        // disjoint(A, B); with the told `A ⊑ B` the disjointness→unsat
        // rule fires. Previously the saturator dropped the `¬B` (it only
        // derived disjoint pairs from explicit DisjointClasses), so it
        // missed this — the Horn fast-path unsat gap.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A ObjectIntersectionOf(:B ObjectComplementOf(:B)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.is_unsatisfiable(class(&internal, "A")));
        assert!(!subs.is_unsatisfiable(class(&internal, "B")));
    }

    #[test]
    fn subclass_of_complement_disjointness_is_directional_and_sound() {
        // `A ⊑ ¬B` registers disjoint(A, B) but does NOT by itself make
        // A or B unsat (A simply can't also be B). Guards against an
        // over-eager fix that flags satisfiable classes.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A ObjectComplementOf(:B))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(!subs.is_unsatisfiable(class(&internal, "A")));
        assert!(!subs.is_unsatisfiable(class(&internal, "B")));
    }

    #[test]
    fn tseitin_introduces_synthetic_for_compound_existential_body() {
        // X ⊑ ∃r.(B ⊓ C); ∃r.B_and_C_synth ⊑ W shouldn't be needed
        // — the trigger we have is over the *atomic* subsumers of
        // the synthetic, so any class with both B and C as
        // subsumers picks up the synthetic, and the trigger fires
        // from there.
        //
        // The reverse path: X has the existential fact (X, r, S)
        // where S is the synthetic. We trigger on
        // ∃r.B ⊑ W (note: trigger body is B, not the synthetic).
        // Because S ⊑ B (Tseitin emits this), B ∈ subsumers(S), so
        // the existing CR5 fires the trigger on X.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:W))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :C)))\n\
    SubClassOf(ObjectSomeValuesFrom(:r :B) :W)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "X"), class(&internal, "W")));
    }

    #[test]
    fn tseitin_trigger_side_compound_body_classifies() {
        // Symmetric: ∃r.(B ⊓ C) ⊑ W (compound body on the trigger
        // side). X has B and C as subsumers and an r-edge to
        // anything in B ⊓ C. With Tseitin the trigger body becomes
        // the synthetic S; we still need an existential fact whose
        // target has S in its subsumers.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X))\n\
    Declaration(Class(:Y))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:W))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Y :B)\n\
    SubClassOf(:Y :C)\n\
    SubClassOf(:X ObjectSomeValuesFrom(:r :Y))\n\
    SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :C)) :W)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "X"), class(&internal, "W")));
    }

    #[test]
    fn nominal_transitive_abox_fold_classifies() {
        // Wine region pattern: AlsatianWine ≡ Wine ⊓ ∃locatedIn.{Alsace};
        // FrenchWine ≡ Wine ⊓ ∃locatedIn.{French}; locatedIn transitive;
        // ABox Alsace locatedIn French. By transitivity AlsatianWine's
        // locatedIn-witness reaches French ⟹ AlsatianWine ⊑ FrenchWine.
        // Exercises the nominal NomKey fold (B) + transitive-ABox
        // propagation (A). EL alone drops nominal-filler existentials.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Wine))\n\
    Declaration(Class(:AlsatianWine))\n\
    Declaration(Class(:FrenchWine))\n\
    Declaration(NamedIndividual(:Alsace))\n\
    Declaration(NamedIndividual(:French))\n\
    Declaration(ObjectProperty(:locatedIn))\n\
    TransitiveObjectProperty(:locatedIn)\n\
    ObjectPropertyAssertion(:locatedIn :Alsace :French)\n\
    EquivalentClasses(:AlsatianWine ObjectIntersectionOf(:Wine ObjectHasValue(:locatedIn :Alsace)))\n\
    EquivalentClasses(:FrenchWine ObjectIntersectionOf(:Wine ObjectHasValue(:locatedIn :French)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(
                class(&internal, "AlsatianWine"),
                class(&internal, "FrenchWine")
            ),
            "nominal transitive-ABox fold failed: AlsatianWine ⊑ FrenchWine"
        );
        // Soundness: the reverse must NOT hold (French does not locate in Alsace).
        assert!(
            !subs.contains(
                class(&internal, "FrenchWine"),
                class(&internal, "AlsatianWine")
            ),
            "unsound: FrenchWine ⊑ AlsatianWine should not hold"
        );
    }

    #[test]
    fn transitive_abox_without_tbox_nominals_allocates_no_nomkeys() {
        // A transitive role with ABox edges but NO TBox nominal (`∃R.{a}` /
        // ObjectHasValue). `build_abox_nominal_reach`'s output
        // (`abox_nominal_reach`) is only ever consulted when a processed fact
        // targets a NomKey, which requires a TBox nominal — so with none, the
        // ABox NomKeys are provably inert and must not be allocated. Regression
        // guard for the ore_ont_3914 memory blow-up: 570K inert ABox NomKeys
        // were sizing the O(num_total_classes²) subsumer matrix into tens of GB.
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:x))\n\
    Declaration(NamedIndividual(:y))\n\
    Declaration(NamedIndividual(:z))\n\
    Declaration(ObjectProperty(:r))\n\
    TransitiveObjectProperty(:r)\n\
    ObjectPropertyAssertion(:r :x :y)\n\
    ObjectPropertyAssertion(:r :y :z)\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        let (subs, _facts, nom) = saturate_with_exists_facts(&internal);
        // TBox subsumption is still computed correctly.
        assert!(subs.contains(class(&internal, "A"), class(&internal, "B")));
        // No NomKeys allocated: the transitive ABox is inert for the class
        // hierarchy because no TBox construct consumes a nominal.
        assert!(
            nom.is_empty(),
            "expected no ABox NomKeys without a TBox nominal, got {}",
            nom.len()
        );
    }

    /// Cluster-C canary (wine residual-29, grape-varietal pattern): a defined
    /// class with a `≤n R` cardinality conjunct.
    /// `Gamay ≡ Wine ⊓ ∃madeFromGrape.{GamayGrape} ⊓ ≤1 madeFromGrape`;
    /// `Beaujolais` has all three told ⟹ `Beaujolais ⊑ Gamay`. Requires the
    /// `MaxKey` synthetic-subsumer lever (lower the `≤n` conjunct into a trackable
    /// marker on both the defined-class trigger and the told `≤n` seed) — EL
    /// drops the `≤n` conjunct today, so the whole `Gamay` trigger is dropped.
    /// The `MultiGrape` negative pins soundness: `∃grape` WITHOUT `≤1` must NOT
    /// classify under `Gamay` (else the lever degenerates to "drop the ≤n").
    /// See `docs/classify-recovery-scope-2026-06-07.md` §3.
    #[test]
    fn max_cardinality_nominal_varietal_classifies() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Wine))\n\
    Declaration(Class(:Gamay))\n\
    Declaration(Class(:Beaujolais))\n\
    Declaration(Class(:MultiGrape))\n\
    Declaration(NamedIndividual(:GamayGrape))\n\
    Declaration(ObjectProperty(:madeFromGrape))\n\
    EquivalentClasses(:Gamay ObjectIntersectionOf(:Wine ObjectHasValue(:madeFromGrape :GamayGrape) ObjectMaxCardinality(1 :madeFromGrape)))\n\
    SubClassOf(:Beaujolais :Wine)\n\
    SubClassOf(:Beaujolais ObjectHasValue(:madeFromGrape :GamayGrape))\n\
    SubClassOf(:Beaujolais ObjectMaxCardinality(1 :madeFromGrape))\n\
    SubClassOf(:MultiGrape :Wine)\n\
    SubClassOf(:MultiGrape ObjectHasValue(:madeFromGrape :GamayGrape))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "Beaujolais"), class(&internal, "Gamay")),
            "MaxKey lever: Beaujolais ⊑ Gamay (Wine ⊓ ∃madeFromGrape.{{g}} ⊓ ≤1)"
        );
        assert!(
            !subs.contains(class(&internal, "MultiGrape"), class(&internal, "Gamay")),
            "unsound: MultiGrape (∃grape, no ≤1) must NOT be ⊑ Gamay"
        );
    }

    /// SP-B1 differentiator: forced-disjunct via a DERIVED (not told) subsumer.
    /// `X ⊑ A1`, `X ⊑ A2`, `A1 ⊓ A2 ⊑ G` ⟹ the saturator derives `X ⊑ G` via the
    /// conjunctive trigger (NOT a told subsumption — the LHS is an intersection);
    /// `X ⊑ A⊔B`, `Disjoint(G,A)` ⟹ B1 forces `X ⊑ B`. SP-A's told-only pass cannot
    /// (G ∉ told-subsumers(X)) — this is exactly what the derived-closure rule adds.
    #[test]
    fn b1_forced_disjunct_via_derived_subsumer() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A1)) Declaration(Class(:A2))\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))\n\
    SubClassOf(:X :A1)\n\
    SubClassOf(:X :A2)\n\
    SubClassOf(ObjectIntersectionOf(:A1 :A2) :G)\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
    DisjointClasses(:G :A)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "G")),
            "precondition: X ⊑ G must be DERIVED via the A1⊓A2 conjunctive trigger"
        );
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "B")),
            "B1: forced-disjunct via derived subsumer G ⟹ X ⊑ B"
        );
        assert!(
            !subs.contains(class(&internal, "X"), class(&internal, "A")),
            "must NOT force the excluded disjunct A"
        );
    }

    /// SP-B1: forced-disjunct via a told subsumer. `X⊑G`, `X⊑A⊔B`, `Disjoint(G,A)`
    /// ⟹ `X⊑B`.
    #[test]
    fn b1_forced_disjunct_via_told_subsumer() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))\n\
    SubClassOf(:X :G)\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
    DisjointClasses(:G :A)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "B")),
            "X ⊑ B"
        );
    }

    /// SP-B1: all disjuncts excluded ⟹ class unsat. `X⊑G`, `X⊑A⊔B`,
    /// `Disjoint(G,A)`, `Disjoint(G,B)` ⟹ X unsatisfiable.
    #[test]
    fn b1_forced_to_bot() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))\n\
    SubClassOf(:X :G)\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
    DisjointClasses(:G :A)\n\
    DisjointClasses(:G :B)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.is_unsatisfiable(class(&internal, "X")),
            "X ⊑ ⊥ (all disjuncts excluded)"
        );
    }

    /// SP-B1: inherited disjunction. `X⊑C`, `C⊑A⊔B`, `X⊑G`, `Disjoint(G,A)` ⟹
    /// `X⊑B` (X inherits C's disjunction, forced by X's own subsumer G).
    #[test]
    fn b1_inherited_disjunction() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))\n\
    SubClassOf(:X :C)\n\
    SubClassOf(:C ObjectUnionOf(:A :B))\n\
    SubClassOf(:X :G)\n\
    DisjointClasses(:G :A)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "B")),
            "inherited: X ⊑ B"
        );
    }

    /// SP-B1 negative control: an undetermined disjunction forces nothing.
    /// `X⊑A⊔B` with no disjointness ⟹ no `X⊑A`/`X⊑B`, X satisfiable.
    #[test]
    fn b1_undetermined_forces_nothing() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B))\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.contains(class(&internal, "X"), class(&internal, "A")),
            "no spurious X ⊑ A"
        );
        assert!(
            !subs.contains(class(&internal, "X"), class(&internal, "B")),
            "no spurious X ⊑ B"
        );
        assert!(
            !subs.is_unsatisfiable(class(&internal, "X")),
            "X must stay satisfiable"
        );
    }

    /// SP-B1 negative control: nominal `⊔` is NOT ingested (atomic-only scope).
    /// `X ⊑ {a}⊔{b}` ⟹ B1 registers nothing; X stays satisfiable, no spurious force.
    #[test]
    fn b1_nominal_disjunction_not_touched() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
    SubClassOf(:X ObjectOneOf(:a :b))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.is_unsatisfiable(class(&internal, "X")),
            "nominal ⊔ untouched: X satisfiable"
        );
    }

    /// SP-B2a differentiator: forced-disjunct via a DEEP (non-disjoint-pair)
    /// incompatibility. `X⊑A⊔B`, `A⊑∃r.P`, `X⊑∃r.Q`, functional `r`, `Disjoint(P,Q)`
    /// ⟹ `X⊓A` is unsat via functional-merge (the single `r`-successor is `P⊓Q⊑⊥`),
    /// so the `Sₐ=X⊓A` synthetic becomes unsat ⟹ force `X⊑B`. B1 cannot force this
    /// (no subsumer of X is disjoint with A) — exactly what B2a's synthetic test adds.
    #[test]
    fn b2_forced_disjunct_via_deep_incompatibility() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B))\n\
    Declaration(Class(:P)) Declaration(Class(:Q))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :P))\n\
    SubClassOf(:X ObjectSomeValuesFrom(:r :Q))\n\
    FunctionalObjectProperty(:r)\n\
    DisjointClasses(:P :Q)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "X"), class(&internal, "B")),
            "B2a: X⊓A unsat via functional-merge ⟹ force X ⊑ B"
        );
        assert!(
            !subs.is_unsatisfiable(class(&internal, "X")),
            "X itself stays satisfiable (only A is excluded)"
        );
    }

    /// SP-B2a: both disjuncts deeply incompatible ⟹ class unsat. As above but also
    /// `B⊑∃r.P2`, `X⊑∃r.Q` functional, `Disjoint(P2,Q)` ⟹ X⊓A and X⊓B both unsat ⟹ X⊑⊥.
    #[test]
    fn b2_forced_to_bot_via_deep() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B))\n\
    Declaration(Class(:P)) Declaration(Class(:Q))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :P))\n\
    SubClassOf(:B ObjectSomeValuesFrom(:r :P))\n\
    SubClassOf(:X ObjectSomeValuesFrom(:r :Q))\n\
    FunctionalObjectProperty(:r)\n\
    DisjointClasses(:P :Q)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.is_unsatisfiable(class(&internal, "X")),
            "B2a: both X⊓A and X⊓B unsat ⟹ X ⊑ ⊥"
        );
    }

    /// SP-B2b differentiator: the wine/food Course hierarchy via `ForallAtomicKey`.
    /// `FishCourse ≡ MealCourse ⊓ ∀hasFood.Fish`, `SeafoodCourse ≡ MealCourse ⊓
    /// ∀hasFood.Seafood`, `Fish ⊑ Seafood` ⟹ `FishCourse ⊑ SeafoodCourse` via
    /// ∀-monotonicity (the saturator misses this without B2b). Transitive
    /// `BlandFishCourse ⊑ FishCourse ⊑ SeafoodCourse` with `BlandFish ⊑ Fish`.
    #[test]
    fn b2b_forall_course_hierarchy() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:MealCourse)) Declaration(Class(:FishCourse)) Declaration(Class(:SeafoodCourse)) Declaration(Class(:BlandFishCourse))\n\
    Declaration(Class(:Fish)) Declaration(Class(:Seafood)) Declaration(Class(:BlandFish))\n\
    Declaration(ObjectProperty(:hasFood))\n\
    SubClassOf(:Fish :Seafood)\n\
    SubClassOf(:BlandFish :Fish)\n\
    EquivalentClasses(:FishCourse ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:hasFood :Fish)))\n\
    EquivalentClasses(:SeafoodCourse ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:hasFood :Seafood)))\n\
    EquivalentClasses(:BlandFishCourse ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:hasFood :BlandFish)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(
                class(&internal, "FishCourse"),
                class(&internal, "SeafoodCourse")
            ),
            "B2b: FishCourse ⊑ SeafoodCourse via ∀hasFood monotonicity (Fish⊑Seafood)"
        );
        assert!(
            subs.contains(
                class(&internal, "BlandFishCourse"),
                class(&internal, "SeafoodCourse")
            ),
            "B2b transitive: BlandFishCourse ⊑ SeafoodCourse"
        );
    }

    /// SP-B2b negative controls: no spurious ∀-subsumption. Unrelated fillers ⟹
    /// no Course subsumption; different role ⟹ no subsumption.
    #[test]
    fn b2b_forall_no_spurious() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:MealCourse)) Declaration(Class(:CourseA)) Declaration(Class(:CourseB)) Declaration(Class(:CourseC))\n\
    Declaration(Class(:K)) Declaration(Class(:L))\n\
    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
    EquivalentClasses(:CourseA ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:r :K)))\n\
    EquivalentClasses(:CourseB ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:r :L)))\n\
    EquivalentClasses(:CourseC ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:s :K)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.contains(class(&internal, "CourseA"), class(&internal, "CourseB")),
            "unrelated K,L (no K⊑L) ⟹ CourseA ⋢ CourseB"
        );
        assert!(
            !subs.contains(class(&internal, "CourseA"), class(&internal, "CourseC")),
            "different role (r vs s) ⟹ CourseA ⋢ CourseC"
        );
    }

    /// SP-B2c: union class. `Fruit ≡ NonSweetFruit ⊔ SweetFruit`, both ⊑ `EdibleThing`
    /// ⟹ `Fruit ⊑ EdibleThing` (#1 common-subsumer) AND `NonSweetFruit ⊑ Fruit`,
    /// `SweetFruit ⊑ Fruit` (#2 disjunct⊑union, equivalence-only).
    #[test]
    fn b2c_union_class_fruit() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Fruit)) Declaration(Class(:NonSweetFruit)) Declaration(Class(:SweetFruit)) Declaration(Class(:EdibleThing))\n\
    EquivalentClasses(:Fruit ObjectUnionOf(:NonSweetFruit :SweetFruit))\n\
    SubClassOf(:NonSweetFruit :EdibleThing)\n\
    SubClassOf(:SweetFruit :EdibleThing)\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "Fruit"), class(&internal, "EdibleThing")),
            "#1: Fruit ⊑ EdibleThing"
        );
        assert!(
            subs.contains(class(&internal, "NonSweetFruit"), class(&internal, "Fruit")),
            "#2: NonSweetFruit ⊑ Fruit"
        );
        assert!(
            subs.contains(class(&internal, "SweetFruit"), class(&internal, "Fruit")),
            "#2: SweetFruit ⊑ Fruit"
        );
    }

    /// SP-B2c × B2b combine: `NonSweetFruit ⊑ Fruit` (#2) + `ForallAtomicKey`
    /// monotonicity ⟹ `NonSweetFruitCourse ⊑ FruitCourse`.
    #[test]
    fn b2c_union_course_combine() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Fruit)) Declaration(Class(:NonSweetFruit)) Declaration(Class(:SweetFruit))\n\
    Declaration(Class(:MealCourse)) Declaration(Class(:FruitCourse)) Declaration(Class(:NonSweetFruitCourse))\n\
    Declaration(ObjectProperty(:hasFood))\n\
    EquivalentClasses(:Fruit ObjectUnionOf(:NonSweetFruit :SweetFruit))\n\
    EquivalentClasses(:FruitCourse ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:hasFood :Fruit)))\n\
    EquivalentClasses(:NonSweetFruitCourse ObjectIntersectionOf(:MealCourse ObjectAllValuesFrom(:hasFood :NonSweetFruit)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(
                class(&internal, "NonSweetFruitCourse"),
                class(&internal, "FruitCourse")
            ),
            "B2c×B2b: NonSweetFruitCourse ⊑ FruitCourse"
        );
    }

    /// SP-B2c negative control: #2 (disjunct⊑X) is EQUIVALENCE-ONLY. A plain
    /// `SubClassOf(X, A⊔B)` must NOT yield `A⊑X`/`B⊑X` (unsound: `X⊑A⊔B` ⊬ `A⊑X`).
    #[test]
    fn b2c_subclassof_or_no_disjunct_to_x() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:B))\n\
    SubClassOf(:X ObjectUnionOf(:A :B))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            !subs.contains(class(&internal, "A"), class(&internal, "X")),
            "X⊑A⊔B must NOT give A⊑X"
        );
        assert!(
            !subs.contains(class(&internal, "B"), class(&internal, "X")),
            "X⊑A⊔B must NOT give B⊑X"
        );
    }

    /// Cluster-B canary, path (a) (wine residual-9, sugar pattern): a defined
    /// class with a `∀R.OneOf(S)` conjunct. `WhiteNonSweet ≡ White ⊓
    /// ∀hasSugar.{Dry,OffDry}`; a sub `C` that has `C ⊑ White` and inherits a
    /// TOLD `∀hasSugar.{Dry,OffDry}` (via a varietal superclass `CheninBlanc`)
    /// ⟹ `C ⊑ WhiteNonSweet`. Requires the `ForallKey` synthetic-subsumer lever
    /// (the `∀R.OneOf` analog of `MaxKey`: lower the conjunct into a trackable
    /// `(R, S)` marker on both the defined-class trigger and the told-`∀` seed).
    /// `RedSugar` negative pins soundness: a `∀hasSugar.{Dry,Sweet}` (Sweet ∉
    /// the target set) must NOT classify under `WhiteNonSweet`.
    #[test]
    fn forall_oneof_nominal_sugar_classifies() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:White))\n\
    Declaration(Class(:WhiteNonSweet))\n\
    Declaration(Class(:CheninBlanc))\n\
    Declaration(Class(:Tours))\n\
    Declaration(Class(:RedSugar))\n\
    Declaration(NamedIndividual(:Dry))\n\
    Declaration(NamedIndividual(:OffDry))\n\
    Declaration(NamedIndividual(:Sweet))\n\
    Declaration(ObjectProperty(:hasSugar))\n\
    EquivalentClasses(:WhiteNonSweet ObjectIntersectionOf(:White ObjectAllValuesFrom(:hasSugar ObjectOneOf(:Dry :OffDry))))\n\
    SubClassOf(:CheninBlanc ObjectAllValuesFrom(:hasSugar ObjectOneOf(:Dry :OffDry)))\n\
    SubClassOf(:Tours :CheninBlanc)\n\
    SubClassOf(:Tours :White)\n\
    SubClassOf(:RedSugar :White)\n\
    SubClassOf(:RedSugar ObjectAllValuesFrom(:hasSugar ObjectOneOf(:Dry :Sweet)))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(class(&internal, "Tours"), class(&internal, "WhiteNonSweet")),
            "ForallKey lever: Tours ⊑ WhiteNonSweet (White ⊓ inherited ∀hasSugar.{{Dry,OffDry}})"
        );
        assert!(
            !subs.contains(
                class(&internal, "RedSugar"),
                class(&internal, "WhiteNonSweet")
            ),
            "unsound: RedSugar (∀hasSugar.{{Dry,Sweet}}, Sweet∉target) must NOT be ⊑ WhiteNonSweet"
        );
    }

    /// Cluster-B canary, path (b): a FUNCTIONAL role + `∃R.{a}` with `a ∈ S` ⟹
    /// `∀R.OneOf(S)` (the unique R-filler is `a`). `hasSugar` functional,
    /// `DryThing ⊑ White` + `∃hasSugar.{Dry}` ⟹ `DryThing ⊑ WhiteNonSweet`.
    /// Two soundness negatives: `SweetThing` (`∃hasSugar.{Sweet}`, Sweet∉target)
    /// and `NonFunc` (`∃g.{Dry}` on a NON-functional role) must NOT classify —
    /// `∃` without both `a∈S` AND functionality does not give `∀`.
    #[test]
    fn forall_oneof_functional_existential_classifies() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:White))\n\
    Declaration(Class(:WhiteNonSweet))\n\
    Declaration(Class(:NfTarget))\n\
    Declaration(Class(:DryThing))\n\
    Declaration(Class(:SweetThing))\n\
    Declaration(Class(:NonFunc))\n\
    Declaration(Class(:Max1Thing))\n\
    Declaration(NamedIndividual(:Dry))\n\
    Declaration(NamedIndividual(:OffDry))\n\
    Declaration(NamedIndividual(:Sweet))\n\
    Declaration(ObjectProperty(:hasSugar))\n\
    Declaration(ObjectProperty(:g))\n\
    FunctionalObjectProperty(:hasSugar)\n\
    EquivalentClasses(:WhiteNonSweet ObjectIntersectionOf(:White ObjectAllValuesFrom(:hasSugar ObjectOneOf(:Dry :OffDry))))\n\
    EquivalentClasses(:NfTarget ObjectIntersectionOf(:White ObjectAllValuesFrom(:g ObjectOneOf(:Dry :OffDry))))\n\
    SubClassOf(:DryThing :White)\n\
    SubClassOf(:DryThing ObjectHasValue(:hasSugar :Dry))\n\
    SubClassOf(:SweetThing :White)\n\
    SubClassOf(:SweetThing ObjectHasValue(:hasSugar :Sweet))\n\
    SubClassOf(:NonFunc :White)\n\
    SubClassOf(:NonFunc ObjectHasValue(:g :Dry))\n\
    SubClassOf(:Max1Thing :White)\n\
    SubClassOf(:Max1Thing ObjectHasValue(:g :Dry))\n\
    SubClassOf(:Max1Thing ObjectMaxCardinality(1 :g))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(
            subs.contains(
                class(&internal, "DryThing"),
                class(&internal, "WhiteNonSweet")
            ),
            "ForallKey path (b): DryThing ⊑ WhiteNonSweet (functional hasSugar + ∃hasSugar.{{Dry}})"
        );
        // ≤1-driven path (b): a told `≤1 g` (per-class, g NOT globally functional)
        // + `∃g.{Dry}` ⟹ `∀g.OneOf(Dry,OffDry)` ⟹ ⊑ NfTarget. (Sancerre pattern.)
        assert!(
            subs.contains(class(&internal, "Max1Thing"), class(&internal, "NfTarget")),
            "ForallKey path (b) ≤1-driven: Max1Thing ⊑ NfTarget (≤1 g + ∃g.{{Dry}})"
        );
        assert!(
            !subs.contains(
                class(&internal, "SweetThing"),
                class(&internal, "WhiteNonSweet")
            ),
            "unsound: SweetThing (∃hasSugar.{{Sweet}}, Sweet∉target) must NOT be ⊑ WhiteNonSweet"
        );
        assert!(
            !subs.contains(class(&internal, "NonFunc"), class(&internal, "NfTarget")),
            "unsound: NonFunc (∃g.{{Dry}}, NO ≤1, g non-functional) must NOT be ⊑ NfTarget"
        );
    }

    #[test]
    fn out_of_fragment_axioms_dont_panic() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
    SubClassOf(:A ObjectUnionOf(:B :A))\n\
)\n"
        ));
        let subs = saturate(&internal);
        assert!(subs.contains(class(&internal, "A"), class(&internal, "A")));
    }

    /// Phase 2a canary: synthetic mimicking GALEN's
    /// <Region>Pathology / `PathologicalCondition` pattern. A functional
    /// super-role `r_func` has two sibling sub-properties `r_i` and `r_j`.
    /// Class `Subject` has existential edges via both sub-properties;
    /// class `Target` is the conjunctive consumer through `r_func`.
    ///
    /// The expected entailment `Subject ⊑ Target` requires the EL++
    /// functional-role witness-merge rule. ASSERTS THE FIX (Phase 2a rule active).
    /// Do not delete; this canary is the regression test for the rule.
    #[test]
    fn functional_role_merge_canary_recovers_entailment() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2a/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2a/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(ObjectSomeValuesFrom(:r_func ObjectIntersectionOf(:A :B)) :Target)
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _prefixes): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("canary ontology parses");
        let internal = convert_ontology(&set_onto).expect("canary lowers to IR");
        let subsumers = crate::saturate(&internal);

        let subject = internal
            .vocabulary
            .class_id("http://rustdl.test/p2a/Subject")
            .expect("Subject declared");
        let target = internal
            .vocabulary
            .class_id("http://rustdl.test/p2a/Target")
            .expect("Target declared");

        assert!(
            subsumers.contains(subject, target),
            "Phase 2a regression: the functional-role witness-merge rule failed to derive \
             Subject ⊑ Target. The rule, the role-hierarchy index, or the runtime Tseitin \
             allocator likely regressed."
        );
    }

    /// Phase 2a — 3-sub-property fan-in: `r_i`, `r_j`, `r_k` all ⊑ functional
    /// `r_func`; Subject has ∃`r_i.A`, ∃`r_j.B`, ∃`r_k.C`; Target ≡ via
    /// ∃`r_func.(A` ⊓ B ⊓ C). The witness-merge rule must accumulate
    /// the growing conjunction across three fact arrivals.
    ///
    /// Previously ignored as a known limitation of T4's single-synthetic
    /// tracking. Fixed in T4.5 by the atom-set redesign: `merged_atom_sets`
    /// accumulates the flat set {A, B, C} incrementally; each arrival
    /// checks whether the set grew; termination is bounded by the atomic
    /// vocabulary size.
    #[test]
    fn functional_role_merge_3_sub_property_fan_in() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2a3/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2a3/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    Declaration(ObjectProperty(:r_k))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubObjectPropertyOf(:r_k :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_k :C))
    SubClassOf(ObjectSomeValuesFrom(:r_func ObjectIntersectionOf(:A :B :C)) :Target)
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let subject = internal
            .vocabulary
            .class_id("http://rustdl.test/p2a3/Subject")
            .expect("Subject declared");
        let target = internal
            .vocabulary
            .class_id("http://rustdl.test/p2a3/Target")
            .expect("Target declared");
        assert!(
            subsumers.contains(subject, target),
            "Phase 2a 3-sub-property fan-in: the witness-merge rule failed \
             to accumulate {{A, B, C}} across three sub-property facts."
        );
    }

    /// Phase 2e — witness-merge with the existential body on a SUB-role
    /// (not the functional super-role). This is the notgalen IPBP shape:
    /// `Subject` has `∃r_i.A` and `∃r_j.B` (both `r_i,r_j ⊑` functional
    /// `r_func`); `Target ≡ ∃r_i.B`. By functionality of `r_func` the two
    /// witnesses coincide, so `r_i`'s witness is `A ⊓ B` and `Subject ⊑
    /// ∃r_i.B = Target`. The pre-Phase-2e back-prop skipped the
    /// merge-triggering sub-role, so the merged synthetic never reached
    /// `r_i` when `r_i`'s fact happened to be processed second — an
    /// order-dependent miss. Mirrors `Anonymous-349 ⊑
    /// IntrinsicallyPathologicalBodyProcess` (notgalen's 18 MISSED).
    #[test]
    fn functional_role_merge_body_on_sub_role() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2e/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2e/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:Target))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    EquivalentClasses(:Subject ObjectIntersectionOf(:D ObjectSomeValuesFrom(:r_i :A)))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    EquivalentClasses(:Target ObjectIntersectionOf(ObjectSomeValuesFrom(:r_i :B) :D))
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let subject = internal
            .vocabulary
            .class_id("http://rustdl.test/p2e/Subject")
            .expect("Subject declared");
        let target = internal
            .vocabulary
            .class_id("http://rustdl.test/p2e/Target")
            .expect("Target declared");
        assert!(
            subsumers.contains(subject, target),
            "Phase 2e: witness-merge failed to propagate the merged synthetic \
             to the body's sub-role r_i (Subject ⊑ ∃r_i.B = Target)."
        );
    }

    /// Phase 2a — 4-sub-property fan-in. Approximates GALEN's denser
    /// functional roles (`StatusAttribute` has 5 sub-properties;
    /// `ProcessModifierAttribute` has 12). Confirms atom-set redesign
    /// scales beyond the 3-property case T4.5 was designed for.
    #[test]
    fn functional_role_merge_4_sub_property_fan_in() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2a4/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2a4/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    Declaration(ObjectProperty(:r_k))
    Declaration(ObjectProperty(:r_l))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubObjectPropertyOf(:r_k :r_func)
    SubObjectPropertyOf(:r_l :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_k :C))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_l :D))
    SubClassOf(ObjectSomeValuesFrom(:r_func ObjectIntersectionOf(:A :B :C :D)) :Target)
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let subject = internal
            .vocabulary
            .class_id("http://rustdl.test/p2a4/Subject")
            .expect("Subject declared");
        let target = internal
            .vocabulary
            .class_id("http://rustdl.test/p2a4/Target")
            .expect("Target declared");
        assert!(
            subsumers.contains(subject, target),
            "Phase 2a 4-sub-property fan-in: atom-set design should scale to 4 sub-properties."
        );
    }

    /// Phase 2a — chained functional super-roles: `r_i`, `r_j` ⊑ `r_func` ⊑
    /// `r_super`, both `r_func` and `r_super` functional. When (sub, `r_j`, B)
    /// arrives, funcs = `functional_supers_of(r_j)` enumerates BOTH `r_func`
    /// AND `r_super` in a single rule pass; `merged_atom_sets` is updated
    /// for both keys (sub, `r_func`) and (sub, `r_super`), and synthetics are
    /// emitted at both levels. The runtime-emitted derived facts then
    /// short-circuit on re-entry because their atom sets already match
    /// `merged_atom_sets`. Tests that the precomputed `functional_supers_of`
    /// correctly includes BOTH ancestors.
    #[test]
    fn functional_role_merge_chained_functional_supers() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2ac/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2ac/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_super))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    FunctionalObjectProperty(:r_super)
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_func :r_super)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(ObjectSomeValuesFrom(:r_super ObjectIntersectionOf(:A :B)) :Target)
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let subject = internal
            .vocabulary
            .class_id("http://rustdl.test/p2ac/Subject")
            .expect("Subject declared");
        let target = internal
            .vocabulary
            .class_id("http://rustdl.test/p2ac/Target")
            .expect("Target declared");
        assert!(
            subsumers.contains(subject, target),
            "Phase 2a chained functional supers: the witness-merge rule \
             failed to cascade from r_func to r_super; check that \
             functional_supers_of(r_func) includes r_super."
        );
    }

    /// Phase 2d canary: fact-on-subclass inheritance materializes
    /// `(A, R, T)` on `facts_by_sub[A]` when `A ⊑ B` and B has a
    /// `(B, R, T)` existential fact. Asserts both the materialized
    /// fact and the `phase2d_facts_inherited` counter.
    ///
    /// Mirrors the design's Step 4 structural assertion in
    /// `docs/phase2d-design.md` §"Code-change surface" §"Structural
    /// canary" — the minimal A⊑B + (B,R,T) → (A,R,T) inheritance.
    #[test]
    fn phase2d_fact_inherits_to_subclass() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2d/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2d/test>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:T))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A :B)
    SubClassOf(:B ObjectSomeValuesFrom(:R :T))
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("canary parses");
        let internal = convert_ontology(&set_onto).expect("canary lowers");

        // Mirror saturate() inline so we retain ownership of the engine
        // and can inspect its internal facts_by_sub + counter.
        let n = internal.vocabulary.num_classes();
        let role_super_map = build_role_super(&internal);
        let (rules, tseitin, num_total_classes) = collect_el_rules(&internal, &role_super_map);
        let role_super = freeze_role_super(&role_super_map);
        let mut engine = WorklistEngine::new(
            n,
            num_total_classes,
            rules,
            tseitin,
            role_super,
            false,
            None,
        );
        engine.seed(&internal);
        engine.run();

        let a = internal
            .vocabulary
            .class_id("http://rustdl.test/p2d/A")
            .expect("A declared");
        let b = internal
            .vocabulary
            .class_id("http://rustdl.test/p2d/B")
            .expect("B declared");

        let a_facts: Vec<ExistentialFact> = engine.facts_by_sub[a.index() as usize]
            .iter()
            .map(|&idx| engine.facts[idx])
            .collect();
        let b_facts: Vec<ExistentialFact> = engine.facts_by_sub[b.index() as usize]
            .iter()
            .map(|&idx| engine.facts[idx])
            .collect();

        assert!(
            !b_facts.is_empty(),
            "Phase 2d canary precondition: B should have at least one \
             existential fact from `B ⊑ ∃R.T`; got: {b_facts:?}"
        );
        let b_fact = b_facts[0];
        assert!(
            a_facts
                .iter()
                .any(|f| f.role == b_fact.role && f.target == b_fact.target),
            "Phase 2d should have inherited B's existential fact onto A's \
             facts_by_sub. a_facts={a_facts:?} b_facts={b_facts:?}"
        );
        assert!(
            engine.phase2d_facts_inherited > 0,
            "Phase 2d counter `phase2d_facts_inherited` should bump on \
             inheritance; got 0."
        );
    }

    /// Phase 2c-redux — structural sanity: the sub-role witness-
    /// propagation rule bumps its counter on the 4-sub-property fan-in
    /// canary. Restored from b83fcd6 (reverted at cc2019e) on top of
    /// Phase 2d.
    ///
    /// Setup: Subject has 4 existential facts on sub-roles `r_i`, `r_j`,
    /// `r_k`, `r_l` all sharing functional super `r_func`. The Phase 2c
    /// inner loop fires after Phase 2a's emission on the 2nd, 3rd, 4th
    /// fact arrivals (each grows the `merged_atom_set`, emits the
    /// merged synthetic on `r_func`, then iterates `facts_by_sub[Subject]`
    /// to propagate the synthetic onto sibling sub-roles).
    ///
    /// We assert `phase2c_sub_role_propagations > 0` after `engine.run()`.
    /// The exact count is implementation-defined (depends on dedup +
    /// iteration order); the load-bearing property is "the rule fired
    /// at least once on this clean positive shape". Note: under Phase
    /// 2d, the synthetic facts ALSO inherit to subclasses; the counter
    /// still tracks only the direct sub-role propagations.
    #[test]
    fn phase2c_sub_role_propagation_counter_bumps_on_4_fan_in() {
        let src = "\
Prefix(:=<http://rustdl.test/p2c_counter/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2c_counter/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    Declaration(ObjectProperty(:r_k))
    Declaration(ObjectProperty(:r_l))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubObjectPropertyOf(:r_k :r_func)
    SubObjectPropertyOf(:r_l :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_k :C))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_l :D))
    SubClassOf(ObjectSomeValuesFrom(:r_func ObjectIntersectionOf(:A :B :C :D)) :Target)
)
";
        let internal = parse_internal(src);

        // Mirror `saturate()` inline so we retain ownership of the
        // engine and can inspect its private counter.
        let n = internal.vocabulary.num_classes();
        let role_super_map = build_role_super(&internal);
        let (rules, tseitin, num_total_classes) = collect_el_rules(&internal, &role_super_map);
        let role_super = freeze_role_super(&role_super_map);
        let mut engine = WorklistEngine::new(
            n,
            num_total_classes,
            rules,
            tseitin,
            role_super,
            false,
            None,
        );
        engine.seed(&internal);
        engine.run();

        assert!(
            engine.phase2c_sub_role_propagations > 0,
            "Phase 2c-redux rule did not fire on the 4-sub-property \
             fan-in canary. Expected at least one (X, R_k, synthetic) \
             propagation; got 0. Either the rule was disabled, the inner \
             loop's preconditions changed, or Phase 2a's emission \
             condition (!was_first && grew) no longer triggers on this \
             shape."
        );
    }

    /// Phase 2a Task 3: verify that `collect_el_rules` builds the
    /// `functional_roles` bitset and `functional_supers_of` index correctly
    /// on a simple 4-role / 1-declared-functional / 2-sub-properties ontology.
    #[test]
    fn collect_el_rules_records_functional_roles_and_their_supers() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2a/>)
Ontology(<http://rustdl.test/p2a/funcrole>
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    Declaration(ObjectProperty(:r_unrelated))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let role_super = crate::build_role_super(&internal);
        let (rules, _tseitin, _num_total) = crate::collect_el_rules(&internal, &role_super);

        let id = |iri: &str| internal.vocabulary.role_id(iri).expect("role declared");
        let rf = id("http://rustdl.test/p2a/r_func");
        let ri = id("http://rustdl.test/p2a/r_i");
        let rj = id("http://rustdl.test/p2a/r_j");
        let ru = id("http://rustdl.test/p2a/r_unrelated");

        assert!(rules.is_functional(rf), "r_func is declared functional");
        assert!(!rules.is_functional(ri));
        assert!(!rules.is_functional(rj));
        assert!(!rules.is_functional(ru));

        let supers = |r| rules.functional_supers_of(r).to_vec();
        assert_eq!(
            supers(ri),
            vec![rf],
            "r_i ⊑ r_func and r_func is functional"
        );
        assert_eq!(supers(rj), vec![rf], "r_j ⊑ r_func");
        assert_eq!(supers(rf), vec![rf], "r_func is its own super (reflexive)");
        assert!(supers(ru).is_empty(), "r_unrelated has no functional super");
    }

    /// Phase 2b canary: minimal repro of GALEN's
    /// `KneeJointStability ⊑ JointStability` pattern (`pair_08` in the
    /// Phase 2b.0 fixture set). The axiom shape:
    ///
    ///   T ≡ A ⊓ ∃R.(B ⊓ ∃S.C)
    ///   X ≡ A ⊓ ∃R.(B ⊓ ∃S'.C')   where S' ⊑ S, C' ⊑ C
    ///
    /// Expected entailment: X ⊑ T. Derivation: X's R-witness is in
    /// (B ⊓ ∃S'.C'); via sub-property S' ⊑ S, the witness is also in
    /// ∃S.C' (CR9); via sub-class C' ⊑ C, the witness has subsumer
    /// `∃S.C` (CR5); so the witness is in B ⊓ ∃S.C = T's R-body;
    /// closing the conjunctive trigger that defines T.
    ///
    /// Phase 2b.0's analysis (docs/phase2b-galen-diagnosis.md) traced
    /// the bug to `introduce_existential_marker`'s one-way semantics
    /// being inadequate when the marker is reused inside a Tseitin
    /// synthetic that needs full equivalence. This canary ASSERTS THE
    /// FIX (Phase 2b rule active). Task 4 of Phase 2b introduced
    /// `introduce_equivalent_existential_marker` which emits both the
    /// trigger and the fact, enabling CR5/CR9 propagation through
    /// the marker.
    #[test]
    fn compound_existential_body_canary_recovers_entailment() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2b/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2b/test>
    Declaration(Class(:T))
    Declaration(Class(:X))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:C_sub))
    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:S_sub))
    SubObjectPropertyOf(:S_sub :S)
    SubClassOf(:C_sub :C)
    EquivalentClasses(:T ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S :C)))))
    EquivalentClasses(:X ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S_sub :C_sub)))))
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("canary parses");
        let internal = convert_ontology(&set_onto).expect("canary lowers");
        let subsumers = crate::saturate(&internal);
        let x = internal
            .vocabulary
            .class_id("http://rustdl.test/p2b/X")
            .expect("X declared");
        let t = internal
            .vocabulary
            .class_id("http://rustdl.test/p2b/T")
            .expect("T declared");

        assert!(
            subsumers.contains(x, t),
            "Phase 2b regression: the compound existential-body fix \
             failed to derive X ⊑ T. introduce_equivalent_existential_marker \
             likely regressed."
        );
    }

    /// Phase 2b — cluster A shape canary: paired-anatomy pattern.
    /// `Paired ≡ Body ⊓ ∃isPaired.Paired_self` style (the actual GALEN
    /// shape) — verifies the fix carries through more complex nested
    /// shapes than the simple `pair_08` single-hop case.
    #[test]
    fn compound_existential_body_cluster_a_paired_anatomy_canary() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2bA/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2bA/test>
    Declaration(Class(:Paired))
    Declaration(Class(:Body))
    Declaration(Class(:Limb))
    Declaration(Class(:Femur))
    Declaration(ObjectProperty(:isPaired))
    Declaration(ObjectProperty(:isLimbDivision))
    Declaration(ObjectProperty(:isBodyDivision))
    SubObjectPropertyOf(:isLimbDivision :isBodyDivision)
    SubClassOf(:Limb :Body)
    EquivalentClasses(:Paired ObjectIntersectionOf(:Body ObjectSomeValuesFrom(:isBodyDivision :Body)))
    SubClassOf(:Femur ObjectIntersectionOf(:Body ObjectSomeValuesFrom(:isLimbDivision :Limb)))
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let femur = internal
            .vocabulary
            .class_id("http://rustdl.test/p2bA/Femur")
            .expect("Femur declared");
        let paired = internal
            .vocabulary
            .class_id("http://rustdl.test/p2bA/Paired")
            .expect("Paired declared");

        assert!(
            subsumers.contains(femur, paired),
            "Phase 2b cluster-A canary: Femur ⊑ Paired should derive via \
             (Femur ⊑ ∃isLimbDivision.Limb) + (isLimbDivision ⊑ isBodyDivision) + (Limb ⊑ Body)."
        );
    }

    /// Phase 2b.5 canary: `SubClassOf(And(A, B), ∃R.C)` where the RHS
    /// is a non-atomic existential. This shape was the actual cause
    /// of `pair_01`'s miss (`FemoralHead` ⊑ `ExactlyPairedBodyStructure`
    /// per docs/phase2b-trace2.md). The LHS-And arm of
    /// `lower_sub_class_of` currently drops this trigger because
    /// `atomic_operands_on_right` returns [] for a non-atomic RHS.
    ///
    /// Expected entailment: Y ⊑ T via:
    ///   1. Y ⊑ A, Y ⊑ B (told subsumption)
    ///   2. A ⊓ B ⊑ ∃R.C (the failing axiom)
    ///   3. ∃R.C ⊑ T (existential trigger that consumes the witness)
    ///
    /// ASSERTS THE FIX (Phase 2b.5 active).
    #[test]
    fn lhs_and_with_existential_rhs_canary_recovers_entailment() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2b5/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2b5/test>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:T))
    Declaration(Class(:Y))
    Declaration(ObjectProperty(:R))
    SubClassOf(:Y :A)
    SubClassOf(:Y :B)
    SubClassOf(ObjectIntersectionOf(:A :B) ObjectSomeValuesFrom(:R :C))
    SubClassOf(ObjectSomeValuesFrom(:R :C) :T)
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let y = internal
            .vocabulary
            .class_id("http://rustdl.test/p2b5/Y")
            .expect("Y declared");
        let t = internal
            .vocabulary
            .class_id("http://rustdl.test/p2b5/T")
            .expect("T declared");
        // Asserts the FIX (Phase 2b.5 active). When the fix lands, this passes.
        assert!(
            subsumers.contains(y, t),
            "Phase 2b.5 regression: A ⊓ B ⊑ ∃R.C didn't lower to a conjunctive trigger; \
             the LHS-And arm of lower_sub_class_of dropped the axiom because RHS is non-atomic Some."
        );
    }

    /// Phase 2b — deeper nesting: A ⊓ ∃R.(B ⊓ ∃S.(C ⊓ ∃U.D)). Two
    /// levels of nesting, verifying the equivalent-marker fix is
    /// transitive through chains.
    #[test]
    fn compound_existential_body_deeper_nesting_canary() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use owl_dl_core::convert::convert_ontology;
        use std::io::Cursor;

        let src = "\
Prefix(:=<http://rustdl.test/p2bD/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2bD/test>
    Declaration(Class(:T))
    Declaration(Class(:X))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:D_sub))
    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:U))
    Declaration(ObjectProperty(:U_sub))
    SubObjectPropertyOf(:U_sub :U)
    SubClassOf(:D_sub :D)
    EquivalentClasses(:T ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S ObjectIntersectionOf(:C ObjectSomeValuesFrom(:U :D)))))))
    EquivalentClasses(:X ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S ObjectIntersectionOf(:C ObjectSomeValuesFrom(:U_sub :D_sub)))))))
)
";
        let mut reader = Cursor::new(src);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parses");
        let internal = convert_ontology(&set_onto).expect("lowers");
        let subsumers = crate::saturate(&internal);
        let x = internal
            .vocabulary
            .class_id("http://rustdl.test/p2bD/X")
            .expect("X declared");
        let t = internal
            .vocabulary
            .class_id("http://rustdl.test/p2bD/T")
            .expect("T declared");

        assert!(
            subsumers.contains(x, t),
            "Phase 2b deeper nesting canary: 2-level nested existential lowering should work."
        );
    }

    // -----------------------------------------------------------------------
    // Proof recording smoke tests (Track B)
    // -----------------------------------------------------------------------

    /// Smoke test §7.1 — EL chain proof.
    ///
    /// `Pizza ⊑ ∃hasTopping.Topping`
    /// `Topping ⊑ EdibleThing`
    /// `∃hasTopping.EdibleThing ⊑ FoodItem`
    /// ⟹ `Pizza ⊑ FoodItem`
    ///
    /// Assert that `prove_subsumption` returns a `ProofNode` with the root
    /// rule being `ExistentialTrigger*` (fact or sub), and that the tree
    /// has premises for `ToldFact` and `ToldSubsumer`. The faithfulness
    /// checker must pass.
    #[test]
    fn proof_recording_el_chain_pizza_food() {
        use crate::proof::{DerivedFact, ElRule, check_proof, prove_subsumption};
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Pizza))\n\
    Declaration(Class(:Topping))\n\
    Declaration(Class(:EdibleThing))\n\
    Declaration(Class(:FoodItem))\n\
    Declaration(ObjectProperty(:hasTopping))\n\
    SubClassOf(:Pizza ObjectSomeValuesFrom(:hasTopping :Topping))\n\
    SubClassOf(:Topping :EdibleThing)\n\
    SubClassOf(ObjectSomeValuesFrom(:hasTopping :EdibleThing) :FoodItem)\n\
)\n"
        ));
        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let pizza = class(&internal, "Pizza");
        let food = class(&internal, "FoodItem");
        assert!(
            subs.contains(pizza, food),
            "Pizza ⊑ FoodItem must be entailed"
        );
        let trace = maybe_trace.expect("proof trace must be Some when record_proofs=true");
        let mut memo = std::collections::HashMap::new();
        let root = prove_subsumption(&trace, pizza, food, &mut memo)
            .expect("prove_subsumption must return Some for a derived pair");
        // Root rule should be an existential trigger.
        assert!(
            matches!(
                root.rule,
                ElRule::ExistentialTriggerFact
                    | ElRule::ExistentialTriggerTarget
                    | ElRule::ExistentialTriggerSub
            ),
            "root rule should be an existential trigger, got {:?}",
            root.rule
        );
        // Faithfulness check.
        check_proof(&root, internal.axioms.len())
            .expect("proof checker must pass on the EL chain proof");
        // Check that the conclusion fact matches.
        assert_eq!(
            root.conclusion,
            DerivedFact::Sub(pizza, food),
            "root conclusion must be Sub(Pizza, FoodItem)"
        );
    }

    /// Smoke test §7.2 — Role-chain proof.
    ///
    /// Niece ⊑ ∃hasParent.Parent
    /// Parent ⊑ ∃hasBrother.Man
    /// `SubObjectPropertyOf(ObjectPropertyChain(hasParent hasBrother) hasUncle)`
    /// `∃hasUncle.Man ⊑ HasUncle`
    /// ⟹ `Niece ⊑ HasUncle`
    #[test]
    fn proof_recording_role_chain() {
        use crate::proof::{DerivedFact, check_proof, prove_subsumption};
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Niece))\n\
    Declaration(Class(:Parent))\n\
    Declaration(Class(:Man))\n\
    Declaration(Class(:HasUncle))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(ObjectProperty(:hasBrother))\n\
    Declaration(ObjectProperty(:hasUncle))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:hasParent :hasBrother) :hasUncle)\n\
    SubClassOf(:Niece ObjectSomeValuesFrom(:hasParent :Parent))\n\
    SubClassOf(:Parent ObjectSomeValuesFrom(:hasBrother :Man))\n\
    SubClassOf(ObjectSomeValuesFrom(:hasUncle :Man) :HasUncle)\n\
)\n"
        ));
        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let niece = class(&internal, "Niece");
        let has_uncle = class(&internal, "HasUncle");
        assert!(
            subs.contains(niece, has_uncle),
            "Niece ⊑ HasUncle must be entailed"
        );
        let trace = maybe_trace.expect("trace must be Some");
        let mut memo = std::collections::HashMap::new();
        let root = prove_subsumption(&trace, niece, has_uncle, &mut memo)
            .expect("prove_subsumption must return Some");
        assert_eq!(root.conclusion, DerivedFact::Sub(niece, has_uncle));
        check_proof(&root, internal.axioms.len()).expect("proof checker must pass on role-chain");
    }

    /// Verify that proof recording does NOT affect the closure: the same
    /// subsumption set is derived with and without recording on.
    #[test]
    fn proof_recording_verdicts_identical_to_baseline() {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
    SubClassOf(:B :C)\n\
    SubClassOf(ObjectSomeValuesFrom(:r :C) :D)\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        // Baseline: proof OFF.
        let baseline = saturate(&internal);
        // With proof recording ON.
        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (with_proof, _) = saturate_with_config(&internal, &cfg);
        // Both closures must be identical.
        let num = u32::try_from(internal.vocabulary.num_classes()).expect("class count fits u32");
        for i in 0..num {
            let c = ClassId::new(i);
            for j in 0..num {
                let d = ClassId::new(j);
                assert_eq!(
                    baseline.contains(c, d),
                    with_proof.contains(c, d),
                    "proof recording changed verdict for class pair ({i}, {j})"
                );
            }
        }
    }

    /// Transitivity proof should chain correctly.
    #[test]
    fn proof_recording_transitivity() {
        use crate::proof::{DerivedFact, ElRule, check_proof, prove_subsumption};
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let a = class(&internal, "A");
        let c = class(&internal, "C");
        assert!(subs.contains(a, c));
        let trace = maybe_trace.expect("trace must be Some");
        let mut memo = std::collections::HashMap::new();
        let root = prove_subsumption(&trace, a, c, &mut memo).expect("should have proof");
        assert_eq!(root.conclusion, DerivedFact::Sub(a, c));
        // Root should be transitivity.
        assert!(
            matches!(
                root.rule,
                ElRule::SubsumerTransitivityFwd | ElRule::SubsumerTransitivityBwd
            ),
            "expected transitivity rule, got {:?}",
            root.rule
        );
        check_proof(&root, internal.axioms.len()).expect("checker must pass");
    }

    /// Reflexivity: every class has `Sub(C,C)` in the trace.
    #[test]
    fn proof_recording_reflexivity_seeded() {
        use crate::proof::{DerivedFact, ElRule, prove_subsumption};
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:X))\n\
)\n"
        ));
        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (_subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let trace = maybe_trace.expect("trace must be Some");
        let x = class(&internal, "X");
        let mut memo = std::collections::HashMap::new();
        let root = prove_subsumption(&trace, x, x, &mut memo).expect("reflexivity proof");
        assert_eq!(root.conclusion, DerivedFact::Sub(x, x));
        // Should be Reflexivity or ToldSubsumer (if overridden by told).
        assert!(
            matches!(root.rule, ElRule::Reflexivity | ElRule::ToldSubsumer),
            "expected Reflexivity or ToldSubsumer, got {:?}",
            root.rule
        );
    }

    /// Verify that the proof trace is non-empty after saturation with proof recording.
    /// Also verifies that every step reachable from the proof root has a recorded rule.
    #[test]
    fn proof_recording_trace_non_empty_on_nontrivial_ontology() {
        use crate::proof::prove_subsumption;
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n\
    SubClassOf(:B :C)\n\
    SubClassOf(ObjectSomeValuesFrom(:r :C) :C)\n\
)\n"
        ));
        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let a = class(&internal, "A");
        let c = class(&internal, "C");
        assert!(subs.contains(a, c), "A ⊑ C must hold");
        let trace = maybe_trace.expect("trace must be Some");
        assert!(!trace.steps.is_empty(), "trace must be non-empty");
        let mut memo = std::collections::HashMap::new();
        let root = prove_subsumption(&trace, a, c, &mut memo).expect("proof must exist");
        // Walk the entire proof tree and assert every node has a rule.
        fn walk(node: &crate::proof::ProofNode) {
            // Rule is always present (it's a non-Option field).
            for premise in &node.premises {
                walk(premise);
            }
        }
        walk(&root);
    }

    /// Faithfulness corpus test: run proof recording on pizza + go-basic (if
    /// available), then extract and `check_proof` on a sample of derived
    /// subsumptions.  Every proof must pass the checker AND bottom out at
    /// genuine axiom leaves (no truncation, no misaligned axiom-ref).
    ///
    /// Intentionally marked `#[ignore]` so it only runs on demand
    /// (`cargo test proof_faithfulness_corpus -- --ignored`) or in CI
    /// via the explicit `--include-ignored` flag.
    ///
    /// Fail criteria:
    /// - Any `check_proof` call returns `Err`.
    /// - Any ToldSubsumer/ToldFact/ToldUnsat leaf has an empty `axiom_refs`
    ///   (these should always cite the source axiom — axiom-ref absent = coarse;
    ///   we report but do not hard-fail on pure coarse).
    #[test]
    #[ignore = "requires real ontology files; run with --include-ignored"]
    fn proof_faithfulness_corpus_pizza() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use std::fs::File;
        use std::io::BufReader;

        use crate::proof::{ElRule, check_proof_with_content, prove_subsumption};

        // Resolve path relative to the workspace root (two levels up from this crate).
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest_dir}/../../ontologies/real/pizza.ofn");
        if !std::path::Path::new(&path).exists() {
            eprintln!("# SKIP: {path} not found");
            return;
        }
        let f = File::open(&path).expect("open pizza.ofn");
        let mut reader = BufReader::new(f);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parse pizza.ofn");
        let internal =
            owl_dl_core::convert::convert_ontology(&set_onto).expect("convert pizza.ofn");

        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let trace = maybe_trace.expect("trace must be Some");

        let num_classes = u32::try_from(internal.vocabulary.num_classes()).expect("fits");

        let mut total = 0usize;
        let mut pass = 0usize;
        let mut coarse_leaf = 0usize;
        let mut failures: Vec<String> = Vec::new();

        // Sample up to 500 derived subsumptions (skip trivial C⊑C).
        // Uses check_proof_with_content which validates axiom-ref content (not just range).
        'outer: for i in 0..num_classes {
            let sub = ClassId::new(i);
            for j in 0..num_classes {
                if i == j {
                    continue;
                }
                let sup = ClassId::new(j);
                if !subs.contains(sub, sup) {
                    continue;
                }
                let mut memo = std::collections::HashMap::new();
                let Some(root) = prove_subsumption(&trace, sub, sup, &mut memo) else {
                    continue;
                };
                total += 1;
                match check_proof_with_content(&root, &internal) {
                    Ok(()) => {
                        // Check for coarse leaves (axiom-ref absent on ToldSubsumer/ToldFact/ToldUnsat).
                        fn count_coarse(node: &crate::proof::ProofNode, count: &mut usize) {
                            match node.rule {
                                ElRule::ToldSubsumer | ElRule::ToldFact | ElRule::ToldUnsat
                                    if node.axiom_refs.is_empty() =>
                                {
                                    *count += 1;
                                }
                                _ => {}
                            }
                            for p in &node.premises {
                                count_coarse(p, count);
                            }
                        }
                        let mut coarse = 0;
                        count_coarse(&root, &mut coarse);
                        coarse_leaf += coarse;
                        pass += 1;
                    }
                    Err(e) => {
                        failures.push(format!("({i},{j}): {e}"));
                        if failures.len() >= 10 {
                            break 'outer;
                        }
                    }
                }
                if total >= 500 {
                    break 'outer;
                }
            }
        }

        eprintln!(
            "# corpus pizza: {pass}/{total} pass, {coarse_leaf} coarse leaves, {} failures",
            failures.len()
        );
        for f in &failures {
            eprintln!("  FAIL: {f}");
        }
        assert!(
            failures.is_empty(),
            "Faithfulness failures on pizza corpus:\n{failures:#?}"
        );
    }

    /// Faithfulness corpus test on go-basic (large EL ontology with role chains).
    /// Same semantics as `proof_faithfulness_corpus_pizza` but samples a broader
    /// rule set (role hierarchy, chain, transitivity).
    #[test]
    #[ignore = "requires real ontology files; run with --include-ignored"]
    fn proof_faithfulness_corpus_go_basic() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use std::fs::File;
        use std::io::BufReader;

        use crate::proof::{ElRule, check_proof_with_content, prove_subsumption};

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest_dir}/../../ontologies/real/go-basic.ofn");
        if !std::path::Path::new(&path).exists() {
            eprintln!("# SKIP: {path} not found");
            return;
        }
        let f = File::open(&path).expect("open go-basic.ofn");
        let mut reader = BufReader::new(f);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parse go-basic.ofn");
        let internal =
            owl_dl_core::convert::convert_ontology(&set_onto).expect("convert go-basic.ofn");

        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let trace = maybe_trace.expect("trace must be Some");

        let num_classes = u32::try_from(internal.vocabulary.num_classes()).expect("fits");

        let mut total = 0usize;
        let mut pass = 0usize;
        let mut coarse_leaf = 0usize;
        let mut failures: Vec<String> = Vec::new();

        // Sample 2000 derived non-trivial subsumptions.
        // Uses check_proof_with_content which validates axiom-ref content (not just range).
        'outer: for i in 0..num_classes {
            let sub = ClassId::new(i);
            for j in 0..num_classes {
                if i == j {
                    continue;
                }
                let sup = ClassId::new(j);
                if !subs.contains(sub, sup) {
                    continue;
                }
                let mut memo = std::collections::HashMap::new();
                let Some(root) = prove_subsumption(&trace, sub, sup, &mut memo) else {
                    continue;
                };
                total += 1;
                match check_proof_with_content(&root, &internal) {
                    Ok(()) => {
                        fn count_coarse(node: &crate::proof::ProofNode, count: &mut usize) {
                            match node.rule {
                                ElRule::ToldSubsumer | ElRule::ToldFact | ElRule::ToldUnsat
                                    if node.axiom_refs.is_empty() =>
                                {
                                    *count += 1;
                                }
                                _ => {}
                            }
                            for p in &node.premises {
                                count_coarse(p, count);
                            }
                        }
                        let mut coarse = 0;
                        count_coarse(&root, &mut coarse);
                        coarse_leaf += coarse;
                        pass += 1;
                    }
                    Err(e) => {
                        failures.push(format!("({i},{j}): {e}"));
                        if failures.len() >= 10 {
                            break 'outer;
                        }
                    }
                }
                if total >= 2000 {
                    break 'outer;
                }
            }
        }

        eprintln!(
            "# corpus go-basic: {pass}/{total} pass, {coarse_leaf} coarse leaves, {} failures",
            failures.len()
        );
        for f in &failures {
            eprintln!("  FAIL: {f}");
        }
        assert!(
            failures.is_empty(),
            "Faithfulness failures on go-basic corpus:\n{failures:#?}"
        );
    }

    /// Faithfulness corpus test on galen (large EL+functional ontology — exercises
    /// Phase-2d `FactInheritance` and `FunctionalMerge`/`FunctionalMergeSubRole`).
    /// Uses `check_proof_with_content` (axiom-ref content validation, not just range).
    #[test]
    #[ignore = "requires real ontology files; run with --include-ignored"]
    fn proof_faithfulness_corpus_galen() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use std::fs::File;
        use std::io::BufReader;

        use crate::proof::{ElRule, check_proof_with_content, prove_subsumption};

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest_dir}/../../ontologies/external/galen.ofn");
        if !std::path::Path::new(&path).exists() {
            eprintln!("# SKIP: {path} not found");
            return;
        }
        let f = File::open(&path).expect("open galen.ofn");
        let mut reader = BufReader::new(f);
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parse galen.ofn");
        let internal =
            owl_dl_core::convert::convert_ontology(&set_onto).expect("convert galen.ofn");

        let cfg = SaturateConfig {
            record_proofs: true,
        };
        let (subs, maybe_trace) = saturate_with_config(&internal, &cfg);
        let trace = maybe_trace.expect("trace must be Some");

        let num_classes = u32::try_from(internal.vocabulary.num_classes()).expect("fits");

        let mut total = 0usize;
        let mut pass = 0usize;
        let mut coarse_leaf = 0usize;
        let mut failures: Vec<String> = Vec::new();
        let mut fact_inheritance_seen = 0usize;
        let mut functional_merge_seen = 0usize;

        // Sample 2000 derived non-trivial subsumptions from galen.
        // Galen is EL+functional — exercises Phase-2d and FunctionalMerge.
        'outer: for i in 0..num_classes {
            let sub = ClassId::new(i);
            for j in 0..num_classes {
                if i == j {
                    continue;
                }
                let sup = ClassId::new(j);
                if !subs.contains(sub, sup) {
                    continue;
                }
                let mut memo = std::collections::HashMap::new();
                let Some(root) = prove_subsumption(&trace, sub, sup, &mut memo) else {
                    continue;
                };
                total += 1;
                // Count rule types for diagnostic.
                fn count_rules(
                    node: &crate::proof::ProofNode,
                    fi: &mut usize,
                    fm: &mut usize,
                    coarse: &mut usize,
                ) {
                    match node.rule {
                        ElRule::FactInheritance => *fi += 1,
                        ElRule::FunctionalMerge | ElRule::FunctionalMergeSubRole => *fm += 1,
                        ElRule::ToldSubsumer | ElRule::ToldFact | ElRule::ToldUnsat
                            if node.axiom_refs.is_empty() =>
                        {
                            *coarse += 1;
                        }
                        _ => {}
                    }
                    for p in &node.premises {
                        count_rules(p, fi, fm, coarse);
                    }
                }
                let mut fi = 0;
                let mut fm = 0;
                let mut c = 0;
                count_rules(&root, &mut fi, &mut fm, &mut c);
                fact_inheritance_seen += fi;
                functional_merge_seen += fm;
                coarse_leaf += c;

                match check_proof_with_content(&root, &internal) {
                    Ok(()) => {
                        pass += 1;
                    }
                    Err(e) => {
                        failures.push(format!("({i},{j}): {e}"));
                        if failures.len() >= 10 {
                            break 'outer;
                        }
                    }
                }
                if total >= 2000 {
                    break 'outer;
                }
            }
        }

        eprintln!(
            "# corpus galen: {pass}/{total} pass, {coarse_leaf} coarse leaves, \
             {fact_inheritance_seen} FactInheritance steps, \
             {functional_merge_seen} FunctionalMerge steps, \
             {} failures",
            failures.len()
        );
        for f in &failures {
            eprintln!("  FAIL: {f}");
        }
        assert!(
            failures.is_empty(),
            "Faithfulness failures on galen corpus:\n{failures:#?}"
        );
    }
}
