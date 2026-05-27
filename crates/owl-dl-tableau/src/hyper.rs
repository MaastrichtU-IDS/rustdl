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

use owl_dl_core::clause::{Atom, DlClause, Var, X};
use owl_dl_core::ir::{ClassId, Role, RoleId};
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
    /// `horn_fixpoint` outer-loop passes summed across the search.
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
    /// Horn-clause index by *representative trigger* — the first
    /// `Class(_, X)` body atom's class. `trigger_index[c.index()]`
    /// lists the Horn clauses that can only fire at a node carrying
    /// class `c`. A clause needs *all* its `X`-classes present, but
    /// the representative being absent already rules it out, so the
    /// fixpoint only attempts trigger-present clauses instead of the
    /// whole clause set (see the §profiling note in the doc).
    trigger_index: Vec<Vec<usize>>,
    /// Horn clauses with no `X`-class body atom (role-only / `⊤`
    /// bodies) — no trigger, so attempted at every node.
    untriggered: Vec<usize>,
}

/// Build the Horn-clause trigger index: clauses keyed by their first
/// `Class(_, X)` body atom (the representative trigger), plus the
/// untriggered (no-`X`-class) Horn clauses. Non-Horn clauses are
/// branch points handled by `find_open_disjunction`, not indexed here.
fn build_trigger_index(clauses: &[DlClause]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let repr = |cl: &DlClause| -> Option<usize> {
        cl.body.iter().find_map(|a| match a {
            Atom::Class(c, v) if *v == X => Some(c.index() as usize),
            _ => None,
        })
    };
    let max_trigger = clauses
        .iter()
        .filter(|cl| cl.is_horn())
        .filter_map(repr)
        .max()
        .map_or(0, |m| m + 1);
    let mut index: Vec<Vec<usize>> = vec![Vec::new(); max_trigger];
    let mut untriggered = Vec::new();
    for (ci, cl) in clauses.iter().enumerate() {
        if !cl.is_horn() {
            continue;
        }
        match repr(cl) {
            Some(c) => index[c].push(ci),
            None => untriggered.push(ci),
        }
    }
    (index, untriggered)
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
        let (trigger_index, untriggered) = build_trigger_index(clauses);
        Self {
            clauses,
            nodes: vec![root_node],
            stats: SearchStats::default(),
            init_depth: 0,
            deadline: None,
            trigger_index,
            untriggered,
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
        HNode(id)
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

    /// Saturate under the Horn fragment. Non-Horn clauses are
    /// ignored ([`fire_clause`] guards on `is_horn`); branching is
    /// the caller's job ([`solve`]).
    fn horn_fixpoint(&mut self, max_iters: usize) -> HyperResult {
        for _ in 0..max_iters {
            self.stats.fixpoint_passes += 1;
            let mut changed = false;
            // Snapshot node count; new successors are processed on
            // the next outer pass.
            let n_count = self.nodes.len();
            for idx in 0..n_count {
                let node = HNode(u32::try_from(idx).expect("fits u32"));
                match self.fire_clauses_at(node) {
                    FireOutcome::Clash => return HyperResult::Unsat,
                    FireOutcome::Changed => changed = true,
                    FireOutcome::NoChange => {}
                }
            }
            if !changed {
                return HyperResult::Sat;
            }
        }
        HyperResult::Stalled
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
        let Some((ci, node, binding)) = self.find_open_disjunction() else {
            return HyperResult::Sat;
        };
        if depth == 0 {
            // Branching needed but the budget is exhausted — undetermined.
            return HyperResult::Stalled;
        }
        let level = u32::try_from(self.init_depth - depth + 1).unwrap_or(u32::MAX);
        if level > self.stats.max_branch_depth {
            self.stats.max_branch_depth = level;
        }
        let head_len = self.clauses[ci].head.len();
        let mut any_stalled = false;
        for k in 0..head_len {
            let head_atom = self.clauses[ci].head[k];
            let saved = self.nodes.clone();
            self.stats.node_clones += 1;
            self.stats.branches_taken += 1;
            let _ = self.apply_head_atom(head_atom, node, &binding);
            match self.solve(depth - 1) {
                // Keep the satisfiable branch's graph; do not restore.
                HyperResult::Sat => return HyperResult::Sat,
                HyperResult::Unsat => {
                    self.nodes = saved;
                    self.stats.restores += 1;
                }
                HyperResult::Stalled => {
                    self.nodes = saved;
                    self.stats.restores += 1;
                    any_stalled = true;
                }
            }
        }
        // Every disjunct failed. If any was merely undetermined we
        // cannot soundly conclude Unsat.
        if any_stalled {
            HyperResult::Stalled
        } else {
            HyperResult::Unsat
        }
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
                            role_matches(*er, *role) && self.nodes[t.index()].has(*cls)
                        })
                    {
                        return true;
                    }
                }
                Atom::Equal(..) | Atom::Role(..) => {}
            }
        }
        false
    }

    /// Fire the Horn clauses that can match at `node`: the untriggered
    /// ones plus those whose representative trigger class is present in
    /// the node's labels (via [`trigger_index`]) — not the whole clause
    /// set. A clause whose trigger is absent cannot fire, so this is
    /// complete; [`match_body`] still verifies the rest of each body.
    fn fire_clauses_at(&mut self, node: HNode) -> FireOutcome {
        // Collect candidate clause indices first (so the immutable
        // label borrow is released before `fire_clause`'s mutation).
        let mut cands: Vec<usize> = self.untriggered.clone();
        for &l in &self.nodes[node.index()].labels {
            if let Some(v) = self.trigger_index.get(l.index() as usize) {
                cands.extend_from_slice(v);
            }
        }
        let mut changed = false;
        for ci in cands {
            match self.fire_clause(ci, node) {
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
        let targets: Vec<HNode> = self.nodes[src.index()]
            .edges
            .iter()
            .filter(|(er, _)| role_matches(*er, role))
            .map(|(_, t)| *t)
            .collect();
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
                if self.nodes[target.index()].add(c) {
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
            // Equality heads (≤n) are H3; not produced yet.
            Atom::Equal(_, _) | Atom::Role(..) => FireOutcome::NoChange,
        }
    }

    /// `∃role.cls` at `src`: reuse an existing role-successor that
    /// already carries `cls`; otherwise (if `src` isn't blocked)
    /// create a fresh successor seeded with `cls`.
    fn fire_exists(&mut self, src: HNode, role: Role, cls: ClassId) -> FireOutcome {
        // Witness reuse: any role-matching successor already in cls.
        let has_witness = self.nodes[src.index()]
            .edges
            .iter()
            .any(|(er, t)| role_matches(*er, role) && self.nodes[t.index()].has(cls));
        if has_witness {
            return FireOutcome::NoChange;
        }
        if self.is_blocked(src) {
            // Blocked: the witness ancestor already realises this
            // existential; don't generate.
            return FireOutcome::NoChange;
        }
        let succ = self.new_node();
        self.nodes[succ.index()].add(cls);
        self.nodes[src.index()].edges.push((role, succ));
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

/// Two named roles match if their underlying `RoleId`s are equal.
/// Inverse-role and sub-role matching are H3 refinements; H1's
/// clausifier emits named roles only.
fn role_matches(edge: Role, wanted: Role) -> bool {
    fn id(r: Role) -> RoleId {
        match r {
            Role::Named(x) | Role::Inverse(x) => x,
        }
    }
    edge.is_inverse() == wanted.is_inverse() && id(edge) == id(wanted)
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
