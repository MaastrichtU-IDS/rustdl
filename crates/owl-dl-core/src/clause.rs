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

use crate::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, Role};
use crate::normalize::nnf_axioms;
use crate::ontology::{Axiom, InternalOntology};

/// A clause variable. `X` (0) is the central individual; 1.. are
/// successors introduced by role atoms.
pub type Var = u32;

/// The central individual variable `x`.
pub const X: Var = 0;

/// Cap on the number of alternative bodies an antecedent may
/// distribute into (DNF cross-product). Antecedents that would
/// exceed it are deferred rather than blow up the clause set. Real
/// ontologies have at most one or two `Or`s per antecedent; this is
/// a guard against pathological inputs, not a normal-case limit.
const ANTECEDENT_DNF_CAP: usize = 64;

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
    /// Head-only: `v` has at most `n` `R`-successors (in class `A`
    /// when the qualifier is `Some`, else unqualified). The `≤n`
    /// constraint for cardinality (H3c); enforced by the merge rule.
    AtMost(Role, Option<ClassId>, u32, Var),
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
    /// Base id of the nominal-class region: a nominal `{a}` is treated
    /// as the atomic class `nominal_base + a.index()`. Reserved
    /// `[num_classes, num_classes + num_individuals)`, before structural
    /// names. Treating a nominal as a plain class is a sound
    /// under-approximation for the refutation direction (it only loses
    /// the singleton-equality constraint, which can only *add* clashes).
    nominal_base: u32,
    /// Next fresh clause variable, reset per axiom (X is always 0;
    /// successors introduced by nested `∃`/`∀` take 1, 2, …).
    next_var: Var,
    /// Constructs not yet handled (antecedent `∀`/`Or`/`Not`,
    /// cardinality, deep shapes). Counted, not clausified — surfaced
    /// by [`ClauseStats`] so coverage is measurable.
    deferred: usize,
}

