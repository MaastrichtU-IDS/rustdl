//! Told-subsumer and told-disjoint tables.
//!
//! "Told" = explicitly stated in axioms, before any reasoning. We extract
//! the *trivial* subsumption graph between named classes — atomic-to-atomic
//! relationships only — and the pairwise disjointness graph. Complex shapes
//! (`And ⊑ B`, `Some(R, C) ⊑ D`, ...) are out of scope here; absorption and
//! the tableau handle those.
//!
//! Sources of told relationships:
//!
//! | source axiom                                       | told fact         |
//! |----------------------------------------------------|-------------------|
//! | `SubClassOf(Atomic(A), Atomic(B))`                 | A ⊑ B             |
//! | `SubClassOf(Atomic(A), Not(Atomic(B)))`            | A, B disjoint     |
//! | `EquivalentClasses([…, Atomic(A), …, Atomic(B), …])`| A ⊑ B and B ⊑ A   |
//! | `DisjointClasses([…, Atomic(A), …, Atomic(B), …])` | A, B disjoint     |
//! | `DisjointUnion { class: P, members: […, Atomic(Ci), …] }` | Ci ⊑ P + pairs disjoint |
//!
//! The subsumption table is closed (reflexive + transitive) at build time.
//! Disjointness is symmetric but not transitive, so we leave it as a flat
//! pairwise set.

use std::collections::VecDeque;

use std::collections::HashSet;

use smallvec::SmallVec;

use crate::ConceptPool;
use crate::ir::{ClassId, ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};

/// Reflexive-transitive closure of told subsumption plus told-disjoint
/// pairs. Both sub-class and super-class queries return slices sorted by
/// [`ClassId`].
#[derive(Debug, Clone)]
pub struct ToldTables {
    super_closure: Vec<Box<[ClassId]>>,
    sub_closure: Vec<Box<[ClassId]>>,
    disjoint_with: Vec<Box<[ClassId]>>,
}

impl ToldTables {
    /// All named classes `B` such that the ontology told us `c ⊑ B`,
    /// including `c` itself. Sorted ascending.
    ///
    /// # Panics
    /// Panics if `c` is out of range for this table.
    #[must_use]
    pub fn super_classes(&self, c: ClassId) -> &[ClassId] {
        &self.super_closure[c.index() as usize]
    }

    /// All named classes `S` such that the ontology told us `S ⊑ c`,
    /// including `c` itself. Sorted ascending.
    ///
    /// # Panics
    /// Panics if `c` is out of range for this table.
    #[must_use]
    pub fn sub_classes(&self, c: ClassId) -> &[ClassId] {
        &self.sub_closure[c.index() as usize]
    }

    /// True iff `sub ⊑ sup` was told (reflexive, transitive).
    ///
    /// # Panics
    /// Panics if either id is out of range.
    #[must_use]
    pub fn is_told_sub(&self, sub: ClassId, sup: ClassId) -> bool {
        self.super_closure[sub.index() as usize]
            .binary_search(&sup)
            .is_ok()
    }

    /// All named classes told to be disjoint with `c`. Sorted ascending. Note
    /// `c` itself does not appear unless the ontology explicitly stated
    /// `DisjointClasses(c, c)` (a degenerate axiom).
    ///
    /// # Panics
    /// Panics if `c` is out of range.
    #[must_use]
    pub fn disjoints_of(&self, c: ClassId) -> &[ClassId] {
        &self.disjoint_with[c.index() as usize]
    }

    /// True iff `a` and `b` were told disjoint (symmetric).
    ///
    /// # Panics
    /// Panics if either id is out of range.
    #[must_use]
    pub fn are_told_disjoint(&self, a: ClassId, b: ClassId) -> bool {
        self.disjoint_with[a.index() as usize]
            .binary_search(&b)
            .is_ok()
    }

    #[must_use]
    pub fn num_classes(&self) -> usize {
        self.super_closure.len()
    }
}

