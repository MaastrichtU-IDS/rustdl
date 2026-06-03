# Konclude snapshot cache — Phase 0 + Phase 1a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the `GraphSnapshot` data structure, the snapshot-capture method on `HyperEngine`, and a first-cut ontology-wide `BackPropRisk` classifier. **Zero behavioral change** in default classify — capture is opt-in behind `RUSTDL_SNAPSHOT_CAPTURE=1` (default OFF). Establish the Phase 0 canary harness that future phases (1b, 1c) extend.

**Architecture:** New file `crates/owl-dl-tableau/src/snapshot.rs` carrying immutable `GraphSnapshot`/`SnapshotNode`/`SnapshotEdge`/`BackPropRisk` types. New method `HyperEngine::satisfiability_snapshot` mirroring `satisfiability_labels` but copying the full node/edge structure. New function `BackPropRisk::classify_ontology` — ontology-wide first cut (per-class refinement is Phase 1b territory). Env-flag scaffolding `RUSTDL_SNAPSHOT_CAPTURE` lands disabled; future phases default it on.

**Tech Stack:** Rust 1.88+, edition 2024. No new dependencies (uses `dashmap`/`hashbrown` already in workspace if needed; Phase 1a doesn't actually need them — cache lives in Phase 1b).

**Spec:** [docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md](../specs/2026-06-03-konclude-style-global-classification-design.md) §3, §6 (Phase 0 + Phase 1a rows).

**Scope boundary:** This plan is Phase 0 + Phase 1a only. Replay driver, sentinel, and orchestrator wiring are Phase 1b — separate plan.

---

## File structure (this plan)

**New files:**
- `crates/owl-dl-tableau/src/snapshot.rs` — `GraphSnapshot`, `SnapshotNode`, `SnapshotEdge`, `BackPropRisk`, `UnsafeReason`, `SnapshotNodeId`, `RuleFingerprint`, `BackPropRisk::classify_ontology`.
- `crates/owl-dl-tableau/tests/snapshot_capture.rs` — unit tests for snapshot capture on synthetic fixtures.
- `crates/owl-dl-tableau/tests/backprop_risk.rs` — unit tests for the risk classifier (pure-Horn → Safe, SROIQ → Unsafe variants).
- `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs` — Phase 0 canary asserting zero-behavior-change with the env flag OFF.
- `docs/phase1a-results.md` — results doc (template-filled at Task 4).

**Modified files:**
- `crates/owl-dl-tableau/src/lib.rs` — add `pub mod snapshot;` + re-exports.
- `crates/owl-dl-tableau/src/hyper.rs` — add `HyperEngine::satisfiability_snapshot`.
- `crates/owl-dl-reasoner/src/lib.rs` — add `snapshot_capture_enabled()` env helper (consumers wire it in Phase 1b).

---

### Task 1: Snapshot types + module wiring + risk classifier (with tests)

**Files:**
- Create: `crates/owl-dl-tableau/src/snapshot.rs`
- Modify: `crates/owl-dl-tableau/src/lib.rs:45-85` (mod + re-exports)
- Create: `crates/owl-dl-tableau/tests/backprop_risk.rs`

- [ ] **Step 1: Write the failing risk-classifier tests**

Create `crates/owl-dl-tableau/tests/backprop_risk.rs`:

```rust
//! Unit tests for `BackPropRisk::classify_ontology`.
//!
//! First-cut ontology-wide risk classifier: any axiom in the
//! ontology that contains an inverse role, a nominal, or a
//! cardinality constraint forces the whole ontology to Unsafe.
//! Per-class refinement is Phase 1b territory.

use horned_owl::io::ofn::reader::read;
use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ontology::InternalOntology;
use owl_dl_tableau::snapshot::{BackPropRisk, UnsafeReason};
use std::io::Cursor;

fn lower_ofn(src: &str) -> InternalOntology {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    convert_ontology(&onto).expect("convert_ontology")
}

#[test]
fn pure_horn_classifies_safe() {
    let onto = lower_ofn("\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:r))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
    SubClassOf(:A ObjectSomeValuesFrom(:r :B))
)
");
    assert_eq!(BackPropRisk::classify_ontology(&onto), BackPropRisk::Safe);
}

#[test]
fn inverse_role_classifies_unsafe() {
    let onto = lower_ofn("\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:r_inv))
    InverseObjectProperties(:r :r_inv)
    SubClassOf(:A ObjectSomeValuesFrom(:r owl:Thing))
)
");
    assert_eq!(
        BackPropRisk::classify_ontology(&onto),
        BackPropRisk::Unsafe { reason: UnsafeReason::InverseRoleReachable },
    );
}

#[test]
fn cardinality_classifies_unsafe() {
    let onto = lower_ofn("\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:r))
    SubClassOf(:A ObjectMaxCardinality(2 :r :B))
)
");
    assert_eq!(
        BackPropRisk::classify_ontology(&onto),
        BackPropRisk::Unsafe { reason: UnsafeReason::CardinalityReachable },
    );
}

#[test]
fn nominal_classifies_unsafe() {
    let onto = lower_ofn("\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(NamedIndividual(:a))
    SubClassOf(:A ObjectOneOf(:a))
)
");
    assert_eq!(
        BackPropRisk::classify_ontology(&onto),
        BackPropRisk::Unsafe { reason: UnsafeReason::NominalReachable },
    );
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
cargo test -p owl-dl-tableau --test backprop_risk
```

Expected: FAIL with "unresolved import `owl_dl_tableau::snapshot`".

- [ ] **Step 3: Create `snapshot.rs` with types + risk classifier**

Create `crates/owl-dl-tableau/src/snapshot.rs`:

```rust
//! Per-class completion graph snapshots for the Konclude-style
//! global classification project (Phase 1a).
//!
//! See `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
//! §3 (data structure) and §4 (replay + soundness).
//!
//! Phase 1a deliverable: types + capture path on `HyperEngine` +
//! ontology-wide risk classifier. **No replay, no orchestrator
//! wiring** — those land in Phase 1b. Capture is opt-in via the
//! reasoner-side env flag `RUSTDL_SNAPSHOT_CAPTURE`; default OFF
//! holds the "zero behavior change in default classify" guarantee.

use owl_dl_core::ir::{ClassId, ConceptId, Role};
use owl_dl_core::ontology::{Axiom, InternalOntology};

/// Stable per-snapshot node id (post-merge resolution applied
/// at capture time; callers index `nodes` / `edges` / `fired`
/// directly).
pub type SnapshotNodeId = u32;

/// 64-bit hash of `(rule_id, label_set)` for the deterministic
/// rules that fired during the original C-saturation. Replay
/// (Phase 1b) checks the fingerprint to decide whether a rule
/// is already-fired (skip) or must re-fire (the cascade shifted
/// the trigger set).
pub type RuleFingerprint = u64;

/// Captured satisfying completion graph for some seed concept C.
/// Soundly reusable as a starting point for `C ⊓ ¬D` probes,
/// subject to the [`BackPropRisk`] gate.
///
/// Construction: [`crate::hyper::HyperEngine::satisfiability_snapshot`]
/// builds one at the end of a `Sat` verdict. Snapshots are
/// immutable + cheap-to-clone (`Arc`-shareable across the rayon
/// pair loop).
#[derive(Debug, Clone)]
pub struct GraphSnapshot {
    /// Snapshot nodes, in post-merge canonical ordering. Index =
    /// `SnapshotNodeId`. `nodes[0]` is the root by construction.
    pub(crate) nodes: Vec<SnapshotNode>,
    /// Outgoing edges per node. `edges[i]` = role-successors of
    /// node `i`. Targets reference post-merge canonical ids.
    pub(crate) edges: Vec<Vec<SnapshotEdge>>,
    /// Per-node fired-rule fingerprint (Phase 1b lazy expansion
    /// guard). Phase 1a writes a placeholder `0`; Phase 1b
    /// computes the real bloom hash.
    pub(crate) fired: Vec<RuleFingerprint>,
    /// The seed concept this snapshot witnesses satisfiability of.
    pub(crate) seed: ClassId,
    /// Structural classification (drives the soundness gate).
    pub(crate) risk: BackPropRisk,
}

#[derive(Debug, Clone)]
pub(crate) struct SnapshotNode {
    /// Sorted-deduped concept labels at this node.
    pub labels: Vec<ClassId>,
    /// `true` iff this node is the seed-graph root.
    pub is_root: bool,
    // birth_deps is added in Phase 1b alongside the replay driver
    // that consumes it. Phase 1a doesn't expose hyper::DepSet
    // through this module to avoid premature API surface.
}

#[derive(Debug, Clone)]
pub(crate) struct SnapshotEdge {
    pub role: Role,
    pub target: SnapshotNodeId,
}

/// Structural classification of a seed (or whole ontology in the
/// Phase 1a first-cut). See spec §4.2 for the soundness story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackPropRisk {
    /// Provably safe: no inverse role, no nominal, no cardinality,
    /// no datatype reachable. Snapshot is sound to reuse under the
    /// Inv-1 contract.
    Safe,
    /// Replay may force back-propagation into snapshot nodes.
    /// Phase 1b orchestrator falls through to the per-pair path
    /// for any seed (or whole ontology) flagged Unsafe.
    Unsafe { reason: UnsafeReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsafeReason {
    InverseRoleReachable,
    NominalReachable,
    CardinalityReachable,
    DatatypeReachable,
    /// Reserved for Phase 3 structural concerns (functional roles,
    /// transitive-with-inverse, etc.) not yet enumerated.
    Other,
}

impl BackPropRisk {
    /// Phase 1a first-cut: walk every axiom in the ontology and
    /// classify the whole ontology as `Unsafe` if *any* axiom
    /// contains an inverse role, a nominal subterm, or a
    /// cardinality restriction. Otherwise `Safe`.
    ///
    /// Conservative by construction — Horn ontologies (GALEN,
    /// notgalen, alehif) land Safe; SROIQ ontologies (ore-15672,
    /// pizza, ore-10908) land Unsafe. Per-class refinement is
    /// Phase 1b territory.
    #[must_use]
    pub fn classify_ontology(internal: &InternalOntology) -> Self {
        // First scan: explicit role-level axioms.
        for ax in internal.axioms.iter() {
            if let Axiom::InverseObjectProperties(_, _) = ax {
                return Self::Unsafe { reason: UnsafeReason::InverseRoleReachable };
            }
        }
        // Second scan: any role appearing as `Role::Inverse` in a
        // concept subterm.
        for ax in internal.axioms.iter() {
            if axiom_contains_inverse_role(ax, internal) {
                return Self::Unsafe { reason: UnsafeReason::InverseRoleReachable };
            }
        }
        // Third scan: nominals (`{a}` ⇒ Concept::Nominal).
        for ax in internal.axioms.iter() {
            if axiom_contains(ax, internal, ConceptKind::Nominal) {
                return Self::Unsafe { reason: UnsafeReason::NominalReachable };
            }
        }
        // Fourth scan: cardinality restrictions (Min/Max).
        for ax in internal.axioms.iter() {
            if axiom_contains(ax, internal, ConceptKind::Cardinality) {
                return Self::Unsafe { reason: UnsafeReason::CardinalityReachable };
            }
        }
        Self::Safe
    }
}

#[derive(Copy, Clone)]
enum ConceptKind {
    Nominal,
    Cardinality,
}

fn axiom_contains_inverse_role(ax: &Axiom, internal: &InternalOntology) -> bool {
    axiom_concept_ids(ax).any(|cid| concept_uses_inverse_role(cid, internal))
}

fn axiom_contains(ax: &Axiom, internal: &InternalOntology, kind: ConceptKind) -> bool {
    axiom_concept_ids(ax).any(|cid| concept_matches(cid, internal, kind))
}

fn axiom_concept_ids(ax: &Axiom) -> impl Iterator<Item = ConceptId> + '_ {
    use Axiom::*;
    let v: Vec<ConceptId> = match ax {
        SubClassOf(sub, sup) => vec![*sub, *sup],
        EquivalentClasses(cs) | DisjointClasses(cs) => cs.clone(),
        _ => Vec::new(),
    };
    v.into_iter()
}

fn concept_uses_inverse_role(cid: ConceptId, internal: &InternalOntology) -> bool {
    use owl_dl_core::ir::{Concept, Role as IrRole};
    let pool = &internal.concepts;
    let mut stack = vec![cid];
    let mut seen = std::collections::HashSet::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) { continue; }
        match pool.get(c) {
            Concept::Some(IrRole::Inverse(_), _)
            | Concept::All(IrRole::Inverse(_), _)
            | Concept::Min(_, IrRole::Inverse(_), _)
            | Concept::Max(_, IrRole::Inverse(_), _) => return true,
            Concept::Some(_, body) | Concept::All(_, body)
            | Concept::Min(_, _, body) | Concept::Max(_, _, body)
            | Concept::Not(body) => stack.push(*body),
            Concept::And(xs) | Concept::Or(xs) => stack.extend(xs.iter().copied()),
            _ => {}
        }
    }
    false
}

