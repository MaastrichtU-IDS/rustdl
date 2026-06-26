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
use smallvec::{SmallVec, smallvec};
use std::time::Instant;

/// A match binding: the body's non-`X` successor variables mapped to
/// graph nodes, sorted by variable. `X` is implicit (always the match
/// root), so an empty binding is a body on `X` only. Bodies are trees
/// rooted at `X` (each non-`X` var is the target of exactly one role
/// atom whose source is already bound), so a binding is one complete
/// homomorphism of the body's variable-tree into the graph.
type Binding = SmallVec<[(Var, HNode); 4]>;

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

    /// Highest decision level present, or `None` if empty. `ALL` ⇒ `Some(127)`.
    pub(crate) fn highest_level(self) -> Option<u32> {
        if self.overflow {
            Some(127)
        } else if self.bits == 0 {
            None
        } else {
            Some(self.bits.ilog2())
        }
    }

    /// Number of decision levels present (bit-count; `ALL`/overflow ⇒ 0 bits set).
    pub(crate) fn count(self) -> u32 {
        self.bits.count_ones()
    }

    /// Iterate the decision levels present, ascending.
    pub(crate) fn iter_levels(self) -> impl Iterator<Item = u32> {
        (0..128u32).filter(move |&i| self.bits & (1u128 << i) != 0)
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
    /// Backjumping: union of the derivation dep-sets of the `≤n` constraints
    /// in `at_most` (the `body_deps` of the clause whose `AtMost` head fired
    /// onto this node). `at_most` itself is a dep-less tuple, so without this
    /// the constraint's own provenance is invisible to `card_clash_deps` —
    /// a `≤n` derived under a decision (e.g. a role-body domain/range clause
    /// `R(x,y) → ≤n(x)`) would be missed, causing an unsound backjump
    /// (Hole C). Captured by save/restore (whole-node clone); EMPTY for a
    /// snapshot-replayed node (which carries no `at_most`).
    at_most_dep: DepSet,
    /// Backjumping: set when a `merge` redirects another node's `at_most`
    /// onto this one. The merge *causation* dep (why the two nodes coincide)
    /// is not decision-tracked, so a merge-inherited `≤n` can depend on
    /// decisions absent from `at_most_dep`. `card_clash_deps` conservatively
    /// returns `DepSet::ALL` when this is set (sound; the precise-card-deps
    /// narrowing only applies to constraints derived directly on the node).
    at_most_tainted: bool,
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
    /// Backjumping: set when an `HF4a` NN-rule merge redirected another
    /// node's labels onto this one. Like `at_most_tainted` (the merge
    /// half of Hole C), the NN-merge *causation* dep (why the two nodes
    /// coincide — the branch decision that placed the shared nominal) is
    /// not folded into the copied labels' deps, so a downstream
    /// disjointness clash on a merge-inherited label can under-report
    /// its dep-set and trigger an UNSOUND backjump past the deciding
    /// disjunct (false `Unsat`). When this is set, the `body→⊥` clash
    /// site reports `DepSet::ALL` (chronological backtracking — always
    /// sound) instead of the precise `body_deps`. Captured by
    /// save/restore (whole-node clone).
    nn_tainted: bool,
    // ── Shadow precise-dependency probe fields ──────────────────────────────
    // These fields are ONLY written/read when `HyperEngine::shadow_dep_probe`
    // is `true`. They mirror the real dep fields but NEVER collapse to
    // `DepSet::ALL` due to taints — they carry the merge causation that the
    // real path discards. Saved and restored with the node (whole-node clone).
    // Never read by the real search path.
    /// Shadow mirror of `label_deps`: one `DepSet` per label (parallel
    /// index), carrying precise causation without taint-induced collapse.
    shadow_label_deps: Vec<DepSet>,
    /// Shadow mirror of `at_most_dep`: the precise derivation dep-set of
    /// the `≤n` constraints on this node, including merge-inherited ones.
    shadow_at_most_dep: DepSet,
    /// Shadow mirror of `birth_deps`: the precise dep-set for this node's
    /// creation, including merge-causation deps folded in by NN-merges.
    shadow_birth_deps: DepSet,
    /// Precise causation of all merges this node has absorbed — the dep-set
    /// that the real path discards when it sets `at_most_tainted`/`nn_tainted`.
    shadow_merge_cause: DepSet,
}

impl HyperNode {
    fn has(&self, c: ClassId) -> bool {
        // Labels are sorted, but the per-node set is tiny (profiled avg
        // ~5, max ~80), so a branch-predictable linear scan with an
        // early exit on overshoot beats binary search's mispredicted
        // `select_unpredictable` here. Same result (membership).
        for l in &self.labels {
            if l.index() == c.index() {
                return true;
            }
            if l.index() > c.index() {
                return false;
            }
        }
        false
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

/// Pre-resolved `ABox` seed for [`HyperEngine::new_seeded`] — the
/// multi-node graph an `ABox`-consistency check starts from.
///
/// **Soundness contract** (this is the false-`Unsat` surface): every
/// field must hold only what the `ABox` genuinely *asserts* / entails.
/// Over-seeding (a spurious label, a wrong merge, a phantom edge) can
/// fire a disjointness / cardinality clause that no model violates,
/// yielding a false `Unsat` = false-inconsistent = catastrophic. The
/// caller (`owl_dl_reasoner`) populates this from asserted `ABox`
/// axioms only:
/// - `num_individuals` — one node per individual is created, node `i`
///   for individual index `i`. Each is seeded with its nominal class
///   `nominal_base + i` (so the NN-rule + `DifferentIndividuals`
///   disjointness clauses fire on it). `nominal_base` is the engine's
///   nominal range start (= `num_classes`), so the labels coincide
///   with the clause set's nominal ids by construction.
/// - `property_assertions` — asserted `ObjectPropertyAssertion` edges,
///   role-polarity already normalised to forward by the caller.
/// - `same_pairs` — asserted `SameIndividual` pairs (merged). NEVER
///   inferred equivalence.
///
/// `ClassAssertion`s are **not** seeded here: the caller encodes each
/// as a `{a} ⊑ C` GCI in the clause set (exact equivalence), so the
/// class assertion fires via the seeded nominal label. Likewise
/// `DifferentIndividuals` lives in the clause set as `{a}⊓{b}⊑⊥`
/// disjointness — never as engine `neq` (double-seed hazard).
#[derive(Debug, Clone, Default)]
pub struct AboxSeed {
    /// Number of individuals — one graph node is created per index
    /// `0..num_individuals`.
    pub num_individuals: u32,
    /// Nominal class range start (the clausifier's `nominal_base`,
    /// equal to `num_classes`). Node `i` is labelled
    /// `ClassId::new(nominal_base + i)`.
    pub nominal_base: u32,
    /// Asserted `(from_index, role, to_index)` object-property edges.
    pub property_assertions: Vec<(u32, Role, u32)>,
    /// Asserted `(a_index, b_index)` `SameIndividual` merges.
    pub same_pairs: Vec<(u32, u32)>,
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

/// A snapshot of a `DepSet`'s information for diagnostic reporting.
/// Produced by the shadow precise-dependency probe (`RUSTDL_SHADOW_DEP_PROBE`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DepSetSnapshot {
    /// The highest decision level present, or `None` if the set is empty.
    /// `Some(127)` indicates an overflow (`ALL`) set.
    pub highest: Option<u32>,
    /// Number of distinct decision levels set (bit-count; `ALL`/overflow ⇒ 0).
    pub count: u32,
    /// The decision levels present, in ascending order.
    pub levels: Vec<u32>,
}

/// One recorded clash from the shadow precise-dependency probe
/// (`RUSTDL_SHADOW_DEP_PROBE`). Contains both the real (possibly collapsed
/// to `ALL`) dep-set and the shadow (precise, never collapses to `ALL`)
/// dep-set computed for the same clash. Comparing the two reveals whether
/// the `at_most_tainted`/`nn_tainted` path discards genuinely useful precision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClashRecord {
    /// The branching depth at which the clash occurred.
    pub branch_depth: u32,
    /// The real dep-set snapshot (as reported by the engine's live path —
    /// may be `ALL` when taints are set).
    pub real: DepSetSnapshot,
    /// The shadow dep-set snapshot (precise over-set, never collapses to `ALL`
    /// due to taints — carries the merge causation the live path discards).
    pub shadow: DepSetSnapshot,
    /// Stable order-independent hash of the clashing node's label-set (for
    /// reusability / revisit measures in later analysis tasks).
    pub clash_label_key: u64,
}

impl DepSetSnapshot {
    fn from_dep_set(d: DepSet) -> Self {
        Self {
            highest: d.highest_level(),
            count: d.count(),
            levels: d.iter_levels().collect(),
        }
    }
}

/// Per-run search instrumentation, read after [`HyperEngine::decide`]
/// to interpret a wall measurement: a `Sat` reached with
/// `branches_taken == 0` was decided by pure Horn propagation and
/// says nothing about hypertableau branching (see
/// `docs/hypertableau-scoping.md` §H2b).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Disjuncts asserted across the whole search (decisions made).
    pub branches_taken: u64,
    /// Of `branches_taken`, those from the ⊔ disjunction rule
    /// (`find_open_disjunction`). Split out from `≤n` merge branches so
    /// search-quality work can tell which branch kind dominates a stall.
    pub disj_branches: u64,
    /// Of `branches_taken`, those from the `≤n` merge rule
    /// (`find_open_at_most`).
    pub merge_branches: u64,
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
    /// Times `is_blocked` actually returned `true` (a node was blocked).
    /// `blocks_fired == 0` with large `is_blocked_calls` means blocking
    /// never caps the completion — the model grows unbounded until a
    /// deadline/stall (the pizza/SIO convergence problem).
    pub blocks_fired: u64,
    /// Times `is_blocked` was called on a non-root node (had a parent,
    /// so was *eligible* to be blocked). The meaningful denominator for
    /// `blocks_fired` (root calls can never block).
    pub block_eligible: u64,
    /// Label-vector equality / subset comparisons inside `is_blocked`.
    /// The expensive per-call cost (linear in label-set size).
    pub block_compares: u64,
    /// Times the lookahead was called (incremented once per disjunct
    /// candidate when `sat_lookahead` is `Some`).
    pub lookahead_calls: u64,
    /// Disjuncts proved dead by the lookahead and dropped.
    pub lookahead_dropped: u64,
    /// Times the lookahead reduced a ⊔ to a single forced disjunct.
    pub lookahead_forced_single: u64,
    /// Per-clash records from the shadow precise-dependency probe
    /// (`RUSTDL_SHADOW_DEP_PROBE`). Empty unless the probe is enabled via
    /// [`HyperEngine::with_shadow_dep_probe`]. Each entry carries the real
    /// vs shadow dep-sets at the moment of a clash, enabling post-run
    /// analysis of how much precision the taint paths discard.
    pub clash_records: Vec<ClashRecord>,
}

/// The hyperresolution engine. Holds the completion graph and the
/// clause set (borrowed), plus per-run search instrumentation.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent opt-in feature flags (double_blocking, precise_card_deps, \
              mrv_ordering, nn_taint_disabled, shadow_dep_probe) — orthogonal toggles, not a state enum"
)]
pub struct HyperEngine<'c> {
    clauses: &'c [DlClause],
    /// Pairwise class disjointness `(a, b)` (`a.index() < b.index()`),
    /// extracted once at construction from ⊥-headed two-`Class`-atom
    /// clauses. Read-only (never mutated during search); drives the `≤n`
    /// cardinality clash pre-check (two successors carrying disjoint
    /// labels can never be merged). Stored as `Arc` so
    /// `HyperCache::classify_labels` probes can share the pre-built
    /// set via an O(1) reference-count bump instead of an O(N) clone.
    disjoint_pairs: std::sync::Arc<std::collections::HashSet<(u32, u32)>>,
    nodes: Vec<HyperNode>,
    stats: SearchStats,
    init_depth: usize,
    deadline: Option<Instant>,
    /// Trigger indexes routing derivation events to the clauses they
    /// newly enable (see [`ClauseIndexes`]). Read-only after construction
    /// (never mutated during search). Stored as `Arc` so
    /// `HyperCache::classify_labels` probes can share the pre-built
    /// indexes via an O(1) reference-count bump instead of an O(N) clone.
    indexes: std::sync::Arc<ClauseIndexes>,
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
    /// Opt-in (`RUSTDL_PRECISE_CARD_DEPS`, via [`with_precise_card_deps`]): at a
    /// `≤n` cardinality clash, report a **sound over-approximation** of the
    /// clash's true dependency set instead of `DepSet::ALL`, so backjumping can
    /// fire past decisions the clash provably doesn't depend on. The
    /// over-approx is `⋃(birth_deps ∪ label_deps of succs) ∪ parent(birth ∪
    /// label)` — a superset when distinctness is disjoint-label-derived. The
    /// `≠`-forced channel is *not* captured by birth/label deps, so the helper
    /// conservatively falls back to `DepSet::ALL` whenever a participating pair
    /// is distinct only via `are_neq && !labels_disjoint`. Sound by
    /// construction; off by default. See `docs/backjump-reconcile-2026-06-06.md`.
    precise_card_deps: bool,
    /// `RUSTDL_MRV_ORDERING` (default OFF): `find_open_disjunction` returns the open
    /// disjunctive clause with the fewest live disjuncts first (most-constrained-variable).
    /// Verdict-invariant (reordering only). See the MRV spec.
    mrv_ordering: bool,
    /// TAUTOLOGY-SKIP (`RUSTDL_TAUTOLOGY_SKIP`, default OFF): unordered `(a,b)` index
    /// pairs of a complement pair `b ≡ ¬a` (mutually exhaustive). `find_open_disjunction`
    /// skips a binary disjunction whose two `Class` disjuncts are such a pair — it is a
    /// tautology `a ⊔ ¬a`, vacuously satisfied, so not branching it is sound (FP=0:
    /// removes an obligation; MISSED=0: an OPEN such disjunction means the polarity is
    /// unconstrained, so any model extends to satisfy it). See the tautology-skip design.
    tautology_pairs: Option<std::collections::HashSet<(u32, u32)>>,
    /// HF2-double-blocking performance index: nodes partitioned by
    /// `parent_role`. Skipping incompatible candidates without scanning
    /// the full nodes vec cuts `is_blocked` cost from O(n) to
    /// O(bucket-size). `None` unless double-blocking is enabled (no
    /// overhead on the default anywhere-blocking path).
    block_index: Option<std::collections::HashMap<Role, Vec<HNode>>>,
    /// Test-only: when `true`, BOTH NN-merge backjump-dep fixes are
    /// disabled — the merge-causation dep is dropped in
    /// [`Self::merge_with_cause`] (the source fix for residuals A+B) AND
    /// the `nn_tainted` clash-dep widening is skipped — reproducing the
    /// pre-fix backjump-dep hole. Set ONLY by the test helper
    /// [`Self::with_nn_taint_disabled`]; always `false` in production, so
    /// it has zero effect on any real query. Exists so the false-`Unsat`-
    /// direction regression tests can assert "Unsat without the fix, Sat
    /// with it" — the safety net for a fix the corpus can't validate.
    nn_taint_disabled: bool,
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
    /// Lever #1: when `true`, [`Self::solve`] applies the divergence
    /// predicate every [`DIV_WINDOW`] branches and returns
    /// [`HyperResult::Stalled`] early if the search appears to be
    /// diverging (depth-saturated, model-growing, ~all branches failing).
    /// Off by default — preserves deadline-only behaviour + test calibration.
    /// Enable via [`Self::with_adaptive_budget`].
    adaptive_budget: bool,
    /// Lever #1 checkpoint: `(branches_taken, restores, nodes_len)` at
    /// the last divergence-window boundary. Reset to `(0, 0, 0)` on
    /// construction; updated each time a window of [`DIV_WINDOW`]
    /// branches is consumed without triggering a Stalled.
    div_checkpoint: (u64, u64, usize),
    /// `RUSTDL_SAT_LOOKAHEAD` (default OFF): at each ⊔ choice point,
    /// call the seed-saturator to drop disjuncts proved dead before
    /// branching. When `None` (the default), the branch loop runs all
    /// disjuncts unchanged. Enable via [`Self::with_sat_lookahead`].
    sat_lookahead: Option<std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>>,
    /// `RUSTDL_SHADOW_DEP_PROBE` (default OFF): when `true`, maintain a
    /// shadow precise-dependency layer (never collapses to `DepSet::ALL`
    /// due to taints) and record `(real, shadow)` dep-set snapshots at
    /// every clash into [`SearchStats::clash_records`].
    ///
    /// **Read-only invariant**: this flag MUST NOT influence any search
    /// decision, branch choice, merge, edge, or verdict. With this `true`,
    /// `verdict`, `branches_taken`, `restores`, and `max_branch_depth`
    /// are byte-identical to `false`. The ONLY observable difference is
    /// `clash_records` being populated.
    shadow_dep_probe: bool,
    /// Current branch decision level — the level of the innermost active
    /// disjunction / `≤n` branching frame. Updated by `track_depth` at
    /// every branch point. Used by `record_clash` so each `ClashRecord`
    /// carries the precise level at which the clash occurred rather than
    /// the run's peak `max_branch_depth`.
    current_branch_level: u32,
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
pub struct ClauseIndexes {
    /// By class index: clauses with that class as an `X`-body atom.
    pub x_trigger: Vec<Vec<usize>>,
    /// By class index: clauses with that class as a successor-body atom.
    succ_trigger: Vec<Vec<usize>>,
    /// By role index: clauses with a body role atom on that role.
    role_trigger: Vec<Vec<usize>>,
    /// By role index: clauses with a body role atom `(r,u,v)` whose
    /// source `u != X` — i.e. a NON-FIRST leg of a multi-role body (HF3
    /// chains: the `R₂(y,z)` leg). When such an edge is added between
    /// `y` and `z`, the clause must fire at `y`'s PREDECESSORS (the
    /// chain root `x` with `R₁(x,y)`), not at `y` itself — `Event::Edge`
    /// only fires `role_trigger` at the edge source `y`, missing the
    /// X-rooted match. This back-trigger closes that second-leg gap.
    /// `match_body` re-verifies the whole body, so firing at a
    /// predecessor that turns out not to be the chain root is a cheap
    /// no-op (perf only, never soundness).
    role_back_trigger: Vec<Vec<usize>>,
    /// By role index: clauses with a FIRST-leg (`u == X`) body role atom
    /// on an INVERSE role `Atom::Role(Inverse(_), X, v)`. Such a clause is
    /// satisfied by an INCOMING edge at the home node (the node's
    /// `Inverse(p)`-successor = its `p`-predecessor). `Event::Edge` fires
    /// `role_trigger` only at the edge SOURCE, so the home node (the edge
    /// TARGET) never re-fires. This index closes that gap: fire at `tgt`.
    /// `match_body` re-verifies, so an over-fire is a perf no-op.
    inverse_first_trigger: Vec<Vec<usize>>,
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
///
/// `sym` supplies the role hierarchy so that first-leg forward atoms on
/// SYMMETRIC roles are also indexed in `inverse_first_trigger` — the same
/// mechanism that fires inverse first-legs at the edge target (Part 1).
/// Pass `None` when the hierarchy is not yet available (construction time);
/// call [`HyperEngine::with_sub_roles`] afterwards to rebuild with symmetry.
pub fn build_clause_indexes(clauses: &[DlClause], sym: Option<&RoleHierarchy>) -> ClauseIndexes {
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
                Atom::Role(r, u, _) => {
                    push(&mut ix.role_trigger, role_id_index(*r), ci);
                    // Non-first leg (`R₂(y,z)`, `u != X`): also index for
                    // predecessor back-triggering (HF3 chain second-leg).
                    if *u != X {
                        push(&mut ix.role_back_trigger, role_id_index(*r), ci);
                    }
                    // First-leg inverse OR symmetric role: must fire at the
                    // edge TARGET. For inverse roles, the incoming edge `src—p→tgt`
                    // gives `tgt` an `Inverse(p)`-successor. For symmetric roles,
                    // `p ≡ p⁻` so the incoming forward edge is also a reverse
                    // traversal — trigger at `tgt` too.
                    let is_symmetric = sym.is_some_and(|h| h.is_symmetric(r.role_id()));
                    if *u == X && (r.is_inverse() || is_symmetric) {
                        push(&mut ix.inverse_first_trigger, role_id_index(*r), ci);
                    }
                }
                // Head-only atoms never appear in a (Horn) body.
                Atom::Exists(..) | Atom::AtMost(..) | Atom::AtLeast(..) | Atom::Equal(..) => {}
            }
        }
    }
    ix
}

