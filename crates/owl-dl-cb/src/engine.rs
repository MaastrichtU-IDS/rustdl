//! The consequence-based ALCH calculus: context saturation (Task B).
//!
//! This is a faithful implementation of the consequence-based classification
//! procedure for ALCH (Simančík–Kazakov–Horrocks, IJCAI 2011, "Consequence-Based
//! Reasoning beyond Horn Ontologies"), in the **context-decomposed** form of
//! §5 of that paper (and the later Bate-et-al / Tena-Cucala SROIQ context
//! calculus, restricted to ALCH). Each *context* reasons about a hypothetical
//! element whose `core` conjunction of atoms holds; derived clauses are the
//! disjunctions `core ⊑ ⊔(literals)` entailed at that element. Existentials
//! create successor contexts (reused by core — the termination key); `∀` and
//! the complement-encoded left-existentials propagate information back to
//! predecessors.
//!
//! ## Clause form
//! A *derived clause* in a context is a disjunction of literals `⊔ⱼ Lⱼ`, where
//! each literal is atomic `B`, `∃R.B`, or `∀R.B` (B atomic). It is understood as
//! `core ⊑ ⊔ⱼ Lⱼ` — i.e. *given the core conjunction, one of the disjuncts
//! holds*. The empty disjunction is `core ⊑ ⊥` (the core is unsatisfiable).
//! (The frozen [`DerivedClause`] carries a `premise` field; the context-internal
//! clauses are conditioned on the whole core, so `premise` stays empty — the
//! core itself is the conjunctive antecedent.)
//!
//! ## Inference rules (context form)
//! 1. **Init** — every core atom `A` is a derived unit clause `{A}`.
//! 2. **Hyper (hyperresolution, SKH `Rⁿ⊓`)** — an ontology clause
//!    `⊓ᵢ Pᵢ ⊑ ⊔ M` fires when each premise atom `Pᵢ` occurs in *some* derived
//!    clause `Nᵢ ⊔ {Pᵢ}`; it derives `(⋃ᵢ Nᵢ) ⊔ M`. This single rule realizes
//!    hyperresolution, disjunctive case-splitting, `R⁻A` (resolution against a
//!    disjointness clause `{A,X} ⊑ ⊥`), and the `⊥` rule (`M` empty). The
//!    resolution is **unordered** — see `apply_hyper` for why (the ordering
//!    restriction preserves only refutational completeness, which needs the
//!    goal-directed `A ⊓ ¬B` seeds the frozen model does not have).
//! 3. **Succ (`R⁺∃`)** — every `∃R.B` literal in a derived clause `N ⊔ {∃R.B}`
//!    spawns/links a successor context whose core contains `B`; the edge carries
//!    the residual `N`.
//! 4. **∀-prop (`R∀`)** — a `∀S.B` literal in a clause `N ⊔ {∀S.B}` at `v` with
//!    an edge `v —R→ u`, `R ⊑* S`, adds a *new* edge `v —R→ (core(u) ∪ {B})`
//!    with residual `edge_res ⊔ N`. The shared successor `u` is **never
//!    mutated** (that would corrupt other predecessors — unsound). `R∀`
//!    compounds across the fixpoint, accumulating multiple universals.
//! 5. **⊥-back-prop (`R⁻∃` via complement encoding, `R∀` residual)** — when a
//!    successor `u` of `v` (edge residual `N`, role `R`) derives the empty
//!    clause (`u ⊑ ⊥`), `v` derives the residual `N` (the disjunction; `⊥`
//!    itself only when `N` is empty — the soundness landmine). The positive
//!    consequences of left-existentials (`∃R.A ⊑ B`) are realized through this:
//!    Task A encodes `∃R.A ⊑ B` as `⊤ ⊑ B ⊔ ∀R.X` + `{A,X} ⊑ ⊥`, so `R∀` forms
//!    the augmented context `core(u) ∪ {X}`, its `A`/`X` clash gives `⊥`, and
//!    the residual `{B}` reflects to `v`.
//! 6. **Read-off** — `A ⊑ B` iff the root context of `A` derives the *singleton*
//!    unit clause `{B}`; unsat iff it derives the empty clause.

