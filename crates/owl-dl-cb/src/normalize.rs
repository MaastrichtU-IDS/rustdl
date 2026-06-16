//! IR → ALCH clausal normal form + the ALCH fragment gate (Task A).
//!
//! # Normalization overview
//!
//! ALCH axioms are translated into **flat clausal form** `⊓ᵢ Aᵢ ⊑ ⊔ⱼ Lⱼ` where:
//! - Premise atoms `Aᵢ` are **positive** atomic concepts (`Atomic(ClassId)` or `⊤`)
//! - Head literals `Lⱼ` are: atomic `B`, `∃R.B`, or `∀R.B` where `B` is a
//!   **positive** atom (`Atomic`/`⊤`/`⊥`). **No raw `Not` literal survives** —
//!   every `¬C` is replaced by a positive **complement atom** `X` (`X ⊑ ¬C`)
//!   defined by the disjointness clause `{C, X} ⊑ ⊥`.
//!
//! Nested/compound sub-concepts are eliminated via a **structural transformation**:
//! a fresh definitional atom `X_φ` is introduced for each non-literal sub-concept
//! `φ`, and equivalence clauses `X_φ ≡ φ` are emitted (both directions). The cache
//! deduplicates by `ConceptId` so each sub-concept gets at most one fresh atom.
//!
//! # Polarity handling (the A↔B convention — option (b))
//!
//! Because both premise atoms AND head literals must be **positive** (the
//! standard SKH clausal form the engine consumes), negation is eliminated at
//! normalization time via complement atoms (see [`Normalizer::complement_atom`]):
//!
//! - `¬A ⊑ D`          → `⊤ ⊑ A ⊔ D`  (A moves to the head as a positive literal)
//! - `∃R.C ⊑ D`        → `⊤ ⊑ ∀R.X ⊔ D`  with `X ⊑ ¬C`, `{C, X} ⊑ ⊥`
//! - `∀R.C ⊑ D`        → `⊤ ⊑ ∃R.X ⊔ D`  with `X ⊑ ¬C`, `{C, X} ⊑ ⊥`
//! - `A ⊑ ¬C`          → `A ⊑ X`  with `X ⊑ ¬C`, `{C, X} ⊑ ⊥`
//! - `(A ⊓ ¬B ⊓ ...) ⊑ D` → premise `[A]`, head `[B, D_literals...]`
//!
//! The input is put through `nnf_axioms` before processing so that `Not` only
//! appears directly on atomic concepts; the complement-atom reduction then
//! removes those `Not(Atomic)` literals entirely.
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
use owl_dl_core::normalize::nnf_axioms;
use owl_dl_core::ontology::{Axiom, InternalOntology, SubRolePath};

/// A normalized ALCH ontology: clausal axioms + the reportable atomic-class
/// vocabulary + the role hierarchy (for `∀`-propagation) + the (possibly
/// extended) concept pool.
pub(crate) struct Normalized {
    pub(crate) clauses: Vec<OntClause>,
    /// Reportable atomic classes (excludes definitional/synthetic atoms).
    pub(crate) classes: Vec<ClassId>,
    /// `R ⊑ S` edges (used by the engine's `∀`-propagation).
    pub(crate) role_hierarchy: Vec<(Role, Role)>,
    /// Owned pool; may gain definitional atoms from the structural transform.
    pub(crate) pool: ConceptPool,
}

// ── Normalization state ──────────────────────────────────────────────────────

/// Tracks fresh definitional atoms allocated during normalization.
struct DefAtoms {
    by_concept: HashMap<ConceptId, ClassId>,
    /// Complement atoms: `comp[c] = X` with `X ≡ ¬c` (`c` a positive atom).
    comp: HashMap<ConceptId, ClassId>,
    next_id: u32,
}

impl DefAtoms {
    fn new(next_id: u32) -> Self {
        Self {
            by_concept: HashMap::new(),
            comp: HashMap::new(),
            next_id,
        }
    }
}

/// Central normalizer state.
struct Normalizer {
    pool: ConceptPool,
    clauses: Vec<OntClause>,
    role_hierarchy: Vec<(Role, Role)>,
    def: DefAtoms,
    /// IRIs for DKey-prefix check (indexed by [`ClassId`]).
    class_iris: Vec<String>,
}

impl Normalizer {
    fn new(pool: ConceptPool, num_classes: usize, class_iris: Vec<String>) -> Self {
        let next_id = u32::try_from(num_classes).expect("class count fits in u32");
        Self {
            pool,
            clauses: Vec::new(),
            role_hierarchy: Vec::new(),
            def: DefAtoms::new(next_id),
            class_iris,
        }
    }

    // ── Fragment gate ─────────────────────────────────────────────────────────

