//! The Sequoia ordered consequence-based calculus — saturation (S1, ALCH).
//!
//! Implements the ALCH subset of Tena-Cucala–Cuenca-Grau–Horrocks
//! (arXiv:1805.01396): Core, **ordered** Hyper (side-condition `Δᵢ ⊁ᵥ Aᵢσ`),
//! Succ (`∃R.B` spawns a successor cored at `{B}`), R∀ (`∀S.B` propagation over
//! the role hierarchy), Elim (Def-4 redundancy), and `⊥`-back-propagation along
//! edges under the carried residual. NO Eq/Ineq/Fact/Pred-equality/Nom/terms
//! (those are S2+).
//!
//! The make-or-break property is the per-context **subsumer-respecting order**
//! (`seq_order.rs`, per design §0.45) that makes ordered Hyper + the positive
//! `∈̂` read-off complete on ALCH — validated by the differential gate against
//! the sound+complete unordered B1 engine.
//!
//! # Soundness
//! Every rule is an instance of a sound Sequoia inference (Theorem 1, order-
//! independent). The order only gates WHICH atomic resolutions fire ⟹ an order
//! bug is MISS-biased, never an FP. Back-prop reflects `⊥` to the parent only
//! under the spawning edge's residual disjunction (`v ⊑ ⊥` only when the
//! residual is empty) — the standard CB soundness landmine, guarded.

use crate::normalize::Normalized;
use crate::seq_model::{Atom, ContextId, SeqClause, SeqContext, SeqEdge, SeqGraph, SeqLit};
use crate::seq_order::OrderBuilder;
use owl_dl_core::ir::{ConceptExpr, ConceptId, Role};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// `a ⊆ b` for two ascending-sorted literal slices (set subset). NOTE: the head
/// is sorted by `≻ᵥ`, but subset on a sorted-by-key slice still needs a true set
/// test, so we operate on COPIES sorted by `ConceptId` (see `head_set`).
fn subset_sorted(a: &[SeqLit], b: &[SeqLit]) -> bool {
    let mut j = 0;
    for x in a {
        while j < b.len() && b[j] < *x {
            j += 1;
        }
        if j >= b.len() || b[j] != *x {
            return false;
        }
        j += 1;
    }
    true
}

/// An edge into a context for back-prop indexing: `(parent, residual)`.
type PredEdge = (ContextId, Vec<SeqLit>);

struct SeqEngine<'a> {
    norm: &'a Normalized,
    order: OrderBuilder,
    graph: SeqGraph,
    /// Predecessors of each context: `(parent, residual)`. A child reflects `⊥`
    /// (under the residual) to each parent here.
    preds: Vec<Vec<PredEdge>>,
    /// Worklist of contexts whose clause set changed.
    dirty: VecDeque<ContextId>,
    in_queue: Vec<bool>,
}

impl<'a> SeqEngine<'a> {
    fn new(norm: &'a Normalized) -> Self {
        let order = OrderBuilder::build(&norm.clauses, &norm.pool);
        Self {
            norm,
            order,
            graph: SeqGraph::default(),
            preds: Vec::new(),
            dirty: VecDeque::new(),
            in_queue: Vec::new(),
        }
    }

    /// Resolve a `ClassId` to its interned `Atomic` `ConceptId`.
    fn atom_of_class(&self, c: owl_dl_core::ir::ClassId) -> ConceptId {
        for (id, e) in self.norm.pool.iter_with_ids() {
            if let ConceptExpr::Atomic(cc) = e
                && *cc == c
            {
                return id;
            }
        }
        unreachable!("reportable class atom not interned in pool");
    }

    fn is_atomic(&self, l: ConceptId) -> bool {
        matches!(
            self.norm.pool.get(l),
            ConceptExpr::Atomic(_) | ConceptExpr::Top
        )
    }

    fn as_some(&self, l: ConceptId) -> Option<(Role, ConceptId)> {
        match self.norm.pool.get(l) {
            ConceptExpr::Some(r, c) => Some((*r, *c)),
            _ => None,
        }
    }

