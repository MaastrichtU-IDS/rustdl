//! Sound ABox-driven ontology-level inconsistency pre-check.
//!
//! Runs after `collect_abox` and the EL saturator (so both the
//! `Abox` struct and `Subsumers` closure are available). Returns
//! [`AboxVerdict::Inconsistent`] on a detected clash;
//! [`AboxVerdict::Unknown`] otherwise. The caller falls through to
//! the existing tableau path on `Unknown`.
//!
//! Sound under-approximation: every positive verdict is a direct
//! semantic clash on the `ABox`; no inferred subsumption is created.
//!
//! Eight clash patterns implemented incrementally (P1 direct-Bot
//! assertion, P2 disjoint types per individual, P3 NegOPA-vs-OPA,
//! P4 SameAs∩DifferentFrom, P5 Functional+two-distinct-witnesses,
//! P6 Asymmetric/Irreflexive violations, P7 domain/range as a
//! stretch, P9 `DisjointObjectProperties` direct `ABox` clash).
//!
//! Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`

use crate::union_find::UnionFind;
use owl_dl_core::ir::{ClassId, IndividualId, Role, RoleId};

/// Verdict from the `ABox` consistency check.
///
/// Sound under-approximation: `Inconsistent` is unconditional;
/// `Unknown` means "we couldn't catch a clash with the cheap
/// patterns" — caller should fall through to the full tableau.
#[derive(Debug, Clone)]
pub(crate) enum AboxVerdict {
    Inconsistent { reason: ClashReason },
    Unknown,
}

/// The specific clash the check detected. Surfaced in `RUSTDL_TRACE`
/// output and intended for a future `consistent --explain` extension
/// (not part of this project's scope).
///
/// The per-variant fields are read only through the derived `Debug`
/// impl (the `RUSTDL_TRACE=1` `abox_check: inconsistent — {reason:?}`
/// line). Rust's dead-code analysis doesn't count `Debug`-only reads,
/// so the fields would warn without this allow. They're load-bearing
/// for the trace output and the planned `--explain` surface — keep them.
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = "fields read via Debug in RUSTDL_TRACE output + future --explain"
)]
pub(crate) enum ClashReason {
    /// P1: `ClassAssertion(C, a)` with `Subsumers::is_unsatisfiable(C)`.
    AssertedBot {
        individual: IndividualId,
        class: ClassId,
    },
    /// P2 / P7: individual `a` has both `c` and `d` in its asserted-
    /// or-derived type set, and `(c, d)` is in `told_disjoint_pairs`.
    DisjointTypes {
        individual: IndividualId,
        c: ClassId,
        d: ClassId,
    },
    /// P3: positive `R(a, b)` and `NegativeObjectPropertyAssertion(R, a, b)`.
    NegOpaConflict {
        from: IndividualId,
        role: RoleId,
        to: IndividualId,
    },
    /// P4 / P5: `(a, b)` in `DifferentIndividuals` and union-find
    /// (post-`SameIndividual` and post-functional-merges) finds them equal.
    SameDifferent { a: IndividualId, b: IndividualId },
    /// P5 detail: `Functional(R) ∧ R(a, b1) ∧ R(a, b2)` forced a
    /// merge of `b1` and `b2` that subsequently clashed with a
    /// `DifferentIndividuals` declaration.
    FunctionalDiff {
        role: RoleId,
        a: IndividualId,
        b1: IndividualId,
        b2: IndividualId,
    },
    /// P6: `Asymmetric(R) ∧ R(a, b) ∧ R(b, a)`.
    AsymmetricViolation {
        role: RoleId,
        a: IndividualId,
        b: IndividualId,
    },
    /// P6: `Irreflexive(R) ∧ R(a, a)` (or `R(a, b)` with `a ≡ b` after merge).
    IrreflexiveViolation { role: RoleId, a: IndividualId },
    /// P8: `Functional(R)` and individual `a`'s type-closure implies both
    /// `∃R.q1` and `∃R.q2` with `q1`, `q2` told-disjoint. The single
    /// `R`-successor must be both → ⊥.
    FunctionalCollapse {
        individual: IndividualId,
        /// The signed role: `r⁻` when the collapse is on `r`-predecessors.
        role: Role,
        q1: ClassId,
        q2: ClassId,
    },
    /// P9: `DisjointObjectProperties(R, S)` and both `R(a, b)` and
    /// `S(a, b)` are directly asserted in the `ABox`. `R` is forward; an inverse `S`
    /// (#534) means the clashing assertion is `S(b, a)`.
    DisjointRolePairViolation {
        r: RoleId,
        s: Role,
        from: IndividualId,
        to: IndividualId,
    },
}

/// Collect existential restrictions `∃R.Q` (atomic `Q`) that appear as
/// `sup` itself or as a top-level `And` operand of `sup`. Used by P8 to
/// read a class's functional-role existential implications straight from
/// its defining axioms (`SubClassOf` / `EquivalentClasses`).
fn existential_funcs_in(
    sup: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ir::ConceptPool,
    out: &mut Vec<(Role, ClassId)>,
) {
    use owl_dl_core::ir::ConceptExpr;
    // #528: keep the SIGNED role. `∃r⁻.Q` is an `r`-predecessor in `Q`, which
    // `Functional(r)` does not bound; reading it as `∃r.Q` collapsed two
    // predecessors into one successor (a false inconsistency).
    match pool.get(sup) {
        ConceptExpr::Some(r, q) => {
            if let ConceptExpr::Atomic(qid) = pool.get(*q) {
                out.push((*r, *qid));
            }
        }
        ConceptExpr::And(ops) => {
            for &op in ops {
                if let ConceptExpr::Some(r, q) = pool.get(op)
                    && let ConceptExpr::Atomic(qid) = pool.get(*q)
                {
                    out.push((*r, *qid));
                }
            }
        }
        _ => {}
    }
}

