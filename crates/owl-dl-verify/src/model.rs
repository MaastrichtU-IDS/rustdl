//! The canonical model: elements are INTERNED LABEL SETS.
//!
//! Two classes share an element exactly when their subsumer sets coincide,
//! which (because `subsumers_of` is reflexive) happens exactly for
//! derived-equivalent classes.

use hashbrown::HashMap;
use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, InternalOntology, RoleHierarchy,
    RoleHierarchyBuilder, RoleId, SubRolePath,
};
use owl_dl_saturation::Subsumers;

use crate::interp::{Element, Interpretation};
use crate::{Bounds, UnresolvedReason};

#[derive(Debug, Default)]
pub struct FiniteModel {
    labels: Vec<Box<[ClassId]>>,
    label_ix: HashMap<Box<[ClassId]>, Element>,
    /// Indexed by `RoleId`; holds edges of the DECLARED role only. Sub-role
    /// inclusion is answered on demand so that closure is never materialised.
    edges: Vec<Vec<(Element, Element)>>,
    class_of: HashMap<ClassId, Element>,
    /// `RoleHierarchy` derives only `Debug + Clone`, not `Default`, so this
    /// must stay optional to keep `FiniteModel`'s derived `Default` (which
    /// `seed` builds on via `..Self::default()`) working. Absent reads the
    /// same as an out-of-range role: an empty sub-role extension.
    hierarchy: Option<RoleHierarchy>,
    /// Running total of edges across all roles, checked against
    /// `bounds.max_edges` by `push_edge`. Shared by both expansion paths.
    edge_count: usize,
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

    /// Attaches the role hierarchy, which `successors`/`has_edge`/`edges` need
    /// to answer sub-role inclusion on demand.
    #[must_use]
    pub fn with_hierarchy(mut self, h: RoleHierarchy) -> Self {
        self.hierarchy = Some(h);
        self
    }

    /// Sub-roles of `r`, or `&[]` for a role outside the hierarchy.
    ///
    /// `RoleHierarchy::sub_roles` PANICS out of range, and "the edit introduces a
    /// role" is the normal case for `still_holds_after`, so an unknown role must
    /// read as an empty extension rather than crashing. A model with no
    /// hierarchy attached yet behaves the same way.
    fn hierarchy_sub_roles(&self, r: RoleId) -> &[RoleId] {
        match &self.hierarchy {
            Some(h) if (r.index() as usize) < h.num_roles() => h.sub_roles(r),
            _ => &[],
        }
    }

    /// Target label for a fact `(_, r, y)`.
    ///
    /// Returns `Err(aug)` when the label would need `TBox` closure this local rule
    /// cannot supply. Task 5 replaces that with injection; until then the caller
    /// reports `LabelNotClosed`, because a plain union is a FALSE-`Violated`
    /// generator: with `Range(u,F)` and `F ⊑ G` it yields `{A,F}`, missing `G`,
    /// so `SubClassOf(F,G)` reads violated on a HEALTHY ontology.
    fn target_label(
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        r: RoleId,
        y: ClassId,
    ) -> Result<Vec<ClassId>, Vec<ClassId>> {
        let base = subs.subsumers_of(y);
        let Some(ranges) = eff.get(&r) else {
            return Ok(base);
        };
        let aug: Vec<ClassId> = ranges
            .iter()
            .copied()
            .filter(|c| {
                base.binary_search_by_key(&c.index(), |k| k.index())
                    .is_err()
            })
            .collect();
        if aug.is_empty() { Ok(base) } else { Err(aug) }
    }

    /// Appends the edge `(from, r, to)` if not already present, checking
    /// `bounds.max_edges` against the running total.
    ///
    /// Shared by both expansion paths (`expand` and `expand_from_axioms` via
    /// `materialise_exists`) so they cannot drift on bound handling — a path
    /// that appended edges without this check would silently exceed a bound
    /// the other path honours.
    ///
    /// Returns `true` iff `max_edges` tripped (a `BoundTripped` reason was
    /// pushed and the caller must stop).
    fn push_edge(
        &mut self,
        r: RoleId,
        from: Element,
        to: Element,
        bounds: &Bounds,
        reasons: &mut Vec<UnresolvedReason>,
    ) -> bool {
        let Some(bucket) = self.edges.get_mut(r.index() as usize) else {
            return false;
        };
        if bucket.contains(&(from, to)) {
            return false;
        }
        bucket.push((from, to));
        self.edge_count += 1;
        if self.edge_count > bounds.max_edges {
            reasons.push(UnresolvedReason::BoundTripped {
                bound: "max_edges",
                limit: Some(bounds.max_edges),
            });
            return true;
        }
        false
    }

