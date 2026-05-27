//! Tableau engine for SROIQ.
//!
//! ## What this engine covers
//!
//! Concept satisfiability under a `TBox` / `ABox` / role hierarchy
//! over the full SROIQ surface implemented today:
//!
//! - ALC core: `⊓`, `⊔`, `∀`, `∃`, residual GCIs.
//! - ALCH: role hierarchy and inverse-role pair-blocking.
//! - ALCHIQ: qualified `≥n` / `≤n` cardinality with successor
//!   merging, plus the choose rule.
//! - SROIQ extras: nominals (`{a}`) and individual merging,
//!   `ObjectHasSelf`, `Reflexive` / `Irreflexive` / `Asymmetric`
//!   characteristics, `DisjointObjectProperties`, length-2 role
//!   chains, `TransitiveObjectProperty`, full `ABox` (class
//!   assertions, property assertions, negative property assertions,
//!   `SameIndividual`, `DifferentIndividuals`).
//!
//! ## Core types
//!
//! - [`CompletionGraph`] with [`NodeId`]-indexed nodes carrying
//!   sorted label lists, outgoing/incoming edge lists, inequality
//!   marks, and `merged_into` redirects.
//! - [`TableauTrail`] with log-and-undo backtracking via
//!   [`Checkpoint`] markers.
//! - [`TableauContext`] — the only sanctioned mutation interface;
//!   every label addition, edge addition, node creation, distinct
//!   mark, nominal assignment, and merge goes through it and is
//!   recorded on the trail.
//! - Clash detection: [`TableauContext::clash_in`] checks `Bot` in
//!   the label set or a complementary `c` / `Not(c)` pair.
//! - [`saturate`] — fixed-point rule sweep; [`search`] — the
//!   backtracking driver for `⊔` and the choose rule.
//!
//! ## Out of scope for now
//!
//! - Role chains of length ≠ 2 and any chain involving an inverse
//!   role (rejected upstream as `RoleChainUnsupported`).
//! - Datatypes (the `owl-dl-datatypes` crate is scaffolded but not
//!   wired into clash detection yet).
//! - Phase 4 optimisation stack (dependency-directed backtracking,
//!   lazy unfolding integration, priority queue over rules).

#[cfg(feature = "counters")]
mod counters;
mod deps;
mod graph;
pub mod hyper;
mod rules;
mod saturate;
mod search;
mod trail;

/// Bump `ctx.counters.$field` by 1 under the `counters` feature.
/// No-op otherwise (the macro body is removed at preprocessing time
/// via `cfg`). Defined at the crate root so callers can write
/// `crate::bump_counter!(ctx, apply_and)` regardless of feature state.
#[macro_export]
macro_rules! bump_counter {
    ($ctx:expr, $field:ident) => {{
        #[cfg(feature = "counters")]
        $crate::counters::inc(&$ctx.counters.$field);
    }};
}

/// Add `n` to `ctx.counters.$field` under the `counters` feature.
/// No-op otherwise.
#[macro_export]
macro_rules! add_counter {
    ($ctx:expr, $field:ident, $n:expr) => {{
        #[cfg(feature = "counters")]
        $crate::counters::add(&$ctx.counters.$field, $n);
    }};
}

