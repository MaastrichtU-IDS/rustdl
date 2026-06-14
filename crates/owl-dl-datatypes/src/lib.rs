//! Concrete-domain satisfiability — datatype reasoners for rustdl.
//!
//! See `docs/superpowers/specs/2026-06-11-concrete-domain-solver-design.md`.
//!
//! Decides whether a node's data constraints on one property are jointly
//! satisfiable: *min-demands* (`≥n p.R`, `∃p.R`, `DataHasValue` — at least `n`
//! distinct values in `R`), *max-limits* (`≤m p.S` — at most `m` distinct values
//! in `S`), under a *universal filter* (`∀p.U` — every value in `U`). The
//! feasibility logic ([`card_sat`]) is written once over the [`ValueRange`]
//! trait; the per-datatype *capacity* model is what differs:
//! - **discrete** ([`IntInterval`]) — a bounded interval holds finitely many;
//! - **dense** ([`DenseInterval`], for float/decimal/date/dateTime) — any
//!   non-degenerate interval holds infinitely many (only a single inclusive
//!   point holds exactly one);
//! - **finite-set** ([`FiniteSet`], for `xsd:string`/`DataOneOf`) — `Top` is
//!   infinite, a finite enumeration holds its cardinality.
//!
//! **Load-bearing invariant — REFUTE-ONLY.** This module may only ever justify
//! turning a tableau node UNSAT; it must never license SAT/pruning. So
//! [`card_sat`] returns [`CardSat::Unsat`] *only when provably infeasible* and
//! [`CardSat::Sat`] for everything else (including patterns it does not yet
//! decide). A spurious `Unsat` would be a false clash ⇒ false subsumption (the
//! FP-critical direction); a spurious `Sat` is merely incomplete, never unsound.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

// Re-export the key types from owl-dl-core that parameterize DenseInterval
// for the dense datatype buckets. These are part of the public API of this
// crate (they appear in CardRange variants).
pub use owl_dl_core::{DateKey, DateTimeKey, Decimal};

/// A totally-ordered wrapper around `f64` using [`f64::total_cmp`]. NaN is
/// placed after all finite values and ±∞, which is arbitrary but consistent
/// — we never actually store NaN here (it is rejected at parse time in
/// `owl-dl-core`). The only semantic property we need is: two equal-bits
/// values compare `Equal`, which is guaranteed by `total_cmp`. Eq is
/// derived from the total-order (consistent by construction).
///
/// SOUNDNESS NOTE: `PartialEq` is implemented via `total_cmp` (same as
/// `Ord`) so the two agree — `DenseInterval::capacity()`'s `lo == hi`
/// point check and the `is_empty`/`disjoint` `lo > hi` checks all rely on
/// this consistency.
///
/// SIGNED-ZERO LANDMINE: `f64::total_cmp` orders `-0.0 < +0.0` (it does
/// NOT collapse them), whereas IEEE-754 equality treats `-0.0 == +0.0` as
/// the SAME value. If a raw `-0.0` bound reached the interval algebra, the
/// disjoint-packing rule could see `[a,-0.0]` and `[+0.0,b]` as disjoint
/// (their intersection `[+0.0,-0.0]` is `lo > hi` under `total_cmp` ⇒
/// "empty") and fire a SPURIOUS counting clash = false unsat = FP. The
/// only finite value where `total_cmp` disagrees with IEEE equality is
/// signed zero (NaN is parse-rejected), so [`OrdF64::new`] normalizes
/// `-0.0 → +0.0` at construction, restoring agreement. Construct `OrdF64`
/// bounds ONLY via [`OrdF64::new`], never the tuple constructor.
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

/// Result of a concrete-domain satisfiability check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardSat {
    /// A satisfying assignment may exist (NOT proven unsat). No clash.
    Sat,
    /// Provably no satisfying assignment exists — a sound clash.
    Unsat,
}

/// A decoded datatype range, tagged by its value-space **bucket**. This is the
/// value type of the `ClassId → CardRange` side-map the tableau consults to
/// recognise a `DKey` filler and recover its range without IRI access (see the
/// P2/P3 design spec). Two ranges interact in [`card_sat`] only within the same
/// bucket; the tableau groups a node's data constraints by `(property, bucket)`
/// before deciding. Integer and string buckets are discrete/finite-set;
/// float/decimal/date/dateTime are dense intervals (infinite capacity except
/// for a single inclusive point).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardRange {
    /// `xsd:integer` and its subtypes (discrete).
    Int(IntInterval),
    /// `xsd:string` / `DataOneOf`-of-strings (finite-set; equality-typed, no ordering).
    Str(FiniteSet<String>),
    /// `xsd:float` / `xsd:double` (dense; infinite capacity except a single
    /// inclusive point). Uses [`OrdF64`] so the bound has a total order.
    Float(DenseInterval<OrdF64>),
    /// `xsd:decimal` (dense; exact lexical representation, never `f64`).
    Decimal(DenseInterval<Decimal>),
    /// `xsd:date` (dense; timezone-free component tuple).
    Date(DenseInterval<DateKey>),
    /// `xsd:dateTime` (dense; timezone-free, integer-second component tuple).
    DateTime(DenseInterval<DateTimeKey>),
}

