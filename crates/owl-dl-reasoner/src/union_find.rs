//! Disjoint-set union (union-find) over `u32` indices.
//!
//! Used by the `ABox` consistency check (`abox_check.rs`) to track
//! merge-equivalence classes induced by `SameIndividual` axioms and
//! `FunctionalObjectProperty` / `InverseFunctionalObjectProperty`
//! inferences. The keys are indices into `Abox::individuals`, not
//! `IndividualId`s — the caller maintains the index map.
//!
//! Path compression on `find`; union by rank on `union`.

#[derive(Debug, Clone)]
pub(crate) struct UnionFind {
    parent: Vec<u32>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub(crate) fn new(n: usize) -> Self {
        let parent = (0..u32::try_from(n).expect("n fits in u32")).collect();
        let rank = vec![0u8; n];
        Self { parent, rank }
    }

    pub(crate) fn find(&mut self, x: u32) -> u32 {
        let mut root = x;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        // Path compression: point every node on the path directly at root.
        let mut cur = x;
        while self.parent[cur as usize] != root {
            let next = self.parent[cur as usize];
            self.parent[cur as usize] = root;
            cur = next;
        }
        root
    }

    /// Returns `true` iff the two elements were in distinct classes
    /// (i.e., a merge actually happened).
    pub(crate) fn union(&mut self, a: u32, b: u32) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }
        let (ra_idx, rb_idx) = (ra as usize, rb as usize);
        match self.rank[ra_idx].cmp(&self.rank[rb_idx]) {
            std::cmp::Ordering::Less => self.parent[ra_idx] = rb,
            std::cmp::Ordering::Greater => self.parent[rb_idx] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb_idx] = ra;
                self.rank[ra_idx] += 1;
            }
        }
        true
    }

    pub(crate) fn same(&mut self, a: u32, b: u32) -> bool {
        self.find(a) == self.find(b)
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn singletons_are_distinct() {
        let mut uf = UnionFind::new(3);
        assert!(!uf.same(0, 1));
        assert!(!uf.same(1, 2));
    }

    #[test]
    fn union_merges_components() {
        let mut uf = UnionFind::new(4);
        assert!(uf.union(0, 1));
        assert!(uf.same(0, 1));
        assert!(uf.union(2, 3));
        assert!(!uf.same(0, 2));
        assert!(uf.union(1, 3));
        assert!(uf.same(0, 3));
    }

    #[test]
    fn redundant_union_returns_false() {
        let mut uf = UnionFind::new(2);
        assert!(uf.union(0, 1));
        assert!(!uf.union(0, 1));
        assert!(!uf.union(1, 0));
    }

    #[test]
    fn path_compression_after_find() {
        // Force a 3-deep chain via union order, then check find collapses it.
        let mut uf = UnionFind::new(4);
        uf.union(0, 1);
        uf.union(2, 3);
        uf.union(0, 2);
        let root = uf.find(3);
        // After find(3), parent[3] should point straight at root.
        assert_eq!(uf.parent[3], root);
    }
}
