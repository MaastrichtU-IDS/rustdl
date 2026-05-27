//! Hyperresolution engine — hypertableau Phase H1 (Horn-only).
//!
//! See [`docs/hypertableau-scoping.md`](../../docs/hypertableau-scoping.md).
//! This is the first phase that *reasons*: it runs Horn-only
//! hyperresolution (DL-clauses with ≤1 head atom — no branching)
//! over a minimal class-labelled completion graph, with anywhere
//! blocking to terminate cyclic `∃`. Disjunctive heads (branching)
//! arrive in H2.
//!
//! It is **not** wired into the reasoner facade or the default
//! tableau — it's a standalone engine, validated in isolation
//! against hand-built Horn ontologies and (in a later step) the EL
//! saturation closure. The existing path is untouched.
//!
//! ## Why Horn is deterministic
//!
//! A clause `U1 ∧ … ∧ Um → V` fires only when its *whole* body
//! matches at a node (binding the central variable `x` and, if the
//! body has a role atom `R(x,y)`, a successor `y`). A single head
//! atom is then asserted with no choice — that's the
//! demand-driven, branch-free propagation that makes the ~96 %
//! Horn fragment of the corpus cheap (see
//! `docs/hypertableau-scoping.md` §H0).

use owl_dl_core::clause::{Atom, DlClause, Var, X};
use owl_dl_core::ir::{ClassId, Role, RoleId};

/// Node id in the hyper completion graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HNode(u32);

impl HNode {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Default, Clone)]
struct HyperNode {
    /// Class atoms true at this node — sorted by id, deduped.
    labels: Vec<ClassId>,
    /// Outgoing role edges `(role, target)`.
    edges: Vec<(Role, HNode)>,
    /// Creation order index — used by anywhere blocking ("blocked
    /// by an *earlier* node"). Equal to the node's own index here.
    order: u32,
}

impl HyperNode {
    fn has(&self, c: ClassId) -> bool {
        self.labels
            .binary_search_by_key(&c.index(), |l| l.index())
            .is_ok()
    }

    /// Insert a class label; returns true if newly added.
    fn add(&mut self, c: ClassId) -> bool {
        match self.labels.binary_search_by_key(&c.index(), |l| l.index()) {
            Ok(_) => false,
            Err(pos) => {
                self.labels.insert(pos, c);
                true
            }
        }
    }
}

/// Outcome of a Horn hyperresolution run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HyperResult {
    /// A clash-free completion exists (the root concept is
    /// satisfiable in the Horn fragment).
    Sat,
    /// A `body → ⊥` clause fired — the root concept is unsat.
    Unsat,
    /// The iteration cap was hit (defensive; shouldn't happen on
    /// well-formed Horn input thanks to anywhere blocking).
    Stalled,
}

/// The Horn hyperresolution engine. Holds the completion graph and
/// the clause set (borrowed).
pub struct HyperEngine<'c> {
    clauses: &'c [DlClause],
    nodes: Vec<HyperNode>,
}

impl<'c> HyperEngine<'c> {
    /// Build an engine for `clauses` seeded with a single root node
    /// labelled `root`.
    #[must_use]
    pub fn new(clauses: &'c [DlClause], root: ClassId) -> Self {
        let mut root_node = HyperNode {
            order: 0,
            ..HyperNode::default()
        };
        root_node.add(root);
        Self {
            clauses,
            nodes: vec![root_node],
        }
    }

    /// True iff every clause is Horn (≤1 head atom). H1 only
    /// handles this fragment; callers gate on it.
    #[must_use]
    pub fn all_horn(clauses: &[DlClause]) -> bool {
        clauses.iter().all(DlClause::is_horn)
    }

    fn new_node(&mut self) -> HNode {
        let id = u32::try_from(self.nodes.len()).expect("node count fits u32");
        self.nodes.push(HyperNode {
            order: id,
            ..HyperNode::default()
        });
        HNode(id)
    }

    /// Anywhere blocking: `n` is blocked if some *earlier-created*
    /// node `m` has `L(n) ⊆ L(m)`. A blocked node generates no
    /// successors (the witness `m` already realises everything `n`
    /// would). Sound for the Horn fragment (no inverse roles enter
    /// the blocking condition here; that refinement is H3).
    fn is_blocked(&self, n: HNode) -> bool {
        let ln = &self.nodes[n.index()];
        for m in &self.nodes {
            if m.order < ln.order && subset_sorted(&ln.labels, &m.labels) {
                return true;
            }
        }
        false
    }

