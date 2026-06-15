//! IR → ALCH clausal normal form + the ALCH fragment gate (Task A).
//!
//! # Normalization overview
//!
//! ALCH axioms are translated into **flat clausal form** `⊓ᵢ Aᵢ ⊑ ⊔ⱼ Lⱼ` where:
//! - premise atoms `Aᵢ` are atomic concepts (or ⊤ for the empty premise)
//! - head literals `Lⱼ` are one of: atomic `B`, `∃R.B` (B atomic), `∀R.B` (B atomic)
//!
//! Nested/compound sub-concepts are eliminated via a **structural transformation**:
//! a fresh definitional atom `X_φ` is introduced for each non-literal sub-concept
//! `φ`, and equivalence clauses `X_φ ⊑ φ` / `φ ⊑ X_φ` are emitted. The cache
//! deduplicates by `ConceptId` so each sub-concept gets at most one fresh atom.
//!
//! # Fragment gate
//!
//! Returns `Err(reason)` on the first out-of-ALCH construct encountered.
//! Out-of-ALCH: `≤n`/`≥n`, nominal, self-restriction, inverse role (in any role
//! position), datatype/DKey IRI, role chain, transitive/functional/other role
//! characteristics beyond simple hierarchy. Role hierarchy (`R ⊑ S`, both named)
//! is ALCH — it is recorded in `Normalized.role_hierarchy`, not emitted as clauses.

use crate::model::OntClause;
use hashbrown::HashMap;
use owl_dl_core::DKEY_IRI_PREFIX;
use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, Role, RoleId};
use owl_dl_core::ontology::{Axiom, InternalOntology, SubRolePath};

/// A normalized ALCH ontology: clausal axioms + the reportable atomic-class
/// vocabulary + the role hierarchy (for `∀`-propagation) + the (possibly
/// extended) concept pool.
pub struct Normalized {
    pub clauses: Vec<OntClause>,
    /// Reportable atomic classes (excludes definitional/synthetic atoms).
    pub classes: Vec<ClassId>,
    /// `R ⊑ S` edges (used by the engine's `∀`-propagation).
    pub role_hierarchy: Vec<(Role, Role)>,
    /// Owned pool; may gain definitional atoms from the structural transform.
    pub pool: ConceptPool,
}

// ── Normalization state ──────────────────────────────────────────────────────

/// Tracks fresh definitional atoms allocated during normalization.
struct DefAtoms {
    /// Maps a `ConceptId` (the sub-concept being named) → the fresh `ClassId`
    /// of its definitional atom. Populated lazily.
    by_concept: HashMap<ConceptId, ClassId>,
    /// Counter for allocating fresh `ClassId`s above the vocabulary.
    next_id: u32,
}

impl DefAtoms {
    fn new(next_id: u32) -> Self {
        Self {
            by_concept: HashMap::new(),
            next_id,
        }
    }
}

/// Central normalizer state.
struct Normalizer<'a> {
    /// Reference to the input ontology for vocabulary lookups.
    internal: &'a InternalOntology,
    /// Owned pool (cloned from internal at construction; may gain def atoms).
    pool: ConceptPool,
    /// Accumulated output clauses.
    clauses: Vec<OntClause>,
    /// Role hierarchy edges `R ⊑ S` (both named roles).
    role_hierarchy: Vec<(Role, Role)>,
    /// Definitional atom allocator.
    def: DefAtoms,
}

impl<'a> Normalizer<'a> {
    fn new(internal: &'a InternalOntology) -> Self {
        let pool = internal.concepts.clone();
        let next_id =
            u32::try_from(internal.vocabulary.num_classes()).expect("class count fits in u32");
        Self {
            internal,
            pool,
            clauses: Vec::new(),
            role_hierarchy: Vec::new(),
            def: DefAtoms::new(next_id),
        }
    }

    // ── Fragment gate ─────────────────────────────────────────────────────────

