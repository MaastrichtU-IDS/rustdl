//! Phase D4 (2026-06-03): preprocessing pass that recognizes specific
//! OWL 2 data-axiom patterns and emits derived class-level axioms,
//! sidestepping the need for full ConceptExpr extensions or tableau-
//! side data-cardinality reasoning.
//!
//! Patterns recognized (drives `derive_data_axioms`):
//!
//! 1. **Functional + DataMin clash** (closes
//!    `tests/datatype_completeness::functional_data_property_unsat`):
//!    `FunctionalDataProperty(dp)` ≡ `Top ⊑ ≤1 dp`; combined with any
//!    `SubClassOf(C, ≥n dp)` for `n ≥ 2`, derive `C ⊑ Bot`.
//!
//! 2. **DataMin/Max intra-class clash** (closes
//!    `tests/datatype_completeness::data_cardinality_disjointness`):
//!    `SubClassOf(C, ≥n dp)` + `SubClassOf(C, ≤m dp)` with `n > m`
//!    ⇒ `C ⊑ Bot`. Handles `EquivalentClasses(C, ObjectIntersectionOf(A, B))`
//!    by collecting bounds across all decomposed conjuncts.
//!
//! 3. **DataPropertyDomain inference** (closes
//!    `tests/datatype_completeness::data_property_domain_inference`):
//!    `DataPropertyDomain(dp, D)` + `SubClassOf(C, DataSome(dp, _))`
//!    ⇒ `C ⊑ D`. We treat the data-range as opaque.
//!
//! 4. **SubDataPropertyOf transitivity** (closes
//!    `tests/datatype_completeness::sub_data_property_transitivity`):
//!    `SubDataPropertyOf(specific, general)` lifts `DataSome(specific, _)`
//!    to `DataSome(general, _)` for subsumption purposes. Combined with
//!    `SubClassOf(C, DataSome(specific, _))` and
//!    `SubClassOf(DataSome(general, _), D)`, derive `C ⊑ D`. Hierarchy
//!    is transitively closed over `SubDataPropertyOf` chains.
//!
//! Patterns NOT addressed (remain MISSED — Tier C):
//! - Datatype facet conflict (`xsd:integer min/max` on `Functional(age)`
//!   producing `Adult ⊓ Child ⊑ Bot`).
//! - Inter-class inherited bounds (a class's bounds propagated to its
//!   subclasses transitively). Patterns above are intra-class only —
//!   subclass-inherited bounds aren't checked. Phase D5 work if needed.
//! - DataPropertyRange-induced contradictions.
//!
//! Soundness: every emitted axiom is sound by direct logical derivation
//! from the patterns above. False positives would require a pattern-
//! matching bug; corpus-validated on `tests/datatype_completeness` + the
//! Phase 0 net (alehif/ore-10908/ore-15672 — these have no data axioms
//! so the preprocessing pass is a no-op on them, but they verify no
//! regression).

#![allow(clippy::doc_markdown)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{
    ClassExpression, Component, DataProperty, DataRange, FacetRestriction, ForIRI, Literal,
};
use horned_owl::ontology::set::SetOntology;
use horned_owl::vocab::Facet;

use crate::Vocabulary;
use crate::ir::{ClassId, ConceptId};
use crate::ontology::Axiom;

/// Extract data-property facts from the source ontology and derive
/// class-level subsumption / unsat axioms per the patterns documented
/// in the module-level comment. Returns axioms ready to append to the
/// `InternalOntology::axioms` vector.
///
/// `vocab` and `concepts` are read-only: classes referenced in the
/// derived axioms must already be interned (the caller runs this AFTER
/// the main `convert_ontology` pass has populated the vocabulary).
/// `bot_id` is the pool's interned `Bot` (used in derived unsat axioms).
pub fn derive_data_axioms<A: ForIRI>(
    src: &SetOntology<A>,
    vocab: &Vocabulary,
    top_id: ConceptId,
    bot_id: ConceptId,
    atomic_id: impl Fn(ClassId) -> ConceptId,
) -> Vec<Axiom> {
    let mut facts = extract_facts(src);
    // Phase D4: propagate bounds through Intersection-equivalence
    // (`EquivalentClasses(C, ObjectIntersectionOf(M1, M2, ...))` lifts
    // bounds from each atomic Mi to C). Iterate to fixpoint — chains
    // of definitions (C₁ ≡ … ⊓ C₂, C₂ ≡ … ⊓ C₃, …) propagate up.
    // Bounded by class count × dp count; small in practice.
    propagate_intersection_bounds(src, &mut facts);
    let mut out = Vec::new();
    emit_clashes(&facts, vocab, bot_id, &atomic_id, &mut out);
    emit_domain_inferences(&facts, vocab, &atomic_id, &mut out);
    emit_subdataprop_transitivity(&facts, vocab, &atomic_id, &mut out);
    emit_data_range_violations(&facts, top_id, bot_id, &mut out);
    emit_data_oneof_violations(&facts, top_id, bot_id, &mut out);
    out
}

/// DP-1b: a string-`DataOneOf` membership VIOLATION ⇒ global inconsistency.
/// `DataPropertyAssertion(p, a, "v")` with `DataPropertyRange(q, DataOneOf(S))`
/// for a (reflexive) super-data-property `q` of `p` forces `"v" ∈ S`. When the
/// asserted string `"v"` is not an element of the enumerated set `S`, the value
/// is disallowed ⇒ no model ⇒ emit `Top ⊑ Bot`. Closes the `ore_ont_13219`
/// cluster (e.g. `""` asserted on a `{"all","driver",…}` enumeration).
///
/// **Sound by construction:** `DataOneOf(S)` as a *range* means every value of
/// the property must be a member of `S`; exact-string membership is decidable
/// and exact (`exact_string_literal` + `BTreeSet`). Only string enumerations
/// are handled (`parse_string_range` returns `Some(Set)` only when every member
/// is an `xsd:string` literal); mixed / typed-numeric `DataOneOf` ⇒ skipped.
/// Super-property direction only (matches DP-1).
fn emit_data_oneof_violations(
    f: &Facts,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    if f.data_string_assertions.is_empty() || f.dp_string_enums.is_empty() {
        return;
    }
    let closure = closure_sub_dp(&f.sub_data_property);
    for (p, value) in &f.data_string_assertions {
        // Enumerations on p, plus on every strict super-dp of p.
        let mut enums: Vec<&BTreeSet<String>> =
            f.dp_string_enums.get(p).into_iter().flatten().collect();
        if let Some(supers) = closure.get(p) {
            for q in supers {
                if q != p {
                    enums.extend(f.dp_string_enums.get(q).into_iter().flatten());
                }
            }
        }
        if enums.iter().any(|s| !s.contains(value)) {
            out.push(Axiom::SubClassOf {
                sub: top_id,
                sup: bot_id,
            });
            return;
        }
    }
}

/// DP-1: a data-property-range VIOLATION ⇒ **global inconsistency**.
/// `DataPropertyAssertion(p, a, lit)` together with `DataPropertyRange(q, R)`
/// for any (reflexive) super-data-property `q` of `p` forces `lit ∈ R`. When
/// `family(lit)` is value-space-disjoint from `family(R)` the value cannot
/// lie in the range, so the ontology has no model — emit `Top ⊑ Bot` once
/// (the pipeline reads that as inconsistent: every class becomes unsat,
/// mirroring Konclude). rustdl otherwise DROPS ABox data-property reasoning,
/// so this is a sound completeness gain, not a behaviour change on
/// data-clean inputs.
///
/// **Sound by construction (the false-`Inconsistent` gate):** only fires when
/// both families are classified ([`dt_family`] returns `None` on any
/// uncertainty) and *different* (every [`DtFamily`] variant is a distinct,
/// pairwise-disjoint value space; all numerics are merged so `int`/`decimal`/
/// `float` never cross-flag). Union/oneOf/complement ranges classify to
/// `None` ([`data_range_family`]) and are never flagged. Super-property
/// direction only (range of a *super*-dp constrains the *sub*-dp's values).
fn emit_data_range_violations(
    f: &Facts,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    if f.data_assertions.is_empty() || f.dp_range_families.is_empty() {
        return;
    }
    // Reflexive-transitive super-dp closure (dp → {dp} ∪ supers).
    let closure = closure_sub_dp(&f.sub_data_property);
    for (p, lit_fam) in &f.data_assertions {
        // Ranges directly on p …
        let mut applicable: Vec<DtFamily> = f.dp_range_families.get(p).cloned().unwrap_or_default();
        // … plus ranges on every strict super-dp of p.
        if let Some(supers) = closure.get(p) {
            for q in supers {
                if q != p {
                    applicable.extend(f.dp_range_families.get(q).into_iter().flatten().copied());
                }
            }
        }
        if applicable.iter().any(|rf| *rf != *lit_fam) {
            out.push(Axiom::SubClassOf {
                sub: top_id,
                sup: bot_id,
            });
            return;
        }
    }
}

/// Disjunctive-data-property-domain inference (closes the SAO/BFO cross-
/// ontology cluster, `docs/sao-bfo-chain-2026-06-10.md`).
///
/// For `DataPropertyDomain(dp, D₁ ⊔ … ⊔ Dₙ)` (all atomic) and every
/// class `C` that *uses* `dp` (a `DataHasValue` / `DataSomeValuesFrom` /
/// `DataMin≥1` / `DataExact≥1` super — i.e. `C` is in `class_some`,
/// which only captures mandatory-filler restrictions), the OWL domain
/// semantics give `C ⊑ ∃dp.⊤ ⊑ (D₁ ⊔ … ⊔ Dₙ)`. This returns the
/// `(C, [D₁ … Dₙ])` pairs (as `ClassId`s); the caller builds the bare
/// disjunctive GCI `SubClassOf(C, ObjectUnionOf(D₁ … Dₙ))` in the IR
/// (it owns the `ConceptPool`, which we don't here), after which
/// `disjunction_existential::derive_disjunction_existentials` reduces it
/// to `C ⊑ E` for each minimal common told-subsumer `E`.
///
/// **Sound**: every disjunct is atomic (the scan rejects non-atomic
/// unions), so the told tables see all of them; `Dᵢ ⊑ E ∀i ⟹
/// (⊔Dᵢ) ⊑ E ⟹ C ⊑ E`. Emits nothing when a referenced IRI is not
/// interned or when `C` is itself a disjunct (trivial).
pub fn derive_data_domain_unions<A: ForIRI>(
    src: &SetOntology<A>,
    vocab: &Vocabulary,
) -> Vec<(ClassId, Vec<ClassId>)> {
    let facts = extract_facts(src);
    let mut out = Vec::new();
    for (dp, disjunct_iris) in &facts.union_domains {
        // Resolve all disjunct IRIs once; skip the whole domain if any
        // is uninterned (keeps the common-subsumer set complete).
        let Some(disjunct_ids) = disjunct_iris
            .iter()
            .map(|iri| vocab.class_id(iri))
            .collect::<Option<Vec<ClassId>>>()
        else {
            continue;
        };
        for (class_iri, c_dp) in &facts.class_some {
            if c_dp != dp || disjunct_iris.contains(class_iri) {
                continue;
            }
            if let Some(c_id) = vocab.class_id(class_iri) {
                out.push((c_id, disjunct_ids.clone()));
            }
        }
    }
    out
}

