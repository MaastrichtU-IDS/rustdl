//! Concrete-domain satisfiability — datatype reasoners for rustdl.
//!
//! See `docs/superpowers/specs/2026-06-11-concrete-domain-solver-design.md`.
//!
//! **P0: integer concrete domain.** [`integer_sat`] decides whether a set of
//! distinct integers can satisfy a node's data constraints on one property:
//! *min-demands* (`≥n p.R`, `∃p.R`, `DataHasValue` — at least `n` distinct
//! values in `R`), *max-limits* (`≤m p.S` — at most `m` distinct values in
//! `S`), under a *universal filter* (`∀p.U` — every value in `U`).
//!
//! **Load-bearing invariant — REFUTE-ONLY.** This module may only ever justify
//! turning a tableau node UNSAT; it must never license SAT/pruning. Hence
//! [`integer_sat`] returns [`CardSat::Unsat`] *only when provably infeasible*
//! and [`CardSat::Sat`] for everything else (including patterns it does not yet
//! decide). A spurious `Unsat` would be a false clash ⇒ false subsumption (the
//! FP-critical direction); a spurious `Sat` is merely a missed clash
//! (incomplete, never unsound). P0 implements a SOUND subset of the integer
//! feasibility decision (the counting + conflict + disjoint-packing clashes);
//! full interval-feasibility is a later refinement and only adds `Unsat`s.

#![forbid(unsafe_code)]

/// A closed integer interval `[min, max]` (inclusive). `None` = unbounded on
/// that side, so `IntInterval { min: None, max: None }` is all of `xsd:integer`.
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

    /// Empty iff `min > max` (both bounded).
    #[must_use]
    pub fn is_empty(self) -> bool {
        matches!((self.min, self.max), (Some(lo), Some(hi)) if lo > hi)
    }

    /// Intersection (closed-form). May be empty.
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
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

    /// Whether `self ⊆ other`.
    #[must_use]
    pub fn subset_of(self, other: Self) -> bool {
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

    /// Number of distinct integers in the interval. `None` = infinite
    /// (unbounded on at least one side); `Some(0)` = empty.
    #[must_use]
    pub fn count(self) -> Option<u128> {
        match (self.min, self.max) {
            (Some(lo), Some(hi)) if lo <= hi => {
                // hi - lo + 1, computed in i128 to avoid i64 overflow.
                #[allow(clippy::cast_sign_loss)]
                Some((i128::from(hi) - i128::from(lo) + 1) as u128)
            }
            (Some(_), Some(_)) => Some(0), // lo > hi ⇒ empty
            _ => None,                     // an unbounded side ⇒ infinitely many
        }
    }

    /// Whether two intervals are disjoint (share no integer).
    #[must_use]
    pub fn disjoint(self, other: Self) -> bool {
        self.intersect(other).is_empty()
    }
}

/// A cardinality demand or limit: `n` (distinct) values within `interval`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    /// The qualifying range.
    pub interval: IntInterval,
    /// The bound `n` (a `≥n` for a demand, a `≤m` for a limit).
    pub n: u32,
}

