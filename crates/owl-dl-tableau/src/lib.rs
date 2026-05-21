//! Tableau engine for SROIQ.
//!
//! Phase 2 starts with ALC and grows to ALCHIQ (Phase 3), receives its
//! optimization stack in Phase 4, and adds nominals + complex role
//! hierarchies in Phase 5.
//!
//! ## Phase 2 commit 1: infrastructure only
//!
//! This commit lands the storage layer:
//!
//! - [`CompletionGraph`] with [`NodeId`]-indexed nodes carrying sorted
//!   label lists and edge lists.
//! - [`TableauTrail`] with log-and-undo backtracking via
//!   [`Checkpoint`] markers.
//! - [`TableauContext`] — the only sanctioned mutation interface;
//!   every label addition, edge addition, or node creation goes
//!   through it and is recorded on the trail.
//! - Clash detection: [`TableauContext::clash_in`] checks `Bot` in
//!   the label set or a complementary `c` / `Not(c)` pair.
//! - Stub [`TableauContext::is_satisfiable`] that handles only the
//!   trivial top-level shapes (`Top`, `Bot`, `Atomic`, `Nominal`,
//!   `Not(Atomic)`). Real ⊓ / ⊔ / ∃ / ∀ rules arrive in later
//!   Phase 2 commits.
//!
//! The crate compiles and tests pass, but it cannot yet decide
//! satisfiability of any non-trivial concept.

mod graph;
mod rules;
mod saturate;
mod search;
mod trail;

pub use graph::{CompletionGraph, Node, NodeId};
pub use rules::{
    RuleOutcome, apply_and, apply_concept_rules, apply_exists, apply_forall, apply_nominal_rules,
    apply_residual_gcis, apply_role_rules,
};
pub use saturate::{SaturationResult, saturate};
pub use search::search;
pub use trail::{Checkpoint, TableauTrail, TrailEntry};

use owl_dl_core::{
    AbsorbedTBox, ConceptExpr, ConceptId, ConceptPool, RoleHierarchy, RoleId, is_nnf,
};

/// Coordinator owning the completion graph and trail for one tableau
/// run.
///
/// Borrows the [`ConceptPool`] (frozen by the end of Phase 1) and,
/// optionally, an [`AbsorbedTBox`] (rules applied by the driver) plus
/// a [`RoleHierarchy`] (consulted by the `∀` / `∃` / `RoleRule` rules
/// when deciding whether an edge's role satisfies a role mentioned in
/// a label). Without a `TBox` and hierarchy the context decides
/// concept satisfiability in isolation.
///
/// All graph mutation goes through this type so the trail stays in
/// sync.
#[derive(Debug)]
pub struct TableauContext<'pool, 'tbox, 'hier> {
    pool: &'pool ConceptPool,
    tbox: Option<&'tbox AbsorbedTBox>,
    hierarchy: Option<&'hier RoleHierarchy>,
    graph: CompletionGraph,
    trail: TableauTrail,
}

impl<'pool> TableauContext<'pool, 'static, 'static> {
    /// Build a context with no `TBox` and no role hierarchy. Useful
    /// for testing individual rules and for concept-only
    /// satisfiability.
    #[must_use]
    pub fn new(pool: &'pool ConceptPool) -> Self {
        Self {
            pool,
            tbox: None,
            hierarchy: None,
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
        }
    }
}

impl<'pool, 'tbox> TableauContext<'pool, 'tbox, 'static> {
    /// Build a context that applies the rules from `tbox` during
    /// saturation, with no role hierarchy.
    #[must_use]
    pub fn with_tbox(pool: &'pool ConceptPool, tbox: &'tbox AbsorbedTBox) -> Self {
        Self {
            pool,
            tbox: Some(tbox),
            hierarchy: None,
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
        }
    }
}

impl<'pool, 'tbox, 'hier> TableauContext<'pool, 'tbox, 'hier> {
    /// Build a context with both a `TBox` and a role hierarchy.
    /// The hierarchy is consulted when matching edge roles against
    /// roles mentioned in `∀` / `∃` / `RoleRule` labels.
    #[must_use]
    pub fn with_tbox_and_hierarchy(
        pool: &'pool ConceptPool,
        tbox: &'tbox AbsorbedTBox,
        hierarchy: &'hier RoleHierarchy,
    ) -> Self {
        Self {
            pool,
            tbox: Some(tbox),
            hierarchy: Some(hierarchy),
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
        }
    }

