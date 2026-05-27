//! Hybrid saturation+tableau OWL DL reasoner — the public API surface.
//!
//! End-users depend on this crate. Internally it orchestrates
//! `owl-dl-core` (IR, preprocessing), `owl-dl-saturation` (EL
//! fragment), `owl-dl-tableau` (SROIQ), and `owl-dl-datatypes`
//! (concrete domains).
//!
//! ## Public API
//!
//! - [`is_class_satisfiable`] — concept satisfiability.
//! - [`is_consistent`] — does the KB have any model.
//! - [`is_subclass_of`] — KB ⊨ sub ⊑ super (via the standard
//!   `sub ⊓ ¬sup` reduction).
//! - [`is_instance_of`] / [`instances_of`] — entailed class
//!   memberships of declared individuals.
//! - [`classify`] — full atomic-class hierarchy with equivalences,
//!   direct super-classes, and the unsat-class set. Returns
//!   [`ClassificationStats`] tracking how many queries each engine
//!   handled.
//! - [`realize`] — per-individual entailed types + Hasse leaves.
//!
//! ## Orchestrator
//!
//! Every entry point that issues at least one tableau query first
//! runs the EL saturation engine (sound but only complete for the
//! supported EL fragment) and short-circuits on a hit. When the
//! entire ontology lives inside that fragment, [`classify_internal`]
//! takes a saturation-only fast path with zero tableau calls
//! (`stats.pure_el_mode == true`).
//!
//! `PreparedOntology::from_internal` snapshots the post-expand /
//! NNF / absorb / `ABox`-seed state once so the pairwise
//! classification loop reuses it across queries instead of
//! re-running the pipeline per pair. The pairwise loop runs in
//! parallel via rayon.
//!
//! ## DL fragment coverage
//!
//! The tableau side handles `SROIQ` (Phase 5 complete except full
//! role-chain automata — length-2 chains + `TransitiveRole` only).
//! Datatypes are scaffolded but not wired into reasoning yet.

mod classify;
mod model_cache;
mod realize;

pub use classify::{
    Classification, ClassificationStats, classify, classify_internal, classify_n2,
    classify_n2_with_timeout, classify_saturation_only, classify_top_down,
    classify_top_down_with_timeout, classify_with_timeout,
};
pub use realize::{
    Realization, instances_of, instances_of_internal, instances_of_saturation_only,
    instances_of_saturation_only_internal, is_instance_of, is_instance_of_internal,
    is_instance_of_saturation_only, is_instance_of_saturation_only_internal, realize,
    realize_internal, realize_saturation_only, realize_saturation_only_internal,
};

/// Compute a sparse summary of the signature-locality partition
/// (see [`docs/module-extraction-plan.md`]). Counts and the
/// largest-component-size are the diagnostics most useful for
/// deciding whether the partition will actually skip pair-queries
/// — if one component dominates, the filter has nothing to do.
#[derive(Debug, Clone, Copy)]
pub struct LocalityStats {
    pub num_classes: usize,
    pub num_components: usize,
    pub largest_component: usize,
    pub singleton_components: usize,
}

/// Sparse summary of the absorbed `TBox` shape — how many rules
/// of each kind survive absorption, and how the residual GCIs
/// break down by top-level `ConceptExpr` variant. Used by the
/// `rustdl tbox-stats` CLI to inform the lazy-unfolding plan; see
/// `docs/lazy-unfolding-plan.md`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TBoxStats {
    pub concept_rules: usize,
    pub nominal_rules: usize,
    pub role_rules_guarded: usize,
    pub role_rules_unguarded: usize,
    pub residual_gcis: usize,
    /// Residual GCIs whose body is a top-level `Or(_)` — these
    /// are the universal disjunctions that drive the pizza
    /// search-tree explosion (one Or per residual × one
    /// branching decision per node).
    pub residual_or_count: usize,
    /// Residual GCIs whose body is `Atomic(_)` — pure
    /// "everything is a C" assertions; cheap because they don't
    /// branch.
    pub residual_atomic_count: usize,
    /// Residual GCIs of other shapes (`And`, `Some`, `Min`,
    /// `Max`, `Not`, `SelfRestriction`, `Nominal`) — buckets
    /// kept summed because each is rarer.
    pub residual_other_count: usize,
    /// Concept rules `A ⊑ ψ` whose conclusion `ψ` is `Or(_)`.
    /// These are the per-trigger disjunctions; on pizza they're
    /// the dominant branching source (the residual count is only
    /// 4). Candidates for the Lever-A-extension lazy unfolding.
    pub concept_rule_or_count: usize,
}

/// Clausify the ontology into DL-clauses and return the shape
/// histogram (hypertableau Phase H0 — see
/// `docs/hypertableau-scoping.md`). Produces no reasoning; the
/// stats measure clause-shape distribution and clausifier
/// coverage (`deferred`) across the corpus.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn clause_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<owl_dl_core::clause::ClauseStats, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let (_clauses, stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    Ok(stats)
}

/// Per-category census of what the clausifier still defers — the HF1
/// coverage target list (see `docs/hypertableau-full-scoping.md`).
///
/// # Errors
///
/// See [`ReasonError`].
pub fn clause_deferred_census<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<Vec<(&'static str, usize)>, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    Ok(owl_dl_core::clause::deferred_census(&internal))
}

pub use owl_dl_tableau::hyper::{HyperResult, SearchStats};

/// Per-class concept-satisfiability result from the hypertableau
/// engine ([`owl_dl_tableau::hyper`]), for the H2b wall measurement.
#[derive(Debug, Clone)]
pub struct HyperSatClassResult {
    /// The named class tested as the root concept.
    pub iri: String,
    /// `decide`'s verdict over the **clausifiable fragment**.
    pub result: owl_dl_tableau::hyper::HyperResult,
    /// Wall time for this class (milliseconds).
    pub wall_ms: f64,
    /// Search instrumentation (branches taken, restores, depth).
    pub stats: owl_dl_tableau::hyper::SearchStats,
}

/// Summary of a [`hyper_sat_probe`] run.
#[derive(Debug, Clone)]
pub struct HyperSatProbe {
    /// Per-class results, in vocabulary order.
    pub results: Vec<HyperSatClassResult>,
    /// Clause-set shape (so the deferred count is visible alongside).
    pub clause_stats: owl_dl_core::clause::ClauseStats,
}

