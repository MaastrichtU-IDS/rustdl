//! Absorption: turn `TBox` axioms into focused triggers the tableau can
//! apply lazily.
//!
//! Three flavors are produced in one pipeline:
//!
//! - **Binary absorption (class trigger)** — `⊤ ⊑ ¬A ⊔ ψ` becomes
//!   `ConceptRule { trigger: A, conclusion: ψ }`. Fires when `A` shows
//!   up in a node label.
//! - **Nominal absorption** — `⊤ ⊑ ¬{a} ⊔ ψ` becomes
//!   `NominalRule { individual: a, conclusion: ψ }`. Applies directly
//!   to the named individual `a`.
//! - **Role absorption** — a `ConceptRule`/residual GCI whose conclusion
//!   is `∀R.D` is rewritten as `RoleRule { role: R, guard, target_label: D }`.
//!   Fires when an R-edge from a node carrying the guard (if any) is
//!   added. This is the second pass — it consumes the output of binary
//!   absorption.
//!
//! ## Why
//!
//! A naive tableau must apply every GCI `⊤ ⊑ φ` universally — to every
//! node, adding `φ` to its label. Disjunctive `φ` then causes branching
//! everywhere. Absorption finds patterns that let triggers fire only
//! when needed.
//!
//! ## Algorithm (single-trigger v0)
//!
//! For each input axiom, encode as `⊤ ⊑ φ`:
//!
//! - `SubClassOf { sub, sup }` → `φ = nnf(¬sub) ⊔ sup`.
//! - `EquivalentClasses(ids)` → decompose into pairwise `SubClassOf`.
//! - `DisjointClasses(ids)` → decompose into pairwise `SubClassOf(Ci, ¬Cj)`.
//! - `DisjointUnion { class, members }` → emit the equivalence half and
//!   pairwise-disjoint half.
//! - `ObjectPropertyDomain { role, domain }` → `∃role.⊤ ⊑ domain`.
//! - `ObjectPropertyRange { role, range }`  → `⊤ ⊑ ∀role.range`.
//!
//! Then walk the top-level disjuncts of `φ` looking for the first that
//! has shape `Not(Atomic(A))` or `Not(Nominal(a))`. If found, emit a
//! `ConceptRule` or `NominalRule` accordingly; the conclusion is the
//! `Or` of the remaining disjuncts. Otherwise `φ` joins `residual_gcis`.
//!
//! After the binary/nominal pass, [`absorb_roles`] rewrites every rule
//! or residual GCI whose conclusion is exactly `∀R.D` into a `RoleRule`.
//!
//! Multi-trigger absorption (`A ⊓ B ⊑ C`) is a Phase 4 refinement.

use std::collections::HashMap;

use crate::ConceptPool;
use crate::ir::{ClassId, ConceptExpr, ConceptId, IndividualId, Role};
use crate::normalize::to_nnf;
use crate::ontology::Axiom;

/// The output of absorption. Always a derived view of an `InternalOntology`'s
/// axiom list — never a replacement.
///
/// In addition to the four `Vec`-based axiom families, holds two
/// dispatch indices ([`Self::concept_rules_by_trigger`],
/// [`Self::nominal_rules_by_individual`]). They map a trigger to the
/// list of conclusions to apply, so [`crate::AbsorbedTBox`]-driven
/// tableau rules do `O(triggers × hits_per_trigger)` work per node
/// instead of `O(triggers × |rules|)`. [`absorb`] and [`absorb_roles`]
/// keep the indices in sync; callers who build an [`AbsorbedTBox`] by
/// hand should call [`Self::finalize`] before handing it to the
/// tableau (the tableau falls back to a linear scan when the indices
/// are empty, so this is "for performance, not correctness").
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct AbsorbedTBox {
    /// `A ⊑ ψ` — when the trigger class appears in a node label,
    /// add the conclusion concept.
    pub concept_rules: Vec<ConceptRule>,
    /// `{a} ⊑ ψ` — apply the conclusion directly to the named
    /// individual.
    pub nominal_rules: Vec<NominalRule>,
    /// `[guard ⊑] ∀R.D` — when an R-edge to `y` is added (from a node
    /// carrying the guard if `Some`, or any node if `None`), add
    /// `target_label` to `y`'s label.
    pub role_rules: Vec<RoleRule>,
    /// `⊤ ⊑ φ` — applied universally by the tableau, after every other
    /// pattern was tried.
    pub residual_gcis: Vec<ConceptId>,
    /// Subset of [`Self::residual_gcis`] whose body is `Or(_)` — the
    /// lazy-unfolding deferral candidates (see
    /// `docs/lazy-unfolding-plan.md`). `apply_residual_gcis` skips
    /// these (they're not materialised on every node eagerly);
    /// `apply_deferred_or_residuals` materialises them at saturate
    /// stable-state, but only on nodes where no disjunct is already
    /// present. Populated by [`Self::finalize`].
    pub deferred_or_residuals: Vec<ConceptId>,
    /// Index: every conclusion `ConceptId` that should fire for a
    /// given trigger class. Derived from `concept_rules` by
    /// [`Self::finalize`]; consulted by `apply_concept_rules` to skip
    /// the linear scan.
    pub concept_rules_by_trigger: HashMap<ClassId, Vec<ConceptId>>,
    /// Same idea for nominal rules — index by individual id.
    pub nominal_rules_by_individual: HashMap<IndividualId, Vec<ConceptId>>,
    /// `RoleRule`s with no class guard — they fire on any node that
    /// has an outgoing edge matching their `role`. Partition of
    /// `role_rules` produced by [`Self::finalize`].
    pub unguarded_role_rules: Vec<RoleRule>,
    /// Guarded `RoleRule`s indexed by guard class. Partition of
    /// `role_rules` produced by [`Self::finalize`].
    pub guarded_role_rules_by_guard: HashMap<ClassId, Vec<RoleRule>>,
}

