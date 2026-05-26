//! Signature-based locality analysis — Phase 1 scaffolding.
//!
//! See [`docs/module-extraction-plan.md`](../../docs/module-extraction-plan.md)
//! for the design. This module ships the data structure and
//! computation; integration with the classify orchestrator lands
//! in Phase 2.
//!
//! ## Theorem (sound under-approximation of non-subsumption)
//!
//! Let `sig(C)` be the connected component of `C` in the
//! co-occurrence graph of class IRIs mentioned in `TBox` axioms.
//! If `sig(A) ≠ sig(B)` and `B ≢ ⊤`, then `A ⊑ B` is not
//! entailed by the ontology — assuming `A` is satisfiable.
//!
//! Proof sketch: pick a model `M` of the KB with `A`-non-empty.
//! Construct `M'` from `M` by removing every individual from
//! every class in `sig(B) ∖ sig(A)` and from every role-IRI in
//! `sig(B) ∖ sig(A)`. By construction `M'` still satisfies every
//! axiom (because every axiom only touches IRIs from one
//! connected component, and the IRIs we changed all belong to
//! components other than `sig(A)`'s). In `M'`, `A` is still
//! non-empty (its component wasn't touched) but `B` is empty.
//! Hence `A ⊑ B` does not hold.

use std::collections::HashSet;

use crate::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool};
use crate::ontology::Axiom;

/// Per-class component assignment. `component_of[C]` returns the
/// id of the connected component `C` belongs to in the
/// co-occurrence graph. Two classes are guaranteed
/// non-subsumed (in either direction) if they have distinct
/// component ids and neither is `⊤`/`⊥`.
#[derive(Clone, Debug, Default)]
pub struct LocalityPartition {
    /// `component_of[i]` = component id of class with `index() == i`.
    /// `u32::MAX` means "class not mentioned in any `TBox` axiom" —
    /// such a class forms its own degenerate singleton component
    /// for the purposes of the disjointness check.
    component_of: Vec<u32>,
}

impl LocalityPartition {
    /// Compute the partition by walking every `TBox` axiom and
    /// unioning the classes it mentions.
    #[must_use]
    pub fn build(axioms: &[Axiom], pool: &ConceptPool, num_classes: usize) -> Self {
        let mut uf = UnionFind::new(num_classes);
        for axiom in axioms {
            let mut bucket: Vec<ClassId> = Vec::new();
            collect_classes_in_axiom(axiom, pool, &mut bucket);
            // Skip the singleton case — nothing to union.
            if bucket.len() < 2 {
                continue;
            }
            let root = bucket[0];
            for &c in &bucket[1..] {
                uf.union(root.index() as usize, c.index() as usize);
            }
        }
        Self {
            component_of: (0..num_classes)
                .map(|i| u32::try_from(uf.find(i)).expect("class count fits in u32"))
                .collect(),
        }
    }

    /// Component id for `c`. Classes not mentioned in any axiom
    /// keep their own index as their component (each is its own
    /// singleton), which is the correct semantic for the
    /// disjointness check.
    #[must_use]
    pub fn component(&self, c: ClassId) -> u32 {
        let idx = c.index() as usize;
        self.component_of.get(idx).copied().unwrap_or(c.index())
    }

    /// True iff `a` and `b` belong to different connected
    /// components — sufficient to conclude `a ⋢ b` (and `b ⋢ a`)
    /// modulo the `⊤` / `⊥` special cases the caller must handle
    /// upstream.
    #[must_use]
    pub fn definitely_disjoint(&self, a: ClassId, b: ClassId) -> bool {
        self.component(a) != self.component(b)
    }

    /// Number of distinct components. Diagnostic only; not used
    /// on the hot path.
    #[must_use]
    pub fn num_components(&self) -> usize {
        let mut seen: std::collections::HashSet<u32> =
            std::collections::HashSet::with_capacity(self.component_of.len());
        for &c in &self.component_of {
            seen.insert(c);
        }
        seen.len()
    }
}

/// Walk every `ConceptId` an axiom references and push the
/// underlying `ClassId`s into `out`. Roles are ignored — the
/// co-occurrence graph is over class IRIs only (role hierarchy
/// is its own structure and doesn't enter the partition).
fn collect_classes_in_axiom(axiom: &Axiom, pool: &ConceptPool, out: &mut Vec<ClassId>) {
    match axiom {
        Axiom::SubClassOf { sub, sup } => {
            collect_classes_in_concept(*sub, pool, out);
            collect_classes_in_concept(*sup, pool, out);
        }
        Axiom::EquivalentClasses(members) | Axiom::DisjointClasses(members) => {
            for &c in members {
                collect_classes_in_concept(c, pool, out);
            }
        }
        Axiom::DisjointUnion { class, members } => {
            out.push(*class);
            for &m in members {
                collect_classes_in_concept(m, pool, out);
            }
        }
        Axiom::ObjectPropertyDomain { domain, .. } => {
            collect_classes_in_concept(*domain, pool, out);
        }
        Axiom::ObjectPropertyRange { range, .. } => {
            collect_classes_in_concept(*range, pool, out);
        }
        // RBox-only axioms (role chains, characteristics, etc.)
        // don't mention class IRIs and so contribute nothing.
        // ABox axioms similarly don't link classes via shared
        // signature — `ClassAssertion(C, a)` connects `a` to `C`'s
        // existing component but `a` isn't a class.
        Axiom::ClassAssertion { class, .. } => {
            collect_classes_in_concept(*class, pool, out);
        }
        _ => {}
    }
    // Sort + dedup so the caller's union loop doesn't pay for
    // repeat ids from structural sharing in deep concept trees.
    out.sort_unstable_by_key(|c: &ClassId| c.index());
    out.dedup();
}