/// Run the hypertableau engine's concept-satisfiability decision
/// ([`owl_dl_tableau::hyper::HyperEngine::decide`]) once per named
/// class, timing each, for the H2b wall measurement (see
/// `docs/hypertableau-scoping.md`).
///
/// **This is a performance probe, not a correctness claim.** The
/// H1c clausifier defers cardinality/nominals, so the clause set is
/// an under-approximation of the ontology. Dropping axioms only
/// *removes* constraints, hence `Models(full) ⊆ Models(fragment)`:
/// a `Unsat` verdict is sound for the full ontology, but a `Sat`
/// verdict is **not** (the full ontology may still be unsatisfiable
/// via a dropped axiom). Use this to ask "does `decide` terminate
/// quickly with branching exercised", not "is class C satisfiable".
///
/// `max_depth` bounds branching recursion; `per_class_timeout` (if
/// set) is the wall budget per class, after which the result is
/// `Stalled`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn hyper_sat_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    max_depth: usize,
    per_class_timeout: Option<std::time::Duration>,
) -> Result<HyperSatProbe, ReasonError> {
    use owl_dl_tableau::hyper::HyperEngine;
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let (clauses, clause_stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    let mut results = Vec::with_capacity(internal.vocabulary.num_classes());
    for (class_id, iri) in internal.vocabulary.classes() {
        let mut engine = HyperEngine::new(&clauses, class_id);
        let deadline = per_class_timeout.map(|t| std::time::Instant::now() + t);
        let start = std::time::Instant::now();
        let result = engine.decide_with_deadline(max_depth, deadline);
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        results.push(HyperSatClassResult {
            iri: iri.to_string(),
            result,
            wall_ms,
            stats: engine.stats(),
        });
    }
    Ok(HyperSatProbe {
        results,
        clause_stats,
    })
}

/// Smallest `ClassId` strictly greater than every class index that
/// appears in `clauses` — a fresh id usable for the subsumption
/// probe's helper concept `Q`.
fn fresh_class_id(clauses: &[owl_dl_core::clause::DlClause]) -> owl_dl_core::ir::ClassId {
    use owl_dl_core::clause::Atom;
    let mut max = 0u32;
    for cl in clauses {
        for atom in cl.body.iter().chain(cl.head.iter()) {
            if let Atom::Class(c, _) | Atom::Exists(_, c, _) = atom {
                max = max.max(c.index() + 1);
            }
        }
    }
    owl_dl_core::ir::ClassId::new(max)
}

/// Get-or-allocate the complement class `Ā` for atomic `a`, emitting
/// the clash clause `A(x) ∧ Ā(x) → ⊥` to `clauses` on first use. The
/// complement is a positive label the engine treats normally; the
/// clash clause is what makes asserting `Ā` refute a derived `A`.
/// Sound for *refutation only* (we assert `Ā`, never derive it from
/// the absence of `A`). See `docs/hypertableau-h3b-scoping.md` §2.
fn complement_of(
    a: owl_dl_core::ir::ClassId,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> owl_dl_core::ir::ClassId {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    if let Some(&c) = complements.get(&a) {
        return c;
    }
    let c = ClassId::new(*next_fresh);
    *next_fresh += 1;
    complements.insert(a, c);
    clauses.push(DlClause {
        body: vec![Atom::Class(a, X), Atom::Class(c, X)],
        head: vec![],
    });
    c
}

/// Translate one disjunct of `NNF(¬sup-definition)` into a head atom,
/// or `None` if it falls outside the supported set (caller falls back
/// to the bare-complement test). Supported: `atomic` → `Class(A)`,
/// `¬atomic` → `Class(Ā)`, `∃R.atomic` → `Exists(R,A)`, `∃R.¬atomic`
/// → `Exists(R,Ā)`. See `docs/hypertableau-h3b-scoping.md` §3.
fn encode_neg_disjunct(
    d: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ConceptPool,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> Option<owl_dl_core::clause::Atom> {
    use owl_dl_core::ConceptExpr;
    use owl_dl_core::clause::{Atom, X};
    match pool.get(d) {
        ConceptExpr::Atomic(a) => Some(Atom::Class(*a, X)),
        ConceptExpr::Not(inner) => match pool.get(*inner) {
            ConceptExpr::Atomic(a) => Some(Atom::Class(
                complement_of(*a, complements, clauses, next_fresh),
                X,
            )),
            _ => None,
        },
        ConceptExpr::Some(role, inner) => match pool.get(*inner) {
            ConceptExpr::Atomic(a) => Some(Atom::Exists(*role, *a, X)),
            ConceptExpr::Not(i2) => match pool.get(*i2) {
                ConceptExpr::Atomic(a) => Some(Atom::Exists(
                    *role,
                    complement_of(*a, complements, clauses, next_fresh),
                    X,
                )),
                _ => None,
            },
            // `∃R.(L1 ⊓ … ⊓ Lk)` with `Li` literals (atomic / ¬atomic):
            // name the conjunction with a fresh `N ⊑ ⊓Li` and assert
            // `∃R.N`. The `VegetarianPizzaEquivalent2` shape
            // `∃hT.(¬Cheese ⊓ … ⊓ ¬Veg)`. `N` is a sound under-name
            // (anything `N` satisfies every literal), fresh, so it
            // never affects other reasoning — refutation stays sound.
            ConceptExpr::And(parts) => {
                let parts: Vec<owl_dl_core::ir::ConceptId> = parts.to_vec();
                let lits = name_literal_conjunction(&parts, pool, clauses, next_fresh)?;
                Some(Atom::Exists(*role, lits, X))
            }
            _ => None,
        },
        // `≤n R.C` (NNF of `¬(≥(n+1) R.C)`) → an at-most constraint.
        // Unqualified when the qualifier is `⊤` (the pizza
        // `InterestingPizza` shape `≤2 hasTopping`); a named-class
        // qualifier is carried through, anything else defers.
        ConceptExpr::Max(n, role, inner) => match pool.get(*inner) {
            ConceptExpr::Top => Some(Atom::AtMost(*role, None, *n, X)),
            ConceptExpr::Atomic(a) => Some(Atom::AtMost(*role, Some(*a), *n, X)),
            _ => None,
        },
        _ => None,
    }
}

/// Allocate a fresh class `N` with `N ⊑ ⊓parts` where every part is a
/// literal (`atomic` → `N → A`, `¬atomic` → `N ∧ A → ⊥`). Returns
/// `None` (no clauses emitted) if any part is non-literal. Used to
/// name the inner concept of an `∃R.(…)` disjunct (§3 extension).
fn name_literal_conjunction(
    parts: &[owl_dl_core::ir::ConceptId],
    pool: &owl_dl_core::ConceptPool,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> Option<owl_dl_core::ir::ClassId> {
    use owl_dl_core::ConceptExpr;
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    // Reject early if any part is non-literal — emit nothing.
    let mut lits: Vec<(ClassId, bool)> = Vec::with_capacity(parts.len());
    for &p in parts {
        match pool.get(p) {
            ConceptExpr::Atomic(a) => lits.push((*a, true)),
            ConceptExpr::Not(inner) => match pool.get(*inner) {
                ConceptExpr::Atomic(a) => lits.push((*a, false)),
                _ => return None,
            },
            _ => return None,
        }
    }
    let n = ClassId::new(*next_fresh);
    *next_fresh += 1;
    for (a, positive) in lits {
        if positive {
            // N(x) → A(x)
            clauses.push(DlClause {
                body: vec![Atom::Class(n, X)],
                head: vec![Atom::Class(a, X)],
            });
        } else {
            // N(x) ∧ A(x) → ⊥  (N implies ¬A)
            clauses.push(DlClause {
                body: vec![Atom::Class(n, X), Atom::Class(a, X)],
                head: vec![],
            });
        }
    }
    Some(n)
}

/// Encode `NNF(¬def)` as the Q-gated disjunctive head atoms for the
/// H3b subsumption test, or `None` if any top-level disjunct is
/// untranslatable (caller falls back). The disjunction's atoms are
/// later emitted as `Q(x) → d1 ∨ … ∨ dk` — gated on `Q` so the
/// constraint binds only the root (never generated successors).
fn encode_neg_definition(
    neg: owl_dl_core::ir::ConceptId,
    pool: &owl_dl_core::ConceptPool,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> Option<Vec<owl_dl_core::clause::Atom>> {
    use owl_dl_core::ConceptExpr;
    let disjuncts: Vec<owl_dl_core::ir::ConceptId> = match pool.get(neg) {
        ConceptExpr::Or(parts) => parts.to_vec(),
        _ => vec![neg],
    };
    let mut out = Vec::with_capacity(disjuncts.len());
    for d in disjuncts {
        out.push(encode_neg_disjunct(
            d,
            pool,
            complements,
            clauses,
            next_fresh,
        )?);
    }
    Some(out)
}

/// One subsumption-pair result from [`hyper_subsumption_probe`].
#[derive(Debug, Clone)]
pub struct HyperSubResult {
    /// Sub-class IRI.
    pub sub: String,
    /// Super-class IRI.
    pub sup: String,
    /// `Unsat` ⇒ `sub ⊑ sup` (sound for the full ontology); `Sat` ⇒
    /// not entailed *over the fragment* (NOT sound for the full
    /// ontology); `Stalled` ⇒ budget exhausted.
    pub result: HyperResult,
    /// Wall time for this pair (milliseconds).
    pub wall_ms: f64,
    /// Search instrumentation.
    pub stats: SearchStats,
}

/// Summary of a [`hyper_subsumption_probe`] run.
#[derive(Debug, Clone)]
pub struct HyperSubProbe {
    /// Only the *interesting* pairs are retained (those that branched
    /// or whose verdict was `Unsat`/`Stalled`) to bound output; the
    /// counters below summarise the full N² sweep.
    pub results: Vec<HyperSubResult>,
    /// Total ordered pairs tested (`n·(n−1)`).
    pub pairs_tested: u64,
    /// Pairs decided `Unsat` (i.e. entailed subsumptions found).
    pub subsumptions: u64,
    /// Pairs whose decision exercised branching (`branches_taken>0`).
    pub pairs_branched: u64,
    /// Pairs that hit the budget (`Stalled`).
    pub stalled: u64,
    /// Deepest branch nesting across all pairs.
    pub max_branch_depth: u32,
    /// Total wall across all pairs (milliseconds).
    pub total_wall_ms: f64,
    /// Pairs whose `sup` used the H3b `¬sup`-expansion encoding
    /// (`sup` had a translatable definition). The rest used the bare
    /// `Q ∧ sup → ⊥` fallback.
    pub pairs_via_expansion: u64,
    /// Complement classes introduced for negative literals (§2).
    pub complements_introduced: usize,
    /// Clause-set shape (deferred count visible alongside).
    pub clause_stats: owl_dl_core::clause::ClauseStats,
}

/// Run the hypertableau subsumption test ([`decide_subsumption`])
/// over **every ordered pair** of named classes, for the H2c pizza
/// wall measurement (see `docs/hypertableau-scoping.md`). This is the
/// analog of `classify`'s pair loop, but routed through the
/// hyperresolution engine.
///
/// **Performance probe, not a complete classifier.** As with
/// [`hyper_sat_probe`], deferred axioms make the clause set an
/// under-approximation: an `Unsat` (subsumption-holds) verdict is
/// sound for the full ontology, but `Sat` (not-subsumed) is not. So
/// the reported `subsumptions` count is a sound *lower bound* on the
/// true hierarchy.
///
/// `per_pair_timeout`, if set, bounds each pair's wall.
///
/// Pre-pass for [`hyper_subsumption_probe`]: for each defined `sup`,
/// expand `NNF(¬def)` into Q-gated disjunct atoms, appending any
/// complement/structural clash clauses to `clauses` (once). Returns
/// the per-`sup` disjunct atoms for the sups whose `¬def` fully
/// translated; the rest fall back to the bare-complement test.
fn build_sup_neg_map(
    vocab: &[(owl_dl_core::ir::ClassId, String)],
    defs: &owl_dl_core::definitions::Definitions,
    pool: &mut owl_dl_core::ConceptPool,
    complements: &mut std::collections::HashMap<owl_dl_core::ir::ClassId, owl_dl_core::ir::ClassId>,
    clauses: &mut Vec<owl_dl_core::clause::DlClause>,
    next_fresh: &mut u32,
) -> std::collections::HashMap<owl_dl_core::ir::ClassId, Vec<owl_dl_core::clause::Atom>> {
    let mut sup_neg = std::collections::HashMap::new();
    for (sup, _) in vocab {
        let Some(def) = defs.body_of(*sup) else {
            continue;
        };
        let neg = owl_dl_core::normalize::nnf_complement(def, pool);
        if let Some(atoms) = encode_neg_definition(neg, pool, complements, clauses, next_fresh) {
            sup_neg.insert(*sup, atoms);
        }
    }
    sup_neg
}

/// # Errors
///
/// See [`ReasonError`].
pub fn hyper_subsumption_probe<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
    max_depth: usize,
    per_pair_timeout: Option<std::time::Duration>,
) -> Result<HyperSubProbe, ReasonError> {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;
    use owl_dl_tableau::hyper::HyperEngine;

    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let (base, clause_stats) = owl_dl_core::clause::clausify_with_stats(&internal);
    let defs = owl_dl_core::definitions::extract_definitions(&internal);

    // Fresh id space: past every class used in clauses and the
    // vocabulary. `q` first, then complement classes.
    let num_classes = u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
    let mut next_fresh = fresh_class_id(&base).index().max(num_classes);
    let q = ClassId::new(next_fresh);
    next_fresh += 1;

    let vocab: Vec<(ClassId, String)> = internal
        .vocabulary
        .classes()
        .map(|(id, iri)| (id, iri.to_string()))
        .collect();

    let mut clauses = base;
    let mut complements: std::collections::HashMap<ClassId, ClassId> =
        std::collections::HashMap::new();

    // Pre-pass: for each defined `sup`, expand `NNF(¬def)` into Q-gated
    // disjunct atoms (§1/§3). Complement clash clauses (§2) are
    // appended to `clauses` here, once — so the engine's clause set is
    // monotonic across the pair loop below.
    let sup_neg = build_sup_neg_map(
        &vocab,
        &defs,
        &mut internal.concepts,
        &mut complements,
        &mut clauses,
        &mut next_fresh,
    );
    // base clauses + complement clash clauses — fixed for every pair.
    let fixed_len = clauses.len();

    let mut probe = HyperSubProbe {
        results: Vec::new(),
        pairs_tested: 0,
        subsumptions: 0,
        pairs_branched: 0,
        stalled: 0,
        max_branch_depth: 0,
        total_wall_ms: 0.0,
        pairs_via_expansion: 0,
        complements_introduced: complements.len(),
        clause_stats,
    };
    for (sub, sub_iri) in &vocab {
        for (sup, sup_iri) in &vocab {
            if sub == sup {
                continue;
            }
            clauses.truncate(fixed_len);
            clauses.push(DlClause {
                body: vec![Atom::Class(q, X)],
                head: vec![Atom::Class(*sub, X)],
            });
            // H3b: if `sup` has a translatable definition, assert the
            // Q-gated `¬sup` disjunction; else fall back to the bare
            // `Q ∧ sup → ⊥` complement test (H2c behaviour).
            let via_expansion = if let Some(atoms) = sup_neg.get(sup) {
                clauses.push(DlClause {
                    body: vec![Atom::Class(q, X)],
                    head: atoms.clone(),
                });
                true
            } else {
                clauses.push(DlClause {
                    body: vec![Atom::Class(q, X), Atom::Class(*sup, X)],
                    head: vec![],
                });
                false
            };

            let deadline = per_pair_timeout.map(|t| std::time::Instant::now() + t);
            let start = std::time::Instant::now();
            let mut engine = HyperEngine::new(&clauses, q);
            let result = engine.decide_with_deadline(max_depth, deadline);
            let stats = engine.stats();
            let wall_ms = start.elapsed().as_secs_f64() * 1000.0;

            probe.pairs_tested += 1;
            probe.total_wall_ms += wall_ms;
            probe.max_branch_depth = probe.max_branch_depth.max(stats.max_branch_depth);
            if via_expansion {
                probe.pairs_via_expansion += 1;
            }
            if result == HyperResult::Unsat {
                probe.subsumptions += 1;
            }
            if result == HyperResult::Stalled {
                probe.stalled += 1;
            }
            if stats.branches_taken > 0 {
                probe.pairs_branched += 1;
            }
            // Retain only interesting pairs to bound memory/output.
            if stats.branches_taken > 0 || result != HyperResult::Sat {
                probe.results.push(HyperSubResult {
                    sub: sub_iri.clone(),
                    sup: sup_iri.clone(),
                    result,
                    wall_ms,
                    stats,
                });
            }
        }
    }
    Ok(probe)
}

/// Branching-recursion depth cap for the H4 in-orchestrator hyper
/// subsumption check (the per-pair wall budget bounds it further).
const HYPER_WEDGE_DEPTH: usize = 256;

/// Whether the hypertableau sound-accelerator wedge (H4) is enabled.
/// Gated by the `RUSTDL_HYPERTABLEAU` env var (default off) for a
/// release of soak time before the default flips — see
/// `docs/hypertableau-h4-scoping.md` §3.
#[must_use]
pub fn hyper_wedge_enabled() -> bool {
    std::env::var_os("RUSTDL_HYPERTABLEAU").is_some_and(|v| v != "0" && !v.is_empty())
}

/// Cached clausified state for the H4 sound accelerator: built once
/// per ontology (the expensive clausify + `¬sup` pre-pass), then
/// reused across every subsumption pair. [`proves`](Self::proves)
/// answers `sub ⊑ sup` soundly via the hyper engine — `true` only on
/// a `Unsat` verdict, which is sound for *any* ontology (see
/// `docs/hypertableau-h4-scoping.md` §0). A `false` means "hyper
/// can't prove it" and the caller must fall back to the tableau.
pub(crate) struct HyperCache {
    /// Base clauses + complement clash clauses (the per-pair Q-clauses
    /// are appended to a clone in `proves`).
    clauses: Vec<owl_dl_core::clause::DlClause>,
    /// Per-defined-`sup` `NNF(¬def)` disjunct atoms (Q-gated).
    sup_neg: std::collections::HashMap<owl_dl_core::ir::ClassId, Vec<owl_dl_core::clause::Atom>>,
    /// Fresh helper concept `q` for the `sub ⊓ ¬sup` injection.
    fresh_q: owl_dl_core::ir::ClassId,
}

impl HyperCache {
    /// Clausify `internal` and pre-compute the `¬sup` expansions once.
    pub(crate) fn build(internal: &InternalOntology) -> Self {
        use owl_dl_core::ir::ClassId;
        let mut internal = internal.clone();
        let (base, _stats) = owl_dl_core::clause::clausify_with_stats(&internal);
        let defs = owl_dl_core::definitions::extract_definitions(&internal);
        let num_classes = u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX);
        let mut next_fresh = fresh_class_id(&base).index().max(num_classes);
        let fresh_q = ClassId::new(next_fresh);
        next_fresh += 1;
        let vocab: Vec<(ClassId, String)> = internal
            .vocabulary
            .classes()
            .map(|(id, iri)| (id, iri.to_string()))
            .collect();
        let mut clauses = base;
        let mut complements: std::collections::HashMap<ClassId, ClassId> =
            std::collections::HashMap::new();
        let sup_neg = build_sup_neg_map(
            &vocab,
            &defs,
            &mut internal.concepts,
            &mut complements,
            &mut clauses,
            &mut next_fresh,
        );
        Self {
            clauses,
            sup_neg,
            fresh_q,
        }
    }

    /// Sound subsumption test: `true` iff the hyper engine proves
    /// `sub ⊑ sup` (`Unsat` on `sub ⊓ ¬sup`). `false` = "couldn't
    /// prove" (`Sat`/`Stalled`/deadline) — the caller falls back.
    pub(crate) fn proves(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> bool {
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
        let mut clauses = self.clauses.clone();
        clauses.push(DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(sub, X)],
        });
        if let Some(atoms) = self.sup_neg.get(&sup) {
            clauses.push(DlClause {
                body: vec![Atom::Class(self.fresh_q, X)],
                head: atoms.clone(),
            });
        } else {
            clauses.push(DlClause {
                body: vec![Atom::Class(self.fresh_q, X), Atom::Class(sup, X)],
                head: vec![],
            });
        }
        let mut engine = HyperEngine::new(&clauses, self.fresh_q);
        engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) == HyperResult::Unsat
    }
}

/// Build the absorbed `TBox` and classify every residual GCI's
/// trigger per [`owl_dl_core::residual_trigger`]. The result is
/// the histogram needed to decide whether the lazy-unfolding
/// Phase-2 integration will move walls — see
/// `docs/lazy-unfolding-plan.md`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn residual_trigger_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<owl_dl_core::residual_trigger::ResidualTriggerStats, ReasonError> {
    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let normalized = owl_dl_core::normalize::nnf_axioms(&mut internal);
    let tbox = owl_dl_core::absorb::absorb(&normalized, &mut internal.concepts);
    let (_triggers, stats) =
        owl_dl_core::residual_trigger::classify_residuals(&tbox.residual_gcis, &internal.concepts);
    Ok(stats)
}

