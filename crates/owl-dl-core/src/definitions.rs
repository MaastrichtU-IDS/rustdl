//! Definition extraction for lazy unfolding.
//!
//! A *definition* is an axiom of the shape `A ≡ C` where `A` is a named
//! atomic class and `C` is a complex concept. The Phase 2 tableau treats
//! these specially: `A` stays in node labels as an atom, and the body `C`
//! is unfolded only when needed (a contradiction or a forced rule fires
//! on `A`). That's "lazy unfolding".
//!
//! What this module extracts:
//!
//! - Exact `EquivalentClasses` axioms with two members, one atomic and
//!   one complex, are picked up.
//! - Multi-way `EquivalentClasses` and `SubClassOf`-only equivalences
//!   are ignored — Phase 1 absorption is the right place for the harder
//!   cases.
//! - When multiple definitions disagree about a name, the first one wins
//!   (silently). Merging by intersection is a Phase 4 refinement.
//!
//! Cycle handling is deferred to the tableau's blocking machinery.

use crate::ConceptPool;
use crate::ir::{ClassId, ConceptExpr, ConceptId};
use crate::ontology::{Axiom, InternalOntology};

/// Map from named-class id to its definition body, if it has one.
#[derive(Debug, Default, Clone)]
pub struct Definitions {
    defs: Vec<Option<ConceptId>>,
}

impl Definitions {
    /// Body of `c`'s definition, or `None` if `c` is primitive.
    #[must_use]
    pub fn body_of(&self, c: ClassId) -> Option<ConceptId> {
        self.defs.get(c.index() as usize).copied().flatten()
    }

    /// Whether `c` has a definition body.
    #[must_use]
    pub fn is_defined(&self, c: ClassId) -> bool {
        self.body_of(c).is_some()
    }

    /// Total number of named classes covered by this table (including
    /// primitives without a definition).
    #[must_use]
    pub fn num_classes(&self) -> usize {
        self.defs.len()
    }

    /// Count of names that have an attached definition body.
    #[must_use]
    pub fn num_defined(&self) -> usize {
        self.defs.iter().filter(|o| o.is_some()).count()
    }

    /// Iterate over `(class, body)` pairs in class-id order.
    pub fn iter(&self) -> impl Iterator<Item = (ClassId, ConceptId)> + '_ {
        (0u32..)
            .zip(self.defs.iter())
            .filter_map(|(i, body)| body.map(|b| (ClassId::new(i), b)))
    }
}

/// Walk an ontology's axioms and collect lazy-unfolding-eligible definitions.
#[must_use]
pub fn extract_definitions(ontology: &InternalOntology) -> Definitions {
    let n = ontology.vocabulary.num_classes();
    let mut defs: Vec<Option<ConceptId>> = vec![None; n];

    for axiom in &ontology.axioms {
        if let Axiom::EquivalentClasses(ids) = axiom
            && ids.len() == 2
            && let Some((name, body)) = pick_def_pair(ids[0], ids[1], &ontology.concepts)
        {
            let idx = name.index() as usize;
            if defs[idx].is_none() {
                defs[idx] = Some(body);
            }
            // First-definition-wins; subsequent disagreements are dropped.
        }
    }

    Definitions { defs }
}