use crate::model::{Atom, Context, ContextGraph, ContextId, DerivedClause, Literal};
use crate::normalize::Normalized;
use owl_dl_core::ir::{ConceptExpr, ConceptId, Role};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A head disjunction of literals, sorted+deduped. Empty = `⊥`.
type Clause = Vec<Literal>;

/// Engine-internal mutable state layered over the frozen [`ContextGraph`].
struct Engine<'a> {
    norm: &'a Normalized,
    graph: ContextGraph,
    /// Predecessors of each context: `(pred_context, role, residual)`.
    /// The residual is the disjunction `N` from the spawning clause `N ⊔ ∃R.B`.
    preds: Vec<Vec<(ContextId, Role, Clause)>>,
    /// Worklist of contexts whose clause set changed and must be (re)processed.
    dirty: VecDeque<ContextId>,
    /// Membership guard so a context is enqueued at most once at a time.
    in_queue: Vec<bool>,
}

impl<'a> Engine<'a> {
    fn new(norm: &'a Normalized) -> Self {
        Self {
            norm,
            graph: ContextGraph::default(),
            preds: Vec::new(),
            dirty: VecDeque::new(),
            in_queue: Vec::new(),
        }
    }

    /// Find-or-create the context whose core is exactly `core`. Seeds the core
    /// atoms as unit clauses and enqueues it.
    fn intern_context(&mut self, core: BTreeSet<Atom>) -> ContextId {
        if let Some(&id) = self.graph.by_core.get(&core) {
            return id;
        }
        let id = self.graph.contexts.len();
        let mut ctx = Context {
            core: core.clone(),
            ..Context::default()
        };
        // Init rule: each core atom is a unit clause.
        for &a in &core {
            let dc = DerivedClause {
                premise: Vec::new(),
                head: vec![a],
            };
            ctx.seen.insert(dc.clone());
            ctx.clauses.push(dc);
        }
        self.graph.contexts.push(ctx);
        self.graph.by_core.insert(core, id);
        self.preds.push(Vec::new());
        self.in_queue.push(false);
        self.enqueue(id);
        id
    }

    fn enqueue(&mut self, v: ContextId) {
        if !self.in_queue[v] {
            self.in_queue[v] = true;
            self.dirty.push_back(v);
        }
    }

    /// Add a derived clause `head` (sorted+deduped) to context `v`. Returns
    /// `true` if it was new. Enqueues `v` and its predecessors on change.
    fn add_clause(&mut self, v: ContextId, mut head: Clause) -> bool {
        head.sort_unstable();
        head.dedup();
        let dc = DerivedClause {
            premise: Vec::new(),
            head,
        };
        if self.graph.contexts[v].seen.contains(&dc) {
            return false;
        }
        self.graph.contexts[v].seen.insert(dc.clone());
        self.graph.contexts[v].clauses.push(dc);
        self.enqueue(v);
        // Predecessors may now be able to back-propagate.
        let preds: Vec<ContextId> = self.preds[v].iter().map(|(p, _, _)| *p).collect();
        for p in preds {
            self.enqueue(p);
        }
        true
    }

    /// Literal classification helpers (frozen `Literal = ConceptId`).
    fn is_atomic(&self, l: Literal) -> bool {
        matches!(
            self.norm.pool.get(l),
            ConceptExpr::Atomic(_) | ConceptExpr::Top
        )
    }

    fn as_some(&self, l: Literal) -> Option<(Role, ConceptId)> {
        match self.norm.pool.get(l) {
            ConceptExpr::Some(r, c) => Some((*r, *c)),
            _ => None,
        }
    }

    fn as_all(&self, l: Literal) -> Option<(Role, ConceptId)> {
        match self.norm.pool.get(l) {
            ConceptExpr::All(r, c) => Some((*r, *c)),
            _ => None,
        }
    }

