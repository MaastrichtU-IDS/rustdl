# Konclude-style global classification — design

**Date:** 2026-06-03
**Status:** approved (per brainstorming session 2026-06-03)
**Companion:** `docs/handoff-2026-06-03.md` (project context),
`docs/hypertableau-dead-ends.md` §2 (the failed precursor),
`docs/model-caching-plan.md` §B (the HermiT-style sketch this builds on).

This is the **project-level** design record for closing the
orchestrator's O(N²) per-pair scaling. Multi-month project; this doc
covers the architecture across all phases.

**Sub-project decomposition for implementation plans.** A single
implementation plan covers one shippable slice. This project is
intentionally decomposed into plan-sized slices:

| Plan | Scope | Sessions | Source spec |
|---|---|---|---|
| 1 | Phase 0 + Phase 1a (snapshot data + capture + risk classifier; no replay; no behavior change) | 5-8 | this doc |
| 2 | Phase 1b (replay driver + sentinel + flag-gated wiring) | 4-6 | this doc |
| 3 | Phase 1c (default-on + measure + accept/revert) | 1-2 | this doc |
| 4+ | Phase 2/3 each get their own brainstorm → spec → plan cycle, informed by Phase 1c recon | varies | future spec |

The first writing-plans call after this spec ships covers **Plan 1
only** (Phase 0 + Phase 1a). Subsequent plans branch off as each
prior phase lands and recon informs the next.

---

## 1. Goal & non-goals

### Goal

Close the orchestrator's O(N²) per-pair scaling by building per-class
completion graphs once and replaying sup queries onto them with lazy
expansion. Target: GALEN/notgalen/sio-stripped from 50–530× Konclude
down to ≤10× ratio, while preserving FP=0 + MISSED=0 across the corpus
and keeping glass-box axiom-justifications structurally feasible.

### Non-goals