/// Phase D4: for every `EquivalentClasses(C, ObjectIntersectionOf(M1, M2, ...))`
/// (or its decomposition into mutual SubClassOf), if any atomic Mi
/// has cardinality bounds on a data property dp, propagate those
/// bounds to C. Iterates to fixpoint to handle transitive defs.
#[allow(
    clippy::too_many_lines,
    reason = "single fixpoint with 4 facts to propagate; splitting hurts readability"
)]
fn propagate_intersection_bounds<A: ForIRI>(src: &SetOntology<A>, facts: &mut Facts) {
    // Collect: class_iri → vec of atomic-member iris from Intersection
    // equivalences. Includes EquivalentClasses(C, Intersection(...)) and
    // SubClassOf(C, Intersection(...)) (the SubClass-only direction also
    // propagates bounds soundly: C ⊑ ⊓Mi → C inherits Mi's bounds).
    let mut inherited_from: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for ac in src {
        match &ac.component {
            Component::EquivalentClasses(ax) => {
                let atomic_members: Vec<String> = ax.0.iter().filter_map(class_iri).collect();
                let intersection_members = ax.0.iter().filter_map(|ce| {
                    if let ClassExpression::ObjectIntersectionOf(parts) = ce {
                        Some(parts)
                    } else {
                        None
                    }
                });
                for parts in intersection_members {
                    let part_iris: Vec<String> = parts.iter().filter_map(class_iri).collect();
                    for owner in &atomic_members {
                        for part in &part_iris {
                            if owner != part {
                                inherited_from
                                    .entry(owner.clone())
                                    .or_default()
                                    .insert(part.clone());
                            }
                        }
                    }
                }
            }
            Component::SubClassOf(ax) => {
                if let (Some(owner), ClassExpression::ObjectIntersectionOf(parts)) =
                    (class_iri(&ax.sub), &ax.sup)
                {
                    for part in parts.iter().filter_map(class_iri) {
                        if owner != part {
                            inherited_from
                                .entry(owner.clone())
                                .or_default()
                                .insert(part);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Fixpoint propagation. Each iteration: for each (owner, parents),
    // pull parents' bounds onto owner. Repeat until no change.
    let mut changed = true;
    while changed {
        changed = false;
        for (owner, parents) in &inherited_from {
            for parent in parents {
                // Min bounds: take parent's min if greater.
                let parent_mins: Vec<((String, String), u32)> = facts
                    .class_min
                    .iter()
                    .filter(|((c, _), _)| c == parent)
                    .map(|((_, dp), n)| ((owner.clone(), dp.clone()), *n))
                    .collect();
                for (key, n) in parent_mins {
                    let entry = facts.class_min.entry(key).or_insert(0);
                    if n > *entry {
                        *entry = n;
                        changed = true;
                    }
                }
                // Max bounds: take parent's max if smaller.
                let parent_maxes: Vec<((String, String), u32)> = facts
                    .class_max
                    .iter()
                    .filter(|((c, _), _)| c == parent)
                    .map(|((_, dp), n)| ((owner.clone(), dp.clone()), *n))
                    .collect();
                for (key, n) in parent_maxes {
                    let entry = facts.class_max.entry(key).or_insert(u32::MAX);
                    if n < *entry {
                        *entry = n;
                        changed = true;
                    }
                }
                // DataSome: inherit too (for domain inference).
                let parent_somes: Vec<(String, String)> = facts
                    .class_some
                    .iter()
                    .filter(|(c, _)| c == parent)
                    .map(|(_, dp)| (owner.clone(), dp.clone()))
                    .collect();
                for pair in parent_somes {
                    if facts.class_some.insert(pair) {
                        changed = true;
                    }
                }
                // Phase D5 (Tier C): integer ranges inherit too.
                let parent_ranges: Vec<((String, String), Vec<IntegerRange>)> = facts
                    .class_int_ranges
                    .iter()
                    .filter(|((c, _), _)| c == parent)
                    .map(|((_, dp), rs)| ((owner.clone(), dp.clone()), rs.clone()))
                    .collect();
                for (key, ranges) in parent_ranges {
                    let entry = facts.class_int_ranges.entry(key).or_default();
                    // Dedup-ish: only append ranges whose representation
                    // isn't already present (covers the common case where
                    // a chain of equivalences would otherwise grow Vec
                    // unboundedly on fixpoint iterations).
                    for r in ranges {
                        if !entry.iter().any(|e| e.min == r.min && e.max == r.max) {
                            entry.push(r);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
}

/// Phase D5 (Tier C): integer range with explicit inclusive bounds.
/// Used for `xsd:integer` `DatatypeRestriction` facets
/// (`minInclusive`, `minExclusive`, `maxInclusive`, `maxExclusive`).
/// `min`/`max = None` represents ±∞ open ends.
///
/// Closed-form intersection + emptiness check. Sound for OWL 2's
/// integer-typed value space; other numeric types (`xsd:decimal`,
/// `xsd:double`, `xsd:dateTime`) would extend with their own range
/// types but share the same algebra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntegerRange {
    pub(crate) min: Option<i64>,
    pub(crate) max: Option<i64>,
}

impl IntegerRange {
    pub(crate) const fn unbounded() -> Self {
        Self {
            min: None,
            max: None,
        }
    }

    /// A single integer point value `v`, i.e. the inclusive range `[v, v]`.
    pub(crate) const fn point(v: i64) -> Self {
        Self {
            min: Some(v),
            max: Some(v),
        }
    }

    /// `self ⊆ other` over the `xsd:integer` value space.
    ///
    /// An empty `self` is a subset of everything (the empty set is
    /// contained in every set). This empty-self short-circuit is a
    /// *completeness* aid — the bare bound comparison would otherwise
    /// (soundly, but incompletely) report empty-self as a non-subset.
    ///
    /// Non-empty case: every bound of `self` must be at least as tight
    /// as the corresponding bound of `other`. An unbounded end on
    /// `other` (`None`) imposes no constraint; an unbounded end on
    /// `self` against a bounded `other` end means `self` reaches past
    /// `other`, so it is NOT contained.
    pub(crate) fn subset(self, other: Self) -> bool {
        if self.is_empty() {
            return true;
        }
        let min_ok = match (self.min, other.min) {
            (_, None) => true,
            (Some(s), Some(o)) => s >= o,
            (None, Some(_)) => false,
        };
        let max_ok = match (self.max, other.max) {
            (_, None) => true,
            (Some(s), Some(o)) => s <= o,
            (None, Some(_)) => false,
        };
        min_ok && max_ok
    }

    fn intersect(self, other: Self) -> Self {
        let min = match (self.min, other.min) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => Some(if a > b { a } else { b }),
        };
        let max = match (self.max, other.max) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => Some(if a < b { a } else { b }),
        };
        Self { min, max }
    }
    fn is_empty(self) -> bool {
        matches!((self.min, self.max), (Some(a), Some(b)) if a > b)
    }

    /// Phase D11b: `self ∩ other = ∅` over the integer value space — the
    /// FP-critical predicate for the `∃p.DKey(v) ⊓ ∀p.DKey(r)` clash. MUST
    /// be conservative: `true` only when provably no integer is shared.
    /// Disjoint iff one range lies entirely below the other (inclusive
    /// bounds ⟹ a shared endpoint means they OVERLAP, not disjoint:
    /// `[0,5]`,`[5,10]` share `5`).
    pub(crate) fn disjoint(self, other: Self) -> bool {
        Self::strictly_below(self, other) || Self::strictly_below(other, self)
    }

    /// `a` ends before `b` starts (no shared integer): both ends finite and
    /// `a.max < b.min`. Unbounded ends (`None`) ⟹ not below (the range
    /// reaches ±∞), so the conservative `false`.
    fn strictly_below(a: Self, b: Self) -> bool {
        matches!((a.max, b.min), (Some(amax), Some(bmin)) if amax < bmin)
    }
}

/// Phase D6 (Part B): real-number range with EXPLICIT inclusive/exclusive
/// bounds. Used for `xsd:float` / `xsd:double` `DatatypeRestriction`
/// facets and float `DataHasValue` point values.
///
/// CRITICAL — unlike [`IntegerRange`], the `±1` exclusive↔inclusive
/// normalization is INVALID for the reals (there is no "next" real after
/// an excluded bound), so the inclusive/exclusive flag is carried
/// explicitly and consulted in [`FloatRange::subset`]. This is the
/// false-positive hotspot: any imprecision in the boundary comparison
/// is an unsound subsumption.
///
/// `min`/`max = None` represents the open (±∞) ends; the flag is
/// irrelevant when the bound is `None`. All stored `f64` values are
/// guaranteed finite (NaN / ±∞ are rejected at parse time → the whole
/// range drops, a sound under-approximation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FloatRange {
    pub(crate) min: Option<f64>,
    pub(crate) min_incl: bool,
    pub(crate) max: Option<f64>,
    pub(crate) max_incl: bool,
}

impl FloatRange {
    pub(crate) const fn unbounded() -> Self {
        Self {
            min: None,
            min_incl: false,
            max: None,
            max_incl: false,
        }
    }

    /// A single finite point value `v`, i.e. the closed range `[v, v]`.
    pub(crate) const fn point(v: f64) -> Self {
        Self {
            min: Some(v),
            min_incl: true,
            max: Some(v),
            max_incl: true,
        }
    }

    /// `self ⊆ other` over the real value space — the FP core.
    ///
    /// Every `x ∈ self` must satisfy `other`. For the lower bound:
    /// - `other` unbounded-below ⟹ no lower constraint, OK.
    /// - `self` unbounded-below but `other` bounded-below ⟹ `self`
    ///   reaches past `other`, NOT contained.
    /// - both bounded at `s`, `o`: OK iff `s > o`, OR (`s == o` AND
    ///   `other.min_incl || !self.min_incl`). The equal-endpoint clause
    ///   is the subtle part: if `other` EXCLUDES `o` but `self` INCLUDES
    ///   `o = s`, then `o ∈ self` yet `o ∉ other` → NOT a subset.
    ///
    /// Upper bound is symmetric. NaN can never reach here (rejected at
    /// parse), but the comparisons are written so a hypothetical NaN
    /// would fail every `>`/`==` branch → `subset = false` (sound).
    #[allow(
        clippy::float_cmp,
        reason = "EXACT IEEE-754 endpoint equality is the intended semantics — both \
                  operands originate from the same parsed literal / round-tripped \
                  to_bits key, so equal endpoints are bit-identical. An epsilon \
                  comparison would be UNSOUND (it would widen ranges, causing FP \
                  subsumptions). NaN is excluded at parse time."
    )]
    pub(crate) fn subset(self, other: Self) -> bool {
        let min_ok = match (self.min, other.min) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(s), Some(o)) => s > o || (s == o && (other.min_incl || !self.min_incl)),
        };
        let max_ok = match (self.max, other.max) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(s), Some(o)) => s < o || (s == o && (other.max_incl || !self.max_incl)),
        };
        min_ok && max_ok
    }

    /// Phase D11b: `self ∩ other = ∅` over the reals — conservative (`true`
    /// only when provably disjoint). Disjoint iff one range ends before the
    /// other begins; a SHARED endpoint counts as overlap ONLY when both
    /// sides include it (so `[0,5]`,`[5,10]` overlap at 5, but `[0,5)`,`[5,…]`
    /// or `[…,5]`,`(5,…]` are disjoint).
    #[allow(
        clippy::float_cmp,
        reason = "exact endpoint equality is intended — same datatype, bounds \
                  round-tripped through the same to_bits key; epsilon would \
                  WIDEN the disjoint region and cause a spurious ⊥ clash = FP"
    )]
    pub(crate) fn disjoint(self, other: Self) -> bool {
        fn below(a: FloatRange, b: FloatRange) -> bool {
            match (a.max, b.min) {
                (Some(amax), Some(bmin)) => {
                    amax < bmin || (amax == bmin && (!a.max_incl || !b.min_incl))
                }
                _ => false,
            }
        }
        below(self, other) || below(other, self)
    }
}

