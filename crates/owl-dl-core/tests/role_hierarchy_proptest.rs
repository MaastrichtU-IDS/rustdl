//! Property test: the BFS-based [`RoleHierarchy`] closure must agree with a
//! naive Warshall reference on arbitrary sub-role axiom sets.

// Warshall is intrinsically index-based; the iterator rewrite would be less
// readable and the borrow checker doesn't permit the obvious nested iter_mut.
#![allow(clippy::needless_range_loop)]

use owl_dl_core::ir::RoleId;
use owl_dl_core::role_hierarchy::RoleHierarchyBuilder;
use proptest::prelude::*;

proptest! {
    #[test]
    fn closure_matches_warshall(
        n in 1u32..8,
        edges in prop::collection::vec((0u32..8, 0u32..8), 0..24),
    ) {
        let n_usize = n as usize;
        let mut builder = RoleHierarchyBuilder::with_roles(n);
        let valid: Vec<(u32, u32)> = edges
            .into_iter()
            .filter(|(s, p)| *s < n && *p < n)
            .collect();
        for &(s, p) in &valid {
            builder.add_sub_role(RoleId::new(s), RoleId::new(p));
        }
        let h = builder.build();

        // Reference: Warshall on a dense reachability matrix.
        let mut reach = vec![vec![false; n_usize]; n_usize];
        for i in 0..n_usize {
            reach[i][i] = true;
        }
        for &(s, p) in &valid {
            reach[s as usize][p as usize] = true;
        }
        for k in 0..n_usize {
            for i in 0..n_usize {
                if reach[i][k] {
                    for j in 0..n_usize {
                        if reach[k][j] {
                            reach[i][j] = true;
                        }
                    }
                }
            }
        }

        for i in 0..n {
            for j in 0..n {
                let expected = reach[i as usize][j as usize];
                let actual = h.is_sub_role(RoleId::new(i), RoleId::new(j));
                prop_assert_eq!(expected, actual, "mismatch at ({}, {})", i, j);
            }
        }
    }
}