impl Card {
    /// Convenience constructor.
    #[must_use]
    pub const fn new(interval: IntInterval, n: u32) -> Self {
        Self { interval, n }
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

/// Decide whether distinct integers can satisfy the constraints on one data
/// property at one node. **Refute-only**: returns [`CardSat::Unsat`] only when
/// provably infeasible, [`CardSat::Sat`] otherwise.
///
/// - `universal`: the intersection of all `∀p.U` ranges (`None` = no `∀`, i.e.
///   the whole line). Every chosen value must lie in it; demands/limits are
///   evaluated after intersecting with it.
/// - `min_demands`: `≥n` requirements (`∃` ⇒ `n=1`).
/// - `max_limits`: `≤m` limits.
///
/// P0 soundly detects: empty universal under a positive demand; a demand whose
/// (universal-restricted) range holds fewer than `n` integers (the counting
/// clash, e.g. `≥3` over `[0,1]`); a `≥n`-vs-`≤m` subset conflict (`n>m`); and a
/// set of pairwise-disjoint demands inside one limit whose `n`-sum exceeds it.
#[must_use]
pub fn integer_sat(
    universal: Option<IntInterval>,
    min_demands: &[Card],
    max_limits: &[Card],
) -> CardSat {
    let u = universal.unwrap_or_else(IntInterval::all);

    // Restrict every range to the universal filter (all fillers ∈ U).
    let mins: Vec<Card> = min_demands
        .iter()
        .filter(|d| d.n > 0)
        .map(|d| Card::new(d.interval.intersect(u), d.n))
        .collect();
    let maxs: Vec<Card> = max_limits
        .iter()
        .map(|l| Card::new(l.interval.intersect(u), l.n))
        .collect();

    // (a) Empty universal: any positive demand is unsatisfiable.
    if u.is_empty() {
        return if mins.is_empty() {
            CardSat::Sat
        } else {
            CardSat::Unsat
        };
    }

    // (b) Per-demand capacity: a `≥n` over a range with < n distinct integers
    // (incl. empty) cannot be met — the integer-counting clash.
    for d in &mins {
        if matches!(d.interval.count(), Some(cap) if cap < u128::from(d.n)) {
            return CardSat::Unsat;
        }
    }

    // (c) Direct `≥n` vs `≤m` subset conflict: a demand inside a limit with n>m.
    for d in &mins {
        for l in &maxs {
            if d.interval.subset_of(l.interval) && d.n > l.n {
                return CardSat::Unsat;
            }
        }
    }

    // (d) Disjoint-packing: among demands whose range is inside one limit's
    // range, any pairwise-disjoint subset needs distinct values, so its n-sum
    // must fit under the limit. Greedy by interval end (sound: any disjoint
    // subset whose sum exceeds m proves infeasibility).
    for l in &maxs {
        let mut inside: Vec<Card> = mins
            .iter()
            .copied()
            .filter(|d| d.interval.subset_of(l.interval))
            .collect();
        inside.sort_by_key(|d| d.interval.max.unwrap_or(i64::MAX));
        let mut last_end: Option<i64> = None;
        let mut sum: u64 = 0;
        for d in inside {
            let starts_after = match (last_end, d.interval.min) {
                (None, _) => true,
                (Some(e), Some(s)) => s > e,
                (Some(_), None) => false, // unbounded-below overlaps previous
            };
            if starts_after {
                sum += u64::from(d.n);
                last_end = d.interval.max;
                if sum > u64::from(l.n) {
                    return CardSat::Unsat;
                }
            }
        }
    }

    CardSat::Sat
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
        assert_eq!(cl(0, 1).count(), Some(2));
        assert_eq!(cl(5, 5).count(), Some(1));
        assert_eq!(ALL.count(), None);
        assert_eq!(
            IntInterval {
                min: Some(0),
                max: None
            }
            .count(),
            None
        );
        assert!(cl(3, 2).is_empty());
        assert_eq!(cl(3, 2).count(), Some(0));
        assert_eq!(
            cl(i64::MIN, i64::MAX).count(),
            Some(u128::from(u64::MAX) + 1)
        );
    }

    #[test]
    fn subset_and_intersect() {
        assert!(cl(2, 4).subset_of(cl(0, 10)));
        assert!(!cl(0, 10).subset_of(cl(2, 4)));
        assert!(cl(0, 10).subset_of(ALL));
        assert!(!ALL.subset_of(cl(0, 10)));
        assert_eq!(cl(0, 10).intersect(cl(5, 20)), cl(5, 10));
        assert!(cl(0, 3).intersect(cl(5, 9)).is_empty());
        assert!(cl(0, 3).disjoint(cl(5, 9)));
        assert!(!cl(0, 5).disjoint(cl(5, 9))); // share 5
    }

    // ── POSITIVES: provably unsat (sound clashes) ───────────────────
    #[test]
    fn counting_clash_more_demanded_than_integers_exist() {
        // ≥3 over [0,1] — only 2 integers. UNSAT.
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 1), 3)], &[]),
            CardSat::Unsat
        );
    }

    #[test]
    fn subset_conflict_min_exceeds_max() {
        // ≥3 over [0,9] and ≤2 over [0,100]: [0,9] ⊆ [0,100], 3 > 2. UNSAT.
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 9), 3)], &[Card::new(cl(0, 100), 2)]),
            CardSat::Unsat
        );
    }

    #[test]
    fn empty_universal_with_demand_is_unsat() {
        assert_eq!(
            integer_sat(Some(cl(5, 3)), &[Card::new(ALL, 1)], &[]),
            CardSat::Unsat
        );
    }

    #[test]
    fn universal_shrinks_demand_below_capacity() {
        // ∀p.[0,1], ≥3 p.⊤ ⟹ effective ≥3 over [0,1] = 2 ints. UNSAT.
        assert_eq!(
            integer_sat(Some(cl(0, 1)), &[Card::new(ALL, 3)], &[]),
            CardSat::Unsat
        );
    }

    #[test]
    fn disjoint_packing_exceeds_limit() {
        // ≥2 over [0,1] and ≥2 over [10,11] (disjoint), ≤3 over [0,100].
        // Need 4 distinct values in [0,100], limit 3. UNSAT.
        assert_eq!(
            integer_sat(
                None,
                &[Card::new(cl(0, 1), 2), Card::new(cl(10, 11), 2)],
                &[Card::new(cl(0, 100), 3)]
            ),
            CardSat::Unsat
        );
    }

    // ── NEGATIVES-FIRST: satisfiable-but-tight MUST stay SAT (FP gate) ─
    #[test]
    fn exactly_enough_integers_is_sat() {
        // ≥2 over [0,1] — exactly 2 ints. SAT (tight).
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 1), 2)], &[]),
            CardSat::Sat
        );
    }

    #[test]
    fn min_equals_max_is_sat() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 9), 2)], &[Card::new(cl(0, 9), 2)]),
            CardSat::Sat
        );
    }

    #[test]
    fn room_under_limit_is_sat() {
        assert_eq!(
            integer_sat(None, &[Card::new(cl(0, 10), 2)], &[Card::new(cl(0, 10), 5)]),
            CardSat::Sat
        );
    }

    #[test]
    fn overlapping_demands_do_not_sum_against_limit() {
        // Two ≥2 demands over the SAME [0,10]: one set of 2 values satisfies
        // both. ≤2 over [0,10]. SAT — must NOT sum to 4.
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
    fn unbounded_range_has_infinite_capacity() {
        assert_eq!(
            integer_sat(None, &[Card::new(ALL, 1000)], &[]),
            CardSat::Sat
        );
    }

    #[test]
    fn limit_on_disjoint_range_does_not_constrain() {
        // ≥3 over [0,9]; ≤1 over [100,200] (disjoint). Limit irrelevant. SAT.
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
    fn no_constraints_is_sat() {
        assert_eq!(integer_sat(None, &[], &[]), CardSat::Sat);
        assert_eq!(
            integer_sat(None, &[], &[Card::new(cl(0, 1), 0)]),
            CardSat::Sat
        );
    }

    #[test]
    fn zero_demand_is_sat() {
        // ≥0 is vacuous even over an empty range.
        assert_eq!(
            integer_sat(Some(cl(5, 3)), &[Card::new(ALL, 0)], &[]),
            CardSat::Sat
        );
    }
}
