//! The consequence-based ALCHQ calculus: context saturation.
//!
//! B1 (ALCH) is the Simančík–Kazakov–Horrocks (IJCAI 2011) context calculus with
//! **unordered** hyperresolution; B2 Tier-2 layers Sequoia-style equality
//! reasoning for qualified number restrictions `≤n R.C` / `≥n R.C` (ALCHQ) on top
//! of an unordered-Hyper host (Tena-Cucala et al., arXiv:1805.01396), per
//! `docs/superpowers/specs/2026-06-16-cb-b2-tier2-equality-design.md` (§9 is
//! authoritative).
//!
//! ## Clause form
//! A *derived clause* in a context is a disjunction of [`HeadLit`]s — `Concept(_)`
//! (atomic `B`, `∃R.B`, `∀R.B`) plus the B2 equality literals `Eq(s,t)`/`Neq(s,t)`
//! ranging over **successor terms**. Understood as `core ⊑ ⊔ⱼ Lⱼ`. Empty = `⊥`.
//!
//! ## Stratification (the Slice-0 / §3 constraint)
//! `apply_hyper` operates ONLY on `Concept(_)` literals and stays **unordered** —
//! it never indexes `Eq`/`Neq`. The equality disjunction is discharged by the
//! §9.2 recursive `apply_eq_discharge` rule (speculative merge per `Eq`-disjunct,
//! residual = the clause minus that literal), NOT by Hyper.
//!
//! ## Soundness (the sacred invariant)
//! A speculative merge reflects its residual to the parent **only when its
//! union-core derives `⊥`** (§4.2/§9.3); bare `⊥` is reflected only when the
//! residual is empty (full case-exhaustion). Merge edges are ⊥-back-prop ONLY —
//! never a source of positive `Concept` back-prop, never an R∀ source — so a
//! satisfiable union-core's derived atoms can never leak to the parent as a
//! spurious subsumption.

use crate::model::{
    Atom, Context, ContextGraph, ContextId, DerivedClause, EdgeKind, HeadLit, Term, TermId,
};
use crate::normalize::Normalized;
use owl_dl_core::ir::{ConceptExpr, ConceptId, Role};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A head disjunction of literals, sorted+deduped. Empty = `⊥`.
type Clause = Vec<HeadLit>;

