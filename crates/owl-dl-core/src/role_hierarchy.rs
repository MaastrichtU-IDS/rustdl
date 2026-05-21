//! Named role hierarchy with reflexive-transitive closure.
//!
//! Captures `R ⊑ S` axioms (and equivalences `R ≡ S`, expressed as two
//! sub-role axioms in both directions). The closure is precomputed once at
//! [`RoleHierarchyBuilder::build`] time.
//!
//! Complex role hierarchies (`R₁ ∘ ... ∘ Rₙ ⊑ S`) land in Phase 5 via finite
//! state automata over role names; that machinery lives in a separate module.

use std::collections::VecDeque;

use smallvec::SmallVec;

use crate::ir::RoleId;

/// Mutable accumulator for sub-role axioms. Build it once, then call
/// [`Self::build`] to produce the immutable closed [`RoleHierarchy`].
#[derive(Debug, Default, Clone)]
pub struct RoleHierarchyBuilder {
    /// `direct_super[r.index()]` holds the roles directly above `r`.
    direct_super: Vec<SmallVec<[RoleId; 4]>>,
}

impl RoleHierarchyBuilder {
    /// Create an empty builder for `n` named roles (ids `0 ..= n-1`).
    #[must_use]
    pub fn with_roles(n: u32) -> Self {
        Self {
            direct_super: (0..n as usize).map(|_| SmallVec::new()).collect(),
        }
    }

    /// Record the axiom `sub ⊑ sup`. Duplicates are idempotent.
    ///
    /// # Panics
    /// Panics if either `sub` or `sup` is out of range for this builder.
    pub fn add_sub_role(&mut self, sub: RoleId, sup: RoleId) {
        let s = sub.index() as usize;
        let supers = &mut self.direct_super[s];
        if !supers.contains(&sup) {
            supers.push(sup);
        }
    }

    #[must_use]
    pub fn num_roles(&self) -> usize {
        self.direct_super.len()
    }

    /// Compute the reflexive-transitive closure and freeze.
    ///
    /// # Panics
    /// Panics if the builder holds more than `u32::MAX` roles.
    #[must_use]
    pub fn build(self) -> RoleHierarchy {
        let n_u32: u32 =
            u32::try_from(self.direct_super.len()).expect("RoleHierarchyBuilder: too many roles");
        let n = self.direct_super.len();
        let mut super_closure: Vec<Box<[RoleId]>> = Vec::with_capacity(n);
        let mut sub_closure: Vec<Vec<RoleId>> = vec![Vec::new(); n];

        for r in 0..n_u32 {
            let mut visited = vec![false; n];
            let mut queue: VecDeque<u32> = VecDeque::new();
            queue.push_back(r);
            let mut ups: Vec<RoleId> = Vec::new();
            while let Some(curr) = queue.pop_front() {
                let curr_idx = curr as usize;
                if visited[curr_idx] {
                    continue;
                }
                visited[curr_idx] = true;
                ups.push(RoleId::new(curr));
                for &sup in &self.direct_super[curr_idx] {
                    queue.push_back(sup.index());
                }
            }
            ups.sort_unstable();
            for &sup in &ups {
                sub_closure[sup.index() as usize].push(RoleId::new(r));
            }
            super_closure.push(ups.into_boxed_slice());
        }

        let sub_closure: Vec<Box<[RoleId]>> = sub_closure
            .into_iter()
            .map(|mut v| {
                v.sort_unstable();
                v.into_boxed_slice()
            })
            .collect();

        RoleHierarchy {
            super_closure,
            sub_closure,
        }
    }
}

/// Immutable closed role hierarchy. Both `sub` and `super` closures are
/// reflexive (every role is its own sub and its own super) and transitively
/// closed. Returned slices are sorted ascending by [`RoleId`].
#[derive(Debug, Clone)]
pub struct RoleHierarchy {
    super_closure: Vec<Box<[RoleId]>>,
    sub_closure: Vec<Box<[RoleId]>>,
}

impl RoleHierarchy {
    /// All roles `s` such that `r ⊑ s`, including `r` itself. Sorted ascending.
    ///
    /// # Panics
    /// Panics if `r` is out of range.
    #[must_use]
    pub fn super_roles(&self, r: RoleId) -> &[RoleId] {
        &self.super_closure[r.index() as usize]
    }