    /// Run Horn hyperresolution to fixpoint. `max_iters` bounds the
    /// outer loop defensively.
    #[must_use]
    pub fn run(&mut self, max_iters: usize) -> HyperResult {
        for _ in 0..max_iters {
            let mut changed = false;
            // Snapshot node count; new successors are processed on
            // the next outer pass.
            let n_count = self.nodes.len();
            for idx in 0..n_count {
                let node = HNode(u32::try_from(idx).expect("fits u32"));
                match self.fire_clauses_at(node) {
                    FireOutcome::Clash => return HyperResult::Unsat,
                    FireOutcome::Changed => changed = true,
                    FireOutcome::NoChange => {}
                }
            }
            if !changed {
                return HyperResult::Sat;
            }
        }
        HyperResult::Stalled
    }

    /// Try every clause with the central variable bound to `node`.
    fn fire_clauses_at(&mut self, node: HNode) -> FireOutcome {
        let mut changed = false;
        // Clause indices snapshot is the static clause set; safe to
        // iterate while mutating the graph.
        for ci in 0..self.clauses.len() {
            match self.fire_clause(ci, node) {
                FireOutcome::Clash => return FireOutcome::Clash,
                FireOutcome::Changed => changed = true,
                FireOutcome::NoChange => {}
            }
        }
        if changed {
            FireOutcome::Changed
        } else {
            FireOutcome::NoChange
        }
    }

    /// Fire one clause with `x = node`. Handles the body shapes the
    /// H0 clausifier produces: class atoms on `x`, plus at most one
    /// role atom `R(x,y)` that binds a successor `y`.
    fn fire_clause(&mut self, ci: usize, node: HNode) -> FireOutcome {
        let clause = &self.clauses[ci];
        // Partition the body: class atoms on x, and an optional
        // role atom R(x, y).
        let mut role_atom: Option<(Role, Var)> = None;
        for atom in &clause.body {
            match atom {
                Atom::Class(c, v) if *v == X => {
                    if !self.nodes[node.index()].has(*c) {
                        return FireOutcome::NoChange;
                    }
                }
                Atom::Role(r, u, v) if *u == X => {
                    role_atom = Some((*r, *v));
                }
                // Body shapes outside the H0 clausifier output
                // (class on a non-x var, equality, multi-role) —
                // H1 doesn't match them; skip conservatively.
                _ => return FireOutcome::NoChange,
            }
        }

        match role_atom {
            None => {
                // Single-variable body matched at `node`. Fire head
                // with x = node.
                self.fire_head(ci, node, None)
            }
            Some((r, yvar)) => {
                // Body needs a successor: for each edge node -r-> m
                // (named-role match), bind y = m and fire.
                let edges: Vec<HNode> = self.nodes[node.index()]
                    .edges
                    .iter()
                    .filter(|(er, _)| role_matches(*er, r))
                    .map(|(_, t)| *t)
                    .collect();
                let mut changed = false;
                for m in edges {
                    match self.fire_head(ci, node, Some((yvar, m))) {
                        FireOutcome::Clash => return FireOutcome::Clash,
                        FireOutcome::Changed => changed = true,
                        FireOutcome::NoChange => {}
                    }
                }
                if changed {
                    FireOutcome::Changed
                } else {
                    FireOutcome::NoChange
                }
            }
        }
    }

    /// Assert the (single, Horn) head atom. `ybind` maps the body's
    /// successor variable to a node when present.
    fn fire_head(&mut self, ci: usize, xnode: HNode, ybind: Option<(Var, HNode)>) -> FireOutcome {
        let clause = &self.clauses[ci];
        if clause.head.is_empty() {
            // body → ⊥ : the body matched, so this is a clash.
            return FireOutcome::Clash;
        }
        // Horn: exactly one head atom (caller gated on all_horn).
        let head = clause.head[0];
        let resolve = |v: Var| -> Option<HNode> {
            if v == X {
                Some(xnode)
            } else if let Some((yv, yn)) = ybind {
                if v == yv { Some(yn) } else { None }
            } else {
                None
            }
        };
        match head {
            Atom::Class(c, v) => {
                let Some(target) = resolve(v) else {
                    return FireOutcome::NoChange;
                };
                if self.nodes[target.index()].add(c) {
                    FireOutcome::Changed
                } else {
                    FireOutcome::NoChange
                }
            }
            Atom::Exists(role, cls, v) => {
                let Some(src) = resolve(v) else {
                    return FireOutcome::NoChange;
                };
                self.fire_exists(src, role, cls)
            }
            // Equality heads (≤n) are H3; not produced for Horn EL.
            Atom::Equal(_, _) | Atom::Role(..) => FireOutcome::NoChange,
        }
    }