/// `a ⊆ b` for two ascending-sorted literal slices (set subset).
fn is_subset(a: &[HeadLit], b: &[HeadLit]) -> bool {
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

/// An edge into a context: `(parent, kind, residual)`.
type Edge = (ContextId, EdgeKind, Clause);

/// Engine-internal mutable state layered over the frozen [`ContextGraph`].
struct Engine<'a> {
    norm: &'a Normalized,
    graph: ContextGraph,
    /// Predecessors of each context: `(pred_context, edge_kind, residual)`.
    preds: Vec<Vec<Edge>>,
    /// Global term store. `TermId` indexes this directly; `Term.ctx` is the
    /// witness's *type* context, and a term's *owner* (the parent it is a
    /// successor of) is `term_owner[id]`. `merged_into` always points within
    /// the same owner (a merge unions two of one parent's terms).
    terms: Vec<Term>,
    /// Owner (parent) context of each term.
    term_owner: Vec<ContextId>,
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
            terms: Vec::new(),
            term_owner: Vec::new(),
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
        for &a in &core {
            let dc = DerivedClause {
                premise: Vec::new(),
                head: vec![HeadLit::Concept(a)],
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

    /// Union-find representative of a term, following `merged_into` chains.
    fn find(&self, t: TermId) -> TermId {
        let mut cur = t;
        while let Some(next) = self.terms[cur].merged_into {
            cur = next;
        }
        cur
    }

    /// Mint a fresh term owned by `owner`, typed by context `ctx`, on role `r`,
    /// with the given edge `residual` (sorted+deduped) as its signature.
    fn new_term(&mut self, owner: ContextId, ctx: ContextId, r: Role, residual: Clause) -> TermId {
        let id = self.terms.len();
        let term = Term {
            ctx,
            role: r,
            residual,
            merged_into: None,
        };
        self.terms.push(term.clone());
        self.term_owner.push(owner);
        self.graph.contexts[owner].terms.push(term);
        id
    }

    /// Add a derived clause `head` to context `v`, applying the redundancy gate
    /// (tautology deletion + forward/backward subsumption + equality
    /// canonicalization). Returns `true` if stored. Enqueues `v` and its
    /// predecessors on change. Every drop removes only an entailed (redundant)
    /// clause ⟹ MISS-free, never an FP.
    #[allow(clippy::needless_pass_by_value)] // logically consumes the clause
    fn add_clause(&mut self, v: ContextId, head: Clause) -> bool {
        // Canonicalize equalities via union-find; drop reflexive Eq (taut) and
        // false Neq(s,s) disjuncts; tautology-delete `Concept(Top)`.
        let mut filtered: Clause = Vec::with_capacity(head.len());
        for &l in &head {
            match l {
                HeadLit::Concept(c) if matches!(self.norm.pool.get(c), ConceptExpr::Top) => {
                    return false;
                }
                HeadLit::Eq(s, t) => {
                    let (cs, ct) = (self.find(s), self.find(t));
                    if cs == ct {
                        return false; // Eq(s,s) reflexive ⇒ clause is a tautology.
                    }
                    filtered.push(HeadLit::Eq(cs.min(ct), cs.max(ct)));
                }
                HeadLit::Neq(s, t) => {
                    let (cs, ct) = (self.find(s), self.find(t));
                    if cs == ct {
                        // Neq(s,s) is false: this disjunct contributes nothing.
                    } else {
                        filtered.push(HeadLit::Neq(cs.min(ct), cs.max(ct)));
                    }
                }
                other @ HeadLit::Concept(_) => filtered.push(other),
            }
        }
        filtered.sort_unstable();
        filtered.dedup();
        let dc = DerivedClause {
            premise: Vec::new(),
            head: filtered,
        };
        {
            let ctx = &self.graph.contexts[v];
            if ctx.seen.contains(&dc) {
                return false;
            }
            if ctx.clauses.iter().any(|e| is_subset(&e.head, &dc.head)) {
                return false;
            }
        }
        // Backward subsumption: drop strictly-weaker (superset) clauses whose
        // head carries no structural `∃/∀` literal (purely Atomic / Eq / Neq).
        let mut removed: Vec<DerivedClause> = Vec::new();
        for e in &self.graph.contexts[v].clauses {
            if e.head != dc.head
                && is_subset(&dc.head, &e.head)
                && e.head.iter().all(|l| match l {
                    HeadLit::Concept(c) => matches!(self.norm.pool.get(*c), ConceptExpr::Atomic(_)),
                    HeadLit::Eq(_, _) | HeadLit::Neq(_, _) => true,
                })
            {
                removed.push(e.clone());
            }
        }
        let ctx = &mut self.graph.contexts[v];
        if !removed.is_empty() {
            let rm: BTreeSet<&DerivedClause> = removed.iter().collect();
            ctx.clauses.retain(|e| !rm.contains(e));
            for r in &removed {
                ctx.seen.remove(r);
            }
        }
        ctx.seen.insert(dc.clone());
        ctx.clauses.push(dc);
        self.enqueue(v);
        let preds: Vec<ContextId> = self.preds[v].iter().map(|(p, _, _)| *p).collect();
        for p in preds {
            self.enqueue(p);
        }
        true
    }

    /// Literal classification helpers.
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

    /// Saturate to fixpoint.
    fn run(&mut self) {
        let mut pool_classes: Vec<ConceptId> = Vec::new();
        for &c in &self.norm.classes {
            pool_classes.push(self.atom_of_class(c));
        }
        for a in pool_classes {
            let mut core = BTreeSet::new();
            core.insert(a);
            self.intern_context(core);
        }

        let debug = std::env::var("RUSTDL_CB_DEBUG").is_ok();
        if debug {
            eprintln!("[cb] seeded {} root contexts", self.graph.contexts.len());
        }
        let mut dequeues: u64 = 0;
        while let Some(v) = self.dirty.pop_front() {
            self.in_queue[v] = false;
            self.process(v);
            dequeues += 1;
            if debug && dequeues.is_multiple_of(100) {
                let total: usize = self.graph.contexts.iter().map(|c| c.clauses.len()).sum();
                eprintln!(
                    "[cb] dequeues={dequeues} contexts={} total_clauses={total} queue={}",
                    self.graph.contexts.len(),
                    self.dirty.len()
                );
            }
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

    /// Process all rules for context `v` over its current clause set.
    fn process(&mut self, v: ContextId) {
        self.apply_hyper(v);
        self.record_at_most(v);
        self.apply_succ_and_forall(v);
        self.apply_at_most(v);
        self.apply_eq_discharge(v);
        self.apply_eq_resolution(v);
        self.apply_back_prop(v);
    }

    /// Record `(n, R, C)` in `v.at_most` for every **unit** derived clause
    /// `{Max(n,R,C)}` — i.e. `core(v) ⊑ ≤n R.C` is *entailed*. A `Max` literal
    /// inside a multi-literal disjunction is NOT recorded (the constraint is not
    /// forced): sound, MISS-biased. Idempotent (dedup by `(n,R,C)`).
    fn record_at_most(&mut self, v: ContextId) {
        let mut found: Vec<(u32, Role, Atom)> = Vec::new();
        for dc in &self.graph.contexts[v].clauses {
            if dc.head.len() == 1
                && let HeadLit::Concept(cid) = dc.head[0]
                && let ConceptExpr::Max(n, r, c) = self.norm.pool.get(cid)
            {
                found.push((*n, *r, *c));
            }
        }
        let ctx = &mut self.graph.contexts[v];
        let mut added = false;
        for triple in found {
            if !ctx.at_most.contains(&triple) {
                ctx.at_most.push(triple);
                added = true;
            }
        }
        if added {
            self.enqueue(v);
        }
    }

    /// Rule 2: unordered hyperresolution (SKH Table 3, `Rⁿ⊓` generalized).
    ///
    /// Resolves ONLY on `Concept(_)` atomic literals; `Eq`/`Neq` are invisible
    /// here (§3.2 — keeping the term ordering off the concept stratum). They are
    /// retained in residuals (we remove only the pivot index, never strip them).
    fn apply_hyper(&mut self, v: ContextId) {
        let clauses: Vec<Clause> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|dc| dc.head.clone())
            .collect();

        // Index: for each atom `p`, the residuals `N` of every derived clause
        // `N ⊔ {p}` (p removed) — *any* occurrence. Unordered (directly complete).
        let mut by_atom: BTreeMap<ConceptId, Vec<Clause>> = BTreeMap::new();
        for c in &clauses {
            for (i, lit) in c.iter().enumerate() {
                if let HeadLit::Concept(cid) = lit
                    && self.is_atomic(*cid)
                {
                    let mut residual = c.clone();
                    residual.remove(i);
                    by_atom.entry(*cid).or_default().push(residual);
                }
            }
        }

        let mut new_clauses: Vec<Clause> = Vec::new();
        let ont_clauses = self.norm.clauses.clone();
        for oc in &ont_clauses {
            let mut combos: Vec<Clause> = vec![Vec::new()];
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
                head.extend(oc.head.iter().map(|&c| HeadLit::Concept(c)));
                new_clauses.push(head);
            }
        }
        for h in new_clauses {
            self.add_clause(v, h);
        }
    }

    /// Rule 3 (`R⁺∃` / Succ) + the `≥n` term-minting + Rule 4 (`R∀`).
    ///
    /// **Succ.** Every `∃R.B` literal mints/links a successor *term* whose type
    /// context contains `B`. The edge carries the residual disjunction `N`.
    /// **`≥n`.** A `Min(n,R,B)` literal mints `n` distinct terms (each typed by
    /// `B`) plus pairwise `Neq` (rule lowered in normalize → see `Min` arm). In
    /// this engine `Min` is lowered by normalize to a fresh marker; here we mint
    /// the terms when we encounter the marker literal.
    /// **R∀ (find-or-create, never mutate).** A `∀S.B` literal at `v`, over an
    /// edge `v —R→ u` with `R ⊑* S`, adds a *new* edge to the augmented
    /// successor `core(u) ∪ {B}`. The shared `u` is never mutated.
    fn apply_succ_and_forall(&mut self, v: ContextId) {
        let clauses: Vec<Clause> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|dc| dc.head.clone())
            .collect();

        // ── Succ + ≥n: each ∃R.B / ≥n R.B literal. ──
        // For each (role, filler-core, residual) signature we ensure at most the
        // requested count of live terms — `1` for ∃, `n` for ≥n. This keys term
        // minting so the worklist re-processing is idempotent (termination).
        let mut requests: Vec<(Role, BTreeSet<Atom>, u32, Clause)> = Vec::new();
        for c in &clauses {
            for (i, lit) in c.iter().enumerate() {
                let HeadLit::Concept(cid) = lit else {
                    continue;
                };
                let mut residual = c.clone();
                residual.remove(i);
                if let Some((r, bfill)) = self.as_some(*cid) {
                    let core = self.filler_core(bfill);
                    if let Some(core) = core {
                        requests.push((r, core, 1, residual));
                    }
                } else if let ConceptExpr::Min(n, r, bfill) = self.norm.pool.get(*cid) {
                    let (n, r, bfill) = (*n, *r, *bfill);
                    if n == 0 {
                        continue; // ≥0 ≡ ⊤.
                    }
                    if let Some(core) = self.filler_core(bfill) {
                        requests.push((r, core, n, residual));
                    }
                }
            }
        }
        for (r, core, count, residual) in requests {
            self.ensure_terms(v, r, &core, count, &residual);
        }

        // ── R∀: for each outgoing Succ-edge v —R→ u (residual M) and each ∀S.B
        // with R ⊑* S, add edge v —R→ (core(u) ∪ {B}) residual M ⊔ N. ──
        let edges: Vec<(Role, ContextId, Clause)> = self.succ_edges(v);
        let mut foralls: Vec<(Role, ConceptId, Clause)> = Vec::new();
        for c in &clauses {
            for (i, lit) in c.iter().enumerate() {
                if let HeadLit::Concept(cid) = lit
                    && let Some((s, bb)) = self.as_all(*cid)
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
                    continue;
                }
                let mut new_res = edge_res.clone();
                new_res.extend_from_slice(fa_res);
                // The augmented edge is a Succ edge typed at the same role `r`,
                // re-using a fresh term (the ∀-augmented witness). We mint one
                // term per (role, augmented-core, residual) signature.
                self.ensure_terms(v, *r, &new_core, 1, &new_res);
            }
        }
    }

    /// The successor core for an existential filler: `Some(core)` (atomic ⇒
    /// `{B}`, `⊤` ⇒ `{}`), or `None` if the filler is non-atomic (shouldn't
    /// occur post-normalize).
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

    /// Ensure context `v` has at least `count` live terms with the given
    /// `(role, type-core, residual)` signature, minting any shortfall, plus
    /// pairwise `Neq` (conditioned on the residual) among the `count` terms of
    /// this signature. Idempotent (re-processing mints nothing new once the
    /// count is met) — the termination key for terms.
    fn ensure_terms(
        &mut self,
        v: ContextId,
        r: Role,
        core: &BTreeSet<Atom>,
        count: u32,
        residual: &Clause,
    ) {
        let u = self.intern_context(core.clone());
        // Existing live terms of `v` with this exact (role, ctx, residual)
        // signature. We tie the signature to the *edge residual* (so distinct
        // residuals don't share witnesses — matches B1 `link_successor` dedup).
        let mut want = residual.clone();
        want.sort_unstable();
        want.dedup();
        // Key idempotency on ALL terms of the signature (not just live ones): a
        // term that witnessed this `(role, ctx, residual)` is never re-minted,
        // removing any dependence on merge state. (With edge-only merges no term
        // is ever merged-away, but this stays robust if forced merges land.)
        let mut sig_terms: Vec<TermId> = Vec::new();
        for (gid, owner) in self.term_owner.iter().enumerate() {
            if *owner != v {
                continue;
            }
            let t = &self.terms[gid];
            if t.role == r && t.ctx == u && t.residual == want {
                sig_terms.push(gid);
            }
        }
        let have = u32::try_from(sig_terms.len()).unwrap_or(u32::MAX);
        let mut minted = sig_terms.clone();
        for _ in have..count {
            let gid = self.new_term(v, u, r, want.clone());
            self.register_edge(v, u, EdgeKind::Succ(r), residual.clone());
            minted.push(gid);
        }
        // Pairwise Neq among ALL terms of this signature (≥n distinctness),
        // conditioned on the residual (the n witnesses exist only under ¬N).
        if count >= 2 {
            for i in 0..minted.len() {
                for j in (i + 1)..minted.len() {
                    let mut h = residual.clone();
                    h.push(HeadLit::Neq(minted[i], minted[j]));
                    self.add_clause(v, h);
                }
            }
        }
    }

    /// Register an edge `v —kind→ u` with `residual` (deduped), if absent.
    fn register_edge(&mut self, v: ContextId, u: ContextId, kind: EdgeKind, mut residual: Clause) {
        residual.sort_unstable();
        residual.dedup();
        let exists = self.preds[u]
            .iter()
            .any(|(p, k, res)| *p == v && *k == kind && res == &residual);
        if !exists {
            self.preds[u].push((v, kind, residual));
            self.enqueue(u);
            self.enqueue(v);
        }
    }

    /// Outgoing `Succ` edges of `v`: `(role, successor, residual)`.
    fn succ_edges(&self, v: ContextId) -> Vec<(Role, ContextId, Clause)> {
        let mut out = Vec::new();
        for (u, edges) in self.preds.iter().enumerate() {
            for (p, kind, res) in edges {
                if *p == v
                    && let EdgeKind::Succ(r) = kind
                {
                    out.push((*r, u, res.clone()));
                }
            }
        }
        out
    }

    /// Tier-2 `r≤` choose rule (§2.2): record `≤n R.C` constraints and, when a
    /// context has `≥ n+1` live `C`-witnesses on `R`, derive the equality
    /// disjunction `⋁_{i<j} Eq(sᵢ,sⱼ)` (plus the chosen witnesses' residuals).
    fn apply_at_most(&mut self, v: ContextId) {
        let constraints = self.graph.contexts[v].at_most.clone();
        for (n, r, c) in constraints {
            // Collect live witnesses: terms s with role R' ⊑* R and C ∈ core(s.ctx)
            // (C = ⊤ ⇒ every term qualifies).
            let c_is_top = matches!(self.norm.pool.get(c), ConceptExpr::Top);
            let mut witnesses: Vec<TermId> = Vec::new();
            let live: Vec<TermId> = self.live_terms(v);
            for s in live {
                let t = &self.terms[s];
                if !self.role_subsumed(t.role, r) {
                    continue;
                }
                let qualifies = c_is_top || self.graph.contexts[t.ctx].core.contains(&c);
                if qualifies {
                    witnesses.push(s);
                }
            }
            let need = n as usize + 1;
            if witnesses.len() < need {
                continue;
            }
            // Choose every (n+1)-subset; emit the equality disjunction conditioned
            // on the chosen witnesses' edge residuals.
            self.choose_and_emit(v, r, &witnesses, need);
        }
    }

    /// For each `(n+1)`-subset of `witnesses`, derive
    /// `⋁(chosen residuals) ⊔ ⋁_{i<j} Eq(sᵢ,sⱼ)`.
    fn choose_and_emit(&mut self, v: ContextId, r: Role, witnesses: &[TermId], need: usize) {
        let _ = r;
        let idxs: Vec<usize> = (0..witnesses.len()).collect();
        let mut combo: Vec<usize> = Vec::with_capacity(need);
        let mut chosen_sets: Vec<Vec<TermId>> = Vec::new();
        Self::for_each_subset(&idxs, need, 0, &mut combo, &mut |sub| {
            chosen_sets.push(sub.iter().map(|&i| witnesses[i]).collect());
        });
        for chosen in chosen_sets {
            // Residual = ⋃(chosen witnesses' edge residuals): the equality
            // disjunction is conditioned on every disjunct that had to be true
            // for these witnesses to exist (§2.2, the FP guard).
            let mut head: Clause = Vec::new();
            for &s in &chosen {
                head.extend_from_slice(&self.terms[s].residual);
            }
            for i in 0..chosen.len() {
                for j in (i + 1)..chosen.len() {
                    head.push(HeadLit::Eq(chosen[i], chosen[j]));
                }
            }
            self.add_clause(v, head);
        }
    }

    /// All live (un-merged) terms owned by `v`, as global ids.
    fn live_terms(&self, v: ContextId) -> Vec<TermId> {
        self.term_owner
            .iter()
            .enumerate()
            .filter(|(gid, owner)| **owner == v && self.terms[*gid].merged_into.is_none())
            .map(|(gid, _)| gid)
            .collect()
    }

    /// Generate every `k`-subset of `items` (by index), invoking `f` on each.
    fn for_each_subset<F: FnMut(&[usize])>(
        items: &[usize],
        k: usize,
        start: usize,
        combo: &mut Vec<usize>,
        f: &mut F,
    ) {
        if combo.len() == k {
            f(combo);
            return;
        }
        for i in start..items.len() {
            combo.push(items[i]);
            Self::for_each_subset(items, k, i + 1, combo, f);
            combo.pop();
        }
    }

    /// §9.2 Eq-disjunction discharge rule (runs every `process`). For ANY stored
    /// clause with ≥1 `Eq` literal, spawn one speculative `merge_terms` per
    /// `Eq`-disjunct with `res = head \ {that Eq}` (the rest of the clause).
    fn apply_eq_discharge(&mut self, v: ContextId) {
        let clauses: Vec<Clause> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|dc| dc.head.clone())
            .collect();
        for head in &clauses {
            // Find Eq literals (canonical reps).
            let eqs: Vec<(usize, TermId, TermId)> = head
                .iter()
                .enumerate()
                .filter_map(|(i, l)| match l {
                    HeadLit::Eq(s, t) => Some((i, self.find(*s), self.find(*t))),
                    _ => None,
                })
                .collect();
            for (i, s, t) in eqs {
                if s == t {
                    continue; // reflexive — dropped on insert, but guard anyway.
                }
                let mut res = head.clone();
                res.remove(i);
                self.merge_terms(v, s, t, res);
            }
        }
    }

    /// §2.3/§9.2 speculative merge — **edge-only** (the load-bearing soundness +
    /// termination discipline). Find-or-create the union-core context and
    /// register a ⊥-back-prop `Merge` edge carrying `res`; do NOT mutate
    /// `merged_into`. The union-find binding is *local to this edge's reasoning*
    /// (the disjunction is NOT committed) — exactly like B1 `link_successor`,
    /// which mutates no union-find and discharges purely by the union-core's `⊥`
    /// back-propagating its residual. Committing the merge globally would (a)
    /// corrupt subsequent `Eq`/`Neq` canonicalization and (b) drop the witness
    /// below `n+1`, re-triggering `∃`-minting of a fresh witness — an unbounded
    /// merge↔re-mint oscillation. Dedup by `(v, union_core, res)` (§9.2).
    #[allow(clippy::needless_pass_by_value)] // logically consumes the residual
    fn merge_terms(&mut self, v: ContextId, s: TermId, t: TermId, res: Clause) {
        if s == t {
            return;
        }
        let union_core: BTreeSet<Atom> = {
            let core_s = &self.graph.contexts[self.terms[s].ctx].core;
            let core_t = &self.graph.contexts[self.terms[t].ctx].core;
            core_s.union(core_t).copied().collect()
        };
        let u = self.intern_context(union_core);
        self.register_edge(v, u, EdgeKind::Merge, res);
    }

    /// §10.1 general `Eq/Neq` resolution (closes the cardinality pigeonhole).
    ///
    /// For any two stored clauses at `v`, `C₁ = R ⊔ Eq(s,t)` and
    /// `C₂ = R′ ⊔ Neq(s,t)` where `(s,t)` are the SAME canonical pair (union-find
    /// representatives, min/max), derive `R ⊔ R′` via [`Self::add_clause`]. `R`,
    /// `R′` are the remaining literals (any mix of `Concept`/`Eq`/`Neq`, possibly
    /// empty). This is plain binary resolution on the complementary literal
    /// `(s=t)`/`(s≠t)`: `R ∨ R′` holds in **all** models, so it is FP-safe (§10.2)
    /// — it resolves only the sound stored clause heads (the `r≤` disjunction §4.1
    /// and the `≥n` `Neq` facts §2.1), never speculative-merge-internal state
    /// (merges are edge-only, never set `merged_into`, so `find` is identity here).
    ///
    /// The **unit case** `R = R′ = ∅` yields the empty clause = `⊥` — exactly the
    /// former same-pair clash (`tier2_neq_meets_forced_eq_is_bot`), now subsumed.
    ///
    /// Keying by the canonical pair gives FP-guard (a) structurally: `Eq` and
    /// `Neq` on DIFFERENT pairs land in different buckets and never cross-resolve.
    /// Lives entirely in the equality stratum — `apply_hyper` is untouched.
    fn apply_eq_resolution(&mut self, v: ContextId) {
        // Snapshot heads; bucket each clause's residual (clause minus the pivot
        // literal) by the canonical `Eq`/`Neq` pair it carries.
        let clauses: Vec<Clause> = self.graph.contexts[v]
            .clauses
            .iter()
            .map(|dc| dc.head.clone())
            .collect();
        let mut eq_by_pair: BTreeMap<(TermId, TermId), Vec<Clause>> = BTreeMap::new();
        let mut neq_by_pair: BTreeMap<(TermId, TermId), Vec<Clause>> = BTreeMap::new();
        for head in &clauses {
            for (i, lit) in head.iter().enumerate() {
                match lit {
                    HeadLit::Eq(s, t) => {
                        let (cs, ct) = (self.find(*s), self.find(*t));
                        if cs == ct {
                            continue; // reflexive — not a resolvable Eq.
                        }
                        let mut residual = head.clone();
                        residual.remove(i);
                        eq_by_pair
                            .entry((cs.min(ct), cs.max(ct)))
                            .or_default()
                            .push(residual);
                    }
                    HeadLit::Neq(s, t) => {
                        let (cs, ct) = (self.find(*s), self.find(*t));
                        if cs == ct {
                            continue; // Neq(s,s) is false — not a resolvable Neq.
                        }
                        let mut residual = head.clone();
                        residual.remove(i);
                        neq_by_pair
                            .entry((cs.min(ct), cs.max(ct)))
                            .or_default()
                            .push(residual);
                    }
                    HeadLit::Concept(_) => {}
                }
            }
        }
        let mut resolvents: Vec<Clause> = Vec::new();
        for (pair, eq_residuals) in &eq_by_pair {
            let Some(neq_residuals) = neq_by_pair.get(pair) else {
                continue;
            };
            for er in eq_residuals {
                for nr in neq_residuals {
                    let mut head = er.clone();
                    head.extend_from_slice(nr);
                    resolvents.push(head);
                }
            }
        }
        for h in resolvents {
            self.add_clause(v, h);
        }
    }

    /// Rule 5: back-propagation from `v`'s successors / merge edges.
    ///
    /// For each edge `v —(kind)→ u` (residual `N`): if `u` derives the empty
    /// clause (`u ⊑ ⊥`), reflect `N` to `v` (`v ⊑ ⊥` only when `N` is empty —
    /// the soundness landmine). `Succ` and `Merge` edges are treated
    /// identically here (⊥-back-prop only).
    fn apply_back_prop(&mut self, v: ContextId) {
        // Gather all edges out of v: (u, residual).
        let mut edges: Vec<(ContextId, Clause)> = Vec::new();
        for (u, ins) in self.preds.iter().enumerate() {
            for (p, _kind, res) in ins {
                if *p == v {
                    edges.push((u, res.clone()));
                }
            }
        }
        for (u, residual) in edges {
            let u_has_bot = self.graph.contexts[u]
                .clauses
                .iter()
                .any(|dc| dc.head.is_empty());
            if u_has_bot {
                self.add_clause(v, residual);
            }
        }
    }
}