/// Given two `ConceptId`s, return `(named, body)` if exactly one side is atomic.
fn pick_def_pair(a: ConceptId, b: ConceptId, pool: &ConceptPool) -> Option<(ClassId, ConceptId)> {
    let a_atom = match pool.get(a) {
        ConceptExpr::Atomic(c) => Some(*c),
        _ => None,
    };
    let b_atom = match pool.get(b) {
        ConceptExpr::Atomic(c) => Some(*c),
        _ => None,
    };
    match (a_atom, b_atom) {
        (Some(name), None) => Some((name, b)),
        (None, Some(name)) => Some((name, a)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::many_single_char_names)]

    use super::*;
    use crate::ir::{Role, RoleId};

    fn ontology_with(classes: &[&str]) -> InternalOntology {
        let mut o = InternalOntology::new();
        for c in classes {
            o.vocabulary.intern_class(c);
        }
        o
    }

    fn cid(o: &InternalOntology, name: &str) -> ClassId {
        o.vocabulary.class_id(name).expect("class missing")
    }

    fn atom(o: &mut InternalOntology, name: &str) -> ConceptId {
        let c = cid(o, name);
        o.concepts.atomic(c)
    }

    #[test]
    fn empty_ontology_has_no_definitions() {
        let o = ontology_with(&["A", "B"]);
        let d = extract_definitions(&o);
        assert_eq!(d.num_classes(), 2);
        assert_eq!(d.num_defined(), 0);
        assert!(!d.is_defined(cid(&o, "A")));
    }

    #[test]
    fn equiv_atomic_to_complex_creates_definition() {
        // A ≡ ∃R.B
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        o.axioms.push(Axiom::EquivalentClasses(vec![a, some_b]));
        let d = extract_definitions(&o);
        let a_id = cid(&o, "A");
        assert!(d.is_defined(a_id));
        assert_eq!(d.body_of(a_id), Some(some_b));
        // B is not defined — it's primitive.
        assert!(!d.is_defined(cid(&o, "B")));
    }

    #[test]
    fn equiv_complex_to_atomic_creates_definition_with_swap() {
        // Same as above but operand order flipped.
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        o.axioms.push(Axiom::EquivalentClasses(vec![some_b, a]));
        let d = extract_definitions(&o);
        assert_eq!(d.body_of(cid(&o, "A")), Some(some_b));
    }

    #[test]
    fn two_atomic_members_is_not_a_definition() {
        // A ≡ B  — both atomic, told-subsumer territory, not lazy unfolding.
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::EquivalentClasses(vec![a, b]));
        let d = extract_definitions(&o);
        assert_eq!(d.num_defined(), 0);
    }

    #[test]
    fn three_or_more_members_skipped() {
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        o.axioms.push(Axiom::EquivalentClasses(vec![a, b, some_b]));
        let d = extract_definitions(&o);
        assert_eq!(d.num_defined(), 0);
    }

    #[test]
    fn two_complex_members_skipped() {
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        let not_a = o.concepts.not(a);
        o.axioms.push(Axiom::EquivalentClasses(vec![some_b, not_a]));
        let d = extract_definitions(&o);
        assert_eq!(d.num_defined(), 0);
    }

    #[test]
    fn first_definition_wins_when_conflicts() {
        // A ≡ ∃R.B  and  A ≡ ∃R.C  — keep first.
        let mut o = ontology_with(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        let some_c = o.concepts.some(r, cc);
        o.axioms.push(Axiom::EquivalentClasses(vec![a, some_b]));
        o.axioms.push(Axiom::EquivalentClasses(vec![a, some_c]));
        let d = extract_definitions(&o);
        assert_eq!(d.body_of(cid(&o, "A")), Some(some_b));
    }

    #[test]
    fn idempotent_when_same_def_appears_twice() {
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        o.axioms.push(Axiom::EquivalentClasses(vec![a, some_b]));
        o.axioms.push(Axiom::EquivalentClasses(vec![a, some_b]));
        let d = extract_definitions(&o);
        assert_eq!(d.num_defined(), 1);
    }

    #[test]
    fn multiple_distinct_class_definitions() {
        // A ≡ ∃R.X, B ≡ ∃S.Y
        let mut o = ontology_with(&["A", "B", "X", "Y"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let x = atom(&mut o, "X");
        let y = atom(&mut o, "Y");
        let r = Role::named(RoleId::new(0));
        let s = Role::named(RoleId::new(1));
        let some_x = o.concepts.some(r, x);
        let some_y = o.concepts.some(s, y);
        o.axioms.push(Axiom::EquivalentClasses(vec![a, some_x]));
        o.axioms.push(Axiom::EquivalentClasses(vec![b, some_y]));
        let d = extract_definitions(&o);
        assert_eq!(d.num_defined(), 2);
        let pairs: Vec<(ClassId, ConceptId)> = d.iter().collect();
        assert_eq!(pairs, vec![(cid(&o, "A"), some_x), (cid(&o, "B"), some_y)]);
    }

    #[test]
    fn sub_class_only_is_not_a_definition() {
        // A ⊑ ∃R.B (not equivalence) — not a definition.
        let mut o = ontology_with(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let some_b = o.concepts.some(r, b);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: some_b,
        });
        let d = extract_definitions(&o);
        assert_eq!(d.num_defined(), 0);
    }
}
