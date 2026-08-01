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
    ClassExpression, Component, DataProperty, DataRange, FacetRestriction, ForIRI, Individual,
    Literal,
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
    emit_data_cardinality_violations(&facts, top_id, bot_id, &mut out);
    emit_data_range_value_violations(src, top_id, bot_id, &mut out);
    emit_functional_dp_cardinality_violations(src, top_id, bot_id, &mut out);
    emit_data_cardinality_violations_typed(src, top_id, bot_id, &mut out);
    emit_disjoint_dp_same_value_clash(&facts, top_id, bot_id, &mut out);
    out
}

/// DP-2: a data-CARDINALITY violation ⇒ global inconsistency.
/// `ClassAssertion(C₀, a)` with `C₀ ⊑* C` and `C ⊑ ≤n dp` (a `DataMax`/
/// `DataExact` constraint, captured in `class_max`) forces `a` to have at most
/// `n` distinct `dp`-fillers. When `a` is asserted **more than `n` distinct
/// `xsd:string` values** for `dp` (directly or via a sub-data-property
/// `dp' ⊑ dp`), the ABox has no model ⇒ emit `Top ⊑ Bot`. Closes
/// `ore_ont_12174` (`EnumerationElement ⊑ =1 literal`, an element with both
/// `"L"` and `"L "`).
///
/// **Sound by construction (the false-`Inconsistent` gate):**
/// - distinctness is **exact `xsd:string` lexical inequality only** — no numeric
///   normalization (so the `int`/`decimal`/`float` equality landmines never
///   apply); non-string values are ignored, an under-count (safe direction).
/// - data literals with distinct values are inherently distinct fillers (no
///   merge, unlike object successors), so `>n` distinct ⇒ a genuine `≤n` clash.
/// - typing via the reflexive-transitive **told** atomic-`SubClassOf` closure;
///   `≤n` only from `DataMax`/`DataExact` (never `DataMin`); fillers routed via
///   the sub→super data-property direction (`dp' ⊑ dp` ⇒ `dp'`-values are
///   `dp`-values).
fn emit_data_cardinality_violations(
    f: &Facts,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    if f.class_assertions.is_empty()
        || f.class_max_string.is_empty()
        || f.ind_string_values.is_empty()
    {
        return;
    }
    let class_closure = closure_sub_dp(&f.subclass_atomic); // class → {self ∪ supers}
    let dp_closure = closure_sub_dp(&f.sub_data_property); // dp → {self ∪ supers}
    // Individual → all (told) types.
    let mut ind_types: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (ind, c) in &f.class_assertions {
        let entry = ind_types.entry(ind.as_str()).or_default();
        match class_closure.get(c) {
            Some(supers) => entry.extend(supers.iter().map(String::as_str)),
            None => {
                entry.insert(c.as_str());
            }
        }
    }
    // Individual → dp → distinct string values (owned view for routing).
    let mut ind_dp_vals: BTreeMap<&str, BTreeMap<&str, &BTreeSet<String>>> = BTreeMap::new();
    for ((ind, dp), vals) in &f.ind_string_values {
        ind_dp_vals
            .entry(ind.as_str())
            .or_default()
            .insert(dp.as_str(), vals);
    }
    for ((class, dp), &n) in &f.class_max_string {
        for (ind, types) in &ind_types {
            if !types.contains(class.as_str()) {
                continue;
            }
            let Some(dp_map) = ind_dp_vals.get(ind) else {
                continue;
            };
            // Collect distinct string fillers across dp and every sub-dp of dp.
            let mut distinct: BTreeSet<&str> = BTreeSet::new();
            for (dpp, vals) in dp_map {
                // `dpp ⊑ dp` iff `dp` is in `dpp`'s super-closure; a dp with no
                // hierarchy edges isn't in the closure map, so match it directly.
                let is_sub = dp_closure
                    .get(*dpp)
                    .map_or(*dpp == dp.as_str(), |s| s.contains(dp));
                if is_sub {
                    distinct.extend(vals.iter().map(String::as_str));
                }
            }
            if distinct.len() > n as usize {
                out.push(Axiom::SubClassOf {
                    sub: top_id,
                    sup: bot_id,
                });
                return;
            }
        }
    }
}