    /// All roles `s` such that `s ⊑ r`, including `r` itself. Sorted ascending.
    ///
    /// # Panics
    /// Panics if `r` is out of range.
    #[must_use]
    pub fn sub_roles(&self, r: RoleId) -> &[RoleId] {
        &self.sub_closure[r.index() as usize]
    }

    /// Returns `true` iff `sub ⊑ sup` (reflexive, transitive).
    ///
    /// # Panics
    /// Panics if either id is out of range.
    #[must_use]
    pub fn is_sub_role(&self, sub: RoleId, sup: RoleId) -> bool {
        self.super_closure[sub.index() as usize]
            .binary_search(&sup)
            .is_ok()
    }

    #[must_use]
    pub fn num_roles(&self) -> usize {
        self.super_closure.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: u32) -> RoleId {
        RoleId::new(n)
    }

    #[test]
    fn reflexive_only_when_no_axioms() {
        let h = RoleHierarchyBuilder::with_roles(3).build();
        for i in 0..3 {
            assert_eq!(h.super_roles(r(i)), [r(i)].as_slice());
            assert_eq!(h.sub_roles(r(i)), [r(i)].as_slice());
            assert!(h.is_sub_role(r(i), r(i)));
        }
    }

    #[test]
    fn linear_chain() {
        // 0 ⊑ 1 ⊑ 2
        let mut b = RoleHierarchyBuilder::with_roles(3);
        b.add_sub_role(r(0), r(1));
        b.add_sub_role(r(1), r(2));
        let h = b.build();
        assert_eq!(h.super_roles(r(0)), [r(0), r(1), r(2)].as_slice());
        assert_eq!(h.super_roles(r(1)), [r(1), r(2)].as_slice());
        assert_eq!(h.super_roles(r(2)), [r(2)].as_slice());
        assert_eq!(h.sub_roles(r(2)), [r(0), r(1), r(2)].as_slice());
        assert!(h.is_sub_role(r(0), r(2)));
        assert!(!h.is_sub_role(r(2), r(0)));
    }

    #[test]
    fn diamond() {
        // 0 ⊑ 1, 0 ⊑ 2, 1 ⊑ 3, 2 ⊑ 3
        let mut b = RoleHierarchyBuilder::with_roles(4);
        b.add_sub_role(r(0), r(1));
        b.add_sub_role(r(0), r(2));
        b.add_sub_role(r(1), r(3));
        b.add_sub_role(r(2), r(3));
        let h = b.build();
        assert!(h.is_sub_role(r(0), r(3)));
        assert!(h.is_sub_role(r(0), r(1)));
        assert!(h.is_sub_role(r(0), r(2)));
        assert!(!h.is_sub_role(r(1), r(2)));
        assert!(!h.is_sub_role(r(2), r(1)));
        assert_eq!(h.super_roles(r(0)), [r(0), r(1), r(2), r(3)].as_slice());
    }

    #[test]
    fn equivalence_cycle() {
        // 0 ⊑ 1, 1 ⊑ 0  ⇒  0 ≡ 1
        let mut b = RoleHierarchyBuilder::with_roles(2);
        b.add_sub_role(r(0), r(1));
        b.add_sub_role(r(1), r(0));
        let h = b.build();
        assert_eq!(h.super_roles(r(0)), [r(0), r(1)].as_slice());
        assert_eq!(h.super_roles(r(1)), [r(0), r(1)].as_slice());
        assert!(h.is_sub_role(r(0), r(1)));
        assert!(h.is_sub_role(r(1), r(0)));
    }

    #[test]
    fn disconnected_components() {
        // 0 ⊑ 1, 2 ⊑ 3 — separate components
        let mut b = RoleHierarchyBuilder::with_roles(4);
        b.add_sub_role(r(0), r(1));
        b.add_sub_role(r(2), r(3));
        let h = b.build();
        assert!(h.is_sub_role(r(0), r(1)));
        assert!(h.is_sub_role(r(2), r(3)));
        assert!(!h.is_sub_role(r(0), r(2)));
        assert!(!h.is_sub_role(r(0), r(3)));
        assert!(!h.is_sub_role(r(1), r(2)));
    }

    #[test]
    fn duplicate_axioms_are_idempotent() {
        let mut b = RoleHierarchyBuilder::with_roles(2);
        b.add_sub_role(r(0), r(1));
        b.add_sub_role(r(0), r(1));
        b.add_sub_role(r(0), r(1));
        let h = b.build();
        assert_eq!(h.super_roles(r(0)), [r(0), r(1)].as_slice());
    }
}