impl<'a> Clausifier<'a> {
    fn new(pool: &'a ConceptPool, first_fresh: u32, nominal_base: u32) -> Self {
        Self {
            pool,
            clauses: Vec::new(),
            next_fresh: first_fresh,
            nominal_base,
            next_var: X + 1,
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

    /// The atomic class naming `c` directly: `c` itself if atomic, or
    /// the reserved nominal class for a nominal `{a}`. `None` for
    /// compound concepts (which need a structural name instead).
    fn class_id_of(&self, c: ConceptId) -> Option<ClassId> {
        match self.pool.get(c) {
            ConceptExpr::Atomic(a) => Some(*a),
            ConceptExpr::Nominal(i) => Some(ClassId::new(self.nominal_base + i.index())),
            _ => None,
        }
    }

    fn fresh_var(&mut self) -> Var {
        let v = self.next_var;
        self.next_var = self.next_var.checked_add(1).expect("fresh var overflow");
        v
    }

    /// Clausify one (NNF) axiom via structural transformation.
    /// This is the H1c entry — it works from the GCI structure
    /// directly (not the absorbed `TBox`), so an antecedent `∃`
    /// becomes a body role+class pair (`∃R.E ⊑ F` →
    /// `R(x,y) ∧ E(y) → F(x)`), the shape the absorbed route lost.
    fn clausify_axiom(&mut self, ax: &Axiom) {
        match ax {
            Axiom::SubClassOf { sub, sup } => self.clausify_gci(*sub, *sup),
            Axiom::EquivalentClasses(ids) => {
                // Every ordered pair `A ⊑ B`.
                for (i, &a) in ids.iter().enumerate() {
                    for (j, &b) in ids.iter().enumerate() {
                        if i != j {
                            self.clausify_gci(a, b);
                        }
                    }
                }
            }
            Axiom::DisjointClasses(ids) => {
                // Pairwise `Ai ⊓ Aj ⊑ ⊥`. Each antecedent may now be
                // a DNF; emit a ⊥-clause per alternative-pair.
                for i in 0..ids.len() {
                    for j in (i + 1)..ids.len() {
                        self.next_var = X + 1;
                        let (Some(alts_a), Some(alts_b)) = (
                            self.encode_antecedent(ids[i], X),
                            self.encode_antecedent(ids[j], X),
                        ) else {
                            self.deferred += 1;
                            continue;
                        };
                        for a in &alts_a {
                            for b in &alts_b {
                                let mut body = a.clone();
                                body.extend_from_slice(b);
                                self.clauses.push(DlClause {
                                    body,
                                    head: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
            Axiom::DisjointUnion { class, members } => {
                // class ≡ ⊔members, plus pairwise disjoint members.
                let cls_concept_eq = members; // handled below via gci on the union
                let _ = cls_concept_eq;
                // class ⊑ ⊔members and each member ⊑ class:
                // approximate via the union concept if present is
                // complex; defer the equivalence half, emit the
                // pairwise disjointness which is the load-bearing
                // part for unsat detection.
                for i in 0..members.len() {
                    for j in (i + 1)..members.len() {
                        self.next_var = X + 1;
                        if let (Some(alts_a), Some(alts_b)) = (
                            self.encode_antecedent(members[i], X),
                            self.encode_antecedent(members[j], X),
                        ) {
                            for a in &alts_a {
                                for b in &alts_b {
                                    let mut body = a.clone();
                                    body.extend_from_slice(b);
                                    self.clauses.push(DlClause {
                                        body,
                                        head: Vec::new(),
                                    });
                                }
                            }
                        }
                    }
                }
                // member ⊑ class (each member implies the union class)
                for &m in members {
                    self.next_var = X + 1;
                    if let Some(bodies) = self.encode_antecedent(m, X) {
                        for body in bodies {
                            self.clauses.push(DlClause {
                                body,
                                head: vec![Atom::Class(*class, X)],
                            });
                        }
                    } else {
                        self.deferred += 1;
                    }
                }
            }
            Axiom::ObjectPropertyDomain { role, domain } => {
                // ∃role.⊤ ⊑ domain  →  role(x,y) → domain(x)
                self.next_var = X + 1;
                let y = self.fresh_var();
                self.clausify_consequent(vec![Atom::Role(*role, X, y)], *domain, X);
            }
            Axiom::ObjectPropertyRange { role, range } => {
                // ⊤ ⊑ ∀role.range  →  role(x,y) → range(y)
                self.next_var = X + 1;
                let y = self.fresh_var();
                self.clausify_consequent(vec![Atom::Role(*role, X, y)], *range, y);
            }
            // RBox (role chains/characteristics), ABox, declarations:
            // not class clauses. RBox role propagation is H3.
            _ => {}
        }
    }

    /// Clausify a GCI `sub ⊑ sup`. A top-level antecedent `Or`
    /// splits into one clause per disjunct (`(A ⊔ B) ⊑ D` ≡
    /// `A ⊑ D ∧ B ⊑ D`). Otherwise encode the antecedent into a
    /// body and recurse into the consequent.
    fn clausify_gci(&mut self, sub: ConceptId, sup: ConceptId) {
        self.next_var = X + 1;
        match self.encode_antecedent(sub, X) {
            // The antecedent is a disjunction of conjunctions (DNF):
            // `(c11⊓…) ⊔ (c21⊓…) ⊑ sup` ≡ one GCI per disjunct, each
            // `(ci1⊓…) ⊑ sup`. Emit a consequent clause per body.
            Some(bodies) => {
                for body in bodies {
                    self.clausify_consequent(body, sup, X);
                }
            }
            None => self.deferred += 1,
        }
    }

    /// Encode an antecedent concept `c` (which must hold at `var`)
    /// into **disjunctive normal form**: a list of alternative bodies,
    /// each a conjunction of atoms. `A ⊓ (B ⊔ C)` yields `[[A,B],
    /// [A,C]]`; the caller emits one clause per alternative. `None`
    /// when the shape isn't supported yet (antecedent `∀`/`Not`/
    /// cardinality/nominal — deferred) or the cross-product exceeds
    /// [`ANTECEDENT_DNF_CAP`] (deferred rather than blow up).
    fn encode_antecedent(&mut self, c: ConceptId, var: Var) -> Option<Vec<Vec<Atom>>> {
        // Atomic / nominal antecedent atom: a single one-literal body.
        if let Some(cls) = self.class_id_of(c) {
            return Some(vec![vec![Atom::Class(cls, var)]]);
        }
        match self.pool.get(c) {
            // `⊤`: a single empty-conjunction alternative.
            ConceptExpr::Top => Some(vec![Vec::new()]),
            // `Atomic`/`Nominal` handled by the early return above.
            ConceptExpr::And(parts) => {
                // Cross-product: each conjunct contributes alternatives;
                // the And's alternatives are all combinations.
                let parts: Vec<ConceptId> = parts.to_vec();
                let mut acc: Vec<Vec<Atom>> = vec![Vec::new()];
                for p in parts {
                    let child = self.encode_antecedent(p, var)?;
                    let mut next = Vec::with_capacity(acc.len() * child.len());
                    for a in &acc {
                        for b in &child {
                            let mut combined = a.clone();
                            combined.extend_from_slice(b);
                            next.push(combined);
                        }
                    }
                    if next.len() > ANTECEDENT_DNF_CAP {
                        return None;
                    }
                    acc = next;
                }
                Some(acc)
            }
            ConceptExpr::Or(parts) => {
                // Union: each disjunct's alternatives, concatenated.
                let parts: Vec<ConceptId> = parts.to_vec();
                let mut out = Vec::new();
                for p in parts {
                    out.extend(self.encode_antecedent(p, var)?);
                    if out.len() > ANTECEDENT_DNF_CAP {
                        return None;
                    }
                }
                Some(out)
            }
            ConceptExpr::Some(role, inner) => {
                // ∃role.inner in the antecedent → role(var, y) plus
                // inner's body atoms at the fresh successor y. One
                // fresh `y` for this occurrence, shared across the
                // occurrence's alternatives (each alternative becomes
                // a *separate* clause, so a shared id is clause-local
                // and sound). The H1b-gap shape, now also distributed.
                let (role, inner) = (*role, *inner);
                let y = self.fresh_var();
                let inner_alts = self.encode_antecedent(inner, y)?;
                let mut out = Vec::with_capacity(inner_alts.len());
                for alt in inner_alts {
                    let mut body = vec![Atom::Role(role, var, y)];
                    body.extend(alt);
                    out.push(body);
                }
                Some(out)
            }
            // Antecedent `∀`/`Not`/cardinality: deferred to later
            // phases (H3 ∀-in-body / cardinality). `Atomic`/`Nominal`
            // are handled by the early return above.
            ConceptExpr::All(_, _)
            | ConceptExpr::Not(_)
            | ConceptExpr::Bot
            | ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _)
            | ConceptExpr::Atomic(_)
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_) => None,
        }
    }

    /// Clausify `body → sup(var)` (the consequent side). Renamed
    /// from the H0 `emit_head`; unchanged in behaviour but now uses
    /// per-clause fresh variables for nested `∀`.
    fn clausify_consequent(&mut self, body: Vec<Atom>, sup: ConceptId, var: Var) {
        self.emit_head(body, sup, var);
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
            ConceptExpr::Atomic(_) | ConceptExpr::Nominal(_) => {
                let cls = self.class_id_of(head_concept).expect("atomic/nominal");
                self.clauses.push(DlClause {
                    body,
                    head: vec![Atom::Class(cls, var)],
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
                let (role, inner) = (*role, *inner);
                let y = self.fresh_var();
                let mut b = body;
                b.push(Atom::Role(role, var, y));
                self.emit_head(b, inner, y);
            }
            ConceptExpr::Not(inner) => {
                // body → ¬C  ≡  body ∧ C → ⊥. Only handled when
                // `C` is atomic (the common disjointness shape);
                // a nested negation under NNF shouldn't occur.
                if let Some(a) = self.class_id_of(*inner) {
                    let mut b = body;
                    b.push(Atom::Class(a, var));
                    self.clauses.push(DlClause {
                        body: b,
                        head: Vec::new(),
                    });
                } else {
                    self.deferred += 1;
                }
            }
            // Cardinality, self-restriction: deferred to later phases
            // (H3). Counted for coverage.
            ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _)
            | ConceptExpr::SelfRestriction(_) => {
                self.deferred += 1;
            }
        }
    }

    /// Map a head disjunct to a single head atom, naming compounds.
    /// Returns `None` if the disjunct can't be expressed as one
    /// atom yet (caller defers).
    fn head_atom_for(&mut self, c: ConceptId, var: Var) -> Option<Atom> {
        if let Some(cls) = self.class_id_of(c) {
            return Some(Atom::Class(cls, var));
        }
        match self.pool.get(c) {
            ConceptExpr::Some(role, inner) => {
                let cls = self.atomic_name_of(*inner)?;
                Some(Atom::Exists(*role, cls, var))
            }
            ConceptExpr::Not(inner) => {
                // ¬A as a disjunct: name it with a fresh class Q
                // and the auxiliary clause Q ⊓ A → ⊥, i.e. Q means
                // "¬A". (H1 treats Q as the negative literal.)
                if let Some(a) = self.class_id_of(*inner) {
                    let q = self.fresh_class();
                    self.clauses.push(DlClause {
                        body: vec![Atom::Class(q, var), Atom::Class(a, var)],
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
        if let Some(cls) = self.class_id_of(c) {
            return Some(cls);
        }
        // Fresh Q with Q(x) → c(x). Bounded recursion via emit_head.
        let q = self.fresh_class();
        self.emit_head(vec![Atom::Class(q, X)], c, X);
        Some(q)
    }
}

/// Clausify an ontology into DL-clauses. Runs NNF then a
/// structural transformation over the GCI axioms (H1c). Returns
/// the clauses; does **not** reason.
#[must_use]
pub fn clausify(internal: &InternalOntology) -> Vec<DlClause> {
    clausify_with_stats(internal).0
}

/// Clausify and also return the coverage [`ClauseStats`].
#[must_use]
pub fn clausify_with_stats(internal: &InternalOntology) -> (Vec<DlClause>, ClauseStats) {
    let mut internal = internal.clone();
    let normalized = nnf_axioms(&mut internal);
    let num_classes =
        u32::try_from(internal.vocabulary.num_classes()).expect("class count fits in u32");
    let num_individuals =
        u32::try_from(internal.vocabulary.num_individuals()).expect("individual count fits in u32");
    // Reserve `[num_classes, num_classes + num_individuals)` for the
    // nominal classes; structural names start after them.
    let nominal_base = num_classes;
    let first_fresh = num_classes
        .checked_add(num_individuals)
        .expect("class+individual count fits in u32");
    let mut c = Clausifier::new(&internal.concepts, first_fresh, nominal_base);
    for ax in &normalized {
        c.clausify_axiom(ax);
    }
    let stats = ClauseStats::of(&c.clauses, c.deferred);
    (c.clauses, stats)
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

    // ---- H3a: antecedent DNF-distribution ----

    /// `A ⊓ (B ⊔ C) ⊑ D` distributes to two Horn clauses
    /// `A⊓B → D` and `A⊓C → D` (the `VegetarianTopping` shape: a
    /// covering union nested inside an antecedent conjunction).
    #[test]
    fn antecedent_conjunction_with_or_distributes_to_horn() {
        let (clauses, stats) = clausify_ofn(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
Declaration(Class(:C))\nDeclaration(Class(:D))\n\
SubClassOf(ObjectIntersectionOf(:A ObjectUnionOf(:B :C)) :D)\n)\n"
        ));
        // No deferral — this used to bail (nested Or in And antecedent).
        assert_eq!(stats.deferred, 0, "should no longer defer; stats={stats:?}");
        // Two Horn clauses, both with head D and a 2-atom body.
        let body_classes = |c: &DlClause| -> Vec<u32> {
            let mut v: Vec<u32> = c
                .body
                .iter()
                .filter_map(|a| match a {
                    Atom::Class(id, _) => Some(id.index()),
                    _ => None,
                })
                .collect();
            v.sort_unstable();
            v
        };
        let d_clauses: Vec<&DlClause> = clauses
            .iter()
            .filter(|c| c.head.len() == 1 && c.body.len() == 2)
            .collect();
        let bodies: Vec<Vec<u32>> = d_clauses.iter().map(|c| body_classes(c)).collect();
        // A=0,B=1,C=2,D=3 in declaration order ⇒ {A,B} and {A,C}.
        assert!(
            bodies.contains(&vec![0, 1]),
            "expected A⊓B body; got {bodies:?}"
        );
        assert!(
            bodies.contains(&vec![0, 2]),
            "expected A⊓C body; got {bodies:?}"
        );
        assert!(
            clauses.iter().all(DlClause::is_horn),
            "distributed clauses must be Horn"
        );
    }

    /// The cross-product cap: an antecedent with more `Or`-branches
    /// than [`ANTECEDENT_DNF_CAP`] would expand to is deferred, not
    /// exploded. Seven binary `Or`s ⇒ 2⁷ = 128 > 64 ⇒ defer.
    #[test]
    fn antecedent_cross_product_over_cap_defers() {
        use std::fmt::Write;
        let mut decls = String::new();
        let mut ors = String::new();
        for i in 0..7 {
            let (l, r) = (format!("L{i}"), format!("R{i}"));
            let _ = write!(
                decls,
                "Declaration(Class(:{l}))\nDeclaration(Class(:{r}))\n"
            );
            let _ = write!(ors, "ObjectUnionOf(:{l} :{r}) ");
        }
        let src = format!(
            "{HEADER}Ontology(\nDeclaration(Class(:D))\n{decls}\
SubClassOf(ObjectIntersectionOf({ors}) :D)\n)\n"
        );
        let (_clauses, stats) = clausify_ofn(&src);
        assert!(
            stats.deferred >= 1,
            "over-cap antecedent must defer; stats={stats:?}"
        );
    }
}
