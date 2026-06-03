# Konclude snapshot cache — Phase 3a recon plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure whether per-class `BackPropRisk` refinement (replacing the current ontology-wide classifier with a per-class one) would actually deliver wall savings on SROIQ workloads. Spec §6 Phase 3 acceptance: `ore-15672 ≤ 10× Konclude` (~17.5s wall). Recon validates this is reachable before committing 3-5 sessions to implementation.

**Architecture (recon-driven):** Extend the existing Phase 2a instrumentation to count per-class `BackPropRisk` classification on SROIQ fixtures. Three measurements:
1. **Per-class Safe ratio**: with a per-class classifier, how many of ore-15672's 82 classes / ore-10908's 692 classes would be Safe? (Ontology-wide is 0 of them today.)
2. **Snapshot path impact on the per-class-Safe subset**: project the wall savings if those classes could actually use the snapshot cache.
3. **Runtime sentinel abort rate**: estimate how often the sentinel would fire if we let "borderline" classes try the snapshot path. Spec §6's revert criterion is `aborts > 30% of attempts`.

**Tech Stack:** Rust 1.88+. No new deps. Pure instrumentation; revertable depending on outcome.

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §6 Phase 3.

**Predecessor:** Phase 2b shipped at `ba07b4e..b0b23c1`. SROIQ workloads (out-of-EL, currently Unsafe at ontology level) are unchanged from Phase 1c: ore-10908 5.28s, ore-15672 29.12s, pizza 3.47s. Konclude head-to-head:
- ore-10908: 3.05× Konclude (project's named target already met at Phase 8: ≤ 5×)
- ore-15672: 16.6× Konclude (spec §6 Phase 3 target: ≤ 10×)
- pizza: 2.04× Konclude (already good)
- sio-stripped (1585 classes, mixed): 58× Konclude (Phase 1c didn't measure precisely)

---

## Why recon-first

Phase 3 is the project's last queued work. Its target (ore-15672 ≤ 10× Konclude) is concrete but modest (~40% wall reduction). Implementation cost (3-5 sessions) is non-trivial. Dead-end §18 explicitly documents ore-15672's 3-class residual as **search-budget-bound, not rule-bound** — per-class snapshot reuse may not help those hard classes.

The recon answers two questions before commit:
1. **Is the structural argument viable?** What fraction of SROIQ classes would be Safe under per-class refinement?
2. **Is the upper bound meaningful?** Even if all per-class-Safe classes used snapshot, would that close the ore-15672 17× → 10× gap, OR is the hard-class cost the dominant component?

The recon's job is to answer with measurement before we commit to implementation.

---

## File structure (this plan)

**Modified files (temporary instrumentation):**
- `crates/owl-dl-tableau/src/snapshot.rs` — add a per-class variant of `BackPropRisk::classify_ontology` (e.g., `classify_class(class_id, internal)`). Phase 3a only USES it for diagnostic counting; Phase 3b (if green-lit) wires it into `SnapshotCache::try_replay`.
- `crates/owl-dl-reasoner/src/lib.rs` — count per-class Safe/Unsafe ratio on PreparedOntology::from_internal; expose via stats.
- `crates/owl-dl-reasoner/src/classify.rs` — add `per_class_safe_count: usize` + `per_class_unsafe_count: usize` to ClassificationStats; surface in CLI banner.
- `crates/owl-dl-cli/src/main.rs` — banner line for the new counts.

**New files (kept):**
- `docs/phase3a-recon.md` — go/no-go/dead-end recommendation.

If GO: Phase 3b implementation plan.
If NO-GO: dead-end §19 entry + close project.

---

### Task 1: Per-class `BackPropRisk::classify_class` + diagnostic counting

**Goal:** add a per-class variant of the existing ontology-wide classifier. Count Safe/Unsafe per-class for SROIQ fixtures.

**Files:**
- Modify: `crates/owl-dl-tableau/src/snapshot.rs` (new fn)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (count per-class at `PreparedOntology::from_internal`; expose)
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (stats fields)
- Modify: `crates/owl-dl-cli/src/main.rs` (banner)

- [ ] **Step 1: Add `BackPropRisk::classify_class`**

In `crates/owl-dl-tableau/src/snapshot.rs`, alongside the existing `classify_ontology`, add:

```rust
impl BackPropRisk {
    /// Phase 3a recon: per-class variant of [`Self::classify_ontology`].
    /// Walks the structural reachability graph from `class_id` and
    /// returns `Unsafe` iff some construct reachable from this class's
    /// axioms touches an inverse role / nominal / cardinality. Returns
    /// `Safe` otherwise.
    ///
    /// More expensive than `classify_ontology` (must walk per-class
    /// signature) but enables per-class snapshot dispatch on
    /// SROIQ workloads where the ontology-wide classifier is Unsafe.
    ///
    /// Phase 3a uses this for diagnostic counting only; Phase 3b
    /// (if green-lit) wires it into `SnapshotCache::try_replay`.
    #[must_use]
    pub fn classify_class(
        class_id: owl_dl_core::ir::ClassId,
        internal: &owl_dl_core::ontology::InternalOntology,
    ) -> Self {
        // Implementer judgment: the existing classify_ontology walks
        // every axiom + every concept it references. For per-class
        // refinement, walk only axioms reachable from `class_id`'s
        // told-subsumers + the closure transitively. Use the existing
        // `concept_uses_inverse_role` / `concept_matches` helpers (same
        // structural recursion); only the entry-point axioms differ.
        //
        // Simplest correct first cut: filter axioms by "does this
        // axiom mention class_id (as sub or sup)". Conservative —
        // may flag Unsafe when a class transitively touches a risk
        // construct via a not-immediately-reachable axiom. Acceptable
        // for recon. Phase 3b can refine.

        use owl_dl_core::ontology::Axiom;
        for ax in &internal.axioms {
            if !axiom_mentions_class(ax, class_id) {
                continue;
            }
            // existing scan logic from classify_ontology, scoped
            // to just this axiom
            if let Axiom::InverseObjectProperties(_, _) = ax {
                return Self::Unsafe {
                    reason: UnsafeReason::InverseRoleReachable,
                };
            }
            // ... etc per the existing helpers
        }
        Self::Safe
    }
}

fn axiom_mentions_class(
    ax: &owl_dl_core::ontology::Axiom,
    class_id: owl_dl_core::ir::ClassId,
) -> bool {
    // Walk axiom_concept_ids (already defined in this file); for each
    // concept, check if it transitively references class_id.
    // Conservative: if any axiom concept's reachable closure includes
    // class_id, the axiom is "about" this class.
    //
    // Implementer judgment on exact reachability:
    // - SubClassOf(class_id, _) and SubClassOf(_, class_id) trivially.
    // - EquivalentClasses containing class_id.
    // - DisjointClasses containing class_id.
    // - More distant: class_id appearing inside a concept expression
    //   in some axiom's body. Implementer chooses whether to include
    //   these (more accurate but slower) or skip (faster, possibly
    //   over-flags Unsafe).
    todo!("implementer: walk axiom_concept_ids + check ClassId presence; document the chosen reachability depth")
}
```

The `todo!()` is intentional — the recon spec leaves the exact reachability shape to the implementer. Document the choice in code comments. **First-cut recommendation**: trivial direct mentions only (SubClassOf/EquivalentClasses/DisjointClasses where class_id is one of the operands). This over-flags Unsafe (conservative) but is cheap and gives a baseline for the recon.

- [ ] **Step 2: Count per-class in PreparedOntology::from_internal**

In `crates/owl-dl-reasoner/src/lib.rs`, locate `PreparedOntology::from_internal` (Phase 1c stable). After the `snapshot_cache = ...` initialization, add diagnostic counting:

```rust
let (per_class_safe_count, per_class_unsafe_count) = {
    let n = internal.vocabulary.num_classes();
    let mut safe = 0usize;
    let mut unsafe_count = 0usize;
    for i in 0..n {
        let cid = owl_dl_core::ir::ClassId::new(u32::try_from(i).expect("fits"));
        match owl_dl_tableau::BackPropRisk::classify_class(cid, &internal) {
            owl_dl_tableau::BackPropRisk::Safe => safe += 1,
            owl_dl_tableau::BackPropRisk::Unsafe { .. } => unsafe_count += 1,
        }
    }
    (safe, unsafe_count)
};
```

Thread these through `PreparedOntology` fields:
```rust
pub(crate) per_class_safe_count: usize,
pub(crate) per_class_unsafe_count: usize,
```

And expose via pub(crate) accessors so classify_top_down_internal can read them.

- [ ] **Step 3: Add stats fields**

In `crates/owl-dl-reasoner/src/classify.rs`, near the Phase 2a recon fields, add:

```rust
/// Phase 3a recon: count of classes that `BackPropRisk::classify_class`
/// would mark Safe (per-class variant). Compare to total class count
/// to assess Phase 3 per-class-refinement upside.
pub per_class_safe_count: usize,
/// Phase 3a recon: count of classes that `BackPropRisk::classify_class`
/// would mark Unsafe.
pub per_class_unsafe_count: usize,
```

Populate from `prepared.per_class_safe_count()` etc. in classify_top_down_internal (or wherever stats are assembled).

- [ ] **Step 4: CLI banner**

In `crates/owl-dl-cli/src/main.rs`'s `write_classification`, after the existing wall-breakdown banner, add:

```rust
println!(
    "# per-class BackPropRisk: safe={} unsafe={} (Phase 3a recon)",
    s.per_class_safe_count,
    s.per_class_unsafe_count,
);
```

- [ ] **Step 5: Smoke check on alehif + ore-15672**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo build --release -p owl-dl-cli 2>&1 | tail -3
./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/alehif-test.ofn 2>&1 | grep "per-class BackPropRisk"
./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/ore-15672-shoin.ofn 2>&1 | grep "per-class BackPropRisk"
```

Expected: each shows a `safe / unsafe` split. alehif is Horn (already short-circuited by Phase 2b — won't reach the per-class counting if classify exits early; if so, run with `RUSTDL_HORN_SHORTCIRCUIT=0`). ore-15672 is the load-bearing SROIQ measurement.

- [ ] **Step 6: Soundness gate**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary 2>&1 | tail -5
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tail -10
```

Expected: 4/4 canary + 3/3 closure-diff all FP=0/MISSED=0. Pure instrumentation; no behavior change.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-tableau/src/snapshot.rs \
        crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/src/classify.rs \
        crates/owl-dl-cli/src/main.rs
git commit -m "$(cat <<'EOF'
recon(snapshot): per-class BackPropRisk diagnostic counting (Phase 3a T1)

Adds BackPropRisk::classify_class (per-class variant of the
existing ontology-wide classifier) and counts safe/unsafe per-
class at PreparedOntology::from_internal. Surfaces via CLI banner.

Phase 3a uses this for diagnostic counting only; Phase 3b (if
green-lit) would wire classify_class into SnapshotCache::try_replay
to enable per-class snapshot dispatch on SROIQ ontologies that are
currently ontology-wide Unsafe.

First-cut classify_class reachability: trivial direct mentions
(SubClassOf/EquivalentClasses/DisjointClasses where class_id is an
operand). Conservative (over-flags Unsafe) but cheap. Phase 3b
can refine if green-lit.

Temporary instrumentation — kept or removed based on Phase 3a
recon outcome.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §6 Phase 3

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Measure SROIQ fixtures + write recon doc

**Goal:** run the instrumented build on SROIQ fixtures; extract per-class Safe/Unsafe distribution; project Phase 3 savings; recommend GO / NO-GO / DEAD-END.

**Files:**
- Create: `docs/phase3a-recon.md`

- [ ] **Step 1: Run instrumentation on SROIQ fixtures**

```bash
mkdir -p /tmp/p3a
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH

for fixture in ore-15672-shoin ore-10908-sroiq pizza family-stripped ore-15516-alchoiq sio-fp2-module; do
  echo "=== $fixture ===" | tee -a /tmp/p3a/sroiq-perclass.log
  ./target/release/rustdl classify --pair-timeout-ms 200 \
    ontologies/external/$fixture.ofn 2>&1 | \
    grep -E "^# (classes|fragment|per-class BackPropRisk|wall breakdown)|wall=" | \
    tee -a /tmp/p3a/sroiq-perclass.log
  echo "" | tee -a /tmp/p3a/sroiq-perclass.log
done
```

Also run sio-stripped if available (1585 classes; longer run).

### Step 2: Compute per-class Safe ratios

For each SROIQ fixture, extract:
- Total classes
- Per-class Safe count
- Per-class Unsafe count
- Safe ratio = safe / total

Then compute the projected wall savings assuming Phase 3 enables snapshot on the Safe subset:
- Current wall (Phase 1c reference): from existing matrix.
- Snapshot-cost-per-pair on the Safe subset (estimated from Phase 1b.5's wedge cost ~1-2ms per call).
- Number of (Safe-sub, *) pair reductions if snapshot can short-circuit.

If Safe ratio < 50% on every SROIQ fixture, Phase 3's upside is bounded — half the classes still pay full per-pair cost.

If Safe ratio > 80% on ore-15672, Phase 3 could meaningfully reduce its wall.

### Step 3: Write recon doc

Create `docs/phase3a-recon.md`:

```markdown
# Phase 3a recon — does per-class BackPropRisk refinement pay off on SROIQ?

Run 2026-06-XX at HEAD `<sha>`. Recon decides whether Phase 3
(per-class BackPropRisk + runtime sentinel safety net) is the right
next investment, or whether the project's natural endpoint is
Phase 2b.

## Headline

<one-sentence GO / NO-GO / DEAD-END>

## Per-class Safe ratios (Phase 3a instrumentation)

| Fixture | Classes | Safe | Unsafe | Safe ratio | Current wall | Konclude ratio |
|---|---:|---:|---:|---:|---:|---:|
| ore-15672-shoin | 82 | <s> | <u> | <pct>% | 29.12s | 16.6× |
| ore-10908-sroiq | 692 | <s> | <u> | <pct>% | 5.27s | 3.05× |
| pizza | 99 | <s> | <u> | <pct>% | 3.47s | 2.04× |
| family-stripped | 58 | <s> | <u> | <pct>% | 27.41s | ~ |
| sio-fp2-module | 74 | <s> | <u> | <pct>% | 0.43s | flat |

## Break-even projection

<analysis: if Safe ratio is X% and per-pair cost is Y ms,
projected wall savings = N seconds vs current wall...>

## Spec §6 Phase 3 acceptance target

ore-15672 ≤ 10× Konclude = 17.5s. Current 29.12s. Gap = 11.6s.
Phase 3 needs to deliver ~40% wall reduction on ore-15672.

Recon's projection: <X seconds achievable> — sufficient / insufficient.

## Recommendation

<one of:>

**GO**: per-class Safe ratio on ore-15672 is <X>% — Phase 3
projected to bring ore-15672 wall to <Y>s, within the spec §6
≤17.5s target. Write Phase 3b implementation plan.

OR

**NO-GO (close project at Phase 2b)**: Per-class refinement
projected to deliver only <Y>s wall improvement on ore-15672 —
insufficient to close the 17× → 10× Konclude gap. Per dead-end
§18, ore-15672's residual is search-budget-bound, not rule-bound;
snapshot reuse on the Safe subset can't reach those hard classes.
Project's natural endpoint: Phase 2b. Headline wins (GALEN 400×,
notgalen 503×) are already shipped; SROIQ workloads are at
acceptable Konclude ratios (ore-10908 3.05× — well under named
5× target; pizza 2.04×). Write dead-end §19 + handoff doc.

## What's deferred to Phase 3b (if GO)

- Wire `classify_class` into `SnapshotCache::try_replay`.
- Runtime sentinel reliability work (per spec §6 revert criterion:
  abort rate > 30% triggers revert).
- Multi-fixture verification.

## Cross-references

- Project spec, Phase 2b results, dead-end §18, Phase 1c matrix.
```

Fill placeholders from real data.

### Step 4: Commit

```bash
git add docs/phase3a-recon.md
git commit -m "$(cat <<'EOF'
docs(phase3a): recon — does per-class BackPropRisk refinement help SROIQ?

Per-class Safe ratios on SROIQ fixtures: ore-15672 <pct>%,
ore-10908 <pct>%, pizza <pct>%. Projected Phase 3 wall savings
on ore-15672: <Y> seconds. Spec §6 Phase 3 target (≤10×
Konclude = 17.5s) <reachable/unreachable>.

Recommendation: <GO / NO-GO>.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Recon outcome determines next plan:

- **GO → Phase 3b implementation plan**: per-class classifier + sentinel reliability work + multi-fixture verification.
- **NO-GO → close project at Phase 2b**: write dead-end §19 (per-class refinement projected insufficient on SROIQ); handoff doc summarizing the full project arc; recognize Phase 2b as the natural endpoint.

The project has already shipped its named target (ore-10908 ≤ 5× Konclude, met at Phase 8 with 3.05× ratio); Phase 2b delivered 400-503× speedups on Horn workloads. Phase 3 is "incremental SROIQ polish" — recommend GO only if recon's projection clearly closes the spec §6 ore-15672 ≤ 10× gap.