/// Extract told-subsumer + told-disjoint relationships from the explicit
/// axioms of `ontology`. Only atomic-to-atomic (and `Not(Atomic)` for
/// disjointness) shapes are recognized; complex axioms are left to Phase 1
/// absorption.
#[must_use]
pub fn build_told_tables(ontology: &InternalOntology) -> ToldTables {
    let n = ontology.vocabulary.num_classes();
    let mut direct_super: Vec<SmallVec<[ClassId; 4]>> = vec![SmallVec::new(); n];
    // Disjoint sets use a per-class `HashSet` (not `SmallVec`) so membership
    // dedup during build is O(1), not O(k) linear-scan. A single large
    // `DisjointClasses`/DKey-disjointness axiom produces O(k²) pairs; with the
    // old `SmallVec::contains` that was O(k³) and stalled front-end conversion
    // (ore_ont_10425: 5261 distinct data values → ~14M disjoint pairs → >150 s
    // DNF). Output below is still a sorted `Box<[ClassId]>`, so the told table
    // is byte-identical to the previous representation.
    let mut disjoint: Vec<HashSet<ClassId>> = vec![HashSet::new(); n];

    let pool = &ontology.concepts;

    for axiom in &ontology.axioms {
        match axiom {
            Axiom::SubClassOf { sub, sup } => {
                let sub_atom = as_atomic(*sub, pool);
                if let (Some(a), Some(b)) = (sub_atom, as_atomic(*sup, pool)) {
                    add_edge(&mut direct_super, a, b);
                } else if let (Some(a), Some(b)) = (sub_atom, as_not_atomic(*sup, pool)) {
                    add_disjoint_pair(&mut disjoint, a, b);
                } else if let Some((a, b)) = as_atomic_pair_to_bot(*sub, *sup, pool) {
                    // `A ⊓ B ⊑ ⊥` — the form the negation-GCI rewrite produces,
                    // and the form users write directly. Without this arm,
                    // rewriting `A ⊑ ¬B` would silently shrink told-disjoint
                    // coverage (the tier walk and the tableau read these tables).
                    add_disjoint_pair(&mut disjoint, a, b);
                }
            }
            Axiom::EquivalentClasses(ids) => {
                let atoms: Vec<ClassId> =
                    ids.iter().filter_map(|&id| as_atomic(id, pool)).collect();
                for i in 0..atoms.len() {
                    for j in (i + 1)..atoms.len() {
                        add_edge(&mut direct_super, atoms[i], atoms[j]);
                        add_edge(&mut direct_super, atoms[j], atoms[i]);
                    }
                }
            }
            Axiom::DisjointClasses(ids) => {
                let atoms: Vec<ClassId> =
                    ids.iter().filter_map(|&id| as_atomic(id, pool)).collect();
                for i in 0..atoms.len() {
                    for j in (i + 1)..atoms.len() {
                        add_disjoint_pair(&mut disjoint, atoms[i], atoms[j]);
                    }
                }
            }
            Axiom::DisjointUnion { class, members } => {
                let atoms: Vec<ClassId> = members
                    .iter()
                    .filter_map(|&id| as_atomic(id, pool))
                    .collect();
                for &m in &atoms {
                    add_edge(&mut direct_super, m, *class);
                }
                for i in 0..atoms.len() {
                    for j in (i + 1)..atoms.len() {
                        add_disjoint_pair(&mut disjoint, atoms[i], atoms[j]);
                    }
                }
            }
            _ => {} // Role / ABox / declaration axioms contribute nothing to told tables.
        }
    }

    // Reflexive-transitive closure on the subsumption graph (BFS per node).
    let n_u32 = u32::try_from(n).expect("ToldTables: too many classes");
    let mut super_closure: Vec<Box<[ClassId]>> = Vec::with_capacity(n);
    let mut sub_closure_acc: Vec<Vec<ClassId>> = vec![Vec::new(); n];

    // `visited` and `queue` are HOISTED and reused across the per-class BFS.
    //
    // They were allocated fresh inside this loop, which made the closure build
    // O(n^2) in *memset traffic* alone: `vec![false; n]` zeroes n bytes, n times.
    // On `ore_ont_9674` (981,148 classes) that is ~963 GB of zeroing, and a perf
    // profile attributed **69.7%** of `tbox-stats` self-time to
    // `__memset_avx2_unaligned_erms` with `build_told_tables` the top rustdl frame.
    // The quadratic was confirmed by fit rather than by inspection alone: across
    // six large ontologies spanning a 1.34x range of n, `convert_ms / n^2` is
    // constant to within 3.6% (55,462-57,519). It is quadratic in the CLASS COUNT,
    // not the file size, which is why the corpus's *largest* file converted
    // fastest.
    //
    // Reuse is exact, not approximate: `ups` receives a node exactly when that
    // node is marked visited, so clearing the `ups` entries restores `visited` to
    // all-false in O(|ups|) rather than O(n). The BFS is otherwise untouched, so
    // the tables are byte-identical.
    let mut visited = vec![false; n];
    let mut queue: VecDeque<u32> = VecDeque::new();

    for c in 0..n_u32 {
        queue.clear();
        queue.push_back(c);
        let mut ups: Vec<ClassId> = Vec::new();
        while let Some(curr) = queue.pop_front() {
            let curr_idx = curr as usize;
            if visited[curr_idx] {
                continue;
            }
            visited[curr_idx] = true;
            ups.push(ClassId::new(curr));
            for &sup in &direct_super[curr_idx] {
                queue.push_back(sup.index());
            }
        }
        // Restore the shared buffer for the next class. MUST happen before `ups`
        // is moved into `super_closure` below.
        for &u in &ups {
            visited[u.index() as usize] = false;
        }
        ups.sort_unstable();
        for &sup in &ups {
            sub_closure_acc[sup.index() as usize].push(ClassId::new(c));
        }
        super_closure.push(ups.into_boxed_slice());
    }

    let sub_closure: Vec<Box<[ClassId]>> = sub_closure_acc
        .into_iter()
        .map(|mut v| {
            v.sort_unstable();
            v.into_boxed_slice()
        })
        .collect();

    let disjoint_with: Vec<Box<[ClassId]>> = disjoint
        .into_iter()
        .map(|sv| {
            let mut v: Vec<ClassId> = sv.into_iter().collect();
            v.sort_unstable();
            v.dedup();
            v.into_boxed_slice()
        })
        .collect();

    ToldTables {
        super_closure,
        sub_closure,
        disjoint_with,
    }
}