/// Build the absorbed `TBox` for `ontology` and summarise its
/// shape.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn tbox_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<TBoxStats, ReasonError> {
    use owl_dl_core::ConceptExpr;
    let mut internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let normalized = owl_dl_core::normalize::nnf_axioms(&mut internal);
    let tbox = owl_dl_core::absorb::absorb(&normalized, &mut internal.concepts);
    let mut stats = TBoxStats {
        concept_rules: tbox.concept_rules.len(),
        nominal_rules: tbox.nominal_rules.len(),
        role_rules_guarded: tbox
            .guarded_role_rules_by_guard
            .values()
            .map(Vec::len)
            .sum(),
        role_rules_unguarded: tbox.unguarded_role_rules.len(),
        residual_gcis: tbox.residual_gcis.len(),
        ..TBoxStats::default()
    };
    for &gci in &tbox.residual_gcis {
        match internal.concepts.get(gci) {
            ConceptExpr::Or(_) => stats.residual_or_count += 1,
            ConceptExpr::Atomic(_) => stats.residual_atomic_count += 1,
            _ => stats.residual_other_count += 1,
        }
    }
    for rule in &tbox.concept_rules {
        if matches!(internal.concepts.get(rule.conclusion), ConceptExpr::Or(_)) {
            stats.concept_rule_or_count += 1;
        }
    }
    Ok(stats)
}

/// Build the locality partition for `ontology` and summarise it.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn locality_stats<A: horned_owl::model::ForIRI>(
    ontology: &horned_owl::ontology::set::SetOntology<A>,
) -> Result<LocalityStats, ReasonError> {
    let internal = owl_dl_core::convert::convert_ontology(ontology)?;
    let n_classes = internal.vocabulary.num_classes();
    let partition = owl_dl_core::locality::LocalityPartition::build(
        &internal.axioms,
        &internal.concepts,
        n_classes,
    );
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for i in 0..n_classes {
        let cid = owl_dl_core::ClassId::new(u32::try_from(i).expect("class count fits in u32"));
        *sizes.entry(partition.component(cid)).or_insert(0) += 1;
    }
    let num_components = partition.num_components();
    let largest_component = sizes.values().copied().max().unwrap_or(0);
    let singleton_components = sizes.values().filter(|&&s| s == 1).count();
    Ok(LocalityStats {
        num_classes: n_classes,
        num_components,
        largest_component,
        singleton_components,
    })
}

use std::collections::HashMap;

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use thiserror::Error;

use owl_dl_core::convert::{ConversionError, convert_ontology};
use owl_dl_core::{
    AbsorbedTBox, Axiom, ConceptExpr, ConceptId, ConceptPool, IndividualId, InternalOntology, Role,
    RoleHierarchy, RoleHierarchyBuilder, RoleId, SubRolePath, absorb, nnf_axioms, nnf_complement,
};
use owl_dl_tableau::{NodeId, TableauContext};

/// Recursion depth cap for the search driver — generous and
/// defensive. Real ALCHIQ inputs terminate via pair blocking long
/// before this matters.
const MAX_SEARCH_DEPTH: usize = 256;

/// Per-query instrumentation: did the EL closure alone answer this
/// query, or did the tableau have to run? Returned alongside the
/// boolean verdict by the `_with_stats` variants of the public
/// reasoning entry points.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryStats {
    /// `true` iff the EL saturation closure was sufficient to
    /// produce the verdict — no tableau call was made.
    pub answered_by_saturation: bool,
    /// `true` iff this run took the pure-EL fast path (the closure
    /// is also complete for the input, so a closure miss is itself
    /// the verdict).
    pub pure_el_mode: bool,
}

/// Errors that can surface from the public reasoning API.
#[derive(Debug, Error)]
pub enum ReasonError {
    /// horned-owl axioms couldn't be lowered to the internal IR.
    /// Most often: a construct rustdl doesn't support yet (inverse
    /// roles, data ranges, anonymous individuals, ...).
    #[error("conversion from horned-owl: {0}")]
    Conversion(#[from] ConversionError),

    /// The IRI given to [`is_class_satisfiable`] was not declared as
    /// a class in the input ontology. Most often a typo or a missing
    /// `Declaration(Class(...))`.
    #[error("class IRI not in ontology: {0}")]
    UnknownClass(String),

    /// The tableau hit its internal iteration/recursion cap. Should
    /// not happen for inputs in the implemented fragment; bug
    /// indicator.
    #[error("tableau bailed out without a verdict (likely an internal limit)")]
    NoVerdict,

    /// A role chain sub-property axiom is outside the supported
    /// fragment. Phase 5 (R) supports **length-2** chains
    /// (`r ∘ s ⊑ t`) over **named** roles only. Anything longer, or
    /// any chain containing an `ObjectInverseOf` role expression,
    /// surfaces here.
    #[error(
        "role chain sub-property axiom outside supported fragment (only length-2 named-role chains are implemented)"
    )]
    RoleChainUnsupported,
}

/// Decide whether `class_iri` is satisfiable in the ontology.
///
/// Pipeline:
/// 1. Lower horned-owl axioms to the internal IR ([`convert_ontology`]).
/// 2. Push every concept to NNF ([`nnf_axioms`]).
/// 3. Run binary, nominal and role absorption ([`absorb`]).
/// 4. Build a [`TableauContext`] backed by the absorbed `TBox`.
/// 5. Add `Atomic(class)` to a fresh root node and call
///    [`TableauContext::is_satisfiable`].
///
/// Returns `Ok(true)` if `class_iri` is satisfiable w.r.t. the
/// ontology, `Ok(false)` if unsatisfiable, and a [`ReasonError`]
/// otherwise.
///
/// # Errors
///
/// See [`ReasonError`] variants. The most common cause is the IRI
/// not appearing as a declared class in the ontology.
pub fn is_class_satisfiable<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_class_satisfiable_internal(internal, class_iri)
}

/// Like [`is_class_satisfiable`] but the tableau run is bounded by
/// `deadline`. Returns `Ok(Some(sat))` if the tableau reached a
/// verdict before the deadline elapsed, or `Ok(None)` on timeout.
/// EL-closure / pure-EL fast paths are checked first and never
/// time out.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
    deadline: std::time::Duration,
) -> Result<Option<bool>, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let closure = owl_dl_saturation::saturate(&internal);
    if closure.is_unsatisfiable(class_id) {
        return Ok(Some(false));
    }
    if classify::is_pure_el(&internal) {
        return Ok(Some(true));
    }
    let prepared = PreparedOntology::from_internal(internal)?;
    let when = std::time::Instant::now() + deadline;
    prepared.decide_with_deadline(when, move |pool| pool.atomic(class_id))
}

/// Internal entry point that takes the already-lowered ontology.
/// Exposed for tests that want to assemble an `InternalOntology` by
/// hand or share one across multiple satisfiability checks.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_internal(
    internal: InternalOntology,
    class_iri: &str,
) -> Result<bool, ReasonError> {
    is_class_satisfiable_internal_full(internal, class_iri).map(|(b, _)| b)
}

/// Stats-returning variant of [`is_class_satisfiable`]; the verdict
/// is paired with a [`QueryStats`] recording whether the EL closure
/// answered alone.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_with_stats<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_class_satisfiable_internal_full(internal, class_iri)
}

fn is_class_satisfiable_internal_full(
    internal: InternalOntology,
    class_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    // EL closure oracle: a sound `⊑ ⊥` flag means the class is
    // definitively unsatisfiable, regardless of whether the rest of
    // the ontology is in the EL fragment. And for *pure*-EL inputs
    // the closure is also complete, so a *lack* of `⊑ ⊥` is itself
    // the verdict.
    let closure = owl_dl_saturation::saturate(&internal);
    let pure_el = classify::is_pure_el(&internal);
    if closure.is_unsatisfiable(class_id) {
        return Ok((
            false,
            QueryStats {
                answered_by_saturation: true,
                pure_el_mode: pure_el,
            },
        ));
    }
    if pure_el {
        return Ok((
            true,
            QueryStats {
                answered_by_saturation: true,
                pure_el_mode: true,
            },
        ));
    }
    let sat = run_satisfiability(internal, move |pool| pool.atomic(class_id))?;
    Ok((
        sat,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}

/// Decide whether `ontology` is consistent — i.e. whether it has any
/// model at all. Reduces to satisfiability of `⊤` under the full
/// `TBox` + `ABox`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_consistent<A: ForIRI>(ontology: &SetOntology<A>) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_consistent_internal(internal)
}

/// Internal entry point that takes the already-lowered ontology.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_consistent_internal(internal: InternalOntology) -> Result<bool, ReasonError> {
    is_consistent_internal_full(internal).map(|(b, _)| b)
}