impl CardRange {
    /// The range as an [`IntInterval`] if this is the integer bucket.
    #[must_use]
    pub fn as_int(&self) -> Option<IntInterval> {
        match self {
            CardRange::Int(i) => Some(*i),
            CardRange::Str(_)
            | CardRange::Float(_)
            | CardRange::Decimal(_)
            | CardRange::Date(_)
            | CardRange::DateTime(_) => None,
        }
    }

    /// The range as a [`FiniteSet<String>`] if this is the string bucket.
    #[must_use]
    pub fn as_str(&self) -> Option<FiniteSet<String>> {
        match self {
            CardRange::Str(s) => Some(s.clone()),
            CardRange::Int(_)
            | CardRange::Float(_)
            | CardRange::Decimal(_)
            | CardRange::Date(_)
            | CardRange::DateTime(_) => None,
        }
    }

    /// The range as a [`DenseInterval<OrdF64>`] if this is the float bucket.
    #[must_use]
    pub fn as_float(&self) -> Option<DenseInterval<OrdF64>> {
        match self {
            CardRange::Float(d) => Some(*d),
            CardRange::Int(_)
            | CardRange::Str(_)
            | CardRange::Decimal(_)
            | CardRange::Date(_)
            | CardRange::DateTime(_) => None,
        }
    }

    /// The range as a [`DenseInterval<Decimal>`] if this is the decimal bucket.
    #[must_use]
    pub fn as_decimal(&self) -> Option<DenseInterval<Decimal>> {
        match self {
            CardRange::Decimal(d) => Some(d.clone()),
            CardRange::Int(_)
            | CardRange::Str(_)
            | CardRange::Float(_)
            | CardRange::Date(_)
            | CardRange::DateTime(_) => None,
        }
    }

    /// The range as a [`DenseInterval<DateKey>`] if this is the date bucket.
    #[must_use]
    pub fn as_date(&self) -> Option<DenseInterval<DateKey>> {
        match self {
            CardRange::Date(d) => Some(*d),
            CardRange::Int(_)
            | CardRange::Str(_)
            | CardRange::Float(_)
            | CardRange::Decimal(_)
            | CardRange::DateTime(_) => None,
        }
    }

    /// The range as a [`DenseInterval<DateTimeKey>`] if this is the datetime bucket.
    #[must_use]
    pub fn as_datetime(&self) -> Option<DenseInterval<DateTimeKey>> {
        match self {
            CardRange::DateTime(d) => Some(*d),
            CardRange::Int(_)
            | CardRange::Str(_)
            | CardRange::Float(_)
            | CardRange::Decimal(_)
            | CardRange::Date(_) => None,
        }
    }
}

/// A range of values in some datatype's value space. Implementors supply the
/// boolean algebra and the **capacity** (number of distinct values; `None` =
/// infinite) — the only thing that differs across discrete / dense / finite-set
/// domains.
pub trait ValueRange: Clone {
    /// No value lies in the range.
    fn is_empty(&self) -> bool;
    /// Intersection (may be empty).
    #[must_use]
    fn intersect(&self, other: &Self) -> Self;
    /// Whether `self ⊆ other`.
    fn subset_of(&self, other: &Self) -> bool;
    /// Number of distinct values; `None` = infinite.
    fn capacity(&self) -> Option<u128>;
    /// The unconstrained value space (used when there is no `∀`).
    fn universe() -> Self;
    /// Whether two ranges share no value. Default via [`Self::intersect`].
    fn disjoint(&self, other: &Self) -> bool {
        self.intersect(other).is_empty()
    }
}

/// A cardinality demand or limit: `n` (distinct) values within `range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card<R> {
    /// The qualifying range.
    pub range: R,
    /// The bound `n` (`≥n` for a demand, `≤m` for a limit).
    pub n: u32,
}

impl<R> Card<R> {
    /// Convenience constructor.
    pub const fn new(range: R, n: u32) -> Self {
        Self { range, n }
    }
}