fn as_atomic(cid: ConceptId, pool: &ConceptPool) -> Option<ClassId> {
    if let ConceptExpr::Atomic(c) = pool.get(cid) {
        Some(*c)
    } else {
        None
    }
}

fn as_not_atomic(cid: ConceptId, pool: &ConceptPool) -> Option<ClassId> {
    if let ConceptExpr::Not(inner) = pool.get(cid)
        && let ConceptExpr::Atomic(c) = pool.get(*inner)
    {
        return Some(*c);
    }
    None
}

/// `And(Atomic(a), Atomic(b)) ⊑ Bot` ⟹ `Some((a, b))`. Exactly two atomic
/// operands: an n-ary conjunction asserts only that the whole conjunction is
/// unsatisfiable, which does NOT make any particular pair disjoint.
fn as_atomic_pair_to_bot(
    sub: ConceptId,
    sup: ConceptId,
    pool: &ConceptPool,
) -> Option<(ClassId, ClassId)> {
    if !matches!(pool.get(sup), ConceptExpr::Bot) {
        return None;
    }
    let ConceptExpr::And(ops) = pool.get(sub) else {
        return None;
    };
    if ops.len() != 2 {
        return None;
    }
    let a = as_atomic(ops[0], pool)?;
    let b = as_atomic(ops[1], pool)?;
    Some((a, b))
}

fn add_edge(direct_super: &mut [SmallVec<[ClassId; 4]>], sub: ClassId, sup: ClassId) {
    let idx = sub.index() as usize;
    if !direct_super[idx].contains(&sup) {
        direct_super[idx].push(sup);
    }
}