- **Axiom-justification** (a Pellet/HermiT-style "minimal axiom set
  entailing C ⊑ D" subsystem) is not built here. We only preserve the
  door: data-flow supports a future `DepSet → AxiomDep` extension
  without retrofit.
- **Saturator provenance recording** is not built. Layer 2 (global
  saturation filter) is a filter, not a proof carrier.
- **`hyper.rs` rewrite from scratch** — we extend its `decide` path
  with snapshot/replay hooks.
- **Data-property frontend gap** — separate queued work. Snapshot
  capture treats datatype-bearing classes as Unsafe until the
  datatype frontend is wired (`crates/owl-dl-datatypes`).
- **Improving small-N workloads** (the 9 fixtures already at ≤2× Konclude
  ratio per `docs/perf-2026-06-03-konclude-vs-rustdl.md`). They stay flat.

### Ship/revert criterion

If Phase 1 (snapshot+replay, default-on) does not move GALEN to
≤ 150 s wall (~3× current 445 s) AND notgalen to ≤ 400 s (~3× current
~1170 s) after recon-driven tuning, **revert per the
[model-caching-plan §A](../../model-caching-plan.md) criterion** — a
partial cache that doesn't move the headline is the same mistake the
§2 dead-end recorded.

---

## 2. Architecture overview

Two layers compounding the existing engine; neither replaces it.

```
┌──────────────────────────────────────────────────────────────────┐
│ classify_top_down_internal (existing orchestrator, evolved)      │
│                                                                  │
│  Phase 7 label cache ──►  Phase 1 snapshot cache  ──►  fallback  │
│  (sound non-sub-prune)    (per-class graph reuse)      (current  │
│                            for verify-positives)        path)    │
│                                                                  │
│                                  ▲                               │
│                                  │ candidate pairs               │
│  Phase 2 global saturation  ─────┘                               │
│  candidate filter (later)                                        │
└──────────────────────────────────────────────────────────────────┘
```

### Layer 1: per-class snapshot cache (`SnapshotCache`)

Sibling to `HyperCache` in `crates/owl-dl-reasoner/src/lib.rs`. For
each class C in the top-down walk, capture the post-Sat completion
graph of `wedge(C)` as a serializable `GraphSnapshot { nodes, edges,
labels, derivation_meta }`. When the orchestrator probes
`subsumes(C, D)`, the verify path replays only `¬D`'s constraints onto
a fresh graph seeded from the snapshot, with **lazy expansion** —
re-derive a snapshot node's outgoing rules only when D-injected
constraints touch it.

Existing `hyper.rs` engine handles the replay; new code is the snapshot
capture + the seeding/replay driver.

### Layer 2: global saturation candidate filter (Phase 2)

Gated `RUSTDL_GLOBAL_SAT_FILTER` (default OFF until Phase 2 recon
validates). A single TBox-wide saturation pass over `(class, label)`
pairs that produces a candidate sup-set per class. The orchestrator's
pair loop only considers `(C, D)` pairs where D ∈ candidates(C).

Phase 2 design is deliberately incomplete here — the filter's shape
depends on what Layer 1 measurements reveal at the end of Phase 1c
(see §6 phasing). Section 5 below sketches the bounds; the detailed
spec is deferred to a successor design doc after Phase 1c recon.

### Soundness boundary (the §2 lesson)

The snapshot is **only sound to reuse when ¬D cannot back-propagate
into the cached C-graph.** Back-propagation triggers:

- Inverse role constraints reaching root from a ¬D-introduced node
  (`∀R⁻.X`).
- Nominal coupling (`{a}` constraints merging cached nodes).
- Cardinality merges (`≤n R` forcing snapshot node merges).

Detection is **structural at C's signature**, not at replay time: if
C's signature touches any role with an inverse declared in O, or any
nominal/cardinality construct reachable from C, mark the snapshot as
`BackPropRisk::Unsafe` and **fall through to the current per-pair path**
for that C entirely. Conservative — the filter false-negatives (refusing
to cache safe-but-suspect classes), but never false-positives (never
claims a subsumption that doesn't hold).

This is *not* HermiT's actual approach (HermiT replays bidirectionally
with derivation reactivation); this is the **conservative Konclude-style**
approach: cache when provably safe, fall through when not. Empirically
validated by the Phase 7 evidence — GALEN/notgalen are Horn fragments
where back-prop never fires, so the cache will hit 100% (or near-100%);
pizza/ore-15672 are SROIQ where the fall-through path stays current
behavior.

### Glass-box justification preservation

The snapshot is the derivation graph for C; the replay records which
axioms fire for D. A future justification subsystem can union
`axioms(snapshot)` ∪ `axioms(replay_clash_chain)` to seed a justification.
This is the structural reason Layer 1 is the proof carrier and Layer 2
is filter-only — preserving the per-pair derivation trace is what keeps
glass-box justifications feasible.

---

## 3. Per-class snapshot data structure

Lives in `crates/owl-dl-tableau/src/snapshot.rs` (new file). Sibling to
`graph.rs` (the live `CompletionGraph`) but immutable + cheap-to-clone.

```rust
/// Captured satisfying completion graph for some seed concept C.
/// Soundly reusable as a *starting point* for `C ⊓ ¬D` probes,
/// subject to the BackPropRisk gate.
#[derive(Debug, Clone)]
pub struct GraphSnapshot {
    /// Snapshot nodes, in pre-merge ordering. Index = SnapshotNodeId.
    pub(crate) nodes: Vec<SnapshotNode>,
    /// Outgoing edges per node. edges[i] = role-successors of node i.
    pub(crate) edges: Vec<Vec<SnapshotEdge>>,
    /// Union-find resolution at snapshot time. Caller resolves before reading.
    pub(crate) merge_repr: Vec<SnapshotNodeId>,
    /// Per-node "fired rule keys" — what deterministic rules have already
    /// fired against this node's labels. Replay uses this to skip
    /// re-deriving anything already done.
    pub(crate) fired: Vec<RuleFingerprint>,
    /// The seed concept this snapshot witnesses satisfiability of.
    pub(crate) seed: ConceptId,
    /// Structural classification (drives the BackProp gate).
    pub(crate) risk: BackPropRisk,
}

#[derive(Debug, Clone)]
pub(crate) struct SnapshotNode {
    /// Sorted-deduped concept labels at this node.
    pub labels: Vec<ConceptId>,
    /// `birth_deps` from the live graph — propagated for future axiom-dep work.
    pub birth_deps: DepSet,
    /// Whether this node is the root (only one root per snapshot).
    pub is_root: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SnapshotEdge {
    pub role: RoleId,
    pub target: SnapshotNodeId,
    /// Sorted-deduped role labels for hierarchy lookups.
    pub role_labels: Vec<RoleId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackPropRisk {
    /// Provably safe: C's reachable signature has no inverse role,
    /// no nominal, no cardinality constraint. Cache freely.
    Safe,
    /// Replay may force back-propagation into snapshot nodes.
    /// Fall through to per-pair path for this seed.
    Unsafe { reason: UnsafeReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsafeReason {
    InverseRoleReachable,
    NominalReachable,
    CardinalityReachable,
    DatatypeReachable,
    /// Reserved: future structural concerns (functional roles,
    /// transitive-with-inverse, etc.)
    Other,
}

pub type SnapshotNodeId = u32;
pub type RuleFingerprint = u64; // bloom-hashed rule application
```

### Snapshot capture path

Added to `HyperEngine`:

```rust
pub fn satisfiability_snapshot(&self, seed: ConceptId) -> Option<GraphSnapshot>;
```

Builds in O(nodes × labels) at the end of a Sat verdict. Replaces
nothing — `satisfiability_labels` from Phase 7 stays as the cheaper
alternative used by the label cache. The label cache becomes a strict
subset of snapshot data (`labels[root_index]`).

### Size sanity-check

GALEN at 2748 classes × estimated ~80 nodes/snapshot × ~40 labels/node
× 4 bytes = ~36 MB. Within budget. notgalen scales similarly. SROIQ
workloads with Unsafe seeds skip capture entirely → no memory cost.

### Risk classification

`BackPropRisk::classify(seed, prepared) -> BackPropRisk` runs at
snapshot-capture time. Walks C's structural signature via the existing
`InternalOntology` told-subsumers + `role_hierarchy` to determine
reachability. Cheap (signature size, not graph size). Results memoized
per class.

### Cross-thread safety

`GraphSnapshot: Send + Sync`. Cache is
`Arc<DashMap<ClassId, Arc<GraphSnapshot>>>` (matches the `Arc<DashMap>`
pattern already used by `HyperCache` per the model-caching-plan
§Concurrency).

### Justification hook (forward-looking, not built here)

Each `SnapshotNode` carries `birth_deps: DepSet`. When `DepSet` is
later extended to carry axiom-source ids, the snapshot will
automatically carry them through. No retrofit needed.

---

## 4. Replay algorithm + soundness contract

This is the heart. The §2 dead-end killed the previous attempt because
soundness was hand-waved; this section is what makes Layer 1 worth
shipping.

### 4.1 Replay path

`subsumes_via_tableau(C, D)` is the existing per-pair function
([classify.rs:1249](../../../crates/owl-dl-reasoner/src/classify.rs#L1249)).
It tests satisfiability of `C ⊓ ¬D`. The new path:

```rust
fn subsumes_via_snapshot_replay(
    prepared: &PreparedOntology,
    snapshot: &GraphSnapshot,    // C-snapshot, gated Safe
    not_d: ConceptId,             // already ¬D
) -> Option<SubsumesVerdict> {
    debug_assert!(matches!(snapshot.risk, BackPropRisk::Safe));

    // Step 1: Seed a fresh CompletionGraph from snapshot. Mass-import
    // nodes, edges, labels, fired-rule fingerprints. No rule firing yet.
    let mut graph = CompletionGraph::seeded_from(snapshot);
    let root = graph.snapshot_root();

    // Step 2: Add ¬D as a label at root, plus its birth_deps from
    // a fresh decision level (this is the "new query" decision).
    let query_dep = graph.fresh_decision();
    graph.push_label(root, not_d, DepSet::single(query_dep));

    // Step 3: Lazy expansion — fire only rules whose triggers were
    // injected by Step 2 OR by Step 2's downstream cascade. Snapshot
    // nodes never re-fire their already-fingerprinted rules.
    let mut driver = LazyReplayDriver::new(&mut graph, snapshot);
    match driver.run() {
        Outcome::Clash(_) => Some(SubsumesVerdict::Subsumed),
        Outcome::Sat => Some(SubsumesVerdict::NotSubsumed),
        Outcome::BackPropAborted => None,  // Step 4 below
    }
}
```

### Lazy expansion guard

Each `SnapshotNode` carries `fired: RuleFingerprint` — a 64-bit hash of
`(rule_id, label_set)` for the deterministic rules that fired during
the original C-saturation. When the replay considers re-firing a rule
on a snapshot node, it checks the fingerprint: if the rule's trigger
set hasn't changed since snapshot capture, skip (the rule's effects
are already in the snapshot). If `¬D`'s cascade *added* a label to a
snapshot node, the fingerprint shifts → re-fire.

This is the **work saving** of the cache: GALEN's typical C-graph
involves ~thousands of rule firings; if `¬D` only touches a handful of
nodes (the common case for non-back-prop classes), replay touches
dozens not thousands.

### 4.2 Soundness contract

Two invariants, each tested as `debug_assert!` in dev and as a corpus
diff in CI.

#### Inv-1 (the headline invariant)

If `subsumes_via_snapshot_replay(C, D) = Some(NotSubsumed)`, then
`subsumes_via_tableau(C, D) = NotSubsumed`.

Stated contrapositively: any pair the snapshot-replay accepts as a
counterexample must also be rejectable by the full per-pair tableau.
The snapshot is a *witness model* for `C` constructed by the trusted
Sat verdict; if `¬D` can be added to that witness without clash (under
lazy expansion that re-fires rules with shifted fingerprints), then
the extended graph is a witness model for `C ⊓ ¬D` → C is not subsumed
by D.

Proof sketch: lazy expansion preserves completeness on the *replay
subgraph* by definition (every rule whose trigger set changes
re-fires); the snapshot's untouched subgraph is unchanged since
C-saturation, so its completeness invariants still hold. Conjunction
→ the whole graph is rule-complete → no clash means Sat.

The critical precondition: ¬D's effects don't leak *into the snapshot's
untouched subgraph*. This is exactly what `BackPropRisk::Safe`
guarantees structurally (no inverse roles reaching back, no nominal
merges, no cardinality merges). If a snapshot was misclassified Safe,
leakage happens silently → MISSED. Defense in 4.3 handles this case.

#### Inv-2 (the corpus invariant)

Across the 19-fixture corpus diff suite,
`{snapshot-replay path verdicts} ⊆ {current-path verdicts}`.

Tested by gating the new path behind `RUSTDL_SNAPSHOT_REPLAY=1`
(default OFF until Phase 1c), running the existing
`tests/konclude_closure_diff.rs` with the flag on, and asserting
FP=0 + MISSED=0 unchanged vs. baseline. This catches `BackPropRisk`
classifier bugs that would have silently lost subsumptions.

### 4.3 BackPropAborted — defense in depth

Even with Inv-1 guaranteed, the replay driver carries a **runtime
back-prop sentinel**: any operation that would propagate a label
*into a snapshot node's labels via an inverse edge or merge* triggers
`Outcome::BackPropAborted`, which falls through to the per-pair
tableau path. Three reasons this is worth the code:

1. **Defense against `BackPropRisk` classifier bugs.** If the
   structural classifier wrongly marks a class Safe, the runtime
   sentinel still catches the propagation event before it produces a
   wrong Sat verdict. FP=0 holds even with a buggy classifier.
2. **Defense against future SROIQ extension.** When Phase 3 relaxes
   the classifier to `Safe-but-needs-runtime-check` for borderline
   classes, the sentinel becomes the soundness boundary instead of
   the structural classifier. (NB: "SROIQ extension" here means
   extending the snapshot's coverage of the SROIQ constructs we
   already reason over — inverse roles, nominals, cardinality. Datatype
   reasoning is separate; see Non-goals.)
3. **Observability.** Counter `snapshot_replay_aborts` lets us measure
   how often the sentinel fires — informs whether the structural
   classifier is too aggressive or too conservative.

The sentinel is cheap (one bit-flag on snapshot nodes + check on every
`push_label`/`merge_into`). Worth carrying.

---

## 5. Layer 2 — global saturation candidate filter

Short section because most decisions defer to mid-project recon — the
filter is a *Phase 2 lever*, not Phase 1 work, and its design will be
informed by what Layer 1 measurements reveal.

### What it does

Once per classify, run a single TBox-wide saturation pass that walks
every class C and produces `candidates(C) ⊆ classes` — the set of D
for which `C ⊑ D` is *not provably refuted by saturation alone*. The
orchestrator's pair loop then iterates only over `D ∈ candidates(C)`
instead of all classes.

### Why this helps on top of Phase 7's label cache

Phase 7 already prunes 89-100% of pairs at the `(C, D)` query site.
But: it builds a per-class wedge satisfiability call (cost: O(N) wedge
runs); the label cache fires *during* the orchestrator's walk. Layer 2
produces a one-shot ontology-wide filter cheaper than N wedge calls.
For GALEN the wedge-cache build is currently ~30% of the wall (per
Phase 7 results); replacing it with one global saturation run could
cut that further.

### Soundness

The filter produces a *superset* of true subsumptions — every actual
`C ⊑ D` survives the filter. Conservative under-filtering
(false-positives in candidates → extra verify work) is fine;
over-filtering (false-negatives, missing actual subsumptions) is the
soundness bug. Filter logic is "discharge by saturation closure" —
same engine, same soundness story as the existing `--saturation-only`
path which is already corpus-validated.

### Why this isn't designed in detail now

The shape of this filter depends on what Phase 1 measurements show:

- If Layer 1 lands GALEN at <100 s (≤50× Konclude), the global filter's
  marginal value is small and we may skip it.
- If Layer 1 lands GALEN at 150-300 s and recon shows the residual wall
  is in label-cache-build, the filter is the obvious next lever and its
  design becomes "replace the wedge-call-per-class with one
  saturation-over-TBox pass."
- If Layer 1 lands GALEN no better than current (~445 s), the project
  gets reverted per the §A criterion before we even get to Layer 2.

Gated as `RUSTDL_GLOBAL_SAT_FILTER` (default OFF until Phase 2 recon
validates).

---

## 6. Phasing & acceptance criteria

| Phase | Scope | Sessions | Acceptance | Revert criterion |
|---|---|---:|---|---|
| **0** | Spec (this doc) + plan + canary harness | 3-5 | Spec approved; subagent-driven plan exists; canary test gates FP=0 + MISSED=0 on snapshot path | n/a |
| **1a** | `GraphSnapshot` data structure + capture from `HyperEngine`; risk classifier; no replay yet | 2-3 | Snapshots build correctly; risk classifier matches structural analysis (unit tests); zero behavioral change in default classify | Memory/build cost > 30% of classify wall on GALEN |
| **1b** | `LazyReplayDriver` + `BackPropAborted` sentinel + wiring into `subsumes_via_tableau` behind `RUSTDL_SNAPSHOT_REPLAY=1` | 4-6 | Inv-1 + Inv-2 hold across Phase 0 net + GALEN; counter telemetry reports prune/replay/abort rates; no behavior change with flag OFF | FP=0 violated on any fixture; or aborts > 50% of attempts |
| **1c** | Default-on `RUSTDL_SNAPSHOT_REPLAY=1`; measure | 1-2 | GALEN ≤ 150 s wall AND notgalen ≤ 400 s AND no fixture regresses > 10% AND FP=0 + MISSED=0 unchanged | **§A revert: GALEN > 300 s after recon-driven tuning** — partial cache is worse than no cache |
| **2a** | Recon: is Layer 2 worth building? | 1 | Decision doc: skip / build / change-shape | n/a |
| **2b** | Global saturation filter (if green-lit by 2a) | 4-8 | Cuts label-cache build cost by ≥30% on GALEN; FP=0; no fixture regresses | Filter overhead > savings |
| **3** | Loosen `BackPropRisk` classifier for SROIQ workloads (ore-15672, pizza) using runtime sentinel | 3-5 | ore-15672 ≤ 10× Konclude; FP=0 | Sentinel abort rate > 30% → too aggressive, revert classifier change |

**Total: 18-32 sessions across phases 0-3.** Phase 1c is the first
shippable headline win (~10-15 sessions in). The §A revert at Phase 1c
is the budget-protection point.

### Phase 1c outcome bands

| GALEN wall | Decision |
|---|---|
| ≤ 150 s | **Ship + proceed to Phase 2a** (Layer 1 lever proven; Layer 2 incremental) |
| 150–300 s | **Ship + mandatory Phase 2 build** (Layer 1 partial; Layer 2 is the path to the headline target) |
| > 300 s after recon-driven tuning | **§A revert** — partial cache without headline movement is the dead-end §2 mistake; write dead-end §19 and close the project |

### Execution pattern

Subagent-driven development per phase (the pattern that shipped
Phases 7-8 cleanly): each phase gets its own spec → plan → subagent
dispatch with two-stage review. Recon docs between phases. Handoff doc
per session boundary.

### Soundness gates per phase

FP=0 + MISSED=0 on `tests/konclude_closure_diff.rs` runs in CI for
every phase landing commit. The flag-gated phases (1b, 2b) run the
soundness gate with the flag both ON and OFF — both must pass.

---

## 7. Acceptance summary (project-level)

**Must hold at end of every shipped phase:**

- FP=0 + MISSED=0 on all corpus fixtures with pinned Konclude
  closures (alehif, ORE-10908, ORE-15672, GALEN, notgalen).
- All in-tree tests pass (`cargo test --workspace`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  clean.
- No fixture wall regresses > 10% vs. **the project baseline pinned
  in `docs/perf-2026-06-03-konclude-vs-rustdl.md` Table § "Results —
  all 19 ontologies"** (post-Phase-8 walls, captured 2026-06-03).

**Project succeeds if:**

- GALEN ≤ 150 s wall (≤ 75× Konclude, ~3× current).
- notgalen ≤ 400 s wall (≤ 180× Konclude, ~3× current).
- sio-stripped ≤ 30 s wall (≤ 15× Konclude, ~4× current).
- Or some combination thereof, with corpus-wide regressions held to
  the ≤ 10% bound.

**Project gets reverted if:**

- Layer 1 alone (Phase 1c) doesn't move GALEN ≤ 300 s after
  recon-driven tuning.
- OR Inv-1 or Inv-2 violated on any fixture at any phase.

Each revert is recorded in `docs/hypertableau-dead-ends.md` so the
work survives.

---

## 8. Cross-references

- Project context: `docs/handoff-2026-06-03.md`.
- Failed precursor: `docs/hypertableau-dead-ends.md` §2.
- HermiT-style sketch this builds on: `docs/model-caching-plan.md` §B.
- Phase 7 label heuristic (partial-progress lever):
  `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`,
  `docs/phase7-results.md`.
- Phase 8 cache-deadline tuning: `docs/phase8-results.md`.
- Head-to-head context: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Architectural roadmap: `docs/architecture-roadmap.md`.