/// Stats-returning variant of [`is_consistent`].
///
/// `is_consistent` always goes through the tableau today because the
/// EL closure can't soundly answer "every model is empty" without
/// `⊤`-sub-class lowering — so the returned stats will currently
/// report `answered_by_saturation: false`. Surfacing the field
/// anyway keeps the API symmetric and ready for a future fast path.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_consistent_with_stats<A: ForIRI>(
    ontology: &SetOntology<A>,
) -> Result<(bool, QueryStats), ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_consistent_internal_full(internal)
}

fn is_consistent_internal_full(
    internal: InternalOntology,
) -> Result<(bool, QueryStats), ReasonError> {
    let consistent = run_satisfiability(internal, ConceptPool::top)?;
    Ok((
        consistent,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}

/// Decide whether `sub_iri ⊑ super_iri` holds in `ontology`. Standard
/// reduction: subsumption holds iff `sub ⊓ ¬sup` is *unsatisfiable*.
///
/// Returns `Ok(true)` if `sub ⊑ sup`, `Ok(false)` if there is a model
/// in which some `sub`-instance is not a `sup`-instance.
///
/// # Errors
///
/// See [`ReasonError`]. Either IRI not declared as a class surfaces as
/// [`ReasonError::UnknownClass`].
pub fn is_subclass_of<A: ForIRI>(
    ontology: &SetOntology<A>,
    sub_iri: &str,
    super_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_subclass_of_internal(internal, sub_iri, super_iri)
}

/// Internal entry point that takes the already-lowered ontology.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_subclass_of_internal(
    internal: InternalOntology,
    sub_iri: &str,
    super_iri: &str,
) -> Result<bool, ReasonError> {
    is_subclass_of_internal_full(internal, sub_iri, super_iri).map(|(b, _)| b)
}

/// Stats-returning variant of [`is_subclass_of`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_subclass_of_with_stats<A: ForIRI>(
    ontology: &SetOntology<A>,
    sub_iri: &str,
    super_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_subclass_of_internal_full(internal, sub_iri, super_iri)
}

/// Saturation-only counterpart of [`is_subclass_of`]. Skips the
/// `sub ⊓ ¬sup` tableau probe and answers purely from the EL
/// closure: `true` iff the closure contains the subsumption or
/// proves `sub` unsatisfiable. Sound under-approximation: positive
/// answers are genuine, negatives may be missed positives the full
/// classifier would catch.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_subclass_of_saturation_only<A: ForIRI>(
    ontology: &SetOntology<A>,
    sub_iri: &str,
    super_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    let sub_id = internal
        .vocabulary
        .class_id(sub_iri)
        .ok_or_else(|| ReasonError::UnknownClass(sub_iri.to_owned()))?;
    let super_id = internal
        .vocabulary
        .class_id(super_iri)
        .ok_or_else(|| ReasonError::UnknownClass(super_iri.to_owned()))?;
    if sub_id == super_id {
        return Ok(true);
    }
    let closure = owl_dl_saturation::saturate(&internal);
    Ok(closure.contains(sub_id, super_id) || closure.is_unsatisfiable(sub_id))
}

fn is_subclass_of_internal_full(
    internal: InternalOntology,
    sub_iri: &str,
    super_iri: &str,
) -> Result<(bool, QueryStats), ReasonError> {
    let sub_id = internal
        .vocabulary
        .class_id(sub_iri)
        .ok_or_else(|| ReasonError::UnknownClass(sub_iri.to_owned()))?;
    let super_id = internal
        .vocabulary
        .class_id(super_iri)
        .ok_or_else(|| ReasonError::UnknownClass(super_iri.to_owned()))?;
    let pure_el = classify::is_pure_el(&internal);
    let sat_stats = QueryStats {
        answered_by_saturation: true,
        pure_el_mode: pure_el,
    };
    // Reflexive shortcut.
    if sub_id == super_id {
        return Ok((true, sat_stats));
    }
    // Saturation fast path: the EL closure is sound (every entry is a
    // genuine entailment) but only complete for the EL fragment of the
    // input. If it answers `yes`, we're done — skip the tableau. A
    // `no` just means "the EL subset doesn't witness it"; full
    // tableau still needs to run.
    let closure = owl_dl_saturation::saturate(&internal);
    if closure.contains(sub_id, super_id) {
        return Ok((true, sat_stats));
    }
    // If `sub` is itself unsat in the closure, every superclass —
    // including `super` — vacuously subsumes it.
    if closure.is_unsatisfiable(sub_id) {
        return Ok((true, sat_stats));
    }
    // Pure-EL inputs: the closure is complete, so a miss is the
    // verdict, no tableau needed.
    if pure_el {
        return Ok((false, sat_stats));
    }
    // H4 sound-accelerator wedge: a hyper `Unsat` proves the
    // subsumption (sound for any ontology), skipping the tableau. A
    // non-proof falls through. No-op when the wedge is disabled.
    if hyper_wedge_enabled() && HyperCache::build(&internal).proves(sub_id, super_id, None) {
        return Ok((
            true,
            QueryStats {
                answered_by_saturation: false,
                pure_el_mode: false,
            },
        ));
    }
    // `sub ⊓ ¬sup` is unsatisfiable iff every model that contains a
    // `sub`-instance also makes it a `sup`-instance.
    let sat = run_satisfiability(internal, move |pool| {
        let sub_concept = pool.atomic(sub_id);
        let super_concept = pool.atomic(super_id);
        let not_super = pool.not(super_concept);
        pool.and(vec![sub_concept, not_super])
    })?;
    Ok((
        !sat,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}

/// Shared end-of-pipeline runner: takes a (possibly mutated)
/// `InternalOntology`, runs the full normalize/absorb/`ABox`-seed
/// pipeline once, and asks the tableau whether `build_test_concept`
/// produces a satisfiable concept against the rest of the model.
///
/// One-shot convenience wrapper for callers (`is_class_satisfiable`,
/// `is_consistent`, `is_subclass_of`) that only ask a single tableau
/// question. For repeated queries against the same ontology — the
/// pairwise loop in `classify`, or the per-class probes in
/// `realize` — prefer [`PreparedOntology::from_internal`] +
/// [`PreparedOntology::decide`], which shares the expensive
/// prepare work across calls.
///
/// The closure is invoked *after* the pool has been cloned for the
/// tableau run, so the concept it returns is guaranteed to live in
/// the pool the tableau will use.
pub(crate) fn run_satisfiability<F>(
    internal: InternalOntology,
    build_test_concept: F,
) -> Result<bool, ReasonError>
where
    F: FnOnce(&mut ConceptPool) -> ConceptId,
{
    let prepared = PreparedOntology::from_internal(internal)?;
    prepared.decide(build_test_concept)
}

/// Snapshot of an ontology after every pre-tableau pass has run.
/// Holds the absorbed `TBox`, role-side metadata, `ABox` seed data and
/// the (now-frozen) concept pool, so each tableau query reuses one
/// preparation pass.
pub(crate) struct PreparedOntology {
    pool: ConceptPool,
    tbox: AbsorbedTBox,
    hierarchy: RoleHierarchy,
    inverse_pairs: Vec<(RoleId, RoleId)>,
    chain_axioms: Vec<(Role, Role, Role)>,
    asymmetric_roles: Vec<RoleId>,
    disjoint_role_pairs: Vec<(RoleId, RoleId)>,
    complements: Vec<(ConceptId, ConceptId)>,
    abox: Abox,
    /// Phase 1 scaffolding for the satisfying-model cache. The
    /// field is shipped now so [`crate::PreparedOntology::decide`]
    /// callers can be wired one at a time in Phase 2 without a
    /// signature change. See [`docs/model-caching-plan.md`] for
    /// the full design and the §A revert criterion if the cache
    /// doesn't move pizza/SIO walls.
    #[allow(dead_code)]
    model_cache: model_cache::ModelCache,
    /// H4 sound-accelerator state (clausified clauses + `¬sup`
    /// expansions), `Some` iff [`hyper_wedge_enabled`]. The classify
    /// pair loop consults it before the tableau (`subsumes_via_tableau`).
    hyper: Option<HyperCache>,
}

impl PreparedOntology {
    /// Run every preparation pass against `internal` so subsequent
    /// `decide` calls only have to allocate a fresh tableau and run
    /// the search.
    pub(crate) fn from_internal(mut internal: InternalOntology) -> Result<Self, ReasonError> {
        // H4: build the hyper cache from the un-mutated ontology
        // (before the absorb/NNF passes below consume it), iff enabled.
        let hyper = hyper_wedge_enabled().then(|| HyperCache::build(&internal));
        expand_role_characteristics(&mut internal);
        let hierarchy = build_role_hierarchy(&internal);
        let inverse_pairs = collect_inverse_pairs(&internal);
        let asymmetric_roles = collect_asymmetric_roles(&internal);
        let disjoint_role_pairs = collect_disjoint_role_pairs(&internal);
        let chain_axioms = collect_chain_axioms(&internal)?;
        let normalized = nnf_axioms(&mut internal);
        let tbox = absorb(&normalized, &mut internal.concepts);
        // Ensure `⊥` is interned — `apply_max` flags inequality
        // clashes by adding `Bot` to the offending node's label set,
        // and looks up the canonical id via `pool.bot_id()`. Cheap
        // & idempotent.
        let _ = internal.concepts.bot();
        let complements = precompute_max_complements(&mut internal.concepts);
        let abox = collect_abox(&mut internal);
        Ok(Self {
            pool: internal.concepts,
            tbox,
            hierarchy,
            inverse_pairs,
            chain_axioms,
            asymmetric_roles,
            disjoint_role_pairs,
            complements,
            abox,
            model_cache: model_cache::ModelCache::new(),
            hyper,
        })
    }

    /// H4 sound accelerator: `true` iff the hyper engine proves
    /// `sub ⊑ sup`. Always `false` when the wedge is disabled (no
    /// cache). A `true` is sound for any ontology; a `false` means the
    /// caller must fall back to the tableau ([`decide`](Self::decide)).
    pub(crate) fn hyper_proves(
        &self,
        sub: owl_dl_core::ir::ClassId,
        sup: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> bool {
        self.hyper
            .as_ref()
            .is_some_and(|hc| hc.proves(sub, sup, deadline))
    }

    /// Decide whether the test concept built by `build_test_concept`
    /// is satisfiable in this prepared ontology. The closure is
    /// invoked on a freshly-cloned pool so the prepared pool stays
    /// intact for the next call.
    pub(crate) fn decide<F>(&self, build_test_concept: F) -> Result<bool, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            &self.abox,
            None,
            build_test_concept,
        )
        .map(|opt| opt.expect("no deadline ⇒ search always returns Some(_)"))
    }

    /// Like [`Self::decide`] but the search is bounded by `deadline`.
    /// Returns `Ok(Some(sat))` if the tableau reached a verdict in
    /// time, or `Ok(None)` if the deadline elapsed first.
    pub(crate) fn decide_with_deadline<F>(
        &self,
        deadline: std::time::Instant,
        build_test_concept: F,
    ) -> Result<Option<bool>, ReasonError>
    where
        F: FnOnce(&mut ConceptPool) -> ConceptId,
    {
        decide(
            &self.pool,
            &self.tbox,
            &self.hierarchy,
            &self.inverse_pairs,
            &self.chain_axioms,
            &self.asymmetric_roles,
            &self.disjoint_role_pairs,
            &self.complements,
            &self.abox,
            Some(deadline),
            build_test_concept,
        )
    }
}

/// Pre-resolved `ABox` state, ready to seed into the tableau context.
/// All `ConceptId` fields are interned in the pool by
/// [`collect_abox`] (the last stage to mutate the pool); the tableau
/// then runs with a frozen pool.
#[derive(Default, Debug)]
struct Abox {
    /// `(individual, Nominal(individual)_id)` — one entry per
    /// individual referenced in any `ABox` axiom. Each gets a root
    /// node seeded with the nominal label before the test class is
    /// added.
    individuals: Vec<(IndividualId, ConceptId)>,
    /// `(individual, class_concept_id)` from `ClassAssertion`.
    class_assertions: Vec<(IndividualId, ConceptId)>,
    /// `(from_individual, role_id, to_individual)` from
    /// `ObjectPropertyAssertion`. Role polarity has been normalized:
    /// an inverse-role assertion swaps subject/object so the role
    /// stored here is always forward.
    property_assertions: Vec<(IndividualId, RoleId, IndividualId)>,
    /// `(individual, ∀r.¬{b}_concept_id)` from
    /// `NegativeObjectPropertyAssertion`. Encoded as a label that
    /// will be propagated by `apply_forall` along any matching
    /// edge — any actual r-relation to `b`'s nominal causes a
    /// `Not(Nominal(b))` / `Nominal(b)` clash.
    negative_property_assertions: Vec<(IndividualId, ConceptId)>,
    /// `(a, b)` pairs from `SameIndividual(a, b, ...)`. Decomposed
    /// pairwise — the tableau merges `b` into `a` for each pair.
    same_pairs: Vec<(IndividualId, IndividualId)>,
    /// `(a, b)` pairs from `DifferentIndividuals(a, b, ...)`.
    /// Likewise pairwise; the tableau marks them distinct.
    different_pairs: Vec<(IndividualId, IndividualId)>,
}

fn collect_abox(internal: &mut InternalOntology) -> Abox {
    use std::collections::HashSet;
    let mut abox = Abox::default();
    let mut seen: HashSet<IndividualId> = HashSet::new();
    let record_individual = |ind: IndividualId,
                             pool: &mut ConceptPool,
                             seen: &mut HashSet<IndividualId>,
                             abox: &mut Abox| {
        if seen.insert(ind) {
            let nom = pool.nominal(ind);
            abox.individuals.push((ind, nom));
        }
    };
    // First pass: enumerate every individual referenced and intern
    // its Nominal expression.
    for ax in &internal.axioms {
        match ax {
            Axiom::ClassAssertion { individual, .. } => {
                record_individual(*individual, &mut internal.concepts, &mut seen, &mut abox);
            }
            Axiom::ObjectPropertyAssertion {
                subject, object, ..
            }
            | Axiom::NegativeObjectPropertyAssertion {
                subject, object, ..
            } => {
                record_individual(*subject, &mut internal.concepts, &mut seen, &mut abox);
                record_individual(*object, &mut internal.concepts, &mut seen, &mut abox);
            }
            Axiom::SameIndividual(inds) | Axiom::DifferentIndividuals(inds) => {
                for ind in inds {
                    record_individual(*ind, &mut internal.concepts, &mut seen, &mut abox);
                }
            }
            _ => {}
        }
    }
    // Second pass: derive concrete assertions / clashes / pairs.
    // We collect axiom references in a local Vec to avoid double-
    // borrowing internal during the body.
    let axioms: Vec<Axiom> = internal.axioms.clone();
    for ax in &axioms {
        match ax {
            Axiom::ClassAssertion { class, individual } => {
                abox.class_assertions.push((*individual, *class));
            }
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                let (from, to) = if role.is_inverse() {
                    (*object, *subject)
                } else {
                    (*subject, *object)
                };
                abox.property_assertions.push((from, role.role_id(), to));
            }
            Axiom::NegativeObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                // Encode `(subject, object) ∉ role` as
                // `{subject} ⊑ ∀role.¬{object}`. Polarity of the
                // role passes through unchanged.
                let nom_b = internal.concepts.nominal(*object);
                let not_nom_b = internal.concepts.not(nom_b);
                let forall = internal.concepts.all(*role, not_nom_b);
                abox.negative_property_assertions.push((*subject, forall));
            }
            Axiom::SameIndividual(inds) => {
                for i in 0..inds.len() {
                    for j in (i + 1)..inds.len() {
                        abox.same_pairs.push((inds[i], inds[j]));
                    }
                }
            }
            Axiom::DifferentIndividuals(inds) => {
                for i in 0..inds.len() {
                    for j in (i + 1)..inds.len() {
                        abox.different_pairs.push((inds[i], inds[j]));
                    }
                }
            }
            _ => {}
        }
    }
    abox
}