pub use graph::{CompletionGraph, DepSet, Node, NodeId};
pub use rules::{
    RuleOutcome, apply_and, apply_concept_rules, apply_deferred_concept_or_rules,
    apply_deferred_or_residuals, apply_exists, apply_forall, apply_max, apply_min,
    apply_nominal_assignment, apply_nominal_rules, apply_residual_gcis, apply_role_axioms,
    apply_role_chains, apply_role_rules, apply_self_restriction,
};
pub use saturate::{SaturationResult, saturate, verify_node_local_clash};
pub use search::{SearchVerdict, search};
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
    /// Declared inverse pairs: `(r, s)` means an `InverseObjectProperties(r, s)`
    /// axiom in the source ontology. Stored symmetrically (both
    /// `(r, s)` and `(s, r)` are pushed) so `are_declared_inverses`
    /// is a single linear scan. `Vec` rather than `HashMap` because real
    /// ontologies declare ≤ a handful of inverses (pizza: 3 pairs →
    /// 6 entries) where linear scan on `u32` equality beats hashing
    /// by an order of magnitude — `are_declared_inverses` was the
    /// second-hottest frame in pizza flamegraphs (14 %, dominated
    /// by `make_hash<RoleId>` / `find_inner`).
    inverse_pairs: Vec<(RoleId, RoleId)>,
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
    chains: Vec<(Role, Role, Role)>,
    /// Roles declared `AsymmetricObjectProperty`. The
    /// [`apply_role_axioms`] rule flags `⊥` at any node that has both
    /// an outgoing `r`-edge and an incoming `r`-edge to/from the same
    /// neighbour (i.e. both `node —r→ x` and `x —r→ node`).
    asymmetric_roles: Vec<RoleId>,
    /// Pairs of roles declared mutually disjoint (decomposed from
    /// `DisjointObjectProperties` n-ary axioms). The
    /// [`apply_role_axioms`] rule flags `⊥` when a node has both an
    /// `r`-edge and an `s`-edge to the same neighbour.
    disjoint_role_pairs: Vec<(RoleId, RoleId)>,
    graph: CompletionGraph,
    trail: TableauTrail,
    /// Optional wall-clock deadline. When set, [`crate::search`] checks
    /// it at every node and abandons the search (returning `None`) the
    /// moment it elapses. Lets a caller cap a single satisfiability
    /// probe without resorting to OS-thread cancellation. Inspected
    /// via [`Self::deadline_reached`].
    deadline: Option<std::time::Instant>,
    /// Set to `true` by [`crate::search`] the first time it notices
    /// `deadline` has elapsed. Sticky — callers read this *after*
    /// search returns to distinguish "depth-limited" from "timed out".
    deadline_hit: bool,
    /// Stack of `branch_id`s currently in scope (outer-most first).
    /// [`crate::search::branch`] pushes a fresh id when it enters a
    /// disjunction and pops it on the way out. Rules use the stack
    /// to tag derived labels' [`crate::DepSet`]s with the active
    /// branch decisions. Phase 4 DDB — see
    /// `docs/phase4-backjumping-plan.md`.
    active_branches: Vec<u32>,
    /// Monotonic counter handing out the next `branch_id`. Reset to
    /// zero per tableau run; lives as long as the context.
    next_branch_id: u32,
    /// Conflict-driven learned no-goods. Each entry says: at `node`,
    /// the disjunct `bad_disjunct` of an `Or` whose pool id is
    /// `or_label` was tried and failed with a clash whose deps —
    /// after subtracting the trying branch's own `my_id` — were
    /// `preconditions`. The implication: whenever `preconditions ⊆
    /// active_branches`, asserting `bad_disjunct` at `node` is known
    /// unsat under the current context, so [`crate::search::branch`]
    /// skips it without paying for the checkpoint/recurse/rollback.
    ///
    /// Persists across rollbacks (that's the whole point — learning
    /// survives backtracking). Bounded by branch-failure events
    /// during one tableau run; rate-limited indirectly by the search
    /// depth cap. See `docs/perf-2026-05-24-new-server.md` §5.
    learned_nogoods: Vec<(NodeId, ConceptId, ConceptId, crate::graph::DepSet)>,
    /// CDBL Phase 1 (see `docs/cdbl-plan.md`): for each
    /// `branch_id` handed out by [`Self::push_branch`], the
    /// `(node, disjunct)` decision it represents — i.e. which
    /// disjunct concept was asserted at which node when the
    /// branch was opened. Lets a clash's `DepSet` (branch-ids) be
    /// translated into the *structural* set of disjunct concepts
    /// that jointly caused the clash, via
    /// [`Self::clash_decision_labels`]. Indexed by `branch_id`:
    /// `decision_labels[id]` is the decision for branch `id`, or
    /// `None` for branch ids that didn't record one (e.g. the
    /// ≤n choose-rule branch). Grows monotonically with
    /// `next_branch_id`; never shrinks (decisions are run-scoped
    /// facts, like `learned_nogoods`).
    decision_labels: Vec<Option<(NodeId, ConceptId)>>,
    /// Per-rule call counters, populated under `cfg(feature =
    /// "counters")`. Dumped to stderr in `Drop` when
    /// `RUSTDL_COUNTERS=1`. Zero cost in non-counter builds (field
    /// is omitted from the struct).
    #[cfg(feature = "counters")]
    counters: crate::counters::RuleCounters,
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
            inverse_pairs: Vec::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
            asymmetric_roles: Vec::new(),
            disjoint_role_pairs: Vec::new(),
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
            deadline: None,
            deadline_hit: false,
            active_branches: Vec::new(),
            next_branch_id: 0,
            learned_nogoods: Vec::new(),
            decision_labels: Vec::new(),
            #[cfg(feature = "counters")]
            counters: crate::counters::RuleCounters::default(),
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
            inverse_pairs: Vec::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
            asymmetric_roles: Vec::new(),
            disjoint_role_pairs: Vec::new(),
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
            deadline: None,
            deadline_hit: false,
            active_branches: Vec::new(),
            next_branch_id: 0,
            learned_nogoods: Vec::new(),
            decision_labels: Vec::new(),
            #[cfg(feature = "counters")]
            counters: crate::counters::RuleCounters::default(),
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
            inverse_pairs: Vec::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
            asymmetric_roles: Vec::new(),
            disjoint_role_pairs: Vec::new(),
            graph: CompletionGraph::new(),
            trail: TableauTrail::new(),
            deadline: None,
            deadline_hit: false,
            active_branches: Vec::new(),
            next_branch_id: 0,
            learned_nogoods: Vec::new(),
            decision_labels: Vec::new(),
            #[cfg(feature = "counters")]
            counters: crate::counters::RuleCounters::default(),
        }
    }

    /// Cap the search at a wall-clock instant. The driver in
    /// [`crate::search`] consults it on every recursion and bails out
    /// (returning `None`) the moment the deadline has passed. Use this
    /// to bound per-pair tableau time in higher-level classifiers
    /// without resorting to OS-thread cancellation.
    pub fn set_deadline(&mut self, deadline: std::time::Instant) -> &mut Self {
        self.deadline = Some(deadline);
        self
    }

    /// True iff a previously-set deadline was observed elapsed during
    /// the search. Sticky once set. Read this *after* [`crate::search`]
    /// returns to disambiguate "depth limit reached" from "deadline
    /// hit".
    #[must_use]
    pub fn deadline_reached(&self) -> bool {
        self.deadline_hit
    }

    /// Allocate the next fresh `branch_id` and push it onto the
    /// active-branches stack. Returns the freshly issued id.
    /// `branch()` calls this on entry, [`Self::pop_branch`] on exit.
    #[doc(hidden)]
    pub fn push_branch(&mut self) -> u32 {
        let id = self.next_branch_id;
        self.next_branch_id = self.next_branch_id.checked_add(1).expect(
            "TableauContext: branch id counter overflowed u32 — \
             pathological search tree",
        );
        self.active_branches.push(id);
        id
    }

    /// Pop the most recently pushed `branch_id`. Must be paired with
    /// [`Self::push_branch`].
    #[doc(hidden)]
    pub fn pop_branch(&mut self) {
        self.active_branches.pop();
    }

    /// Snapshot of the currently active branch decisions, outer-most
    /// first. Used by rules that want to tag a derived label/edge's
    /// [`crate::DepSet`] with "everything currently in scope".
    #[must_use]
    pub fn active_branches(&self) -> &[u32] {
        &self.active_branches
    }

    /// Look up a conflict-driven no-good for `(node, or_label, disjunct)`.
    /// Returns `Some(preconditions)` if any learned entry's
    /// preconditions are a subset of the current `active_branches`,
    /// meaning the disjunct is known unsat in this context and
    /// [`crate::search::branch`] can skip it. Returns `None` if no
    /// applicable no-good exists.
    #[doc(hidden)]
    pub fn nogood_blocks(
        &self,
        node: NodeId,
        or_label: ConceptId,
        disjunct: ConceptId,
    ) -> Option<&crate::graph::DepSet> {
        let active = self.active_branches.as_slice();
        for (n, ol, d, precond) in &self.learned_nogoods {
            if *n != node || *ol != or_label || *d != disjunct {
                continue;
            }
            if precond.iter().all(|p| active.contains(p)) {
                return Some(precond);
            }
        }
        None
    }

    /// Record a conflict-driven no-good. Called by
    /// [`crate::search::branch`] when a disjunct's failure carries
    /// `clash_deps` that include the trying branch's `my_id`; the
    /// stored `preconditions` are `clash_deps − {my_id}` (the
    /// ancestor branch decisions that the failure additionally
    /// depended on).
    #[doc(hidden)]
    pub fn record_nogood(
        &mut self,
        node: NodeId,
        or_label: ConceptId,
        disjunct: ConceptId,
        preconditions: crate::graph::DepSet,
    ) {
        self.learned_nogoods
            .push((node, or_label, disjunct, preconditions));
    }

    /// CDBL Phase 1: record that branch `branch_id` asserted
    /// disjunct concept `disjunct` at `node`. Called by
    /// [`crate::search::branch`] just before it asserts each
    /// disjunct, so the latest entry for a branch id reflects the
    /// choice currently under trial. The `decision_labels` vector
    /// is indexed by branch id and grown on demand.
    #[doc(hidden)]
    pub fn record_decision(&mut self, branch_id: u32, node: NodeId, disjunct: ConceptId) {
        let idx = branch_id as usize;
        if idx >= self.decision_labels.len() {
            self.decision_labels.resize(idx + 1, None);
        }
        self.decision_labels[idx] = Some((node, disjunct));
    }

    /// CDBL Phase 1: translate a clash's `DepSet` (branch ids)
    /// into the structural set of disjunct concepts those
    /// branches chose. This is the transferable, label-keyed form
    /// of the run-local branch-id explanation — the basis for the
    /// sound label-set no-goods designed in `docs/cdbl-plan.md`.
    ///
    /// Branch ids with no recorded decision (e.g. the ≤n
    /// choose-rule branch) are skipped. The result is sorted +
    /// deduped so callers can use it as a canonical key.
    #[must_use]
    pub fn clash_decision_labels(&self, clash_deps: &[u32]) -> Vec<ConceptId> {
        let mut out: Vec<ConceptId> = clash_deps
            .iter()
            .filter_map(|&id| self.decision_labels.get(id as usize).copied().flatten())
            .map(|(_node, disjunct)| disjunct)
            .collect();
        out.sort_unstable_by_key(|c: &ConceptId| c.index());
        out.dedup();
        out
    }

    /// Read the [`crate::DepSet`] of label `c` on `node`, if present.
    /// Returns `None` when the label isn't currently in `L(node)`.
    #[must_use]
    pub fn label_deps_of(&self, node: NodeId, c: ConceptId) -> Option<&crate::graph::DepSet> {
        self.graph.node(node).deps_of_label(c)
    }

    /// Read the [`crate::DepSet`] of the first edge `node —role→ target`,
    /// if present.
    #[must_use]
    pub fn edge_deps_of(
        &self,
        node: NodeId,
        role: RoleId,
        target: NodeId,
    ) -> Option<&crate::graph::DepSet> {
        self.graph.node(node).deps_of_edge(role, target)
    }

    /// Internal: returns true iff a deadline is configured and `now`
    /// has reached it. Marks the sticky `deadline_hit` flag.
    #[doc(hidden)]
    pub fn check_deadline(&mut self) -> bool {
        if let Some(d) = self.deadline
            && std::time::Instant::now() >= d
        {
            self.deadline_hit = true;
            return true;
        }
        false
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
        // Dedup so a caller that double-declares doesn't bloat the
        // linear scan. Each direction is stored once.
        if !self.inverse_pairs.contains(&(r, s)) {
            self.inverse_pairs.push((r, s));
        }
        if !self.inverse_pairs.contains(&(s, r)) {
            self.inverse_pairs.push((s, r));
        }
        self
    }

    /// True if `r` and `s` are linked by a declared
    /// `InverseObjectProperties` axiom.
    #[must_use]
    pub fn are_declared_inverses(&self, r: RoleId, s: RoleId) -> bool {
        // Fast path: most ontologies declare 0–3 inverse pairs.
        // Linear scan on `u32` equality beats `HashMap::get` at this
        // size and avoids the make_hash/find_inner chain that
        // dominated apply_max in pizza flamegraphs.
        if self.inverse_pairs.is_empty() {
            return false;
        }
        self.inverse_pairs.iter().any(|&(a, b)| a == r && b == s)
    }

    /// Register the NNF complement of `body`. Must be called before
    /// satisfiability for every `body` appearing in a `Max(_, _, body)`
    /// so [`apply_choose`] can look the complement up at branching
    /// time without mutating the pool.
    pub fn set_complement(&mut self, body: ConceptId, complement: ConceptId) -> &mut Self {
        self.complements.insert(body, complement);
        self
    }

    /// Register a length-2 role chain axiom `r₁ ∘ r₂ ⊑ sup`. Each
    /// position carries its own polarity ([`Role::Named`] or
    /// [`Role::Inverse`]). The [`apply_role_chains`](crate::apply_role_chains)
    /// rule walks the appropriate edge direction at each position
    /// (named ⇒ outgoing, inverse ⇒ incoming) and adds an edge of
    /// the appropriate polarity at `sup`.
    pub fn declare_chain_axiom(&mut self, r1: Role, r2: Role, sup: Role) -> &mut Self {
        self.chains.push((r1, r2, sup));
        self
    }

    /// Slice of all registered length-2 chain axioms.
    #[must_use]
    pub fn chains(&self) -> &[(Role, Role, Role)] {
        &self.chains
    }

    /// Declare `r` as `AsymmetricObjectProperty`. The
    /// [`apply_role_axioms`](crate::apply_role_axioms) rule then
    /// rejects any node carrying both directions of `r` to/from the
    /// same neighbour.
    pub fn declare_asymmetric_role(&mut self, r: RoleId) -> &mut Self {
        if !self.asymmetric_roles.contains(&r) {
            self.asymmetric_roles.push(r);
        }
        self
    }

    /// Declare `r` and `s` as `DisjointObjectProperties` (one
    /// unordered pair). The [`apply_role_axioms`](crate::apply_role_axioms)
    /// rule rejects any node with both an `r`- and an `s`-edge to the
    /// same neighbour. Stored once; the rule symmetrizes when
    /// iterating.
    pub fn declare_disjoint_role_pair(&mut self, r: RoleId, s: RoleId) -> &mut Self {
        if r != s
            && !self.disjoint_role_pairs.contains(&(r, s))
            && !self.disjoint_role_pairs.contains(&(s, r))
        {
            self.disjoint_role_pairs.push((r, s));
        }
        self
    }

    #[must_use]
    pub fn asymmetric_roles(&self) -> &[RoleId] {
        &self.asymmetric_roles
    }

    #[must_use]
    pub fn disjoint_role_pairs(&self) -> &[(RoleId, RoleId)] {
        &self.disjoint_role_pairs
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

    /// Mutable accessor for the completion graph. Used by the
    /// saturator's worklist plumbing — clearing the `dirty` bit on
    /// the node about to be processed, and marking all nodes dirty
    /// at the start of each `saturate()` call.
    pub fn graph_mut(&mut self) -> &mut CompletionGraph {
        &mut self.graph
    }

    /// Set the residual-saturation memo on `node`. Called from
    /// [`crate::apply_residual_gcis`] after a full materialisation
    /// pass; lets subsequent calls short-circuit.
    pub fn mark_residuals_saturated(&mut self, node: NodeId) {
        self.graph.set_residuals_saturated(node, true);
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
        self.new_successor_with_deps(from, role, &[])
    }

    /// Like [`Self::new_successor`] but tags the generative edge
    /// with `deps` — the [`crate::DepSet`] of the `∃R.C` label that
    /// licensed this generation. Used by `apply_exists` to propagate
    /// branch-decision provenance through fresh witnesses.
    pub fn new_successor_with_deps(&mut self, from: NodeId, role: RoleId, deps: &[u32]) -> NodeId {
        let prior_len = self.graph.len();
        let id = self
            .graph
            .push_node_with_parent(Some(from), Some(Role::Named(role)));
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        self.add_edge_inner(from, role, id, deps);
        id
    }

    /// Allocate a fresh *predecessor* of `to`: the inverse direction
    /// of [`Self::new_successor`]. The new node `new` is created
    /// with an outgoing edge `new —role→ to`. The new node's
    /// `parent` is `to` (its *creator* — pair-blocking ancestry runs
    /// through the creator), and `parent_role = Role::Inverse(role)`
    /// because the inbound generative role at the creator is `r⁻`.
    pub fn new_predecessor(&mut self, to: NodeId, role: RoleId) -> NodeId {
        self.new_predecessor_with_deps(to, role, &[])
    }

    /// Dep-aware variant of [`Self::new_predecessor`].
    pub fn new_predecessor_with_deps(&mut self, to: NodeId, role: RoleId, deps: &[u32]) -> NodeId {
        let prior_len = self.graph.len();
        let id = self
            .graph
            .push_node_with_parent(Some(to), Some(Role::Inverse(role)));
        self.trail.record(TrailEntry::NodeCreated { prior_len });
        self.add_edge_inner(id, role, to, deps);
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
    #[allow(clippy::similar_names)]
    pub fn is_blocked(&self, y: NodeId) -> bool {
        crate::bump_counter!(self, is_blocked_calls);
        let yb = self.graph.blocking(y);
        let (Some(yp_id), Some(yr)) = (yb.parent, yb.parent_role) else {
            return false;
        };
        let yl_sig = yb.label_sig;
        let yp_sig = self.graph.blocking(yp_id).label_sig;

        // Iterate strict tree-ancestors of y via the dense
        // `blocking` summary (24 bytes/node, ≥2 entries per cache
        // line) instead of the ~200-byte `Node`. This is the
        // exclusive-time hot path post-B.4 on pizza-shaped inputs.
        //
        // Subset prefilter: a sound necessary condition for
        // `L(y) ⊆ L(x')` is `yl_sig & !x_sig == 0` — if any bit set
        // in `yl_sig` is missing from the candidate's signature, at
        // least one label is missing too. Same for `L(parent(y)) ⊆
        // L(parent(x'))`. Only candidates that pass both prefilters
        // pay for a full `Node` load and the linear subset scan.
        let mut x_prime_id = yp_id;
        // Cycle detector. Strict tree-ancestors of y must be all
        // distinct; revisiting any node means parent pointers formed
        // a cycle. In debug, that's an invariant violation — fail
        // loudly with the chain so the creating mutation can be
        // identified. In release, fall back to a step-cap and return
        // "not blocked" so callers don't hang. This is a workaround,
        // not a fix: see TODO(blocking-cycle).
        let max_steps = self.graph.len();
        let mut visited: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut chain: Vec<NodeId> = vec![x_prime_id];
        let _ = (&mut visited, &mut chain); // referenced in debug_assert
        for _step in 0..=max_steps {
            let xb = self.graph.blocking(x_prime_id);
            if let (Some(xp_id), Some(xr)) = (xb.parent, xb.parent_role)
                && xr == yr
            {
                if (yl_sig & !xb.label_sig) != 0
                    || (yp_sig & !self.graph.blocking(xp_id).label_sig) != 0
                {
                    crate::bump_counter!(self, is_blocked_prefilter_rejects);
                } else {
                    crate::bump_counter!(self, is_blocked_subset_scans);
                    let y_labels = &self.graph.node(y).labels;
                    let x_labels = &self.graph.node(x_prime_id).labels;
                    if is_subset_sorted(y_labels, x_labels) {
                        let yp_labels = &self.graph.node(yp_id).labels;
                        let xp_labels = &self.graph.node(xp_id).labels;
                        if is_subset_sorted(yp_labels, xp_labels) {
                            crate::bump_counter!(self, is_blocked_true);
                            return true;
                        }
                    }
                }
            }
            match xb.parent {
                Some(next) => {
                    debug_assert!(
                        visited.insert(x_prime_id),
                        "parent-pointer cycle: y={} chain={:?} revisiting={}",
                        y.index(),
                        chain.iter().map(|n| n.index()).collect::<Vec<_>>(),
                        x_prime_id.index()
                    );
                    chain.push(next);
                    x_prime_id = next;
                }
                None => return false,
            }
        }
        false
    }

    /// Add concept `c` to `node`'s label list if not already present.
    ///
    /// Returns `true` if the label was newly inserted, `false` if it
    /// was already there. Records a [`TrailEntry::LabelAdded`] on
    /// insertion. The new label's [`crate::DepSet`] is empty —
    /// callers that know which branch decisions the derivation
    /// depended on should use [`Self::add_label_with_deps`] instead.
    ///
    /// `c` must be in NNF; debug-asserted at the boundary so any rule
    /// that forgets to normalize is caught in tests but pays no cost
    /// in release.
    pub fn add_label(&mut self, node: NodeId, c: ConceptId) -> bool {
        self.add_label_with_deps(node, c, &[])
    }

    /// Like [`Self::add_label`] but attaches `deps` to the new label
    /// (empty for deterministic-rule conclusions; non-empty for
    /// branch-decision-derived labels). On duplicate add the existing
    /// `DepSet` is preserved — widening will arrive with the per-rule
    /// propagation commit (see `docs/phase4-backjumping-plan.md`).
    pub fn add_label_with_deps(&mut self, node: NodeId, c: ConceptId, deps: &[u32]) -> bool {
        crate::bump_counter!(self, add_label_calls);
        debug_assert!(
            is_nnf(c, self.pool),
            "TableauContext::add_label received non-NNF concept"
        );
        let n = self.graph.node_mut(node);
        match n.labels.binary_search(&c) {
            Ok(_) => false,
            Err(pos) => {
                n.labels.insert(pos, c);
                let mut owned: crate::graph::DepSet = crate::graph::DepSet::from_slice(deps);
                owned.sort_unstable();
                owned.dedup();
                n.label_deps.insert(pos, owned);
                self.graph.blocking_mut(node).label_sig |= crate::graph::label_sig_bit(c);
                self.trail
                    .record(TrailEntry::LabelAdded { node, concept: c });
                crate::bump_counter!(self, add_label_inserted);
                // Worklist: any rule keyed on the node's label set
                // (and-decomposition, ∀-propagation, atomic-class
                // triggers, existentials, cardinality, …) must
                // re-fire after a new label appears.
                self.graph.set_dirty(node, true);
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
        self.add_edge_with_deps(from, role, target, &[]);
    }

    /// Like [`Self::add_edge`] but attaches `deps` (the branch
    /// decisions whose firing produced this edge) to both the
    /// outgoing and the mirror incoming slot. Phase 4 DDB will read
    /// these on clash to compute the clash's full dependency set.
    pub fn add_edge_with_deps(&mut self, from: NodeId, role: RoleId, target: NodeId, deps: &[u32]) {
        self.add_edge_inner(from, role, target, deps);
    }

    fn add_edge_inner(&mut self, from: NodeId, role: RoleId, target: NodeId, deps: &[u32]) {
        crate::bump_counter!(self, add_edge_calls);
        let mut owned: crate::graph::DepSet = crate::graph::DepSet::from_slice(deps);
        owned.sort_unstable();
        owned.dedup();
        let from_node = self.graph.node_mut(from);
        from_node.edges.push((role, target));
        from_node.edge_deps.push(owned.clone());
        let target_node = self.graph.node_mut(target);
        target_node.in_edges.push((role, from));
        target_node.in_edge_deps.push(owned);
        self.trail
            .record(TrailEntry::EdgeAdded { from, role, target });
        // Worklist: rules that read edges on either endpoint must
        // re-fire (apply_forall, apply_role_*, apply_role_chains,
        // apply_exists witness check, apply_min/max cardinality
        // counting).
        self.graph.set_dirty(from, true);
        if target != from {
            self.graph.set_dirty(target, true);
        }
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
    /// Convenience wrapper: merge with no extra merge-condition deps.
    /// Equivalent to `merge_into_with_deps(source, target, &[])`. Use
    /// `merge_into_with_deps` when the merge is conditional on branch
    /// decisions whose ids should follow the moved labels and edges.
    #[allow(clippy::missing_panics_doc)]
    pub fn merge_into(&mut self, source: NodeId, target: NodeId) -> bool {
        self.merge_into_with_deps(source, target, &[])
    }

    /// Like [`Self::merge_into`] but every label and edge moved from
    /// `source` to `target` has `merge_deps` unioned into its
    /// `DepSet`. Use when the *reason* for merging — typically the
    /// deps of the two nominal labels in `apply_nominal_assignment`,
    /// or of the `≤n R.C` label plus matching edges in `apply_max`
    /// — is conditional on prior branch decisions. Without these the
    /// soundness invariant for dependency-directed back-jumping is
    /// violated: a clash after the merge can return `clash_deps`
    /// missing the branch ids that licensed the merge in the first
    /// place, and the search back-jumps past them. (Pizza
    /// `:NamedPizza` false-positive 2026-05-25.)
    #[allow(clippy::missing_panics_doc, clippy::too_many_lines)]
    pub fn merge_into_with_deps(
        &mut self,
        source: NodeId,
        target: NodeId,
        merge_deps: &[u32],
    ) -> bool {
        debug_assert_ne!(source, target, "merge_into: source and target must differ");
        if self.are_distinct(source, target) {
            return false;
        }
        // The target absorbs everything from the source — labels,
        // edges, inequalities, child parent-pointers. All of those
        // are downstream mutations that mark target dirty themselves;
        // we set it here as well to ensure the post-merge node gets
        // re-visited even if the merge happened to move no labels.
        self.graph.set_dirty(target, true);
        // Helper: union a label/edge's per-element DepSet with the
        // merge-condition deps so the moved item depends on both
        // "this label/edge was at the source" and "the source was
        // merged into the target."
        let with_merge = |elt_deps: &crate::graph::DepSet| -> crate::graph::DepSet {
            crate::deps::union(elt_deps, &crate::graph::DepSet::from_slice(merge_deps))
        };
        // Snapshot source's state before mutating. We use clones so
        // the loops don't borrow the graph mutably during iteration.
        // Labels are paired with their `DepSet` so the merge preserves
        // the per-label dependency-directed-backjumping invariant —
        // dropping deps here would make label-clash detection on the
        // merged node return an empty `clash_deps`, which the search
        // back-jumps *past* the licensing disjunction instead of
        // letting the alternative disjunct fire (pizza regression
        // 2026-05-25, see `pizza_functional_equiv_some_should_be_sat`
        // and `docs/perf-2026-05-24-new-server.md` §5).
        let source_labels: Vec<(ConceptId, crate::graph::DepSet)> = {
            let n = self.graph.node(source);
            n.labels
                .iter()
                .zip(n.label_deps.iter())
                .map(|(c, d)| (*c, d.clone()))
                .collect()
        };
        let source_out: Vec<(RoleId, NodeId)> = self.graph.node(source).edges.to_vec();
        let source_in: Vec<(RoleId, NodeId)> = self.graph.node(source).in_edges.to_vec();
        let source_ineq: Vec<NodeId> = self.graph.node(source).inequalities.to_vec();

        // 1. Replay labels on target with their deps so back-jumping
        //    can identify which branch the merge-clash depends on.
        //    Union with `merge_deps` — the labels are at the target
        //    *because* the merge happened, so the merge condition's
        //    deps belong here too.
        for (c, deps) in source_labels {
            let final_deps = with_merge(&deps);
            self.add_label_with_deps(target, c, final_deps.as_slice());
        }

        // 2. Re-anchor outgoing edges: for each (r, x) in source.edges,
        //    remove it and add (r, x) on target carrying the same
        //    DepSet — dropping it here is the edge-side analogue of the
        //    label-deps bug fixed in step 1 (pizza regression).
        for (role, x) in source_out {
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
            // Capture the edge's DepSet before we remove it.
            let prior_edge_deps = self
                .graph
                .node(source)
                .edge_deps
                .get(from_pos)
                .cloned()
                .unwrap_or_default();
            self.remove_edge_recorded(source, role, x, from_pos, in_pos);
            let new_target = if x == source { target } else { x };
            let final_deps = with_merge(&prior_edge_deps);
            self.add_edge_inner(target, role, new_target, final_deps.as_slice());
        }

        // 3. Re-anchor incoming edges with their DepSets (same fix as
        //    step 2 on the inverse side).
        for (role, y) in source_in {
            let y_eff = self.resolve(y);
            if y_eff == source {
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
            let prior_edge_deps = self
                .graph
                .node(y_eff)
                .edge_deps
                .get(from_pos)
                .cloned()
                .unwrap_or_default();
            self.remove_edge_recorded(y_eff, role, source, from_pos, in_pos);
            let final_deps = with_merge(&prior_edge_deps);
            self.add_edge_inner(y_eff, role, target, final_deps.as_slice());
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
                // Mirror the parent update into the cache-dense
                // blocking summary; `is_blocked` walks ancestors via
                // this array, and a stale entry would cause it to
                // miss or false-find a blocker — both unsound.
                self.graph.blocking_mut(nid).parent = Some(target);
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
        let from_node = self.graph.node_mut(from);
        let removed = from_node.edges.remove(position);
        debug_assert_eq!(removed, (role, target));
        let prior_edge_deps = from_node.edge_deps.remove(position);
        let target_node = self.graph.node_mut(target);
        let mirror = target_node.in_edges.remove(in_position);
        debug_assert_eq!(mirror, (role, from));
        let prior_in_edge_deps = target_node.in_edge_deps.remove(in_position);
        self.trail.record(TrailEntry::EdgeRemoved {
            from,
            role,
            target,
            position,
            in_position,
            prior_edge_deps,
            prior_in_edge_deps,
        });
        // Edge removal changes the edge set on both endpoints — rules
        // keyed on edges (apply_forall, apply_role_*, apply_exists
        // witness, cardinality counting) need to re-fire.
        self.graph.set_dirty(from, true);
        if target != from {
            self.graph.set_dirty(target, true);
        }
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
        self.clash_deps_at(node).is_some()
    }

    /// Dep-aware clash check: returns `Some(deps)` when this node
    /// holds either a `Bot` label or a complementary `(c, ¬c)` pair,
    /// where `deps` is the union of the offending labels' deps —
    /// i.e. the set of branch decisions necessary to derive the
    /// clash. Returns `None` if the node is clash-free.
    ///
    /// Used by [`crate::saturate`] to construct
    /// [`crate::SaturationResult::Clash`] with the clash provenance
    /// attached, which then drives Phase 4 back-jumping.
    #[must_use]
    pub fn clash_deps_at(&self, node: NodeId) -> Option<crate::graph::DepSet> {
        let n = self.graph.node(node);
        let labels = n.labels();
        for (pos, &c) in labels.iter().enumerate() {
            match self.pool.get(c) {
                ConceptExpr::Bot => return Some(n.label_deps[pos].clone()),
                ConceptExpr::Not(inner) => {
                    if let Ok(inner_pos) = labels.binary_search(inner) {
                        return Some(crate::deps::union(
                            &n.label_deps[pos],
                            &n.label_deps[inner_pos],
                        ));
                    }
                }
                _ => {}
            }
        }
        None
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
        search::search(self, MAX_DEPTH).to_option()
    }
}

#[cfg(feature = "counters")]
impl Drop for TableauContext<'_, '_, '_> {
    fn drop(&mut self) {
        if std::env::var("RUSTDL_COUNTERS").as_deref() == Ok("1") {
            // Label each dump with the worker thread id so a parallel
            // classify run's interleaved blocks can be sorted out
            // post-hoc.
            let tid = std::thread::current().id();
            self.counters.dump(&format!("thread={tid:?}"));
        }
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
        AbsorbedTBox, ClassId, ConceptRule, IndividualId, NominalRule, Role, RoleHierarchyBuilder,
        RoleId, RoleRule,
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
    fn add_label_default_deps_are_empty() {
        // Phase 4 commit 1 invariant: the legacy `add_label` API
        // attaches an empty `DepSet`. This pins the no-behaviour-change
        // contract for the data-plumbing commit.
        let (pool, a, _) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, a);
        assert_eq!(
            ctx.label_deps_of(n, a).map(smallvec::SmallVec::as_slice),
            Some(&[][..])
        );
    }

    #[test]
    fn add_label_with_deps_normalises_and_persists() {
        // Deps are stored sorted + deduped; lockstep with the label.
        let (pool, a, _) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        // Intentionally unsorted with a duplicate — the API should
        // canonicalise.
        ctx.add_label_with_deps(n, a, &[3, 1, 1, 2]);
        assert_eq!(
            ctx.label_deps_of(n, a).map(smallvec::SmallVec::as_slice),
            Some(&[1u32, 2, 3][..])
        );
    }

    #[test]
    fn rollback_drops_label_and_its_deps_in_lockstep() {
        // After a checkpointed add_label_with_deps and a rollback, both
        // labels[] and label_deps[] must shrink by one.
        let (pool, a, _) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let cp = ctx.checkpoint();
        ctx.add_label_with_deps(n, a, &[7]);
        assert_eq!(ctx.graph().node(n).labels(), &[a]);
        assert_eq!(
            ctx.label_deps_of(n, a).map(smallvec::SmallVec::as_slice),
            Some(&[7u32][..])
        );
        ctx.rollback_to(cp);
        assert!(ctx.graph().node(n).labels().is_empty());
        assert!(ctx.label_deps_of(n, a).is_none());
    }

    #[test]
    fn apply_and_propagates_deps_to_conjuncts() {
        // L(x) ← And([a, b]) with deps={5} ⇒ a and b should both be
        // added to L(x) with deps={5}. Phase 4 commit 2 propagation.
        use crate::apply_and;
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let and_ab = pool.and([a, b]);
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label_with_deps(n, and_ab, &[5]);
        let _ = apply_and(&mut ctx, n);
        assert_eq!(
            ctx.label_deps_of(n, a).map(smallvec::SmallVec::as_slice),
            Some(&[5u32][..])
        );
        assert_eq!(
            ctx.label_deps_of(n, b).map(smallvec::SmallVec::as_slice),
            Some(&[5u32][..])
        );
    }

    #[test]
    fn push_pop_branch_round_trip() {
        // Branch ids monotonic-increasing; active_branches reflects
        // push/pop discipline.
        let pool = ConceptPool::new();
        let mut ctx = TableauContext::new(&pool);
        assert!(ctx.active_branches().is_empty());
        let b0 = ctx.push_branch();
        let b1 = ctx.push_branch();
        assert_eq!(b0, 0);
        assert_eq!(b1, 1);
        assert_eq!(ctx.active_branches(), &[0, 1]);
        ctx.pop_branch();
        assert_eq!(ctx.active_branches(), &[0]);
        let b2 = ctx.push_branch();
        // Branch ids don't reuse popped values.
        assert_eq!(b2, 2);
        ctx.pop_branch();
        ctx.pop_branch();
        assert!(ctx.active_branches().is_empty());
    }

    #[test]
    fn clash_decision_labels_translates_branch_ids_to_disjuncts() {
        // CDBL Phase 1: record (branch → disjunct) decisions, then
        // translate a clash DepSet (branch ids) into the structural
        // disjunct-concept set.
        let mut pool = ConceptPool::new();
        let d_a = pool.atomic(ClassId::new(10));
        let d_b = pool.atomic(ClassId::new(11));
        let d_c = pool.atomic(ClassId::new(12));
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let b0 = ctx.push_branch();
        ctx.record_decision(b0, n, d_a);
        let b1 = ctx.push_branch();
        ctx.record_decision(b1, n, d_b);
        let b2 = ctx.push_branch();
        ctx.record_decision(b2, n, d_c);
        // A clash that depended on branches 0 and 2 maps to {d_a, d_c}.
        let labels = ctx.clash_decision_labels(&[b0, b2]);
        assert_eq!(labels, vec![d_a, d_c]);
        // All three branches → all three disjuncts, sorted+deduped.
        let all = ctx.clash_decision_labels(&[b2, b0, b1]);
        assert_eq!(all, vec![d_a, d_b, d_c]);
    }

    #[test]
    fn clash_decision_labels_skips_undecided_and_out_of_range_branches() {
        // Branch ids without a recorded decision (e.g. the choose
        // rule's branch) and ids beyond the recorded range are
        // skipped, not panicked on.
        let mut pool = ConceptPool::new();
        let d_a = pool.atomic(ClassId::new(5));
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let b0 = ctx.push_branch();
        ctx.record_decision(b0, n, d_a);
        let b1 = ctx.push_branch(); // no record_decision for b1
        // Clash deps reference b1 (undecided) and 99 (out of range).
        let labels = ctx.clash_decision_labels(&[b0, b1, 99]);
        assert_eq!(labels, vec![d_a]);
    }

    #[test]
    fn verify_node_local_clash_detects_complementary_pair() {
        // {A, ¬A} co-occurring clashes node-locally.
        let (pool, a, not_a) = pool_with_a_and_not_a();
        let tbox = AbsorbedTBox::default();
        let hierarchy = RoleHierarchyBuilder::with_roles(0).build();
        assert!(verify_node_local_clash(
            &pool,
            &tbox,
            &hierarchy,
            &[a, not_a],
            64
        ));
    }

    #[test]
    fn verify_node_local_clash_detects_disjointness_via_concept_rule() {
        // A ⊑ ¬B (disjointness absorbed as a concept rule). The
        // label-set {A, B} clashes node-locally: the rule fires
        // `¬B` from `A`, contradicting the present `B`.
        let mut pool = ConceptPool::new();
        let a_cls = ClassId::new(0);
        let b_cls = ClassId::new(1);
        let a = pool.atomic(a_cls);
        let b = pool.atomic(b_cls);
        let not_b = pool.not(b);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: a_cls,
                conclusion: not_b,
            }],
            ..AbsorbedTBox::default()
        };
        let mut tbox = tbox;
        tbox.finalize();
        let hierarchy = RoleHierarchyBuilder::with_roles(0).build();
        assert!(verify_node_local_clash(
            &pool,
            &tbox,
            &hierarchy,
            &[a, b],
            64
        ));
    }

    #[test]
    fn verify_node_local_clash_is_false_for_satisfiable_set() {
        // {A, B} with no disjointness — node-locally satisfiable.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let tbox = AbsorbedTBox::default();
        let hierarchy = RoleHierarchyBuilder::with_roles(0).build();
        assert!(!verify_node_local_clash(
            &pool,
            &tbox,
            &hierarchy,
            &[a, b],
            64
        ));
    }

    #[test]
    fn verify_node_local_clash_ignores_existential_only_clashes() {
        // A ⊑ ∃R.(B ⊓ ¬B): the clash needs a successor, so it is
        // NOT node-local — verify must return false (conservative),
        // since the label-set {A} alone doesn't reproduce the clash
        // without generating the successor.
        let mut pool = ConceptPool::new();
        let a_cls = ClassId::new(0);
        let a = pool.atomic(a_cls);
        let b = pool.atomic(ClassId::new(1));
        let not_b = pool.not(b);
        let bad = pool.and([b, not_b]);
        let r = RoleId::new(0);
        let some_r_bad = pool.some(Role::named(r), bad);
        let mut tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: a_cls,
                conclusion: some_r_bad,
            }],
            ..AbsorbedTBox::default()
        };
        tbox.finalize();
        let hierarchy = RoleHierarchyBuilder::with_roles(1).build();
        // {A} alone: node-local rules add the ∃ label but do NOT
        // generate the successor, so no clash is detected here.
        assert!(!verify_node_local_clash(&pool, &tbox, &hierarchy, &[a], 64));
    }

    #[test]
    fn record_decision_latest_wins_for_reused_branch_id() {
        // branch() reuses my_id across the disjunct loop, recording
        // each disjunct as it's tried. The latest record for a branch
        // id reflects the choice currently under trial.
        let mut pool = ConceptPool::new();
        let d_first = pool.atomic(ClassId::new(1));
        let d_second = pool.atomic(ClassId::new(2));
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        let b = ctx.push_branch();
        ctx.record_decision(b, n, d_first);
        ctx.record_decision(b, n, d_second); // overwrites
        assert_eq!(ctx.clash_decision_labels(&[b]), vec![d_second]);
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
        // Deps empty: this trail uses the deps-free `add_label`/
        // `add_edge` API, so the clash propagation has nothing to
        // attribute branch-wise. We assert the variant + the node;
        // the DepSet is just the empty Vec.
        assert!(matches!(result, SaturationResult::Clash(n, _) if n == y));
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
        assert!(matches!(result, SaturationResult::Clash(n, _) if n == x));
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
        assert_eq!(result, search::SearchVerdict::Sat);
        assert_eq!(ctx.graph().node(x).labels().len(), initial_label_count);
    }

    #[test]
    fn backjumping_unsat_carries_empty_deps_when_clash_is_root() {
        // L(x) = {A, ¬A} — clash is present in the *root* state
        // before any `⊔` branching could happen. `search` should
        // return `Unsat(empty)` — empty deps mean the clash didn't
        // depend on any branch decision. Phase 4 commit 5 invariant.
        let (pool, a, not_a) = pool_with_a_and_not_a();
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        ctx.add_label(x, a);
        ctx.add_label(x, not_a);
        let result = search::search(&mut ctx, 16);
        assert!(
            matches!(result, search::SearchVerdict::Unsat(ref deps) if deps.is_empty()),
            "expected Unsat(empty deps) for a root-level clash, got {result:?}"
        );
    }

    #[test]
    fn backjumping_unsat_inside_branch_keeps_state_clean() {
        // L(x) = {A, ¬A, A ⊔ B}. The clash on `{A, ¬A}` is
        // pre-branch — even after `branch()` allocates a `branch_id`
        // and tries adding A, the saturation finds the clash with
        // *zero* deps (neither label depends on `branch_id`). Because
        // `branch_id ∉ {}`, back-jumping fires: branch() returns
        // Unsat without trying the second disjunct.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let not_a = pool.not(a);
        let or_ab = pool.or([a, b]);
        let mut ctx = TableauContext::new(&pool);
        let x = ctx.new_node();
        ctx.add_label(x, a);
        ctx.add_label(x, not_a);
        ctx.add_label(x, or_ab);
        let result = search::search(&mut ctx, 16);
        assert!(
            matches!(result, search::SearchVerdict::Unsat(_)),
            "expected Unsat, got {result:?}"
        );
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
        assert_eq!(result, search::SearchVerdict::Sat);
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