/// The IRI of a named individual (anonymous individuals → `None`; they can't be
/// referenced by the per-individual data-cardinality bookkeeping).
fn individual_iri<A: ForIRI>(i: &Individual<A>) -> Option<String> {
    match i {
        Individual::Named(ni) => Some(ni.0.to_string()),
        Individual::Anonymous(_) => None,
    }
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

    /// `DataIntersectionOf` support: `self ∩ other` for float-family ranges.
    /// Intersects the bounds using the same tighten logic as facet parsing.
    /// The result may satisfy `is_empty()` if the bounds cross.
    #[allow(
        clippy::float_cmp,
        reason = "exact endpoint equality is intended (same as tighten_min/tighten_max)"
    )]
    pub(crate) fn intersect(self, other: Self) -> Self {
        let mut result = self;
        // Tighten lower bound by other.min.
        if let Some(v) = other.min {
            let tighter = match result.min {
                None => true,
                Some(existing) => {
                    v > existing || (v == existing && !other.min_incl && result.min_incl)
                }
            };
            if tighter {
                result.min = Some(v);
                result.min_incl = other.min_incl;
            }
        }
        // Tighten upper bound by other.max.
        if let Some(v) = other.max {
            let tighter = match result.max {
                None => true,
                Some(existing) => {
                    v < existing || (v == existing && !other.max_incl && result.max_incl)
                }
            };
            if tighter {
                result.max = Some(v);
                result.max_incl = other.max_incl;
            }
        }
        result
    }

    /// True iff the range contains no float value.
    /// A range `[a, b]`/`(a, b]`/etc. is empty when `a > b`, or when
    /// `a == b` and at least one endpoint is exclusive (the open interval
    /// `(v, v)` contains nothing).
    #[allow(
        clippy::float_cmp,
        reason = "exact endpoint equality is intended — same as disjoint/subset"
    )]
    pub(crate) fn is_empty(self) -> bool {
        match (self.min, self.max) {
            (Some(lo), Some(hi)) => lo > hi || (lo == hi && (!self.min_incl || !self.max_incl)),
            _ => false, // unbounded end → non-empty
        }
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

    /// `DataIntersectionOf` support: `self ∩ other`. Uses the same
    /// `tighten_min`/`tighten_max` logic as facet folding.
    /// The result may satisfy `is_empty()` if the bounds cross.
    pub(crate) fn intersect(&self, other: &Self) -> Self {
        let mut result = self.clone();
        if let Some(v) = other.min.clone() {
            result.tighten_min(v, other.min_incl);
        }
        if let Some(v) = other.max.clone() {
            result.tighten_max(v, other.max_incl);
        }
        result
    }

    /// True iff the range contains no value. A range is empty when
    /// `lo > hi`, or when `lo == hi` and at least one endpoint is exclusive.
    pub(crate) fn is_empty(&self) -> bool {
        match (&self.min, &self.max) {
            (Some(lo), Some(hi)) => lo > hi || (lo == hi && (!self.min_incl || !self.max_incl)),
            _ => false,
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
pub struct Decimal {
    pub negative: bool,
    pub int: String,
    pub frac: String,
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
pub type DateKey = (i64, u8, u8);
/// Component key for `xsd:dateTime`: `(year, month, day, hour, min, sec)`.
/// Integer-second, timezone-free only (fractional seconds / any `Z`/offset
/// are dropped at parse — sound under-approx).
pub type DateTimeKey = (i64, u8, u8, u8, u8, u8);

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
pub enum StrSet {
    Top,
    Set(BTreeSet<String>),
}

impl StrSet {
    pub fn singleton(s: String) -> Self {
        StrSet::Set([s].into_iter().collect())
    }

    /// `self ⊆ other`: anything ⊆ `Top`; `Top` is a subset only of `Top`;
    /// two finite sets compare by ordinary set inclusion.
    pub fn subset(&self, other: &Self) -> bool {
        match (self, other) {
            (_, StrSet::Top) => true,
            (StrSet::Top, StrSet::Set(_)) => false,
            (StrSet::Set(a), StrSet::Set(b)) => a.is_subset(b),
        }
    }

    /// Phase D11b: `self ∩ other = ∅` — conservative. `Top` (= every string)
    /// overlaps everything, so it is NEVER disjoint; two finite enumerations
    /// are disjoint iff they share no member.
    pub fn disjoint(&self, other: &Self) -> bool {
        match (self, other) {
            (StrSet::Top, _) | (_, StrSet::Top) => false,
            (StrSet::Set(a), StrSet::Set(b)) => a.is_disjoint(b),
        }
    }

    /// `DataIntersectionOf` support: `self ∩ other`.
    /// `Top ∩ Top = Top`; `Top ∩ Set(S) = Set(S)`; `Set(A) ∩ Set(B) = Set(A∩B)`.
    pub(crate) fn intersect(&self, other: &Self) -> Self {
        match (self, other) {
            (StrSet::Top, x) | (x, StrSet::Top) => x.clone(),
            (StrSet::Set(a), StrSet::Set(b)) => StrSet::Set(a.intersection(b).cloned().collect()),
        }
    }

    /// True iff the set contains no string (only possible for finite sets).
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            StrSet::Top => false,
            StrSet::Set(s) => s.is_empty(),
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

// ─────────────────────────────────────────────────────────────────────
// Numeric DataOneOf parsers (Phase D-numeric-oneof)
//
// Each parser recognizes a `DataOneOf(l1 l2 …)` where EVERY member is
// the correct numeric type, deduplicates by VALUE (BTreeSet), and returns
// the set. ANY member that fails its value-parser (wrong datatype, NaN,
// timezone, mixed type) → `None` (drops the whole range — sound
// under-approximation). Capacity = |distinct values|.
//
// FP-critical invariant: under-counting capacity (too few distinct values)
// → spurious clash → false subsumption. So dedup MUST be by value.
// Over-counting (treating the same value as two) is the safe direction.
// ─────────────────────────────────────────────────────────────────────

/// Parse a `DataOneOf` whose members are ALL `xsd:integer`-typed literals
/// into a `BTreeSet<i64>` (capacity = |distinct values|). Returns `None`
/// for any non-`DataOneOf`, empty set, or member with a non-integer literal.
pub(crate) fn parse_integer_oneof<A: ForIRI>(dr: &DataRange<A>) -> Option<BTreeSet<i64>> {
    match dr {
        DataRange::DataOneOf(lits) if !lits.is_empty() => {
            let mut set = BTreeSet::new();
            for l in lits {
                let v = match l {
                    Literal::Datatype {
                        literal,
                        datatype_iri,
                    } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#integer" => {
                        literal.parse::<i64>().ok()?
                    }
                    _ => return None,
                };
                set.insert(v);
            }
            Some(set)
        }
        _ => None,
    }
}

/// A totally-ordered, signed-zero-normalizing wrapper for `f64` values.
///
/// Serves two roles:
/// - **Dense-interval bounds** (`DenseInterval<OrdF64>` for `xsd:float`/`double`):
///   `total_cmp` gives the magnitude order; the signed-zero normalization below
///   ensures `-0.0` and `+0.0` collapse to the same bound, preventing a spurious
///   "empty intersection" (see the SIGNED-ZERO LANDMINE note).
/// - **Finite-set members** (`FiniteSet<OrdF64>` for `DataOneOf` of floats):
///   `total_cmp` gives a total order for `BTreeSet` storage; the same
///   normalization ensures `-0.0` and `+0.0` dedup to the same element
///   (capacity 1, not 2 — the FP hazard in the set bucket).
///
/// Both uses require the SAME invariants, so this single type serves both.
///
/// SOUNDNESS NOTE: `PartialEq` is implemented via `total_cmp` (same as `Ord`)
/// so the two agree — `DenseInterval::capacity()`'s `lo == hi` point check and
/// the `is_empty`/`disjoint` `lo > hi` checks all rely on this consistency.
///
/// SIGNED-ZERO LANDMINE: `f64::total_cmp` orders `-0.0 < +0.0` (it does NOT
/// collapse them), whereas IEEE-754 equality treats `-0.0 == +0.0` as the SAME
/// value. If a raw `-0.0` reached the interval algebra, the disjoint-packing
/// rule could see `[a,-0.0]` and `[+0.0,b]` as disjoint (their intersection
/// `[+0.0,-0.0]` is `lo > hi` under `total_cmp` ⇒ "empty") and fire a SPURIOUS
/// counting clash = false unsat = FP. For the set bucket, `-0.0` and `+0.0`
/// would appear as two distinct elements (capacity 2) even though they are the
/// same IEEE value (capacity 1) — also FP. The only finite value where
/// `total_cmp` disagrees with IEEE equality is signed zero (NaN is
/// parse-rejected), so [`OrdF64::new`] normalizes `-0.0 → +0.0` at
/// construction, restoring agreement. Construct `OrdF64` bounds ONLY via
/// [`OrdF64::new`], never the tuple constructor.
#[derive(Clone, Copy, Debug)]
pub struct OrdF64(pub f64);

impl OrdF64 {
    /// Construct, normalizing signed zero so `total_cmp`-equality agrees
    /// with IEEE-754 equality on the only finite value where they diverge.
    /// FP-critical — see the type-level signed-zero note.
    #[must_use]
    pub fn new(v: f64) -> Self {
        // `v == 0.0` is true for both `-0.0` and `+0.0`; `+ 0.0` canonicalizes
        // `-0.0` to `+0.0` (and is a no-op for `+0.0`).
        Self(if v == 0.0 { 0.0 } else { v })
    }

    /// Recover the wrapped `f64`.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        self.0
    }
}

impl PartialEq for OrdF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}
impl Eq for OrdF64 {}
impl PartialOrd for OrdF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrdF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Shared core of the two f64-keyed `DataOneOf` parsers: accept a `DataOneOf`
/// whose members are ALL literals typed EXACTLY `want_dt`, mapping each lexical
/// through `to_value`. A single member of another datatype (or a bare/
/// language-tagged literal) drops the WHOLE enumeration — never a partial set,
/// which would be unsound on a sufficient-direction RHS.
fn parse_typed_float_oneof<A: ForIRI>(
    dr: &DataRange<A>,
    want_dt: &str,
    to_value: impl Fn(&str) -> Option<f64>,
) -> Option<BTreeSet<OrdF64>> {
    match dr {
        DataRange::DataOneOf(lits) if !lits.is_empty() => {
            let mut set = BTreeSet::new();
            for l in lits {
                let v = match l {
                    Literal::Datatype {
                        literal,
                        datatype_iri,
                    } if datatype_iri.as_ref() == want_dt => to_value(literal)?,
                    _ => return None,
                };
                set.insert(OrdF64::new(v));
            }
            Some(set)
        }
        _ => None,
    }
}

/// Parse a `DataOneOf` whose members are ALL **`xsd:float`**-typed literals into
/// a `BTreeSet<OrdF64>` (capacity = |distinct values|), at **f32 precision**
/// (parse as `f32`, widen via `f64::from`). NaN / ±∞ → `None` (sound
/// under-approx). Signed-zero dedup via `OrdF64::new` is FP-critical (see type
/// doc above).
///
/// **FP-CRITICAL — f32 precision, and a separate bucket from `xsd:double`.**
/// Two lexicals that round to the same `f32` (`"0.1"` and
/// `"0.100000001490116119384765625"`) denote the SAME `xsd:float` value; parsing
/// them as `f64` would give two distinct keys, which the `DataOneOf`
/// disjointness seeding (`RUSTDL_DKEY_ONEOF_SEED`) would then declare DISJOINT —
/// a false unsat. Same lesson as `parse_float32_facets` on the interval side.
pub(crate) fn parse_xsd_float_oneof<A: ForIRI>(dr: &DataRange<A>) -> Option<BTreeSet<OrdF64>> {
    parse_typed_float_oneof(dr, "http://www.w3.org/2001/XMLSchema#float", |lex| {
        let v: f32 = lex.parse().ok().filter(|v: &f32| v.is_finite())?;
        Some(f64::from(v))
    })
}

/// Parse a `DataOneOf` whose members are ALL **`xsd:double`**-typed literals
/// (f64 precision — `xsd:double`'s value space IS f64, exact round-trip).
///
/// **FP-CRITICAL — a SEPARATE bucket from `xsd:float`.** OWL 2 gives
/// `xsd:float` and `xsd:double` DISJOINT value spaces, so a float `1.0` and a
/// double `1.0` are different data values. Folding both into one f64 key (the
/// pre-2026-08-01 behaviour) made `∃p.DataOneOf("1.0"^^xsd:float)` and
/// `∃p.DataOneOf("1.0"^^xsd:double)` the SAME `DKey` class and hence
/// EQUIVALENT — a false positive against Konclude ∪ HermiT, present with no
/// seeding at all. A `DataOneOf` mixing float and double members is rejected by
/// both parsers and drops (sound under-approx).
pub(crate) fn parse_xsd_double_oneof<A: ForIRI>(dr: &DataRange<A>) -> Option<BTreeSet<OrdF64>> {
    parse_typed_float_oneof(dr, "http://www.w3.org/2001/XMLSchema#double", |lex| {
        lex.parse::<f64>().ok().filter(|v: &f64| v.is_finite())
    })
}

/// Parse a `DataOneOf` whose members are ALL `xsd:decimal`-typed literals
/// into a `BTreeSet<Decimal>` (capacity = |distinct values|).
/// Normalized lexical form ensures e.g. `"1.5"` and `"1.50"` dedup to one
/// element. Any member that fails `parse_decimal` → `None`.
pub(crate) fn parse_decimal_oneof<A: ForIRI>(dr: &DataRange<A>) -> Option<BTreeSet<Decimal>> {
    match dr {
        DataRange::DataOneOf(lits) if !lits.is_empty() => {
            let mut set = BTreeSet::new();
            for l in lits {
                let v = match l {
                    Literal::Datatype {
                        literal,
                        datatype_iri,
                    } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#decimal" => {
                        parse_decimal(literal)?
                    }
                    _ => return None,
                };
                set.insert(v);
            }
            Some(set)
        }
        _ => None,
    }
}

/// Parse a `DataOneOf` whose members are ALL `xsd:date`-typed literals into
/// a `BTreeSet<DateKey>` (capacity = |distinct values|). Timezone-bearing
/// dates → `None` (sound under-approx via `parse_date`).
pub(crate) fn parse_date_oneof<A: ForIRI>(dr: &DataRange<A>) -> Option<BTreeSet<DateKey>> {
    match dr {
        DataRange::DataOneOf(lits) if !lits.is_empty() => {
            let mut set = BTreeSet::new();
            for l in lits {
                let v = match l {
                    Literal::Datatype {
                        literal,
                        datatype_iri,
                    } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#date" => {
                        parse_date(literal)?
                    }
                    _ => return None,
                };
                set.insert(v);
            }
            Some(set)
        }
        _ => None,
    }
}

