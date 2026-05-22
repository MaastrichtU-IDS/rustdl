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
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, InternalOntology, Role, RoleId,
    SubRolePath,
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
    let (rules, num_total_classes) = collect_el_rules(internal);
    let role_super = build_role_super(internal);
    let mut engine = WorklistEngine::new(n, num_total_classes, rules, role_super);
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
}

impl WorklistEngine {
    fn new(
        num_user_classes: usize,
        num_total_classes: usize,
        rules: ElRules,
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
    fn push_fact(&mut self, fact: ExistentialFact) -> Option<usize> {
        if !self.seen_facts.insert((fact.sub, fact.role, fact.target)) {
            return None;
        }
        let idx = self.facts.len();
        self.facts.push(fact);
        self.facts_by_sub[fact.sub.index() as usize].push(idx);
        self.facts_by_target[fact.target.index() as usize].push(idx);
        self.todo_fact.push_back(idx);
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
    fn process_fact(&mut self, idx: usize) {
        let fact = self.facts[idx];
        let role_supers = supers_of(&self.role_super, fact.role);
        // Range axiom: target gains the range for every super-role.
        for super_role in &role_supers {
            let ranges: Vec<ClassId> = self
                .rules
                .role_ranges
                .get(super_role)
                .cloned()
                .unwrap_or_default();
            for rng in ranges {
                self.enqueue_subsumer(fact.target, rng);
            }
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
}

impl TseitinAllocator {
    fn new(num_original_classes: usize) -> Self {
        Self {
            next_id: u32::try_from(num_original_classes).expect("class count fits in u32"),
            by_body: HashMap::new(),
        }
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
}

fn collect_el_rules(internal: &InternalOntology) -> (ElRules, usize) {
    let mut rules = ElRules::default();
    let mut tseitin = TseitinAllocator::new(internal.vocabulary.num_classes());
    for ax in &internal.axioms {
        match ax {
            Axiom::SubClassOf { sub, sup } => {
                lower_sub_class_of(*sub, *sup, &internal.concepts, &mut rules, &mut tseitin);
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
                            );
                        }
                    }
                }
            }
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
    let total_classes = tseitin.next_id as usize;
    (rules, total_classes)
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
) {
    match pool.get(sub) {
        ConceptExpr::Atomic(sub_id) => {
            for atomic_sup in atomic_operands_on_right(sup, pool) {
                rules.atomic_subsumptions.push(AtomicSubsumption {
                    sub: *sub_id,
                    sup: atomic_sup,
                });
            }
            // Atomic ⊑ ∃r.Y: existential fact. Tseitin introduces a
            // synthetic atomic if the body is a compound And.
            if let Some((role, target)) = atomic_existential(sup, pool, rules, tseitin) {
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
                    if let Some((role, target)) = atomic_existential(*op, pool, rules, tseitin) {
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
            let Some(bodies) = atomic_classes(operands, pool) else {
                return;
            };
            for head in atomic_operands_on_right(sup, pool) {
                rules.conjunctive_triggers.push(ConjunctiveTrigger {
                    bodies: bodies.clone(),
                    head,
                });
            }
        }
        ConceptExpr::Some(role, body) => {
            // ∃r.B ⊑ C: existential trigger. Named role only; the
            // body may be atomic or an `And` of atomics, in which
            // case Tseitin introduces a synthetic atomic stand-in.
            if role.is_inverse() {
                return;
            }
            let Some(body_id) = atomic_or_tseitin_body(*body, pool, rules, tseitin) else {
                return;
            };
            for head in atomic_operands_on_right(sup, pool) {
                rules.existential_triggers.push(ExistentialTrigger {
                    role: role.role_id(),
                    body: body_id,
                    head,
                });
            }
        }
        _ => {}
    }
}

/// Extract `(role_id, target_class_id)` from `∃<named-role>.<body>`
/// where `body` is either an atomic class or an `And` of atomics
/// (Tseitin introduces a synthetic atomic stand-in in the latter
/// case). Returns `None` for inverse roles or any other shape.
fn atomic_existential(
    c: ConceptId,
    pool: &ConceptPool,
    rules: &mut ElRules,
    tseitin: &mut TseitinAllocator,
) -> Option<(RoleId, ClassId)> {
    let ConceptExpr::Some(role, body) = pool.get(c) else {
        return None;
    };
    if role.is_inverse() {
        return None;
    }
    let body_id = atomic_or_tseitin_body(*body, pool, rules, tseitin)?;
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
    match pool.get(body) {
        ConceptExpr::Atomic(id) => Some(*id),
        ConceptExpr::And(operands) => {
            let atomics = atomic_classes(operands, pool)?;
            Some(tseitin.introduce(atomics, rules))
        }
        _ => None,
    }
}

fn atomic_classes(ids: &[ConceptId], pool: &ConceptPool) -> Option<Vec<ClassId>> {
    let mut out = Vec::with_capacity(ids.len());
    for &c in ids {
        match pool.get(c) {
            ConceptExpr::Atomic(id) => out.push(*id),
            _ => return None,
        }
    }
    Some(out)
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
    fn property_range_propagates_to_targets() {
        // ObjectPropertyRange(hasOwner, Person); Pet ⊑ ∃hasOwner.Dog
        // ⇒ Dog ⊑ Person (every hasOwner-target is a Person — and
        // Dog appears as such a target via Pet's existential).
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
        assert!(subs.contains(class(&internal, "Dog"), class(&internal, "Person")));
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
}
