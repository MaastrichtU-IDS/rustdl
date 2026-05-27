//! Hyperresolution engine — hypertableau Phases H1 (Horn) + H2
//! (disjunctive-head branching).
//!
//! See [`docs/hypertableau-scoping.md`](../../docs/hypertableau-scoping.md).
//! This is the first phase that *reasons*: it runs Horn
//! hyperresolution (DL-clauses with ≤1 head atom — no branching)
//! over a minimal class-labelled completion graph, with anywhere
//! blocking to terminate cyclic `∃`. H2 adds backtracking search
//! over disjunctive-head clauses ([`HyperEngine::decide`]): Horn
//! propagation runs to fixpoint, then an open disjunction is split
//! and each disjunct tried in turn with save/restore of the graph.
//!
//! It is **not** wired into the reasoner facade or the default
//! tableau — it's a standalone engine, validated in isolation
//! against hand-built Horn ontologies and (in a later step) the EL
//! saturation closure. The existing path is untouched.
//!
//! ## Why Horn is deterministic
//!
//! A clause `U1 ∧ … ∧ Um → V` fires only when its *whole* body
//! matches at a node (binding the central variable `x` and, if the
//! body has a role atom `R(x,y)`, a successor `y`). A single head
//! atom is then asserted with no choice — that's the
//! demand-driven, branch-free propagation that makes the ~96 %
//! Horn fragment of the corpus cheap (see
//! `docs/hypertableau-scoping.md` §H0).

use owl_dl_core::RoleHierarchy;
use owl_dl_core::clause::{Atom, DlClause, Var, X};
use owl_dl_core::ir::{ClassId, Role};
use std::time::Instant;

/// A match binding: the body's non-`X` successor variables mapped to
/// graph nodes, sorted by variable. `X` is implicit (always the match
/// root), so an empty binding is a body on `X` only. Bodies are trees
/// rooted at `X` (each non-`X` var is the target of exactly one role
/// atom whose source is already bound), so a binding is one complete
/// homomorphism of the body's variable-tree into the graph.
type Binding = Vec<(Var, HNode)>;

/// Defensive cap on the number of body variables `match_body` will
/// bind; bodies above it are treated as unsupported (deferred). Real
/// clausifier bodies are 1–3 vars; this guards pathological inputs.
const MAX_BODY_VARS: usize = 8;

/// Defensive cap on the Horn-fixpoint inner loop during branching
/// search. Anywhere blocking bounds the graph, so a real fixpoint is
/// reached well under this; hitting it yields `Stalled`, not `Unsat`.
const FIXPOINT_ITERS: usize = 100_000;

/// Node id in the hyper completion graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HNode(u32);

impl HNode {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Default, Clone)]
struct HyperNode {
    /// Class atoms true at this node — sorted by id, deduped.
    labels: Vec<ClassId>,
    /// Outgoing role edges `(role, target)`.
    edges: Vec<(Role, HNode)>,
    /// Incoming role edges `(role, source)` — the reverse of `edges`,
    /// so a label added here can re-queue its predecessors (the
    /// back-propagation wake-up for semi-naive evaluation).
    preds: Vec<(Role, HNode)>,
    /// `≤n` constraints `(role, qualifier, bound)` attached to this
    /// node (H3c). Enforced by the merge rule when the node has more
    /// matching `role`-successors than `bound`.
    at_most: Vec<(Role, Option<ClassId>, u32)>,
    /// `≥n` constraints `(role, qualifier, bound)` already *generated*
    /// at this node (`HF3a`). Fire-once tracking: the `≥n`-rule creates
    /// `n` fresh pairwise-`≠` successors exactly once per constraint, so
    /// it can't regenerate (which would loop). Part of node state, so
    /// it's captured by save/restore with the rest of the node.
    at_least_done: Vec<(Role, Option<ClassId>, u32)>,
    /// Creation order index — used by anywhere blocking ("blocked
    /// by an *earlier* node"). Equal to the node's own index here.
    order: u32,
}

impl HyperNode {
    fn has(&self, c: ClassId) -> bool {
        self.labels
            .binary_search_by_key(&c.index(), |l| l.index())
            .is_ok()
    }

    /// Insert a class label; returns true if newly added.
    fn add(&mut self, c: ClassId) -> bool {
        match self.labels.binary_search_by_key(&c.index(), |l| l.index()) {
            Ok(_) => false,
            Err(pos) => {
                self.labels.insert(pos, c);
                true
            }
        }
    }
}

/// Outcome of a Horn hyperresolution run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HyperResult {
    /// A clash-free completion exists (the root concept is
    /// satisfiable in the Horn fragment).
    Sat,
    /// A `body → ⊥` clause fired — the root concept is unsat.
    Unsat,
    /// The iteration cap was hit (defensive; shouldn't happen on
    /// well-formed Horn input thanks to anywhere blocking).
    Stalled,
}

/// Per-run search instrumentation, read after [`HyperEngine::decide`]
/// to interpret a wall measurement: a `Sat` reached with
/// `branches_taken == 0` was decided by pure Horn propagation and
/// says nothing about hypertableau branching (see
/// `docs/hypertableau-scoping.md` §H2b).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Disjuncts asserted across the whole search (decisions made).
    pub branches_taken: u64,
    /// Failed branches whose graph was restored (`Unsat`/`Stalled`).
    pub restores: u64,
    /// Deepest branch nesting reached (0 ⇒ no branching).
    pub max_branch_depth: u32,
    /// `match_body` calls — every (clause × node) match attempt in the
    /// Horn fixpoint. Profiling counter for the search-quality work.
    pub match_attempts: u64,
    /// `self.nodes` clones (one per branch decision). Profiling
    /// counter: the save/restore cost the trail would remove.
    pub node_clones: u64,
    /// `horn_fixpoint` worklist drains (one per call) across the search.
    pub fixpoint_passes: u64,
}

/// The hyperresolution engine. Holds the completion graph and the
/// clause set (borrowed), plus per-run search instrumentation.
pub struct HyperEngine<'c> {
    clauses: &'c [DlClause],
    nodes: Vec<HyperNode>,
    stats: SearchStats,
    init_depth: usize,
    deadline: Option<Instant>,
    /// Trigger indexes routing derivation events to the clauses they
    /// newly enable (see [`ClauseIndexes`]).
    indexes: ClauseIndexes,
    /// Semi-naive worklist of derivation *events* (LIFO). Each event
    /// fires only the clauses it newly enables (not all of a node's
    /// clauses), which is what prunes the re-fire cost. See
    /// `docs/hypertableau-seminaive-scoping.md`.
    worklist: Vec<Event>,
    /// Union-find over nodes for the `≤n` merge rule (H3c): when node
    /// `j` is merged into `i`, `representative[j] = i`. Identity for
    /// un-merged nodes. Resolve role-successors through this when
    /// counting/following edges so a merged node is seen once.
    representative: Vec<HNode>,
    /// HF2 role hierarchy: an `R`-edge satisfies an `S`-atom when
    /// `R ⊑* S`. Unlike inverse pairs (an equivalence, canonicalized in
    /// the clausifier), `⊑` is one-way, so it must be consulted at match
    /// time. `None` ⇒ reflexive only (every role subsumes just itself),
    /// the pre-HF2 behaviour.
    sub_roles: Option<RoleHierarchy>,
    /// `HF3a` node inequalities `x ≠ y`. Stored as resolved pairs at
    /// insert time; queried through `resolve` so merges keep the
    /// relation correct without rewriting. The `≥n`-rule marks its
    /// generated successors pairwise `≠`; the `≤n` merge rule refuses
    /// to merge a `≠` pair (a forced such merge is a clash — what makes
    /// `≥2 ⊓ ≤1` unsat). Captured by save/restore.
    neq: Vec<(HNode, HNode)>,
}