    /// `R ⊑* S` under the (reflexive-transitive closure of the) role hierarchy.
    fn role_subsumed(&self, sub: Role, sup: Role) -> bool {
        if sub == sup {
            return true;
        }
        // BFS over role_hierarchy edges (sub ⊑ sup).
        let mut frontier = vec![sub];
        let mut seen: BTreeSet<Role> = BTreeSet::new();
        seen.insert(sub);
        while let Some(r) = frontier.pop() {
            for &(a, b) in &self.norm.role_hierarchy {
                if a == r && seen.insert(b) {
                    if b == sup {
                        return true;
                    }
                    frontier.push(b);
                }
            }
        }
        false
    }

    /// Saturate to fixpoint.
    fn run(&mut self) {
        // Seed root contexts: one per reportable class.
        let mut pool_classes: Vec<ConceptId> = Vec::new();
        for &c in &self.norm.classes {
            // Intern the atomic concept id for this class.
            // The pool already contains it (normalize interns all classes).
            pool_classes.push(self.atom_of_class(c));
        }
        for a in pool_classes {
            let mut core = BTreeSet::new();
            core.insert(a);
            self.intern_context(core);
        }

        while let Some(v) = self.dirty.pop_front() {
            self.in_queue[v] = false;
            self.process(v);
        }
    }

    /// Resolve a `ClassId` to its interned `Atomic` `ConceptId` in the pool.
    fn atom_of_class(&self, c: owl_dl_core::ir::ClassId) -> ConceptId {
        // The pool is read-only here; the class atom must already be interned.
        // Linear scan is acceptable (done once per class at seed time).
        for (id, e) in self.norm.pool.iter_with_ids() {
            if let ConceptExpr::Atomic(cc) = e
                && *cc == c
            {
                return id;
            }
        }
        // Fallback: should not happen for reportable classes.
        unreachable!("reportable class atom not interned in pool");
    }

    /// Process all rules for context `v` over its current clause set.
    fn process(&mut self, v: ContextId) {
        // 1+2. Hyper (ordered hyperresolution) over ontology clauses.
        self.apply_hyper(v);
        // 3+4. Successor creation + ∀-propagation.
        self.apply_succ_and_forall(v);
        // 5. Back-propagation from successors.
        self.apply_back_prop(v);
    }

    /// Rule 2: hyperresolution (SKH Table 3, `Rⁿ⊓` generalized to disjunctions).
    ///
    /// For each ontology clause `⊓ᵢ Pᵢ ⊑ ⊔ M`, if each premise atom `Pᵢ` occurs
    /// in some derived clause `Nᵢ ⊔ {Pᵢ}` (with `Pᵢ` resolved away), derive
    /// `(⋃ᵢ Nᵢ) ⊔ M`. This is the *unordered* form — it resolves on **any**
    /// occurrence of `Pᵢ`, not only the maximal one.
    ///
    /// **Why unordered.** The ordering restriction (SKH Remark 5) preserves only
    /// *refutational* completeness, which the §5.1 procedure exploits by seeding
    /// the goal-directed contexts `H = A ⊓ ¬B` and deriving `⊥`. The frozen
    /// model seeds exactly one root context per class (`core = {A}`) and reads
    /// the *positive* hierarchy directly, so it must rely on the *direct*
    /// completeness of the full (unordered) Table 3, where `O ⊢ H ⊑ A` for
    /// every entailed atomic subsumption. Unordered resolution still terminates:
    /// derived clauses are disjunctions over the finite literal vocabulary, so
    /// each context has finitely many (≤ `2^vocab`) clauses (the `ExpTime`
    /// bound).
    fn apply_hyper(&mut self, v: ContextId) {
        // Snapshot the current clauses.
        let clauses: Vec<Clause> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|dc| dc.head.clone())
            .collect();

        // Index: for each atom `p`, the residuals `N` of every derived clause
        // `N ⊔ {p}` (p removed) — *any* occurrence, not only the maximal one.
        let mut by_atom: BTreeMap<ConceptId, Vec<Clause>> = BTreeMap::new();
        for c in &clauses {
            for (i, &lit) in c.iter().enumerate() {
                if self.is_atomic(lit) {
                    let mut residual = c.clone();
                    residual.remove(i);
                    by_atom.entry(lit).or_default().push(residual);
                }
            }
        }