/// Parse a `DataOneOf` whose members are ALL `xsd:dateTime`-typed literals
/// into a `BTreeSet<DateTimeKey>`. Fractional seconds / timezone → `None`.
pub(crate) fn parse_datetime_oneof<A: ForIRI>(dr: &DataRange<A>) -> Option<BTreeSet<DateTimeKey>> {
    match dr {
        DataRange::DataOneOf(lits) if !lits.is_empty() => {
            let mut set = BTreeSet::new();
            for l in lits {
                let v = match l {
                    Literal::Datatype {
                        literal,
                        datatype_iri,
                    } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#dateTime" => {
                        parse_datetime(literal)?
                    }
                    _ => return None,
                };
                set.insert(v);
            }
            Some(set)
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
    /// DP-2: `ClassAssertion(C, a)` with `C` atomic → `(ind_iri, class_iri)`.
    class_assertions: Vec<(String, String)>,
    /// DP-2: atomic `SubClassOf(sub, super)` edges (for the told-subsumer
    /// closure used to resolve an individual's types).
    subclass_atomic: Vec<(String, String)>,
    /// DP-2: per-individual `xsd:string`-typed data values, keyed
    /// `(ind_iri, dp_iri) → {value …}` (for data-cardinality counting).
    ind_string_values: BTreeMap<(String, String), BTreeSet<String>>,
    /// DP-2: `C ⊑ ≤n dp` (`DataMax`/`DataExact` Max-half) keyed
    /// `(class_iri, dp_iri) → min-n`, **only when the cardinality's data-range
    /// qualifier admits string values** (`rdfs:Literal`/unqualified or a
    /// string datatype). A numeric/other qualifier is excluded — its `≤n`
    /// bounds non-string fillers, so counting string values against it would
    /// be unsound (false `Inconsistent`). Separate from `class_max` (which is
    /// qualifier-agnostic and feeds the D4 single-class clash patterns).
    class_max_string: BTreeMap<(String, String), u32>,
    /// DP-DJ: `DisjointDataProperties(dp1, dp2, …)` — recorded as all
    /// pairwise unordered pairs `(a, b)` with `a < b` (lexicographic).
    /// Used by `emit_disjoint_dp_same_value_clash` to detect same-value
    /// violations: `DisjointDataProperties(dp,dq)` + `DataPropertyAssertion(dp,a,v)` +
    /// `DataPropertyAssertion(dq,a,v)` (same individual `a`, same canonical value `v`)
    /// ⇒ global inconsistency ⇒ emit `Top ⊑ Bot`.
    disjoint_dp_pairs: Vec<(String, String)>,
    /// DP-DJ: per-`(individual_iri, dp_iri)` → set of canonical [`DjLiteralKey`]
    /// values. Populated in parallel with `ind_string_values` but handles a
    /// broader set of datatypes (integer, decimal, double, float via f32, date,
    /// dateTime, string). Used by `emit_disjoint_dp_same_value_clash`.
    ind_dj_values: BTreeMap<(String, String), BTreeSet<DjLiteralKey>>,
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
                f.data_string_assertions.push((dpe_iri(&ax.dp), v.clone()));
                // DP-2: also record per-individual (for data-cardinality).
                if let Some(ind) = individual_iri(&ax.from) {
                    f.ind_string_values
                        .entry((ind, dpe_iri(&ax.dp)))
                        .or_default()
                        .insert(v);
                }
            }
            // DP-DJ: record per-individual canonical value for same-value clash.
            if let (Some(ind), Some(key)) = (individual_iri(&ax.from), literal_to_dj_key(&ax.to)) {
                f.ind_dj_values
                    .entry((ind, dpe_iri(&ax.dp)))
                    .or_default()
                    .insert(key);
            }
        }
        C::ClassAssertion(ax) => {
            // DP-2: atomic class membership for the type-resolution closure.
            if let (Some(c), Some(ind)) = (class_iri(&ax.ce), individual_iri(&ax.i)) {
                f.class_assertions.push((ind, c));
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
        C::DisjointDataProperties(ax) => {
            // DP-DJ: record all pairwise unordered pairs (a < b) for the
            // same-value clash check in `emit_disjoint_dp_same_value_clash`.
            let iris: Vec<String> = ax.0.iter().map(dp_iri).collect();
            for i in 0..iris.len() {
                for j in (i + 1)..iris.len() {
                    let (a, b) = if iris[i] <= iris[j] {
                        (iris[i].clone(), iris[j].clone())
                    } else {
                        (iris[j].clone(), iris[i].clone())
                    };
                    f.disjoint_dp_pairs.push((a, b));
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
        // DP-2: atomic SubClassOf edge for the told-subsumer closure.
        if let Some(sup_iri) = class_iri(sup) {
            f.subclass_atomic.push((sub_iri.clone(), sup_iri));
        }
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
        ClassExpression::DataMaxCardinality { n, dp, dr } => {
            let key = (class_iri.to_string(), dpe_iri(dp));
            let entry = f.class_max.entry(key.clone()).or_insert(u32::MAX);
            *entry = (*entry).min(*n);
            if dr_admits_strings(dr) {
                let e = f.class_max_string.entry(key).or_insert(u32::MAX);
                *e = (*e).min(*n);
            }
        }
        ClassExpression::DataExactCardinality { n, dp, dr } => {
            let key = (class_iri.to_string(), dpe_iri(dp));
            let min_entry = f.class_min.entry(key.clone()).or_insert(0);
            *min_entry = (*min_entry).max(*n);
            let max_entry = f.class_max.entry(key.clone()).or_insert(u32::MAX);
            *max_entry = (*max_entry).min(*n);
            if dr_admits_strings(dr) {
                let e = f.class_max_string.entry(key).or_insert(u32::MAX);
                *e = (*e).min(*n);
            }
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
        // handled by `parse_xsd_float_range`/`parse_xsd_double_range`
        // (DISTINCT datatype buckets — see the DKey datatype-tagging in
        // `convert.rs`).
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

/// Parse an **`xsd:float`-only** `DataRange` into a [`FloatRange`] using
/// **f32 precision**: each facet bound is parsed as `f32` then widened to
/// `f64` (`s.parse::<f32>().ok().map(f64::from)`). This guarantees that two
/// lexicals denoting the same `xsd:float` value map to bit-identical `f64`
/// bounds — the precondition for sound `FloatRange::subset` comparisons
/// inside the DKey encoding (different f64 parses of the same f32 would
/// falsely look like distinct range endpoints).
///
/// Returns `None` for non-`xsd:float` data ranges, unrecognized facets,
/// and any unparseable / non-finite (NaN, ±∞) literal.
pub(crate) fn parse_xsd_float_range<A: ForIRI>(dr: &DataRange<A>) -> Option<FloatRange> {
    const XSD_FLOAT: &str = "http://www.w3.org/2001/XMLSchema#float";
    match dr {
        DataRange::Datatype(dt) if dt.0.as_ref() == XSD_FLOAT => Some(FloatRange::unbounded()),
        DataRange::DatatypeRestriction(dt, facets) if dt.0.as_ref() == XSD_FLOAT => {
            parse_float32_facets(facets)
        }
        _ => None,
    }
}

/// Parse an **`xsd:double`-only** `DataRange` into a [`FloatRange`] using
/// f64 precision (`xsd:double` value space IS f64 — exact round-trip).
/// Mirrors [`parse_xsd_float_range`] but for the double bucket.
/// Returns `None` for non-`xsd:double` ranges, unrecognized facets, NaN/±∞.
pub(crate) fn parse_xsd_double_range<A: ForIRI>(dr: &DataRange<A>) -> Option<FloatRange> {
    const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
    match dr {
        DataRange::Datatype(dt) if dt.0.as_ref() == XSD_DOUBLE => Some(FloatRange::unbounded()),
        DataRange::DatatypeRestriction(dt, facets) if dt.0.as_ref() == XSD_DOUBLE => {
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

/// f32-precision counterpart of [`parse_float_facets`]: each bound is parsed
/// as `f32` then widened to `f64`. Same-f32 lexicals (`"0.1000000014"` and
/// `"0.1000000015"` both round to f32 `0x3DCCCCCD`) produce the SAME bound,
/// ensuring the DKey encoding is sound for the `xsd:float` value space.
fn parse_float32_facets<A: ForIRI>(facets: &[FacetRestriction<A>]) -> Option<FloatRange> {
    let mut range = FloatRange::unbounded();
    for fr in facets {
        // Parse as f32; NaN/±∞ in the f32 domain are also rejected by
        // `is_finite()` on the widened f64 (they widen to NaN/±∞ as well).
        let val: f64 =
            fr.l.literal()
                .parse::<f32>()
                .ok()
                .map(f64::from)
                .filter(|v| v.is_finite())?;
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

// ─────────────────────────────────────────────────────────────────────────────
// DataIntersectionOf lowering (DataIntersectionOf feature)
//
// `DataIntersectionOf([r1, r2, ...])` is lowered to the intersection of the
// member ranges.  This is EXACT (not an approximation), so it is FP-safe by
// construction.
//
// Rules:
//  - ALL members must be parseable to the SAME bucket type.  If any member is
//    unrecognized (no parser matches) → `None` (drop the whole intersection —
//    sound under-approximation).
//  - Mixed buckets (e.g. integer + string) → `DataIntersectionDkey::Empty`
//    (the value spaces are disjoint, so the intersection is provably empty).
//  - Same-bucket intersection that turns out empty (lo > hi etc.) →
//    `DataIntersectionDkey::Empty`.
//  - Successful non-empty fold → `DataIntersectionDkey::Iri(dkey_iri)`.
//
// Nested composites (DataIntersectionOf of DataUnionOf, etc.) → `None` (DROP).
// ─────────────────────────────────────────────────────────────────────────────

/// A single parsed range from one member of `DataIntersectionOf`.
/// Each variant corresponds to one DKey bucket; they are mutually exclusive.
#[derive(Clone, Debug)]
pub(crate) enum RangeBucket {
    Integer(IntegerRange),
    Float(FloatRange),  // xsd:float (f32-precision)
    Double(FloatRange), // xsd:double (f64-precision)
    Decimal(OrdRange<Decimal>),
    Date(OrdRange<DateKey>),
    DateTime(OrdRange<DateTimeKey>),
    Str(StrSet),
}

/// Discriminant tag for [`RangeBucket`] (for cross-bucket detection).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum BucketKind {
    Integer,
    Float,
    Double,
    Decimal,
    Date,
    DateTime,
    Str,
}

impl RangeBucket {
    fn kind(&self) -> BucketKind {
        match self {
            RangeBucket::Integer(_) => BucketKind::Integer,
            RangeBucket::Float(_) => BucketKind::Float,
            RangeBucket::Double(_) => BucketKind::Double,
            RangeBucket::Decimal(_) => BucketKind::Decimal,
            RangeBucket::Date(_) => BucketKind::Date,
            RangeBucket::DateTime(_) => BucketKind::DateTime,
            RangeBucket::Str(_) => BucketKind::Str,
        }
    }
}

/// Tri-state result of [`parse_data_intersection_dkey`].
pub(crate) enum DataIntersectionDkey {
    /// Non-empty intersection; the inner `RangeBucket` holds the folded range.
    /// The caller (in `convert.rs`) converts it to a DKey IRI.
    Bucket(RangeBucket),
    /// The intersection is provably empty (either same-bucket bounds cross,
    /// or members belong to different datatypes with disjoint value spaces).
    /// For `∃p.empty` this signals `C ⊑ ⊥`; for `∀`/PropertyRange, DROP.
    Empty,
}

/// Try to parse a single `DataRange` member into its bucket type.
/// Returns `None` for any unrecognized or composite range.
fn parse_range_bucket<A: horned_owl::model::ForIRI>(dr: &DataRange<A>) -> Option<RangeBucket> {
    if let Some(r) = parse_integer_range(dr) {
        return Some(RangeBucket::Integer(r));
    }
    if let Some(r) = parse_xsd_float_range(dr) {
        return Some(RangeBucket::Float(r));
    }
    if let Some(r) = parse_xsd_double_range(dr) {
        return Some(RangeBucket::Double(r));
    }
    if let Some(r) = parse_decimal_range(dr) {
        return Some(RangeBucket::Decimal(r));
    }
    if let Some(r) = parse_date_range(dr) {
        return Some(RangeBucket::Date(r));
    }
    if let Some(r) = parse_datetime_range(dr) {
        return Some(RangeBucket::DateTime(r));
    }
    if let Some(s) = parse_string_range(dr) {
        return Some(RangeBucket::Str(s));
    }
    // Numeric DataOneOf, nested composites, and other ranges are NOT handled
    // here — they return None → DROP the whole DataIntersectionOf (sound).
    None
}

/// Fold a sequence of same-bucket members into their intersection, then check
/// for emptiness.  Returns `None` if the bucket mixes are incompatible (should
/// not happen — caller guarantees same-kind; kept for safety).
fn fold_same_bucket(members: Vec<RangeBucket>) -> Option<DataIntersectionDkey> {
    // Guaranteed non-empty by caller.
    match members
        .into_iter()
        .try_fold(None::<RangeBucket>, |acc, next| {
            Some(match acc {
                None => Some(next),
                Some(RangeBucket::Integer(a)) => {
                    if let RangeBucket::Integer(b) = next {
                        Some(RangeBucket::Integer(a.intersect(b)))
                    } else {
                        return None;
                    }
                }
                Some(RangeBucket::Float(a)) => {
                    if let RangeBucket::Float(b) = next {
                        Some(RangeBucket::Float(a.intersect(b)))
                    } else {
                        return None;
                    }
                }
                Some(RangeBucket::Double(a)) => {
                    if let RangeBucket::Double(b) = next {
                        Some(RangeBucket::Double(a.intersect(b)))
                    } else {
                        return None;
                    }
                }
                Some(RangeBucket::Decimal(a)) => {
                    if let RangeBucket::Decimal(b) = next {
                        Some(RangeBucket::Decimal(a.intersect(&b)))
                    } else {
                        return None;
                    }
                }
                Some(RangeBucket::Date(a)) => {
                    if let RangeBucket::Date(b) = next {
                        Some(RangeBucket::Date(a.intersect(&b)))
                    } else {
                        return None;
                    }
                }
                Some(RangeBucket::DateTime(a)) => {
                    if let RangeBucket::DateTime(b) = next {
                        Some(RangeBucket::DateTime(a.intersect(&b)))
                    } else {
                        return None;
                    }
                }
                Some(RangeBucket::Str(a)) => {
                    if let RangeBucket::Str(b) = next {
                        Some(RangeBucket::Str(a.intersect(&b)))
                    } else {
                        return None;
                    }
                }
            })
        }) {
        // None = mixed bucket (defensive) or empty accumulator (should not happen).
        None | Some(None) => None,
        Some(Some(result)) => {
            // Check for emptiness.
            let empty = match &result {
                RangeBucket::Integer(r) => r.is_empty(),
                RangeBucket::Float(r) | RangeBucket::Double(r) => r.is_empty(),
                RangeBucket::Decimal(r) => r.is_empty(),
                RangeBucket::Date(r) => r.is_empty(),
                RangeBucket::DateTime(r) => r.is_empty(),
                RangeBucket::Str(s) => s.is_empty(),
            };
            if empty {
                return Some(DataIntersectionDkey::Empty);
            }
            // Return the bucket; convert.rs will build the DKey IRI.
            Some(DataIntersectionDkey::Bucket(result))
        }
    }
}

/// Phase (DataIntersectionOf): attempt to lower a `DataRange::DataIntersectionOf`
/// to a single DKey IRI (or empty-intersection signal).
///
/// Returns:
/// - `Some(DataIntersectionDkey::Bucket(b))` — folded non-empty range.
/// - `Some(DataIntersectionDkey::Empty)` — provably empty intersection
///   (same-bucket disjoint, or cross-bucket with provably disjoint value
///   spaces; see soundness note on `integer ∩ decimal` below).
/// - `None` — any member unrecognized, nested composite, cross-bucket
///   integer×decimal mix (value spaces overlap), or mixed-repr such as
///   integer interval + integer `DataOneOf` → DROP (sound).
///
/// **Soundness note — `xsd:integer` ∩ `xsd:decimal`:** XSD defines
/// `xsd:integer` as a sub-datatype of `xsd:decimal`, so their value
/// spaces overlap.  A cross-bucket set whose *only* kinds are Integer
/// and Decimal is therefore NOT provably empty → returns `None` (DROP).
/// All other cross-bucket combinations (e.g. numeric × temporal,
/// numeric × string, float × double) involve genuinely disjoint XSD
/// value spaces → `DataIntersectionDkey::Empty`.
///
/// Only triggers when `dr` is `DataRange::DataIntersectionOf` **and**
/// `data_properties_enabled()` is true; all other `DataRange` variants
/// return `None` so the caller falls through to the normal parsers.
pub(crate) fn parse_data_intersection_dkey<A: horned_owl::model::ForIRI>(
    dr: &DataRange<A>,
) -> Option<DataIntersectionDkey> {
    let DataRange::DataIntersectionOf(members) = dr else {
        return None;
    };
    if members.is_empty() {
        // Empty DataIntersectionOf is formally ⊤ in OWL 2, but the spec says
        // DataIntersectionOf needs ≥2 members; treat as unrecognized → DROP.
        return None;
    }
    // Parse every member.  ANY unrecognized member → drop the whole thing.
    let parsed: Vec<RangeBucket> = members
        .iter()
        .map(parse_range_bucket)
        .collect::<Option<_>>()?;

    // Check bucket kinds.
    let first_kind = parsed[0].kind();
    let all_same = parsed.iter().all(|b| b.kind() == first_kind);
    if !all_same {
        // Cross-bucket: value spaces are disjoint → empty intersection …
        // EXCEPT for the `xsd:integer` × `xsd:decimal` pair: integer is a
        // *subset* of decimal in XSD, so their intersection is non-empty.
        // For any set of kinds that is a subset of {Integer, Decimal},
        // we can't prove emptiness → DROP (sound under-approximation).
        let present_kinds: std::collections::HashSet<BucketKind> =
            parsed.iter().map(RangeBucket::kind).collect();
        let only_int_dec = present_kinds
            .iter()
            .all(|k| matches!(k, BucketKind::Integer | BucketKind::Decimal));
        if only_int_dec {
            return None;
        }
        // All other cross-bucket mixes involve genuinely disjoint value
        // spaces → the intersection is provably empty.
        return Some(DataIntersectionDkey::Empty);
    }
    // All same bucket — fold.
    fold_same_bucket(parsed)
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

/// Whether a data-cardinality qualifier `dr` admits `xsd:string` fillers —
/// i.e. counting an individual's string values against a `≤n dp dr` bound is
/// sound. True for the unqualified case (`rdfs:Literal`, where every value
/// counts) and for a string-family datatype; false for numeric/temporal/etc.
/// qualifiers (strings are not in `dr`, so must not be counted) and for
/// unions/unknown (uncertain → excluded). Gates DP-2.
fn dr_admits_strings<A: ForIRI>(dr: &DataRange<A>) -> bool {
    const RDFS_LITERAL: &str = "http://www.w3.org/2000/01/rdf-schema#Literal";
    if let DataRange::Datatype(dt) = dr
        && dt.0.as_ref() == RDFS_LITERAL
    {
        return true;
    }
    data_range_family(dr) == Some(DtFamily::TextPlain)
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

/// DP-1 value-level: test whether `lit` is **provably outside** `range`.
///
/// Returns `true` ONLY when BOTH conditions hold:
/// 1. The range and literal belong to the **same datatype bucket** (integer /
///    float / decimal / date / dateTime / string).
/// 2. The literal's value is outside the range's bounds.
///
/// Returns `false` on any uncertainty: different buckets, parse failure,
/// unrecognised range — sound under-approximation (miss ⇒ incomplete,
/// never an FP). Cross-datatype mismatch is already handled by the
/// family-level `emit_data_range_violations`; this function is strictly
/// same-bucket value checking.
///
/// **Membership via `subset`**: `v ∈ range` ⟺ `point(v).subset(range)`.
/// Delegates to the audited boundary logic in each range type's
/// `subset` method — no new comparison arithmetic is introduced here.
///
/// **Soundness for floats**: NaN/±∞ are rejected by `float_literal_value`
/// (returns `None`), so they can never falsely fire.
pub fn literal_provably_outside_range<A: ForIRI>(range: &DataRange<A>, lit: &Literal<A>) -> bool {
    // Integer bucket: both must parse to i64 in the xsd:integer value space.
    if let Some(r) = parse_integer_range(range) {
        // Only fire when the literal is also xsd:integer-typed.
        let Some(v) = integer_literal_value_pub(lit) else {
            return false;
        };
        // v ∈ r  ⟺  point(v).subset(r). NOT in range ⟺ NOT subset.
        return !IntegerRange::point(v).subset(r);
    }
    // Float bucket: **xsd:double ONLY** (NOT xsd:float).
    //
    // SOUNDNESS LANDMINE: `FloatRange::subset` is only sound when its two
    // operands are bit-identical for equal values — its own `float_cmp`
    // allow is justified by "both operands round-tripped through the same
    // `to_bits` key" (the D6 DKey subsumption path). DP-1 BREAKS that
    // precondition: the ABox value literal and the facet bound are
    // INDEPENDENTLY-AUTHORED lexicals. For 32-bit `xsd:float`, a value
    // genuinely in range whose decimal isn't exactly f32-representable can
    // parse (as f64) just past the bound's f64 → `!subset` → a FALSE
    // `Top ⊑ Bot` (catastrophic false-inconsistent — e.g. bound
    // `0.1000000014` and value `0.1000000015` denote the SAME f32 yet differ
    // as f64). `xsd:double`'s value space IS f64, so the f64 comparison is
    // exact and sound there. `xsd:float` is therefore DROPPED (returns
    // `false` — sound under-approximation; a MISS, never an FP).
    if let Some(r) = parse_double_range(range) {
        let Some(v) = double_literal_value_pub(lit) else {
            return false;
        };
        return !FloatRange::point(v).subset(r);
    }
    // Decimal bucket: xsd:decimal (NEVER f64 — exact value comparison).
    if let Some(r) = parse_decimal_range(range) {
        let Some(v) = decimal_literal_value_pub(lit) else {
            return false;
        };
        return !OrdRange::point(v).subset(&r);
    }
    // Date bucket: xsd:date.
    if let Some(r) = parse_date_range(range) {
        let Some(v) = date_literal_value_pub(lit) else {
            return false;
        };
        return !OrdRange::point(v).subset(&r);
    }
    // DateTime bucket: xsd:dateTime.
    if let Some(r) = parse_datetime_range(range) {
        let Some(v) = datetime_literal_value_pub(lit) else {
            return false;
        };
        return !OrdRange::point(v).subset(&r);
    }
    // String bucket: xsd:string or DataOneOf.
    // For StrSet::Top every string is in range (never outside).
    // For StrSet::Set check membership.
    if let Some(r) = parse_string_range(range) {
        let Some(s) = exact_string_literal(lit) else {
            return false;
        };
        return match &r {
            StrSet::Top => false,
            StrSet::Set(members) => !members.contains(&s),
        };
    }
    // Unrecognised range: don't fire.
    false
}

// DP-1 value-level public accessors — these are the `convert.rs`-private
// literal parsers re-exposed so `literal_provably_outside_range` can call
// them without duplicating logic. Named `*_pub` to avoid colliding with the
// private helpers that also live in `convert.rs`.

fn integer_literal_value_pub<A: ForIRI>(l: &Literal<A>) -> Option<i64> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#integer" => {
            literal.parse::<i64>().ok()
        }
        _ => None,
    }
}

/// xsd:double value literal → exact f64. `xsd:float` is intentionally NOT
/// accepted (see the soundness note in `literal_provably_outside_range`):
/// only `xsd:double`'s value space coincides with f64, so the f64 comparison
/// is exact. NaN / ±∞ are rejected (sound — they drop the check).
fn double_literal_value_pub<A: ForIRI>(l: &Literal<A>) -> Option<f64> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#double" => {
            literal.parse::<f64>().ok().filter(|v| v.is_finite())
        }
        _ => None,
    }
}

/// Parse an **xsd:double-only** `DataRange` into a [`FloatRange`]. Restricts
/// to `xsd:double` so DP-1's value-membership comparison stays exact in f64 —
/// `xsd:float` is dropped at the value-literal side, but a range whose facet
/// datatype is `xsd:float` must ALSO not match here (a float-typed bound
/// against a double value would re-introduce the f32/f64 mismatch). The
/// returned bounds are f64-exact for `xsd:double` facets.
fn parse_double_range<A: ForIRI>(range: &DataRange<A>) -> Option<FloatRange> {
    const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
    match range {
        DataRange::Datatype(dt) if dt.0.as_ref() == XSD_DOUBLE => Some(FloatRange::unbounded()),
        DataRange::DatatypeRestriction(dt, facets) if dt.0.as_ref() == XSD_DOUBLE => {
            parse_float_facets(facets)
        }
        _ => None,
    }
}

fn decimal_literal_value_pub<A: ForIRI>(l: &Literal<A>) -> Option<Decimal> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#decimal" => {
            parse_decimal(literal)
        }
        _ => None,
    }
}

fn date_literal_value_pub<A: ForIRI>(l: &Literal<A>) -> Option<DateKey> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#date" => {
            parse_date(literal)
        }
        _ => None,
    }
}

