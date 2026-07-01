//! Standalone consequence-based `ABox` saturator for named-individual inconsistency
//! detection.
//!
//! Implements a minimal fixpoint saturator operating **only on named individuals**
//! (no anonymous witness generation). This makes it:
//! - **Sound**: every clash reported is a genuine contradiction.
//! - **Incomplete**: subsumptions that require generating anonymous witnesses
//!   (e.g. existential body `Marriage ⊑ ∃hasFemalePartner.Woman` when no named
//!   hasFemalePartner edge exists) are not derived.
//!
//! ## Rules implemented
//!
//! 1. **Seed**: `ClassAssertion(C, a)` → add C to types(a); expand `And`
//!    conjuncts recursively; `ObjectPropertyAssertion(R, a, b)` → add (R,a,b) to
//!    edges.
//! 2. **Inverse materialization**: `InverseObjectProperties(R, S)` + edge(R,a,b)
//!    → edge(S,b,a); and vice versa. `SymmetricObjectProperty(R)` is the
//!    self-inverse case `edge(R,a,b)` → `edge(R,b,a)`.
//! 3. **Role hierarchy**: `SubObjectPropertyOf(R, S)` + edge(R,a,b)
//!    → edge(S,a,b).
//! 4. **Role chains + transitivity**: `SubObjectPropertyOf(R₁∘R₂, S)` +
//!    edge(R₁,a,b) + edge(R₂,b,c) → edge(S,a,c) (chains of length 3 too);
//!    `TransitiveObjectProperty(R)` is the self-chain `R∘R⊑R`, closed to the
//!    full transitive closure by the fixpoint.
//! 5. **Domain/range propagation**: `ObjectPropertyDomain(R, D)` + edge(R,a,b)
//!    → add D to types(a). `ObjectPropertyRange(R, D)` + edge(R,a,b)
//!    → add D to types(b).
//! 6. **Type propagation**: `SubClassOf(C, D)` with C ∈ types(a) → add D to
//!    types(a). `EquivalentClasses([C, D, ...])` treated as bidirectional
//!    `SubClassOf` pairs. Recursive `And`-unfolding.
//! 7. **Functional merge**: `FunctionalRole(R)` + two distinct named R-fillers b₁, b₂
//!    of individual a → merge: propagate types(b₁) ∪ types(b₂) to both; repeat
//!    until stable. The merged entity must satisfy all collected types simultaneously.
//! 8. **Disjoint clash**: `DisjointClasses([C₁, C₂, ...])` + any cᵢ, cⱼ both in
//!    types(a) for some a → CLASH (inconsistent).
//! 9. **`ObjectHasValue` ground edges**: `a : ∃R.{b}` (asserted or via
//!    `C ⊑ ∃R.{b}` + `a:C`) → edge(R,a,b) (`b` is a named individual, so this is
//!    a ground entailment, not an anonymous witness).
//! 10. **`SameIndividual` folding**: `a ≡ a'` propagates edges and types across
//!    the union-find equivalence class (`R(a,b)`→`R(a',b)`, `a:C`→`a':C`).
//!
//! ## Instrumentation
//!
//! When `RUSTDL_TRACE=1` is set, emits per-iteration counters to stderr:
//! - chain-1 (role-chain) fires
//! - chain-2 (role-chain 3-hop) fires
//! - individuals with both `∃hasSex.Male` and `∃hasSex.Female` (sex-clash candidates)

// Clippy: this diagnostic/algorithm module uses complex types, nested ifs, and
// deliberate `if !cond` patterns for readability of the chain-matching logic.
#![allow(
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::explicit_iter_loop,
    clippy::if_not_else,
    clippy::match_same_arms,
    clippy::for_kv_map,
    clippy::unnecessary_map_or,
    clippy::doc_markdown,
    clippy::doc_lazy_continuation,
    clippy::uninlined_format_args
)]

use std::collections::{HashMap, HashSet, VecDeque};

use owl_dl_core::ir::{ClassId, ConceptExpr, IndividualId, Role, RoleId};
use owl_dl_core::ontology::{Axiom, InternalOntology, SubRolePath};

// ─── State ────────────────────────────────────────────────────────────────────

/// Per-individual types (named atomic classes only).
type TypeMap = HashMap<IndividualId, HashSet<ClassId>>;

/// A named edge triple stored in normalized form.
/// canonical: `Named(r)` → (r, a, b); `Inverse(r)` → (r, b, a) and store forward.
type RawEdge = (RoleId, IndividualId, IndividualId);

// ─── Saturator ────────────────────────────────────────────────────────────────

/// Result of `saturate_abox_consistency` with diagnostic counters.
#[derive(Debug, Clone)]
pub struct SaturationResult {
    /// True iff a clash was found (inconsistent).
    pub clash: bool,
    /// Number of times a 2-hop role chain fired to produce a new edge.
    pub chain2_fires: u64,
    /// Number of times a 3-hop role chain fired to produce a new edge.
    pub chain3_fires: u64,
    /// Number of individuals that have BOTH `:Man` and `:Woman` in their atomic
    /// type set after full saturation. This is the precondition for the
    /// functional-hasSex clash (Man ≡ ∃hasSex.Male, Woman ≡ ∃hasSex.Female,
    /// Functional(hasSex) → the hasSex filler must be both Male and Female →
    /// DisjointClasses(Male,Female) clash). Zero means no named individual
    /// accumulated both types, so the clash is unreachable in named-only semantics.
    pub sex_clash_candidates: u64,
    /// Number of type additions made during saturation.
    pub type_additions: u64,
    /// Number of edge additions made during saturation.
    pub edge_additions: u64,
    /// The full set of derived role edges `(role_id, subject, object)` over named
    /// individuals at fixpoint (asserted + propagated via hierarchy/inverse/symmetric/
    /// chains/transitivity). Empty when a clash was found. Sound: every edge is
    /// entailed. Used by `materialize_object_property_assertions`; ignored by the
    /// consistency pre-check.
    pub edges: Vec<(RoleId, IndividualId, IndividualId)>,
}