/// Phase D8 (2026-06-09): a totally-ordered range with EXPLICIT
/// inclusive/exclusive bounds, generic over an exactly-comparable key
/// `T: Ord`. Backs the `xsd:decimal`, `xsd:date`, and `xsd:dateTime`
/// `DKey` buckets — three domains that are dense-or-discrete TOTAL orders
/// once the soundness landmines are removed at parse time:
///
/// - **decimal** uses the exact [`Decimal`] lexical representation (NEVER
///   `f64` — `1.1`-decimal ≠ `1.1`-binary, and rounding two distinct
///   decimals to one `f64` would seed a spurious equality = FP).
/// - **date/dateTime** use component tuples ([`DateKey`] / [`DateTimeKey`]);
///   tuple order = chronological order with ZERO calendar arithmetic. The
///   xsd partial-order across timezone-presence is sidestepped by DROPPING
///   any value carrying a `Z`/offset at parse time (sound under-approx) —
///   so every key that reaches here is timezone-free and totally ordered.
///
/// The subset algebra is identical to [`FloatRange`] (explicit-boundary,
/// no ±1 normalization — valid for dense domains and harmless for the
/// discrete ones). Each `T` lives in its OWN `DKey` bucket: keys are never
/// compared across datatypes (see `seed_dkey_subsumptions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrdRange<T> {
    pub(crate) min: Option<T>,
    pub(crate) min_incl: bool,
    pub(crate) max: Option<T>,
    pub(crate) max_incl: bool,
}

impl<T: Ord + Clone> OrdRange<T> {
    pub(crate) const fn unbounded() -> Self {
        Self {
            min: None,
            min_incl: false,
            max: None,
            max_incl: false,
        }
    }

    /// A single point value `v`, i.e. the closed range `[v, v]`.
    pub(crate) fn point(v: T) -> Self {
        Self {
            min: Some(v.clone()),
            min_incl: true,
            max: Some(v),
            max_incl: true,
        }
    }

    /// `self ⊆ other` over the (timezone-free, totally ordered) value
    /// space. Identical structure to [`FloatRange::subset`] — the FP core.
    /// The equal-endpoint clause (`other` excludes `o` but `self` includes
    /// `o = s` ⟹ NOT a subset) is the subtle part; `Ord::cmp` gives exact
    /// equality so there is no widening.
    pub(crate) fn subset(&self, other: &Self) -> bool {
        let min_ok = match (&self.min, &other.min) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(s), Some(o)) => *s > *o || (*s == *o && (other.min_incl || !self.min_incl)),
        };
        let max_ok = match (&self.max, &other.max) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(s), Some(o)) => *s < *o || (*s == *o && (other.max_incl || !self.max_incl)),
        };
        min_ok && max_ok
    }

    /// Phase D11b: `self ∩ other = ∅` — conservative (`true` only when
    /// provably disjoint). Same explicit-boundary algebra as
    /// [`FloatRange::disjoint`] but exact via `Ord` (no float concerns): a
    /// shared endpoint is overlap only when both sides include it.
    pub(crate) fn disjoint(&self, other: &Self) -> bool {
        fn below<T: Ord>(a: &OrdRange<T>, b: &OrdRange<T>) -> bool {
            match (&a.max, &b.min) {
                (Some(amax), Some(bmin)) => {
                    *amax < *bmin || (*amax == *bmin && (!a.max_incl || !b.min_incl))
                }
                _ => false,
            }
        }
        below(self, other) || below(other, self)
    }

    /// Tighten the lower bound to the more restrictive of the existing
    /// bound and `(val, incl)` (larger value tighter; at equality
    /// exclusive beats inclusive). Symmetric to [`OrdRange::tighten_max`].
    fn tighten_min(&mut self, val: T, incl: bool) {
        let tighter = match &self.min {
            None => true,
            Some(e) => val > *e || (val == *e && !incl && self.min_incl),
        };
        if tighter {
            self.min = Some(val);
            self.min_incl = incl;
        }
    }

    fn tighten_max(&mut self, val: T, incl: bool) {
        let tighter = match &self.max {
            None => true,
            Some(e) => val < *e || (val == *e && !incl && self.max_incl),
        };
        if tighter {
            self.max = Some(val);
            self.max_incl = incl;
        }
    }
}

/// Phase D8: an exact `xsd:decimal` value, stored in NORMALIZED lexical
/// form so structural equality == value equality and the manual [`Ord`]
/// is exact. NEVER lossy — there is no `f64` anywhere on this path.
///
/// Normalization: `int` carries the integer digits with leading zeros
/// stripped (`""` = zero integer part); `frac` carries the fractional
/// digits with trailing zeros stripped (`""` = no fraction); `negative`
/// is forced `false` for the zero value so `0`, `-0`, `0.00` collapse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decimal {
    pub(crate) negative: bool,
    pub(crate) int: String,
    pub(crate) frac: String,
}

impl Decimal {
    /// Compare magnitudes (ignoring sign): integer part by length (no
    /// leading zeros ⟹ longer = larger), then lexically; then fractional
    /// part padded to equal length and compared lexically.
    fn mag_cmp(&self, other: &Self) -> Ordering {
        self.int
            .len()
            .cmp(&other.int.len())
            .then_with(|| self.int.cmp(&other.int))
            .then_with(|| cmp_frac(&self.frac, &other.frac))
    }
}