/// Decide whether distinct values can satisfy the constraints on one data
/// property at one node, over any [`ValueRange`] domain. **Refute-only**:
/// returns [`CardSat::Unsat`] only when provably infeasible.
///
/// Soundly detects: empty universal under a positive demand; a demand whose
/// (universal-restricted) range holds fewer than `n` distinct values (the
/// counting clash); a `≥n`-vs-`≤m` subset conflict (`n>m`); and a set of
/// pairwise-disjoint demands inside one limit whose `n`-sum exceeds it.
#[must_use]
pub fn card_sat<R: ValueRange>(
    universal: Option<R>,
    min_demands: &[Card<R>],
    max_limits: &[Card<R>],
) -> CardSat {
    let u = universal.unwrap_or_else(R::universe);

    // Restrict every range to the universal filter (all fillers ∈ U).
    let mins: Vec<Card<R>> = min_demands
        .iter()
        .filter(|d| d.n > 0)
        .map(|d| Card::new(d.range.intersect(&u), d.n))
        .collect();
    let maxs: Vec<Card<R>> = max_limits
        .iter()
        .map(|l| Card::new(l.range.intersect(&u), l.n))
        .collect();

    // (a) Empty universal: any positive demand is unsatisfiable.
    if u.is_empty() {
        return if mins.is_empty() {
            CardSat::Sat
        } else {
            CardSat::Unsat
        };
    }

    // (b) Per-demand capacity: a `≥n` over a range with < n distinct values
    // (incl. empty) cannot be met — the counting clash.
    for d in &mins {
        if matches!(d.range.capacity(), Some(cap) if cap < u128::from(d.n)) {
            return CardSat::Unsat;
        }
    }

    // (c) Direct `≥n` vs `≤m` subset conflict: a demand inside a limit with n>m.
    for d in &mins {
        for l in &maxs {
            if d.range.subset_of(&l.range) && d.n > l.n {
                return CardSat::Unsat;
            }
        }
    }

    // (d) Disjoint-packing: among demands whose range is inside one limit's
    // range, any pairwise-disjoint subset needs distinct values, so its n-sum
    // must fit under the limit. Domain-agnostic greedy — any disjoint subset
    // whose sum exceeds the limit proves infeasibility (sound; not maximal).
    for l in &maxs {
        let mut chosen: Vec<&R> = Vec::new();
        let mut sum: u64 = 0;
        for d in mins.iter().filter(|d| d.range.subset_of(&l.range)) {
            if chosen.iter().all(|c| c.disjoint(&d.range)) {
                chosen.push(&d.range);
                sum += u64::from(d.n);
                if sum > u64::from(l.n) {
                    return CardSat::Unsat;
                }
            }
        }
    }

    CardSat::Sat
}

// ─────────────────────────────────────────────────────────────────────
// Discrete domain: xsd:integer (and subtypes)
// ─────────────────────────────────────────────────────────────────────

/// A closed integer interval `[min, max]` (inclusive). `None` = unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntInterval {
    /// Inclusive lower bound; `None` = −∞.
    pub min: Option<i64>,
    /// Inclusive upper bound; `None` = +∞.
    pub max: Option<i64>,
}

impl IntInterval {
    /// The whole integer line.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            min: None,
            max: None,
        }
    }
    /// The singleton `[v, v]`.
    #[must_use]
    pub const fn point(v: i64) -> Self {
        Self {
            min: Some(v),
            max: Some(v),
        }
    }
    /// `[lo, hi]` inclusive.
    #[must_use]
    pub const fn closed(lo: i64, hi: i64) -> Self {
        Self {
            min: Some(lo),
            max: Some(hi),
        }
    }
}

impl ValueRange for IntInterval {
    fn is_empty(&self) -> bool {
        matches!((self.min, self.max), (Some(lo), Some(hi)) if lo > hi)
    }
    fn intersect(&self, other: &Self) -> Self {
        let min = match (self.min, other.min) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        let max = match (self.max, other.max) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        Self { min, max }
    }
    fn subset_of(&self, other: &Self) -> bool {
        if self.is_empty() {
            return true;
        }
        let lower_ok = match (other.min, self.min) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(o), Some(s)) => s >= o,
        };
        let upper_ok = match (other.max, self.max) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(o), Some(s)) => s <= o,
        };
        lower_ok && upper_ok
    }
    fn capacity(&self) -> Option<u128> {
        match (self.min, self.max) {
            (Some(lo), Some(hi)) if lo <= hi =>
            {
                #[allow(clippy::cast_sign_loss)]
                Some((i128::from(hi) - i128::from(lo) + 1) as u128)
            }
            (Some(_), Some(_)) => Some(0),
            _ => None,
        }
    }
    fn universe() -> Self {
        Self::all()
    }
}

/// Integer concrete-domain satisfiability (thin wrapper over [`card_sat`]).
#[must_use]
pub fn integer_sat(
    universal: Option<IntInterval>,
    min_demands: &[Card<IntInterval>],
    max_limits: &[Card<IntInterval>],
) -> CardSat {
    card_sat(universal, min_demands, max_limits)
}

// ─────────────────────────────────────────────────────────────────────
// Dense domain: xsd:float / double / decimal / date / dateTime
// ─────────────────────────────────────────────────────────────────────

/// A dense interval over a totally-ordered value type, with explicit
/// inclusive/exclusive bounds. Backs `xsd:float`/`double`/`decimal` and (as a
/// conservatively-dense model — no calendar arithmetic) `xsd:date`/`dateTime`.
/// Capacity is **infinite** for any non-degenerate interval; a single inclusive
/// point holds exactly one value (so `≥2 p.{v}` is a sound counting clash).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseInterval<T> {
    /// Lower bound value; `None` = −∞.
    pub min: Option<T>,
    /// Whether the lower bound is inclusive.
    pub min_incl: bool,
    /// Upper bound value; `None` = +∞.
    pub max: Option<T>,
    /// Whether the upper bound is inclusive.
    pub max_incl: bool,
}