/// Saturate the context graph under the consequence-based ALCHQ inference rules.
#[must_use]
pub(crate) fn saturate(norm: &Normalized) -> ContextGraph {
    let mut eng = Engine::new(norm);
    eng.run();
    eng.graph
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::CbHierarchy;
    use crate::classify::read_hierarchy;
    use crate::model::OntClause;
    use owl_dl_core::ir::{ClassId, ConceptPool, RoleId};

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

    // A ⊑ B ⊔ C, B ⊑ D, C ⊑ D  ⟹  A ⊑ D
    #[test]
    fn disjunctive_subsumption_by_cases() {
        let mut b = B::new();
        let (a, bb, c, d) = (b.class(0), b.class(1), b.class(2), b.class(3));
        let (ea, eb, ec, ed) = (b.atom(a), b.atom(bb), b.atom(c), b.atom(d));
        b.clause(vec![ea], vec![eb, ec]);
        b.clause(vec![eb], vec![ed]);
        b.clause(vec![ec], vec![ed]);
        let h = classify_built(&b.finish());
        assert!(subsumes(&h, 0, 3), "A ⊑ D by reasoning-by-cases");
    }

    #[test]
    fn disjunction_alone_no_unit_subsumption() {
        let mut b = B::new();
        let (a, bb, c) = (b.class(0), b.class(1), b.class(2));
        let (ea, eb, ec) = (b.atom(a), b.atom(bb), b.atom(c));
        b.clause(vec![ea], vec![eb, ec]);
        let h = classify_built(&b.finish());
        assert!(!subsumes(&h, 0, 1), "A ⊑ B must NOT be derived");
        assert!(!subsumes(&h, 0, 2), "A ⊑ C must NOT be derived");
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
