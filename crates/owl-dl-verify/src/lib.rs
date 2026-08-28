//! Verified canonical models for pure-EL ontologies, and an engine-blind
//! axiom evaluator over them.
//!
//! See `docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`.

pub mod eval;
pub mod interp;
pub mod model;

pub use interp::{Element, Interpretation};

use owl_dl_core::{ClassId, InternalOntology, RoleId};
use owl_dl_saturation::Subsumers;

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
}

/// Builds the canonical model.
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
        // Task 6 inserts the ChainRangeOutOfProfile check HERE. Do not add it in
        // this task: `chain_range_out_of_profile` does not exist yet and Task 5
        // must compile and pass on its own.
        let eff = model::effective_ranges(&working, &h);
        let mut m = FiniteModel::seed(&working, &subs, &facts).with_hierarchy(h);
        let mut step = m.expand(&subs, &facts, &eff, bounds);
        // BOTH expansion paths run. The fact path alone cannot reach a nested
        // existential witness (the saturator emits no inner fact and gives the
        // marker an empty subsumer set), which is why Task 4b exists.
        step.extend(m.expand_from_axioms(&working, &subs, &eff, bounds));

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

/// Reports every ORIGINAL class whose satisfiability changed between the first
/// and final saturation. Measured on `unsatnested.ofn`: injection flips `X` from
/// satisfiable to unsatisfiable, and `HermiT` agrees `X` is unsat — so this is a
/// defect signal, not noise.
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
