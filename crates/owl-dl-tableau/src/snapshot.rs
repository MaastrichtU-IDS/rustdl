//! Per-class completion graph snapshots for the Konclude-style
//! global classification project (Phase 1a).
//!
//! See `docs/superpowers/specs/2026-06-03-konclude-style-global-classification-design.md`
//! §3 (data structure) and §4 (replay + soundness).
//!
//! Phase 1a deliverable: types + ontology-wide risk classifier.
//! **No capture path, no replay, no orchestrator wiring** — those
//! land in Phase 1b. Default classify behaviour is unchanged.

use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId, Role};
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
/// Construction: Phase 1b will add
/// `crate::hyper::HyperEngine::satisfiability_snapshot` that
/// builds one at the end of a `Sat` verdict. Snapshots are
/// immutable + cheap-to-clone (`Arc`-shareable across the rayon
/// pair loop).
// Phase 1a lands the field shapes; the capture path (T2) + replay
// driver (Phase 1b) are the first readers. `#[allow(dead_code)]`
// keeps the workspace's `-D warnings` CI gate happy in the interim.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    ///
    /// The four reason variants are checked in priority order:
    /// inverse-role > nominal > cardinality (datatype is reserved
    /// for Phase 3). The first matching reason wins. The order is
    /// driven by the spec's soundness-trap pedigree — inverse
    /// roles are the §2 dead-end's smoking gun and so are reported
    /// first.
    #[must_use]
    pub fn classify_ontology(internal: &InternalOntology) -> Self {
        // First pass: explicit role-level axioms.
        for ax in &internal.axioms {
            if matches!(ax, Axiom::InverseObjectProperties(_, _)) {
                return Self::Unsafe {
                    reason: UnsafeReason::InverseRoleReachable,
                };
            }
        }
        // Single concept-walk pass: for each concept reachable from
        // any axiom, ask all three structural questions at once and
        // emit the highest-priority hit found across the ontology.
        let mut hit_inverse = false;
        let mut hit_nominal = false;
        let mut hit_cardinality = false;
        for ax in &internal.axioms {
            for cid in axiom_concept_ids(ax) {
                let (inv, nom, card) = scan_concept(cid, internal);
                hit_inverse |= inv;
                hit_nominal |= nom;
                hit_cardinality |= card;
                if hit_inverse {
                    return Self::Unsafe {
                        reason: UnsafeReason::InverseRoleReachable,
                    };
                }
            }
        }
        if hit_nominal {
            return Self::Unsafe {
                reason: UnsafeReason::NominalReachable,
            };
        }
        if hit_cardinality {
            return Self::Unsafe {
                reason: UnsafeReason::CardinalityReachable,
            };
        }
        Self::Safe
    }
}

/// Collect every `ConceptId` that appears as a top-level operand of
/// `ax`. Sub-concepts reached by walking the pool are not flattened
/// here — [`scan_concept`] handles that on each returned id.
fn axiom_concept_ids(ax: &Axiom) -> Vec<ConceptId> {
    match ax {
        Axiom::SubClassOf { sub, sup } => vec![*sub, *sup],
        Axiom::EquivalentClasses(cs) | Axiom::DisjointClasses(cs) => cs.clone(),
        Axiom::DisjointUnion { members, .. } => members.clone(),
        Axiom::ObjectPropertyDomain { domain, .. } => vec![*domain],
        Axiom::ObjectPropertyRange { range, .. } => vec![*range],
        Axiom::ClassAssertion { class, .. } => vec![*class],
        _ => Vec::new(),
    }
}

/// Walk the sub-expression tree rooted at `cid` once and report
/// `(uses_inverse_role, contains_nominal, contains_cardinality)`.
/// Single traversal so each `ConceptId` is visited at most once
/// per top-level axiom operand.
fn scan_concept(cid: ConceptId, internal: &InternalOntology) -> (bool, bool, bool) {
    let pool = &internal.concepts;
    let mut stack: Vec<ConceptId> = vec![cid];
    let mut seen: hashbrown::HashSet<ConceptId> = hashbrown::HashSet::new();
    let mut inverse = false;
    let mut nominal = false;
    let mut cardinality = false;
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        match pool.get(c) {
            ConceptExpr::Top | ConceptExpr::Bot | ConceptExpr::Atomic(_) => {}
            ConceptExpr::Nominal(_) => nominal = true,
            ConceptExpr::SelfRestriction(r) => {
                if matches!(r, Role::Inverse(_)) {
                    inverse = true;
                }
            }
            ConceptExpr::Not(body) => stack.push(*body),
            ConceptExpr::And(xs) | ConceptExpr::Or(xs) => stack.extend(xs.iter().copied()),
            ConceptExpr::Some(r, body) | ConceptExpr::All(r, body) => {
                if matches!(r, Role::Inverse(_)) {
                    inverse = true;
                }
                stack.push(*body);
            }
            ConceptExpr::Min(_, r, body) | ConceptExpr::Max(_, r, body) => {
                cardinality = true;
                if matches!(r, Role::Inverse(_)) {
                    inverse = true;
                }
                stack.push(*body);
            }
        }
    }
    (inverse, nominal, cardinality)
}