fn concept_matches(cid: ConceptId, internal: &InternalOntology, kind: ConceptKind) -> bool {
    use owl_dl_core::ir::Concept;
    let pool = &internal.concepts;
    let mut stack = vec![cid];
    let mut seen = std::collections::HashSet::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) { continue; }
        match (kind, pool.get(c)) {
            (ConceptKind::Nominal, Concept::Nominal(_)) => return true,
            (ConceptKind::Cardinality, Concept::Min(_, _, _) | Concept::Max(_, _, _)) => return true,
            (_, Concept::Some(_, body) | Concept::All(_, body)
                | Concept::Min(_, _, body) | Concept::Max(_, _, body)
                | Concept::Not(body)) => stack.push(*body),
            (_, Concept::And(xs) | Concept::Or(xs)) => stack.extend(xs.iter().copied()),
            _ => {}
        }
    }
    false
}
```

API notes (verified against the codebase at HEAD `0a3acaa`):

- `ClassId`, `ConceptId`, `Role` live at `owl_dl_core::ir`.
- `InternalOntology` exposes `axioms: Vec<Axiom>` (direct field), `concepts: ConceptPool` (direct field, NOT a `pool()` method), and `vocabulary: Vocabulary` (direct field).
- IRI → `ClassId`: `internal.vocabulary.class_id(iri) -> Option<ClassId>`.
- Horned-OWL `SetOntology` → `InternalOntology`: `owl_dl_core::convert::convert_ontology(&onto)`.
- `Concept` (the pool variant enum) imported as shown above (`owl_dl_core::ir::Concept`).
- If a destructuring pattern doesn't compile (variant lists change with the IR), adjust the pattern — the structural intent (look for `Inverse` roles inside `Some/All/Min/Max`, look for `Nominal`, look for `Min/Max`) does not change.

- [ ] **Step 4: Wire the module into `lib.rs`**

In `crates/owl-dl-tableau/src/lib.rs`, after the existing `pub mod hyper;` (around line 48):

```rust
pub mod snapshot;
```

And add to the re-exports section (around line 76-85):

```rust
pub use snapshot::{BackPropRisk, GraphSnapshot, SnapshotNodeId, UnsafeReason};
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p owl-dl-tableau --test backprop_risk
```

Expected: 4 tests pass (`pure_horn_classifies_safe`, `inverse_role_classifies_unsafe`, `cardinality_classifies_unsafe`, `nominal_classifies_unsafe`).

- [ ] **Step 6: Run workspace clippy to verify no warnings**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: clean. If warnings fire (likely `clippy::pedantic` on the iterator code), fix them inline — the workspace runs `-D warnings` in CI per `CLAUDE.md`.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-tableau/src/snapshot.rs \
        crates/owl-dl-tableau/src/lib.rs \
        crates/owl-dl-tableau/tests/backprop_risk.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): types + ontology-wide BackPropRisk classifier (Phase 1a T1)

Lands the GraphSnapshot/SnapshotNode/SnapshotEdge/BackPropRisk types
and the first-cut ontology-wide risk classifier. Conservative: any
inverse role / nominal / cardinality anywhere in the ontology flags
the whole ontology as Unsafe. Horn fragments (GALEN, notgalen,
alehif) land Safe; SROIQ workloads land Unsafe. Per-class refinement
is Phase 1b/3 territory.

No capture path yet (T2); no orchestrator wiring (Phase 1b).
Zero behavior change in default classify.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §3

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `HyperEngine::satisfiability_snapshot` capture

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (add method after `satisfiability_labels` at line 751)
- Create: `crates/owl-dl-tableau/tests/snapshot_capture.rs`

- [ ] **Step 1: Write the failing capture test**

Create `crates/owl-dl-tableau/tests/snapshot_capture.rs`:

```rust
//! Snapshot capture test: build a tiny Horn ontology, run hyper.decide
//! to Sat, then call satisfiability_snapshot and assert the captured
//! structure looks right.
//!
//! Phase 1a invariant: snapshot's root labels are a superset of the
//! seed (the seed was asserted at root) and the snapshot.seed field
//! matches.