    /// Expands seeded elements into a graph: every existential fact `(x, r, y)`
    /// on a labelled element becomes an `r`-edge to the interned target label,
    /// iterated to a fixpoint (or a tripped bound).
    ///
    /// Report-only for now: a fact whose target label would need `TBox` closure
    /// (`target_label` returning `Err`) is reported as `LabelNotClosed` rather
    /// than approximated, per the module doc above.
    pub fn expand(
        &mut self,
        subs: &Subsumers,
        facts: &[(ClassId, RoleId, ClassId)],
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
    ) -> Vec<UnresolvedReason> {
        let mut by_sub: HashMap<ClassId, Vec<(RoleId, ClassId)>> = HashMap::new();
        for &(s, r, t) in facts {
            by_sub.entry(s).or_default().push((r, t));
        }
        let mut reasons = Vec::new();
        let mut queue: Vec<Element> = self.elements().collect();
        while let Some(e) = queue.pop() {
            let classes: Vec<ClassId> = self.label(e).to_vec();
            for x in classes {
                let Some(outs) = by_sub.get(&x).cloned() else {
                    continue;
                };
                for (r, y) in outs {
                    match Self::target_label(subs, eff, r, y) {
                        Ok(label) => {
                            let before = self.labels.len();
                            let t = self.intern(label);
                            if self.labels.len() > bounds.max_elements {
                                reasons.push(UnresolvedReason::BoundTripped {
                                    bound: "max_elements",
                                    limit: Some(bounds.max_elements),
                                });
                                return reasons;
                            }
                            if self.push_edge(r, e, t, bounds, &mut reasons) {
                                return reasons;
                            }
                            if self.labels.len() > before {
                                queue.push(t);
                            }
                        }
                        Err(_aug) => {
                            reasons.push(UnresolvedReason::LabelNotClosed { class: y, role: r });
                        }
                    }
                }
            }
        }
        reasons
    }

    /// The atomic classes a concept expression directly requires of an
    /// element, or nothing when the expression is not a shape we can label
    /// from.
    ///
    /// `Some(..)` bodies contribute NO classes of their own — an element
    /// standing for `∃u.A` is opaque as a class, and its content is carried
    /// by the edge the caller materialises, not by its label.
    fn required_atoms(pool: &ConceptPool, ce: ConceptId, out: &mut Vec<ClassId>) {
        match pool.get(ce) {
            ConceptExpr::Atomic(c) => out.push(*c),
            ConceptExpr::And(ops) => {
                for op in ops {
                    Self::required_atoms(pool, *op, out);
                }
            }
            _ => {}
        }
    }

