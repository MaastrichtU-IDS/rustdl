//! Finite models built from pure-EL ontologies' EL-saturation closures, and an
//! engine-blind axiom evaluator over them.
//!
//! The model built here is **not proven canonical**: the same logical nested-existential
//! witness can be labelled differently by the two expansion paths and end up interned as two
//! separate elements — see
//! `docs/known-limitations/verify-two-expansion-paths-split-a-witness.md`, which also records
//! two more ways a witness's label can come out wrong. Every known imprecision so far points
//! toward a spurious `Violated`, never a false `Verified`.
//!
//! See `docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`.

pub mod eval;
pub mod interp;
pub mod model;

pub use interp::{Element, Interpretation};

use std::time::Instant;

use owl_dl_core::{Axiom, ClassId, ConceptPool, InternalOntology, RoleId};
use owl_dl_saturation::Subsumers;

use eval::AxiomVerdict;
use model::FiniteModel;

/// Construction bounds. Checking is bounded separately, by a deadline passed to
/// `verify`, so no stale `Instant` is ever read off a model.
#[derive(Clone, Debug)]
pub struct Bounds {
    pub max_elements: usize,
    pub max_edges: usize,
    pub max_rounds: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            max_elements: 50_000,
            max_edges: 2_000_000,
            max_rounds: 8,
        }
    }
}

/// Why a run could not reach a verdict. NEVER treated as `Verified`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    UnhandledAxiom {
        axiom_index: usize,
        variant: &'static str,
    },
    UnhandledConcept {
        axiom_index: usize,
        variant: &'static str,
    },
    /// `limit: None` means a deadline expired rather than a count being exceeded.
    BoundTripped {
        bound: &'static str,
        limit: Option<usize>,
    },
    GuardedRoleHasEdges {
        role: RoleId,
    },
    ChainRangeOutOfProfile {
        chain_super: RoleId,
    },
    LabelNotClosed {
        class: ClassId,
        role: RoleId,
    },
    /// A run-delta on an ORIGINAL class between the first and final saturation:
    /// direct evidence the shipped classification is incomplete.
    RunDelta {
        class: ClassId,
    },
    /// One or more axioms in the ontology were not understood by conversion and were silently
    /// dropped BEFORE this crate ever saw them (`owl_dl_core::convert_ontology`'s
    /// `DroppedAxioms`) — so neither `build_model` nor `verify` checked them at all. A caller
    /// that reports `Verified`/`Violated` over such an ontology would be vouching for (or
    /// flagging a defect in) a strictly WEAKER closure than the one the reasoner's own
    /// `classify`/`consistent` would have used, which had those axioms. `count` is the total
    /// number of dropped axioms, summed across kinds — see the caller for the per-kind
    /// breakdown, which this crate has no way to represent (it does not depend on the CLI's
    /// conversion-reporting types).
    AxiomsDroppedAtConversion {
        count: u64,
    },
}

/// One axiom that a checked model fails to satisfy.
///
/// `witness` names the element(s) responsible for the failure (a single
/// element for a concept-level check, both edge endpoints for a role/chain
/// check). `note` carries the human-readable explanation, and it is where
/// each witness element's LABEL gets rendered — see `verify`'s doc for why
/// that has to happen there rather than here: the `FiniteModel` that could
/// answer what an `Element` even means is consumed by `verify` before a
/// caller ever sees a `Violation`, so `witness` alone would otherwise be
/// uninterpretable.
///
/// This doc is written from `verify`'s perspective, but `Violation` is not
/// exclusive to it: [`VerifiedModel::still_holds_after`] produces `Violation`s
/// too, via a `&self` **borrow** of the existing model rather than consuming
/// it, and checking only the freshly-`added` axioms rather than the whole
/// ontology. On a `Violation` that came from `still_holds_after`, `axiom_index`
/// indexes into the `added` slice passed to that call — **not** into
/// `internal.axioms`, which is what it indexes into on a `Violation` from
/// `verify`. The two call sites cannot be told apart from a bare `Violation`
/// value; a caller that stores both kinds needs to remember which produced
/// which.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub axiom_index: usize,
    pub axiom: Axiom,
    pub witness: Vec<Element>,
    pub note: String,
}