        let mut new_clauses: Vec<Clause> = Vec::new();
        let ont_clauses = self.norm.clauses.clone();
        for oc in &ont_clauses {
            // Build the cartesian product of premise-supporting clauses.
            // Empty premise ⇒ single empty combination (derive M directly).
            let mut combos: Vec<Clause> = vec![Vec::new()];
            let mut ok = true;
            for &p in &oc.premise {
                // `Top` premise atoms are vacuously satisfied.
                if matches!(self.norm.pool.get(p), ConceptExpr::Top) {
                    continue;
                }
                let supports = match by_atom.get(&p) {
                    Some(s) if !s.is_empty() => s,
                    _ => {
                        ok = false;
                        break;
                    }
                };
                let mut next: Vec<Clause> = Vec::new();
                for base in &combos {
                    for sup in supports {
                        let mut merged = base.clone();
                        merged.extend_from_slice(sup);
                        next.push(merged);
                    }
                }
                combos = next;
            }
            if !ok {
                continue;
            }
            for base in combos {
                let mut head = base;
                head.extend_from_slice(&oc.head);
                new_clauses.push(head);
            }
        }
        for h in new_clauses {
            self.add_clause(v, h);
        }
    }

    /// Rule 3 (`R⁺∃` / Succ) + Rule 4 (`R∀`, the back-propagation enabler).
    ///
    /// **Succ.** Every `∃R.B` literal in any derived clause `N ⊔ {∃R.B}` spawns
    /// (or links to) a successor context whose core contains `B`. The edge
    /// `v —R→ u` is tagged with the residual disjunction `N` (the rest of the
    /// clause), which rides the edge for back-propagation.
    ///
    /// **R∀ (find-or-create, never mutate).** SKH Table 3 `R∀`:
    /// `H ⊑ M ⊔ ∃R.K`, `H ⊑ N ⊔ ∀S.B`, `R ⊑* S`
    /// ⟹ `H ⊑ M ⊔ N ⊔ ∃R.(K ⊓ B)`. In the context decomposition the combined
    /// existential `∃R.(K ⊓ B)` is an **edge to the augmented successor**
    /// `core(u) ∪ {B}` carrying residual `M ⊔ N`. The shared successor `u` is
    /// never mutated (that would corrupt other predecessors — unsound); we
    /// find-or-create the augmented context and add a *new* edge. Finitely many
    /// cores ⇒ this still terminates. `R∀` compounds across the fixpoint: the
    /// augmented edge is itself eligible for further `R∀` applications, so
    /// multiple universals accumulate (`{C,X₁}` → `{C,X₁,X₂}` …).
    fn apply_succ_and_forall(&mut self, v: ContextId) {
        let clauses: Vec<Clause> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|dc| dc.head.clone())
            .collect();

        // ── Succ: every ∃R.B literal in every clause. ──
        let mut existentials: Vec<(Role, ConceptId, Clause)> = Vec::new();
        for c in &clauses {
            for (i, &lit) in c.iter().enumerate() {
                if let Some((r, bfill)) = self.as_some(lit) {
                    let mut residual = c.clone();
                    residual.remove(i);
                    existentials.push((r, bfill, residual));
                }
            }
        }
        for (r, b, residual) in existentials {
            let mut core: BTreeSet<Atom> = BTreeSet::new();
            match self.norm.pool.get(b) {
                ConceptExpr::Atomic(_) => {
                    core.insert(b);
                }
                ConceptExpr::Top => {} // ∃R.⊤ — successor core is just ⊤.
                _ => continue,         // non-atomic filler shouldn't occur post-norm.
            }
            self.link_successor(v, r, core, residual);
        }

        // ── R∀: for each edge v —R→ u (residual M) and each derived clause
        // `N ⊔ {∀S.B}` with R ⊑* S, add edge v —R→ (core(u) ∪ {B}) with
        // residual M ⊔ N. ──
        let edges: Vec<(Role, ContextId, Clause)> = self.preds_of_v_edges(v);
        // Collect (role S, filler B, residual N) for every ∀ literal occurrence.
        let mut foralls: Vec<(Role, ConceptId, Clause)> = Vec::new();
        for c in &clauses {
            for (i, &lit) in c.iter().enumerate() {
                if let Some((s, bb)) = self.as_all(lit)
                    && matches!(self.norm.pool.get(bb), ConceptExpr::Atomic(_))
                {
                    let mut residual = c.clone();
                    residual.remove(i);
                    foralls.push((s, bb, residual));
                }
            }
        }
        for (r, u, edge_res) in &edges {
            for (s, bb, fa_res) in &foralls {
                if !self.role_subsumed(*r, *s) {
                    continue;
                }
                let mut new_core = self.graph.contexts[*u].core.clone();
                if !new_core.insert(*bb) {
                    continue; // B already in successor core — nothing new.
                }
                let mut new_res = edge_res.clone();
                new_res.extend_from_slice(fa_res);
                new_res.sort_unstable();
                new_res.dedup();
                self.link_successor(v, *r, new_core, new_res);
            }
        }
    }

    /// The set of outgoing edges of `v`: `(role, successor, residual)`.
    fn preds_of_v_edges(&self, v: ContextId) -> Vec<(Role, ContextId, Clause)> {
        let mut out = Vec::new();
        for (r, u) in &self.graph.contexts[v].succ {
            for (p, role, res) in &self.preds[*u] {
                if *p == v && role == r {
                    out.push((*r, *u, res.clone()));
                }
            }
        }
        out
    }

    /// Find-or-create the successor with the given `core` and record the edge
    /// `v —R→ u` with `residual` (deduped), if not already present.
    fn link_successor(
        &mut self,
        v: ContextId,
        r: Role,
        core: BTreeSet<Atom>,
        mut residual: Clause,
    ) {
        residual.sort_unstable();
        residual.dedup();
        let u = self.intern_context(core);
        let edge_exists = self.preds[u]
            .iter()
            .any(|(p, role, res)| *p == v && *role == r && res == &residual);
        if !edge_exists {
            self.preds[u].push((v, r, residual));
            self.graph.contexts[v].succ.push((r, u));
            self.enqueue(u);
            self.enqueue(v);
        }
    }

    /// Rule 5: back-propagation from successors of `v`.
    ///
    /// For each successor edge `v —R→ u` (residual `N`):
    /// - If `u` derives the empty clause (`u ⊑ ⊥`), then `v ⊑ N` (the residual
    ///   disjunction). When `N` is empty this is `v ⊑ ⊥`.
    /// - For every ontology clause encoding a left-existential `∃S.A ⊑ ⊔M`
    ///   (carried as the `∀S.X`/complement clauses), if `u` derives a clause
    ///   `D ⊔ {A}` with `A` maximal and `R ⊑* S`, then `v` derives
    ///   `N ⊔ M ⊔ (back-projected D)`. In B1, the complement encoding routes
    ///   this through ordinary atomic clauses, so the *atomic* consequences of
    ///   `u` that are forced by every model (unit clauses `{A}` with `A`
    ///   atomic) combine with the `∀`-machinery; the residual-`N` reflection
    ///   below is the general (disjunctive) case.
    fn apply_back_prop(&mut self, v: ContextId) {
        let succ = self.graph.contexts[v].succ.clone();
        for (r, u) in succ {
            // Gather the residual(s) for this (v -> u, R) edge.
            let residuals: Vec<Clause> = self.preds[u]
                .iter()
                .filter(|(p, role, _)| *p == v && *role == r)
                .map(|(_, _, res)| res.clone())
                .collect();
            if residuals.is_empty() {
                continue;
            }
            // ⊥-back-prop: if u derives the empty clause, reflect each residual.
            let u_has_bot = self.graph.contexts[u]
                .clauses
                .iter()
                .any(|dc| dc.head.is_empty());
            if u_has_bot {
                for residual in &residuals {
                    self.add_clause(v, residual.clone());
                }
            }
        }
    }
}