fn datetime_literal_value_pub<A: ForIRI>(l: &Literal<A>) -> Option<DateTimeKey> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#dateTime" => {
            parse_datetime(literal)
        }
        _ => None,
    }
}

/// DP-1 value-level violation check. For each `DataPropertyAssertion(p, a, lit)`
/// and each `DataPropertyRange(q, R)` where `q` is a (reflexive) super-property
/// of `p`: if `literal_provably_outside_range(R, lit)` ⇒ emit `Top ⊑ Bot`.
///
/// **Soundness**: fires ONLY on a same-bucket provable out-of-range value (see
/// `literal_provably_outside_range`). Cross-type violations are already caught by
/// the family-level `emit_data_range_violations`; this function closes the
/// same-bucket value-level gap (e.g. `-5` against `xsd:integer[>=0]`).
fn emit_data_range_value_violations<A: ForIRI>(
    src: &SetOntology<A>,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    use Component as C;
    // Collect raw DataPropertyRange: p_iri → Vec<DataRange>.
    let mut dp_ranges: BTreeMap<String, Vec<DataRange<A>>> = BTreeMap::new();
    // Sub-data-property edges for super-dp closure.
    let mut sub_dp: Vec<(String, String)> = Vec::new();
    // DataPropertyAssertions: (p_iri, Literal).
    let mut dp_assertions: Vec<(String, Literal<A>)> = Vec::new();
    for ac in src {
        match &ac.component {
            C::DataPropertyRange(ax) => {
                dp_ranges
                    .entry(dpe_iri(&ax.dp))
                    .or_default()
                    .push(ax.dr.clone());
            }
            C::SubDataPropertyOf(ax) => {
                let sub = dpe_iri(&ax.sub);
                let sup = dpe_iri(&ax.sup);
                if !sub.is_empty() && !sup.is_empty() {
                    sub_dp.push((sub, sup));
                }
            }
            C::EquivalentDataProperties(ax) => {
                let iris: Vec<String> = ax.0.iter().map(dpe_iri).collect();
                for i in 0..iris.len() {
                    for j in 0..iris.len() {
                        if i != j {
                            sub_dp.push((iris[i].clone(), iris[j].clone()));
                        }
                    }
                }
            }
            C::DataPropertyAssertion(ax) => {
                dp_assertions.push((dpe_iri(&ax.dp), ax.to.clone()));
            }
            _ => {}
        }
    }
    if dp_ranges.is_empty() || dp_assertions.is_empty() {
        return;
    }
    // Build reflexive-transitive super-dp closure (reuse the existing helper).
    let closure = closure_sub_dp(&sub_dp);
    for (p, lit) in &dp_assertions {
        // Gather all ranges on p and its super-dps.
        let mut ranges: Vec<&DataRange<A>> = dp_ranges.get(p).into_iter().flatten().collect();
        if let Some(supers) = closure.get(p) {
            for q in supers {
                if q != p {
                    ranges.extend(dp_ranges.get(q).into_iter().flatten());
                }
            }
        }
        if ranges
            .iter()
            .any(|r| literal_provably_outside_range(r, lit))
        {
            out.push(Axiom::SubClassOf {
                sub: top_id,
                sup: bot_id,
            });
            return;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// DP-2: FunctionalDataProperty ABox cardinality violation
// ─────────────────────────────────────────────────────────────────────

/// Canonical literal value used for DP-2 distinctness counting.
///
/// Two literals are **provably distinct** iff they differ within the same
/// bucket OR fall in different buckets (disjoint value spaces). A literal
/// that does not parse to any bucket is excluded (sound under-count).
///
/// SOUNDNESS CRITICAL — bucket design:
/// - `Num(Decimal)`: `xsd:integer` AND `xsd:decimal` literals, both parsed
///   via `parse_decimal`. These share the decimal value space
///   (`xsd:integer ⊆ xsd:decimal`), so `"1"^^xsd:integer` and
///   `"1"^^xsd:decimal` denote the SAME value and must NOT be counted as
///   distinct (they collapse to the same `Decimal`). Folding them into one
///   bucket with a shared normalising parser prevents this false-fire.
/// - `Double(OrdF64)`: `xsd:double` ONLY. `xsd:float` is EXCLUDED — two
///   different f64 parses of an xsd:float literal can denote the SAME f32
///   value (the DP-1 f32/f64 mismatch lesson); counting them as distinct
///   would be a false-fire. `OrdF64::new` normalises signed zero so that
///   `-0.0` and `+0.0` (IEEE-equal) hash to the same key.
/// - `Date` / `DateTime`: timezone-bearing values are dropped at parse
///   (see `parse_date`/`parse_datetime`) — sound under-count.
/// - `Str(String)`: exact lexical `xsd:string` identity.
///
/// Cross-bucket pairs are provably distinct (disjoint value spaces) and
/// will naturally cause `BTreeSet<DistinctVal>` to grow beyond 1.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum DistinctVal {
    Num(Decimal),
    Double(OrdF64),
    Date(DateKey),
    DateTime(DateTimeKey),
    Str(String),
}

/// Parse a literal to its canonical [`DistinctVal`], or `None` if the
/// datatype is excluded or unrecognised. A `None` contributes nothing to
/// the count — the sound under-approximation.
fn literal_to_distinct_val<A: ForIRI>(l: &Literal<A>) -> Option<DistinctVal> {
    match l {
        // ── Num bucket: xsd:integer and xsd:decimal both normalise via
        // `parse_decimal`. An integer literal "01" and decimal "1" must
        // map to the SAME Decimal (normalisation strips leading zeros).
        // `integer_literal_value_pub` gives i64 but can't normalise across
        // the two types; `parse_decimal` does: "1" → Decimal{int:"1",…}.
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#integer" => {
            // Parse via decimal so "1" and "01" share the same normalised key,
            // and so the value is deduped against any xsd:decimal "1" assertion.
            parse_decimal(literal).map(DistinctVal::Num)
        }
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#decimal" => {
            parse_decimal(literal).map(DistinctVal::Num)
        }
        // ── Double bucket: xsd:double ONLY (xsd:float excluded).
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#double" => {
            let v: f64 = literal.parse().ok().filter(|v: &f64| v.is_finite())?;
            Some(DistinctVal::Double(OrdF64::new(v)))
        }
        // ── Date bucket.
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#date" => {
            parse_date(literal).map(DistinctVal::Date)
        }
        // ── DateTime bucket.
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#dateTime" => {
            parse_datetime(literal).map(DistinctVal::DateTime)
        }
        // ── String bucket: bare (xsd:string) or typed xsd:string.
        // Language-tagged literals (Literal::Language) are EXCLUDED —
        // they are rdf:langString, a DIFFERENT datatype than xsd:string.
        Literal::Simple { literal } => Some(DistinctVal::Str(literal.clone())),
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#string" => {
            Some(DistinctVal::Str(literal.clone()))
        }
        // ── Everything else (xsd:float, rdf:langString, xsd:boolean, …)
        // is excluded (sound under-count — contributes nothing).
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────
// DP-DJ canonical value key
// ─────────────────────────────────────────────────────────────────────