/// The outcome of checking a model against every axiom in its ontology.
///
/// `domain_size` is on ALL THREE variants: `verify` consumes the model by
/// value, so a caller has no other way to recover it once a verdict comes
/// back. `Violated` OUTRANKS `Unresolved` — a run that produces both is
/// reported `Violated` — but it still carries its own `unresolved` rows, so
/// coverage is never hidden behind a violation: a caller can see both "I
/// found a violation" and "N axiom forms I could not judge" from the one
/// verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Verified {
        axioms_checked: usize,
        domain_size: usize,
    },
    Violated {
        domain_size: usize,
        violations: Vec<Violation>,
        unresolved: Vec<UnresolvedReason>,
    },
    Unresolved {
        domain_size: usize,
        reasons: Vec<UnresolvedReason>,
    },
}

/// Type-state witness that a [`FiniteModel`] was checked against its own
/// ontology by [`verify`] and found to satisfy every axiom `check_axiom`
/// could judge (see [`Verdict::Verified`]).
///
/// The inner `FiniteModel` is a PRIVATE tuple field, so nothing outside this
/// module can construct a `VerifiedModel` other than by going through
/// `verify`'s checking loop. Today, nothing can reach the model mutably
/// through this type at all either — but that is because this module
/// defines no accessor at all yet, not because the language forbids adding
/// one: `verified_model_does_not_expose_its_inner_finite_model_mutably`
/// (`tests/evaluator.rs`) checks the field stays un-`pub`, but nothing here
/// stops a future `impl VerifiedModel` from adding an `into_inner` or
/// `&mut FiniteModel` accessor that would reopen this hole — that is a
/// review discipline, not something this test can enforce. Task 11 adds
/// `still_holds_after` as a method ON `VerifiedModel`, never on
/// `FiniteModel` itself: `FiniteModel::still_holds_after` must not exist,
/// because that would let a caller ask the question of a model that was
/// never checked and get an answer with no soundness guarantee behind it.
#[derive(Debug)]
pub struct VerifiedModel(FiniteModel);

