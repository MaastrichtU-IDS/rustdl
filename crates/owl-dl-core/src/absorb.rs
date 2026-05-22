//! Absorption: turn `TBox` axioms into focused triggers the tableau can
//! apply lazily.
//!
//! Three flavors are produced in one pipeline:
//!
//! - **Binary absorption (class trigger)** — `⊤ ⊑ ¬A ⊔ ψ` becomes
//!   `ConceptRule { trigger: A, conclusion: ψ }`. Fires when `A` shows
//!   up in a node label.
//! - **Nominal absorption** — `⊤ ⊑ ¬{a} ⊔ ψ` becomes
//!   `NominalRule { individual: a, conclusion: ψ }`. Applies directly
//!   to the named individual `a`.
//! - **Role absorption** — a `ConceptRule`/residual GCI whose conclusion
//!   is `∀R.D` is rewritten as `RoleRule { role: R, guard, target_label: D }`.
//!   Fires when an R-edge from a node carrying the guard (if any) is
//!   added. This is the second pass — it consumes the output of binary
//!   absorption.
//!
//! ## Why
//!
//! A naive tableau must apply every GCI `⊤ ⊑ φ` universally — to every
//! node, adding `φ` to its label. Disjunctive `φ` then causes branching
//! everywhere. Absorption finds patterns that let triggers fire only
//! when needed.
//!
//! ## Algorithm (single-trigger v0)
//!
//! For each input axiom, encode as `⊤ ⊑ φ`:
//!
//! - `SubClassOf { sub, sup }` → `φ = nnf(¬sub) ⊔ sup`.
//! - `EquivalentClasses(ids)` → decompose into pairwise `SubClassOf`.
//! - `DisjointClasses(ids)` → decompose into pairwise `SubClassOf(Ci, ¬Cj)`.
//! - `DisjointUnion { class, members }` → emit the equivalence half and
//!   pairwise-disjoint half.
//! - `ObjectPropertyDomain { role, domain }` → `∃role.⊤ ⊑ domain`.
//! - `ObjectPropertyRange { role, range }`  → `⊤ ⊑ ∀role.range`.
//!
//! Then walk the top-level disjuncts of `φ` looking for the first that
//! has shape `Not(Atomic(A))` or `Not(Nominal(a))`. If found, emit a
//! `ConceptRule` or `NominalRule` accordingly; the conclusion is the
//! `Or` of the remaining disjuncts. Otherwise `φ` joins `residual_gcis`.
//!
//! After the binary/nominal pass, [`absorb_roles`] rewrites every rule
//! or residual GCI whose conclusion is exactly `∀R.D` into a `RoleRule`.
//!
//! Multi-trigger absorption (`A ⊓ B ⊑ C`) is a Phase 4 refinement.

use std::collections::HashMap;

use crate::ConceptPool;
use crate::ir::{ClassId, ConceptExpr, ConceptId, IndividualId, Role};
use crate::normalize::to_nnf;
use crate::ontology::Axiom;

/// The output of absorption. Always a derived view of an `InternalOntology`'s
/// axiom list — never a replacement.
///
/// In addition to the four `Vec`-based axiom families, holds two
/// dispatch indices ([`Self::concept_rules_by_trigger`],
/// [`Self::nominal_rules_by_individual`]). They map a trigger to the
/// list of conclusions to apply, so [`crate::AbsorbedTBox`]-driven
/// tableau rules do `O(triggers × hits_per_trigger)` work per node
/// instead of `O(triggers × |rules|)`. [`absorb`] and [`absorb_roles`]
/// keep the indices in sync; callers who build an [`AbsorbedTBox`] by
/// hand should call [`Self::finalize`] before handing it to the
/// tableau (the tableau falls back to a linear scan when the indices
/// are empty, so this is "for performance, not correctness").
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AbsorbedTBox {
    /// `A ⊑ ψ` — when the trigger class appears in a node label,
    /// add the conclusion concept.
    pub concept_rules: Vec<ConceptRule>,
    /// `{a} ⊑ ψ` — apply the conclusion directly to the named
    /// individual.
    pub nominal_rules: Vec<NominalRule>,
    /// `[guard ⊑] ∀R.D` — when an R-edge to `y` is added (from a node
    /// carrying the guard if `Some`, or any node if `None`), add
    /// `target_label` to `y`'s label.
    pub role_rules: Vec<RoleRule>,
    /// `⊤ ⊑ φ` — applied universally by the tableau, after every other
    /// pattern was tried.
    pub residual_gcis: Vec<ConceptId>,
    /// Index: every conclusion `ConceptId` that should fire for a
    /// given trigger class. Derived from `concept_rules` by
    /// [`Self::finalize`]; consulted by `apply_concept_rules` to skip
    /// the linear scan.
    pub concept_rules_by_trigger: HashMap<ClassId, Vec<ConceptId>>,
    /// Same idea for nominal rules — index by individual id.
    pub nominal_rules_by_individual: HashMap<IndividualId, Vec<ConceptId>>,
    /// `RoleRule`s with no class guard — they fire on any node that
    /// has an outgoing edge matching their `role`. Partition of
    /// `role_rules` produced by [`Self::finalize`].
    pub unguarded_role_rules: Vec<RoleRule>,
    /// Guarded `RoleRule`s indexed by guard class. Partition of
    /// `role_rules` produced by [`Self::finalize`].
    pub guarded_role_rules_by_guard: HashMap<ClassId, Vec<RoleRule>>,
}