    /// `∃role.cls` at `src`: reuse an existing role-successor that
    /// already carries `cls`; otherwise (if `src` isn't blocked)
    /// create a fresh successor seeded with `cls`.
    fn fire_exists(&mut self, src: HNode, role: Role, cls: ClassId) -> FireOutcome {
        // Witness reuse: any role-matching successor already in cls.
        let has_witness = self.nodes[src.index()]
            .edges
            .iter()
            .any(|(er, t)| role_matches(*er, role) && self.nodes[t.index()].has(cls));
        if has_witness {
            return FireOutcome::NoChange;
        }
        if self.is_blocked(src) {
            // Blocked: the witness ancestor already realises this
            // existential; don't generate.
            return FireOutcome::NoChange;
        }
        let succ = self.new_node();
        self.nodes[succ.index()].add(cls);
        self.nodes[src.index()].edges.push((role, succ));
        FireOutcome::Changed
    }

    /// Number of nodes in the completion graph (diagnostic).
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Class labels of the root node (node 0) — the derived
    /// subsumers of the root concept, for EL-closure cross-checks.
    #[must_use]
    pub fn root_labels(&self) -> &[ClassId] {
        &self.nodes[0].labels
    }
}

enum FireOutcome {
    Clash,
    Changed,
    NoChange,
}

/// Two named roles match if their underlying `RoleId`s are equal.
/// Inverse-role and sub-role matching are H3 refinements; H1's
/// clausifier emits named roles only.
fn role_matches(edge: Role, wanted: Role) -> bool {
    fn id(r: Role) -> RoleId {
        match r {
            Role::Named(x) | Role::Inverse(x) => x,
        }
    }
    edge.is_inverse() == wanted.is_inverse() && id(edge) == id(wanted)
}

/// `a ⊆ b` for sorted-by-index class-id slices.
fn subset_sorted(a: &[ClassId], b: &[ClassId]) -> bool {
    let mut bi = b.iter();
    'outer: for x in a {
        for y in bi.by_ref() {
            if y.index() == x.index() {
                continue 'outer;
            }
            if y.index() > x.index() {
                return false;
            }
        }
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::{ClassId, Role, RoleId};

    fn cls(i: u32) -> ClassId {
        ClassId::new(i)
    }

    #[test]
    fn horn_chain_derives_transitive_subsumers() {
        // A(x)→B(x), B(x)→C(x). Root A ⇒ root labels {A,B,C}, Sat.
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(1), X)],
                head: vec![Atom::Class(cls(2), X)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        assert_eq!(e.root_labels(), &[cls(0), cls(1), cls(2)]);
    }

    #[test]
    fn disjointness_clause_makes_root_unsat() {
        // A(x)→B(x), A(x)∧B(x)→⊥. Root A ⇒ Unsat.
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Class(cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(0), X), Atom::Class(cls(1), X)],
                head: vec![],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Unsat);
    }

    #[test]
    fn cyclic_existential_terminates_via_blocking() {
        // A(x)→∃R.A(x). Naively infinite; anywhere blocking caps it.
        let r = Role::Named(RoleId::new(0));
        let clauses = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Exists(r, cls(0), X)],
        }];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        // Root + one successor, then the successor is blocked by the
        // root (same label set {A}).
        assert!(
            e.node_count() <= 2,
            "blocking should cap at 2 nodes, got {}",
            e.node_count()
        );
    }

    #[test]
    fn forall_propagates_into_successor() {
        // A(x)→∃R.B(x); A(x)∧R(x,y)→C(y). The R-successor (seeded B)
        // also gains C. Root stays sat.
        let r = Role::Named(RoleId::new(0));
        let clauses = vec![
            DlClause {
                body: vec![Atom::Class(cls(0), X)],
                head: vec![Atom::Exists(r, cls(1), X)],
            },
            DlClause {
                body: vec![Atom::Class(cls(0), X), Atom::Role(r, X, 1)],
                head: vec![Atom::Class(cls(2), 1)],
            },
        ];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        // Two nodes: root {A}, successor {B,C}.
        assert_eq!(e.node_count(), 2);
    }

    #[test]
    fn universal_body_fires_everywhere() {
        // ⊤(x)→T(x): every node gains T. Root A ⇒ {A,T}.
        let clauses = vec![DlClause {
            body: vec![],
            head: vec![Atom::Class(cls(9), X)],
        }];
        let mut e = HyperEngine::new(&clauses, cls(0));
        assert_eq!(e.run(1024), HyperResult::Sat);
        assert_eq!(e.root_labels(), &[cls(0), cls(9)]);
    }

    #[test]
    fn all_horn_detects_disjunctive_clause() {
        let horn = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X)],
        }];
        assert!(HyperEngine::all_horn(&horn));
        let disj = vec![DlClause {
            body: vec![Atom::Class(cls(0), X)],
            head: vec![Atom::Class(cls(1), X), Atom::Class(cls(2), X)],
        }];
        assert!(!HyperEngine::all_horn(&disj));
    }
}