/// A derivation event driving semi-naive Horn evaluation.
#[derive(Debug, Clone, Copy)]
enum Event {
    /// Node `n` gained class `c`.
    Label(HNode, ClassId),
    /// A role edge `src —role→ tgt` was added.
    Edge(HNode, Role, HNode),
    /// Node `n` was created (fires empty-body / `⊤` clauses).
    NodeNew(HNode),
}

/// Per-clause-set trigger indexes for semi-naive evaluation. Each Horn
/// clause is indexed under **every** trigger atom in its body, so
/// whichever atom is satisfied *last* fires the (now-complete) clause.
/// Firing a clause whose other atoms aren't yet present is a cheap
/// `match_body` no-op (the duplicate-fire cost, bounded by body size).
#[derive(Debug, Default, Clone)]
struct ClauseIndexes {
    /// By class index: clauses with that class as an `X`-body atom.
    x_trigger: Vec<Vec<usize>>,
    /// By class index: clauses with that class as a successor-body atom.
    succ_trigger: Vec<Vec<usize>>,
    /// By role index: clauses with a body role atom on that role.
    role_trigger: Vec<Vec<usize>>,
    /// Clauses with an empty body (`⊤ → …`) — fire at every node.
    empty_body: Vec<usize>,
}

fn role_id_index(r: Role) -> usize {
    match r {
        Role::Named(x) | Role::Inverse(x) => x.index() as usize,
    }
}

/// Build the [`ClauseIndexes`] for the Horn clauses. Non-Horn clauses
/// are branch points handled by `find_open_disjunction`, not indexed.
fn build_clause_indexes(clauses: &[DlClause]) -> ClauseIndexes {
    let mut ix = ClauseIndexes::default();
    let push = |v: &mut Vec<Vec<usize>>, key: usize, ci: usize| {
        if key >= v.len() {
            v.resize(key + 1, Vec::new());
        }
        if v[key].last() != Some(&ci) {
            v[key].push(ci);
        }
    };
    for (ci, cl) in clauses.iter().enumerate() {
        if !cl.is_horn() {
            continue;
        }
        if cl.body.is_empty() {
            ix.empty_body.push(ci);
            continue;
        }
        for atom in &cl.body {
            match atom {
                Atom::Class(c, v) if *v == X => push(&mut ix.x_trigger, c.index() as usize, ci),
                Atom::Class(c, _) => push(&mut ix.succ_trigger, c.index() as usize, ci),
                Atom::Role(r, _, _) => push(&mut ix.role_trigger, role_id_index(*r), ci),
                // Head-only atoms never appear in a (Horn) body.
                Atom::Exists(..) | Atom::AtMost(..) | Atom::AtLeast(..) | Atom::Equal(..) => {}
            }
        }
    }
    ix
}

impl<'c> HyperEngine<'c> {
    /// Build an engine for `clauses` seeded with a single root node
    /// labelled `root`.
    #[must_use]
    pub fn new(clauses: &'c [DlClause], root: ClassId) -> Self {
        let mut root_node = HyperNode {
            order: 0,
            ..HyperNode::default()
        };
        root_node.add(root);
        Self {
            clauses,
            nodes: vec![root_node],
            stats: SearchStats::default(),
            init_depth: 0,
            deadline: None,
            indexes: build_clause_indexes(clauses),
            worklist: Vec::new(),
            representative: vec![HNode(0)],
            sub_roles: None,
            neq: Vec::new(),
        }
    }

    /// Supply the HF2 role hierarchy so `R`-edges satisfy `S`-atoms
    /// when `R ⊑* S`. Without it, role matching is reflexive only.
    #[must_use]
    pub fn with_sub_roles(mut self, hierarchy: RoleHierarchy) -> Self {
        self.sub_roles = Some(hierarchy);
        self
    }

    /// Resolve a node through the merge union-find to its canonical
    /// representative (H3c). Identity for un-merged nodes.
    fn resolve(&self, n: HNode) -> HNode {
        let mut r = n;
        while self.representative[r.index()] != r {
            r = self.representative[r.index()];
        }
        r
    }

    /// Add class `c` to node `n`, emitting a [`Event::Label`] on a
    /// *first* add (so its newly-enabled clauses fire). Returns whether
    /// the label was newly added.
    fn add_label(&mut self, n: HNode, c: ClassId) -> bool {
        if self.nodes[n.index()].add(c) {
            self.worklist.push(Event::Label(n, c));
            true
        } else {
            false
        }
    }

    /// Search instrumentation from the last [`decide`] call.
    #[must_use]
    pub fn stats(&self) -> SearchStats {
        self.stats
    }

    /// True iff every clause is Horn (≤1 head atom). H1 only
    /// handles this fragment; callers gate on it.
    #[must_use]
    pub fn all_horn(clauses: &[DlClause]) -> bool {
        clauses.iter().all(DlClause::is_horn)
    }

    fn new_node(&mut self) -> HNode {
        let id = u32::try_from(self.nodes.len()).expect("node count fits u32");
        self.nodes.push(HyperNode {
            order: id,
            ..HyperNode::default()
        });
        let n = HNode(id);
        self.representative.push(n);
        // Fire empty-body (`⊤ → …`) clauses at the new node.
        if !self.indexes.empty_body.is_empty() {
            self.worklist.push(Event::NodeNew(n));
        }
        n
    }

    /// Anywhere blocking: `n` is blocked if some *earlier-created*
    /// node `m` has `L(n) ⊆ L(m)`. A blocked node generates no
    /// successors (the witness `m` already realises everything `n`
    /// would). Sound for the Horn fragment (no inverse roles enter
    /// the blocking condition here; that refinement is H3).
    fn is_blocked(&self, n: HNode) -> bool {
        let ln = &self.nodes[n.index()];
        for m in &self.nodes {
            if m.order < ln.order && subset_sorted(&ln.labels, &m.labels) {
                return true;
            }
        }
        false
    }

    /// Run Horn hyperresolution to fixpoint. `max_iters` bounds the
    /// outer loop defensively. Disjunctive (non-Horn) clauses are
    /// skipped here — use [`HyperEngine::decide`] for branching.
    #[must_use]
    pub fn run(&mut self, max_iters: usize) -> HyperResult {
        self.horn_fixpoint(max_iters)
    }

    /// Saturate under the Horn fragment by a semi-naive event drain:
    /// re-seed the worklist from the current graph, then process each
    /// derivation event by firing only the clauses it newly enables.
    /// Firings emit more events (via [`add_label`]/edge creation),
    /// cascading to fixpoint. Non-Horn clauses are branch points
    /// ([`solve`]). `max_iters` caps total events processed defensively
    /// (anywhere blocking bounds the graph; hitting it yields
    /// `Stalled`). See `docs/hypertableau-seminaive-scoping.md`.
    fn horn_fixpoint(&mut self, max_iters: usize) -> HyperResult {
        self.stats.fixpoint_passes += 1;
        // Re-seed from scratch (keeps the worklist out of the cloned
        // branch state — seminaive scoping §4). A failed branch may
        // have left stale events; clearing here discards them and the
        // (restored) graph re-seeds correctly.
        self.worklist.clear();
        for idx in 0..self.nodes.len() {
            let n = HNode(u32::try_from(idx).expect("fits u32"));
            // Skip merged-away (non-canonical) nodes — their facts live
            // on the representative.
            if self.resolve(n) != n {
                continue;
            }
            if !self.indexes.empty_body.is_empty() {
                self.worklist.push(Event::NodeNew(n));
            }
            for c in self.nodes[idx].labels.clone() {
                self.worklist.push(Event::Label(n, c));
            }
            for (r, m) in self.nodes[idx].edges.clone() {
                self.worklist.push(Event::Edge(n, r, m));
            }
        }
        let mut steps = 0usize;
        while let Some(ev) = self.worklist.pop() {
            steps += 1;
            if steps > max_iters {
                return HyperResult::Stalled;
            }
            if matches!(self.process_event(ev), FireOutcome::Clash) {
                return HyperResult::Unsat;
            }
        }
        HyperResult::Sat
    }

