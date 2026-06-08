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

use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{
    ClassExpression, Component, DataProperty, DataRange, FacetRestriction, ForIRI,
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
}