impl AbsorbedTBox {
    /// Rebuild the dispatch indices from the canonical `Vec` fields.
    /// Idempotent — safe to call after any mutation of the rule lists.
    /// Linear in the total rule count; cheap.
    pub fn finalize(&mut self) {
        self.concept_rules_by_trigger.clear();
        self.concept_rules_by_trigger
            .reserve(self.concept_rules.len());
        for rule in &self.concept_rules {
            self.concept_rules_by_trigger
                .entry(rule.trigger)
                .or_default()
                .push(rule.conclusion);
        }
        self.nominal_rules_by_individual.clear();
        self.nominal_rules_by_individual
            .reserve(self.nominal_rules.len());
        for rule in &self.nominal_rules {
            self.nominal_rules_by_individual
                .entry(rule.individual)
                .or_default()
                .push(rule.conclusion);
        }
        self.unguarded_role_rules.clear();
        self.guarded_role_rules_by_guard.clear();
        for rule in &self.role_rules {
            match rule.guard {
                None => self.unguarded_role_rules.push(*rule),
                Some(g) => self
                    .guarded_role_rules_by_guard
                    .entry(g)
                    .or_default()
                    .push(*rule),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ConceptRule {
    pub trigger: ClassId,
    pub conclusion: ConceptId,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct NominalRule {
    pub individual: IndividualId,
    pub conclusion: ConceptId,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct RoleRule {
    /// The role expression to match against an edge incident on the
    /// labelled node. `Role::Named(r)` fires on outgoing r-edges;
    /// `Role::Inverse(r)` fires on incoming r-edges. Sub-role
    /// propagation is consulted by the tableau, not by absorption.
    pub role: Role,
    pub guard: Option<ClassId>,
    pub target_label: ConceptId,
}

/// Run absorption over the NNF axiom list. The full pipeline:
///
/// 1. Binary/nominal absorption: walk every axiom, encode as `⊤ ⊑ φ`,
///    extract a class or individual trigger when possible.
/// 2. Role absorption: rewrite rules whose conclusion is `∀R.D` as
///    `RoleRule`s.
#[must_use]
pub fn absorb(axioms_nnf: &[Axiom], pool: &mut ConceptPool) -> AbsorbedTBox {
    let mut tbox = AbsorbedTBox::default();
    for ax in axioms_nnf {
        absorb_one(ax, pool, &mut tbox);
    }
    absorb_roles(&mut tbox, pool);
    tbox
}

// `absorb_roles` is the last mutator on `concept_rules` /
// `nominal_rules`, so it owns the responsibility for refreshing the
// dispatch indices — see the `finalize()` call at its tail.

/// Second pass over an [`AbsorbedTBox`]: rewrite rules / residual GCIs of
/// shape `∀R.D` as [`RoleRule`]s. Conceptually a separate stage from
/// binary/nominal absorption, exposed publicly so consumers can run it
/// against an externally-built tbox.
pub fn absorb_roles(tbox: &mut AbsorbedTBox, pool: &mut ConceptPool) {
    // Concept rules with conclusion All(R, D) become guarded role rules.
    let mut kept = Vec::with_capacity(tbox.concept_rules.len());
    for rule in std::mem::take(&mut tbox.concept_rules) {
        if let ConceptExpr::All(role, target) = pool.get(rule.conclusion) {
            tbox.role_rules.push(RoleRule {
                role: *role,
                guard: Some(rule.trigger),
                target_label: *target,
            });
        } else {
            kept.push(rule);
        }
    }
    tbox.concept_rules = kept;

    // Residual GCIs of shape ⊤ ⊑ All(R, D) become unguarded role rules.
    let mut kept = Vec::with_capacity(tbox.residual_gcis.len());
    for gci in std::mem::take(&mut tbox.residual_gcis) {
        if let ConceptExpr::All(role, target) = pool.get(gci) {
            tbox.role_rules.push(RoleRule {
                role: *role,
                guard: None,
                target_label: *target,
            });
        } else {
            kept.push(gci);
        }
    }
    tbox.residual_gcis = kept;

    // Nominal rules with conclusion All(R, D): less common (the tableau
    // can handle these as nominal-plus-All) but follow the same pattern
    // for consistency.
    let mut kept = Vec::with_capacity(tbox.nominal_rules.len());
    for rule in std::mem::take(&mut tbox.nominal_rules) {
        if let ConceptExpr::All(role, target) = pool.get(rule.conclusion) {
            // Express as an *unguarded* role rule: the nominal-level
            // application is handled at ABox time by the tableau, which
            // walks edges from the specific individual. Phase 1 keeps it
            // as a NominalRule so the original individual stays attached.
            // No conversion here; leave the nominal rule unchanged.
            let _ = (role, target);
            kept.push(rule);
        } else {
            kept.push(rule);
        }
    }
    tbox.nominal_rules = kept;

    // Domain absorption (`RUSTDL_DOMAIN_ABSORPTION`, default ON since
    // 2026-08-05; `=0` reverts).
    // Runs *after* the two rewrites above so their priority is
    // unchanged: a singleton `∀R.D` residual is still consumed as a
    // range-style `RoleRule`, and only what survives is offered here.
    if domain_absorption_enabled() {
        absorb_domain_residuals(tbox, pool);
    }

    // Nominal-existential absorption (`RUSTDL_NOMINAL_EXISTS_ABSORPTION`,
    // default ON since 2026-08-25; `=0` reverts). Runs after domain absorption
    // so that pass keeps first claim on any residual carrying both shapes; the
    // two triggers are disjoint by construction (`∀R.⊥` vs `∀R.¬{a}`), so the
    // order is a stability guarantee rather than a live tie-break.
    if nominal_exists_absorption_enabled() {
        absorb_nominal_exists_residuals(tbox, pool);
    }

    // Lazy-unfolding split: precompute the `Or(_)`-shaped residual
    // GCIs so the tableau can defer their materialisation to
    // saturate stable-state instead of asserting them on every
    // node. See `docs/lazy-unfolding-plan.md`. The eager residuals
    // stay in `residual_gcis` and `apply_residual_gcis` skips the
    // Or-shaped ones, which `apply_deferred_or_residuals` handles.
    tbox.deferred_or_residuals = tbox
        .residual_gcis
        .iter()
        .copied()
        .filter(|&g| matches!(pool.get(g), ConceptExpr::Or(_)))
        .collect();
    // Sorted so `apply_residual_gcis` can binary_search to skip the
    // deferred entries. The set defines *exactly* which residuals
    // are deferred — `apply_residual_gcis` skips members,
    // `apply_deferred_or_residuals` materialises them, so the two
    // stay consistent even for hand-built TBoxes that never run this
    // split (empty set ⇒ everything eager ⇒ sound, just unoptimised).
    tbox.deferred_or_residuals
        .sort_unstable_by_key(|c| c.index());

    // Rebuild the dispatch indices now that every mutator has run.
    tbox.finalize();
}

/// `RUSTDL_DOMAIN_ABSORPTION` — domain absorption
/// ([`absorb_domain_residuals`]). **Default ON** since 2026-08-05; `=0` reverts.
///
/// Flipped on the strength of a full 1,920-ontology two-arm sweep
/// (`docs/2026-08-04-domain-absorption-default-decision.md`): **3 recoveries**
/// (`ore_ont_16372` 60 s→8.36 s, `6132`→32.46 s, `9899`→32.86 s, all
/// peer-solvable) and **0 answer changes** across all 1,750 ontologies that
/// complete in both arms, with median wall delta `+0.000 s`. Zero correctness
/// exposure is expected by construction — this rewrite is a logical identity
/// with `ObjectPropertyDomain`, so a differing closure would be a bug, not a
/// trade-off.
///
/// **Known cost, and it is real:** `ore_ont_7011` 5.05 s→17.53 s (3.5×) and
/// `ore_ont_13545` 5.35 s→15.47 s (2.9×), both with byte-identical output. One
/// ontology also crosses a 60 s cap — `ore_ont_14351` 59.96 s→61.47 s, output
/// unchanged — so at a 60 s budget the net is +2 completions rather than +3.
/// No *fast* ontology becomes a DNF, which is the failure mode that made the
/// v0.4.8 `RUSTDL_CLASSIFY_INCONSISTENCY` flip a regression.
#[must_use]
pub fn domain_absorption_enabled() -> bool {
    // Default-ON idiom (see the house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_DOMAIN_ABSORPTION").is_none_or(|v| v != "0")
}

/// Recognise a **domain-absorbable** disjunct and return its role.
///
/// A residual body `d₁ ⊔ … ⊔ dₙ` stands for `⊤ ⊑ d₁ ⊔ … ⊔ dₙ`, so a
/// disjunct `dᵢ` contributes the *antecedent* `¬dᵢ`. Exactly two NNF
/// shapes give the unqualified existential antecedent `∃R.⊤`:
///
/// | disjunct | antecedent | reading |
/// |---|---|---|
/// | `Max(0, R, ⊤)` | `≥1 R` | `(≥1 R) ⊑ rest` |
/// | `All(R, ⊥)`    | `∃R.⊤` | `∃R.⊤ ⊑ rest`   |
///
/// Both are **logically identical to `ObjectPropertyDomain(R, rest)`**,
/// hence sound *and* completeness-preserving to absorb.
///
/// # Soundness boundaries — the whole risk lives here
///
/// - **`Max(k, R, _)` with `k ≥ 1` is rejected.** Its antecedent is
///   `≥ k+1 R`, needing at least two successors; a domain rule fires at
///   the *first* edge, so absorbing it would be **strictly too strong —
///   UNSOUND**.
/// - **`All(R, D)` with `D ≠ ⊥` is rejected**, as is `Max(0, R, C)` with
///   `C ≠ ⊤`. Those are the *qualified* antecedents `∃R.¬D` / `∃R.C`,
///   which need a filler check and do not reduce to a domain axiom.
///
/// Mirrors [`crate::residual_absorbability::Bucket::DomainAbsorbable`],
/// which measured the population these two shapes cover.
fn as_domain_trigger(cid: ConceptId, pool: &ConceptPool) -> Option<Role> {
    match pool.get(cid) {
        // `≤0 R.⊤` = `¬∃R.⊤`. Qualified (`filler ≠ ⊤`) must NOT match.
        ConceptExpr::Max(0, role, filler) => {
            matches!(pool.get(*filler), ConceptExpr::Top).then_some(*role)
        }
        // `∀R.⊥` = `¬∃R.⊤`. `∀R.D` with `D ≠ ⊥` must NOT match.
        ConceptExpr::All(role, inner) => {
            matches!(pool.get(*inner), ConceptExpr::Bot).then_some(*role)
        }
        _ => None,
    }
}

/// Domain absorption: rewrite every residual GCI carrying a
/// domain-absorbable disjunct as an **unguarded [`RoleRule`]**, so it
/// fires on edge creation instead of being re-applied at every node.
///
/// `⊤ ⊑ ¬∃R.⊤ ⊔ rest` ≡ `∃R.⊤ ⊑ rest` ≡ `⊤ ⊑ ∀R⁻.rest`, and
/// `⊤ ⊑ ∀S.ψ` is exactly `RoleRule { role: S, guard: None, target_label: ψ }`
/// — the tableau adds `target_label` to the *neighbour* across a matching
/// edge, and the neighbour across an `R⁻` edge is the R-**predecessor**,
/// i.e. the node the domain axiom constrains. Hence `role.flip()`.
/// (When `R` is itself `Role::Inverse(r)` the flip yields `Role::Named(r)`
/// and the rule degenerates to the range-style form, which is correct:
/// `∃r⁻.⊤ ⊑ rest` constrains r-edge *targets*.)
///
/// Sub-role propagation is handled by the tableau's `edge_satisfies`, so
/// an `s`-edge with `s ⊑ r` fires an `r`-domain rule as it should.
///
/// An empty `rest` (`⊤ ⊑ ¬∃R.⊤`, i.e. `R` has no edges in any model)
/// normalises to `⊥` via [`ConceptPool::or`], giving a rule that clashes
/// any node with an `R`-successor — the intended reading.
fn absorb_domain_residuals(tbox: &mut AbsorbedTBox, pool: &mut ConceptPool) {
    let mut kept = Vec::with_capacity(tbox.residual_gcis.len());
    for gci in std::mem::take(&mut tbox.residual_gcis) {
        // A non-`Or` body is its own singleton disjunct set — the same
        // convention `absorb_gci` and the census classifier use.
        let disjuncts: Vec<ConceptId> = match pool.get(gci) {
            ConceptExpr::Or(args) => args.to_vec(),
            _ => vec![gci],
        };
        let found = disjuncts
            .iter()
            .enumerate()
            .find_map(|(i, &d)| as_domain_trigger(d, pool).map(|r| (i, r)));
        let Some((pos, role)) = found else {
            kept.push(gci);
            continue;
        };
        let rest: Vec<ConceptId> = disjuncts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| (i != pos).then_some(c))
            .collect();
        let target_label = pool.or(rest);
        tbox.role_rules.push(RoleRule {
            role: role.flip(),
            guard: None,
            target_label,
        });
    }
    tbox.residual_gcis = kept;
}

/// `RUSTDL_NOMINAL_EXISTS_ABSORPTION` — nominal-existential absorption
/// ([`absorb_nominal_exists_residuals`]). **Default ON**; `=0` reverts to the
/// pre-2026-08-25 behaviour where `∃R.{a} ⊑ ψ` stayed an untriggered residual.
///
/// Closes issue #70: `EquivalentClasses(Q, ObjectHasValue(p, b))` left the
/// residual `⊤ ⊑ ∀p.¬{b} ⊔ Q`, whose `Q` disjunct is *generating* — picking it
/// forces `∃p.{b}`, whose fresh witness is itself nominal, gets the residual,
/// picks `Q`, and generates again. On the deadline-free query paths
/// (`is_class_satisfiable`, `is_consistent`, un-timed `realize`) the main
/// tableau runs anywhere-blocking, and `is_blocked_anywhere` refuses to block a
/// nominal node — so nothing ever cut the cycle and a two-axiom ontology ran for
/// hours. Absorbing the antecedent removes the residual, so the branch point
/// never exists.
#[must_use]
pub fn nominal_exists_absorption_enabled() -> bool {
    // Default-ON idiom (see the house convention): an EMPTY value enables.
    std::env::var_os("RUSTDL_NOMINAL_EXISTS_ABSORPTION").is_none_or(|v| v != "0")
}

/// Recognise a **nominal-existential-absorbable** disjunct and return the role
/// and individual of the antecedent it contributes.
///
/// A residual body `d₁ ⊔ … ⊔ dₙ` stands for `⊤ ⊑ d₁ ⊔ … ⊔ dₙ`, so a disjunct
/// `dᵢ` contributes the *antecedent* `¬dᵢ`. Two NNF shapes give the
/// nominal-filler existential antecedent `∃R.{a}`:
///
/// | disjunct | antecedent |
/// |---|---|
/// | `∀R.¬{a}`   | `∃R.{a}` |
/// | `≤0 R.{a}`  | `∃R.{a}` |
///
/// # Soundness boundaries — the whole risk lives here
///
/// - **`Max(k, R, {a})` with `k ≥ 1` is rejected.** Its antecedent is
///   `≥ k+1 R.{a}`, which needs `k+1` distinct successors; the rule emitted
///   below fires off a single edge, so absorbing it would be **strictly too
///   strong — UNSOUND**. (`{a}` is a singleton, so such a shape is
///   unsatisfiable in the antecedent anyway; rejecting it is the conservative
///   reading and costs nothing.)
/// - **`∀R.D` with `D` not a negated nominal is rejected** — that is either the
///   unqualified domain antecedent (handled by [`as_domain_trigger`]) or a
///   genuinely qualified one needing a filler check.
fn as_nominal_exists_trigger(cid: ConceptId, pool: &ConceptPool) -> Option<(Role, IndividualId)> {
    match pool.get(cid) {
        // `∀R.¬{a}` = `¬∃R.{a}`.
        ConceptExpr::All(role, inner) => match pool.get(*inner) {
            ConceptExpr::Not(negated) => match pool.get(*negated) {
                ConceptExpr::Nominal(i) => Some((*role, *i)),
                _ => None,
            },
            _ => None,
        },
        // `≤0 R.{a}` = `¬∃R.{a}`. `k ≥ 1` must NOT match.
        ConceptExpr::Max(0, role, filler) => match pool.get(*filler) {
            ConceptExpr::Nominal(i) => Some((*role, *i)),
            _ => None,
        },
        _ => None,
    }
}

/// Nominal-existential absorption: rewrite every residual GCI carrying a
/// nominal-existential antecedent as a [`NominalRule`], so it is triggered by
/// the individual it names instead of being re-applied at every node.
///
/// `⊤ ⊑ ¬∃R.{a} ⊔ ψ` ≡ `∃R.{a} ⊑ ψ` ≡ `{a} ⊑ ∀R⁻.ψ`, and a `NominalRule`
/// carrying the conclusion `∀R⁻.ψ` is exactly that: the tableau attaches it to
/// whatever node bears the `Nominal(a)` label, and the `∀`-rule then propagates
/// `ψ` back across the R-edge to the R-**predecessor** — the node the axiom
/// constrains. Hence `role.flip()`, the same move [`absorb_domain_residuals`]
/// makes for its unguarded `RoleRule`. (When `R` is itself `Role::Inverse(r)`
/// the flip yields `Role::Named(r)`, which is correct: `∃r⁻.{a} ⊑ ψ`
/// constrains r-edge *targets*.)
///
/// This is a **logical identity**, so it is sound and completeness-preserving
/// by construction; a differing closure would be a bug, not a trade-off. Note
/// the equivalence needs no assumption about `{a}` beyond it being a nominal:
/// it is plain quantifier duality, valid for any filler. Restricting to
/// nominals is what makes the *rule form* available — a `NominalRule` needs an
/// individual to key on.
///
/// Sub-role propagation is handled by the tableau's `edge_satisfies`, so an
/// `s`-edge with `s ⊑ r` fires an `r`-keyed rule as it should.
///
/// An empty `ψ` cannot reach here: the body would then be the singleton
/// `∀R.¬{a}`, which the bare-`All` residual rewrite earlier in [`absorb_roles`]
/// already claims as an unguarded `RoleRule` — the same axiom in a different
/// triggered form. Pinned by
/// `nominal_exists_bot_consequent_is_claimed_by_bare_all_rewrite`. Were it ever
/// to arrive, [`ConceptPool::or`] would normalise it to `⊥`, giving
/// `{a} ⊑ ∀R⁻.⊥`, which is the intended reading anyway.
fn absorb_nominal_exists_residuals(tbox: &mut AbsorbedTBox, pool: &mut ConceptPool) {
    let mut kept = Vec::with_capacity(tbox.residual_gcis.len());
    for gci in std::mem::take(&mut tbox.residual_gcis) {
        // A non-`Or` body is its own singleton disjunct set — the same
        // convention `absorb_gci` and `absorb_domain_residuals` use.
        let disjuncts: Vec<ConceptId> = match pool.get(gci) {
            ConceptExpr::Or(args) => args.to_vec(),
            _ => vec![gci],
        };
        let found = disjuncts
            .iter()
            .enumerate()
            .find_map(|(i, &d)| as_nominal_exists_trigger(d, pool).map(|t| (i, t)));
        let Some((pos, (role, individual))) = found else {
            kept.push(gci);
            continue;
        };
        let rest: Vec<ConceptId> = disjuncts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| (i != pos).then_some(c))
            .collect();
        let psi = pool.or(rest);
        let conclusion = pool.all(role.flip(), psi);
        tbox.nominal_rules.push(NominalRule {
            individual,
            conclusion,
        });
    }
    tbox.residual_gcis = kept;
}

fn absorb_one(ax: &Axiom, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    match ax {
        Axiom::SubClassOf { sub, sup } => absorb_sub_sup(*sub, *sup, pool, tbox),
        Axiom::EquivalentClasses(ids) => {
            for i in 0..ids.len() {
                for j in 0..ids.len() {
                    if i != j {
                        absorb_sub_sup(ids[i], ids[j], pool, tbox);
                    }
                }
            }
        }
        Axiom::DisjointClasses(ids) => {
            emit_pairwise_disjoint(ids, pool, tbox);
        }
        Axiom::DisjointUnion { class, members } => {
            let class_concept = pool.atomic(*class);
            let union_concept = pool.or(members.iter().copied());
            // The equivalence half.
            absorb_sub_sup(class_concept, union_concept, pool, tbox);
            absorb_sub_sup(union_concept, class_concept, pool, tbox);
            // Pairwise-disjoint half.
            emit_pairwise_disjoint(members, pool, tbox);
        }
        Axiom::ObjectPropertyDomain { role, domain } => {
            // ∃role.⊤ ⊑ domain
            let top = pool.top();
            let some_r_top = pool.some(*role, top);
            absorb_sub_sup(some_r_top, *domain, pool, tbox);
        }
        Axiom::ObjectPropertyRange { role, range } => {
            // ⊤ ⊑ ∀role.range — a clean residual GCI.
            let all_r = pool.all(*role, *range);
            tbox.residual_gcis.push(all_r);
        }
        _ => {
            // Role characteristics, ABox, declarations — not TBox content.
            // They flow through to the reasoner via separate paths.
        }
    }
}

fn emit_pairwise_disjoint(ids: &[ConceptId], pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let not_cj = pool.not(ids[j]);
            let not_cj_nnf = to_nnf(not_cj, pool);
            absorb_sub_sup(ids[i], not_cj_nnf, pool, tbox);
        }
    }
}

/// Encode `sub ⊑ sup` as `⊤ ⊑ nnf(¬sub) ⊔ sup` and try to extract a trigger.
fn absorb_sub_sup(sub: ConceptId, sup: ConceptId, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    // Distribute a union on the LHS: `(D₁ ⊔ … ⊔ Dₙ) ⊑ sup ≡ ⋀ᵢ Dᵢ ⊑ sup`
    // (a logical equivalence, sound). Without this, the GCI internalizes to
    // `⊤ ⊑ (¬D₁ ⊓ … ⊓ ¬Dₙ) ⊔ sup`, whose top-level Or has no `Not(Atomic)`
    // disjunct, so `absorb_gci` files it as a residual Or-GCI — and the
    // saturator's forward closure never derives `Dᵢ ⊑ sup`, a classify miss
    // (found on ore_ont_3914-class ontologies with `(A⊔B)⊑C` axioms and via
    // the sufficient direction of `X ≡ (A⊔B)`-style equivalences). Recursion
    // terminates because `pool.or` flattens nested unions, so each `Dᵢ` here
    // is itself not an `Or`.
    // Own the disjuncts before the recursive `&mut pool` calls (the `to_vec`
    // releases the immutable borrow from `pool.get`).
    let union_args: Option<Vec<ConceptId>> = match pool.get(sub) {
        ConceptExpr::Or(args) => Some(args.to_vec()),
        _ => None,
    };
    if let Some(args) = union_args {
        for arg in args {
            absorb_sub_sup(arg, sup, pool, tbox);
        }
        return;
    }
    let neg_sub = pool.not(sub);
    let neg_sub_nnf = to_nnf(neg_sub, pool);
    let disjunction = pool.or([neg_sub_nnf, sup]);
    absorb_gci(disjunction, pool, tbox);
}

/// Process a `⊤ ⊑ φ` GCI: extract a `Not(Atomic)` or `Not(Nominal)`
/// disjunct as trigger if any, otherwise add `φ` to the residual list.
fn absorb_gci(phi: ConceptId, pool: &mut ConceptPool, tbox: &mut AbsorbedTBox) {
    let disjuncts: Vec<ConceptId> = match pool.get(phi) {
        ConceptExpr::Or(args) => args.to_vec(),
        _ => vec![phi],
    };

    // Find first disjunct of the form Not(Atomic) or Not(Nominal).
    let mut chosen: Option<(usize, Trigger)> = None;
    for (i, &d) in disjuncts.iter().enumerate() {
        if let Some(t) = as_trigger(d, pool) {
            chosen = Some((i, t));
            break;
        }
    }

    if let Some((pos, trigger)) = chosen {
        let rest: Vec<ConceptId> = disjuncts
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| (i != pos).then_some(c))
            .collect();
        // Or normalizations handle empty (→ Bot), single (→ operand), or
        // multi-operand cases.
        let conclusion = pool.or(rest);
        match trigger {
            Trigger::Class(trigger) => tbox.concept_rules.push(ConceptRule {
                trigger,
                conclusion,
            }),
            Trigger::Individual(individual) => tbox.nominal_rules.push(NominalRule {
                individual,
                conclusion,
            }),
        }
    } else {
        tbox.residual_gcis.push(phi);
    }
}

/// What kind of "trigger" a `Not(...)` disjunct can produce.
enum Trigger {
    Class(ClassId),
    Individual(IndividualId),
}

/// Recognize `Not(Atomic(A))` or `Not(Nominal(a))` shapes; otherwise None.
fn as_trigger(cid: ConceptId, pool: &ConceptPool) -> Option<Trigger> {
    if let ConceptExpr::Not(inner) = pool.get(cid) {
        match pool.get(*inner) {
            ConceptExpr::Atomic(c) => Some(Trigger::Class(*c)),
            ConceptExpr::Nominal(i) => Some(Trigger::Individual(*i)),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::many_single_char_names)]

    use super::*;
    use crate::Vocabulary;
    use crate::ir::{Role, RoleId};
    use crate::nnf_axioms;
    use crate::ontology::InternalOntology;

    fn fresh(class_names: &[&str]) -> InternalOntology {
        let mut o = InternalOntology::new();
        for n in class_names {
            o.vocabulary.intern_class(n);
        }
        o
    }

    fn cid(o: &InternalOntology, name: &str) -> ClassId {
        o.vocabulary.class_id(name).expect("class missing")
    }

    fn atom(o: &mut InternalOntology, name: &str) -> ConceptId {
        let c = cid(o, name);
        o.concepts.atomic(c)
    }

    /// NNF the ontology's axioms and run absorption. Returns the absorbed
    /// tbox and the NNF'd axioms (for inspection in tests).
    fn run(o: &mut InternalOntology) -> AbsorbedTBox {
        let nnf = nnf_axioms(o);
        absorb(&nnf, &mut o.concepts)
    }

    #[test]
    fn atomic_sub_class_of_yields_one_rule() {
        // A ⊑ B  →  rule (A, B).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.concept_rules[0].trigger, cid(&o, "A"));
        assert_eq!(t.concept_rules[0].conclusion, b);
    }

    #[test]
    fn sub_class_of_with_conjunctive_conclusion() {
        // A ⊑ B ⊓ C  →  rule (A, And([B, C])).
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let and_bc = o.concepts.and([b, cc]);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: and_bc,
        });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert_eq!(t.concept_rules[0].trigger, cid(&o, "A"));
        assert_eq!(t.concept_rules[0].conclusion, and_bc);
    }

    #[test]
    fn complex_lhs_with_atomic_rhs_absorbs_via_double_negation() {
        // (B ⊓ C) ⊑ A  →  ⊤ ⊑ ¬(B⊓C) ⊔ A  →  ⊤ ⊑ ¬B ⊔ ¬C ⊔ A.
        // Top-level disjuncts include Not(B) and Not(C); pick one (first
        // by id) as trigger, conclusion = Or of the rest.
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let and_bc = o.concepts.and([b, cc]);
        o.axioms.push(Axiom::SubClassOf {
            sub: and_bc,
            sup: a,
        });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        assert!(t.residual_gcis.is_empty());
        // The trigger must be one of B or C (whichever Not is sorted first).
        let trigger = t.concept_rules[0].trigger;
        assert!(trigger == cid(&o, "B") || trigger == cid(&o, "C"));
    }

    #[test]
    fn union_lhs_distributes_into_per_disjunct_rules() {
        // (A ⊔ B) ⊑ C  ≡  A ⊑ C  ∧  B ⊑ C  →  rules (A, C) and (B, C), no
        // residual. Without distribution this GCI becomes ⊤ ⊑ (¬A⊓¬B) ⊔ C,
        // which has no Not(Atomic) top-level disjunct and falls to a residual
        // Or-GCI — the classify miss found on ore_ont_3914-class ontologies.
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let or_ab = o.concepts.or([a, b]);
        o.axioms.push(Axiom::SubClassOf {
            sub: or_ab,
            sup: cc,
        });
        let t = run(&mut o);
        assert!(
            t.residual_gcis.is_empty(),
            "union-LHS must distribute, not fall to a residual Or-GCI"
        );
        assert_eq!(t.concept_rules.len(), 2);
        let triggers: std::collections::HashSet<ClassId> =
            t.concept_rules.iter().map(|r| r.trigger).collect();
        assert!(triggers.contains(&cid(&o, "A")));
        assert!(triggers.contains(&cid(&o, "B")));
        for r in &t.concept_rules {
            assert_eq!(r.conclusion, cc, "each disjunct-rule concludes C");
        }
    }

    #[test]
    fn pure_existential_gci_is_residual() {
        // ⊤ ⊑ ∃R.A  has no Not(Atomic) top-level disjunct → residual.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let some_a = o.concepts.some(r, a);
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf {
            sub: top,
            sup: some_a,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        assert_eq!(t.residual_gcis[0], some_a);
    }

    #[test]
    fn equivalent_classes_creates_pairwise_rules() {
        // A ≡ B  →  rules (A, B) and (B, A).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::EquivalentClasses(vec![a, b]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 2);
        // One rule for each direction.
        let triggers: Vec<ClassId> = t.concept_rules.iter().map(|r| r.trigger).collect();
        assert!(triggers.contains(&cid(&o, "A")));
        assert!(triggers.contains(&cid(&o, "B")));
    }

    #[test]
    fn disjoint_classes_yields_not_atom_conclusion() {
        // DisjointClasses(A, B)  →  rule (A, Not(B)).
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::DisjointClasses(vec![a, b]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 1);
        let rule = &t.concept_rules[0];
        // The trigger is whichever Not gets matched first — but both
        // operands are atoms, so the trigger and conclusion partition them.
        let trigger = rule.trigger;
        assert!(trigger == cid(&o, "A") || trigger == cid(&o, "B"));
        // Conclusion must be Not(Atomic(other)).
        let other = if trigger == cid(&o, "A") {
            cid(&o, "B")
        } else {
            cid(&o, "A")
        };
        let expected_other_atom = o.concepts.atomic(other);
        let expected_conclusion = o.concepts.not(expected_other_atom);
        assert_eq!(rule.conclusion, expected_conclusion);
    }

    #[test]
    fn disjoint_classes_three_way_yields_three_pairwise_rules() {
        // DisjointClasses(A, B, C) — pairs are (A,B), (A,C), (B,C).
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        o.axioms.push(Axiom::DisjointClasses(vec![a, b, cc]));
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 3);
    }

    #[test]
    fn disjoint_union_emits_subsumption_and_pairwise_disjoint_rules() {
        // DisjointUnion(P, [C1, C2]):
        //   1. P ⊑ C1 ⊔ C2     →  one rule (P, Or([C1, C2]))
        //   2. C1 ⊔ C2 ⊑ P    →  distributes to rules (C1, P) and (C2, P)
        //   3. C1 ⌖ C2         →  one rule (C1, Not(C2))
        // The LHS-union direction (2) is now distributed rather than dropped to
        // a residual Or-GCI (the union-LHS completeness fix), so the sound
        // subsumptions C1 ⊑ P / C2 ⊑ P are captured.
        let mut o = fresh(&["P", "C1", "C2"]);
        let c1 = atom(&mut o, "C1");
        let c2 = atom(&mut o, "C2");
        o.axioms.push(Axiom::DisjointUnion {
            class: cid(&o, "P"),
            members: vec![c1, c2],
        });
        let p_atom = atom(&mut o, "P");
        let t = run(&mut o);
        // (P, Or[C1,C2]) + (C1, P) + (C2, P) + (C1, ¬C2) = 4 rules, 0 residual.
        assert_eq!(t.concept_rules.len(), 4);
        assert!(t.residual_gcis.is_empty());
        // C1 ⊑ P and C2 ⊑ P both captured (conclusion = atom P).
        let members_subsuming_p: std::collections::HashSet<ClassId> = t
            .concept_rules
            .iter()
            .filter(|r| r.conclusion == p_atom)
            .map(|r| r.trigger)
            .collect();
        assert!(members_subsuming_p.contains(&cid(&o, "C1")));
        assert!(members_subsuming_p.contains(&cid(&o, "C2")));
    }

    #[test]
    fn object_property_range_becomes_unguarded_role_rule() {
        // ObjectPropertyRange(r, A)  ≡  ⊤ ⊑ ∀r.A
        // After binary absorption: residual GCI ∀r.A.
        // After role absorption: RoleRule { role: r, guard: None, target_label: A }.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        o.axioms
            .push(Axiom::ObjectPropertyRange { role: r, range: a });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.role_rules.len(), 1);
        let rr = t.role_rules[0];
        assert_eq!(rr.role, crate::Role::Named(r.role_id()));
        assert_eq!(rr.guard, None);
        assert_eq!(rr.target_label, a);
    }

    #[test]
    fn sub_class_of_all_becomes_guarded_role_rule() {
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let all_r_b = o.concepts.all(r, b);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: all_r_b,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.role_rules.len(), 1);
        let rr = t.role_rules[0];
        assert_eq!(rr.role, crate::Role::Named(r.role_id()));
        assert_eq!(rr.guard, Some(cid(&o, "A")));
        assert_eq!(rr.target_label, b);
    }

    // ─────────────────── domain absorption ───────────────────
    //
    // NEGATIVES FIRST. The two shapes that must NOT be absorbed carry the
    // whole risk of this feature:
    //
    //   * `Max(k, R, _)` with `k ≥ 1` — antecedent `≥ k+1 R`, needs two or
    //     more successors. A domain rule fires at the FIRST edge, so
    //     absorbing it is strictly too strong ⇒ **UNSOUND** (false positives).
    //   * `All(R, D)` with `D ≠ ⊥` (and its `Max(0, R, C≠⊤)` twin) — the
    //     *qualified* antecedent `∃R.¬D` / `∃R.C`, which needs a filler check
    //     and does not reduce to a domain axiom.
    //
    // Each is asserted to stay a residual under the flag, i.e. the feature
    // declines it. Both are also guarded end-to-end (verdict level) in
    // `crates/owl-dl-reasoner/tests/domain_absorption.rs`.

    /// All tests that flip `RUSTDL_DOMAIN_ABSORPTION` hold this lock.
    static DOMAIN_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard: set `RUSTDL_DOMAIN_ABSORPTION`, restore prior on drop.
    ///
    /// SAFETY: `set_var`/`remove_var` is `unsafe` under edition 2024;
    /// serialised by `DOMAIN_ENV_LOCK` within this module.
    struct DomainGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl DomainGuard {
        #[allow(unsafe_code)]
        fn set(value: &str) -> Self {
            let prior = std::env::var_os("RUSTDL_DOMAIN_ABSORPTION");
            // SAFETY: serialised by DOMAIN_ENV_LOCK; restored on Drop.
            unsafe { std::env::set_var("RUSTDL_DOMAIN_ABSORPTION", value) };
            Self { prior }
        }
    }
    impl Drop for DomainGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: see DomainGuard::set.
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var("RUSTDL_DOMAIN_ABSORPTION", v),
                    None => std::env::remove_var("RUSTDL_DOMAIN_ABSORPTION"),
                }
            }
        }
    }

    /// `(≥2 R) ⊑ A` — NNF disjunct `Max(1, R, ⊤)`. Antecedent needs TWO
    /// successors; a domain rule fires at the first. **Must stay residual.**
    #[test]
    fn max_k_ge_1_is_not_domain_absorbed() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let top = o.concepts.top();
        let min2 = o.concepts.min(2, r, top);
        o.axioms.push(Axiom::SubClassOf { sub: min2, sup: a });
        let t = run(&mut o);
        assert_eq!(
            t.residual_gcis.len(),
            1,
            "≥2 R antecedent must NOT be domain-absorbed (unsound); tbox={t:?}"
        );
        assert!(t.role_rules.is_empty(), "no role rule may be emitted");
    }

    /// `∃R.E ⊑ A` — NNF disjunct `All(R, ¬E)`, a *qualified* antecedent.
    /// **Must stay residual** (needs a filler check).
    #[test]
    fn all_non_bot_filler_is_not_domain_absorbed() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["A", "E"]);
        let a = atom(&mut o, "A");
        let e = atom(&mut o, "E");
        let r = Role::named(RoleId::new(0));
        let some_r_e = o.concepts.some(r, e);
        o.axioms.push(Axiom::SubClassOf {
            sub: some_r_e,
            sup: a,
        });
        let t = run(&mut o);
        assert_eq!(
            t.residual_gcis.len(),
            1,
            "qualified ∃R.E antecedent must NOT be domain-absorbed; tbox={t:?}"
        );
        assert!(t.role_rules.is_empty());
    }

    /// `Max(0, R, C)` with `C ≠ ⊤` is `¬∃R.C` — the same qualified case
    /// written as a `≤0` cardinality. **Must stay residual.**
    #[test]
    fn max_zero_qualified_is_not_domain_absorbed() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["A", "C"]);
        let a = atom(&mut o, "A");
        let c = atom(&mut o, "C");
        let r = Role::named(RoleId::new(0));
        let max0_qual = o.concepts.max(0, r, c);
        let body = o.concepts.or([max0_qual, a]);
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf {
            sub: top,
            sup: body,
        });
        let t = run(&mut o);
        assert_eq!(
            t.residual_gcis.len(),
            1,
            "≤0 R.C with C ≠ ⊤ must NOT be domain-absorbed; tbox={t:?}"
        );
        assert!(t.role_rules.is_empty());
    }

    /// POSITIVE: `ObjectPropertyDomain(R, D)` becomes an **unguarded**
    /// `RoleRule` on `R⁻` — the tableau labels the neighbour across an
    /// `R⁻` edge, which is the R-*predecessor*, i.e. the constrained node.
    #[test]
    fn object_property_domain_becomes_inverse_role_rule() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["D"]);
        let d = atom(&mut o, "D");
        let r = Role::named(RoleId::new(0));
        o.axioms
            .push(Axiom::ObjectPropertyDomain { role: r, domain: d });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty(), "tbox={t:?}");
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(t.role_rules[0].role, Role::Inverse(RoleId::new(0)));
        assert_eq!(t.role_rules[0].guard, None);
        assert_eq!(t.role_rules[0].target_label, d);
        assert_eq!(t.unguarded_role_rules.len(), 1, "finalize() must re-run");
    }

    /// POSITIVE: `(≥1 R) ⊑ D` — the `ore_ont_3281` shape — is the same
    /// axiom and must absorb identically.
    #[test]
    fn min_one_antecedent_becomes_inverse_role_rule() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["D"]);
        let d = atom(&mut o, "D");
        let r = Role::named(RoleId::new(0));
        let top = o.concepts.top();
        let min1 = o.concepts.min(1, r, top);
        o.axioms.push(Axiom::SubClassOf { sub: min1, sup: d });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty(), "tbox={t:?}");
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(t.role_rules[0].role, Role::Inverse(RoleId::new(0)));
        assert_eq!(t.role_rules[0].target_label, d);
    }

    /// CONTROL: with the flag OFF (the default) the very same axiom stays a
    /// residual GCI. Pins that the feature is genuinely opt-in.
    #[test]
    fn flag_off_leaves_domain_axiom_as_residual() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("0");
        let mut o = fresh(&["D"]);
        let d = atom(&mut o, "D");
        let r = Role::named(RoleId::new(0));
        o.axioms
            .push(Axiom::ObjectPropertyDomain { role: r, domain: d });
        let t = run(&mut o);
        assert_eq!(t.residual_gcis.len(), 1, "flag OFF ⇒ unchanged; tbox={t:?}");
        assert!(t.role_rules.is_empty());
    }

    /// `∃R⁻.⊤ ⊑ D` — an inverse-role domain. `flip` sends `R⁻` to `R`, so
    /// the rule labels R-edge *targets*, which is exactly the set of nodes
    /// having an R-predecessor.
    #[test]
    fn inverse_role_domain_flips_to_named() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["D"]);
        let d = atom(&mut o, "D");
        let r_inv = Role::inverse(RoleId::new(0));
        o.axioms.push(Axiom::ObjectPropertyDomain {
            role: r_inv,
            domain: d,
        });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty(), "tbox={t:?}");
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(t.role_rules[0].role, Role::Named(RoleId::new(0)));
    }

    /// `⊤ ⊑ ≤0 R.⊤` (no `R`-edge in any model): the "rest" is empty and
    /// `ConceptPool::or([])` normalises to `⊥`, so any R-predecessor
    /// clashes. Guards the empty-`rest` corner.
    ///
    /// Written as a `Max` rather than `Domain(R, ⊥)` deliberately: the
    /// latter NNFs to a singleton `∀R.⊥`, which `absorb_roles`' pre-existing
    /// rewrite consumes first (see
    /// `domain_to_bot_is_consumed_by_the_prior_forall_rewrite`).
    #[test]
    fn empty_rest_yields_bot_target_label() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&[]);
        let r = Role::named(RoleId::new(0));
        let top = o.concepts.top();
        let max0 = o.concepts.max(0, r, top);
        o.axioms.push(Axiom::SubClassOf {
            sub: top,
            sup: max0,
        });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty(), "tbox={t:?}");
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(t.role_rules[0].role, Role::Inverse(RoleId::new(0)));
        assert_eq!(t.role_rules[0].target_label, o.concepts.bot());
    }

    /// PRIORITY (the other half): `Domain(R, ⊥)` NNFs to the singleton
    /// `∀R.⊥`, which the pre-existing rewrite in `absorb_roles` turns into a
    /// FORWARD rule labelling R-successors `⊥`. Domain absorption never sees
    /// it. Both encodings make an R-edge impossible, so this is a priority
    /// fact, not a semantic difference — pinned so a reordering is noticed.
    #[test]
    fn domain_to_bot_is_consumed_by_the_prior_forall_rewrite() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&[]);
        let r = Role::named(RoleId::new(0));
        let bot = o.concepts.bot();
        o.axioms.push(Axiom::ObjectPropertyDomain {
            role: r,
            domain: bot,
        });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty(), "tbox={t:?}");
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(t.role_rules[0].role, Role::Named(RoleId::new(0)));
        assert_eq!(t.role_rules[0].target_label, o.concepts.bot());
    }

    /// PRIORITY: `absorb_roles`' pre-existing singleton-`∀` rewrite still
    /// runs first, so `ObjectPropertyRange(R, A)` keeps producing the
    /// forward `RoleRule { role: R }` and is NOT re-read as a domain
    /// trigger. (`∀R.A` with `A ≠ ⊥` is not a domain trigger anyway; this
    /// pins the ordering so a future edit cannot silently flip it.)
    #[test]
    fn range_singleton_all_keeps_forward_role_rule_under_flag() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        o.axioms
            .push(Axiom::ObjectPropertyRange { role: r, range: a });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(
            t.role_rules[0].role,
            Role::Named(RoleId::new(0)),
            "range must stay a FORWARD role rule, not be flipped"
        );
        assert_eq!(t.role_rules[0].target_label, a);
    }

    /// A multi-disjunct residual keeps its remaining disjuncts: the rule's
    /// `target_label` is the `Or` of everything except the consumed one.
    #[test]
    fn multi_disjunct_residual_keeps_rest_as_target_label() {
        let _l = DOMAIN_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DomainGuard::set("1");
        // (≥1 R) ⊑ A ⊔ B  ⇒  ⊤ ⊑ ≤0 R.⊤ ⊔ A ⊔ B
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let r = Role::named(RoleId::new(0));
        let top = o.concepts.top();
        let min1 = o.concepts.min(1, r, top);
        let a_or_b = o.concepts.or([a, b]);
        o.axioms.push(Axiom::SubClassOf {
            sub: min1,
            sup: a_or_b,
        });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty(), "tbox={t:?}");
        assert_eq!(t.role_rules.len(), 1);
        let expected = o.concepts.or([a, b]);
        assert_eq!(t.role_rules[0].target_label, expected);
    }

    #[test]
    fn nominal_sub_class_yields_nominal_rule() {
        // {a} ⊑ B  → NominalRule(individual=a, conclusion=B).
        let mut o = fresh(&["B"]);
        let b = atom(&mut o, "B");
        let ind = o.vocabulary.intern_individual("a");
        let nom = o.concepts.nominal(ind);
        o.axioms.push(Axiom::SubClassOf { sub: nom, sup: b });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.nominal_rules.len(), 1);
        assert_eq!(t.nominal_rules[0].individual, ind);
        assert_eq!(t.nominal_rules[0].conclusion, b);
    }

    // ---- Nominal-existential absorption (issue #70) ----
    //
    // These are the ENTIRE safety net for `RUSTDL_NOMINAL_EXISTS_ABSORPTION`:
    // the curated corpus is INERT for this pass (it fires on 0 of the 7 curated
    // fixtures), so a green closure-diff there shows non-regression only.

    /// The issue-#70 shape. `∃r.{a} ⊑ B` must leave NO residual GCI — the
    /// residual is what made the completion graph grow without bound.
    #[test]
    fn nominal_exists_antecedent_absorbs_to_nominal_rule() {
        let mut o = fresh(&["B"]);
        let b = atom(&mut o, "B");
        let ind = o.vocabulary.intern_individual("a");
        let nom = o.concepts.nominal(ind);
        let r = Role::named(RoleId::new(0));
        let sub = o.concepts.some(r, nom);
        o.axioms.push(Axiom::SubClassOf { sub, sup: b });
        let t = run(&mut o);
        assert!(
            t.residual_gcis.is_empty(),
            "residual survived: {:?}",
            t.residual_gcis
        );
        assert_eq!(t.nominal_rules.len(), 1);
        assert_eq!(t.nominal_rules[0].individual, ind);
        // `{a} ⊑ ∀r⁻.B` — the flip is what points the propagation back at the
        // r-PREDECESSOR, i.e. the node the axiom constrains.
        let expected = o.concepts.all(r.flip(), b);
        assert_eq!(t.nominal_rules[0].conclusion, expected);
    }

    /// `∃r⁻.{a} ⊑ B` — an already-inverse role flips back to the named form.
    #[test]
    fn nominal_exists_inverse_role_flips_to_named() {
        let mut o = fresh(&["B"]);
        let b = atom(&mut o, "B");
        let ind = o.vocabulary.intern_individual("a");
        let nom = o.concepts.nominal(ind);
        let r_inv = Role::Inverse(RoleId::new(0));
        let sub = o.concepts.some(r_inv, nom);
        o.axioms.push(Axiom::SubClassOf { sub, sup: b });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty());
        assert_eq!(t.nominal_rules.len(), 1);
        let expected = o.concepts.all(Role::named(RoleId::new(0)), b);
        assert_eq!(t.nominal_rules[0].conclusion, expected);
    }

    /// PRECEDENCE: `∃r.{a} ⊑ ⊥` never reaches this pass. Its GCI body
    /// `∀r.¬{a} ⊔ ⊥` normalises to the singleton `∀r.¬{a}`, which the
    /// pre-existing bare-`All` residual rewrite in [`absorb_roles`] claims first
    /// as an unguarded `RoleRule`. That is the same axiom (`⊤ ⊑ ∀r.¬{a}`) in a
    /// different triggered form, so the outcome that matters — no residual GCI —
    /// holds either way. Pinned so a future reordering of `absorb_roles` cannot
    /// silently change which pass owns this shape.
    #[test]
    fn nominal_exists_bot_consequent_is_claimed_by_bare_all_rewrite() {
        let mut o = fresh(&[]);
        let bot = o.concepts.bot();
        let ind = o.vocabulary.intern_individual("a");
        let nom = o.concepts.nominal(ind);
        let r = Role::named(RoleId::new(0));
        let sub = o.concepts.some(r, nom);
        o.axioms.push(Axiom::SubClassOf { sub, sup: bot });
        let t = run(&mut o);
        assert!(t.residual_gcis.is_empty());
        assert!(t.nominal_rules.is_empty(), "claimed by the wrong pass");
        assert_eq!(t.role_rules.len(), 1);
        assert_eq!(t.role_rules[0].role, r);
        assert!(t.role_rules[0].guard.is_none());
        let nom_again = o.concepts.nominal(ind);
        let expected = o.concepts.not(nom_again);
        assert_eq!(t.role_rules[0].target_label, expected);
    }

    /// NEGATIVE CONTROL, and the one that carries the soundness argument:
    /// `≤k r.{a}` with `k ≥ 1` has antecedent `≥ k+1 r.{a}`, which needs more
    /// than one successor. The emitted rule fires off a SINGLE edge, so
    /// matching it would be strictly too strong — unsound.
    #[test]
    fn nominal_exists_rejects_max_cardinality_above_zero() {
        let mut o = fresh(&[]);
        let ind = o.vocabulary.intern_individual("a");
        let nom = o.concepts.nominal(ind);
        let r = Role::named(RoleId::new(0));
        let zero = o.concepts.max(0, r, nom);
        let one = o.concepts.max(1, r, nom);
        assert!(as_nominal_exists_trigger(zero, &o.concepts).is_some());
        assert!(as_nominal_exists_trigger(one, &o.concepts).is_none());
    }

    /// NEGATIVE CONTROL: `∀r.¬A` (a negated ATOM, not a nominal) is not this
    /// pass's shape — absorbing it would need a class-keyed guard the
    /// `NominalRule` form cannot express.
    #[test]
    fn nominal_exists_rejects_non_nominal_filler() {
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let not_a = o.concepts.not(a);
        let r = Role::named(RoleId::new(0));
        let all_not_a = o.concepts.all(r, not_a);
        assert!(as_nominal_exists_trigger(all_not_a, &o.concepts).is_none());
        // And `∀r.⊥` stays DOMAIN absorption's shape, not this one — the two
        // triggers must not overlap or the ordering in `absorb_roles` would be
        // a live tie-break rather than a stability guarantee.
        let bot = o.concepts.bot();
        let all_bot = o.concepts.all(r, bot);
        assert!(as_nominal_exists_trigger(all_bot, &o.concepts).is_none());
        assert!(as_domain_trigger(all_bot, &o.concepts).is_some());
        assert!(as_domain_trigger(all_not_a, &o.concepts).is_none());
    }

    /// A multi-member `ObjectOneOf` filler must NOT be absorbed: `∃r.({a} ⊔ {b})`
    /// NNFs its negation to `∀r.(¬{a} ⊓ ¬{b})`, whose inner is a conjunction,
    /// not a bare negated nominal. Absorbing per-member would be unsound.
    #[test]
    fn nominal_exists_rejects_multi_member_one_of() {
        let mut o = fresh(&[]);
        let ia = o.vocabulary.intern_individual("a");
        let ib = o.vocabulary.intern_individual("b");
        let na = o.concepts.nominal(ia);
        let nb = o.concepts.nominal(ib);
        let one_of = o.concepts.or([na, nb]);
        let r = Role::named(RoleId::new(0));
        let negated = o.concepts.not(one_of);
        let all_negated = o.concepts.all(r, negated);
        // Pre-NNF the inner `Not(Or(..))` is not a `Not(Nominal)`, and post-NNF
        // it becomes a conjunction — neither matches.
        assert!(as_nominal_exists_trigger(all_negated, &o.concepts).is_none());
    }

    #[test]
    fn unrelated_axioms_pass_through_without_contribution() {
        // Role characteristics and ABox don't show up in AbsorbedTBox.
        let mut o = fresh(&["A"]);
        let _ = atom(&mut o, "A");
        let r = Role::named(RoleId::new(0));
        let _ = Vocabulary::new(); // placate clippy::dead_code
        o.axioms.push(Axiom::TransitiveRole(r));
        let i = o.vocabulary.intern_individual("a");
        let a = o.concepts.atomic(cid(&o, "A"));
        o.axioms.push(Axiom::ClassAssertion {
            class: a,
            individual: i,
        });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert!(t.residual_gcis.is_empty());
    }

    #[test]
    fn sub_class_of_top_to_atom_is_residual() {
        // ⊤ ⊑ A — no Not(Atomic) anywhere; just `A` is the residual GCI.
        let mut o = fresh(&["A"]);
        let a = atom(&mut o, "A");
        let top = o.concepts.top();
        o.axioms.push(Axiom::SubClassOf { sub: top, sup: a });
        let t = run(&mut o);
        assert!(t.concept_rules.is_empty());
        assert_eq!(t.residual_gcis.len(), 1);
        assert_eq!(t.residual_gcis[0], a);
    }

    #[test]
    fn finalize_indexes_concept_rules_by_trigger() {
        // A ⊑ B, A ⊑ C, D ⊑ E — two rules trigger on A, one on D.
        let mut o = fresh(&["A", "B", "C", "D", "E"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let d = atom(&mut o, "D");
        let e = atom(&mut o, "E");
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: cc });
        o.axioms.push(Axiom::SubClassOf { sub: d, sup: e });
        let t = run(&mut o);
        assert_eq!(t.concept_rules.len(), 3);
        // Index reachable from each trigger.
        let a_id = cid(&o, "A");
        let d_id = cid(&o, "D");
        let from_a = t.concept_rules_by_trigger.get(&a_id).expect("A indexed");
        assert_eq!(from_a.len(), 2);
        assert!(from_a.contains(&b));
        assert!(from_a.contains(&cc));
        let from_d = t.concept_rules_by_trigger.get(&d_id).expect("D indexed");
        assert_eq!(from_d, &vec![e]);
        // Triggers not present in the index ⇒ not in any rule.
        assert!(!t.concept_rules_by_trigger.contains_key(&cid(&o, "B")));
    }

    #[test]
    fn finalize_indexes_nominal_rules_by_individual() {
        // {a} ⊑ B, {a} ⊑ C, {b} ⊑ D — two rules trigger on a, one on b.
        let mut o = fresh(&["B", "C", "D"]);
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let d = atom(&mut o, "D");
        let ind_a = o.vocabulary.intern_individual("a");
        let ind_b = o.vocabulary.intern_individual("b");
        let nom_a = o.concepts.nominal(ind_a);
        let nom_b = o.concepts.nominal(ind_b);
        o.axioms.push(Axiom::SubClassOf { sub: nom_a, sup: b });
        o.axioms.push(Axiom::SubClassOf {
            sub: nom_a,
            sup: cc,
        });
        o.axioms.push(Axiom::SubClassOf { sub: nom_b, sup: d });
        let t = run(&mut o);
        assert_eq!(t.nominal_rules.len(), 3);
        let from_a = t
            .nominal_rules_by_individual
            .get(&ind_a)
            .expect("a indexed");
        assert_eq!(from_a.len(), 2);
        assert!(from_a.contains(&b));
        assert!(from_a.contains(&cc));
        assert_eq!(t.nominal_rules_by_individual.get(&ind_b), Some(&vec![d]));
    }

    #[test]
    fn finalize_partitions_role_rules_by_guard() {
        // A ⊑ ∀r.B (guarded) plus Range(r, C) which lowers to ⊤ ⊑ ∀r.C
        // (unguarded). Partition must split them correctly.
        let mut o = fresh(&["A", "B", "C"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        let cc = atom(&mut o, "C");
        let r = Role::named(RoleId::new(0));
        let all_r_b = o.concepts.all(r, b);
        o.axioms.push(Axiom::SubClassOf {
            sub: a,
            sup: all_r_b,
        });
        o.axioms
            .push(Axiom::ObjectPropertyRange { role: r, range: cc });
        let t = run(&mut o);
        assert_eq!(t.role_rules.len(), 2);
        assert_eq!(t.unguarded_role_rules.len(), 1);
        assert_eq!(t.unguarded_role_rules[0].target_label, cc);
        let a_id = cid(&o, "A");
        let guarded = t
            .guarded_role_rules_by_guard
            .get(&a_id)
            .expect("guarded on A");
        assert_eq!(guarded.len(), 1);
        assert_eq!(guarded[0].target_label, b);
    }

    #[test]
    fn finalize_is_idempotent() {
        let mut o = fresh(&["A", "B"]);
        let a = atom(&mut o, "A");
        let b = atom(&mut o, "B");
        o.axioms.push(Axiom::SubClassOf { sub: a, sup: b });
        let mut t = run(&mut o);
        let before = t.clone();
        t.finalize();
        t.finalize();
        assert_eq!(t, before);
    }
}