    /// Fire the clauses an event newly enables. Reuses [`fire_clause`]
    /// (which re-verifies the full body), so over-firing on a not-yet-
    /// complete clause is a cheap no-op.
    fn process_event(&mut self, ev: Event) -> FireOutcome {
        match ev {
            Event::Label(n, c) => {
                let key = c.index() as usize;
                // Clauses with `c` as an `X`-class fire at `n`.
                let n_x = self.indexes.x_trigger.get(key).map_or(0, Vec::len);
                for i in 0..n_x {
                    let ci = self.indexes.x_trigger[key][i];
                    if matches!(self.fire_clause(ci, n), FireOutcome::Clash) {
                        return FireOutcome::Clash;
                    }
                }
                // Clauses with `c` as a successor-class fire at `n`'s
                // predecessors (back-propagation: a successor gained `c`).
                let n_s = self.indexes.succ_trigger.get(key).map_or(0, Vec::len);
                if n_s > 0 {
                    let preds: Vec<HNode> = self.nodes[n.index()]
                        .preds
                        .iter()
                        .map(|&(_, p)| p)
                        .collect();
                    for p in preds {
                        for i in 0..n_s {
                            let ci = self.indexes.succ_trigger[key][i];
                            if matches!(self.fire_clause(ci, p), FireOutcome::Clash) {
                                return FireOutcome::Clash;
                            }
                        }
                    }
                }
            }
            Event::Edge(src, role, _tgt) => {
                // Clauses with a body atom on this role fire at `src`;
                // these re-check the (now-present) successor's labels,
                // covering the edge-added-after-label case.
                let key = role_id_index(role);
                let n_r = self.indexes.role_trigger.get(key).map_or(0, Vec::len);
                for i in 0..n_r {
                    let ci = self.indexes.role_trigger[key][i];
                    if matches!(self.fire_clause(ci, src), FireOutcome::Clash) {
                        return FireOutcome::Clash;
                    }
                }
            }
            Event::NodeNew(n) => {
                for i in 0..self.indexes.empty_body.len() {
                    let ci = self.indexes.empty_body[i];
                    if matches!(self.fire_clause(ci, n), FireOutcome::Clash) {
                        return FireOutcome::Clash;
                    }
                }
            }
        }
        FireOutcome::NoChange
    }

    /// Decide satisfiability of the root concept over the **full**
    /// (Horn + disjunctive) clause set by backtracking search.
    ///
    /// Each step saturates under Horn propagation, then if an *open*
    /// disjunctive clause remains (body matched, no head disjunct yet
    /// satisfied) it branches: each disjunct is asserted in turn over
    /// a saved copy of the graph, recursing. Restore happens only on
    /// a failed (`Unsat`/`Stalled`) branch, so a `Sat` branch keeps
    /// its completion intact (and `root_labels` is meaningful after).
    ///
    /// `max_depth` bounds branching recursion. The three-valued
    /// result respects it: `Sat` if any branch is satisfiable;
    /// `Unsat` only if **every** branch is decisively unsatisfiable;
    /// `Stalled` if a branch hit the depth/iteration bound and no
    /// branch decisively succeeded (so we must not claim `Unsat`).
    #[must_use]
    pub fn decide(&mut self, max_depth: usize) -> HyperResult {
        self.decide_with_deadline(max_depth, None)
    }

    /// As [`decide`], but abort with `Stalled` once `deadline` passes
    /// (wall-clock budget per call). Resets [`stats`].
    #[must_use]
    pub fn decide_with_deadline(
        &mut self,
        max_depth: usize,
        deadline: Option<Instant>,
    ) -> HyperResult {
        self.stats = SearchStats::default();
        self.init_depth = max_depth;
        self.deadline = deadline;
        self.solve(max_depth)
    }

    fn solve(&mut self, depth: usize) -> HyperResult {
        if let Some(dl) = self.deadline
            && Instant::now() >= dl
        {
            return HyperResult::Stalled;
        }
        match self.horn_fixpoint(FIXPOINT_ITERS) {
            HyperResult::Unsat => return HyperResult::Unsat,
            HyperResult::Stalled => return HyperResult::Stalled,
            HyperResult::Sat => {}
        }
        // Disjunctive-head branching (H2).
        if let Some((ci, node, binding)) = self.find_open_disjunction() {
            if depth == 0 {
                return HyperResult::Stalled;
            }
            self.track_depth(depth);
            let head_len = self.clauses[ci].head.len();
            let mut any_stalled = false;
            for k in 0..head_len {
                let head_atom = self.clauses[ci].head[k];
                let saved = self.save();
                self.stats.branches_taken += 1;
                let _ = self.apply_head_atom(head_atom, node, &binding);
                match self.solve(depth - 1) {
                    HyperResult::Sat => return HyperResult::Sat,
                    HyperResult::Unsat => self.restore(saved),
                    HyperResult::Stalled => {
                        self.restore(saved);
                        any_stalled = true;
                    }
                }
            }
            return if any_stalled {
                HyperResult::Stalled
            } else {
                HyperResult::Unsat
            };
        }
        // `≤n` merge branching (H3c): merge one pair of the violating
        // node's successors per branch, recursing.
        if let Some((_node, succs)) = self.find_open_at_most() {
            if depth == 0 {
                return HyperResult::Stalled;
            }
            self.track_depth(depth);
            let mut any_stalled = false;
            for i in 0..succs.len() {
                for j in (i + 1)..succs.len() {
                    // A `≠`-forced pair can't be merged — that branch is
                    // unsat (the `≥n` ⋈ `≤n` clash). Skip it.
                    if self.are_neq(succs[i], succs[j]) {
                        continue;
                    }
                    let saved = self.save();
                    self.stats.branches_taken += 1;
                    if self.merge(succs[i], succs[j]) {
                        // Merge clashed on `≠` (defensive — pre-checked).
                        self.restore(saved);
                        continue;
                    }
                    match self.solve(depth - 1) {
                        HyperResult::Sat => return HyperResult::Sat,
                        HyperResult::Unsat => self.restore(saved),
                        HyperResult::Stalled => {
                            self.restore(saved);
                            any_stalled = true;
                        }
                    }
                }
            }
            return if any_stalled {
                HyperResult::Stalled
            } else {
                HyperResult::Unsat
            };
        }
        HyperResult::Sat
    }

    fn track_depth(&mut self, depth: usize) {
        let level = u32::try_from(self.init_depth - depth + 1).unwrap_or(u32::MAX);
        if level > self.stats.max_branch_depth {
            self.stats.max_branch_depth = level;
        }
    }

    /// Snapshot the mutable graph state for branch save/restore: the
    /// nodes, the merge union-find, and the `≠` relation (all revert on
    /// a failed branch).
    fn save(&mut self) -> (Vec<HyperNode>, Vec<HNode>, Vec<(HNode, HNode)>) {
        self.stats.node_clones += 1;
        (
            self.nodes.clone(),
            self.representative.clone(),
            self.neq.clone(),
        )
    }

    fn restore(&mut self, saved: (Vec<HyperNode>, Vec<HNode>, Vec<(HNode, HNode)>)) {
        self.nodes = saved.0;
        self.representative = saved.1;
        self.neq = saved.2;
        self.stats.restores += 1;
    }

    /// Find an *open* disjunctive clause: one whose body matches at
    /// some node-binding and **none** of whose head disjuncts is
    /// already satisfied there. A clause with a satisfied disjunct is
    /// not a branch point — skipping it avoids redundant branching.
    fn find_open_disjunction(&self) -> Option<(usize, HNode, Binding)> {
        for idx in 0..self.nodes.len() {
            let node = HNode(u32::try_from(idx).expect("fits u32"));
            for ci in 0..self.clauses.len() {
                if self.clauses[ci].is_horn() {
                    continue;
                }
                let Some(bindings) = self.match_body(ci, node) else {
                    continue;
                };
                for binding in bindings {
                    if !self.any_head_satisfied(ci, node, &binding) {
                        return Some((ci, node, binding));
                    }
                }
            }
        }
        None
    }