    /// Check that `role` is a named (non-inverse) role.
    fn check_role(role: Role) -> Result<RoleId, &'static str> {
        match role {
            Role::Named(id) => Ok(id),
            Role::Inverse(_) => Err("inverse role"),
        }
    }

    /// Recursively check that a `ConceptId` uses only ALCH constructs.
    fn check_concept(&self, cid: ConceptId) -> Result<(), &'static str> {
        match self.pool.get(cid) {
            ConceptExpr::Top | ConceptExpr::Bot => Ok(()),
            ConceptExpr::Atomic(cls) => {
                let iri = self.internal.vocabulary.class_iri(*cls);
                if iri.starts_with(DKEY_IRI_PREFIX) {
                    Err("datatype")
                } else {
                    Ok(())
                }
            }
            ConceptExpr::Nominal(_) => Err("nominal"),
            ConceptExpr::SelfRestriction(_) => Err("self"),
            ConceptExpr::Not(inner) => {
                let inner = *inner;
                self.check_concept(inner)
            }
            ConceptExpr::And(args) | ConceptExpr::Or(args) => {
                let args = args.to_vec();
                for a in args {
                    self.check_concept(a)?;
                }
                Ok(())
            }
            ConceptExpr::Some(role, filler) | ConceptExpr::All(role, filler) => {
                let role = *role;
                let filler = *filler;
                Self::check_role(role)?;
                self.check_concept(filler)
            }
            ConceptExpr::Min(_, _, _) | ConceptExpr::Max(_, _, _) => Err("cardinality"),
        }
    }

    // ── Structural transformation helpers ────────────────────────────────────

    /// Allocate a fresh definitional atom `X` for sub-concept `cid`, and emit
    /// equivalence clauses for `X ≡ cid` (both directions).
    ///
    /// If `cid` already has a def atom, returns it without re-emitting clauses.
    ///
    /// Both directions are emitted unconditionally for equisatisfiability:
    /// - Forward  `X ⊑ φ`  (via `process_subclassof_raw(x_cid, cid)`)
    /// - Backward `φ ⊑ X`  (via `process_subclassof_or_split(cid, x_cid)`,
    ///   which handles `Or`-on-LHS by splitting into per-disjunct subclauses)
    fn def_atom_for(&mut self, cid: ConceptId) -> ClassId {
        if let Some(&cls) = self.def.by_concept.get(&cid) {
            return cls;
        }
        let cls = ClassId::new(self.def.next_id);
        self.def.next_id += 1;
        self.def.by_concept.insert(cid, cls);
        self.pool.atomic(cls);

        let x_cid = self.pool.atomic(cls);

        // Forward: X ⊑ φ  (x_cid as premise, cid as head)
        self.process_subclassof_raw(x_cid, cid);
        // Backward: φ ⊑ X  — use the Or-splitting entry point so that
        // `Or([B, C]) ⊑ X` becomes `B ⊑ X` and `C ⊑ X`.
        self.process_subclassof_or_split(cid, x_cid);

        cls
    }

    /// `Or`-splitting entry point for `SubClassOf(sub, sup)`.
    ///
    /// If `sub` is `Or([C₁, …, Cₙ])`, emit `C₁ ⊑ sup`, …, `Cₙ ⊑ sup`
    /// (splitting a disjunction on the LHS into individual subclauses).
    /// Otherwise delegates to `process_subclassof_raw`.
    fn process_subclassof_or_split(&mut self, sub: ConceptId, sup: ConceptId) {
        match self.pool.get(sub).clone() {
            ConceptExpr::Or(disjuncts) => {
                let disjuncts = disjuncts.to_vec();
                for d in disjuncts {
                    self.process_subclassof_or_split(d, sup);
                }
            }
            _ => self.process_subclassof_raw(sub, sup),
        }
    }

    /// Core `SubClassOf(sub, sup)` processor (no `Or`-on-LHS splitting).
    fn process_subclassof_raw(&mut self, sub: ConceptId, sup: ConceptId) {
        let premise_atoms = self.flatten_sub_premise(sub);
        let mut out = Vec::new();
        self.flatten_sup(premise_atoms, sup, &mut out);
        self.clauses.extend(out);
    }

    /// Process `SubClassOf(sub, sup)` from an axiom.  Handles `Or`-on-LHS
    /// splitting before premise-flattening.
    fn process_subclassof(&mut self, sub: ConceptId, sup: ConceptId) {
        self.process_subclassof_or_split(sub, sup);
    }

    /// Flatten the LHS of a `SubClassOf(sub, sup)` into a list of premise atoms.
    ///
    /// Rules:
    /// - `⊤` → empty premise (universal clause)
    /// - Atomic `A` → `[A]`
    /// - `And([A₁, …, Aₙ])` → `[A₁, …, Aₙ]` (flatten recursively)
    /// - `Not(_)` → treat as an opaque atom (NNF guarantees this is `¬Atomic`)
    /// - Non-atomic, non-And → introduce def atom and return `[X]`
    fn flatten_sub_premise(&mut self, cid: ConceptId) -> Vec<ConceptId> {
        match self.pool.get(cid).clone() {
            ConceptExpr::Top => vec![],
            ConceptExpr::Atomic(_) | ConceptExpr::Not(_) => vec![cid],
            ConceptExpr::And(conjuncts) => {
                let conjuncts = conjuncts.to_vec();
                let mut atoms = Vec::new();
                for c in conjuncts {
                    let sub_atoms = self.flatten_sub_premise(c);
                    atoms.extend(sub_atoms);
                }
                atoms
            }
            _ => {
                // Non-atomic, non-And, non-Top: introduce a def atom.
                // Note: `Or` on premise is handled at the `process_subclassof` level
                // by `process_subclassof_or_split`, so we shouldn't reach here with
                // an `Or`.  If we do (e.g. nested in `And`), name it.
                let cls = self.def_atom_for(cid);
                vec![self.pool.atomic(cls)]
            }
        }
    }

    /// Flatten the RHS of a clause: append resulting clauses to `out`.
    ///
    /// - `⊤`: no clause (trivially satisfied)
    /// - `⊥`: empty-head clause (unsatisfiable)
    /// - `Atomic`: single-head clause `premise ⊑ {B}`
    /// - `Not(_)`: single-head literal (NNF negation of an atom)
    /// - `And([…])`: split into one clause per conjunct (RHS ⊓ splits)
    /// - `Or([…])`: single clause with multiple head literals
    /// - `Some(R, C)` / `All(R, C)`: single-head literal; flatten `C` if compound
    fn flatten_sup(&mut self, premise: Vec<ConceptId>, cid: ConceptId, out: &mut Vec<OntClause>) {
        match self.pool.get(cid).clone() {
            // ⊤ on RHS: tautologically true, no clause needed.
            // Nominal/Self/Min/Max: should be caught by the fragment gate.
            ConceptExpr::Top
            | ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_)
            | ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _) => {
                let _ = (premise, out);
            }
            ConceptExpr::Bot => {
                out.push(OntClause {
                    premise: premise.into_iter().map(|a| self.as_atom(a)).collect(),
                    head: vec![],
                });
            }
            ConceptExpr::Atomic(_) | ConceptExpr::Not(_) => {
                let head_lit = cid;
                out.push(OntClause {
                    premise: premise.into_iter().map(|a| self.as_atom(a)).collect(),
                    head: vec![head_lit],
                });
            }
            ConceptExpr::And(conjuncts) => {
                let conjuncts = conjuncts.to_vec();
                for c in conjuncts {
                    self.flatten_sup(premise.clone(), c, out);
                }
            }
            ConceptExpr::Or(disjuncts) => {
                let disjuncts = disjuncts.to_vec();
                let head_lits: Vec<ConceptId> = disjuncts
                    .into_iter()
                    .map(|d| self.concept_to_literal(d))
                    .collect();
                out.push(OntClause {
                    premise: premise.into_iter().map(|a| self.as_atom(a)).collect(),
                    head: head_lits,
                });
            }
            ConceptExpr::Some(role, filler) => {
                let flat_filler = self.flatten_filler(filler);
                let lit = self.pool.some(role, flat_filler);
                out.push(OntClause {
                    premise: premise.into_iter().map(|a| self.as_atom(a)).collect(),
                    head: vec![lit],
                });
            }
            ConceptExpr::All(role, filler) => {
                let flat_filler = self.flatten_filler(filler);
                let lit = self.pool.all(role, flat_filler);
                out.push(OntClause {
                    premise: premise.into_iter().map(|a| self.as_atom(a)).collect(),
                    head: vec![lit],
                });
            }
        }
    }

    /// Ensure the filler of `∃R.filler` / `∀R.filler` is atomic (a literal),
    /// introducing a def atom if it is compound.
    fn flatten_filler(&mut self, filler: ConceptId) -> ConceptId {
        match self.pool.get(filler).clone() {
            ConceptExpr::Atomic(_) | ConceptExpr::Not(_) | ConceptExpr::Bot | ConceptExpr::Top => {
                filler
            }
            _ => {
                let cls = self.def_atom_for(filler);
                self.pool.atomic(cls)
            }
        }
    }

    /// Convert a disjunct in `Or([…])` to a head literal.
    ///
    /// If already a literal form (atomic, `Not(atomic)`, `∃R.atomic`, `∀R.atomic`),
    /// return as-is; otherwise introduce a def atom.
    fn concept_to_literal(&mut self, cid: ConceptId) -> ConceptId {
        match self.pool.get(cid).clone() {
            ConceptExpr::Atomic(_) | ConceptExpr::Not(_) | ConceptExpr::Bot | ConceptExpr::Top => {
                cid
            }
            ConceptExpr::Some(role, filler) => {
                let flat_filler = self.flatten_filler(filler);
                self.pool.some(role, flat_filler)
            }
            ConceptExpr::All(role, filler) => {
                let flat_filler = self.flatten_filler(filler);
                self.pool.all(role, flat_filler)
            }
            ConceptExpr::Or(_) | ConceptExpr::And(_) => {
                let cls = self.def_atom_for(cid);
                self.pool.atomic(cls)
            }
            ConceptExpr::Nominal(_)
            | ConceptExpr::SelfRestriction(_)
            | ConceptExpr::Min(_, _, _)
            | ConceptExpr::Max(_, _, _) => cid,
        }
    }

    /// Convert a premise-position `ConceptId` to an `Atom` (must be atomic or ⊤).
    ///
    /// `Atomic(_)`, `Top`, and `Not(_)` (NNF negated atom) are returned as-is.
    /// Anything else: introduce a def atom.
    fn as_atom(&mut self, cid: ConceptId) -> ConceptId {
        match self.pool.get(cid).clone() {
            ConceptExpr::Atomic(_) | ConceptExpr::Top | ConceptExpr::Not(_) => cid,
            _ => {
                let cls = self.def_atom_for(cid);
                self.pool.atomic(cls)
            }
        }
    }

    // ── Axiom processing ──────────────────────────────────────────────────────

    fn process_axioms(&mut self) -> Result<(), &'static str> {
        // First pass: fragment check over all axioms.
        for axiom in &self.internal.axioms {
            match axiom {
                Axiom::SubClassOf { sub, sup } => {
                    self.check_concept(*sub)?;
                    self.check_concept(*sup)?;
                }
                Axiom::EquivalentClasses(members)
                | Axiom::DisjointClasses(members)
                | Axiom::DisjointUnion { members, .. } => {
                    for m in members {
                        self.check_concept(*m)?;
                    }
                }
                Axiom::SubObjectPropertyOf { sub, sup } => {
                    match sub {
                        SubRolePath::Role(r) => {
                            Self::check_role(*r)?;
                        }
                        SubRolePath::Chain(_) => return Err("role chain"),
                    }
                    Self::check_role(*sup)?;
                }
                Axiom::EquivalentObjectProperties(roles) => {
                    for r in roles {
                        Self::check_role(*r)?;
                    }
                }
                Axiom::InverseObjectProperties(_, _) => {
                    return Err("inverse role");
                }
                Axiom::ObjectPropertyDomain { role, domain } => {
                    Self::check_role(*role)?;
                    self.check_concept(*domain)?;
                }
                Axiom::ObjectPropertyRange { role, range } => {
                    Self::check_role(*role)?;
                    self.check_concept(*range)?;
                }
                Axiom::TransitiveRole(_)
                | Axiom::SymmetricRole(_)
                | Axiom::AsymmetricRole(_)
                | Axiom::ReflexiveRole(_)
                | Axiom::IrreflexiveRole(_)
                | Axiom::FunctionalRole(_)
                | Axiom::InverseFunctionalRole(_)
                | Axiom::DisjointObjectProperties(_) => {
                    return Err("role characteristic");
                }
                Axiom::ClassAssertion { .. }
                | Axiom::ObjectPropertyAssertion { .. }
                | Axiom::NegativeObjectPropertyAssertion { .. }
                | Axiom::SameIndividual(_)
                | Axiom::DifferentIndividuals(_) => {
                    return Err("abox");
                }
                Axiom::DeclareClass(_)
                | Axiom::DeclareObjectProperty(_)
                | Axiom::DeclareNamedIndividual(_) => {}
            }
        }

        // Second pass: emit clauses.
        let axioms: Vec<Axiom> = self.internal.axioms.clone();
        for axiom in axioms {
            match axiom {
                Axiom::SubClassOf { sub, sup } => {
                    self.process_subclassof(sub, sup);
                }
                Axiom::EquivalentClasses(members) => {
                    for i in 0..members.len() {
                        for j in 0..members.len() {
                            if i != j {
                                self.process_subclassof(members[i], members[j]);
                            }
                        }
                    }
                }
                Axiom::DisjointClasses(members) => {
                    let bot_id = self.pool.bot();
                    for i in 0..members.len() {
                        for j in (i + 1)..members.len() {
                            let conj = self.pool.and([members[i], members[j]]);
                            self.process_subclassof(conj, bot_id);
                        }
                    }
                }
                Axiom::DisjointUnion { class, members } => {
                    let class_id = self.pool.atomic(class);
                    let union_id = self.pool.or(members.iter().copied());
                    self.process_subclassof(class_id, union_id);
                    self.process_subclassof(union_id, class_id);
                    let bot_id = self.pool.bot();
                    for i in 0..members.len() {
                        for j in (i + 1)..members.len() {
                            let conj = self.pool.and([members[i], members[j]]);
                            self.process_subclassof(conj, bot_id);
                        }
                    }
                }
                Axiom::SubObjectPropertyOf {
                    sub: SubRolePath::Role(r),
                    sup,
                } => {
                    self.role_hierarchy.push((r, sup));
                }
                Axiom::EquivalentObjectProperties(roles) => {
                    for i in 0..roles.len() {
                        for j in 0..roles.len() {
                            if i != j {
                                self.role_hierarchy.push((roles[i], roles[j]));
                            }
                        }
                    }
                }
                Axiom::ObjectPropertyDomain { role, domain } => {
                    // ∃R.⊤ ⊑ domain
                    let top_id = self.pool.top();
                    let some_r_top = self.pool.some(role, top_id);
                    self.process_subclassof(some_r_top, domain);
                }
                Axiom::ObjectPropertyRange { role, range } => {
                    // ⊤ ⊑ ∀R.range
                    let top_id = self.pool.top();
                    let all_r_d = self.pool.all(role, range);
                    self.process_subclassof(top_id, all_r_d);
                }
                // Already handled / guarded in first pass.
                _ => {}
            }
        }
        Ok(())
    }

    fn collect_reportable_classes(&self) -> Vec<ClassId> {
        (0..self.internal.vocabulary.num_classes())
            .filter_map(|i| {
                let cls = ClassId::new(u32::try_from(i).expect("fits in u32"));
                let iri = self.internal.vocabulary.class_iri(cls);
                if iri.starts_with(DKEY_IRI_PREFIX) {
                    None
                } else {
                    Some(cls)
                }
            })
            .collect()
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Normalize `internal` to ALCH clausal form, or `Err(reason)` naming the first
/// out-of-ALCH construct encountered.
pub fn normalize(internal: &InternalOntology) -> Result<Normalized, &'static str> {
    let mut n = Normalizer::new(internal);
    n.process_axioms()?;
    let classes = n.collect_reportable_classes();
    Ok(Normalized {
        clauses: n.clauses,
        classes,
        role_hierarchy: n.role_hierarchy,
        pool: n.pool,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    fn parse(src: &str) -> InternalOntology {
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("OFN parses");
        convert_ontology(&onto).expect("conversion")
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn onto(body: &str) -> String {
        format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
{body}\
)\n"
        )
    }

    fn class_id(internal: &InternalOntology, local: &str) -> ClassId {
        internal
            .vocabulary
            .class_id(&format!("http://rustdl.test/{local}"))
            .unwrap_or_else(|| panic!("class {local} not found"))
    }

    fn role_id_for(internal: &InternalOntology, local: &str) -> RoleId {
        internal
            .vocabulary
            .role_id(&format!("http://rustdl.test/{local}"))
            .unwrap_or_else(|| panic!("role {local} not found"))
    }

    /// Check that `clauses` contains a clause with exactly the given premise
    /// atoms (by [`ClassId`]) and head atoms (by [`ClassId`]).
    fn has_atomic_clause(
        clauses: &[OntClause],
        pool: &ConceptPool,
        premise_classes: &[ClassId],
        head_classes: &[ClassId],
    ) -> bool {
        clauses.iter().any(|c| {
            let prem_match = c.premise.len() == premise_classes.len()
                && c.premise.iter().zip(premise_classes).all(
                    |(&cid, &cls)| matches!(pool.get(cid), ConceptExpr::Atomic(c) if *c == cls),
                );
            let head_match = c.head.len() == head_classes.len()
                && c.head.iter().zip(head_classes).all(
                    |(&cid, &cls)| matches!(pool.get(cid), ConceptExpr::Atomic(c) if *c == cls),
                );
            prem_match && head_match
        })
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Basic axiom normalization
    // ─────────────────────────────────────────────────────────────────────────

    /// `A ⊑ B ⊓ C` → two clauses `{A} ⊑ {B}` and `{A} ⊑ {C}`.
    #[test]
    fn sub_and_right_splits_into_two_clauses() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             SubClassOf(:A ObjectIntersectionOf(:B :C))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let c = class_id(&internal, "C");
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[a], &[b]),
            "expected {{A}} ⊑ {{B}} clause"
        );
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[a], &[c]),
            "expected {{A}} ⊑ {{C}} clause"
        );
    }

    /// `A ⊑ B ⊔ C` → one clause `{A} ⊑ {B, C}`.
    #[test]
    fn sub_or_right_single_clause_with_two_head_literals() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             SubClassOf(:A ObjectUnionOf(:B :C))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let c = class_id(&internal, "C");
        let found = norm.clauses.iter().any(|cl| {
            let prem_ok = cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a);
            let head_classes: Vec<ClassId> = cl
                .head
                .iter()
                .filter_map(|&cid| match norm.pool.get(cid) {
                    ConceptExpr::Atomic(id) => Some(*id),
                    _ => None,
                })
                .collect();
            prem_ok
                && head_classes.len() == 2
                && head_classes.contains(&b)
                && head_classes.contains(&c)
        });
        assert!(
            found,
            "expected {{A}} ⊑ {{B, C}} clause; clauses: {:?}",
            norm.clauses
        );
    }

    /// `A ⊓ B ⊑ C` → one clause with premise `{A, B}` and head `{C}`.
    #[test]
    fn and_left_premise_clause() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             SubClassOf(ObjectIntersectionOf(:A :B) :C)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let c = class_id(&internal, "C");
        let found = norm.clauses.iter().any(|cl| {
            let prem_classes: Vec<ClassId> = cl
                .premise
                .iter()
                .filter_map(|&cid| match norm.pool.get(cid) {
                    ConceptExpr::Atomic(id) => Some(*id),
                    _ => None,
                })
                .collect();
            let head_ok = cl.head.len() == 1
                && matches!(norm.pool.get(cl.head[0]), ConceptExpr::Atomic(id) if *id == c);
            prem_classes.len() == 2
                && prem_classes.contains(&a)
                && prem_classes.contains(&b)
                && head_ok
        });
        assert!(found, "expected {{A,B}} ⊑ {{C}}");
    }

    /// `A ⊑ ∃R.B` → clause `{A} ⊑ {∃R.B}`.
    #[test]
    fn sub_existential_right_literal() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:R :B))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let r_id = role_id_for(&internal, "R");
        let role = Role::named(r_id);
        let found = norm.clauses.iter().any(|cl| {
            let prem_ok = cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a);
            let head_ok = cl.head.len() == 1
                && matches!(norm.pool.get(cl.head[0]),
                    ConceptExpr::Some(r, filler)
                    if *r == role
                        && matches!(norm.pool.get(*filler), ConceptExpr::Atomic(id) if *id == b));
            prem_ok && head_ok
        });
        assert!(found, "expected {{A}} ⊑ {{∃R.B}}");
    }

    /// `A ⊑ ∀R.B` → clause `{A} ⊑ {∀R.B}`.
    #[test]
    fn sub_universal_right_literal() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectAllValuesFrom(:R :B))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let r_id = role_id_for(&internal, "R");
        let role = Role::named(r_id);
        let found = norm.clauses.iter().any(|cl| {
            let prem_ok = cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a);
            let head_ok = cl.head.len() == 1
                && matches!(norm.pool.get(cl.head[0]),
                    ConceptExpr::All(r, filler)
                    if *r == role
                        && matches!(norm.pool.get(*filler), ConceptExpr::Atomic(id) if *id == b));
            prem_ok && head_ok
        });
        assert!(found, "expected {{A}} ⊑ {{∀R.B}}");
    }

    /// Nested `A ⊑ ∃R.(B ⊔ C)` → definitional atom X for `B ⊔ C`, plus:
    /// - `{A} ⊑ {∃R.X}` (the original axiom flattened)
    /// - `{X} ⊑ {B, C}` (forward: X ⊑ B⊔C)
    #[test]
    fn nested_existential_introduces_def_atom() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:R ObjectUnionOf(:B :C)))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let c = class_id(&internal, "C");
        let r_id = role_id_for(&internal, "R");
        let role = Role::named(r_id);

        // Find the clause {A} ⊑ {∃R.X}.
        let some_clause = norm.clauses.iter().find(|cl| {
            cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a)
                && cl.head.len() == 1
                && matches!(norm.pool.get(cl.head[0]), ConceptExpr::Some(r, _) if *r == role)
        });
        assert!(some_clause.is_some(), "expected {{A}} ⊑ {{∃R.X}} clause");

        // Extract the def atom X.
        let head_cid = some_clause.expect("clause present").head[0];
        let def_atom_id: ClassId = match norm.pool.get(head_cid) {
            ConceptExpr::Some(_, filler) => match norm.pool.get(*filler) {
                ConceptExpr::Atomic(id) => *id,
                other => panic!("filler should be atomic, got {other:?}"),
            },
            other => panic!("head should be ∃R.X, got {other:?}"),
        };

        // Forward: {X} ⊑ {B, C} (from X ⊑ B⊔C).
        let or_clause = norm.clauses.iter().any(|cl| {
            let prem_ok = cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == def_atom_id);
            let head_classes: Vec<ClassId> = cl
                .head
                .iter()
                .filter_map(|&cid| match norm.pool.get(cid) {
                    ConceptExpr::Atomic(id) => Some(*id),
                    _ => None,
                })
                .collect();
            prem_ok && head_classes.len() == 2
                && head_classes.contains(&b)
                && head_classes.contains(&c)
        });
        assert!(or_clause, "expected {{X}} ⊑ {{B,C}} clause for def atom X");
    }

    /// `EquivalentClasses(A, B)` → both `{A} ⊑ {B}` and `{B} ⊑ {A}`.
    #[test]
    fn equivalent_classes_both_directions() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             EquivalentClasses(:A :B)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[a], &[b]),
            "expected A ⊑ B"
        );
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[b], &[a]),
            "expected B ⊑ A"
        );
    }

    /// `A ⊑ ⊥` → clause `{A} ⊑ {}` (empty head).
    #[test]
    fn sub_bot_gives_empty_head_clause() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             SubClassOf(:A owl:Nothing)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let found = norm.clauses.iter().any(|cl| {
            cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a)
                && cl.head.is_empty()
        });
        assert!(found, "expected {{A}} ⊑ {{}} (empty head)");
    }

    /// Role hierarchy `R ⊑ S` is recorded in `role_hierarchy`, not emitted as clauses.
    #[test]
    fn role_hierarchy_recorded() {
        let internal = parse(&onto(
            "Declaration(ObjectProperty(:R))\n\
             Declaration(ObjectProperty(:S))\n\
             SubObjectPropertyOf(:R :S)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let r_id = role_id_for(&internal, "R");
        let s_id = role_id_for(&internal, "S");
        assert!(
            norm.role_hierarchy
                .contains(&(Role::named(r_id), Role::named(s_id))),
            "expected R ⊑ S in role_hierarchy"
        );
        assert!(
            norm.clauses.is_empty(),
            "role hierarchy should not produce clauses; got: {:?}",
            norm.clauses
        );
    }

    /// Classes list includes all declared classes.
    #[test]
    fn classes_list_populated() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        assert!(norm.classes.contains(&a), "A should be in classes");
        assert!(norm.classes.contains(&b), "B should be in classes");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Fragment gate tests — each must return Err
    // ─────────────────────────────────────────────────────────────────────────

    /// `Max` (`≤n`) → `Err("cardinality")`
    #[test]
    fn fragment_gate_max_cardinality() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectMaxCardinality(1 :R :B))\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("cardinality")),
            "expected Err(\"cardinality\")"
        );
    }

    /// `Min` (`≥n`, n≥1) → `Err("cardinality")`
    #[test]
    fn fragment_gate_min_cardinality() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectMinCardinality(2 :R :B))\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("cardinality")),
            "expected Err(\"cardinality\")"
        );
    }

    /// `ObjectHasSelf` (`∃R.Self`) → `Err("self")`
    #[test]
    fn fragment_gate_self_restriction() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectHasSelf(:R))\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("self")),
            "expected Err(\"self\")"
        );
    }

    /// Inverse role in concept (`∃R⁻.B`) → `Err("inverse role")`
    #[test]
    fn fragment_gate_inverse_role_in_concept() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:R) :B))\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("inverse role")),
            "expected Err(\"inverse role\")"
        );
    }

    /// `ObjectHasValue` (nominal filler) → `Err` (datatype or nominal — either
    /// is correct depending on how the converter lowers it).
    #[test]
    fn fragment_gate_nominal_via_has_value() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(ObjectProperty(:R))\n\
             Declaration(NamedIndividual(:a))\n\
             SubClassOf(:A ObjectHasValue(:R :a))\n",
        ));
        assert!(
            normalize(&internal).is_err(),
            "ObjectHasValue should be out of fragment"
        );
    }

    /// Role chain (`R ∘ S ⊑ T`) → `Err("role chain")`
    #[test]
    fn fragment_gate_role_chain() {
        let internal = parse(&onto(
            "Declaration(ObjectProperty(:R))\n\
             Declaration(ObjectProperty(:S))\n\
             Declaration(ObjectProperty(:T))\n\
             SubObjectPropertyOf(ObjectPropertyChain(:R :S) :T)\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("role chain")),
            "expected Err(\"role chain\")"
        );
    }

    /// Transitive role → `Err("role characteristic")`
    #[test]
    fn fragment_gate_transitive_role() {
        let internal = parse(&onto(
            "Declaration(ObjectProperty(:R))\n\
             TransitiveObjectProperty(:R)\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("role characteristic")),
            "expected Err(\"role characteristic\")"
        );
    }

    /// Functional role → out of fragment (either "role characteristic" from
    /// `FunctionalRole` or "cardinality" from the derived `∃R.⊤ ⊑ ≤1 R.⊤`
    /// axiom emitted by `convert_ontology`; both are correct).
    #[test]
    fn fragment_gate_functional_role() {
        let internal = parse(&onto(
            "Declaration(ObjectProperty(:R))\n\
             FunctionalObjectProperty(:R)\n",
        ));
        assert!(
            normalize(&internal).is_err(),
            "FunctionalObjectProperty should be out of fragment"
        );
    }

    /// `InverseObjectProperties` → `Err("inverse role")`
    #[test]
    fn fragment_gate_inverse_object_properties() {
        let internal = parse(&onto(
            "Declaration(ObjectProperty(:R))\n\
             Declaration(ObjectProperty(:S))\n\
             InverseObjectProperties(:R :S)\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("inverse role")),
            "expected Err(\"inverse role\")"
        );
    }

    /// `ABox` (`ClassAssertion`) → `Err("abox")`
    #[test]
    fn fragment_gate_abox() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(NamedIndividual(:a))\n\
             ClassAssertion(:A :a)\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("abox")),
            "expected Err(\"abox\")"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Equisatisfiability / structural transformation correctness
    // ─────────────────────────────────────────────────────────────────────────

    /// `A ⊑ ∃R.(B ⊔ C)` → def atom X, backward clauses `B ⊑ X` and `C ⊑ X`.
    ///
    /// The backward direction (`φ ⊑ X`, expanded as `B ⊑ X`, `C ⊑ X`) ensures
    /// equisatisfiability: if something is B (or C), it is X (the def atom for
    /// B⊔C). Together with the forward `X ⊑ B⊔C` direction, X ≡ B⊔C.
    #[test]
    fn def_atom_backward_clauses_emitted() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:R ObjectUnionOf(:B :C)))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let b = class_id(&internal, "B");
        let c = class_id(&internal, "C");

        // Find the def atom X (filler of ∃R.X).
        let def_atom_id: Option<ClassId> = norm.clauses.iter().find_map(|cl| {
            if cl.head.len() == 1
                && let ConceptExpr::Some(_, filler) = norm.pool.get(cl.head[0])
                && let ConceptExpr::Atomic(id) = norm.pool.get(*filler)
            {
                return Some(*id);
            }
            None
        });
        let x = def_atom_id.expect("def atom X should exist");

        // Backward clauses: `B ⊑ X` and `C ⊑ X`.
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[b], &[x]),
            "expected B ⊑ X backward clause"
        );
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[c], &[x]),
            "expected C ⊑ X backward clause"
        );
    }

    /// Dedup: the same sub-concept `B ⊔ C` used in two axioms yields the same
    /// definitional atom `X`.
    #[test]
    fn def_atom_dedup() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             Declaration(Class(:D))\n\
             Declaration(ObjectProperty(:R))\n\
             Declaration(ObjectProperty(:S))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:R ObjectUnionOf(:B :C)))\n\
             SubClassOf(:D ObjectSomeValuesFrom(:S ObjectUnionOf(:B :C)))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        // Both existentials should have the same def-atom filler.
        let fillers: Vec<ClassId> = norm
            .clauses
            .iter()
            .filter_map(|cl| {
                if cl.head.len() == 1
                    && let ConceptExpr::Some(_, filler) = norm.pool.get(cl.head[0])
                    && let ConceptExpr::Atomic(id) = norm.pool.get(*filler)
                {
                    return Some(*id);
                }
                None
            })
            .collect();
        assert!(
            fillers.len() >= 2,
            "expected at least two existential head clauses"
        );
        assert!(
            fillers.iter().all(|&f| f == fillers[0]),
            "expected the same def atom X for B⊔C used twice; got: {fillers:?}"
        );
    }
}
