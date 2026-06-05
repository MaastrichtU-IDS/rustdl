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

/// A set of branch *decision levels* a derivation depends on, for
/// dependency-directed backjumping. A `u128` bitset (decision levels
/// 0..128) plus an `overflow` flag: once branching exceeds 128 levels
/// the set degrades to "depends on everything" — conservative, so the
/// solver falls back to chronological backtracking rather than risking
/// an unsound backjump. Empty (`EMPTY`) is the common case: every label
/// derived before any branching depends on no decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DepSet {
    pub(crate) bits: u128,
    pub(crate) overflow: bool,
}

impl DepSet {
    const EMPTY: DepSet = DepSet {
        bits: 0,
        overflow: false,
    };

    /// "Depends on everything" — the conservative dep-set for clashes
    /// whose precise provenance isn't tracked (merge `≠`, NN-rule).
    /// Forces chronological backtracking (no backjump past), which is
    /// always sound.
    const ALL: DepSet = DepSet {
        bits: 0,
        overflow: true,
    };

    fn singleton(level: u32) -> DepSet {
        if level < 128 {
            DepSet {
                bits: 1u128 << level,
                overflow: false,
            }
        } else {
            DepSet {
                bits: 0,
                overflow: true,
            }
        }
    }

    /// `true` if `level` is in the set — conservatively `true` on
    /// overflow (so the solver won't backjump past it).
    fn contains(self, level: u32) -> bool {
        self.overflow || (level < 128 && (self.bits & (1u128 << level)) != 0)
    }

    fn union(self, other: DepSet) -> DepSet {
        DepSet {
            bits: self.bits | other.bits,
            overflow: self.overflow || other.overflow,
        }
    }

    fn insert(self, level: u32) -> DepSet {
        self.union(DepSet::singleton(level))
    }

