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

use crate::ir::{Role, RoleId};

/// Index of a SIGNED role (`Role`) in the signed closure: `2·id + polarity`.
#[inline]
fn sidx(r: Role) -> usize {
    r.role_id().index() as usize * 2 + usize::from(r.is_inverse())
}

/// Inverse of [`sidx`].
#[inline]
fn srole(i: usize) -> Role {
    let id = RoleId::new(u32::try_from(i / 2).expect("signed index fits"));
    if i % 2 == 1 {
        Role::inverse(id)
    } else {
        Role::named(id)
    }
}

/// Mutable accumulator for sub-role axioms. Build it once, then call
/// [`Self::build`] to produce the immutable closed [`RoleHierarchy`].
#[derive(Debug, Default, Clone)]
pub struct RoleHierarchyBuilder {
    /// `direct_super[r.index()]` holds the roles directly above `r`.
    direct_super: Vec<SmallVec<[RoleId; 4]>>,
    /// `symmetric[r.index()]` is `true` iff `r` was declared symmetric.
    symmetric: Vec<bool>,
    /// SIGNED direct supers (#177): `signed_direct_super[sidx(r)]` holds the
    /// signed roles directly above the signed role `r`. Unlike the id-space
    /// table above, this one can express polarity-CROSSING inclusions —
    /// `subsetOf⁻ ⊑ setRelation` from `Inverse(subsetOf, supersetOf)` +
    /// `supersetOf ⊑ setRelation` — which the id table structurally cannot.
    signed_direct_super: Vec<SmallVec<[u32; 4]>>,
}