/// Recursively walk a `ConceptExpr` collecting every atomic
/// `ClassId` mention. Avoids re-visiting the same `ConceptId`
/// via a small memo set — concept expressions are DAGs with
/// structural sharing.
fn collect_classes_in_concept(c: ConceptId, pool: &ConceptPool, out: &mut Vec<ClassId>) {
    let mut memo: HashSet<u32> = HashSet::new();
    collect_classes_in_concept_inner(c, pool, out, &mut memo);
}

fn collect_classes_in_concept_inner(
    c: ConceptId,
    pool: &ConceptPool,
    out: &mut Vec<ClassId>,
    memo: &mut HashSet<u32>,
) {
    if !memo.insert(c.index()) {
        return;
    }
    match pool.get(c) {
        ConceptExpr::Top
        | ConceptExpr::Bot
        | ConceptExpr::Nominal(_)
        | ConceptExpr::SelfRestriction(_) => {}
        ConceptExpr::Atomic(id) => out.push(*id),
        ConceptExpr::Not(inner) => collect_classes_in_concept_inner(*inner, pool, out, memo),
        ConceptExpr::And(args) | ConceptExpr::Or(args) => {
            for &a in args.as_ref() {
                collect_classes_in_concept_inner(a, pool, out, memo);
            }
        }
        ConceptExpr::Some(_, inner)
        | ConceptExpr::All(_, inner)
        | ConceptExpr::Min(_, _, inner)
        | ConceptExpr::Max(_, _, inner) => {
            collect_classes_in_concept_inner(*inner, pool, out, memo);
        }
    }
}

/// Standard union-find with path compression. Small enough to
/// inline rather than pulling in a dep.
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            let p = self.parent[x];
            self.parent[x] = self.parent[p];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] = self.rank[ra].saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::ir::ConceptPool;

    /// Two disjoint `SubClassOf` chains form two components.
    #[test]
    fn two_disjoint_chains_have_distinct_components() {
        let mut pool = ConceptPool::new();
        let a = ClassId::new(0);
        let b = ClassId::new(1);
        let c = ClassId::new(2);
        let d = ClassId::new(3);
        let a_e = pool.atomic(a);
        let b_e = pool.atomic(b);
        let c_e = pool.atomic(c);
        let d_e = pool.atomic(d);
        let axioms = vec![
            Axiom::SubClassOf { sub: a_e, sup: b_e },
            Axiom::SubClassOf { sub: c_e, sup: d_e },
        ];
        let p = LocalityPartition::build(&axioms, &pool, 4);
        assert!(!p.definitely_disjoint(a, b));
        assert!(!p.definitely_disjoint(c, d));
        assert!(p.definitely_disjoint(a, c));
        assert!(p.definitely_disjoint(a, d));
        assert!(p.definitely_disjoint(b, c));
        assert!(p.definitely_disjoint(b, d));
        assert_eq!(p.num_components(), 2);
    }

    /// A bridging axiom merges previously-disjoint components.
    #[test]
    fn bridging_axiom_merges_components() {
        let mut pool = ConceptPool::new();
        let a = ClassId::new(0);
        let b = ClassId::new(1);
        let c = ClassId::new(2);
        let d = ClassId::new(3);
        let a_e = pool.atomic(a);
        let b_e = pool.atomic(b);
        let c_e = pool.atomic(c);
        let d_e = pool.atomic(d);
        let axioms = vec![
            Axiom::SubClassOf { sub: a_e, sup: b_e },
            Axiom::SubClassOf { sub: c_e, sup: d_e },
            // `b ⊑ c` bridges the two chains.
            Axiom::SubClassOf { sub: b_e, sup: c_e },
        ];
        let p = LocalityPartition::build(&axioms, &pool, 4);
        assert_eq!(p.num_components(), 1);
        assert!(!p.definitely_disjoint(a, d));
    }

    /// Classes that don't appear in any axiom keep their own
    /// degenerate-singleton component id.
    #[test]
    fn unmentioned_class_is_its_own_component() {
        let mut pool = ConceptPool::new();
        let a = ClassId::new(0);
        let b = ClassId::new(1);
        let isolated = ClassId::new(2);
        let a_e = pool.atomic(a);
        let b_e = pool.atomic(b);
        let axioms = vec![Axiom::SubClassOf { sub: a_e, sup: b_e }];
        let p = LocalityPartition::build(&axioms, &pool, 3);
        assert!(!p.definitely_disjoint(a, b));
        assert!(p.definitely_disjoint(a, isolated));
        assert!(p.definitely_disjoint(b, isolated));
    }

    /// Deeply-nested concept expressions are walked recursively.
    /// `A ⊑ ∃R.(B ⊓ ¬C)` unions all three.
    #[test]
    fn nested_concept_unions_all_mentioned_classes() {
        let mut pool = ConceptPool::new();
        let a = ClassId::new(0);
        let b = ClassId::new(1);
        let c = ClassId::new(2);
        let a_e = pool.atomic(a);
        let b_e = pool.atomic(b);
        let c_e = pool.atomic(c);
        let not_c = pool.not(c_e);
        let b_and_not_c = pool.and([b_e, not_c]);
        let role = crate::ir::Role::Named(crate::ir::RoleId::new(0));
        let exists = pool.some(role, b_and_not_c);
        let axioms = vec![Axiom::SubClassOf {
            sub: a_e,
            sup: exists,
        }];
        let p = LocalityPartition::build(&axioms, &pool, 3);
        assert_eq!(p.num_components(), 1);
        assert!(!p.definitely_disjoint(a, b));
        assert!(!p.definitely_disjoint(a, c));
    }
}