/// Check whether `internal` is ABox-inconsistent under named-only semantics.
///
/// Returns a [`SaturationResult`] with diagnostic counters. The `.clash` field
/// is the primary result.
///
/// # Algorithm
///
/// Iterative fixpoint over named individuals:
/// 1. Seed edges and types from ABox assertions.
/// 2. Apply inverse materialization, role hierarchy, role chains, domain/range,
///    type propagation (SubClassOf/EquivalentClasses), and functional merge until stable.
/// 3. After every iteration, check disjoint clash.
#[allow(clippy::too_many_lines)]
pub fn saturate_abox_consistency(internal: &InternalOntology) -> SaturationResult {
    let trace = std::env::var("RUSTDL_TRACE").map_or(false, |v| v == "1");

    let pool = &internal.concepts;
    let vocab = &internal.vocabulary;

    // ── Pre-index axioms for efficient rule application ───────────────────────

    // SubClassOf: sub_concept → Vec<sup_concept>
    let mut sub_of: HashMap<ClassId, Vec<ClassId>> = HashMap::new();

    // InverseObjectProperties: role → set of inverse roles
    let mut inverses: HashMap<RoleId, HashSet<RoleId>> = HashMap::new();

    // SubObjectPropertyOf (single): sub_role → set of super_roles
    let mut role_super: HashMap<(RoleId, bool), HashSet<(RoleId, bool)>> = HashMap::new();
    // key: (role_id, is_inverse); value: set of (super_role_id, super_is_inverse)

    // SubObjectPropertyOf (chain len=2): Vec<((r1_id, r1_inv), (r2_id, r2_inv), (sup_id, sup_inv))>
    let mut chains2: Vec<((RoleId, bool), (RoleId, bool), (RoleId, bool))> = Vec::new();

    // SubObjectPropertyOf (chain len=3)
    let mut chains3: Vec<(
        (RoleId, bool),
        (RoleId, bool),
        (RoleId, bool),
        (RoleId, bool),
    )> = Vec::new();

    // ObjectPropertyDomain: (role_id, is_inverse) → Vec<ClassId>
    let mut domains: HashMap<(RoleId, bool), Vec<ClassId>> = HashMap::new();
    // ObjectPropertyRange: (role_id, is_inverse) → Vec<ClassId>
    let mut ranges: HashMap<(RoleId, bool), Vec<ClassId>> = HashMap::new();

    // FunctionalRole: set of (role_id, is_inverse) that are functional
    let mut functional: HashSet<(RoleId, bool)> = HashSet::new();

    // DisjointClasses: Vec<Vec<ClassId>>
    let mut disjoint_pairs: Vec<(ClassId, ClassId)> = Vec::new();

    // Existential markers: for each atomic class A, what ∃R.C it implies.
    // Used to detect functional-role clashes without creating anonymous witnesses.
    // Built from SubClassOf(A, ...⊓ ∃R.C ⊓...) and EquivalentClasses(A, ...⊓ ∃R.C ⊓...).
    // Only Named roles (not inverse) are stored; filler must be atomic.
    // key: class A (atomic), value: Vec<(role_id, filler_class_id)>
    let mut existential_of: HashMap<ClassId, Vec<(RoleId, ClassId)>> = HashMap::new();

    // Nominal-filler analog of `existential_of`: `C ⊑ ∃R.{b}` (ObjectHasValue),
    // where the filler is a NAMED individual `b`. When an individual gets type `C`
    // the GROUND edge `R(ind, b)` is entailed (not an anonymous witness). Role is
    // stored with polarity and normalized at use (so `∃R⁻.{b}` works too).
    let mut has_value_of: HashMap<ClassId, Vec<(Role, IndividualId)>> = HashMap::new();

    // EquivalentClasses: used to populate sub_of both ways
    // (handled inline below)

    // Helper to extract atomic ClassId from a ConceptId (if it's Atomic)
    let atomic_class = |cid| -> Option<ClassId> {
        match pool.get(cid) {
            ConceptExpr::Atomic(c) => Some(*c),
            _ => None,
        }
    };

    // Helper to decompose a Role into (role_id, is_inverse)
    let role_key = |r: Role| -> (RoleId, bool) { (r.role_id(), r.is_inverse()) };

    // Helper: extract `∃R.C` markers (Named R with atomic C) from a concept expression.
    // Recursively descends into And-bodies. Does NOT descend into Some/All/Not/Or.
    // Appends to `out`; used to build `existential_of`.
    let collect_existentials = |cid: owl_dl_core::ir::ConceptId,
                                out: &mut Vec<(RoleId, ClassId)>| {
        // Iterative stack-based traversal to avoid recursive closure issues
        let mut stack = vec![cid];
        while let Some(cur) = stack.pop() {
            match pool.get(cur) {
                ConceptExpr::Some(r, filler) => {
                    // Only Named(r) with atomic filler
                    if !r.is_inverse() {
                        if let ConceptExpr::Atomic(c) = pool.get(*filler) {
                            out.push((r.role_id(), *c));
                        }
                    }
                }
                ConceptExpr::And(parts) => {
                    for &p in parts.iter() {
                        stack.push(p);
                    }
                }
                _ => {}
            }
        }
    };

    // Like `collect_existentials`, but for nominal fillers: `∃R.{b}`
    // (ObjectHasValue) → `(R, b)`, a ground-edge marker. `R` keeps its polarity
    // (normalized via `normalize_edge` at use); the filler must be a `Nominal`.
    let collect_hasvalues = |cid: owl_dl_core::ir::ConceptId,
                             out: &mut Vec<(Role, IndividualId)>| {
        let mut stack = vec![cid];
        while let Some(cur) = stack.pop() {
            match pool.get(cur) {
                ConceptExpr::Some(r, filler) => {
                    if let ConceptExpr::Nominal(b) = pool.get(*filler) {
                        out.push((*r, *b));
                    }
                }
                ConceptExpr::And(parts) => {
                    for &p in parts.iter() {
                        stack.push(p);
                    }
                }
                _ => {}
            }
        }
    };

    // Index TBox/RBox axioms
    for axiom in &internal.axioms {
        match axiom {
            Axiom::SubClassOf { sub, sup } => {
                if let (Some(sub_c), Some(sup_c)) = (atomic_class(*sub), atomic_class(*sup)) {
                    sub_of.entry(sub_c).or_default().push(sup_c);
                }
                // Also handle sup being And → unpack conjuncts (atomic AND existential)
                if let Some(sub_c) = atomic_class(*sub) {
                    if let ConceptExpr::And(parts) = pool.get(*sup) {
                        for p in parts.iter() {
                            if let Some(sup_c) = atomic_class(*p) {
                                sub_of.entry(sub_c).or_default().push(sup_c);
                            }
                        }
                    }
                    // Existential markers: sub_c ⊑ ... ⊓ ∃R.C ⊓ ... → sub_c has marker (R,C)
                    let mut exs = Vec::new();
                    collect_existentials(*sup, &mut exs);
                    if !exs.is_empty() {
                        existential_of.entry(sub_c).or_default().extend(exs);
                    }
                    let mut hvs = Vec::new();
                    collect_hasvalues(*sup, &mut hvs);
                    if !hvs.is_empty() {
                        has_value_of.entry(sub_c).or_default().extend(hvs);
                    }
                }
            }
            Axiom::EquivalentClasses(cs) => {
                // Collect atomic classes in the equivalence
                let atomic_cs: Vec<ClassId> = cs.iter().filter_map(|&c| atomic_class(c)).collect();
                // Add `SubClassOf` in both directions for all pairs of atomic classes.
                // e.g. EquivalentClasses(A, B) → sub_of[A]+=B, sub_of[B]+=A
                for i in 0..atomic_cs.len() {
                    for j in 0..atomic_cs.len() {
                        if i != j {
                            sub_of.entry(atomic_cs[i]).or_default().push(atomic_cs[j]);
                        }
                    }
                }
                // Also handle And-bodies: for each atomic C ≡ D₁ ⊓ D₂ ⊓ ..., add C → Dᵢ
                // (C implies each conjunct — atomic AND existential markers).
                // The reverse D₁ ⊓ D₂ → C requires conjunction tracking; we omit it.
                for (i, &cid) in cs.iter().enumerate() {
                    if let Some(c) = atomic_class(cid) {
                        for (j, &did) in cs.iter().enumerate() {
                            if i != j {
                                if let ConceptExpr::And(parts) = pool.get(did) {
                                    for p in parts.iter() {
                                        if let Some(d) = atomic_class(*p) {
                                            sub_of.entry(c).or_default().push(d);
                                        }
                                    }
                                }
                                // Existential markers from And-body
                                let mut exs = Vec::new();
                                collect_existentials(did, &mut exs);
                                if !exs.is_empty() {
                                    existential_of.entry(c).or_default().extend(exs);
                                }
                                let mut hvs = Vec::new();
                                collect_hasvalues(did, &mut hvs);
                                if !hvs.is_empty() {
                                    has_value_of.entry(c).or_default().extend(hvs);
                                }
                            }
                        }
                    }
                    // NOTE: we deliberately do NOT add the reverse direction
                    // (individual conjunct part_c → atomic A from the equivalence)
                    // because part_c alone does NOT imply A — ALL conjuncts must hold.
                    // The old code incorrectly added this and produced spurious
                    // Person → Man/Woman derivations.
                }
            }
            Axiom::InverseObjectProperties(r1, r2) => {
                let (id1, inv1) = role_key(*r1);
                let (id2, inv2) = role_key(*r2);
                // r1 and r2 are inverses of each other
                // Normalize: if R1 = Named(p) and R2 = Named(q), then p⁻ = q
                // We store: for each canonical role_id, what other role_ids are its inverses
                // A more general approach: resolve what the "inverse" of each role direction is
                // R and S are inverse means: edge(R,a,b) → edge(S,b,a)
                // In our (role_id, is_inverse) scheme:
                //   edge((id1, inv1), a, b) → edge((id2, !inv2), b, a)
                //   more precisely: if R1 holds(a,b), then R2 holds(b,a)
                //   R1 as a direction is (id1, inv1); to say R2(b,a), that is
                //   the direction (id2, inv2) applied at (b,a).
                // We'll add to role_super: canonical direction of r1 gets r2's canonical-reversed
                // approach: just record direct inverse pairs for materialization
                // Simplest: for each InverseObjectProperties(R,S):
                //   when we see edge(R_id, R_inv, a, b), add edge(S_id, S_inv, b, a) if not already
                // The set stores: (r_id, r_inv) -> set of (s_id, s_inv) that are its inverses
                // (i.e., if edge(r_id, r_inv, a, b) → then for each (s_id, s_inv) in inverses,
                //  emit edge(s_id, s_inv, b, a))
                inverses.entry(id1).or_default().insert(id2);
                inverses.entry(id2).or_default().insert(id1);
                // We need to track the full polarity. Store as: role_inverses map
                // maps (role_id, is_inverse) → set of (role_id, is_inverse) that hold in reversed direction
                // i.e. if (id1, inv1)(a,b) then (id2, inv2)(b,a)
                // Internally we store edges as raw triples (role_id, a, b) canonicalized
                // so we need: when edge role_id1 normalized a→b exists, what else to derive
                // Let's use a separate inverse_map: (RoleId, bool) → Vec<(RoleId, bool)>
                // meaning "if this role-direction fires, also fire these in reversed direction"
                // We'll handle this in the main loop using the inverse_rules structure below.
                let _ = (id1, inv1, id2, inv2); // will be used below via inverse_rules
            }
            Axiom::SubObjectPropertyOf { sub, sup } => {
                match sub {
                    SubRolePath::Role(r) => {
                        let k = role_key(*r);
                        let v = role_key(*sup);
                        role_super.entry(k).or_default().insert(v);
                    }
                    SubRolePath::Chain(roles) => {
                        if roles.len() == 2 {
                            chains2.push((role_key(roles[0]), role_key(roles[1]), role_key(*sup)));
                        } else if roles.len() == 3 {
                            chains3.push((
                                role_key(roles[0]),
                                role_key(roles[1]),
                                role_key(roles[2]),
                                role_key(*sup),
                            ));
                        }
                        // longer chains not supported
                    }
                }
            }
            Axiom::TransitiveRole(r) => {
                // Transitivity is the self-chain `R ∘ R ⊑ R`; registering it as a
                // length-2 chain lets the Rule-4 fixpoint close the full transitive
                // closure of `R`-edges — jointly with the role hierarchy, inverse
                // materialization, and declared chains (a transitive edge can feed
                // a sub-property/chain rule and vice-versa). The inverse of a
                // transitive role is transitive too, and is covered by inverse
                // materialization re-running over the closed forward edges.
                let k = role_key(*r);
                chains2.push((k, k, k));
            }
            Axiom::ObjectPropertyDomain { role, domain } => {
                if let Some(d) = atomic_class(*domain) {
                    domains.entry(role_key(*role)).or_default().push(d);
                }
                // Also handle And body
                if let ConceptExpr::And(parts) = pool.get(*domain) {
                    for p in parts.iter() {
                        if let Some(d) = atomic_class(*p) {
                            domains.entry(role_key(*role)).or_default().push(d);
                        }
                    }
                }
            }
            Axiom::ObjectPropertyRange { role, range } => {
                if let Some(d) = atomic_class(*range) {
                    ranges.entry(role_key(*role)).or_default().push(d);
                }
                if let ConceptExpr::And(parts) = pool.get(*range) {
                    for p in parts.iter() {
                        if let Some(d) = atomic_class(*p) {
                            ranges.entry(role_key(*role)).or_default().push(d);
                        }
                    }
                }
            }
            Axiom::FunctionalRole(r) => {
                functional.insert(role_key(*r));
            }
            Axiom::InverseFunctionalRole(r) => {
                // InverseFunctionalRole(R) = FunctionalRole(R⁻)
                let (id, inv) = role_key(*r);
                functional.insert((id, !inv));
            }
            Axiom::DisjointClasses(cs) => {
                // Add all pairs (cᵢ, cⱼ) with i < j
                let atomic_cs: Vec<ClassId> = cs.iter().filter_map(|&c| atomic_class(c)).collect();
                for i in 0..atomic_cs.len() {
                    for j in (i + 1)..atomic_cs.len() {
                        disjoint_pairs.push((atomic_cs[i], atomic_cs[j]));
                    }
                }
            }
            _ => {}
        }
    }

    // Build inverse_rules: (role_id, is_inverse) → Vec<(role_id, is_inverse)>
    // meaning "if edge(role_id, is_inverse, a, b), also add edge(inv_role, inv_inv, b, a)"
    // From InverseObjectProperties(R, S): R(a,b) → S(b,a), S(a,b) → R(b,a)
    // But note Role can be Named(p) or Inverse(q).
    // InverseObjectProperties(Named(p), Named(q)):
    //   edge(p, a, b) → q(b,a)  → add edge (q_id, false, b, a)
    //   edge(q, a, b) → p(b,a)  → add edge (p_id, false, b, a)
    // InverseObjectProperties(Named(p), Inverse(q)):
    //   p(a,b) → q⁻(b,a) = q(a,b)?  Actually q⁻(b,a) = q-in-inverse(b,a) = q(a,b)
    //   This is the self-inverse case.
    // We build this from the axioms directly:
    let mut inverse_rules: HashMap<(RoleId, bool), Vec<(RoleId, bool)>> = HashMap::new();
    for axiom in &internal.axioms {
        if let Axiom::InverseObjectProperties(r1, r2) = axiom {
            // r1(a,b) → r2(b,a) means:
            //   if we have canonical edge for r1-direction going a→b,
            //   we should add canonical edge for r2-direction going b→a
            // In (role_id, is_inverse) terms:
            //   r1 = (id1, inv1) → means r1-direction a→b is (id1, inv1)
            //   "r2(b,a)" as a direction is r2 applied to (b,a); r2 = (id2, inv2)
            //   To normalize: store canonical direction of r2 applied at reversed endpoints
            //   = (id2, inv2) going b→a = same as (id2, !inv2) going a→b? No.
            // The cleanest approach: represent edges as directed (role_id, a, b) where
            // role_id is always the "Named" role and direction is always forward.
            // Then InverseObjectProperties(Named(p), Named(q)) means:
            //   edge(p, a, b) → edge(q, b, a)
            //   edge(q, a, b) → edge(p, b, a)
            // InverseObjectProperties(Named(p), Inverse(q)) means:
            //   p(a,b) → (Inv q)(b,a) = q⁻(b,a) = q(a,b) → so edge(q, a, b)!
            //   Wait: Inverse(q)(b,a) means the inverse of q holds between b,a
            //        = q(a,b). So p(a,b) ↔ q(a,b)? That would make them equal, not inverse.
            // Actually InverseObjectProperties(p, q⁻) = Inverse(q) means p and q⁻ are inverses
            // i.e. p(x,y) ↔ q⁻(y,x) = q(x,y). So p = q. This is a degenerate case.
            // Let's treat (id1, inv1) → (id2, inv2) as "if edge of direction (id1, inv1)
            // fires a→b, add edge of direction that is (id2, inv2) reversed"
            // i.e. if (id1, inv1, a, b), add (id2, inv2, b, a).
            // In our canonical storage where edges are (role_id, a, b) (Named direction):
            //   If r1=Named(p), r2=Named(q): edge(p,a,b) → edge(q,b,a)
            //   If r1=Named(p), r2=Inverse(q): edge(p,a,b) → Inverse(q)(b,a)=q(a,b) → edge(q,a,b)
            // So the inverse rule is: (id1, inv1) → make edge (id2, if inv2 then !same else same...)
            // For simplicity, let's represent edges as raw (role_id, IndividualId, IndividualId)
            // and InverseObjectProperties(R, S) yields:
            //   (R_id, a, b) → (S_id, b, a) when R=Named(p), S=Named(q)
            //   (R_id, a, b) → (S_id, a, b) when R=Named(p), S=Inverse(q)
            // We encode this as: inverse_rule_fwd(r1_id, r1_inv) → (r2_id, r2_inv)
            // and the firing semantics is:
            //   if canonical_edge(r1_id, r1_inv, a, b) → add canonical_edge(r2_id, r2_inv, b, a)
            // where canonical_edge normalization: if is_inverse, swap a and b, store Named direction.
            // Actually let's just store as (role_id, a, b) in raw triples and normalize inverse at read.
            // For simplicity: store all edges as HashSet<(RoleId, IndividualId, IndividualId)>
            // representing "Named role holds (a, b)" and separately track which role is the
            // "inverse" version of which via the inverse_map.
            // inverse_rules entry: "when edge(key_role, a, b) is added, also add edge(val_role, b, a)"
            let (id1, inv1) = (r1.role_id(), r1.is_inverse());
            let (id2, inv2) = (r2.role_id(), r2.is_inverse());
            // Case: inv1=false, inv2=false → Named(p) and Named(q) are inverses
            //   edge(p, a, b) → edge(q, b, a)
            //   edge(q, a, b) → edge(p, b, a)
            // Case: inv1=false, inv2=true → Named(p) and Inverse(q) are inverses
            //   p(a,b) ↔ Inv(q)(b,a) = q(a,b) → so p and q are equal (self-inverse degenerate)
            //   edge(p, a, b) → edge(q, a, b)  [because Inverse(q)(b,a) = q(a,b)]
            //   edge(q, a, b) → edge(p, a, b)
            // Case: inv1=true, inv2=false → symmetric to case above (just swap roles)
            // We encode: (from_role, from_inv) → (to_role, to_inv)
            // where the firing rule is: edge(from_role, a, b) → edge(to_role, rev_a, rev_b)
            //   where rev = swap if !(from_inv xor to_inv) else keep
            // Simplest encoding: store the (to_role, reverse_endpoints) flag
            // reverse_endpoints = !(inv1 == inv2) [XOR: opposite inversions mean don't swap]
            // Actually: if inv1=false, inv2=false → swap. If inv1=false, inv2=true → don't swap.
            // if inv1=true, inv2=false → don't swap. if inv1=true, inv2=true → swap.
            // reverse = (inv1 == inv2)
            let reverse = inv1 == inv2;
            inverse_rules
                .entry((id1, false))
                .or_default()
                .push((id2, reverse));
            inverse_rules
                .entry((id2, false))
                .or_default()
                .push((id1, reverse));
        }
    }
    // Note: the above stores rules for Named(id1) → Named(id2) with reverse flag.
    // The actual logic: when we see edge (id1, a, b), we check inverse_rules[(id1, false)]
    // and for each (id2, do_reverse): if do_reverse, add (id2, b, a); else add (id2, a, b).

    // SymmetricRole(R): R ≡ R⁻, i.e. `edge(R,a,b) ⟹ edge(R,b,a)`. Encode as a
    // self inverse-rule with reversed endpoints so Rule 2 (inverse materialization)
    // closes it. `SymmetricRole(Inverse(r))` ⟹ `r` symmetric — same `role_id`.
    for axiom in &internal.axioms {
        if let Axiom::SymmetricRole(r) = axiom {
            inverse_rules
                .entry((r.role_id(), false))
                .or_default()
                .push((r.role_id(), true));
        }
    }

    // SameIndividual equivalence classes (union-find). Edges and types propagate
    // across same-individuals: `a ≡ a2` + `R(a,b)` ⟹ `R(a2,b)`, `R(c,a)` ⟹
    // `R(c,a2)`, and `a:C` ⟹ `a2:C`. `same_members[i]` is `i`'s full class
    // (incl. `i`); individuals in no `SameIndividual` axiom are absent ⟹ no cost.
    let mut uf: HashMap<IndividualId, IndividualId> = HashMap::new();
    for axiom in &internal.axioms {
        if let Axiom::SameIndividual(inds) = axiom {
            for w in inds.windows(2) {
                let (x, y) = (w[0], w[1]);
                uf.entry(x).or_insert(x);
                uf.entry(y).or_insert(y);
                let (rx, ry) = (uf_find(&uf, x), uf_find(&uf, y));
                if rx != ry {
                    uf.insert(rx, ry);
                }
            }
        }
    }
    let same_members: HashMap<IndividualId, Vec<IndividualId>> = {
        let mut by_root: HashMap<IndividualId, Vec<IndividualId>> = HashMap::new();
        for &m in uf.keys() {
            by_root.entry(uf_find(&uf, m)).or_default().push(m);
        }
        let mut m = HashMap::new();
        for grp in by_root.values() {
            for &i in grp {
                m.insert(i, grp.clone());
            }
        }
        m
    };

    // ── ABox state ────────────────────────────────────────────────────────────

    let mut types: TypeMap = HashMap::new();
    let mut edges: HashSet<RawEdge> = HashSet::new(); // (role_id, a, b)

    // Existential markers: per-individual (role_id, filler_class) markers.
    // An individual X has marker (R, C) if X has some type A with A ⊑ ∃R.C.
    // Used for functional-role clash detection without anonymous witnesses.
    // Key: (IndividualId, RoleId), Value: set of ClassId fillers
    let mut existential_markers: HashMap<(IndividualId, RoleId), HashSet<ClassId>> = HashMap::new();

    // Worklists for types and edges
    let mut type_queue: VecDeque<(IndividualId, ClassId)> = VecDeque::new();
    let mut edge_queue: VecDeque<RawEdge> = VecDeque::new();

    let mut result = SaturationResult {
        clash: false,
        chain2_fires: 0,
        chain3_fires: 0,
        sex_clash_candidates: 0,
        type_additions: 0,
        edge_additions: 0,
        edges: Vec::new(),
    };

    // Helper closures (we'll use inline logic for borrow reasons)

    // ── Seed from ABox ─────────────────────────────────────────────────────────

    for axiom in &internal.axioms {
        match axiom {
            Axiom::ClassAssertion { class, individual } => {
                // Expand the concept recursively to collect atomic class IDs
                enqueue_concept_types(*individual, *class, pool, &mut type_queue);
                // ObjectHasValue ground edges: `a : ∃R.{b}` ⟹ edge `R(a, b)`.
                let mut hvs = Vec::new();
                collect_hasvalues(*class, &mut hvs);
                for (r, b) in hvs {
                    let e = normalize_edge(r, *individual, b);
                    if edges.insert(e) {
                        edge_queue.push_back(e);
                    }
                }
            }
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                // Normalize: Named(r)(a,b) → store (r, a, b)
                //            Inverse(r)(a,b) → store (r, b, a)
                let (rid, a, b) = normalize_edge(*role, *subject, *object);
                if edges.insert((rid, a, b)) {
                    edge_queue.push_back((rid, a, b));
                }
            }
            _ => {}
        }
    }

    // ── Fixpoint ───────────────────────────────────────────────────────────────

    let mut changed = true;
    while changed {
        changed = false;

        // Drain type queue
        while let Some((ind, cls)) = type_queue.pop_front() {
            if types.entry(ind).or_default().insert(cls) {
                result.type_additions += 1;
                changed = true;

                if trace {
                    let ind_iri = vocab.individual_iri(ind);
                    let class_iri = vocab.class_iri(cls);
                    // Only trace specific individuals or Man/Woman types
                    if (class_iri.ends_with("#Woman")
                        || class_iri.ends_with("/Woman")
                        || class_iri.ends_with("#Man")
                        || class_iri.ends_with("/Man"))
                        && (ind_iri.ends_with("richard_john_bright_1962")
                            || ind_iri.ends_with("robert_david_bright_1965")
                            || ind_iri.ends_with("james_bright_1964"))
                    {
                        eprintln!(
                            "[abox-sat] TYPE {} → {} (from type queue)",
                            ind_iri, class_iri
                        );
                    }
                }

                // Rule 6: type propagation via SubClassOf
                if let Some(supers) = sub_of.get(&cls) {
                    for &sup_c in supers {
                        if !types.entry(ind).or_default().contains(&sup_c) {
                            type_queue.push_back((ind, sup_c));
                        }
                    }
                }

                // Rule 7a: existential markers — if A ⊑ ∃R.C and ind:A, record marker (R,C) for ind
                if let Some(exs) = existential_of.get(&cls) {
                    for &(role_id, filler_cls) in exs {
                        existential_markers
                            .entry((ind, role_id))
                            .or_default()
                            .insert(filler_cls);
                    }
                }

                // Rule 7b: nominal-filler existential (ObjectHasValue) — `A ⊑ ∃R.{b}`,
                // `ind:A` ⟹ ground edge `R(ind, b)`.
                if let Some(hvs) = has_value_of.get(&cls) {
                    let hvs: Vec<(Role, IndividualId)> = hvs.clone();
                    for (r, b) in hvs {
                        let e = normalize_edge(r, ind, b);
                        if edges.insert(e) {
                            edge_queue.push_back(e);
                        }
                    }
                }

                // Rule 9a: SameIndividual type propagation — `ind ≡ m` ⟹ `m:cls`.
                if let Some(members) = same_members.get(&ind) {
                    let members = members.clone();
                    for m in members {
                        if m != ind && !types.entry(m).or_default().contains(&cls) {
                            type_queue.push_back((m, cls));
                        }
                    }
                }
            }
        }

        // Drain edge queue
        while let Some((rid, a, b)) = edge_queue.pop_front() {
            result.edge_additions += 1;
            changed = true;

            // Rule 2: inverse materialization
            if let Some(inv_rules) = inverse_rules.get(&(rid, false)) {
                let inv_rules = inv_rules.clone();
                for (inv_rid, do_reverse) in inv_rules {
                    let (na, nb) = if do_reverse { (b, a) } else { (a, b) };
                    if edges.insert((inv_rid, na, nb)) {
                        edge_queue.push_back((inv_rid, na, nb));
                    }
                }
            }

            // Rule 3: role hierarchy (single step)
            let role_fwd = (rid, false);
            if let Some(supers) = role_super.get(&role_fwd) {
                let supers: Vec<_> = supers.iter().copied().collect();
                for (sup_id, sup_inv) in supers {
                    let (na, nb) = if sup_inv { (b, a) } else { (a, b) };
                    if edges.insert((sup_id, na, nb)) {
                        edge_queue.push_back((sup_id, na, nb));
                    }
                }
            }

            // Rule 5: domain/range propagation
            if let Some(dom_classes) = domains.get(&(rid, false)) {
                let dom_classes = dom_classes.clone();
                for d in dom_classes {
                    if !types.entry(a).or_default().contains(&d) {
                        if trace {
                            let ind_iri = vocab.individual_iri(a);
                            let class_iri = vocab.class_iri(d);
                            if (class_iri.ends_with("#Woman")
                                || class_iri.ends_with("/Woman")
                                || class_iri.ends_with("#Man")
                                || class_iri.ends_with("/Man"))
                                && (ind_iri.ends_with("richard_john_bright_1962")
                                    || ind_iri.ends_with("robert_david_bright_1965")
                                    || ind_iri.ends_with("james_bright_1964"))
                            {
                                let b_iri = vocab.individual_iri(b);
                                let role_iri = vocab.role_iri(rid);
                                eprintln!(
                                    "[abox-sat] DOMAIN-DERIVED {} : {} via {}({}, {})",
                                    ind_iri, class_iri, role_iri, ind_iri, b_iri
                                );
                            }
                        }
                        type_queue.push_back((a, d));
                    }
                }
            }
            if let Some(rng_classes) = ranges.get(&(rid, false)) {
                let rng_classes = rng_classes.clone();
                for d in rng_classes {
                    if !types.entry(b).or_default().contains(&d) {
                        type_queue.push_back((b, d));
                    }
                }
            }

            // Rule 9b: SameIndividual edge propagation — `a ≡ a'`, `b ≡ b'`
            // ⟹ `R(a', b')`. Only fires when an endpoint is in a same-class
            // (the cross-product otherwise reduces to the original edge).
            let a_eq = same_members.get(&a);
            let b_eq = same_members.get(&b);
            if a_eq.is_some() || b_eq.is_some() {
                let av: Vec<IndividualId> = a_eq.cloned().unwrap_or_else(|| vec![a]);
                let bv: Vec<IndividualId> = b_eq.cloned().unwrap_or_else(|| vec![b]);
                for &na in &av {
                    for &nb in &bv {
                        if (na, nb) != (a, b) && edges.insert((rid, na, nb)) {
                            edge_queue.push_back((rid, na, nb));
                        }
                    }
                }
            }
        }

        // Rule 4: role chains — scan all pairs of edges for each chain rule
        // We do this once per outer iteration to avoid O(n³) inner loops
        let edge_vec: Vec<RawEdge> = edges.iter().copied().collect();

        for &(r1_id, r1_inv, r2_id, r2_inv, sup_id, sup_inv) in &chains2
            .iter()
            .map(|&((a, b), (c, d), (e, f))| (a, b, c, d, e, f))
            .collect::<Vec<_>>()
        {
            // We need: edge matching r1 direction (r1_id, r1_inv) going a→b
            //         + edge matching r2 direction (r2_id, r2_inv) going b→c
            //         → add edge sup direction (sup_id, sup_inv) going a→c
            for &(ea_id, ea, eb) in &edge_vec {
                // Check if this edge matches r1 direction
                let (a, b) = if !r1_inv {
                    // Named(r1_id): edge (r1_id, a, b) matches
                    if ea_id != r1_id {
                        continue;
                    }
                    (ea, eb)
                } else {
                    // Inverse(r1_id): edge (r1_id, b, a) matches → direction a→b is (r1_id, b, a)
                    // i.e. we need canonical edge (r1_id, eb, ea) to represent Inv(r1_id)(ea, eb)
                    // But we store Named(r1_id)(eb, ea) which is the same edge reversed.
                    // Actually: Inverse(r1_id)(a,b) ↔ Named(r1_id)(b,a)
                    // So edge (r1_id, eb, ea) in our set means Inverse(r1_id)(ea, eb)
                    // We need to check if (r1_id, eb, ea) is in edges, but we're iterating ea_id==r1_id...
                    // Rethink: if r1_inv=true, the chain fires when Inverse(r1_id)(x,y) holds
                    // = when Named(r1_id)(y,x) holds = edge(r1_id, y, x) in our set
                    // So we need: find edge (r1_id, eb, ea) to mean Inv(r1_id)(ea, eb)
                    // The current edge is (ea_id, ea, eb); for r1_inv, the source is eb and dest is ea
                    if ea_id != r1_id {
                        continue;
                    }
                    (eb, ea) // "a" in chain sense is eb, "b" is ea
                };

                // Now find edge matching r2 direction going b→c
                for &(eb2_id, eb2, ec) in &edge_vec {
                    let b2_src = if !r2_inv {
                        if eb2_id != r2_id || eb2 != b {
                            continue;
                        }
                        ec
                    } else {
                        // Inv(r2_id)(b,c) ↔ Named(r2_id)(c,b)
                        if eb2_id != r2_id || ec != b {
                            continue;
                        }
                        eb2
                    };

                    let c = b2_src;
                    // Derive sup direction a→c
                    let (na, nc) = if !sup_inv { (a, c) } else { (c, a) };
                    if edges.insert((sup_id, na, nc)) {
                        edge_queue.push_back((sup_id, na, nc));
                        result.chain2_fires += 1;
                        changed = true;
                    }
                }
            }
        }

        // 3-hop chains
        for &(r1_id, r1_inv, r2_id, r2_inv, r3_id, r3_inv, sup_id, sup_inv) in &chains3
            .iter()
            .map(|&((a, b), (c, d), (e, f), (g, h))| (a, b, c, d, e, f, g, h))
            .collect::<Vec<_>>()
        {
            for &(ea_id, ea, eb) in &edge_vec {
                let (a, b) = if !r1_inv {
                    if ea_id != r1_id {
                        continue;
                    }
                    (ea, eb)
                } else {
                    if ea_id != r1_id {
                        continue;
                    }
                    (eb, ea)
                };

                for &(eb2_id, eb2, ec) in &edge_vec {
                    let (b2, c) = if !r2_inv {
                        if eb2_id != r2_id || eb2 != b {
                            continue;
                        }
                        (eb2, ec)
                    } else {
                        if eb2_id != r2_id || ec != b {
                            continue;
                        }
                        (ec, eb2)
                    };
                    let _ = b2;

                    for &(ec2_id, ec2, ed) in &edge_vec {
                        let d = if !r3_inv {
                            if ec2_id != r3_id || ec2 != c {
                                continue;
                            }
                            ed
                        } else {
                            if ec2_id != r3_id || ed != c {
                                continue;
                            }
                            ec2
                        };

                        // Derive sup direction a→d
                        let (na, nd) = if !sup_inv { (a, d) } else { (d, a) };
                        if edges.insert((sup_id, na, nd)) {
                            edge_queue.push_back((sup_id, na, nd));
                            result.chain3_fires += 1;
                            changed = true;
                        }
                    }
                }
            }
        }

        // Rule 7: functional role merge
        // For each functional role R and each individual a that has ≥2 distinct R-fillers,
        // propagate types bidirectionally between the fillers.
        for &(func_rid, func_inv) in &functional {
            // Collect all a → fillers(a) via role func_rid (in direction func_inv)
            // functional role: FunctionalRole(R) means at most one R-successor
            // InverseFunctionalRole(R) = FunctionalRole(R⁻) → at most one R-predecessor
            // For each a, collect fillers via R-direction:
            let mut fillers_by_subj: HashMap<IndividualId, Vec<IndividualId>> = HashMap::new();
            for &(rid, a, b) in &edges {
                if rid == func_rid {
                    if !func_inv {
                        // Named(func_rid)(a,b) → filler of a is b
                        fillers_by_subj.entry(a).or_default().push(b);
                    } else {
                        // Inverse(func_rid)(a,b) = Named(func_rid)(b,a) → filler of b is a
                        // So if func_inv=true, the role is Inverse(func_rid), subject is b, filler is a
                        fillers_by_subj.entry(b).or_default().push(a);
                    }
                }
            }

            for (_, fillers) in &fillers_by_subj {
                if fillers.len() >= 2 {
                    // Merge all fillers: propagate all types from each to all others
                    // We do pairwise: for each pair (f1, f2), add all types of f1 to f2 and vice versa
                    let all_types: HashSet<ClassId> = fillers
                        .iter()
                        .flat_map(|f| types.get(f).into_iter().flatten().copied())
                        .collect();
                    for &f in fillers {
                        let current = types.entry(f).or_default();
                        for &t in &all_types {
                            if current.insert(t) {
                                type_queue.push_back((f, t));
                                result.type_additions += 1;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // Rule 7b: functional existential marker clash.
        // If individual X has existential markers (R, C1) and (R, C2) for a functional role R,
        // and C1 and C2 are told-disjoint → the single R-successor must be both → CLASH.
        // This detects the family-style functional-hasSex clash without anonymous witnesses.
        for (&(ind, role_id), fillers) in &existential_markers {
            if !functional.contains(&(role_id, false)) {
                continue; // Only functional roles
            }
            let filler_vec: Vec<ClassId> = fillers.iter().copied().collect();
            for i in 0..filler_vec.len() {
                for j in (i + 1)..filler_vec.len() {
                    let (f1, f2) = (filler_vec[i], filler_vec[j]);
                    // Check disjointness
                    if disjoint_pairs
                        .iter()
                        .any(|&(d1, d2)| (d1 == f1 && d2 == f2) || (d1 == f2 && d2 == f1))
                    {
                        if trace {
                            eprintln!(
                                "[abox-sat] FUNCTIONAL-MARKER CLASH: {} has ∃{}.{} ∩ ∃{}.{} with Functional + Disjoint",
                                vocab.individual_iri(ind),
                                vocab.role_iri(role_id),
                                vocab.class_iri(f1),
                                vocab.role_iri(role_id),
                                vocab.class_iri(f2),
                            );
                        }
                        result.clash = true;
                    }
                }
            }
        }

        // Rule 8: disjoint clash check
        for &(c1, c2) in &disjoint_pairs {
            for (ind, ind_types) in &types {
                if ind_types.contains(&c1) && ind_types.contains(&c2) {
                    if trace {
                        eprintln!(
                            "[abox-sat] CLASH: {} has both {:?} and {:?}",
                            vocab.individual_iri(*ind),
                            vocab.class_iri(c1),
                            vocab.class_iri(c2)
                        );
                    }
                    result.clash = true;
                }
            }
        }

        if result.clash {
            break;
        }
    }

    // ── Diagnostic: sex-clash candidates (Man ∩ Woman co-occurrence) ─────────
    // Count individuals that have BOTH :Man and :Woman in their type set after
    // full saturation. This is the precondition for the functional-hasSex clash:
    // Man ≡ Person ⊓ ∃hasSex.Male, Woman ≡ Person ⊓ ∃hasSex.Female,
    // Functional(hasSex) → same individual would need Male and Female → clash.
    let man_id = vocab
        .classes()
        .find(|(_, iri)| iri.ends_with("#Man") || iri.ends_with("/Man"))
        .map(|(id, _)| id);
    let woman_id = vocab
        .classes()
        .find(|(_, iri)| iri.ends_with("#Woman") || iri.ends_with("/Woman"))
        .map(|(id, _)| id);

    if let (Some(man), Some(woman)) = (man_id, woman_id) {
        for (ind, ind_types) in &types {
            if ind_types.contains(&man) && ind_types.contains(&woman) {
                result.sex_clash_candidates += 1;
                if trace {
                    eprintln!(
                        "[abox-sat] MAN+WOMAN clash candidate: {}",
                        vocab.individual_iri(*ind)
                    );
                }
            }
        }
    }

    if trace {
        eprintln!(
            "[abox-sat] chain2_fires={} chain3_fires={} type_additions={} edge_additions={} sex_clash_candidates={}",
            result.chain2_fires,
            result.chain3_fires,
            result.type_additions,
            result.edge_additions,
            result.sex_clash_candidates,
        );
        eprintln!(
            "[abox-sat] individuals={} total_edges={}",
            types.len(),
            edges.len()
        );
    }

    // Expose the derived edge set on the normal (non-clash) return path. On a
    // clash, leave `edges` empty (the `Vec::new()` default) — matching the
    // documented "empty when a clash was found" contract.
    if !result.clash {
        result.edges = edges.iter().copied().collect();
    }

    result
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Normalize a role-directed edge into canonical `(role_id, a, b)` form.
/// `Named(r)(a,b)` → `(r, a, b)`.
/// `Inverse(r)(a,b)` → `(r, b, a)`.
/// Union-find root of `x` (iterative; no path compression — equivalence classes
/// from `SameIndividual` are tiny). Returns `x` itself when absent/unmerged.
fn uf_find(uf: &HashMap<IndividualId, IndividualId>, mut x: IndividualId) -> IndividualId {
    loop {
        match uf.get(&x).copied() {
            Some(p) if p != x => x = p,
            _ => return x,
        }
    }
}

fn normalize_edge(role: Role, a: IndividualId, b: IndividualId) -> RawEdge {
    if role.is_inverse() {
        (role.role_id(), b, a)
    } else {
        (role.role_id(), a, b)
    }
}

/// Recursively enqueue atomic type IDs from a concept expression.
/// Handles: Atomic, And (conjuncts), Bot → nothing, Top → nothing.
/// Other constructors (Some, All, etc.) are skipped for named-only semantics.
fn enqueue_concept_types(
    ind: IndividualId,
    cid: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ir::ConceptPool,
    queue: &mut VecDeque<(IndividualId, ClassId)>,
) {
    match pool.get(cid) {
        ConceptExpr::Atomic(c) => {
            queue.push_back((ind, *c));
        }
        ConceptExpr::And(parts) => {
            for &p in parts.iter() {
                enqueue_concept_types(ind, p, pool, queue);
            }
        }
        ConceptExpr::Top | ConceptExpr::Bot | ConceptExpr::Not(_) | ConceptExpr::Or(_) => {
            // Not handled in named-only semantics
        }
        ConceptExpr::Some(_, _)
        | ConceptExpr::All(_, _)
        | ConceptExpr::Min(_, _, _)
        | ConceptExpr::Max(_, _, _)
        | ConceptExpr::Nominal(_)
        | ConceptExpr::SelfRestriction(_) => {
            // Not handled
        }
    }
}