/// Saturate the context graph under the consequence-based ALCH inference rules
/// (core resolution, ordered `⊔` resolution, `∃`-Succ, `∀`-Pred, `⊥`) to a
/// fixpoint.
#[must_use]
pub fn saturate(norm: &Normalized) -> ContextGraph {
    let mut eng = Engine::new(norm);
    eng.run();
    eng.graph
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)] // DL canaries: A, B, C … are concepts.
mod tests {
    use super::*;
    use crate::CbHierarchy;
    use crate::classify::read_hierarchy;
    use crate::model::OntClause;
    use owl_dl_core::ir::{ClassId, ConceptPool, RoleId};

    /// Builder for a hand-constructed `Normalized` (does NOT depend on Task A).
    struct B {
        pool: ConceptPool,
        clauses: Vec<OntClause>,
        classes: Vec<ClassId>,
        role_hierarchy: Vec<(Role, Role)>,
    }

    impl B {
        fn new() -> Self {
            Self {
                pool: ConceptPool::new(),
                clauses: Vec::new(),
                classes: Vec::new(),
                role_hierarchy: Vec::new(),
            }
        }
        fn class(&mut self, n: u32) -> ClassId {
            let c = ClassId::new(n);
            let _ = self.pool.atomic(c);
            if !self.classes.contains(&c) {
                self.classes.push(c);
            }
            c
        }
        fn atom(&mut self, c: ClassId) -> ConceptId {
            self.pool.atomic(c)
        }
        fn role(n: u32) -> Role {
            Role::named(RoleId::new(n))
        }
        /// `⊓premise ⊑ ⊔head` from raw `ConceptId`s.
        fn clause(&mut self, premise: Vec<ConceptId>, head: Vec<ConceptId>) {
            self.clauses.push(OntClause { premise, head });
        }
        fn some(&mut self, r: Role, c: ConceptId) -> ConceptId {
            self.pool.some(r, c)
        }
        fn all(&mut self, r: Role, c: ConceptId) -> ConceptId {
            self.pool.all(r, c)
        }
        fn role_incl(&mut self, sub: Role, sup: Role) {
            self.role_hierarchy.push((sub, sup));
        }
        fn finish(self) -> Normalized {
            Normalized {
                clauses: self.clauses,
                classes: self.classes,
                role_hierarchy: self.role_hierarchy,
                pool: self.pool,
            }
        }
    }

