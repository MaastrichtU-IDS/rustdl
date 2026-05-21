//! Vocabulary: bidirectional interning between OWL IRIs and our compact ids.
//!
//! Class, role, and individual namespaces are kept separate so that
//! [punning](https://www.w3.org/TR/owl2-new-features/#F12:_Punning) — the same
//! IRI denoting entities of different kinds — works naturally. Each kind has
//! its own [`u32`]-indexed table and produces its own id type.
//!
//! IRIs are stored as `Arc<str>` so id-to-IRI lookups are cheap and the
//! representation survives parallel classification (Phase 4+).

use std::sync::Arc;

use hashbrown::HashMap;

use crate::ir::{ClassId, IndividualId, RoleId};

#[derive(Default, Clone, Debug)]
struct IriTable {
    by_id: Vec<Arc<str>>,
    by_iri: HashMap<Arc<str>, u32>,
}

impl IriTable {
    fn intern(&mut self, iri: &str) -> u32 {
        if let Some(&id) = self.by_iri.get(iri) {
            return id;
        }
        let arc: Arc<str> = Arc::from(iri);
        let id = u32::try_from(self.by_id.len()).expect("Vocabulary: namespace exceeds u32::MAX");
        self.by_iri.insert(Arc::clone(&arc), id);
        self.by_id.push(arc);
        id
    }

    fn get(&self, id: u32) -> &str {
        &self.by_id[id as usize]
    }

    fn lookup(&self, iri: &str) -> Option<u32> {
        self.by_iri.get(iri).copied()
    }

    fn len(&self) -> usize {
        self.by_id.len()
    }

    fn iter(&self) -> impl Iterator<Item = (u32, &str)> + '_ {
        // `0u32..` matches `by_id.len() <= u32::MAX` invariant enforced by `intern`.
        (0u32..)
            .zip(self.by_id.iter())
            .map(|(i, s)| (i, s.as_ref()))
    }
}

/// Bidirectional interning of OWL named entities (classes, object properties,
/// individuals). Each namespace is independent: the same IRI string interned
/// as a class and as an individual produces unrelated ids.
#[derive(Default, Clone, Debug)]
pub struct Vocabulary {
    classes: IriTable,
    roles: IriTable,
    individuals: IriTable,
}

impl Vocabulary {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern_class(&mut self, iri: &str) -> ClassId {
        ClassId::new(self.classes.intern(iri))
    }

    pub fn intern_role(&mut self, iri: &str) -> RoleId {
        RoleId::new(self.roles.intern(iri))
    }

    pub fn intern_individual(&mut self, iri: &str) -> IndividualId {
        IndividualId::new(self.individuals.intern(iri))
    }

    /// # Panics
    /// Panics if `id` was not produced by this vocabulary.
    #[must_use]
    pub fn class_iri(&self, id: ClassId) -> &str {
        self.classes.get(id.index())
    }

    /// # Panics
    /// Panics if `id` was not produced by this vocabulary.
    #[must_use]
    pub fn role_iri(&self, id: RoleId) -> &str {
        self.roles.get(id.index())
    }

    /// # Panics
    /// Panics if `id` was not produced by this vocabulary.
    #[must_use]
    pub fn individual_iri(&self, id: IndividualId) -> &str {
        self.individuals.get(id.index())
    }

    #[must_use]
    pub fn class_id(&self, iri: &str) -> Option<ClassId> {
        self.classes.lookup(iri).map(ClassId::new)
    }

    #[must_use]
    pub fn role_id(&self, iri: &str) -> Option<RoleId> {
        self.roles.lookup(iri).map(RoleId::new)
    }

    #[must_use]
    pub fn individual_id(&self, iri: &str) -> Option<IndividualId> {
        self.individuals.lookup(iri).map(IndividualId::new)
    }

    #[must_use]
    pub fn num_classes(&self) -> usize {
        self.classes.len()
    }

    #[must_use]
    pub fn num_roles(&self) -> usize {
        self.roles.len()
    }

    #[must_use]
    pub fn num_individuals(&self) -> usize {
        self.individuals.len()
    }

    pub fn classes(&self) -> impl Iterator<Item = (ClassId, &str)> + '_ {
        self.classes.iter().map(|(i, s)| (ClassId::new(i), s))
    }

    pub fn roles(&self) -> impl Iterator<Item = (RoleId, &str)> + '_ {
        self.roles.iter().map(|(i, s)| (RoleId::new(i), s))
    }

    pub fn individuals(&self) -> impl Iterator<Item = (IndividualId, &str)> + '_ {
        self.individuals
            .iter()
            .map(|(i, s)| (IndividualId::new(i), s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_deterministic() {
        let mut v = Vocabulary::new();
        let a1 = v.intern_class("http://example.org/A");
        let a2 = v.intern_class("http://example.org/A");
        assert_eq!(a1, a2);
        assert_eq!(v.num_classes(), 1);
    }

    #[test]
    fn distinct_iris_distinct_ids() {
        let mut v = Vocabulary::new();
        let a = v.intern_class("http://example.org/A");
        let b = v.intern_class("http://example.org/B");
        assert_ne!(a, b);
        assert_eq!(v.num_classes(), 2);
    }

    #[test]
    fn punning_keeps_namespaces_separate() {
        // Same IRI as class and as individual → unrelated ids,
        // both indexable in their own namespace.
        let mut v = Vocabulary::new();
        let iri = "http://example.org/Foo";
        let class_id = v.intern_class(iri);
        let ind_id = v.intern_individual(iri);
        assert_eq!(class_id.index(), 0);
        assert_eq!(ind_id.index(), 0);
        assert_eq!(v.class_iri(class_id), iri);
        assert_eq!(v.individual_iri(ind_id), iri);
        assert_eq!(v.num_classes(), 1);
        assert_eq!(v.num_individuals(), 1);
        assert_eq!(v.num_roles(), 0);
    }

    #[test]
    fn iri_round_trip() {
        let mut v = Vocabulary::new();
        let r1 = v.intern_role("http://example.org/hasParent");
        let r2 = v.intern_role("http://example.org/hasChild");
        assert_eq!(v.role_iri(r1), "http://example.org/hasParent");
        assert_eq!(v.role_iri(r2), "http://example.org/hasChild");
        assert_eq!(v.role_id("http://example.org/hasParent"), Some(r1));
        assert_eq!(v.role_id("http://example.org/missing"), None);
    }

    #[test]
    fn iteration_in_insertion_order() {
        let mut v = Vocabulary::new();
        let _ = v.intern_class("C0");
        let _ = v.intern_class("C1");
        let _ = v.intern_class("C2");
        let collected: Vec<(ClassId, &str)> = v.classes().collect();
        assert_eq!(
            collected,
            vec![
                (ClassId::new(0), "C0"),
                (ClassId::new(1), "C1"),
                (ClassId::new(2), "C2"),
            ]
        );
    }
}