impl<T: Ord + Clone> DenseInterval<T> {
    /// The whole (unbounded) line.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            min: None,
            min_incl: false,
            max: None,
            max_incl: false,
        }
    }
    /// The inclusive singleton `[v, v]`.
    #[must_use]
    pub fn point(v: T) -> Self {
        Self {
            min: Some(v.clone()),
            min_incl: true,
            max: Some(v),
            max_incl: true,
        }
    }
}

impl<T: Ord + Clone> ValueRange for DenseInterval<T> {
    fn is_empty(&self) -> bool {
        match (&self.min, &self.max) {
            (Some(lo), Some(hi)) => lo > hi || (lo == hi && !(self.min_incl && self.max_incl)),
            _ => false,
        }
    }
    fn intersect(&self, other: &Self) -> Self {
        // tighter lower: larger value wins; at equal values, exclusive is tighter.
        let (min, min_incl) = match (&self.min, &other.min) {
            (None, None) => (None, false),
            (Some(a), None) => (Some(a.clone()), self.min_incl),
            (None, Some(b)) => (Some(b.clone()), other.min_incl),
            (Some(a), Some(b)) => match a.cmp(b) {
                std::cmp::Ordering::Greater => (Some(a.clone()), self.min_incl),
                std::cmp::Ordering::Less => (Some(b.clone()), other.min_incl),
                std::cmp::Ordering::Equal => (Some(a.clone()), self.min_incl && other.min_incl),
            },
        };
        // tighter upper: smaller value wins; at equal values, exclusive is tighter.
        let (max, max_incl) = match (&self.max, &other.max) {
            (None, None) => (None, false),
            (Some(a), None) => (Some(a.clone()), self.max_incl),
            (None, Some(b)) => (Some(b.clone()), other.max_incl),
            (Some(a), Some(b)) => match a.cmp(b) {
                std::cmp::Ordering::Less => (Some(a.clone()), self.max_incl),
                std::cmp::Ordering::Greater => (Some(b.clone()), other.max_incl),
                std::cmp::Ordering::Equal => (Some(a.clone()), self.max_incl && other.max_incl),
            },
        };
        Self {
            min,
            min_incl,
            max,
            max_incl,
        }
    }
    fn subset_of(&self, other: &Self) -> bool {
        if self.is_empty() {
            return true;
        }
        // `other` must extend at least as far on each side (equal-endpoint rule:
        // ok iff other includes it OR self excludes it).
        let lower_ok = match (&other.min, &self.min) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(o), Some(s)) => s > o || (s == o && (other.min_incl || !self.min_incl)),
        };
        let upper_ok = match (&other.max, &self.max) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(o), Some(s)) => s < o || (s == o && (other.max_incl || !self.max_incl)),
        };
        lower_ok && upper_ok
    }
    fn capacity(&self) -> Option<u128> {
        if self.is_empty() {
            return Some(0);
        }
        // A single inclusive point holds exactly one value; any other
        // non-empty interval is dense ⇒ infinitely many.
        match (&self.min, &self.max) {
            (Some(lo), Some(hi)) if lo == hi && self.min_incl && self.max_incl => Some(1),
            _ => None,
        }
    }
    fn universe() -> Self {
        Self::all()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Finite-set / equality domain: xsd:string + DataOneOf enumerations
// ─────────────────────────────────────────────────────────────────────

/// An equality-typed (non-ordered) value set: `Top` (the whole datatype, e.g.
/// every `xsd:string`) or a finite enumeration. Capacity is infinite for `Top`,
/// the cardinality for a `Set`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FiniteSet<T: Ord> {
    /// The whole value space (infinite).
    Top,
    /// A finite enumeration.
    Set(BTreeSet<T>),
}

