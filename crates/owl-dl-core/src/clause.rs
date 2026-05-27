//! DL-clause representation and clausifier — hypertableau Phase H0.
//!
//! See [`docs/hypertableau-scoping.md`](../../docs/hypertableau-scoping.md).
//! This module produces DL-clauses from an ontology but performs
//! **no reasoning** — Phase H0 ships the clausifier and a
//! statistics aggregator only, so the clause-shape distribution of
//! the corpus can be measured before the hypertableau engine
//! (H1+) is built. The existing absorb/saturate/tableau path is
//! untouched.
//!
//! ## Form
//!
//! A DL-clause is `U1 ∧ … ∧ Um → V1 ∨ … ∨ Vn`: a conjunctive body
//! of [`Atom`]s implying a disjunctive head of [`Atom`]s. An empty
//! head is `⊥` (a clash); an empty body is `⊤` (the head holds
//! universally). Hyperresolution (H1+) fires a clause only when
//! its *entire* body matches — that's what makes branching
//! demand-driven rather than eager.
//!
//! ## Clausifying from the absorbed `TBox`
//!
//! Rather than re-derive the structural transformation from
//! scratch, Phase H0 clausifies the [`crate::absorb::AbsorbedTBox`]:
//! absorption has already split each axiom into a single trigger
//! (`ConceptRule`), a universal body (`residual_gcis`), or a
//! role-propagation (`RoleRule`). Only the *head* concept needs
//! clausifying, which keeps the polarity handling local. Compound
//! head sub-concepts that don't map to a single atom (nested `∃`,
//! `Or`) get a fresh structural name and an auxiliary clause —
//! standard Tseitin naming, which the saturation engine already
//! uses in limited form.

use crate::absorb::{AbsorbedTBox, absorb};
use crate::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, Role};
use crate::normalize::nnf_axioms;
use crate::ontology::InternalOntology;

/// A clause variable. `X` (0) is the central individual; 1.. are
/// successors introduced by role atoms.
pub type Var = u32;

/// The central individual variable `x`.
pub const X: Var = 0;

/// An atom in a DL-clause body or head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atom {
    /// `A(v)` — variable `v` is an instance of class `A`.
    Class(ClassId, Var),
    /// `R(u, v)` — `u` is related to `v` by role `R` (named
    /// polarity; inverse handled by the role's own polarity at
    /// match time in H1+).
    Role(Role, Var, Var),
    /// Head-only: `v` must have an `R`-successor in class `A`
    /// (`∃R.A(v)`). The hypertableau generation step (H1) realises
    /// it by creating a successor.
    Exists(Role, ClassId, Var),
    /// `u ≈ v` — equality, for `≤n` / functional reasoning (H3).
    Equal(Var, Var),
}

/// A DL-clause: `body (∧) → head (∨)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DlClause {
    pub body: Vec<Atom>,
    pub head: Vec<Atom>,
}

impl DlClause {
    /// A clause with an empty head encodes `body → ⊥` (a clash
    /// when the body matches).
    #[must_use]
    pub fn is_bottom_headed(&self) -> bool {
        self.head.is_empty()
    }

    /// A clause with at most one head atom is Horn — it fires
    /// deterministically (no branching) in hyperresolution.
    #[must_use]
    pub fn is_horn(&self) -> bool {
        self.head.len() <= 1
    }
}

/// Clausifier state. Allocates fresh structural-name [`ClassId`]s
/// starting past the vocabulary's real classes; these have no IRI
/// (internal Tseitin names) but are valid clause atoms.
struct Clausifier<'a> {
    pool: &'a ConceptPool,
    clauses: Vec<DlClause>,
    next_fresh: u32,
    /// Constructs not yet handled (cardinality, nominals, deep
    /// shapes). Counted, not clausified — surfaced by
    /// [`ClauseStats`] so coverage is measurable.
    deferred: usize,
}

impl<'a> Clausifier<'a> {
    fn new(pool: &'a ConceptPool, first_fresh: u32) -> Self {
        Self {
            pool,
            clauses: Vec::new(),
            next_fresh: first_fresh,
            deferred: 0,
        }
    }

    fn fresh_class(&mut self) -> ClassId {
        let id = ClassId::new(self.next_fresh);
        self.next_fresh = self
            .next_fresh
            .checked_add(1)
            .expect("fresh class id overflow");
        id
    }

    /// Clausify `trigger(x) → head_concept(x)`. `trigger` is
    /// `Some` for a `ConceptRule`, `None` for a residual GCI
    /// (`⊤` body).
    fn clausify_rule(&mut self, trigger: Option<ClassId>, head_concept: ConceptId) {
        let base_body: Vec<Atom> = trigger.map(|t| vec![Atom::Class(t, X)]).unwrap_or_default();
        self.emit_head(base_body, head_concept, X);
    }

