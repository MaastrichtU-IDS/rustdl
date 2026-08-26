//! Inferred `inferred_object_property_values` / `inferred_data_property_values`
//! queries (issue #45).
//!
//! **Object values**: sound seed (`materialize_object_property_assertions`,
//! already a sound lower bound over named individuals — asserted + sub-property
//! / inverse / symmetric / role-chain / transitive closure) plus a BUDGETED,
//! HARD-BOUNDED entailment extension. The extension deliberately does NOT
//! enumerate the full `|individuals|² × |properties|` cross-product — see
//! [`candidate_extension_pairs`] for the exact bounded policy. Each extension
//! candidate is checked via `PreparedOntology::consistent_with_extra`'s
//! `extra_neg_prop` path (Task 0.3): `R(a,b)` is entailed iff
//! `KB ∪ {¬R(a,b)}` is inconsistent — sound by construction (never derived
//! from a satisfying model).
//!
//! **Data values**: structural passthrough only (v1 boundary) —
//! `materialize_data_property_assertions`'s 5-tuples with `lang` dropped to a
//! 4-tuple. No entailment probe; `incomplete` is always `false` (complete for
//! that structural fragment, same as `materialize_data_property_assertions`
//! itself).
use crate::{PreparedOntology, ReasonError};
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ir::{ConceptExpr, ConceptId, ConceptPool, IndividualId, RoleId};
use owl_dl_core::ontology::{Axiom, InternalOntology, SubRolePath};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Entailed OBJECT property triples over named individuals, plus a
/// completeness flag.
#[derive(Debug, Clone)]
pub struct ObjectPropertyValues {
    triples: Vec<(String, String, String)>,
    incomplete: bool,
}

impl ObjectPropertyValues {
    /// `(subject_iri, property_iri, object_iri)` triples, sorted and
    /// deduplicated.
    #[must_use]
    pub fn triples(&self) -> &[(String, String, String)] {
        &self.triples
    }

    /// `true` iff: the ontology contains an axiom outside
    /// [`object_property_edge_complete`]'s whitelist — the sound
    /// over-approximation of the `ABox` saturator's (`abox_saturation::
    /// saturate_abox_consistency`, which this query's seed actually runs)
    /// genuinely edge-complete fragment (e.g. a conjunctive antecedent
    /// `SubClassOf(A ⊓ B, C)`, or a disjunctive `C ⊑ ∃R.{b} ⊔ ∃R.{c}` — either
    /// can entail an edge the seed's Horn-only propagation never derives);
    /// OR a bounded-extension probe timed out (`None`); OR the bounded
    /// (non-exhaustive) extension ran at all (even when it added nothing —
    /// running a probe at all means the answer is no longer provably the
    /// full closure). Object values beyond the seed neighborhood — i.e.
    /// entailed edges between individuals that never co-occur in a seed edge
    /// — may be missed whenever this is `true`. `false` is a genuine
    /// guarantee: every entailed object-property edge over named individuals
    /// is included. It requires BOTH that every axiom is in
    /// `object_property_edge_complete`'s whitelist AND that the bounded
    /// extension found no candidate pair to probe at all.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Entailed DATA property values over named individuals, plus a completeness
/// flag.
#[derive(Debug, Clone)]
pub struct DataPropertyValues {
    quints: Vec<(String, String, String, String, String)>,
    incomplete: bool,
}

impl DataPropertyValues {
    /// `(subject_iri, property_iri, lexical, datatype_iri, lang)` 5-tuples,
    /// sorted and deduplicated.
    ///
    /// `lang` is the language tag when the datatype is `rdf:langString`, and
    /// EMPTY otherwise — the same representation
    /// [`crate::materialize_data_property_assertions`] uses, so this is a
    /// passthrough with no lossy conversion in between.
    /// It is part of the KEY, not decoration: under RDF semantics
    /// `"bonjour"@fr` and `"bonjour"@de` are DISTINCT literals, so dropping
    /// the tag before `dedup` (which is what this did until 2026-08-26, issue
    /// #72) silently merged them into one row — losing an assertion outright
    /// when they shared a subject, not merely losing the tag.
    #[must_use]
    pub fn quints(&self) -> &[(String, String, String, String, String)] {
        &self.quints
    }