/// Canonical value key for disjoint-data-property same-value clash detection
/// (DP-DJ). Like [`DistinctVal`] but adds `Float(OrdF64)` for `xsd:float` at
/// **f32 precision** (parse as `f32`, widen via `f64::from`). This means two
/// lexicals that round to the same `f32` (e.g., `"0.1000000014"` and
/// `"0.1000000015"`) map to the same key — consistent with how DKey lowering
/// in `convert.rs` treats `xsd:float` values, so the same-value equality here
/// agrees with the DKey-level equality check. `Num` covers both `xsd:integer`
/// and `xsd:decimal` (integer ⊆ decimal value space → sound merged bucket).
///
/// `OrdF64::new` normalises `−0.0 → +0.0` (the only finite value where
/// `total_cmp` disagrees with IEEE equality), so signed-zero FP is impossible.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum DjLiteralKey {
    Num(Decimal),
    Float(OrdF64),
    Double(OrdF64),
    Date(DateKey),
    DateTime(DateTimeKey),
    Str(String),
}

/// Parse a literal to its canonical [`DjLiteralKey`], or `None` if the
/// datatype is unrecognised or should be dropped (timezone-bearing
/// date/dateTime, NaN/±∞ float/double). `None` is a sound under-count —
/// it never causes a false `Inconsistent`.
fn literal_to_dj_key<A: ForIRI>(l: &Literal<A>) -> Option<DjLiteralKey> {
    match l {
        // ── Num bucket: xsd:integer and xsd:decimal both via parse_decimal.
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#integer" => {
            parse_decimal(literal).map(DjLiteralKey::Num)
        }
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#decimal" => {
            parse_decimal(literal).map(DjLiteralKey::Num)
        }
        // ── Float bucket: xsd:float at f32 precision (parse as f32, widen).
        // This matches the DKey lowering precision so same-value equality
        // is consistent end-to-end. Reject NaN and ±∞ (no finite model).
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#float" => {
            let v: f32 = literal.parse().ok().filter(|v: &f32| v.is_finite())?;
            Some(DjLiteralKey::Float(OrdF64::new(f64::from(v))))
        }
        // ── Double bucket: xsd:double at f64 precision. Reject NaN and ±∞.
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#double" => {
            let v: f64 = literal.parse().ok().filter(|v: &f64| v.is_finite())?;
            Some(DjLiteralKey::Double(OrdF64::new(v)))
        }
        // ── Date bucket (timezone-bearing dropped at parse — see parse_date).
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#date" => {
            parse_date(literal).map(DjLiteralKey::Date)
        }
        // ── DateTime bucket (timezone / fractional-second dropped at parse).
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#dateTime" => {
            parse_datetime(literal).map(DjLiteralKey::DateTime)
        }
        // ── String bucket: bare (xsd:string) or typed xsd:string.
        // Language-tagged (rdf:langString) excluded — different datatype.
        Literal::Simple { literal } => Some(DjLiteralKey::Str(literal.clone())),
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#string" => {
            Some(DjLiteralKey::Str(literal.clone()))
        }
        // ── Everything else: excluded (sound under-count, never FP).
        _ => None,
    }
}