    /// Drop `level` from the set (used when a decision is *exhausted* —
    /// proved Unsat for all its disjuncts). No-op on overflow (keeps the
    /// conservative "depends on all" — never under-counts).
    fn remove(self, level: u32) -> DepSet {
        if self.overflow || level >= 128 {
            self
        } else {
            DepSet {
                bits: self.bits & !(1u128 << level),
                overflow: false,
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
struct HyperNode {
    /// Class atoms true at this node — sorted by id, deduped.
    labels: Vec<ClassId>,
    /// Backjumping dependency sets, parallel to `labels` (same index):
    /// `label_deps[i]` is the set of decision levels `labels[i]`'s
    /// derivation depends on. Empty before any branching.
    label_deps: Vec<DepSet>,
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
    /// Backjumping: the decision dep-set this node was *created* under
    /// (the `∃`/`≥n` that generated it). A clause matching this node via
    /// a role atom depends on the node existing, hence on `birth_deps` —
    /// without this, domain-style `R(x,y) → D(x)` clauses under-count
    /// their deps and cause unsound backjumps. Root node: `EMPTY`.
    birth_deps: DepSet,
    /// HF2-double-blocking: the node that created this one via `∃`/`≥n`
    /// (the parent in the completion tree). `None` for the root. Set
    /// once at node creation; merge doesn't change it (the merge target
    /// retains its own parent). Used by the double-blocking condition
    /// to require the *parents'* labels match too (not just the nodes').
    parent: Option<HNode>,
    /// HF2-double-blocking: the role of the edge from `parent` to this
    /// node. `None` for the root. Set once at creation. Used by the
    /// double-blocking condition to require equal incoming-edge labels.
    parent_role: Option<Role>,
}

impl HyperNode {
    fn has(&self, c: ClassId) -> bool {
        self.labels
            .binary_search_by_key(&c.index(), |l| l.index())
            .is_ok()
    }

    /// Insert a class label with its backjumping dep-set; returns true
    /// if newly added. On an already-present label the existing dep-set
    /// is **kept** (the "keep first" rule — narrower is sound; widening
    /// to the union would defeat backjumping when a label is re-derived
    /// along multiple branches).
    fn add(&mut self, c: ClassId, deps: DepSet) -> bool {
        match self.labels.binary_search_by_key(&c.index(), |l| l.index()) {
            Ok(_) => false,
            Err(pos) => {
                self.labels.insert(pos, c);
                self.label_deps.insert(pos, deps);
                true
            }
        }
    }

    /// The dep-set of label `c` at this node (`EMPTY` if absent).
    fn deps_of(&self, c: ClassId) -> DepSet {
        match self.labels.binary_search_by_key(&c.index(), |l| l.index()) {
            Ok(pos) => self.label_deps[pos],
            Err(_) => DepSet::EMPTY,
        }
    }
}

/// Branch save/restore snapshot — captures every mutable engine state
/// that branching can alter, so a failed branch fully reverts.
///
/// Note: `lazy_replay_state` is intentionally NOT saved — it's a
/// read-only contract from [`HyperEngine::from_snapshot_lazy`],
/// untouched by branching.
struct Snapshot {
    nodes: Vec<HyperNode>,
    representative: Vec<HNode>,
    neq: Vec<(HNode, HNode)>,
    block_index: Option<std::collections::HashMap<Role, Vec<HNode>>>,
    /// Per-node sentinel-origin bits, saved alongside `nodes` so they
    /// stay in sync after restore. The `snapshot_backprop_aborted`
    /// flag on the engine is intentionally NOT saved — once back-prop
    /// into a snapshot node was observed in any branch, the verdict
    /// for the whole query should be `BackPropAborted` regardless of
    /// whether the branch that observed it succeeded.
    origin: Vec<bool>,
}

/// Phase 1b.5 lazy expansion state for snapshot replay. When set
/// (via [`HyperEngine::from_snapshot_lazy`]), `horn_fixpoint`'s
/// re-seed loop consults this state to skip pushing
/// `Event::Label(n, c)` for snapshot-origin nodes whose `c` is in
/// `pre_capture_labels[n]` and `c` is not in `new_trigger_atoms`.
///
/// Soundness: pre-captured labels' effects are already realized
/// in the snapshot's saturated state (snapshot was captured at
/// `HyperResult::Sat`); new clauses appended at replay only trigger
/// on body atoms in `new_trigger_atoms`. Skipping re-seed for the
/// intersection is sound by construction. See spec §4.1 + Phase 1b.5
/// plan's soundness contract.
///
/// `None` means full-re-run mode (Phase 1b first-cut behavior; the
/// existing `from_snapshot` constructor leaves this `None`).
struct LazyReplayState {
    /// Per-node immutable labels at snapshot capture. Parallel to
    /// `HyperEngine.nodes` (indexed by `HNode.index()`). Snapshot-
    /// origin nodes have populated entries cloned from the snapshot;
    /// non-snapshot nodes (created during decide via `new_node`)
    /// have entries beyond this `Vec`'s length, which the guard's
    /// `pre_capture_labels.get(idx)` branch naturally handles
    /// (returns `None` → "not pre-captured" → seed normally).
    pre_capture_labels: Vec<Vec<ClassId>>,
    /// Body-atom class ids of every clause appended at replay (the
    /// caller's `neg_sup_clauses`). `std::collections::HashSet` for
    /// constant-time lookup in the re-seed loop. Constructed by
    /// `replay_with_neg_sup` (Task 3) from the new clauses' body
    /// `Atom::Class` entries.
    new_trigger_atoms: std::collections::HashSet<u32>,
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
    /// `is_blocked` invocations. HF2 double-blocking profiling: if this
    /// dwarfs `match_attempts`, the blocking check is the bottleneck.
    pub is_blocked_calls: u64,
    /// Label-vector equality / subset comparisons inside `is_blocked`.
    /// The expensive per-call cost (linear in label-set size).
    pub block_compares: u64,
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
    /// HF2-double-blocking flag (opt-in via [`with_double_blocking`]).
    /// When `true`, [`is_blocked`] uses the Motik/Shearer/Horrocks 2009
    /// §3.4 pair-blocking variant — `L(n) = L(m)` *and*
    /// `L(parent(n)) = L(parent(m))` *and* equal incoming-edge role —
    /// instead of anywhere blocking's subset check. Required for `Sat`-
    /// soundness with inverse roles; without it, `RUSTDL_HYPERTABLEAU_TRUST_SAT`
    /// is corpus-only safe (see SIO finding).
    double_blocking: bool,
    /// HF2-double-blocking performance index: nodes partitioned by
    /// `parent_role`. Skipping incompatible candidates without scanning
    /// the full nodes vec cuts `is_blocked` cost from O(n) to
    /// O(bucket-size). `None` unless double-blocking is enabled (no
    /// overhead on the default anywhere-blocking path).
    block_index: Option<std::collections::HashMap<Role, Vec<HNode>>>,
    /// `HF4a` nominal class range `[start, start + count)`. A class id in
    /// this range names a singleton `{a}`, so any two distinct nodes
    /// carrying it must be the *same* individual — the NN-rule merges
    /// them (clashing if they are `≠`). `None` ⇒ no nominals (every
    /// class is ordinary), the pre-HF4 behaviour.
    nominals: Option<(u32, u32)>,
    /// Backjumping: the dep-set of the most recent clash, set at each
    /// clash site just before returning [`FireOutcome::Clash`] and read
    /// by [`HyperEngine::solve`] after [`HyperEngine::horn_fixpoint`]
    /// reports `Unsat`. Decision-free clashes (the Horn-only path) leave
    /// it `EMPTY`, so a subsumption proved without branching propagates
    /// "depends on no decision".
    clash_deps: DepSet,
    /// Phase 1b snapshot-origin tracking: `snapshot_origin[i]` is `true`
    /// iff node `i` was reconstructed from a [`crate::snapshot::GraphSnapshot`]
    /// via [`Self::from_snapshot`], not created during the current decide
    /// run. Engines built via [`Self::new`] have `vec![false]` (the one
    /// root node is not snapshot-origin).
    ///
    /// Read by the `BackPropAborted` runtime sentinel — see spec §4.3.
    snapshot_origin: Vec<bool>,
    /// Phase 1b `BackPropAborted` runtime sentinel: set to `true` if any
    /// call to [`Self::add_label_via_backprop`] targets a node flagged
    /// `snapshot_origin`. Read by [`crate::replay::replay_with_neg_sup`]
    /// after `decide` returns; on a fired sentinel, replay returns
    /// `ReplayVerdict::BackPropAborted` instead of the raw verdict so
    /// the orchestrator falls through to the wedge/tableau path.
    ///
    /// Phase 1b: this flag rarely fires on Safe-classified seeds
    /// (`BackPropRisk` excludes inverse/nominal/cardinality hazards).
    /// The sentinel becomes load-bearing in Phase 3 when the
    /// per-class classifier loosens the Unsafe gate.
    snapshot_backprop_aborted: bool,
    /// Phase 1b.5: optional lazy-replay state. `None` for fresh
    /// engines (via [`Self::new`]) or full-re-run replays (via
    /// [`Self::from_snapshot`]). `Some` only via
    /// [`Self::from_snapshot_lazy`].
    lazy_replay_state: Option<LazyReplayState>,
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
        root_node.add(root, DepSet::EMPTY);
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
            nominals: None,
            clash_deps: DepSet::EMPTY,
            double_blocking: false,
            block_index: None,
            snapshot_origin: vec![false],
            snapshot_backprop_aborted: false,
            lazy_replay_state: None,
        }
    }

    /// Opt into HF2 double-blocking — the SROIQ-sound blocking
    /// condition required for `Sat` soundness with inverse roles. Off
    /// by default (preserves existing-test calibration); the production
    /// HF5 wedge enables it via `RUSTDL_HYPER_DOUBLE_BLOCK`.
    #[must_use]
    pub fn with_double_blocking(mut self) -> Self {
        self.double_blocking = true;
        self.block_index = Some(std::collections::HashMap::new());
        self
    }

    /// Supply the `HF4a` nominal class range `[start, start + count)` so
    /// the NN-rule merges distinct nodes carrying the same singleton.
    #[must_use]
    pub fn with_nominals(mut self, start: u32, count: u32) -> Self {
        self.nominals = Some((start, count));
        self
    }

    /// Whether class `c` names a singleton nominal `{a}` (`HF4a`).
    fn is_nominal(&self, c: ClassId) -> bool {
        match self.nominals {
            Some((start, count)) => {
                let i = c.index();
                i >= start && i < start.saturating_add(count)
            }
            None => false,
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

    /// Add class `c` to node `n` with backjumping dep-set `deps`,
    /// emitting a [`Event::Label`] on a *first* add (so its newly-enabled
    /// clauses fire). Returns whether the label was newly added.
    fn add_label(&mut self, n: HNode, c: ClassId, deps: DepSet) -> bool {
        if self.nodes[n.index()].add(c, deps) {
            self.worklist.push(Event::Label(n, c));
            true
        } else {
            false
        }
    }

    /// Phase 1b `BackPropAborted` runtime sentinel hook. Adds a label
    /// identically to [`Self::add_label`], but additionally sets the
    /// `snapshot_backprop_aborted` flag whenever `n` is a snapshot-
    /// origin node (i.e., reconstructed via [`Self::from_snapshot`]).
    ///
    /// Phase 1b ships the infrastructure (this method, the flag, the
    /// accessor) but no production code path invokes it yet —
    /// `BackPropRisk::Safe` already excludes the hazards that would
    /// trigger genuine back-propagation. Phase 3 will hook this at
    /// the inverse-role / nominal / cardinality back-prop sites
    /// (`fire_clause`'s `succ_trigger` path and `merge`'s label
    /// propagation) when the per-class classifier loosens the gate.
    ///
    /// Replay reads the flag after `decide` and returns
    /// `ReplayVerdict::BackPropAborted` if it fired — see
    /// [`crate::replay::replay_with_neg_sup`]. See spec §4.3.
    #[allow(
        dead_code,
        reason = "Phase 1b infrastructure for Phase 3 back-prop site hooks"
    )]
    pub(crate) fn add_label_via_backprop(&mut self, n: HNode, c: ClassId, deps: DepSet) -> bool {
        if self
            .snapshot_origin
            .get(n.index())
            .copied()
            .unwrap_or(false)
        {
            self.snapshot_backprop_aborted = true;
        }
        self.add_label(n, c, deps)
    }

    /// Phase 1b `BackPropAborted` runtime sentinel accessor. Read after
    /// `decide` to detect whether any back-propagation event during
    /// the run targeted a snapshot-origin node. See
    /// [`Self::add_label_via_backprop`] and spec §4.3.
    #[must_use]
    pub(crate) fn snapshot_backprop_aborted(&self) -> bool {
        self.snapshot_backprop_aborted
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
        // Nodes created during decide are NOT snapshot-origin; the
        // sentinel only fires on labels propagated INTO the original
        // snapshot's reconstructed nodes.
        self.snapshot_origin.push(false);
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
    fn is_blocked(&mut self, n: HNode) -> bool {
        self.stats.is_blocked_calls += 1;
        let ln_order = self.nodes[n.index()].order;
        if self.double_blocking {
            // HF2 double-blocking (Motik et al. §3.4): require equal
            // labels + equal parent labels + equal incoming-edge role.
            // The root is never blocked (no parent). Performance:
            // iterate only same-parent-role nodes via `block_index`
            // (O(bucket) vs O(n) for the full scan).
            let (np, nr) = {
                let ln = &self.nodes[n.index()];
                let Some(np) = ln.parent else { return false };
                let nr = ln.parent_role.expect("non-root has parent_role");
                (np, nr)
            };
            // Snapshot the candidate list (clone to release the
            // immutable borrow on `block_index` before we mutate stats).
            let candidates: Vec<HNode> = self
                .block_index
                .as_ref()
                .and_then(|ix| ix.get(&nr))
                .cloned()
                .unwrap_or_default();
            for m_hnode in candidates {
                let m_order = self.nodes[m_hnode.index()].order;
                if m_order >= ln_order {
                    continue;
                }
                let Some(mp) = self.nodes[m_hnode.index()].parent else {
                    continue;
                };
                self.stats.block_compares += 1;
                // Anywhere pair-blocking (Horrocks 1998 / Motik 2009):
                // *subset* semantics — the blocker is "at least as
                // rich" as the blocked. Stricter than anywhere
                // blocking (requires parent + edge-role match, so
                // sound with inverses) but weaker than label-equality
                // (so SROIFV-class ontologies block in tractable
                // depth instead of generating exponentially).
                if subset_sorted(
                    &self.nodes[n.index()].labels,
                    &self.nodes[m_hnode.index()].labels,
                ) && subset_sorted(
                    &self.nodes[np.index()].labels,
                    &self.nodes[mp.index()].labels,
                ) {
                    return true;
                }
            }
            false
        } else {
            // Anywhere blocking (legacy; sound for SHIQ-no-inverse).
            // Snapshot the node count and iterate by index to keep
            // mutating `stats` clean of borrow conflicts.
            let n_nodes = self.nodes.len();
            for i in 0..n_nodes {
                let m_order = self.nodes[i].order;
                if m_order >= ln_order {
                    continue;
                }
                self.stats.block_compares += 1;
                let ln_labels = &self.nodes[n.index()].labels;
                let m_labels = &self.nodes[i].labels;
                if subset_sorted(ln_labels, m_labels) {
                    return true;
                }
            }
            false
        }
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
                // Phase 1b.5 lazy expansion guard: skip Event::Label
                // seeding for snapshot-origin nodes whose label `c`
                // was pre-captured AND not a new-clause trigger. The
                // label's effects under the capture-time clause set
                // are already realized in the snapshot; skipping the
                // event saves the redundant rule firings (~89% CPU
                // reduction projected on GALEN per
                // docs/phase1b5-recon.md).
                if let Some(ref lazy) = self.lazy_replay_state {
                    let was_pre_captured = lazy
                        .pre_capture_labels
                        .get(idx)
                        .is_some_and(|pre| pre.binary_search(&c).is_ok());
                    let is_new_trigger = lazy.new_trigger_atoms.contains(&c.index());
                    if was_pre_captured && !is_new_trigger {
                        continue;
                    }
                }
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
                // `HF4a` NN-rule: a singleton nominal on `n` merges any
                // other node carrying it (clashing if they are `≠`).
                if matches!(self.apply_nn_rule(n, c), FireOutcome::Clash) {
                    return FireOutcome::Clash;
                }
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

    /// On a successful satisfiability search, return the labels of the
    /// node seeded with `seed`. Returns `None` if the search hasn't
    /// returned Sat OR if no node is labeled with `seed` (shouldn't
    /// happen for a well-formed Q-clause setup — Q's seed is always
    /// asserted at node 0 by `new`, but the seed-node may have been
    /// merged into another representative during the search; we
    /// resolve through the union-find to find the canonical owner).
    ///
    /// The returned set is the basis for the per-class label heuristic
    /// in `owl-dl-reasoner::classify_top_down_internal`: any atomic
    /// class D ∈ this set is a candidate subsumer of `seed`; any
    /// D ∉ this set is a sound non-subsumer (this completion graph IS
    /// a counterexample model). See
    /// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
    #[must_use]
    pub fn satisfiability_labels(&self, seed: ClassId) -> Option<Vec<ClassId>> {
        // The seed is asserted at node 0 by `new`. Merges redirect the
        // union-find but leave stale labels on the merged-away node;
        // resolve through the union-find to read the canonical
        // (post-merge) label set, then verify it actually contains
        // the seed (defensive).
        let rep = self.resolve(HNode(0));
        let labels = &self.nodes[rep.index()].labels;
        if labels.contains(&seed) {
            Some(labels.clone())
        } else {
            None
        }
    }

    /// Capture a [`crate::snapshot::GraphSnapshot`] of the current
    /// completion graph. Soundly callable only after [`Self::decide`]
    /// (or [`Self::decide_with_deadline`]) has returned
    /// [`HyperResult::Sat`] — otherwise the graph state may carry an
    /// incomplete or post-clash structure that violates the snapshot's
    /// "witness model" contract.
    ///
    /// Returns `None` if the seed isn't present at the resolved root
    /// (defensive: matches the [`Self::satisfiability_labels`] guard).
    ///
    /// Phase 1a: `fired` fingerprint slots are placeholder `0`; the
    /// real fingerprint computation lands in Phase 1b alongside the
    /// lazy replay driver. `risk` is stamped `Safe` here — the Phase
    /// 1b orchestrator runs `BackPropRisk::classify_ontology` once
    /// and overrides this per snapshot.
    #[must_use]
    pub fn satisfiability_snapshot(&self, seed: ClassId) -> Option<crate::snapshot::GraphSnapshot> {
        use crate::snapshot::{GraphSnapshot, SnapshotEdge, SnapshotNode};

        let root_rep = self.resolve(HNode(0));
        if !self.nodes[root_rep.index()].labels.contains(&seed) {
            return None;
        }

        // Walk every node, resolving through the union-find. Skip
        // merged-away nodes (those whose resolve != self).
        let n_nodes = self.nodes.len();
        let mut canonical: Vec<HNode> = Vec::with_capacity(n_nodes);
        let mut hnode_to_snap: Vec<Option<u32>> = vec![None; n_nodes];
        for (i, slot) in hnode_to_snap.iter_mut().enumerate().take(n_nodes) {
            let h = HNode(u32::try_from(i).expect("node count fits u32"));
            if self.resolve(h) == h {
                let snap_id = u32::try_from(canonical.len()).expect("snap node count fits u32");
                *slot = Some(snap_id);
                canonical.push(h);
            }
        }
        // Aliased nodes inherit their representative's snap id.
        // Two-pass borrow split: collect the rep mapping for slots that
        // need filling, then write them back. Keeps the `&self.resolve`
        // and `&mut hnode_to_snap` borrows disjoint without spuriously
        // indexing in a loop body.
        let fills: Vec<(usize, Option<u32>)> = (0..n_nodes)
            .filter(|&i| hnode_to_snap[i].is_none())
            .map(|i| {
                let rep = self.resolve(HNode(u32::try_from(i).expect("fits u32")));
                (i, hnode_to_snap[rep.index()])
            })
            .collect();
        for (i, mapped) in fills {
            hnode_to_snap[i] = mapped;
        }

        let root_snap_idx = hnode_to_snap[root_rep.index()].expect("root mapped") as usize;
        let mut nodes = Vec::with_capacity(canonical.len());
        let mut edges: Vec<Vec<SnapshotEdge>> = Vec::with_capacity(canonical.len());
        let mut fired = Vec::with_capacity(canonical.len());
        for (snap_id, h) in canonical.iter().enumerate() {
            let hn = &self.nodes[h.index()];
            nodes.push(SnapshotNode {
                labels: hn.labels.clone(),
                is_root: snap_id == root_snap_idx,
                birth_deps: hn.birth_deps,
                pre_capture_labels: hn.labels.clone(),
            });
            let mut snap_edges = Vec::with_capacity(hn.edges.len());
            for (role, tgt) in &hn.edges {
                let tgt_rep = self.resolve(*tgt);
                if let Some(snap_tgt) = hnode_to_snap[tgt_rep.index()] {
                    snap_edges.push(SnapshotEdge {
                        role: *role,
                        target: snap_tgt,
                    });
                }
            }
            edges.push(snap_edges);
            fired.push(0); // Phase 1a placeholder; Phase 1b computes real fingerprint.
        }

        Some(GraphSnapshot::from_parts(
            nodes,
            edges,
            fired,
            seed,
            crate::snapshot::BackPropRisk::Safe,
        ))
    }

    /// Reconstruct a `HyperEngine` from a captured `GraphSnapshot`,
    /// suitable as the seed state for a snapshot-replay query.
    ///
    /// The returned engine has the snapshot's `node` / `edge` / `label` /
    /// `birth_deps` state populated, and the clause set ready to receive
    /// additional query clauses (e.g., a `¬sup` injection) before `decide`
    /// is called.
    ///
    /// Note: this is the first half of the snapshot-replay path. Replay
    /// proper lives in `crate::replay::replay_with_neg_sup` (Task 2).
    /// Phase 1b first-cut uses full-re-run (no lazy expansion skip);
    /// Phase 1b.5 will add fingerprint-gated lazy firing.
    ///
    /// Fields NOT round-tripped (deferred to a future phase):
    /// `parent`/`parent_role` (HF2 double-blocking will conservatively
    /// skip blocking on these nodes — sound, possibly slower);
    /// `at_most`/`at_least_done`/`neq` (cardinality state — snapshots
    /// of cardinality-bearing seeds aren't replayed today because the
    /// `BackPropRisk` gate flags them `Unsafe`, so this gap is moot
    /// at the orchestrator layer); `block_index` (rebuilt lazily by the
    /// engine when double-blocking is enabled).
    #[must_use]
    pub fn from_snapshot(
        clauses: &'c [DlClause],
        snapshot: &crate::snapshot::GraphSnapshot,
    ) -> Self {
        // Start with a fresh engine, then overwrite the graph state with
        // the snapshot's nodes/edges/labels/deps. The clause set, indexes,
        // and other run-state default to the same shape as a brand-new
        // engine.
        let mut engine = Self::new(clauses, snapshot.seed());

        // Reset graph state (Self::new initialized one node carrying the
        // seed; we replace with the snapshot's full graph).
        engine.nodes.clear();
        engine.representative.clear();
        engine.neq.clear();
        engine.worklist.clear();
        engine.clash_deps = DepSet::EMPTY;
        engine.snapshot_origin.clear();
        engine.snapshot_backprop_aborted = false;
        if let Some(ix) = engine.block_index.as_mut() {
            ix.clear();
        }

        let n_nodes = snapshot.nodes().len();
        for (i, snap_node) in snapshot.nodes().iter().enumerate() {
            let order = u32::try_from(i).expect("node count fits u32");
            let mut hn = HyperNode {
                order,
                birth_deps: snap_node.birth_deps,
                ..HyperNode::default()
            };
            hn.labels.clone_from(&snap_node.labels);
            hn.label_deps = vec![snap_node.birth_deps; snap_node.labels.len()];
            // parent / parent_role: unknown from snapshot (Phase 1b.5
            // will capture them); leave None. Double-blocking is a
            // soundness-completeness lever, not a soundness requirement,
            // so the engine will conservatively skip blocking decisions
            // that lack parent info — sound, possibly slower.
            engine.nodes.push(hn);
        }
        for i in 0..n_nodes {
            engine
                .representative
                .push(HNode(u32::try_from(i).expect("node count fits u32")));
        }
        // Sentinel infrastructure: every reconstructed node is
        // snapshot-origin. Any add_label_via_backprop call targeting
        // one of them sets snapshot_backprop_aborted (read by replay).
        engine.snapshot_origin = vec![true; n_nodes];
        // from_snapshot is the full-re-run path: lazy_replay_state
        // stays None. from_snapshot_lazy populates it after delegating
        // here.
        engine.lazy_replay_state = None;
        for (i, edges) in snapshot.edges_per_node().iter().enumerate() {
            for edge in edges {
                let src = HNode(u32::try_from(i).expect("fits u32"));
                let tgt = HNode(edge.target);
                engine.nodes[i].edges.push((edge.role, tgt));
                // Mirror as a pred on the target for back-propagation
                // bookkeeping (matches the inline comment on
                // HyperNode.preds).
                engine.nodes[edge.target as usize]
                    .preds
                    .push((edge.role, src));
            }
        }
        engine
    }

    /// Phase 1b.5: lazy-expansion constructor for snapshot replay.
    /// Same shape as [`Self::from_snapshot`] but additionally populates
    /// `lazy_replay_state` with the snapshot's `pre_capture_labels`
    /// plus the caller's `new_trigger_atoms`. `horn_fixpoint` re-seed
    /// (Task 3) will skip `Event::Label` events for pre-captured labels
    /// at snapshot-origin nodes when those labels are not in
    /// `new_trigger_atoms`.
    ///
    /// Sound iff the snapshot was built from a `Sat` verdict and the
    /// caller's `new_trigger_atoms` is a complete enumeration of body
    /// class-atom ids in clauses appended since capture. See spec §4.1
    /// + the Phase 1b.5 plan's soundness contract.
    ///
    /// `new_trigger_atoms` is a `std::collections::HashSet<u32>` of
    /// `ClassId` indices for constant-time lookup. Caller (typically
    /// [`crate::replay::replay_with_neg_sup`]) derives it from the new
    /// clauses' body `Atom::Class` entries.
    #[must_use]
    pub fn from_snapshot_lazy(
        clauses: &'c [DlClause],
        snapshot: &crate::snapshot::GraphSnapshot,
        new_trigger_atoms: std::collections::HashSet<u32>,
    ) -> Self {
        let mut engine = Self::from_snapshot(clauses, snapshot);
        let pre_capture_labels: Vec<Vec<ClassId>> = snapshot
            .nodes()
            .iter()
            .map(|n| n.pre_capture_labels.clone())
            .collect();
        engine.lazy_replay_state = Some(LazyReplayState {
            pre_capture_labels,
            new_trigger_atoms,
        });
        engine
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
        // Disjunctive-head branching (H2) with dependency-directed
        // backjumping. The decision level of this frame is `d`; the
        // asserted disjunct inherits the clause body's dep-set ∪ {d}.
        if let Some((ci, node, binding)) = self.find_open_disjunction() {
            if depth == 0 {
                return HyperResult::Stalled;
            }
            self.track_depth(depth);
            let d = u32::try_from(self.init_depth - depth).unwrap_or(u32::MAX);
            let body_deps = self.clause_body_deps(ci, node, &binding);
            let decision_deps = body_deps.insert(d);
            let head_len = self.clauses[ci].head.len();
            let mut any_stalled = false;
            let mut combined = DepSet::EMPTY;
            for k in 0..head_len {
                let head_atom = self.clauses[ci].head[k];
                let saved = self.save();
                self.stats.branches_taken += 1;
                let _ = self.apply_head_atom(head_atom, node, &binding, decision_deps);
                match self.solve(depth - 1) {
                    HyperResult::Sat => return HyperResult::Sat,
                    HyperResult::Unsat => {
                        let child_deps = self.clash_deps;
                        self.restore(saved);
                        if !child_deps.contains(d) {
                            // This decision wasn't responsible for the
                            // clash — backjump: propagate the child's
                            // dep-set up, skipping the remaining
                            // disjuncts (and this whole decision).
                            self.clash_deps = child_deps;
                            return HyperResult::Unsat;
                        }
                        combined = combined.union(child_deps);
                    }
                    HyperResult::Stalled => {
                        self.restore(saved);
                        any_stalled = true;
                    }
                }
            }
            if any_stalled {
                return HyperResult::Stalled;
            }
            // Every disjunct failed with `d` in its clash deps: the
            // decision is exhausted, so drop `d` from the propagated set.
            self.clash_deps = combined.remove(d);
            return HyperResult::Unsat;
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
            if any_stalled {
                return HyperResult::Stalled;
            }
            // `≤n` merge provenance isn't tracked precisely, so report
            // a conservative "depends on everything" — the parent frame
            // then backtracks chronologically past this merge decision
            // (sound; no backjump). Backjumping through merge decisions
            // is future work (interacts with the in-edge redirect gap).
            self.clash_deps = DepSet::ALL;
            return HyperResult::Unsat;
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
    /// nodes, the merge union-find, the `≠` relation, and (when
    /// double-blocking is on) the parent-role partition index. All
    /// revert on a failed branch.
    fn save(&mut self) -> Snapshot {
        self.stats.node_clones += 1;
        Snapshot {
            nodes: self.nodes.clone(),
            representative: self.representative.clone(),
            neq: self.neq.clone(),
            block_index: self.block_index.clone(),
            origin: self.snapshot_origin.clone(),
        }
    }

    fn restore(&mut self, saved: Snapshot) {
        self.nodes = saved.nodes;
        self.representative = saved.representative;
        self.neq = saved.neq;
        self.block_index = saved.block_index;
        self.snapshot_origin = saved.origin;
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
            // ≠ violated — merging is impossible. Conservative deps
            // (precise `≠`/merge provenance isn't tracked).
            self.clash_deps = DepSet::ALL;
            return true;
        }
        self.representative[s_j.index()] = s_i;
        let s_j_labels: Vec<(ClassId, DepSet)> = {
            let nj = &self.nodes[s_j.index()];
            nj.labels
                .iter()
                .copied()
                .zip(nj.label_deps.iter().copied())
                .collect()
        };
        for (c, c_deps) in s_j_labels {
            self.add_label(s_i, c, c_deps);
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
            let body_deps = self.clause_body_deps(ci, node, &binding);
            match self.fire_head(ci, node, &binding, body_deps) {
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

    /// The backjumping dep-set of clause `ci`'s body under `binding`:
    /// the union of the dep-sets of every body *class* atom at its bound
    /// node (role atoms carry no decision dependency). This is the
    /// dep-set a derived head inherits, and the clash dep-set for a
    /// `body → ⊥` clause.
    fn clause_body_deps(&self, ci: usize, xnode: HNode, binding: &Binding) -> DepSet {
        // Every node the clause body touches contributes its `birth_deps`
        // (a role atom depends on its successor existing), and every body
        // class atom contributes its label deps.
        let mut deps = self.nodes[xnode.index()].birth_deps;
        for &(_, node) in binding {
            deps = deps.union(self.nodes[node.index()].birth_deps);
        }
        for atom in &self.clauses[ci].body {
            if let Atom::Class(c, v) = atom
                && let Some(node) = resolve_var(*v, xnode, binding)
            {
                deps = deps.union(self.nodes[node.index()].deps_of(*c));
            }
        }
        deps
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
    /// non-`X` variables to nodes; `body_deps` is the clause body's
    /// backjumping dep-set (the head inherits it; a `body → ⊥` clash
    /// records it).
    fn fire_head(
        &mut self,
        ci: usize,
        xnode: HNode,
        binding: &Binding,
        body_deps: DepSet,
    ) -> FireOutcome {
        let clause = &self.clauses[ci];
        if clause.head.is_empty() {
            // body → ⊥ : the body matched, so this is a clash. Record
            // the dep-set so `solve` can backjump.
            self.clash_deps = body_deps;
            return FireOutcome::Clash;
        }
        // Horn: exactly one head atom (caller gated on is_horn).
        let head = clause.head[0];
        self.apply_head_atom(head, xnode, binding, body_deps)
    }

    /// Assert one head atom (`Class` label or `∃` successor) at the
    /// resolved binding. Shared by Horn firing and disjunctive
    /// branching. Never reports a clash itself — clashes surface when
    /// a `body → ⊥` clause subsequently fires in [`horn_fixpoint`].
    fn apply_head_atom(
        &mut self,
        head: Atom,
        xnode: HNode,
        binding: &Binding,
        deps: DepSet,
    ) -> FireOutcome {
        match head {
            Atom::Class(c, v) => {
                let Some(target) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                if self.add_label(target, c, deps) {
                    FireOutcome::Changed
                } else {
                    FireOutcome::NoChange
                }
            }
            Atom::Exists(role, cls, v) => {
                let Some(src) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                self.fire_exists(src, role, cls, deps)
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
                self.generate_at_least(target, role, qual, n, deps)
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
    fn fire_exists(&mut self, src: HNode, role: Role, cls: ClassId, deps: DepSet) -> FireOutcome {
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
        self.nodes[succ.index()].birth_deps = deps;
        self.nodes[succ.index()].parent = Some(src);
        self.nodes[succ.index()].parent_role = Some(role);
        if let Some(ix) = self.block_index.as_mut() {
            ix.entry(role).or_default().push(succ);
        }
        self.nodes[src.index()].edges.push((role, succ));
        self.nodes[succ.index()].preds.push((role, src));
        // The new edge fires role-triggered clauses at `src`; the seed
        // label fires the successor's clauses (and, via Event::Label,
        // back-prop at `src`).
        self.worklist.push(Event::Edge(src, role, succ));
        // The seed label inherits the ∃'s body dep-set.
        self.add_label(succ, cls, deps);
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
        deps: DepSet,
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
            self.nodes[succ.index()].birth_deps = deps;
            self.nodes[succ.index()].parent = Some(x);
            self.nodes[succ.index()].parent_role = Some(role);
            if let Some(ix) = self.block_index.as_mut() {
                ix.entry(role).or_default().push(succ);
            }
            self.nodes[x.index()].edges.push((role, succ));
            self.nodes[succ.index()].preds.push((role, x));
            self.worklist.push(Event::Edge(x, role, succ));
            if let Some(q) = qual {
                self.add_label(succ, q, deps);
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

    /// `HF4a` NN-rule: node `n` just gained the singleton nominal `c`, so
    /// any *other* node also carrying `{c}` is the same individual and
    /// must merge into `n` (a forced merge — clashes if they are `≠`,
    /// which is exactly how `≥2 R.{o}` becomes unsat). Deterministic, so
    /// it runs in the Horn fixpoint on the triggering `Label` event.
    ///
    /// `HF4b` (the deferred cousins) turns out to be achieved by
    /// composition, not extra rules — verified by three probes:
    /// nominal-under-`∀` propagation works because `∀R.{o}` clausifies
    /// to `R(x,y) → {o}(y)`, whose `Label` event triggers this rule;
    /// nominal-aware blocking is moot because same-nominal nodes *merge*
    /// rather than one blocking the other; and multi-predecessor merge
    /// needs no in-edge redirect because each `{o}` node back-propagates
    /// to its own predecessor *before* the merge collapses identity. The
    /// in-edge redirect would still be principled for inverse-heavy
    /// ontologies with post-merge label derivation (corpus-inert, no
    /// constructible canary fails) — deliberately not built on
    /// speculation; revisit when HF2 double-blocking exercises in-edges.
    fn apply_nn_rule(&mut self, n: HNode, c: ClassId) -> FireOutcome {
        if !self.is_nominal(c) {
            return FireOutcome::NoChange;
        }
        let rn = self.resolve(n);
        let other = (0..self.nodes.len())
            .map(|i| HNode(u32::try_from(i).expect("fits u32")))
            .find(|&m| self.resolve(m) == m && m != rn && self.nodes[m.index()].has(c));
        match other {
            Some(m) => {
                if self.merge(rn, m) {
                    FireOutcome::Clash
                } else {
                    FireOutcome::Changed
                }
            }
            None => FireOutcome::NoChange,
        }
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

    /// `DepSet` algebra — the off-by-one surface for unsound pruning.
    #[test]
    fn depset_operations() {
        assert!(!DepSet::EMPTY.contains(0));
        // singleton / contains / insert
        let s = DepSet::singleton(3);
        assert!(s.contains(3) && !s.contains(2) && !s.contains(4));
        assert_ne!(s, DepSet::EMPTY);
        let s = s.insert(0).insert(5);
        assert!(s.contains(0) && s.contains(3) && s.contains(5) && !s.contains(1));
        // union
        let u = DepSet::singleton(1).union(DepSet::singleton(8));
        assert!(u.contains(1) && u.contains(8) && !u.contains(2));
        // remove clears exactly one level (the exhausted-decision rule)
        let r = u.remove(8);
        assert!(r.contains(1) && !r.contains(8));
        assert_eq!(DepSet::singleton(1).remove(1), DepSet::EMPTY);
        // remove of an absent level is a no-op
        assert!(DepSet::singleton(1).remove(2).contains(1));
        // overflow is conservative: contains everything, remove is inert
        assert!(DepSet::ALL.contains(0) && DepSet::ALL.contains(127) && DepSet::ALL.contains(200));
        assert!(DepSet::ALL.remove(5).contains(5));
        assert_ne!(DepSet::ALL, DepSet::EMPTY);
        // level >= 128 degrades to overflow (conservative)
        assert!(DepSet::singleton(200).contains(0)); // overflow ⇒ contains all
    }

    /// Backjumping canary: `n` independent root disjunctions
    /// `R ⊑ ⊓ᵢ(Aᵢ ⊔ Bᵢ)`, where only the **first and last** pair clash
    /// (all four combinations of `{A₁,B₁} × {Aₙ,Bₙ}` are `⊥`), so `R` is
    /// unsat. The `n-2` middle disjunctions are irrelevant to the clash.
    /// Chronological backtracking re-explores those `2^(n-2)` irrelevant
    /// combinations; dependency-directed backjumping recognises the
    /// clash depends only on decisions 1 and `n` and closes `R` in a
    /// linear number of branches. The branch-count bound is what the
    /// backjumping phase must satisfy — it fails (blows up) today.
    #[test]
    fn backjumping_collapses_irrelevant_middle_decisions() {
        const N: u32 = 8;
        let root = cls(0);
        let a = |i: u32| cls(2 * i - 1); // Aᵢ
        let b = |i: u32| cls(2 * i); // Bᵢ
        let mut clauses = Vec::new();
        for i in 1..=N {
            clauses.push(DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Class(a(i), X), Atom::Class(b(i), X)],
            });
        }
        // First (1) and last (N) pair clash in all four combinations.
        for &first in &[a(1), b(1)] {
            for &last in &[a(N), b(N)] {
                clauses.push(DlClause {
                    body: vec![Atom::Class(first, X), Atom::Class(last, X)],
                    head: vec![],
                });
            }
        }
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(256), HyperResult::Unsat);
        let branches = e.stats().branches_taken;
        assert!(
            branches <= 4 * u64::from(N),
            "backjumping should close R in O(N) branches, got {branches} (2^(N-2) = blowup)"
        );
    }

    #[test]
    fn satisfiability_labels_returns_horn_consequences_at_seed_node() {
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_core::ir::ClassId;

        let q = ClassId::new(100);
        let a = ClassId::new(101);
        let b = ClassId::new(102);

        let clauses = vec![
            // q ⊑ a (q's seed label triggers a)
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(a, X)],
            },
            // a ⊑ b (then b)
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Class(b, X)],
            },
        ];
        let mut engine = HyperEngine::new(&clauses, q);
        let result = engine.decide(8);
        assert_eq!(result, HyperResult::Sat, "Horn fixpoint should be Sat");

        let labels = engine
            .satisfiability_labels(q)
            .expect("Sat result must expose seed-node labels");
        assert!(
            labels.contains(&a),
            "labels must contain A (q ⊑ a): {labels:?}"
        );
        assert!(
            labels.contains(&b),
            "labels must contain B (Horn-derived): {labels:?}"
        );
        assert!(
            labels.contains(&q),
            "labels include the seed class itself: {labels:?}"
        );
    }

    #[test]
    fn satisfiability_labels_resolves_through_merge() {
        // Regression test for a correctness bug: `merge()` copies labels
        // from the merged-away node into the representative but leaves
        // stale labels behind on the source. The prior linear-scan
        // implementation returned the FIRST node containing `seed`,
        // typically `nodes[0]` (stale). The fix reads
        // `nodes[resolve(HNode(0))].labels` so the canonical
        // post-merge union is returned.
        //
        // Limitation: this test does not force a true merge — forcing
        // a merge requires a `≤n R.C` clause setup (a few dozen lines
        // of clause-building). What it DOES pin is the contract:
        // `satisfiability_labels` must walk the union-find from
        // node 0, not scan-and-pick. A regression that reverts to
        // a linear scan would still pass; a regression that reads
        // `nodes[0].labels` directly (ignoring `resolve`) would also
        // pass when no merge has occurred. The structural fix is
        // verified by code review + the corpus delta on GALEN/SIO.
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_core::ir::ClassId;

        let q = ClassId::new(200);
        let a = ClassId::new(201);
        let b = ClassId::new(202);

        let clauses = vec![
            // q ⊑ a
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(a, X)],
            },
            // q ⊑ b
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(b, X)],
            },
        ];
        let mut engine = HyperEngine::new(&clauses, q);
        let result = engine.decide(8);
        assert_eq!(result, HyperResult::Sat);

        let labels = engine
            .satisfiability_labels(q)
            .expect("Sat result must expose seed-node labels");
        assert!(labels.contains(&a), "labels must contain A: {labels:?}");
        assert!(labels.contains(&b), "labels must contain B: {labels:?}");
        assert!(labels.contains(&q));
    }

    /// Phase 1b T3 sentinel test: `add_label_via_backprop` on a
    /// snapshot-origin node sets `snapshot_backprop_aborted`; the
    /// same call on a non-snapshot node does not. Verifies the
    /// infrastructure that Phase 3 will hook at the real
    /// inverse-role / merge back-prop sites.
    #[test]
    fn sentinel_fires_on_simulated_backprop_into_snapshot_node() {
        // Build a snapshot of a single-class Horn ontology.
        let clauses: Vec<DlClause> = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X)],
        }];
        let mut eng = HyperEngine::new(&clauses, cls(0));
        assert_eq!(eng.decide(64), HyperResult::Sat);
        let snap = eng.satisfiability_snapshot(cls(0)).expect("snapshot built");

        // Reconstructed engine: root (HNode(0)) is snapshot-origin.
        let mut eng2 = HyperEngine::from_snapshot(&clauses, &snap);
        assert!(
            !eng2.snapshot_backprop_aborted(),
            "fresh from_snapshot must not have the sentinel set"
        );

        // Simulate back-prop into the snapshot root: must fire.
        eng2.add_label_via_backprop(HNode(0), cls(2), DepSet::EMPTY);
        assert!(
            eng2.snapshot_backprop_aborted(),
            "back-prop into snapshot-origin node must fire the sentinel"
        );
    }

    #[test]
    fn sentinel_does_not_fire_on_non_snapshot_node() {
        // Engine built via `new`, not `from_snapshot` — no snapshot-origin.
        let clauses: Vec<DlClause> = vec![];
        let mut eng = HyperEngine::new(&clauses, cls(0));
        eng.add_label_via_backprop(HNode(0), cls(1), DepSet::EMPTY);
        assert!(
            !eng.snapshot_backprop_aborted(),
            "back-prop into a non-snapshot node must NOT fire the sentinel"
        );
    }
}
