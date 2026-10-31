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
pub mod replay;
mod rules;
mod saturate;
mod search;
pub mod shadow_measures;
pub mod snapshot;
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
pub use replay::{ReplayVerdict, replay_with_neg_sup, replay_with_neg_sup_full_rerun};
pub use rules::{
    RuleOutcome, apply_and, apply_concept_rules, apply_deferred_concept_or_rules,
    apply_deferred_or_residuals, apply_exists, apply_forall, apply_max, apply_min,
    apply_nominal_assignment, apply_nominal_rules, apply_residual_gcis, apply_role_axioms,
    apply_role_chains, apply_role_rules, apply_self_restriction,
};
pub use saturate::{SaturationResult, saturate, verify_node_local_clash};
pub use search::{SearchVerdict, search};
pub use snapshot::{BackPropRisk, GraphSnapshot, SnapshotNodeId, UnsafeReason};
pub use trail::{Checkpoint, TableauTrail, TrailEntry};

use std::collections::HashMap;

use hashbrown::HashSet;
use owl_dl_core::{
    AbsorbedTBox, ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, Role, RoleHierarchy,
    RoleId, is_nnf,
};

/// Where `saturate` is, for incremental saturation's bookkeeping (#288).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanPhase {
    /// Between `saturate` calls.
    Idle,
    /// The first scan of a call, at node `pos`, ending at `end`. A full pass
    /// still visits every node in between, so a mutation next to one of them
    /// dirties it at once.
    FirstScan { pos: usize, end: usize },
    /// Inside a call, after its first scan.
    Later,
}

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
// The bools are per-context caches of independent env flags (read once,
// because the rules that consult them are on the hot path), not a state
// machine; grouping them would only move the count.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct TableauContext<'pool, 'tbox, 'hier> {
    pool: &'pool ConceptPool,
    tbox: Option<&'tbox AbsorbedTBox>,
    hierarchy: Option<&'hier RoleHierarchy>,
    /// Cached [`signed_role_edges_enabled`] (#211), read once per context
    /// because [`Self::edge_satisfies`] is on the hot path.
    signed_roles: bool,
    /// Cached [`chain_middle_enabled`] (#213), read once per context.
    chain_middle: bool,
    /// Cached [`forall_first_enabled`] (#288), read once per context because
    /// disjunct ordering runs at every branch.
    forall_first: bool,
    /// Cached [`tree_sticky_branching_enabled`] (#377), read once per context
    /// because the decision selectors run at every expansion.
    tree_sticky: bool,
    /// Cached [`max_first_enabled`] (#382).
    max_first: bool,
    /// Cached [`top_filler_enabled`] (#382).
    top_filler: bool,
    /// The tree root of the last decision the search opened (#377), `u32::MAX`
    /// before the first. With [`Self::tree_sticky_branching`], the selectors
    /// prefer an open decision in this tree.
    last_decision_root: u32,
    /// Last [`Self::is_blocked`] answer as `(node, trail generation, blocked)`
    /// (#288). Six rules ask for the same node in a row, usually with no graph
    /// change in between, and anywhere-blocking scans every candidate each time.
    /// Keyed on the trail generation, which changes on every graph mutation and
    /// rollback, so a hit is exactly the value a fresh call would return.
    blocked_memo: std::cell::Cell<(NodeId, u64, bool)>,
    /// Cached [`incremental_saturate_enabled`] (#288).
    incremental_saturate: bool,
    /// Nodes a full pass would visit again on the next `saturate` call and an
    /// incremental one must therefore dirty (#288), deduplicated by
    /// `deferred_seen`: nodes a rule skipped as blocked, and nodes next to a
    /// mutation that the current scan had already passed. Each `StableMark`
    /// stores a copy.
    deferred: Vec<NodeId>,
    deferred_seen: Vec<bool>,
    /// Where `saturate` is (#288); see [`ScanPhase`].
    scan_phase: ScanPhase,
    /// Declared inverse pairs: `(r, s)` means an `InverseObjectProperties(r, s)`
    /// axiom in the source ontology. Stored symmetrically (both `(r, s)` and
    /// `(s, r)` are pushed) to match `inverse_pairs_set`'s contents. Retained
    /// for declaration-order iteration; lookups go through `inverse_pairs_set`
    /// (Phase 3b — the pizza-era "0-3 pairs" linear-scan assumption was
    /// empirically wrong on SIO which has 84 inverse-pair declarations →
    /// ~168 Vec entries; see `docs/phase3b-fix-target.md`).
    inverse_pairs: Vec<(RoleId, RoleId)>,
    /// Phase 3b O(1) lookup mirror for `are_declared_inverses`. Parallel to
    /// `inverse_pairs`; populated by `declare_inverse_pair` (mirror write) and
    /// in every constructor that initialises `inverse_pairs`. Used by the hot
    /// path in `apply_max` (SIO flamegraph attributed 25.76% to the previous
    /// `Vec::iter().any()` linear scan). See `docs/phase3b-fix-target.md`.
    /// (Mirror invariant: `declare_inverse_pair` is the only writer of
    /// `inverse_pairs`; mirror writes here keep the two structures in
    /// sync. Add the mirror writes if a new writer ever lands.)
    inverse_pairs_set: HashSet<(RoleId, RoleId)>,
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
    /// a search frame (`crate::search`) pushes a fresh id when it enters a
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
    /// unsat under the current context, so a search frame (`crate::search`)
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
    /// Concrete-domain solver (P3): `ClassId → CardRange` for the synthetic
    /// `DKey` filler classes, supplied by the reasoner (which has the
    /// vocabulary the tableau lacks). Empty ⇒ the concrete-domain clash is
    /// inert. Read by `concrete_domain_clash` (an additive, refute-only clash
    /// over a node's data `∃/∀/≥n/≤n` labels) and by the `apply_min`/`apply_max`
    /// suppression guard (object-cardinality expansion is skipped for these
    /// fillers — the counting is done by `card_sat`, and expanding a large
    /// `≥n` over a `DKey` would not terminate).
    dkey_ranges: HashMap<ClassId, owl_dl_datatypes::CardRange>,
    /// #327: the `TBox` role rules whose target is a `DKey`, i.e. the told data `∀`s
    /// that absorption moved off the node label: unguarded ones (`⊤ ⊑ ∀p.D`) by
    /// role, guarded ones (`G ⊑ ∀p.D`) by guard. Built by [`Self::set_dkey_ranges`]
    /// so `concrete_domain_clash` never scans the full rule list.
    dkey_unguarded_rules: HashMap<RoleId, Vec<ConceptId>>,
    dkey_guarded_rules: HashMap<ClassId, Vec<(RoleId, ConceptId)>>,
    /// Anywhere (pairwise) blocking toggle. When `true`, [`Self::is_blocked`]
    /// scopes the pair-blocking candidate `x'` to ANY earlier-created
    /// non-excluded node instead of only tree-ancestors of `y`. Cached once at
    /// construction from `RUSTDL_ANYWHERE_BLOCKING` (default `false`) so the
    /// hot `is_blocked` path pays no per-call env read. See
    /// `docs/superpowers/plans/2026-06-15-anywhere-pairwise-blocking.md`.
    anywhere_blocking: bool,
    /// `RUSTDL_NOMINAL_BLOCK_GUARD`, cached (#361/#362).
    nominal_block_guard: bool,
    /// `RUSTDL_MERGE_EXACT_DEPS`, cached (#370).
    merge_exact_deps: bool,
    /// Memoised transitive told-subsumer closure over **atomic** classes:
    /// `trigger ClassId → sorted atomic-super ConceptIds` reachable through
    /// chains of `ConceptRule`s whose conclusion is itself `Atomic(_)`. Built
    /// lazily once from [`Self::tbox`]. Used by [`apply_min`](crate::rules::apply_min)
    /// so that a `≥n R.C` count recognises an existing `R`-neighbour carrying a
    /// told *sub*class of `C` as a witness — preventing a redundant successor
    /// that would otherwise make a `≤m R.D` (with `C ⊑ D`) spuriously clash
    /// (inverse-role + qualified-cardinality soundness fix).
    told_super_closure: std::cell::OnceCell<HashMap<owl_dl_core::ClassId, Vec<ConceptId>>>,
    /// Per-rule call counters, populated under `cfg(feature =
    /// "counters")`. Dumped to stderr in `Drop` when
    /// `RUSTDL_COUNTERS=1`. Zero cost in non-counter builds (field
    /// is omitted from the struct).
    #[cfg(feature = "counters")]
    counters: crate::counters::RuleCounters,
    /// Adaptive early-abandon accounting for the **main tableau** search
    /// (`RUSTDL_TABLEAU_EARLY_ABANDON`, default ON since 0.4.14 — the reasoner facade owns
    /// the flag and calls [`Self::enable_early_abandon`]). `None` ⇒ every hook is
    /// one `Option` discriminant test and the search behaves exactly as before,
    /// so the flag-OFF path is byte-identical by construction.
    early_abandon: Option<EarlyAbandon>,
}