impl<T: Ord + Clone> ValueRange for FiniteSet<T> {
    fn is_empty(&self) -> bool {
        matches!(self, FiniteSet::Set(s) if s.is_empty())
    }
    fn intersect(&self, other: &Self) -> Self {
        match (self, other) {
            (FiniteSet::Top, x) | (x, FiniteSet::Top) => x.clone(),
            (FiniteSet::Set(a), FiniteSet::Set(b)) => {
                FiniteSet::Set(a.intersection(b).cloned().collect())
            }
        }
    }
    fn subset_of(&self, other: &Self) -> bool {
        match (self, other) {
            (_, FiniteSet::Top) => true,
            (FiniteSet::Top, FiniteSet::Set(_)) => false,
            (FiniteSet::Set(a), FiniteSet::Set(b)) => a.is_subset(b),
        }
    }
    fn capacity(&self) -> Option<u128> {
        match self {
            FiniteSet::Top => None,
            FiniteSet::Set(s) => Some(s.len() as u128),
        }
    }
    fn universe() -> Self {
        FiniteSet::Top
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: IntInterval = IntInterval {
        min: None,
        max: None,
    };
    fn cl(lo: i64, hi: i64) -> IntInterval {
        IntInterval::closed(lo, hi)
    }

    // ── interval algebra ────────────────────────────────────────────
    #[test]
    fn count_and_emptiness() {
        assert_eq!(cl(0, 1).capacity(), Some(2));
        assert_eq!(cl(5, 5).capacity(), Some(1));
        assert_eq!(ALL.capacity(), None);
        assert_eq!(
            IntInterval {
                min: Some(0),
                max: None
            }
            .capacity(),
            None
        );
        assert!(cl(3, 2).is_empty());
        assert_eq!(cl(3, 2).capacity(), Some(0));
        assert_eq!(
            cl(i64::MIN, i64::MAX).capacity(),
            Some(u128::from(u64::MAX) + 1)
        );
    }

    #[test]
    fn subset_and_intersect() {
        assert!(cl(2, 4).subset_of(&cl(0, 10)));
        assert!(!cl(0, 10).subset_of(&cl(2, 4)));
        assert!(cl(0, 10).subset_of(&ALL));
        assert!(!ALL.subset_of(&cl(0, 10)));
        assert_eq!(cl(0, 10).intersect(&cl(5, 20)), cl(5, 10));
        assert!(cl(0, 3).intersect(&cl(5, 9)).is_empty());
        assert!(cl(0, 3).disjoint(&cl(5, 9)));
        assert!(!cl(0, 5).disjoint(&cl(5, 9)));
    }

    // ── integer: provably unsat ─────────────────────────────────────
    #[test]
    fn int_counting_clash() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 1), 3)], &[]),
            CardSat::Unsat
        );
    }
    #[test]
    fn int_subset_conflict() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 9), 3)], &[Card::new(cl(0, 100), 2)]),
            CardSat::Unsat
        );
    }
    #[test]
    fn int_empty_universal_with_demand() {
        assert_eq!(
            integer_sat(Some(cl(5, 3)), &[Card::new(ALL, 1)], &[]),
            CardSat::Unsat
        );
    }
    #[test]
    fn int_universal_shrinks_below_capacity() {
        assert_eq!(
            integer_sat(Some(cl(0, 1)), &[Card::new(ALL, 3)], &[]),
            CardSat::Unsat
        );
    }
    #[test]
    fn int_disjoint_packing() {
        assert_eq!(
            integer_sat(
                None,
                &[Card::new(cl(0, 1), 2), Card::new(cl(10, 11), 2)],
                &[Card::new(cl(0, 100), 3)]
            ),
            CardSat::Unsat
        );
    }

    // ── integer: satisfiable-but-tight MUST stay SAT (FP gate) ───────
    #[test]
    fn int_exactly_enough_sat() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 1), 2)], &[]),
            CardSat::Sat
        );
    }
    #[test]
    fn int_min_equals_max_sat() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 9), 2)], &[Card::new(cl(0, 9), 2)]),
            CardSat::Sat
        );
    }
    #[test]
    fn int_room_under_limit_sat() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 10), 2)], &[Card::new(cl(0, 10), 5)]),
            CardSat::Sat
        );
    }
    #[test]
    fn int_overlapping_demands_do_not_sum() {
        assert_eq!(
            integer_sat(
                None,
                &[Card::new(cl(0, 10), 2), Card::new(cl(0, 10), 2)],
                &[Card::new(cl(0, 10), 2)]
            ),
            CardSat::Sat
        );
    }
    #[test]
    fn int_unbounded_infinite_capacity() {
        assert_eq!(
            integer_sat(None, &[Card::new(ALL, 1000)], &[]),
            CardSat::Sat
        );
    }
    #[test]
    fn int_limit_on_disjoint_range_irrelevant() {
        assert_eq!(
            integer_sat(
                None,
                &[Card::new(cl(0, 9), 3)],
                &[Card::new(cl(100, 200), 1)]
            ),
            CardSat::Sat
        );
    }
    #[test]
    fn int_no_constraints_sat() {
        assert_eq!(integer_sat(None, &[], &[]), CardSat::Sat);
    }
    #[test]
    fn int_zero_demand_sat() {
        assert_eq!(
            integer_sat(Some(cl(5, 3)), &[Card::new(ALL, 0)], &[]),
            CardSat::Sat
        );
    }

    // ── DENSE (reals/decimal/temporal): capacity = ∞ unless a point ──
    type D = DenseInterval<i64>; // use i64 as a stand-in ordered value type
    fn open(lo: i64, hi: i64) -> D {
        DenseInterval {
            min: Some(lo),
            min_incl: false,
            max: Some(hi),
            max_incl: false,
        }
    }
    fn closed_d(lo: i64, hi: i64) -> D {
        DenseInterval {
            min: Some(lo),
            min_incl: true,
            max: Some(hi),
            max_incl: true,
        }
    }

    #[test]
    fn dense_interval_has_infinite_capacity() {
        // ≥1000 over a dense [0.0,1.0] — infinitely many reals. SAT.
        assert_eq!(
            card_sat::<D>(None, &[Card::new(closed_d(0, 1), 1000)], &[]),
            CardSat::Sat
        );
    }
    #[test]
    fn dense_point_holds_one_value() {
        // ≥2 over the single point {5} — only one value. UNSAT.
        assert_eq!(
            card_sat::<D>(None, &[Card::new(DenseInterval::point(5), 2)], &[]),
            CardSat::Unsat
        );
        // ≥1 over the point — fine. SAT.
        assert_eq!(
            card_sat::<D>(None, &[Card::new(DenseInterval::point(5), 1)], &[]),
            CardSat::Sat
        );
    }
    #[test]
    fn dense_empty_open_point_is_empty() {
        // (5,5) is empty; ∃ over it ⇒ UNSAT.
        assert_eq!(
            card_sat::<D>(None, &[Card::new(open(5, 5), 1)], &[]),
            CardSat::Unsat
        );
    }
    #[test]
    fn dense_subset_conflict() {
        // ≥3 over [0,1] dense ⊆ [0,10], ≤2 over [0,10]. n>m. UNSAT.
        assert_eq!(
            card_sat::<D>(
                None,
                &[Card::new(closed_d(0, 1), 3)],
                &[Card::new(closed_d(0, 10), 2)]
            ),
            CardSat::Unsat
        );
    }
    #[test]
    fn dense_exclusive_boundary_subset() {
        // [1,5] ⊄ (1,5) (1 excluded by other); but (1,5) ⊆ [1,5].
        assert!(!closed_d(1, 5).subset_of(&open(1, 5)));
        assert!(open(1, 5).subset_of(&closed_d(1, 5)));
    }
    #[test]
    fn dense_shared_exclusive_endpoint_disjoint() {
        // [0,5) and (5,10] don't share 5 → disjoint; [0,5] and [5,10] share 5.
        let a = DenseInterval {
            min: Some(0),
            min_incl: true,
            max: Some(5),
            max_incl: false,
        };
        let b = DenseInterval {
            min: Some(5),
            min_incl: false,
            max: Some(10),
            max_incl: true,
        };
        assert!(a.disjoint(&b));
        assert!(!closed_d(0, 5).disjoint(&closed_d(5, 10)));
    }

    // ── FINITE-SET (string/oneOf): capacity = |set|, Top = ∞ ─────────
    fn set(items: &[&str]) -> FiniteSet<String> {
        FiniteSet::Set(items.iter().map(|s| (*s).to_string()).collect())
    }
    #[test]
    fn string_enum_counting_clash() {
        // ≥3 over a 2-element enumeration. UNSAT.
        assert_eq!(
            card_sat(None, &[Card::new(set(&["a", "b"]), 3)], &[]),
            CardSat::Unsat
        );
    }
    #[test]
    fn string_enum_exactly_enough_sat() {
        assert_eq!(
            card_sat(None, &[Card::new(set(&["a", "b"]), 2)], &[]),
            CardSat::Sat
        );
    }
    #[test]
    fn string_top_infinite_capacity() {
        // ≥1000 over xsd:string (Top). SAT.
        assert_eq!(
            card_sat(None, &[Card::new(FiniteSet::<String>::Top, 1000)], &[]),
            CardSat::Sat
        );
    }
    #[test]
    fn string_universal_shrinks_to_enum() {
        // ∀p.{a,b}, ≥3 p.Top ⟹ effective ≥3 over {a,b} = 2 values. UNSAT.
        assert_eq!(
            card_sat(
                Some(set(&["a", "b"])),
                &[Card::new(FiniteSet::<String>::Top, 3)],
                &[]
            ),
            CardSat::Unsat
        );
    }
    #[test]
    fn string_subset_conflict() {
        // ≥2 over {a,b} ⊆ {a,b,c}, ≤1 over {a,b,c}. UNSAT.
        assert_eq!(
            card_sat(
                None,
                &[Card::new(set(&["a", "b"]), 2)],
                &[Card::new(set(&["a", "b", "c"]), 1)]
            ),
            CardSat::Unsat
        );
    }
    #[test]
    fn string_disjoint_sets_irrelevant_limit() {
        // ≥2 over {a,b}; ≤1 over {x,y} (disjoint). Limit irrelevant. SAT.
        assert_eq!(
            card_sat(
                None,
                &[Card::new(set(&["a", "b"]), 2)],
                &[Card::new(set(&["x", "y"]), 1)]
            ),
            CardSat::Sat
        );
    }

    // ── CONCRETE T CAPACITY CANARIES: OrdF64, Decimal, DateKey, DateTimeKey ─

    /// `OrdF64` point: `≥2 p.{1.5}` UNSAT (capacity 1); `≥1 p.{1.5}` SAT.
    #[test]
    fn ordf64_point_capacity() {
        let pt = DenseInterval::point(OrdF64(1.5_f64));
        assert_eq!(
            card_sat::<DenseInterval<OrdF64>>(None, &[Card::new(pt, 2)], &[]),
            CardSat::Unsat,
            "point capacity=1 < 2 => UNSAT"
        );
        assert_eq!(
            card_sat::<DenseInterval<OrdF64>>(None, &[Card::new(pt, 1)], &[]),
            CardSat::Sat,
            "point capacity=1 >= 1 => SAT"
        );
    }

    /// FP GUARD (signed-zero landmine): two `≥1` demands `[-1,-0.0]` and
    /// `[+0.0,1]` under a `≤1` limit `[-1,1]`. They share the value `0.0`
    /// (`-0.0 == +0.0` in IEEE), so ONE filler satisfies both ⟹ SAT. Without
    /// signed-zero normalization, `total_cmp` orders `-0.0 < +0.0`, the
    /// disjoint-packing rule sees them as disjoint (intersection `[+0.0,-0.0]`
    /// is `lo > hi` ⇒ "empty"), and `1+1=2 > 1` fires a SPURIOUS clash = FP.
    /// `OrdF64::new` canonicalizes `-0.0 → +0.0`, restoring overlap.
    #[test]
    fn ordf64_signed_zero_not_falsely_disjoint() {
        let limit = DenseInterval {
            min: Some(OrdF64::new(-1.0)),
            min_incl: true,
            max: Some(OrdF64::new(1.0)),
            max_incl: true,
        };
        let lo_half = DenseInterval {
            min: Some(OrdF64::new(-1.0)),
            min_incl: true,
            max: Some(OrdF64::new(-0.0)),
            max_incl: true,
        };
        let hi_half = DenseInterval {
            min: Some(OrdF64::new(0.0)),
            min_incl: true,
            max: Some(OrdF64::new(1.0)),
            max_incl: true,
        };
        // The two halves must NOT be considered disjoint (they share 0.0).
        assert!(
            !lo_half.disjoint(&hi_half),
            "[-1,-0.0] and [+0.0,1] share 0.0 — must overlap, not be disjoint"
        );
        assert_eq!(
            card_sat::<DenseInterval<OrdF64>>(
                None,
                &[Card::new(lo_half, 1), Card::new(hi_half, 1)],
                &[Card::new(limit, 1)],
            ),
            CardSat::Sat,
            "one filler 0.0 satisfies both demands under the ≤1 limit — must be SAT"
        );
    }

    /// `OrdF64` interval `[0.0,1.0]`: `≥1000` SAT (infinite capacity).
    #[test]
    fn ordf64_interval_infinite_capacity() {
        let iv = DenseInterval {
            min: Some(OrdF64(0.0)),
            min_incl: true,
            max: Some(OrdF64(1.0)),
            max_incl: true,
        };
        assert_eq!(
            card_sat::<DenseInterval<OrdF64>>(None, &[Card::new(iv, 1000)], &[]),
            CardSat::Sat
        );
    }

    /// `OrdF64` exclusive point `(0.0,0.0)`: empty ⟹ `≥1` UNSAT.
    #[test]
    fn ordf64_exclusive_point_empty() {
        let empty = DenseInterval {
            min: Some(OrdF64(0.0)),
            min_incl: false,
            max: Some(OrdF64(0.0)),
            max_incl: false,
        };
        assert_eq!(
            card_sat::<DenseInterval<OrdF64>>(None, &[Card::new(empty, 1)], &[]),
            CardSat::Unsat
        );
    }

    /// `OrdF64` exclusive bounds `≥2` over `(0.0, 1.0)`: capacity ∞ ⟹ SAT.
    #[test]
    fn ordf64_exclusive_bounds_infinite() {
        let iv = DenseInterval {
            min: Some(OrdF64(0.0)),
            min_incl: false,
            max: Some(OrdF64(1.0)),
            max_incl: false,
        };
        assert_eq!(
            card_sat::<DenseInterval<OrdF64>>(None, &[Card::new(iv, 2)], &[]),
            CardSat::Sat
        );
    }

    /// Decimal point: `≥2 p.{1.5}` UNSAT; `≥1` SAT.
    #[test]
    fn decimal_point_capacity() {
        // "1.5" decimal
        let dec = Decimal {
            negative: false,
            int: "1".to_string(),
            frac: "5".to_string(),
        };
        let pt = DenseInterval::point(dec);
        assert_eq!(
            card_sat::<DenseInterval<Decimal>>(None, &[Card::new(pt.clone(), 2)], &[]),
            CardSat::Unsat,
            "decimal point capacity=1 < 2 => UNSAT"
        );
        assert_eq!(
            card_sat::<DenseInterval<Decimal>>(None, &[Card::new(pt, 1)], &[]),
            CardSat::Sat,
            "decimal point capacity=1 >= 1 => SAT"
        );
    }

    /// Decimal interval `[0, 100]`: `≥1000` SAT (infinite capacity).
    #[test]
    fn decimal_interval_infinite_capacity() {
        let zero = Decimal {
            negative: false,
            int: String::new(),
            frac: String::new(),
        };
        let hundred = Decimal {
            negative: false,
            int: "100".to_string(),
            frac: String::new(),
        };
        let iv = DenseInterval {
            min: Some(zero),
            min_incl: true,
            max: Some(hundred),
            max_incl: true,
        };
        assert_eq!(
            card_sat::<DenseInterval<Decimal>>(None, &[Card::new(iv, 1000)], &[]),
            CardSat::Sat
        );
    }

    /// `DateKey` point: `≥2 p.{2020-01-01}` UNSAT; `≥1` SAT.
    #[test]
    fn datekey_point_capacity() {
        let pt = DenseInterval::point((2020_i64, 1_u8, 1_u8));
        assert_eq!(
            card_sat::<DenseInterval<DateKey>>(None, &[Card::new(pt, 2)], &[]),
            CardSat::Unsat,
            "date point capacity=1 < 2 => UNSAT"
        );
        assert_eq!(
            card_sat::<DenseInterval<DateKey>>(None, &[Card::new(pt, 1)], &[]),
            CardSat::Sat
        );
    }

    /// `DateKey` range `[2020-01-01, 2021-12-31]`: `≥1000` SAT (infinite).
    #[test]
    fn datekey_interval_infinite_capacity() {
        let iv = DenseInterval {
            min: Some((2020_i64, 1_u8, 1_u8)),
            min_incl: true,
            max: Some((2021_i64, 12_u8, 31_u8)),
            max_incl: true,
        };
        assert_eq!(
            card_sat::<DenseInterval<DateKey>>(None, &[Card::new(iv, 1000)], &[]),
            CardSat::Sat
        );
    }

    /// `DateTimeKey` point: `≥2 p.{2020-01-01T00:00:00}` UNSAT; `≥1` SAT.
    #[test]
    fn datetimekey_point_capacity() {
        let pt = DenseInterval::point((2020_i64, 1_u8, 1_u8, 0_u8, 0_u8, 0_u8));
        assert_eq!(
            card_sat::<DenseInterval<DateTimeKey>>(None, &[Card::new(pt, 2)], &[]),
            CardSat::Unsat,
            "datetime point capacity=1 < 2 => UNSAT"
        );
        assert_eq!(
            card_sat::<DenseInterval<DateTimeKey>>(None, &[Card::new(pt, 1)], &[]),
            CardSat::Sat
        );
    }

    /// `DateTimeKey` range: `≥1000` SAT (infinite).
    #[test]
    fn datetimekey_interval_infinite_capacity() {
        let iv = DenseInterval {
            min: Some((2020_i64, 1_u8, 1_u8, 0_u8, 0_u8, 0_u8)),
            min_incl: true,
            max: Some((2021_i64, 1_u8, 1_u8, 0_u8, 0_u8, 0_u8)),
            max_incl: true,
        };
        assert_eq!(
            card_sat::<DenseInterval<DateTimeKey>>(None, &[Card::new(iv, 1000)], &[]),
            CardSat::Sat
        );
    }
}