impl RoleHierarchyBuilder {
    /// Create an empty builder for `n` named roles (ids `0 ..= n-1`).
    #[must_use]
    pub fn with_roles(n: u32) -> Self {
        Self {
            direct_super: (0..n as usize).map(|_| SmallVec::new()).collect(),
            symmetric: vec![false; n as usize],
            signed_direct_super: (0..n as usize * 2).map(|_| SmallVec::new()).collect(),
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

    /// Record that `role` is symmetric (`role ≡ role⁻`).
    ///
    /// # Panics
    /// Panics if `role` is out of range for this builder.
    pub fn mark_symmetric(&mut self, role: RoleId) {
        self.symmetric[role.index() as usize] = true;
        self.push_signed(Role::named(role), Role::inverse(role));
        self.push_signed(Role::inverse(role), Role::named(role));
    }

    fn push_signed(&mut self, sub: Role, sup: Role) {
        let i = sidx(sub);
        let s = u32::try_from(sidx(sup)).expect("signed index fits in u32");
        if !self.signed_direct_super[i].contains(&s) {
            self.signed_direct_super[i].push(s);
        }
    }

    /// Record the SIGNED inclusion `sub ⊑ sup` (#177) plus its mirror
    /// `sub⁻ ⊑ sup⁻`, which is a logical consequence (`R ⊑ S ⟹ R⁻ ⊑ S⁻`).
    ///
    /// # Panics
    /// Panics if either role's id is out of range for this builder.
    pub fn add_signed_sub_role(&mut self, sub: Role, sup: Role) {
        self.push_signed(sub, sup);
        self.push_signed(sub.flip(), sup.flip());
    }

    /// Record `InverseObjectProperties(a, b)` — `a ≡ b⁻` — as four signed
    /// inclusions (#177): `a ⊑ b⁻`, `b⁻ ⊑ a`, `a⁻ ⊑ b`, `b ⊑ a⁻`. This is
    /// what makes polarity-CROSSING chains representable: with `r ⊑ s`,
    /// `Inverse(s, t)` and `t ⊑ u`, the closure composes
    /// `r⁻ ⊑ s⁻ ⊑ t ⊑ u`, i.e. an `r`-edge read backwards is a `u`-edge —
    /// the SWEET `Range(setRelation)` mechanism that owns 49% of the
    /// remaining ORE missed-entailment mass.
    ///
    /// # Panics
    /// Panics if either role's id is out of range for this builder.
    pub fn add_inverse_pair(&mut self, a: Role, b: Role) {
        // a ≡ b⁻:
        self.push_signed(a, b.flip());
        self.push_signed(b.flip(), a);
        self.push_signed(a.flip(), b);
        self.push_signed(b, a.flip());
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

        // `visited`/`queue` are hoisted and reused, as in `told::build_told_tables`.
        // This loop had the identical O(n^2)-memset shape (`vec![false; n]` zeroed n
        // times); reuse is exact because `ups` receives a role exactly when that role
        // is marked visited, so clearing the `ups` entries restores the buffer in
        // O(|ups|).
        //
        // PRECAUTIONARY, with NO measured corpus win: n here is the ROLE count, whose
        // ORE maximum is 11,312 (median 24, mean 98) — about 128 MB of zeroing, ~13 ms.
        // Applied because it is the same defect class as the told-table quadratic (which
        // WAS worth 6.2x) and the transformation is output-identical, not because a
        // measurement asked for it. It would bite an ontology with ~100k roles.
        let mut visited = vec![false; n];
        let mut queue: VecDeque<u32> = VecDeque::new();

        for r in 0..n_u32 {
            queue.clear();
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
            // Restore the shared buffer before `ups` is moved into `super_closure`.
            for &u in &ups {
                visited[u.index() as usize] = false;
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

        // SIGNED closure (#177): same reflexive-transitive BFS over the signed
        // graph (2n nodes). Reuses the visited-buffer discipline above; the
        // graph is twice as large but the same O(n·edges) shape.
        let sn = self.signed_direct_super.len();
        let mut signed_super_closure: Vec<Box<[u32]>> = Vec::with_capacity(sn);
        let mut signed_sub_closure: Vec<Vec<u32>> = vec![Vec::new(); sn];
        let mut svisited = vec![false; sn];
        let mut squeue: VecDeque<u32> = VecDeque::new();
        let mut polarity_crossing = false;
        for r in 0..sn {
            squeue.clear();
            squeue.push_back(u32::try_from(r).expect("signed index fits"));
            let mut ups: Vec<u32> = Vec::new();
            while let Some(curr) = squeue.pop_front() {
                let ci = curr as usize;
                if svisited[ci] {
                    continue;
                }
                svisited[ci] = true;
                ups.push(curr);
                for &sup in &self.signed_direct_super[ci] {
                    squeue.push_back(sup);
                }
            }
            for &u in &ups {
                svisited[u as usize] = false;
            }
            ups.sort_unstable();
            for &sup in &ups {
                // A closure edge between DIFFERENT ids at DIFFERENT polarities is
                // what the id-space hierarchy cannot express; its presence is what
                // obliges target-side trigger dispatch on NAMED edges (see the
                // `pos_first_target_trigger` gate in `hyper.rs`).
                if sup as usize / 2 != r / 2 && (sup as usize % 2) != (r % 2) {
                    polarity_crossing = true;
                }
                signed_sub_closure[sup as usize].push(u32::try_from(r).expect("fits"));
            }
            signed_super_closure.push(ups.into_boxed_slice());
        }
        let signed_sub_closure: Vec<Box<[u32]>> = signed_sub_closure
            .into_iter()
            .map(|mut v| {
                v.sort_unstable();
                v.into_boxed_slice()
            })
            .collect();

        RoleHierarchy {
            super_closure,
            sub_closure,
            symmetric: self.symmetric.into_boxed_slice(),
            signed_super_closure,
            signed_sub_closure,
            polarity_crossing,
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
    /// `symmetric[r.index()]` is `true` iff `r` was declared symmetric.
    symmetric: Box<[bool]>,
    /// SIGNED reflexive-transitive closure (#177), indexed by [`sidx`]. Can
    /// express polarity-crossing inclusions the id-space closure cannot.
    signed_super_closure: Vec<Box<[u32]>>,
    signed_sub_closure: Vec<Box<[u32]>>,
    /// True iff some closure inclusion connects DIFFERENT ids at DIFFERENT
    /// polarities — the condition under which a NAMED edge, read backwards,
    /// can satisfy a NAMED body atom (the SWEET mechanism, #177).
    polarity_crossing: bool,
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

    /// Returns `true` iff `role` was declared symmetric (`role ≡ role⁻`),
    /// directly via `SymmetricObjectProperty` or via self-inverse
    /// `InverseObjectProperties(role, role)`.
    ///
    /// # Panics
    /// Panics if `role` is out of range.
    #[must_use]
    pub fn is_symmetric(&self, role: RoleId) -> bool {
        self.symmetric[role.index() as usize]
    }

    /// SIGNED inclusion (#177): `sub ⊑ sup` over signed roles, reflexive and
    /// transitively closed, including polarity-crossing inclusions mediated by
    /// declared inverse pairs and symmetry.
    ///
    /// # Panics
    /// Panics if either role's id is out of range.
    #[must_use]
    pub fn is_signed_sub(&self, sub: Role, sup: Role) -> bool {
        self.signed_super_closure[sidx(sub)]
            .binary_search(&u32::try_from(sidx(sup)).expect("signed index fits"))
            .is_ok()
    }

    /// All signed roles `s` with `s ⊑ sup` (reflexive, closed). Sorted by
    /// signed index.
    ///
    /// # Panics
    /// Panics if `sup`'s id is out of range.
    pub fn signed_sub_roles(&self, sup: Role) -> impl Iterator<Item = Role> + '_ {
        self.signed_sub_closure[sidx(sup)]
            .iter()
            .map(|&i| srole(i as usize))
    }

    /// True iff the closure contains an inclusion between DIFFERENT ids at
    /// DIFFERENT polarities — when false, a named edge read backwards can only
    /// match via the symmetric flag, and the cheaper pre-#177 trigger gating
    /// remains sufficient.
    #[must_use]
    pub fn has_polarity_crossing(&self) -> bool {
        self.polarity_crossing
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

    #[test]
    fn symmetric_set_roundtrips() {
        let mut b = RoleHierarchyBuilder::with_roles(3);
        b.mark_symmetric(r(1));
        let h = b.build();
        assert!(!h.is_symmetric(r(0)));
        assert!(h.is_symmetric(r(1)));
        assert!(!h.is_symmetric(r(2)));
    }
}