impl AbsorbedTBox {
    /// Rebuild the dispatch indices from the canonical `Vec` fields.
    /// Idempotent — safe to call after any mutation of the rule lists.
    /// Linear in the total rule count; cheap.
    pub fn finalize(&mut self) {
        self.concept_rules_by_trigger.clear();
        self.concept_rules_by_trigger
            .reserve(self.concept_rules.len());
        for rule in &self.concept_rules {
            self.concept_rules_by_trigger
                .entry(rule.trigger)
                .or_default()
                .push(rule.conclusion);
        }
        self.nominal_rules_by_individual.clear();
        self.nominal_rules_by_individual
            .reserve(self.nominal_rules.len());
        for rule in &self.nominal_rules {
            self.nominal_rules_by_individual
                .entry(rule.individual)
                .or_default()
                .push(rule.conclusion);
        }
        self.unguarded_role_rules.clear();
        self.guarded_role_rules_by_guard.clear();
        for rule in &self.role_rules {
            match rule.guard {
                None => self.unguarded_role_rules.push(*rule),
                Some(g) => self
                    .guarded_role_rules_by_guard
                    .entry(g)
                    .or_default()
                    .push(*rule),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ConceptRule {
    pub trigger: ClassId,
    pub conclusion: ConceptId,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct NominalRule {
    pub individual: IndividualId,
    pub conclusion: ConceptId,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct RoleRule {
    /// The role expression to match against an edge incident on the
    /// labelled node. `Role::Named(r)` fires on outgoing r-edges;
    /// `Role::Inverse(r)` fires on incoming r-edges. Sub-role
    /// propagation is consulted by the tableau, not by absorption.
    pub role: Role,
    pub guard: Option<ClassId>,
    pub target_label: ConceptId,
}

/// Run absorption over the NNF axiom list. The full pipeline:
///
/// 1. Binary/nominal absorption: walk every axiom, encode as `⊤ ⊑ φ`,
///    extract a class or individual trigger when possible.
/// 2. Role absorption: rewrite rules whose conclusion is `∀R.D` as
///    `RoleRule`s.
#[must_use]
pub fn absorb(axioms_nnf: &[Axiom], pool: &mut ConceptPool) -> AbsorbedTBox {
    let mut tbox = AbsorbedTBox::default();
    for ax in axioms_nnf {
        absorb_one(ax, pool, &mut tbox);
    }
    absorb_roles(&mut tbox, pool);
    tbox
}

// `absorb_roles` is the last mutator on `concept_rules` /
// `nominal_rules`, so it owns the responsibility for refreshing the
// dispatch indices — see the `finalize()` call at its tail.

/// Second pass over an [`AbsorbedTBox`]: rewrite rules / residual GCIs of
/// shape `∀R.D` as [`RoleRule`]s. Conceptually a separate stage from
/// binary/nominal absorption, exposed publicly so consumers can run it
/// against an externally-built tbox.
pub fn absorb_roles(tbox: &mut AbsorbedTBox, pool: &ConceptPool) {
    // Concept rules with conclusion All(R, D) become guarded role rules.
    let mut kept = Vec::with_capacity(tbox.concept_rules.len());
    for rule in std::mem::take(&mut tbox.concept_rules) {
        if let ConceptExpr::All(role, target) = pool.get(rule.conclusion) {
            tbox.role_rules.push(RoleRule {
                role: *role,
                guard: Some(rule.trigger),
                target_label: *target,
            });
        } else {
            kept.push(rule);
        }
    }
    tbox.concept_rules = kept;

    // Residual GCIs of shape ⊤ ⊑ All(R, D) become unguarded role rules.
    let mut kept = Vec::with_capacity(tbox.residual_gcis.len());
    for gci in std::mem::take(&mut tbox.residual_gcis) {
        if let ConceptExpr::All(role, target) = pool.get(gci) {
            tbox.role_rules.push(RoleRule {
                role: *role,
                guard: None,
                target_label: *target,
            });
        } else {
            kept.push(gci);
        }
    }
    tbox.residual_gcis = kept;

    // Nominal rules with conclusion All(R, D): less common (the tableau
    // can handle these as nominal-plus-All) but follow the same pattern
    // for consistency.
    let mut kept = Vec::with_capacity(tbox.nominal_rules.len());
    for rule in std::mem::take(&mut tbox.nominal_rules) {
        if let ConceptExpr::All(role, target) = pool.get(rule.conclusion) {
            // Express as an *unguarded* role rule: the nominal-level
            // application is handled at ABox time by the tableau, which
            // walks edges from the specific individual. Phase 1 keeps it
            // as a NominalRule so the original individual stays attached.
            // No conversion here; leave the nominal rule unchanged.
            let _ = (role, target);
            kept.push(rule);
        } else {
            kept.push(rule);
        }
    }
    tbox.nominal_rules = kept;

    // Rebuild the dispatch indices now that every mutator has run.
    tbox.finalize();
}

fn absorb_one(ax: &Axiom, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    match ax {
        Axiom::SubClassOf { sub, sup } => absorb_sub_sup(*sub, *sup, pool, tbox),
        Axiom::EquivalentClasses(ids) => {
            for i in 0..ids.len() {
                for j in 0..ids.len() {
                    if i != j {
                        absorb_sub_sup(ids[i], ids[j], pool, tbox);
                    }
                }
            }
        }
        Axiom::DisjointClasses(ids) => {
            emit_pairwise_disjoint(ids, pool, tbox);
        }
        Axiom::DisjointUnion { class, members } => {
            let class_concept = pool.atomic(*class);
            let union_concept = pool.or(members.iter().copied());
            // The equivalence half.
            absorb_sub_sup(class_concept, union_concept, pool, tbox);
            absorb_sub_sup(union_concept, class_concept, pool, tbox);
            // Pairwise-disjoint half.
            emit_pairwise_disjoint(members, pool, tbox);
        }
        Axiom::ObjectPropertyDomain { role, domain } => {
            // ∃role.⊤ ⊑ domain
            let top = pool.top();
            let some_r_top = pool.some(*role, top);
            absorb_sub_sup(some_r_top, *domain, pool, tbox);
        }
        Axiom::ObjectPropertyRange { role, range } => {
            // ⊤ ⊑ ∀role.range — a clean residual GCI.
            let all_r = pool.all(*role, *range);
            tbox.residual_gcis.push(all_r);
        }
        _ => {
            // Role characteristics, ABox, declarations — not TBox content.
            // They flow through to the reasoner via separate paths.
        }
    }
}

fn emit_pairwise_disjoint(ids: &[ConceptId], pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let not_cj = pool.not(ids[j]);
            let not_cj_nnf = to_nnf(not_cj, pool);
            absorb_sub_sup(ids[i], not_cj_nnf, pool, tbox);
        }
    }
}

/// Encode `sub ⊑ sup` as `⊤ ⊑ nnf(¬sub) ⊔ sup` and try to extract a trigger.
fn absorb_sub_sup(sub: ConceptId, sup: ConceptId, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    let neg_sub = pool.not(sub);
    let neg_sub_nnf = to_nnf(neg_sub, pool);
    let disjunction = pool.or([neg_sub_nnf, sup]);
    absorb_gci(disjunction, pool, tbox);
}

/// Process a `⊤ ⊑ φ` GCI: extract a `Not(Atomic)` or `Not(Nominal)`
/// disjunct as trigger if any, otherwise add `φ` to the residual list.
fn absorb_gci(phi: ConceptId, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    let disjuncts: Vec<ConceptId> = match pool.get(phi) {
        ConceptExpr::Or(args) => args.to_vec(),
        _ => vec![phi],
    };

    // Find first disjunct of the form Not(Atomic) or Not(Nominal).
    let mut chosen: Option<(usize, Trigger)> = None;
    for (i, &d) in disjuncts.iter().enumerate() {
        if let Some(t) = as_trigger(d, pool) {
            chosen = Some((i, t));
            break;
        }
    }

    if let Some((pos, trigger)) = chosen {
        let rest: Vec<ConceptId> = disjuncts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| (i != pos).then_some(c))
            .collect();
        // Or normalizations handle empty (→ Bot), single (→ operand), or
        // multi-operand cases.
        let conclusion = pool.or(rest);
        match trigger {
            Trigger::Class(trigger) => tbox.concept_rules.push(ConceptRule {
                trigger,
                conclusion,
            }),
            Trigger::Individual(individual) => tbox.nominal_rules.push(NominalRule {
                individual,
                conclusion,
            }),
        }
    } else {
        tbox.residual_gcis.push(phi);
    }
}

/// What kind of "trigger" a `Not(...)` disjunct can produce.
enum Trigger {
    Class(ClassId),
    Individual(IndividualId),
}

/// Recognize `Not(Atomic(A))` or `Not(Nominal(a))` shapes; otherwise None.
fn as_trigger(cid: ConceptId, pool: &ConceptPool) -> Option<Trigger> {
    if let ConceptExpr::Not(inner) = pool.get(cid) {
        match pool.get(*inner) {
            ConceptExpr::Atomic(c) => Some(Trigger::Class(*c)),
            ConceptExpr::Nominal(i) => Some(Trigger::Individual(*i)),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::many_single_char_names)]

    use super::*;
    use crate::Vocabulary;
    use crate::ir::{Role, RoleId};
    use crate::nnf_axioms;
    use crate::ontology::InternalOntology;

    fn fresh(class_names: &[&str]) -> InternalOntology {
        let mut o = InternalOntology::new();
        for n in class_names {
            o.vocabulary.intern_class(n);
        }
        o
    }

    fn cid(o: &InternalOntology, name: &str) -> ClassId {
        o.vocabulary.class_id(name).expect("class missing")
    }

    fn atom(o: &mut InternalOntology, name: &str) -> ConceptId {
        let c = cid(o, name);
        o.concepts.atomic(c)
    }

    /// NNF the ontology's axioms and run absorption. Returns the absorbed
    /// tbox and the NNF'd axioms (for inspection in tests).
    fn run(o: &mut InternalOntology) -> AbsorbedTBox {
        let nnf = nnf_axioms(o);
        absorb(&nnf, &mut o.concepts)
    }

    #[test]
    fn atomic_sub_class_of_yields_one_rule() {
        // A ⊑ B  →  rule (A, B).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.concept_rules[0].trigger, cid(&o, "A"));
        assert_eq!(t.concept_rules[0].conclusion, b);
    }

    #[test]
    fn sub_class_of_with_conjunctive_conclusion() {
        // A ⊑ B ⊓ C  →  rule (A, And([B, C])).
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let and_bc = o.concepts.and([b, cc]);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: and_bc,
        });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert_eq!(t.concept_rules[0].trigger, cid(&o, "A"));
        assert_eq!(t.concept_rules[0].conclusion, and_bc);
    }

    #[test]
    fn complex_lhs_with_atomic_rhs_absorbs_via_double_negation() {
        // (B ⊓ C) ⊑ A  →  ⊤ ⊑ ¬(B⊓C) ⊔ A  →  ⊤ ⊑ ¬B ⊔ ¬C ⊔ A.
        // Top-level disjuncts include Not(B) and Not(C); pick one (first
        // by id) as trigger, conclusion = Or of the rest.
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let and_bc = o.concepts.and([b, cc]);
        o.axioms.push(Axiom::SubClassOf {
            sub: and_bc,
            sup: a,
        });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert!(t.residual_gcis.is_empty());
        // The trigger must be one of B or C (whichever Not is sorted first).
        let trigger = t.concept_rules[0].trigger;
        assert!(trigger == cid(&o, "B") || trigger == cid(&o, "C"));
    }

    #[test]
    fn pure_existential_gci_is_residual() {
        // ⊤ ⊑ ∃R.A  has no Not(Atomic) top-level disjunct → residual.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let some_a = o.concepts.some(r, a);
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf {
            sub: top,
            sup: some_a,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        assert_eq!(t.residual_gcis[0], some_a);
    }

    #[test]
    fn equivalent_classes_creates_pairwise_rules() {
        // A ≡ B  →  rules (A, B) and (B, A).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::EquivalentClasses(vec![a, b]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 2);
        // One rule for each direction.
        let triggers: Vec<ClassId> = t.concept_rules.iter().map(|r| r.trigger).collect();
        assert!(triggers.contains(&cid(&o, "A")));
        assert!(triggers.contains(&cid(&o, "B")));
    }

    #[test]
    fn disjoint_classes_yields_not_atom_conclusion() {
        // DisjointClasses(A, B)  →  rule (A, Not(B)).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::DisjointClasses(vec![a, b]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        let rule = &t.concept_rules[0];
        // The trigger is whichever Not gets matched first — but both
        // operands are atoms, so the trigger and conclusion partition them.
        let trigger = rule.trigger;
        assert!(trigger == cid(&o, "A") || trigger == cid(&o, "B"));
        // Conclusion must be Not(Atomic(other)).
        let other = if trigger == cid(&o, "A") {
            cid(&o, "B")
        } else {
            cid(&o, "A")
        };
        let expected_other_atom = o.concepts.atomic(other);
        let expected_conclusion = o.concepts.not(expected_other_atom);
        assert_eq!(rule.conclusion, expected_conclusion);
    }

    #[test]
    fn disjoint_classes_three_way_yields_three_pairwise_rules() {
        // DisjointClasses(A, B, C) — pairs are (A,B), (A,C), (B,C).
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        o.axioms.push(Axiom::DisjointClasses(vec![a, b, cc]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 3);
    }

    #[test]
    fn disjoint_union_emits_subsumption_and_pairwise_disjoint_rules() {
        // DisjointUnion(P, [C1, C2]):
        //   1. P ⊑ C1 ⊔ C2     →  one rule (P, Or([C1, C2]))
        //   2. C1 ⊔ C2 ⊑ P    →  residual (multi-trigger required)
        //   3. C1 ⌖ C2         →  one rule (C1, Not(C2))
        let mut o = fresh(&["P", "C1", "C2"]);
        let c1 = atom(&mut o, "C1");
        let c2 = atom(&mut o, "C2");
        o.axioms.push(Axiom::DisjointUnion {
            class: cid(&o, "P"),
            members: vec![c1, c2],
        });
        let t = run(&mut o);
        // One concept rule from P ⊑ C1 ⊔ C2.
        // One concept rule from pairwise disjoint.
        // C1 ⊔ C2 ⊑ P drops to residual.
        assert_eq!(t.concept_rules.len(), 2);
        assert_eq!(t.residual_gcis.len(), 1);
    }

    #[test]
    fn object_property_range_becomes_unguarded_role_rule() {
        // ObjectPropertyRange(r, A)  ≡  ⊤ ⊑ ∀r.A
        // After binary absorption: residual GCI ∀r.A.
        // After role absorption: RoleRule { role: r, guard: None, target_label: A }.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        o.axioms
            .push(Axiom::ObjectPropertyRange { role: r, range: a });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.role_rules.len(), 1);
        let rr = t.role_rules[0];
        assert_eq!(rr.role, crate::Role::Named(r.role_id()));
        assert_eq!(rr.guard, None);
        assert_eq!(rr.target_label, a);
    }

    #[test]
    fn sub_class_of_all_becomes_guarded_role_rule() {
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let all_r_b = o.concepts.all(r, b);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: all_r_b,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.role_rules.len(), 1);
        let rr = t.role_rules[0];
        assert_eq!(rr.role, crate::Role::Named(r.role_id()));
        assert_eq!(rr.guard, Some(cid(&o, "A")));
        assert_eq!(rr.target_label, b);
    }

    #[test]
    fn nominal_sub_class_yields_nominal_rule() {
        // {a} ⊑ B  → NominalRule(individual=a, conclusion=B).
        let mut o = fresh(&["B"]);
        let b = atom(&mut o, "B");
        let ind = o.vocabulary.intern_individual("a");
        let nom = o.concepts.nominal(ind);
        o.axioms.push(Axiom::SubClassOf { sub: nom, sup: b });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.nominal_rules.len(), 1);
        assert_eq!(t.nominal_rules[0].individual, ind);
        assert_eq!(t.nominal_rules[0].conclusion, b);
    }

    #[test]
    fn unrelated_axioms_pass_through_without_contribution() {
        // Role characteristics and ABox don't show up in AbsorbedTBox.
        let mut o = fresh(&["A"]);
        let _ = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let _ = Vocabulary::new(); // placate clippy::dead_code
        o.axioms.push(Axiom::TransitiveRole(r));
        let i = o.vocabulary.intern_individual("a");
        let a = o.concepts.atomic(cid(&o, "A"));
        o.axioms.push(Axiom::ClassAssertion {
            class: a,
            individual: i,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert!(t.residual_gcis.is_empty());
    }

    #[test]
    fn sub_class_of_top_to_atom_is_residual() {
        // ⊤ ⊑ A — no Not(Atomic) anywhere; just `A` is the residual GCI.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf { sub: top, sup: a });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        assert_eq!(t.residual_gcis[0], a);
    }
}