/// Exactly the values [`check`] reads. Extracted so the dependency set is
/// compiler-checked: `abox_check` must never grow a dependency on the expensive
/// parts of `PreparedOntology` (`hyper`, `tbox`) without this struct changing,
/// which is what lets the classify fast path skip building them. See
/// `docs/superpowers/specs/2026-07-30-abox-check-reduced-input-design.md`.
pub(crate) struct AboxCheckInputs<'a> {
    pub(crate) abox: &'a crate::Abox,
    pub(crate) axioms: &'a [owl_dl_core::ontology::Axiom],
    pub(crate) told: &'a owl_dl_core::told::ToldTables,
    pub(crate) pool: &'a owl_dl_core::ir::ConceptPool,
    pub(crate) inverse_pairs: &'a [(RoleId, RoleId)],
    pub(crate) hierarchy: &'a crate::RoleHierarchy,
    pub(crate) disjoint_role_pairs: &'a [(Role, Role)],
    pub(crate) closure: &'a owl_dl_saturation::Subsumers,
}

/// Entry point. Runs all implemented clash patterns and returns the first
/// detected clash, or [`AboxVerdict::Unknown`] if none fire.
pub(crate) fn check(inputs: &AboxCheckInputs<'_>) -> AboxVerdict {
    // Early return: no individuals → no ABox → no clash possible.
    if inputs.abox.individuals.is_empty() {
        return AboxVerdict::Unknown;
    }
    let closure = inputs.closure;
    let pool = inputs.pool;

    // P1: direct-⊥ assertion.
    for &(individual, class_concept) in &inputs.abox.class_assertions {
        if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(class_concept)
            && closure.is_unsatisfiable(*c)
        {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::AssertedBot {
                    individual,
                    class: *c,
                },
            };
        }
    }

    // Per-individual atomic-type set: index → HashSet<ClassId>.
    // For each ClassAssertion(C, a) with C atomic, insert c and
    // every subsumer of c from the EL closure.
    let n = inputs.abox.individuals.len();
    let ind_index: std::collections::HashMap<owl_dl_core::ir::IndividualId, usize> = inputs
        .abox
        .individuals
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let mut types: Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>> =
        vec![std::collections::HashSet::new(); n];
    for &(individual, class_concept) in &inputs.abox.class_assertions {
        if let Some(&i) = ind_index.get(&individual)
            && let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(class_concept)
        {
            types[i].insert(*c);
            for s in closure.subsumers_of(*c) {
                types[i].insert(s);
            }
        }
    }

    // P2: pairwise told-disjoint over types[i].
    // Filter to class ids within ToldTables bounds: subsumers_of can
    // return Tseitin-fresh ids beyond vocabulary.num_classes().
    let told = inputs.told;
    let told_n = told.num_classes();
    for (i, type_set) in types.iter().enumerate() {
        let (individual, _) = inputs.abox.individuals[i];
        let cs: Vec<_> = type_set
            .iter()
            .copied()
            .filter(|c| (c.index() as usize) < told_n)
            .collect();
        for a in 0..cs.len() {
            for b in (a + 1)..cs.len() {
                if told.are_told_disjoint(cs[a], cs[b]) {
                    return AboxVerdict::Inconsistent {
                        reason: ClashReason::DisjointTypes {
                            individual,
                            c: cs[a],
                            d: cs[b],
                        },
                    };
                }
            }
        }
    }

    // P3: NegativeObjectPropertyAssertion vs positive
    // ObjectPropertyAssertion. Build a HashSet of positive triples
    // and test each negative against it.
    let pos: std::collections::HashSet<(
        owl_dl_core::ir::IndividualId,
        owl_dl_core::ir::RoleId,
        owl_dl_core::ir::IndividualId,
    )> = inputs.abox.property_assertions.iter().copied().collect();
    for &(from, role, to) in &inputs.abox.negative_property_triples {
        // Role-hierarchy downward propagation: a positive assertion on
        // any sub-role S of `role` (S ⊑ R) implies `R(a, b)` and so
        // clashes with NegOPA(R, a, b). A super-role assertion does
        // NOT imply R(a, b) and must not be flagged.
        //
        // `sub_roles(role)` returns the reflexive-transitive closure
        // (including `role` itself), so the direct-match case is
        // covered.
        for &sub_role in inputs.hierarchy.sub_roles(role) {
            if pos.contains(&(from, sub_role, to)) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::NegOpaConflict { from, role, to },
                };
            }
        }
    }

    // P4: SameAs ∩ DifferentFrom. Build union-find over individual
    // indices via same_pairs; check each different_pair against it.
    let n_ind = inputs.abox.individuals.len();
    let mut uf = UnionFind::new(n_ind);
    for &(a, b) in &inputs.abox.same_pairs {
        if let (Some(&i), Some(&j)) = (ind_index.get(&a), ind_index.get(&b)) {
            uf.union(
                u32::try_from(i).expect("ind index fits in u32"),
                u32::try_from(j).expect("ind index fits in u32"),
            );
        }
    }
    for &(a, b) in &inputs.abox.different_pairs {
        if let (Some(&i), Some(&j)) = (ind_index.get(&a), ind_index.get(&b))
            && uf.same(
                u32::try_from(i).expect("ind index fits in u32"),
                u32::try_from(j).expect("ind index fits in u32"),
            )
        {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::SameDifferent { a, b },
            };
        }
    }

    // P5: Functional + two-distinct-witnesses. For each functional
    // role R, group property_assertions by `(from, role)`; for each
    // group with ≥2 distinct `to`s, merge them in uf. After every
    // merge, re-test all `different_pairs`. Inverse-functional is
    // the dual: group `(role, to)`.
    use std::collections::HashMap as Map;
    let mut functional_roles: std::collections::HashSet<RoleId> = std::collections::HashSet::new();
    let mut inverse_functional_roles: std::collections::HashSet<RoleId> =
        std::collections::HashSet::new();
    for ax in inputs.axioms {
        match ax {
            // #528: the characteristics swap across polarity. `Functional(r⁻)` is
            // `InverseFunctional(r)` (the converter keeps it as `FunctionalRole(r⁻)`,
            // see `convert.rs`'s `InverseFunctionalObjectProperty` arm); reading it as
            // `Functional(r)` merged two `r`-successors of one individual.
            owl_dl_core::ontology::Axiom::FunctionalRole(r) => {
                if r.is_inverse() {
                    inverse_functional_roles.insert(r.role_id());
                } else {
                    functional_roles.insert(r.role_id());
                }
            }
            // `InverseFunctional(r⁻)` is `Functional(r)`; the converter normalises it
            // already (#149), so this arm only keeps a hand-built IR correct.
            owl_dl_core::ontology::Axiom::InverseFunctionalRole(r) => {
                if r.is_inverse() {
                    functional_roles.insert(r.role_id());
                } else {
                    inverse_functional_roles.insert(r.role_id());
                }
            }
            _ => {}
        }
    }

    // Group (from, role) → Vec<to>.
    let mut by_from_role: Map<(IndividualId, RoleId), Vec<IndividualId>> = Map::new();
    for &(from, role, to) in &inputs.abox.property_assertions {
        if functional_roles.contains(&role) {
            by_from_role.entry((from, role)).or_default().push(to);
        }
    }
    for ((from, role), tos) in &by_from_role {
        if tos.len() < 2 {
            continue;
        }
        let first = tos[0];
        let Some(&i0) = ind_index.get(&first) else {
            continue;
        };
        for &b in &tos[1..] {
            if let Some(&j) = ind_index.get(&b)
                && uf.union(
                    u32::try_from(i0).expect("fits in u32"),
                    u32::try_from(j).expect("fits in u32"),
                )
            {
                // New merge — re-check all different_pairs.
                for &(da, db) in &inputs.abox.different_pairs {
                    if let (Some(&ip), Some(&jp)) = (ind_index.get(&da), ind_index.get(&db))
                        && uf.same(
                            u32::try_from(ip).expect("fits"),
                            u32::try_from(jp).expect("fits"),
                        )
                    {
                        return AboxVerdict::Inconsistent {
                            reason: ClashReason::FunctionalDiff {
                                role: *role,
                                a: *from,
                                b1: first,
                                b2: b,
                            },
                        };
                    }
                }
            }
        }
    }

    // Inverse-functional: group (role, to) → Vec<from>, merge as above.
    let mut by_role_to: Map<(RoleId, IndividualId), Vec<IndividualId>> = Map::new();
    for &(from, role, to) in &inputs.abox.property_assertions {
        if inverse_functional_roles.contains(&role) {
            by_role_to.entry((role, to)).or_default().push(from);
        }
    }
    for ((role, to), froms) in &by_role_to {
        if froms.len() < 2 {
            continue;
        }
        let first = froms[0];
        let Some(&i0) = ind_index.get(&first) else {
            continue;
        };
        for &a in &froms[1..] {
            if let Some(&j) = ind_index.get(&a)
                && uf.union(
                    u32::try_from(i0).expect("fits"),
                    u32::try_from(j).expect("fits"),
                )
            {
                for &(da, db) in &inputs.abox.different_pairs {
                    if let (Some(&ip), Some(&jp)) = (ind_index.get(&da), ind_index.get(&db))
                        && uf.same(
                            u32::try_from(ip).expect("fits"),
                            u32::try_from(jp).expect("fits"),
                        )
                    {
                        return AboxVerdict::Inconsistent {
                            reason: ClashReason::FunctionalDiff {
                                role: *role,
                                a: *to,
                                b1: first,
                                b2: a,
                            },
                        };
                    }
                }
            }
        }
    }

    // P6: Asymmetric + Irreflexive.
    let mut asymmetric_roles: std::collections::HashSet<owl_dl_core::ir::RoleId> =
        std::collections::HashSet::new();
    let mut irreflexive_roles: std::collections::HashSet<owl_dl_core::ir::RoleId> =
        std::collections::HashSet::new();
    for ax in inputs.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::AsymmetricRole(r) => {
                asymmetric_roles.insert(r.role_id());
            }
            owl_dl_core::ontology::Axiom::IrreflexiveRole(r) => {
                irreflexive_roles.insert(r.role_id());
            }
            _ => {}
        }
    }
    // Asymmetric: scan for (a, R, b) and (b, R, a) both present.
    for &(from, role, to) in &inputs.abox.property_assertions {
        if asymmetric_roles.contains(&role) && pos.contains(&(to, role, from)) {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::AsymmetricViolation {
                    role,
                    a: from,
                    b: to,
                },
            };
        }
    }
    // Irreflexive: any (a, R, a). Also fires when SameAs merges
    // collapsed from == to: scan property_assertions and test via uf.
    for &(from, role, to) in &inputs.abox.property_assertions {
        if !irreflexive_roles.contains(&role) {
            continue;
        }
        if from == to {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::IrreflexiveViolation { role, a: from },
            };
        }
        if let (Some(&i), Some(&j)) = (ind_index.get(&from), ind_index.get(&to))
            && uf.same(
                u32::try_from(i).expect("fits in u32"),
                u32::try_from(j).expect("fits in u32"),
            )
        {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::IrreflexiveViolation { role, a: from },
            };
        }
    }

    // P7 stretch: domain/range propagation. For each
    // ObjectPropertyDomain(R, D) and assertion R(a, _), add D's
    // class (if atomic) + its EL subsumers to types[a]. Same for
    // range applied to the object. Then re-run the P2 scan.
    let mut domains: Vec<(owl_dl_core::ir::RoleId, owl_dl_core::ir::ConceptId)> = Vec::new();
    let mut ranges: Vec<(owl_dl_core::ir::RoleId, owl_dl_core::ir::ConceptId)> = Vec::new();
    for ax in inputs.axioms {
        match ax {
            // #527: the tables are keyed by the NAMED role of the normalised edges, so an
            // inverse-role axiom goes to the other table: `Domain(r⁻) = D` is
            // `Range(r) = D`, and `Range(r⁻) = D` is `Domain(r) = D`. Read as the named
            // role's, it typed the wrong end of every `r`-edge (a false inconsistency).
            owl_dl_core::ontology::Axiom::ObjectPropertyDomain { role, domain } => {
                if role.is_inverse() {
                    ranges.push((role.role_id(), *domain));
                } else {
                    domains.push((role.role_id(), *domain));
                }
            }
            owl_dl_core::ontology::Axiom::ObjectPropertyRange { role, range } => {
                if role.is_inverse() {
                    domains.push((role.role_id(), *range));
                } else {
                    ranges.push((role.role_id(), *range));
                }
            }
            _ => {}
        }
    }
    // Inverse-derived domain/range: `range(R) = D ⟺ domain(inv(R)) = D`
    // (and dually). Sound by definition of inverse. This is what lets a
    // family-style `isFatherOf(a, x)` — inverse of `hasFather` whose
    // *range* is `Man` — contribute `Man` to `types[a]`, even though
    // `isFatherOf` has no domain of its own. Without it the functional
    // collapse below (P8) can't see the two sexes on one individual.
    let inv_of = |r: RoleId| -> Option<RoleId> {
        inputs.inverse_pairs.iter().find_map(|&(a, b)| {
            if a == r {
                Some(b)
            } else if b == r {
                Some(a)
            } else {
                None
            }
        })
    };
    for (role, concept) in ranges.clone() {
        if let Some(s) = inv_of(role) {
            domains.push((s, concept)); // domain(inv(R)) = range(R)
        }
    }
    for (role, concept) in domains.clone() {
        if let Some(s) = inv_of(role) {
            ranges.push((s, concept)); // range(inv(R)) = domain(R)
        }
    }

    let mut augmented = false;
    for &(from, role, to) in &inputs.abox.property_assertions {
        for &(d_role, d_concept) in &domains {
            if d_role != role {
                continue;
            }
            if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(d_concept)
                && let Some(&i) = ind_index.get(&from)
            {
                augmented |= types[i].insert(*c);
                for s in closure.subsumers_of(*c) {
                    augmented |= types[i].insert(s);
                }
            }
        }
        for &(r_role, r_concept) in &ranges {
            if r_role != role {
                continue;
            }
            if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(r_concept)
                && let Some(&i) = ind_index.get(&to)
            {
                augmented |= types[i].insert(*c);
                for s in closure.subsumers_of(*c) {
                    augmented |= types[i].insert(s);
                }
            }
        }
    }

    if augmented {
        for (i, type_set) in types.iter().enumerate() {
            let (individual, _) = inputs.abox.individuals[i];
            let cs: Vec<_> = type_set
                .iter()
                .copied()
                .filter(|c| (c.index() as usize) < told_n)
                .collect();
            for a in 0..cs.len() {
                for b in (a + 1)..cs.len() {
                    if told.are_told_disjoint(cs[a], cs[b]) {
                        return AboxVerdict::Inconsistent {
                            reason: ClashReason::DisjointTypes {
                                individual,
                                c: cs[a],
                                d: cs[b],
                            },
                        };
                    }
                }
            }
        }
    }

    // P8: functional-role existential collapse. If an individual's
    // (augmented) type set implies both `∃R.q1` and `∃R.q2` with `R`
    // functional and `q1`, `q2` told-disjoint, its single `R`-successor
    // must be both → ⊥. `functional_roles` is from P5; `types` is fully
    // augmented above (incl. inverse-derived domain/range, which is what
    // lets the family `isFatherOf`/`isMotherOf` individuals carry both
    // `Man` and `Woman`). Sound + monotonic: only ever *adds* an
    // inconsistency the tableau would also find.
    if !functional_roles.is_empty() || !inverse_functional_roles.is_empty() {
        let mut existentials: std::collections::HashMap<ClassId, Vec<(Role, ClassId)>> =
            std::collections::HashMap::new();
        for ax in inputs.axioms {
            match ax {
                owl_dl_core::ontology::Axiom::SubClassOf { sub, sup } => {
                    if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(*sub) {
                        existential_funcs_in(*sup, pool, existentials.entry(*c).or_default());
                    }
                }
                owl_dl_core::ontology::Axiom::EquivalentClasses(members) => {
                    for &m in members {
                        if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(m) {
                            for &o in members {
                                if o != m {
                                    existential_funcs_in(
                                        o,
                                        pool,
                                        existentials.entry(*c).or_default(),
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for (i, type_set) in types.iter().enumerate() {
            let (individual, _) = inputs.abox.individuals[i];
            // #528: group by the SIGNED role, and ask whether THAT role is functional:
            // `r` is functional iff `Functional(r)`, `r⁻` iff `InverseFunctional(r)`.
            let mut by_role: std::collections::HashMap<Role, Vec<ClassId>> =
                std::collections::HashMap::new();
            for &t in type_set {
                if let Some(exs) = existentials.get(&t) {
                    for &(r, q) in exs {
                        let functional = if r.is_inverse() {
                            inverse_functional_roles.contains(&r.role_id())
                        } else {
                            functional_roles.contains(&r.role_id())
                        };
                        if functional {
                            by_role.entry(r).or_default().push(q);
                        }
                    }
                }
            }
            for (role, quals) in &by_role {
                for a in 0..quals.len() {
                    for b in (a + 1)..quals.len() {
                        if (quals[a].index() as usize) < told_n
                            && (quals[b].index() as usize) < told_n
                            && told.are_told_disjoint(quals[a], quals[b])
                        {
                            return AboxVerdict::Inconsistent {
                                reason: ClashReason::FunctionalCollapse {
                                    individual,
                                    role: *role,
                                    q1: quals[a],
                                    q2: quals[b],
                                },
                            };
                        }
                    }
                }
            }
        }
    }

    // P9: DisjointObjectProperties direct ABox clash.
    // For each disjoint pair (r, s), check whether any (from, to) has
    // r(from, to) and s(from, to) directly asserted (s(to, from) when `s` is
    // inverse, #534). Uses `pos`
    // (already built for P3) for O(1) lookup per triple.
    // Sound: only reports when both assertions are present verbatim —
    // no role-hierarchy expansion (a sub-role assertion implies a
    // super-role, but that direction never CREATES a new disjoint
    // violation; the violation is on the asserted roles themselves).
    // #534: the pair is signed with `r` forward. `Disjoint(r, s⁻)` forbids `r(a,b)` with
    // `s(b,a)`, so an inverse `s` looks for the reversed assertion.
    for &(r, s) in inputs.disjoint_role_pairs {
        // The collector normalises; re-normalise anyway, so a pair built elsewhere with
        // an inverse first operand cannot be read as its polarity-stripped (FP) form.
        let (r, s) = if r.is_inverse() {
            (r.flip(), s.flip())
        } else {
            (r, s)
        };
        let r_id = r.role_id();
        let s_id = s.role_id();
        for &(from, role, to) in &inputs.abox.property_assertions {
            let clash = if s.is_inverse() {
                pos.contains(&(to, s_id, from))
            } else {
                pos.contains(&(from, s_id, to))
            };
            if role == r_id && clash {
                let r = r_id;
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::DisjointRolePairViolation { r, s, from, to },
                };
            }
        }
    }

    AboxVerdict::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::ir::ConceptPool;
    use owl_dl_core::ontology::InternalOntology;
    use owl_dl_core::vocab::Vocabulary;

    #[test]
    fn skeleton_returns_unknown_for_empty_abox() {
        // Build the tiniest InternalOntology (no axioms), wrap in
        // PreparedOntology, and confirm the skeleton check returns
        // Unknown. This guards the entry-point signature; pattern
        // tests live in tests/abox_consistency.rs.
        let internal = InternalOntology {
            vocabulary: Vocabulary::default(),
            concepts: ConceptPool::default(),
            axioms: Vec::new(),
            dropped: owl_dl_core::DroppedAxioms::default(),
            builtin: Vec::new(),
            builtin_licenses: Vec::new(),
            universal_role_folds: 0,
            dkey_split_risk_keys: 0,
            collapsed_operands: owl_dl_core::CollapsedOperands::default(),
        };
        let prepared =
            crate::PreparedOntology::from_internal(internal).expect("empty ontology prepares");
        assert!(matches!(check_prepared(&prepared), AboxVerdict::Unknown));
    }

    /// Run [`check`] over a [`crate::PreparedOntology`]'s fields. Mirrors
    /// `PreparedOntology::abox_verdict`, including its `None`-closure arm
    /// (`RUSTDL_LAZY_ABOX_SATURATION` elided the saturation ⟹ provably
    /// `ABox`-free ⟹ `Unknown`).
    fn check_prepared(prepared: &crate::PreparedOntology) -> AboxVerdict {
        let Some(closure) = prepared.closure.as_ref() else {
            return AboxVerdict::Unknown;
        };
        check(&AboxCheckInputs {
            abox: &prepared.abox,
            axioms: &prepared.axioms,
            told: &prepared.told,
            pool: &prepared.pool,
            inverse_pairs: &prepared.inverse_pairs,
            hierarchy: &prepared.hierarchy,
            disjoint_role_pairs: &prepared.disjoint_role_pairs,
            closure,
        })
    }

    /// Parse an OFN string, lower it, and run the `ABox` pre-check.
    fn verdict_of(src: &str) -> AboxVerdict {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read as read_ofn;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use std::io::Cursor;
        let mut r = Cursor::new(src.to_string());
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn");
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let prepared = crate::PreparedOntology::from_internal(internal).expect("prepare");
        check_prepared(&prepared)
    }

    /// P8 positive: `isFatherOf(a,_)` (inverse of `hasFather`, range
    /// `Man`) and `isMotherOf(a,_)` (range `Woman`) force `a: Man ⊓
    /// Woman`; with `Man ⊑ ∃hasSex.Male`, `Woman ⊑ ∃hasSex.Female`,
    /// `Functional(hasSex)` and disjoint `Male`/`Female`, the single
    /// `hasSex` witness must be both → inconsistent. Exercises the
    /// inverse-derived domain/range augmentation *and* the functional
    /// collapse, the path the family target needs.
    const P8_BASE: &str = "Prefix(:=<http://t/>)\nOntology(<http://t/o>\n\
 Declaration(Class(:Man)) Declaration(Class(:Woman)) Declaration(Class(:Person))\n\
 Declaration(Class(:Male)) Declaration(Class(:Female))\n\
 Declaration(ObjectProperty(:hasSex)) Declaration(ObjectProperty(:hasFather))\n\
 Declaration(ObjectProperty(:isFatherOf)) Declaration(ObjectProperty(:hasMother))\n\
 Declaration(ObjectProperty(:isMotherOf))\n\
 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:k1)) Declaration(NamedIndividual(:k2))\n\
 EquivalentClasses(:Man ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Male)))\n\
 EquivalentClasses(:Woman ObjectIntersectionOf(:Person ObjectSomeValuesFrom(:hasSex :Female)))\n\
 InverseObjectProperties(:hasFather :isFatherOf)\n\
 InverseObjectProperties(:hasMother :isMotherOf)\n\
 ObjectPropertyRange(:hasFather :Man)\n\
 ObjectPropertyRange(:hasMother :Woman)\n\
 ObjectPropertyAssertion(:isFatherOf :a :k1)\n\
 ObjectPropertyAssertion(:isMotherOf :a :k2)\n";

    #[test]
    fn p8_functional_collapse_via_inverse_is_inconsistent() {
        let src = format!(
            "{P8_BASE} FunctionalObjectProperty(:hasSex)\n DisjointClasses(:Male :Female)\n)\n"
        );
        assert!(
            matches!(
                verdict_of(&src),
                AboxVerdict::Inconsistent {
                    reason: ClashReason::FunctionalCollapse { .. }
                }
            ),
            "P8 must fire: {:?}",
            verdict_of(&src)
        );
    }

    #[test]
    fn p8_not_functional_is_consistent() {
        // No Functional(hasSex): the two sex witnesses needn't merge.
        let src = format!("{P8_BASE} DisjointClasses(:Male :Female)\n)\n");
        assert!(
            !matches!(
                verdict_of(&src),
                AboxVerdict::Inconsistent {
                    reason: ClashReason::FunctionalCollapse { .. }
                }
            ),
            "P8 must NOT fire without a functional role"
        );
    }

    #[test]
    fn p8_non_disjoint_qualifiers_is_consistent() {
        // Male/Female not disjoint: the merged witness can be both.
        let src = format!("{P8_BASE} FunctionalObjectProperty(:hasSex)\n)\n");
        assert!(
            !matches!(
                verdict_of(&src),
                AboxVerdict::Inconsistent {
                    reason: ClashReason::FunctionalCollapse { .. }
                }
            ),
            "P8 must NOT fire when the qualifiers aren't disjoint"
        );
    }

    // ── P9: DisjointObjectProperties direct ABox clash ─────────────────

    /// P9 positive (forward–forward): `DisjointObjectProperties(:r :s)` +
    /// `r(a,b)` + `s(a,b)` → `DisjointRolePairViolation`.
    #[test]
    fn p9_forward_disjoint_roles_is_inconsistent() {
        let src = "Prefix(:=<http://t/>)\n\
                   Ontology(<http://t/p9pos>\n\
                     Declaration(ObjectProperty(:r))\n\
                     Declaration(ObjectProperty(:s))\n\
                     Declaration(NamedIndividual(:a))\n\
                     Declaration(NamedIndividual(:b))\n\
                     DisjointObjectProperties(:r :s)\n\
                     ObjectPropertyAssertion(:r :a :b)\n\
                     ObjectPropertyAssertion(:s :a :b)\n\
                   )";
        assert!(
            matches!(
                verdict_of(src),
                AboxVerdict::Inconsistent {
                    reason: ClashReason::DisjointRolePairViolation { .. }
                }
            ),
            "P9 must fire: DisjointObjectProperties(:r :s) + r(a,b) + s(a,b): {:?}",
            verdict_of(src)
        );
    }

    /// P9 negative (no disjoint axiom): `r(a,b)` + `s(a,b)` without any
    /// disjoint axiom → consistent (no clash source).
    #[test]
    fn p9_no_disjoint_axiom_is_consistent() {
        let src = "Prefix(:=<http://t/>)\n\
                   Ontology(<http://t/p9neg1>\n\
                     Declaration(ObjectProperty(:r))\n\
                     Declaration(ObjectProperty(:s))\n\
                     Declaration(NamedIndividual(:a))\n\
                     Declaration(NamedIndividual(:b))\n\
                     ObjectPropertyAssertion(:r :a :b)\n\
                     ObjectPropertyAssertion(:s :a :b)\n\
                   )";
        assert!(
            !matches!(
                verdict_of(src),
                AboxVerdict::Inconsistent {
                    reason: ClashReason::DisjointRolePairViolation { .. }
                }
            ),
            "P9 must NOT fire when there is no DisjointObjectProperties axiom"
        );
    }

    /// P9 negative (inverse-disjoint — the FP that the fix prevents):
    /// `DisjointObjectProperties(:r ObjectInverseOf(:s))` + `r(a,b)` +
    /// `s(a,b)` → consistent.
    ///
    /// The axiom forbids `r(a,b) ∧ s(b,a)`, NOT `r(a,b) ∧ s(a,b)`. A
    /// polarity-stripped `(r,s)` pair would fire here, a false positive. Since #534
    /// the pair is kept SIGNED, as `(r, s⁻)`, and P9 looks for the reversed
    /// assertion; this same-direction configuration stays `Unknown`.
    #[test]
    fn p9_inverse_disjoint_same_direction_is_consistent() {
        let src = "Prefix(:=<http://t/>)\n\
                   Ontology(<http://t/p9neg2>\n\
                     Declaration(ObjectProperty(:r))\n\
                     Declaration(ObjectProperty(:s))\n\
                     Declaration(NamedIndividual(:a))\n\
                     Declaration(NamedIndividual(:b))\n\
                     DisjointObjectProperties(:r ObjectInverseOf(:s))\n\
                     ObjectPropertyAssertion(:r :a :b)\n\
                     ObjectPropertyAssertion(:s :a :b)\n\
                   )";
        assert!(
            !matches!(
                verdict_of(src),
                AboxVerdict::Inconsistent {
                    reason: ClashReason::DisjointRolePairViolation { .. }
                }
            ),
            "P9 must NOT fire for inverse-disjoint + same-direction assertions \
             (this was the FP the fix closes): {:?}",
            verdict_of(src)
        );
    }

    /// #534: P9 on signed pairs. `true` when P9 reports a disjoint-role clash.
    fn p9_fires(axioms: &str) -> bool {
        let src = format!(
            "Prefix(:=<http://t/>)\n\
             Ontology(<http://t/p9signed>\n\
               Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
               Declaration(ObjectProperty(:u))\n\
               Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
               {axioms}\n)"
        );
        matches!(
            verdict_of(&src),
            AboxVerdict::Inconsistent {
                reason: ClashReason::DisjointRolePairViolation { .. }
            }
        )
    }

    /// `Disjoint(r, s⁻)` with `r(a,b)` and `s(b,a)`: the reversed assertion clashes.
    #[test]
    fn p9_mixed_pair_reversed_assertion_fires() {
        assert!(p9_fires(
            "DisjointObjectProperties(:r ObjectInverseOf(:s)) \
             ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :a)"
        ));
        // The operand order does not matter.
        assert!(p9_fires(
            "DisjointObjectProperties(ObjectInverseOf(:s) :r) \
             ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :a)"
        ));
    }

    /// `Disjoint(r⁻, s⁻)` is `Disjoint(r, s)`: same direction fires, reversed does not.
    #[test]
    fn p9_both_inverse_is_the_forward_pair() {
        let ax = "DisjointObjectProperties(ObjectInverseOf(:r) ObjectInverseOf(:s))";
        assert!(p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :a :b)"
        )));
        assert!(!p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :a)"
        )));
    }

    /// A three-operand axiom with one inverse: each pair keeps its own polarity.
    #[test]
    fn p9_three_operands_with_one_inverse() {
        let ax = "DisjointObjectProperties(:r ObjectInverseOf(:s) :u)";
        // (r, s⁻): reversed fires.
        assert!(p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :a)"
        )));
        // (r, u): forward, same direction fires.
        assert!(p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:u :a :b)"
        )));
        // (s⁻, u) = (s, u⁻): `s(a,b)` with `u(b,a)` fires; same direction does not.
        assert!(p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:s :a :b) ObjectPropertyAssertion(:u :b :a)"
        )));
        assert!(!p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:s :a :b) ObjectPropertyAssertion(:u :a :b)"
        )));
    }

    /// `Disjoint(r, r⁻)` is asymmetry: `r(a,b)` with `r(b,a)` fires, and so does a
    /// single `r(a,a)` (asymmetry entails irreflexivity).
    #[test]
    fn p9_asymmetry_form() {
        let ax = "DisjointObjectProperties(:r ObjectInverseOf(:r))";
        assert!(p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :b :a)"
        )));
        assert!(p9_fires(&format!("{ax} ObjectPropertyAssertion(:r :a :a)")));
        assert!(!p9_fires(&format!(
            "{ax} ObjectPropertyAssertion(:r :a :b)"
        )));
    }

    // ── #527 / #528: the arms the polarity fix added fire on the right edges ─────
    //
    // The end-to-end canaries in `tests/abox_check_role_polarity.rs` cannot pin these:
    // with `abox_check` silent, their inconsistent controls are still decided by the
    // tableau. These assert the verdict of the pass itself.

    /// [`verdict_of`], with `edit` applied to the converted IR first.
    fn verdict_with(src: &str, edit: impl FnOnce(&mut InternalOntology)) -> AboxVerdict {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read as read_ofn;
        use horned_owl::model::RcStr;
        use horned_owl::ontology::set::SetOntology;
        use std::io::Cursor;
        let mut r = Cursor::new(src.to_string());
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn");
        let mut internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        edit(&mut internal);
        let prepared = crate::PreparedOntology::from_internal(internal).expect("prepare");
        check_prepared(&prepared)
    }

    fn pol(body: &str) -> String {
        format!(
            "Prefix(:=<http://t/>)\nOntology(<http://t/pol>\n\
             Declaration(ObjectProperty(:r)) Declaration(Class(:C))\n\
             Declaration(Class(:Q1)) Declaration(Class(:Q2))\n\
             Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
             Declaration(NamedIndividual(:c))\n{body}\n)\n"
        )
    }

    fn is_functional_diff(v: &AboxVerdict) -> bool {
        matches!(
            v,
            AboxVerdict::Inconsistent {
                reason: ClashReason::FunctionalDiff { .. }
            }
        )
    }

    fn collapse_role(v: &AboxVerdict) -> Option<Role> {
        match v {
            AboxVerdict::Inconsistent {
                reason: ClashReason::FunctionalCollapse { role, .. },
            } => Some(*role),
            _ => None,
        }
    }

    const PREDS: &str = "ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :c :b) \
                         DifferentIndividuals(:a :c)";
    const SUCCS: &str = "ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :a :c) \
                         DifferentIndividuals(:b :c)";

    /// #528, P5: `Functional(r⁻)` merges the `r`-predecessors of one individual, never
    /// its `r`-successors.
    #[test]
    fn p5_functional_inverse_merges_predecessors_only() {
        let func_inv = "FunctionalObjectProperty(ObjectInverseOf(:r))";
        let v = verdict_of(&pol(&format!("{func_inv} {SUCCS}")));
        assert!(matches!(v, AboxVerdict::Unknown), "two successors: {v:?}");
        let v = verdict_of(&pol(&format!("{func_inv} {PREDS}")));
        assert!(is_functional_diff(&v), "two predecessors: {v:?}");
    }

    /// P5's `InverseFunctionalRole(r⁻)` arm (= `Functional(r)`). The converter
    /// normalises that axiom (#149), so the IR is edited by hand: `Functional(r)` is
    /// replaced by `InverseFunctional(r⁻)`.
    #[test]
    fn p5_hand_built_inverse_functional_inverse_merges_successors_only() {
        let swap = |o: &mut InternalOntology| {
            let mut n = 0;
            for ax in &mut o.axioms {
                if let owl_dl_core::ontology::Axiom::FunctionalRole(r) = ax {
                    *ax = owl_dl_core::ontology::Axiom::InverseFunctionalRole(r.flip());
                    n += 1;
                }
            }
            assert_eq!(n, 1, "the fixture must carry exactly one FunctionalRole");
        };
        let func = "FunctionalObjectProperty(:r)";
        let v = verdict_with(&pol(&format!("{func} {PREDS}")), swap);
        assert!(matches!(v, AboxVerdict::Unknown), "two predecessors: {v:?}");
        let v = verdict_with(&pol(&format!("{func} {SUCCS}")), swap);
        assert!(is_functional_diff(&v), "two successors: {v:?}");
    }

    /// #528, P8: `∃r⁻.Q1`, `∃r⁻.Q2` collapse only when `r⁻` is functional
    /// (`InverseFunctional(r)` or `Functional(r⁻)`), and `∃r` only when `r` is. The
    /// `InverseFunctional(r)`-only fixture also pins the pass's gate, which used to
    /// open on functional roles alone.
    #[test]
    fn p8_collapse_keys_on_the_signed_role() {
        let ex = |r: &str| {
            format!(
                "DisjointClasses(:Q1 :Q2) ClassAssertion(:C :a) \
                 SubClassOf(:C ObjectSomeValuesFrom({r} :Q1)) \
                 SubClassOf(:C ObjectSomeValuesFrom({r} :Q2))"
            )
        };
        let (named, inv) = (":r", "ObjectInverseOf(:r)");
        for (case, axioms) in [
            (
                "∃r⁻ under Functional(r)",
                format!("FunctionalObjectProperty(:r) {}", ex(inv)),
            ),
            (
                "∃r under Functional(r⁻)",
                format!("FunctionalObjectProperty({inv}) {}", ex(named)),
            ),
            (
                "∃r.Q1 and ∃r⁻.Q2 under Functional(r) and InverseFunctional(r)",
                "FunctionalObjectProperty(:r) InverseFunctionalObjectProperty(:r) \
                 DisjointClasses(:Q1 :Q2) ClassAssertion(:C :a) \
                 SubClassOf(:C ObjectSomeValuesFrom(:r :Q1)) \
                 SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q2))"
                    .to_string(),
            ),
        ] {
            let v = verdict_of(&pol(&axioms));
            assert!(matches!(v, AboxVerdict::Unknown), "{case}: {v:?}");
        }
        for (case, axioms) in [
            (
                "∃r⁻ under InverseFunctional(r)",
                format!("InverseFunctionalObjectProperty(:r) {}", ex(inv)),
            ),
            (
                "∃r⁻ under Functional(r⁻)",
                format!("FunctionalObjectProperty({inv}) {}", ex(inv)),
            ),
        ] {
            let v = verdict_of(&pol(&axioms));
            assert_eq!(
                collapse_role(&v).map(Role::is_inverse),
                Some(true),
                "{case}: {v:?}"
            );
        }
        // `∃r` under `Functional(r)` is inconsistent too, but the EL saturator derives
        // `C ⊑ ⊥` first, so P1 reports it (`AssertedBot`). P8's named arm is pinned by
        // `p8_functional_collapse_via_inverse_is_inconsistent`.
        let v = verdict_of(&pol(&format!("FunctionalObjectProperty(:r) {}", ex(named))));
        assert!(
            matches!(v, AboxVerdict::Inconsistent { .. }),
            "∃r under Functional(r): {v:?}"
        );
    }
}