    fn classify_built(norm: &Normalized) -> CbHierarchy {
        let graph = saturate(norm);
        read_hierarchy(norm, &graph)
    }

    fn subsumes(h: &CbHierarchy, sub: u32, sup: u32) -> bool {
        h.subsumptions
            .contains(&(ClassId::new(sub), ClassId::new(sup)))
    }

    // ── Headline: disjunctive subsumption (EL saturator CANNOT do this) ──
    // A ⊑ B ⊔ C, B ⊑ D, C ⊑ D  ⟹  A ⊑ D
    #[test]
    fn disjunctive_subsumption_by_cases() {
        let mut b = B::new();
        let (a, bb, c, d) = (b.class(0), b.class(1), b.class(2), b.class(3));
        let (ea, eb, ec, ed) = (b.atom(a), b.atom(bb), b.atom(c), b.atom(d));
        b.clause(vec![ea], vec![eb, ec]); // A ⊑ B ⊔ C
        b.clause(vec![eb], vec![ed]); // B ⊑ D
        b.clause(vec![ec], vec![ed]); // C ⊑ D
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 3), "A ⊑ D by reasoning-by-cases");
    }

    // FP guard: A ⊑ B ⊔ C alone does NOT give A ⊑ B (nor A ⊑ C).
    #[test]
    fn disjunction_alone_no_unit_subsumption() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        b.clause(vec![ea], vec![eb, ec]); // A ⊑ B ⊔ C
        let h = classify_built(&b.finish());
        assert!(!subsumes(&h, 0, 1), "A ⊑ B must NOT be derived");
        assert!(!subsumes(&h, 0, 2), "A ⊑ C must NOT be derived");
    }

    // ∀ + ∃ + ¬ clash → unsat: A ⊑ ∀R.B, A ⊑ ∃R.C, C ⊓ B ⊑ ⊥  ⟹  A ⊑ ⊥
    #[test]
    fn forall_exists_clash_unsat() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let all_rb = b.all(r, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_rb]); // A ⊑ ∀R.B
        b.clause(vec![ea], vec![some_rc]); // A ⊑ ∃R.C
        b.clause(vec![ec, eb], vec![]); // C ⊓ B ⊑ ⊥
        let h = classify_built(&b.finish());
        assert!(h.unsat.contains(&ClassId::new(0)), "A unsat via ∀+∃+clash");
    }

    // ⊥ up ∃: A ⊑ ∃R.C, C ⊑ ⊥  ⟹  A ⊑ ⊥
    #[test]
    fn bot_propagates_up_existential() {
        let mut b = B::new();
        let (a, c) = (b.class(0), b.class(1));
        let (ea, ec) = (b.atom(a), b.atom(c));
        let r = B::role(0);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![some_rc]); // A ⊑ ∃R.C
        b.clause(vec![ec], vec![]); // C ⊑ ⊥
        let h = classify_built(&b.finish());
        assert!(h.unsat.contains(&ClassId::new(0)), "A unsat (⊥ up ∃)");
    }

    // Role-hierarchy ∀-prop: A ⊑ ∀S.B, A ⊑ ∃R.C, R ⊑ S  ⟹  C-context gets B
    // Observable as: with C ⊓ B ⊑ ⊥ added, A becomes unsat (B reached C via S⊒R).
    #[test]
    fn forall_propagation_over_role_hierarchy() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let s = B::role(1);
        b.role_incl(r, s); // R ⊑ S
        let all_sb = b.all(s, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_sb]); // A ⊑ ∀S.B
        b.clause(vec![ea], vec![some_rc]); // A ⊑ ∃R.C
        b.clause(vec![ec, eb], vec![]); // C ⊓ B ⊑ ⊥  (B did reach the R-succ)
        let h = classify_built(&b.finish());
        assert!(
            h.unsat.contains(&ClassId::new(0)),
            "A unsat: ∀S.B reaches the R-successor (R⊑S), clashes with C"
        );
    }

    // Pure-EL still correct (left-existential via complement encoding):
    // A ⊑ ∃R.B, B ⊑ C, ∃R.C ⊑ D  ⟹  A ⊑ D
    // Encode ∃R.C ⊑ D as: ⊤ ⊑ D ⊔ ∀R.X  and  {C, X} ⊑ ⊥.
    #[test]
    fn pure_el_left_existential() {
        let mut b = B::new();
        let (a, bb, c, d) = (b.class(0), b.class(1), b.class(2), b.class(3));
        let x = b.class(4); // complement atom X ≡ ¬C
        let (ea, eb, ec, ed, ex) = (b.atom(a), b.atom(bb), b.atom(c), b.atom(d), b.atom(x));
        let r = B::role(0);
        let some_rb = b.some(r, eb);
        let all_rx = b.all(r, ex);
        b.clause(vec![ea], vec![some_rb]); // A ⊑ ∃R.B
        b.clause(vec![eb], vec![ec]); // B ⊑ C
        b.clause(vec![], vec![ed, all_rx]); // ⊤ ⊑ D ⊔ ∀R.X   (= ∃R.C ⊑ D)
        b.clause(vec![ec, ex], vec![]); // {C, X} ⊑ ⊥
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 3), "A ⊑ D via left-existential complement");
    }

    // General (non-⊥) disjunctive back-prop — alehif-class case:
    // A ⊑ ∃R.C, C ⊑ D₁ ⊔ D₂, ∃R.D₁ ⊑ F, ∃R.D₂ ⊑ F  ⟹  A ⊑ F
    // Encode ∃R.Dᵢ ⊑ F as: ⊤ ⊑ F ⊔ ∀R.Xᵢ  and  {Dᵢ, Xᵢ} ⊑ ⊥.
    #[test]
    fn disjunctive_back_propagation() {
        let mut b = B::new();
        let (a, c, d1, d2, f) = (b.class(0), b.class(1), b.class(2), b.class(3), b.class(4));
        let x1 = b.class(5);
        let x2 = b.class(6);
        let (ea, ec, ed1, ed2, ef, ex1, ex2) = (
            b.atom(a),
            b.atom(c),
            b.atom(d1),
            b.atom(d2),
            b.atom(f),
            b.atom(x1),
            b.atom(x2),
        );
        let r = B::role(0);
        let some_rc = b.some(r, ec);
        let all_rx1 = b.all(r, ex1);
        let all_rx2 = b.all(r, ex2);
        b.clause(vec![ea], vec![some_rc]); // A ⊑ ∃R.C
        b.clause(vec![ec], vec![ed1, ed2]); // C ⊑ D₁ ⊔ D₂
        b.clause(vec![], vec![ef, all_rx1]); // ∃R.D₁ ⊑ F
        b.clause(vec![ed1, ex1], vec![]); // {D₁,X₁} ⊑ ⊥
        b.clause(vec![], vec![ef, all_rx2]); // ∃R.D₂ ⊑ F
        b.clause(vec![ed2, ex2], vec![]); // {D₂,X₂} ⊑ ⊥
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 4), "A ⊑ F via disjunctive back-propagation");
    }

    // Plain told subsumption + transitivity: A ⊑ B, B ⊑ C ⟹ A ⊑ C.
    #[test]
    fn told_transitive() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        b.clause(vec![ea], vec![eb]);
        b.clause(vec![eb], vec![ec]);
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 1));
        assert!(subsumes(&h, 1, 2));
        assert!(subsumes(&h, 0, 2), "transitive A ⊑ C");
    }

    // Termination on a cyclic existential: A ⊑ ∃R.A reuses A's own context by
    // core (the termination key); saturation must halt and report A ⊑ A only
    // (excluded as reflexive) — i.e. no spurious subsumptions, and no hang.
    #[test]
    fn cyclic_existential_terminates() {
        let mut b = B::new();
        let a = b.class(0);
        let ea = b.atom(a);
        let r = B::role(0);
        let some_ra = b.some(r, ea);
        b.clause(vec![ea], vec![some_ra]); // A ⊑ ∃R.A
        let h = classify_built(&b.finish());
        assert!(h.subsumptions.is_empty(), "only reflexive A ⊑ A (excluded)");
        assert!(h.unsat.is_empty());
    }

    // FP guard for back-prop: a satisfiable successor must NOT make the parent
    // unsat. A ⊑ ∀R.B, A ⊑ ∃R.C (B, C consistent) ⟹ A satisfiable, no A ⊑ ⊥.
    #[test]
    fn satisfiable_successor_no_false_unsat() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let all_rb = b.all(r, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_rb]); // A ⊑ ∀R.B
        b.clause(vec![ea], vec![some_rc]); // A ⊑ ∃R.C   (no B⊓C clash)
        let h = classify_built(&b.finish());
        assert!(
            !h.unsat.contains(&ClassId::new(0)),
            "A must stay satisfiable"
        );
    }

    // ∀ over a role hierarchy must NOT fire when the roles are unrelated:
    // A ⊑ ∀S.B, A ⊑ ∃R.C, C⊓B⊑⊥, but R ⋢ S ⟹ A satisfiable (∀S.B does not
    // reach the R-successor). FP guard for the role-subsumption side-condition.
    #[test]
    fn forall_unrelated_role_no_propagation() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let s = B::role(1); // unrelated to R (no role_incl)
        let all_sb = b.all(s, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_sb]); // A ⊑ ∀S.B
        b.clause(vec![ea], vec![some_rc]); // A ⊑ ∃R.C
        b.clause(vec![ec, eb], vec![]); // C ⊓ B ⊑ ⊥
        let h = classify_built(&b.finish());
        assert!(
            !h.unsat.contains(&ClassId::new(0)),
            "∀S.B must NOT reach the R-successor when R ⋢ S"
        );
    }
}
