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
    RuleOutcome, apply_and, apply_concept_rules, apply_exists, apply_forall, apply_max, apply_min,
    apply_nominal_assignment, apply_nominal_rules, apply_residual_gcis, apply_role_chains,
    apply_role_rules, apply_self_restriction,
};
pub use saturate::{SaturationResult, saturate};
pub use search::search;
pub use trail::{Checkpoint, TableauTrail, TrailEntry};

use std::collections::HashMap;

use owl_dl_core::{
    AbsorbedTBox, ConceptExpr, ConceptId, ConceptPool, IndividualId, Role, RoleHierarchy, RoleId,
    is_nnf,
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
    /// Declared inverse pairs: `r → s` means an `InverseObjectProperties(r, s)`
    /// axiom in the source ontology. The map is symmetric (both
    /// directions populated). Owned by the context rather than
    /// borrowed because it's small (one entry per declared pair) and
    /// avoiding a fourth lifetime keeps the API tractable.
    inverse_pairs: HashMap<RoleId, RoleId>,
    /// NNF complement table: `body → nnf(¬body)`. Populated by the
    /// reasoner facade for every `body` appearing in a
    /// `Max(_, _, body)` expression, so `apply_choose` can branch
    /// on `C` vs `¬C` without ever needing to intern at tableau
    /// time. `ConceptPool` is logically frozen during the tableau.
    complements: HashMap<ConceptId, ConceptId>,
    /// Length-2 role chain axioms `r₁ ∘ r₂ ⊑ sup`. Populated by the
    /// reasoner facade from `SubObjectPropertyOf::Chain` axioms (with
    /// length 2, named roles only) and from `TransitiveRole(r)` lowered
    /// as `(r, r, r)`. The [`apply_role_chains`] rule walks two
    /// consecutive named-role edges and adds the implied `sup` edge.
    chains: Vec<(RoleId, RoleId, RoleId)>,
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
            inverse_pairs: HashMap::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
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
            inverse_pairs: HashMap::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
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
            inverse_pairs: HashMap::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
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

    /// Declare `r` and `s` as mutual inverses (corresponding to an
    /// `InverseObjectProperties(r, s)` axiom). After this,
    /// [`Self::edge_satisfies`] will accept a cross-polarity match
    /// between `Role::Named(r)` and `Role::Inverse(s)` (or vice
    /// versa). The map is populated symmetrically.
    pub fn declare_inverse_pair(&mut self, r: RoleId, s: RoleId) -> &mut Self {
        self.inverse_pairs.insert(r, s);
        self.inverse_pairs.insert(s, r);
        self
    }

    /// True if `r` and `s` are linked by a declared
    /// `InverseObjectProperties` axiom.
    #[must_use]
    pub fn are_declared_inverses(&self, r: RoleId, s: RoleId) -> bool {
        self.inverse_pairs.get(&r) == Some(&s)
    }

    /// Register the NNF complement of `body`. Must be called before
    /// satisfiability for every `body` appearing in a `Max(_, _, body)`
    /// so [`apply_choose`] can look the complement up at branching
    /// time without mutating the pool.
    pub fn set_complement(&mut self, body: ConceptId, complement: ConceptId) -> &mut Self {
        self.complements.insert(body, complement);
        self
    }

    /// Register a length-2 role chain axiom `r₁ ∘ r₂ ⊑ sup`. The
    /// tableau's [`apply_role_chains`](crate::apply_role_chains)
    /// rule walks two consecutive named-role edges and adds the
    /// implied `sup` edge.
    pub fn declare_chain_axiom(&mut self, r1: RoleId, r2: RoleId, sup: RoleId) -> &mut Self {
        self.chains.push((r1, r2, sup));
        self
    }

    /// Slice of all registered length-2 chain axioms.
    #[must_use]
    pub fn chains(&self) -> &[(RoleId, RoleId, RoleId)] {
        &self.chains
    }

    /// Lookup the pre-registered NNF complement of `body`.
    #[must_use]
    pub fn complement_of(&self, body: ConceptId) -> Option<ConceptId> {
        self.complements.get(&body).copied()
    }

    /// True iff a role-tagged neighbour view `seen` (as produced by
    /// [`Node::neighbours`]) satisfies a `wanted` role expression
    /// from a `∀R.C` / `∃R.C` / `RoleRule`.
    ///
    /// Three regimes:
    /// 1. Same polarity — sub-role propagation on the underlying
    ///    [`RoleId`]s: an `r`-edge satisfies `∀s.C` when `r ⊑ s`,
    ///    and likewise on the inverse axis.
    /// 2. Cross polarity, linked by an `InverseObjectProperties`
    ///    declaration — match. `Role::Named(r)` satisfies
    ///    `Role::Inverse(s)` iff `r ≡ s⁻`, i.e.
    ///    `inverse_pairs[r] == Some(s)`.
    /// 3. Otherwise — no match.
    ///
    /// Falls back to plain equality on the underlying [`RoleId`]s
    /// when no hierarchy is attached, preserving the H-only
    /// semantics for callers that don't opt in.
    #[must_use]
    pub fn edge_satisfies(&self, seen: Role, wanted: Role) -> bool {
        let s = seen.role_id();
        let w = wanted.role_id();
        if seen.is_inverse() == wanted.is_inverse() {
            match self.hierarchy {
                Some(h) => h.is_sub_role(s, w),
                None => s == w,
            }
        } else {
            self.are_declared_inverses(s, w)
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

    /// Allocate a fresh successor of `from` reachable by `role`,
    /// i.e. an edge `from —role→ new`. Records both `NodeCreated`
    /// and `EdgeAdded` on the trail and stamps the new node with
    /// `parent = Some(from)`, `parent_role = Some(Role::Named(role))`
    /// for pair blocking. Also wires the in-edge `(role, from)`
    /// into the new node so inverse-aware traversal sees it.
    pub fn new_successor(&mut self, from: NodeId, role: RoleId) -> NodeId {
        let prior_len = self.graph.len();
        let id = self
            .graph
            .push_node_with_parent(Some(from), Some(Role::Named(role)));
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        self.add_edge_inner(from, role, id);
        id
    }

    /// Allocate a fresh *predecessor* of `to`: the inverse direction
    /// of [`Self::new_successor`]. The new node `new` is created
    /// with an outgoing edge `new —role→ to`. The new node's
    /// `parent` is `to` (its *creator* — pair-blocking ancestry runs
    /// through the creator), and `parent_role = Role::Inverse(role)`
    /// because the inbound generative role at the creator is `r⁻`.
    pub fn new_predecessor(&mut self, to: NodeId, role: RoleId) -> NodeId {
        let prior_len = self.graph.len();
        let id = self
            .graph
            .push_node_with_parent(Some(to), Some(Role::Inverse(role)));
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        self.add_edge_inner(id, role, to);
        id
    }

    /// Pair blocking (a.k.a. double blocking).
    ///
    /// A non-root node `y` is blocked by a tree-ancestor `x'` iff:
    ///
    /// 1. `x'` is itself non-root (has its own creator);
    /// 2. `parent_role(y) == parent_role(x')` — the creating edge
    ///    role and polarity match;
    /// 3. `L(y) ⊆ L(x')`;
    /// 4. `L(parent(y)) ⊆ L(parent(x'))`.
    ///
    /// Roots and orphan nodes always answer `false`. Naive subset
    /// blocking would only require (3) — that's unsound the moment
    /// inverse roles enter the picture, because an existential at
    /// `y` may demand a label at `parent(y)` that subset-blocking
    /// can't see. Pair blocking restores soundness for `ALCHI`.
    #[must_use]
    pub fn is_blocked(&self, y: NodeId) -> bool {
        let yn = self.graph.node(y);
        let (Some(yp_id), Some(yr)) = (yn.parent(), yn.parent_role()) else {
            return false;
        };
        let yl = yn.labels();
        let ypn = self.graph.node(yp_id);
        let ypl = ypn.labels();

        // Iterate strict tree-ancestors of y (starting at yp_id, walking
        // upward through `parent`). x' is a valid blocking candidate
        // when its own creator/role match yp/yr and the two subset
        // checks pass.
        let mut x_prime_id = yp_id;
        loop {
            let xn = self.graph.node(x_prime_id);
            if let (Some(xp_id), Some(xr)) = (xn.parent(), xn.parent_role())
                && xr == yr
            {
                let xpn = self.graph.node(xp_id);
                if is_subset_sorted(yl, xn.labels()) && is_subset_sorted(ypl, xpn.labels()) {
                    return true;
                }
            }
            match xn.parent() {
                Some(next) => x_prime_id = next,
                None => return false,
            }
        }
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

    /// Append `(role, target)` to `from`'s edge list, mirror it as
    /// `(role, from)` on `target.in_edges`, and record one trail
    /// entry covering both. Rollback pops both halves in reverse —
    /// see [`crate::TrailEntry::EdgeAdded`].
    ///
    /// Edges are not deduplicated here — distinct role assertions
    /// between the same nodes can be meaningful for cardinality
    /// reasoning later. Higher-level rules can check before adding.
    pub fn add_edge(&mut self, from: NodeId, role: RoleId, target: NodeId) {
        self.add_edge_inner(from, role, target);
    }

    fn add_edge_inner(&mut self, from: NodeId, role: RoleId, target: NodeId) {
        self.graph.node_mut(from).edges.push((role, target));
        self.graph.node_mut(target).in_edges.push((role, from));
        self.trail
            .record(TrailEntry::EdgeAdded { from, role, target });
    }

    /// Mark `a` and `b` as denoting distinct individuals. Symmetric.
    /// Idempotent: a no-op if the pair is already marked. Records a
    /// [`TrailEntry::DistinctMarked`] when the mark is fresh.
    pub fn mark_distinct(&mut self, a: NodeId, b: NodeId) {
        if a == b || self.are_distinct(a, b) {
            return;
        }
        self.graph.node_mut(a).inequalities.push(b);
        self.graph.node_mut(b).inequalities.push(a);
        self.trail.record(TrailEntry::DistinctMarked { a, b });
    }

    /// True iff `a` and `b` are known distinct via a prior
    /// [`Self::mark_distinct`].
    #[must_use]
    pub fn are_distinct(&self, a: NodeId, b: NodeId) -> bool {
        self.graph.node(a).inequalities().contains(&b)
    }

    /// Set `node` as the canonical witness for nominal `individual`.
    /// Idempotent: a no-op if already set to `node`. Records a
    /// [`TrailEntry::NominalAssigned`] when the mapping changes.
    pub fn assign_nominal(&mut self, individual: IndividualId, node: NodeId) {
        let prior = self.graph.nominals.get(&individual).copied();
        if prior == Some(node) {
            return;
        }
        self.graph.nominals.insert(individual, node);
        self.trail
            .record(TrailEntry::NominalAssigned { individual, prior });
    }

    /// Follow the merge-redirect chain for `node` until an
    /// unmerged node is reached. Returns `node` unchanged if it
    /// has no `merged_into` link.
    #[must_use]
    pub fn resolve(&self, node: NodeId) -> NodeId {
        let mut cur = node;
        while let Some(next) = self.graph.node(cur).merged_into() {
            cur = next;
        }
        cur
    }

    /// Merge `source` into `target`. After this call:
    /// - every label of `source` is also a label of `target` (or
    ///   was already);
    /// - every outgoing edge `source —r→ x` is re-anchored as
    ///   `target —r→ x`;
    /// - every incoming edge `y —r→ source` is re-anchored as
    ///   `y —r→ target`;
    /// - every distinct-mark on `source` is also a distinct-mark
    ///   on `target`;
    /// - every node whose parent was `source` now has parent
    ///   `target`;
    /// - `source.merged_into` becomes `Some(target)`.
    ///
    /// Returns `true` if the merge happened, `false` if it was
    /// rejected because `source` and `target` are already known
    /// distinct (signalling an inequality clash to the caller).
    /// All mutations are recorded on the trail so rollback restores
    /// the prior state.
    #[allow(clippy::missing_panics_doc)]
    pub fn merge_into(&mut self, source: NodeId, target: NodeId) -> bool {
        debug_assert_ne!(source, target, "merge_into: source and target must differ");
        if self.are_distinct(source, target) {
            return false;
        }
        // Snapshot source's state before mutating. We use clones so
        // the loops don't borrow the graph mutably during iteration.
        let source_labels: Vec<ConceptId> = self.graph.node(source).labels.to_vec();
        let source_out: Vec<(RoleId, NodeId)> = self.graph.node(source).edges.to_vec();
        let source_in: Vec<(RoleId, NodeId)> = self.graph.node(source).in_edges.to_vec();
        let source_ineq: Vec<NodeId> = self.graph.node(source).inequalities.to_vec();

        // 1. Replay labels on target via add_label (LabelAdded trail
        //    entries; idempotent).
        for c in source_labels {
            self.add_label(target, c);
        }

        // 2. Re-anchor outgoing edges: for each (r, x) in source.edges,
        //    remove it and add (r, x) on target. This works even when
        //    x == source (self-loop) — handled by the in_position
        //    bookkeeping.
        for (role, x) in source_out {
            // Find this edge's positions and remove.
            let from_pos = self
                .graph
                .node(source)
                .edges
                .iter()
                .position(|&e| e == (role, x))
                .expect("edge present at merge time");
            let in_pos = self
                .graph
                .node(x)
                .in_edges
                .iter()
                .position(|&e| e == (role, source))
                .expect("mirror in-edge present at merge time");
            self.remove_edge_recorded(source, role, x, from_pos, in_pos);
            // Add (role, x') where x' = x unless x was source (a
            // self-loop turns into target —r→ target).
            let new_target = if x == source { target } else { x };
            self.add_edge_inner(target, role, new_target);
        }

        // 3. Re-anchor incoming edges: each (r, y) in source.in_edges
        //    means y —r→ source exists. Remove it and add y —r→ target.
        for (role, y) in source_in {
            // y may itself be merged; resolve so we don't operate on
            // a redirect.
            let y_eff = self.resolve(y);
            if y_eff == source {
                // Self-loop already handled above.
                continue;
            }
            let from_pos = self
                .graph
                .node(y_eff)
                .edges
                .iter()
                .position(|&e| e == (role, source));
            let Some(from_pos) = from_pos else { continue };
            let in_pos = self
                .graph
                .node(source)
                .in_edges
                .iter()
                .position(|&e| e == (role, y))
                .expect("source in-edge present at merge time");
            self.remove_edge_recorded(y_eff, role, source, from_pos, in_pos);
            self.add_edge_inner(y_eff, role, target);
        }

        // 4. Carry inequalities. mark_distinct is symmetric and
        //    idempotent.
        for other in source_ineq {
            if other != target && other != source {
                self.mark_distinct(target, other);
            }
        }

        // 5. Rewrite children-parent pointers: any node whose
        //    parent equals source becomes parented at target.
        //    Iterate through the node arena. Skip the source itself.
        let node_count = self.graph.len();
        for idx in 0..node_count {
            let nid = NodeId::new(u32::try_from(idx).expect("node count fits in u32"));
            if nid == source {
                continue;
            }
            if self.graph.node(nid).parent() == Some(source) {
                let prior_parent = Some(source);
                let prior_parent_role = self.graph.node(nid).parent_role();
                self.graph.node_mut(nid).parent = Some(target);
                self.trail.record(TrailEntry::ParentRewritten {
                    node: nid,
                    prior_parent,
                    prior_parent_role,
                });
            }
        }

        // 6. Mark source as redirected.
        let prior = self.graph.node(source).merged_into();
        self.graph.node_mut(source).merged_into = Some(target);
        self.trail.record(TrailEntry::MergedRedirect {
            node: source,
            new_target: target,
            prior_redirect: prior,
        });

        true
    }

    fn remove_edge_recorded(
        &mut self,
        from: NodeId,
        role: RoleId,
        target: NodeId,
        position: usize,
        in_position: usize,
    ) {
        let removed = self.graph.node_mut(from).edges.remove(position);
        debug_assert_eq!(removed, (role, target));
        let mirror = self.graph.node_mut(target).in_edges.remove(in_position);
        debug_assert_eq!(mirror, (role, from));
        self.trail.record(TrailEntry::EdgeRemoved {
            from,
            role,
            target,
            position,
            in_position,
        });
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
                role: Role::Named(r),
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
                role: Role::Named(r),
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
                role: Role::Named(r),
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
    fn pair_blocking_fires_on_depth_two_repeat() {
        // root —r→ s1 —r→ s2.  Both s1 and s2 carry {A}.
        // Pair blocking: s2 blocked by s1 iff
        //   parent_role(s2) == parent_role(s1)  (both Named(r))   ✓
        //   L(s2) ⊆ L(s1)                        ✓ ({A} ⊆ {A})
        //   L(parent(s2)=s1) ⊆ L(parent(s1)=root) ✓
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, a);
        let s1 = ctx.new_successor(root, r);
        ctx.add_label(s1, a);
        let s2 = ctx.new_successor(s1, r);
        ctx.add_label(s2, a);
        assert!(ctx.is_blocked(s2));
    }

    #[test]
    fn pair_blocking_skips_when_parent_role_differs() {
        // root —r→ s1 —s→ s2.  Label sets match, but the creating
        // role at s2 (Named(s)) ≠ creating role at s1 (Named(r)),
        // so pair blocking refuses.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let r = RoleId::new(0);
        let s_role = RoleId::new(1);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, a);
        let s1 = ctx.new_successor(root, r);
        ctx.add_label(s1, a);
        let s2 = ctx.new_successor(s1, s_role);
        ctx.add_label(s2, a);
        assert!(!ctx.is_blocked(s2));
    }

    #[test]
    fn pair_blocking_requires_parent_subset_too() {
        // root has {A,B}, s1 has {A}, s2 has {A}, both via r.
        // L(s2) ⊆ L(s1)  ✓  but L(parent(s2)=s1)={A} ⊆ L(root)={A,B} ✓
        // so this IS blocked. Now if root only carries {A} (no B):
        //   L(s2)={A,X} ⊄ L(s1)={A} when we add X to s2 — not blocked.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let x = pool.atomic(ClassId::new(2));
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        ctx.add_label(root, a);
        let s1 = ctx.new_successor(root, r);
        ctx.add_label(s1, a);
        let s2 = ctx.new_successor(s1, r);
        ctx.add_label(s2, a);
        assert!(ctx.is_blocked(s2));
        ctx.add_label(s2, x);
        assert!(!ctx.is_blocked(s2));
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