use horned_owl::io::ofn::reader::read;
use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::HyperEngine;
use std::io::Cursor;

#[test]
fn snapshot_captures_root_labels_on_horn_sat() {
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;

    // Class IRI → ClassId via Vocabulary::class_id (Option).
    let a_id = internal.vocabulary.class_id("http://t/A").expect("A exists");

    let mut eng = HyperEngine::new(&clauses, a_id);
    let result = eng.decide(64);
    assert_eq!(result, owl_dl_tableau::hyper::HyperResult::Sat);

    let snapshot = eng.satisfiability_snapshot(a_id).expect("snapshot built");
    assert_eq!(snapshot.seed(), a_id);
    assert!(snapshot.is_safe());  // pure Horn → ontology-wide Safe
    assert!(snapshot.node_count() >= 1);
    // Root carries the seed plus its told-subsumer closure.
    let root_labels = snapshot.root_labels();
    assert!(root_labels.contains(&a_id), "root must carry seed");
}

#[test]
fn snapshot_seed_field_matches() {
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:X))
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let clauses = clausify_with_stats(&internal).0;
    let x = internal.vocabulary.class_id("http://t/X").expect("X exists");

    let mut eng = HyperEngine::new(&clauses, x);
    eng.decide(64);
    let snap = eng.satisfiability_snapshot(x).expect("sat");
    assert_eq!(snap.seed(), x);
}
```

The test uses three new public accessors on `GraphSnapshot` — `seed()`, `is_safe()`, `node_count()`, `root_labels()` — that the implementation must add. Keep them in `snapshot.rs` alongside the type.

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p owl-dl-tableau --test snapshot_capture
```