/// A [`Decimal`] as an `i64`, or `None` when it is not a representable integer:
/// a non-empty fraction (`1.5` is not an `xsd:integer` value) or a magnitude
/// outside `i64`. `None` => the value is not counted against an integer range
/// (a sound under-count — never a false-fire).
fn decimal_as_i64(d: &Decimal) -> Option<i64> {
    if !d.frac.is_empty() {
        return None;
    }
    let mag: i128 = if d.int.is_empty() {
        0
    } else {
        d.int.parse().ok()?
    };
    let signed: i128 = if d.negative { -mag } else { mag };
    i64::try_from(signed).ok()
}

/// `true` iff `v` is **provably** a value of `dr`. Reuses each range type's
/// reviewed `subset` via its singleton constructor — `{v} ⊆ dr` is exactly
/// `v ∈ dr`, with no new boundary algebra. The parsers are pairwise mutually
/// exclusive by datatype, so a value is tested only when its family matches
/// `dr`; any parser returning `None` => not provably in `dr` => `false` (sound
/// under-count). Subtleties:
/// - A `Num` (integer/decimal) is checked against `xsd:integer` first via
///   `decimal_as_i64`; if that returns `None` (non-integer decimal like `1.5`)
///   the integer range rejects it, and the decimal range is tried next.
/// - `Double` (`xsd:double` only) uses `parse_double_range` (not the DKey
///   path) to avoid the f32/f64 soundness landmine: an `xsd:float`-typed bound
///   with f64 arithmetic can mis-compare; `xsd:double` is exactly f64.
/// - Cross-family values never match (the matching parser returns `None`).
///
/// **FP-critical**: a false `true` would over-count distinct in-range values
/// and emit a spurious `Top ⊑ Bot`. When uncertain, MUST return `false`.
fn value_in_range<A: ForIRI>(v: &DistinctVal, dr: &DataRange<A>) -> bool {
    match v {
        DistinctVal::Num(dec) => {
            if let Some(ir) = parse_integer_range(dr) {
                return decimal_as_i64(dec).is_some_and(|i| IntegerRange::point(i).subset(ir));
            }
            parse_decimal_range(dr).is_some_and(|r| OrdRange::point(dec.clone()).subset(&r))
        }
        DistinctVal::Double(f) => {
            parse_double_range(dr).is_some_and(|r| FloatRange::point(f.0).subset(r))
        }
        DistinctVal::Date(d) => {
            parse_date_range(dr).is_some_and(|r| OrdRange::point(*d).subset(&r))
        }
        DistinctVal::DateTime(d) => {
            parse_datetime_range(dr).is_some_and(|r| OrdRange::point(*d).subset(&r))
        }
        DistinctVal::Str(s) => {
            parse_string_range(dr).is_some_and(|r| StrSet::singleton(s.clone()).subset(&r))
        }
    }
}