/// Pre-compute NNF complements for every concept that the tableau
/// may need to negate at search time. Two sources of targets:
///
/// 1. **`Max(_, _, body)` bodies.** The choose rule branches on
///    `C` vs `¬C` around an unlabelled neighbour of a `≤n R.C`
///    constraint.
/// 2. **Literal `Or` disjuncts** — atomic, nominal, self-restriction,
///    or `Not(_)` of those. Phase 4 commit 6's *restricted semantic
///    branching* (see `docs/phase4-backjumping-plan.md`) asserts
///    `¬d_j` for previously-tried literal disjuncts `d_j` in
///    [`crate::search::branch`] so a re-derivation clashes
///    immediately. Complex (Or/And/quantified) disjuncts are
///    deliberately *excluded* — their complements are themselves
///    compound expressions whose addition would inflate the label
///    set faster than the back-jump can prune (Phase 4 attempt 1
///    regressed corpus 2× this way).
///
/// This is the last stage that mutates the pool; after this call
/// the pool is frozen for the tableau run.
fn precompute_max_complements(pool: &mut ConceptPool) -> Vec<(ConceptId, ConceptId)> {
    let mut targets: Vec<ConceptId> = pool
        .iter_with_ids()
        .filter_map(|(_, e)| match e {
            ConceptExpr::Max(_, _, body) => Some(*body),
            _ => None,
        })
        .collect();
    // Atomic-shaped Or disjuncts for semantic branching.
    let literal_disjuncts: Vec<ConceptId> = pool
        .iter_with_ids()
        .filter_map(|(_, e)| match e {
            ConceptExpr::Or(args) => Some(args.to_vec()),
            _ => None,
        })
        .flatten()
        .filter(|d| {
            matches!(
                pool.get(*d),
                ConceptExpr::Atomic(_)
                    | ConceptExpr::Nominal(_)
                    | ConceptExpr::SelfRestriction(_)
                    | ConceptExpr::Not(_)
            )
        })
        .collect();
    targets.extend(literal_disjuncts);
    targets.sort_unstable();
    targets.dedup();
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let neg = nnf_complement(target, pool);
        out.push((target, neg));
    }
    out
}

/// Build the ALCH role hierarchy from atomic `SubObjectPropertyOf` and
/// `EquivalentObjectProperties` axioms. Chain sub-property axioms are
/// not encoded in the hierarchy itself — they are collected separately
/// by [`collect_chain_axioms`] and registered on the
/// [`TableauContext`].
fn build_role_hierarchy(internal: &InternalOntology) -> RoleHierarchy {
    let mut builder = RoleHierarchyBuilder::with_roles(
        u32::try_from(internal.vocabulary.num_roles()).expect("vocabulary role count fits in u32"),
    );
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(sub_role),
                sup,
            } => {
                // Only encode the named-to-named portion of the
                // sub-role lattice; the inverse axis still hangs
                // off the polarity-check in `edge_satisfies`. If
                // either side carries an inverse polarity, we'd
                // need a Role-keyed hierarchy — defer to a later
                // commit, but still record the underlying-id
                // relation so same-polarity sub-role inference
                // remains correct.
                builder.add_sub_role(sub_role.role_id(), sup.role_id());
            }
            Axiom::EquivalentObjectProperties(roles) => {
                // r ≡ s ≡ … expands to pairwise sub-property both ways.
                for a in roles {
                    for b in roles {
                        if a != b {
                            builder.add_sub_role(a.role_id(), b.role_id());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    builder.build()
}

/// Collect the length-2 role-chain axioms supported by Phase 5 (R).
///
/// Two sources:
/// 1. `SubObjectPropertyOf` with a `Chain` LHS — must have exactly
///    length 2 and use only named roles end-to-end (including the
///    super-role).
/// 2. `TransitiveRole(Role::Named(r))` lowered to `(r, r, r)` — the
///    standard chain encoding of role transitivity.
///
/// Length-N chains (N > 2) are silently *skipped* rather than
/// erroring out: dropping them under-approximates the role-side
/// closure (some role-level entailments are missed) but is sound
/// for class-side reasoning, which is what `classify` consumes.
/// Family ontology has 4 length-3 chains (cousins, great-relatives)
/// whose super-roles only appear in role-axiom declarations, not in
/// any class definition — so classification under this skip matches
/// `HermiT` on the class hierarchy. Inverse roles in any position
/// (including the super-role) are accepted; the tableau's chain
/// rule reads each position's polarity to choose edge direction.
fn collect_chain_axioms(
    internal: &InternalOntology,
) -> Result<Vec<(Role, Role, Role)>, ReasonError> {
    let mut chains = Vec::new();
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(parts),
                sup,
            } => {
                if parts.len() != 2 {
                    // Length-N (N > 2) chain: drop. See doc comment.
                    continue;
                }
                chains.push((parts[0], parts[1], *sup));
            }
            Axiom::TransitiveRole(role) => {
                // Transitivity on `r` lowers to `r ∘ r ⊑ r` —
                // including the inverse polarity if the user
                // declared `TransitiveObjectProperty` against an
                // inverse-typed role expression.
                chains.push((*role, *role, *role));
            }
            _ => {}
        }
    }
    Ok(chains)
}

/// Lower the simple role-characteristic axioms into the equivalent
/// concept- and inverse-axiom forms the rest of the pipeline already
/// handles. This runs before [`nnf_axioms`] so the new axioms ride
/// through normalization + absorption like any other input.
///
/// Lowerings (Phase 5 part S — "simple" SROIQ role characteristics):
/// - `SymmetricRole(Named(r))` ⇒ `InverseObjectProperties(r, r)` — a
///   role that is its own inverse is symmetric. Picked up by
///   [`collect_inverse_pairs`].
/// - `FunctionalRole(Named(r))` ⇒ `SubClassOf(⊤, Max(1, r, ⊤))`.
/// - `InverseFunctionalRole(Named(r))` ⇒ `SubClassOf(⊤, Max(1, r⁻, ⊤))`.
///
/// Inverse-polarity inputs (`SymmetricRole(Inverse(r))`) are
/// semantically equivalent to the same-named axiom but we don't bother
/// special-casing — converter only emits named-role characteristics
/// today.
///
/// Original axioms are kept in `internal.axioms` so that downstream
/// passes (e.g., reverse conversion) still see them; the lowered
/// duplicates are appended.
fn expand_role_characteristics(internal: &mut InternalOntology) {
    let top = internal.concepts.top();
    let mut additions: Vec<Axiom> = Vec::new();
    for ax in &internal.axioms {
        match ax {
            Axiom::SymmetricRole(role) if !role.is_inverse() => {
                additions.push(Axiom::InverseObjectProperties(*role, *role));
            }
            Axiom::FunctionalRole(role) if !role.is_inverse() => {
                let max1 = internal.concepts.max(1, *role, top);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: max1,
                });
            }
            Axiom::InverseFunctionalRole(role) if !role.is_inverse() => {
                let inv = Role::inverse(role.role_id());
                let max1 = internal.concepts.max(1, inv, top);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: max1,
                });
            }
            Axiom::ReflexiveRole(role) => {
                // ⊤ ⊑ Self(r) — every individual carries the
                // self-restriction concept; the tableau's
                // `apply_self_restriction` then materializes the
                // self-edge.
                let self_r = internal.concepts.self_restriction(*role);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: self_r,
                });
            }
            Axiom::IrreflexiveRole(role) => {
                // ⊤ ⊑ ¬Self(r) — every individual is constrained to
                // not have an r-self-edge. NNF-safe: `Not(Self)` is
                // already in NNF.
                let self_r = internal.concepts.self_restriction(*role);
                let not_self = internal.concepts.not(self_r);
                additions.push(Axiom::SubClassOf {
                    sub: top,
                    sup: not_self,
                });
            }
            _ => {}
        }
    }
    internal.axioms.extend(additions);
}

