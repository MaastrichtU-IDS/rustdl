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
    let mut changed = true;
    while changed {
        changed = false;
        changed |= apply_atomic_subsumptions(&mut subsumers, &rules);
        changed |= apply_conjunctive_triggers(&mut subsumers, &rules);
        changed |= apply_existential_propagation(&mut subsumers, &rules, &role_super);
        changed |= apply_domain_and_range(&mut subsumers, &rules, &role_super);
        changed |= apply_disjointness(&mut subsumers, &rules);
        changed |= apply_transitivity(&mut subsumers);
    }
    subsumers
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
fn apply_existential_propagation(
    subsumers: &mut Subsumers,
    rules: &ElRules,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
) -> bool {
    let mut changed = false;
    for fact in &rules.existential_facts {
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

/// Property domain + range. For every existential fact `(A, r, Y)`:
/// - `domain(r')` ∈ subsumers(X) for every `X` with `A` in its
///   subsumers and every `r' ⊒ r`;
/// - `range(r')` ∈ subsumers(Y) for every `r' ⊒ r`.
fn apply_domain_and_range(
    subsumers: &mut Subsumers,
    rules: &ElRules,
    role_super: &HashMap<RoleId, HashSet<RoleId>>,
) -> bool {
    let mut changed = false;
    for fact in &rules.existential_facts {
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

fn collect_el_rules(internal: &InternalOntology) -> ElRules {
    let mut rules = ElRules::default();
    for ax in &internal.axioms {
        match ax {
            Axiom::SubClassOf { sub, sup } => {
                lower_sub_class_of(*sub, *sup, &internal.concepts, &mut rules);
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
            _ => {}
        }
    }
    rules
}

/// Lower a single `SubClassOf(sub, sup)` axiom into atomic facts
/// and conjunctive triggers. Anything that doesn't fit (existentials,
/// disjunction, complement, cardinality, ...) is silently dropped —
/// the orchestrator handles those via tableau fallback.
fn lower_sub_class_of(sub: ConceptId, sup: ConceptId, pool: &ConceptPool, rules: &mut ElRules) {
    match pool.get(sub) {
        ConceptExpr::Atomic(sub_id) => {
            for atomic_sup in atomic_operands_on_right(sup, pool) {
                rules.atomic_subsumptions.push(AtomicSubsumption {
                    sub: *sub_id,
                    sup: atomic_sup,
                });
            }
            // Atomic ⊑ ∃r.Y: existential fact.
            if let Some((role, target)) = atomic_existential(sup, pool) {
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
                    if let Some((role, target)) = atomic_existential(*op, pool) {
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
            // ∃r.B ⊑ C: existential trigger. Only named-role + atomic-
            // body shapes are in scope.
            if role.is_inverse() {
                return;
            }
            let ConceptExpr::Atomic(body_id) = pool.get(*body) else {
                return;
            };
            for head in atomic_operands_on_right(sup, pool) {
                rules.existential_triggers.push(ExistentialTrigger {
                    role: role.role_id(),
                    body: *body_id,
                    head,
                });
            }
        }
        _ => {}
    }
}

/// Extract `(role_id, target_class_id)` from a concept of the shape
/// `∃<named-role>.<atomic-class>`; `None` for any other shape.
fn atomic_existential(c: ConceptId, pool: &ConceptPool) -> Option<(RoleId, ClassId)> {
    let ConceptExpr::Some(role, body) = pool.get(c) else {
        return None;
    };
    if role.is_inverse() {
        return None;
    }
    let ConceptExpr::Atomic(body_id) = pool.get(*body) else {
        return None;
    };
    Some((role.role_id(), *body_id))
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