    #[must_use]
    pub fn pool(&self) -> &ConceptPool {
        self.pool
    }

    #[must_use]
    pub fn tbox(&self) -> Option<&AbsorbedTBox> {
        self.tbox
    }

    #[must_use]
    pub fn hierarchy(&self) -> Option<&RoleHierarchy> {
        self.hierarchy
    }

    /// True iff `edge_role ⊑ wanted` under the active hierarchy.
    /// Falls back to plain equality when no hierarchy is attached,
    /// preserving Phase 2 semantics for callers that don't opt in.
    #[must_use]
    pub fn edge_satisfies(&self, edge_role: RoleId, wanted: RoleId) -> bool {
        match self.hierarchy {
            Some(h) => h.is_sub_role(edge_role, wanted),
            None => edge_role == wanted,
        }
    }

    #[must_use]
    pub fn graph(&self) -> &CompletionGraph {
        &self.graph
    }

    #[must_use]
    pub fn trail(&self) -> &TableauTrail {
        &self.trail
    }

    /// Take a checkpoint that can later be passed to [`Self::rollback_to`]
    /// to undo every mutation made after this call.
    pub fn checkpoint(&mut self) -> Checkpoint {
        self.trail.checkpoint()
    }

    /// Restore the graph to the state it had when `cp` was created.
    pub fn rollback_to(&mut self, cp: Checkpoint) {
        self.trail.rollback_to(cp, &mut self.graph);
    }

    /// Allocate a fresh root-level node and return its id. Records a
    /// [`TrailEntry::NodeCreated`] so rollback drops the node.
    ///
    /// Root nodes have `parent: None` and are never subject to
    /// subset blocking. Use [`Self::new_successor`] for `∃`-rule
    /// generation where blocking applies.
    pub fn new_node(&mut self) -> NodeId {
        let prior_len = self.graph.len();
        let id = self.graph.push_node();
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        id
    }

    /// Allocate a fresh successor of `from` reachable by `role`.
    /// Records both `NodeCreated` and `EdgeAdded` on the trail and
    /// stamps the new node with `parent = Some(from)` so subset
    /// blocking can walk the tree.
    pub fn new_successor(&mut self, from: NodeId, role: RoleId) -> NodeId {
        let prior_len = self.graph.len();
        let id = self.graph.push_node_with_parent(Some(from));
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        self.graph.node_mut(from).edges.push((role, id));
        self.trail.record(TrailEntry::EdgeAdded {
            from,
            role,
            target: id,
        });
        id
    }

    /// True if `node` has an ancestor (via the `parent` chain)
    /// whose label set is a superset of `node`'s. Root nodes
    /// (`parent: None`) are never blocked.
    ///
    /// Subset blocking is naive: O(depth · |labels|) per check.
    /// Phase 4 swaps in pairwise blocking with caches.
    #[must_use]
    pub fn is_blocked(&self, node: NodeId) -> bool {
        let my_labels = self.graph.node(node).labels();
        let mut cursor = self.graph.node(node).parent();
        while let Some(p) = cursor {
            let p_labels = self.graph.node(p).labels();
            if is_subset_sorted(my_labels, p_labels) {
                return true;
            }
            cursor = self.graph.node(p).parent();
        }
        false
    }

    /// Add concept `c` to `node`'s label list if not already present.
    ///
    /// Returns `true` if the label was newly inserted, `false` if it
    /// was already there. Records a [`TrailEntry::LabelAdded`] on
    /// insertion.
    ///
    /// `c` must be in NNF; debug-asserted at the boundary so any rule
    /// that forgets to normalize is caught in tests but pays no cost
    /// in release.
    pub fn add_label(&mut self, node: NodeId, c: ConceptId) -> bool {
        debug_assert!(
            is_nnf(c, self.pool),
            "TableauContext::add_label received non-NNF concept"
        );
        let labels = &mut self.graph.node_mut(node).labels;
        match labels.binary_search(&c) {
            Ok(_) => false,
            Err(pos) => {
                labels.insert(pos, c);
                self.trail
                    .record(TrailEntry::LabelAdded { node, concept: c });
                true
            }
        }
    }

