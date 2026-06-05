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
//! Seven clash patterns implemented incrementally (P1 direct-Bot
//! assertion, P2 disjoint types per individual, P3 NegOPA-vs-OPA,
//! P4 SameAs∩DifferentFrom, P5 Functional+two-distinct-witnesses,
//! P6 Asymmetric/Irreflexive violations, P7 domain/range as a
//! stretch).
//!
//! Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`

use crate::union_find::UnionFind;
use owl_dl_core::ir::{ClassId, IndividualId, RoleId};

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
        role: RoleId,
        q1: ClassId,
        q2: ClassId,
    },
}

/// Collect existential restrictions `∃R.Q` (atomic `Q`) that appear as
/// `sup` itself or as a top-level `And` operand of `sup`. Used by P8 to
/// read a class's functional-role existential implications straight from
/// its defining axioms (`SubClassOf` / `EquivalentClasses`).
fn existential_funcs_in(
    sup: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ir::ConceptPool,
    out: &mut Vec<(RoleId, ClassId)>,
) {
    use owl_dl_core::ir::ConceptExpr;
    match pool.get(sup) {
        ConceptExpr::Some(r, q) => {
            if let ConceptExpr::Atomic(qid) = pool.get(*q) {
                out.push((r.role_id(), *qid));
            }
        }
        ConceptExpr::And(ops) => {
            for &op in ops {
                if let ConceptExpr::Some(r, q) = pool.get(op)
                    && let ConceptExpr::Atomic(qid) = pool.get(*q)
                {
                    out.push((r.role_id(), *qid));
                }
            }
        }
        _ => {}
    }
}

/// Entry point. Runs all implemented clash patterns and returns the first
/// detected clash, or [`AboxVerdict::Unknown`] if none fire.
pub(crate) fn check(prepared: &crate::PreparedOntology) -> AboxVerdict {
    // Early return: no individuals → no ABox → no clash possible.
    if prepared.abox.individuals.is_empty() {
        return AboxVerdict::Unknown;
    }
    let closure = &prepared.closure;
    let pool = &prepared.pool;

    // P1: direct-⊥ assertion.
    for &(individual, class_concept) in &prepared.abox.class_assertions {
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
    let n = prepared.abox.individuals.len();
    let ind_index: std::collections::HashMap<owl_dl_core::ir::IndividualId, usize> = prepared
        .abox
        .individuals
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let mut types: Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>> =
        vec![std::collections::HashSet::new(); n];
    for &(individual, class_concept) in &prepared.abox.class_assertions {
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
    let told = &prepared.told;
    let told_n = told.num_classes();
    for (i, type_set) in types.iter().enumerate() {
        let (individual, _) = prepared.abox.individuals[i];
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
    )> = prepared.abox.property_assertions.iter().copied().collect();
    for &(from, role, to) in &prepared.abox.negative_property_triples {
        // Role-hierarchy downward propagation: a positive assertion on
        // any sub-role S of `role` (S ⊑ R) implies `R(a, b)` and so
        // clashes with NegOPA(R, a, b). A super-role assertion does
        // NOT imply R(a, b) and must not be flagged.
        //
        // `sub_roles(role)` returns the reflexive-transitive closure
        // (including `role` itself), so the direct-match case is
        // covered.
        for &sub_role in prepared.hierarchy.sub_roles(role) {
            if pos.contains(&(from, sub_role, to)) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::NegOpaConflict { from, role, to },
                };
            }
        }
    }

    // P4: SameAs ∩ DifferentFrom. Build union-find over individual
    // indices via same_pairs; check each different_pair against it.
    let n_ind = prepared.abox.individuals.len();
    let mut uf = UnionFind::new(n_ind);
    for &(a, b) in &prepared.abox.same_pairs {
        if let (Some(&i), Some(&j)) = (ind_index.get(&a), ind_index.get(&b)) {
            uf.union(
                u32::try_from(i).expect("ind index fits in u32"),
                u32::try_from(j).expect("ind index fits in u32"),
            );
        }
    }
    for &(a, b) in &prepared.abox.different_pairs {
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
    for ax in &prepared.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::FunctionalRole(r) => {
                functional_roles.insert(r.role_id());
            }
            owl_dl_core::ontology::Axiom::InverseFunctionalRole(r) => {
                inverse_functional_roles.insert(r.role_id());
            }
            _ => {}
        }
    }

    // Group (from, role) → Vec<to>.
    let mut by_from_role: Map<(IndividualId, RoleId), Vec<IndividualId>> = Map::new();
    for &(from, role, to) in &prepared.abox.property_assertions {
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
                for &(da, db) in &prepared.abox.different_pairs {
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
    for &(from, role, to) in &prepared.abox.property_assertions {
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
                for &(da, db) in &prepared.abox.different_pairs {
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
    for ax in &prepared.axioms {
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
    for &(from, role, to) in &prepared.abox.property_assertions {
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
    for &(from, role, to) in &prepared.abox.property_assertions {
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
    for ax in &prepared.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::ObjectPropertyDomain { role, domain } => {
                domains.push((role.role_id(), *domain));
            }
            owl_dl_core::ontology::Axiom::ObjectPropertyRange { role, range } => {
                ranges.push((role.role_id(), *range));
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
        prepared.inverse_pairs.iter().find_map(|&(a, b)| {
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
    for &(from, role, to) in &prepared.abox.property_assertions {
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
            let (individual, _) = prepared.abox.individuals[i];
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
    if !functional_roles.is_empty() {
        let mut existentials: std::collections::HashMap<ClassId, Vec<(RoleId, ClassId)>> =
            std::collections::HashMap::new();
        for ax in &prepared.axioms {
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
            let (individual, _) = prepared.abox.individuals[i];
            let mut by_role: std::collections::HashMap<RoleId, Vec<ClassId>> =
                std::collections::HashMap::new();
            for &t in type_set {
                if let Some(exs) = existentials.get(&t) {
                    for &(r, q) in exs {
                        if functional_roles.contains(&r) {
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
        };
        let prepared =
            crate::PreparedOntology::from_internal(internal).expect("empty ontology prepares");
        assert!(matches!(check(&prepared), AboxVerdict::Unknown));
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
        check(&prepared)
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
}
