//! The canonical model: elements are INTERNED LABEL SETS.
//!
//! Two classes share an element exactly when their subsumer sets coincide,
//! which (because `subsumers_of` is reflexive) happens exactly for
//! derived-equivalent classes.

use hashbrown::HashMap;
use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, InternalOntology, RoleHierarchy, RoleHierarchyBuilder, RoleId,
    SubRolePath,
};
use owl_dl_saturation::Subsumers;

use crate::interp::{Element, Interpretation};

#[derive(Debug, Default)]
pub struct FiniteModel {
    labels: Vec<Box<[ClassId]>>,
    label_ix: HashMap<Box<[ClassId]>, Element>,
    /// Indexed by `RoleId`; holds edges of the DECLARED role only. Sub-role
    /// inclusion is answered on demand so that closure is never materialised.
    edges: Vec<Vec<(Element, Element)>>,
    class_of: HashMap<ClassId, Element>,
}

impl FiniteModel {
    /// Interns `label` (which MUST be sorted ascending) and returns its element.
    pub fn intern(&mut self, label: Vec<ClassId>) -> Element {
        debug_assert!(
            label.windows(2).all(|w| w[0] <= w[1]),
            "label must be sorted"
        );
        let key: Box<[ClassId]> = label.into_boxed_slice();
        if let Some(&e) = self.label_ix.get(&key) {
            return e;
        }
        let e = Element::new(u32::try_from(self.labels.len()).unwrap_or(u32::MAX));
        self.labels.push(key.clone());
        self.label_ix.insert(key, e);
        e
    }

    #[must_use]
    pub fn label(&self, e: Element) -> &[ClassId] {
        self.labels
            .get(e.index() as usize)
            .map_or(&[], |l| l.as_ref())
    }

    #[must_use]
    pub fn element_of_class(&self, c: ClassId) -> Option<Element> {
        self.class_of.get(&c).copied()
    }

    /// Seeds one element per SATISFIABLE class, over the union of the named
    /// vocabulary and every id appearing in `facts` in either position.
    ///
    /// Unsatisfiable classes get NO element. That is inertness hygiene, not a
    /// detection mechanism: a dropped `⊑ ⊥` axiom leaves its class satisfiable
    /// and therefore seeded, and the evaluator is what catches it.
    #[must_use]
    pub fn seed(
        internal: &InternalOntology,
        subs: &Subsumers,
        facts: &[(ClassId, RoleId, ClassId)],
    ) -> Self {
        let mut model = Self {
            edges: vec![Vec::new(); internal.vocabulary.num_roles()],
            ..Self::default()
        };
        let mut population: Vec<ClassId> =
            internal.vocabulary.classes().map(|(id, _)| id).collect();
        for &(sub, _, target) in facts {
            population.push(sub);
            population.push(target);
        }
        population.sort_unstable_by_key(|c| c.index());
        population.dedup();

        for c in population {
            if subs.is_unsatisfiable(c) {
                continue;
            }
            let label = subs.subsumers_of(c);
            let e = model.intern(label);
            model.class_of.insert(c, e);
        }
        model
    }
}

impl Interpretation for FiniteModel {
    fn domain_size(&self) -> usize {
        self.labels.len()
    }
    fn elements(&self) -> impl Iterator<Item = Element> + '_ {
        (0..u32::try_from(self.labels.len()).unwrap_or(u32::MAX)).map(Element::new)
    }
    fn in_concept(&self, e: Element, c: ClassId) -> bool {
        self.label(e)
            .binary_search_by_key(&c.index(), |cid| cid.index())
            .is_ok()
    }
    fn successors(&self, _e: Element, _r: RoleId) -> Vec<Element> {
        Vec::new() // edges arrive in Task 4
    }
    fn has_edge(&self, _from: Element, _r: RoleId, _to: Element) -> bool {
        false
    }
    fn edges(&self, _r: RoleId) -> Vec<(Element, Element)> {
        Vec::new()
    }
    fn num_roles(&self) -> usize {
        self.edges.len()
    }
}

/// Builds the named-role hierarchy from the lowered axioms.
///
/// `is_pure_el` admits no inverse-role USE, so inverse canonicalization (which
/// the reasoner's private builder performs) is deliberately not replicated: any
/// inverse occurrence puts the ontology out of fragment.
#[must_use]
pub fn build_role_hierarchy(internal: &InternalOntology) -> RoleHierarchy {
    let n = u32::try_from(internal.vocabulary.num_roles()).unwrap_or(u32::MAX);
    let mut b = RoleHierarchyBuilder::with_roles(n);
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(r),
                sup,
            } if !r.is_inverse() && !sup.is_inverse() => {
                b.add_sub_role(r.role_id(), sup.role_id());
            }
            Axiom::EquivalentObjectProperties(roles) => {
                for a in roles {
                    for c in roles {
                        if !a.is_inverse() && !c.is_inverse() {
                            b.add_sub_role(a.role_id(), c.role_id());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    b.build()
}

/// `eff_ranges(r) = ⋃ { ranges(s) : s ∈ super_roles(r) }`.
///
/// SUPER-roles, because `r ⊑ s` makes an `r`-edge an `s`-edge, so `Range(s)`
/// constrains `r`-successors. `super_roles` is reflexive.
///
/// `Top` fillers are skipped (trivial). `Bot` fillers are skipped too: a label
/// cannot carry `⊥`, and the axiom check is its home — which is exactly what
/// makes the `Range(r,⊥)` case a DETECTION rather than a refusal.
#[must_use]
pub fn effective_ranges(
    internal: &InternalOntology,
    h: &RoleHierarchy,
) -> HashMap<RoleId, Vec<ClassId>> {
    let mut declared: HashMap<RoleId, Vec<ClassId>> = HashMap::new();
    for ax in &internal.axioms {
        if let Axiom::ObjectPropertyRange { role, range } = ax {
            if role.is_inverse() {
                continue;
            }
            if let ConceptExpr::Atomic(c) = internal.concepts.get(*range) {
                declared.entry(role.role_id()).or_default().push(*c);
            }
        }
    }
    let mut out: HashMap<RoleId, Vec<ClassId>> = HashMap::new();
    for r in 0..u32::try_from(h.num_roles()).unwrap_or(u32::MAX) {
        let rid = RoleId::new(r);
        let mut acc: Vec<ClassId> = Vec::new();
        for s in h.super_roles(rid) {
            if let Some(cs) = declared.get(s) {
                acc.extend_from_slice(cs);
            }
        }
        acc.sort_unstable_by_key(|c| c.index());
        acc.dedup();
        if !acc.is_empty() {
            out.insert(rid, acc);
        }
    }
    out
}