impl VerifiedModel {
    /// Does the classification this model already witnesses remain valid if
    /// every axiom in `added` also holds?
    ///
    /// # The claim, and why it is licensed only here
    ///
    /// If every axiom in `added` holds in `self`'s model, the classification
    /// previously reported for the ontology `self` was built from remains
    /// valid IN FULL under the edit: every reported negative
    /// (non-)subsumption is witnessed by this SAME model (nothing about the
    /// model needs to change to keep witnessing it), and every reported
    /// positive still holds by monotonicity — see the "positives half is
    /// conditional" caveat below. So one model check replaces re-running the
    /// reasoner on the edited ontology.
    ///
    /// This only works because `self` is a `VerifiedModel`: it exists ONLY
    /// via `verify`'s `Verified` arm, i.e. every axiom the original report
    /// was based on was already confirmed to hold in this exact model. A
    /// model that was never checked (a bare [`model::FiniteModel`]) witnesses
    /// nothing — which is why this method is not `FiniteModel::still_holds_
    /// after`; see that type's `compile_fail` doctest in `model.rs`.
    ///
    /// # Caller contract
    ///
    /// - **Additions only.** `added` says nothing about REMOVED axioms. A
    ///   removed axiom is free for a reported NEGATIVE (shrinking the
    ///   entailed set can only preserve a non-subsumption, by monotonicity),
    ///   but it can invalidate a reported POSITIVE — checking that needs the
    ///   justification half of this project, out of scope here.
    /// - **`added` is already-lowered IR, and must be interned against the
    ///   SAME tables `self` was built from.** Convert each new horned-owl
    ///   `Component` via `owl_dl_core::convert::convert_component(&component,
    ///   &mut vocab, &mut pool)`
    ///   (`crates/owl-dl-core/src/convert.rs:1889`) against the ORIGINAL
    ///   ontology's `vocabulary`/`concepts` — never by re-converting the
    ///   whole edited ontology from scratch, which allocates a FRESH
    ///   `ConceptPool`/`Vocabulary` and silently gives the same-looking
    ///   `ClassId`/`ConceptId`/`RoleId` values a different meaning. `pool` is
    ///   an explicit parameter here precisely so that mistake cannot happen
    ///   silently: it must be the identical pool `added` was converted
    ///   against, and the one `self`'s labels are drawn from.
    /// - **Check `dropped` did not grow.** If converting `added` drops an
    ///   axiom, that axiom never makes it into `added` at all, so it is
    ///   invisible to this method — a `Verified` verdict says nothing about
    ///   it. The caller, not this method, must check
    ///   `InternalOntology::dropped` did not grow before trusting a
    ///   `Verified` result.
    /// - **The positives half is conditional.** "Reported positives hold by
    ///   monotonicity" presupposes they were correct in the first place. This
    ///   method is a completeness instrument for the EDIT, not a fresh
    ///   false-positive net over the original classification.
    ///
    /// # `SubObjectPropertyOf(Role)` / `EquivalentObjectProperties` are
    /// genuine checks here, unlike in `verify`
    ///
    /// [`eval::check_axiom`]'s doc explains why those two arms are true by
    /// construction when checking a FRESHLY BUILT model against its OWN
    /// ontology (`build_role_hierarchy` already folded `sub ⊑ sup` into the
    /// same hierarchy the check reads back, before the check ever runs).
    /// That argument does NOT carry over here: `added` is checked against
    /// `self`'s EXISTING, unchanged hierarchy, which does not contain the new
    /// `sub ⊑ sup` — so here those two arms genuinely ask whether the OLD
    /// model happens to already satisfy the NEW role axiom, and can and do
    /// report `Violated` (see `tests/incremental.rs`).
    ///
    /// # Witness rendering
    ///
    /// Unlike `verify`, this method has no `InternalOntology`/`Vocabulary` in
    /// scope — only a bare [`ConceptPool`] — so a `Violation::note` here
    /// renders each witness element's label as its raw `ClassId` list rather
    /// than a resolved IRI.
    pub fn still_holds_after(
        &self,
        pool: &ConceptPool,
        added: &[Axiom],
        deadline: Option<Instant>,
    ) -> Verdict {
        let domain_size = self.0.domain_size();
        let mut violations: Vec<Violation> = Vec::new();
        let mut unresolved: Vec<UnresolvedReason> = Vec::new();

        for (index, ax) in added.iter().enumerate() {
            if deadline.is_some_and(|dl| Instant::now() >= dl) {
                unresolved.push(UnresolvedReason::BoundTripped {
                    bound: "deadline",
                    limit: None,
                });
                break;
            }
            match eval::check_axiom(pool, &self.0, index, ax) {
                AxiomVerdict::Holds => {}
                AxiomVerdict::Fails { witness, note } => {
                    let rendered = render_incremental_witness(&self.0, &witness);
                    violations.push(Violation {
                        axiom_index: index,
                        axiom: ax.clone(),
                        witness,
                        note: format!("{note} [{rendered}]"),
                    });
                }
                AxiomVerdict::Unresolved(reason) => unresolved.push(reason),
            }
        }

        if !violations.is_empty() {
            return Verdict::Violated {
                domain_size,
                violations,
                unresolved,
            };
        }
        if !unresolved.is_empty() {
            return Verdict::Unresolved {
                domain_size,
                reasons: unresolved,
            };
        }
        Verdict::Verified {
            axioms_checked: added.len(),
            domain_size,
        }
    }
}