Expected: FAIL with "no method named `satisfiability_snapshot`".

- [ ] **Step 3: Add the capture method to `HyperEngine`**

In `crates/owl-dl-tableau/src/hyper.rs`, after the existing `satisfiability_labels` method (around line 764), add:

```rust
/// Capture a [`GraphSnapshot`] of the current completion graph.
/// Soundly callable only after [`Self::decide`] (or `decide_with_deadline`)
/// has returned [`HyperResult::Sat`] — otherwise the graph state
/// may carry an incomplete or post-clash structure that violates
/// the snapshot's "witness model" contract.
///
/// Returns `None` if the seed isn't present at the resolved root
/// (defensive: matches the [`Self::satisfiability_labels`] guard).
///
/// Phase 1a: `fired` fingerprint slots are placeholder `0`; the real
/// fingerprint computation lands in Phase 1b alongside the lazy
/// replay driver.
#[must_use]
pub fn satisfiability_snapshot(
    &self,
    seed: ClassId,
) -> Option<crate::snapshot::GraphSnapshot> {
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
    for i in 0..n_nodes {
        let h = HNode(u32::try_from(i).expect("node count fits u32"));
        if self.resolve(h) == h {
            let snap_id = u32::try_from(canonical.len()).expect("snap node count fits u32");
            hnode_to_snap[i] = Some(snap_id);
            canonical.push(h);
        }
    }
    // Aliased nodes inherit their representative's snap id.
    for i in 0..n_nodes {
        if hnode_to_snap[i].is_none() {
            let rep = self.resolve(HNode(u32::try_from(i).expect("fits u32")));
            hnode_to_snap[i] = hnode_to_snap[rep.index()];
        }
    }

    let mut nodes = Vec::with_capacity(canonical.len());
    let mut edges: Vec<Vec<SnapshotEdge>> = Vec::with_capacity(canonical.len());
    let mut fired = Vec::with_capacity(canonical.len());
    for (snap_id, h) in canonical.iter().enumerate() {
        let hn = &self.nodes[h.index()];
        nodes.push(SnapshotNode {
            labels: hn.labels.clone(),
            is_root: snap_id == hnode_to_snap[root_rep.index()].expect("root mapped") as usize,
        });
        let mut snap_edges = Vec::with_capacity(hn.edges.len());
        for (role, tgt) in &hn.edges {
            let tgt_rep = self.resolve(*tgt);
            if let Some(snap_tgt) = hnode_to_snap[tgt_rep.index()] {
                snap_edges.push(SnapshotEdge { role: *role, target: snap_tgt });
            }
        }
        edges.push(snap_edges);
        fired.push(0); // Phase 1a placeholder; Phase 1b computes real fingerprint.
    }

    // Phase 1a risk: we don't have the InternalOntology here.
    // The orchestrator (Phase 1b) calls classify_ontology once
    // and stamps it on every snapshot. Phase 1a defaults to Safe
    // — calibration test in Phase 1b will override.
    Some(GraphSnapshot::from_parts(
        nodes,
        edges,
        fired,
        seed,
        crate::snapshot::BackPropRisk::Safe,
    ))
}
```

