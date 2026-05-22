//! Helpers for [`crate::DepSet`] arithmetic.
//!
//! Phase 4 dependency-directed back-jumping needs two operations
//! everywhere a rule fires:
//!
//! 1. **Union** — when a conclusion is derived from two antecedents
//!    (an `∀R.C` label *and* an R-edge, say), the conclusion's deps
//!    are the union of both antecedents' deps.
//! 2. **Membership** — `branch()` checks whether a clash deps set
//!    contains its own `branch_id` to decide whether to keep trying
//!    options or jump back.
//!
//! [`crate::DepSet`] is a sorted+dedup'd `Vec<u32>` so both ops are
//! cheap: union is two-pointer merge in O(|a| + |b|), membership is
//! O(log n).

use crate::graph::DepSet;

/// Union of two sorted+dedup'd [`DepSet`]s into a fresh sorted+dedup'd
/// vector. O(|a| + |b|).
#[must_use]
pub(crate) fn union(a: &[u32], b: &[u32]) -> DepSet {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_empty_inputs() {
        assert!(union(&[], &[]).is_empty());
        assert_eq!(union(&[1, 2], &[]), vec![1, 2]);
        assert_eq!(union(&[], &[3, 4]), vec![3, 4]);
    }

    #[test]
    fn union_dedups_overlap() {
        assert_eq!(union(&[1, 2, 3], &[2, 3, 4]), vec![1, 2, 3, 4]);
    }

    #[test]
    fn union_preserves_sort() {
        assert_eq!(union(&[1, 5, 9], &[2, 4, 7]), vec![1, 2, 4, 5, 7, 9]);
    }
}