/// Renders every witness element's label as its raw `ClassId` list, for
/// [`VerifiedModel::still_holds_after`]'s `Violation::note`.
///
/// Unlike `render_witness`/`render_label` (used by `verify`), this has no
/// `InternalOntology`/`Vocabulary` in scope — `still_holds_after` takes only
/// a bare `ConceptPool` — so it cannot resolve a `ClassId` to its IRI.
fn render_incremental_witness(model: &FiniteModel, witness: &[Element]) -> String {
    witness
        .iter()
        .map(|e| format!("{e:?}={:?}", model.label(*e)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Builds the finite model checked against the ontology's axioms.
///
/// Despite the module doc's original framing, this model is **not proven canonical** — see
/// `docs/known-limitations/verify-two-expansion-paths-split-a-witness.md`.
///
/// # Why the model comes from the FINAL augmented run
///
/// `TseitinAllocator::new(internal.vocabulary.num_classes())`
/// (`owl-dl-saturation/src/lib.rs:3398`) bases marker ids at the user-class
/// count, so injecting `k` classes shifts EVERY Tseitin id by `k`, and markers
/// have no IRIs to remap by. Joining a later run's ids against an earlier run's
/// facts therefore mislabels elements arbitrarily — a constructible false
/// `Verified`. So seeds, facts and labels all come from the final run, while the
/// classification being VERIFIED is the one the user actually received (run 1).
///
/// A delta between run 1 and the final run on an ORIGINAL class is reported as
/// `RunDelta`: it is direct evidence the shipped classification is incomplete.
/// Injection is a sound MONOTONE extension — the final run can only ADD entailed
/// derivations among original classes — not an observationally inert one.
pub fn build_model(
    internal: &InternalOntology,
    bounds: &Bounds,
) -> Result<(FiniteModel, Vec<UnresolvedReason>), UnresolvedReason> {
    let (first_subs, _, _) = owl_dl_saturation::saturate_with_exists_facts(internal);
    let mut working = internal.clone();
    let mut reasons: Vec<UnresolvedReason> = Vec::new();
    let mut round = 0usize;

    loop {
        let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&working);
        let h = model::build_role_hierarchy(&working);
        if let Some(chain_super) = model::chain_range_out_of_profile(&working, &h) {
            return Err(UnresolvedReason::ChainRangeOutOfProfile { chain_super });
        }
        let eff = model::effective_ranges(&working, &h);
        let mut m = FiniteModel::seed(&working, &subs, &facts).with_hierarchy(h);
        let mut step = m.expand(&working, &subs, &facts, &eff, bounds);
        // BOTH expansion paths run. The fact path alone cannot reach a nested
        // existential witness (the saturator emits no inner fact and gives the
        // marker an empty subsumer set), which is why Task 4b exists.
        step.extend(m.expand_from_axioms(&working, &subs, &eff, bounds));
        // Chain/transitive edges are materialised AFTER both expansion paths so
        // the composed pairs have real endpoints to compose from.
        step.extend(m.close_chains_and_transitivity(&working, bounds));

        // Only a LabelNotClosed naming a REAL class is injectable. `expand`'s
        // fact-driven path can also report one whose `class` is a Tseitin
        // marker — `saturate_with_exists_facts` targets a nested existential's
        // fact at its marker, which `TseitinAllocator` allocates starting at
        // `working.vocabulary.num_classes()` and never interns an IRI for.
        // Injecting for a marker is worse than a no-op: the marker has no name
        // to key an IRI on, and — because injecting shifts `num_classes()` and
        // therefore the NEXT round's marker base by one — the very same
        // conceptual gap gets a DIFFERENT marker id every round, so it can
        // never re-match a previously injected class and `pending` would never
        // empty out (measured: on `cascade.ofn` the reported class climbed
        // 8, 9, 10, … in lock-step with the round number, forever).
        //
        // A marker-targeted report is NOT dropped, only excluded from
        // `pending`: it is a genuine "this local rule could not close, and
        // injection cannot reach it" signal, and silently discarding it would
        // be exactly the false-`Verified` channel this crate exists to
        // prevent. It survives into `reasons` on both exit paths below,
        // unfiltered.
        let num_real_classes = working.vocabulary.num_classes();
        let mut pending: Vec<(ClassId, RoleId)> = step
            .iter()
            .filter_map(|r| match r {
                UnresolvedReason::LabelNotClosed { class, role }
                    if (class.index() as usize) < num_real_classes =>
                {
                    Some((*class, *role))
                }
                _ => None,
            })
            .collect();
        // Dedup: the same (class, role) pair can be reported once per
        // matching element (multiple elements can independently hit the same
        // unclosed gap), and injecting it twice would push a second,
        // redundant EquivalentClasses axiom for the same already-interned Q.
        pending.sort_unstable();
        pending.dedup();

        if pending.is_empty() {
            // Any LabelNotClosed still in `step` here is necessarily
            // marker-targeted (a real-class one would have been captured into
            // `pending` above and this branch would not have been reached) —
            // keep it.
            reasons.extend(step);
            reasons.extend(run_deltas(internal, &first_subs, &subs));
            return Ok((m, reasons));
        }
        round += 1;
        if round >= bounds.max_rounds {
            reasons.push(UnresolvedReason::BoundTripped {
                bound: "max_rounds",
                limit: Some(bounds.max_rounds),
            });
            reasons.extend(step);
            return Ok((m, reasons));
        }
        for (y, r) in pending {
            model::inject_conjunction(&mut working, &subs, &eff, y, r);
        }
    }
}

/// Checks `model` against every axiom in `internal.axioms`, in order, and
/// returns a verdict plus — **only** when that verdict is `Verified` — the
/// model wrapped as a [`VerifiedModel`].
///
/// # `Some(VerifiedModel)` iff `Verified`, and why that is not "iff no violations"
///
/// `VerifiedModel::still_holds_after` (Task 11) derives its entire soundness
/// argument from the model having been checked, in full, against the
/// ontology it was built from. A `Violated` run obviously cannot license
/// that. An `Unresolved` run is the subtler case — `model` itself may be
/// perfectly fine — but "some axioms were never actually judged" is exactly
/// the situation the type-state exists to rule out, so it must ALSO refuse
/// the wrapper. Handing one out on anything short of a full `Verified` pass
/// would silently void the guarantee for every downstream caller, who has
/// no way to tell from the type alone that the check was incomplete.
///
/// # `Bounds` vs. `deadline`
///
/// [`Bounds`] governs `build_model`'s CONSTRUCTION only. Checking here is
/// bounded solely by `deadline`, passed in fresh by the caller — never a
/// stale `Instant` read off `model` — because construction and checking can
/// happen arbitrarily far apart in time (e.g. `still_holds_after`, called
/// long after the model that backs it was built). If `deadline` expires
/// before every axiom has been checked, the remaining axioms are left
/// unjudged and a `BoundTripped { bound: "deadline", limit: None }` is
/// recorded — `None` distinguishes a deadline from a count-based bound.
///
/// # Witness rendering happens HERE, not in the caller
///
/// [`eval::AxiomVerdict::Fails`] carries bare [`Element`]s, and `model` is
/// consumed by this function — a caller has no `FiniteModel` left to look an
/// element's label up against once a `Violation` reaches them. Rendering
/// therefore happens inside this loop, into `Violation::note`, while `model`
/// is still alive (see `render_witness`). It never calls
/// `Vocabulary::class_iri` on a class id `internal`'s vocabulary did not
/// itself intern — a Tseitin marker, or an `inject_conjunction`-created
/// `verify-aug:` class — because that call PANICS out of range; such an id
/// renders as a synthetic tag instead.
///
/// # `SubObjectPropertyOf(Role)` / `EquivalentObjectProperties` scoping
///
/// [`eval::check_axiom`]'s doc explains why those two arms cannot be
/// sabotaged by deleting a model edge when checking a FRESHLY BUILT model
/// against its own ontology: `build_role_hierarchy` records `sub ⊑ sup`
/// into the same closure that `has_edge`/`edges` walk, so the antecedent's
/// edges are already inside the consequent's search space, and the check
/// reads vacuously `Holds`. Do not read that as those two arms being dead
/// weight in general — it is NOT true under `still_holds_after`, which
/// checks ADDED axioms against an EXISTING model whose hierarchy does not
/// yet contain the new `sub ⊑ sup`.
pub fn verify(
    model: FiniteModel,
    internal: &InternalOntology,
    deadline: Option<Instant>,
) -> (Verdict, Option<VerifiedModel>) {
    let domain_size = model.domain_size();
    let mut violations: Vec<Violation> = Vec::new();
    let mut unresolved: Vec<UnresolvedReason> = Vec::new();

    for (index, ax) in internal.axioms.iter().enumerate() {
        if deadline.is_some_and(|dl| Instant::now() >= dl) {
            unresolved.push(UnresolvedReason::BoundTripped {
                bound: "deadline",
                limit: None,
            });
            break;
        }
        match eval::check_axiom(&internal.concepts, &model, index, ax) {
            AxiomVerdict::Holds => {}
            AxiomVerdict::Fails { witness, note } => {
                let rendered = render_witness(&model, internal, &witness);
                violations.push(Violation {
                    axiom_index: index,
                    axiom: ax.clone(),
                    witness,
                    note: format!("{note} [{rendered}]"),
                });
            }
            AxiomVerdict::Unresolved(reason) => unresolved.push(reason),
        }
    }

    if !violations.is_empty() {
        return (
            Verdict::Violated {
                domain_size,
                violations,
                unresolved,
            },
            None,
        );
    }
    if !unresolved.is_empty() {
        return (
            Verdict::Unresolved {
                domain_size,
                reasons: unresolved,
            },
            None,
        );
    }
    let axioms_checked = internal.axioms.len();
    (
        Verdict::Verified {
            axioms_checked,
            domain_size,
        },
        Some(VerifiedModel(model)),
    )
}

/// Renders every witness element's label into one comma-joined string, for
/// `Violation::note`. Must run while `model` is alive — see `verify`'s doc.
fn render_witness(model: &FiniteModel, internal: &InternalOntology, witness: &[Element]) -> String {
    witness
        .iter()
        .map(|e| format!("{e:?}={}", render_label(model, internal, *e)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders one element's label as `{iri, iri, ...}`.
///
/// Falls back to a synthetic tag for any class id `internal`'s vocabulary
/// did not itself intern — a Tseitin marker (`TseitinAllocator` bases marker
/// ids at `vocabulary.num_classes()` and never interns an IRI for one) or an
/// `inject_conjunction`-created `verify-aug:` class: `Vocabulary::class_iri`
/// indexes its own interned table directly and PANICS on an id outside it,
/// so this checks `num_classes()` first rather than ever calling it
/// speculatively.
fn render_label(model: &FiniteModel, internal: &InternalOntology, e: Element) -> String {
    let num_real = internal.vocabulary.num_classes();
    let parts: Vec<String> = model
        .label(e)
        .iter()
        .map(|c| {
            if (c.index() as usize) < num_real {
                internal.vocabulary.class_iri(*c).to_string()
            } else {
                format!("<synthetic#{}>", c.index())
            }
        })
        .collect();
    format!("{{{}}}", parts.join(", "))
}

/// Reports every ORIGINAL class whose satisfiability changed between the first
/// and final saturation.
///
/// The conceptual point stands regardless of whether it has ever fired: if an
/// original class's satisfiability differs between what the user's run
/// reported (`first`) and what the fully-injected final run computed
/// (`final_`), that is direct evidence of engine incompleteness on the
/// user's actual classification, not an artifact of this crate's own model
/// construction (injection is a sound monotone extension — see
/// `inject_conjunction`'s doc).
///
/// CORRECTION (Task 5 review, round 1): this comment previously claimed
/// "measured on `unsatnested.ofn`: injection flips `X` from satisfiable to
/// unsatisfiable, and `HermiT` agrees `X` is unsat." That does NOT reproduce —
/// re-measured, `build_model` on `unsatnested.ofn` finds no `LabelNotClosed`
/// at all (the automatic `aug`-driven injection never fires on that
/// fixture), so no injection happens and `first == final_` trivially. The
/// original claim came from a separate, MANUAL experiment (a hand-written
/// `Q ≡ ∃s.Y ⊓ F` added directly to the ontology by an investigator, not
/// something this function's automatic mechanism ever constructs) and was
/// wrongly attributed to this code path.
///
/// I looked for a fixture where THIS function's comparison (as opposed to
/// the sibling per-atom check inside `expand`/`materialise_exists`, which
/// DOES have a firing test — see `injected_q_unsatisfiable_reports_run_
/// delta_not_label_not_closed` in `tests/model.rs`) genuinely fires, and
/// could not construct one: `inject_conjunction` builds `aug` as exactly the
/// range classes `y` does not already subsume, so the EL saturator's own
/// conjunction-introduction rule for `y ⊓ aug ⊑ Q` can never independently
/// fire on `y` alone — only the synthetic `Q` ever satisfies both sides at
/// once — so injecting `Q` has no path back to changing `y`'s (or any other
/// original class's) own `is_unsatisfiable` verdict under the current
/// mechanism. This function is therefore currently unexercised by a
/// positive case in this crate's test suite; every existing test compares
/// and finds no delta, never compares and finds one. Kept as a safety net
/// for a mechanism this function does not itself have to predict (e.g. a
/// future injection shape, or an unexpected saturator interaction) — not
/// weakened or removed for being currently unreachable.
fn run_deltas(
    internal: &InternalOntology,
    first: &Subsumers,
    final_: &Subsumers,
) -> Vec<UnresolvedReason> {
    internal
        .vocabulary
        .classes()
        .filter(|(c, _)| first.is_unsatisfiable(*c) != final_.is_unsatisfiable(*c))
        .map(|(c, _)| UnresolvedReason::RunDelta { class: c })
        .collect()
}