    /// Emit clause(s) for `body → head_concept(var)`. Splits `And`
    /// heads into multiple clauses, encodes `Or` as a disjunctive
    /// head, names nested compounds.
    fn emit_head(&mut self, body: Vec<Atom>, head_concept: ConceptId, var: Var) {
        match self.pool.get(head_concept) {
            ConceptExpr::Top => { /* trivially true; no clause */ }
            ConceptExpr::Bot => {
                // body → ⊥
                self.clauses.push(DlClause {
                    body,
                    head: Vec::new(),
                });
            }
            ConceptExpr::Atomic(a) => {
                self.clauses.push(DlClause {
                    body,
                    head: vec![Atom::Class(*a, var)],
                });
            }
            ConceptExpr::And(parts) => {
                // body → (P1 ⊓ … ⊓ Pn): one clause per conjunct.
                for &p in parts {
                    self.emit_head(body.clone(), p, var);
                }
            }
            ConceptExpr::Or(parts) => {
                // body → (D1 ∨ … ∨ Dn): a single disjunctive-head
                // clause. Each disjunct must map to a single head
                // atom; compound disjuncts get a structural name.
                let mut head: Vec<Atom> = Vec::with_capacity(parts.len());
                for &p in parts {
                    if let Some(atom) = self.head_atom_for(p, var) {
                        head.push(atom);
                    } else {
                        self.deferred += 1;
                        return;
                    }
                }
                self.clauses.push(DlClause { body, head });
            }
            ConceptExpr::Some(role, inner) => {
                // body → ∃role.inner(var). Name `inner` if compound.
                if let Some(cls) = self.atomic_name_of(*inner) {
                    self.clauses.push(DlClause {
                        body,
                        head: vec![Atom::Exists(*role, cls, var)],
                    });
                } else {
                    self.deferred += 1;
                }
            }
            ConceptExpr::All(role, inner) => {
                // body ∧ role(var, y) → inner(y).
                let y = var + 1;
                let mut b = body;
                b.push(Atom::Role(*role, var, y));
                self.emit_head(b, *inner, y);
            }
            ConceptExpr::Not(inner) => {
                // body → ¬C  ≡  body ∧ C → ⊥. Only handled when
                // `C` is atomic (the common disjointness shape);
                // a nested negation under NNF shouldn't occur.
                if let ConceptExpr::Atomic(a) = self.pool.get(*inner) {
                    let mut b = body;
                    b.push(Atom::Class(*a, var));
                    self.clauses.push(DlClause {
                        body: b,
                        head: Vec::new(),
                    });
                } else {
                    self.deferred += 1;
                }
            }
            // Cardinality, nominals, self-restriction: deferred to
            // later hypertableau phases (H3). Counted for coverage.
            ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_) => {
                self.deferred += 1;
            }
        }
    }

    /// Map a head disjunct to a single head atom, naming compounds.
    /// Returns `None` if the disjunct can't be expressed as one
    /// atom yet (caller defers).
    fn head_atom_for(&mut self, c: ConceptId, var: Var) -> Option<Atom> {
        match self.pool.get(c) {
            ConceptExpr::Atomic(a) => Some(Atom::Class(*a, var)),
            ConceptExpr::Some(role, inner) => {
                let cls = self.atomic_name_of(*inner)?;
                Some(Atom::Exists(*role, cls, var))
            }
            ConceptExpr::Not(inner) => {
                // ¬A as a disjunct: name it with a fresh class Q
                // and the auxiliary clause Q ⊓ A → ⊥, i.e. Q means
                // "¬A". (H1 treats Q as the negative literal.)
                if let ConceptExpr::Atomic(a) = self.pool.get(*inner) {
                    let q = self.fresh_class();
                    self.clauses.push(DlClause {
                        body: vec![Atom::Class(q, var), Atom::Class(*a, var)],
                        head: Vec::new(),
                    });
                    Some(Atom::Class(q, var))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Return the [`ClassId`] naming `c`: itself if already atomic,
    /// else a fresh structural name `Q` with `Q ⊑ c` clausified so
    /// the successor seeded with `Q` carries `c`'s consequences.
    fn atomic_name_of(&mut self, c: ConceptId) -> Option<ClassId> {
        if let ConceptExpr::Atomic(a) = self.pool.get(c) {
            return Some(*a);
        }
        // Fresh Q with Q(x) → c(x). Bounded recursion via emit_head.
        let q = self.fresh_class();
        self.emit_head(vec![Atom::Class(q, X)], c, X);
        Some(q)
    }
}

/// Clausify an ontology into DL-clauses (Phase H0). Runs NNF +
/// absorption, then clausifies every absorbed rule. Returns the
/// clauses; does **not** reason.
#[must_use]
pub fn clausify(internal: &InternalOntology) -> Vec<DlClause> {
    clausify_with_stats(internal).0
}

/// Clausify and also return the coverage [`ClauseStats`].
#[must_use]
pub fn clausify_with_stats(internal: &InternalOntology) -> (Vec<DlClause>, ClauseStats) {
    let mut internal = internal.clone();
    let normalized = nnf_axioms(&mut internal);
    let tbox = absorb(&normalized, &mut internal.concepts);
    let first_fresh =
        u32::try_from(internal.vocabulary.num_classes()).expect("class count fits in u32");
    let mut c = Clausifier::new(&internal.concepts, first_fresh);
    clausify_tbox(&mut c, &tbox);
    let stats = ClauseStats::of(&c.clauses, c.deferred);
    (c.clauses, stats)
}

fn clausify_tbox(c: &mut Clausifier<'_>, tbox: &AbsorbedTBox) {
    for rule in &tbox.concept_rules {
        c.clausify_rule(Some(rule.trigger), rule.conclusion);
    }
    for &gci in &tbox.residual_gcis {
        c.clausify_rule(None, gci);
    }
    // Role rules: `[guard(x) ∧] role(x,y) → target(y)`.
    for rr in &tbox.role_rules {
        let y = X + 1;
        let mut body = Vec::new();
        if let Some(g) = rr.guard {
            body.push(Atom::Class(g, X));
        }
        body.push(Atom::Role(rr.role, X, y));
        // target_label is a head concept at `y`.
        c.emit_head(body, rr.target_label, y);
    }
    // Nominal rules deferred (H3).
    c.deferred += tbox.nominal_rules.len();
    let _ = &tbox.guarded_role_rules_by_guard; // indices; covered via role_rules
    let _ = &tbox.unguarded_role_rules;
    let _ = &tbox.concept_rules_by_trigger;
    let _ = &tbox.nominal_rules_by_individual;
}

/// Shape histogram of a clause set, for the `rustdl clause-stats`
/// diagnostic. `deferred` counts head constructs the H0 clausifier
/// doesn't yet handle (cardinality, nominals, deep shapes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClauseStats {
    pub total: usize,
    pub horn: usize,
    pub disjunctive: usize,
    pub bottom_headed: usize,
    pub with_exists_head: usize,
    pub deferred: usize,
}

impl ClauseStats {
    #[must_use]
    fn of(clauses: &[DlClause], deferred: usize) -> Self {
        let mut s = ClauseStats {
            total: clauses.len(),
            deferred,
            ..ClauseStats::default()
        };
        for cl in clauses {
            if cl.is_bottom_headed() {
                s.bottom_headed += 1;
            }
            if cl.is_horn() {
                s.horn += 1;
            } else {
                s.disjunctive += 1;
            }
            if cl.head.iter().any(|a| matches!(a, Atom::Exists(..))) {
                s.with_exists_head += 1;
            }
        }
        s
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::convert::convert_ontology;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn clausify_ofn(src: &str) -> (Vec<DlClause>, ClauseStats) {
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("parse");
        let internal = convert_ontology(&onto).expect("convert");
        clausify_with_stats(&internal)
    }

    const HEADER: &str = "Prefix(:=<http://x/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    #[test]
    fn told_subsumption_is_a_horn_clause() {
        // A ⊑ B  →  A(x) → B(x)
        let (clauses, stats) = clausify_ofn(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
SubClassOf(:A :B)\n)\n"
        ));
        assert!(stats.horn >= 1);
        assert!(
            clauses
                .iter()
                .any(|c| c.body.len() == 1 && c.head.len() == 1)
        );
    }

    #[test]
    fn covering_axiom_yields_a_disjunctive_clause() {
        // A ⊑ B ⊔ C  →  A(x) → B(x) ∨ C(x)
        let (_clauses, stats) = clausify_ofn(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
SubClassOf(:A ObjectUnionOf(:B :C))\n)\n"
        ));
        assert!(
            stats.disjunctive >= 1,
            "expected a disjunctive clause, stats={stats:?}"
        );
    }

    #[test]
    fn disjointness_yields_a_bottom_headed_clause() {
        // DisjointClasses(A, B) → A(x) ∧ B(x) → ⊥
        let (_clauses, stats) = clausify_ofn(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
DisjointClasses(:A :B)\n)\n"
        ));
        assert!(
            stats.bottom_headed >= 1,
            "expected a ⊥-headed clause, stats={stats:?}"
        );
    }

    #[test]
    fn existential_yields_an_exists_head() {
        // A ⊑ ∃R.B  →  A(x) → ∃R.B(x)
        let (_clauses, stats) = clausify_ofn(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(ObjectProperty(:r))\n\
SubClassOf(:A ObjectSomeValuesFrom(:r :B))\n)\n"
        ));
        assert!(
            stats.with_exists_head >= 1,
            "expected an ∃-head clause, stats={stats:?}"
        );
    }

    #[test]
    fn universal_moves_role_into_body() {
        // A ⊑ ∀R.B  →  A(x) ∧ R(x,y) → B(y)
        let (clauses, _stats) = clausify_ofn(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(ObjectProperty(:r))\n\
SubClassOf(:A ObjectAllValuesFrom(:r :B))\n)\n"
        ));
        // Some clause has a Role atom in its body and a Class head
        // on a non-X variable.
        assert!(clauses.iter().any(|c| {
            c.body.iter().any(|a| matches!(a, Atom::Role(..)))
                && c.head
                    .iter()
                    .any(|a| matches!(a, Atom::Class(_, v) if *v != X))
        }));
    }
}