/// Collect roles declared `AsymmetricObjectProperty`. Inverse-typed
/// declarations resolve to the same underlying `RoleId` (the
/// asymmetry constraint is about the unordered role pair regardless
/// of source polarity).
fn collect_asymmetric_roles(internal: &InternalOntology) -> Vec<RoleId> {
    let mut out = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::AsymmetricRole(role) = ax {
            out.push(role.role_id());
        }
    }
    out
}

/// Decompose every `DisjointObjectProperties(r, s, …)` axiom into its
/// pairwise constituents. Reflexive entries `(r, r)` (degenerate
/// `Disjoint(r)`) are skipped — they'd assert the role is disjoint
/// from itself, which is only satisfiable when no pair is in `r`. We
/// leave that diagnosis to higher-level validators rather than seed
/// universal clashes.
fn collect_disjoint_role_pairs(internal: &InternalOntology) -> Vec<(RoleId, RoleId)> {
    let mut pairs = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::DisjointObjectProperties(roles) = ax {
            for i in 0..roles.len() {
                for j in (i + 1)..roles.len() {
                    let a = roles[i].role_id();
                    let b = roles[j].role_id();
                    if a != b {
                        pairs.push((a, b));
                    }
                }
            }
        }
    }
    pairs
}

/// Collect declared inverse-role pairs from `InverseObjectProperties`
/// axioms. Each axiom `InverseObjectProperties(r, s)` contributes one
/// `(r.role_id(), s.role_id())` pair; the tableau context populates
/// the map symmetrically.
fn collect_inverse_pairs(internal: &InternalOntology) -> Vec<(RoleId, RoleId)> {
    let mut pairs = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::InverseObjectProperties(a, b) = ax {
            pairs.push((a.role_id(), b.role_id()));
        }
    }
    pairs
}

#[allow(clippy::too_many_arguments)]
fn decide<F>(
    pool: &ConceptPool,
    tbox: &AbsorbedTBox,
    hierarchy: &RoleHierarchy,
    inverse_pairs: &[(RoleId, RoleId)],
    chain_axioms: &[(Role, Role, Role)],
    asymmetric_roles: &[RoleId],
    disjoint_role_pairs: &[(RoleId, RoleId)],
    complements: &[(ConceptId, ConceptId)],
    abox: &Abox,
    deadline: Option<std::time::Instant>,
    build_test_concept: F,
) -> Result<Option<bool>, ReasonError>
where
    F: FnOnce(&mut ConceptPool) -> ConceptId,
{
    let mut pool = pool.clone();
    let test_concept: ConceptId = build_test_concept(&mut pool);
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, tbox, hierarchy);
    if let Some(d) = deadline {
        ctx.set_deadline(d);
    }
    for &(r, s) in inverse_pairs {
        ctx.declare_inverse_pair(r, s);
    }
    for &(r1, r2, sup) in chain_axioms {
        ctx.declare_chain_axiom(r1, r2, sup);
    }
    for &r in asymmetric_roles {
        ctx.declare_asymmetric_role(r);
    }
    for &(r, s) in disjoint_role_pairs {
        ctx.declare_disjoint_role_pair(r, s);
    }
    for &(body, comp) in complements {
        ctx.set_complement(body, comp);
    }

    // Phase 5 `ABox` seeding. Order matters:
    // 1. Create a nominal root for each individual.
    // 2. DifferentIndividuals — mark before any merges so a later
    //    SameIndividual on the same pair is detected as a clash.
    // 3. SameIndividual merges; failed merges (declared distinct)
    //    flag the surviving node with ⊥.
    // 4. ClassAssertion / NegativeObjectPropertyAssertion labels.
    // 5. ObjectPropertyAssertion edges between nominal roots.
    // Then add the test class to a fresh anonymous root and run.
    let mut roots: HashMap<IndividualId, NodeId> = HashMap::new();
    for &(ind, nom) in &abox.individuals {
        let node = ctx.new_node();
        ctx.add_label(node, nom);
        ctx.assign_nominal(ind, node);
        roots.insert(ind, node);
    }
    for &(left, right) in &abox.different_pairs {
        if let (Some(&nleft), Some(&nright)) = (roots.get(&left), roots.get(&right)) {
            let nleft = ctx.resolve(nleft);
            let nright = ctx.resolve(nright);
            ctx.mark_distinct(nleft, nright);
        }
    }
    for &(left, right) in &abox.same_pairs {
        if let (Some(&nleft), Some(&nright)) = (roots.get(&left), roots.get(&right)) {
            let target = ctx.resolve(nleft);
            let source = ctx.resolve(nright);
            if target == source {
                continue;
            }
            if !ctx.merge_into(source, target)
                && let Some(bot) = ctx.pool().bot_id()
            {
                ctx.add_label(target, bot);
            }
        }
    }
    for &(ind, c) in &abox.class_assertions {
        if let Some(&n) = roots.get(&ind) {
            let target = ctx.resolve(n);
            ctx.add_label(target, c);
        }
    }
    for &(ind, c) in &abox.negative_property_assertions {
        if let Some(&n) = roots.get(&ind) {
            let target = ctx.resolve(n);
            ctx.add_label(target, c);
        }
    }
    for &(from, role, to) in &abox.property_assertions {
        if let (Some(&nf), Some(&nt)) = (roots.get(&from), roots.get(&to)) {
            let from_n = ctx.resolve(nf);
            let to_n = ctx.resolve(nt);
            ctx.add_edge(from_n, role, to_n);
        }
    }

    // Now the test class on a fresh anonymous root.
    let test_root = ctx.new_node();
    ctx.add_label(test_root, test_concept);

    let outcome = owl_dl_tableau::search(&mut ctx, MAX_SEARCH_DEPTH);
    match outcome {
        owl_dl_tableau::SearchVerdict::Sat => Ok(Some(true)),
        owl_dl_tableau::SearchVerdict::Unsat(_) => Ok(Some(false)),
        owl_dl_tableau::SearchVerdict::DepthLimit if ctx.deadline_reached() => Ok(None),
        owl_dl_tableau::SearchVerdict::DepthLimit => Err(ReasonError::NoVerdict),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn check(onto: &SetOntology<RcStr>, iri: &str) -> bool {
        is_class_satisfiable(onto, iri).expect("verdict returned")
    }

    /// Hypertableau Phase H1c: end-to-end cross-check of the
    /// structural-transformation clausifier + Horn engine against
    /// the EL entailment on the `∃R.E ⊑ F` back-propagation shape.
    ///
    /// The H1b finding (clausify-from-absorbed deferred `∃`-on-LHS)
    /// is fixed by the H1c clausifier, which transforms the GCI
    /// axioms directly: `∃R.E ⊑ F` now becomes the Horn clause
    /// `R(x,y) ∧ E(y) → F(x)`, so the engine derives `C ⊑ F`. This
    /// test (formerly `#[ignore]`d) now passes.
    #[test]
    fn hyper_horn_matches_el_closure_with_existential_backprop() {
        use owl_dl_core::clause::clausify;
        use owl_dl_core::convert::convert_ontology;
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};

        // C ⊑ ∃R.D,  D ⊑ E,  ∃R.E ⊑ F  ⊨  C ⊑ F.
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:C))\nDeclaration(Class(:D))\nDeclaration(Class(:E))\n\
Declaration(Class(:F))\nDeclaration(ObjectProperty(:r))\n\
SubClassOf(:C ObjectSomeValuesFrom(:r :D))\n\
SubClassOf(:D :E)\n\
SubClassOf(ObjectSomeValuesFrom(:r :E) :F)\n\
)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let clauses = clausify(&internal);
        assert!(
            HyperEngine::all_horn(&clauses),
            "pure-EL ontology must clausify to all-Horn"
        );
        let c_id = internal
            .vocabulary
            .class_id("http://rustdl.test/C")
            .expect("C interned");
        let f_id = internal
            .vocabulary
            .class_id("http://rustdl.test/F")
            .expect("F interned");
        let mut engine = HyperEngine::new(&clauses, c_id);
        assert_eq!(engine.run(4096), HyperResult::Sat);
        assert!(
            engine.root_labels().contains(&f_id),
            "hyper engine must derive C ⊑ F via ∃R.E ⊑ F back-propagation; \
             root labels = {:?}",
            engine.root_labels()
        );
    }

    /// Hypertableau Phase H2c: the `¬B`-injection subsumption probe
    /// decides entailed subsumptions (`Unsat`) and correctly rejects
    /// non-entailed ones (`Sat`). `A ⊑ B ⊑ C` ⊨ `A ⊑ C` but ⊭ `C ⊑ A`.
    #[test]
    fn hyper_subsumption_probe_finds_transitive_and_rejects_converse() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
SubClassOf(:A :B)\nSubClassOf(:B :C)\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        // A⊑C is entailed (transitively) ⇒ Unsat reported.
        assert!(holds("A", "C"), "A ⊑ C must be found");
        assert!(holds("A", "B"), "A ⊑ B must be found");
        assert!(holds("B", "C"), "B ⊑ C must be found");
        // The converse C⊑A is not entailed ⇒ never reported as Unsat.
        assert!(!holds("C", "A"), "C ⊑ A must NOT be reported");
        assert!(!holds("C", "B"), "C ⊑ B must NOT be reported");
        // 3 classes ⇒ 6 ordered pairs; 3 are entailed subsumptions.
        assert_eq!(probe.pairs_tested, 6);
        assert_eq!(probe.subsumptions, 3);
    }

    /// Hypertableau Phase H3a: antecedent DNF-distribution unlocks the
    /// pizza-style covering subsumption. `Vegetarian ≡ Topping ⊓
    /// (Cheese ⊔ Veg)`, `Cheese ⊑ Topping` ⊨ `Cheese ⊑ Vegetarian` —
    /// previously a miss because the nested `Or` in the antecedent
    /// conjunction was deferred.
    #[test]
    fn hyper_subsumption_probe_handles_distributed_covering() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:Topping))\nDeclaration(Class(:Cheese))\n\
Declaration(Class(:Veg))\nDeclaration(Class(:Vegetarian))\n\
SubClassOf(:Cheese :Topping)\n\
EquivalentClasses(:Vegetarian \
ObjectIntersectionOf(:Topping ObjectUnionOf(:Cheese :Veg)))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("Cheese", "Vegetarian"),
            "Cheese ⊑ Vegetarian must be derivable after antecedent distribution"
        );
    }

    /// H3b ¬sup-expansion fires: `A ≡ B ⊓ ¬C`, `D ⊑ B`, `D` disjoint
    /// `C` ⊨ `D ⊑ A`. Refuting `D ⊓ ¬A` needs expanding
    /// `¬A = ¬B ⊔ C`: the `¬B` branch clashes (`D ⊑ B`), the `C`
    /// branch clashes (`D`⊓`C` disjoint). Bare `D ∧ A → ⊥` could not
    /// derive this — it would need `A` positively.
    #[test]
    fn hyper_subsumption_probe_expands_negated_definition() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\n\