    fn as_all(&self, l: ConceptId) -> Option<(Role, ConceptId)> {
        match self.norm.pool.get(l) {
            ConceptExpr::All(r, c) => Some((*r, *c)),
            _ => None,
        }
    }

    /// `R ⊑* S` under the role hierarchy (reflexive-transitive closure).
    fn role_subsumed(&self, sub: Role, sup: Role) -> bool {
        if sub == sup {
            return true;
        }
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

    /// Find-or-create the context whose core is exactly `core`. `query` is
    /// `Some(atom)` for a root classification context (forces it `≻`-minimal),
    /// `None` for a successor context. Seeds the core as Core units, enqueues.
    fn intern_context(&mut self, core: BTreeSet<Atom>, query: Option<ConceptId>) -> ContextId {
        if let Some(&id) = self.graph.by_core.get(&core) {
            return id;
        }
        let id = self.graph.contexts.len();
        let order = self.order.per_context(query);
        let mut ctx = SeqContext {
            core: core.clone(),
            order,
            ..SeqContext::default()
        };
        // Core: seed each core atom as a unit clause `→ A`.
        for &a in &core {
            let cl = SeqClause { head: vec![a] };
            ctx.seen.insert(cl.clone());
            ctx.clauses.push(cl);
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

    /// Add a head disjunction as a clause to context `v` under the Elim (Def-4)
    /// redundancy gate. Returns `true` if stored. Enqueues `v` + predecessors.
    ///
    /// Redundancy: the new clause is dropped if some existing clause's head is a
    /// subset of it (forward subsumption); existing clauses whose head is a
    /// strict superset and carry only atomic literals are removed (backward).
    /// Every drop removes an entailed (redundant) clause ⟹ MISS-free, never FP.
    #[allow(clippy::needless_pass_by_value)] // logically consumes the clause
    fn add_clause(&mut self, v: ContextId, head: Vec<SeqLit>) -> bool {
        // Tautology: `Top` in head ⟹ clause is trivially true, drop.
        if head
            .iter()
            .any(|&l| matches!(self.norm.pool.get(l), ConceptExpr::Top))
        {
            return false;
        }
        // Canonical set form (sorted by ConceptId, deduped) for subset tests.
        let mut canon = head.clone();
        canon.sort_unstable();
        canon.dedup();

        {
            let ctx = &self.graph.contexts[v];
            // Forward subsumption: an existing clause whose head ⊆ canon makes
            // this clause redundant.
            for e in &ctx.clauses {
                if subset_sorted(&e.head_canon(), &canon) {
                    return false;
                }
            }
        }
        // Backward subsumption: drop existing clauses strictly superset of canon
        // that carry ONLY atomic literals (a structural `∃`/`∀` head must stay —
        // it spawns/propagates).
        let mut removed: Vec<SeqClause> = Vec::new();
        for e in &self.graph.contexts[v].clauses {
            let ec = e.head_canon();
            if ec != canon
                && subset_sorted(&canon, &ec)
                && ec
                    .iter()
                    .all(|&l| matches!(self.norm.pool.get(l), ConceptExpr::Atomic(_)))
            {
                removed.push(e.clone());
            }
        }
        // Build the stored clause: head sorted by `≻ᵥ` (maximal last).
        let mut ordered = canon.clone();
        let order = self.graph.contexts[v].order.clone();
        order.sort_head(&self.norm.pool, &mut ordered);
        let dc = SeqClause { head: ordered };

        let ctx = &mut self.graph.contexts[v];
        if ctx.seen.contains(&dc) {
            return false;
        }
        if !removed.is_empty() {
            let rm: BTreeSet<&SeqClause> = removed.iter().collect();
            ctx.clauses.retain(|e| !rm.contains(e));
            for r in &removed {
                ctx.seen.remove(r);
            }
        }
        ctx.seen.insert(dc.clone());
        ctx.clauses.push(dc);
        self.enqueue(v);
        let preds: Vec<ContextId> = self.preds[v].iter().map(|(p, _)| *p).collect();
        for p in preds {
            self.enqueue(p);
        }
        true
    }

    /// Saturate to fixpoint.
    fn run(&mut self) {
        // Seed one root context per reportable class, each query-minimal at its
        // own atom (Condition C2).
        let mut roots: Vec<ConceptId> = Vec::new();
        for &c in &self.norm.classes {
            roots.push(self.atom_of_class(c));
        }
        for a in roots {
            let mut core = BTreeSet::new();
            core.insert(a);
            self.intern_context(core, Some(a));
        }

        let debug = std::env::var("RUSTDL_CB_DEBUG").is_ok();
        if debug {
            eprintln!(
                "[seq] seeded {} root contexts, {} ont clauses",
                self.graph.contexts.len(),
                self.norm.clauses.len()
            );
        }
        let mut dequeues: u64 = 0;
        while let Some(v) = self.dirty.pop_front() {
            self.in_queue[v] = false;
            self.process(v);
            dequeues += 1;
            if debug && dequeues.is_multiple_of(100) {
                let total: usize = self.graph.contexts.iter().map(|c| c.clauses.len()).sum();
                eprintln!(
                    "[seq] dequeues={dequeues} contexts={} total_clauses={total} queue={}",
                    self.graph.contexts.len(),
                    self.dirty.len()
                );
            }
        }
    }

    fn process(&mut self, v: ContextId) {
        self.apply_hyper(v);
        self.apply_succ_and_forall(v);
        self.apply_back_prop(v);
    }

    /// Ordered Hyper (Sequoia Table 2). For each ontology clause `⋀ Aᵢ → Δ`:
    /// pick, for each premise atom `Aᵢ`, a context clause `→ Δᵢ ∨ Aᵢ` with the
    /// eligibility side-condition `Δᵢ ⊁ᵥ Aᵢ` (no atomic literal of the residual
    /// `Δᵢ` is `≻ᵥ`-greater than `Aᵢ`). Derive `→ ⋁ Δᵢ ∨ Δ`.
    fn apply_hyper(&mut self, v: ContextId) {
        let heads: Vec<Vec<SeqLit>> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|c| c.head.clone())
            .collect();
        let order = self.graph.contexts[v].order.clone();

        // Index per resolvable atom `p`: the ELIGIBLE residuals `Δᵢ` of context
        // clauses `→ Δᵢ ∨ p` (p removed) with `Δᵢ ⊁ᵥ p`.
        let mut by_atom: BTreeMap<ConceptId, Vec<Vec<SeqLit>>> = BTreeMap::new();
        for h in &heads {
            for (i, &lit) in h.iter().enumerate() {
                if self.is_atomic(lit) {
                    let mut residual = h.clone();
                    residual.remove(i);
                    if order.eligible(&self.norm.pool, &residual, lit) {
                        by_atom.entry(lit).or_default().push(residual);
                    }
                }
            }
        }

        let ont_clauses = self.norm.clauses.clone();
        let mut new_clauses: Vec<Vec<SeqLit>> = Vec::new();
        for oc in &ont_clauses {
            let mut combos: Vec<Vec<SeqLit>> = vec![Vec::new()];
            let mut ok = true;
            for &p in &oc.premise {
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
                let mut next: Vec<Vec<SeqLit>> = Vec::new();
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

    /// The successor core for an existential filler: atomic ⇒ `{B}`, `⊤` ⇒ `{}`,
    /// else `None` (shouldn't occur post-normalize for ALCH).
    fn filler_core(&self, bfill: ConceptId) -> Option<BTreeSet<Atom>> {
        let mut core: BTreeSet<Atom> = BTreeSet::new();
        match self.norm.pool.get(bfill) {
            ConceptExpr::Atomic(_) => {
                core.insert(bfill);
                Some(core)
            }
            ConceptExpr::Top => Some(core),
            _ => None,
        }
    }

    /// Succ + R∀.
    ///
    /// **Succ.** Every UNIT `∃R.B` head (`→ ∃R.B`, residual empty) spawns/links a
    /// successor context cored at `{B}`. A non-unit `→ Δ ∨ ∃R.B` (residual `Δ`)
    /// also spawns an edge carrying residual `Δ` (the successor exists only when
    /// `¬Δ`). The edge carries the residual for `⊥`-back-prop.
    /// **R∀.** For an outgoing edge `v —R→ u` (residual `M`) and a head
    /// `→ N ∨ ∀S.B` with `R ⊑* S`, link a NEW edge to `core(u) ∪ {B}` with
    /// residual `M ⊔ N` (find-or-create; the shared `u` is never mutated).
    fn apply_succ_and_forall(&mut self, v: ContextId) {
        let heads: Vec<Vec<SeqLit>> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|c| c.head.clone())
            .collect();

        // Succ: each `∃R.B` literal → edge to `{B}` carrying residual.
        let mut succ_requests: Vec<(Role, BTreeSet<Atom>, Vec<SeqLit>)> = Vec::new();
        for h in &heads {
            for (i, &lit) in h.iter().enumerate() {
                if let Some((r, bfill)) = self.as_some(lit)
                    && let Some(core) = self.filler_core(bfill)
                {
                    let mut residual = h.clone();
                    residual.remove(i);
                    succ_requests.push((r, core, residual));
                }
            }
        }
        for (r, core, residual) in succ_requests {
            let u = self.intern_context(core, None);
            self.link_edge(v, u, r, residual);
        }

        // R∀: ∀S.B over each outgoing edge with R ⊑* S.
        let edges: Vec<(Role, ContextId, Vec<SeqLit>)> = self.graph.contexts[v]
            .succ
            .iter()
            .map(|e| (e.role, e.child, e.residual.clone()))
            .collect();
        let mut foralls: Vec<(Role, ConceptId, Vec<SeqLit>)> = Vec::new();
        for h in &heads {
            for (i, &lit) in h.iter().enumerate() {
                if let Some((s, bb)) = self.as_all(lit)
                    && matches!(self.norm.pool.get(bb), ConceptExpr::Atomic(_))
                {
                    let mut residual = h.clone();
                    residual.remove(i);
                    foralls.push((s, bb, residual));
                }
            }
        }
        let mut forall_links: Vec<(Role, BTreeSet<Atom>, Vec<SeqLit>)> = Vec::new();
        for (r, u, edge_res) in &edges {
            for (s, bb, fa_res) in &foralls {
                if !self.role_subsumed(*r, *s) {
                    continue;
                }
                let mut new_core = self.graph.contexts[*u].core.clone();
                if !new_core.insert(*bb) {
                    continue; // B already in the successor core — no new edge.
                }
                let mut new_res = edge_res.clone();
                new_res.extend_from_slice(fa_res);
                forall_links.push((*r, new_core, new_res));
            }
        }
        for (r, core, residual) in forall_links {
            let u = self.intern_context(core, None);
            self.link_edge(v, u, r, residual);
        }
    }

    /// Register an edge `v —R→ u` carrying `residual` (sorted+deduped), if
    /// absent. Records both the outgoing edge (on `v`) and the predecessor entry
    /// (on `u`) and enqueues both endpoints on a genuine addition.
    fn link_edge(&mut self, v: ContextId, u: ContextId, r: Role, mut residual: Vec<SeqLit>) {
        residual.sort_unstable();
        residual.dedup();
        let exists = self.graph.contexts[v]
            .succ
            .iter()
            .any(|e| e.child == u && e.role == r && e.residual == residual);
        if exists {
            return;
        }
        self.graph.contexts[v].succ.push(SeqEdge {
            child: u,
            role: r,
            residual: residual.clone(),
        });
        self.preds[u].push((v, residual));
        self.enqueue(u);
        self.enqueue(v);
    }

    /// `⊥`-back-propagation: for each edge `v —R→ u` (residual `N`), if `u`
    /// derives `⊥` (an empty-head clause) reflect `N` to `v` (so `v ⊑ ⊥` only
    /// when `N` is empty). The soundness landmine — guarded by the residual.
    fn apply_back_prop(&mut self, v: ContextId) {
        let edges: Vec<(ContextId, Vec<SeqLit>)> = self.graph.contexts[v]
            .succ
            .iter()
            .map(|e| (e.child, e.residual.clone()))
            .collect();
        for (u, residual) in edges {
            let u_has_bot = self.graph.contexts[u]
                .clauses
                .iter()
                .any(|c| c.head.is_empty());
            if u_has_bot {
                self.add_clause(v, residual);
            }
        }
    }
}

impl SeqClause {
    /// The head sorted by `ConceptId` (for set subset tests, independent of the
    /// `≻ᵥ` storage order).
    fn head_canon(&self) -> Vec<SeqLit> {
        let mut c = self.head.clone();
        c.sort_unstable();
        c.dedup();
        c
    }
}

/// Saturate the ordered context graph (S1 / ALCH).
#[must_use]
pub(crate) fn saturate(norm: &Normalized) -> SeqGraph {
    let mut eng = SeqEngine::new(norm);
    eng.run();
    eng.graph
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::CbHierarchy;
    use crate::model::OntClause;
    use crate::normalize::Normalized;
    use crate::seq_classify::read_hierarchy;
    use owl_dl_core::ir::{ClassId, ConceptPool, RoleId};
    use std::collections::BTreeSet;

    /// Hand-build a normalized ALCH ontology directly (bypassing the IR layer),
    /// mirroring `engine.rs`'s `B` builder so the S1 engine can be validated
    /// against the SAME clause shapes — including an ADVERSARIAL order where the
    /// told-subsumer-depth construction's correctness is the make-or-break point.
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
                max_roles: BTreeSet::new(),
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

    // ── THE crux gate (§6 / §0.45): A ⊑ B⊔C, B ⊑ D, C ⊑ D ⟹ A ⊑ D ──
    // Must derive A⊑D under the subsumer-respecting order. This is the
    // inference Slice-0's ordering broke; passing it proves the ordered Hyper
    // + order + ∈̂ read-off genuinely produce the hierarchy.
    #[test]
    fn by_cases_a_sub_d_under_subsumer_order() {
        let mut b = B::new();
        let (a, bb, c, d) = (b.class(0), b.class(1), b.class(2), b.class(3));
        let (ea, eb, ec, ed) = (b.atom(a), b.atom(bb), b.atom(c), b.atom(d));
        b.clause(vec![ea], vec![eb, ec]); // A ⊑ B ⊔ C
        b.clause(vec![eb], vec![ed]); // B ⊑ D
        b.clause(vec![ec], vec![ed]); // C ⊑ D
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 3), "A ⊑ D by reasoning-by-cases (ordered)");
        assert!(subsumes(&h, 1, 3), "B ⊑ D direct");
        assert!(subsumes(&h, 2, 3), "C ⊑ D direct");
    }

    // ADVERSARIAL: the SAME inference where ClassIds are assigned so a naive
    // by-id order would put the query-superclass D as ≻-maximal and (per §0.45)
    // MISS A⊑D. The subsumer-respecting depth construction must still derive it.
    // Here D = class 0 (smallest id), A = class 3 (largest). A naive
    // ascending-id order with A minimal would make B,C ≻ D fail eligibility.
    #[test]
    fn by_cases_adversarial_id_assignment() {
        let mut b = B::new();
        let (d, bb, c, a) = (b.class(0), b.class(1), b.class(2), b.class(3));
        let (ed, eb, ec, ea) = (b.atom(d), b.atom(bb), b.atom(c), b.atom(a));
        b.clause(vec![ea], vec![eb, ec]); // A ⊑ B ⊔ C
        b.clause(vec![eb], vec![ed]); // B ⊑ D
        b.clause(vec![ec], vec![ed]); // C ⊑ D
        let h = classify_built(&b.finish());
        assert!(
            subsumes(&h, 3, 0),
            "A(3) ⊑ D(0) under subsumer-respecting order (adversarial ids)"
        );
    }

    // FP guard: disjunction alone gives no unit subsumption.
    #[test]
    fn disjunction_alone_no_unit() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        b.clause(vec![ea], vec![eb, ec]);
        let h = classify_built(&b.finish());
        assert!(!subsumes(&h, 0, 1), "A ⊑ B must NOT derive");
        assert!(!subsumes(&h, 0, 2), "A ⊑ C must NOT derive");
    }

    #[test]
    fn forall_exists_clash_unsat() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let all_rb = b.all(r, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_rb]);
        b.clause(vec![ea], vec![some_rc]);
        b.clause(vec![ec, eb], vec![]);
        let h = classify_built(&b.finish());
        assert!(h.unsat.contains(&ClassId::new(0)), "A unsat via ∀+∃+clash");
    }

    #[test]
    fn bot_propagates_up_existential() {
        let mut b = B::new();
        let (a, c) = (b.class(0), b.class(1));
        let (ea, ec) = (b.atom(a), b.atom(c));
        let r = B::role(0);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![some_rc]);
        b.clause(vec![ec], vec![]);
        let h = classify_built(&b.finish());
        assert!(h.unsat.contains(&ClassId::new(0)), "A unsat (⊥ up ∃)");
    }

    #[test]
    fn forall_propagation_over_role_hierarchy() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let s = B::role(1);
        b.role_incl(r, s);
        let all_sb = b.all(s, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_sb]);
        b.clause(vec![ea], vec![some_rc]);
        b.clause(vec![ec, eb], vec![]);
        let h = classify_built(&b.finish());
        assert!(
            h.unsat.contains(&ClassId::new(0)),
            "A unsat: ∀S.B reaches the R-successor (R⊑S), clashes with C"
        );
    }

    #[test]
    fn pure_el_left_existential() {
        let mut b = B::new();
        let (a, bb, c, d) = (b.class(0), b.class(1), b.class(2), b.class(3));
        let x = b.class(4);
        let (ea, eb, ec, ed, ex) = (b.atom(a), b.atom(bb), b.atom(c), b.atom(d), b.atom(x));
        let r = B::role(0);
        let some_rb = b.some(r, eb);
        let all_rx = b.all(r, ex);
        b.clause(vec![ea], vec![some_rb]);
        b.clause(vec![eb], vec![ec]);
        b.clause(vec![], vec![ed, all_rx]);
        b.clause(vec![ec, ex], vec![]);
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 3), "A ⊑ D via left-existential complement");
    }

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
        b.clause(vec![ea], vec![some_rc]);
        b.clause(vec![ec], vec![ed1, ed2]);
        b.clause(vec![], vec![ef, all_rx1]);
        b.clause(vec![ed1, ex1], vec![]);
        b.clause(vec![], vec![ef, all_rx2]);
        b.clause(vec![ed2, ex2], vec![]);
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 4), "A ⊑ F via disjunctive back-propagation");
    }

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

    #[test]
    fn cyclic_existential_terminates() {
        let mut b = B::new();
        let a = b.class(0);
        let ea = b.atom(a);
        let r = B::role(0);
        let some_ra = b.some(r, ea);
        b.clause(vec![ea], vec![some_ra]);
        let h = classify_built(&b.finish());
        assert!(h.subsumptions.is_empty(), "only reflexive A ⊑ A (excluded)");
        assert!(h.unsat.is_empty());
    }

    #[test]
    fn satisfiable_successor_no_false_unsat() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let all_rb = b.all(r, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_rb]);
        b.clause(vec![ea], vec![some_rc]);
        let h = classify_built(&b.finish());
        assert!(
            !h.unsat.contains(&ClassId::new(0)),
            "A must stay satisfiable"
        );
    }

    #[test]
    fn forall_unrelated_role_no_propagation() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        let r = B::role(0);
        let s = B::role(1);
        let all_sb = b.all(s, eb);
        let some_rc = b.some(r, ec);
        b.clause(vec![ea], vec![all_sb]);
        b.clause(vec![ea], vec![some_rc]);
        b.clause(vec![ec, eb], vec![]);
        let h = classify_built(&b.finish());
        assert!(
            !h.unsat.contains(&ClassId::new(0)),
            "∀S.B must NOT reach the R-successor when R ⋢ S"
        );
    }
}