#[cfg(test)]
mod fuzz {
    //! Property-based soundness fuzz: `integer_sat` is REFUTE-ONLY, so its one
    //! inviolable property is **no false `Unsat`** — whenever it reports
    //! `Unsat`, NO satisfying set of distinct integers exists. We check that
    //! against an independent brute-force oracle over a small universe `[0,U]`
    //! (enumerate every subset; a constraint set is feasible iff some subset
    //! satisfies all `≥`/`≤` bounds and lies within the universal filter).
    use super::*;
    use proptest::prelude::*;

    const U: i64 = 10; // universe [0, U] → 2^(U+1) subsets per check

    fn brute_feasible(
        universal: Option<(i64, i64)>,
        mins: &[(i64, i64, u32)],
        maxs: &[(i64, i64, u32)],
    ) -> bool {
        let vals: Vec<i64> = (0..=U).collect();
        for mask in 0u32..(1u32 << vals.len()) {
            let v: Vec<i64> = vals
                .iter()
                .enumerate()
                .filter(|(i, _)| (mask >> i) & 1 == 1)
                .map(|(_, &x)| x)
                .collect();
            if let Some((lo, hi)) = universal
                && v.iter().any(|&x| x < lo || x > hi)
            {
                continue;
            }
            let count_in = |lo: i64, hi: i64| v.iter().filter(|&&x| x >= lo && x <= hi).count();
            if mins
                .iter()
                .all(|&(lo, hi, k)| count_in(lo, hi) >= k as usize)
                && maxs
                    .iter()
                    .all(|&(lo, hi, m)| count_in(lo, hi) <= m as usize)
            {
                return true;
            }
        }
        false
    }