Declaration(Class(:C))\nDeclaration(Class(:D))\n\
EquivalentClasses(:A ObjectIntersectionOf(:B ObjectComplementOf(:C)))\n\
SubClassOf(:D :B)\nDisjointClasses(:C :D)\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        assert!(probe.pairs_via_expansion > 0, "¬sup expansion must be used");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("D", "A"),
            "D ⊑ A must derive via expanding ¬A = ¬B ⊔ C"
        );
    }

    /// H4 encoding-drift guard: the hyper Q-injection and the tableau
    /// `sub ⊓ ¬sup` are *different encodings* of the same query. Every
    /// pair hyper proves (`Unsat`) must agree with the complete
    /// tableau (`is_subclass_of` = true). Catches clausifier/tableau
    /// drift before it reaches users — the wedge's soundness contract.
    #[test]
    fn hyper_wedge_agrees_with_tableau() {
        // A SROIQ-ish ontology with a covering + disjointness so the
        // hierarchy isn't all told: Veg ≡ Topping ⊓ (Cheese ⊔ Plant);
        // Cheese, Meat disjoint; Cheese, Plant ⊑ Topping.
        let src = format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:Topping))\nDeclaration(Class(:Cheese))\n\
Declaration(Class(:Plant))\nDeclaration(Class(:Meat))\nDeclaration(Class(:Veg))\n\
SubClassOf(:Cheese :Topping)\nSubClassOf(:Plant :Topping)\nSubClassOf(:Meat :Topping)\n\
DisjointClasses(:Cheese :Meat)\n\
EquivalentClasses(:Veg ObjectIntersectionOf(:Topping ObjectUnionOf(:Cheese :Plant)))\n)\n"
        );
        let onto = parse(&src);
        let internal = convert_ontology(&onto).expect("convert");
        let cache = HyperCache::build(&internal);
        let classes: Vec<(owl_dl_core::ir::ClassId, String)> = internal
            .vocabulary
            .classes()
            .map(|(id, iri)| (id, iri.to_string()))
            .collect();
        for (sub, sub_iri) in &classes {
            for (sup, sup_iri) in &classes {
                if sub == sup {
                    continue;
                }
                if cache.proves(*sub, *sup, None) {
                    // Hyper proved it ⇒ the complete tableau must agree.
                    let tableau =
                        is_subclass_of_internal(internal.clone(), sub_iri, sup_iri).expect("ok");
                    assert!(
                        tableau,
                        "hyper proved {sub_iri} ⊑ {sup_iri} but tableau disagrees"
                    );
                }
            }
        }
    }

    /// H4 `HyperCache::proves` works in isolation on the
    /// distributed-covering subsumption (saturation misses it, hyper
    /// proves it). Rules out a cache bug vs an orchestrator-wiring bug.
    #[test]
    fn hyper_cache_proves_distributed_covering() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:Topping))\nDeclaration(Class(:Cheese))\n\
Declaration(Class(:Veg))\nDeclaration(Class(:Vegetarian))\n\
SubClassOf(:Cheese :Topping)\n\
EquivalentClasses(:Vegetarian \
ObjectIntersectionOf(:Topping ObjectUnionOf(:Cheese :Veg)))\n)\n"
        ));
        let internal = convert_ontology(&onto).expect("convert");
        let cheese = internal
            .vocabulary
            .class_id("http://rustdl.test/Cheese")
            .expect("interned");
        let vegetarian = internal
            .vocabulary
            .class_id("http://rustdl.test/Vegetarian")
            .expect("interned");
        let cache = HyperCache::build(&internal);
        assert!(
            cache.proves(cheese, vegetarian, None),
            "HyperCache must prove Cheese ⊑ Vegetarian"
        );
        let topping = internal
            .vocabulary
            .class_id("http://rustdl.test/Topping")
            .expect("interned");
        assert!(
            !cache.proves(topping, vegetarian, None),
            "Topping ⊑ Vegetarian must NOT be proven (not entailed)"
        );
    }

    /// Nominals (`hasValue`): `A ≡ P ⊓ ∃r.{o}`, `B ⊑ P`, `B ⊑ ∃r.{o}`
    /// ⊨ `B ⊑ A`. The nominal `{o}` is clausified as an atomic class,
    /// so the `⊒`-direction `P ⊓ ∃r.{o} ⊑ A` derives `A` on `B`. The
    /// `RealItalianPizza` shape.
    #[test]
    fn hyper_subsumption_probe_handles_nominal_has_value() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:P))\n\
Declaration(ObjectProperty(:r))\nDeclaration(NamedIndividual(:o))\n\
EquivalentClasses(:A ObjectIntersectionOf(:P ObjectHasValue(:r :o)))\n\
SubClassOf(:B :P)\nSubClassOf(:B ObjectHasValue(:r :o))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(holds("B", "A"), "B ⊑ A must derive via the nominal {{o}}");
    }

    /// H3b Q-gating: the `¬sup` disjunction must bind only the root,
    /// never generated successors. `sub ≡ ∃R.A`, `sup ≡ ¬∃R.A` are
    /// disjoint but neither subsumes the other, so `sub ⊑ sup` must be
    /// `Sat` (not reported). If `¬sup` leaked onto the `R`-successor,
    /// the engine would clash spuriously and wrongly report `Unsat`.
    #[test]
    fn hyper_subsumption_probe_q_gating_no_spurious_subsumption() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(ObjectProperty(:r))\n\
Declaration(Class(:Sub))\nDeclaration(Class(:Sup))\n\
EquivalentClasses(:Sub ObjectSomeValuesFrom(:r :A))\n\
EquivalentClasses(:Sup ObjectComplementOf(ObjectSomeValuesFrom(:r :A)))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let reported = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        // Sub = ∃r.A, Sup = ¬∃r.A — genuinely disjoint, NOT subsuming.
        assert!(
            !reported("Sub", "Sup"),
            "Sub ⊑ Sup must NOT be reported (Q-gating leak would clash the r-successor)"
        );
    }

    /// HF2 canary (inverse-role propagation). `A ⊑ ∃R.B`,
    /// `B ⊑ ∀R⁻.C` ⊨ `A ⊑ C`: an `A` has an `R`-successor `b:B`;
    /// `b`'s `∀R⁻.C` forces every `R`-predecessor of `b` — including
    /// the `A` node — to be `C`. Deriving this requires propagating
    /// `∀R⁻` across the *reverse* edge. HF2 made this pass via
    /// inverse-aware matching in `enumerate_matches`: following `R⁻`
    /// from a node walks its `R`-predecessors. See
    /// `docs/hypertableau-hf2-scoping.md` §4.1.
    #[test]
    fn hyper_subsumption_probe_propagates_inverse_universal() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