/// Lexicographic comparison of two normalized fractional-digit strings,
/// right-padding the shorter with `'0'` so e.g. `"5"` (0.5) > `"45"`
/// (0.45) compares as `"50"` vs `"45"`.
fn cmp_frac(a: &str, b: &str) -> Ordering {
    let n = a.len().max(b.len());
    let pad = |s: &str| {
        let mut t = s.to_string();
        t.push_str(&"0".repeat(n - s.len()));
        t
    };
    pad(a).cmp(&pad(b))
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.mag_cmp(other),
            // Both negative: larger magnitude ⟹ smaller value.
            (true, true) => other.mag_cmp(self),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Component key for `xsd:date`: `(year, month, day)`. Derived tuple order
/// is chronological for timezone-free dates (the only kind we accept).
pub(crate) type DateKey = (i64, u8, u8);
/// Component key for `xsd:dateTime`: `(year, month, day, hour, min, sec)`.
/// Integer-second, timezone-free only (fractional seconds / any `Z`/offset
/// are dropped at parse — sound under-approx).
pub(crate) type DateTimeKey = (i64, u8, u8, u8, u8, u8);

/// Parse an `xsd:decimal` lexical literal into a normalized [`Decimal`].
/// Conservative: returns `None` on any non-digit, an exponent (that is
/// `xsd:double`, a DIFFERENT value space), or an empty mantissa. A dropped
/// value contributes no constraint — never a wrong one.
pub(crate) fn parse_decimal(s: &str) -> Option<Decimal> {
    let (negative, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if rest.is_empty() {
        return None;
    }
    let (int_part, frac_part) = rest.split_once('.').unwrap_or((rest, ""));
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let int = int_part.trim_start_matches('0').to_string();
    let frac = frac_part.trim_end_matches('0').to_string();
    // Collapse the zero value's sign so `-0` == `0`.
    let negative = negative && !(int.is_empty() && frac.is_empty());
    Some(Decimal {
        negative,
        int,
        frac,
    })
}

/// Parse an `xsd:date` lexical literal `(-?)YYYY-MM-DD` into a [`DateKey`].
/// DROPS (returns `None`) anything carrying a timezone (`Z` or `±hh:mm`):
/// the xsd value space is only PARTIALLY ordered across timezone-presence,
/// so mixing zoned and unzoned would be unsound. Validates `month ∈ 1..=12`,
/// `day ∈ 1..=31` (no leap-precision needed — tuple order is exact for any
/// well-formed component triple).
pub(crate) fn parse_date(s: &str) -> Option<DateKey> {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    // Exactly three '-'-separated integer fields and nothing else; a tz
    // suffix (`Z`, `+hh:mm`, `-hh:mm`) leaves a non-numeric tail or an
    // extra field, so it fails this parse → dropped.
    let mut it = rest.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let mo: u8 = it.next()?.parse().ok()?;
    let d: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    Some((if neg { -y } else { y }, mo, d))
}

/// Parse an `xsd:dateTime` lexical literal `(-?)YYYY-MM-DDThh:mm:ss` into a
/// [`DateTimeKey`]. DROPS anything with fractional seconds (a `'.'` in the
/// time) or a timezone (`Z`/offset) — the sound first cut handles the
/// integer-second, timezone-free form by tuple comparison.
pub(crate) fn parse_datetime(s: &str) -> Option<DateTimeKey> {
    let (date_s, time_s) = s.split_once('T')?;
    let (y, mo, d) = parse_date(date_s)?;
    // Reject fractional seconds and any tz suffix outright.
    if time_s.bytes().any(|b| !(b.is_ascii_digit() || b == b':')) {
        return None;
    }
    let mut it = time_s.split(':');
    let h: u8 = it.next()?.parse().ok()?;
    let mi: u8 = it.next()?.parse().ok()?;
    let sec: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    if h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    Some((y, mo, d, h, mi, sec))
}

/// Generic facet folder for the [`OrdRange`] datatypes: intersect all
/// Min/Max Inclusive/Exclusive facets into one range. Any unrecognized
/// facet or any value `parse_val` rejects ⟹ `None` (drops the whole range,
/// which drops the whole axiom — the load-bearing conservatism).
fn parse_ord_facets<A: ForIRI, T: Ord + Clone>(
    facets: &[FacetRestriction<A>],
    parse_val: impl Fn(&str) -> Option<T>,
) -> Option<OrdRange<T>> {
    let mut range = OrdRange::unbounded();
    for fr in facets {
        let v = parse_val(fr.l.literal())?;
        match fr.f {
            Facet::MinInclusive => range.tighten_min(v, true),
            Facet::MinExclusive => range.tighten_min(v, false),
            Facet::MaxInclusive => range.tighten_max(v, true),
            Facet::MaxExclusive => range.tighten_max(v, false),
            _ => return None,
        }
    }
    Some(range)
}

/// Generic `DataRange` → [`OrdRange<T>`] parser shared by decimal / date /
/// dateTime. `matches_dt` selects the datatype IRI; a bare datatype is the
/// unbounded range; a `DatatypeRestriction` folds its facets.
fn parse_ord_range<A: ForIRI, T: Ord + Clone>(
    dr: &DataRange<A>,
    matches_dt: impl Fn(&str) -> bool,
    parse_val: impl Fn(&str) -> Option<T>,
) -> Option<OrdRange<T>> {
    match dr {
        DataRange::Datatype(dt) if matches_dt(dt.0.as_ref()) => Some(OrdRange::unbounded()),
        DataRange::DatatypeRestriction(dt, facets) if matches_dt(dt.0.as_ref()) => {
            parse_ord_facets(facets, parse_val)
        }
        _ => None,
    }
}

const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
const XSD_DATE: &str = "http://www.w3.org/2001/XMLSchema#date";
const XSD_DATETIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

/// Phase D8: parse an `xsd:decimal` `DataRange` into an exact range.
pub(crate) fn parse_decimal_range<A: ForIRI>(dr: &DataRange<A>) -> Option<OrdRange<Decimal>> {
    parse_ord_range(dr, |iri| iri == XSD_DECIMAL, parse_decimal)
}

/// Phase D8: parse an `xsd:date` `DataRange` into a component-tuple range.
pub(crate) fn parse_date_range<A: ForIRI>(dr: &DataRange<A>) -> Option<OrdRange<DateKey>> {
    parse_ord_range(dr, |iri| iri == XSD_DATE, parse_date)
}

/// Phase D8: parse an `xsd:dateTime` `DataRange` into a component-tuple range.
pub(crate) fn parse_datetime_range<A: ForIRI>(dr: &DataRange<A>) -> Option<OrdRange<DateTimeKey>> {
    parse_ord_range(dr, |iri| iri == XSD_DATETIME, parse_datetime)
}

const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";

/// Phase D9 (2026-06-09): an `xsd:string` value set — the EQUALITY-typed
/// (non-ordered) datatype. `Top` is the bare `xsd:string` (every string);
/// `Set` is a finite enumeration from `DataOneOf`. Subset is set-containment
/// (anything ⊆ `Top`; `Top` ⊄ a finite set). Completes the value-membership
/// fragment for strings: `DataHasValue(p,"x") ⊑ DataSomeValuesFrom(p, oneOf)`
/// iff `"x"` is a member, and `⊑ DataSomeValuesFrom(p, xsd:string)` always.
///
/// SOUNDNESS (the decimal-equality analog): only EXACT lexical identity
/// within `xsd:string` is set-equal. Language-tagged literals and any other
/// datatype are rejected at parse → the whole range/value drops (sound
/// under-approx), so two members can never spuriously coincide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StrSet {
    Top,
    Set(BTreeSet<String>),
}

impl StrSet {
    pub(crate) fn singleton(s: String) -> Self {
        StrSet::Set([s].into_iter().collect())
    }

    /// `self ⊆ other`: anything ⊆ `Top`; `Top` is a subset only of `Top`;
    /// two finite sets compare by ordinary set inclusion.
    pub(crate) fn subset(&self, other: &Self) -> bool {
        match (self, other) {
            (_, StrSet::Top) => true,
            (StrSet::Top, StrSet::Set(_)) => false,
            (StrSet::Set(a), StrSet::Set(b)) => a.is_subset(b),
        }
    }

    /// Phase D11b: `self ∩ other = ∅` — conservative. `Top` (= every string)
    /// overlaps everything, so it is NEVER disjoint; two finite enumerations
    /// are disjoint iff they share no member.
    pub(crate) fn disjoint(&self, other: &Self) -> bool {
        match (self, other) {
            (StrSet::Top, _) | (_, StrSet::Top) => false,
            (StrSet::Set(a), StrSet::Set(b)) => a.is_disjoint(b),
        }
    }
}

/// Extract an EXACT `xsd:string` value from a literal: `Simple` (untyped is
/// `xsd:string` by OWL 2) or `Datatype` tagged exactly `xsd:string`. ALL
/// other forms — language-tagged, or any non-string datatype — return
/// `None`, so they drop rather than risk a cross-datatype / lexical-vs-typed
/// coincidence.
pub(crate) fn exact_string_literal<A: ForIRI>(l: &Literal<A>) -> Option<String> {
    match l {
        Literal::Simple { literal } => Some(literal.clone()),
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == XSD_STRING => Some(literal.clone()),
        _ => None,
    }
}

/// Phase D9: parse a string-valued `DataRange` — bare `xsd:string` (→ `Top`)
/// or a `DataOneOf` whose members are ALL exact `xsd:string` literals
/// (→ `Set`). A `DataOneOf` with any non-string / language-tagged member
/// returns `None` (drops the whole enumeration — never a partial set, which
/// would be unsound in a sufficient-direction RHS).
pub(crate) fn parse_string_range<A: ForIRI>(dr: &DataRange<A>) -> Option<StrSet> {
    match dr {
        DataRange::Datatype(dt) if dt.0.as_ref() == XSD_STRING => Some(StrSet::Top),
        DataRange::DataOneOf(lits) if !lits.is_empty() => {
            let mut set = BTreeSet::new();
            for l in lits {
                set.insert(exact_string_literal(l)?);
            }
            Some(StrSet::Set(set))
        }
        _ => None,
    }
}

/// Internal: collected data-axiom facts. IRIs kept as `String` so we
/// can look them up in the vocabulary once at emission time.
#[derive(Default, Debug)]
struct Facts {
    /// Data properties declared `Functional`.
    functional_dps: BTreeSet<String>,
    /// `SubClassOf(C, ≥n dp)` or `SubClassOf(C, =n dp)` (the Min half
    /// of Exact). Keyed by `(class_iri, dp_iri)` → max-over-all-axioms
    /// of `n` (any conflicting min on the same key takes the larger).
    /// Also accumulates bounds from `EquivalentClasses(C, ObjectIntersectionOf(...))`
    /// when an Intersection conjunct is a data-cardinality restriction.
    class_min: BTreeMap<(String, String), u32>,
    /// `SubClassOf(C, ≤m dp)` or `SubClassOf(C, =m dp)` (the Max half
    /// of Exact). Keyed `(class_iri, dp_iri)` → min-over-all of `m`.
    class_max: BTreeMap<(String, String), u32>,
    /// `DataPropertyDomain(dp, D)` → dp_iri → domain class iri.
    /// Note: multiple domain axioms on the same dp produce a single
    /// class iri (last-write-wins). True OWL semantics intersect the
    /// domains; we approximate by emitting subsumptions for each
    /// observed domain class separately at emit time, which preserves
    /// soundness even with multiple domains.
    domains: Vec<(String, String)>,
    /// `DataPropertyDomain(dp, ObjectUnionOf(D₁ … Dₙ))` with **all**
    /// disjuncts atomic → `dp_iri → [D₁_iri … Dₙ_iri]`. A disjunctive
    /// domain is a sound `∃dp.⊤ ⊑ (D₁ ⊔ … ⊔ Dₙ)`; combined with a class
    /// `C` that uses `dp` (in `class_some`) it yields the bare
    /// disjunctive GCI `C ⊑ (D₁ ⊔ … ⊔ Dₙ)`, which the common-told-
    /// subsumer fold (`disjunction_existential`) reduces to `C ⊑ E`.
    /// Recorded only when every member is atomic — a non-atomic member
    /// is invisible to the told tables, so the common-subsumer step
    /// would be unsound (see `derive_data_domain_unions`).
    union_domains: Vec<(String, Vec<String>)>,
    /// `SubDataPropertyOf(specific, general)` edges. Hierarchy is
    /// transitively closed at emit time.
    sub_data_property: Vec<(String, String)>,
    /// `SubClassOf(C, DataSome(dp, _))` — class C exists with data
    /// property dp. The range is opaque (we don't track it).
    class_some: BTreeSet<(String, String)>,
    /// `SubClassOf(DataSome(dp, _), D)` — class D is a superset of
    /// "anything with data property dp". Range opaque.
    some_super: BTreeMap<String, BTreeSet<String>>,
    /// Phase D5 (Tier C): per-(class, dp) integer-range constraints
    /// derived from `SubClassOf(C, DataSome(dp, DatatypeRestriction(xsd:integer, ...)))`
    /// or equivalent in Min/Exact-cardinality forms. Multiple ranges
    /// accumulate; emit-time intersects them. Empty intersection on
    /// a Functional dp ⇒ C unsat.
    class_int_ranges: BTreeMap<(String, String), Vec<IntegerRange>>,
    /// DP-1: `DataPropertyRange(p, R)` → `p_iri → [family(R) …]` for every
    /// range `R` whose value-space family we can classify (bare datatype
    /// or `DatatypeRestriction`; union/oneOf/complement are skipped).
    dp_range_families: BTreeMap<String, Vec<DtFamily>>,
    /// DP-1: `DataPropertyAssertion(p, a, lit)` → `(p_iri, family(lit))`
    /// for every assertion whose literal family we can classify. The
    /// individual is irrelevant to the check (one violating value makes
    /// the whole ontology inconsistent), so we don't record it.
    data_assertions: Vec<(String, DtFamily)>,
    /// DP-1b: `DataPropertyRange(p, DataOneOf(strings))` → `p_iri →
    /// [enum-set …]` (the allowed string values). Only string enumerations
    /// (every member an `xsd:string` literal); mixed/non-string `DataOneOf`
    /// is skipped (`parse_string_range` returns `None`).
    dp_string_enums: BTreeMap<String, Vec<BTreeSet<String>>>,
    /// DP-1b: `DataPropertyAssertion(p, a, "v")` → `(p_iri, v)` for every
    /// `xsd:string`-typed asserted value.
    data_string_assertions: Vec<(String, String)>,
}

fn extract_facts<A: ForIRI>(src: &SetOntology<A>) -> Facts {
    let mut f = Facts::default();
    for ac in src {
        scan_component(&ac.component, &mut f);
    }
    f
}

fn scan_component<A: ForIRI>(c: &Component<A>, f: &mut Facts) {
    use Component as C;
    match c {
        C::FunctionalDataProperty(ax) => {
            f.functional_dps.insert(dp_iri(&ax.0));
        }
        C::SubDataPropertyOf(ax) => {
            let sub = dpe_iri(&ax.sub);
            let sup = dpe_iri(&ax.sup);
            if !sub.is_empty() && !sup.is_empty() {
                f.sub_data_property.push((sub, sup));
            }
        }
        C::EquivalentDataProperties(ax) => {
            // Bi-directional: each pair becomes two SubDataPropertyOf edges.
            let iris: Vec<String> = ax.0.iter().map(dp_iri).collect();
            for i in 0..iris.len() {
                for j in 0..iris.len() {
                    if i != j {
                        f.sub_data_property.push((iris[i].clone(), iris[j].clone()));
                    }
                }
            }
        }
        C::DataPropertyDomain(ax) => {
            let dp = dpe_iri(&ax.dp);
            if let Some(d) = class_iri(&ax.ce) {
                f.domains.push((dp, d));
            } else if let ClassExpression::ObjectUnionOf(members) = &ax.ce {
                // Disjunctive domain. Record ONLY when every member is an
                // atomic class — a non-atomic member is invisible to the
                // told tables, so the downstream common-subsumer fold must
                // not run over a partial set (advisor soundness gate).
                let atoms: Option<Vec<String>> = members.iter().map(class_iri).collect();
                if let Some(atoms) = atoms.filter(|a| a.len() >= 2) {
                    f.union_domains.push((dp, atoms));
                }
            }
        }
        C::DataPropertyRange(ax) => {
            // DP-1: record the range's value-space family (if classifiable).
            if let Some(fam) = data_range_family(&ax.dr) {
                f.dp_range_families
                    .entry(dpe_iri(&ax.dp))
                    .or_default()
                    .push(fam);
            }
            // DP-1b: record a string `DataOneOf` enumeration's allowed set.
            if let Some(StrSet::Set(s)) = parse_string_range(&ax.dr) {
                f.dp_string_enums
                    .entry(dpe_iri(&ax.dp))
                    .or_default()
                    .push(s);
            }
        }
        C::DataPropertyAssertion(ax) => {
            // DP-1: record the asserted literal's value-space family.
            if let Some(fam) = literal_family(&ax.to) {
                f.data_assertions.push((dpe_iri(&ax.dp), fam));
            }
            // DP-1b: record the asserted string value (for enum membership).
            if let Some(v) = exact_string_literal(&ax.to) {
                f.data_string_assertions.push((dpe_iri(&ax.dp), v));
            }
        }
        C::SubClassOf(ax) => {
            scan_subclass_axiom(&ax.sub, &ax.sup, f);
        }
        C::EquivalentClasses(ax) => {
            // For each pair (a, b) in the equivalence group, treat as
            // SubClassOf(a, b) AND SubClassOf(b, a) for pattern-matching
            // purposes. Bound-collection: if a is atomic class C and b
            // is an ObjectIntersectionOf with data-cardinality conjuncts,
            // those conjuncts' bounds apply to C.
            let atomic: Vec<String> = ax.0.iter().filter_map(class_iri).collect();
            for c in &atomic {
                for other in &ax.0 {
                    scan_class_for_bounds(c, other, f);
                    scan_class_for_existentials(c, other, f);
                }
            }
        }
        _ => {}
    }
}

fn scan_subclass_axiom<A: ForIRI>(
    sub: &ClassExpression<A>,
    sup: &ClassExpression<A>,
    f: &mut Facts,
) {
    // sub side may be an existential under which we infer a super class.
    // sup side may be a data-cardinality or existential under which we
    // infer bounds / data-some for the sub class.
    if let Some(sub_iri) = class_iri(sub) {
        scan_class_for_bounds(&sub_iri, sup, f);
        scan_class_for_existentials(&sub_iri, sup, f);
    }
    // SubClassOf(DataSome(dp, _), D) — sub is a DataSome; D is the super.
    if let (Some(dp), Some(sup_iri)) = (data_some_property(sub), class_iri(sup)) {
        f.some_super.entry(dp).or_default().insert(sup_iri);
    }
}

/// Recognize `DataMin/Max/Exact` cardinality restrictions in `ce` and
/// record bounds for `class_iri`. Recurses into `ObjectIntersectionOf`
/// (only — disjunctive/negated containers don't propagate bounds
/// soundly).
fn scan_class_for_bounds<A: ForIRI>(class_iri: &str, ce: &ClassExpression<A>, f: &mut Facts) {
    match ce {
        ClassExpression::DataMinCardinality { n, dp, .. } => {
            let key = (class_iri.to_string(), dpe_iri(dp));
            let entry = f.class_min.entry(key).or_insert(0);
            *entry = (*entry).max(*n);
        }
        ClassExpression::DataMaxCardinality { n, dp, .. } => {
            let key = (class_iri.to_string(), dpe_iri(dp));
            let entry = f.class_max.entry(key).or_insert(u32::MAX);
            *entry = (*entry).min(*n);
        }
        ClassExpression::DataExactCardinality { n, dp, .. } => {
            let key = (class_iri.to_string(), dpe_iri(dp));
            let min_entry = f.class_min.entry(key.clone()).or_insert(0);
            *min_entry = (*min_entry).max(*n);
            let max_entry = f.class_max.entry(key).or_insert(u32::MAX);
            *max_entry = (*max_entry).min(*n);
        }
        ClassExpression::ObjectIntersectionOf(parts) => {
            for p in parts {
                scan_class_for_bounds(class_iri, p, f);
            }
        }
        _ => {}
    }
}

/// Recognize `DataSomeValuesFrom(dp, _)` (range opaque) and record
/// `(class_iri, dp_iri)` in `f.class_some`. Recurses into
/// `ObjectIntersectionOf`. Phase D5 (Tier C): also records integer
/// ranges from `DataSomeValuesFrom(dp, DatatypeRestriction(xsd:integer, ...))`
/// into `f.class_int_ranges`.
fn scan_class_for_existentials<A: ForIRI>(class_iri: &str, ce: &ClassExpression<A>, f: &mut Facts) {
    match ce {
        ClassExpression::DataSomeValuesFrom { dp, dr } => {
            f.class_some.insert((class_iri.to_string(), dpe_iri(dp)));
            if let Some(range) = parse_integer_range(dr) {
                f.class_int_ranges
                    .entry((class_iri.to_string(), dpe_iri(dp)))
                    .or_default()
                    .push(range);
            }
        }
        ClassExpression::DataHasValue { dp, .. } => {
            f.class_some.insert((class_iri.to_string(), dpe_iri(dp)));
        }
        ClassExpression::DataMinCardinality { dp, n, .. } if *n >= 1 => {
            f.class_some.insert((class_iri.to_string(), dpe_iri(dp)));
        }
        ClassExpression::DataExactCardinality { dp, n, .. } if *n >= 1 => {
            f.class_some.insert((class_iri.to_string(), dpe_iri(dp)));
        }
        ClassExpression::ObjectIntersectionOf(parts) => {
            for p in parts {
                scan_class_for_existentials(class_iri, p, f);
            }
        }
        _ => {}
    }
}

/// Phase D5 (Tier C): parse an `xsd:integer` `DatatypeRestriction`
/// into an `IntegerRange`. Returns `None` for non-integer base
/// datatypes, unrecognized facets, unparseable literals, or
/// overflowing exclusive-bound adjustments — sound under-approximation:
/// unrecognized ranges contribute no constraint (vs. wrong constraints).
pub(crate) fn parse_integer_range<A: ForIRI>(dr: &DataRange<A>) -> Option<IntegerRange> {
    const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
    match dr {
        // Phase D6 (Part A): a bare `xsd:integer` datatype (no facet) is
        // the unbounded integer range. `DataSomeValuesFrom(p, xsd:integer)`
        // thus lowers to `∃p.DKey(-∞,+∞)` — a sound necessary condition
        // that keeps the enclosing conjunction alive (e.g. Prime/Zoom).
        DataRange::Datatype(dt) if dt.0.to_string() == XSD_INTEGER => {
            Some(IntegerRange::unbounded())
        }
        // Only xsd:integer for Tier C; other numeric datatypes
        // (xsd:decimal, xsd:dateTime) extend with their own range types
        // but share this preprocessing's algebra. Float/double are
        // handled by `parse_float_range` (a DISTINCT datatype bucket —
        // see the DKey datatype-tagging in `convert.rs`).
        DataRange::DatatypeRestriction(dt, facets) if dt.0.to_string() == XSD_INTEGER => {
            parse_integer_facets(facets)
        }
        _ => None,
    }
}

fn parse_integer_facets<A: ForIRI>(facets: &[FacetRestriction<A>]) -> Option<IntegerRange> {
    let mut range = IntegerRange::unbounded();
    for fr in facets {
        let val: i64 = fr.l.literal().parse().ok()?;
        match fr.f {
            Facet::MinInclusive => {
                range.min = Some(range.min.map_or(val, |existing| existing.max(val)));
            }
            Facet::MinExclusive => {
                // xsd:integer-semantics: exclusive ≥ val + 1
                let inclusive = val.checked_add(1)?;
                range.min = Some(
                    range
                        .min
                        .map_or(inclusive, |existing| existing.max(inclusive)),
                );
            }
            Facet::MaxInclusive => {
                range.max = Some(range.max.map_or(val, |existing| existing.min(val)));
            }
            Facet::MaxExclusive => {
                let inclusive = val.checked_sub(1)?;
                range.max = Some(
                    range
                        .max
                        .map_or(inclusive, |existing| existing.min(inclusive)),
                );
            }
            _ => return None,
        }
    }
    Some(range)
}

/// Phase D6 (Part B): the float-family datatype IRIs we model. Both
/// share the real value space and the same facet algebra. We keep them
/// in SEPARATE DKey buckets from each other and from integer (see
/// `convert.rs`) so no cross-datatype subsumption can ever be seeded.
fn is_float_datatype(iri: &str) -> bool {
    iri == "http://www.w3.org/2001/XMLSchema#float"
        || iri == "http://www.w3.org/2001/XMLSchema#double"
}

/// Phase D6 (Part B): parse an `xsd:float` / `xsd:double` `DataRange`
/// into a [`FloatRange`]. Returns `None` for non-float datatypes,
/// unrecognized facets, unparseable / non-finite (NaN, ±∞) literals —
/// sound under-approximation (a dropped range contributes no constraint,
/// never a wrong one).
pub(crate) fn parse_float_range<A: ForIRI>(dr: &DataRange<A>) -> Option<FloatRange> {
    match dr {
        // Bare `xsd:float` / `xsd:double` (no facet) is the unbounded
        // real range. NOTE: intentionally NOT emitted from `convert.rs`'s
        // `DataSomeValuesFrom` bare arm (it's a standalone necessary
        // condition that drops harmlessly and is not needed for the 37);
        // kept here only for completeness of the parser.
        DataRange::Datatype(dt) if is_float_datatype(dt.0.as_ref()) => {
            Some(FloatRange::unbounded())
        }
        DataRange::DatatypeRestriction(dt, facets) if is_float_datatype(dt.0.as_ref()) => {
            parse_float_facets(facets)
        }
        _ => None,
    }
}

fn parse_float_facets<A: ForIRI>(facets: &[FacetRestriction<A>]) -> Option<FloatRange> {
    let mut range = FloatRange::unbounded();
    for fr in facets {
        // Reject NaN and ±∞ outright: a non-finite bound would poison the
        // `==`/`>`/`<` comparisons in `subset` (NaN compares false to
        // everything, which could spuriously hit the equal-endpoint
        // branch). Dropping is the sound direction.
        let val: f64 =
            fr.l.literal()
                .parse()
                .ok()
                .filter(|v: &f64| v.is_finite())?;
        match fr.f {
            Facet::MinInclusive => tighten_min(&mut range, val, true),
            Facet::MinExclusive => tighten_min(&mut range, val, false),
            Facet::MaxInclusive => tighten_max(&mut range, val, true),
            Facet::MaxExclusive => tighten_max(&mut range, val, false),
            _ => return None,
        }
    }
    Some(range)
}

/// Tighten a [`FloatRange`]'s lower bound to the more restrictive of the
/// existing bound and `(val, incl)`. "More restrictive" = larger lower
/// bound; at equal values, exclusive (`!incl`) is tighter than inclusive.
#[allow(
    clippy::float_cmp,
    reason = "exact endpoint equality is intended (same datatype, two facets on the \
              same property); epsilon would mis-merge distinct bounds"
)]
fn tighten_min(range: &mut FloatRange, val: f64, incl: bool) {
    let tighter = match range.min {
        None => true,
        // Larger value is tighter; at equality, exclusive beats inclusive.
        Some(existing) => val > existing || (val == existing && !incl && range.min_incl),
    };
    if tighter {
        range.min = Some(val);
        range.min_incl = incl;
    }
}