    /// Always `false`: `inferred_data_property_values` is a pure structural
    /// passthrough over `materialize_data_property_assertions`, which is
    /// complete for its own (structural) fragment. Kept as a field (not a
    /// constant) so the type stays parallel to `ObjectPropertyValues` and free
    /// to grow a real entailment extension later without an API break.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Named object properties declared in `internal` — i.e. genuine
/// `Axiom::DeclareObjectProperty` entries. Deliberately NOT
/// `vocabulary.roles()`: that namespace also holds data properties lowered to
/// roles (first-class data properties, `RUSTDL_DATA_PROPERTIES`), which must
/// not appear in the object-property candidate cross product.
fn named_object_properties(internal: &owl_dl_core::ontology::InternalOntology) -> Vec<RoleId> {
    let mut out: Vec<RoleId> = internal
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::DeclareObjectProperty(r) => Some(*r),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Canonical `(lo, hi)` ordering by index for a symmetric pair key.
fn canon_key(a: IndividualId, b: IndividualId) -> (u32, u32) {
    let (x, y) = (a.index(), b.index());
    if x <= y { (x, y) } else { (y, x) }
}

/// The bounded candidate-pair policy: distinct named-individual pairs that
/// co-occur in at least one seed edge (in either direction). This is
/// deliberately NOT the full `|individuals|²` cross product — it is bounded by
/// the number of DISTINCT pairs actually touched by the seed, which is at most
/// `2 × |seed edges|`. Both orderings of each unordered pair are candidates
/// (an entailed edge need not run in the same direction as the seed edge that
/// put the pair in the neighborhood — e.g. an inverse-role entailment).
fn candidate_extension_pairs(
    seed: &[(IndividualId, RoleId, IndividualId)],
) -> Vec<(IndividualId, IndividualId)> {
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut pairs: Vec<(IndividualId, IndividualId)> = Vec::new();
    for &(a, _, b) in seed {
        if seen.insert(canon_key(a, b)) {
            pairs.push((a, b));
            if a != b {
                pairs.push((b, a));
            }
        }
    }
    pairs
}

// ─── Sound over-approximation gate for `incomplete` (PR #50 review Fix 2,
// "proper" pass) ─────────────────────────────────────────────────────────────
//
// The gate previously used here (`!matches!(analyze_fragment(&internal),
// PureEl | Horn)`) was a MISMATCHED proxy: `analyze_fragment` measures the
// classification wedge / EL-saturation engine's completeness, not the ABox
// saturator's (`abox_saturation::saturate_abox_consistency`, which
// `materialize_object_property_assertions` — this query's seed — actually
// runs). The mismatch under-reports: a Horn-classified TBox with a
// conjunctive antecedent (`SubClassOf(A ⊓ B, C)`) was reported
// `incomplete = false` even though the ABox saturator's own `SubClassOf`
// indexing silently DROPS the entire axiom whenever the sub-concept is
// non-atomic (`atomic_class(*sub)` returning `None` in `abox_saturation.rs`),
// so a genuinely entailed edge (`C ⊑ ∃R.{c}` firing once an individual gets
// type `C`) was missed while `incomplete` claimed otherwise.
//
// `object_property_edge_complete` replaces that gate with a predicate keyed
// EXACTLY to the shapes `abox_saturation::saturate_abox_consistency` provably
// captures for named-individual object-property EDGE derivation. It is a
// SOUND OVER-APPROXIMATION: `true` only when every axiom is a shape
// enumerated below; ANY axiom outside the whitelist ⇒ `false`.
// Under-reporting completeness is acceptable; over-reporting (the bug this
// replaces) is not.
//
// ## Whitelist (enumerated by reading `abox_saturation.rs`'s indexing loop,
// `collect_existentials`/`collect_hasvalues`, and the edge-drain rules in
// full)
//
// TBox / class axioms:
// - `SubClassOf { sub, sup }`: `sub` MUST be `Atomic` — `atomic_class(*sub)`
//   gates the ENTIRE axiom in `abox_saturation.rs` (type propagation via
//   `sub_of`, existential markers via `existential_of`, AND `ObjectHasValue`
//   ground-edge markers via `has_value_of` are all skipped together for any
//   non-atomic `sub`: `And`/`Or`/`Some`/`Not`/cardinality/nominal). `sup`
//   must satisfy [`is_edge_complete_concept`] below.
// - `EquivalentClasses(members)`: ALL members must be `Atomic`. The saturator
//   only derives "each atomic peer ⊑ each OTHER atomic peer" for atomic
//   members; for a conjunctive member (`C ≡ D1 ⊓ D2`) the REVERSE direction
//   (`D1 ⊓ D2 ⊑ C`, needed when an individual separately holds types D1 and
//   D2) is explicitly NOT added — see the "we deliberately do NOT add the
//   reverse direction" comment in `abox_saturation.rs` — the same
//   conjunctive-antecedent gap as `SubClassOf`, so any non-atomic member
//   rejects the whole axiom.
// - `DisjointClasses(_)`: edge-irrelevant. It only ever DETECTS a clash
//   (Rule 8) over types already derived by other axioms; it cannot itself
//   entail a new type or edge, so it is safe regardless of member shape.
// - `DisjointUnion { .. }`: **NOT whitelisted**, despite superficially
//   resembling `DisjointClasses`. `DisjointUnion(P, [C1, C2, ...])` also
//   entails `Ci ⊑ P` for every member — a genuine `SubClassOf`-shaped type
//   entailment — but `abox_saturation.rs` has NO `DisjointUnion` arm at all
//   (falls to the catch-all `_ => {}`), so that entailment is silently
//   dropped: a real edge-completeness gap. (`classify.rs`'s
//   `is_saturator_axiom` documents the identical reasoning for its own,
//   different, saturator/fragment gate — same root cause, independently
//   confirmed here for this module.)
//
// RBox / role axioms:
// - `SubObjectPropertyOf { sub, sup }`: `sub` as a single `Role` MUST be
//   non-inverse. The saturator's role-hierarchy lookup at edge-drain time is
//   keyed `(role_id, false)` ONLY (`role_super.get(&(rid, false))`), but
//   insertion keys on `role_key(*sub)` — so an axiom whose `sub` is an
//   `Inverse` role is indexed under `(role_id, true)` and NEVER matched: a
//   dead, silently-dropped entry. `sup`'s polarity is applied correctly at
//   push time regardless, so no restriction is needed there. `sub` as a
//   `Chain` is captured for length 2 or 3 ONLY (`chains2`/`chains3`); any
//   other length is the documented "longer chains not supported" drop.
//   Chain leg polarities are all handled correctly (the chain matcher
//   re-derives direction from `r*_inv` at read time — unlike the single-role
//   hierarchy lookup, it is not keyed by a fixed-polarity map slot), so no
//   non-inverse restriction is needed for chain legs.
// - `EquivalentObjectProperties(_)`: **NOT whitelisted** — `abox_saturation.rs`
//   has no arm for it at all (falls to `_ => {}`); `R ≡ S` plus edge
//   `R(a,b)` should entail `S(a,b)`, but nothing derives it.
// - `DisjointObjectProperties(_)`: edge-irrelevant (no arm, but a pure
//   negative constraint — it cannot itself entail a new edge).
// - `InverseObjectProperties(_, _)`: fully captured (`inverse_rules`,
//   consulted at `(role_id, false)` where `role_id` is ALWAYS the base
//   `role_id()` of the declared role regardless of that role's OWN polarity
//   — insertion is keyed the same way, so no restriction is needed).
// - `ObjectPropertyDomain { role, domain }` / `ObjectPropertyRange { role,
//   range }`: `role` MUST be non-inverse (the identical `(role_id,
//   false)`-only drain-time lookup bug as `SubObjectPropertyOf`'s
//   single-role case: `domains`/`ranges` are populated under
//   `role_key(*role)` but drained only at `(rid, false)`). `domain`/`range`
//   must be `Atomic`, or `And` of ALL-atomic parts (the indexing loop's
//   `And`-unpacking silently drops any non-atomic conjunct — conservative:
//   reject if any part isn't atomic).
// - `TransitiveRole` / `SymmetricRole`: fully captured regardless of the
//   declared role's own polarity — both register through the
//   polarity-general chain / `inverse_rules` machinery (not the restricted
//   `role_super` path).
// - `FunctionalRole` / `InverseFunctionalRole`: **NOT whitelisted** —
//   NOT edge-safe, despite the merge loop itself scanning the raw edge set
//   directly by role id with no keyed lookup (that part IS captured). The
//   gap is downstream of the merge: Rule 7 (`abox_saturation.rs`) propagates
//   only TYPES between the functionally-forced-equal fillers, never edges —
//   edge-folding onto a merged pair only happens via Rule 9b, which fires
//   exclusively for an EXPLICIT `SameIndividual` axiom, not for identity
//   forced by functionality/inverse-functionality. So a named-individual
//   identity forced by this axiom can leave a genuinely entailed
//   object-property edge between two individuals that never co-occur in a
//   seed edge — the bounded extension probe (`candidate_extension_pairs`)
//   never gets a chance to check that pair either. Concretely:
//   `InverseFunctionalObjectProperty(R)`, `SymmetricObjectProperty(R)`,
//   `R(a,b)`, `R(a,c)`, `R(b,e)` entails `R(c,e)` (via `R(b,a)`/`R(c,a)` from
//   symmetry, then `b=c` from inverse-functionality) but `(c,e)` is never a
//   seed pair. `FunctionalRole` happens to be incidentally shadowed today by
//   `convert.rs` co-emitting a non-atomic `≤1` GCI that this predicate's
//   `SubClassOf`/concept-shape checks already reject — but that is a
//   coincidence of a DIFFERENT lowering pass, not a property of this
//   predicate, and `InverseFunctionalRole` has no such GCI, so it is directly
//   exploitable. Both are rejected explicitly here rather than relying on
//   that incidental coupling.
// - `AsymmetricRole` / `IrreflexiveRole`: edge-irrelevant — pure negative
//   constraints (no arm, and none needed: neither can entail a new edge).
// - `ReflexiveRole`: **NOT whitelisted**. `ReflexiveRole(R)` entails
//   `R(a,a)` for EVERY named individual `a` — a genuine, always-missed edge,
//   since `abox_saturation.rs` has no reflexivity handling at all.
//
// ABox axioms:
// - `ClassAssertion { class, individual }`: `class` must satisfy
//   [`is_edge_complete_concept`] (the SAME recursive check as a `SubClassOf`
//   head — deliberately shared and conservative: a bare `Not`/`Or`/`Some`
//   filler on a `ClassAssertion` is rejected even though some such shapes
//   are harmless no-ops for the seed, because distinguishing "harmless
//   no-op" from "silently-dropped disjunctive entailment" case-by-case is
//   exactly the kind of judgment call the "when in doubt, false" rule is
//   meant to foreclose).
// - `ObjectPropertyAssertion` / `SameIndividual`: fully captured (direct
//   edge seed; union-find + types/edges propagation rules 9a/9b).
// - `NegativeObjectPropertyAssertion` / `DifferentIndividuals`: edge-
//   irrelevant (no arm; both are negative/non-generative facts that cannot
//   themselves entail a positive edge via this saturator).
//
// Declarations (`DeclareClass` / `DeclareObjectProperty` /
// `DeclareNamedIndividual`): always safe — no semantic content.

/// Recursive concept-shape check shared by `SubClassOf`'s head / RHS,
/// `EquivalentClasses`' complex members, and `ClassAssertion`'s class
/// expression — the exact shapes `abox_saturation.rs`'s `collect_existentials`
/// / `collect_hasvalues` / `sub_of`-`And`-unpacking capture:
/// - `Top` / `Bot` / `Atomic`: leaves that are always safe — `Top` and `Bot`
///   contribute no type/edge in named-only semantics (genuine no-ops for this
///   saturator, not a silently-dropped entailment).
/// - `And(parts)`: safe iff every conjunct is (recursively).
/// - `Some(role, filler)` where `filler` is `Nominal`: an `ObjectHasValue`
///   ground-edge marker, captured regardless of `role`'s polarity
///   (`collect_hasvalues` stores the whole `Role`, normalized correctly at
///   use).
/// - `Some(role, filler)` where `filler` is `Atomic`: a plain existential
///   marker, captured ONLY for a non-inverse (Named) `role`
///   (`collect_existentials` checks `!r.is_inverse()` explicitly).
/// - anything else (`Or`, `Not`, `All`, `Min`, `Max`, a bare `Nominal`,
///   `SelfRestriction`, or a `Some` with any other filler shape): NOT
///   captured ⇒ `false`.
fn is_edge_complete_concept(cid: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(cid) {
        ConceptExpr::Top | ConceptExpr::Bot | ConceptExpr::Atomic(_) => true,
        ConceptExpr::And(parts) => parts.iter().all(|&p| is_edge_complete_concept(p, pool)),
        ConceptExpr::Some(role, filler) => match pool.get(*filler) {
            ConceptExpr::Nominal(_) => true,
            ConceptExpr::Atomic(_) => !role.is_inverse(),
            _ => false,
        },
        _ => false,
    }
}

/// `ObjectPropertyDomain`/`ObjectPropertyRange`'s concept argument: the
/// indexing loop only ever populates `domains`/`ranges` from an `Atomic`
/// concept, or (unpacking one level) an `And` whose parts are individually
/// `Atomic` — any non-atomic conjunct is silently skipped for THAT part
/// (never rejecting the whole axiom in `abox_saturation.rs` itself, but we
/// conservatively require every conjunct be atomic here so no entailed
/// domain/range type is ever silently lost).
fn is_edge_complete_domain_range(cid: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(cid) {
        ConceptExpr::Atomic(_) => true,
        ConceptExpr::And(parts) => parts
            .iter()
            .all(|&p| matches!(pool.get(p), ConceptExpr::Atomic(_))),
        _ => false,
    }
}

/// Per-axiom whitelist check — see the module-level doc block above this
/// function for the full enumeration and justification of every arm.
///
/// `match_same_arms` is deliberately silenced: several arms share a body
/// (`true`/`false`) purely by coincidence of outcome, not of reasoning — each
/// is enumerated and justified separately above (mirrors the same allow on
/// `abox_saturation.rs`'s own rule-matching code, for the same readability
/// reason: collapsing arms with different justifications into one pattern
/// would make future edits (e.g. discovering a new gap in one shape but not
/// its neighbor) error-prone).
#[allow(clippy::match_same_arms)]
fn is_edge_complete_axiom(ax: &Axiom, pool: &ConceptPool) -> bool {
    match ax {
        Axiom::SubClassOf { sub, sup } => {
            matches!(pool.get(*sub), ConceptExpr::Atomic(_)) && is_edge_complete_concept(*sup, pool)
        }
        Axiom::EquivalentClasses(members) => members
            .iter()
            .all(|&c| matches!(pool.get(c), ConceptExpr::Atomic(_))),
        Axiom::DisjointClasses(_) => true,
        Axiom::DisjointUnion { .. } => false,
        Axiom::SubObjectPropertyOf { sub, .. } => match sub {
            SubRolePath::Role(r) => !r.is_inverse(),
            SubRolePath::Chain(roles) => roles.len() == 2 || roles.len() == 3,
        },
        Axiom::EquivalentObjectProperties(_) => false,
        Axiom::DisjointObjectProperties(_) => true,
        Axiom::InverseObjectProperties(_, _) => true,
        Axiom::ObjectPropertyDomain { role, domain } => {
            !role.is_inverse() && is_edge_complete_domain_range(*domain, pool)
        }
        Axiom::ObjectPropertyRange { role, range } => {
            !role.is_inverse() && is_edge_complete_domain_range(*range, pool)
        }
        Axiom::TransitiveRole(_) | Axiom::SymmetricRole(_) => true,
        Axiom::FunctionalRole(_) | Axiom::InverseFunctionalRole(_) => false,
        Axiom::AsymmetricRole(_) | Axiom::IrreflexiveRole(_) => true,
        Axiom::ReflexiveRole(_) => false,
        Axiom::ClassAssertion { class, .. } => is_edge_complete_concept(*class, pool),
        Axiom::ObjectPropertyAssertion { .. } => true,
        Axiom::NegativeObjectPropertyAssertion { .. } => true,
        Axiom::SameIndividual(_) => true,
        Axiom::DifferentIndividuals(_) => true,
        Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => true,
    }
}

/// `true` iff EVERY axiom in `internal` is provably within the `ABox`
/// saturator's (`abox_saturation::saturate_abox_consistency`) edge-complete
/// fragment for named-individual object-property edges — see the module-level
/// doc block above for the full whitelist and per-shape justification. A
/// sound OVER-approximation: `false` whenever any axiom's capture is not
/// proven (never the reverse).
pub(crate) fn object_property_edge_complete(internal: &InternalOntology) -> bool {
    let pool = &internal.concepts;
    internal
        .axioms
        .iter()
        .all(|ax| is_edge_complete_axiom(ax, pool))
}

/// Inferred OBJECT property values over named individuals: the sound
/// `materialize_object_property_assertions` seed, extended by a bounded
/// entailment probe over the seed's own individual-pair neighborhood (see the
/// module doc and [`candidate_extension_pairs`]). `pair_deadline` bounds each
/// extension probe; `None` = unbounded.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent (everything
/// is vacuously entailed); [`ReasonError::Conversion`] on lowering failure.
pub fn inferred_object_property_values<A: ForIRI>(
    onto: &SetOntology<A>,
    pair_deadline: Option<Duration>,
) -> Result<ObjectPropertyValues, ReasonError> {
    // Sound lower-bound seed; already runs the inconsistent-KB guard and
    // returns `Err(Inconsistent)` on a clash.
    let seed = crate::materialize_object_property_assertions(onto)?;
    let mut seed_set: HashSet<(String, String, String)> = seed.iter().cloned().collect();
    let mut triples = seed.clone();

    let internal = convert_ontology(onto)?;
    let object_properties = named_object_properties(&internal);

    // Honest `incomplete` initialization (review Fix 2, "proper" pass): gate
    // directly on whether every axiom is within the ABox saturator's OWN
    // provably edge-complete fragment (`object_property_edge_complete`,
    // below) — NOT on `analyze_fragment`'s `PureEl`/`Horn`, which measures a
    // DIFFERENT engine (the classification wedge/EL-saturator) and is a
    // mismatched proxy: a Horn-classified TBox can still contain a
    // conjunctive antecedent (`SubClassOf(A ⊓ B, C)`) that the ABox
    // saturator's `SubClassOf` indexing silently drops in its entirety
    // (non-atomic `sub` ⟹ no type propagation, no existential marker, no
    // `ObjectHasValue` ground-edge marker for that axiom at all — see
    // `abox_saturation.rs`), which previously left `incomplete = false` while
    // a real edge went undetected. See `object_property_edge_complete`'s doc
    // for the full enumerated whitelist.
    let mut incomplete = !object_property_edge_complete(&internal);

    // `from_internal` clones `internal.vocabulary` before consuming
    // `internal`, so `prepared.vocabulary` resolves the same IRI ↔ id mapping
    // used to build the seed triples above.
    let prepared = PreparedOntology::from_internal(internal)?;
    let vocab = &prepared.vocabulary;

    // Resolve the seed's string triples back to ids so the candidate-pair
    // neighborhood can be computed over ids (needed for `consistent_with_extra`).
    let mut seed_ids: Vec<(IndividualId, RoleId, IndividualId)> = Vec::new();
    for (s, p, o) in &seed {
        if let (Some(a), Some(r), Some(b)) = (
            vocab.individual_id(s),
            vocab.role_id(p),
            vocab.individual_id(o),
        ) {
            seed_ids.push((a, r, b));
        }
    }

    let candidate_pairs = candidate_extension_pairs(&seed_ids);
    for (a, b) in candidate_pairs {
        let a_iri = vocab.individual_iri(a).to_string();
        let b_iri = vocab.individual_iri(b).to_string();
        for &r in &object_properties {
            let r_iri = vocab.role_iri(r).to_string();
            let candidate = (a_iri.clone(), r_iri, b_iri.clone());
            if seed_set.contains(&candidate) {
                continue;
            }
            incomplete = true;
            let deadline = pair_deadline.map(|d| Instant::now() + d);
            if prepared.consistent_with_extra(&[], &[(a, r, b)], deadline)? == Some(false) {
                // KB ∪ {¬R(a,b)} is inconsistent ⇒ R(a,b) is entailed.
                seed_set.insert(candidate.clone());
                triples.push(candidate);
            }
        }
    }

    triples.sort();
    triples.dedup();
    Ok(ObjectPropertyValues {
        triples,
        incomplete,
    })
}

/// Inferred DATA property values over named individuals: a pure structural
/// passthrough over `materialize_data_property_assertions`, preserving the
/// full 5-tuple INCLUDING the language tag. See the module doc for the v1
/// scope boundary (no negative-data-assertion entailment extension).
///
/// The tag must survive into the dedup key. It did not until 2026-08-26
/// (issue #72), and because `dedup` ran on the truncated tuple, two
/// language-tagged assertions on one subject sharing a lexical form —
/// `"bonjour"@fr` and `"bonjour"@de` — collapsed to a single row. That is
/// data loss, not a formatting choice, and it was silent.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] on lowering failure.
pub fn inferred_data_property_values<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<DataPropertyValues, ReasonError> {
    let mut quints = crate::materialize_data_property_assertions(onto)?;
    quints.sort();
    quints.dedup();
    Ok(DataPropertyValues {
        quints,
        incomplete: false,
    })
}