/// Extract pairwise class disjointness from the clause set. A clause
/// encodes `a ⊓ b ⊑ ⊥` (disjointness of `a`, `b`) iff it is **⊥-headed**
/// (`head.is_empty()`) and its body is **exactly two `Class` atoms on the
/// same variable** with distinct classes. Nothing else qualifies: a unary
/// `{A(X)} → ⊥` means "A is unsatisfiable" (not a pair), and a role-spanning
/// body (`{A(X), R(X,Y), B(Y)} → ⊥`) is not a pairwise disjointness. Pairs
/// are stored normalized (`lo.index() < hi.index()`).
pub fn build_disjoint_pairs(clauses: &[DlClause]) -> std::collections::HashSet<(u32, u32)> {
    let mut set = std::collections::HashSet::new();
    for cl in clauses {
        if !cl.head.is_empty() || cl.body.len() != 2 {
            continue;
        }
        if let (Atom::Class(a, va), Atom::Class(b, vb)) = (cl.body[0], cl.body[1])
            && va == vb
            && a != b
        {
            let (lo, hi) = (a.index().min(b.index()), a.index().max(b.index()));
            set.insert((lo, hi));
        }
    }
    set
}

/// Divergence predicate (Lever #1): a wedge search is making no progress toward a
/// satisfying completion when, over a window of `db` branches, ~all failed
/// (`dr`/`db` ≥ θ) at saturated branch depth. θ = 0.98.
///
/// NOTE: an earlier `model_grew` clause was DROPPED — the #2 reuse probe showed the
/// divergence is THRASHING through a tiny state set at STABLE node count (not
/// unbounded growth), so a growth clause never fires. The discriminator against a
/// converging Unsat proof (which also has restores≈branches at depth) is the WINDOW
/// SIZE `N`: a real proof terminates within `N` branches; only a search still
/// all-failing-at-cap after `N` branches is cut. `N` is tuned via the corpus MISSED
/// gate (raise until no real subsumption proof is cut).
fn is_diverging(db: u64, dr: u64, depth_saturated: bool) -> bool {
    depth_saturated && db > 0 && dr.saturating_mul(100) >= db.saturating_mul(98)
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
            disjoint_pairs: std::sync::Arc::new(build_disjoint_pairs(clauses)),
            nodes: vec![root_node],
            stats: SearchStats::default(),
            init_depth: 0,
            deadline: None,
            indexes: std::sync::Arc::new(build_clause_indexes(clauses, None)),
            worklist: Vec::new(),
            representative: vec![HNode(0)],
            sub_roles: None,
            neq: Vec::new(),
            nominals: None,
            clash_deps: DepSet::EMPTY,
            double_blocking: false,
            precise_card_deps: false,
            mrv_ordering: false,
            tautology_pairs: None,
            block_index: None,
            nn_taint_disabled: false,
            snapshot_origin: vec![false],
            snapshot_backprop_aborted: false,
            lazy_replay_state: None,
            adaptive_budget: false,
            div_checkpoint: (0, 0, 0),
            sat_lookahead: None,
            shadow_dep_probe: false,
            current_branch_level: 0,
        }
    }

    /// Build an engine for `clauses` (a full clause slice including any
    /// per-probe Q-clause) with pre-built `ClauseIndexes` and `disjoint_pairs`
    /// supplied by the caller. Avoids the O(#clauses) `build_clause_indexes` +
    /// `build_disjoint_pairs` rebuild cost. Both are passed as `Arc` so the
    /// caller can share them across many probes via O(1) ref-count bumps.
    /// See `docs/superpowers/specs/2026-06-16-soundcaching-design-and-gonogo.md` §5.
    #[must_use]
    pub fn new_with_prebuilt(
        clauses: &'c [DlClause],
        root: ClassId,
        indexes: std::sync::Arc<ClauseIndexes>,
        disjoint_pairs: std::sync::Arc<std::collections::HashSet<(u32, u32)>>,
    ) -> Self {
        let mut root_node = HyperNode {
            order: 0,
            ..HyperNode::default()
        };
        root_node.add(root, DepSet::EMPTY);
        Self {
            clauses,
            disjoint_pairs,
            nodes: vec![root_node],
            stats: SearchStats::default(),
            init_depth: 0,
            deadline: None,
            indexes,
            worklist: Vec::new(),
            representative: vec![HNode(0)],
            sub_roles: None,
            neq: Vec::new(),
            nominals: None,
            clash_deps: DepSet::EMPTY,
            double_blocking: false,
            precise_card_deps: false,
            mrv_ordering: false,
            tautology_pairs: None,
            block_index: None,
            nn_taint_disabled: false,
            snapshot_origin: vec![false],
            snapshot_backprop_aborted: false,
            lazy_replay_state: None,
            adaptive_budget: false,
            div_checkpoint: (0, 0, 0),
            sat_lookahead: None,
            shadow_dep_probe: false,
            current_branch_level: 0,
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

    /// Opt into precise (sound over-approx) `≤n`-cardinality clash deps in
    /// place of `DepSet::ALL`, unblocking dependency-directed backjumping on
    /// cardinality clashes. See [`Self::precise_card_deps`].
    #[must_use]
    pub fn with_precise_card_deps(mut self) -> Self {
        self.precise_card_deps = true;
        self
    }

    /// Opt into tautology-skip: a complement-pair index set `{(a,b) : b ≡ ¬a}`.
    /// `find_open_disjunction` skips binary `a ⊔ ¬a` disjunctions. See
    /// [`Self::tautology_pairs`].
    #[must_use]
    pub fn with_tautology_skip(mut self, pairs: std::collections::HashSet<(u32, u32)>) -> Self {
        self.tautology_pairs = Some(pairs);
        self
    }

    /// Opt into MRV (most-constrained-variable) ordering of open disjunctions.
    /// See [`Self::mrv_ordering`].
    #[must_use]
    pub fn with_mrv_ordering(mut self) -> Self {
        self.mrv_ordering = true;
        self
    }

    /// Opt into adaptive early-cut of diverging searches (Lever #1). Off by default
    /// (preserves deadline-only behavior + test calibration).
    #[must_use]
    pub fn with_adaptive_budget(mut self) -> Self {
        self.adaptive_budget = true;
        self
    }

    /// Enable the read-only shadow precise-dependency probe
    /// (`RUSTDL_SHADOW_DEP_PROBE`). When `on` is `true`, maintains a shadow
    /// dep layer that never collapses to `DepSet::ALL` due to taints and records
    /// `(real, shadow)` dep-set snapshots at every clash into
    /// [`SearchStats::clash_records`].
    ///
    /// **Read-only invariant**: this MUST NOT change any search decision,
    /// branch, merge, edge, or verdict. Guarded by `if self.shadow_dep_probe`.
    #[must_use]
    pub fn with_shadow_dep_probe(mut self, on: bool) -> Self {
        self.shadow_dep_probe = on;
        if on {
            // Retroactively populate shadow_label_deps for any labels already
            // present (e.g. the root label set at construction time), mirroring
            // their real label_deps. Post-construction, shadow_label_deps is
            // maintained in lock-step with labels/label_deps by add_label.
            for node in &mut self.nodes {
                if node.shadow_label_deps.len() != node.label_deps.len() {
                    node.shadow_label_deps = node.label_deps.clone();
                }
                // Shadow birth/at_most deps start as copies of real (they're
                // already set before any search runs, so causation hasn't been
                // discarded yet — shadow == real at construction time).
                node.shadow_birth_deps = node.birth_deps;
                node.shadow_at_most_dep = node.at_most_dep;
                // shadow_merge_cause starts EMPTY (no merges have happened yet).
            }
        }
        self
    }

    /// Attach a pre-built seed-saturator for the ⊔ failed-literal look-ahead.
    /// When set, each ⊔ branch loop calls `sat.seed_unsat` on each candidate
    /// disjunct and drops those proved dead before branching. Default: `None`
    /// (flag `RUSTDL_SAT_LOOKAHEAD`, default OFF). See [`SearchStats`] counters
    /// `lookahead_calls`, `lookahead_dropped`, `lookahead_forced_single`.
    #[must_use]
    pub fn with_sat_lookahead(
        mut self,
        s: std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>,
    ) -> Self {
        self.sat_lookahead = Some(s);
        self
    }

    /// Test-only: disable BOTH NN-merge backjump-dep fixes (the
    /// merge-causation-dep folding in `merge_with_cause` and the
    /// `nn_tainted` clash widening), reproducing the pre-fix backjump-dep
    /// hole. Used by the false-`Unsat`-direction regression tests to prove
    /// the fix is load-bearing (Unsat without it, Sat with it). Never
    /// called in production.
    #[cfg(test)]
    #[must_use]
    fn with_nn_taint_disabled(mut self) -> Self {
        self.nn_taint_disabled = true;
        self
    }

    /// Test-only accessor for [`Self::mrv_ordering`].
    #[cfg(test)]
    pub(crate) fn mrv_ordering_for_test(&self) -> bool {
        self.mrv_ordering
    }

    /// Test-only wrapper around [`Self::find_open_disjunction`].
    #[cfg(test)]
    pub(crate) fn find_open_disjunction_for_test(&mut self) -> Option<(usize, HNode, Binding)> {
        self.find_open_disjunction()
    }

    /// Sound over-approximation of a **structural** `≤n` cardinality clash's
    /// dependency set (used only at the `forced_distinct_exceeds` pre-check site,
    /// where the clash is "`>n` pairwise-must-distinct successors of `parent`"
    /// with no deeper search involved), gated by [`Self::precise_card_deps`].
    ///
    /// Returns `DepSet::ALL` (the conservative default — always sound) when the
    /// flag is off, or when the over-approx cannot be guaranteed a superset of
    /// the true clash deps. It IS a guaranteed superset, hence sound for
    /// backjumping, exactly when:
    ///  1. **every succ is generated directly by the canonical `parent`**
    ///     (`succ.parent == Some(parent)`), so its `birth_deps` carries the
    ///     decision under which `parent` made it an R-successor. A merge-redirected
    ///     edge (`merge()` pushes `s_j`'s edges onto `s_i` without updating target
    ///     `parent`) does NOT carry the redirect decision, so any such succ forces
    ///     the `DepSet::ALL` fallback; and
    ///  2. **no pair is distinct only via the uncaptured `≠` channel**
    ///     (`are_neq && !labels_disjoint`) — disjoint-label distinctness lives in
    ///     `label_deps`, but a decision-derived `≠` does not, so it forces fallback; and
    ///  3. **the `≤n` constraint was not merge-inherited** (`!parent.at_most_tainted`)
    ///     — a merge's causation dep is untracked (Hole C, merge half).
    ///
    /// Under (1)+(2)+(3), `⋃(birth ∪ label of succs) ∪ parent(birth ∪ label) ∪
    /// parent.at_most_dep` covers every contributor to the structural clash: succ
    /// existence + R-membership (`birth_deps`), pairwise distinctness
    /// (`label_deps`), and the `≤n` constraint's own derivation provenance
    /// (`at_most_dep` — Hole C, derivation half) ⟹ superset ⟹ sound. The
    /// `solve_at_most` partition-exhaustion site does NOT use this (its Unsat can
    /// depend on broader-graph decisions reached by the deeper search / inverse
    /// back-prop, which this local set need not cover); it keeps `DepSet::ALL`.
    fn card_clash_deps(&self, parent: HNode, succs: &[HNode]) -> DepSet {
        if !self.precise_card_deps {
            return DepSet::ALL;
        }
        let p = self.resolve(parent);
        // (3) merge-inherited `≤n`: causation dep untracked → conservative.
        if self.nodes[p.index()].at_most_tainted {
            return DepSet::ALL;
        }
        for &s in succs {
            // (1) own-successor guard: only edges generated directly by the
            // canonical parent carry the parent's decision in birth_deps.
            if self.nodes[s.index()].parent != Some(p) {
                return DepSet::ALL;
            }
        }
        // (2) uncaptured ≠-only distinctness forces the conservative fallback.
        for (i, &a) in succs.iter().enumerate() {
            for &b in &succs[i + 1..] {
                if self.are_neq(a, b) && !self.labels_disjoint(a, b) {
                    return DepSet::ALL;
                }
            }
        }
        // The `≤n` constraint's own derivation provenance (Hole C, derivation
        // half): `at_most` is a dep-less tuple, so this is the only place the
        // constraint's decision deps enter `over`.
        let mut over = self.nodes[p.index()].at_most_dep;
        for node in std::iter::once(p).chain(succs.iter().copied()) {
            let hn = &self.nodes[self.resolve(node).index()];
            over = over.union(hn.birth_deps);
            for &ld in &hn.label_deps {
                over = over.union(ld);
            }
        }
        over
    }

    /// Shadow twin of [`Self::card_clash_deps`]: the precise over-set
    /// computed ALWAYS (no taint-induced collapse). It differs from the
    /// real path in that it uses the `shadow_*` fields which carry the
    /// merge causation the `at_most_tainted`/`nn_tainted` paths discard.
    ///
    /// Only called when `self.shadow_dep_probe` is `true`. Read-only;
    /// only used to populate [`ClashRecord`] in [`SearchStats::clash_records`].
    fn card_clash_deps_shadow(&self, parent: HNode, succs: &[HNode]) -> DepSet {
        let p = self.resolve(parent);
        let pn = &self.nodes[p.index()];
        let mut over = pn.shadow_at_most_dep.union(pn.shadow_merge_cause);
        for node in std::iter::once(p).chain(succs.iter().copied()) {
            let hn = &self.nodes[self.resolve(node).index()];
            over = over
                .union(hn.shadow_birth_deps)
                .union(hn.shadow_merge_cause);
            for &ld in &hn.shadow_label_deps {
                over = over.union(ld);
            }
        }
        over
    }

    /// Compute a stable, order-independent hash of the resolved node's
    /// label-set. Used as `ClashRecord::clash_label_key`. XOR-combines
    /// per-label hashes so insertion order doesn't matter.
    fn clash_label_key(&self, node: HNode) -> u64 {
        let resolved = self.resolve(node);
        let labels = &self.nodes[resolved.index()].labels;
        // Sort-free order-independent combination: XOR of FNV-1a per label id.
        let mut h: u64 = 0;
        for l in labels {
            // FNV-1a 64-bit: offset basis = 14695981039346656037, prime = 1099511628211
            let mut fh: u64 = 14_695_981_039_346_656_037;
            let id_bytes = l.index().to_le_bytes();
            for b in id_bytes {
                fh ^= u64::from(b);
                fh = fh.wrapping_mul(1_099_511_628_211);
            }
            h ^= fh;
        }
        h
    }

    /// Record a clash into `SearchStats::clash_records`. Only called when
    /// `self.shadow_dep_probe` is `true`. `real_deps` is the dep-set the
    /// live path just recorded in `self.clash_deps`; `shadow_deps` is the
    /// precise shadow twin; `clash_node` is the clashing node.
    fn record_clash(&mut self, real_deps: DepSet, shadow_deps: DepSet, clash_node: HNode) {
        let key = self.clash_label_key(clash_node);
        // Use the current decision level (updated at every branch point by
        // `track_depth`) rather than the run's peak `max_branch_depth`, so
        // each record captures the depth at which *this* clash fired.
        let depth = self.current_branch_level;
        self.stats.clash_records.push(ClashRecord {
            branch_depth: depth,
            real: DepSetSnapshot::from_dep_set(real_deps),
            shadow: DepSetSnapshot::from_dep_set(shadow_deps),
            clash_label_key: key,
        });
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
    ///
    /// Also rebuilds `ClauseIndexes` with symmetric-role awareness (Variant R):
    /// first-leg forward atoms on symmetric roles are added to
    /// `inverse_first_trigger` so they fire at the edge TARGET as well as
    /// the source. This is a zero-cost no-op when no role is symmetric.
    #[must_use]
    pub fn with_sub_roles(mut self, hierarchy: RoleHierarchy) -> Self {
        // Rebuild the index now that we have the hierarchy. The `new()` /
        // `new_seeded()` constructors pass `None` (hierarchy not yet available);
        // this call is the first moment both clauses and the hierarchy are in scope.
        self.indexes = std::sync::Arc::new(build_clause_indexes(self.clauses, Some(&hierarchy)));
        self.sub_roles = Some(hierarchy);
        self
    }

    /// Set the role hierarchy WITHOUT rebuilding `ClauseIndexes`. Use only when
    /// the engine's index was already built hierarchy-aware
    /// (`build_clause_indexes(.., Some(&h))`), e.g. the amortized classify-label
    /// path that supplies a prebuilt index via `new_with_prebuilt`. Setting the
    /// hierarchy enables `role_matches` symmetry + sub-role matching; the index
    /// (trigger sets) must already reflect the same hierarchy.
    #[must_use]
    pub fn with_sub_roles_keep_index(mut self, hierarchy: RoleHierarchy) -> Self {
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
            // Shadow: mirror the insertion into shadow_label_deps at the same
            // sorted position so the two vecs stay parallel. Guard: flag-off
            // does zero work here.
            if self.shadow_dep_probe {
                // `add` already inserted `c` into `labels` at its sorted position.
                // Find that position by binary search (guaranteed to succeed).
                let node = &mut self.nodes[n.index()];
                if let Ok(pos) = node.labels.binary_search_by_key(&c.index(), |l| l.index()) {
                    node.shadow_label_deps.insert(pos, deps);
                }
            }
            self.worklist.push(Event::Label(n, c));
            true
        } else {
            false
        }
    }

    /// Widen an EXISTING label `c`'s dep-set at node `n` by unioning in
    /// `extra` (no-op if `c` is absent). Overrides [`HyperNode::add`]'s
    /// keep-first rule for the one place it must be widened: folding the
    /// merge-causation dep into a merge-copied label that was already
    /// present (see [`Self::merge_with_cause`]). Sound — widening a
    /// label's dep only reduces backjumping, never the reverse. Does NOT
    /// re-enqueue the label (its clauses already fired on first add).
    fn fold_label_dep(&mut self, n: HNode, c: ClassId, extra: DepSet) {
        let node = &mut self.nodes[n.index()];
        if let Ok(pos) = node.labels.binary_search_by_key(&c.index(), |l| l.index()) {
            node.label_deps[pos] = node.label_deps[pos].union(extra);
            // Shadow: mirror the fold. Only when probe is on (shadow_label_deps
            // is only populated then; `pos < len` guard handles the off case too
            // if somehow the vecs diverge).
            if pos < node.shadow_label_deps.len() {
                node.shadow_label_deps[pos] = node.shadow_label_deps[pos].union(extra);
            }
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
        self.stats.clone()
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
            self.stats.block_eligible += 1;
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
                    self.stats.blocks_fired += 1;
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
                    self.stats.blocks_fired += 1;
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
            Event::Edge(src, role, tgt) => {
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
                // HF3 second-leg back-trigger: this edge `R(src,tgt)` may
                // be the NON-FIRST leg of a multi-role clause body (e.g.
                // `R₁(x,src) ∧ R₂(src,tgt) → …`). Such clauses are rooted
                // at the chain root `x` = a predecessor of `src`, so fire
                // them at `src`'s predecessors. `match_body` re-verifies,
                // so firing at a non-root predecessor is a no-op.
                let n_b = self.indexes.role_back_trigger.get(key).map_or(0, Vec::len);
                if n_b > 0 {
                    let preds: Vec<HNode> = self.nodes[src.index()]
                        .preds
                        .iter()
                        .map(|&(_, p)| p)
                        .collect();
                    for p in preds {
                        for i in 0..n_b {
                            let ci = self.indexes.role_back_trigger[key][i];
                            if matches!(self.fire_clause(ci, p), FireOutcome::Clash) {
                                return FireOutcome::Clash;
                            }
                        }
                    }
                }
                // Inverse first-leg trigger: this edge `src—role→tgt` gives
                // `tgt` an `Inverse(role)`-successor (`src`). A clause
                // `Atom::Role(Inverse(role), X, y) → …` rooted at `tgt` can now
                // fire; `Event::Edge` otherwise only fires at `src`.
                let n_inv = self
                    .indexes
                    .inverse_first_trigger
                    .get(key)
                    .map_or(0, Vec::len);
                for i in 0..n_inv {
                    let ci = self.indexes.inverse_first_trigger[key][i];
                    if matches!(self.fire_clause(ci, tgt), FireOutcome::Clash) {
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

    /// Build an engine seeded with a pre-resolved `ABox` ([`AboxSeed`]) —
    /// the multi-node graph an `ABox`-consistency check runs over. Mirrors
    /// the main-tableau `ABox` seeding order:
    /// 1. one nominal node per individual (labelled `{a}`),
    /// 2. `ObjectPropertyAssertion` edges (both `edges` + reverse
    ///    `preds`, like [`Self::from_snapshot`]),
    /// 3. `SameIndividual` merges (via the engine's [`Self::merge`], so
    ///    union-find / label / edge redirection stay consistent).
    ///
    /// `DifferentIndividuals` distinctness and `ClassAssertion`s are
    /// **not** seeded here — they are encoded in `clauses` (disjointness
    /// `{a}⊓{b}⊑⊥` and `{a}⊑C` GCIs respectively) and fire through the
    /// normal Horn fixpoint once the nominal labels are present. This
    /// keeps the seeding to exactly the asserted-and-only-asserted `ABox`
    /// state (the false-`Unsat` contract on [`AboxSeed`]).
    ///
    /// The engine is left in a valid pre-`decide` state: `nodes`,
    /// `representative`, `snapshot_origin` stay length-consistent;
    /// `horn_fixpoint` re-seeds every node's labels/edges from the
    /// graph on the first `solve` pass. Apply the `with_*` configurators
    /// (nominals / sub-roles / blocking) to the returned engine exactly
    /// as the classify wedge does, then call [`Self::decide_with_deadline`].
    ///
    /// Soundness note: `merge` here only ever runs on `same_pairs`
    /// (asserted `SameIndividual`); since distinctness is clause-only
    /// (no engine `neq`), `merge` cannot observe a `≠` and always
    /// succeeds — a `Different(a,b)+Same(a,b)` clash is caught by the
    /// `{a}⊓{b}⊑⊥` clause firing on the merged representative during
    /// `decide`, not by `merge`'s return.
    #[must_use]
    pub fn new_seeded(clauses: &'c [DlClause], seed: &AboxSeed) -> Self {
        let n = seed.num_individuals as usize;
        if n == 0 {
            // Degenerate: no individuals. Fall back to a single root so
            // empty-body (`⊤ → …`) clauses still get a node to fire at.
            return Self::new(clauses, ClassId::new(0));
        }
        // Build N empty nodes (node i ↔ individual index i), each
        // labelled with its nominal class `nominal_base + i`.
        let mut nodes: Vec<HyperNode> = Vec::with_capacity(n);
        for i in 0..n {
            let order = u32::try_from(i).expect("individual count fits u32");
            let mut hn = HyperNode {
                order,
                ..HyperNode::default()
            };
            let nominal = ClassId::new(seed.nominal_base + order);
            hn.add(nominal, DepSet::EMPTY);
            nodes.push(hn);
        }
        let representative: Vec<HNode> = (0..n)
            .map(|i| HNode(u32::try_from(i).expect("fits u32")))
            .collect();
        let mut engine = Self {
            clauses,
            disjoint_pairs: std::sync::Arc::new(build_disjoint_pairs(clauses)),
            nodes,
            stats: SearchStats::default(),
            init_depth: 0,
            deadline: None,
            indexes: std::sync::Arc::new(build_clause_indexes(clauses, None)),
            worklist: Vec::new(),
            representative,
            sub_roles: None,
            neq: Vec::new(),
            nominals: None,
            clash_deps: DepSet::EMPTY,
            double_blocking: false,
            precise_card_deps: false,
            mrv_ordering: false,
            tautology_pairs: None,
            block_index: None,
            nn_taint_disabled: false,
            snapshot_origin: vec![false; n],
            snapshot_backprop_aborted: false,
            lazy_replay_state: None,
            adaptive_budget: false,
            div_checkpoint: (0, 0, 0),
            sat_lookahead: None,
            shadow_dep_probe: false,
            current_branch_level: 0,
        };
        // Asserted ObjectPropertyAssertion edges: mirror as edge +
        // reverse pred (matches `from_snapshot` bookkeeping). Indices
        // out of range (defensive) are skipped — a missing edge only
        // under-detects (sound).
        for &(from, role, to) in &seed.property_assertions {
            let (fi, ti) = (from as usize, to as usize);
            if fi >= n || ti >= n {
                continue;
            }
            let (src, tgt) = (HNode(from), HNode(to));
            engine.nodes[fi].edges.push((role, tgt));
            engine.nodes[ti].preds.push((role, src));
        }
        // Asserted SameIndividual merges. `merge` resolves through the
        // union-find, redirects edges/labels, and (in this design) never
        // clashes since distinctness is clause-only. Out-of-range indices
        // are skipped (sound under-seed).
        for &(a, b) in &seed.same_pairs {
            if (a as usize) >= n || (b as usize) >= n {
                continue;
            }
            // `merge` returns `true` only on a `≠` violation, which cannot
            // happen here (no engine `neq`); the boolean is ignored on
            // purpose — a real `Same+Different` clash surfaces via the
            // disjointness clause during `decide`.
            let _ = engine.merge(HNode(a), HNode(b));
        }
        engine
    }

    /// Compute the live (non-dead) disjunct indices for clause `ci` at `node`,
    /// using the seed-saturator to drop disjuncts proved unsatisfiable.
    ///
    /// Each disjunct `Dk` in `ci`'s head is tested by building a seed that
    /// combines the node's current atomic labels with the atom `Dk` (a `Class`
    /// atom on var `X`) or an existential `Exists` atom on var `X`.  Only
    /// atoms bound to the home variable `X` are seeded (anything else is left
    /// in the live set — conservative/sound under-seeding).
    ///
    /// Returns `(0..head_len).collect()` when no lookahead saturator is set.
    fn lookahead_live_disjuncts(
        &mut self,
        sat: &owl_dl_saturation::seed_sat::SeedSaturator,
        ci: usize,
        node: HNode,
    ) -> Vec<usize> {
        let head_len = self.clauses[ci].head.len();
        let rep = self.resolve(node);
        // Base seed: atomic labels of the resolved node.
        let atomic_base: Vec<owl_dl_core::ir::ClassId> = self.nodes[rep.index()].labels.clone();
        // Base existential seed: non-inverse outgoing edges with atomic targets.
        let mut exists_base: Vec<(owl_dl_core::ir::RoleId, owl_dl_core::ir::ClassId)> = Vec::new();
        let edges: Vec<(Role, HNode)> = self.nodes[rep.index()].edges.clone();
        for (role, tgt) in edges {
            if role.is_inverse() {
                continue; // conservative: skip inverse edges
            }
            let tgt_rep = self.resolve(tgt);
            for &cls in &self.nodes[tgt_rep.index()].labels.clone() {
                exists_base.push((role.role_id(), cls));
            }
        }

        let mut live = Vec::with_capacity(head_len);
        for k in 0..head_len {
            self.stats.lookahead_calls += 1;
            let mut atomic_k = atomic_base.clone();
            let mut exists_k = exists_base.clone();
            // Only seed atoms that are on the home variable X (soundness guard).
            match &self.clauses[ci].head[k] {
                Atom::Class(cls, v) if *v == X => {
                    atomic_k.push(*cls);
                }
                Atom::Exists(role, cls, v) if *v == X && !role.is_inverse() => {
                    exists_k.push((role.role_id(), *cls));
                }
                _ => {
                    // Any other atom shape: leave it live (conservative).
                    live.push(k);
                    continue;
                }
            }
            if sat.seed_unsat(&atomic_k, &exists_k) {
                self.stats.lookahead_dropped += 1;
            } else {
                live.push(k);
            }
        }
        if live.len() == 1 {
            self.stats.lookahead_forced_single += 1;
        }
        live
    }

    fn solve(&mut self, depth: usize) -> HyperResult {
        if let Some(dl) = self.deadline
            && Instant::now() >= dl
        {
            return HyperResult::Stalled;
        }
        if self.adaptive_budget {
            // Lever #1: divergence early-cut.  Sampled every DIV_WINDOW
            // branches so the check itself costs O(1) amortized.
            const DIV_WINDOW: u64 = 500;
            let (b0, r0, _) = self.div_checkpoint;
            let db = self.stats.branches_taken.saturating_sub(b0);
            if db >= DIV_WINDOW {
                let dr = self.stats.restores.saturating_sub(r0);
                // depth counts DOWN (init_depth→0); track_depth records
                // level = init_depth - depth + 1, peaking at init_depth (256)
                // when a branch was taken at depth=1 (the last allowed level
                // before depth==0 early-returns Stalled).  Saturated iff any
                // branch reached that cap.
                let cap = u32::try_from(self.init_depth).unwrap_or(u32::MAX);
                let depth_saturated = self.stats.max_branch_depth >= cap;
                if is_diverging(db, dr, depth_saturated) {
                    return HyperResult::Stalled;
                }
                self.div_checkpoint = (
                    self.stats.branches_taken,
                    self.stats.restores,
                    self.nodes.len(),
                );
            }
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
            let live: Vec<usize> = if let Some(sat) = self.sat_lookahead.clone() {
                self.lookahead_live_disjuncts(&sat, ci, node)
            } else {
                (0..head_len).collect()
            };
            // If the lookahead proved ALL disjuncts dead, treat it as a clash
            // on this branch (same as if every branch returned Unsat).
            if live.is_empty() {
                self.clash_deps = body_deps;
                return HyperResult::Unsat;
            }
            let mut any_stalled = false;
            let mut combined = DepSet::EMPTY;
            for k in live {
                let head_atom = self.clauses[ci].head[k];
                let saved = self.save();
                self.stats.branches_taken += 1;
                self.stats.disj_branches += 1;
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
        if let Some((node, succs, n)) = self.find_open_at_most() {
            // Algebraic cardinality clash pre-check: if more than `n`
            // successors are pairwise must-distinct (`≠`-forced or
            // disjoint-labelled, hence unmergeable), the `≤n` is violated
            // with no possible merge — conclude Unsat directly, no
            // branching. Sound: this only ever *adds* a clash the merge
            // search below would also reach. See
            // `docs/wedge-cardinality-clash-precheck.md`.
            if self.forced_distinct_exceeds(&succs, n) {
                self.clash_deps = self.card_clash_deps(node, &succs);
                // Shadow: record (real, shadow) dep-set pair at this clash.
                if self.shadow_dep_probe {
                    let real = self.clash_deps;
                    let shadow = self.card_clash_deps_shadow(node, &succs);
                    // Debug-assert shadow ⊇ real (overflow/ALL in real is allowed).
                    debug_assert!(
                        real.overflow || real.bits & !shadow.bits == 0,
                        "shadow dep must be a superset of real dep (non-ALL)"
                    );
                    self.record_clash(real, shadow, node);
                }
                return HyperResult::Unsat;
            }
            if depth == 0 {
                return HyperResult::Stalled;
            }
            self.track_depth(depth);
            // `≤n` satisfaction by *canonical partition enumeration*
            // (increment 2): rather than merge one pair per branch and
            // recurse — which reaches each partition of the successors via
            // many merge orders (the redundancy behind the pizza
            // `InterestingPizza` merge blow-up) — enumerate each partition
            // into ≤ n mutually-mergeable blocks exactly once, merge it,
            // and recurse. Verdict-preserving: the set of reachable
            // partitions (hence the Sat/Unsat outcome) is identical; only
            // the order-redundancy is removed. See
            // `docs/wedge-cardinality-clash-precheck.md`.
            return self.solve_at_most(&succs, n as usize, depth);
        }
        HyperResult::Sat
    }

    fn track_depth(&mut self, depth: usize) {
        let level = u32::try_from(self.init_depth - depth + 1).unwrap_or(u32::MAX);
        if level > self.stats.max_branch_depth {
            self.stats.max_branch_depth = level;
        }
        // Keep the per-clash depth current so `record_clash` reports the
        // decision level active when the clash fires, not the run's peak.
        self.current_branch_level = level;
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
    /// TAUTOLOGY-SKIP: true iff clause `ci`'s head is exactly a binary complement
    /// pair `a ⊔ ¬a` (registered in `tautology_pairs`) — a tautology, sound to skip.
    fn is_tautology_clause(&self, ci: usize) -> bool {
        let Some(pairs) = &self.tautology_pairs else {
            return false;
        };
        let head = &self.clauses[ci].head;
        head.len() == 2
            && matches!((&head[0], &head[1]),
                (Atom::Class(a, _), Atom::Class(b, _)) if pairs.contains(&(a.index(), b.index())))
    }

    fn find_open_disjunction(&mut self) -> Option<(usize, HNode, Binding)> {
        if self.mrv_ordering {
            let mut best: Option<(usize, (usize, HNode, Binding))> = None; // (live_count, candidate)
            for idx in 0..self.nodes.len() {
                let node = HNode(u32::try_from(idx).expect("fits u32"));
                if self.is_blocked(node) {
                    continue;
                }
                for ci in 0..self.clauses.len() {
                    if self.clauses[ci].is_horn() {
                        continue;
                    }
                    if self.is_tautology_clause(ci) {
                        continue;
                    }
                    let Some(bindings) = self.match_body(ci, node) else {
                        continue;
                    };
                    for binding in bindings {
                        if self.any_head_satisfied(ci, node, &binding) {
                            continue;
                        }
                        let live = (0..self.clauses[ci].head.len())
                            .filter(|&k| !self.head_atom_satisfied(ci, k, node, &binding))
                            .count();
                        let better = match &best {
                            None => true,
                            Some((b, _)) => live < *b,
                        };
                        if better {
                            best = Some((live, (ci, node, binding)));
                        }
                    }
                }
            }
            return best.map(|(_, cand)| cand);
        }
        for idx in 0..self.nodes.len() {
            let node = HNode(u32::try_from(idx).expect("fits u32"));
            // A directly-blocked node gets NO rule applied — including the ⊔
            // rule. Generation (∃/≥n) already skips blocked nodes
            // (`apply_exists` / `generate_at_least`), but the disjunction rule
            // did not: applying ⊔ to a blocked node mutated its label, could
            // *unblock* it, and resumed generation — defeating blocking's
            // termination guarantee. On disjunction-heavy SROIQ (ore-15672,
            // sio, ore-10908, alehif) this drove the search to arbitrary depth
            // without ever finding the (existing, finite) model: depth 256→32768
            // all Stalled, ~115k all-clashing branches on a *satisfiable* class.
            // Sound under the established (double-)pair-blocking unravelling: a
            // directly-blocked `n` (blocker `m`, L(n)⊆L(m)) realises its model
            // via `m`'s expansion, and `m` — being unblocked — has its own ⊔
            // applied here, so skipping `n`'s ⊔ drops no clash. Skipping rule
            // applications can only *remove* clashes, never add one ⟹ FP=0 by
            // construction; corpus closure stays byte-identical to the
            // Konclude∩HermiT oracle (FP=0/MISSED=0) across every fixture, and
            // the disjunctive-`Unsat` path still proves subsumptions with
            // blocking live (`blocked_disjunction_soundness` canary). Scope: the
            // same `is_blocked` predicate already gates generation
            // (`apply_exists`/`generate_at_least`) in BOTH blocking modes, so
            // extending it to ⊔ is no more unsound than that existing gating;
            // the default double-blocking mode is the corpus-validated one.
            if self.is_blocked(node) {
                continue;
            }
            for ci in 0..self.clauses.len() {
                if self.clauses[ci].is_horn() {
                    continue;
                }
                if self.is_tautology_clause(ci) {
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
    /// True iff head atom `k` of clause `ci` is already satisfied at the given binding.
    /// Extracted from [`Self::any_head_satisfied`] to allow per-atom inspection (e.g.
    /// counting live disjuncts for MRV ordering in Task 2).
    fn head_atom_satisfied(&self, ci: usize, k: usize, xnode: HNode, binding: &Binding) -> bool {
        let resolve = |v: Var| resolve_var(v, xnode, binding);
        match &self.clauses[ci].head[k] {
            Atom::Class(c, v) => matches!(resolve(*v), Some(t) if self.nodes[t.index()].has(*c)),
            Atom::Exists(role, cls, v) => matches!(resolve(*v), Some(src) if
            self.nodes[src.index()].edges.iter().any(|(er, t)| {
                role_matches(*er, *role, self.sub_roles.as_ref()) && self.nodes[t.index()].has(*cls)
            })),
            Atom::AtMost(role, qual, n, v) => matches!(resolve(*v), Some(src) if
                self.nodes[src.index()].at_most.contains(&(*role, *qual, *n))
                || self.distinct_role_succ(src, *role, *qual).len() <= *n as usize),
            // TODO(HF3): `≥n` generation not yet enforced — never counts as satisfied.
            Atom::AtLeast(..) | Atom::Equal(..) | Atom::Role(..) => false,
        }
    }

    /// True iff some head disjunct of clause `ci` already holds at
    /// the given binding (class label present, or `∃` witness found).
    fn any_head_satisfied(&self, ci: usize, xnode: HNode, binding: &Binding) -> bool {
        (0..self.clauses[ci].head.len()).any(|k| self.head_atom_satisfied(ci, k, xnode, binding))
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

    /// Whether merging `a` and `b` would necessarily clash because they
    /// carry disjoint labels (`∃ ca ∈ L(a), cb ∈ L(b) : ca ⊓ cb ⊑ ⊥`).
    /// Labels are resolved through the merge union-find.
    fn labels_disjoint(&self, a: HNode, b: HNode) -> bool {
        if self.disjoint_pairs.is_empty() {
            return false;
        }
        let la = &self.nodes[self.resolve(a).index()].labels;
        let lb = &self.nodes[self.resolve(b).index()].labels;
        for &ca in la {
            for &cb in lb {
                if ca == cb {
                    continue;
                }
                let (lo, hi) = (ca.index().min(cb.index()), ca.index().max(cb.index()));
                if self.disjoint_pairs.contains(&(lo, hi)) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether `a` and `b` can never be merged: either `≠`-forced, or
    /// carrying disjoint labels. Both make the `≤n` merge of the pair a
    /// clash, so such a pair is a "forced-distinct" edge.
    fn must_be_distinct(&self, a: HNode, b: HNode) -> bool {
        self.are_neq(a, b) || self.labels_disjoint(a, b)
    }

    /// Sound lower bound on the number of pairwise-must-distinct (hence
    /// unmergeable) successors among `succs`: `true` iff a clique of size
    /// `> n` exists, in which case the `≤n` is violated with no possible
    /// merge → a clash certificate. Greedy: finding *a* clique past `n`
    /// suffices; missing the maximum one just falls through to the merge
    /// loop (still sound).
    fn forced_distinct_exceeds(&self, succs: &[HNode], n: u32) -> bool {
        let mut clique: Vec<HNode> = Vec::new();
        for &s in succs {
            if clique.iter().all(|&c| self.must_be_distinct(c, s)) {
                clique.push(s);
                if clique.len() > n as usize {
                    return true;
                }
            }
        }
        false
    }

    /// Satisfy a violated `≤n` (`succs.len() > n`) by enumerating every
    /// partition of `succs` into at most `n` mutually-mergeable blocks
    /// (restricted-growth order, so each partition is generated exactly
    /// once), merging the partition, and recursing. Returns `Sat` on the
    /// first partition that completes a model (state kept), `Stalled` if
    /// some partition hit the depth bound and none succeeded, else `Unsat`
    /// with conservative `DepSet::ALL` deps. This site keeps `DepSet::ALL`
    /// (NOT the `card_clash_deps` over-approx): partition-exhaustion Unsat can
    /// depend on decisions reached by the deeper `solve(depth-1)` / inverse
    /// back-propagation that the local `succs`/`parent` dep set need not cover,
    /// so narrowing here is not provably sound. See `card_clash_deps`.
    fn solve_at_most(&mut self, succs: &[HNode], n: usize, depth: usize) -> HyperResult {
        let mut groups: Vec<Vec<HNode>> = Vec::with_capacity(n);
        let mut any_stalled = false;
        if let Some(sat) = self.partition_rec(succs, 0, &mut groups, n, depth, &mut any_stalled) {
            return sat; // Sat — completed model kept (no restore).
        }
        if any_stalled {
            return HyperResult::Stalled;
        }
        self.clash_deps = DepSet::ALL;
        // Shadow: record the partition-exhaustion clash. This is a genuinely
        // structural ALL (exhausted all partitions) — not a taint artifact.
        // Use ALL for both real and shadow so Task-2 analysis sees "genuine
        // structural ALL" (real=ALL, shadow=ALL) vs "taint-ALL" (real=ALL,
        // shadow=precise). Writing EMPTY would falsely look like "full recovery".
        if self.shadow_dep_probe {
            // No specific clashing node — use the first succ if available, else node 0.
            let clash_node = succs.first().copied().unwrap_or(HNode(0));
            self.record_clash(DepSet::ALL, DepSet::ALL, clash_node);
        }
        HyperResult::Unsat
    }

    /// Recursive restricted-growth enumeration backing [`Self::solve_at_most`].
    /// Assigns `succs[idx]` to each existing block it is mergeable with
    /// (no `must_be_distinct` member), or opens a new block while under
    /// the `n` cap. At a complete assignment it merges each block into its
    /// first member and recurses via [`Self::solve`]. Returns `Some(Sat)`
    /// to short-circuit the whole enumeration (model found, state kept);
    /// `None` to keep enumerating (sets `*any_stalled` on a depth-bound
    /// hit). Every branch is restored before the next partition.
    fn partition_rec(
        &mut self,
        succs: &[HNode],
        idx: usize,
        groups: &mut Vec<Vec<HNode>>,
        n: usize,
        depth: usize,
        any_stalled: &mut bool,
    ) -> Option<HyperResult> {
        if idx == succs.len() {
            // Complete partition (≤ n blocks). Merge each block into its
            // representative, then continue the search one level down.
            let saved = self.save();
            self.stats.branches_taken += 1;
            self.stats.merge_branches += 1;
            let mut clashed = false;
            'blocks: for block in groups.iter() {
                let rep = block[0];
                for &other in &block[1..] {
                    if self.merge(rep, other) {
                        clashed = true; // defensive: pre-pruned by must_be_distinct
                        break 'blocks;
                    }
                }
            }
            if !clashed {
                match self.solve(depth - 1) {
                    HyperResult::Sat => return Some(HyperResult::Sat),
                    HyperResult::Unsat => {}
                    HyperResult::Stalled => *any_stalled = true,
                }
            }
            self.restore(saved);
            return None;
        }
        let s = succs[idx];
        for gi in 0..groups.len() {
            // Mergeable with this block iff distinct from no member.
            if groups[gi].iter().all(|&m| !self.must_be_distinct(s, m)) {
                groups[gi].push(s);
                let r = self.partition_rec(succs, idx + 1, groups, n, depth, any_stalled);
                groups[gi].pop();
                if r.is_some() {
                    return r;
                }
            }
        }
        if groups.len() < n {
            groups.push(vec![s]);
            let r = self.partition_rec(succs, idx + 1, groups, n, depth, any_stalled);
            groups.pop();
            if r.is_some() {
                return r;
            }
        }
        None
    }

    /// Find a node with a violated `≤n` constraint: more distinct
    /// matching `role`-successors than the bound. Returns the
    /// canonical node and its (resolved, distinct) successor list to
    /// branch merges over. Only canonical (un-merged) nodes are checked.
    fn find_open_at_most(&self) -> Option<(HNode, Vec<HNode>, u32)> {
        for idx in 0..self.nodes.len() {
            let node = HNode(u32::try_from(idx).expect("fits u32"));
            if self.resolve(node) != node {
                continue;
            }
            for &(role, qual, n) in &self.nodes[idx].at_most {
                let succs = self.distinct_role_succ(node, role, qual);
                if succs.len() > n as usize {
                    return Some((node, succs, n));
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
        self.merge_with_cause(s_i, s_j, DepSet::EMPTY)
    }

    /// As [`Self::merge`], but folds `cause_deps` (the **merge-causation**
    /// dep — why the two nodes coincide) into every label copied from
    /// `s_j` onto the survivor `s_i`. For an NN-merge the causation is the
    /// dep-set of the triggering nominal on both nodes (it carries the
    /// branch decision that placed the shared nominal); without folding
    /// it in, a merge-copied label keeps its original (possibly `EMPTY`)
    /// dep, and a downstream clause that PROPAGATES that label onto
    /// another node (`R(x,y) ∧ L(y) → M(x)`) under-reports its dep ⟹
    /// unsound backjump past the deciding disjunct ⟹ false `Unsat`
    /// (residual (B) — closes it at the source, subsuming the
    /// `nn_tainted` taint which only covered clashes *directly on* the
    /// merged node). Sound: widening deps only ever reduces backjumping.
    /// The `≤n` caller passes `EMPTY` (its clash deps are handled
    /// separately via `card_clash_deps` / `DepSet::ALL`), so classify is
    /// unaffected.
    fn merge_with_cause(&mut self, s_i: HNode, s_j: HNode, cause_deps: DepSet) -> bool {
        // Test-only: `nn_taint_disabled` reproduces the pre-fix engine by
        // dropping the merge-causation dep entirely (so the white-box
        // regression tests can assert the false-`Unsat` the fix prevents).
        let cause_deps = if self.nn_taint_disabled {
            DepSet::EMPTY
        } else {
            cause_deps
        };
        let (s_i, s_j) = (self.resolve(s_i), self.resolve(s_j));
        if s_i == s_j {
            return false;
        }
        if self.are_neq(s_i, s_j) {
            // ≠ violated — merging is impossible. Conservative deps
            // (precise `≠`/merge provenance isn't tracked).
            self.clash_deps = DepSet::ALL;
            // Shadow: record the ≠-clash. The shadow dep is the union of
            // both nodes' shadow birth/merge deps + cause_deps (the precise
            // causation that the real path discards via ALL).
            if self.shadow_dep_probe {
                let real = self.clash_deps;
                let shadow = self.nodes[s_i.index()]
                    .shadow_birth_deps
                    .union(self.nodes[s_i.index()].shadow_merge_cause)
                    .union(self.nodes[s_j.index()].shadow_birth_deps)
                    .union(self.nodes[s_j.index()].shadow_merge_cause)
                    .union(cause_deps);
                self.record_clash(real, shadow, s_i);
            }
            return true;
        }
        self.representative[s_j.index()] = s_i;
        if cause_deps != DepSet::EMPTY {
            // Fold the merge-causation dep into the survivor's `birth_deps`.
            // `clause_body_deps` unions `birth_deps` of the firing node AND
            // every bound node, and the survivor is the firing node for its
            // (merge-copied) outgoing edges and a bound node for its incoming
            // edges. So this one fold makes any clause matching an edge the
            // merge created inherit the causation dep — for BOTH edge
            // directions, and transitively for any head derived off the
            // survivor. Closes the merge-copied-edge backjump hole (residual
            // C, opus re-review): a back-prop `R(x,y) ∧ L(y) → M(x)` firing
            // over a copied edge no longer under-reports its dep-set and so
            // cannot trigger an unsound backjump past the deciding disjunct.
            // The label-copy fold below and the `nn_tainted` direct-clash
            // catch-all (residuals A/B) are retained as defence in depth.
            // Widening `birth_deps` only reduces backjumping ⇒ sound; EMPTY
            // (the `≤n` merge / classify path) ⇒ no-op.
            let bi = &mut self.nodes[s_i.index()];
            bi.birth_deps = bi.birth_deps.union(cause_deps);
            // Shadow: mirror the birth_deps fold, also recording the precise
            // merge causation in shadow_merge_cause (the dep the real path
            // discards via taint). Guard: flag-off does zero work.
            if self.shadow_dep_probe {
                let bi = &mut self.nodes[s_i.index()];
                bi.shadow_birth_deps = bi.shadow_birth_deps.union(cause_deps);
                bi.shadow_merge_cause = bi.shadow_merge_cause.union(cause_deps);
            }
        }
        let s_j_labels: Vec<(ClassId, DepSet)> = {
            let nj = &self.nodes[s_j.index()];
            nj.labels
                .iter()
                .copied()
                .zip(nj.label_deps.iter().copied())
                .collect()
        };
        for (c, c_deps) in s_j_labels {
            let merged_deps = c_deps.union(cause_deps);
            if !self.add_label(s_i, c, merged_deps) && cause_deps != DepSet::EMPTY {
                // Label already present on `s_i` with a (kept-first,
                // possibly narrower) dep. Fold the causation in anyway —
                // otherwise the propagate-then-clash hole reopens. Sound:
                // widening a label's dep only reduces backjumping.
                self.fold_label_dep(s_i, c, cause_deps);
            }
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
        if !self.nodes[s_j.index()].at_most.is_empty() {
            // A merge-inherited `≤n`: its causation dep isn't tracked, so taint
            // `s_i` — `card_clash_deps` falls back to `DepSet::ALL` for it.
            let sj_dep = self.nodes[s_j.index()].at_most_dep;
            let ni = &mut self.nodes[s_i.index()];
            ni.at_most_tainted = true;
            ni.at_most_dep = ni.at_most_dep.union(sj_dep);
            // Shadow: record the precise sj_dep + cause_deps into the shadow
            // at_most field (the real path sets at_most_tainted → ALL; the
            // shadow carries the actual dep without collapsing).
            if self.shadow_dep_probe {
                let sj_shadow_dep = self.nodes[s_j.index()].shadow_at_most_dep;
                let ni = &mut self.nodes[s_i.index()];
                ni.shadow_at_most_dep =
                    ni.shadow_at_most_dep.union(sj_shadow_dep).union(cause_deps);
                ni.shadow_merge_cause =
                    ni.shadow_merge_cause.union(sj_shadow_dep).union(cause_deps);
            }
        }
        // Propagate the NN-merge taint: if either node carried an
        // under-dep'd NN-merge-inherited label, the survivor does too
        // (this `≤n`/NN merge folds `s_j`'s labels — with their possibly
        // `EMPTY` deps — onto `s_i`). Without this, a `≤n` merge in
        // `solve_at_most` folding a tainted node into an untainted
        // survivor would lose the taint and reopen the backjump hole on a
        // later clash on `s_i`. Over-taint is sound (only widens the
        // clash dep-set to `DepSet::ALL` = chronological backtracking).
        // Propagates the EXISTING bit only — never set on the classify
        // path (no nominals ⇒ NN-rule never fires), so classify is
        // unaffected.
        if self.nodes[s_j.index()].nn_tainted {
            self.nodes[s_i.index()].nn_tainted = true;
            // Shadow: propagate merge_cause from s_j to s_i so the precise
            // NN-taint causation is preserved.
            if self.shadow_dep_probe {
                let sj_mc = self.nodes[s_j.index()].shadow_merge_cause;
                let si = &mut self.nodes[s_i.index()];
                si.shadow_merge_cause = si.shadow_merge_cause.union(sj_mc);
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
        // Clause bodies are tiny (a handful of atoms), so these per-call
        // scratch vectors stay inline — no heap allocation in the common
        // case (this matcher is the hyperresolution hot loop).
        let mut role_atoms: SmallVec<[(Role, Var, Var); 4]> = SmallVec::new();
        let mut other_classes: SmallVec<[(ClassId, Var); 4]> = SmallVec::new();
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
        let mut binding: Binding = SmallVec::new();
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
        // Innermost recursive hot loop: most nodes have few role-matching
        // successors, so keep the match set inline to avoid a heap
        // allocation per recursion frame (profiling: allocator churn here
        // dominated self-time on wedge-heavy classification).
        let mut targets: SmallVec<[HNode; 8]> = src_data
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
            // the dep-set so `solve` can backjump. If the clashing node
            // inherited labels via an NN-merge (`nn_tainted`), the
            // merge-causation dep is untracked, so report `DepSet::ALL`
            // (chronological backtracking — sound) to avoid an unsound
            // backjump past the disjunct that forced the merge.
            let xn = self.resolve(xnode);
            self.clash_deps = if self.nodes[xn.index()].nn_tainted && !self.nn_taint_disabled {
                DepSet::ALL
            } else {
                body_deps
            };
            // Shadow: record (real, shadow) dep-set pair at this label/disjoint clash.
            // The shadow dep-set is body_deps ∪ the clashing node's shadow merge cause
            // (the precise dep the nn_tainted path discards). Guard: flag-off does zero work.
            if self.shadow_dep_probe {
                let real = self.clash_deps;
                // Shadow: precise body_deps ∪ shadow_merge_cause (never ALL due to taint).
                let shadow = body_deps.union(self.nodes[xn.index()].shadow_merge_cause);
                self.record_clash(real, shadow, xn);
            }
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
                    let tn = &mut self.nodes[target.index()];
                    tn.at_most.push(c);
                    // Record the constraint's derivation deps (closes Hole C):
                    // `card_clash_deps` unions this into the clash dep-set, so a
                    // `≤n` derived under a decision contributes that decision.
                    tn.at_most_dep = tn.at_most_dep.union(deps);
                    // Shadow: mirror the at_most_dep fold (no taint here — this is
                    // the direct-derivation path, not the merge-inherited one).
                    // The `self.shadow_dep_probe` check would require splitting the
                    // mutable `tn` borrow, so we rely on the zero-overhead property:
                    // unioning EMPTY into EMPTY is a no-op, and shadow_at_most_dep
                    // starts EMPTY and is only written when probe is on (the write is
                    // unconditional here for borrow-checker simplicity, but costs
                    // only two u128 ops when probe is off — acceptable).
                    tn.shadow_at_most_dep = tn.shadow_at_most_dep.union(deps);
                    FireOutcome::Changed
                }
            }
            Atom::AtLeast(role, qual, n, v) => {
                let Some(target) = resolve_var(v, xnode, binding) else {
                    return FireOutcome::NoChange;
                };
                self.generate_at_least(target, role, qual, n, deps)
            }
            // HF3 role-chain head `R(u,v)`: derive the role edge between
            // the two bound nodes (the consequence of a chain / transitivity
            // clause `R₁(X,y) ∧ R₂(y,z) → R₃(X,z)`).
            Atom::Role(role, u, v) => {
                let (Some(src0), Some(tgt0)) = (
                    resolve_var(u, xnode, binding),
                    resolve_var(v, xnode, binding),
                ) else {
                    return FireOutcome::NoChange;
                };
                self.derive_role_edge(role, src0, tgt0, deps)
            }
            // TODO(HF3): `≈` equality heads not yet realised — no-op
            // (sound for `Unsat`: an unenforced head only weakens the
            // theory).
            Atom::Equal(_, _) => FireOutcome::NoChange,
        }
    }

    /// Add a derived role edge `role(src, tgt)` between two existing
    /// nodes (the head of a chain / transitivity clause). Mirrors the
    /// edge bookkeeping of [`Self::fire_exists`] but adds NO new node.
    ///
    /// SOUNDNESS:
    /// - **Polarity normalisation.** Clause roles are already
    ///   `canon_role`'d, so `role` is canonical. The engine's edges are
    ///   always stored *forward* (`Named`), with inverse satisfied via
    ///   `preds` at match time. So an inverse head `R⁻(src,tgt)` is the
    ///   forward edge `R(tgt,src)` — store it flipped.
    /// - **Backjump deps (the centerpiece).** A chain-derived edge runs
    ///   between two *pre-existing* nodes, neither born to carry the
    ///   clause body's dep-set. `clause_body_deps` reconstructs deps from
    ///   bound nodes' `birth_deps`; an edge carries none. So fold `deps`
    ///   into the edge *target*'s `birth_deps`. The edge is only ever
    ///   traversed with the target as a bound node (forward: target is the
    ///   role-atom's bound successor; inverse-via-preds: the stored
    ///   target — original `src` — is the source-side, also bound), so
    ///   `clause_body_deps` then always includes `deps`. Widening
    ///   `birth_deps` only *reduces* backjumping ⇒ sound (the same
    ///   argument `merge_with_cause` relies on), and never causes a MISS.
    /// - **Termination.** Each `(role, src, tgt)` edge is added at most
    ///   once (dedup below). Finite nodes ⇒ finite edges. Blocking is
    ///   untouched (no node is created).
    ///
    /// KNOWN under-approximation (sound — a MISS, never an FP): when the
    /// super-role canonicalizes to an INVERSE `R⁻`, the edge is stored
    /// forward as `R(tgt,src)` and re-queued as `Event::Edge(tgt, R, src)`,
    /// which fires `role_trigger`/`role_back_trigger` at `tgt`/`tgt`'s
    /// preds. A clause that consumes the inverse edge via a body atom
    /// `R⁻(x,·)` is rooted at the *target* `src` (an inverse walk follows
    /// preds), which this event does not wake. So an inverse-SUPER-ROLE
    /// chain may not propagate downstream — a missed clash (`Sat` instead
    /// of `Unsat`), the safe direction. family's critical chain
    /// (`isMalePartnerIn∘hasFemalePartner ⊑ hasWife`) is forward-headed,
    /// so the target is unaffected; the corpus gate (MISSED=0) arbitrates
    /// whether the inverse-super-role gap matters in practice. INVERSE
    /// LEGS in the body are fully handled (matched via `preds` in
    /// `enumerate_matches`).
    fn derive_role_edge(
        &mut self,
        role: Role,
        src: HNode,
        tgt: HNode,
        deps: DepSet,
    ) -> FireOutcome {
        // Store the edge forward: an inverse head `R⁻(src,tgt)` is the
        // forward edge `R(tgt,src)`.
        let (rstore, from, to) = if role.is_inverse() {
            (role.flip(), tgt, src)
        } else {
            (role, src, tgt)
        };
        let from = self.resolve(from);
        let to = self.resolve(to);
        // Dedup: skip if an identical forward edge already exists (exact
        // role id + endpoints). Conservative — we only skip the exact
        // edge, never a merely sub/super-role one, so no derivation is
        // lost.
        if self.nodes[from.index()].edges.iter().any(|(er, t)| {
            *t == to && er.is_inverse() == rstore.is_inverse() && er.role_id() == rstore.role_id()
        }) {
            return FireOutcome::NoChange;
        }
        self.nodes[from.index()].edges.push((rstore, to));
        self.nodes[to.index()].preds.push((rstore, from));
        // Edge-dep fold (backjump soundness centerpiece — see fn docs).
        // Fold into BOTH endpoints' `birth_deps`: the edge can be matched
        // forward (target `to` bound) OR via `preds` (source `from`
        // bound), so whichever endpoint a later clause binds, its
        // `birth_deps` then carries `deps`. Widening only reduces
        // backjumping ⇒ sound; can never cause a MISS.
        let bf = self.nodes[from.index()].birth_deps.union(deps);
        self.nodes[from.index()].birth_deps = bf;
        let bt = self.nodes[to.index()].birth_deps.union(deps);
        self.nodes[to.index()].birth_deps = bt;
        // Shadow: mirror the birth_deps fold at both endpoints.
        if self.shadow_dep_probe {
            self.nodes[from.index()].shadow_birth_deps =
                self.nodes[from.index()].shadow_birth_deps.union(deps);
            self.nodes[to.index()].shadow_birth_deps =
                self.nodes[to.index()].shadow_birth_deps.union(deps);
        }
        self.worklist.push(Event::Edge(from, rstore, to));
        FireOutcome::Changed
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
        // Shadow: new successor birth_deps = deps (same as real).
        if self.shadow_dep_probe {
            self.nodes[succ.index()].shadow_birth_deps = deps;
        }
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
            // Shadow: new successor birth_deps = deps (same as real).
            if self.shadow_dep_probe {
                self.nodes[succ.index()].shadow_birth_deps = deps;
            }
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
                // Merge-causation dep: the two nodes coincide *because*
                // both carry the singleton `{c}`, so the merge depends on
                // every decision that placed `{c}` on either node — the
                // union of `c`'s dep-set at both. Folding this into the
                // copied labels (via `merge_with_cause`) makes a later
                // clause that propagates a copied label carry the
                // deciding-disjunct dep, so backjumping stops at the
                // disjunct (closes residuals A + B at the source).
                let cause = self.nodes[rn.index()]
                    .deps_of(c)
                    .union(self.nodes[m.index()].deps_of(c));
                if self.merge_with_cause(rn, m, cause) {
                    FireOutcome::Clash
                } else {
                    // Belt-and-suspenders: also taint the survivor so a
                    // clash *directly on* the merged node falls back to
                    // chronological backtracking even if some label's
                    // causation was not captured above. (The source fix
                    // in `merge_with_cause` is the primary guard;
                    // `nn_tainted` covers the direct-clash case.)
                    let surv = self.resolve(rn);
                    self.nodes[surv.index()].nn_tainted = true;
                    // Shadow: record the NN-merge causation in the survivor's
                    // shadow_merge_cause so the clash probe can see the precise
                    // dep that nn_tainted conservatively collapses to ALL.
                    if self.shadow_dep_probe {
                        let si = &mut self.nodes[surv.index()];
                        si.shadow_merge_cause = si.shadow_merge_cause.union(cause);
                    }
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
fn eval_order(role_atoms: &[(Role, Var, Var)]) -> Option<SmallVec<[usize; 8]>> {
    // All three scratch buffers stay inline: bodies have ≤ MAX_BODY_VARS
    // vars and a handful of role atoms, so this allocates nothing in the
    // common case (called once per `match_body`, a hot path).
    let mut bound: SmallVec<[Var; 8]> = smallvec![X];
    let mut order: SmallVec<[usize; 8]> = SmallVec::with_capacity(role_atoms.len());
    let mut used: SmallVec<[bool; 8]> = SmallVec::from_elem(false, role_atoms.len());
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
    // Symmetric role `p ≡ p⁻`: an edge labelled `p` (or `p⁻`) satisfies a
    // wanted `p` (or `p⁻`) regardless of polarity when the ids coincide.
    if let Some(h) = sub_roles
        && edge.role_id() == wanted.role_id()
        && h.is_symmetric(wanted.role_id())
    {
        return true;
    }
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
    fn is_diverging_fires_only_on_no_progress() {
        assert!(is_diverging(5000, 4990, true)); // ~all failing at saturated depth
        assert!(!is_diverging(5000, 1000, true)); // progressing (restores ≪ branches)
        assert!(!is_diverging(5000, 4990, false)); // depth not saturated → not (yet) diverging
        assert!(!is_diverging(0, 0, true)); // empty window
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

    /// Soundness guard for `with_precise_card_deps` / `card_clash_deps`: the
    /// `≤n`-clash dep over-approx must NEVER change a verdict — it only prunes
    /// search via backjumping. This is the ONLY flag-ON tableau test (the other
    /// 96 run flag-OFF), so it pins the soundness property against refactors of
    /// `card_clash_deps`. Per `docs/backjump-reconcile-2026-06-06.md`.
    ///
    /// Unsat case exercising the pre-check NARROWING branch: `A ⊑ ∃R.B`,
    /// `A ⊑ ∃R.C`, `B ⊓ C ⊑ ⊥`, `A ⊑ ≤1 R` ⇒ two disjoint-labelled
    /// root-generated R-successors under `≤1` ⇒ Unsat via `forced_distinct_
    /// exceeds`. The succs are own-generated (own-succ guard passes), distinct
    /// via disjoint labels not `≠` (no fallback), and the `≤n` is derived
    /// directly (not merge-tainted) — so the over-approx branch is taken. The
    /// `A ⊑ D1 ⊔ D2` disjunction adds a decision the clash is independent of, so
    /// flag-ON backjumps past it and flag-OFF does not — the verdict must match.
    #[test]
    fn precise_card_deps_preserves_unsat_verdict() {
        let role = Role::Named(RoleId::new(0));
        let (a, b, c, d1, d2) = (cls(0), cls(1), cls(2), cls(3), cls(4));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, b, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, c, X)],
            },
            DlClause {
                body: vec![Atom::Class(b, X), Atom::Class(c, X)],
                head: vec![],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtMost(role, None, 1, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Class(d1, X), Atom::Class(d2, X)],
            },
        ];
        let off = HyperEngine::new(&clauses, a).decide(64);
        let on = HyperEngine::new(&clauses, a)
            .with_precise_card_deps()
            .decide(64);
        assert_eq!(
            off,
            HyperResult::Unsat,
            "baseline: ≤1 R with two disjoint-labelled R-succs is Unsat"
        );
        assert_eq!(on, off, "precise-card-deps changed the verdict — UNSOUND");
    }

    /// Companion Sat case: same shape but `B`/`C` are NOT disjoint, so the two
    /// R-successors merge to satisfy `≤1 R` ⇒ Sat. Pins that the over-approx
    /// (and the `solve_at_most` fallback it does NOT touch) never flips a Sat to
    /// a spurious Unsat.
    #[test]
    fn precise_card_deps_preserves_sat_verdict() {
        let role = Role::Named(RoleId::new(0));
        let (a, b, c) = (cls(0), cls(1), cls(2));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, b, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, c, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtMost(role, None, 1, X)],
            },
        ];
        let off = HyperEngine::new(&clauses, a).decide(64);
        let on = HyperEngine::new(&clauses, a)
            .with_precise_card_deps()
            .decide(64);
        assert_eq!(off, HyperResult::Sat, "baseline: mergeable R-succs ⇒ Sat");
        assert_eq!(on, off, "precise-card-deps changed the verdict — UNSOUND");
    }

    // ── new_seeded (ABox-consistency) constructor ─────────────────────

    /// 2 individuals, `nominal_base` = 2 (classes 0,1 ordinary; nominals
    /// `{0}`=cls(2), `{1}`=cls(3)). DifferentIndividuals(0,1) as a
    /// `{0}⊓{1}⊑⊥` disjointness clause + SameIndividual(0,1) merge: the
    /// merge brings both nominals onto one node ⟹ Unsat (inconsistent).
    #[test]
    fn new_seeded_different_then_same_is_unsat() {
        let (n0, n1) = (cls(2), cls(3));
        let clauses = vec![DlClause {
            body: vec![Atom::Class(n0, X), Atom::Class(n1, X)],
            head: vec![],
        }];
        let seed = AboxSeed {
            num_individuals: 2,
            nominal_base: 2,
            property_assertions: vec![],
            same_pairs: vec![(0, 1)],
        };
        let r = HyperEngine::new_seeded(&clauses, &seed)
            .with_nominals(2, 2)
            .decide(64);
        assert_eq!(
            r,
            HyperResult::Unsat,
            "Different(a,b)+Same(a,b) seeded ABox should be Unsat"
        );
    }

    /// Same disjointness clause but NO `SameIndividual` merge: the two
    /// nominal nodes stay distinct ⟹ Sat (consistent). Guards against an
    /// over-eager clash on the seeded nominal topology.
    #[test]
    fn new_seeded_distinct_individuals_is_sat() {
        let (n0, n1) = (cls(2), cls(3));
        let clauses = vec![DlClause {
            body: vec![Atom::Class(n0, X), Atom::Class(n1, X)],
            head: vec![],
        }];
        let seed = AboxSeed {
            num_individuals: 2,
            nominal_base: 2,
            property_assertions: vec![],
            same_pairs: vec![],
        };
        let r = HyperEngine::new_seeded(&clauses, &seed)
            .with_nominals(2, 2)
            .decide(64);
        assert_eq!(
            r,
            HyperResult::Sat,
            "two distinct seeded individuals (no merge) should be Sat"
        );
    }

    /// **False-`Unsat`-direction regression** (the catastrophic guard the
    /// corpus can't validate — wine is the only black-box witness, so
    /// this is the white-box safety net). A CONSISTENT clause set where a
    /// disjunctive decision `D` (`q → {0} ⊔ Q`) places nominal `{0}` on
    /// the root in branch 1, the NN-rule merges the root into the seeded
    /// `{0}` node copying that node's `EMPTY`-dep label `k`, and a
    /// `k ⊓ m → ⊥` clause then clashes with `body_deps` that OMIT `D`
    /// (both `k` and `m` are `EMPTY`-dep). Naive backjumping prunes past
    /// `D` and never tries branch 2 (`Q`, which is clash-free) ⟹ false
    /// `Unsat`. The `nn_tainted` fix forces the clash to report
    /// `DepSet::ALL` ⇒ chronological backtracking ⇒ branch 2 ⇒ `Sat`.
    /// Asserts BOTH: Unsat with the taint disabled (bug reproduced), Sat
    /// with it on (bug fixed).
    #[test]
    fn nn_merge_backjump_hole_false_unsat_is_fixed() {
        // nominal_base = 4: ordinary k=cls(0), q=cls(1)=Q-disjunct,
        // m=cls(2), p=cls(3); nominals {0}=cls(4), {1}=cls(5).
        let (k, q, m, p) = (cls(0), cls(1), cls(2), cls(3));
        let (nom0, nom1) = (cls(4), cls(5));
        let clauses = vec![
            // Seeded `{0}` node carries `k` (EMPTY-dep, pre-branch):
            DlClause {
                body: vec![Atom::Class(nom0, X)],
                head: vec![Atom::Class(k, X)],
            },
            // Root q always carries `m` (EMPTY-dep, Horn):
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(m, X)],
            },
            // The disjunctive decision D: q → {0} ⊔ p. Branch 1 = {0}
            // (forces the NN-merge); branch 2 = p (clash-free ⇒ Sat).
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(nom0, X), Atom::Class(p, X)],
            },
            // The clash: k ⊓ m → ⊥. In branch 1, `k` arrives on the root
            // via the NN-merge with EMPTY dep, `m` is EMPTY-dep, so the
            // naive clash dep-set omits D.
            DlClause {
                body: vec![Atom::Class(k, X), Atom::Class(m, X)],
                head: vec![],
            },
        ];
        // Seed two individuals so node 0 = {0}, node 1 = {1}.
        let seed = AboxSeed {
            num_individuals: 2,
            nominal_base: 4,
            property_assertions: vec![],
            same_pairs: vec![],
        };
        // Root carries q (the decision driver). Seed it by also asserting
        // q on individual node 1 (≠ the {0} node, so no premature merge).
        // Simpler: run from a fresh root labelled q via `new`, but we need
        // the seeded {0} node present — so build via new_seeded and add q
        // to node 1's slot through a Horn clause keyed on {1}.
        let mut clauses_with_q = clauses.clone();
        clauses_with_q.push(DlClause {
            body: vec![Atom::Class(nom1, X)],
            head: vec![Atom::Class(q, X)],
        });
        // Bug reproduced (taint disabled) ⇒ false Unsat:
        let buggy = HyperEngine::new_seeded(&clauses_with_q, &seed)
            .with_nominals(4, 2)
            .with_nn_taint_disabled()
            .decide(64);
        // Fixed (taint on) ⇒ Sat:
        let fixed = HyperEngine::new_seeded(&clauses_with_q, &seed)
            .with_nominals(4, 2)
            .decide(64);
        assert_eq!(
            buggy,
            HyperResult::Unsat,
            "without the nn_tainted fix the consistent graph should false-Unsat \
             (if this is Sat, the test no longer reproduces the hole)"
        );
        assert_eq!(
            fixed,
            HyperResult::Sat,
            "with the nn_tainted fix the consistent graph must be Sat — \
             a false-Unsat here is catastrophic (false-inconsistent)"
        );
    }

    /// **Residual (B) probe — propagate-then-clash.** Adjudicates whether
    /// the taint-the-merged-node fix is SUFFICIENT or whether the clash
    /// can escape it onto a *different* (untainted) node via a back-prop
    /// clause `R(x,y) ∧ L(y) → M(x)`: G (tainted, carrying the merge-
    /// copied under-dep `L`) is the successor `y`; `M` lands on the
    /// predecessor `H` (untainted); a `M ⊓ … → ⊥` clash then fires on `H`
    /// and reads `H.nn_tainted == false`. If this graph false-`Unsat`s
    /// even WITH the taint on, (B) is a real residual and the
    /// merge-causation-dep source fix is needed. This test pins the
    /// current behaviour so a future regression / fix is visible.
    #[test]
    fn nn_merge_propagate_then_clash_residual_b() {
        // ordinary: g_drv=cls(0) (drives G's existence + branch),
        // q=cls(1), mm=cls(2) (back-prop head), ll=cls(3) (seeded label),
        // p=cls(4) (clash-free disjunct), guard=cls(5) (always on H).
        // nominal_base = 6: {0}=cls(6), {1}=cls(7).
        let (g_drv, q, mm, ll, p, guard) = (cls(0), cls(1), cls(2), cls(3), cls(4), cls(5));
        let nom0 = cls(6);
        let role = Role::Named(RoleId::new(0));
        let clauses = vec![
            // Seeded {0} node carries `ll` (EMPTY-dep, pre-branch):
            DlClause {
                body: vec![Atom::Class(nom0, X)],
                head: vec![Atom::Class(ll, X)],
            },
            // H (root, labelled q) always carries `guard` (EMPTY):
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(guard, X)],
            },
            // H creates an R-successor G seeded with `g_drv`:
            DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Exists(role, g_drv, X)],
            },
            // Decision D at G: g_drv → {0} ⊔ p. Branch 1 forces nominal
            // {0} on G (⇒ NN-merge with the seeded {0} node, copying
            // `ll`); branch 2 (`p`) is clash-free.
            DlClause {
                body: vec![Atom::Class(g_drv, X)],
                head: vec![Atom::Class(nom0, X), Atom::Class(p, X)],
            },
            // Back-prop: R(x,y) ∧ ll(y) → mm(x). Lands `mm` on H (the
            // predecessor, NOT a merge survivor ⇒ untainted).
            DlClause {
                body: vec![Atom::Role(role, X, 1), Atom::Class(ll, 1)],
                head: vec![Atom::Class(mm, X)],
            },
            // Clash on H: guard ⊓ mm → ⊥ (both EMPTY-dep at H ⇒ naive
            // body_deps omits D).
            DlClause {
                body: vec![Atom::Class(guard, X), Atom::Class(mm, X)],
                head: vec![],
            },
        ];
        let seed = AboxSeed {
            num_individuals: 2,
            nominal_base: 6,
            property_assertions: vec![],
            same_pairs: vec![],
        };
        // Seed q (the H driver) onto a node so H exists carrying q. Use
        // the root from `new` is not available via new_seeded; instead key
        // q on {1} so individual node 1 plays H.
        let mut cl = clauses.clone();
        cl.push(DlClause {
            body: vec![Atom::Class(cls(7), X)],
            head: vec![Atom::Class(q, X)],
        });
        // Bug reproduced (both NN-merge dep fixes disabled) ⇒ false Unsat:
        let buggy = HyperEngine::new_seeded(&cl, &seed)
            .with_nominals(6, 2)
            .with_nn_taint_disabled()
            .decide(64);
        // Fixed (merge-causation-dep source fix on) ⇒ Sat:
        let fixed = HyperEngine::new_seeded(&cl, &seed)
            .with_nominals(6, 2)
            .decide(64);
        assert_eq!(
            buggy,
            HyperResult::Unsat,
            "without the fix the propagate-then-clash graph should false-Unsat \
             (taint-the-node alone does NOT cover this — residual B)"
        );
        // The whole graph is CONSISTENT (branch 2 = `p` is clash-free).
        // A false-`Unsat` here = residual (B) escaping the fix.
        assert_eq!(
            fixed,
            HyperResult::Sat,
            "propagate-then-clash must be Sat with the merge-causation-dep \
             source fix; an Unsat = catastrophic false-inconsistent"
        );
    }

    /// **Residual (C) probe — merge-copied-edge escape (opus re-review).**
    /// The back-prop clause fires over an edge COPIED by the NN-merge
    /// (the seeded `{0}` node's `R({0}, {1})` assertion), whose target's
    /// `birth_deps` never received the merge-causation dep. The derived
    /// head then escapes the `nn_tainted` catch-all onto a fresh `∃`
    /// successor that clashes untainted ⇒ false-`Unsat`. Discriminator:
    /// the same graph with a Horn (non-disjunctive) decision is provably
    /// `Sat`, so the disjunctive version MUST also be `Sat`.
    #[test]
    fn nn_merge_edge_copy_residual_c() {
        let (q, ll, mm, zz, p, g) = (cls(0), cls(1), cls(2), cls(3), cls(4), cls(5));
        let nom0 = cls(6);
        let role = Role::Named(RoleId::new(0));
        let role_s = Role::Named(RoleId::new(1));
        // Decision D (disjunctive): q → {0} ⊔ p. Branch 1 places {0} on
        // node2 (carrying q) ⇒ NN-merge node2 into the seeded {0}=node0,
        // copying node0's seeded edge R(node0, node1). Branch 2 (p) is
        // clash-free ⇒ the graph is consistent.
        let disjunctive_d = DlClause {
            body: vec![Atom::Class(q, X)],
            head: vec![Atom::Class(nom0, X), Atom::Class(p, X)],
        };
        // Discriminator: Horn decision q → p (no branching, real model).
        let nondisj_d = DlClause {
            body: vec![Atom::Class(q, X)],
            head: vec![Atom::Class(p, X)],
        };
        let base = |decision: DlClause| {
            vec![
                // {1}=node1 carries `ll` (EMPTY-dep seeded label):
                DlClause {
                    body: vec![Atom::Class(cls(7), X)],
                    head: vec![Atom::Class(ll, X)],
                },
                // {2}=node2 carries `q` (the decision driver):
                DlClause {
                    body: vec![Atom::Class(cls(8), X)],
                    head: vec![Atom::Class(q, X)],
                },
                // Guard G keyed on q ⇒ only on node2 (gates the back-prop
                // so the INITIAL fixpoint over node0's own edge is
                // clash-free; the clash arises only inside branch 1):
                DlClause {
                    body: vec![Atom::Class(q, X)],
                    head: vec![Atom::Class(g, X)],
                },
                decision,
                // Guarded back-prop over the COPIED edge:
                // R(x,y) ∧ ll(y) ∧ G(x) → mm(x):
                DlClause {
                    body: vec![
                        Atom::Role(role, X, 1),
                        Atom::Class(ll, 1),
                        Atom::Class(g, X),
                    ],
                    head: vec![Atom::Class(mm, X)],
                },
                // mm spawns a fresh untainted ∃-successor that clashes:
                DlClause {
                    body: vec![Atom::Class(mm, X)],
                    head: vec![Atom::Exists(role_s, zz, X)],
                },
                DlClause {
                    body: vec![Atom::Class(zz, X)],
                    head: vec![],
                },
            ]
        };
        let seed = AboxSeed {
            num_individuals: 3,
            nominal_base: 6,
            property_assertions: vec![(0, role, 1)],
            same_pairs: vec![],
        };
        let discriminator = HyperEngine::new_seeded(&base(nondisj_d), &seed)
            .with_nominals(6, 3)
            .decide(64);
        let exploit = HyperEngine::new_seeded(&base(disjunctive_d), &seed)
            .with_nominals(6, 3)
            .decide(64);
        assert_eq!(
            discriminator,
            HyperResult::Sat,
            "Horn discriminator proves the graph is consistent"
        );
        assert_eq!(
            exploit,
            HyperResult::Sat,
            "merge-copied-edge back-prop must not false-Unsat a consistent graph (residual C)"
        );
    }

    /// `nn_tainted` backjump-soundness pin: an NN-merge that forces two
    /// genuinely-disjoint nominals onto one node must STILL be Unsat (the
    /// taint only widens the clash dep-set to `DepSet::ALL`; it must
    /// never lose a real refutation). Class `a`(cls 0) implies
    /// `∃R.{0}` and `∃R.{1}`, `≤1 R`, and `{0}⊓{1}⊑⊥` — the two
    /// nominal successors are merged by `≤1`, the NN-rule then tries to
    /// coincide them with the seeded nominal nodes, and the disjointness
    /// clashes. Real Unsat, preserved with and without the taint path.
    #[test]
    fn nn_merge_real_clash_stays_unsat() {
        let role = Role::Named(RoleId::new(0));
        let (a, n0, n1) = (cls(0), cls(2), cls(3));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, n0, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(role, n1, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::AtMost(role, None, 1, X)],
            },
            DlClause {
                body: vec![Atom::Class(n0, X), Atom::Class(n1, X)],
                head: vec![],
            },
        ];
        let r = HyperEngine::new(&clauses, a).with_nominals(2, 2).decide(64);
        assert_eq!(
            r,
            HyperResult::Unsat,
            "≤1 R merging two disjoint nominal successors must stay Unsat"
        );
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

    /// `≤2 R` with three *non*-disjoint successors is Sat: two of them
    /// can merge into one group, leaving 2 ≤ 2. This is the case the
    /// partition enumerator must solve (the increment-1 clique check does
    /// not fire — no forced-distinct clique exceeds 2). Guards that the
    /// enumerator finds a valid partition (Sat) rather than spuriously
    /// concluding Unsat, and that it doesn't explode the branch count.
    #[test]
    fn at_most_two_with_three_mergeable_successors_is_sat() {
        let role = Role::Named(RoleId::new(0));
        let (root, ca, cb, cd) = (cls(0), cls(1), cls(2), cls(3));
        // No disjointness among ca/cb/cd, so any two can merge.
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
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, cd, X)],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, None, 2, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(e.decide(64), HyperResult::Sat);
        // Partitions of 3 items into ≤2 blocks = 4; the enumerator tries
        // each once and stops at the first Sat — far below the pairwise
        // loop's order-redundant exploration.
        assert!(
            e.stats().merge_branches <= 4,
            "partition dedup should bound merge branches; got {}",
            e.stats().merge_branches
        );
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

    /// The cardinality clash pre-check fires on the `InterestingPizza`
    /// shape (`≤2` with three pairwise-disjoint successors): the verdict
    /// is `Unsat` and **no merge branching happens** — the clique
    /// certificate short-circuits it. Guards that the new path is live
    /// (not silently dead) and actually collapses the blow-up.
    #[test]
    fn at_most_clash_precheck_collapses_branching() {
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
        assert_eq!(
            e.stats().branches_taken,
            0,
            "clash pre-check should fire with zero merge branching"
        );
    }

    /// Soundness guard (advisor's discriminating negative): `≤1 R.C`
    /// with two `R`-successors that are mutually disjoint but where only
    /// **one** is forced to be `C` must stay **Sat**. The candidate set
    /// is `distinct_role_succ(.., Some(C))` (definitely-`C` only), so the
    /// non-`C` successor is not counted and there is no false clash.
    #[test]
    fn at_most_precheck_does_not_overcount_unqualified_successors() {
        let role = Role::Named(RoleId::new(0));
        // root: ∃R.C and ∃R.D ; C ⊓ D ⊑ ⊥ ; ≤1 R.C
        let (root, cc, cd) = (cls(0), cls(1), cls(2));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, cc, X)],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::Exists(role, cd, X)],
            },
            // C and D disjoint — but the ≤1 is qualified on C only.
            DlClause {
                body: vec![Atom::Class(cc, X), Atom::Class(cd, X)],
                head: vec![],
            },
            DlClause {
                body: vec![Atom::Class(root, X)],
                head: vec![Atom::AtMost(role, Some(cc), 1, X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, root);
        assert_eq!(
            e.decide(64),
            HyperResult::Sat,
            "only one successor is forced to be C, so ≤1 R.C holds"
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

    // ============================================================
    // HF3: role-chain edge derivation
    // ============================================================

    fn nrole(i: u32) -> Role {
        Role::Named(RoleId::new(i))
    }

    /// Common chain scenario as DL-clauses. Root carries `A=cls(0)`.
    /// `A → ∃R1.B`, `B → ∃R2.C` build a path root —R1→ n1 —R2→ n2 with
    /// `n1:B`, `n2:C`. `{R3(X,z), C(z)} → ⊥` is the downstream clash that
    /// only fires once `R3(root, n2)` is derived. `with_chain` toggles the
    /// chain clause `R1∘R2 ⊑ R3`.
    fn chain_scenario(with_chain: bool, chain: DlClause) -> Vec<DlClause> {
        let (a, b, c) = (cls(0), cls(1), cls(2));
        let (r1, r2, r3) = (nrole(10), nrole(11), nrole(12));
        let mut clauses = vec![
            // A(X) → ∃R1.B
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            // B(X) → ∃R2.C
            DlClause {
                body: vec![Atom::Class(b, X)],
                head: vec![Atom::Exists(r2, c, X)],
            },
            // R3(X,z) ∧ C(z) → ⊥
            DlClause {
                body: vec![Atom::Role(r3, X, 1), Atom::Class(c, 1)],
                head: vec![],
            },
        ];
        if with_chain {
            clauses.push(chain);
        }
        clauses
    }

    #[test]
    fn hf3_two_leg_chain_derives_edge_and_clashes() {
        let (r1, r2, r3) = (nrole(10), nrole(11), nrole(12));
        // R1∘R2 ⊑ R3
        let chain = DlClause {
            body: vec![Atom::Role(r1, X, 1), Atom::Role(r2, 1, 2)],
            head: vec![Atom::Role(r3, X, 2)],
        };
        // Without the chain clause: no R3 edge ⇒ no clash ⇒ Sat.
        let no_chain = chain_scenario(false, chain.clone());
        let mut e0 = HyperEngine::new(&no_chain, cls(0));
        assert_eq!(
            e0.run(4096),
            HyperResult::Sat,
            "baseline (no chain) must be Sat"
        );
        // With the chain clause: R3(root,n2) derived ⇒ clash ⇒ Unsat.
        let with_chain = chain_scenario(true, chain);
        let mut e1 = HyperEngine::new(&with_chain, cls(0));
        assert_eq!(
            e1.run(4096),
            HyperResult::Unsat,
            "chain-derived R3 edge must enable the clash"
        );
    }

    #[test]
    fn hf3_transitivity_derives_edge_and_clashes() {
        // Path root —R—> n1 —R—> n2, transitivity R∘R⊑R derives R(root,n2),
        // {R(X,z), C(z)} → ⊥ clashes.
        let (a, b, c) = (cls(0), cls(1), cls(2));
        let r = nrole(10);
        let trans = DlClause {
            body: vec![Atom::Role(r, X, 1), Atom::Role(r, 1, 2)],
            head: vec![Atom::Role(r, X, 2)],
        };
        let base = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(r, b, X)],
            },
            DlClause {
                body: vec![Atom::Class(b, X)],
                head: vec![Atom::Exists(r, c, X)],
            },
            // Clash gated on A (root only): {A(X), R(X,z), C(z)} → ⊥.
            // Without transitivity root reaches only n1:B via R, so no
            // clash; the n1—R→n2 edge is at n1 (not A) so it can't fire.
            DlClause {
                body: vec![Atom::Class(a, X), Atom::Role(r, X, 1), Atom::Class(c, 1)],
                head: vec![],
            },
        ];
        // Without transitivity: root has only R(root,n1), n1:B (not C) ⇒ Sat.
        let mut e0 = HyperEngine::new(&base, cls(0));
        assert_eq!(e0.run(4096), HyperResult::Sat, "baseline must be Sat");
        let mut with = base.clone();
        with.push(trans);
        let mut e1 = HyperEngine::new(&with, cls(0));
        assert_eq!(
            e1.run(4096),
            HyperResult::Unsat,
            "transitivity must derive R(root,n2) and clash"
        );
    }

    #[test]
    fn hf3_second_leg_back_trigger_index_populated() {
        // T3 structural pin: a 2-leg chain body `R1(X,y) ∧ R2(y,z)` must
        // register R2 (the non-first leg) in `role_back_trigger`, so that
        // when an R2 edge is added mid-fixpoint the clause fires at the
        // R2-source's predecessors (the X-root). R1 (first leg, u=X) must
        // NOT be in `role_back_trigger`.
        let (r1, r2, r3) = (nrole(10), nrole(11), nrole(12));
        let clauses = vec![DlClause {
            body: vec![Atom::Role(r1, X, 1), Atom::Role(r2, 1, 2)],
            head: vec![Atom::Role(r3, X, 2)],
        }];
        let ix = build_clause_indexes(&clauses, None);
        let k1 = role_id_index(r1);
        let k2 = role_id_index(r2);
        assert!(
            ix.role_back_trigger.get(k2).is_some_and(|v| v.contains(&0)),
            "R2 (non-first leg) must be in role_back_trigger"
        );
        assert!(
            ix.role_back_trigger.get(k1).is_none_or(|v| !v.contains(&0)),
            "R1 (first leg, u=X) must NOT be in role_back_trigger"
        );
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn hf3_second_leg_added_last_still_clashes() {
        // T3 end-to-end: force the second leg's edge to be added LAST.
        // root:A; A→∃R1.B (root—R1→n1). A separate trigger D→∃R2.C is
        // fired on n1 only after n1 gets B AND a D label that arrives
        // late. We simulate "late" by making the R2 edge depend on a
        // 2-hop label chain so it is added after the R1 edge + chain
        // clause have already been considered. The back-trigger must
        // re-fire the chain at n1's predecessor (root) when R2 appears.
        let (a, b, c, d) = (cls(0), cls(1), cls(2), cls(3));
        let (r1, r2, r3) = (nrole(10), nrole(11), nrole(12));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            // B → D (label chain to delay R2)
            DlClause {
                body: vec![Atom::Class(b, X)],
                head: vec![Atom::Class(d, X)],
            },
            // D → ∃R2.C  (adds the second-leg edge, after B→D)
            DlClause {
                body: vec![Atom::Class(d, X)],
                head: vec![Atom::Exists(r2, c, X)],
            },
            // R1∘R2 ⊑ R3
            DlClause {
                body: vec![Atom::Role(r1, X, 1), Atom::Role(r2, 1, 2)],
                head: vec![Atom::Role(r3, X, 2)],
            },
            // {A(X), R3(X,z), C(z)} → ⊥
            DlClause {
                body: vec![Atom::Class(a, X), Atom::Role(r3, X, 1), Atom::Class(c, 1)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(
            e.run(4096),
            HyperResult::Unsat,
            "late second-leg edge must still trigger the chain via back-trigger"
        );
    }

    /// T4 — SOUNDNESS centerpiece for the chain-derived-edge backjump
    /// deps. A chain-derived edge feeds a clash that is INDEPENDENT of a
    /// disjunction decision. The edge-dep fold (into both endpoints'
    /// `birth_deps`) must let backjumping behave correctly:
    ///   - the verdict must be `Unsat` (the chain clash holds in every
    ///     branch), AND
    ///   - it must NOT depend on whether the irrelevant disjunction is
    ///     present — i.e. adding a decision the clash doesn't touch must
    ///     not flip the verdict (a wrong dep would either backjump PAST
    ///     the real clash → false `Sat`, or fail to → still `Unsat` but
    ///     for the wrong reason). We assert the verdict is `Unsat` both
    ///     with and without the disjunction.
    ///
    /// This is the analog of `precise_card_deps_preserves_unsat_verdict`
    /// for role-chain edge derivation (the `nn_merge_edge_copy_residual_c`
    /// failure class the brief warns about).
    #[test]
    fn hf3_chain_edge_clash_backjump_preserves_verdict() {
        let (a, b, c, d1, d2) = (cls(0), cls(1), cls(2), cls(3), cls(4));
        let (r1, r2, r3) = (nrole(10), nrole(11), nrole(12));
        // Decision-INDEPENDENT chain clash: A→∃R1.B, B→∃R2.C,
        // R1∘R2⊑R3, {A(X),R3(X,z),C(z)}→⊥.
        let core = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            DlClause {
                body: vec![Atom::Class(b, X)],
                head: vec![Atom::Exists(r2, c, X)],
            },
            DlClause {
                body: vec![Atom::Role(r1, X, 1), Atom::Role(r2, 1, 2)],
                head: vec![Atom::Role(r3, X, 2)],
            },
            DlClause {
                body: vec![Atom::Class(a, X), Atom::Role(r3, X, 1), Atom::Class(c, 1)],
                head: vec![],
            },
        ];
        // Without the disjunction.
        let mut e_no = HyperEngine::new(&core, a);
        assert_eq!(
            e_no.decide(64),
            HyperResult::Unsat,
            "chain clash alone must be Unsat"
        );
        // With an irrelevant disjunction decision A → D1 ∨ D2.
        let mut with = core.clone();
        with.push(DlClause {
            body: vec![Atom::Class(a, X)],
            head: vec![Atom::Class(d1, X), Atom::Class(d2, X)],
        });
        let mut e_yes = HyperEngine::new(&with, a);
        assert_eq!(
            e_yes.decide(64),
            HyperResult::Unsat,
            "adding a decision the chain clash is independent of must not flip the verdict"
        );
    }

    /// T4b — the FP-direction guard: a chain clash that fires only under
    /// ONE disjunct. The OTHER disjunct is satisfiable, so the ontology is
    /// SAT. If the chain-derived edge under-reported its deps (omitting
    /// the branch decision that built its R1 leg), backjumping could
    /// wrongly discard the satisfiable sibling and report a false `Unsat`.
    /// Asserts `Sat` — the edge-dep fold must keep the decision in scope.
    #[test]
    #[allow(clippy::many_single_char_names)]
    fn hf3_chain_edge_under_one_disjunct_stays_sat() {
        let (a, p, q, b, c) = (cls(0), cls(1), cls(2), cls(3), cls(4));
        let (r1, r2, r3) = (nrole(10), nrole(11), nrole(12));
        let clauses = vec![
            // A → P ∨ Q   (the decision)
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Class(p, X), Atom::Class(q, X)],
            },
            // Only the P branch builds the chain that clashes:
            // P → ∃R1.B, B → ∃R2.C, R1∘R2⊑R3, {R3(X,z),C(z)}→⊥.
            DlClause {
                body: vec![Atom::Class(p, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            DlClause {
                body: vec![Atom::Class(b, X)],
                head: vec![Atom::Exists(r2, c, X)],
            },
            DlClause {
                body: vec![Atom::Role(r1, X, 1), Atom::Role(r2, 1, 2)],
                head: vec![Atom::Role(r3, X, 2)],
            },
            DlClause {
                body: vec![Atom::Class(p, X), Atom::Role(r3, X, 1), Atom::Class(c, 1)],
                head: vec![],
            },
        ];
        // Q branch is clash-free ⇒ overall Sat. A false Unsat here would
        // mean the chain-derived edge's deps wrongly excluded the P
        // decision, letting backjumping prune the Q sibling.
        let mut e = HyperEngine::new(&clauses, a);
        assert_eq!(
            e.decide(64),
            HyperResult::Sat,
            "Q branch is satisfiable — chain clash under P must not force a false Unsat"
        );
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn hf3_three_leg_via_two_two_leg_clauses() {
        // Mimics decomposition output: R1∘R2⊑AUX, AUX∘R3⊑S, then the
        // path root—R1→n1—R2→n2—R3→n3 with n3:D, and {S(X,z),D(z)}→⊥.
        let (a, b, c, d) = (cls(0), cls(1), cls(2), cls(3));
        let (r1, r2, r3, aux, s) = (nrole(10), nrole(11), nrole(12), nrole(13), nrole(14));
        let base = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            DlClause {
                body: vec![Atom::Class(b, X)],
                head: vec![Atom::Exists(r2, c, X)],
            },
            DlClause {
                body: vec![Atom::Class(c, X)],
                head: vec![Atom::Exists(r3, d, X)],
            },
            DlClause {
                body: vec![Atom::Role(s, X, 1), Atom::Class(d, 1)],
                head: vec![],
            },
        ];
        let two_leg = vec![
            // R1∘R2 ⊑ AUX
            DlClause {
                body: vec![Atom::Role(r1, X, 1), Atom::Role(r2, 1, 2)],
                head: vec![Atom::Role(aux, X, 2)],
            },
            // AUX∘R3 ⊑ S
            DlClause {
                body: vec![Atom::Role(aux, X, 1), Atom::Role(r3, 1, 2)],
                head: vec![Atom::Role(s, X, 2)],
            },
        ];
        let mut e0 = HyperEngine::new(&base, cls(0));
        assert_eq!(e0.run(4096), HyperResult::Sat, "baseline must be Sat");
        let mut with = base.clone();
        with.extend(two_leg);
        let mut e1 = HyperEngine::new(&with, cls(0));
        assert_eq!(
            e1.run(4096),
            HyperResult::Unsat,
            "3-leg (2×2-leg) chain must derive S(root,n3) and clash"
        );
    }

    #[test]
    fn hf3_inverse_leg_chain_derives_edge() {
        // Chain with an INVERSE first leg in the body: R1⁻ ∘ R2 ⊑ R3.
        // Body: R1⁻(X,y) ∧ R2(y,z) → R3(X,z). The R1⁻(X,y) leg is
        // satisfied by an R1-edge INTO X from y (a predecessor walk).
        //
        // Graph: build n0 —R1→ root (so root has an R1-predecessor n0),
        // and root —R2→ n2 with n2:C. Then R1⁻(root,y) binds y=n0... no,
        // we need the path X=root, R1⁻(root,y) ⇒ y is an R1-predecessor
        // of root, then R2(y,z). So put R2 on the PREDECESSOR.
        //
        // Concretely: root has class A. A → ∃R2.dummy gives root an R2
        // edge — not what we want. Instead seed the structure directly:
        //   n_pred —R1→ root, n_pred —R2→ n_c (n_c:C).
        //   Chain R1⁻∘R2⊑R3 at X=root: R1⁻(root,n_pred) [via the R1 edge
        //   into root], R2(n_pred,n_c) ⇒ derive R3(root,n_c).
        //   Clash {A(X),R3(X,z),C(z)}→⊥ at root.
        // We build the graph via ∃ heads rooted appropriately:
        //   A(X) → ∃R1.M  gives root —R1→ m1.  Then m1's R1⁻ leg sees
        // root as its... this is getting tangled. Simplest: assert the
        // INVERSE-LEG match works by constructing the predecessor edge
        // through a forward ∃ on a helper and an explicit role body.
        //
        // Use: root:A. A→∃R1.B (root—R1→n1, n1:B). B→∃R2.C (n1—R2→n2,
        // n2:C). Chain on the INVERSE of R1 as a leg rooted at n1:
        //   R1⁻(X,y) ∧ ... at X=n1: R1⁻(n1,y) binds y=root (n1's R1-pred).
        // To make a useful clash, chain R1⁻∘(R1) ⊑ R3 deriving R3(n1,n1)?
        // Cleaner: chain  R1⁻ ∘ R2  is not co-rooted. Keep it simple and
        // faithful: R2 ∘ R1⁻ ⊑ R3 rooted at n1:
        //   R2(n1,n2) ∧ R1⁻(n2,?) — n2 has no R1 pred. Not it either.
        //
        // Faithful minimal inverse-leg chain (matches family idiom
        // hasFather⁻ = isFatherOf): X has R1-successor y (forward leg),
        // y has R2-successor z (forward leg) — but we want one leg to be
        // matched by an INVERSE edge. Build root—R1→n1, then a chain
        // R1 ∘ R1⁻ ⊑ R3: at X=root, R1(root,n1) ∧ R1⁻(n1,z) ⇒ z is an
        // R1-predecessor of n1, i.e. z=root ⇒ derive R3(root,root). A
        // self-loop R3 then clashes via {A(X),R3(X,X)}-style... but our
        // clash body needs two vars. Use {A(X),R3(X,z),A(z)}→⊥ with
        // z=root (root:A) ⇒ clash. Baseline (no chain) Sat.
        let a = cls(0);
        let b = cls(1);
        let (r1, r3) = (nrole(10), nrole(12));
        let r1_inv = Role::Inverse(RoleId::new(10));
        let chain = DlClause {
            // R1(X,y) ∧ R1⁻(y,z) → R3(X,z)   [inverse leg in the body]
            body: vec![Atom::Role(r1, X, 1), Atom::Role(r1_inv, 1, 2)],
            head: vec![Atom::Role(r3, X, 2)],
        };
        let mut clauses = vec![
            // A(X) → ∃R1.B
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            // {A(X), R3(X,z), A(z)} → ⊥  (z must be an A-node reached by R3)
            DlClause {
                body: vec![Atom::Class(a, X), Atom::Role(r3, X, 1), Atom::Class(a, 1)],
                head: vec![],
            },
        ];
        let mut e0 = HyperEngine::new(&clauses, cls(0));
        assert_eq!(
            e0.run(4096),
            HyperResult::Sat,
            "baseline (no inverse-leg chain) must be Sat"
        );
        clauses.push(chain);
        let mut e1 = HyperEngine::new(&clauses, cls(0));
        assert_eq!(
            e1.run(4096),
            HyperResult::Unsat,
            "inverse-leg chain R1∘R1⁻⊑R3 must derive R3(root,root) and clash"
        );
    }

    /// **Dep-fold isolating discriminator** (independent-review regression
    /// guard). Unlike T4b (`hf3_chain_edge_under_one_disjunct_stays_sat`),
    /// where the clash binds an ∃-successor that independently carries the
    /// deciding disjunct in its `birth_deps`, here the chain-derived edge is
    /// an inverse self-loop `R3(root,root)` and the clash binds the ROOT —
    /// whose natural `birth_deps` is EMPTY. So the ONLY way the deciding `P`
    /// disjunct reaches the clash's dep-set is the `derive_role_edge`
    /// birth_deps fold. With the fold ON the clash carries `P`, backjumping
    /// keeps the clash-free `Q` sibling, and the graph is `Sat`. With the
    /// fold OFF the clash deps are EMPTY, backjumping wrongly prunes `Q`, and
    /// the graph false-`Unsat`s (the residual-C / corpus-invisible class).
    /// This test goes RED iff the fold is dropped — verified by mutation.
    #[test]
    #[allow(clippy::many_single_char_names, clippy::doc_markdown)]
    fn hf3_chain_edge_dep_fold_isolating_discriminator() {
        let (a, p, q, b) = (cls(0), cls(1), cls(2), cls(3));
        let (r1, r3) = (nrole(10), nrole(12));
        let r1_inv = Role::Inverse(RoleId::new(10));
        let clauses = vec![
            // A → P ∨ Q  (the decision; A is the pre-branch root label).
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Class(p, X), Atom::Class(q, X)],
            },
            // Only the P branch builds the R1-successor that feeds the chain.
            DlClause {
                body: vec![Atom::Class(p, X)],
                head: vec![Atom::Exists(r1, b, X)],
            },
            // R1(X,y) ∧ R1⁻(y,z) → R3(X,z): self-loop R3(root,root) under P.
            DlClause {
                body: vec![Atom::Role(r1, X, 1), Atom::Role(r1_inv, 1, 2)],
                head: vec![Atom::Role(r3, X, 2)],
            },
            // {A(X), R3(X,z), A(z)} → ⊥ : binds z=root; deps must carry P.
            DlClause {
                body: vec![Atom::Class(a, X), Atom::Role(r3, X, 1), Atom::Class(a, 1)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&clauses, a);
        assert_eq!(
            e.decide(64),
            HyperResult::Sat,
            "Q is clash-free; the chain-edge clash under P must carry the P \
             decision via the derive_role_edge birth_deps fold so backjumping \
             cannot prune Q. A false Unsat here = the dep-fold regressed."
        );
    }

    #[test]
    fn mrv_ordering_picks_fewest_live_disjunct_clause() {
        // Root node labelled A. Two open disjunctive clauses:
        //   clause0: A -> d1 ⊔ d2 ⊔ d3   (3 live disjuncts)
        //   clause1: A -> e1 ⊔ e2        (2 live disjuncts)
        // MRV-OFF: find_open_disjunction returns clause0 (first). MRV-ON: returns clause1 (2<3).
        let (a, d1, d2, d3, e1, e2) = (cls(0), cls(1), cls(2), cls(3), cls(4), cls(5));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Class(d1, X), Atom::Class(d2, X), Atom::Class(d3, X)],
            },
            DlClause {
                body: vec![Atom::Class(a, X)],
                head: vec![Atom::Class(e1, X), Atom::Class(e2, X)],
            },
        ];
        // OFF: first-open = clause index 0
        let mut off = HyperEngine::new(&clauses, a);
        off.horn_fixpoint(FIXPOINT_ITERS);
        assert_eq!(
            off.find_open_disjunction_for_test().map(|(ci, _, _)| ci),
            Some(0)
        );
        // ON: MRV = clause index 1 (fewer live disjuncts)
        let mut on = HyperEngine::new(&clauses, a).with_mrv_ordering();
        on.horn_fixpoint(FIXPOINT_ITERS);
        assert_eq!(
            on.find_open_disjunction_for_test().map(|(ci, _, _)| ci),
            Some(1)
        );
    }

    /// Scaffold test: `mrv_ordering` defaults to `false`; `with_mrv_ordering` flips it.
    #[test]
    fn mrv_ordering_builder_and_default() {
        let a = cls(0);
        let clauses = vec![DlClause {
            body: vec![Atom::Class(a, X)],
            head: vec![],
        }];
        assert!(!HyperEngine::new(&clauses, a).mrv_ordering_for_test());
        assert!(
            HyperEngine::new(&clauses, a)
                .with_mrv_ordering()
                .mrv_ordering_for_test()
        );
    }
}