/// Symmetric to [`tighten_min`] for the upper bound: smaller value is
/// tighter; at equality, exclusive beats inclusive.
#[allow(
    clippy::float_cmp,
    reason = "exact endpoint equality is intended (see tighten_min)"
)]
fn tighten_max(range: &mut FloatRange, val: f64, incl: bool) {
    let tighter = match range.max {
        None => true,
        Some(existing) => val < existing || (val == existing && !incl && range.max_incl),
    };
    if tighter {
        range.max = Some(val);
        range.max_incl = incl;
    }
}

/// Compute the transitive closure of `sub_data_property` edges:
/// dp → set of all super-dps (including itself).
fn closure_sub_dp(edges: &[(String, String)]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Initialize: every dp mentioned in the hierarchy gets itself as a super.
    let mut all_dps: BTreeSet<String> = BTreeSet::new();
    for (s, t) in edges {
        all_dps.insert(s.clone());
        all_dps.insert(t.clone());
    }
    for dp in &all_dps {
        out.insert(dp.clone(), [dp.clone()].into_iter().collect());
    }
    // Iterate until fixpoint (small N — linear-ish in practice).
    let mut changed = true;
    while changed {
        changed = false;
        for (s, t) in edges {
            let t_supers = out.get(t).cloned().unwrap_or_default();
            let entry = out.entry(s.clone()).or_default();
            for sup in t_supers {
                if entry.insert(sup) {
                    changed = true;
                }
            }
        }
    }
    out
}