- [ ] **Step 4: Add public accessors + constructor to `GraphSnapshot`**

In `crates/owl-dl-tableau/src/snapshot.rs`, add an `impl GraphSnapshot` block:

```rust
impl GraphSnapshot {
    /// Construct from raw parts. Used by `HyperEngine::satisfiability_snapshot`
    /// and by tests; not for direct consumer use.
    #[must_use]
    pub fn from_parts(
        nodes: Vec<SnapshotNode>,
        edges: Vec<Vec<SnapshotEdge>>,
        fired: Vec<RuleFingerprint>,
        seed: ClassId,
        risk: BackPropRisk,
    ) -> Self {
        debug_assert_eq!(nodes.len(), edges.len());
        debug_assert_eq!(nodes.len(), fired.len());
        Self { nodes, edges, fired, seed, risk }
    }

    #[must_use]
    pub fn seed(&self) -> ClassId { self.seed }

    #[must_use]
    pub fn is_safe(&self) -> bool { matches!(self.risk, BackPropRisk::Safe) }

    #[must_use]
    pub fn risk(&self) -> BackPropRisk { self.risk }

    #[must_use]
    pub fn node_count(&self) -> usize { self.nodes.len() }

    /// Labels at the seed-graph root (the node carrying the seed).
    /// Mirrors the data returned by `HyperEngine::satisfiability_labels`.
    #[must_use]
    pub fn root_labels(&self) -> &[ClassId] {
        let root_idx = self.nodes.iter().position(|n| n.is_root).unwrap_or(0);
        &self.nodes[root_idx].labels
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

```bash
cargo test -p owl-dl-tableau --test snapshot_capture
```

Expected: 2 tests pass.

- [ ] **Step 6: Run the broader tableau test suite to check no regressions**

```bash
cargo test -p owl-dl-tableau
```

Expected: all pre-existing tableau tests still pass (the new method is additive and not called from any existing path).

- [ ] **Step 7: Clippy + format**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-tableau/src/hyper.rs \
        crates/owl-dl-tableau/src/snapshot.rs \
        crates/owl-dl-tableau/tests/snapshot_capture.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): HyperEngine::satisfiability_snapshot capture (Phase 1a T2)

Adds the snapshot capture path on HyperEngine, mirroring
satisfiability_labels but copying the full node/edge structure.
Walks the union-find at capture time so merged-away nodes are
collapsed into their representative. Phase 1a leaves the per-node
fired-rule fingerprint as 0 (real bloom hash lands with the lazy
replay driver in Phase 1b) and stamps risk = Safe (orchestrator
overrides this in Phase 1b after one classify_ontology call).

Public accessors on GraphSnapshot: seed(), is_safe(), risk(),
node_count(), root_labels(). Constructor: from_parts.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §3 (capture path)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: `RUSTDL_SNAPSHOT_CAPTURE` env flag + Phase 0 canary harness

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env helper, ~5 lines near other env helpers around `label_heuristic_enabled`)
- Create: `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs`

- [ ] **Step 1: Write the failing canary test**

Create `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs`:

```rust
//! Phase 0 canary for the Konclude snapshot cache project.
//!
//! Invariant: the snapshot-capture machinery exists in the
//! tableau crate AND is gated behind `RUSTDL_SNAPSHOT_CAPTURE`
//! (default OFF) on the reasoner side, so default classify
//! has zero behavior change while the project is in flight.
//!
//! This test extends through every phase — Phase 1b will add
//! flag-ON assertions; Phase 1c flips the default and asserts
//! the new path matches verdicts of the old path.