    /// True iff some head disjunct of clause `ci` already holds at
    /// the given binding (class label present, or `∃` witness found).
    fn any_head_satisfied(&self, ci: usize, xnode: HNode, binding: &Binding) -> bool {
        let resolve = |v: Var| resolve_var(v, xnode, binding);
        for head in &self.clauses[ci].head {
            match head {
                Atom::Class(c, v) => {
                    if let Some(t) = resolve(*v)
                        && self.nodes[t.index()].has(*c)
                    {
                        return true;
                    }
                }
                Atom::Exists(role, cls, v) => {
                    if let Some(src) = resolve(*v)
                        && self.nodes[src.index()].edges.iter().any(|(er, t)| {
                            role_matches(*er, *role, self.sub_roles.as_ref())
                                && self.nodes[t.index()].has(*cls)
                        })
                    {
                        return true;
                    }
                }
                Atom::AtMost(role, qual, n, v) => {
                    // Satisfied (no branch needed) if this `≤n` is
                    // already *asserted* on the node — we committed to
                    // this disjunct, and enforcement is now
                    // `find_open_at_most`'s job — or if it trivially
                    // holds (≤n matching successors already).
                    if let Some(src) = resolve(*v)
                        && (self.nodes[src.index()]
                            .at_most
                            .contains(&(*role, *qual, *n))
                            || self.distinct_role_succ(src, *role, *qual).len() <= *n as usize)
                    {
                        return true;
                    }
                }
                // TODO(HF3): `≥n` generation not yet enforced — never
                // counts as already-satisfied (sound for `Unsat`: an
                // unenforced `≥n` only weakens the theory).
                Atom::AtLeast(..) | Atom::Equal(..) | Atom::Role(..) => {}
            }
        }
        false
    }

    /// The *distinct* (representative-resolved) `role`-successors of
    /// `node`, filtered by the optional class qualifier.
    fn distinct_role_succ(&self, node: HNode, role: Role, qual: Option<ClassId>) -> Vec<HNode> {
        let mut seen: Vec<HNode> = Vec::new();
        for (er, t) in &self.nodes[node.index()].edges {
            if !role_matches(*er, role, self.sub_roles.as_ref()) {
                continue;
            }
            let rt = self.resolve(*t);
            if let Some(q) = qual
                && !self.nodes[rt.index()].has(q)
            {
                continue;
            }
            if !seen.contains(&rt) {
                seen.push(rt);
            }
        }
        seen
    }

    /// Find a node with a violated `≤n` constraint: more distinct
    /// matching `role`-successors than the bound. Returns the
    /// canonical node and its (resolved, distinct) successor list to
    /// branch merges over. Only canonical (un-merged) nodes are checked.
    fn find_open_at_most(&self) -> Option<(HNode, Vec<HNode>)> {
        for idx in 0..self.nodes.len() {
            let node = HNode(u32::try_from(idx).expect("fits u32"));
            if self.resolve(node) != node {
                continue;
            }
            for &(role, qual, n) in &self.nodes[idx].at_most {
                let succs = self.distinct_role_succ(node, role, qual);
                if succs.len() > n as usize {
                    return Some((node, succs));
                }
            }
        }
        None
    }

    /// Merge node `s_j` into `s_i` for the `≤n` rule (H3c): union
    /// `s_j`'s labels (through [`add_label`], so the disjointness clause
    /// fires on incompatible merges), redirect its out-edges, and union
    /// the `≤n`/`≥n`-done constraints. Returns `true` if the merge is a
    /// **clash** because `s_i ≠ s_j` is forced (`HF3a`) — what makes
    /// `≥2 ⊓ ≤1` unsat. First-phase scope: merges happen only among a
    /// root's direct successors, whose sole predecessor is the root
    /// (already linked to `s_i`), so predecessor redirection is
    /// unnecessary.
    fn merge(&mut self, s_i: HNode, s_j: HNode) -> bool {
        let (s_i, s_j) = (self.resolve(s_i), self.resolve(s_j));
        if s_i == s_j {
            return false;
        }
        if self.are_neq(s_i, s_j) {
            return true; // ≠ violated — merging is impossible.
        }
        self.representative[s_j.index()] = s_i;
        for c in self.nodes[s_j.index()].labels.clone() {
            self.add_label(s_i, c);
        }
        for (r, t) in self.nodes[s_j.index()].edges.clone() {
            self.nodes[s_i.index()].edges.push((r, t));
            self.nodes[t.index()].preds.push((r, s_i));
            self.worklist.push(Event::Edge(s_i, r, t));
        }
        for c in self.nodes[s_j.index()].at_most.clone() {
            if !self.nodes[s_i.index()].at_most.contains(&c) {
                self.nodes[s_i.index()].at_most.push(c);
            }
        }
        for c in self.nodes[s_j.index()].at_least_done.clone() {
            if !self.nodes[s_i.index()].at_least_done.contains(&c) {
                self.nodes[s_i.index()].at_least_done.push(c);
            }
        }
        false
    }

    /// Fire one clause with `x = node`. Handles the two body shapes
    /// the clausifier produces: class atoms on `x`, and at most one
    /// role atom `R(x,y)` binding a successor `y` (with optional
    /// class atoms on `y` — the EL back-propagation shape
    /// `R(x,y) ∧ E(y) → F(x)` from `∃R.E ⊑ F`). Bodies with two
    /// role atoms, equality, or a class on a third variable are not
    /// matched (deferred to later phases).
    fn fire_clause(&mut self, ci: usize, node: HNode) -> FireOutcome {
        // Disjunctive clauses are branch points, not Horn-fired here.
        if !self.clauses[ci].is_horn() {
            return FireOutcome::NoChange;
        }
        self.stats.match_attempts += 1;
        let Some(bindings) = self.match_body(ci, node) else {
            return FireOutcome::NoChange;
        };
        let mut changed = false;
        for binding in bindings {
            match self.fire_head(ci, node, &binding) {
                FireOutcome::Clash => return FireOutcome::Clash,
                FireOutcome::Changed => changed = true,
                FireOutcome::NoChange => {}
            }
        }
        if changed {
            FireOutcome::Changed
        } else {
            FireOutcome::NoChange
        }
    }

    /// Match clause `ci`'s body with `x = node`, enumerating every
    /// homomorphism of the body's variable-tree into the graph.
    ///
    /// Returns `None` when the body shape is **unsupported** (an
    /// equality/inverse atom, or a non-tree variable structure — a var
    /// that isn't reachable from `X` through role atoms, a var bound by
    /// two role atoms, or more than [`MAX_BODY_VARS`] vars). Otherwise
    /// returns every complete [`Binding`] (the non-`X` vars mapped to
    /// nodes) satisfying all role and class atoms — an **empty** vec
    /// when the shape is fine but nothing matches (a missing `X`-class
    /// or a role with no qualifying successor). This `None`-vs-empty
    /// distinction is the unsupported-vs-no-match boundary.
    fn match_body(&self, ci: usize, node: HNode) -> Option<Vec<Binding>> {
        let mut role_atoms: Vec<(Role, Var, Var)> = Vec::new();
        let mut other_classes: Vec<(ClassId, Var)> = Vec::new();
        let clause = &self.clauses[ci];
        for atom in &clause.body {
            match atom {
                Atom::Class(c, v) if *v == X => {
                    if !self.nodes[node.index()].has(*c) {
                        // X-class absent: shape OK, no match.
                        return Some(Vec::new());
                    }
                }
                Atom::Role(r, u, v) => role_atoms.push((*r, *u, *v)),
                Atom::Class(c, v) => other_classes.push((*c, *v)),
                // Equality / inverse-role bodies: later phases.
                _ => return None,
            }
        }

        // Topological order on the variable-tree: each role atom is
        // processed only once its source var is already bound. `None`
        // if the body isn't a tree rooted at `X` (cycle, disconnected,
        // or a var bound twice) or has too many vars.
        let order = eval_order(&role_atoms)?;
        let plan = MatchPlan {
            role_atoms: &role_atoms,
            order: &order,
            other_classes: &other_classes,
        };

        let mut out = Vec::new();
        let mut binding: Binding = Vec::new();
        self.enumerate_matches(node, &plan, 0, &mut binding, &mut out);
        Some(out)
    }