    /// Materialises the existential structure of axiom superclass positions.
    ///
    /// The saturator emits no fact for a NESTED existential body and gives its
    /// Tseitin marker an empty subsumer set, so a fact-driven model (`expand`)
    /// has no element for the nested witness at all. This walks the axioms
    /// instead: wherever an element satisfies an axiom's antecedent atoms, the
    /// axiom's consequent existential chain is built out, one element per
    /// body.
    ///
    /// Labels reuse `target_label`, so the `TBox`-closure gap is reported as
    /// `LabelNotClosed` here exactly as it is on the fact path — this adds
    /// reach, not a second labelling policy. Additive: does not touch
    /// `expand`'s edges or elements, only appends to them.
    pub fn expand_from_axioms(
        &mut self,
        internal: &InternalOntology,
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
    ) -> Vec<UnresolvedReason> {
        // (antecedent atoms, consequent concept) pairs, from both axiom shapes.
        let mut rules: Vec<(Vec<ClassId>, ConceptId)> = Vec::new();
        for ax in &internal.axioms {
            match ax {
                Axiom::SubClassOf { sub, sup } => {
                    let mut ante = Vec::new();
                    Self::required_atoms(&internal.concepts, *sub, &mut ante);
                    if !ante.is_empty() {
                        rules.push((ante, *sup));
                    }
                }
                Axiom::EquivalentClasses(members) => {
                    for lhs in members {
                        for rhs in members {
                            if lhs == rhs {
                                continue;
                            }
                            let mut ante = Vec::new();
                            Self::required_atoms(&internal.concepts, *lhs, &mut ante);
                            if !ante.is_empty() {
                                rules.push((ante, *rhs));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut reasons = Vec::new();
        let mut round = 0usize;
        loop {
            let mut grew = false;
            let elems: Vec<Element> = self.elements().collect();
            for e in elems {
                for (ante, sup) in &rules {
                    if !ante.iter().all(|c| self.in_concept(e, *c)) {
                        continue;
                    }
                    if self.materialise_exists(
                        &internal.concepts,
                        subs,
                        eff,
                        bounds,
                        e,
                        *sup,
                        &mut reasons,
                        &mut grew,
                    ) {
                        return reasons; // a bound tripped
                    }
                }
            }
            round += 1;
            if !grew {
                return reasons;
            }
            if round >= bounds.max_rounds {
                reasons.push(UnresolvedReason::BoundTripped {
                    bound: "max_rounds",
                    limit: Some(bounds.max_rounds),
                });
                return reasons;
            }
        }
    }

    /// Builds out every positive `∃` in `ce` starting at `e`. Returns `true`
    /// iff a bound tripped and the caller must stop.
    #[allow(clippy::too_many_arguments)]
    fn materialise_exists(
        &mut self,
        pool: &ConceptPool,
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
        e: Element,
        ce: ConceptId,
        reasons: &mut Vec<UnresolvedReason>,
        grew: &mut bool,
    ) -> bool {
        match pool.get(ce) {
            ConceptExpr::And(ops) => {
                for op in ops {
                    if self.materialise_exists(pool, subs, eff, bounds, e, *op, reasons, grew) {
                        return true;
                    }
                }
                false
            }
            ConceptExpr::Some(role, body) => {
                if role.is_inverse() {
                    return false;
                }
                let r = role.role_id();
                // Label the witness from the body's own required atoms, closed
                // through `target_label` so the range union and the closure
                // report are identical to the fact path.
                let mut atoms = Vec::new();
                Self::required_atoms(pool, *body, &mut atoms);
                let mut label: Vec<ClassId> = Vec::new();
                let mut unclosed = false;
                for a in &atoms {
                    if let Ok(l) = Self::target_label(subs, eff, r, *a) {
                        label.extend(l);
                    } else {
                        // The range augmentation is unclosed and stays
                        // reported below, but the atom's own base closure is
                        // entailed unconditionally by the axiom (the witness
                        // IS an `a`) — dropping it too would be strictly more
                        // lossy than the report requires.
                        label.extend(subs.subsumers_of(*a));
                        unclosed = true;
                    }
                }
                if unclosed {
                    reasons.push(UnresolvedReason::LabelNotClosed {
                        class: *atoms.first().unwrap_or(&ClassId::new(0)),
                        role: r,
                    });
                }
                if atoms.is_empty() {
                    // Opaque body (e.g. a nested `∃`): the witness carries only
                    // the role's effective ranges, and its content comes from
                    // the edges built below.
                    if let Some(rs) = eff.get(&r) {
                        for c in rs {
                            label.extend(subs.subsumers_of(*c));
                        }
                    }
                }
                label.sort_unstable_by_key(|c| c.index());
                label.dedup();
                let before = self.labels.len();
                let w = self.intern(label);
                if self.labels.len() > bounds.max_elements {
                    reasons.push(UnresolvedReason::BoundTripped {
                        bound: "max_elements",
                        limit: Some(bounds.max_elements),
                    });
                    return true;
                }
                if self.labels.len() > before {
                    *grew = true;
                }
                if self.push_edge(r, e, w, bounds, reasons) {
                    return true;
                }
                // Recurse INTO the body at the new witness: this is the hop the
                // fact list omits.
                self.materialise_exists(pool, subs, eff, bounds, w, *body, reasons, grew)
            }
            ConceptExpr::Top
            | ConceptExpr::Bot
            | ConceptExpr::Atomic(_)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_)
            | ConceptExpr::Not(_)
            | ConceptExpr::Or(_)
            | ConceptExpr::All(_, _)
            | ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _) => false,
        }
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
    fn successors(&self, e: Element, r: RoleId) -> Vec<Element> {
        let mut out = Vec::new();
        for s in self.hierarchy_sub_roles(r) {
            if let Some(bucket) = self.edges.get(s.index() as usize) {
                out.extend(bucket.iter().filter(|(f, _)| *f == e).map(|(_, t)| *t));
            }
        }
        out.sort_unstable_by_key(|e| e.index());
        out.dedup();
        out
    }
    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool {
        self.hierarchy_sub_roles(r).iter().any(|s| {
            self.edges
                .get(s.index() as usize)
                .is_some_and(|b| b.contains(&(from, to)))
        })
    }
    fn edges(&self, r: RoleId) -> Vec<(Element, Element)> {
        let mut out = Vec::new();
        for s in self.hierarchy_sub_roles(r) {
            if let Some(bucket) = self.edges.get(s.index() as usize) {
                out.extend_from_slice(bucket);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
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
