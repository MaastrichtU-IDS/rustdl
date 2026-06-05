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
#[derive(Clone, Copy, Debug)]
struct IntegerRange {
    min: Option<i64>,
    max: Option<i64>,
}

impl IntegerRange {
    const fn unbounded() -> Self {
        Self {
            min: None,
            max: None,
        }
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
fn parse_integer_range<A: ForIRI>(dr: &DataRange<A>) -> Option<IntegerRange> {
    let DataRange::DatatypeRestriction(dt, facets) = dr else {
        return None;
    };
    // Only xsd:integer for Tier C; other numeric datatypes
    // (xsd:decimal, xsd:double, xsd:dateTime) extend with their own
    // range types but share this preprocessing's algebra.
    if dt.0.to_string() != "http://www.w3.org/2001/XMLSchema#integer" {
        return None;
    }
    parse_integer_facets(facets)
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
        read_ofn(&mut r, ParserConfiguration::default()).unwrap().0
    }

    #[test]
    fn extracts_functional_dp_min_clash() {
        let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
    Declaration(Class(:HasTwoAges))
    Declaration(DataProperty(:age))
    FunctionalDataProperty(:age)
    SubClassOf(:HasTwoAges DataMinCardinality(2 :age))
)
"#;
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
        let src = r#"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/x>
    Declaration(Class(:HasTwoAges))
    Declaration(DataProperty(:age))
    FunctionalDataProperty(:age)
    SubClassOf(:HasTwoAges DataMinCardinality(2 :age))
)
"#;
        let onto = parse_str(src);
        let mut internal = convert_ontology(&onto).unwrap();
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