    /// Recursively bind role-atom targets to graph successors in
    /// `plan.order`, then (when all are bound) emit the binding if
    /// every class-on-successor constraint holds.
    fn enumerate_matches(
        &self,
        node: HNode,
        plan: &MatchPlan<'_>,
        i: usize,
        binding: &mut Binding,
        out: &mut Vec<Binding>,
    ) {
        if i == plan.order.len() {
            let ok = plan.other_classes.iter().all(|(c, v)| {
                resolve_var(*v, node, binding).is_some_and(|m| self.nodes[m.index()].has(*c))
            });
            if ok {
                let mut b = binding.clone();
                b.sort_unstable_by_key(|&(v, _)| v);
                out.push(b);
            }
            return;
        }
        let (role, src_var, tgt_var) = plan.role_atoms[plan.order[i]];
        let Some(src) = resolve_var(src_var, node, binding) else {
            return;
        };
        let hier = self.sub_roles.as_ref();
        let src_data = &self.nodes[src.index()];
        let mut targets: Vec<HNode> = src_data
            .edges
            .iter()
            .filter(|(er, _)| role_matches(*er, role, hier))
            .map(|(_, t)| *t)
            .collect();
        // Inverse-role matching (HF2): an incoming edge `s —er→ src`
        // asserts `er⁻(src, s)`, so it satisfies the wanted `role`
        // when `er.flip() == role` — i.e. following `R⁻` walks `src`'s
        // `R`-predecessors. (Merge does not redirect in-edges yet, but
        // merges are root-successor-only, so a stale pred is still a
        // sound R-relationship — TODO(HF3) when general merge lands.)
        for (er, s) in &src_data.preds {
            if role_matches(er.flip(), role, hier) {
                targets.push(*s);
            }
        }
        for m in targets {
            binding.push((tgt_var, m));
            self.enumerate_matches(node, plan, i + 1, binding, out);
            binding.pop();
        }
    }

    /// Assert the (single, Horn) head atom. `binding` maps the body's
    /// non-`X` variables to nodes.
    fn fire_head(&mut self, ci: usize, xnode: HNode, binding: &Binding) -> FireOutcome {
        let clause = &self.clauses[ci];
        if clause.head.is_empty() {
            // body → ⊥ : the body matched, so this is a clash.
            return FireOutcome::Clash;
        }
        // Horn: exactly one head atom (caller gated on is_horn).
        let head = clause.head[0];
        self.apply_head_atom(head, xnode, binding)
    }

    /// Assert one head atom (`Class` label or `∃` successor) at the
    /// resolved binding. Shared by Horn firing and disjunctive
    /// branching. Never reports a clash itself — clashes surface when
    /// a `body → ⊥` clause subsequently fires in [`horn_fixpoint`].
    fn apply_head_atom(&mut self, head: Atom, xnode: HNode, binding: &Binding) -> FireOutcome {
        match head {
            Atom::Class(c, v) => {
                let Some(target) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                if self.add_label(target, c) {
                    FireOutcome::Changed
                } else {
                    FireOutcome::NoChange
                }
            }
            Atom::Exists(role, cls, v) => {
                let Some(src) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                self.fire_exists(src, role, cls)
            }
            Atom::AtMost(role, qual, n, v) => {
                let Some(target) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                let c = (role, qual, n);
                if self.nodes[target.index()].at_most.contains(&c) {
                    FireOutcome::NoChange
                } else {
                    self.nodes[target.index()].at_most.push(c);
                    FireOutcome::Changed
                }
            }
            Atom::AtLeast(role, qual, n, v) => {
                let Some(target) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                self.generate_at_least(target, role, qual, n)
            }
            // TODO(HF3): self-loop `Role(x,x)` heads and `≈` equality
            // not yet realised — no-op (sound for `Unsat`: an
            // unenforced head only weakens the theory).
            Atom::Equal(_, _) | Atom::Role(..) => FireOutcome::NoChange,
        }
    }

    /// `∃role.cls` at `src`: reuse an existing role-successor that
    /// already carries `cls`; otherwise (if `src` isn't blocked)
    /// create a fresh successor seeded with `cls`.
    fn fire_exists(&mut self, src: HNode, role: Role, cls: ClassId) -> FireOutcome {
        // Witness reuse: any role-matching successor already in cls.
        let has_witness = self.nodes[src.index()].edges.iter().any(|(er, t)| {
            role_matches(*er, role, self.sub_roles.as_ref()) && self.nodes[t.index()].has(cls)
        });
        if has_witness {
            return FireOutcome::NoChange;
        }
        if self.is_blocked(src) {
            // Blocked: the witness ancestor already realises this
            // existential; don't generate.
            return FireOutcome::NoChange;
        }
        let succ = self.new_node();
        self.nodes[src.index()].edges.push((role, succ));
        self.nodes[succ.index()].preds.push((role, src));
        // The new edge fires role-triggered clauses at `src`; the seed
        // label fires the successor's clauses (and, via Event::Label,
        // back-prop at `src`).
        self.worklist.push(Event::Edge(src, role, succ));
        self.add_label(succ, cls);
        FireOutcome::Changed
    }

    /// Record `a ≠ b` (`HF3a`), resolved to representatives. Idempotent.
    fn add_neq(&mut self, a: HNode, b: HNode) {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b {
            return;
        }
        let pair = (a.min(b), a.max(b));
        if !self.neq.contains(&pair) {
            self.neq.push(pair);
        }
    }

    /// Whether `a ≠ b` is forced. Resolves both args *and* each stored
    /// pair through the merge union-find, so the relation stays correct
    /// after merges without rewriting the store.
    fn are_neq(&self, a: HNode, b: HNode) -> bool {
        let (ra, rb) = (self.resolve(a), self.resolve(b));
        if ra == rb {
            return false;
        }
        self.neq.iter().any(|&(p, q)| {
            let (rp, rq) = (self.resolve(p), self.resolve(q));
            (rp == ra && rq == rb) || (rp == rb && rq == ra)
        })
    }

    /// `HF3a` `≥n role.qual` generation at `x`: create `n` fresh,
    /// pairwise-`≠` `role`-successors seeded with `qual`. Deterministic
    /// (not a branch point), so it runs in the Horn fixpoint via
    /// [`Self::apply_head_atom`]. Three guards, in order: **count-based**
    /// (skip if `x` already has `n` distinct `qual`-successors — the
    /// load-bearing one for performance), **fire-once** per
    /// `(role, qual, n)` (regen defense), and **blocking** (a blocked
    /// node generates nothing — termination). Inverse-role `≥n` is
    /// deferred (TODO HF3): generating predecessors is a separate path
    /// the corpus doesn't exercise.
    fn generate_at_least(
        &mut self,
        x: HNode,
        role: Role,
        qual: Option<ClassId>,
        n: u32,
    ) -> FireOutcome {
        if n == 0 || role.is_inverse() {
            return FireOutcome::NoChange;
        }
        let x = self.resolve(x);
        // Count-based guard: if `x` already has `n` distinct `qual`-R-
        // successors (e.g. from `∃`), `≥n` is already satisfied — don't
        // generate. This keeps cardinality-rich refutations (pizza
        // `InterestingPizza`, which already has its toppings via `∃`)
        // from ballooning the `≤n` merge tree past the search budget.
        // `distinct_role_succ` resolves through the merge map.
        //
        // Regen-hole invariant (verified by probes A–D in tests):
        // count-based skip + generate-`n`-fresh + fire-once-only-on-fire
        // jointly avoid an incomplete `Sat`. Generation never sets
        // fire-once without also adding the `≠`-witnesses, so once it has
        // fired a later `≤n` merge can't drop `distinct < n`; and if it
        // was *skipped*, fire-once is unset, so the rule can still fire
        // after a merge reduces the count. Scope of this claim: HF3a
        // (no inverse `≥n`, no nominal-induced cardinality, anywhere
        // blocking) — not a general SROIQ termination theorem.
        if self.distinct_role_succ(x, role, qual).len() >= n as usize {
            return FireOutcome::NoChange;
        }
        let key = (role, qual, n);
        if self.nodes[x.index()].at_least_done.contains(&key) {
            return FireOutcome::NoChange;
        }
        if self.is_blocked(x) {
            return FireOutcome::NoChange;
        }
        self.nodes[x.index()].at_least_done.push(key);
        let mut fresh = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let succ = self.new_node();
            self.nodes[x.index()].edges.push((role, succ));
            self.nodes[succ.index()].preds.push((role, x));
            self.worklist.push(Event::Edge(x, role, succ));
            if let Some(q) = qual {
                self.add_label(succ, q);
            }
            fresh.push(succ);
        }
        for i in 0..fresh.len() {
            for j in (i + 1)..fresh.len() {
                self.add_neq(fresh[i], fresh[j]);
            }
        }
        FireOutcome::Changed
    }

    /// Number of nodes in the completion graph (diagnostic).
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Class labels of the root node (node 0) — the derived
    /// subsumers of the root concept, for EL-closure cross-checks.
    #[must_use]
    pub fn root_labels(&self) -> &[ClassId] {
        &self.nodes[0].labels
    }
}

