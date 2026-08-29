//! The finite model: elements are INTERNED LABEL SETS.
//!
//! Two classes share an element exactly when their subsumer sets coincide,
//! which (because `subsumers_of` is reflexive) happens exactly for
//! derived-equivalent classes.
//!
//! **This model is not proven canonical.** The same logical nested-existential witness can be
//! labelled differently by the fact-driven and axiom-driven expansion paths and get interned as
//! two separate `Element`s, and (see the `ConceptExpr::Some` arm of `materialise_exists` below)
//! an opaque body's witness label can also come out empty or under-labelled relative to the
//! label a `Domain`/nested recursion later hangs an edge off of. See
//! `docs/known-limitations/verify-two-expansion-paths-split-a-witness.md` for the mechanism and
//! three reproduced cases where it causes a spurious `Violated`.

use hashbrown::HashMap;
use owl_dl_core::{
    Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, InternalOntology, RoleHierarchy,
    RoleHierarchyBuilder, RoleId, SubRolePath,
};
use owl_dl_saturation::Subsumers;

/// The deterministic IRI an injected `(y, r)` conjunction class is interned
/// under. Shared by `inject_conjunction` (which creates the class) and
/// `materialise_exists` (which looks it up on a later round, once the
/// ontology carrying it has been re-saturated) so the two can never drift
/// onto different naming schemes.
fn injected_class_iri(y: ClassId, r: RoleId) -> String {
    format!(
        "{}verify-aug:{}:{}",
        owl_dl_core::residual_absorbability::SYNTHETIC_CLASS_IRI_PREFIX,
        y.index(),
        r.index()
    )
}

/// What a prior round's injection did for gap `(a, r)`, looked up by the
/// deterministic IRI `inject_conjunction` interns under.
///
/// Shared by `expand` and `materialise_exists`'s `Err` arms so the two paths
/// cannot disagree about whether a gap is already closed. Review finding
/// (Task 5, round 1): `expand`'s fact-driven path never learned this lookup
/// when it was first added only inside `materialise_exists`, so on any
/// fixture where the FACT path (not the axiom path) hits the closure gap,
/// `expand` re-reported `LabelNotClosed` every round forever — `pending`
/// never emptied, and `inject_conjunction` kept re-pushing an equivalent
/// axiom for the same already-injected `Q`. Fails safe (`BoundTripped`, never
/// a false `Verified`) but defeats convergence. `AND_WRAPPED_NESTED_RANGE`
/// (`tests/model.rs`) is the fixture that exposed it: `expand` alone reaches
/// this gap via the fact path, and no test previously drove `build_model`
/// (as opposed to bare `expand`) on it.
enum InjectedLookup {
    /// No `Q` has been injected for this pair (yet, or ever).
    NotFound,
    /// `Q` was injected and is satisfiable — its own subsumer row is the
    /// correctly-closed label.
    Closed(Vec<ClassId>),
    /// `Q` was injected but turned out unsatisfiable: `a` itself is
    /// genuinely unsatisfiable under the augmented `TBox`.
    RunDelta,
}

fn lookup_injected(
    internal: &InternalOntology,
    subs: &Subsumers,
    a: ClassId,
    r: RoleId,
) -> InjectedLookup {
    match internal.vocabulary.class_id(&injected_class_iri(a, r)) {
        Some(q) if !subs.is_unsatisfiable(q) => InjectedLookup::Closed(subs.subsumers_of(q)),
        Some(_) => InjectedLookup::RunDelta,
        None => InjectedLookup::NotFound,
    }
}

/// Adds `Q ≡ Y ⊓ ⨅aug` to `working`, with an IRI carrying
/// `SYNTHETIC_CLASS_IRI_PREFIX` so reporting filters it.
///
/// A fresh defined class is a conservative extension in the SEMANTIC sense: it
/// cannot make a non-entailment entailed. It is NOT observationally inert on
/// derived output when the engine is incomplete — that is what `RunDelta`
/// records.
pub fn inject_conjunction(
    working: &mut InternalOntology,
    subs: &Subsumers,
    eff: &HashMap<RoleId, Vec<ClassId>>,
    y: ClassId,
    r: RoleId,
) {
    let base = subs.subsumers_of(y);
    let Some(ranges) = eff.get(&r) else { return };
    let mut operands: Vec<ConceptId> = vec![working.concepts.atomic(y)];
    for c in ranges {
        if base
            .binary_search_by_key(&c.index(), |k| k.index())
            .is_err()
        {
            operands.push(working.concepts.atomic(*c));
        }
    }
    if operands.len() < 2 {
        return;
    }
    let iri = injected_class_iri(y, r);
    let q = working.vocabulary.intern_class(&iri);
    let q_expr = working.concepts.atomic(q);
    let conj = working.concepts.and(operands);
    working
        .axioms
        .push(Axiom::EquivalentClasses(vec![q_expr, conj]));
}