fn emit_clashes(
    f: &Facts,
    vocab: &Vocabulary,
    bot_id: ConceptId,
    atomic_id: &impl Fn(ClassId) -> ConceptId,
    out: &mut Vec<Axiom>,
) {
    // Pattern 1: Functional(dp) + min ≥ 2 → unsat.
    for ((class_iri, dp_iri), min) in &f.class_min {
        if *min >= 2
            && f.functional_dps.contains(dp_iri)
            && let Some(cid) = vocab.class_id(class_iri)
        {
            out.push(Axiom::SubClassOf {
                sub: atomic_id(cid),
                sup: bot_id,
            });
        }
    }
    // Pattern 2: min > max on same (class, dp) → unsat.
    for ((class_iri, dp_iri), min) in &f.class_min {
        if let Some(max) = f.class_max.get(&(class_iri.clone(), dp_iri.clone()))
            && min > max
            && let Some(cid) = vocab.class_id(class_iri)
        {
            out.push(Axiom::SubClassOf {
                sub: atomic_id(cid),
                sup: bot_id,
            });
        }
    }
    // Phase D5 (Tier C) Pattern: Functional(dp) + 2+ integer-range
    // constraints on (C, dp) with empty intersection → C ⊑ Bot.
    // Functional is required: without it, an instance could satisfy
    // multiple ranges via separate values; with it, the single value
    // must satisfy all ranges intersected.
    for ((class_iri, dp_iri), ranges) in &f.class_int_ranges {
        if ranges.len() < 2 || !f.functional_dps.contains(dp_iri) {
            continue;
        }
        let intersection = ranges
            .iter()
            .copied()
            .fold(IntegerRange::unbounded(), IntegerRange::intersect);
        if intersection.is_empty()
            && let Some(cid) = vocab.class_id(class_iri)
        {
            out.push(Axiom::SubClassOf {
                sub: atomic_id(cid),
                sup: bot_id,
            });
        }
    }
    // Pattern 1.5: Functional(dp) + max = 1 is the SAME constraint, no
    // new clash. (Captured by Pattern 2 if user supplied both.)
    // Pattern 2.5: Functional(dp) interacts with min/max symmetrically;
    // no new derivations beyond 1 and 2.
}

fn emit_domain_inferences(
    f: &Facts,
    vocab: &Vocabulary,
    atomic_id: &impl Fn(ClassId) -> ConceptId,
    out: &mut Vec<Axiom>,
) {
    // Pattern 3: DataPropertyDomain(dp, D) + C ⊑ DataSome(dp, _) ⇒ C ⊑ D.
    for (dp_iri, domain_iri) in &f.domains {
        for (class_iri, c_dp) in &f.class_some {
            if c_dp != dp_iri {
                continue;
            }
            if class_iri == domain_iri {
                continue; // C ⊑ C is trivial.
            }
            if let (Some(c_id), Some(d_id)) =
                (vocab.class_id(class_iri), vocab.class_id(domain_iri))
            {
                out.push(Axiom::SubClassOf {
                    sub: atomic_id(c_id),
                    sup: atomic_id(d_id),
                });
            }
        }
    }
}

