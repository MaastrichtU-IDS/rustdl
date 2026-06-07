//! Consequence-based saturation engine for the EL fragment.
//!
//! Algorithm follows Kazakov, Krötzsch, Simančík (JAR 2014) "The Incredible
//! ELK". The Rust crate `whelk-rs` is the working reference implementation;
//! we re-implement against our own IR (see `owl-dl-core`) to avoid IR-boundary
//! copies in the hot loop.
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

use std::collections::{HashMap, HashSet, VecDeque};

use fixedbitset::FixedBitSet;
use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, InternalOntology, Role,
    RoleId, SubRolePath,
};

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
    let n = internal.vocabulary.num_classes();
    let role_super = build_role_super(internal);
    let (rules, tseitin, num_total_classes) = collect_el_rules(internal, &role_super);
    let mut engine = WorklistEngine::new(n, num_total_classes, rules, tseitin, role_super);
    engine.seed();
    engine.run();
    engine.subsumers
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
    role_super: HashMap<RoleId, HashSet<RoleId>>,
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
}

impl WorklistEngine {
    fn new(
        num_user_classes: usize,
        num_total_classes: usize,
        rules: ElRules,
        tseitin: TseitinAllocator,
        role_super: HashMap<RoleId, HashSet<RoleId>>,
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
        }
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
    fn seed(&mut self) {
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
        // Told atomic subsumers.
        for rule in &self.rules.atomic_subsumptions {
            self.todo_subsumer.push_back((rule.sub, rule.sup));
        }
        // Told existential facts (snapshot first to release the
        // borrow into `self.rules`).
        let told: Vec<ExistentialFact> = self.rules.existential_facts.clone();
        for fact in told {
            self.push_fact(fact);
        }
        // Phase D4: classes told directly to be unsatisfiable via
        // `SubClassOf(Atomic, Bot)` (data-axiom preprocessing clash
        // emission). enqueue_unsat queues them; process_unsat
        // propagates to subclasses + fact targets via the standard
        // rules.
        let directly_unsat: Vec<ClassId> = self.rules.directly_unsat.clone();
        for c in directly_unsat {
            self.enqueue_unsat(c);
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
        if !self.seen_facts.insert((fact.sub, fact.role, fact.target)) {
            return None;
        }
        let idx = self.facts.len();
        self.facts.push(fact);
        self.facts_by_sub[fact.sub.index() as usize].push(idx);
        self.facts_by_target[fact.target.index() as usize].push(idx);
        self.todo_fact.push_back(idx);
        // Phase 2d: propagate to every subclass of fact.sub.
        // subs_of_class returns an owned Vec, so no borrow conflict
        // with the recursive push_fact mutable borrow.
        let subs = self.subs_of_class(fact.sub);
        for c in subs {
            if c == fact.sub {
                continue;
            }
            let inherited = ExistentialFact {
                sub: c,
                role: fact.role,
                target: fact.target,
            };
            if self.push_fact(inherited).is_some() {
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
        // Transitivity (forward): anything D ⊑ is also a subsumer
        // of C.
        let supers_of_d = self.supers_of_class(d);
        for e in supers_of_d {
            self.enqueue_subsumer(c, e);
        }
        // Transitivity (backward): anything that had C as a
        // subsumer now also has D as a subsumer.
        let subs_of_c = self.subs_of_class(c);
        for x in subs_of_c {
            self.enqueue_subsumer(x, d);
        }
        // Unsat propagation: if D is unsat, C is unsat too.
        if self.subsumers.is_unsatisfiable(d) {
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
                    self.enqueue_subsumer(c, head);
                }
            }
        }
        // Disjointness: if any class disjoint from D is already a
        // subsumer of C, C is unsat.
        if let Some(others) = Some(self.disjoints_by_class[d.index() as usize].clone())
            && others
                .iter()
                .any(|other| self.subsumers.contains(c, *other))
        {
            self.enqueue_unsat(c);
        }
        // Existential trigger firing — target side: for facts whose
        // target is C, a new subsumer D may match a trigger body.
        if let Some(fact_idxs) = Some(self.facts_by_target[c.index() as usize].clone())
            && let Some(trigger_idxs) =
                Some(self.existential_triggers_by_body[d.index() as usize].clone())
        {
            for fidx in fact_idxs {
                let fact = self.facts[fidx];
                let fact_role_supers = supers_of(&self.role_super, fact.role);
                for tidx in &trigger_idxs {
                    let trigger = self.rules.existential_triggers[*tidx];
                    if !fact_role_supers.contains(&trigger.role) {
                        continue;
                    }
                    // Every Y with fact.sub ∈ subsumers(Y) gains
                    // trigger.head — walk subsumed_by.
                    let head = trigger.head;
                    let candidates = self.subs_of_class(fact.sub);
                    for y in candidates {
                        self.enqueue_subsumer(y, head);
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
                let fact_role_supers = supers_of(&self.role_super, fact.role);
                for sub in target_subsumers {
                    if let Some(trigger_idxs) =
                        Some(self.existential_triggers_by_body[sub.index() as usize].clone())
                    {
                        for tidx in trigger_idxs {
                            let trigger = self.rules.existential_triggers[tidx];
                            if !fact_role_supers.contains(&trigger.role) {
                                continue;
                            }
                            self.enqueue_subsumer(c, trigger.head);
                        }
                    }
                }
                // Domain axiom: if there's a domain for any super
                // of fact.role, C now gets that domain.
                for super_role in &fact_role_supers {
                    let doms: Vec<ClassId> = self
                        .rules
                        .role_domains
                        .get(super_role)
                        .cloned()
                        .unwrap_or_default();
                    for dom in doms {
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
            if self.push_fact(inherited).is_some() {
                self.phase2d_facts_inherited += 1;
            }
        }
        // Chain rule — `c` is fact1.target side: when a new subsumer
        // `d` lands on `c`, for every fact1 = (A, r1', c) with the
        // chain's r1 in r1's super-roles, and every fact2 = (d, r2',
        // T) whose sub is the new subsumer `d`, derive (A, sup, T)
        // when the chain matches.
        let chain_axioms = self.rules.chain_axioms.clone();
        if !chain_axioms.is_empty() {
            let head_facts: Vec<ExistentialFact> = self.facts_by_target[c.index() as usize]
                .iter()
                .map(|&i| self.facts[i])
                .collect();
            let tail_facts: Vec<ExistentialFact> = self.facts_by_sub[d.index() as usize]
                .iter()
                .map(|&i| self.facts[i])
                .collect();
            for (r1, r2, sup) in chain_axioms {
                for head in &head_facts {
                    if !supers_of(&self.role_super, head.role).contains(&r1) {
                        continue;
                    }
                    for tail in &tail_facts {
                        if !supers_of(&self.role_super, tail.role).contains(&r2) {
                            continue;
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
                self.push_fact(ExistentialFact {
                    sub: fact.sub,
                    role: fact.role,
                    target: b_key,
                });
            }
        }
        let role_supers = supers_of(&self.role_super, fact.role);
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
        for super_role in &role_supers {
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
                    self.enqueue_subsumer(fact.sub, dom);
                    for y in &candidates {
                        self.enqueue_subsumer(*y, dom);
                    }
                }
            }
        }
        // Unsat propagation: if the target is unsat, the source is
        // unsat (an A-instance would need an r-successor in an
        // empty class).
        if self.subsumers.is_unsatisfiable(fact.target) {
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
                    if !role_supers.contains(&trigger.role) {
                        continue;
                    }
                    let head = trigger.head;
                    self.enqueue_subsumer(fact.sub, head);
                    for y in &candidates {
                        self.enqueue_subsumer(*y, head);
                    }
                }
            }
        }
        // Chain rule: pair with existing facts.
        let chain_axioms = self.rules.chain_axioms.clone();
        for (r1, r2, sup) in chain_axioms {
            let role_in_r1 = role_supers.contains(&r1);
            let role_in_r2 = role_supers.contains(&r2);
            if role_in_r1 {
                // This fact is the head; pair with tails whose sub
                // is a subsumer of fact.target.
                let target_subs = target_subsumers.clone();
                for sub in &target_subs {
                    let tail_idxs = self.facts_by_sub[sub.index() as usize].clone();
                    for tidx in tail_idxs {
                        let tail = self.facts[tidx];
                        if supers_of(&self.role_super, tail.role).contains(&r2) {
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
                        let head = self.facts[hidx];
                        if supers_of(&self.role_super, head.role).contains(&r1) {
                            self.push_fact(ExistentialFact {
                                sub: head.sub,
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
        let funcs = self.rules.functional_supers_of(fact.role).to_vec();
        if !funcs.is_empty() {
            let new_atoms = self.atomic_content_of_or_self(fact.target);
            for rf in funcs {
                let key = (fact.sub, rf);
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
                    if !self.rules.functional_supers_of(other.role).contains(&rf) {
                        continue;
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
            self.enqueue_unsat(d);
        }
        // Every fact with c as its target makes its source unsat.
        if let Some(fact_idxs) = Some(self.facts_by_target[c.index() as usize].clone()) {
            for fidx in fact_idxs {
                let fact = self.facts[fidx];
                self.enqueue_unsat(fact.sub);
            }
        }
    }
}

/// Look up the reflexive + transitive super-role closure for `r`,
/// falling back to `[r]` if the closure has no entry.
fn supers_of(role_super: &HashMap<RoleId, HashSet<RoleId>>, r: RoleId) -> Vec<RoleId> {
    role_super
        .get(&r)
        .map_or_else(|| vec![r], |set| set.iter().copied().collect())
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

#[derive(Debug, Default)]
struct ElRules {
    /// Direct named-to-named `A ⊑ B` facts.
    atomic_subsumptions: Vec<AtomicSubsumption>,
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
#[derive(Debug)]
struct TseitinAllocator {
    next_id: u32,
    by_body: HashMap<Vec<ClassId>, ClassId>,
    /// Cache for existential markers used to lower LHS conjunctions
    /// containing existential operands (e.g. `∃R.B ⊓ A ⊑ C`). Keyed
    /// by `(role, body_class_id)` so repeated occurrences of the same
    /// `∃R.B` shape across different conjunctions share one marker.
    by_existential: HashMap<(RoleId, ClassId), ClassId>,
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
}

impl TseitinAllocator {
    fn new(num_original_classes: usize) -> Self {
        Self {
            next_id: u32::try_from(num_original_classes).expect("class count fits in u32"),
            by_body: HashMap::new(),
            by_existential: HashMap::new(),
            nominal_by_ind: HashMap::new(),
            max_key_by_role: HashMap::new(),
            forall_key_by_role: HashMap::new(),
        }
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
    build_abox_nominal_reach(internal, &mut tseitin, &mut rules);

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
                        // Allocate one marker for this existential
                        // operand. If the body is `Or(C1, ..., Cn)`
                        // we emit one trigger `∃R.Ci ⊑ marker` per
                        // operand, all sharing the marker so any
                        // operand satisfies it.
                        let marker = if body_ids.len() == 1 {
                            tseitin.introduce_existential_marker(role.role_id(), body_ids[0], rules)
                        } else {
                            let primary = tseitin.introduce_existential_marker(
                                role.role_id(),
                                body_ids[0],
                                rules,
                            );
                            // Reuse the same marker for the alternative
                            // bodies — emit additional triggers tying
                            // each alternative ∃R.Cj to the same marker.
                            for &alt_body in &body_ids[1..] {
                                rules.existential_triggers.push(ExistentialTrigger {
                                    role: role.role_id(),
                                    body: alt_body,
                                    head: primary,
                                });
                            }
                            primary
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
    let mut closure: HashMap<RoleId, HashSet<RoleId>> = HashMap::with_capacity(num_roles);
    for i in 0..num_roles {
        let id = RoleId::new(u32::try_from(i).expect("role count fits in u32"));
        closure.entry(id).or_default().insert(id);
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
                    closure.entry(a).or_default().insert(b);
                }
            }
            Axiom::EquivalentObjectProperties(members) => {
                let named: Vec<RoleId> = members.iter().filter_map(edge).collect();
                for a in &named {
                    for b in &named {
                        if a != b {
                            closure.entry(*a).or_default().insert(*b);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Transitive closure (Warshall-style, small Vec-of-ids domain).
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot: Vec<(RoleId, Vec<RoleId>)> = closure
            .iter()
            .map(|(k, v)| (*k, v.iter().copied().collect()))
            .collect();
        for (a, supers) in snapshot {
            let to_add: Vec<RoleId> = supers
                .iter()
                .flat_map(|s| {
                    closure
                        .get(s)
                        .into_iter()
                        .flat_map(|set| set.iter().copied())
                })
                .collect();
            let entry = closure.entry(a).or_default();
            for s in to_add {
                if entry.insert(s) {
                    changed = true;
                }
            }
        }
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
        let role_super = build_role_super(&internal);
        let (rules, tseitin, num_total_classes) = collect_el_rules(&internal, &role_super);
        let mut engine = WorklistEngine::new(n, num_total_classes, rules, tseitin, role_super);
        engine.seed();
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
        let role_super = build_role_super(&internal);
        let (rules, tseitin, num_total_classes) = collect_el_rules(&internal, &role_super);
        let mut engine = WorklistEngine::new(n, num_total_classes, rules, tseitin, role_super);
        engine.seed();
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
}
