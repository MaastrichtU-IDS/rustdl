//! Consequence-based saturation engine for the EL fragment.
//!
//! Algorithm follows Kazakov, Krötzsch, Simančík (JAR 2014) "The Incredible
//! ELK". The Rust crate `whelk-rs` is the working reference implementation;
//! we re-implement against our own IR (see `owl-dl-core`) to avoid IR-boundary
//! copies in the hot loop.
//!
//! ## Phase 6 scaffold — what this commit covers
//!
//! A minimal saturation closure over the *atomic*-class subset of the
//! input ontology:
//!
//! - Atomic `SubClassOf(A, B)` is told-subsumption fact.
//! - `SubClassOf(A, ObjectIntersectionOf([B₁, …, Bₙ]))` distributes
//!   to `A ⊑ Bᵢ` for each atomic `Bᵢ`.
//! - `SubClassOf(ObjectIntersectionOf([B₁, …, Bₙ]), C)` triggers a
//!   conjunctive subsumption: any class that has all `Bᵢ` as
//!   subsumers also has `C`.
//! - `EquivalentClasses(A₁, …, Aₙ)` decomposes pairwise.
//! - Closure is taken under transitivity.
//!
//! Out of scope for this scaffold:
//! - `ObjectSomeValuesFrom` propagation (Kazakov's CR5–CR8).
//! - Role hierarchies and role chains (CR9–CR11).
//! - The non-EL parts: union, complement, cardinality, nominals.
//!
//! When the engine sees an axiom outside the supported fragment, it
//! silently drops it; the orchestrator (separate commit) will fall
//! back to the tableau for queries that depend on those axioms.

use std::collections::{HashMap, HashSet};

use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, InternalOntology, Role, RoleId,
    SubRolePath,
};

/// Compute the subsumer closure over the EL-fragment subset of
/// `internal`. The result maps every declared `ClassId` to the set
/// of named classes that subsume it (including itself).
#[must_use]
pub fn saturate(internal: &InternalOntology) -> Subsumers {
    let n = internal.vocabulary.num_classes();
    let mut subsumers = Subsumers::with_capacity(n);
    for i in 0..n {
        let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
        subsumers.add(id, id);
    }
    let rules = collect_el_rules(internal);
    let role_super = build_role_super(internal);
    // Existential facts grow over time: chain propagation derives new
    // (A, sup, C) entries that further chain/trigger steps consume.
    // Seed from told axioms; dedup via the (sub, role, target) key.
    let mut facts = ExistentialFacts::default();
    for fact in &rules.existential_facts {
        facts.add(*fact);
    }
    let mut changed = true;
    // Did the subsumer table grow during the previous outer-loop
    // iteration? Used by `apply_role_chains` to decide whether its
    // delta optimisation is safe: when subsumers grow, previously-
    // failing chain conditions can newly hold, so we must re-scan
    // every (i, j) pair.
    let mut subsumers_grew_last_round = true;
    while changed {
        let subsumer_size_before = subsumers_total_entries(&subsumers);
        changed = false;
        changed |= apply_atomic_subsumptions(&mut subsumers, &rules);
        changed |= apply_conjunctive_triggers(&mut subsumers, &rules);
        changed |= apply_existential_propagation(&mut subsumers, &facts, &rules, &role_super);
        changed |= apply_role_chains(
            &mut facts,
            &subsumers,
            &rules,
            &role_super,
            subsumers_grew_last_round,
        );
        changed |= apply_domain_and_range(&mut subsumers, &facts, &rules, &role_super);
        changed |= apply_disjointness(&mut subsumers, &rules);
        changed |= apply_transitivity(&mut subsumers);
        subsumers_grew_last_round = subsumers_total_entries(&subsumers) > subsumer_size_before;
    }
    subsumers
}

/// Total number of `(C, D)` pairs currently in the subsumer table
/// (including the `unsatisfiable` set). Used by `saturate` to detect
/// whether the previous round grew subsumers, so the chain rule
/// knows whether its delta-only path is safe.
fn subsumers_total_entries(subsumers: &Subsumers) -> usize {
    subsumers.table.values().map(HashSet::len).sum::<usize>() + subsumers.unsatisfiable.len()
}