fn add_disjoint_pair(disjoint: &mut [HashSet<ClassId>], a: ClassId, b: ClassId) {
    let ai = a.index() as usize;
    let bi = b.index() as usize;
    disjoint[ai].insert(b);
    if a != b {
        disjoint[bi].insert(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::Axiom;

    /// Build an `InternalOntology` with the given class IRIs interned and
    /// axioms produced by the closure.
    fn build_ontology(
        class_iris: &[&str],
        make_axioms: impl FnOnce(&mut InternalOntology),
    ) -> InternalOntology {
        let mut o = InternalOntology::new();
        for iri in class_iris {
            o.vocabulary.intern_class(iri);
        }
        make_axioms(&mut o);
        o
    }

    fn c(o: &InternalOntology, name: &str) -> ClassId {
        o.vocabulary.class_id(name).expect("class missing")
    }

    fn atom(o: &mut InternalOntology, name: &str) -> ConceptId {
        let id = c(o, name);
        o.concepts.atomic(id)
    }

    #[test]
    fn empty_ontology_is_reflexive_only() {
        let o = build_ontology(&["A", "B", "C"], |_| {});
        let t = build_told_tables(&o);
        for name in ["A", "B", "C"] {
            let cid = c(&o, name);
            assert_eq!(t.super_classes(cid), [cid].as_slice());
            assert_eq!(t.sub_classes(cid), [cid].as_slice());
            assert_eq!(t.disjoints_of(cid), [].as_slice());
        }
    }

    #[test]
    fn sub_class_of_chain_closes_transitively() {
        // A ⊑ B ⊑ C
        let o = build_ontology(&["A", "B", "C"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            let cc = atom(o, "C");
            o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
            o.axioms.push(Axiom::SubClassOf { sub: b, sup: cc });
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let b = c(&o, "B");
        let cc = c(&o, "C");
        assert_eq!(t.super_classes(a), [a, b, cc].as_slice());
        assert_eq!(t.super_classes(b), [b, cc].as_slice());
        assert_eq!(t.super_classes(cc), [cc].as_slice());
        assert_eq!(t.sub_classes(cc), [a, b, cc].as_slice());
        assert!(t.is_told_sub(a, cc));
        assert!(!t.is_told_sub(cc, a));
    }

    #[test]
    fn equivalent_classes_creates_bidirectional_subsumption() {
        let o = build_ontology(&["A", "B"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            o.axioms.push(Axiom::EquivalentClasses(vec![a, b]));
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let b = c(&o, "B");
        assert!(t.is_told_sub(a, b));
        assert!(t.is_told_sub(b, a));
    }

    #[test]
    fn disjoint_classes_pairwise() {
        // DisjointClasses(A, B, C)
        let o = build_ontology(&["A", "B", "C"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            let cc = atom(o, "C");
            o.axioms.push(Axiom::DisjointClasses(vec![a, b, cc]));
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let b = c(&o, "B");
        let cc = c(&o, "C");
        assert!(t.are_told_disjoint(a, b));
        assert!(t.are_told_disjoint(a, cc));
        assert!(t.are_told_disjoint(b, cc));
        assert!(t.are_told_disjoint(b, a)); // symmetric
        assert_eq!(t.disjoints_of(a), [b, cc].as_slice());
    }

    #[test]
    fn sub_class_of_not_atomic_creates_disjoint() {
        // SubClassOf(A, Not(B))  ⇒  A and B disjoint.
        let o = build_ontology(&["A", "B"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            let not_b = o.concepts.not(b);
            o.axioms.push(Axiom::SubClassOf { sub: a, sup: not_b });
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let b = c(&o, "B");
        assert!(t.are_told_disjoint(a, b));
        // And no subsumption edge — Not(B) isn't a named class.
        assert!(!t.is_told_sub(a, b));
    }

    #[test]
    fn disjoint_union_contributes_both_subsumption_and_disjointness() {
        // DisjointUnion(P, [C1, C2])  ⇒  C1 ⊑ P, C2 ⊑ P, and C1 ⌖ C2.
        let o = build_ontology(&["P", "C1", "C2"], |o| {
            let c1 = atom(o, "C1");
            let c2 = atom(o, "C2");
            o.axioms.push(Axiom::DisjointUnion {
                class: c(o, "P"),
                members: vec![c1, c2],
            });
        });
        let t = build_told_tables(&o);
        let p = c(&o, "P");
        let c1 = c(&o, "C1");
        let c2 = c(&o, "C2");
        assert!(t.is_told_sub(c1, p));
        assert!(t.is_told_sub(c2, p));
        assert!(t.are_told_disjoint(c1, c2));
    }

    #[test]
    fn complex_shapes_are_skipped() {
        // SubClassOf(A ⊓ B, C)  — the LHS is And, not Atomic, so this
        // contributes nothing to told subsumers.
        let o = build_ontology(&["A", "B", "C"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            let cc = atom(o, "C");
            let and = o.concepts.and([a, b]);
            o.axioms.push(Axiom::SubClassOf { sub: and, sup: cc });
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let cc = c(&o, "C");
        assert!(!t.is_told_sub(a, cc));
    }

    #[test]
    fn equivalence_with_complex_member_only_uses_atomic_pairs() {
        // EquivalentClasses(A, B, And(C, D))  — A ⊑ B and B ⊑ A get added;
        // the third (complex) member does not contribute.
        let o = build_ontology(&["A", "B", "C", "D"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            let cc = atom(o, "C");
            let d = atom(o, "D");
            let and = o.concepts.and([cc, d]);
            o.axioms.push(Axiom::EquivalentClasses(vec![a, b, and]));
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let b = c(&o, "B");
        let cc = c(&o, "C");
        assert!(t.is_told_sub(a, b));
        assert!(t.is_told_sub(b, a));
        assert!(!t.is_told_sub(a, cc));
    }

    #[test]
    fn diamond_shape() {
        // A ⊑ B, A ⊑ C, B ⊑ D, C ⊑ D
        let o = build_ontology(&["A", "B", "C", "D"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            let cc = atom(o, "C");
            let d = atom(o, "D");
            o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
            o.axioms.push(Axiom::SubClassOf { sub: a, sup: cc });
            o.axioms.push(Axiom::SubClassOf { sub: b, sup: d });
            o.axioms.push(Axiom::SubClassOf { sub: cc, sup: d });
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let d = c(&o, "D");
        assert!(t.is_told_sub(a, d));
    }

    #[test]
    fn duplicate_axioms_idempotent() {
        let o = build_ontology(&["A", "B"], |o| {
            let a = atom(o, "A");
            let b = atom(o, "B");
            for _ in 0..3 {
                o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
            }
        });
        let t = build_told_tables(&o);
        let a = c(&o, "A");
        let b = c(&o, "B");
        assert_eq!(t.super_classes(a), [a, b].as_slice());
    }

    /// All three spellings of "A and B are disjoint" must yield the same told
    /// pair: `DisjointClasses(A B)`, `A ⊑ ¬B`, and `A ⊓ B ⊑ ⊥`. The third is
    /// what the negation-GCI rewrite produces, and it is also how users write
    /// disjointness directly.
    #[test]
    fn and_bot_gci_records_told_disjoint_pair() {
        let mut o = InternalOntology::new();
        let a = o.vocabulary.intern_class("http://t/A");
        let b = o.vocabulary.intern_class("http://t/B");
        let a_c = o.concepts.atomic(a);
        let b_c = o.concepts.atomic(b);
        let and_ab = o.concepts.and(vec![a_c, b_c]);
        let bot = o.concepts.bot();
        o.axioms.push(Axiom::SubClassOf {
            sub: and_ab,
            sup: bot,
        });

        let told = build_told_tables(&o);
        assert!(
            told.are_told_disjoint(a, b),
            "A ⊓ B ⊑ ⊥ must record A and B as told-disjoint"
        );
    }
}