enum FireOutcome {
    Clash,
    Changed,
    NoChange,
}

/// The fixed structure of a body match: the role atoms, a
/// topological evaluation order over them, and the class-on-successor
/// constraints. Built once per `match_body` call, borrowed by the
/// recursive [`HyperEngine::enumerate_matches`].
struct MatchPlan<'p> {
    role_atoms: &'p [(Role, Var, Var)],
    order: &'p [usize],
    other_classes: &'p [(ClassId, Var)],
}

/// Order role atoms so every atom's source variable is bound before
/// it (BFS from `X`). `None` if the variables don't form a tree rooted
/// at `X` (unbindable source, duplicate target, or more than
/// [`MAX_BODY_VARS`] vars) — an unsupported shape.
fn eval_order(role_atoms: &[(Role, Var, Var)]) -> Option<Vec<usize>> {
    let mut bound: Vec<Var> = vec![X];
    let mut order = Vec::with_capacity(role_atoms.len());
    let mut used = vec![false; role_atoms.len()];
    while order.len() < role_atoms.len() {
        let mut progressed = false;
        for (i, (_, u, v)) in role_atoms.iter().enumerate() {
            if used[i] || !bound.contains(u) {
                continue;
            }
            if bound.contains(v) {
                // `v` already bound ⇒ not a tree (two role atoms
                // target the same var, or a cycle). Unsupported.
                return None;
            }
            used[i] = true;
            bound.push(*v);
            order.push(i);
            progressed = true;
            if bound.len() > MAX_BODY_VARS {
                return None;
            }
        }
        if !progressed {
            // Remaining atoms have unbindable sources (disconnected
            // from `X`). Unsupported.
            return None;
        }
    }
    Some(order)
}

/// Resolve a clause variable to a graph node: `X` is the match root
/// `xnode`; any other variable is looked up in `binding`. `None` if an
/// unbound non-`X` variable (e.g. a head var with no body role atom).
fn resolve_var(v: Var, xnode: HNode, binding: &[(Var, HNode)]) -> Option<HNode> {
    if v == X {
        Some(xnode)
    } else {
        binding.iter().find(|(bv, _)| *bv == v).map(|&(_, n)| n)
    }
}

/// An `edge` satisfies a `wanted` role atom when their polarities agree
/// and the edge's role is a sub-role of (or equal to) the wanted role.
/// `R ⊑ S` implies `R⁻ ⊑ S⁻`, so the same-polarity + sub-role-id test
/// covers both axes. With no hierarchy (`None`), this is reflexive —
/// equal ids only, the pre-HF2 behaviour.
fn role_matches(edge: Role, wanted: Role, sub_roles: Option<&RoleHierarchy>) -> bool {
    if edge.is_inverse() != wanted.is_inverse() {
        return false;
    }
    match sub_roles {
        Some(h) => h.is_sub_role(edge.role_id(), wanted.role_id()),
        None => edge.role_id() == wanted.role_id(),
    }
}