use crate::interp::{Element, Interpretation};
use crate::{Bounds, UnresolvedReason};

/// Outcome of [`FiniteModel::push_edge`]. Distinguishing every no-op case
/// from a real append is what lets `close_chains_and_transitivity`'s fixpoint
/// loop terminate correctly: setting its `changed` flag on anything other
/// than `Appended` would risk looping forever on a no-op that never stops
/// being reported as "something changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushOutcome {
    /// The edge was newly appended.
    Appended,
    /// The edge was already present; no-op.
    AlreadyPresent,
    /// `r` is out of range for this model — a caller precondition violation.
    /// Neither in-tree call site can trigger this today (see `push_edge`'s
    /// doc), but a future caller passing a `RoleHierarchy`/`InternalOntology`
    /// pair inconsistent with the one the model was seeded from could.
    NoBucket,
    /// `bounds.max_edges` tripped; a `BoundTripped` reason was already pushed.
    BoundTripped,
}

/// # Every public mutator here is truth-DECREASING
///
/// `intern` only ever ADDS elements (a fresh label is a new obligation the existential/edge
/// checks in `eval.rs` now have to satisfy, never one discharged), and
/// `test_only_remove_from_label`/`test_only_remove_edge` (gated behind the `test-mutations`
/// feature — see this crate's `Cargo.toml`) only ever REMOVE, never add, label content or
/// edges. An under-built model — missing a label entry, missing an edge — can only make an
/// existential/role-hierarchy check FAIL, never spuriously succeed: there is no public
/// operation that can make a check pass by omission. So the entire public surface errs toward
/// `Violated`, never toward a false `Verified`.
///
/// This is why a caller doing `seed(...)` then `verify(...)` without ever calling `expand`/
/// `expand_from_axioms` is safe (an unexpanded model is simply MORE under-built, which by the
/// same argument can only produce MORE `Violated`/`Unresolved`, never a false `Verified`), and
/// why building the whole crate with `--all-features` — which exposes `test-mutations` outside
/// its own test binaries — is benign rather than a soundness hole: the mutator it exposes can
/// only push a model further in the direction this crate already treats as safe.
///
/// **This property is about `FiniteModel`'s own construction, and is orthogonal to the
/// model-BUILDING defects tracked in
/// `docs/known-limitations/verify-two-expansion-paths-split-a-witness.md` (F1/F2/F3).** Those
/// are cases where `build_model` calls `intern`/`push_edge` with a label or edge set that is
/// itself wrong (missing content that SHOULD have been there per another axiom) — the mutators
/// did exactly what they were asked, faithfully, and still produced a spurious `Violated`
/// because what they were asked to add was already incomplete. Truth-decreasing mutators bound
/// the risk from calling them in the wrong ORDER or not at all; they say nothing about whether
/// each call was given the right label to begin with.
///
/// # `still_holds_after` belongs on `VerifiedModel`, never here
///
/// Task 11 adds `still_holds_after`. It must land on `VerifiedModel`
/// (`lib.rs`) — the type-state that proves a model was actually checked
/// against its own ontology — never on `FiniteModel` itself, or a caller
/// could ask the soundness question of a model nobody ever verified. This
/// doctest is a real compile check of that absence, not a string scan: it
/// is expected to FAIL to compile today, and must keep failing until (and
/// unless) this crate legitimately adds the method to `FiniteModel` — which
/// it should not.
///
/// ```compile_fail
/// use owl_dl_verify::model::FiniteModel;
///
/// let m = FiniteModel::default();
/// m.still_holds_after();
/// ```
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

    /// Test-only mutation: removes `c` from element `e`'s label in place.
    ///
    /// Exists so the sabotage matrix (`tests/evaluator.rs`) can manufacture a
    /// specific, pinned violation without going through the builder —
    /// production code must never mutate a model after construction, hence
    /// the cfg gate. `#[cfg(test)]` alone would not reach the separate
    /// `tests/` integration crates, so `Cargo.toml` also requests this
    /// crate's own `test-mutations` feature as a dev-dependency; see that
    /// entry's comment.
    ///
    /// Deliberately does not touch `label_ix`: the reverse lookup for the
    /// PRE-mutation label becomes stale, but nothing re-interns through it
    /// for this element after the mutation, and leaving it stale (rather
    /// than trying to keep it consistent) is what keeps this a one-line,
    /// obviously-correct test seam.
    #[cfg(any(test, feature = "test-mutations"))]
    pub fn test_only_remove_from_label(&mut self, e: Element, c: ClassId) {
        if let Some(slot) = self.labels.get_mut(e.index() as usize) {
            let mut retained: Vec<ClassId> = slot.iter().copied().filter(|&x| x != c).collect();
            retained.sort_unstable_by_key(|k| k.index());
            *slot = retained.into_boxed_slice();
        }
    }

    /// Test-only mutation: removes the edge `(from, to)` from role `r`'s OWN
    /// declared bucket in place (never a sub-role's bucket — the caller picks
    /// which one).
    ///
    /// Exists for the same reason as `test_only_remove_from_label` (see its
    /// doc): the `RBox` sabotage matrix (`tests/evaluator.rs`) needs to
    /// manufacture a specific, pinned edge-level violation without going
    /// through the builder. Gated identically.
    ///
    /// This can only ever break a check that reads role `r`'s bucket WITHOUT
    /// that same read also being how the check's own antecedent was
    /// populated — e.g. a chain- or transitivity-COMPOSED edge, which lives
    /// solely in the composed role's bucket, distinct from the leg roles'
    /// buckets the antecedent scan reads. It is a deliberate no-op for
    /// `SubObjectPropertyOf(Role)` / `EquivalentObjectProperties`: there, the
    /// target role's `has_edge` already unions in the very sub-role bucket
    /// the antecedent iterates (that union is exactly what
    /// `build_role_hierarchy` built FROM the axiom under test), so deleting
    /// the edge from either bucket removes the antecedent along with it and
    /// the check reads vacuously `Holds` either way — see `eval.rs`'s doc on
    /// those two arms for the full argument.
    #[cfg(any(test, feature = "test-mutations"))]
    pub fn test_only_remove_edge(&mut self, r: RoleId, from: Element, to: Element) {
        if let Some(bucket) = self.edges.get_mut(r.index() as usize) {
            bucket.retain(|&(f, t)| (f, t) != (from, to));
        }
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
    /// Returns a [`PushOutcome`] rather than a bare `bool` so a caller that
    /// runs its own fixpoint (`close_chains_and_transitivity`) can tell a real
    /// append apart from every kind of no-op — including `NoBucket`, which
    /// fires only if `r` is out of range for this model (a caller precondition
    /// violation, not something either in-tree call site can trigger today).
    /// Folding `NoBucket` into `AlreadyPresent`-style silence, as the `bool`
    /// version did, is exactly what let a future out-of-range `r` turn a
    /// fixpoint loop into an infinite one: `changed` would be set on a no-op
    /// forever instead of on nothing.
    fn push_edge(
        &mut self,
        r: RoleId,
        from: Element,
        to: Element,
        bounds: &Bounds,
        reasons: &mut Vec<UnresolvedReason>,
    ) -> PushOutcome {
        let Some(bucket) = self.edges.get_mut(r.index() as usize) else {
            return PushOutcome::NoBucket;
        };
        if bucket.contains(&(from, to)) {
            return PushOutcome::AlreadyPresent;
        }
        bucket.push((from, to));
        self.edge_count += 1;
        if self.edge_count > bounds.max_edges {
            reasons.push(UnresolvedReason::BoundTripped {
                bound: "max_edges",
                limit: Some(bounds.max_edges),
            });
            return PushOutcome::BoundTripped;
        }
        PushOutcome::Appended
    }

    /// Expands seeded elements into a graph: every existential fact `(x, r, y)`
    /// on a labelled element becomes an `r`-edge to the interned target label,
    /// iterated to a fixpoint (or a tripped bound).
    ///
    /// A fact whose target label would need `TBox` closure (`target_label`
    /// returning `Err`) first checks `lookup_injected` for a prior round's
    /// injected `Q` (shared with `materialise_exists` — see its doc) before
    /// falling back to reporting `LabelNotClosed`.
    pub fn expand(
        &mut self,
        internal: &InternalOntology,
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
                    let label = match Self::target_label(subs, eff, r, y) {
                        Ok(label) => label,
                        Err(_aug) => match lookup_injected(internal, subs, y, r) {
                            InjectedLookup::Closed(label) => label,
                            InjectedLookup::RunDelta => {
                                reasons.push(UnresolvedReason::RunDelta { class: y });
                                subs.subsumers_of(y)
                            }
                            InjectedLookup::NotFound => {
                                reasons
                                    .push(UnresolvedReason::LabelNotClosed { class: y, role: r });
                                continue;
                            }
                        },
                    };
                    let before = self.labels.len();
                    let t = self.intern(label);
                    if self.labels.len() > bounds.max_elements {
                        reasons.push(UnresolvedReason::BoundTripped {
                            bound: "max_elements",
                            limit: Some(bounds.max_elements),
                        });
                        return reasons;
                    }
                    if self.push_edge(r, e, t, bounds, &mut reasons) == PushOutcome::BoundTripped {
                        return reasons;
                    }
                    if self.labels.len() > before {
                        queue.push(t);
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
                        internal,
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
        internal: &InternalOntology,
        subs: &Subsumers,
        eff: &HashMap<RoleId, Vec<ClassId>>,
        bounds: &Bounds,
        e: Element,
        ce: ConceptId,
        reasons: &mut Vec<UnresolvedReason>,
        grew: &mut bool,
    ) -> bool {
        let pool = &internal.concepts;
        match pool.get(ce) {
            ConceptExpr::And(ops) => {
                for op in ops {
                    if self.materialise_exists(internal, subs, eff, bounds, e, *op, reasons, grew) {
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
                    match Self::target_label(subs, eff, r, *a) {
                        Ok(l) => label.extend(l),
                        Err(_aug) => {
                            // A prior round may have injected `Q ≡ a ⊓ aug` for
                            // exactly this (class, role) pair. `lookup_injected`
                            // is shared with `expand`'s Err arm so the two
                            // paths cannot disagree about whether the gap is
                            // already closed.
                            match lookup_injected(internal, subs, *a, r) {
                                InjectedLookup::Closed(l) => label.extend(l),
                                InjectedLookup::RunDelta => {
                                    reasons.push(UnresolvedReason::RunDelta { class: *a });
                                    label.extend(subs.subsumers_of(*a));
                                }
                                InjectedLookup::NotFound => {
                                    // The range augmentation is unclosed and
                                    // stays reported below, but the atom's own
                                    // base closure is entailed unconditionally
                                    // by the axiom (the witness IS an `a`) —
                                    // dropping it too would be strictly more
                                    // lossy than the report requires.
                                    label.extend(subs.subsumers_of(*a));
                                    unclosed = true;
                                }
                            }
                        }
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
                    //
                    // KNOWN LIMITATION: `eff.get(&r)` is frequently EMPTY, while
                    // `expand`'s fact path labels this same logical witness from
                    // `subsumers_of(Tseitin Q)` (never empty — it contains at
                    // least `{Q}`). Because `intern` dedups purely by label
                    // CONTENT, the two paths can allocate two different
                    // `Element`s for one nested-existential witness — one
                    // under-labelled here, one correctly labelled by `expand`.
                    //
                    // NOT edge-less: `push_edge` a few lines below gives `w` an
                    // incoming edge unconditionally, and the recursive
                    // `materialise_exists` call right after that can give it an
                    // OUTGOING edge too. An under-labelled `w` sitting at the
                    // source of a `Domain`-constrained edge is exactly how F3 in
                    // `docs/known-limitations/verify-two-expansion-paths-split-a-witness.md`
                    // reproduces a false `Violated` — read that doc before
                    // trusting this element's label anywhere new.
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
                if self.push_edge(r, e, w, bounds, reasons) == PushOutcome::BoundTripped {
                    return true;
                }
                // Recurse INTO the body at the new witness: this is the hop the
                // fact list omits.
                self.materialise_exists(internal, subs, eff, bounds, w, *body, reasons, grew)
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

    /// Materialises chain and transitive edges to a fixpoint, writing to the
    /// DECLARED role's vector and reading via `has_edge` (sub-role aware).
    ///
    /// Sub-role inclusion itself is never materialised: it is a lookup, whereas
    /// chains and transitivity generate NEW pairs.
    ///
    /// # Soundness
    ///
    /// This closure is label-blind by construction: it only composes edges
    /// between elements that already exist and never allocates a label. That
    /// is sound because `effective_ranges` is super-role-closed, so `s ⊑ b`
    /// implies `eff_ranges(s) ⊇ eff_ranges(b)`; every edge under `s` was
    /// created after checking `eff_ranges(s)` against the target's label; and
    /// `chain_range_out_of_profile` guarantees `eff_ranges(v) ⊆ eff_ranges(u)`
    /// before any `Chain(t,u) ⊑ v` materialisation is permitted here (and the
    /// `TransitiveRole` rule composes `v` with itself, so the same containment
    /// holds trivially). So a composed target already satisfies whatever `v`
    /// would require of it.
    ///
    /// # Panics / termination precondition
    ///
    /// Termination of the `while changed` loop below relies on `push_edge`
    /// reporting `Appended` only for a genuine append (see `PushOutcome`'s
    /// doc) — a caller passing an `internal` inconsistent with the one this
    /// model was `seed`ed from could in principle name a `RoleId` this
    /// model's `edges` vector has no bucket for, which `push_edge` reports as
    /// `NoBucket` rather than `Appended`, so it can never spuriously keep
    /// `changed` true.
    pub fn close_chains_and_transitivity(
        &mut self,
        internal: &InternalOntology,
        bounds: &Bounds,
    ) -> Vec<UnresolvedReason> {
        let mut rules: Vec<(RoleId, RoleId, RoleId)> = Vec::new();
        for ax in &internal.axioms {
            match ax {
                Axiom::SubObjectPropertyOf {
                    sub: SubRolePath::Chain(parts),
                    sup,
                } if !sup.is_inverse() => {
                    if let [a, b] = parts.as_slice()
                        && !a.is_inverse()
                        && !b.is_inverse()
                    {
                        rules.push((a.role_id(), b.role_id(), sup.role_id()));
                    }
                }
                Axiom::TransitiveRole(r) if !r.is_inverse() => {
                    rules.push((r.role_id(), r.role_id(), r.role_id()));
                }
                _ => {}
            }
        }
        let mut reasons = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b, v) in &rules {
                for (x, y) in self.edges(a) {
                    for (y2, z) in self.edges(b) {
                        if y != y2 || self.has_edge(x, v, z) {
                            continue;
                        }
                        match self.push_edge(v, x, z, bounds, &mut reasons) {
                            PushOutcome::BoundTripped => return reasons,
                            PushOutcome::Appended => changed = true,
                            PushOutcome::AlreadyPresent | PushOutcome::NoBucket => {}
                        }
                    }
                }
            }
        }
        reasons
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

/// The OWL 2 EL profile forbids a range on a property implied by a chain
/// (Baader–Brandt–Lutz 2008), precisely because the unrestricted combination
/// breaks the canonical-model technique. `is_el_axiom` admits the two
/// constructs INDEPENDENTLY, so rustdl accepts the combination — see issue #82.
///
/// Refuse iff some admitted 2-leg chain `Chain(t,u) ⊑ v` has
/// `eff_ranges(v) ⊄ eff_ranges(u)`.
///
/// `eff_ranges` MUST be the super-role-closed set, never the declared ranges
/// alone: measured over the 1,920-ontology ORE pool, the precise predicate fires
/// on 61 ontologies and **44 of those only via a super-role of the chain head**.
/// Reading it as declared-only therefore misses the MAJORITY case, and that miss
/// is a false `Verified` (the evaluator reads ranges per declared-role edge
/// vector), not a false `Violated`.
///
/// `TransitiveRole` is exempt for a DIFFERENT reason than the `⊆` test:
/// `TransitiveObjectProperty` lowers to `Axiom::TransitiveRole`, a distinct
/// variant this loop never matches at all — it is exempt by never being
/// inspected here, not because the containment check passes on it.
///
/// The self-chain spelling `Chain(r,r) ⊑ r`, by contrast, IS inspected, and is
/// the case the `⊆ eff_ranges(u)` test actually exempts by construction
/// (`u == sup == r`, so `head == second` trivially).
#[must_use]
pub fn chain_range_out_of_profile(
    internal: &InternalOntology,
    h: &RoleHierarchy,
) -> Option<RoleId> {
    let eff = effective_ranges(internal, h);
    for ax in &internal.axioms {
        if let Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(parts),
            sup,
        } = ax
        {
            let [_t, u] = parts.as_slice() else { continue };
            if sup.is_inverse() || u.is_inverse() {
                continue;
            }
            let head = eff.get(&sup.role_id()).cloned().unwrap_or_default();
            let second = eff.get(&u.role_id()).cloned().unwrap_or_default();
            if !head.iter().all(|c| second.contains(c)) {
                return Some(sup.role_id());
            }
        }
    }
    None
}