    /// Append `(role, target)` to `from`'s edge list and record the
    /// addition on the trail.
    ///
    /// Edges are not deduplicated here — distinct role assertions
    /// between the same nodes can be meaningful for cardinality
    /// reasoning later. Higher-level rules can check before adding.
    pub fn add_edge(&mut self, from: NodeId, role: RoleId, target: NodeId) {
        self.graph.node_mut(from).edges.push((role, target));
        self.trail
            .record(TrailEntry::EdgeAdded { from, role, target });
    }

    /// Return true if `node` contains a clash:
    /// 1. `Bot` is in its label set, or
    /// 2. some concept `c` and its negation `Not(c)` are both in
    ///    its label set.
    ///
    /// This is the local clash check; later commits may add global
    /// clashes (e.g., individual identity for nominals).
    #[must_use]
    pub fn clash_in(&self, node: NodeId) -> bool {
        let labels = self.graph.node(node).labels();
        for &c in labels {
            match self.pool.get(c) {
                ConceptExpr::Bot => return true,
                ConceptExpr::Not(inner) if labels.binary_search(inner).is_ok() => return true,
                _ => {}
            }
        }
        false
    }

    /// Partial satisfiability check.
    ///
    /// Seeds a fresh root node labelled with `c` and runs the naive
    /// saturation driver. Returns:
    /// - `Some(false)` if saturation hits a clash;
    /// - `Some(true)` if saturation reaches a stable state under the
    ///   currently-wired rules;
    /// - `None` if the iteration cap was hit before settling
    ///   (defensive guard while the ruleset is incomplete).
    ///
    /// As of commit 6 the full ALC ruleset is wired: `⊓`, `⊔` (via
    /// backtracking search), `∀`, `∃` with naive subset blocking,
    /// and the four absorbed-TBox families (`ConceptRule`,
    /// `NominalRule`, `RoleRule`, residual GCI). For pure ALC with
    /// an absorbed `TBox`, verdicts are sound and complete.
    /// Phase 3 (`ALCHIQ`) and Phase 5 (nominals + complex role
    /// hierarchies) extend the ruleset further.
    pub fn is_satisfiable(&mut self, c: ConceptId) -> Option<bool> {
        const MAX_DEPTH: usize = 256;
        let root = self.new_node();
        self.add_label(root, c);
        search::search(self, MAX_DEPTH)
    }
}