Declaration(ObjectProperty(:R))\n\
SubClassOf(:A ObjectSomeValuesFrom(:R :B))\n\
SubClassOf(:B ObjectAllValuesFrom(ObjectInverseOf(:R) :C))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "C"),
            "A ⊑ C must be derivable via ∀R⁻ propagation across the reverse edge"
        );
    }

    /// HF2 named-inverse canary (`RBox` inverse-pair clausification).
    /// `InverseObjectProperties(R, S)` makes `S ≡ R⁻`, so `B ⊑ ∀S.C`
    /// is `B ⊑ ∀R⁻.C` and `A ⊑ ∃R.B` ⊨ `A ⊑ C` exactly as the inline
    /// canary — but here the inverse comes from the `RBox`, not an inline
    /// `ObjectInverseOf`. The clausifier rewrites role `S` to `R⁻`
    /// (`build_inverse_canon` / `canon_role`), after which the engine's
    /// flip-matching propagates `∀S` across the `R`-edge. See
    /// `docs/hypertableau-hf2-scoping.md` §1.
    #[test]
    fn hyper_subsumption_probe_propagates_named_inverse() {
        let onto = parse(&format!(
            "{HEADER}Ontology(\n\
Declaration(Class(:A))\nDeclaration(Class(:B))\nDeclaration(Class(:C))\n\
Declaration(ObjectProperty(:R))\nDeclaration(ObjectProperty(:S))\n\
InverseObjectProperties(:R :S)\n\
SubClassOf(:A ObjectSomeValuesFrom(:R :B))\n\
SubClassOf(:B ObjectAllValuesFrom(:S :C))\n)\n"
        ));
        let probe = hyper_subsumption_probe(&onto, 64, None).expect("probe runs");
        let holds = |sub: &str, sup: &str| {
            probe.results.iter().any(|r| {
                r.sub == format!("http://rustdl.test/{sub}")
                    && r.sup == format!("http://rustdl.test/{sup}")
                    && r.result == HyperResult::Unsat
            })
        };
        assert!(
            holds("A", "C"),
            "A ⊑ C must be derivable: S ≡ R⁻ so ∀S.C propagates across the R-edge"
        );
    }

    /// Regression for the pizza false-positive-unsat bug fixed
    /// 2026-05-25. Minimal repro extracted from pizza.ofn via ROBOT
    /// STAR extraction + axiom-level bisection. Bug was in
    /// [`TableauContext::merge_into`]: it copied source-node labels
    /// without their [`DepSet`]s, so a merge-induced clash returned
    /// empty `clash_deps`, which the back-jumping search treated as
    /// "branch-independent unsat" and back-jumped past the licensing
    /// disjunction (the `:S ⊔ ∀hs.¬:Hot` choice introduced by
    /// absorbing the equivalence). `HermiT` says `:A` is sat; rustdl
    /// agreed only after the fix.
    ///
    /// Pattern:
    ///   :A ⊑ :PT
    ///   :A ⊑ ∃hs.Mild
    ///   FunctionalObjectProperty(:hs)
    ///   :S ≡ :PT ⊓ ∃hs.Hot
    ///   Disjoint(:Hot, :Mild)
    ///
    /// Each axiom is essential — dropping any one yields the
    /// correct `sat` verdict (verified by bisection).
    /// Regression for the second pizza false-positive-unsat bug
    /// fixed 2026-05-25. Minimal repro of the
    /// `VegetarianTopping ≡ PizzaTopping ⊓ (CheeseTopping ⊔ … ⊔
    /// VegetableTopping)` shape: `:A` is `:F` is `:PT`; `:F` is
    /// disjoint with the union members. `HermiT` says `:A` is sat.
    /// Bug was in [`crate::search::branch`]: when asserting a
    /// disjunct, it used only `[my_id]` as deps instead of the
    /// parent `Or` label's deps ∪ `my_id`. A clash on a nested
    /// branch then returned `clash_deps` missing the outer branch's
    /// id, and back-jumping skipped past the licensing disjunction.
    #[test]
    fn pizza_equiv_pizzatopping_union_should_be_sat() {
        let onto = parse(
            "Prefix(:=<http://example.org/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://example.org/min-veg>\n\
Declaration(Class(:A))\n\
Declaration(Class(:F))\n\
Declaration(Class(:PT))\n\
Declaration(Class(:V))\n\
Declaration(Class(:C))\n\
Declaration(Class(:N))\n\
SubClassOf(:A :F)\n\
SubClassOf(:F :PT)\n\
SubClassOf(:C :PT)\n\
SubClassOf(:N :PT)\n\
DisjointClasses(:C :F :N)\n\
EquivalentClasses(:V ObjectIntersectionOf(:PT ObjectUnionOf(:C :N)))\n\
)\n",
        );
        assert!(
            check(&onto, "http://example.org/A"),
            "A should be satisfiable (matches HermiT) but rustdl returned unsat"
        );
    }

    /// Regression for the named-pizza false-positive unsat fixed
    /// 2026-05-25. With both `:DomainConcept` reverse-equiv (Country
    /// nominals branching) and `:Pizza ⊑ ∃:hasBase.:PizzaBase`
    /// generating a successor that also gets the same branching,
    /// `apply_nominal_assignment` ends up merging the root and the
    /// hasBase-successor as the same individual. The merge then
    /// moves `:Pizza` (which was added with deps=[] from initial
    /// concept-rule chain) to the merged node where it triggers
    /// disjointness (`Pizza ⊓ PizzaBase ⊑ ⊥`), producing a clash with
    /// empty `clash_deps`. Back-jumping skips past every branch
    /// because `[]` doesn't contain any `my_id` — `:NamedPizza`
    /// wrongly reported unsat.
    ///
    /// Fix: `merge_into_with_deps(source, target, merge_deps)` —
    /// the merge condition's deps (union of both sides' nominal
    /// label deps) flow into every moved label / edge, so a
    /// post-merge clash inherits them. Both `apply_nominal_assignment`
    /// and `apply_max` now pass the precise merge-condition deps.
    /// Regression for the `apply_min` over-assert bug fixed
    /// 2026-05-25 (the SIO bug). When `Min(n, R, body)` fires after
    /// subclass propagation has put `body` on additional existing
    /// R-witnesses, the rule was pairwise-marking *all* witnesses
    /// distinct — not just the `n` it commits to. The resulting
    /// over-constraint blocked any `Max(k, R, body)` merge with
    /// `k < witnesses.len()`, producing false-positive unsats on
    /// the 22-class cluster around `:SIO_000450` ("axis").
    ///
    /// Minimal repro (`HermiT`: sat):
    ///   :A ⊑ :B; :B ⊑ :C
    ///   :X508 ⊑ :X532
    ///   :C ⊑ =2 :r.:X532   (Min(2) + Max(2))
    ///   :B ⊑ =1 :r.:X508   (Min(1) + Max(1))
    /// A satisfying model has two :r-successors: one of type
    /// {:X508, :X532}, one of type {:X532} only.
    #[test]
    fn sio_apply_min_over_assert_should_be_sat() {
        let onto = parse(
            "Prefix(:=<http://example.org/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://example.org/min-card>\n\
Declaration(Class(:A))\n\
Declaration(Class(:B))\n\
Declaration(Class(:C))\n\
Declaration(Class(:X508))\n\
Declaration(Class(:X532))\n\
Declaration(ObjectProperty(:r))\n\
SubClassOf(:A :B)\n\
SubClassOf(:B :C)\n\
SubClassOf(:X508 :X532)\n\
SubClassOf(:C ObjectExactCardinality(2 :r :X532))\n\
SubClassOf(:B ObjectExactCardinality(1 :r :X508))\n\
)\n",
        );
        assert!(
            check(&onto, "http://example.org/A"),
            ":A should be sat (matches HermiT); apply_min was over-asserting distinctness"
        );
    }

    #[test]
    fn pizza_named_pizza_country_should_be_sat() {
        // Use the saved 84-line STAR-extraction fixture — small
        // enough to be in-tree, large enough to exercise the
        // role-characteristics chain that the original synthetic
        // 10-axiom repros couldn't reproduce.
        let src = include_str!("../tests/fixtures/named-pizza-country-bug.ofn");
        let onto = parse(src);
        assert!(
            check(
                &onto,
                "http://www.co-ode.org/ontologies/pizza/pizza.owl#NamedPizza"
            ),
            ":NamedPizza should be sat (matches HermiT) — merge-deps regression"
        );
    }

    #[test]
    fn pizza_functional_equiv_some_should_be_sat() {
        let onto = parse(
            "Prefix(:=<http://example.org/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://example.org/min-bug>\n\
Declaration(Class(:A))\n\
Declaration(Class(:PT))\n\
Declaration(Class(:S))\n\
Declaration(Class(:Hot))\n\
Declaration(Class(:Mild))\n\
Declaration(ObjectProperty(:hs))\n\
SubClassOf(:A :PT)\n\
SubClassOf(:A ObjectSomeValuesFrom(:hs :Mild))\n\
FunctionalObjectProperty(:hs)\n\
EquivalentClasses(:S ObjectIntersectionOf(:PT ObjectSomeValuesFrom(:hs :Hot)))\n\
DisjointClasses(:Hot :Mild)\n\
)\n",
        );
        assert!(
            check(&onto, "http://example.org/A"),
            "A should be satisfiable (matches HermiT) but rustdl returned unsat"
        );
    }

    #[test]
    fn satisfiable_atomic_class() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn unsatisfiable_via_equivalence() {
        // Test ≡ A ⊓ ¬A — :Test must be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    EquivalentClasses(:Test ObjectIntersectionOf(:A ObjectComplementOf(:A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn unsatisfiable_via_subsumption_chain() {
        // A ⊑ B, B ⊑ C, Test ≡ A ⊓ ¬C — :Test must be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:Test))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(:A ObjectComplementOf(:C)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn cyclic_tbox_terminates_via_blocking() {
        // A ⊑ ∃r.A — :A is satisfiable; must terminate.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn role_hierarchy_makes_concept_unsat() {
        // r ⊑ s; ∃r.A ⊓ ∀s.¬A — the sub-property axiom forces the
        // ¬A from ∀s to land on the r-witness too, producing a clash.
        // Without role hierarchy support this would (wrongly) be sat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SubObjectPropertyOf(:r :s)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r :A) \
        ObjectAllValuesFrom(:s ObjectComplementOf(:A))))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn inverse_object_properties_declared_inverse_matches() {
        // InverseObjectProperties(r, s); Test ≡ ∃r.A ⊓ ∀s⁻.¬A.
        // The declared pair lets the ∀s⁻ rule propagate ¬A through
        // the r-edge, clashing at the witness.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    InverseObjectProperties(:r :s)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r :A) \
        ObjectAllValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:A))))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn abox_class_assertion_propagates_to_nominal() {
        // ClassAssertion(A, alice); Test ≡ {alice} ⊓ ¬A — unsat
        // because the `ABox` forces alice into A.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectOneOf(:alice) ObjectComplementOf(:A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn abox_same_and_different_is_inconsistent() {
        // SameIndividual + DifferentIndividuals on the same pair —
        // the ontology has no model. Any class query should be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Test))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    DifferentIndividuals(:alice :bob)\n\
    SameIndividual(:alice :bob)\n\
    EquivalentClasses(:Test ObjectOneOf(:alice))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn nominal_forces_witness_merge() {
        // ∃r.(A ⊓ {alice}) ⊓ ∃r.(B ⊓ {alice}) — the two existentials
        // generate separate witnesses, but both carry {alice}; the
        // nominal-assignment rule merges them into one node carrying
        // A and B. Satisfiable.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A ObjectOneOf(:alice))) \
        ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B ObjectOneOf(:alice)))))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn min_cardinality_generates_distinct_witnesses() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMinCardinality(3 :r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn max_cardinality_alone_is_satisfiable() {
        // ≤1 r.A alone is trivially satisfiable — pick a model with
        // zero or one r-successors. Tests that Max parses, lowers,
        // and saturates without error.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMaxCardinality(1 :r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn min_and_max_conflict_unsat() {
        // ≥2 r.A ⊓ ≤1 r.A — two distinct A-witnesses required, only
        // one allowed. The merge rule cannot collapse them
        // (apply_min marked them distinct); inequality clash.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectIntersectionOf(\
        ObjectMinCardinality(2 :r :A) \
        ObjectMaxCardinality(1 :r :A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn role_chain_length_three_silently_skipped() {
        // Length-N (N > 2) chain axioms are silently dropped — sound
        // for class-side reasoning, just under-approximates the
        // role-side closure. Lets the family ontology classify
        // instead of hard-erroring; whoever needs the dropped role
        // entailments can flag it via `--features chain-strict` in
        // the future. The test just confirms the absence of an error.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    Declaration(ObjectProperty(:u))\n\
    Declaration(ObjectProperty(:t))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:r :s :u) :t)\n\
)\n"
        ));
        // No axiom forbids :A; with the length-3 chain dropped, the
        // ontology is just a class declaration plus inert role
        // declarations.
        assert!(is_class_satisfiable(&onto, "http://rustdl.test/A").expect("verdict returned"));
    }

    #[test]
    fn length_two_role_chain_supported() {
        // SubObjectPropertyOf(ObjectPropertyChain(r s) t) at length 2
        // is in scope for Phase 5 (R): the named-role two-hop chain
        // axiom is registered on the tableau context, so this
        // ontology is consistent and the test class is satisfiable
        // (no axioms forbid it).
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    Declaration(ObjectProperty(:t))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn query_stats_pure_el_answered_by_saturation() {
        // Pure EL ontology — every query should be answered by the
        // closure with `pure_el_mode == true`.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        let (verdict, stats) =
            is_subclass_of_with_stats(&onto, "http://rustdl.test/A", "http://rustdl.test/B")
                .expect("verdict");
        assert!(verdict);
        assert!(stats.answered_by_saturation);
        assert!(stats.pure_el_mode);

        let (sat, sat_stats) =
            is_class_satisfiable_with_stats(&onto, "http://rustdl.test/A").expect("verdict");
        assert!(sat);
        assert!(sat_stats.answered_by_saturation);
        assert!(sat_stats.pure_el_mode);
    }

    #[test]
    fn query_stats_hybrid_falls_through_to_tableau() {
        // Disjunction lives outside the EL fragment; the subsumption
        // check should fall through to the tableau and the stats
        // should reflect that.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A ObjectUnionOf(:B :C))\n\
)\n"
        ));
        let (_verdict, stats) =
            is_subclass_of_with_stats(&onto, "http://rustdl.test/A", "http://rustdl.test/B")
                .expect("verdict");
        assert!(!stats.pure_el_mode);
        assert!(!stats.answered_by_saturation);
    }

    #[test]
    fn unknown_class_iri_errors() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        let err = is_class_satisfiable(&onto, "http://rustdl.test/Nope")
            .expect_err("unknown class should error");
        assert!(matches!(err, ReasonError::UnknownClass(_)));
    }

    #[test]
    fn empty_ontology_is_consistent() {
        let onto = parse(&format!("{HEADER}Ontology(<http://rustdl.test/test>\n)\n"));
        assert!(is_consistent(&onto).expect("verdict"));
    }

    #[test]
    fn contradictory_abox_is_inconsistent() {
        // SameIndividual + DifferentIndividuals on the same pair —
        // no model exists.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    DifferentIndividuals(:alice :bob)\n\
    SameIndividual(:alice :bob)\n\
)\n"
        ));
        assert!(!is_consistent(&onto).expect("verdict"));
    }

    #[test]
    fn explicit_subclassof_axiom_entails_subsumption() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/B").expect("verdict")
        );
    }

    #[test]
    fn transitive_subclassof_is_entailed() {
        // A ⊑ B, B ⊑ C ⇒ A ⊑ C
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/C").expect("verdict")
        );
    }

    #[test]
    fn unrelated_classes_are_not_subclass() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
)\n"
        ));
        assert!(
            !is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/B")
                .expect("verdict")
        );
    }

    #[test]
    fn subclass_via_saturation_then_tableau_mixed_ontology() {
        // Mixed input: an EL subsumption (A ⊑ B ⊑ C reachable by the
        // saturation engine) plus a non-EL one (D ⊑ ∀r.A which the
        // saturation drops but the tableau handles). The
        // orchestrator should resolve both correctly.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    SubClassOf(:D ObjectAllValuesFrom(:r :A))\n\
)\n"
        ));
        // EL chain: saturation should handle without invoking tableau.
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/C").expect("verdict")
        );
        // Reflexive: handled by the in-function shortcut.
        assert!(
            is_subclass_of(&onto, "http://rustdl.test/D", "http://rustdl.test/D").expect("verdict")
        );
        // A doesn't subsume D (truly false; tableau-confirmed).
        assert!(
            !is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/D")
                .expect("verdict")
        );
    }

    #[test]
    fn subclass_of_unknown_class_errors() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        let err = is_subclass_of(&onto, "http://rustdl.test/A", "http://rustdl.test/Nope")
            .expect_err("unknown sup should error");
        assert!(matches!(err, ReasonError::UnknownClass(_)));
    }
}