/// Dedup-aware growable store of existential facts.
#[derive(Debug, Default)]
struct ExistentialFacts {
    list: Vec<ExistentialFact>,
    seen: HashSet<(ClassId, RoleId, ClassId)>,
    /// Inverted index: `by_sub[c]` is the set of `list` indices whose
    /// `sub` is `c`. Lets the chain rule jump directly to candidate
    /// tail facts from a head fact's target subsumer set, instead of
    /// scanning the whole `list`.
    by_sub: HashMap<ClassId, Vec<usize>>,
    /// Frontier for the chain rule's delta optimisation: facts at
    /// `list[..chained_through]` have already been paired against
    /// everything they could chain with under the current subsumer
    /// state. When subsumers grow, this is reset to 0.
    chained_through: usize,
}

impl ExistentialFacts {
    fn add(&mut self, fact: ExistentialFact) -> bool {
        if self.seen.insert((fact.sub, fact.role, fact.target)) {
            let idx = self.list.len();
            self.list.push(fact);
            self.by_sub.entry(fact.sub).or_default().push(idx);
            true
        } else {
            false
        }
    }
}

/// Direct told subsumers: `A ⊑ B`.
fn apply_atomic_subsumptions(subsumers: &mut Subsumers, rules: &ElRules) -> bool {
    let mut changed = false;
    for rule in &rules.atomic_subsumptions {
        if subsumers.add(rule.sub, rule.sup) {
            changed = true;
        }
    }
    changed
}

/// Conjunctive triggers: if `X` has every `bᵢ` among its subsumers,
/// it gains the trigger's `head`.
fn apply_conjunctive_triggers(subsumers: &mut Subsumers, rules: &ElRules) -> bool {
    let mut changed = false;
    let len = subsumers.table.len();
    for trigger in &rules.conjunctive_triggers {
        for i in 0..len {
            let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
            if trigger.bodies.iter().all(|b| subsumers.contains(id, *b))
                && subsumers.add(id, trigger.head)
            {
                changed = true;
            }
        }
    }
    changed
}