    fn ivl() -> impl Strategy<Value = (i64, i64)> {
        (0i64..=U, 0i64..=U).prop_map(|(a, b)| (a.min(b), a.max(b)))
    }
    fn card() -> impl Strategy<Value = (i64, i64, u32)> {
        (ivl(), 0u32..4).prop_map(|((lo, hi), n)| (lo, hi, n))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]
        #[test]
        fn no_false_unsat(
            universal in proptest::option::of(ivl()),
            mins in proptest::collection::vec(card(), 0..4),
            maxs in proptest::collection::vec(card(), 0..4),
        ) {
            let to_card = |v: &[(i64, i64, u32)]| -> Vec<Card<IntInterval>> {
                v.iter().map(|&(lo, hi, n)| Card::new(IntInterval::closed(lo, hi), n)).collect()
            };
            let u = universal.map(|(lo, hi)| IntInterval::closed(lo, hi));
            let verdict = integer_sat(u, &to_card(&mins), &to_card(&maxs));
            if verdict == CardSat::Unsat {
                // SOUNDNESS: a reported Unsat MUST be genuinely infeasible.
                prop_assert!(
                    !brute_feasible(universal, &mins, &maxs),
                    "FALSE UNSAT: integer_sat said Unsat but a model exists. \
                     u={universal:?} mins={mins:?} maxs={maxs:?}"
                );
            }
        }
    }
}