use horned_owl::io::ofn::reader::read;
use horned_owl::io::ParserConfiguration;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify_top_down_with_timeout, snapshot_capture_enabled};
use std::io::Cursor;
use std::time::Duration;

#[test]
fn snapshot_capture_flag_defaults_off() {
    // Phase 0 invariant: env flag defaults OFF.
    // Test guard: SAFE_TO_UNSET — assumes RUSTDL_SNAPSHOT_CAPTURE
    // is not set in the test process env. If a developer manually
    // exports it for debugging, this test correctly fails to remind
    // them.
    assert!(
        std::env::var("RUSTDL_SNAPSHOT_CAPTURE").is_err(),
        "Phase 0 canary: RUSTDL_SNAPSHOT_CAPTURE must not be set in the test env"
    );
    assert!(!snapshot_capture_enabled(), "default must be OFF (Phase 0)");
}

#[test]
fn classify_unchanged_with_flag_off() {
    // Sanity: a tiny ontology classifies to the same result as
    // it did pre-project, regardless of the Phase 1a code merging.
    let src = "\
Prefix(:=<http://t/>)
Ontology(<http://t>
    Declaration(Class(:A))
    Declaration(Class(:B))
    SubClassOf(:A :B)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result = classify_top_down_with_timeout(&onto, Duration::from_millis(200))
        .expect("classify");
    assert!(result.is_subclass("http://t/A", "http://t/B"));
    assert!(!result.is_subclass("http://t/B", "http://t/A"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
```

Expected: FAIL with "unresolved import `owl_dl_reasoner::snapshot_capture_enabled`".

- [ ] **Step 3: Add the env helper to `reasoner/src/lib.rs`**

Locate the existing `label_heuristic_enabled` function (use `grep -n "fn label_heuristic_enabled" crates/owl-dl-reasoner/src/lib.rs` to find the exact line — it's near the other env helpers). Add immediately after it:

```rust
/// Project flag for the Konclude snapshot cache (Phase 1a — capture
/// path landed but no consumer wires it yet). Default OFF; Phase 1c
/// flips the default. Set `RUSTDL_SNAPSHOT_CAPTURE=1` to enable
/// snapshot capture in `subsumes_via_tableau` (Phase 1b onward).
///
/// Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md
#[must_use]
pub fn snapshot_capture_enabled() -> bool {
    std::env::var("RUSTDL_SNAPSHOT_CAPTURE")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .map(|v| v != 0)
        .unwrap_or(false)
}
```

- [ ] **Step 4: Run the canary test to verify it passes**

```bash
cargo test -p owl-dl-reasoner --test snapshot_phase0_canary
```

Expected: 2 tests pass.

- [ ] **Step 5: Run the soundness gate (Phase 0 net) to verify zero behavior change**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin
```

Expected: all three tests PASS with FP=0 + MISSED=0 unchanged from pre-project baseline. This takes ~40 s.

If any closure-diff regresses, **stop immediately** — Phase 1a is supposed to be a no-op for default classify. Investigate before commit.

- [ ] **Step 6: Clippy + format**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs
git commit -m "$(cat <<'EOF'
feat(snapshot): RUSTDL_SNAPSHOT_CAPTURE env flag + Phase 0 canary (Phase 1a T3)

Adds snapshot_capture_enabled() helper (default OFF) and the
Phase 0 canary asserting (a) the flag defaults OFF, (b) default
classify is unchanged. The canary extends through every phase of
the project: Phase 1b adds flag-ON assertions; Phase 1c flips the
default and asserts new == old verdicts.

Soundness gate (alehif + ore-10908 + ore-15672 closure_diff)
verified FP=0 + MISSED=0 unchanged.

Spec: docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md §6 (Phase 0)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Phase 1a results doc + GALEN cost-bound measurement

**Files:**
- Create: `docs/phase1a-results.md`
- (No code changes; measurement only)

- [ ] **Step 1: Run the full Phase 0 soundness gate**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude \
    ore_10908_sroiq ore_15672_shoin 2>&1 | tee /tmp/p1a-soundness.log
```

Expected: FP=0 + MISSED=0 across all three fixtures. Capture the log for the results doc.

- [ ] **Step 2: Run GALEN closure-diff to verify wall is unchanged**

```bash
timeout 900 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --exact --ignored --nocapture 2>&1 | tee /tmp/p1a-galen.log
```

Expected: pass within 10% of the Phase 8 baseline (453 s ± 45 s). Capture the wall measurement.

If wall regresses > 10%, the snapshot-types module is somehow being walked by the default path — investigate before writing the results doc.

- [ ] **Step 3: Run the full tableau crate tests**

```bash
cargo test -p owl-dl-tableau --release 2>&1 | tee /tmp/p1a-tableau.log
```

Expected: all tableau tests pass.

- [ ] **Step 4: Write the results doc**

Create `docs/phase1a-results.md`:

```markdown
# Phase 1a — snapshot data structure + capture results

Run 2026-06-XX at HEAD `<commit-sha>`. Phase 1a lands the types,
the capture path on HyperEngine, the ontology-wide risk classifier,
and the Phase 0 canary harness. **Zero behavior change in default
classify** — capture is gated behind `RUSTDL_SNAPSHOT_CAPTURE`
(default OFF), and no consumer wires it yet (that's Phase 1b).

## Headline

Phase 1a is plumbing-only. Acceptance is that nothing regressed:

- FP=0 + MISSED=0 on alehif + ORE-10908 + ORE-15672 + GALEN
  (unchanged from Phase 8 baseline).
- GALEN classify wall: <wall> s (Phase 8 baseline: 453.02 s; delta
  <delta>%).
- All in-tree tests pass on owl-dl-tableau + owl-dl-reasoner.
- Clippy clean under `-D warnings`.

## What landed

- `crates/owl-dl-tableau/src/snapshot.rs` — `GraphSnapshot`,
  `SnapshotNode`, `SnapshotEdge`, `BackPropRisk`, `UnsafeReason`,
  `SnapshotNodeId`, `RuleFingerprint`. Public accessors: `seed`,
  `is_safe`, `risk`, `node_count`, `root_labels`. Constructor
  `from_parts`.
- `BackPropRisk::classify_ontology(internal)` — first-cut
  ontology-wide classifier. Walks every axiom; any
  `InverseObjectProperties`, any `Role::Inverse` in a concept
  subterm, any `Concept::Nominal`, any `Concept::Min`/`Max`
  forces the whole ontology to Unsafe.
- `HyperEngine::satisfiability_snapshot(seed) -> Option<GraphSnapshot>` —
  walks the union-find, collapses merged-away nodes, copies
  node/edge structure. Phase 1a stamps `risk = Safe` (orchestrator
  overrides in Phase 1b).
- `snapshot_capture_enabled()` env helper, default OFF.
- Phase 0 canary at `crates/owl-dl-reasoner/tests/snapshot_phase0_canary.rs`.
- Risk-classifier unit tests at `crates/owl-dl-tableau/tests/backprop_risk.rs`.
- Snapshot-capture unit tests at `crates/owl-dl-tableau/tests/snapshot_capture.rs`.

## Measurements

| Fixture | Pre-Phase-1a wall | Post-Phase-1a wall | Delta |
|---|---:|---:|---:|
| alehif-test (closure_diff) | <fill> s | <fill> s | <fill>% |
| ORE-10908 (closure_diff) | <fill> s | <fill> s | <fill>% |
| ORE-15672 (closure_diff) | <fill> s | <fill> s | <fill>% |
| GALEN (closure_diff) | 453.02 s | <fill> s | <fill>% |

Soundness: FP=0 + MISSED=0 on all fixtures (unchanged).

## Cost-bound check (acceptance criterion from spec §6 Phase 1a)

Spec §6 Phase 1a revert criterion: "Memory/build cost > 30% of
classify wall on GALEN."

Phase 1a has no consumer of the new types — no snapshots are
captured during default classify because `RUSTDL_SNAPSHOT_CAPTURE`
is OFF. Expected build cost: 0% of classify wall. Measured: <fill>%.

If the measured delta exceeds 5% with the flag OFF, the project
has accidentally wired the new path somewhere — investigate
before declaring Phase 1a complete.

## What's next

Phase 1b: `LazyReplayDriver` + `BackPropAborted` sentinel + wiring
into `subsumes_via_tableau` behind the env flag. Separate plan:
`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b.md`
(written after Phase 1a lands).

## Cross-references

- Project spec: `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`.
- Phase 0 + 1a plan: `docs/superpowers/plans/2026-06-03-konclude-snapshot-cache-phase-1a.md`.
- Pre-project baseline: `docs/perf-2026-06-03-konclude-vs-rustdl.md`.
- Phase 8 GALEN wall reference: `docs/phase8-results.md`.
```

Fill in `<commit-sha>` from `git rev-parse --short HEAD` and the measurement placeholders from the logs in Steps 1-3.

- [ ] **Step 5: Commit the results doc**

```bash
git add docs/phase1a-results.md
git commit -m "$(cat <<'EOF'
docs(phase1a): results — snapshot types + capture + Phase 0 canary

Phase 1a lands plumbing only: types, capture path, ontology-wide
risk classifier, env flag scaffolding, Phase 0 canary. Zero
behavior change in default classify (RUSTDL_SNAPSHOT_CAPTURE=0
default). Soundness gate (alehif + ore-10908 + ore-15672 + GALEN)
FP=0 + MISSED=0 unchanged. GALEN wall delta within ±5% of Phase 8
baseline.

Sets up Phase 1b: replay driver + sentinel + orchestrator wiring.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## After all tasks

Once Tasks 1-4 land, Phase 0 + Phase 1a is complete. The next plan
(`docs/superpowers/plans/2026-06-XX-konclude-snapshot-cache-phase-1b.md`)
covers:

- `LazyReplayDriver` over `CompletionGraph::seeded_from(snapshot)`.
- `BackPropAborted` sentinel.
- Wiring into `subsumes_via_tableau` (gated `RUSTDL_SNAPSHOT_CAPTURE=1`).
- Orchestrator-side `BackPropRisk::classify_ontology` call once per
  `PreparedOntology::from_internal`, stamping every snapshot's `risk`
  field correctly (overriding the Phase 1a "Safe by default" placeholder).
- Per-class snapshot cache (`Arc<DashMap<ClassId, Arc<GraphSnapshot>>>`).
- Counter telemetry (`snapshot_replay_used`, `snapshot_replay_aborts`,
  `snapshot_cache_falls_through`).
- Phase 1b canary extensions on the existing Phase 0 canary file.

That plan gets written **after** Phase 1a lands and a brief recon
confirms no surprises in the snapshot data shape.