/// CR5: existential propagation. For every fact `(A, r, Y)` —
/// meaning `A ⊑ ∃r.Y` — and every trigger `(r', Z, W)` with `r ⊑ r'`,
/// if `Z` is already a subsumer of `Y`, every class that has `A`
/// among its subsumers also gains `W`.
///
/// Reads from the runtime fact set (told + chain-derived) so further
/// chain steps participate naturally.
fn apply_existential_propagation(
    subsumers: &mut Subsumers,
    facts: &ExistentialFacts,
    rules: &ElRules,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
) -> bool {
    let mut changed = false;
    for fact in &facts.list {
        let supers = supers_of(role_super, fact.role);
        for trigger in &rules.existential_triggers {
            if !supers.contains(&trigger.role) {
                continue;
            }
            if !subsumers.contains(fact.target, trigger.body) {
                continue;
            }
            let candidates = classes_with_subsumer(subsumers, fact.sub);
            for x in candidates {
                if subsumers.add(x, trigger.head) {
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Role chain propagation (Kazakov CR11 — length-2 form). For each
/// registered chain axiom `r₁ ∘ r₂ ⊑ sup`, the *implied* edge
/// `A —sup→ C` whenever `A —r₁→ B` and `B —r₂→ C` chain through.
///
/// We don't materialise derived `ExistentialFact`s; instead we go
/// straight to the trigger side: any `ExistentialTrigger (rt, body,
/// head)` with `sup ⊑ rt` and `body ∈ subsumers(C)` fires `head` to
/// every class that subsumes `A`. Role-hierarchy lifts apply at the
/// fact roles (`r₁` and `r₂`) and at the trigger role (`rt`).
///
/// Inverse-role chains and length ≠ 2 chains are rejected upstream
/// during rule collection — those stay in the tableau's lane.
fn apply_role_chains(
    facts: &mut ExistentialFacts,
    subsumers: &Subsumers,
    rules: &ElRules,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
    subsumers_grew_last_round: bool,
) -> bool {
    if rules.chain_axioms.is_empty() {
        return false;
    }
    // Frontier optimisation: pairs (i, j) where neither side was
    // added since the last chain call AND the subsumer table didn't
    // grow can't produce anything new — we processed them already.
    // When subsumers grow, previously-failing chain conditions
    // (`subsumers.contains(fact1.target, fact2.sub)`) can now hold,
    // so we have to re-scan everything.
    let n = facts.list.len();
    let old_boundary = if subsumers_grew_last_round {
        0
    } else {
        facts.chained_through
    };
    // Collect derivations into a side buffer so the inner loop can
    // keep a borrow into `facts.by_sub` without conflicting with
    // `facts.add()`. The buffer is drained after each chain.
    let mut pending: Vec<ExistentialFact> = Vec::new();
    let mut changed = false;
    for &(r1, r2, sup) in &rules.chain_axioms {
        // For each head fact (i) with role `r1` (or sub-role),
        // candidate tail facts are those whose `sub` is one of the
        // subsumers of `head_edge.target`. We iterate that subsumer
        // set directly via the by_sub index instead of scanning the
        // whole fact list.
        for i in 0..n {
            let head_edge = facts.list[i];
            if !supers_of(role_super, head_edge.role).contains(&r1) {
                continue;
            }
            let Some(target_subsumers) = subsumers.table.get(&head_edge.target) else {
                continue;
            };
            for &candidate_sub in target_subsumers {
                let Some(tail_ids) = facts.by_sub.get(&candidate_sub) else {
                    continue;
                };
                for &j in tail_ids {
                    // Delta gate: if neither side is new, we already
                    // processed (i, j) on the previous chain call.
                    if i < old_boundary && j < old_boundary {
                        continue;
                    }
                    let tail_edge = facts.list[j];
                    if !supers_of(role_super, tail_edge.role).contains(&r2) {
                        continue;
                    }
                    pending.push(ExistentialFact {
                        sub: head_edge.sub,
                        role: sup,
                        target: tail_edge.target,
                    });
                }
            }
        }
        for fact in pending.drain(..) {
            if facts.add(fact) {
                changed = true;
            }
        }
    }
    facts.chained_through = n;
    changed
}

/// Property domain + range. For every existential fact `(A, r, Y)`:
/// - `domain(r')` ∈ subsumers(X) for every `X` with `A` in its
///   subsumers and every `r' ⊒ r`;
/// - `range(r')` ∈ subsumers(Y) for every `r' ⊒ r`.
fn apply_domain_and_range(
    subsumers: &mut Subsumers,
    facts: &ExistentialFacts,
    rules: &ElRules,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
) -> bool {
    let mut changed = false;
    for fact in &facts.list {
        let supers = supers_of(role_super, fact.role);
        for super_role in &supers {
            if let Some(domains) = rules.role_domains.get(super_role) {
                let candidates = classes_with_subsumer(subsumers, fact.sub);
                for &dom in domains {
                    for x in &candidates {
                        if subsumers.add(*x, dom) {
                            changed = true;
                        }
                    }
                }
            }
            if let Some(ranges) = rules.role_ranges.get(super_role) {
                for &rng in ranges {
                    if subsumers.add(fact.target, rng) {
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

/// `DisjointClasses(A, B)` ⇒ every class with both `A` and `B` as
/// subsumers is flagged as unsatisfiable.
fn apply_disjointness(subsumers: &mut Subsumers, rules: &ElRules) -> bool {
    let mut changed = false;
    for &(a, b) in &rules.disjoint_pairs {
        let candidates: Vec<ClassId> = subsumers
            .table
            .iter()
            .filter_map(|(x, s)| {
                if !subsumers.unsatisfiable.contains(x) && s.contains(&a) && s.contains(&b) {
                    Some(*x)
                } else {
                    None
                }
            })
            .collect();
        for x in candidates {
            if subsumers.unsatisfiable.insert(x) {
                changed = true;
            }
        }
    }
    changed
}

/// Transitive closure of the current `subsumers` relation.
fn apply_transitivity(subsumers: &mut Subsumers) -> bool {
    let mut changed = false;
    let snapshot: Vec<(ClassId, Vec<ClassId>)> = subsumers
        .table
        .iter()
        .map(|(k, v)| (*k, v.iter().copied().collect()))
        .collect();
    for (c, ds) in &snapshot {
        for d in ds {
            if let Some(es) = subsumers.table.get(d) {
                let new_subs: Vec<ClassId> = es.iter().copied().collect();
                for e in new_subs {
                    if subsumers.add(*c, e) {
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

/// Look up the reflexive + transitive super-role closure for `r`,
/// falling back to `[r]` if the closure has no entry (defensive).
fn supers_of(role_super: &HashMap<RoleId, HashSet<RoleId>>, r: RoleId) -> Vec<RoleId> {
    role_super
        .get(&r)
        .map_or_else(|| vec![r], |set| set.iter().copied().collect())
}

/// Snapshot every class id whose subsumer set currently contains `c`.
fn classes_with_subsumer(subsumers: &Subsumers, c: ClassId) -> Vec<ClassId> {
    subsumers
        .table
        .iter()
        .filter_map(|(x, s)| if s.contains(&c) { Some(*x) } else { None })
        .collect()
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
#[derive(Debug, Default, Clone)]
pub struct Subsumers {
    table: HashMap<ClassId, HashSet<ClassId>>,
    /// Classes the saturation has proven equivalent to `⊥` —
    /// derived from `DisjointClasses(A, B)` axioms where the closure
    /// shows some class has both `A` and `B` as subsumers.
    unsatisfiable: HashSet<ClassId>,
}

impl Subsumers {
    fn with_capacity(n: usize) -> Self {
        Self {
            table: HashMap::with_capacity(n),
            unsatisfiable: HashSet::new(),
        }
    }

    /// Insert `(sub ⊑ sup)`. Returns `true` if this was new.
    fn add(&mut self, sub: ClassId, sup: ClassId) -> bool {
        self.table.entry(sub).or_default().insert(sup)
    }

    /// True iff the closure contains `sub ⊑ sup`.
    #[must_use]
    pub fn contains(&self, sub: ClassId, sup: ClassId) -> bool {
        self.table.get(&sub).is_some_and(|set| set.contains(&sup))
    }

    /// Every entailed subsumer of `c` (including `c` itself).
    #[must_use]
    pub fn subsumers_of(&self, c: ClassId) -> Vec<ClassId> {
        self.table
            .get(&c)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// True iff saturation proved `c` is empty in every model (i.e.
    /// `c ⊑ ⊥`).
    #[must_use]
    pub fn is_unsatisfiable(&self, c: ClassId) -> bool {
        self.unsatisfiable.contains(&c)
    }

    /// Every class flagged as `⊑ ⊥` by the saturation pass.
    #[must_use]
    pub fn unsatisfiable_classes(&self) -> Vec<ClassId> {
        self.unsatisfiable.iter().copied().collect()
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

fn collect_el_rules(internal: &InternalOntology) -> ElRules {
    let mut rules = ElRules::default();
    let mut tseitin = TseitinAllocator::new(internal.vocabulary.num_classes());
    for ax in &internal.axioms {
        match ax {
            Axiom::SubClassOf { sub, sup } => {
                lower_sub_class_of(*sub, *sup, &internal.concepts, &mut rules, &mut tseitin);
            }
            Axiom::EquivalentClasses(members) => {
                let atomics: Vec<ClassId> = members
                    .iter()
                    .filter_map(|c| match internal.concepts.get(*c) {
                        ConceptExpr::Atomic(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                for a in &atomics {
                    for b in &atomics {
                        if a != b {
                            rules
                                .atomic_subsumptions
                                .push(AtomicSubsumption { sub: *a, sup: *b });
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
    rules
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