/// DP-2: **Functional data property ABox cardinality violation** ⇒ global
/// inconsistency.
///
/// `FunctionalDataProperty(f)` declares that every individual has AT MOST ONE
/// `f`-value. If an individual `a` has ≥ 2 **provably-distinct** `f`-values
/// (directly or via a sub-data-property `q ⊑ f`), the ABox has no model ⇒
/// emit `Top ⊑ Bot`.
///
/// **Soundness guarantees (the false-`Inconsistent` gate):**
///
/// 1. *Distinctness by value, not syntax*: literals are parsed to canonical
///    [`DistinctVal`] keys (integer and decimal folded; `parse_decimal`
///    normalises `"01"` and `"1"` to the same key). Two literals map to the
///    same key iff they denote the same value → no over-counting.
///
/// 2. *xsd:float excluded*: `xsd:float` (32-bit) is dropped — two different
///    f64 parses can denote the same f32 (the DP-1 f32/f64 lesson). Only
///    `xsd:double` is accepted.
///
/// 3. *Sub-property closure in the SOUND direction*: `FunctionalDataProperty(f)` +
///    `SubDataPropertyOf(q, f)` means `q(a,v) ⟹ f(a,v)`, so `q`'s values count
///    toward `f`'s ≤1 budget. We gather values from `f` AND all transitive
///    sub-properties of `f`. The **wrong** direction (gathering super-properties'
///    values for a functional sub-property) would be unsound (false-fire); the
///    correct direction is sub→super reachability (`f ∈ closure[q]?`), which
///    `closure_sub_dp` answers.
///
/// 4. *Scope is FunctionalDataProperty only* — the ≤1 case. General
///    `DataMaxCardinality`/`SubClassOf(C, ≤n p)` with n>1 requires class
///    membership and is deferred to DP-2b.
///
/// 5. *Anonymous individuals are ignored* (no stable IRI to key on).
fn emit_functional_dp_cardinality_violations<A: ForIRI>(
    src: &SetOntology<A>,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    use Component as C;

    // Collect FunctionalDataProperty declarations.
    let mut functional_dps: BTreeSet<String> = BTreeSet::new();
    // Sub-data-property edges for the closure (same shape as in
    // `emit_data_range_value_violations`).
    let mut sub_dp: Vec<(String, String)> = Vec::new();
    // Per-individual, per-property: all parsed literal values.
    // Keys: (ind_iri, prop_iri); values: BTreeSet<DistinctVal>.
    let mut ind_dp_vals: BTreeMap<(String, String), BTreeSet<DistinctVal>> = BTreeMap::new();

    for ac in src {
        match &ac.component {
            C::FunctionalDataProperty(ax) => {
                functional_dps.insert(dp_iri(&ax.0));
            }
            C::SubDataPropertyOf(ax) => {
                let sub = dpe_iri(&ax.sub);
                let sup = dpe_iri(&ax.sup);
                if !sub.is_empty() && !sup.is_empty() {
                    sub_dp.push((sub, sup));
                }
            }
            C::EquivalentDataProperties(ax) => {
                let iris: Vec<String> = ax.0.iter().map(dpe_iri).collect();
                for i in 0..iris.len() {
                    for j in 0..iris.len() {
                        if i != j {
                            sub_dp.push((iris[i].clone(), iris[j].clone()));
                        }
                    }
                }
            }
            C::DataPropertyAssertion(ax) => {
                // Only named individuals (anonymous have no stable IRI key).
                let Some(ind) = individual_iri(&ax.from) else {
                    continue;
                };
                let prop = dpe_iri(&ax.dp);
                if let Some(v) = literal_to_distinct_val(&ax.to) {
                    ind_dp_vals.entry((ind, prop)).or_default().insert(v);
                }
            }
            _ => {}
        }
    }

    if functional_dps.is_empty() || ind_dp_vals.is_empty() {
        return;
    }

    // Build sub-property super-closure: dp → {dp} ∪ all transitive super-dps.
    // `is_sub_of(q, f)` iff `f ∈ closure[q]` (or `q == f` reflexively).
    let closure = closure_sub_dp(&sub_dp);

    // Collect all distinct individuals.
    let mut all_inds: BTreeSet<&str> = BTreeSet::new();
    for (ind, _) in ind_dp_vals.keys() {
        all_inds.insert(ind.as_str());
    }

    for f in &functional_dps {
        for ind in &all_inds {
            // Collect all distinct values for `f` on this individual,
            // including values on sub-properties of `f`.
            let mut distinct: BTreeSet<DistinctVal> = BTreeSet::new();
            for ((i, q), vals) in &ind_dp_vals {
                if i.as_str() != *ind {
                    continue;
                }
                // `q ⊑ f` iff `f` is in `q`'s super-closure.
                // If `q` has no hierarchy entries it isn't in the closure map;
                // the reflexive fallback (`q == f`) handles that case.
                let is_sub = closure
                    .get(q.as_str())
                    .map_or(q == f, |supers| supers.contains(f));
                if is_sub {
                    distinct.extend(vals.iter().cloned());
                }
            }
            if distinct.len() >= 2 {
                out.push(Axiom::SubClassOf {
                    sub: top_id,
                    sup: bot_id,
                });
                return;
            }
        }
    }
}

/// DP-2b: a typed/faceted from-type data-cardinality violation ⇒ global
/// inconsistency. `ClassAssertion(C₀, a)` with `C₀ ⊑* C` and
/// `C ⊑ ≤n dp.dr` (`DataMax`/`DataExact`) bounds the count of `a`'s `dp`-fillers
/// in `dr`. When `a` is asserted MORE than `n` distinct values provably in `dr`
/// (directly or via a sub-dp `dp' ⊑ dp`), the ABox has no model ⇒ `Top ⊑ Bot`.
///
/// Sound by construction: distinctness via canonical [`DistinctVal`] keys
/// (integer+decimal folded; `xsd:float`/language-tagged excluded); membership
/// via [`value_in_range`] (provably-in-range only; cross-family never counts);
/// `DataMax`/`DataExact` only (never `DataMin`); sub→super dp routing; told
/// reflexive-transitive `SubClassOf` typing; anonymous individuals ignored.
/// Leaves the functional-≤1 and bare-string-≤n checks untouched; overlap is a
/// harmless idempotent `Top ⊑ Bot`.
#[allow(clippy::too_many_lines)]
fn emit_data_cardinality_violations_typed<A: ForIRI>(
    src: &SetOntology<A>,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    use Component as C;

    let mut sub_dp: Vec<(String, String)> = Vec::new();
    let mut subclass_atomic: Vec<(String, String)> = Vec::new();
    let mut ind_dp_vals: BTreeMap<(String, String), BTreeSet<DistinctVal>> = BTreeMap::new();
    let mut ind_classes: Vec<(String, String)> = Vec::new();
    // (class_iri, dp_iri, max_n, data_range)
    let mut constraints: Vec<(String, String, u32, DataRange<A>)> = Vec::new();

    for ac in src {
        match &ac.component {
            C::SubDataPropertyOf(ax) => {
                let (sub, sup) = (dpe_iri(&ax.sub), dpe_iri(&ax.sup));
                if !sub.is_empty() && !sup.is_empty() {
                    sub_dp.push((sub, sup));
                }
            }
            C::EquivalentDataProperties(ax) => {
                let iris: Vec<String> = ax.0.iter().map(dpe_iri).collect();
                for i in 0..iris.len() {
                    for j in 0..iris.len() {
                        if i != j {
                            sub_dp.push((iris[i].clone(), iris[j].clone()));
                        }
                    }
                }
            }
            C::SubClassOf(ax) => {
                if let (Some(s), Some(t)) = (class_iri(&ax.sub), class_iri(&ax.sup)) {
                    subclass_atomic.push((s, t));
                }
                if let Some(c) = class_iri(&ax.sub) {
                    match &ax.sup {
                        ClassExpression::DataMaxCardinality { n, dp, dr }
                        | ClassExpression::DataExactCardinality { n, dp, dr } => {
                            constraints.push((c, dpe_iri(dp), *n, dr.clone()));
                        }
                        _ => {}
                    }
                }
            }
            C::ClassAssertion(ax) => {
                if let (Some(c), Some(ind)) = (class_iri(&ax.ce), individual_iri(&ax.i)) {
                    ind_classes.push((ind, c));
                }
            }
            C::DataPropertyAssertion(ax) => {
                let Some(ind) = individual_iri(&ax.from) else {
                    continue;
                };
                if let Some(v) = literal_to_distinct_val(&ax.to) {
                    ind_dp_vals
                        .entry((ind, dpe_iri(&ax.dp)))
                        .or_default()
                        .insert(v);
                }
            }
            _ => {}
        }
    }

    if constraints.is_empty() || ind_classes.is_empty() || ind_dp_vals.is_empty() {
        return;
    }

    let class_closure = closure_sub_dp(&subclass_atomic);
    let dp_closure = closure_sub_dp(&sub_dp);

    // Individual → all (told) types.
    let mut ind_types: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (ind, c) in &ind_classes {
        let entry = ind_types.entry(ind.as_str()).or_default();
        match class_closure.get(c) {
            Some(supers) => entry.extend(supers.iter().map(String::as_str)),
            None => {
                entry.insert(c.as_str());
            }
        }
    }

    for (class, dp, n, dr) in &constraints {
        for (ind, types) in &ind_types {
            if !types.contains(class.as_str()) {
                continue;
            }
            let mut distinct: BTreeSet<DistinctVal> = BTreeSet::new();
            for ((i, q), vals) in &ind_dp_vals {
                if i.as_str() != *ind {
                    continue;
                }
                let is_sub = dp_closure
                    .get(q.as_str())
                    .map_or(q == dp, |supers| supers.contains(dp));
                if !is_sub {
                    continue;
                }
                for v in vals {
                    if value_in_range(v, dr) {
                        distinct.insert(v.clone());
                    }
                }
            }
            if distinct.len() > *n as usize {
                out.push(Axiom::SubClassOf {
                    sub: top_id,
                    sup: bot_id,
                });
                return;
            }
        }
    }
}