fn emit_subdataprop_transitivity(
    f: &Facts,
    vocab: &Vocabulary,
    atomic_id: &impl Fn(ClassId) -> ConceptId,
    out: &mut Vec<Axiom>,
) {
    // Pattern 4: SubDataPropertyOf(specific, general) chain +
    // C ⊑ DataSome(specific) + DataSome(general) ⊑ D ⇒ C ⊑ D.
    let closure = closure_sub_dp(&f.sub_data_property);
    for (class_iri, specific_dp) in &f.class_some {
        let Some(supers) = closure.get(specific_dp) else {
            continue;
        };
        for general_dp in supers {
            let Some(super_classes) = f.some_super.get(general_dp) else {
                continue;
            };
            for d_iri in super_classes {
                if class_iri == d_iri {
                    continue;
                }
                if let (Some(c_id), Some(d_id)) = (vocab.class_id(class_iri), vocab.class_id(d_iri))
                {
                    out.push(Axiom::SubClassOf {
                        sub: atomic_id(c_id),
                        sup: atomic_id(d_id),
                    });
                }
            }
        }
    }
    // Also: DataPropertyDomain inference under hierarchy: a domain
    // assertion on `general` carries to all sub-dps.
    for (general_dp, domain_iri) in &f.domains {
        // Find all dps that are sub of general_dp (closure inverse).
        // Iterate every dp in the closure; check if general_dp is in its supers.
        for (sub_dp, supers) in &closure {
            if !supers.contains(general_dp) || sub_dp == general_dp {
                continue;
            }
            for (class_iri, c_dp) in &f.class_some {
                if c_dp != sub_dp {
                    continue;
                }
                if class_iri == domain_iri {
                    continue;
                }
                if let (Some(c_id), Some(d_id)) =
                    (vocab.class_id(class_iri), vocab.class_id(domain_iri))
                {
                    out.push(Axiom::SubClassOf {
                        sub: atomic_id(c_id),
                        sup: atomic_id(d_id),
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// DP-1: datatype value-space families (data-range-violation detection)
// ─────────────────────────────────────────────────────────────────────

/// Coarse XSD value-space *macro-families*. Each variant is a single
/// value space; **distinct variants are pairwise value-space-disjoint**
/// (no value of one is a value of another). Deliberately conservative:
/// all numerics (decimal / integer subtypes / float / double) collapse
/// into one `Numeric` family so we NEVER flag a numeric-vs-numeric pair
/// (sidesteps both the `int ⊆ decimal` containment trap and the
/// float-vs-decimal value-space subtlety — at the cost of missing those
/// violations, which is the safe direction). A datatype we are not
/// certain about classifies to `None` ⇒ never flagged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DtFamily {
    /// `xsd:string` and its lexical restrictions (token, Name, …).
    TextPlain,
    /// `rdf:langString` — language-tagged; **disjoint** from `xsd:string`.
    LangString,
    /// `xsd:decimal`, every integer subtype, `xsd:float`, `xsd:double`.
    Numeric,
    /// `xsd:boolean`.
    Boolean,
    /// `xsd:dateTime` / `date` / `time` / `g*` / `duration`.
    Temporal,
    /// `xsd:hexBinary` / `xsd:base64Binary`.
    Binary,
}

/// Classify a datatype IRI into a value-space family, or `None` when we
/// are not certain it is value-space-disjoint from the others (e.g.
/// `xsd:anyURI`, `rdfs:Literal`, custom datatypes) — `None` is never
/// flagged, keeping DP-1 a sound under-approximation.
fn dt_family(iri: &str) -> Option<DtFamily> {
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    const RDF_LANGSTRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";
    if iri == RDF_LANGSTRING {
        return Some(DtFamily::LangString);
    }
    let local = iri.strip_prefix(XSD)?;
    Some(match local {
        "string" | "normalizedString" | "token" | "language" | "Name" | "NCName" | "NMTOKEN" => {
            DtFamily::TextPlain
        }
        "decimal" | "integer" | "int" | "long" | "short" | "byte" | "nonNegativeInteger"
        | "positiveInteger" | "negativeInteger" | "nonPositiveInteger" | "unsignedInt"
        | "unsignedLong" | "unsignedShort" | "unsignedByte" | "float" | "double" => {
            DtFamily::Numeric
        }
        "boolean" => DtFamily::Boolean,
        "dateTime" | "dateTimeStamp" | "date" | "time" | "gYear" | "gYearMonth" | "gMonth"
        | "gDay" | "gMonthDay" | "duration" => DtFamily::Temporal,
        "hexBinary" | "base64Binary" => DtFamily::Binary,
        // anyURI, rdfs:Literal restrictions, unknown → not certain → skip.
        _ => return None,
    })
}

/// The value-space family of a literal: `Simple` ⇒ `xsd:string`,
/// `Language` ⇒ `rdf:langString`, `Datatype` ⇒ the datatype's family.
fn literal_family<A: ForIRI>(l: &Literal<A>) -> Option<DtFamily> {
    match l {
        Literal::Simple { .. } => Some(DtFamily::TextPlain),
        Literal::Language { .. } => Some(DtFamily::LangString),
        Literal::Datatype { datatype_iri, .. } => dt_family(datatype_iri.as_ref()),
    }
}

/// The value-space family of a data range, but **only** for a bare
/// `Datatype` or a `DatatypeRestriction` over one (facets don't change
/// the family). `DataOneOf` / `DataUnionOf` / `DataIntersectionOf` /
/// `DataComplementOf` ⇒ `None`: a union/complement/enumeration is NOT a
/// single value space, so a family mismatch with one part proves nothing
/// (the catastrophic false-`Inconsistent` case — gated hard here).
fn data_range_family<A: ForIRI>(dr: &DataRange<A>) -> Option<DtFamily> {
    match dr {
        DataRange::Datatype(dt) | DataRange::DatatypeRestriction(dt, _) => dt_family(dt.0.as_ref()),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────
// IRI extractors
// ─────────────────────────────────────────────────────────────────────

fn class_iri<A: ForIRI>(ce: &ClassExpression<A>) -> Option<String> {
    if let ClassExpression::Class(c) = ce {
        Some(c.0.to_string())
    } else {
        None
    }
}

fn dp_iri<A: ForIRI>(d: &DataProperty<A>) -> String {
    d.0.to_string()
}

fn dpe_iri<A: ForIRI>(d: &DataProperty<A>) -> String {
    // horned-owl 1.x: DataProperty is type-alias for DataProperty.
    // If a future version adds variants (e.g., InverseDataProperty isn't a
    // thing in OWL 2 DL but a top-level wrapper could be added), update here.
    d.0.to_string()
}

fn data_some_property<A: ForIRI>(ce: &ClassExpression<A>) -> Option<String> {
    match ce {
        ClassExpression::DataSomeValuesFrom { dp, .. }
        | ClassExpression::DataHasValue { dp, .. } => Some(dpe_iri(dp)),
        ClassExpression::DataMinCardinality { dp, n, .. } if *n >= 1 => Some(dpe_iri(dp)),
        ClassExpression::DataExactCardinality { dp, n, .. } if *n >= 1 => Some(dpe_iri(dp)),
        _ => None,
    }
}

// Suppress unused-import warning when the DataRange import isn't needed
// at the top of the file (it'd be referenced only if we did range-aware
// matching, which is Tier C territory).
#[allow(dead_code)]
fn _unused_datarange<A: ForIRI>(_: &DataRange<A>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::convert_ontology;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn parse_str(src: &str) -> SetOntology<RcStr> {
        let mut r = Cursor::new(src);
        read_ofn(&mut r, ParserConfiguration::default())
            .expect("test fixture parses")
            .0
    }

    fn incl(lo: i64, hi: i64) -> IntegerRange {
        IntegerRange {
            min: Some(lo),
            max: Some(hi),
        }
    }

    // ── FloatRange helpers (Phase D6 Part B) ─────────────────────────
    fn fr(min: Option<f64>, min_incl: bool, max: Option<f64>, max_incl: bool) -> FloatRange {
        FloatRange {
            min,
            min_incl,
            max,
            max_incl,
        }
    }
    /// `[lo, hi]` closed.
    fn fc(lo: f64, hi: f64) -> FloatRange {
        fr(Some(lo), true, Some(hi), true)
    }
    /// `(lo, hi)` open.
    fn fo(lo: f64, hi: f64) -> FloatRange {
        fr(Some(lo), false, Some(hi), false)
    }

    #[test]
    fn float_range_subset_boundaries() {
        // ── Point vs open/closed interval (the f-stop / exposure cases)
        // 36.0 ∉ (36,101) — exclusive lower boundary.
        assert!(
            !FloatRange::point(36.0).subset(fo(36.0, 101.0)),
            "36.0 ∉ (36,101)"
        );
        // 36.0 ∈ [36,101] — inclusive lower boundary.
        assert!(
            FloatRange::point(36.0).subset(fc(36.0, 101.0)),
            "36.0 ∈ [36,101]"
        );
        // 101.0 ∉ (36,101) — exclusive upper boundary.
        assert!(
            !FloatRange::point(101.0).subset(fo(36.0, 101.0)),
            "101.0 ∉ (36,101)"
        );
        // 101.0 ∈ [36,101] — inclusive upper boundary.
        assert!(
            FloatRange::point(101.0).subset(fc(36.0, 101.0)),
            "101.0 ∈ [36,101]"
        );
        // Interior value.
        assert!(
            FloatRange::point(60.0).subset(fo(36.0, 101.0)),
            "60 ∈ (36,101)"
        );
        // Value outside.
        assert!(
            !FloatRange::point(200.0).subset(fo(36.0, 101.0)),
            "200 ∉ (36,101)"
        );
        assert!(
            !FloatRange::point(0.0).subset(fo(36.0, 101.0)),
            "0 ∉ (36,101)"
        );

        // ── Mixed inclusive/exclusive range-vs-range.
        assert!(fc(40.0, 50.0).subset(fo(36.0, 101.0)), "[40,50] ⊆ (36,101)");
        assert!(
            fo(36.0, 101.0).subset(fc(36.0, 101.0)),
            "(36,101) ⊆ [36,101]"
        );
        // self includes 36.0, other excludes it → NOT subset.
        assert!(
            !fr(Some(36.0), true, None, false).subset(fr(Some(36.0), false, None, false)),
            "[36,..) ⊄ (36,..)"
        );
        // [..,101] ⊄ [..,101) — self includes 101, other excludes it.
        assert!(
            !fr(None, false, Some(101.0), true).subset(fr(None, false, Some(101.0), false)),
            "(..,101] ⊄ (..,101)"
        );

        // ── VeryFastExposure ⊆ FastExposure: (-∞,0.002) ⊆ (-∞,0.01).
        let very_fast = fr(None, false, Some(0.002), false);
        let fast = fr(None, false, Some(0.01), false);
        assert!(very_fast.subset(fast), "(-∞,0.002) ⊆ (-∞,0.01)");
        assert!(!fast.subset(very_fast), "(-∞,0.01) ⊄ (-∞,0.002)");

        // ── SlowExposure (0.01,1.0) vs others: not ⊆ Fast (overlaps but
        // extends right past 0.01).
        let slow = fo(0.01, 1.0);
        assert!(!slow.subset(fast), "(0.01,1.0) ⊄ (-∞,0.01)");

        // ── Unbounded.
        assert!(
            fc(1.0, 2.0).subset(FloatRange::unbounded()),
            "any ⊆ (-∞,+∞)"
        );
        assert!(
            !FloatRange::unbounded().subset(fc(1.0, 2.0)),
            "(-∞,+∞) ⊄ [1,2]"
        );
        // unbounded-below self vs bounded-below other → NOT subset.
        assert!(
            !fr(None, false, Some(50.0), true).subset(fc(37.0, 100.0)),
            "(-∞,50] ⊄ [37,100]"
        );

        // ── Reflexive (uses PartialEq via the same flags).
        assert!(fo(36.0, 101.0).subset(fo(36.0, 101.0)), "R ⊆ R (open)");
        assert!(fc(36.0, 101.0).subset(fc(36.0, 101.0)), "R ⊆ R (closed)");
    }

    fn float_facet(facet: Facet, lit: &str) -> FacetRestriction<RcStr> {
        use horned_owl::model::{Build, Literal};
        let b: Build<RcStr> = Build::new_rc();
        FacetRestriction {
            f: facet,
            l: Literal::Datatype {
                literal: lit.to_string(),
                datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#float"),
            },
        }
    }

    #[test]
    fn float_facets_reject_nan_and_inf() {
        // A NaN or ±∞ facet literal must drop the WHOLE range (None),
        // never yield a spurious subset.
        assert_eq!(
            parse_float_facets(&[float_facet(Facet::MinInclusive, "NaN")]),
            None,
            "NaN facet → drop"
        );
        assert_eq!(
            parse_float_facets(&[float_facet(Facet::MaxExclusive, "INF")]),
            None,
            "INF facet → drop"
        );
        assert_eq!(
            parse_float_facets(&[float_facet(Facet::MaxExclusive, "-INF")]),
            None,
            "-INF facet → drop"
        );
        // A finite facet still parses.
        assert!(parse_float_facets(&[float_facet(Facet::MaxExclusive, "0.01")]).is_some());
    }

    #[test]
    fn float_facet_min_exclusive_is_not_normalized() {
        // CRITICAL: unlike integer (±1), float exclusive bounds must NOT
        // be shifted. (36,..) must keep min=36.0 min_incl=false so that
        // [36,..) is correctly NOT a subset of (36,..).
        let parsed =
            parse_float_facets(&[float_facet(Facet::MinExclusive, "36.0")]).expect("parses");
        assert_eq!(parsed.min, Some(36.0), "min value unchanged (no ±1 shift)");
        assert!(!parsed.min_incl, "min is exclusive");
    }

    #[test]
    fn integer_range_subset_boundaries() {
        // Recovery target: MediumFormat height range is (36,101) =
        // inclusive [37, 100]; point value 60 must be inside.
        let medium_h = incl(37, 100);
        assert!(IntegerRange::point(60).subset(medium_h), "60 ∈ [37,100]");

        // Exclusive boundaries: 36 and 101 are OUTSIDE [37,100].
        assert!(
            !IntegerRange::point(36).subset(medium_h),
            "36 ∉ [37,100] (minExclusive 36)"
        );
        assert!(
            !IntegerRange::point(101).subset(medium_h),
            "101 ∉ [37,100] (maxExclusive 101)"
        );
        // Inclusive endpoints ARE inside.
        assert!(IntegerRange::point(37).subset(medium_h), "37 ∈ [37,100]");
        assert!(IntegerRange::point(100).subset(medium_h), "100 ∈ [37,100]");

        // Value far outside.
        assert!(!IntegerRange::point(200).subset(medium_h), "200 ∉ [37,100]");

        // range ⊆ range.
        assert!(incl(40, 50).subset(medium_h), "[40,50] ⊆ [37,100]");
        assert!(!medium_h.subset(incl(40, 50)), "[37,100] ⊄ [40,50]");

        // Unbounded-below self vs bounded other → NOT a subset.
        let unbounded_below = IntegerRange {
            min: None,
            max: Some(50),
        };
        assert!(!unbounded_below.subset(medium_h), "(-∞,50] ⊄ [37,100]");
        // Unbounded-above self vs bounded other → NOT a subset.
        let unbounded_above = IntegerRange {
            min: Some(40),
            max: None,
        };
        assert!(!unbounded_above.subset(medium_h), "[40,+∞) ⊄ [37,100]");
        // [100,+∞) ⊄ [37,100] (the real ontology has a minInclusive 100
        // range that must NOT be a subset of MediumFormat's height).
        assert!(
            !IntegerRange {
                min: Some(100),
                max: None
            }
            .subset(medium_h),
            "[100,+∞) ⊄ [37,100]"
        );

        // other unbounded → everything is a subset.
        assert!(medium_h.subset(IntegerRange::unbounded()), "any ⊆ (-∞,+∞)");
        assert!(
            IntegerRange::unbounded().subset(IntegerRange::unbounded()),
            "(-∞,+∞) ⊆ (-∞,+∞)"
        );
        assert!(
            !IntegerRange::unbounded().subset(medium_h),
            "(-∞,+∞) ⊄ [37,100]"
        );

        // Empty self (minIncl 100, maxExcl 100 → [100,99]) ⊆ anything.
        let empty = IntegerRange {
            min: Some(100),
            max: Some(99),
        };
        assert!(empty.is_empty(), "[100,99] is empty");
        assert!(empty.subset(medium_h), "∅ ⊆ [37,100]");
        assert!(empty.subset(incl(0, 0)), "∅ ⊆ [0,0]");

        // Reflexive.
        assert!(medium_h.subset(medium_h), "R ⊆ R");
    }

    #[test]
    fn extracts_functional_dp_min_clash() {
        let src = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
    Declaration(Class(:HasTwoAges))
    Declaration(DataProperty(:age))
    FunctionalDataProperty(:age)
    SubClassOf(:HasTwoAges DataMinCardinality(2 :age))
)
";
        let onto = parse_str(src);
        let facts = extract_facts(&onto);
        assert!(facts.functional_dps.contains("http://t/age"));
        assert_eq!(
            facts.class_min.get(&(
                "http://t/HasTwoAges".to_string(),
                "http://t/age".to_string()
            )),
            Some(&2)
        );
    }

    #[test]
    fn derives_functional_dp_min_unsat_in_convert() {
        let src = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
    Declaration(Class(:HasTwoAges))
    Declaration(DataProperty(:age))
    FunctionalDataProperty(:age)
    SubClassOf(:HasTwoAges DataMinCardinality(2 :age))
)
";
        let onto = parse_str(src);
        let mut internal = convert_ontology(&onto).expect("test ontology converts");
        let has_two_ages = internal
            .vocabulary
            .class_id("http://t/HasTwoAges")
            .expect("HasTwoAges interned");
        let bot = internal.concepts.bot();
        let sub_concept = internal.concepts.atomic(has_two_ages);
        let found_unsat = internal.axioms.iter().any(|ax| {
            matches!(ax,
            Axiom::SubClassOf { sub, sup } if *sub == sub_concept && *sup == bot)
        });
        assert!(
            found_unsat,
            "D4: HasTwoAges ⊑ Bot should be derived from Functional + DataMin"
        );
    }

    // ── Phase D11b: disjoint() — the FP surface of the ∀-membership clash.
    // The corpus CANNOT exercise this (no ∃+∀ clash exists in it), so these
    // unit tests are the entire safety net. OVERLAP must NEVER read as
    // disjoint (that would seed a spurious ⊥ = false positive).

    #[test]
    fn integer_disjoint_boundaries() {
        // Inclusive integer endpoints: a shared endpoint = OVERLAP.
        assert!(!incl(0, 5).disjoint(incl(5, 10)), "[0,5],[5,10] share 5");
        assert!(incl(0, 3).disjoint(incl(5, 8)), "[0,3],[5,8] gap");
        assert!(incl(0, 4).disjoint(incl(5, 8)), "[0,4],[5,8] adjacent ints");
        assert!(!incl(0, 10).disjoint(incl(3, 5)), "nested overlaps");
        assert!(!incl(0, 5).disjoint(incl(3, 8)), "partial overlap");
        assert!(!incl(0, 5).disjoint(incl(0, 5)), "identical overlaps");
        // A bounded end facing the other range CAN be disjoint even with an
        // unbounded opposite end: (-∞,3] vs [5,8] → max 3 < min 5 → disjoint.
        assert!(
            IntegerRange {
                min: None,
                max: Some(3)
            }
            .disjoint(incl(5, 8)),
            "(-∞,3] and [5,8] are disjoint (3 < 5)"
        );
    }

    #[test]
    fn integer_disjoint_unbounded_is_conservative() {
        // (-∞,+∞) shares values with everything.
        assert!(!IntegerRange::unbounded().disjoint(incl(5, 8)));
        // [5,+∞) vs [0,8] overlap at [5,8].
        let lo = IntegerRange {
            min: Some(5),
            max: None,
        };
        assert!(!lo.disjoint(incl(0, 8)));
        // [5,+∞) vs (-∞,3] — no shared int (max of second=3 < min of first=5).
        let hi = IntegerRange {
            min: None,
            max: Some(3),
        };
        assert!(lo.disjoint(hi), "[5,+∞) and (-∞,3] are disjoint");
    }

    #[test]
    fn float_disjoint_boundaries() {
        // Shared endpoint, both inclusive → OVERLAP (point 5 is in both).
        assert!(
            !fc(0.0, 5.0).disjoint(fc(5.0, 10.0)),
            "[0,5],[5,10] share 5.0"
        );
        // One side excludes the shared endpoint → disjoint.
        assert!(
            fr(Some(0.0), true, Some(5.0), false).disjoint(fc(5.0, 10.0)),
            "[0,5) and [5,10] disjoint"
        );
        assert!(
            fc(0.0, 5.0).disjoint(fr(Some(5.0), false, Some(10.0), true)),
            "[0,5] and (5,10] disjoint"
        );
        // Both open at the meeting point → disjoint (5.0 in neither).
        assert!(fo(0.0, 5.0).disjoint(fr(Some(5.0), false, Some(10.0), true)));
        // Gap / nested / overlap.
        assert!(fc(0.0, 3.0).disjoint(fc(5.0, 8.0)), "gap");
        assert!(!fc(0.0, 10.0).disjoint(fc(3.0, 5.0)), "nested");
        assert!(!fc(0.0, 5.0).disjoint(fc(3.0, 8.0)), "partial overlap");
        // Unbounded never provably disjoint.
        assert!(!FloatRange::unbounded().disjoint(fc(5.0, 8.0)));
        assert!(
            !fr(Some(5.0), true, None, false).disjoint(fc(0.0, 8.0)),
            "[5,∞) vs [0,8]"
        );
    }

    #[test]
    fn ord_decimal_disjoint_boundaries() {
        fn dr(min: &str, min_incl: bool, max: &str, max_incl: bool) -> OrdRange<Decimal> {
            OrdRange {
                min: parse_decimal(min),
                min_incl,
                max: parse_decimal(max),
                max_incl,
            }
        }
        // [0,0.5] and [0.5,1] share 0.5 (both inclusive) → NOT disjoint.
        assert!(!dr("0", true, "0.5", true).disjoint(&dr("0.5", true, "1", true)));
        // [0,0.5) and [0.5,1] → disjoint.
        assert!(dr("0", true, "0.5", false).disjoint(&dr("0.5", true, "1", true)));
        // distinct-but-close decimals: [0,0.45] and [0.5,1] gap → disjoint.
        assert!(dr("0", true, "0.45", true).disjoint(&dr("0.5", true, "1", true)));
        // overlap.
        assert!(!dr("0", true, "0.6", true).disjoint(&dr("0.5", true, "1", true)));
    }

    #[test]
    fn ord_date_disjoint() {
        fn d(min: DateKey, max: DateKey, mi: bool, ma: bool) -> OrdRange<DateKey> {
            OrdRange {
                min: Some(min),
                min_incl: mi,
                max: Some(max),
                max_incl: ma,
            }
        }
        // [2020-01-01, 2020-06-01] and [2020-06-01, 2021-01-01] share the
        // boundary (both inclusive) → NOT disjoint.
        let a = d((2020, 1, 1), (2020, 6, 1), true, true);
        let b = d((2020, 6, 1), (2021, 1, 1), true, true);
        assert!(!a.disjoint(&b));
        // exclude the shared boundary → disjoint.
        let a2 = d((2020, 1, 1), (2020, 6, 1), true, false);
        assert!(a2.disjoint(&b));
        // clear gap.
        let c = d((2019, 1, 1), (2019, 12, 31), true, true);
        assert!(c.disjoint(&b));
    }

    #[test]
    fn strset_disjoint() {
        let set = |xs: &[&str]| StrSet::Set(xs.iter().map(|s| (*s).to_string()).collect());
        // Top overlaps everything.
        assert!(!StrSet::Top.disjoint(&set(&["a"])));
        assert!(!set(&["a"]).disjoint(&StrSet::Top));
        assert!(!StrSet::Top.disjoint(&StrSet::Top));
        // Disjoint finite sets.
        assert!(set(&["a"]).disjoint(&set(&["b", "c"])));
        // Sharing a member → NOT disjoint.
        assert!(!set(&["a"]).disjoint(&set(&["a", "b"])));
        assert!(!set(&["a", "b"]).disjoint(&set(&["b", "c"])));
    }
}
