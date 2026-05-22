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

use owl_dl_core::{Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, InternalOntology, RoleId};

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
    let mut changed = true;
    while changed {
        changed = false;
        // Direct told subsumers: A ⊑ B.
        for rule in &rules.atomic_subsumptions {
            if subsumers.add(rule.sub, rule.sup) {
                changed = true;
            }
        }
        // Conjunctive triggers: if X has every B_i in its subsumers,
        // it gains the trigger's head.
        for trigger in &rules.conjunctive_triggers {
            let len = subsumers.table.len();
            for i in 0..len {
                let id = ClassId::new(u32::try_from(i).expect("class count fits in u32"));
                if trigger.bodies.iter().all(|b| subsumers.contains(id, *b))
                    && subsumers.add(id, trigger.head)
                {
                    changed = true;
                }
            }
        }
        // Existential propagation (Kazakov CR5): for every fact
        // (A, r, Y) — meaning A ⊑ ∃r.Y — and every trigger (r, Z, W)
        // — meaning ∃r.Z ⊑ W — if Z is already a subsumer of Y,
        // every class that has A among its subsumers also gains W.
        for fact in &rules.existential_facts {
            for trigger in &rules.existential_triggers {
                if trigger.role != fact.role {
                    continue;
                }
                if !subsumers.contains(fact.target, trigger.body) {
                    continue;
                }
                // Propagate W to every X with A ∈ subsumers(X). We
                // snapshot the table to avoid mutating under
                // iteration.
                let candidates: Vec<ClassId> = subsumers
                    .table
                    .iter()
                    .filter_map(|(x, s)| {
                        if s.contains(&fact.sub) {
                            Some(*x)
                        } else {
                            None
                        }
                    })
                    .collect();
                for x in candidates {
                    if subsumers.add(x, trigger.head) {
                        changed = true;
                    }
                }
            }
        }
        // Transitivity: if D ∈ subsumers(C) and E ∈ subsumers(D),
        // add E to subsumers(C). Snapshot to avoid mutating under
        // iteration.
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
    }
    subsumers
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
}

impl Subsumers {
    fn with_capacity(n: usize) -> Self {
        Self {
            table: HashMap::with_capacity(n),
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