/// Per-probe accounting behind the main tableau's adaptive early-abandon.
///
/// **What it is for.** `docs/2026-08-03-constant-audit.md` §4 measured
/// `MAX_SEARCH_DEPTH = 256` binding on 27 of the 33 ORE ontologies that reach the
/// main tableau, with **zero** headroom on every one, and a *fixed lower* cap
/// recovering three DNFs while making two completers ~14× faster with
/// byte-identical answers. `docs/2026-08-03-tableau-iterative-deepening.md` then
/// established, from the classify banner, that the whole 14× is time spent on
/// probes that reach **no verdict at either depth** — the saving is *giving up
/// sooner*, which is why iterative deepening (verdict-monotone by construction)
/// could not capture any of it. A fixed lower cap is the wrong instrument too:
/// `ore_ont_3281` is made two orders of magnitude worse by it (10.3 M `search`
/// entries), so no single constant is right.
///
/// **The criterion: total depth-cap bottom-outs — `depth0 >= cap_hit_limit`.**
/// A bottom-out is the direct evidence that the search is below what *this* cap
/// can close: the frame receiving a `DepthLimit` child cannot return
/// `Unsat(combined)` and must itself report `DepthLimit`, unless some *sibling*
/// yields a `Sat` or a back-jumping `Unsat`. Accumulating `cap_hit_limit` of them
/// says the probe is grinding through a subtree it cannot close at this cap.
///
/// Why that separates *"will not conclude"* from *"needs more depth"*: more depth
/// is measured **not** to be the missing ingredient on this population. The audit
/// read `ore_ont_10019`'s genuine depth requirement off two independent arms (459
/// at cap 512, 460 at cap 2048) and it **still DNFs** with the cap effectively
/// removed; and iterative deepening — the instrument that hands a probe more
/// depth — was built and measured a null on all five of the audit's cases
/// (`docs/2026-08-03-tableau-iterative-deepening.md` §5). A probe that has
/// bottomed out `cap_hit_limit` times is in the regime this cap cannot serve, and
/// raising the cap has been refuted as the answer.
///
/// **A "no progress" shape was tried FIRST and is refuted by its own calibration
/// data** — recorded here because it is the shape the wedge's `is_diverging`
/// suggests and the obvious thing to reach for again. Abandoning on a *run* of cap
/// hits reset by any definite child verdict never fires on the very ontologies the
/// audit names: `ore_ont_2826`'s doomed probes interleave 229 definite verdicts
/// into 1 074 trials with a longest run of **2**, and `ore_ont_13545`'s interleave
/// 242 into 2 570 with a longest run of **32**. These probes look locally
/// productive throughout and still reach no verdict, so "consecutive" measures the
/// wrong statistic here — the same conclusion the wedge write-up reached about a
/// consecutive-miss counter, re-established on this engine.
///
/// **Soundness.** Abandoning yields [`crate::SearchVerdict::DepthLimit`], which
/// the reasoner maps to `Ok(None)`. Exactly as for a depth-cap `DepthLimit` and
/// for `NodeCap`, a cap-driven non-verdict can only **suppress** an `Unsat` and
/// can never manufacture one, so **FP=0 is untouched in both directions**; the
/// exposure is purely completeness (a MISS), which is what the corpus MISSED net
/// measures.
#[derive(Debug, Clone)]
struct EarlyAbandon {
    /// Abandon once `depth0` reaches this. `0` disables the cut while leaving the
    /// accounting — and therefore the telemetry — live; that is the arm the
    /// constant was calibrated on.
    cap_hit_limit: u64,
    /// Depth-cap entries since the last definite verdict. **Telemetry only** —
    /// the refuted "no progress" statistic, kept observable so the refutation
    /// stays checkable rather than becoming folklore.
    stall_run: u64,
    /// High-water mark of `stall_run` over the probe (telemetry).
    max_stall_run: u64,
    /// Total depth-cap (`max_depth == 0`) entries over the probe — the criterion.
    depth0: u64,
    /// Branch-option trials over the probe (telemetry).
    trials: u64,
    /// Trials that returned a definite (`Unsat`/`Sat`) verdict (telemetry).
    definite: u64,
    /// Latched once the criterion fires, so the cut is self-sustaining and the
    /// whole DFS unwinds instead of re-deciding at every frame.
    abandoned: bool,
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
            signed_roles: signed_role_edges_enabled(),
            chain_middle: chain_middle_enabled(),
            forall_first: forall_first_enabled(),
            tree_sticky: tree_sticky_branching_enabled(),
            max_first: max_first_enabled(),
            top_filler: top_filler_enabled(),
            last_decision_root: u32::MAX,
            blocked_memo: std::cell::Cell::new((NodeId::new(0), u64::MAX, false)),
            incremental_saturate: incremental_saturate_enabled(),
            deferred: Vec::new(),
            deferred_seen: Vec::new(),
            scan_phase: ScanPhase::Idle,
            inverse_pairs: Vec::new(),
            inverse_pairs_set: HashSet::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
            asymmetric_roles: Vec::new(),
            disjoint_role_pairs: Vec::new(),
            graph: CompletionGraph::with_block_index(anywhere_blocking_enabled()),
            trail: TableauTrail::new(),
            deadline: None,
            deadline_hit: false,
            active_branches: Vec::new(),
            next_branch_id: 0,
            learned_nogoods: Vec::new(),
            decision_labels: Vec::new(),
            dkey_ranges: HashMap::new(),
            dkey_unguarded_rules: HashMap::new(),
            dkey_guarded_rules: HashMap::new(),
            anywhere_blocking: anywhere_blocking_enabled(),
            nominal_block_guard: nominal_block_guard_enabled(),
            merge_exact_deps: merge_exact_deps_enabled(),
            told_super_closure: std::cell::OnceCell::new(),
            #[cfg(feature = "counters")]
            counters: crate::counters::RuleCounters::default(),
            early_abandon: None,
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
            signed_roles: signed_role_edges_enabled(),
            chain_middle: chain_middle_enabled(),
            forall_first: forall_first_enabled(),
            tree_sticky: tree_sticky_branching_enabled(),
            max_first: max_first_enabled(),
            top_filler: top_filler_enabled(),
            last_decision_root: u32::MAX,
            blocked_memo: std::cell::Cell::new((NodeId::new(0), u64::MAX, false)),
            incremental_saturate: incremental_saturate_enabled(),
            deferred: Vec::new(),
            deferred_seen: Vec::new(),
            scan_phase: ScanPhase::Idle,
            inverse_pairs: Vec::new(),
            inverse_pairs_set: HashSet::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
            asymmetric_roles: Vec::new(),
            disjoint_role_pairs: Vec::new(),
            graph: CompletionGraph::with_block_index(anywhere_blocking_enabled()),
            trail: TableauTrail::new(),
            deadline: None,
            deadline_hit: false,
            active_branches: Vec::new(),
            next_branch_id: 0,
            learned_nogoods: Vec::new(),
            decision_labels: Vec::new(),
            dkey_ranges: HashMap::new(),
            dkey_unguarded_rules: HashMap::new(),
            dkey_guarded_rules: HashMap::new(),
            anywhere_blocking: anywhere_blocking_enabled(),
            nominal_block_guard: nominal_block_guard_enabled(),
            merge_exact_deps: merge_exact_deps_enabled(),
            told_super_closure: std::cell::OnceCell::new(),
            #[cfg(feature = "counters")]
            counters: crate::counters::RuleCounters::default(),
            early_abandon: None,
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
            signed_roles: signed_role_edges_enabled(),
            chain_middle: chain_middle_enabled(),
            forall_first: forall_first_enabled(),
            tree_sticky: tree_sticky_branching_enabled(),
            max_first: max_first_enabled(),
            top_filler: top_filler_enabled(),
            last_decision_root: u32::MAX,
            blocked_memo: std::cell::Cell::new((NodeId::new(0), u64::MAX, false)),
            incremental_saturate: incremental_saturate_enabled(),
            deferred: Vec::new(),
            deferred_seen: Vec::new(),
            scan_phase: ScanPhase::Idle,
            inverse_pairs: Vec::new(),
            inverse_pairs_set: HashSet::new(),
            complements: HashMap::new(),
            chains: Vec::new(),
            asymmetric_roles: Vec::new(),
            disjoint_role_pairs: Vec::new(),
            graph: CompletionGraph::with_block_index(anywhere_blocking_enabled()),
            trail: TableauTrail::new(),
            deadline: None,
            deadline_hit: false,
            active_branches: Vec::new(),
            next_branch_id: 0,
            learned_nogoods: Vec::new(),
            decision_labels: Vec::new(),
            dkey_ranges: HashMap::new(),
            dkey_unguarded_rules: HashMap::new(),
            dkey_guarded_rules: HashMap::new(),
            anywhere_blocking: anywhere_blocking_enabled(),
            nominal_block_guard: nominal_block_guard_enabled(),
            merge_exact_deps: merge_exact_deps_enabled(),
            told_super_closure: std::cell::OnceCell::new(),
            #[cfg(feature = "counters")]
            counters: crate::counters::RuleCounters::default(),
            early_abandon: None,
        }
    }

    /// Supply the concrete-domain solver's `ClassId → CardRange` side-map
    /// (built by the reasoner where the vocabulary is available). Enables the
    /// additive `concrete_domain_clash` check + the `apply_min`/`apply_max`
    /// suppression guard for `DKey` fillers. Empty (the default) ⇒ inert.
    pub fn set_dkey_ranges(
        &mut self,
        ranges: HashMap<ClassId, owl_dl_datatypes::CardRange>,
    ) -> &mut Self {
        self.dkey_ranges = ranges;
        self.dkey_unguarded_rules.clear();
        self.dkey_guarded_rules.clear();
        if let Some(tbox) = self.tbox {
            for rule in &tbox.role_rules {
                let Role::Named(rid) = rule.role else {
                    continue;
                };
                if self.dkey_range_of(rule.target_label).is_none() {
                    continue;
                }
                match rule.guard {
                    None => self
                        .dkey_unguarded_rules
                        .entry(rid)
                        .or_default()
                        .push(rule.target_label),
                    Some(g) => self
                        .dkey_guarded_rules
                        .entry(g)
                        .or_default()
                        .push((rid, rule.target_label)),
                }
            }
        }
        self
    }

    /// The decoded range of a filler concept iff it is a synthetic `DKey`
    /// class in the side-map (i.e. an atomic class whose `ClassId` was
    /// registered as a data range). `None` for ordinary concepts — used to
    /// distinguish data restrictions from object restrictions in the tableau,
    /// which has no IRIs.
    pub(crate) fn dkey_range_of(&self, filler: ConceptId) -> Option<&owl_dl_datatypes::CardRange> {
        if self.dkey_ranges.is_empty() {
            return None;
        }
        match self.pool.get(filler) {
            ConceptExpr::Atomic(c) => self.dkey_ranges.get(c),
            _ => None,
        }
    }

    /// Cap the search at a wall-clock instant. The driver in
    /// [`crate::search`] consults it on every expansion and bails out
    /// (returning `None`) the moment the deadline has passed. Use this
    /// to bound per-pair tableau time in higher-level classifiers
    /// without resorting to OS-thread cancellation.
    pub fn set_deadline(&mut self, deadline: std::time::Instant) -> &mut Self {
        self.deadline = Some(deadline);
        self
    }

    /// Force the anywhere-blocking toggle, overriding the
    /// `RUSTDL_ANYWHERE_BLOCKING`-derived default cached at construction. Lets
    /// callers/tests select the blocking strategy explicitly without depending
    /// on process-global environment (which is racy under parallel tests).
    pub fn set_anywhere_blocking(&mut self, on: bool) -> &mut Self {
        self.anywhere_blocking = on;
        // The blocking mode is not on the trail, so drop any memoised answer
        // computed under the other mode (#288).
        self.blocked_memo.set((NodeId::new(0), u64::MAX, false));
        // Keep the graph's index-maintenance flag in lockstep and (re)build the
        // index from the current arena so a flip after node creation is still
        // correct — used by tests that toggle the mode on a populated context.
        self.graph.set_block_index_enabled(on);
        self
    }

    /// Whether anywhere (pairwise) blocking is active for this context.
    #[must_use]
    pub fn anywhere_blocking(&self) -> bool {
        self.anywhere_blocking
    }

    /// True iff a previously-set deadline was observed elapsed during
    /// the search. Sticky once set. Read this *after* [`crate::search`]
    /// returns to disambiguate "depth limit reached" from "deadline
    /// hit".
    #[must_use]
    pub fn deadline_reached(&self) -> bool {
        self.deadline_hit
    }

    /// Clear the sticky [`Self::deadline_reached`] flag.
    ///
    /// Only for a caller that runs [`crate::search`] **more than once** on one
    /// context under **different** deadlines — today that is the main-tableau
    /// iterative-deepening driver, whose shallow levels each carry a small
    /// sub-deadline while the final level carries the caller's own. Without
    /// this the shallow level's elapsed sub-deadline would stick, and the final
    /// level's genuine depth-cap `DepthLimit` would be misreported as a
    /// deadline cut. Those two are NOT interchangeable at the reasoner's
    /// mapping site: a deadline `DepthLimit` becomes `Ok(None)` while a
    /// depth-cap `DepthLimit` becomes `Err(NoVerdict)`, and
    /// `classify_internal_with_timeout` propagates that `Err` with `?`.
    ///
    /// Cannot affect soundness: the flag is a *reporting* channel, read only to
    /// disambiguate two inconclusive verdicts. It can never turn an
    /// inconclusive verdict into `Sat` or `Unsat`.
    pub fn clear_deadline_hit(&mut self) -> &mut Self {
        self.deadline_hit = false;
        self
    }

    /// Arm the adaptive early-abandon on this probe with the given depth-cap-hit
    /// limit (see [`EarlyAbandon`]). `0` leaves the accounting live but never cuts
    /// — the telemetry-only arm the limit was calibrated on.
    ///
    /// Called by the reasoner facade only when `RUSTDL_TABLEAU_EARLY_ABANDON=1`;
    /// an unarmed context is byte-identical to the pre-change search.
    pub fn enable_early_abandon(&mut self, cap_hit_limit: u64) -> &mut Self {
        self.early_abandon = Some(EarlyAbandon {
            cap_hit_limit,
            stall_run: 0,
            max_stall_run: 0,
            depth0: 0,
            trials: 0,
            definite: 0,
            abandoned: false,
        });
        self
    }

    /// True once the early-abandon criterion has fired on this probe. Latched,
    /// so the whole DFS unwinds. Always `false` when unarmed.
    #[must_use]
    pub fn early_abandoned(&self) -> bool {
        self.early_abandon.as_ref().is_some_and(|e| e.abandoned)
    }

    /// Record that [`crate::search`] bottomed out at the depth cap
    /// (`max_depth == 0`) — the harm this lever exists to bound. Deliberately
    /// NOT called for a deadline cut: the two exits share a `DepthLimit` verdict
    /// but only the cap one is evidence that the probe is thrashing at depth (the
    /// constant audit's split `search_depth0`/`search_deadline0` counters made
    /// exactly this distinction, and merging them would have read a deadline as a
    /// cap hit).
    /// This is also where the cut fires: latching at the source of the harm means
    /// the `cap_hit_limit`-th bottom-out abandons immediately, and every
    /// `crate::search` entry from then on returns `DepthLimit` so the DFS unwinds.
    pub fn note_depth_cap_hit(&mut self) {
        if let Some(e) = self.early_abandon.as_mut() {
            e.depth0 += 1;
            e.stall_run += 1;
            e.max_stall_run = e.max_stall_run.max(e.stall_run);
            if e.cap_hit_limit > 0 && e.depth0 >= e.cap_hit_limit {
                e.abandoned = true;
            }
        }
    }

    /// Record the outcome of one branch-option trial and return `true` iff the
    /// probe has been abandoned (so the caller stops trying siblings).
    ///
    /// `definite` (the child returned `Unsat`/`Sat`/`NodeCap`) resets the
    /// `stall_run` telemetry only; the criterion itself is cumulative and lives in
    /// [`Self::note_depth_cap_hit`] — see [`EarlyAbandon`] for why the
    /// reset-on-progress variant was refuted by measurement.
    pub fn note_branch_trial(&mut self, definite: bool) -> bool {
        let Some(e) = self.early_abandon.as_mut() else {
            return false;
        };
        e.trials += 1;
        if definite {
            e.definite += 1;
            e.stall_run = 0;
        }
        e.abandoned
    }

    /// `(trials, definite, depth0, max_stall_run, abandoned)` — the early-abandon
    /// telemetry for this probe, or `None` when unarmed. Diagnostic only.
    #[must_use]
    pub fn early_abandon_stats(&self) -> Option<(u64, u64, u64, u64, bool)> {
        self.early_abandon
            .as_ref()
            .map(|e| (e.trials, e.definite, e.depth0, e.max_stall_run, e.abandoned))
    }

    /// Allocate the next fresh `branch_id` and push it onto the
    /// active-branches stack. Returns the freshly issued id.
    /// `Frame::or` and `Frame::merge` call this on entry; `Frame::finish`
    /// calls [`Self::pop_branch`] on exit.
    #[doc(hidden)]
    pub fn push_branch(&mut self) -> u32 {
        let id = self.next_branch_id;
        self.next_branch_id = self.next_branch_id.checked_add(1).expect(
            "TableauContext: branch id counter overflowed u32 — \
             pathological search tree",
        );
        // Ids only grow and frames close in reverse order, so the active set
        // stays sorted, which the clash-dep unions rely on (#288).
        debug_assert!(
            self.active_branches.last().is_none_or(|&last| last < id),
            "active branch ids must stay sorted"
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
    /// a search frame (`crate::search`) can skip it. Returns `None` if no
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
    /// a search frame (`crate::search`) when a disjunct's failure carries
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
    /// a search frame (`crate::search`) just before it asserts each
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

    /// True once live node count exceeds [`crate::effective_max_nodes`]. Callers
    /// treat a resulting `NodeCap` verdict as a clean "no verdict" (sound
    /// under-approx).
    #[must_use]
    pub fn node_cap_exceeded(&self) -> bool {
        crate::effective_max_nodes().is_some_and(|cap| self.graph().len() > cap)
    }

    #[must_use]
    pub fn pool(&self) -> &ConceptPool {
        self.pool
    }

    #[must_use]
    pub fn tbox(&self) -> Option<&AbsorbedTBox> {
        self.tbox
    }

    /// True iff `node`'s label set entails the atomic filler `body` either
    /// directly (`body ∈ L(node)`) or through the told-subsumer closure (some
    /// `Atomic(C) ∈ L(node)` with `C ⊑* body` via atomic `ConceptRule`s).
    ///
    /// Soundness: an atomic `ConceptRule` (`trigger ClassId → Atomic super`) is
    /// an unconditional told subsumption, so a node carrying `C` is in every
    /// model also a `body` when `C ⊑* body`. Counting it as a `≥n R.body`
    /// witness therefore never over-counts; it only avoids generating a
    /// redundant successor that a later `≤m R.body` would clash on.
    #[must_use]
    pub fn node_entails_filler(&self, node: NodeId, body: ConceptId) -> bool {
        // #382: every node is in `⊤`.
        if self.top_filler_body(body) {
            return true;
        }
        let labels = &self.graph.node(node).labels;
        if labels.binary_search(&body).is_ok() {
            return true;
        }
        let closure = self
            .told_super_closure
            .get_or_init(|| self.build_told_super_closure());
        if closure.is_empty() {
            return false;
        }
        for &l in labels {
            if let owl_dl_core::ConceptExpr::Atomic(cls) = self.pool.get(l)
                && let Some(supers) = closure.get(cls)
                && supers.binary_search(&body).is_ok()
            {
                return true;
            }
        }
        false
    }

    /// Build the transitive told-subsumer closure over atomic classes from the
    /// `TBox`'s atomic `ConceptRule`s. `trigger → sorted [atomic super ConceptId]`.
    fn build_told_super_closure(&self) -> HashMap<owl_dl_core::ClassId, Vec<ConceptId>> {
        use owl_dl_core::ConceptExpr;
        let mut closure: HashMap<owl_dl_core::ClassId, Vec<ConceptId>> = HashMap::new();
        let Some(tbox) = self.tbox else {
            return closure;
        };
        // Direct atomic told-supers: trigger ClassId -> [Atomic super ConceptId].
        let mut direct: HashMap<owl_dl_core::ClassId, Vec<ConceptId>> = HashMap::new();
        for rule in &tbox.concept_rules {
            if matches!(self.pool.get(rule.conclusion), ConceptExpr::Atomic(_)) {
                direct
                    .entry(rule.trigger)
                    .or_default()
                    .push(rule.conclusion);
            }
        }
        // Transitive closure via BFS over atomic super -> its ClassId -> direct.
        for (&trigger, seeds) in &direct {
            let mut seen: Vec<ConceptId> = Vec::new();
            let mut stack: Vec<ConceptId> = seeds.clone();
            while let Some(c) = stack.pop() {
                if seen.contains(&c) {
                    continue;
                }
                seen.push(c);
                if let ConceptExpr::Atomic(super_cls) = self.pool.get(c)
                    && let Some(next) = direct.get(super_cls)
                {
                    for &n in next {
                        if !seen.contains(&n) {
                            stack.push(n);
                        }
                    }
                }
            }
            seen.sort_unstable();
            closure.insert(trigger, seen);
        }
        closure
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
        // Dedup so a caller that double-declares doesn't bloat the Vec.
        // Each direction is stored once. Phase 3b: mirror both directions into
        // `inverse_pairs_set` unconditionally (HashSet self-dedups).
        if !self.inverse_pairs.contains(&(r, s)) {
            self.inverse_pairs.push((r, s));
        }
        if !self.inverse_pairs.contains(&(s, r)) {
            self.inverse_pairs.push((s, r));
        }
        self.inverse_pairs_set.insert((r, s));
        self.inverse_pairs_set.insert((s, r));
        self
    }

    /// True if `r` and `s` are linked by a declared
    /// True iff the ontology declared ANY `InverseObjectProperties` pair
    /// (`SymmetricObjectProperty` lowers into one, so symmetric roles count).
    ///
    /// Lets a caller that would otherwise enumerate candidate roles — e.g. the
    /// chain rule composing `⊑` with `≡` in
    /// [`chain_leg_targets`](crate::apply_role_chains) — skip that walk outright
    /// on the overwhelmingly common inverse-free ontology.
    #[must_use]
    pub fn has_inverse_pairs(&self) -> bool {
        !self.inverse_pairs_set.is_empty()
    }

    /// `InverseObjectProperties` axiom.
    #[must_use]
    pub fn are_declared_inverses(&self, r: RoleId, s: RoleId) -> bool {
        // Phase 3b: O(1) hashbrown::HashSet lookup replaces the linear
        // `Vec::iter().any()` scan. The pizza-era "0-3 pairs" assumption
        // that justified the linear scan was empirically wrong on SIO
        // (84 inverse-pair declarations → ~168 Vec entries); SIO
        // flamegraph attributed 25.76% of apply_max to the scan.
        // See `docs/phase3b-fix-target.md`.
        if self.inverse_pairs_set.is_empty() {
            return false;
        }
        crate::bump_counter!(self, inverse_pair_fast_hits);
        self.inverse_pairs_set.contains(&(r, s))
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

    /// Whether role matching consults the signed closure (#211, #216).
    #[must_use]
    pub fn signed_roles_active(&self) -> bool {
        self.signed_roles
    }

    /// Whether `apply_role_chains` also runs its middle pass (#213).
    #[must_use]
    pub fn chain_middle_enabled(&self) -> bool {
        self.chain_middle
    }

    /// Whether `saturate` may start from the dirty nodes alone (#288).
    #[must_use]
    pub fn incremental_saturate_enabled(&self) -> bool {
        self.incremental_saturate
    }

    /// The nodes an incremental `saturate` call must dirty, or `None` when it
    /// must take the full path (#288): the last `StableMark`'s deferred set,
    /// plus every node that gained a label since and that node's neighbours.
    /// Read off the trail, so a rollback restores the matching set.
    #[must_use]
    pub fn incremental_start(&self) -> Option<Vec<NodeId>> {
        let pool = self.pool;
        // A nominal label is not local: anywhere-blocking stops using a node
        // as a blocker once it carries one, which can unblock a node anywhere.
        let (labelled, pending) = self
            .trail
            .labels_since_stable(|c| !matches!(pool.get(c), ConceptExpr::Nominal(_)))?;
        let mut start = pending.to_vec();
        for node in labelled {
            start.push(node);
            start.extend(self.graph.node(node).neighbours().map(|(_, m)| m));
        }
        Some(start)
    }

    /// Start a `saturate` call (#288): forget the deferred set, which the
    /// caller has already read into the start set or does not need.
    pub fn begin_saturate(&mut self) {
        for node in std::mem::take(&mut self.deferred) {
            if let Some(seen) = self.deferred_seen.get_mut(node.index() as usize) {
                *seen = false;
            }
        }
        self.scan_phase = ScanPhase::Later;
    }

    /// The first scan of this `saturate` call is at node `pos` and ends at
    /// `end` (#288).
    pub fn set_first_scan_at(&mut self, pos: usize, end: usize) {
        self.scan_phase = ScanPhase::FirstScan { pos, end };
    }

    /// The first scan of this `saturate` call is over (#288).
    pub fn end_first_scan(&mut self) {
        self.scan_phase = ScanPhase::Later;
    }

    /// The `saturate` call is over (#288). Mutations the search makes between
    /// calls need no bookkeeping: the next call reads them off the trail.
    pub fn end_saturate(&mut self) {
        self.scan_phase = ScanPhase::Idle;
    }

    /// A rule skipped `node` because it was blocked: a full pass visits it
    /// again on the next call, so the next incremental call must too (#288).
    ///
    /// Load-bearing: blocking reads more than one hop away (ancestors' labels,
    /// and for anywhere-blocking any blocker's), so a node can become unblocked
    /// without being touched. A node blocked when last visited therefore stays
    /// deferred, call after call, until a visit finds it unblocked. A node that
    /// was not blocked when last visited had all its rules run then, and
    /// becoming blocked never creates work.
    pub fn note_blocked_skip(&mut self, node: NodeId) {
        if self.incremental_saturate {
            self.defer(node);
        }
    }

    fn defer(&mut self, node: NodeId) {
        let i = node.index() as usize;
        if self.deferred_seen.len() <= i {
            self.deferred_seen.resize(i + 1, false);
        }
        if !self.deferred_seen[i] {
            self.deferred_seen[i] = true;
            self.deferred.push(node);
        }
    }

    /// `nodes` changed: they and their neighbours may now have rule work
    /// (#288). A node the current first scan has yet to reach is dirtied, as a
    /// full pass would visit it anyway; any other is deferred to the next call,
    /// which is when a full pass would next visit it. Every mutation touches
    /// both its endpoints, and every rule other than blocking reads at most the
    /// node it runs at and that node's neighbours (a role chain at `x` reads
    /// `x → y → z` and fires when `y`, a neighbour of both, is touched).
    /// Outside a `saturate` call this is a no-op: the next call's start set
    /// comes from the trail.
    pub(crate) fn touch(&mut self, nodes: &[NodeId]) {
        if !self.incremental_saturate || self.scan_phase == ScanPhase::Idle {
            return;
        }
        for &node in nodes {
            let neighbours: Vec<NodeId> =
                self.graph.node(node).neighbours().map(|(_, m)| m).collect();
            for m in std::iter::once(node).chain(neighbours) {
                let i = m.index() as usize;
                match self.scan_phase {
                    ScanPhase::FirstScan { pos, end } if i > pos && i < end => {
                        self.graph.set_dirty(m, true);
                    }
                    _ => self.defer(m),
                }
            }
        }
    }

    /// Record that `saturate` reached a fixpoint, with the deferred set
    /// (#288). A no-op unless incremental saturation is on, so `=0` reverts it.
    pub fn record_stable_mark(&mut self) {
        if !self.incremental_saturate {
            return;
        }
        let pending = self.deferred.clone();
        self.trail.record(TrailEntry::StableMark { pending });
    }

    /// Whether the search tries `∀` disjuncts first (#288).
    #[must_use]
    pub fn forall_first_enabled(&self) -> bool {
        self.forall_first
    }

    /// Whether the search stays in the tree of its last decision (#377).
    #[must_use]
    pub fn tree_sticky_branching(&self) -> bool {
        self.tree_sticky
    }

    /// Whether a met `≤n` disjunct is tried first (#382).
    #[must_use]
    pub fn max_first(&self) -> bool {
        self.max_first
    }

    /// Whether `body` is `⊤` and [`top_filler_enabled`] is on (#382): then every
    /// neighbour counts as carrying it.
    #[must_use]
    pub fn top_filler_body(&self, body: ConceptId) -> bool {
        self.top_filler && matches!(self.pool.get(body), owl_dl_core::ConceptExpr::Top)
    }

    /// Test hook for the two #382 flags.
    #[cfg(test)]
    pub(crate) fn set_counting_flags(&mut self, max_first: bool, top_filler: bool) {
        self.max_first = max_first;
        self.top_filler = top_filler;
    }

    /// The tree root of the last decision the search opened (#377).
    #[must_use]
    pub(crate) fn last_decision_root(&self) -> u32 {
        self.last_decision_root
    }

    /// Record that the search opened a decision at `node` (#377).
    pub(crate) fn note_decision_at(&mut self, node: NodeId) {
        self.last_decision_root = self.graph.node(node).root;
    }

    /// Forget the last decision's tree, so the next one is taken in node order
    /// (#377).
    pub(crate) fn forget_decision_root(&mut self) {
        self.last_decision_root = u32::MAX;
    }

    /// Test hook for [`Self::tree_sticky_branching`].
    #[cfg(test)]
    pub(crate) fn set_tree_sticky_branching(&mut self, on: bool) {
        self.tree_sticky = on;
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
        let direct = if seen.is_inverse() == wanted.is_inverse() {
            match self.hierarchy {
                Some(h) => h.is_sub_role(s, w),
                None => s == w,
            }
        } else {
            self.are_declared_inverses(s, w)
        };
        // #211: the attached hierarchy also carries the SIGNED closure (#177),
        // which composes inclusions across polarity — `s⁻ ⊑ p` makes an
        // `s`-edge a `p⁻`-edge, which neither branch above sees, so a `≤1 p⁻`
        // never counted `s`-successors. Every signed inclusion is entailed, so
        // this can only add true matches.
        direct
            || (self.signed_roles
                && self
                    .hierarchy
                    .is_some_and(|h| h.is_signed_sub(seen, wanted)))
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

    /// True iff `or_id` is `Or(args)` with a `Nominal(_)` disjunct and no
    /// disjunct already present in `labels` (sorted ascending).
    fn is_open_nominal_or(&self, or_id: ConceptId, labels: &[ConceptId]) -> bool {
        if let owl_dl_core::ConceptExpr::Or(args) = self.pool.get(or_id) {
            let open = !args.iter().any(|d| labels.binary_search(d).is_ok());
            open && args
                .iter()
                .any(|&d| matches!(self.pool.get(d), owl_dl_core::ConceptExpr::Nominal(_)))
        } else {
            false
        }
    }

    /// True iff `node` carries a pending nominal-covering disjunction: either
    /// a materialized open `Or`-with-`Nominal` label, OR an `Atomic(class)`
    /// whose absorbed concept-rule conclusion is such an `Or` not yet
    /// satisfied. The `TBox` branch is what fires on the #35 bug — the nominal
    /// `Or` is a deferred concept-rule conclusion, unmaterialized at
    /// generation time (`rules.rs` defers `Or` conclusions; the stable-state
    /// sweep in `saturate.rs` materializes them later), so at generation time
    /// the node has only the atomic trigger label.
    #[must_use]
    pub fn has_pending_nominal_disjunction(&self, node: NodeId) -> bool {
        let labels = self.graph.node(node).labels();
        // (1) materialized open Or on the node.
        if labels.iter().any(|&c| self.is_open_nominal_or(c, labels)) {
            return true;
        }
        // (2) pending via TBox concept-rule keyed by an atomic label's class.
        let Some(tbox) = self.tbox else {
            return false;
        };
        for &c in labels {
            if let owl_dl_core::ConceptExpr::Atomic(class) = self.pool.get(c) {
                if let Some(concls) = tbox.concept_rules_by_trigger.get(class) {
                    if concls.iter().any(|&o| self.is_open_nominal_or(o, labels)) {
                        return true;
                    }
                } else if tbox.concept_rules_by_trigger.is_empty()
                    && tbox.concept_rules.iter().any(|r| {
                        r.trigger == *class && self.is_open_nominal_or(r.conclusion, labels)
                    })
                {
                    // Linear fallback when the index was never built
                    // (finalize not called) — mirrors apply_concept_rules.
                    return true;
                }
            }
        }
        false
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
    ///
    /// Dispatch: by default the candidate `x'` ranges over `y`'s strict
    /// tree-ancestors ([`Self::is_blocked_ancestor`]). When
    /// `RUSTDL_ANYWHERE_BLOCKING=1` (cached in `anywhere_blocking` at
    /// construction) the candidate ranges over ANY earlier-created
    /// non-excluded node ([`Self::is_blocked_anywhere`]) — the standard
    /// Motik/Shearer/Horrocks anywhere-blocking optimisation that keeps the
    /// completion small on large generative `ABox`es.
    #[must_use]
    pub fn is_blocked(&self, y: NodeId) -> bool {
        let generation = self.trail.generation();
        let (memo_node, memo_gen, memo_blocked) = self.blocked_memo.get();
        if memo_node == y && memo_gen == generation {
            debug_assert_eq!(
                memo_blocked,
                self.is_blocked_uncached(y),
                "is_blocked memo is stale: a graph mutation bypassed the trail"
            );
            return memo_blocked;
        }
        let blocked = self.is_blocked_uncached(y);
        self.blocked_memo.set((y, generation, blocked));
        blocked
    }

    fn is_blocked_uncached(&self, y: NodeId) -> bool {
        if self.anywhere_blocking {
            self.is_blocked_anywhere(y)
        } else {
            self.is_blocked_ancestor(y)
        }
    }

    /// Ancestor-scoped pair blocking — the historical default. `x'` ranges
    /// over `y`'s strict tree-ancestors only. See [`Self::is_blocked`] for the
    /// pairwise conditions (1)–(4).
    #[must_use]
    #[allow(clippy::similar_names)]
    fn is_blocked_ancestor(&self, y: NodeId) -> bool {
        crate::bump_counter!(self, is_blocked_calls);
        let yb = self.graph.blocking(y);
        let (Some(yp_id), Some(yr)) = (yb.parent, yb.parent_role) else {
            return false;
        };
        // #361/#362: a nominal node is never blocked (as in anywhere blocking).
        if self.nominal_block_guard && self.node_is_nominal(y) {
            return false;
        }
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
                            // #362: a nominal blocker, or either side standing for
                            // a nominal's `≤`-witness, is refused.
                            let refused = (self.nominal_block_guard
                                && self.node_is_nominal(x_prime_id))
                                || self.nn_guarded(y)
                                || self.nn_guarded(x_prime_id);
                            if !refused {
                                crate::bump_counter!(self, is_blocked_true);
                                return true;
                            }
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

    /// #361/#362: for each `≤n R.C` at a nominal neighbour `w` of `v` that counts
    /// `v` (`v` an `R`-neighbour of `w` without `¬C`), whether `w` has more than `n`
    /// such neighbours. `None` when the guard is off or nothing counts `v`.
    ///
    /// For `C = ⊤` with `¬⊤ = ⊥` registered (every `≤n R.⊤` the reasoner
    /// prepares), this already counts every `R`-neighbour, which agrees with
    /// [`top_filler_enabled`] (#382). A `≤n R.⊤` first interned by a query has no
    /// registered complement and is skipped here while #382 counts it: that can
    /// only keep a node unblocked longer, never add a clash.
    fn nominal_max_overfull(&self, v: NodeId) -> Option<bool> {
        if !self.nominal_block_guard {
            return None;
        }
        let mut found = None;
        for (_, w) in self.graph.node(v).neighbours() {
            if !self.node_is_nominal(w) {
                continue;
            }
            for &c in &self.graph.node(w).labels {
                let ConceptExpr::Max(n, role, body) = self.pool.get(c) else {
                    continue;
                };
                if self.dkey_range_of(*body).is_some() {
                    continue;
                }
                // The choose rule skips a `≤` with no registered complement;
                // so does this count, so the two agree label by label.
                let Some(comp) = self.complement_of(*body) else {
                    continue;
                };
                let mut cands: Vec<NodeId> = Vec::new();
                for (seen, x) in self.graph.node(w).neighbours() {
                    if self.edge_satisfies(seen, *role)
                        && !self.graph.node(x).has_label(comp)
                        && !cands.contains(&x)
                    {
                        cands.push(x);
                    }
                }
                if cands.contains(&v) {
                    let over = cands.len() > *n as usize;
                    if over {
                        return Some(true);
                    }
                    found = Some(false);
                }
            }
        }
        found
    }

    /// #361/#362: `v` stands in for a nominal individual's `≤`-witness and must be
    /// neither blocked nor a blocker: a `≤n R.C` at a nominal neighbour counts `v`
    /// and is not over-full. Over-full, the guard lifts so blocking bounds the
    /// chain until the `≤` merges.
    fn nn_guarded(&self, v: NodeId) -> bool {
        self.nominal_max_overfull(v) == Some(false)
    }

    /// Whether `RUSTDL_NOMINAL_BLOCK_GUARD` is on (cached).
    pub(crate) fn nominal_block_guard(&self) -> bool {
        self.nominal_block_guard
    }

    /// Whether `≤n` merges use exact distinctness deps (#370,
    /// [`merge_exact_deps_enabled`], cached at construction).
    pub(crate) fn merge_exact_deps(&self) -> bool {
        self.merge_exact_deps
    }

    /// Test-only override of the cached `RUSTDL_MERGE_EXACT_DEPS`, so tests do
    /// not set a process-wide env var under parallel test threads.
    #[cfg(test)]
    pub(crate) fn set_merge_exact_deps(&mut self, on: bool) {
        self.merge_exact_deps = on;
    }

    /// #362: generation (`∃`, `≥n`) waits at `v` while a `≤` at a nominal neighbour
    /// that counts `v` is over-full, so the merge runs before `v` makes a child a
    /// later merge would reparent (the SHOIQ priority of the `≤`/o-rules over the
    /// generating rules). Deferral only: a deferring rule registers `v` with
    /// [`Self::note_blocked_skip`], so incremental saturation revisits it. If the
    /// `≤` stops being over-full other than by a merge at its holder (which dirties
    /// every neighbour), the revisit comes from the `Sat`-leaf re-saturation
    /// (`RUSTDL_SAT_LEAF_RESATURATE`, default ON), as for any blocked skip.
    pub(crate) fn defer_generation_at_overfull_nominal(&self, v: NodeId) -> bool {
        self.nominal_max_overfull(v) == Some(true)
    }

    /// True iff any label of `node` is a [`ConceptExpr::Nominal`] — i.e. the
    /// node is pinned to a specific individual `{a}`. Such nodes are excluded
    /// from anywhere blocking on BOTH sides (a nominal node is an unsound
    /// blocker, and a nominal node must not itself be blocked) because the
    /// loop-back model trick that justifies pair blocking does not hold for a
    /// node whose extension is forced to a single named individual.
    #[must_use]
    fn node_is_nominal(&self, node: NodeId) -> bool {
        let labels = &self.graph.node(node).labels;
        // #288: an ontology has few nominal concepts, so look each one up in
        // the sorted labels instead of reading every label's expression. The
        // two branches return the same answer.
        let nominals = self.pool.nominal_ids();
        if nominals.len() <= labels.len() {
            nominals.iter().any(|n| labels.binary_search(n).is_ok())
        } else {
            labels
                .iter()
                .any(|&c| matches!(self.pool.get(c), owl_dl_core::ConceptExpr::Nominal(_)))
        }
    }

    /// Anywhere-scoped pair blocking (Motik/Shearer/Horrocks). `y` is blocked
    /// by ANY node `x'` *created strictly before* `y` (i.e. `x'.index() <
    /// y.index()`, which is exactly creation order — see graph.rs
    /// `push_node_with_parent`) satisfying the SAME pairwise conditions (1)–(4)
    /// as [`Self::is_blocked`], MINUS the following candidate exclusions:
    ///
    /// - roots/orphans (`parent == None`) — condition (1);
    /// - redirected nodes (`merged_into.is_some()`) — stale, state moved to the
    ///   representative;
    /// - nominal-labelled nodes ([`Self::node_is_nominal`]) — unsound blocker.
    ///
    /// A nominal `y` is never blocked (same reason). The strict
    /// `x'.index() < y.index()` order makes the blocking relation acyclic, so a
    /// terminating completion is guaranteed.
    ///
    /// SOUNDNESS NOTE: this is sound *here specifically* because the main
    /// tableau gates GENERATION only on direct `is_blocked(node)` and never
    /// suspends a node when an ancestor becomes blocked. Hence every
    /// non-directly-blocked node is fully expanded, so any candidate `x'` that
    /// passes (1)–(4) has a complete label set and is a valid blocker. The
    /// classic anywhere-blocking bug (blocking against a node with stale labels
    /// because an ancestor is blocked) cannot arise. See the plan doc.
    #[must_use]
    #[allow(clippy::similar_names)]
    fn is_blocked_anywhere(&self, y: NodeId) -> bool {
        crate::bump_counter!(self, is_blocked_calls);
        let yb = self.graph.blocking(y);
        let (Some(yp_id), Some(yr)) = (yb.parent, yb.parent_role) else {
            return false;
        };
        // A nominal `y` denotes a fixed individual and must not be blocked.
        if self.node_is_nominal(y) || self.nn_guarded(y) {
            return false;
        }
        let yl_sig = yb.label_sig;
        let yp_sig = self.graph.blocking(yp_id).label_sig;

        // Phase B: iterate ONLY the candidate bucket for `y`'s parent_role
        // (condition (2) holds by construction — every node in the bucket was
        // created with role `yr`), so this is O(bucket) not O(N). The bucket is
        // ascending (creation order); the strict `cand.index() < y.index()`
        // guard is the termination/acyclicity bound AND skips `y` itself and
        // any later node. Conditions (3)/(4) are the subset checks; the
        // `label_sig` prefilter is identical to the ancestor path.
        //
        // `block_candidates` returns an empty slice when the index is disabled,
        // so this loop is correct only with the index maintained — which
        // `set_anywhere_blocking` / `with_block_index(true)` guarantee whenever
        // `anywhere_blocking` is on.
        for &x_prime_id in self.graph.block_candidates(yr) {
            if x_prime_id.index() >= y.index() {
                // Bucket is ascending: nothing past here is an earlier node.
                break;
            }
            let xb = self.graph.blocking(x_prime_id);
            // (1) candidate must be a non-root with its own creator.
            let Some(xp_id) = xb.parent else {
                continue;
            };
            // Exclude redirected (merged-away) candidates: their live state
            // lives on the representative, so their own label set is stale.
            // Also a candidate whose parent was pruned (#362).
            if self.graph.node(x_prime_id).is_redirected() || self.graph.node(xp_id).is_redirected()
            {
                continue;
            }
            // `label_sig` subset prefilter (necessary condition for (3)/(4)).
            if (yl_sig & !xb.label_sig) != 0
                || (yp_sig & !self.graph.blocking(xp_id).label_sig) != 0
            {
                crate::bump_counter!(self, is_blocked_prefilter_rejects);
                continue;
            }
            // Exclude nominal-labelled candidates (unsound blockers).
            if self.node_is_nominal(x_prime_id) {
                continue;
            }
            crate::bump_counter!(self, is_blocked_subset_scans);
            let y_labels = &self.graph.node(y).labels;
            let x_labels = &self.graph.node(x_prime_id).labels;
            if is_subset_sorted(y_labels, x_labels) {
                let yp_labels = &self.graph.node(yp_id).labels;
                let xp_labels = &self.graph.node(xp_id).labels;
                if is_subset_sorted(yp_labels, xp_labels) && !self.nn_guarded(x_prime_id) {
                    crate::bump_counter!(self, is_blocked_true);
                    return true;
                }
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
                self.touch(&[node]);
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
        self.touch(&[from, target]);
        // Worklist: rules that read edges on either endpoint must
        // re-fire (apply_forall, apply_role_*, apply_role_chains,
        // apply_exists witness check, apply_min/max cardinality
        // counting).
        self.graph.set_dirty(from, true);
        if target != from {
            self.graph.set_dirty(target, true);
        }
    }

    /// Mark `a` and `b` as denoting distinct individuals, with no deps.
    /// Equivalent to `mark_distinct_with_deps(a, b, &[])`: for marks that
    /// hold before any decision (asserted `DifferentIndividuals`).
    pub fn mark_distinct(&mut self, a: NodeId, b: NodeId) {
        self.mark_distinct_with_deps(a, b, &[]);
    }

    /// Mark `a` and `b` as denoting distinct individuals. Symmetric.
    /// Idempotent: a no-op if the pair is already marked, which keeps the
    /// first mark's deps. That is sound with the LIFO trail: a later reason
    /// is undone no later than the mark itself, and the first reason holds
    /// as long as the mark exists. Records a [`TrailEntry::DistinctMarked`]
    /// when the mark is fresh.
    ///
    /// `deps` are the decisions the mark depends on (#370). They are stored
    /// only with `RUSTDL_MERGE_EXACT_DEPS` on; otherwise an empty set is kept,
    /// so the two vectors stay in lockstep at no cost.
    pub fn mark_distinct_with_deps(&mut self, a: NodeId, b: NodeId, deps: &[u32]) {
        if a == b || self.are_distinct(a, b) {
            return;
        }
        let deps = if self.merge_exact_deps {
            DepSet::from_slice(deps)
        } else {
            DepSet::new()
        };
        let na = self.graph.node_mut(a);
        na.inequalities.push(b);
        na.inequality_deps.push(deps.clone());
        let nb = self.graph.node_mut(b);
        nb.inequalities.push(a);
        nb.inequality_deps.push(deps);
        self.trail.record(TrailEntry::DistinctMarked { a, b });
        self.touch(&[a, b]);
    }

    /// The deps of the mark `a ≠ b` (#370): empty when the pair is not
    /// marked, or when `RUSTDL_MERGE_EXACT_DEPS` is off.
    #[must_use]
    pub fn distinct_deps(&self, a: NodeId, b: NodeId) -> DepSet {
        let n = self.graph.node(a);
        n.inequalities
            .iter()
            .position(|&x| x == b)
            .map(|i| n.inequality_deps[i].clone())
            .unwrap_or_default()
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
        // Only `apply_nominal_assignment` reads the map, at nodes carrying the
        // nominal, and those are touched by their own label additions and
        // merges (#309 review). Touching every node here was O(N²) when the
        // reasoner seeds an ABox.
        match prior {
            Some(prior) => self.touch(&[prior, node]),
            None => self.touch(&[node]),
        }
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
    /// Whether `anc` lies on `start`'s parent-ancestor chain (so making
    /// some node's parent point at `start` while that node is `anc`
    /// would close a cycle). Cycle-safe: bounded by the node count, so it
    /// terminates even if the forest is (transiently) malformed.
    fn parent_chain_contains(&self, start: NodeId, anc: NodeId) -> bool {
        let mut cur = start;
        for _ in 0..=self.graph.len() {
            if cur == anc {
                return true;
            }
            match self.graph.node(cur).parent() {
                Some(p) => cur = p,
                None => return false,
            }
        }
        false
    }

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
        let source_ineq: Vec<(NodeId, DepSet)> = {
            let n = self.graph.node(source);
            n.inequalities
                .iter()
                .copied()
                .zip(n.inequality_deps.iter().cloned())
                .collect()
        };

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
            // The forward edge was located on `y_eff` (the union-find
            // representative), so its mirror on `source.in_edges` is
            // `(role, y_eff)` — the pair `add_edge` always maintains. Search
            // for `(role, y_eff)`, NOT the unresolved snapshot node `(role, y)`:
            // when `y` has since merged into `y_eff`, `source.in_edges` can
            // carry BOTH a stale `(role, y)` (orphaned by that earlier merge)
            // and the live `(role, y_eff)`, and matching `(role, y)` removes the
            // wrong mirror — `remove_edge_recorded`'s `(role, from == y_eff)`
            // assertion then fires (issue #38), or in release silently corrupts
            // the edge set. `y == y_eff` reduces to the original lookup.
            let in_pos = self
                .graph
                .node(source)
                .in_edges
                .iter()
                .position(|&e| e == (role, y_eff))
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
        //    idempotent. `target ≠ other` holds because `source ≠ other`
        //    and `source = target`: the mark's deps ∪ the merge's (#370).
        for (other, mark_deps) in source_ineq {
            if other != target && other != source {
                if self.merge_exact_deps {
                    let deps = crate::deps::union(&mark_deps, &DepSet::from_slice(merge_deps));
                    self.mark_distinct_with_deps(target, other, &deps);
                } else {
                    self.mark_distinct(target, other);
                }
            }
        }

        // 5. Rewrite children-parent pointers: any node whose
        //    parent equals source becomes parented at target.
        //    Iterate through the node arena. Skip the source itself, and
        //    skip the scan when no node has ever had `source` as parent.
        let node_count = if self.graph.had_child(source) {
            self.graph.len()
        } else {
            debug_assert!(
                (0..self.graph.len()).all(|i| self.graph.nodes[i].parent != Some(source)),
                "had_child({source:?}) is false but a node has it as parent"
            );
            0
        };
        for idx in 0..node_count {
            let nid = NodeId::new(u32::try_from(idx).expect("node count fits in u32"));
            if nid == source {
                continue;
            }
            if self.graph.node(nid).parent() == Some(source) {
                let prior_parent = Some(source);
                let prior_parent_role = self.graph.node(nid).parent_role();
                // Re-parenting `nid → target` closes a parent-pointer
                // cycle iff `nid` already lies on `target`'s ancestor
                // chain (e.g. `target.parent = nid`). That produced the
                // `parent-pointer cycle` panic (TODO(blocking-cycle)).
                // Break the back-edge instead: make `nid` a root
                // (`parent = None`). Sound — a root is never blocked, so
                // this only ever *reduces* blocking (more expansion,
                // never a false block); it matches the release step-cap
                // fallback's "treat as not-blocked", but keeps the
                // parent forest acyclic so the walk (and the
                // debug_assert) stay valid.
                let new_parent = if self.parent_chain_contains(target, nid) {
                    None
                } else {
                    Some(target)
                };
                self.graph.node_mut(nid).parent = new_parent;
                if let Some(t) = new_parent {
                    self.graph.had_child[t.index() as usize] = true;
                }
                // Mirror the parent update into the cache-dense
                // blocking summary; `is_blocked` walks ancestors via
                // this array, and a stale entry would cause it to
                // miss or false-find a blocker — both unsound.
                self.graph.blocking_mut(nid).parent = new_parent;
                self.trail.record(TrailEntry::ParentRewritten {
                    node: nid,
                    prior_parent,
                    prior_parent_role,
                });
                self.touch(&[nid]);
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
        self.touch(&[source, target]);

        true
    }

    /// #362: the SHOIQ prune, run before `source` merges away under a `≤`.
    ///
    /// Without it the merge reparents `source`'s children, and on a nominal's
    /// `≤`-witnesses each round left a child that nothing could block, which
    /// generated again: `A ⊑ ∃r.A ⊓ ∃s⁻.{a}`, `{a} ⊑ ≤2 s` never terminated.
    /// Instead every blockable descendant of `source` is removed with all its
    /// edges, and a nominal descendant becomes a root (it is an individual and
    /// outlives the subtree). Only `source`'s own edges move in the merge.
    ///
    /// Sound: it only removes facts, and every pruned fact derived from labels
    /// that move to the survivor, which regenerates what it still needs.
    /// Trailed (`EdgeRemoved`, `Pruned`, `ParentRewritten`), so the checkpoint
    /// before the merge undoes it.
    ///
    /// Declines (returns `false`, pruning nothing, so the merge reparents as
    /// before) when `survivor` lies in `source`'s subtree: the survivor would
    /// itself be pruned.
    pub(crate) fn prune_subtree(&mut self, source: NodeId, survivor: NodeId) -> bool {
        if !self.nominal_block_guard || !self.graph.had_child(source) {
            return false;
        }
        let len = self.graph.len();
        let mut inside = vec![false; len];
        inside[source.index() as usize] = true;
        let mut pruned: Vec<NodeId> = Vec::new();
        let mut roots: Vec<(NodeId, NodeId)> = Vec::new();
        // Reparenting can give a child a lower index than its parent, so
        // iterate to a fixpoint rather than one index-order pass.
        let mut changed = true;
        while changed {
            changed = false;
            for idx in 0..len {
                let id = NodeId::new(u32::try_from(idx).expect("node count fits in u32"));
                if inside[idx] || self.graph.node(id).is_redirected() {
                    continue;
                }
                let Some(p) = self.graph.node(id).parent() else {
                    continue;
                };
                if !inside[p.index() as usize] {
                    continue;
                }
                if self.node_is_nominal(id) {
                    if !roots.iter().any(|&(r, _)| r == id) {
                        roots.push((id, p));
                    }
                    continue;
                }
                inside[idx] = true;
                pruned.push(id);
                changed = true;
            }
        }
        if inside[survivor.index() as usize] {
            return false;
        }
        for (id, p) in roots {
            let prior_parent_role = self.graph.node(id).parent_role();
            self.graph.node_mut(id).parent = None;
            self.graph.blocking_mut(id).parent = None;
            self.trail.record(TrailEntry::ParentRewritten {
                node: id,
                prior_parent: Some(p),
                prior_parent_role,
            });
            self.touch(&[id]);
        }
        for p in pruned {
            // Snapshots, and a missing mirror is skipped: a stale orphan
            // entry is reachable (#38) and harmless on a pruned node.
            for (role, t) in self.graph.node(p).edges.clone().into_iter().rev() {
                let (Some(pos), Some(in_pos)) = (
                    self.graph
                        .node(p)
                        .edges
                        .iter()
                        .position(|&e| e == (role, t)),
                    self.graph
                        .node(t)
                        .in_edges
                        .iter()
                        .position(|&e| e == (role, p)),
                ) else {
                    continue;
                };
                self.remove_edge_recorded(p, role, t, pos, in_pos);
            }
            for (role, s) in self.graph.node(p).in_edges.clone().into_iter().rev() {
                let (Some(pos), Some(in_pos)) = (
                    self.graph
                        .node(s)
                        .edges
                        .iter()
                        .position(|&e| e == (role, p)),
                    self.graph
                        .node(p)
                        .in_edges
                        .iter()
                        .position(|&e| e == (role, s)),
                ) else {
                    continue;
                };
                self.remove_edge_recorded(s, role, p, pos, in_pos);
            }
            self.graph.node_mut(p).pruned = true;
            self.graph.set_dirty(p, false);
            self.trail.record(TrailEntry::Pruned { node: p });
        }
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
        self.touch(&[from, target]);
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
        // Concrete-domain solver (P3): an additive, refute-only clash from a
        // node's data constraints. Reads the node's own labels, the role-rule
        // universals of its demanded roles (#327) and their copies along the
        // data-property hierarchy (#349), all entailed, so the constraint set is a
        // SUBSET of the truth — and `card_sat` is monotone, so an `Unsat` on a
        // subset is `Unsat` on the full set ⇒ sound (under-collection only
        // misses clashes, never false-clashes).
        self.concrete_domain_clash(node)
    }

    /// Refute-only concrete-domain clash for `node`'s data constraints.
    /// For each data-property role, collects the `∃p.DKey(R)` (`≥1`),
    /// `Min(n,p,DKey(R))` (`≥n`), `Max(n,p,DKey(S))` (`≤n`) and
    /// `∀p.DKey(U)` (universal filter) labels whose filler is a registered
    /// `DKey` (integer, string, float, decimal, date, or dateTime bucket),
    /// and runs the matching `card_sat`. The non-integer buckets are checked
    /// independently; the three integer buckets are also counted together over
    /// `IntUnion` (#334, #343).
    /// With the role hierarchy attached (#349), a `∀` on a role also filters its
    /// sub-roles and a sub-role's demand, narrowed by its filter, is also a demand
    /// on each super-role; limits are never copied. Returns the union of the contributing labels'
    /// `DepSet`s (a sound superset for backjumping) when the constraints are
    /// unsatisfiable, else `None`. Empty `dkey_ranges` ⇒ instant `None`.
    fn concrete_domain_clash(&self, node: NodeId) -> Option<crate::graph::DepSet> {
        use owl_dl_datatypes::{
            Card, CardRange, CardSat, DateKey, DateTimeKey, Decimal, DenseInterval, FiniteSet,
            IntInterval, IntUnion, OrdF64, ValueRange, card_sat, integer_sat,
        };
        if self.dkey_ranges.is_empty() {
            return None;
        }
        // One accumulator per data property and bucket, generic over the bucket's
        // range type. A `∀` lands in `universal`; the numeric-oneof buckets use
        // `FiniteSet<T>` (capacity = |set|).
        struct SetAcc<R> {
            mins: Vec<Card<R>>,
            maxs: Vec<Card<R>>,
            universal: Option<R>,
            deps: crate::graph::DepSet,
        }
        fn add_set<R: ValueRange>(
            by_role: &mut HashMap<RoleId, SetAcc<R>>,
            rid: RoleId,
            fs: &R,
            kind: u8,
            count: u32,
            dep_label: &crate::graph::DepSet,
        ) {
            let entry = by_role.entry(rid).or_insert_with(|| SetAcc {
                mins: Vec::new(),
                maxs: Vec::new(),
                universal: None,
                deps: crate::graph::DepSet::new(),
            });
            match kind {
                0 => entry.mins.push(Card::new(fs.clone(), 1)),
                2 => entry.mins.push(Card::new(fs.clone(), count)),
                3 => entry.maxs.push(Card::new(fs.clone(), count)),
                1 => {
                    entry.universal = Some(
                        entry
                            .universal
                            .take()
                            .map_or_else(|| fs.clone(), |u| u.intersect(fs)),
                    );
                }
                _ => unreachable!(),
            }
            entry.deps = crate::deps::union(&entry.deps, dep_label);
        }
        /// Whether some role in `roles` (sorted) has a strict super-role in `roles`.
        /// If none does, the fold copies nothing that could clash: a super-role
        /// created by a lifted demand has no limit or filter of its own, so it
        /// clashes only where the sub-role itself does.
        fn any_related(roles: &[RoleId], h: &RoleHierarchy) -> bool {
            roles.iter().any(|&r| {
                h.super_roles(r)
                    .iter()
                    .any(|&s| s != r && roles.binary_search(&s).is_ok())
            })
        }
        /// The first property, in `RoleId` order, whose constraints `card_sat`
        /// refutes, with its deps; the order makes the returned deps deterministic.
        fn first_unsat<R: ValueRange>(
            by_role: HashMap<RoleId, SetAcc<R>>,
        ) -> Option<crate::graph::DepSet> {
            let mut accs: Vec<(RoleId, SetAcc<R>)> = by_role.into_iter().collect();
            accs.sort_unstable_by_key(|(rid, _)| *rid);
            for (_, acc) in accs {
                if card_sat::<R>(acc.universal, &acc.mins, &acc.maxs) == CardSat::Unsat {
                    return Some(acc.deps);
                }
            }
            None
        }
        /// #349: fold the data-property hierarchy into one bucket. With `q ⊑ p`, a
        /// `∀` on `p` holds for `q` (copied down), and a demand on `q`, narrowed by
        /// `q`'s filter (own and inherited), is a demand on `p` (copied up). Limits
        /// are not copied: up is not entailed, and down adds nothing once demands
        /// come up narrowed. A no-op unless two roles of the bucket are related.
        fn lift_through_hierarchy<R: ValueRange>(
            by_role: &mut HashMap<RoleId, SetAcc<R>>,
            h: &RoleHierarchy,
        ) {
            if by_role.len() < 2 {
                return;
            }
            let mut roles: Vec<RoleId> = by_role.keys().copied().collect();
            roles.sort_unstable();
            if !any_related(&roles, h) {
                return;
            }
            let strict_supers =
                |q: RoleId| h.super_roles(q).iter().copied().filter(move |&p| p != q);
            // Down: each role takes the universals of its present super-roles.
            let mut inherited: Vec<(RoleId, R, crate::graph::DepSet)> = Vec::new();
            for &q in &roles {
                for p in strict_supers(q) {
                    if let Some(a) = by_role.get(&p)
                        && let Some(u) = &a.universal
                    {
                        inherited.push((q, u.clone(), a.deps.clone()));
                    }
                }
            }
            for (q, u, d) in inherited {
                let acc = by_role.get_mut(&q).expect("collected from the map");
                acc.universal = Some(
                    acc.universal
                        .take()
                        .map_or_else(|| u.clone(), |x| x.intersect(&u)),
                );
                acc.deps = crate::deps::union(&acc.deps, &d);
            }
            // Up: each role's demands, narrowed by its filter, go to its super-roles.
            let mut lifted: Vec<(RoleId, Vec<Card<R>>, crate::graph::DepSet)> = Vec::new();
            for &q in &roles {
                let a = &by_role[&q];
                if a.mins.is_empty() {
                    continue;
                }
                let narrowed: Vec<Card<R>> = a
                    .mins
                    .iter()
                    .map(|c| {
                        let range = a
                            .universal
                            .as_ref()
                            .map_or_else(|| c.range.clone(), |u| c.range.intersect(u));
                        Card::new(range, c.n)
                    })
                    .collect();
                for p in strict_supers(q) {
                    lifted.push((p, narrowed.clone(), a.deps.clone()));
                }
            }
            for (p, mins, d) in lifted {
                let acc = by_role.entry(p).or_insert_with(|| SetAcc {
                    mins: Vec::new(),
                    maxs: Vec::new(),
                    universal: None,
                    deps: crate::graph::DepSet::new(),
                });
                acc.mins.extend(mins);
                acc.deps = crate::deps::union(&acc.deps, &d);
            }
        }
        let hierarchy = self
            .hierarchy
            .filter(|_| crate::data_counting_role_hierarchy_enabled());
        let n = self.graph.node(node);
        let labels = n.labels();
        let mut bool_by_role: HashMap<RoleId, SetAcc<FiniteSet<bool>>> = HashMap::new();
        // #343: integer interval unions (`iset:` keys), counted only in the combined
        // integer check below.
        let mut int_union_by_role: HashMap<RoleId, SetAcc<IntUnion>> = HashMap::new();
        let mut uri_by_role: HashMap<RoleId, SetAcc<FiniteSet<String>>> = HashMap::new();
        let mut double_by_role: HashMap<RoleId, SetAcc<DenseInterval<OrdF64>>> = HashMap::new();
        let mut double_set_by_role: HashMap<RoleId, SetAcc<FiniteSet<OrdF64>>> = HashMap::new();
        let mut int_by_role: HashMap<RoleId, SetAcc<IntInterval>> = HashMap::new();
        let mut str_by_role: HashMap<RoleId, SetAcc<FiniteSet<String>>> = HashMap::new();
        let mut float_by_role: HashMap<RoleId, SetAcc<DenseInterval<OrdF64>>> = HashMap::new();
        let mut decimal_by_role: HashMap<RoleId, SetAcc<DenseInterval<Decimal>>> = HashMap::new();
        let mut date_by_role: HashMap<RoleId, SetAcc<DenseInterval<DateKey>>> = HashMap::new();
        let mut datetime_by_role: HashMap<RoleId, SetAcc<DenseInterval<DateTimeKey>>> =
            HashMap::new();
        let mut int_set_by_role: HashMap<RoleId, SetAcc<FiniteSet<i64>>> = HashMap::new();
        let mut float_set_by_role: HashMap<RoleId, SetAcc<FiniteSet<OrdF64>>> = HashMap::new();
        let mut decimal_set_by_role: HashMap<RoleId, SetAcc<FiniteSet<Decimal>>> = HashMap::new();
        let mut date_set_by_role: HashMap<RoleId, SetAcc<FiniteSet<DateKey>>> = HashMap::new();
        let mut datetime_set_by_role: HashMap<RoleId, SetAcc<FiniteSet<DateTimeKey>>> =
            HashMap::new();
        // (role, filler, kind 0=∃ 1=∀ 2=≥ 3=≤, n, deps)
        let mut items: Vec<(Role, ConceptId, u8, u32, &crate::graph::DepSet)> = Vec::new();
        for (pos, &c) in labels.iter().enumerate() {
            let (role, filler, kind, count) = match self.pool.get(c) {
                ConceptExpr::Some(r, f) => (*r, *f, 0, 1),
                ConceptExpr::All(r, f) => (*r, *f, 1, 0),
                ConceptExpr::Min(k, r, f) => (*r, *f, 2, *k),
                ConceptExpr::Max(k, r, f) => (*r, *f, 3, *k),
                _ => continue,
            };
            items.push((role, filler, kind, count, &n.label_deps[pos]));
        }
        // A `∀` or `≤n` alone never clashes: only a node that demands values does.
        if !items
            .iter()
            .any(|&(_, _, kind, _, _)| kind == 0 || kind == 2)
        {
            return None;
        }
        // #327: absorption turns a told `G ⊑ ∀p.DKey(U)` (and a data range, `⊤ ⊑ ∀p.D`)
        // into a role rule that fires at successors, so the `∀` never reaches this
        // node's label. Read the ones that hold here as universal filters, for the
        // roles this node demands values on.
        let no_deps = crate::graph::DepSet::new();
        if !(self.dkey_unguarded_rules.is_empty() && self.dkey_guarded_rules.is_empty()) {
            let mut demanded: Vec<RoleId> = items
                .iter()
                .filter_map(|&(role, _, kind, _, _)| match role {
                    Role::Named(rid) if kind == 0 || kind == 2 => Some(rid),
                    _ => None,
                })
                .collect();
            // #349: a demand on `q` is also one on each super-role of `q`, whose
            // role-rule universals must then be read too.
            if let Some(h) = hierarchy {
                for q in demanded.clone() {
                    for &p in h.super_roles(q) {
                        if !demanded.contains(&p) {
                            demanded.push(p);
                        }
                    }
                }
            }
            for &rid in &demanded {
                for &target in self.dkey_unguarded_rules.get(&rid).into_iter().flatten() {
                    items.push((Role::Named(rid), target, 1, 0, &no_deps));
                }
            }
            if !self.dkey_guarded_rules.is_empty() {
                for (pos, &c) in labels.iter().enumerate() {
                    if let ConceptExpr::Atomic(g) = self.pool.get(c)
                        && let Some(rules) = self.dkey_guarded_rules.get(g)
                    {
                        for &(rid, target) in rules {
                            if demanded.contains(&rid) {
                                items.push((Role::Named(rid), target, 1, 0, &n.label_deps[pos]));
                            }
                        }
                    }
                }
            }
        }
        for (role, filler, kind, count, dep_label) in items {
            // Data-property roles are never inverse; skip inverse roles.
            let rid = match role {
                Role::Named(rid) => rid,
                Role::Inverse(_) => continue,
            };
            match self.dkey_range_of(filler) {
                Some(CardRange::Int(iv)) => {
                    add_set(&mut int_by_role, rid, iv, kind, count, dep_label);
                }
                Some(CardRange::Str(fs)) => {
                    add_set(&mut str_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::Float(di)) => {
                    add_set(&mut float_by_role, rid, di, kind, count, dep_label);
                }
                Some(CardRange::Decimal(di)) => {
                    add_set(&mut decimal_by_role, rid, di, kind, count, dep_label);
                }
                Some(CardRange::Date(di)) => {
                    add_set(&mut date_by_role, rid, di, kind, count, dep_label);
                }
                Some(CardRange::DateTime(di)) => {
                    add_set(&mut datetime_by_role, rid, di, kind, count, dep_label);
                }
                Some(CardRange::IntSet(fs)) => {
                    add_set(&mut int_set_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::FloatSet(fs)) => {
                    add_set(&mut float_set_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::DecimalSet(fs)) => {
                    add_set(&mut decimal_set_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::DateSet(fs)) => {
                    add_set(&mut date_set_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::DateTimeSet(fs)) => {
                    add_set(&mut datetime_set_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::Bool(fs)) => {
                    add_set(&mut bool_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::IntUnion(u)) => {
                    add_set(&mut int_union_by_role, rid, u, kind, count, dep_label);
                }
                Some(CardRange::Uri(fs)) => {
                    add_set(&mut uri_by_role, rid, fs, kind, count, dep_label);
                }
                Some(CardRange::Double(di)) => {
                    add_set(&mut double_by_role, rid, di, kind, count, dep_label);
                }
                Some(CardRange::DoubleSet(fs)) => {
                    add_set(&mut double_set_by_role, rid, fs, kind, count, dep_label);
                }
                None => {}
            }
        }
        // #349: fold the data-property hierarchy into every non-integer bucket; the
        // integer buckets are folded together, over `IntUnion`, in the combined
        // check at the end, so a filter in one integer bucket narrows a demand in
        // another.
        if let Some(h) = hierarchy {
            lift_through_hierarchy(&mut str_by_role, h);
            lift_through_hierarchy(&mut float_by_role, h);
            lift_through_hierarchy(&mut decimal_by_role, h);
            lift_through_hierarchy(&mut date_by_role, h);
            lift_through_hierarchy(&mut datetime_by_role, h);
            lift_through_hierarchy(&mut float_set_by_role, h);
            lift_through_hierarchy(&mut decimal_set_by_role, h);
            lift_through_hierarchy(&mut date_set_by_role, h);
            lift_through_hierarchy(&mut datetime_set_by_role, h);
            lift_through_hierarchy(&mut bool_by_role, h);
            lift_through_hierarchy(&mut uri_by_role, h);
            lift_through_hierarchy(&mut double_by_role, h);
            lift_through_hierarchy(&mut double_set_by_role, h);
        }
        // Check integer bucket first (keeps the fast path byte-identical).
        for acc in int_by_role.values() {
            if integer_sat(acc.universal, &acc.mins, &acc.maxs) == CardSat::Unsat {
                return Some(acc.deps.clone());
            }
        }
        // Check string bucket.
        if let Some(deps) = first_unsat(str_by_role) {
            return Some(deps);
        }
        // Check float bucket (dense).
        if let Some(deps) = first_unsat(float_by_role) {
            return Some(deps);
        }
        // Check decimal bucket (dense).
        if let Some(deps) = first_unsat(decimal_by_role) {
            return Some(deps);
        }
        // Check date bucket (dense).
        if let Some(deps) = first_unsat(date_by_role) {
            return Some(deps);
        }
        // Check dateTime bucket (dense).
        if let Some(deps) = first_unsat(datetime_by_role) {
            return Some(deps);
        }
        // Check integer-oneof bucket (FiniteSet<i64>).
        for acc in int_set_by_role.values() {
            if card_sat::<FiniteSet<i64>>(acc.universal.clone(), &acc.mins, &acc.maxs)
                == CardSat::Unsat
            {
                return Some(acc.deps.clone());
            }
        }
        // Check float-oneof bucket (FiniteSet<OrdF64>).
        if let Some(deps) = first_unsat(float_set_by_role) {
            return Some(deps);
        }
        // Check decimal-oneof bucket (FiniteSet<Decimal>).
        if let Some(deps) = first_unsat(decimal_set_by_role) {
            return Some(deps);
        }
        // Check date-oneof bucket (FiniteSet<DateKey>).
        if let Some(deps) = first_unsat(date_set_by_role) {
            return Some(deps);
        }
        // Check dateTime-oneof bucket (FiniteSet<DateTimeKey>).
        if let Some(deps) = first_unsat(datetime_set_by_role) {
            return Some(deps);
        }
        if let Some(deps) = first_unsat(bool_by_role) {
            return Some(deps);
        }
        if let Some(deps) = first_unsat(double_by_role) {
            return Some(deps);
        }
        if let Some(deps) = first_unsat(double_set_by_role) {
            return Some(deps);
        }
        if let Some(deps) = first_unsat(uri_by_role) {
            return Some(deps);
        }
        // #334/#343: integer ranges, integer enumerations and integer interval unions
        // on one property are kept in separate buckets above, so `≥2 p.xsd:integer`
        // never met `∀p.{1}` or `∀p.([0,5] ⊔ [10,15])`. Count them together over
        // `IntUnion`, which embeds the other two exactly, for a property with any union
        // item or with items in two buckets (and, #349, for every integer property
        // when two of them are related by the hierarchy). The `int` and `int_set` per-bucket checks
        // ran first with their own (tighter) deps; unifying the buckets would attach
        // unrelated labels' deps to those clashes, so it stays a later refactor.
        let roles: std::collections::BTreeSet<RoleId> = int_by_role
            .keys()
            .chain(int_set_by_role.keys())
            .chain(int_union_by_role.keys())
            .copied()
            .collect();
        // #349: when two of the node's integer roles are related, a role with items
        // in one bucket can still clash through the other, so every integer role
        // takes part.
        let fold = hierarchy.filter(|&h| {
            let roles: Vec<RoleId> = roles.iter().copied().collect();
            any_related(&roles, h)
        });
        let mut combined: HashMap<RoleId, SetAcc<IntUnion>> = HashMap::new();
        for rid in roles {
            let ia = int_by_role.get(&rid);
            let sa = int_set_by_role.get(&rid);
            let ua = int_union_by_role.get(&rid);
            if fold.is_none() && ua.is_none() && (ia.is_none() || sa.is_none()) {
                continue;
            }
            let iv = |c: &Card<IntInterval>| Card::new(IntUnion::from_interval(c.range), c.n);
            let fs = |c: &Card<FiniteSet<i64>>| Card::new(IntUnion::from_points(&c.range), c.n);
            let mut acc = SetAcc {
                mins: Vec::new(),
                maxs: Vec::new(),
                universal: None,
                deps: crate::graph::DepSet::new(),
            };
            let restrict = |universal: &mut Option<IntUnion>, u: IntUnion| {
                *universal = Some(match universal.take() {
                    Some(prev) => prev.intersect(&u),
                    None => u,
                });
            };
            if let Some(a) = ia {
                acc.mins.extend(a.mins.iter().map(iv));
                acc.maxs.extend(a.maxs.iter().map(iv));
                if let Some(u) = a.universal {
                    restrict(&mut acc.universal, IntUnion::from_interval(u));
                }
                acc.deps = crate::deps::union(&acc.deps, &a.deps);
            }
            if let Some(a) = sa {
                acc.mins.extend(a.mins.iter().map(fs));
                acc.maxs.extend(a.maxs.iter().map(fs));
                if let Some(u) = &a.universal {
                    restrict(&mut acc.universal, IntUnion::from_points(u));
                }
                acc.deps = crate::deps::union(&acc.deps, &a.deps);
            }
            if let Some(a) = ua {
                acc.mins.extend(a.mins.iter().cloned());
                acc.maxs.extend(a.maxs.iter().cloned());
                if let Some(u) = &a.universal {
                    restrict(&mut acc.universal, u.clone());
                }
                acc.deps = crate::deps::union(&acc.deps, &a.deps);
            }
            combined.insert(rid, acc);
        }
        if let Some(h) = fold {
            lift_through_hierarchy(&mut combined, h);
        }
        let mut combined: Vec<(RoleId, SetAcc<IntUnion>)> = combined.into_iter().collect();
        combined.sort_unstable_by_key(|(rid, _)| *rid);
        for (_, acc) in combined {
            if card_sat::<IntUnion>(acc.universal, &acc.mins, &acc.maxs) == CardSat::Unsat {
                return Some(acc.deps);
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

/// Whether the main tableau's role matching (`edge_satisfies`, which drives
/// `∀`, `∃`, role rules and `≤n`) also consults the SIGNED role closure (#211,
/// `RUSTDL_TABLEAU_SIGNED_ROLES`, **default ON**, `=0` reverts).
///
/// Without it, an inclusion that crosses polarity through the hierarchy
/// (`SubObjectPropertyOf(ObjectInverseOf(:s) :p)`, an inverse chain, an
/// equivalence with an inverse) is invisible to the tableau: `IF(p)` with
/// `≥2` on such an `s` was `sat` with `incomplete: false` while `HermiT` and
/// Konclude say unsat. Sound by construction — every signed inclusion is
/// entailed — so it can only add matches.
#[must_use]
pub fn signed_role_edges_enabled() -> bool {
    std::env::var_os("RUSTDL_TABLEAU_SIGNED_ROLES").is_none_or(|v| v != "0")
}

/// Whether the main tableau's role-chain rule also fires with the dirty node
/// as the chain's MIDDLE (#213, `RUSTDL_TABLEAU_CHAIN_MIDDLE`, **default ON**,
/// `=0` reverts).
///
/// The rule only looked forward from a chain's head, and an edge completing a
/// chain below the head dirties only its own endpoints. So `h —r→ m` then
/// `m —r→ t` never derived `h —r→ t` for a transitive `r`, and
/// `Transitive(r)` + `A ⊑ ∃r.∃r.¬D ⊓ ∀r.D` was satisfiable on `sat` and on
/// classify with the wedge off (`incomplete: false`), while `HermiT` and
/// Konclude say unsat. Every derived edge is licensed by the chain axiom.
#[must_use]
pub fn chain_middle_enabled() -> bool {
    std::env::var_os("RUSTDL_TABLEAU_CHAIN_MIDDLE").is_none_or(|v| v != "0")
}

/// Whether the concrete-domain count follows the data-property hierarchy (#349,
/// `RUSTDL_DATA_COUNTING_ROLE_HIERARCHY`, **default ON**, `=0` reverts).
///
/// With `q ⊑ p` every `q`-value is a `p`-value. The count keyed demands, limits
/// and `∀` filters on the exact property, so `≤1 p.{1,2}` never counted values
/// asserted on `q`, a miss `HermiT` and Konclude both report as inconsistent. Under
/// the flag a `∀` on `p` also filters `q`, and a demand on `q`, narrowed by `q`'s
/// own filter, is also a demand on `p`. Both are entailed, so this only adds
/// clashes that hold.
#[must_use]
pub fn data_counting_role_hierarchy_enabled() -> bool {
    std::env::var_os("RUSTDL_DATA_COUNTING_ROLE_HIERARCHY").is_none_or(|v| v != "0")
}

/// Whether the search tries a `∀R` disjunct first at a node with few
/// `R`-successors (#288, `RUSTDL_FORALL_FIRST`, **default ON**, `=0` reverts).
///
/// A definition `D ≡ ∃R.C` leaves the disjunction `∀R.¬C ⊔ D` on every node.
/// Trying `D` first fires `D ⊑ ∃R.C`, whose fresh successor carries the same
/// disjunctions, so the model grows until blocking stops it. The `∀` disjunct
/// generates nothing. On a 6-axiom core of PMD core this took `sat` from
/// 3.9 CPU-s and 361 nodes to 0.01 CPU-s. It does push its filler into every
/// `R`-successor, so a node with more than three keeps the old order: without
/// that bound, hundreds of role assertions on one individual turned a 1.6 s
/// `consistent` into a budget `unknown`.
///
/// This changes only the order branches are tried in. That is sound, and on a
/// complete search the verdict is the same either way. It is not order-proof in
/// practice: budgets cut searches differently, and the `≤n` trial merge (#76)
/// commits to the first pair that does not clash at once, so a different order
/// can reach a different committed pair.
#[must_use]
pub fn forall_first_enabled() -> bool {
    std::env::var_os("RUSTDL_FORALL_FIRST").is_none_or(|v| v != "0")
}

/// Whether the search keeps branching in the completion-graph tree of its last
/// decision while that tree has open decisions (#377,
/// `RUSTDL_TREE_STICKY_BRANCHING=1`, **default OFF**).
///
/// A probe with `ABox` individuals has several trees: the test node's and one
/// per individual. With no edges between them they are independent, but the
/// selectors took the first open decision in node order, and nodes of the
/// trees interleave, so the decision stack did too. A clash in one tree
/// backjumps to that tree's own decision, which is right, but undoing to its
/// checkpoint also pops every decision the other tree made since, and that
/// tree then searches again from there. On `core-3250-b` (6 axioms and one
/// `ClassAssertion`) the test node's tree made over 630,000 decisions in 60 s,
/// where alone its whole search takes 55,329.
///
/// With the flag on, the selectors still scan in node order but prefer an open
/// decision whose node is in the tree of the last decision opened, so a tree is
/// finished before another is started. `sat Alkyl` on that core answers in
/// about 13 CPU-s. A fixed tree order was tried first and was worse: taking the
/// lowest root first finished near-empty individual trees before the test
/// node's, and lost 7 of 1,800 fuzz cases that node order decides at once.
///
/// This changes only which decision is taken next. Every option of every
/// decision is still tried, and a leaf is still `Sat` only when no selector
/// finds an open decision in any tree. That is sound, and on a complete search
/// the verdict is the same either way. Budgets and depth caps can cut a
/// different order at a different place.
#[must_use]
pub fn tree_sticky_branching_enabled() -> bool {
    std::env::var_os("RUSTDL_TREE_STICKY_BRANCHING").is_some_and(|v| v == "1")
}

/// Whether a `≤n R.C` disjunct is tried first at a node that has at most `n`
/// `R`-neighbours (#382, `RUSTDL_MAX_FIRST=1`, **default OFF**).
///
/// A definition `D ≡ A ⊓ =1 R.B` leaves the residual `¬A ⊔ D ⊔ ≤0 R.B ⊔ ≥2 R.B`
/// on every `A` node. Trying `D` first forces `D`'s own `=1` successors, and on
/// `ore_ont_3250`'s functional-group definitions every `CarbonAtom` node became
/// every group it could be, whose overlapping counts clashed deep in the tree.
/// A `≤n` the node already meets generates nothing, as a `∀` does (#288). Only
/// the order of options changes, so this is sound, and on a complete search the
/// verdict is the same.
#[must_use]
pub fn max_first_enabled() -> bool {
    std::env::var_os("RUSTDL_MAX_FIRST").is_some_and(|v| v == "1")
}

/// Whether every node counts as carrying `⊤` for the `∃`, `≥n` and `≤n` witness
/// tests, and the choose rule skips a `≤n R.⊤` (#382, `RUSTDL_TOP_FILLER=1`,
/// **default OFF**).
///
/// `⊤` holds everywhere, so counting every `R`-neighbour for `≤n R.⊤` is the
/// semantics. Before, a neighbour counted only once the choose rule had labelled
/// it `⊤`: its other option, `¬⊤ = ⊥`, always clashes, so the outcome was the
/// same, but each `⊤` label carried the choose decision's id, and every clash
/// downstream depended on it. On `ore_ont_3250` that decision was opened 9,446
/// times in 30 s. A branch id affects backjumping only in the frame that owns it,
/// and an exhausted choose frame returned exactly its `⊤` branch's clash deps
/// minus its own id, so dropping the frame narrows no dependency a clash rests
/// on.
///
/// It also enforces a `≤n R.⊤` the choose rule could not fire on: one first
/// interned by a query, whose body has no registered complement. And with fewer
/// `⊤` labels, subset blocking can fire on nodes it did not before, which only
/// suppresses generation.
///
/// Counting and skipping the choose belong together: skipping the choose alone
/// leaves `≤n R.⊤` unenforced, a false `Sat`.
#[must_use]
pub fn top_filler_enabled() -> bool {
    std::env::var_os("RUSTDL_TOP_FILLER").is_some_and(|v| v == "1")
}

/// Whether the choice of which `≤n` witnesses to merge is a search branch
/// (#293, `RUSTDL_MAX_CHOICE_POINT`, **default ON**, `=0` reverts).
///
/// The `≤`-rule is don't-know nondeterministic over the pair. `apply_max`
/// trial-merged pairs inside saturation and committed to the first one that
/// did not clash at once (#76), so a pair that clashed only later read as
/// `Unsat` with no other pair tried: `A ⊑ ≤2 s.C` with witnesses `C ⊓ ∀p.X`,
/// `C ⊓ ∃p.¬X` and a third, harmless one was reported unsatisfiable. With this
/// on, `apply_max` merges only a forced pair (exactly one candidate) and leaves
/// two or more to `search`, which branches over them and backtracks.
#[must_use]
pub fn max_choice_point_enabled() -> bool {
    std::env::var_os("RUSTDL_MAX_CHOICE_POINT").is_none_or(|v| v != "0")
}

/// #370 `RUSTDL_MERGE_EXACT_DEPS` (**default OFF**, `=1` enables): exact
/// dependency sets for `≤n` merges. Each distinctness mark records the
/// decisions it depends on ([`TableauContext::mark_distinct_with_deps`]), and
/// two sites that took every active decision use them instead:
///
/// * P4, an exhausted merge frame (`search::Frame::merge`): the `≤n` label, the
///   witnesses (their edges and qualifier labels) and the marks among the
///   witnesses up to the pivot, which are what removed the other options;
/// * P7, `apply_max`'s forced merge when some witness pair is distinct (only
///   with [`max_choice_point_enabled`]: without it the merge is a pick among
///   pairs, which the marks do not justify).
///
/// Either conservative set made every clash below a `≤n` depend on every open
/// decision and turned back-jumping off above it: with both exact,
/// `ore_ont_10019`/`6485` `subclass SulfoxideGroup SulfinicAcidGeneralGroup` with
/// the wedge off, and the #288 fuzz case q0244, answer in under a second where
/// main stalls.
///
/// The `≥n` rule's all-witnesses path (`RUSTDL_MIN_FRESH_WITNESSES=0`) marks
/// existing witnesses distinct, which no dep set justifies (#304), so it keeps
/// every active decision.
#[must_use]
pub fn merge_exact_deps_enabled() -> bool {
    std::env::var_os("RUSTDL_MERGE_EXACT_DEPS").is_some_and(|v| v == "1")
}

/// Whether the `≥n R.C` rule counts existing witnesses only when `n` of them
/// are already pairwise distinct, and otherwise makes `n` fresh ones (#304,
/// `RUSTDL_MIN_FRESH_WITNESSES`, **default ON**, `=0` reverts).
///
/// `apply_min` used to count every existing `R`-neighbour carrying `C` and mark
/// up to `n` of them distinct, generating only the shortfall. Distinctness of
/// two existing witnesses is not entailed by `≥n R.C`, and a later `≤m R.D`
/// that had to merge them answered unsat for a class `HermiT` calls
/// satisfiable: `≥2 r.D1` marked two `D1`-carrying witnesses distinct, and the
/// only model merges them.
#[must_use]
pub fn min_fresh_witnesses_enabled() -> bool {
    std::env::var_os("RUSTDL_MIN_FRESH_WITNESSES").is_none_or(|v| v != "0")
}

/// Whether `apply_max`'s `⊥`, added when every candidate merge is rejected or
/// excluded, carries precise deps (#288, `RUSTDL_MAX_PRECISE_BOT`, **default
/// ON**, `=0` reverts): the `≤n` label's, the witnesses' edges and qualifier
/// labels, and the clash each rejected trial merge produced. A distinct pair
/// still adds every active decision, since `mark_distinct` records no deps.
/// The old set, every active decision, stopped back-jumping at each such `⊥`.
#[must_use]
pub fn max_precise_bot_enabled() -> bool {
    std::env::var_os("RUSTDL_MAX_PRECISE_BOT").is_none_or(|v| v != "0")
}

/// Whether a forced `≤n` merge in `apply_max` keeps the older node (#288,
/// `RUSTDL_MAX_MERGE_KEEP_OLDER`, **default ON**, `=0` reverts).
///
/// `apply_max` merged the later witness in neighbour order into the earlier
/// one. Neighbour order lists successors before predecessors, so a functional
/// role reached through an inverse merged the node's predecessor into a fresh
/// successor. Re-parenting the predecessor's children then made one of them a
/// root, which is never blocked, and on PMD core `sat PMD_0010100` generated
/// nodes without end where v0.4.40 answered in 1.2 s. The newer node (higher
/// index) is now merged into the older one, as the standard `≤`-rule merges a
/// successor into its predecessor.
#[must_use]
pub fn max_merge_keep_older_enabled() -> bool {
    std::env::var_os("RUSTDL_MAX_MERGE_KEEP_OLDER").is_none_or(|v| v != "0")
}

/// Whether `search` re-runs `saturate` once more before answering `Sat`
/// (#294, `RUSTDL_SAT_LEAF_RESATURATE`, **default ON**, `=0` reverts).
///
/// A label added to a node does not dirty its neighbours, so within one
/// `saturate` call a rule that reads a neighbour's labels can miss a late one:
/// `A ⊑ ≤1 s.C` with two witnesses created as `C ⊓ …` counted no `C`-witness,
/// because `apply_and` ran on them after `A`. The next `saturate` marks every
/// node dirty and catches it, but a `Sat` leaf had no next call, so `sat`
/// answered `sat` for a class `HermiT` calls unsatisfiable. Costs one full pass
/// per `Sat` leaf.
#[must_use]
pub fn sat_leaf_resaturate_enabled() -> bool {
    std::env::var_os("RUSTDL_SAT_LEAF_RESATURATE").is_none_or(|v| v != "0")
}

/// Whether `saturate` may skip nodes with no rule work (#288,
/// `RUSTDL_INCREMENTAL_SATURATE`, **default ON**, `=0` reverts).
///
/// A full call marks every node dirty and scans them in order. When the trail
/// shows a saturation fixpoint followed by label additions alone, a call
/// instead dirties those nodes, their neighbours and the nodes the last call
/// deferred, and visits nodes in the same order. Every mutation dirties its
/// nodes and their neighbours that the first scan has yet to reach, and defers
/// the rest to the next call, which is when a full pass would next visit them.
/// Rules other than blocking read at most a node and its neighbours, and a node
/// skipped as blocked stays deferred until a visit finds it unblocked (see
/// `TableauContext::note_blocked_skip`). So without a deadline the rules fire
/// in the same order as on the full path and the search makes the same
/// decisions; with one, less work per call can change which probes time out.
/// Every branch decision re-ran every rule on every node before this; on PMD
/// core that was most of the cost.
#[must_use]
pub fn incremental_saturate_enabled() -> bool {
    std::env::var_os("RUSTDL_INCREMENTAL_SATURATE").is_none_or(|v| v != "0")
}

/// Whether anywhere (pairwise) blocking is enabled for the main tableau.
///
/// Opt-IN: returns `true` only when `RUSTDL_ANYWHERE_BLOCKING=1`. Default
/// (unset / any other value) is `false` — the historical ancestor-scoped
/// pair blocking. Read ONCE per [`TableauContext`] at construction and cached
/// in the `anywhere_blocking` field; the hot `is_blocked` path never re-reads
/// the environment. The reasoner crate carries a mirror `*_enabled()` for
/// `RUSTDL_MAX_TRIAL_MERGE` — **default ON**; `=0` reverts.
///
/// Issue #76 (SOUNDNESS). The SROIQ `≤`-rule is don't-know nondeterministic
/// over WHICH pair of witnesses to merge. `apply_max` runs in the
/// deterministic saturation pass and used to commit to the first pair not
/// marked distinct; when that pair was the one inconsistent combination its
/// clash propagated as `Unsat`, yielding a FALSE-POSITIVE subsumption.
///
/// With this on, each candidate pair is merged behind a checkpoint and kept
/// only if the survivor is clash-free, rolling back otherwise. If no pair
/// survives, the pre-existing `Bot` arm still fires — sound, since satisfying
/// `≤n` requires SOME merge and every candidate is immediately contradictory.
///
/// PARTIAL by construction: a pair that clashes only deeper is still committed
/// to without a choice point. The complete fix is a backtrackable choice point
/// over merge pairs in `search.rs`, which branches over CONCEPTS today and so
/// needs a new branching primitive.
#[must_use]
pub fn max_trial_merge_enabled() -> bool {
    // Default-ON idiom: an EMPTY value enables.
    std::env::var_os("RUSTDL_MAX_TRIAL_MERGE").is_none_or(|v| v != "0")
}

/// discoverability alongside the other gate fns.
#[must_use]
pub fn anywhere_blocking_enabled() -> bool {
    std::env::var_os("RUSTDL_ANYWHERE_BLOCKING").is_some_and(|v| v == "1")
}

/// Deterministic live-node cap for the deadline-free saturate/search path.
/// `RUSTDL_MAX_NODES` (default 50000; `0` disables). Cached once (#35 v4).
#[must_use]
pub fn max_nodes() -> Option<usize> {
    use std::sync::OnceLock;
    static CAP: OnceLock<Option<usize>> = OnceLock::new();
    *CAP.get_or_init(|| match std::env::var("RUSTDL_MAX_NODES") {
        Ok(v) => match v.trim().parse::<usize>() {
            Ok(0) => None,
            Ok(n) => Some(n),
            Err(_) => Some(50_000),
        },
        Err(_) => Some(50_000),
    })
}

thread_local! {
    /// A per-thread replacement for [`max_nodes`], set by [`NodeCapOverride`]:
    /// `(active, cap)`, where `cap == None` disables the cap. A thread spawned
    /// for a search does not inherit it; the spawner passes
    /// [`effective_max_nodes`] on explicitly.
    static NODE_CAP_OVERRIDE: std::cell::Cell<(bool, Option<usize>)> =
        const { std::cell::Cell::new((false, None)) };
}

/// The live-node cap the current thread's searches use: the
/// [`NodeCapOverride`] in force, else [`max_nodes`].
#[must_use]
pub fn effective_max_nodes() -> Option<usize> {
    match NODE_CAP_OVERRIDE.with(std::cell::Cell::get) {
        (true, cap) => cap,
        (false, _) => max_nodes(),
    }
}

/// Live-node cap for a deadline-FREE single query (`sat`, `subclass`, `instance`
/// with no probe deadline), #363 A2. Such a query has no other bound, so the cap
/// is its only safety net, but at the 50000 default it cut ORE queries that
/// finish in about a second a little past it, and they now report `unknown`
/// rather than a silent `sat`. An explicit `RUSTDL_MAX_NODES` wins (tests and
/// users who set it keep exact control). Otherwise `RUSTDL_QUERY_MAX_NODES`
/// (default 500000; `0` disables). Cached once.
#[must_use]
pub fn query_max_nodes() -> Option<usize> {
    use std::sync::OnceLock;
    static CAP: OnceLock<Option<usize>> = OnceLock::new();
    *CAP.get_or_init(|| {
        if std::env::var_os("RUSTDL_MAX_NODES").is_some() {
            return max_nodes();
        }
        match std::env::var("RUSTDL_QUERY_MAX_NODES") {
            Ok(v) => match v.trim().parse::<usize>() {
                Ok(0) => None,
                Ok(n) => Some(n),
                Err(_) => Some(500_000),
            },
            Err(_) => Some(500_000),
        }
    })
}

/// Replaces the live-node cap for searches on this thread until dropped
/// (restoring the previous value). Used by the deadline-free single-query
/// paths with [`query_max_nodes`]; classify and every deadline-bounded path
/// never set it, so they keep [`max_nodes`].
#[must_use = "the override lasts only while the guard is alive"]
pub struct NodeCapOverride {
    prev: (bool, Option<usize>),
}

impl NodeCapOverride {
    /// Use `cap` (`None` = no cap) for searches on this thread.
    pub fn enter(cap: Option<usize>) -> Self {
        let prev = NODE_CAP_OVERRIDE.with(|c| c.replace((true, cap)));
        Self { prev }
    }
}

impl Drop for NodeCapOverride {
    fn drop(&mut self) {
        NODE_CAP_OVERRIDE.with(|c| c.set(self.prev));
    }
}

/// Nominals-first scheduling (#35 v4). **Default OFF** — dormant/deferred: a
/// bake-off found this scheduling did not bound the issue #35 target bug (the
/// validated fix is the realize pair-timeout + hard `NodeCap` safety net, gated
/// independently). Set `RUSTDL_NOMINAL_FIRST=1` to opt in to the deferred-A
/// experiment (unconditional ∃/≥ generation otherwise). Cached once.
#[must_use]
pub fn nominal_first_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| matches!(std::env::var("RUSTDL_NOMINAL_FIRST").as_deref(), Ok("1")))
}

/// Count inverse-induced (`preds`/flip) successors in `≤n`/functional merges
/// (sound; closes a completeness gap on inverse+functional cyclic patterns,
/// e.g. galen). **Default ON** since 2026-07-11 — the incremental
/// `horn_fixpoint` merge (rather than the old whole-graph re-fire) makes this
/// fast in practice (galen MISSED 10 → 1, ~840 ms; wine 19.78 s → 90 ms;
/// corpus FP=0). Set `RUSTDL_INVERSE_FUNC_MERGE=0` to revert to the old
/// (incomplete) behaviour. See docs/known-limitations.
/// Skip a Horn clause's body match when its single head atom is `Class(c, X)` and
/// the node already carries `c` (#128). `HyperNode::add` is keep-first, so every
/// binding the match could produce would resolve to `NoChange` — the skip is an
/// IDENTITY on both the verdict and the dep-sets, not an approximation.
/// **Default ON**; set `RUSTDL_HEAD_PRESENT_SKIP=0` to revert.
#[must_use]
pub fn head_present_skip_enabled() -> bool {
    std::env::var_os("RUSTDL_HEAD_PRESENT_SKIP").is_none_or(|v| v != "0")
}

/// Pin the new edge into edge-triggered matches (`RUSTDL_HYPER_EDGE_PIN`,
/// **default ON**, `=0` reverts). An `Event::Edge` fires every Horn clause with a
/// body atom on the edge's role as a full re-match. A census over 227 ORE
/// ontologies found 94–96% of those bindings do not use the new edge at all, so
/// they were already found when their last fact arrived. With the flag on, an
/// edge-triggered match enumerates only bindings in which the new edge occupies
/// one of the body's role-atom positions (in either orientation).
///
/// The roots are unchanged (the same four trigger tables decide where a clause is
/// fired), so this never finds a binding today's dispatch would not. It drops only
/// bindings that do not use the edge. Those that were missed by every trigger are
/// recovered by the #137 re-seed net before any `Sat`, as today.
///
/// With the pin on, that net is the backstop for those bindings, so
/// `RUSTDL_RESEED_VERIFY=0` is no longer a harmless performance knob: unpinned
/// matching recovered some of them incidentally, and pinned matching does not.
///
/// Flip evidence (two arms over all 1,920 ORE, `classify`, 60 s cap, one thread):
/// 1,752 of 1,753 both-completed ontologies answer identically, the one difference
/// being `ore_ont_12698`, which differs between runs of either arm; 3 time-outs
/// finish (`ore_ont_13647`, `2232`, `3562`); none goes the other way.
#[must_use]
pub fn hyper_edge_pin_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPER_EDGE_PIN").is_none_or(|v| v != "0")
}

#[must_use]
pub fn inverse_func_merge_enabled() -> bool {
    std::env::var_os("RUSTDL_INVERSE_FUNC_MERGE").is_none_or(|v| v != "0")
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
    fn pending_nominal_disjunction_tbox_and_materialized() {
        // #35 v4: the nominal-covering Or (`A ⊑ {x,y}`) is a DEFERRED
        // concept-rule conclusion — at generation time the node carries only
        // the atomic trigger, so the predicate must consult the TBox.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let x = pool.nominal(IndividualId::new(0));
        let y = pool.nominal(IndividualId::new(1));
        let one_of = pool.or([x, y]);
        // TBox: A ⊑ {x,y} as a concept-rule (trigger A, conclusion one_of).
        let mut tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: one_of,
            }],
            ..AbsorbedTBox::default()
        };
        tbox.finalize(); // builds concept_rules_by_trigger
        let hierarchy = RoleHierarchyBuilder::with_roles(0).build();
        let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, &tbox, &hierarchy);

        // Pending case: node has only atomic A -> Or not materialized ->
        // still pending.
        let n = ctx.new_node();
        ctx.add_label(n, a);
        assert!(
            ctx.has_pending_nominal_disjunction(n),
            "pending via TBox concept-rule"
        );

        // Materialized-open case.
        let n2 = ctx.new_node();
        ctx.add_label(n2, one_of);
        assert!(
            ctx.has_pending_nominal_disjunction(n2),
            "materialized open Or"
        );

        // Resolved: a disjunct present -> not pending.
        ctx.add_label(n2, x);
        assert!(!ctx.has_pending_nominal_disjunction(n2), "resolved");

        // Plain atomic with no covering rule -> not pending.
        let mut pool2 = ConceptPool::new();
        let b = pool2.atomic(ClassId::new(1));
        let mut ctx2 = TableauContext::new(&pool2);
        let n3 = ctx2.new_node();
        ctx2.add_label(n3, b);
        assert!(
            !ctx2.has_pending_nominal_disjunction(n3),
            "plain atomic, no TBox"
        );
    }

    #[test]
    #[ignore = "nominal-first deferred (A redesign); opt-in RUSTDL_NOMINAL_FIRST=1, run with --ignored"]
    fn min_defers_under_pending_nominal_tbox() {
        // #35 v4 Task 3: with nominals-first scheduling on, `apply_min`
        // (and `apply_exists`) must DEFER generation while the node still
        // carries a pending nominal-covering disjunction (here `A ⊑ {x,y}`
        // as a TBox concept-rule) — the o-rule merges the node into its
        // canonical nominal before it can spawn successors. Mechanical
        // check only; end-to-end validation is the Task 5 driver gate.
        //
        // NOTE (deferred, 2026-07-23): `RUSTDL_NOMINAL_FIRST` now defaults
        // OFF (nominal-first scheduling did not bound the issue #35 target
        // bug; the validated fix is the realize pair-timeout + hard NodeCap
        // safety net, independent of this flag). This test documents the
        // deferred-A behaviour and only passes with `RUSTDL_NOMINAL_FIRST=1`
        // set process-wide before the `OnceLock` initializes (run with
        // `--ignored` in a dedicated process, since `nominal_first_enabled`
        // is OnceLock-cached and shared with other tests in this binary).
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let c = pool.atomic(ClassId::new(1));
        let x = pool.nominal(IndividualId::new(0));
        let y = pool.nominal(IndividualId::new(1));
        let one_of = pool.or([x, y]);
        let min_2rc = pool.min(2, Role::Named(RoleId::new(0)), c);
        let mut tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: one_of,
            }],
            ..AbsorbedTBox::default()
        };
        tbox.finalize();
        let hierarchy = RoleHierarchyBuilder::with_roles(1).build();
        let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, &tbox, &hierarchy);
        let n = ctx.new_node();
        ctx.add_label(n, a);
        ctx.add_label(n, min_2rc);
        assert!(ctx.has_pending_nominal_disjunction(n), "precondition");

        let before = ctx.graph().len();
        let out = apply_min(&mut ctx, n);
        assert_eq!(out, RuleOutcome::NoChange, "≥-generation deferred");
        assert_eq!(ctx.graph().len(), before, "no successor generated");
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

    /// #373: a `¬∃r.Self` clash rests on the self-edge as well as the label, so
    /// the `⊥` carries the edge's deps. With only the (deterministic) label's
    /// deps, the search back-jumped past the decision that made the edge.
    #[test]
    fn negative_self_clash_carries_the_self_edge_deps() {
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let self_r = pool.self_restriction(Role::named(r));
        let not_self_r = pool.not(self_r);
        let bot = pool.bot();
        let mut ctx = TableauContext::new(&pool);
        let n = ctx.new_node();
        ctx.add_label(n, not_self_r);
        ctx.add_edge_with_deps(n, r, n, &[7]);
        let _ = apply_self_restriction(&mut ctx, n);
        let deps = ctx.label_deps_of(n, bot).expect("clash");
        assert!(
            deps.binary_search(&7).is_ok(),
            "deps {deps:?} miss the edge's 7"
        );
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
        // pre-branch — even after `Frame::or` allocates a `branch_id`
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
    fn merge_reparents_a_child_moved_by_an_earlier_merge() {
        // `a —r→ c`; merge `a` into root `b` (c re-parented to b), then `b`
        // into root `d`. `b` never generated a child itself, so only the
        // re-parent in the first merge tells the second that `b` has one;
        // without it the second merge skips its scan and `c` keeps pointing
        // at the merged-away `b`.
        let pool = ConceptPool::new();
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(&pool);
        let a = ctx.new_node();
        let c = ctx.new_successor(a, r);
        let b = ctx.new_node();
        let d = ctx.new_node();
        assert!(ctx.merge_into(a, b));
        assert_eq!(ctx.graph.node(c).parent(), Some(b));
        assert!(ctx.merge_into(b, d));
        assert_eq!(ctx.graph.node(c).parent(), Some(d));
        assert_eq!(ctx.graph.blocking[c.index() as usize].parent, Some(d));
    }

    #[test]
    fn merging_childless_roots_leaves_other_parents_alone() {
        // The skip path: two ABox-style roots merge while an unrelated node
        // has a child. Nothing is re-parented.
        let pool = ConceptPool::new();
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(&pool);
        let p = ctx.new_node();
        let c = ctx.new_successor(p, r);
        let x = ctx.new_node();
        let y = ctx.new_node();
        assert!(ctx.merge_into(x, y));
        assert_eq!(ctx.graph.node(c).parent(), Some(p));
    }

    #[test]
    fn rollback_keeps_had_child_in_step_with_the_nodes() {
        // Nodes created after a checkpoint are truncated on rollback, and the
        // flag vector must shrink with them so a reused index starts clean.
        // Then redo the re-parent case on the reused indices.
        let pool = ConceptPool::new();
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(&pool);
        let root = ctx.new_node();
        let cp = ctx.checkpoint();
        let a = ctx.new_node();
        let _ = ctx.new_successor(a, r);
        let b = ctx.new_node();
        assert!(ctx.merge_into(a, b));
        ctx.rollback_to(cp);
        assert_eq!(ctx.graph.had_child.len(), ctx.graph.len());
        let a = ctx.new_node();
        let c = ctx.new_successor(a, r);
        let b = ctx.new_node();
        assert!(ctx.merge_into(a, b));
        assert!(ctx.merge_into(b, root));
        assert_eq!(ctx.graph.node(c).parent(), Some(root));
        assert_eq!(ctx.graph.had_child.len(), ctx.graph.len());
    }

    #[test]
    fn merge_into_does_not_create_parent_pointer_cycle() {
        // root —r→ s1 —r→ s2 —r→ s3. Merge s1 into s3: step 5 re-parents
        // s1's child s2 to s3 — but s3's ancestor chain already reaches
        // s2 (s3.parent = s2), so pointing s2.parent = s3 would close a
        // cycle s2 ⇄ s3. Pre-fix this happened and `is_blocked` looped
        // and tripped the `parent-pointer cycle` debug_assert (the SIO
        // closure-diff panic). The fix breaks the back-edge instead, so
        // the ancestor walk must terminate without panicking.
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
        let s3 = ctx.new_successor(s2, r);
        ctx.add_label(s3, a);
        ctx.merge_into(s1, s3);
        // The fix breaks the back-edge: re-parenting s2 → s3 would cycle
        // (s3.parent = s2 already), so s2 becomes a root instead. Pre-fix
        // s2.parent was set to s3, producing the s2 ⇄ s3 cycle that made
        // `is_blocked` loop into the `parent-pointer cycle` debug_assert
        // (the SIO closure-diff panic). This assert fails without the fix.
        assert_eq!(
            ctx.graph.node(s2).parent(),
            None,
            "cycle-forming re-parent must break the back-edge (s2 → root)"
        );
        // And the ancestor walk terminates (no panic) for every node.
        let _ = ctx.is_blocked(s3);
        let _ = ctx.is_blocked(s2);
        let _ = ctx.is_blocked(root);
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

// ── Phase 3b canaries ────────────────────────────────────────────────────────
//
// Canary 1 (`phase3b_inverse_heavy_classification_completes`) asserts that a
// synthetic with 5 declared inverse pairs remains satisfiable. Passes pre-fix
// (the linear scan and the future hashset both return the correct boolean).
//
// Canary 2 (`phase3b_inverse_pair_fast_path_consulted`) is gated behind
// `#[cfg(feature = "counters")]`. It references a counter field
// (`inverse_pair_fast_hits`) that does NOT yet exist on `RuleCounters`.
//
// Pre-T3: fails to compile under `--features counters` with
//   "no field `inverse_pair_fast_hits` on type `RuleCounters`"
// That compile failure IS the gap T3 closes. Without `--features counters`
// this block is never compiled, so `cargo test -p owl-dl-tableau` (default)
// stays clean.

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod phase3b_canaries {
    use super::*;
    use owl_dl_core::{
        AbsorbedTBox, ClassId, ConceptPool, ConceptRule, IndividualId, Role, RoleHierarchyBuilder,
        RoleId,
    };

    // ── Synthetic inverse-heavy fixture ─────────────────────────────────────
    //
    // Shape:
    //   - class A, B, C (ClassId 0, 1, 2)
    //   - roles r1..r5  (RoleId 0..4),  inverses s1..s5 (RoleId 5..9)
    //   - 5 InverseObjectProperties declarations: (r1,s1)..(r5,s5)
    //   - A ≡ Max(1, Inverse(s1), ⊤) ⊓ Some(Named(r1), B)
    //
    // When `apply_max` fires on A's root node it iterates its neighbours.
    // The `Some(Named(r1), B)` label forces one `Named(r1)` edge; `apply_max`
    // then calls `edge_satisfies(Named(r1), Inverse(s1))` which is
    // cross-polarity — the only branch that calls `are_declared_inverses`.
    // With exactly one such neighbour and Max(1, ...) the count never
    // exceeds 1, so no merge/clash fires and A is satisfiable.
    //
    // SIO has 84 inverse-pair declarations (Vec ~168 entries); this synthetic
    // pushes past the pizza-era "0-3 pairs" assumption with 5 pairs so the
    // new data-structure path is exercised. Verdict unchanged: A is consistent.
    struct InverseHeavySynth {
        pool: ConceptPool,
        a_expr: ConceptId,
        r1: RoleId,
        s1: RoleId,
        // r2..r5, s2..s5 are declared to reach 5 pairs but not used in axioms.
        r2: RoleId,
        s2: RoleId,
        r3: RoleId,
        s3: RoleId,
        r4: RoleId,
        s4: RoleId,
        r5: RoleId,
        s5: RoleId,
    }

    fn build_inverse_heavy_synth() -> InverseHeavySynth {
        let mut pool = ConceptPool::new();

        let _a_cls = ClassId::new(0);
        let b_cls = ClassId::new(1);
        let _c_cls = ClassId::new(2);

        let r1 = RoleId::new(0);
        let r2 = RoleId::new(1);
        let r3 = RoleId::new(2);
        let r4 = RoleId::new(3);
        let r5 = RoleId::new(4);
        let s1 = RoleId::new(5);
        let s2 = RoleId::new(6);
        let s3 = RoleId::new(7);
        let s4 = RoleId::new(8);
        let s5 = RoleId::new(9);

        let top = pool.top();
        let b = pool.atomic(b_cls);

        // A ≡ Max(1, Inverse(s1), ⊤) ⊓ Some(Named(r1), B)
        let max_inv_s1 = pool.max(1, Role::inverse(s1), top);
        let some_r1_b = pool.some(Role::named(r1), b);
        let a_expr = pool.and([max_inv_s1, some_r1_b]);

        InverseHeavySynth {
            pool,
            a_expr,
            r1,
            s1,
            r2,
            s2,
            r3,
            s3,
            r4,
            s4,
            r5,
            s5,
        }
    }

    // ── Canary 1: verdict preservation ──────────────────────────────────────
    //
    // A must be satisfiable: exactly one Named(r1) neighbour carries ⊤, and
    // Max(1, Inverse(s1), ⊤) counts it as 1 (≤ 1 = n) — no merge, no clash.
    // PASSES pre-fix and must CONTINUE to pass after T3 swaps in the hashset.
    #[test]
    fn phase3b_inverse_heavy_classification_completes() {
        let InverseHeavySynth {
            pool,
            a_expr,
            r1,
            s1,
            r2,
            s2,
            r3,
            s3,
            r4,
            s4,
            r5,
            s5,
        } = build_inverse_heavy_synth();

        let mut ctx = TableauContext::new(&pool);
        ctx.declare_inverse_pair(r1, s1)
            .declare_inverse_pair(r2, s2)
            .declare_inverse_pair(r3, s3)
            .declare_inverse_pair(r4, s4)
            .declare_inverse_pair(r5, s5);

        let result = ctx.is_satisfiable(a_expr);
        assert_eq!(
            result,
            Some(true),
            "A should be satisfiable under the inverse-heavy synthetic \
             (5 declared inverse pairs, Max(1,Inv(s1),⊤) + Some(Named(r1),B))"
        );
    }

    // ── Canary 2: inverse-pair fast-path is consulted (structural) ───────────
    //
    // This canary references `ctx.counters.inverse_pair_fast_hits` — a field
    // that does NOT yet exist on `RuleCounters`.
    //
    // Pre-T3: fails to compile under `--features counters` with
    //   "no field `inverse_pair_fast_hits` on type `RuleCounters`"
    // Post-T3 (once the field + hashset-contains logic are wired in): the
    // test compiles, runs, and asserts that at least one call to
    // `are_declared_inverses` consulted the new hashset.
    //
    // Without `--features counters` this block is never compiled, so
    // `cargo test -p owl-dl-tableau` (default) stays clean.
    #[cfg(feature = "counters")]
    #[test]
    fn phase3b_inverse_pair_fast_path_consulted() {
        let InverseHeavySynth {
            pool,
            a_expr,
            r1,
            s1,
            r2,
            s2,
            r3,
            s3,
            r4,
            s4,
            r5,
            s5,
        } = build_inverse_heavy_synth();

        let mut ctx = TableauContext::new(&pool);
        ctx.declare_inverse_pair(r1, s1)
            .declare_inverse_pair(r2, s2)
            .declare_inverse_pair(r3, s3)
            .declare_inverse_pair(r4, s4)
            .declare_inverse_pair(r5, s5);

        let _ = ctx.is_satisfiable(a_expr);

        // After classification, `apply_max` must have called
        // `edge_satisfies(Named(r1), Inverse(s1))` (cross-polarity), which
        // calls `are_declared_inverses(r1, s1)`. T3 bumps
        // `inverse_pair_fast_hits` on every `contains` call in the hashset
        // path. If the counter stays 0 the fast-path is silently dead code.
        let hits = ctx.counters.inverse_pair_fast_hits.get();
        assert!(
            hits > 0,
            "inverse-pair fast-path counter never bumped; \
             are_declared_inverses isn't consulting the new hashset. hits = {hits}"
        );
    }

    // ----- Anywhere (pairwise) blocking -------------------------------------

    #[test]
    fn anywhere_gate_defaults_off() {
        // With RUSTDL_ANYWHERE_BLOCKING unset (the test default) a fresh
        // context must report ancestor-only blocking. We don't poke the
        // environment (racy under parallel tests); we only assert the cached
        // field's *default* matches the opt-in helper's view. The helper is
        // false unless the env is exactly "1".
        assert!(
            !anywhere_blocking_enabled() || std::env::var_os("RUSTDL_ANYWHERE_BLOCKING").is_some()
        );
        let pool = ConceptPool::new();
        let ctx = TableauContext::new(&pool);
        assert_eq!(ctx.anywhere_blocking(), anywhere_blocking_enabled());
    }

    /// Build: roots p1, p2; then a `Named(r)`-successor of p2 (call it `xprime`,
    /// created FIRST so it has the smaller index), then a `Named(r)`-successor
    /// of p1 (call it `y`). `y` is NOT a tree-ancestor of `xprime` and vice
    /// versa — they are in disjoint subtrees. Labels are set so the pairwise
    /// conditions (3) `L(y) ⊆ L(xprime)` and (4) `L(p1) ⊆ L(p2)` hold.
    /// Returns `(ctx, y, xprime)` so individual tests can perturb labels.
    fn anywhere_disjoint_subtrees(
        pool: &ConceptPool,
        a: ConceptId,
        b: ConceptId,
    ) -> (TableauContext<'_, 'static, 'static>, NodeId, NodeId) {
        let r = RoleId::new(0);
        let mut ctx = TableauContext::new(pool);
        ctx.set_anywhere_blocking(true);
        let p1 = ctx.new_node();
        let p2 = ctx.new_node();
        // xprime created before y (smaller index → valid earlier blocker).
        let xprime = ctx.new_successor(p2, r);
        let y = ctx.new_successor(p1, r);
        // (4): L(p1) ⊆ L(p2). Put `a` on both parents.
        ctx.add_label(p1, a);
        ctx.add_label(p2, a);
        ctx.add_label(p2, b);
        // (3): L(y) ⊆ L(xprime). Put `a` on y, {a,b} on xprime.
        ctx.add_label(y, a);
        ctx.add_label(xprime, a);
        ctx.add_label(xprime, b);
        (ctx, y, xprime)
    }

    #[test]
    fn anywhere_blocks_non_ancestor_earlier_node() {
        // (a) anywhere blocking fires where ancestor-only would not.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let (mut ctx, y, xprime) = anywhere_disjoint_subtrees(&pool, a, b);
        assert!(
            y.index() > xprime.index(),
            "xprime must be the earlier node"
        );
        // Under anywhere blocking: blocked by the disjoint-subtree earlier node.
        assert!(ctx.is_blocked(y), "anywhere should block y by xprime");
        // Under ancestor-only: NOT blocked (xprime is not an ancestor of y).
        ctx.set_anywhere_blocking(false);
        assert!(
            !ctx.is_blocked(y),
            "ancestor-only must NOT block across disjoint subtrees"
        );
    }

    #[test]
    fn anywhere_does_not_block_when_parent_condition_fails() {
        // (b) inverse-role soundness guard: condition (4) L(p1) ⊆ L(p2) fails.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let extra = pool.atomic(ClassId::new(2));
        let (mut ctx, y, _xprime) = anywhere_disjoint_subtrees(&pool, a, b);
        // Add `extra` to L(p1) only — now L(p1) ⊄ L(p2), breaking (4).
        let p1 = ctx.graph().node(y).parent().expect("y has a parent");
        ctx.add_label(p1, extra);
        assert!(
            !ctx.is_blocked(y),
            "must not block when parent-label subset (cond 4) fails"
        );
    }

    #[test]
    fn anywhere_does_not_block_on_parent_role_mismatch() {
        // (b2) condition (2): parent_role must match.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let r = RoleId::new(0);
        let s = RoleId::new(1);
        let mut ctx = TableauContext::new(&pool);
        ctx.set_anywhere_blocking(true);
        let p1 = ctx.new_node();
        let p2 = ctx.new_node();
        let _xprime = ctx.new_successor(p2, s); // role s
        let y = ctx.new_successor(p1, r); // role r ≠ s
        ctx.add_label(p1, a);
        ctx.add_label(p2, a);
        ctx.add_label(p2, b);
        ctx.add_label(y, a);
        let xprime = NodeId::new(2);
        ctx.add_label(xprime, a);
        ctx.add_label(xprime, b);
        assert!(
            !ctx.is_blocked(y),
            "must not block when parent_role (cond 2) differs"
        );
    }

    #[test]
    fn anywhere_excludes_nominal_candidate() {
        // (b3) a candidate carrying a Nominal label is never a blocker.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let nom = pool.nominal(IndividualId::new(0));
        let (mut ctx, y, xprime) = anywhere_disjoint_subtrees(&pool, a, b);
        // Mark the candidate as nominal. (3) still holds: L(y)={a} ⊆ {a,b,nom}.
        ctx.add_label(xprime, nom);
        assert!(
            !ctx.is_blocked(y),
            "nominal candidate must be excluded as a blocker"
        );
    }

    #[test]
    fn anywhere_does_not_block_nominal_y() {
        // A nominal `y` (fixed individual) must not itself be blocked.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let nom = pool.nominal(IndividualId::new(0));
        let (mut ctx, y, _xprime) = anywhere_disjoint_subtrees(&pool, a, b);
        ctx.add_label(y, nom);
        assert!(!ctx.is_blocked(y), "a nominal node must never be blocked");
    }

    #[test]
    fn anywhere_excludes_merged_candidate() {
        // (b4) a redirected (merged-away) candidate is skipped.
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let (mut ctx, y, xprime) = anywhere_disjoint_subtrees(&pool, a, b);
        // Merge xprime into some other node so it becomes a redirect. Create a
        // fresh sink target (after y, so it is not itself a blocker for y).
        let sink = ctx.new_node();
        ctx.merge_into(xprime, sink);
        assert!(
            ctx.graph().node(xprime).is_redirected(),
            "xprime should be redirected after merge"
        );
        assert!(
            !ctx.is_blocked(y),
            "redirected candidate must be excluded as a blocker"
        );
    }

    #[test]
    fn anywhere_terminates_on_cyclic_existential() {
        // (c) A ⊑ ∃r.A — the canonical cyclic TBox. Under ancestor-only the
        // child blocks on its grandparent-chain ancestor; under anywhere the
        // SAME loop is caught (the repeated label set {A} on a same-role
        // successor of a same-label parent). Build the spine by hand and assert
        // the third generation is blocked under anywhere — termination follows
        // from blocking firing.
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let a = pool.atomic(ClassId::new(0));
        let mut ctx = TableauContext::new(&pool);
        ctx.set_anywhere_blocking(true);
        let n0 = ctx.new_node(); // root
        ctx.add_label(n0, a);
        let n1 = ctx.new_successor(n0, r);
        ctx.add_label(n1, a);
        let n2 = ctx.new_successor(n1, r);
        ctx.add_label(n2, a);
        // n2: parent n1 {A}, L(n2)={A}. Candidate n1: parent n0 {A}, L(n1)={A}.
        // (2) both Named(r); (3) {A}⊆{A}; (4) {A}⊆{A}. Blocked.
        assert!(
            ctx.is_blocked(n2),
            "cyclic existential must block under anywhere"
        );
    }

    #[test]
    fn anywhere_real_clash_stays_unsat() {
        // (d) Blocking must not hide a genuine clash. A ⊑ ∃r.(B ⊓ ¬B): the
        // generated r-successor carries {B, ¬B} → clash. The verdict must be
        // Unsat under BOTH ancestor-only and anywhere blocking (blocking only
        // suppresses GENERATION; the first witness is generated and clashes).
        let build = |anywhere: bool| -> search::SearchVerdict {
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
            let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, &tbox, &hierarchy);
            ctx.set_anywhere_blocking(anywhere);
            let x = ctx.new_node();
            ctx.add_label(x, a);
            search::search(&mut ctx, 64)
        };
        assert!(
            matches!(build(false), search::SearchVerdict::Unsat(_)),
            "ancestor-only must find the clash"
        );
        assert!(
            matches!(build(true), search::SearchVerdict::Unsat(_)),
            "anywhere must still find the clash"
        );
    }

    #[test]
    fn anywhere_sat_verdict_matches_ancestor() {
        // A satisfiable cyclic existential (A ⊑ ∃r.A) must be Sat under both
        // modes — anywhere must not over-block into a spurious Unsat, nor
        // under-block into non-termination. (Same verdict is the invariant.)
        let build = |anywhere: bool| -> search::SearchVerdict {
            let mut pool = ConceptPool::new();
            let a_cls = ClassId::new(0);
            let a = pool.atomic(a_cls);
            let r = RoleId::new(0);
            let some_r_a = pool.some(Role::named(r), a);
            let mut tbox = AbsorbedTBox {
                concept_rules: vec![ConceptRule {
                    trigger: a_cls,
                    conclusion: some_r_a,
                }],
                ..AbsorbedTBox::default()
            };
            tbox.finalize();
            let hierarchy = RoleHierarchyBuilder::with_roles(1).build();
            let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, &tbox, &hierarchy);
            ctx.set_anywhere_blocking(anywhere);
            let x = ctx.new_node();
            ctx.add_label(x, a);
            search::search(&mut ctx, 256)
        };
        assert_eq!(build(false), search::SearchVerdict::Sat);
        assert_eq!(build(true), search::SearchVerdict::Sat);
    }

    #[test]
    fn anywhere_does_not_block_root_or_when_strictly_earlier_absent() {
        // The earliest non-root successor cannot be blocked (no earlier
        // qualifying candidate exists). Roots are never blocked.
        let mut pool = ConceptPool::new();
        let r = RoleId::new(0);
        let a = pool.atomic(ClassId::new(0));
        let mut ctx = TableauContext::new(&pool);
        ctx.set_anywhere_blocking(true);
        let root = ctx.new_node();
        ctx.add_label(root, a);
        let first = ctx.new_successor(root, r);
        ctx.add_label(first, a);
        assert!(!ctx.is_blocked(root), "root is never blocked");
        assert!(
            !ctx.is_blocked(first),
            "the earliest successor has no earlier qualifying candidate"
        );
    }
}

/// Whether the wedge's open-rule scans (`find_open_disjunction`,
/// `find_open_at_most`) read the wall-clock deadline (`RUSTDL_OPEN_SCAN_DEADLINE`,
/// **default ON**, `=0` reverts).
///
/// Both scans visit every node and call `is_blocked` on each, so their cost grows
/// with the square of the graph, and before this they never read the clock. On
/// `ore_ont_15635`, once the classify consistency probe's graph reached ~247k
/// nodes, one scan ran 177 s past its budget. A passed deadline ends the scan and
/// the frame concludes `Stalled`, so this can only turn a late verdict into a
/// `Stalled`, never produce a `Sat` or an `Unsat`.
#[must_use]
pub fn hyper_open_scan_deadline_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_OPEN_SCAN_DEADLINE").is_none_or(|v| v != "0")
}

/// `RUSTDL_HYPER_MATCH_DEADLINE` — probe the deadline inside the wedge's match
/// cross-product. **Default ON** since v0.4.16; `=0` reverts.
///
/// Flipped because it repairs a CONTRACT VIOLATION, not because it recovers
/// ontologies: without it, `--pair-timeout-ms` and `--global-timeout-ms` are
/// silently unenforceable on this path. Measured on matched arms of one pinned
/// binary over the 400-ontology MISSED-net population: **ΔMISSED = +0, FP = 0**
/// (5194/0 both arms, 59 onts-with-MISSED both, and the 4 `arm_no_closure` drops
/// present in BOTH arms, so not attributable to the flag). Corpus sweep: **0
/// answer changes** over 1,747 both-arm completers. Wall, min-of-3 on an idle
/// host: neutral to marginally faster.
///
/// An earlier "+40%/+47% wall on two completers" reading was a MEASUREMENT
/// ARTIFACT — single runs taken under load. Clean min-of-3 gives `ore_ont_15491`
/// 62.8→62.0 s and `ore_ont_5617` 67.9→68.1 s. Label-heuristic prune rates are
/// *identical* between arms (13,469,242 / 16,428,289, misses=0), which also
/// refutes the "truncation starves the label cache" explanation offered for it.
///
/// `HyperEngine::solve` checks its deadline only at entry, so a frame whose work
/// stays inside `enumerate_matches` never re-consults it. Measured on
/// `ore_ont_16056`: two `classify_labels` calls ran ~17 s each against a 1 ms
/// label-cache budget, and `label_cache_build` was invariant (±0.3%) across ten
/// configurations because nothing could stop it. See
/// `docs/known-limitations/label-cache-build-unbounded.md`.
#[must_use]
pub fn hyper_match_deadline_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_HYPER_MATCH_DEADLINE").is_none_or(|v| v != "0")
}

/// Whether the `ABox`-seeded consistency engine lets its deadline, instead of the
/// fixed `horn_fixpoint` step cap, bound a deterministic drain
/// (`RUSTDL_SEEDED_FIXPOINT_UNCAPPED`, **default ON**, `=0` reverts).
///
/// On a graph with one node per individual the drain can need more than the cap's
/// 100,000 events, and the cap stopped it early with budget left. Only applied when
/// a deadline is set, so an unbudgeted run still terminates by step count. Read by
/// `owl-dl-reasoner`'s `ConsistencyCache::decide`; realize's witness model keeps the
/// cap.
#[must_use]
pub fn seeded_fixpoint_uncapped_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_SEEDED_FIXPOINT_UNCAPPED").is_none_or(|v| v != "0")
}

/// #362 `RUSTDL_NOMINAL_BLOCK_GUARD` (default ON, `=0` reverts), main tableau. Pair
/// blocking never blocks, and never blocks against, a node that is or stands for a
/// nominal individual: a node carrying a nominal (now also in ancestor blocking) or
/// a node counted by a `≤n R.C` at a nominal neighbour that is not over-full (the
/// case the SHOIQ NN-rule makes a nominal node). Unravelling a blocked copy of such a
/// node duplicates an edge into the nominal, so the copy can break the `≤n`: `sat`
/// missed `Q ⊑ ⊥` for `Q ⊑ ≥2 r.(∃s⁻.{a})` with `s` functional.
///
/// The same flag orders generation and merges, which the guard needs to terminate:
/// an `∃` into a nominal fires first and folds into the nominal's canonical node at
/// once; further `∃`/`≥n` at a node such a `≤` counts wait while it is over-full
/// (the SHOIQ priority of the `≤`/o-rules over the generating rules); and a `≤`
/// merge prunes the merged node's blockable subtree instead of reparenting it
/// ([`TableauContext::prune_subtree`]). Without the prune each merge left a child
/// nothing could block, which generated again without end.
///
/// A guard plus prune, not the NN-rule (no `≤m` guess, no new nominal nodes): a
/// nominal two edges away is still missed, and so is the wedge's half (a wedge
/// blocker merely next to an individual, #376), which default `consistent` trusts.
#[must_use]
pub fn nominal_block_guard_enabled() -> bool {
    std::env::var_os("RUSTDL_NOMINAL_BLOCK_GUARD").is_none_or(|v| v != "0")
}

/// #361 `RUSTDL_WEDGE_NOMINAL_BLOCK_GUARD` (default ON, `=0` reverts): in the wedge's
/// pair blocking, a node that carries a nominal a blocked copy could contradict (one
/// a clause head forces, or any when a `≤` counts predecessors) is never a blocker
/// for a representative node. A nominal node is one individual, so a blocked copy of
/// it would be a second element with the same nominal. The wedge had no such
/// exclusion (the main tableau's anywhere blocking does), and called
/// `{a} : ≥3 s.{a,b}` consistent: two `≥3` witnesses were blocked by a third that had
/// then merged into `a`. Inert on classify's `HyperCache`, which has no nominal
/// classes. Only the label half: a blocker that is merely next to a nominal under
/// predecessor counting (the SHOIQ NN-rule case) is not refused. The main
/// tableau's half is `RUSTDL_NOMINAL_BLOCK_GUARD` (#362); the wedge's is #376.
#[must_use]
pub fn wedge_nominal_block_guard_enabled() -> bool {
    std::env::var_os("RUSTDL_WEDGE_NOMINAL_BLOCK_GUARD").is_none_or(|v| v != "0")
}

/// #376 `RUSTDL_WEDGE_NN_GUARD` (default ON, `=0` reverts): in the wedge's pair
/// blocking, a would-be blocker that stands for an individual's `≤`-witness is
/// refused. A blocked copy of it would add one more witness to that individual,
/// which can break the `≤`: with `A ⊑ ∃s⁻.{a}`, `s` functional and two distinct `A`s,
/// the second `A` was blocked by the first, `a` saw one `s`-neighbour, and
/// `consistent` said consistent. The wedge half of the SHOIQ NN-rule case; the
/// main tableau's is `RUSTDL_NOMINAL_BLOCK_GUARD` (#362).
#[must_use]
pub fn wedge_nn_guard_enabled() -> bool {
    std::env::var_os("RUSTDL_WEDGE_NN_GUARD").is_none_or(|v| v != "0")
}

/// #355 `RUSTDL_MRV_REFUTED` (default ON, `=0` reverts): when MRV ordering is on, the
/// wedge branches first on the open disjunction with the fewest VIABLE disjuncts (live
/// and not already refuted at the binding), then the fewest live ones. Without it a
/// disjunction whose every disjunct clashes ties with a generating one (`Aryl ∨ Alkyl`)
/// and loses the first-found tie-break, and the search dives into the generator:
/// `subclass SulfoxideGroup SulfinicAcidGeneralGroup` on `ore_ont_10019` ran >300 s.
/// Reordering is FP-safe; under a budget or depth cap it can change which pairs are cut.
#[must_use]
pub fn hyper_mrv_refuted_enabled() -> bool {
    std::env::var_os("RUSTDL_MRV_REFUTED").is_none_or(|v| v != "0")
}

/// #371 `RUSTDL_MRV_GEN_LOCAL` (default ON, `=0` reverts; only matters with
/// `RUSTDL_MRV_REFUTED` on): an open disjunction whose only viable disjuncts are
/// generating (`∃`, `≥n`) keeps the viable-first rank only when the best candidate found
/// so far in the scan is at its own node (or there is none yet), and otherwise ranks by
/// live disjuncts as before #355. The scan is in node order, so this stops it from
/// jumping past a candidate at an earlier node.
/// On `ore_ont_7610` the viable-first rank picked `C ∨ ∃r.D` disjunctions with `C`
/// already refuted on ~240 nodes in a row and never got back to node 0, where the clash
/// is, so `consistent` ended `Stalled` at the depth cap instead of `Unsat` (5.2 G
/// instructions vs 0.6 G). The #355 wins keep a class disjunct and are unaffected.
/// Reordering is FP-safe; under a budget or depth cap it can change which pairs are cut.
#[must_use]
pub fn hyper_mrv_gen_local_enabled() -> bool {
    std::env::var_os("RUSTDL_MRV_GEN_LOCAL").is_none_or(|v| v != "0")
}

/// Whether the wedge's `HF4a` NN rule finds the other holder of a nominal through
/// an index instead of scanning every node (`RUSTDL_NN_INDEX`, **default ON**,
/// `=0` reverts).
///
/// The scan is O(nodes) per nominal label event. On an `ABox`-seeded graph that is
/// one node per individual, so the classify consistency probe spent ~45% of its
/// time there on `ore_ont_10073` (~28k nodes). The index returns the same node as
/// the scan: same filter, same lowest-index choice, checked against the scan by a
/// `debug_assert` on every lookup in debug builds.
#[must_use]
pub fn hyper_nn_index_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_NN_INDEX").is_none_or(|v| v != "0")
}

/// Whether a branch restore undoes the NN index by an undo log rather than
/// restoring a cloned copy (`RUSTDL_NN_UNDO_LOG`, **default ON**, `=0` reverts).
///
/// With the clone, every branch save copied the whole index, and an index first
/// built inside a branch was dropped by its restore, so the next branch paid the
/// build scan again. The log records each holder `add_label` pushes; a restore
/// pops the branch's entries and keeps an index built inside the branch, whose
/// lists are a superset of the restored holders (labels only grow along a
/// branch). Lookups answer the same either way: they filter by the current
/// labels, and debug builds check every lookup against the full scan.
#[must_use]
pub fn hyper_nn_undo_log_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_NN_UNDO_LOG").is_none_or(|v| v != "0")
}

/// Whether a branch save shares the engine's nodes copy-on-write instead of
/// deep-copying them (`RUSTDL_COW_NODES`, **default ON**, `=0` reverts).
///
/// The deep copy held every node's label lists once per live snapshot, so a deep
/// branch chain grew peak RSS linearly with depth: `ore_ont_3077` reached branch
/// level 221 over ~7,000 nodes and peaked near 1.2 GB. Shared, a snapshot costs
/// a pointer per node, and a write clones only the node it touches. Answers are
/// unchanged either way; only who owns a node's memory differs. Measured on
/// `ore_ont_3077`: peak RSS 1,498 → 157 MB. Two-arm sweep over 1,920 ORE: no answer
/// changes outside two budget-truncated ontologies (which agree at a 1 s per-pair budget),
/// and the worst serial CPU-time recheck is ×1.11 (`ore_ont_13964`, +0.14 s).
#[must_use]
pub fn hyper_cow_nodes_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_COW_NODES").is_none_or(|v| v != "0")
}

/// Whether `horn_fixpoint`'s drain loop consults the wall-clock deadline.
///
/// The loop is bounded by a `max_iters` STEP count only, so a fixpoint whose
/// events are individually expensive overruns its time budget. Measured on
/// `ore_ont_6134`: 19,906 pairs cost 100-999 ms against a 50 ms per-pair budget,
/// and a stride sweep of the sibling `enumerate_matches` probe left that bucket
/// flat, excluding the match cross-product.
///
/// Sound by reuse: it returns the same `Stalled` every caller already handles
/// from the `max_iters` branch. `Stalled` is never `Sat`, so truncating can only
/// MISS a subsumption, never manufacture one.
///
/// **Default ON since 2026-08-10** (`=0` reverts). It shipped OFF on 2026-08-08 as a
/// documented NO-GO: both gates passed but it was corpus-NEUTRAL (0 recoveries, 0
/// regressions, +0.5% wall over 1,920 ontologies), so there was no reason to pay for
/// it. What changed is not the measurement but the arrival of a CALLER that needs its
/// budget honoured — the classify consistency probe. With the flag off, that probe
/// overshoots a 100 ms budget by **66-80 s** on `ore_ont_1966` (80.28 s at 100 ms,
/// 66.34 s at 1000 ms, against a 5.48 s baseline); with it on, the same probe costs
/// **0.4-1.0 s at any budget** (5.89 s / 6.50 s). Localised by stack-sampling the
/// overshoot, which showed `owl_dl_tableau::hyper::*` — the wedge, not the main
/// tableau, correcting an earlier guess.
#[must_use]
pub fn hyper_fixpoint_deadline_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_FIXPOINT_DEADLINE").is_none_or(|v| v != "0")
}

/// Whether `horn_fixpoint` adapts how many events pass between two reads of the
/// clock (`RUSTDL_FIXPOINT_ADAPTIVE_STRIDE`, **default ON**, `=0` reverts to a
/// fixed 256).
///
/// The fixed stride bounds the deadline overshoot in EVENTS. On a large `ABox`
/// graph an event can cost 10-25 ms, so the classify consistency probe ran up to
/// ~3 s past its budget on `ore_ont_10073` (wedge build + decide 3.4-3.7 s against
/// a 1 s budget in 5 of 6 runs). The adaptive stride targets one clock read per
/// millisecond of drain work and climbs back to 256 when events are cheap.
/// Timing only: a step derives the same facts either way, and a deadline exit is
/// the same `Stalled` as before.
#[must_use]
pub fn hyper_fixpoint_adaptive_stride_enabled() -> bool {
    // Default-ON idiom (house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_FIXPOINT_ADAPTIVE_STRIDE").is_none_or(|v| v != "0")
}

/// #137 completeness net for the incremental Horn drain (`RUSTDL_RESEED_VERIFY`,
/// **default ON**; `=0` reverts). See `hyper::HyperEngine::horn_fixpoint`.
#[must_use]
pub fn reseed_verify_enabled() -> bool {
    std::env::var_os("RUSTDL_RESEED_VERIFY").is_none_or(|v| v != "0" && !v.is_empty())
}

/// Where the #137 net runs inside the wedge search (`RUSTDL_RESEED_AT_LEAF`,
/// **default ON**; `=0` reverts to re-seeding at every frame's fixpoint). ON: the
/// branching search skips the net at intermediate frames and runs it once where a
/// frame would conclude `Sat`. Missing facts can only make a branch look MORE
/// satisfiable, so deferring the net to the `Sat` point loses no verdict; it only
/// stops every branch frame from paying a full re-seed (5-7x CPU on
/// `ore_ont_4903` / `ore_ont_8911`). Intermediate frames still re-seed on an
/// exponential back-off (every frame after a recovery, doubling the gap after
/// each empty re-seed), which keeps the early pruning on searches that do have a
/// missing trigger. See `hyper::HyperEngine::solve_inner`.
#[must_use]
pub fn reseed_at_leaf_enabled() -> bool {
    std::env::var_os("RUSTDL_RESEED_AT_LEAF").is_none_or(|v| v != "0")
}