    fn check_role(role: Role) -> Result<RoleId, &'static str> {
        match role {
            Role::Named(id) => Ok(id),
            Role::Inverse(_) => Err("inverse role"),
        }
    }

    fn check_concept(&self, cid: ConceptId) -> Result<(), &'static str> {
        match self.pool.get(cid) {
            ConceptExpr::Top | ConceptExpr::Bot => Ok(()),
            ConceptExpr::Atomic(cls) => {
                let idx = cls.index() as usize;
                if idx < self.class_iris.len() && self.class_iris[idx].starts_with(DKEY_IRI_PREFIX)
                {
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
            // `∃R`/`∀R` and (B2 ALCHQ) `≥n R`/`≤n R` all admit a NAMED role with
            // an in-fragment filler. Inverse roles ⇒ B3 (rejected by
            // `check_role`); a DKey filler ⇒ datatype (rejected by the filler's
            // `check_concept`).
            ConceptExpr::Some(role, filler)
            | ConceptExpr::All(role, filler)
            | ConceptExpr::Min(_, role, filler)
            | ConceptExpr::Max(_, role, filler) => {
                let role = *role;
                let filler = *filler;
                Self::check_role(role)?;
                self.check_concept(filler)
            }
        }
    }

    // ── Structural transformation helpers ────────────────────────────────────

    /// Allocate (or reuse) a fresh definitional atom `X` for sub-concept `cid`,
    /// and emit equivalence clauses `X ≡ cid` in both directions.
    ///
    /// **Forward** `X ⊑ cid`: processed via `normalize_subclassof_raw(X_cid, cid)`.
    /// **Backward** `cid ⊑ X`: processed via `normalize_subclassof(cid, X_cid)`,
    ///   which correctly splits Or-on-LHS into per-disjunct subclauses.
    fn def_atom_for(&mut self, cid: ConceptId) -> ClassId {
        if let Some(&cls) = self.def.by_concept.get(&cid) {
            return cls;
        }
        let cls = ClassId::new(self.def.next_id);
        self.def.next_id += 1;
        self.def.by_concept.insert(cid, cls);
        self.pool.atomic(cls);
        let x_cid = self.pool.atomic(cls);
        // Forward: X ⊑ cid
        self.normalize_subclassof_raw(x_cid, cid);
        // Backward: cid ⊑ X (handles Or on LHS correctly)
        self.normalize_subclassof(cid, x_cid);
        cls
    }

    /// Compute the NNF complement of `cid` (`¬cid` in NNF) using the pool.
    fn neg_nnf(&mut self, cid: ConceptId) -> ConceptId {
        owl_dl_core::normalize::nnf_complement(cid, &mut self.pool)
    }

    /// Allocate (or reuse) a positive **complement atom** `X` for the positive
    /// literal `c_lit` (`X ⊑ ¬c_lit`), and emit the disjointness clause that
    /// defines it:
    ///   - `{c_lit, X} ⊑ ⊥`   (they cannot co-occur — `X` excludes `c_lit`)
    ///
    /// Returns the `Atomic(X)` concept id (always a **positive** literal — no
    /// raw `Not` ever survives into a clause). `⊤`/`⊥` are handled directly
    /// (`¬⊤ = ⊥`, `¬⊥ = ⊤`). A compound `c_lit` is first reduced to a def atom.
    ///
    /// This is the A↔B convention reconciliation (Task E, option (b)): the
    /// engine consumes only positive-atom literals + disjointness clauses (the
    /// standard SKH clausal form), so `normalize` eliminates every `Not(Atomic)`
    /// filler/head literal in favour of such a complement atom.
    ///
    /// **Why only the disjointness direction (no totality `⊤ ⊑ c_lit ⊔ X`).**
    /// The CB read-off reports an *atomic subsumption* `H ⊑ A` only for genuine
    /// classes `A`; the synthetic `X` is never read off, so the excluded-middle
    /// `c_lit ⊔ X` is never needed to witness a real subsumption — every
    /// positive consequence of a left-`∃`/`∀` flows through disjointness +
    /// `∀`-propagation + `⊥`-back-prop (cf. the engine's `pure_el_left_existential`
    /// / `disjunctive_back_propagation` unit tests, which also omit totality and
    /// pass). The empty-premise totality clause would fire in *every* context,
    /// injecting a 2-literal disjunction per complement atom — a blow-up risk on
    /// `¬`-heavy ALC inputs — for zero completeness gain. Dropping it biases any
    /// residual uncertainty toward a MISS (`only_in_current`), never an FP
    /// (`only_in_cb`), per the soundness discipline. The differential gate on
    /// alehif (`identical: true`) is the empirical confirmation.
    fn complement_atom(&mut self, c_lit: ConceptId) -> ConceptId {
        match self.pool.get(c_lit) {
            ConceptExpr::Top => return self.pool.bot(),
            ConceptExpr::Bot => return self.pool.top(),
            _ => {}
        }
        // Ensure we complement a genuine atom (def-atom any compound first).
        let atom = if matches!(self.pool.get(c_lit), ConceptExpr::Atomic(_)) {
            c_lit
        } else {
            let cls = self.def_atom_for(c_lit);
            self.pool.atomic(cls)
        };
        if let Some(&cls) = self.def.comp.get(&atom) {
            return self.pool.atomic(cls);
        }
        let cls = ClassId::new(self.def.next_id);
        self.def.next_id += 1;
        self.def.comp.insert(atom, cls);
        let x_cid = self.pool.atomic(cls);
        // {atom, X} ⊑ ⊥
        self.clauses.push(OntClause {
            premise: vec![atom, x_cid],
            head: vec![],
        });
        x_cid
    }

    /// Reduce a literal to an equivalent **positive** literal (`Atomic`/`⊤`/`⊥`):
    /// a `Not(Atomic)` becomes its complement atom; a compound becomes a def
    /// atom; positive literals pass through. Used wherever a literal would
    /// otherwise be emitted as a raw `Not` (head literals, `∃`/`∀` fillers).
    fn positive_literal(&mut self, lit: ConceptId) -> ConceptId {
        match self.pool.get(lit).clone() {
            ConceptExpr::Atomic(_) | ConceptExpr::Top | ConceptExpr::Bot => lit,
            ConceptExpr::Not(inner) => {
                // NNF guarantees `inner` is atomic.
                self.complement_atom(inner)
            }
            _ => {
                let cls = self.def_atom_for(lit);
                self.pool.atomic(cls)
            }
        }
    }

    /// Flatten `filler` to a **positive** literal for use inside `∃R.` / `∀R.`:
    /// atomic/`⊤`/`⊥` pass through; `Not(Atomic)` becomes its complement atom;
    /// a compound is named via a def atom. No raw `Not` survives (option (b)).
    fn flatten_to_literal(&mut self, filler: ConceptId) -> ConceptId {
        self.positive_literal(filler)
    }

    // ── Core normalization: SubClassOf(sub, sup) ──────────────────────────────

    /// Normalize `SubClassOf(sub, sup)` — the main entry point for all GCIs.
    ///
    /// Handles structural splits before delegating to `normalize_subclassof_raw`:
    /// - `Bot ⊑ D`: trivially true, skip
    /// - `C₁ ⊔ ... ⊔ Cₙ ⊑ D`: split into per-disjunct subclauses
    /// - `C ⊑ ⊤`: tautological, skip
    /// - `C ⊑ D₁ ⊓ ... ⊓ Dₙ`: split into separate `C ⊑ Dᵢ` subclauses
    fn normalize_subclassof(&mut self, sub: ConceptId, sup: ConceptId) {
        // RHS splitting: C ⊑ A ⊓ B → C ⊑ A  AND  C ⊑ B
        match self.pool.get(sup).clone() {
            ConceptExpr::Top => return, // tautological
            ConceptExpr::And(conjuncts) => {
                let conjuncts = conjuncts.to_vec();
                for c in conjuncts {
                    self.normalize_subclassof(sub, c);
                }
                return;
            }
            _ => {}
        }
        // LHS splitting: (C₁ ⊔ C₂) ⊑ D → C₁ ⊑ D  AND  C₂ ⊑ D
        match self.pool.get(sub).clone() {
            ConceptExpr::Bot => {} // trivially true
            ConceptExpr::Or(disjuncts) => {
                let disjuncts = disjuncts.to_vec();
                for d in disjuncts {
                    self.normalize_subclassof(d, sup);
                }
            }
            _ => self.normalize_subclassof_raw(sub, sup),
        }
    }

    /// Core `SubClassOf(sub, sup)` normalizer (after Or-splitting and Bot-skip).
    ///
    /// Extracts premise atoms and "extra head literals" from the LHS using
    /// polarity-aware decomposition, then flattens the RHS into head literals.
    fn normalize_subclassof_raw(&mut self, sub: ConceptId, sup: ConceptId) {
        // Decompose the LHS into premise atoms + extra head literals.
        let (premise_atoms, extra_head) = self.extract_premise(sub);
        // Flatten the RHS into head literals.
        let mut rhs_head = Vec::new();
        self.flatten_head(sup, &mut rhs_head);
        // Combine extra head literals with the RHS head.
        let head: Vec<ConceptId> = extra_head.into_iter().chain(rhs_head).collect();
        if head.is_empty() && premise_atoms.is_empty() {
            // ⊤ ⊑ ⊥ — an unsatisfiable ontology; emit an empty-head universal clause.
            self.clauses.push(OntClause {
                premise: vec![],
                head: vec![],
            });
        } else {
            self.clauses.push(OntClause {
                premise: premise_atoms,
                head,
            });
        }
    }

    /// Extract premise atoms from the LHS of a GCI, with polarity awareness.
    ///
    /// Returns `(premise_atoms, extra_head_literals)`:
    /// - `premise_atoms`: positive `Atom` ids (conceptids of `Atomic(_)` or `Top`)
    /// - `extra_head_literals`: literals that come from moving negative/complex LHS
    ///   sub-expressions to the head side (keeping the clause flat and positive-premise)
    ///
    /// Rules:
    /// - `⊤` → premise empty (universal clause)
    /// - `Atomic(a)` → premise atom `a`
    /// - `¬Atomic(a)` → extra head literal `Atomic(a)` (flip: ¬A on LHS → A on head)
    /// - `And(cs)` → recurse on each conjunct, collect all atoms and extras
    /// - `∃R.C` → extra head `∀R.X` with `X ⊑ ¬C` (positive complement atom — the
    ///   NNF complement `¬C` reduced to a positive literal, option (b))
    /// - `∀R.C` → extra head `∃R.X` with `X ⊑ ¬C`
    /// - Other compound (def-atom introduction): treat as new atomic premise
    ///
    /// Note: `Or` and `Bot` are handled at the `normalize_subclassof` level.
    fn extract_premise(&mut self, cid: ConceptId) -> (Vec<ConceptId>, Vec<ConceptId>) {
        match self.pool.get(cid).clone() {
            // Top: universal premise (empty). Bot: handled at normalize_subclassof level.
            ConceptExpr::Top | ConceptExpr::Bot => (vec![], vec![]),
            ConceptExpr::Atomic(_) => (vec![cid], vec![]),
            ConceptExpr::Not(inner) => {
                // NNF guarantees `inner` is Atomic.
                let inner_cid = inner;
                // ¬A on LHS → A on head (flip the literal)
                (vec![], vec![inner_cid])
            }
            ConceptExpr::And(conjuncts) => {
                let conjuncts = conjuncts.to_vec();
                let mut atoms = Vec::new();
                let mut extras = Vec::new();
                for c in conjuncts {
                    let (sub_atoms, sub_extras) = self.extract_premise(c);
                    atoms.extend(sub_atoms);
                    extras.extend(sub_extras);
                }
                (atoms, extras)
            }
            ConceptExpr::Some(role, filler) => {
                // ∃R.C ⊑ D  →  ⊤ ⊑ ∀R.¬C ⊔ D
                let neg_filler = self.neg_nnf(filler);
                let flat_neg = self.flatten_to_literal(neg_filler);
                let forall_neg = self.pool.all(role, flat_neg);
                (vec![], vec![forall_neg])
            }
            ConceptExpr::All(role, filler) => {
                // ∀R.C ⊑ D  →  ⊤ ⊑ ∃R.¬C ⊔ D
                let neg_filler = self.neg_nnf(filler);
                let flat_neg = self.flatten_to_literal(neg_filler);
                let some_neg = self.pool.some(role, flat_neg);
                (vec![], vec![some_neg])
            }
            // Bot is handled at normalize_subclassof level; shouldn't reach here.
            // Or is split at normalize_subclassof level; if nested inside And, name it.
            // Nominal/Self/Min/Max should be gate-rejected; fall back to def atom.
            _ => {
                let cls = self.def_atom_for(cid);
                let atom = self.pool.atomic(cls);
                (vec![atom], vec![])
            }
        }
    }

    /// Flatten the RHS of a clause into head literals, appending to `out`.
    ///
    /// Rules:
    /// - `⊤` → nothing (trivially satisfied; remove the clause, but we don't
    ///   return that signal here — callers that need to can check)
    /// - `⊥` → empty disjunction (if also no premise ← handled in caller)
    /// - `Atomic(b)` → head literal `b`
    /// - `¬Atomic(b)` → head literal `¬b` (Not(Atomic(b)))
    /// - `And(cs)` → split into separate clauses (handled by splitting in caller)
    ///   — but within `flatten_head` we can't split; introduce def atom instead.
    ///   *(Note: And-on-RHS properly requires splitting into multiple clauses;
    ///   this is done at the `normalize_subclassof` level for explicit `And` tops.)*
    /// - `Or(cs)` → multiple head literals (each recursed)
    /// - `∃R.C` → head literal `∃R.flatten_to_literal(C)`
    /// - `∀R.C` → head literal `∀R.flatten_to_literal(C)`
    fn flatten_head(&mut self, cid: ConceptId, out: &mut Vec<ConceptId>) {
        match self.pool.get(cid).clone() {
            ConceptExpr::Top => {
                // ⊤ on RHS: tautology — signal by returning a special marker.
                // We mark this by pushing a special `top` id.
                // Actually: ⊤ means the clause is trivially true; we use a sentinel.
                // For now: just don't push anything, and mark with a flag.
                // The simplest: push the top id itself (engine knows top = taut).
                // TODO: ideally signal "tautological clause, skip" to the caller.
                // For correctness, we push nothing (an empty head for a universal
                // clause means ⊥, not ⊤). We need to signal tautology differently.
                // SAFE CHOICE: emit Top as a special case in the clause head.
                // The engine treats any clause containing ⊤ in head as tautological.
                let top = self.pool.top();
                out.push(top);
            }
            // ⊥ in head: contributes nothing (deletes this disjunct).
            // Nominal/Self: gate-rejected; contribute nothing.
            ConceptExpr::Bot | ConceptExpr::Nominal(_) | ConceptExpr::SelfRestriction(_) => {
                let _ = out;
            }
            // B2 `≥n R.C` (§2.1): `≥0` ≡ ⊤ (drop); `≥1 R.C` ≡ ∃R.C; `≥n` (n≥2)
            // keeps the `Min(n,R,C')` literal with `C'` flattened to atomic. The
            // engine mints n distinct terms + pairwise Neq.
            ConceptExpr::Min(n, role, filler) => {
                if n == 0 {
                    // ≥0 R.C ≡ ⊤ — tautological disjunct; mark the clause taut.
                    let top = self.pool.top();
                    out.push(top);
                } else if n == 1 {
                    let flat = self.flatten_to_literal(filler);
                    let lit = self.pool.some(role, flat);
                    out.push(lit);
                } else {
                    let flat = self.flatten_to_literal(filler);
                    let lit = self.pool.min(n, role, flat);
                    out.push(lit);
                }
            }
            // B2 `≤n R.C` (§2.1): `≤0 R.C` ≡ ∀R.¬C (Tier-0 reduction); `≤n`
            // (n≥1) keeps the `Max(n,R,C')` literal (engine records `at_most`).
            ConceptExpr::Max(n, role, filler) => {
                if n == 0 {
                    // ≤0 R.C ≡ ∀R.¬C.
                    let neg_filler = self.neg_nnf(filler);
                    let flat_neg = self.flatten_to_literal(neg_filler);
                    let lit = self.pool.all(role, flat_neg);
                    out.push(lit);
                } else {
                    let flat = self.flatten_to_literal(filler);
                    let lit = self.pool.max(n, role, flat);
                    out.push(lit);
                }
            }
            ConceptExpr::Atomic(_) => {
                out.push(cid);
            }
            ConceptExpr::Not(_) => {
                // `¬B` head literal → positive complement atom (option (b)).
                let pos = self.positive_literal(cid);
                out.push(pos);
            }
            ConceptExpr::And(_) => {
                // And appearing as a disjunct within Or on the RHS:
                // introduce a single def atom for the whole conjunction.
                let cls = self.def_atom_for(cid);
                let atom = self.pool.atomic(cls);
                out.push(atom);
            }
            ConceptExpr::Or(disjuncts) => {
                let disjuncts = disjuncts.to_vec();
                for d in disjuncts {
                    self.flatten_head(d, out);
                }
            }
            ConceptExpr::Some(role, filler) => {
                let flat = self.flatten_to_literal(filler);
                let lit = self.pool.some(role, flat);
                out.push(lit);
            }
            ConceptExpr::All(role, filler) => {
                let flat = self.flatten_to_literal(filler);
                let lit = self.pool.all(role, flat);
                out.push(lit);
            }
        }
    }

    // ── Top-level axiom processing ─────────────────────────────────────────────

    /// First pass: fragment check over all axioms.
    fn check_axioms_fragment(&self, axioms: &[Axiom]) -> Result<(), &'static str> {
        for axiom in axioms {
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
        Ok(())
    }

    /// Second pass: emit clauses from all axioms.
    fn emit_clauses(&mut self, axioms: Vec<Axiom>) {
        for axiom in axioms {
            match axiom {
                Axiom::SubClassOf { sub, sup } => {
                    self.normalize_subclassof(sub, sup);
                }
                Axiom::EquivalentClasses(members) => {
                    for i in 0..members.len() {
                        for j in 0..members.len() {
                            if i != j {
                                self.normalize_subclassof(members[i], members[j]);
                            }
                        }
                    }
                }
                Axiom::DisjointClasses(members) => {
                    let bot_id = self.pool.bot();
                    for i in 0..members.len() {
                        for j in (i + 1)..members.len() {
                            let conj = self.pool.and([members[i], members[j]]);
                            self.normalize_subclassof(conj, bot_id);
                        }
                    }
                }
                Axiom::DisjointUnion { class, members } => {
                    let class_id = self.pool.atomic(class);
                    let union_id = self.pool.or(members.iter().copied());
                    self.normalize_subclassof(class_id, union_id);
                    self.normalize_subclassof(union_id, class_id);
                    let bot_id = self.pool.bot();
                    for i in 0..members.len() {
                        for j in (i + 1)..members.len() {
                            let conj = self.pool.and([members[i], members[j]]);
                            self.normalize_subclassof(conj, bot_id);
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
                    self.normalize_subclassof(some_r_top, domain);
                }
                Axiom::ObjectPropertyRange { role, range } => {
                    // ⊤ ⊑ ∀R.range
                    let top_id = self.pool.top();
                    let all_r_range = self.pool.all(role, range);
                    self.normalize_subclassof(top_id, all_r_range);
                }
                // Already handled / guarded in first pass.
                _ => {}
            }
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Normalize `internal` to ALCH clausal form, or `Err(reason)` naming the first
/// out-of-ALCH construct encountered.
///
/// Applies `nnf_axioms` internally (NNF is required for correct polarity handling
/// of negation in premise and head positions).
pub(crate) fn normalize(internal: &InternalOntology) -> Result<Normalized, &'static str> {
    // Clone and apply NNF (pushes Not to atomic positions).
    let mut onto = internal.clone();
    let axioms = nnf_axioms(&mut onto);

    // Build IRI list for DKey-prefix check (indexed by ClassId).
    let class_iris: Vec<String> = (0..onto.vocabulary.num_classes())
        .map(|i| {
            onto.vocabulary
                .class_iri(ClassId::new(u32::try_from(i).expect("fits")))
                .to_owned()
        })
        .collect();

    let mut n = Normalizer::new(onto.concepts.clone(), class_iris.len(), class_iris.clone());

    // Fragment gate: check all axioms (using NNF-transformed forms).
    n.check_axioms_fragment(&axioms)?;

    // Emit clauses.
    n.emit_clauses(axioms);

    // Collect reportable classes (non-DKey).
    let classes: Vec<ClassId> = (0..class_iris.len())
        .filter_map(|i| {
            if class_iris[i].starts_with(DKEY_IRI_PREFIX) {
                None
            } else {
                Some(ClassId::new(u32::try_from(i).expect("fits")))
            }
        })
        .collect();

    // Ensure every reportable class atom is interned in the pool — a
    // declared-but-unused class (no axiom references it) would otherwise have
    // no `Atomic(_)` ConceptId, panicking the engine's `atom_of_class` seed and
    // silently vanishing from the read-off. Interning is harmless (it adds an
    // isolated atom with no clauses → its root context derives only itself).
    for &cls in &classes {
        let _ = n.pool.atomic(cls);
    }

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
    use std::collections::BTreeSet;
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

    /// Like [`has_atomic_clause`] but order-insensitive: the clause's premise
    /// atoms equal `premise_classes` as a set, and its head atoms equal
    /// `head_classes` as a set (all `Atomic`, no `Not`/role literals).
    fn has_unordered_clause(
        norm: &Normalized,
        premise_classes: &[ClassId],
        head_classes: &[ClassId],
    ) -> bool {
        let as_classes = |lits: &[ConceptId]| -> Option<BTreeSet<ClassId>> {
            lits.iter()
                .map(|&cid| match norm.pool.get(cid) {
                    ConceptExpr::Atomic(id) => Some(*id),
                    _ => None,
                })
                .collect()
        };
        let want_prem: BTreeSet<ClassId> = premise_classes.iter().copied().collect();
        let want_head: BTreeSet<ClassId> = head_classes.iter().copied().collect();
        norm.clauses.iter().any(|cl| {
            cl.premise.len() == premise_classes.len()
                && cl.head.len() == head_classes.len()
                && as_classes(&cl.premise).is_some_and(|p| p == want_prem)
                && as_classes(&cl.head).is_some_and(|h| h == want_head)
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

        // Find {A} ⊑ {∃R.X}
        let some_clause = norm.clauses.iter().find(|cl| {
            cl.premise.len() == 1
                && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a)
                && cl.head.len() == 1
                && matches!(norm.pool.get(cl.head[0]), ConceptExpr::Some(r, _) if *r == role)
        });
        assert!(some_clause.is_some(), "expected {{A}} ⊑ {{∃R.X}} clause");

        let head_cid = some_clause.expect("clause present").head[0];
        let def_atom_id: ClassId = match norm.pool.get(head_cid) {
            ConceptExpr::Some(_, filler) => match norm.pool.get(*filler) {
                ConceptExpr::Atomic(id) => *id,
                other => panic!("filler should be atomic, got {other:?}"),
            },
            other => panic!("head should be ∃R.X, got {other:?}"),
        };

        // Forward: {X} ⊑ {B, C}
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
            prem_ok
                && head_classes.len() == 2
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
    // Polarity / negative-position tests (the critical cases the advisor flagged)
    // ─────────────────────────────────────────────────────────────────────────

    /// `A ⊑ ¬B` → option (b) positive-complement encoding: `{A} ⊑ {X}` where
    /// `X ⊑ ¬B`, defined by the disjointness clause `{B, X} ⊑ ⊥`. No raw `Not`
    /// survives.
    #[test]
    fn sub_neg_right_moves_to_premise() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             SubClassOf(:A ObjectComplementOf(:B))\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        // No clause head may contain a raw Not(Atomic) literal (option (b)).
        assert!(
            !norm.clauses.iter().any(|cl| cl
                .head
                .iter()
                .any(|&l| matches!(norm.pool.get(l), ConceptExpr::Not(_)))),
            "no raw Not head literal should survive; clauses: {:?}",
            norm.clauses
        );
        // Find {A} ⊑ {X} with X a positive atom (the complement atom, X ≢ B).
        let x: ClassId = norm
            .clauses
            .iter()
            .find_map(|cl| {
                if cl.premise.len() == 1
                    && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a)
                    && cl.head.len() == 1
                    && let ConceptExpr::Atomic(id) = norm.pool.get(cl.head[0])
                    && *id != b
                {
                    return Some(*id);
                }
                None
            })
            .unwrap_or_else(|| {
                panic!("expected {{A}} ⊑ {{X}} clause; clauses: {:?}", norm.clauses)
            });
        // X is defined by the disjointness clause {B, X} ⊑ ⊥.
        assert!(
            has_unordered_clause(&norm, &[b, x], &[]),
            "expected {{B, X}} ⊑ ⊥ disjointness clause (X={x:?}); clauses: {:?}",
            norm.clauses
        );
    }

    /// `∃R.B ⊑ D` → option (b): `{} ⊑ {∀R.X, D}` with `X ⊑ ¬B` (positive
    /// complement atom), defined by the disjointness clause `{B, X} ⊑ ⊥`.
    #[test]
    fn existential_on_lhs_converts_to_forall_neg_on_rhs() {
        let internal = parse(&onto(
            "Declaration(Class(:B))\n\
             Declaration(Class(:D))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(ObjectSomeValuesFrom(:R :B) :D)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let b = class_id(&internal, "B");
        let d = class_id(&internal, "D");
        let r_id = role_id_for(&internal, "R");
        let role = Role::named(r_id);

        // Expected clause: {} ⊑ {∀R.X, D} with X a positive atom (X ≢ B).
        let x: ClassId =
            norm.clauses
                .iter()
                .find_map(|cl| {
                    if !cl.premise.is_empty() {
                        return None;
                    }
                    let has_d = cl.head.iter().any(
                        |&cid| matches!(norm.pool.get(cid), ConceptExpr::Atomic(id) if *id == d),
                    );
                    if !has_d {
                        return None;
                    }
                    cl.head.iter().find_map(|&cid| {
                        if let ConceptExpr::All(r, filler) = norm.pool.get(cid)
                            && *r == role
                            && let ConceptExpr::Atomic(id) = norm.pool.get(*filler)
                            && *id != b
                        {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| {
                    panic!(
                        "expected {{}} ⊑ {{∀R.X, D}} clause; clauses: {:?}",
                        norm.clauses
                    )
                });
        assert!(
            has_unordered_clause(&norm, &[b, x], &[]),
            "expected {{B, X}} ⊑ ⊥ defining X; clauses: {:?}",
            norm.clauses
        );
    }

    /// `ObjectPropertyDomain(R, D)` → `∃R.⊤ ⊑ D` → `{} ⊑ {∀R.¬⊤, D}` = `{} ⊑ {∀R.⊥, D}`.
    ///
    /// This is the most common ALCH domain axiom pattern.
    #[test]
    fn property_domain_axiom_produces_forall_clause() {
        let internal = parse(&onto(
            "Declaration(Class(:D))\n\
             Declaration(ObjectProperty(:R))\n\
             ObjectPropertyDomain(:R :D)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let d = class_id(&internal, "D");
        let r_id = role_id_for(&internal, "R");
        let role = Role::named(r_id);

        // ∃R.⊤ ⊑ D → extract_premise(∃R.⊤) → extra head: ∀R.¬⊤ = ∀R.⊥
        // Expected clause: {} ⊑ {∀R.⊥, D}
        let found = norm.clauses.iter().any(|cl| {
            let prem_ok = cl.premise.is_empty();
            let has_d = cl
                .head
                .iter()
                .any(|&cid| matches!(norm.pool.get(cid), ConceptExpr::Atomic(id) if *id == d));
            let has_forall_bot = cl.head.iter().any(|&cid| {
                matches!(norm.pool.get(cid), ConceptExpr::All(r, filler)
                    if *r == role && matches!(norm.pool.get(*filler), ConceptExpr::Bot))
            });
            prem_ok && has_d && has_forall_bot
        });
        assert!(
            found,
            "expected {{}} ⊑ {{∀R.⊥, D}} clause; clauses: {:?}",
            norm.clauses
        );
    }

    /// `A ⊓ ∃R.B ⊑ C` → option (b): `{A} ⊑ {∀R.X, C}` with `X ⊑ ¬B` (positive
    /// complement atom), defined by the disjointness clause `{B, X} ⊑ ⊥`.
    #[test]
    fn and_lhs_with_existential() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(Class(:C))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R :B)) :C)\n",
        ));
        let norm = normalize(&internal).expect("ALCH");
        let a = class_id(&internal, "A");
        let b = class_id(&internal, "B");
        let c = class_id(&internal, "C");
        let r_id = role_id_for(&internal, "R");
        let role = Role::named(r_id);
        // Expected: {A} ⊑ {∀R.X, C} with X positive (X ≢ B).
        let x: ClassId = norm
            .clauses
            .iter()
            .find_map(|cl| {
                let prem_a = cl.premise.len() == 1
                    && matches!(norm.pool.get(cl.premise[0]), ConceptExpr::Atomic(id) if *id == a);
                let has_c = cl
                    .head
                    .iter()
                    .any(|&cid| matches!(norm.pool.get(cid), ConceptExpr::Atomic(id) if *id == c));
                if !(prem_a && has_c) {
                    return None;
                }
                cl.head.iter().find_map(|&cid| {
                    if let ConceptExpr::All(r, filler) = norm.pool.get(cid)
                        && *r == role
                        && let ConceptExpr::Atomic(id) = norm.pool.get(*filler)
                        && *id != b
                    {
                        Some(*id)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| panic!("expected {{A}} ⊑ {{∀R.X, C}}; clauses: {:?}", norm.clauses));
        assert!(
            has_unordered_clause(&norm, &[b, x], &[]),
            "expected {{B, X}} ⊑ ⊥"
        );
    }

    /// Def atom X for `B ⊔ C` gets backward clauses `B ⊑ X` and `C ⊑ X`.
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

        // Find the def atom X (filler of ∃R.X)
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

        // Backward: B ⊑ X and C ⊑ X.
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[b], &[x]),
            "expected B ⊑ X backward clause"
        );
        assert!(
            has_atomic_clause(&norm.clauses, &norm.pool, &[c], &[x]),
            "expected C ⊑ X backward clause"
        );
    }

    /// Dedup: the same sub-concept `B ⊔ C` used twice yields the same def atom.
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

    // ─────────────────────────────────────────────────────────────────────────
    // Fragment gate tests — each must return Err
    // ─────────────────────────────────────────────────────────────────────────

    /// B2 ALCHQ: `Max` (`≤n`) over a NAMED role is now IN fragment (admitted,
    /// lowered to a `Max` head literal the engine records as `at_most`).
    #[test]
    fn fragment_gate_max_cardinality_admitted() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectMaxCardinality(1 :R :B))\n",
        ));
        assert!(
            normalize(&internal).is_ok(),
            "≤n over a named role is in the B2 ALCHQ fragment"
        );
    }

    /// B2 ALCHQ: `Min` (`≥n`, n≥1) over a NAMED role is now IN fragment.
    #[test]
    fn fragment_gate_min_cardinality_admitted() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectMinCardinality(2 :R :B))\n",
        ));
        assert!(
            normalize(&internal).is_ok(),
            "≥n over a named role is in the B2 ALCHQ fragment"
        );
    }

    /// `≥n`/`≤n` over an INVERSE role stays out of fragment (B3).
    #[test]
    fn fragment_gate_inverse_cardinality_rejected() {
        let internal = parse(&onto(
            "Declaration(Class(:A))\n\
             Declaration(Class(:B))\n\
             Declaration(ObjectProperty(:R))\n\
             SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:R) :B))\n",
        ));
        assert!(
            matches!(normalize(&internal), Err("inverse role")),
            "≥n over an inverse role is B3, must be rejected"
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

    /// `ObjectHasValue` (nominal filler) → `Err` (out of fragment)
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

    /// Functional role → out of fragment
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
}