/// `a ⊆ b` for sorted-by-index class-id slices.
fn subset_sorted(a: &[ClassId], b: &[ClassId]) -> bool {
    let mut bi = b.iter();
    'outer: for x in a {
        for y in bi.by_ref() {
            if y.index() == x.index() {
                continue 'outer;
            }
            if y.index() > x.index() {
                return false;
            }
        }
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::{ClassId, Role, RoleId};

    fn cls(i: u32) -> ClassId {
        ClassId::new(i)
    }

    #[test]
    fn horn_chain_derives_transitive_subsumers() {
        // A(x)→B(x), B(x)→C(x). Root A ⇒ root labels {A,B,C}, Sat.
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![Atom::Class(cls(2), X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        assert_eq!(e.root_labels(), &[cls(0), cls(1), cls(2)]);
    }

    #[test]
    fn disjointness_clause_makes_root_unsat() {
        // A(x)→B(x), A(x)∧B(x)→⊥. Root A ⇒ Unsat.
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(0), X), Atom::Class(cls(1), X)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Unsat);
    }

    #[test]
    fn cyclic_existential_terminates_via_blocking() {
        // A(x)→∃R.A(x). Naively infinite; anywhere blocking caps it.
        let r = Role::Named(RoleId::new(0));
        let clauses = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Exists(r, cls(0), X)],
        }];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        // Root + one successor, then the successor is blocked by the
        // root (same label set {A}).
        assert!(
            e.node_count() <= 2,
            "blocking should cap at 2 nodes, got {}",
            e.node_count()
        );
    }

    #[test]
    fn forall_propagates_into_successor() {
        // A(x)→∃R.B(x); A(x)∧R(x,y)→C(y). The R-successor (seeded B)
        // also gains C. Root stays sat.
        let r = Role::Named(RoleId::new(0));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Exists(r, cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(0), X), Atom::Role(r, X, 1)],
                head: vec![Atom::Class(cls(2), 1)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        // Two nodes: root {A}, successor {B,C}.
        assert_eq!(e.node_count(), 2);
    }

    #[test]
    fn existential_backprop_derives_subsumer_on_root() {
        // The EL `∃R.E ⊑ F` shape, hand-clausified as
        // `R(x,y) ∧ E(y) → F(x)`. With C ⊑ ∃R.D, D ⊑ E, the root
        // (C) must gain F via back-propagation from its successor.
        // Proves the engine handles class-atoms on the successor
        // variable in a body (the fire_clause class-on-y fix),
        // independent of the clausifier (which doesn't yet produce
        // this clause from ∃-on-LHS — see hyper Phase H1b note).
        let r = Role::Named(RoleId::new(0));
        let c = cls(0);
        let d = cls(1);
        let e_cls = cls(2);
        let f = cls(3);
        let clauses = vec![
            // C(x) → ∃R.D(x)
            DlClause {
                body: vec![Atom::Class(c, X)],
                head: vec![Atom::Exists(r, d, X)],
            },
            // D(x) → E(x)
            DlClause {
                body: vec![Atom::Class(d, X)],
                head: vec![Atom::Class(e_cls, X)],
            },
            // R(x,y) ∧ E(y) → F(x)
            DlClause {
                body: vec![Atom::Role(r, X, 1), Atom::Class(e_cls, 1)],
                head: vec![Atom::Class(f, X)],
            },
        ];
        let mut engine = HyperEngine::new(&clauses, c);
        assert_eq!(engine.run(1024), HyperResult::Sat);
        assert!(
            engine.root_labels().contains(&f),
            "root must gain F via ∃R.E⊑F back-prop; labels={:?}",
            engine.root_labels()
        );
    }

    #[test]
    fn universal_body_fires_everywhere() {
        // ⊤(x)→T(x): every node gains T. Root A ⇒ {A,T}.
        let clauses = vec![DlClause {
            body: vec![],
            head: vec![Atom::Class(cls(9), X)],
        }];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        assert_eq!(e.root_labels(), &[cls(0), cls(9)]);
    }

    #[test]
    fn all_horn_detects_disjunctive_clause() {
        let horn = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X)],
        }];
        assert!(HyperEngine::all_horn(&horn));
        let disj = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
        }];
        assert!(!HyperEngine::all_horn(&disj));
    }

    // ---- H2: disjunctive-head branching ----

    /// `A ⊑ B ⊔ C` with no further constraint: both disjuncts lead to
    /// a clash-free completion, so the root is Sat. Neither B nor C is
    /// *forced* — the first disjunct (B) is chosen and the search
    /// succeeds immediately, so the completion carries B (not C).
    #[test]
    fn disjunction_sat_takes_first_branch() {
        let clauses = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
        }];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert!(e.root_labels().contains(&cls(1)));
        assert!(!e.root_labels().contains(&cls(2)));
    }

    /// `A ⊑ B ⊔ C`, `B ⊑ ⊥`: the first disjunct clashes, the search
    /// restores and takes the second, so the root is Sat carrying C.
    /// Exercises the restore-on-Unsat path.
    #[test]
    fn disjunction_backtracks_to_second_branch() {
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
            },
            // B ⊑ ⊥
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert!(
            e.root_labels().contains(&cls(2)),
            "second disjunct C must survive; labels={:?}",
            e.root_labels()
        );
        assert!(
            !e.root_labels().contains(&cls(1)),
            "first disjunct B must have been restored away; labels={:?}",
            e.root_labels()
        );
    }

    /// `A ⊑ B ⊔ C`, `B ⊑ ⊥`, `C ⊑ ⊥`: both disjuncts clash, so the
    /// root is decisively Unsat (exhaustive branch failure).
    #[test]
    fn disjunction_both_branches_clash_is_unsat() {
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![],
            },
            DlClause {
                body: vec![Atom::Class(cls(2), X)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// Multi-level backtracking — the test that catches restore bugs.
    /// `A ⊑ B ⊔ C`, `B ⊑ D ⊔ E`, `D ⊑ ⊥`, `E ⊑ ⊥`, `C ⊑ ⊥`.
    /// Taking B forces a nested split (D⊔E) whose disjuncts both
    /// clash, so B is unsat; C also clashes, so the root is Unsat.
    #[test]
    fn nested_disjunction_exhaustive_failure_is_unsat() {
        let bot = |c: u32| DlClause {
            body: vec![Atom::Class(cls(c), X)],
            head: vec![],
        };
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![Atom::Class(cls(3), X), Atom::Class(cls(4), X)],
            },
            bot(3),
            bot(4),
            bot(2),
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// Nested split where the deep branch is satisfiable: same shape
    /// as above but `E` is left clash-free. Taking B then E yields a
    /// completion, so the root is Sat carrying B and E.
    #[test]
    fn nested_disjunction_finds_deep_model() {
        let bot = |c: u32| DlClause {
            body: vec![Atom::Class(cls(c), X)],
            head: vec![],
        };
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![Atom::Class(cls(3), X), Atom::Class(cls(4), X)],
            },
            bot(3), // D ⊑ ⊥ — first nested disjunct fails
            bot(2), // C ⊑ ⊥ — outer second disjunct fails
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert!(e.root_labels().contains(&cls(1)));
        assert!(e.root_labels().contains(&cls(4)));
    }

    /// Depth-bound respect: when *every* branch needs a split deeper
    /// than `max_depth`, the result is `Stalled` (undetermined) —
    /// never a false `Unsat`. `A ⊑ B ⊔ C`, `B ⊑ D ⊔ E`, `C ⊑ F ⊔ G`:
    /// both outer disjuncts leave a nested disjunction open, and
    /// `max_depth = 1` permits only the first split. Both sub-branches
    /// stall, so the overall result is Stalled (the ontology is in
    /// fact satisfiable — Stalled is the conservative "don't know").
    #[test]
    fn shallow_depth_bound_yields_stalled_not_unsat() {
        let split = |a: u32, l: u32, r: u32| DlClause {
            body: vec![Atom::Class(cls(a), X)],
            head: vec![Atom::Class(cls(l), X), Atom::Class(cls(r), X)],
        };
        let clauses = vec![split(0, 1, 2), split(1, 3, 4), split(2, 5, 6)];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(1), HyperResult::Stalled);
        // With enough depth the same ontology is decisively Sat.
        let mut e2 = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e2.decide(64), HyperResult::Sat);
    }

    /// Disjunction already satisfied is not a branch point: if a head
    /// disjunct is forced true by Horn propagation, `decide` must not
    /// branch on it. `A ⊑ B`, `A ⊑ B ⊔ C` ⇒ Sat, and `find_open`
    /// finds nothing because B is already present.
    #[test]
    fn satisfied_disjunct_is_not_a_branch_point() {
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        // Horn propagation forces B before any branching, so the
        // disjunction is already satisfied — `decide` must not branch
        // and therefore must not add the unforced second disjunct C.
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert!(e.root_labels().contains(&cls(1)));
        assert!(!e.root_labels().contains(&cls(2)));
    }

    /// Multi-role body (two-role chain): `A(x) ∧ R(x,y) ∧ B(y) ∧
    /// S(y,z) ∧ C(z) → D(x)`. With `A ⊑ ∃R.B`, `B ⊑ ∃S.C` the root
    /// (A) gains a chain `x —R→ y(B) —S→ z(C)`, so the chain clause
    /// fires and the root gains D. The `SpicyPizzaEquivalent` shape.
    #[test]
    fn multi_role_chain_body_fires() {
        let role_r = Role::Named(RoleId::new(0));
        let role_s = Role::Named(RoleId::new(1));
        let (ca, cb, cc, cd) = (cls(0), cls(1), cls(2), cls(3));
        let clauses = vec![
            // A(x) → ∃R.B(x)
            DlClause {
                body: vec![Atom::Class(ca, X)],
                head: vec![Atom::Exists(role_r, cb, X)],
            },
            // B(x) → ∃S.C(x)
            DlClause {
                body: vec![Atom::Class(cb, X)],
                head: vec![Atom::Exists(role_s, cc, X)],
            },
            // A(x) ∧ R(x,y) ∧ B(y) ∧ S(y,z) ∧ C(z) → D(x)
            DlClause {
                body: vec![
                    Atom::Class(ca, X),
                    Atom::Role(role_r, X, 1),
                    Atom::Class(cb, 1),
                    Atom::Role(role_s, 1, 2),
                    Atom::Class(cc, 2),
                ],
                head: vec![Atom::Class(cd, X)],
            },
        ];
        let mut engine = HyperEngine::new(&clauses, ca);
        assert_eq!(engine.run(1024), HyperResult::Sat);
        assert!(
            engine.root_labels().contains(&cd),
            "root must gain D via the two-role chain; labels={:?}",
            engine.root_labels()
        );
    }

    // ---- H3c: ≤n merge ----

    /// `≤1 R` with two disjoint `R`-successors is Unsat: the merge
    /// rule must identify them, and `A ⊓ B → ⊥` clashes. `C ⊑ ∃R.A`,
    /// `C ⊑ ∃R.B`, `A ⊓ B ⊑ ⊥`, `C ⊑ ≤1 R`.
    #[test]
    fn at_most_one_with_two_disjoint_successors_is_unsat() {
        let role = Role::Named(RoleId::new(0));
        let (root, ca, cb) = (cls(0), cls(1), cls(2));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, ca, X)],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, cb, X)],
            },
            DlClause {
                body: vec![Atom::Class(ca, X), Atom::Class(cb, X)],
                head: vec![],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, None, 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// `HF3a` canary: `≥2 R.⊤ ⊓ ≤1 R.⊤` is unsat. `≥2` must generate two
    /// pairwise-`≠` R-successors; `≤1` then forces a merge, but the `≠`
    /// makes the merge clash. Today `≥n` is a no-op, so this wrongly
    /// reports Sat. See `docs/hypertableau-hf3-scoping.md` §2.
    #[test]
    fn at_least_two_with_at_most_one_is_unsat() {
        let role = Role::Named(RoleId::new(0));
        let root = cls(0);
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtLeast(role, None, 2, X)],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, None, 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// `HF3a` boundary canary: `≥2 R.⊤ ⊓ ≤2 R.⊤` is **Sat** (n == m, no
    /// clash). Two `≠` successors generated, `≤2` is satisfied without
    /// merging. Pins the off-by-one in the count comparison: `≥n` fires
    /// at count 0 < 2, and `find_open_at_most` does not flag 2 ≤ 2.
    #[test]
    fn at_least_two_with_at_most_two_is_sat() {
        let role = Role::Named(RoleId::new(0));
        let root = cls(0);
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtLeast(role, None, 2, X)],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, None, 2, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(64), HyperResult::Sat);
    }

    /// `HF3a` termination canary: cyclic `A ⊑ ≥2 R.A` is **Sat** and must
    /// terminate. Generation creates two `A`-successors; each carries
    /// `{A} ⊆ {A}` of the root, so anywhere blocking blocks them and
    /// they generate nothing further. Proves the blocking-gates-`≥n`
    /// invariant (a no-block design would loop forever). See
    /// `docs/hypertableau-hf3-scoping.md` §1 `HF3b`.
    #[test]
    fn at_least_cyclic_terminates_sat() {
        let role = Role::Named(RoleId::new(0));
        let a = cls(0);
        let clauses = vec![DlClause {
            body: vec![Atom::Class(a, X)],
            head: vec![Atom::AtLeast(role, Some(a), 2, X)],
        }];
        let mut e = HyperEngine::new(&clauses, a);
        assert_eq!(e.decide(64), HyperResult::Sat);
    }

    /// `HF3b` probe A: count-based hole. `A ⊑ ∃R.C`, `A ⊑ ≥2 R.C`,
    /// `A ⊑ ≤1 R.C` — should be Unsat (≥2 ⊓ ≤1 contradict) even with a
    /// pre-existing `∃` successor.
    #[test]
    fn hf3b_probe_existing_succ_plus_geq_leq_is_unsat() {
        let role = Role::Named(RoleId::new(0));
        let (a, c) = (cls(0), cls(1));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, c, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtLeast(role, Some(c), 2, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtMost(role, Some(c), 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, a);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// `HF3b` probe B: non-root cardinality. `A ⊑ ∃R.B`, `B ⊑ ≥2 S.C`,
    /// `B ⊑ ≤1 S.C` — the `B`-node is a *successor* of the root; its
    /// `≥2 ⊓ ≤1` clash must propagate (making `A` unsat).
    #[test]
    fn hf3b_probe_nonroot_cardinality_clash_is_unsat() {
        let (role_r, role_s) = (Role::Named(RoleId::new(0)), Role::Named(RoleId::new(1)));
        let (ca, cb, cc) = (cls(0), cls(1), cls(2));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(ca, X)],
                head: vec![Atom::Exists(role_r, cb, X)],
            },
            DlClause {
                body: vec![Atom::Class(cb, X)],
                head: vec![Atom::AtLeast(role_s, Some(cc), 2, X)],
            },
            DlClause {
                body: vec![Atom::Class(cb, X)],
                head: vec![Atom::AtMost(role_s, Some(cc), 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, ca);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// `HF3b` probe C: termination ping-pong. Cyclic `A ⊑ ≥2 R.A` with
    /// `A ⊑ ≤1 R.A` — generation then forced `≠` merge clash; must
    /// terminate as Unsat (not loop via generate↔merge).
    #[test]
    fn hf3b_probe_cyclic_geq_leq_terminates_unsat() {
        let role = Role::Named(RoleId::new(0));
        let a = cls(0);
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtLeast(role, Some(a), 2, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtMost(role, Some(a), 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, a);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// `HF3b` probe D: the exact `TODO`-warned skip case. Two *distinct*
    /// `∃` successors both `C` (so `≥2 R.C` is count-satisfied and
    /// generation is skipped, leaving them un-`≠`), then `≤1 R.C`
    /// merges them — must still be Unsat. Works because the count-based
    /// skip does **not** set fire-once, so after the merge drops the
    /// count, generation fires (creating `≠` successors) and the next
    /// `≤1` merge clashes. Confirms the regen hole flagged for `HF3b`
    /// is not reachable under the generate-`n`-fresh design.
    #[test]
    fn hf3b_probe_skip_then_merge_is_unsat() {
        let role = Role::Named(RoleId::new(0));
        let (a, c, c1, c2) = (cls(0), cls(1), cls(2), cls(3));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, c1, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, c2, X)],
            },
            DlClause {
                body: vec![Atom::Class(c1, X)],
                head: vec![Atom::Class(c, X)],
            },
            DlClause {
                body: vec![Atom::Class(c2, X)],
                head: vec![Atom::Class(c, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtLeast(role, Some(c), 2, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtMost(role, Some(c), 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, a);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// `≤2 R` with two successors is Sat — no merge needed.
    #[test]
    fn at_most_two_with_two_successors_is_sat() {
        let role = Role::Named(RoleId::new(0));
        let (root, ca, cb) = (cls(0), cls(1), cls(2));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, ca, X)],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, cb, X)],
            },
            DlClause {
                body: vec![Atom::Class(ca, X), Atom::Class(cb, X)],
                head: vec![],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, None, 2, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(64), HyperResult::Sat);
    }

    /// `≤2 R` with three pairwise-disjoint successors is Unsat (the
    /// `InterestingPizza` shape): every pairwise merge clashes.
    #[test]
    fn at_most_two_with_three_disjoint_successors_is_unsat() {
        let role = Role::Named(RoleId::new(0));
        let (root, ca, cb, cd) = (cls(0), cls(1), cls(2), cls(3));
        let bot2 = |lhs, rhs| DlClause {
            body: vec![Atom::Class(lhs, X), Atom::Class(rhs, X)],
            head: vec![],
        };
        let exists = |inner| DlClause {
            body: vec![Atom::Class(root, X)],
            head: vec![Atom::Exists(role, inner, X)],
        };
        let clauses = vec![
            exists(ca),
            exists(cb),
            exists(cd),
            bot2(ca, cb),
            bot2(ca, cd),
            bot2(cb, cd),
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, None, 2, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(64), HyperResult::Unsat);
    }

    /// Instrumentation: branching tracked when it happens, zero on
    /// pure-Horn input (the "fast but branches==0 says nothing" guard
    /// for the H2b wall measurement).
    #[test]
    fn stats_track_branching_and_are_zero_on_horn() {
        // Horn: no branches.
        let horn = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X)],
        }];
        let mut e = HyperEngine::new(&horn, cls(0));
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert_eq!(e.stats().branches_taken, 0);
        assert_eq!(e.stats().max_branch_depth, 0);

        // Disjunction with a clashing first branch: ≥2 disjuncts
        // asserted, ≥1 restore, depth ≥1.
        let disj = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&disj, cls(0));
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert_eq!(e.stats().branches_taken, 2);
        assert_eq!(e.stats().restores, 1);
        assert_eq!(e.stats().max_branch_depth, 1);
    }

    /// `decide` reduces to the Horn fixpoint when the clause set is
    /// all-Horn: same Sat result and root labels as `run`.
    #[test]
    fn decide_matches_run_on_horn_input() {
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![Atom::Class(cls(2), X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.decide(64), HyperResult::Sat);
        assert_eq!(e.root_labels(), &[cls(0), cls(1), cls(2)]);
    }
}