/// Linear-time subset check for two ascending-sorted slices.
fn is_subset_sorted(small: &[ConceptId], big: &[ConceptId]) -> bool {
    let mut i = 0;
    let mut j = 0;
    while i < small.len() && j < big.len() {
        match small[i].cmp(&big[j]) {
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    i == small.len()
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use owl_dl_core::{
        AbsorbedTBox, ClassId, ConceptRule, IndividualId, NominalRule, Role, RoleId, RoleRule,
    };

    fn pool_with_a_and_not_a() -> (ConceptPool, ConceptId, ConceptId) {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        (pool, a, not_a)
    }

    #[test]
    fn new_node_creates_empty_node() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        assert_eq!(n.index(), 0);
        assert!(ctx.graph().node(n).labels().is_empty());
        assert!(ctx.graph().node(n).edges().is_empty());
    }

    #[test]
    fn add_label_is_idempotent_and_records_once() {
        let (pool, a, _) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let trail_len_before = ctx.trail().len();
        assert!(ctx.add_label(n, a));
        assert!(!ctx.add_label(n, a));
        assert_eq!(ctx.graph().node(n).labels(), &[a]);
        assert_eq!(ctx.trail().len(), trail_len_before + 1);
    }

    #[test]
    fn labels_stay_sorted() {
        let mut pool = ConceptPool::new();
        let c0 = pool.atomic(ClassId::new(0));
        let c1 = pool.atomic(ClassId::new(1));
        let c2 = pool.atomic(ClassId::new(2));
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, c2);
        ctx.add_label(n, c0);
        ctx.add_label(n, c1);
        let labels = ctx.graph().node(n).labels();
        let mut sorted = labels.to_vec();
        sorted.sort();
        assert_eq!(labels, sorted.as_slice());
    }

    #[test]
    fn clash_on_bot() {
        let mut pool = ConceptPool::new();
        let bot = pool.bot();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        assert!(!ctx.clash_in(n));
        ctx.add_label(n, bot);
        assert!(ctx.clash_in(n));
    }

    #[test]
    fn clash_on_complementary_pair() {
        let (pool, a, not_a) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, a);
        assert!(!ctx.clash_in(n));
        ctx.add_label(n, not_a);
        assert!(ctx.clash_in(n));
    }

    #[test]
    fn trail_round_trip_undoes_label_and_clears_clash() {
        let (pool, a, not_a) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, a);
        let cp = ctx.checkpoint();
        ctx.add_label(n, not_a);
        assert!(ctx.clash_in(n));
        ctx.rollback_to(cp);
        assert!(!ctx.clash_in(n));
        assert_eq!(ctx.graph().node(n).labels(), &[a]);
    }

    #[test]
    fn rollback_drops_nodes_created_after_checkpoint() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        let n0 = ctx.new_node();
        let cp = ctx.checkpoint();
        let _n1 = ctx.new_node();
        let _n2 = ctx.new_node();
        assert_eq!(ctx.graph().len(), 3);
        ctx.rollback_to(cp);
        assert_eq!(ctx.graph().len(), 1);
        assert_eq!(n0.index(), 0);
    }

    #[test]
    fn rollback_undoes_edge_addition() {
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        let from = ctx.new_node();
        let to = ctx.new_node();
        let cp = ctx.checkpoint();
        ctx.add_edge(from, RoleId::new(0), to);
        assert_eq!(ctx.graph().node(from).edges().len(), 1);
        ctx.rollback_to(cp);
        assert!(ctx.graph().node(from).edges().is_empty());
    }

    fn check_sat(pool: &ConceptPool, c: ConceptId) -> Option<bool> {
        let mut ctx = TableauContext::new(pool);
        ctx.is_satisfiable(c)
    }

    #[test]
    fn satisfiable_trivial_shapes() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let a = pool.atomic(ClassId::new(0));
        let self_r = pool.self_restriction(Role::named(RoleId::new(0)));
        assert_eq!(check_sat(&pool, top), Some(true));
        assert_eq!(check_sat(&pool, a), Some(true));
        assert_eq!(check_sat(&pool, self_r), Some(true));
    }

    #[test]
    fn unsatisfiable_bot() {
        let mut pool = ConceptPool::new();
        let bot = pool.bot();
        assert_eq!(check_sat(&pool, bot), Some(false));
    }

    #[test]
    fn and_rule_decomposes_conjunction() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let a_and_b = pool.and([a, b]);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, a_and_b);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(root).has_label(a));
        assert!(ctx.graph().node(root).has_label(b));
    }

    #[test]
    fn unsatisfiable_a_and_not_a() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let conj = pool.and([a, not_a]);
        assert_eq!(check_sat(&pool, conj), Some(false));
    }

    #[test]
    fn forall_propagates_to_successor() {
        // L(x) = {∀R.A}, x —R→ y  ⇒  L(y) gets A.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let r = RoleId::new(0);
        let forall_r_a = pool.all(Role::named(r), a);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_label(x, forall_r_a);
        ctx.add_edge(x, r, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(y).has_label(a));
    }

    #[test]
    fn forall_skips_other_roles() {
        // L(x) = {∀R.A}, x —S→ y with S ≠ R  ⇒  L(y) stays empty.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let r = RoleId::new(0);
        let s = RoleId::new(1);
        let forall_r_a = pool.all(Role::named(r), a);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_label(x, forall_r_a);
        ctx.add_edge(x, s, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(!ctx.graph().node(y).has_label(a));
    }

    #[test]
    fn forall_clash_via_propagated_label() {
        // L(x) = {∀R.A}, L(y) = {¬A}, x —R→ y  ⇒  clash at y after
        // propagation.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let r = RoleId::new(0);
        let forall_r_a = pool.all(Role::named(r), a);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_label(x, forall_r_a);
        ctx.add_label(y, not_a);
        ctx.add_edge(x, r, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Clash(y));
    }

    #[test]
    fn forall_composes_with_and() {
        // L(x) = {∀R.(A ⊓ B)}, x —R→ y  ⇒  L(y) ends with {A⊓B, A, B}
        // after one ⊓ decomposition at y.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let a_and_b = pool.and([a, b]);
        let r = RoleId::new(0);
        let forall_r_ab = pool.all(Role::named(r), a_and_b);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_label(x, forall_r_ab);
        ctx.add_edge(x, r, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(y).has_label(a));
        assert!(ctx.graph().node(y).has_label(b));
    }

    #[test]
    fn and_rule_decomposes_nested_conjunction() {
        // (A ⊓ B) ⊓ (C ⊓ Not(A)) — the inner conjunctions are
        // flattened by ConceptPool::and, but this test guards the
        // saturation path that finds A and Not(A) co-resident at the
        // root.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let c = pool.atomic(ClassId::new(2));
        let not_a = pool.not(a);
        let left = pool.and([a, b]);
        let right = pool.and([c, not_a]);
        let conj = pool.and([left, right]);
        assert_eq!(check_sat(&pool, conj), Some(false));
    }

    #[test]
    fn concept_rule_fires_on_atomic_label() {
        // A ⊑ B, L(x) = {A}  ⇒  B added to L(x).
        let mut pool = ConceptPool::new();
        let a_class = ClassId::new(0);
        let a = pool.atomic(a_class);
        let b = pool.atomic(ClassId::new(1));
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: a_class,
                conclusion: b,
            }],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        ctx.add_label(x, a);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(x).has_label(b));
    }

    #[test]
    fn concept_rule_unsat_via_chained_trigger() {
        // A ⊑ B, B ⊑ ¬A  ⇒  any model containing A is unsatisfiable.
        let mut pool = ConceptPool::new();
        let a_class = ClassId::new(0);
        let b_class = ClassId::new(1);
        let a = pool.atomic(a_class);
        let b = pool.atomic(b_class);
        let not_a = pool.not(a);
        let tbox = AbsorbedTBox {
            concept_rules: vec![
                ConceptRule {
                    trigger: a_class,
                    conclusion: b,
                },
                ConceptRule {
                    trigger: b_class,
                    conclusion: not_a,
                },
            ],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        ctx.add_label(x, a);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Clash(x));
    }

    #[test]
    fn nominal_rule_fires_on_nominal_label() {
        let mut pool = ConceptPool::new();
        let ind = IndividualId::new(0);
        let nominal = pool.nominal(ind);
        let b = pool.atomic(ClassId::new(0));
        let tbox = AbsorbedTBox {
            nominal_rules: vec![NominalRule {
                individual: ind,
                conclusion: b,
            }],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        ctx.add_label(x, nominal);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(x).has_label(b));
    }

    #[test]
    fn role_rule_unguarded_fires_on_every_edge() {
        // ⊤ ⊑ ∀R.C absorbed to RoleRule { role: R, guard: None,
        // target_label: C }. x —R→ y  ⇒  C ∈ L(y).
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(0));
        let tbox = AbsorbedTBox {
            role_rules: vec![RoleRule {
                role: r,
                guard: None,
                target_label: c,
            }],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_edge(x, r, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(y).has_label(c));
    }

    #[test]
    fn role_rule_guarded_skips_when_guard_absent() {
        // A ⊑ ∀R.C absorbed to RoleRule { role: R, guard: Some(A),
        // target_label: C }. L(x) = {} (no guard), x —R→ y  ⇒  C ∉ L(y).
        let mut pool = ConceptPool::new();
        let a_class = ClassId::new(0);
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(1));
        let tbox = AbsorbedTBox {
            role_rules: vec![RoleRule {
                role: r,
                guard: Some(a_class),
                target_label: c,
            }],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_edge(x, r, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(!ctx.graph().node(y).has_label(c));
    }

    #[test]
    fn role_rule_guarded_fires_when_guard_present() {
        let mut pool = ConceptPool::new();
        let a_class = ClassId::new(0);
        let a = pool.atomic(a_class);
        let r = RoleId::new(0);
        let c = pool.atomic(ClassId::new(1));
        let tbox = AbsorbedTBox {
            role_rules: vec![RoleRule {
                role: r,
                guard: Some(a_class),
                target_label: c,
            }],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_label(x, a);
        ctx.add_edge(x, r, y);
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(y).has_label(c));
    }

    #[test]
    fn or_satisfied_by_first_disjunct() {
        // A ⊔ B is satisfiable; search picks A.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let or = pool.or([a, b]);
        assert_eq!(check_sat(&pool, or), Some(true));
    }

    #[test]
    fn or_with_first_disjunct_unsat_backtracks_to_second() {
        // (A ⊓ ¬A) ⊔ B — first disjunct clashes, search must
        // rollback and try the second.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let b = pool.atomic(ClassId::new(1));
        let bad = pool.and([a, not_a]);
        let or = pool.or([bad, b]);
        assert_eq!(check_sat(&pool, or), Some(true));
    }

    #[test]
    fn or_all_disjuncts_unsat_returns_false() {
        // (A ⊓ ¬A) ⊔ (B ⊓ ¬B) — every branch clashes.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let b = pool.atomic(ClassId::new(1));
        let not_b = pool.not(b);
        let bad_a = pool.and([a, not_a]);
        let bad_b = pool.and([b, not_b]);
        let or = pool.or([bad_a, bad_b]);
        assert_eq!(check_sat(&pool, or), Some(false));
    }

    #[test]
    fn or_resolved_implicitly_by_deterministic_rule() {
        // (A ⊔ B) ⊓ A — ⊓ adds A; the disjunction is then closed
        // by the existing A label without an explicit branch.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let or = pool.or([a, b]);
        let conj = pool.and([or, a]);
        assert_eq!(check_sat(&pool, conj), Some(true));
    }

    #[test]
    fn or_closed_when_a_disjunct_already_present() {
        // L(x) = {A, A ⊔ B} — no open disjunction; saturate &
        // search return Stable / Some(true) without branching.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let or = pool.or([a, b]);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        ctx.add_label(x, a);
        ctx.add_label(x, or);
        let initial_label_count = ctx.graph().node(x).labels().len();
        // search should succeed with no additional labels added.
        let result = search::search(&mut ctx, 16);
        assert_eq!(result, Some(true));
        assert_eq!(ctx.graph().node(x).labels().len(), initial_label_count);
    }

    #[test]
    fn or_with_forall_clash_backtracks() {
        // ∀R.(A ⊓ ¬A) ⊔ ⊤ — the first disjunct propagates a clash
        // to the R-successor; backtrack and the second succeeds.
        // We construct this manually since check_sat doesn't set
        // up edges.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let r = RoleId::new(0);
        let bad = pool.and([a, not_a]);
        let bad_forall = pool.all(Role::named(r), bad);
        let top = pool.top();
        let or = pool.or([bad_forall, top]);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_edge(x, r, y);
        ctx.add_label(x, or);
        let result = search::search(&mut ctx, 32);
        assert_eq!(result, Some(true));
        // Confirm ⊤ wound up in L(x) — the chosen disjunct — not
        // the bad ∀R.…
        assert!(ctx.graph().node(x).has_label(top));
        assert!(!ctx.graph().node(x).has_label(bad_forall));
    }

    #[test]
    fn exists_creates_successor_with_body() {
        // ∃R.A is satisfiable; saturate generates one R-successor
        // labelled with A.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let r = RoleId::new(0);
        let some_r_a = pool.some(Role::named(r), a);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, some_r_a);
        let result = saturate(&mut ctx, 64);
        assert_eq!(result, SaturationResult::Stable);
        assert_eq!(ctx.graph().len(), 2);
        let succ = ctx.graph().node(root).edges()[0].1;
        assert!(ctx.graph().node(succ).has_label(a));
        assert_eq!(ctx.graph().node(succ).parent(), Some(root));
    }

    #[test]
    fn exists_reuses_existing_witness() {
        // L(x) = {∃R.A}, x already has an R-successor y with A.
        // The rule should not create a new node.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let r = RoleId::new(0);
        let some_r_a = pool.some(Role::named(r), a);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        let y = ctx.new_node();
        ctx.add_edge(x, r, y);
        ctx.add_label(y, a);
        ctx.add_label(x, some_r_a);
        let result = saturate(&mut ctx, 64);
        assert_eq!(result, SaturationResult::Stable);
        assert_eq!(ctx.graph().len(), 2);
    }

    #[test]
    fn exists_clash_in_successor_propagates_unsat() {
        // ∃R.(A ⊓ ¬A) — successor clashes; concept is unsat.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let bad = pool.and([a, not_a]);
        let r = RoleId::new(0);
        let some_r_bad = pool.some(Role::named(r), bad);
        assert_eq!(check_sat(&pool, some_r_bad), Some(false));
    }

    #[test]
    fn exists_terminates_on_cyclic_tbox_via_blocking() {
        // A ⊑ ∃R.A — naively loops forever. With subset blocking,
        // the second-level successor is blocked by the root and the
        // search terminates with Some(true).
        let mut pool = ConceptPool::new();
        let a_class = ClassId::new(0);
        let a = pool.atomic(a_class);
        let r = RoleId::new(0);
        let some_r_a = pool.some(Role::named(r), a);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: a_class,
                conclusion: some_r_a,
            }],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        assert_eq!(ctx.is_satisfiable(a), Some(true));
        // Should have at most 2 nodes after blocking kicks in:
        // root (labelled A, ∃R.A) and one R-successor (labelled A,
        // ∃R.A) blocked by the root.
        assert!(ctx.graph().len() <= 4);
    }

    #[test]
    fn exists_with_forall_propagation_into_successor() {
        // ∃R.A ⊓ ∀R.B — the existential's witness must also pick
        // up B from the ∀. Successor ends with {A, B}.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let r = RoleId::new(0);
        let some_r_a = pool.some(Role::named(r), a);
        let all_r_b = pool.all(Role::named(r), b);
        let conj = pool.and([some_r_a, all_r_b]);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, conj);
        let result = saturate(&mut ctx, 64);
        assert_eq!(result, SaturationResult::Stable);
        // The R-successor must have both A and B.
        let succ = ctx.graph().node(root).edges()[0].1;
        assert!(ctx.graph().node(succ).has_label(a));
        assert!(ctx.graph().node(succ).has_label(b));
    }

    #[test]
    fn exists_with_forall_clash_unsat() {
        // ∃R.A ⊓ ∀R.¬A — witness gets A then ¬A; clashes.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let not_a = pool.not(a);
        let r = RoleId::new(0);
        let some_r_a = pool.some(Role::named(r), a);
        let all_r_not_a = pool.all(Role::named(r), not_a);
        let conj = pool.and([some_r_a, all_r_not_a]);
        assert_eq!(check_sat(&pool, conj), Some(false));
    }

    #[test]
    fn is_blocked_root_is_never_blocked() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        ctx.add_label(x, a);
        assert!(!ctx.is_blocked(x));
    }

    #[test]
    fn is_blocked_when_labels_are_subset_of_parent() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let c = pool.atomic(ClassId::new(2));
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, a);
        ctx.add_label(root, b);
        let succ = ctx.new_successor(root, r);
        ctx.add_label(succ, a);
        assert!(ctx.is_blocked(succ));
        ctx.add_label(succ, b);
        assert!(ctx.is_blocked(succ));
        // Add a label not on the parent — successor escapes the
        // subset and is no longer blocked.
        ctx.add_label(succ, c);
        assert!(!ctx.is_blocked(succ));
    }

    #[test]
    fn residual_gci_applies_to_every_node() {
        // Residual ⊤ ⊑ B: every node ends up with B.
        let mut pool = ConceptPool::new();
        let b = pool.atomic(ClassId::new(0));
        let tbox = AbsorbedTBox {
            residual_gcis: vec![b],
            ..AbsorbedTBox::default()
        };
        let mut ctx = TableauContext::with_tbox(&pool, &tbox);
        let x = ctx.new_node();
        let y = ctx.new_node();
        let result = saturate(&mut ctx, 16);
        assert_eq!(result, SaturationResult::Stable);
        assert!(ctx.graph().node(x).has_label(b));
        assert!(ctx.graph().node(y).has_label(b));
    }
}