/// DP-DJ: Disjoint-data-properties same-value clash ⇒ global inconsistency.
///
/// `DisjointDataProperties(dp, dq)` + `DataPropertyAssertion(dp, a, v)` +
/// `DataPropertyAssertion(dq, a, v)` (same individual `a`, same canonical
/// value `v`) ⇒ no model ⇒ emit `Top ⊑ Bot`.
///
/// **Sound by construction:**
/// - Equality is checked via [`DjLiteralKey`], which normalises numeric
///   representations consistently with how DKey lowering handles them.
/// - `xsd:float` uses f32 precision so two lexicals that round to the same
///   f32 (and would be the same DKey) correctly match here too.
/// - `OrdF64::new` normalises `−0.0 → +0.0` so signed-zero ≠ is impossible.
/// - Timezone-bearing date/dateTime are dropped at parse (sound under-count).
/// - Unrecognised datatypes return `None` from `literal_to_dj_key` → skipped
///   (sound under-count, never FP).
/// - Gate: entire function is a no-op when `RUSTDL_DATA_PROPERTIES=0`.
fn emit_disjoint_dp_same_value_clash(
    facts: &Facts,
    top_id: ConceptId,
    bot_id: ConceptId,
    out: &mut Vec<Axiom>,
) {
    if !std::env::var("RUSTDL_DATA_PROPERTIES").map_or(true, |v| v != "0") {
        return;
    }
    if facts.disjoint_dp_pairs.is_empty() {
        return;
    }

    // For each disjoint pair (dp, dq) check every individual `a`:
    // if `ind_dj_values[(a, dp)]` ∩ `ind_dj_values[(a, dq)]` is non-empty ⇒ clash.
    //
    // Collect the set of individuals that have values for any property in a pair.
    // We iterate over disjoint pairs and look up both sides in ind_dj_values.
    for (dp, dq) in &facts.disjoint_dp_pairs {
        // Collect all individuals that have at least one value for `dp`.
        // We'll iterate over `ind_dj_values` keys with prefix `dp`.
        // BTreeMap range query: all keys `(ind, dp)` for any `ind`.
        // Use the BTreeMap ordering: (ind, dp) sorted lexicographically.
        // Gather (ind → values_for_dp) for this dp.
        let dp_inds: Vec<(&str, &BTreeSet<DjLiteralKey>)> = facts
            .ind_dj_values
            .iter()
            .filter(|((_, p), _)| p == dp)
            .map(|((ind, _), vals)| (ind.as_str(), vals))
            .collect();

        for (ind, dp_vals) in dp_inds {
            // Look up dq values for the same individual.
            if let Some(dq_vals) = facts.ind_dj_values.get(&(ind.to_owned(), dq.clone())) {
                // If any value appears in both sets → clash.
                if dp_vals.iter().any(|v| dq_vals.contains(v)) {
                    out.push(Axiom::SubClassOf {
                        sub: top_id,
                        sup: bot_id,
                    });
                    return; // One clash is enough to mark global inconsistency.
                }
            }
        }
    }
}

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

    // ── `literal_provably_outside_range` unit tests ────────────────────
    // Per-bucket: in-range → false, out-of-range → true,
    // cross-bucket → false, unparseable → false.

    fn make_int_range(min: i64, max: i64) -> DataRange<RcStr> {
        use horned_owl::model::{Build, Datatype};
        let b: Build<RcStr> = Build::new_rc();
        DataRange::DatatypeRestriction(
            Datatype(b.iri("http://www.w3.org/2001/XMLSchema#integer")),
            vec![
                FacetRestriction {
                    f: Facet::MinInclusive,
                    l: horned_owl::model::Literal::Datatype {
                        literal: min.to_string(),
                        datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
                    },
                },
                FacetRestriction {
                    f: Facet::MaxInclusive,
                    l: horned_owl::model::Literal::Datatype {
                        literal: max.to_string(),
                        datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
                    },
                },
            ],
        )
    }

    fn int_lit(v: i64) -> horned_owl::model::Literal<RcStr> {
        use horned_owl::model::Build;
        let b: Build<RcStr> = Build::new_rc();
        horned_owl::model::Literal::Datatype {
            literal: v.to_string(),
            datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        }
    }

    fn str_lit(s: &str) -> horned_owl::model::Literal<RcStr> {
        horned_owl::model::Literal::Simple {
            literal: s.to_string(),
        }
    }

    #[test]
    fn literal_provably_outside_range_integer_in_range() {
        let r = make_int_range(0, 10);
        // 5 ∈ [0,10] → false (NOT outside)
        assert!(!literal_provably_outside_range(&r, &int_lit(5)));
        // 0 ∈ [0,10] → false
        assert!(!literal_provably_outside_range(&r, &int_lit(0)));
        // 10 ∈ [0,10] → false
        assert!(!literal_provably_outside_range(&r, &int_lit(10)));
    }

    #[test]
    fn literal_provably_outside_range_integer_outside() {
        let r = make_int_range(0, 10);
        // -1 ∉ [0,10] → true
        assert!(literal_provably_outside_range(&r, &int_lit(-1)));
        // 11 ∉ [0,10] → true
        assert!(literal_provably_outside_range(&r, &int_lit(11)));
    }

    #[test]
    fn literal_provably_outside_range_cross_bucket_false() {
        let r = make_int_range(0, 10);
        // string literal against integer range → false (don't fire cross-bucket)
        assert!(!literal_provably_outside_range(&r, &str_lit("hello")));
    }

    #[test]
    fn literal_provably_outside_range_unparseable_false() {
        let r = make_int_range(0, 10);
        use horned_owl::model::Build;
        let b: Build<RcStr> = Build::new_rc();
        let bad_lit = horned_owl::model::Literal::Datatype {
            literal: "not-an-integer".to_string(),
            datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        };
        // Unparseable integer literal → false (under-approximation)
        assert!(!literal_provably_outside_range(&r, &bad_lit));
    }

    #[test]
    fn literal_provably_outside_range_string_oneof() {
        use horned_owl::model::Build;
        let b: Build<RcStr> = Build::new_rc();
        let range = DataRange::DataOneOf(vec![
            horned_owl::model::Literal::Simple {
                literal: "yes".to_string(),
            },
            horned_owl::model::Literal::Simple {
                literal: "no".to_string(),
            },
        ]);
        // "yes" ∈ {"yes","no"} → false
        assert!(!literal_provably_outside_range(&range, &str_lit("yes")));
        // "maybe" ∉ {"yes","no"} → true
        assert!(literal_provably_outside_range(&range, &str_lit("maybe")));
        // integer literal against string oneof → false (cross-bucket)
        let int = horned_owl::model::Literal::Datatype {
            literal: "5".to_string(),
            datatype_iri: b.iri("http://www.w3.org/2001/XMLSchema#integer"),
        };
        assert!(!literal_provably_outside_range(&range, &int));
    }

    #[test]
    fn decimal_as_i64_integer_values() {
        let d = |s| parse_decimal(s).expect("test decimal parses");
        assert_eq!(decimal_as_i64(&d("5")), Some(5));
        assert_eq!(decimal_as_i64(&d("-7")), Some(-7));
        assert_eq!(decimal_as_i64(&d("0")), Some(0));
        // Leading-zero normalisation: "007" is the integer 7.
        assert_eq!(decimal_as_i64(&d("007")), Some(7));
    }

    #[test]
    fn decimal_as_i64_rejects_non_integer_and_overflow() {
        let d = |s| parse_decimal(s).expect("test decimal parses");
        // Non-empty fraction => not an xsd:integer value.
        assert_eq!(decimal_as_i64(&d("1.5")), None);
        // Beyond i64::MAX => unrepresentable here => None (sound under-count).
        assert_eq!(decimal_as_i64(&d("99999999999999999999")), None);
    }

    // ── value_in_range test helpers ────────────────────────────────────────

    fn vir_dt(local: &str) -> horned_owl::model::Datatype<RcStr> {
        use horned_owl::model::Build;
        Build::new_rc().datatype(format!("http://www.w3.org/2001/XMLSchema#{local}"))
    }

    fn vir_dt_lit(value: &str, local: &str) -> horned_owl::model::Literal<RcStr> {
        horned_owl::model::Literal::Datatype {
            literal: value.to_string(),
            datatype_iri: vir_dt(local).0,
        }
    }

    fn vir_restriction(local: &str, facets: &[(&str, &str, &str)]) -> DataRange<RcStr> {
        let frs = facets
            .iter()
            .map(|(f, v, vlocal)| FacetRestriction {
                f: match *f {
                    "minInclusive" => Facet::MinInclusive,
                    "maxInclusive" => Facet::MaxInclusive,
                    "minExclusive" => Facet::MinExclusive,
                    "maxExclusive" => Facet::MaxExclusive,
                    other => panic!("unhandled facet {other}"),
                },
                l: vir_dt_lit(v, vlocal),
            })
            .collect();
        DataRange::DatatypeRestriction(vir_dt(local), frs)
    }

    fn vir_data_one_of(members: &[&str]) -> DataRange<RcStr> {
        DataRange::DataOneOf(
            members
                .iter()
                .map(|m| horned_owl::model::Literal::Simple {
                    literal: (*m).to_string(),
                })
                .collect(),
        )
    }

    // ── value_in_range unit tests ──────────────────────────────────────────

    #[test]
    fn value_in_range_integer_and_decimal() {
        let int5 =
            literal_to_distinct_val(&vir_dt_lit("5", "integer")).expect("5^^xsd:integer parses");
        let dec_1_5 = literal_to_distinct_val(&vir_dt_lit("1.5", "decimal"))
            .expect("1.5^^xsd:decimal parses");

        // Bare xsd:integer admits any integer.
        let r_int = DataRange::Datatype(vir_dt("integer"));
        assert!(value_in_range(&int5, &r_int), "5 ∈ xsd:integer");
        // 1.5 is not an xsd:integer value (non-integer decimal).
        assert!(!value_in_range(&dec_1_5, &r_int), "1.5 ∉ xsd:integer");

        // Bounded integer range [0,3]: 5 is outside.
        let r_int_0_3 = vir_restriction(
            "integer",
            &[
                ("minInclusive", "0", "integer"),
                ("maxInclusive", "3", "integer"),
            ],
        );
        assert!(!value_in_range(&int5, &r_int_0_3), "5 ∉ [0,3]");

        // Bare xsd:decimal admits both integers and decimals.
        let r_dec = DataRange::Datatype(vir_dt("decimal"));
        assert!(value_in_range(&int5, &r_dec), "int 5 ∈ xsd:decimal");
        assert!(value_in_range(&dec_1_5, &r_dec), "1.5 ∈ xsd:decimal");
    }

    #[test]
    fn value_in_range_cross_datatype_and_string() {
        // xsd:double value 5.0 against integer range → false (cross-family).
        let dbl =
            literal_to_distinct_val(&vir_dt_lit("5.0", "double")).expect("5.0^^xsd:double parses");
        assert!(
            !value_in_range(&dbl, &DataRange::Datatype(vir_dt("integer"))),
            "double 5.0 ∉ xsd:integer (cross-family)"
        );
        // xsd:double 5.0 against bare xsd:double → true.
        assert!(
            value_in_range(&dbl, &DataRange::Datatype(vir_dt("double"))),
            "double 5.0 ∈ xsd:double"
        );

        // String membership in DataOneOf.
        let sa = DistinctVal::Str("a".into());
        let sz = DistinctVal::Str("z".into());
        let enum_ab = vir_data_one_of(&["a", "b"]);
        assert!(value_in_range(&sa, &enum_ab), "\"a\" ∈ {{\"a\",\"b\"}}");
        assert!(!value_in_range(&sz, &enum_ab), "\"z\" ∉ {{\"a\",\"b\"}}");
    }
}
