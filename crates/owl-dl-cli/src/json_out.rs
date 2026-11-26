//! Machine-readable JSON output for the CLI (`--json`). The stable bridge
//! contract consumed by the Protégé plugin. All arrays are sorted for
//! determinism; `schema_version` guards drift.
use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};
use horned_owl::curie::PrefixMapping;
use horned_owl::io::ofn::writer::write as write_ofn;
use horned_owl::model::{
    Build, ClassExpression, Component, Individual, MutableOntology, NamedIndividual, RcStr,
    SubClassOf,
};
use horned_owl::ontology::component_mapped::RcComponentMappedOntology;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{ClassId, IndividualId, InternalOntology, RoleId, Vocabulary};
use owl_dl_reasoner::justify::{JustifyOutcome, Verdict};
use owl_dl_reasoner::{
    Classification, DataPropertyValues, DifferentIndividuals, Disjointness, ObjectPropertyValues,
    PropertyClassification, ProveEntailmentResult, Realization, SameIndividuals, SyntheticDef,
};
use serde::Serialize;

/// IRI of `owl:Nothing`, used to render a `DerivedFact::Unsat` conclusion as
/// a plain `SubClassOf(C owl:Nothing)`.
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

const SCHEMA_VERSION: u32 = 1;

/// The per-kind dropped-axiom counts for `ontology`, as a `--json` block.
///
/// NOTE: this re-runs `convert_ontology` (via `dropped_axioms`), which the
/// caller has typically already done once to reason over the ontology — one
/// extra conversion per CLI invocation, negligible next to actual reasoning.
/// Kept as a separate call (rather than threading it out of the existing
/// `Classification`/`Realization` results) because those types don't carry
/// the conversion's `DroppedAxioms` through their public API.
#[must_use]
pub(crate) fn dropped_block(onto: &SetOntology<RcStr>) -> BTreeMap<String, u64> {
    owl_dl_reasoner::dropped_axioms(onto)
        .map(|d| d.by_kind().clone())
        .unwrap_or_default()
}

/// Decisions taken on the wedge's trusted `Sat` verdict, without the tableau
/// (issue #66; #568 widened it to every channel):
///
/// - `hyper_refuted_pairs`: a pair refuted by the per-pair wedge `Sat`;
/// - `label_cache_pruned`: a pair refuted because the super is absent from the
///   sub's wedge `Sat` labels (the tier walk and the defined-sup sweep);
/// - `snapshot_replay_not_subsumed`: a pair refuted by a replayed snapshot
///   (`RUSTDL_SNAPSHOT_CAPTURE`, default off);
/// - `wedge_sat_probe_hits`: a class taken as satisfiable (`C ⋢ ⊥`) from its
///   wedge `Sat` label.
///
/// A count of DECISIONS, not of distinct pairs (a pair can be pruned in both the
/// walk and the sweep). Undecided pairs below a pruned super are not included: a
/// refutation of `C ⊑ D` also stops the walk below `D`, and those pairs are never
/// decided.
///
/// No fragment is exempt (#568). `Horn` was, on the premise that the Horn
/// fixpoint is complete; #124 refuted that, and the #564 shape with
/// `RUSTDL_CLASSIFY_WEDGE_IF_BOUND=0` read 0 here while `classify` called an
/// unsatisfiable class satisfiable. `PureEl` reads 0 by construction: every
/// driver answers it from the saturator before any wedge runs.
///
/// **This is an EXPOSURE count, not a defect count.** At the CLI's default budget
/// it reads 971 on `ro`, about 119,255 on `sio` and about 2,623 on `pizza`
/// (2026-11-25, three interleaved runs each; label prunes are most of it), and all
/// three are MISSED=0 against the Konclude oracle in the FP=0 net. Before #568 it
/// counted only the per-pair channel, which reads the same now: 164, 336-343 and
/// 102-103 on main `22a69b81`. A non-zero value says these verdicts
/// were reached WITHOUT the tableau, not that any is wrong, which is why it is a
/// diagnostic and not a stderr warning.
///
/// What it IS good for: a consumer that needs exactness (the issue-#66 reporter
/// was diffing two `classify` hierarchies) can see that the hierarchy rests on
/// unverified `Sat` verdicts, and re-run with `RUSTDL_LABEL_HEURISTIC=0` and
/// `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` (the label channels are not gated by
/// `TRUST_SAT`), or ask `subclass` per pair.
pub(crate) fn trusted_sat_risk(stats: &owl_dl_reasoner::ClassificationStats) -> usize {
    stats.hyper_refuted_pairs
        + stats.label_cache_pruned
        + stats.snapshot_replay_not_subsumed
        + stats.wedge_sat_probe_hits
}

// `struct_excessive_bools`: each bool is a field of the published JSON schema,
// so the shape is the wire format, not a design choice an enum could improve.
#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ClassifyJson {
    pub(crate) schema_version: u32,
    pub(crate) consistent: bool,
    /// A class pair or per-class unsat probe was CUT without a verdict and took its sound
    /// default (not subsumed; satisfiable), or a preparation / conversion cut left a
    /// partial closure (`ClassificationStats::timed_out_pairs > 0`). #574: not only a deadline (per-pair,
    /// preparation, conversion or the global one) but also the node cap, the tableau's
    /// decision-depth limit (`depth_capped_pairs`), a #462 `NoVerdict`
    /// (`no_verdict_pairs`) and the bound-the-tail skips (`RUSTDL_BOUND_DIVERGED_TAIL`,
    /// `RUSTDL_BOUND_STALL_TAIL`). Reports only that something was cut; it does NOT
    /// cover the `trust_sat` risk below, nor anything else that is not a pair or probe
    /// cut (e.g. `consistency_undetermined`, `derived_same_undecided`). With
    /// `RUSTDL_UNSAT_PROBE_CUT_INCOMPLETE=0` a cut per-class unsat probe is not counted.
    pub(crate) incomplete: bool,
    /// #162: the global deadline ran out during PREPARATION (EL saturation or
    /// building the prepared ontology; conversion too, but only with
    /// `RUSTDL_HARD_GLOBAL_DEADLINE=1`, see `conversion_timed_out`), so only a
    /// partial saturation closure is reported and consistency was not fully
    /// checked. Implies `incomplete`; a separate field because it says the whole
    /// answer is partial, not that some class pairs were cut. Set at the default
    /// whenever the budget is still meetable when prep bounding is decided. By
    /// default a budget already spent at that point leaves prep unbounded, and an
    /// empty partial closure is re-run unbounded; `RUSTDL_HARD_GLOBAL_DEADLINE=1`
    /// removes both fallbacks, so there the closure may be empty.
    pub(crate) prep_timed_out: bool,
    /// #162: with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, the deadline ran out during
    /// CONVERSION, so only part of the ontology was read. Axioms past the cut
    /// were neither reasoned over nor counted in `dropped`, and classes they
    /// declare may be missing. Implies `prep_timed_out` (and so `incomplete`).
    pub(crate) conversion_timed_out: bool,
    /// #162: with `RUSTDL_HARD_GLOBAL_DEADLINE=1`, the deadline ran out while
    /// PARSING, so nothing was classified and every list below is empty. Implies
    /// `conversion_timed_out` and `consistency_undetermined`. Never set without
    /// hard mode, or with `RUSTDL_PARSE_DEADLINE=0`.
    pub(crate) parse_timed_out: bool,
    /// Issue #66, #568: verdicts taken on the wedge's own trusted `Sat`
    /// (pairs concluded NOT SUBSUMED, and classes taken as satisfiable) without
    /// the tableau — so this hierarchy CAN disagree with `rustdl subclass` on
    /// the same binary and the same pair while `incomplete` stays `false`. An
    /// EXPOSURE count of decisions, not of distinct pairs: see
    /// [`trusted_sat_risk`].
    ///
    /// Deliberately a SEPARATE field rather than folded into `incomplete`:
    /// that flag means "a pair or probe was cut", and overloading it would destroy a
    /// distinction callers already depend on. Same reasoning as
    /// `Realization::witness_prune_active`.
    pub(crate) trusted_sat_refutations: usize,
    /// The inconsistency pre-check reached no verdict: a route gave up (since
    /// #571 a `Stalled` consistency wedge counts only if the main-tableau
    /// fallback after it gave up too), or was
    /// skipped because the global deadline had already passed (#162), or
    /// classify's consistency probe was admitted (an unsatisfiable class, an
    /// incomplete `ABox` run, or counted data values) and gave up (#446), or the
    /// run was `--saturation-only` on an input that is not pure EL, where only the
    /// closure looked for a clash (#539). So
    /// `consistent: true` means "no clash found", not "proven consistent". On
    /// an inconsistent KB every class is unsatisfiable, so the hierarchy below
    /// may then be missing entailments (#204 review: a pigeonhole over
    /// `TBox`-only individuals read `consistent: true` with every flag clean).
    /// A SEPARATE field for the same reason as `trusted_sat_refutations`: the
    /// pre-check usually stops on its depth cap rather than a pair deadline, and
    /// folding it into `incomplete` would change that flag's meaning on about a
    /// fifth of `ABox`-bearing ORE ontologies.
    pub(crate) consistency_undetermined: bool,
    /// #429: `consistent: true` rests on a consistency-wedge `Sat` that no
    /// main-tableau run verified (the #392 name, as on `consistent --json`). Not a
    /// give-up, and never set with `consistent: false`.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #124: the honest calibration signal — `Classification::
    /// completeness_guaranteed()`, i.e. **this hierarchy is provably free of
    /// missed subsumptions**. `true` only on a fragment where the engine that
    /// answered is complete and when none of the other signals in this struct
    /// reports a gap: no timed-out pair, no `wedge_hierarchy_blind`, no
    /// `wedge_clauses_refused`, no `derived_same_undecided`, no
    /// `consistency_undetermined` and nothing in `dropped` (#246).
    /// `ClassificationStats::completeness_guaranteed` is the predicate, and
    /// `docs/assurance/completeness-guarantee-ledger.md` lists each condition.
    ///
    /// Added because on `X ⊑ ∃r.B`, `Domain(r, ∃s.Z)`, `∃s.Z ⊑ W` (⟹ `X ⊑ W`,
    /// derived by both Konclude and `HermiT`) every other field here reads
    /// clean: `consistent: true`, `incomplete: false` (correctly — no class pair
    /// was cut), `trusted_sat_refutations: 0` (before #568, which exempted Horn;
    /// it now reads 4 there, an exposure count that does not name the miss),
    /// `direct_subsumptions: []`. The miss was surfaced NOWHERE.
    ///
    /// A SEPARATE field rather than a change to `incomplete`, whose contract is
    /// "a pair or probe was cut" and which callers depend on: this reports a strictly
    /// broader thing, and `incomplete == false` implies nothing about
    /// completeness on its own.
    pub(crate) completeness_guaranteed: bool,
    /// #214: the wedge decided pairs with Layer A off (by flag, or above the
    /// role-closure cost cap) over a non-trivial role hierarchy, so entailments
    /// needing sub-role, inverse or symmetric reasoning may be missing.
    /// A SEPARATE field for the same reason as `consistency_undetermined`: no class
    /// pair was cut, and `incomplete` means exactly that.
    pub(crate) wedge_hierarchy_blind: bool,
    /// #353: clauses the wedge's matcher refuses, which therefore never fire, so
    /// entailments that need them may be missing. A SEPARATE field for the same
    /// reason as `wedge_hierarchy_blind`: no class pair was cut. 0 when there is none.
    pub(crate) wedge_clauses_refused: usize,
    /// #354: candidate individual equalities classify did not decide (budget, the
    /// global deadline or the probe cap), so it may be missing entailments that need
    /// them. A SEPARATE field: no class pair was cut.
    pub(crate) derived_same_undecided: usize,
    /// #448: pairs the transitive closure of the recorded relation added
    /// (`RUSTDL_CLASSIFY_CLOSE_MATRIX`, default on; `=0` disables): pairs recorded
    /// "not subsumed" (a cut, or a trusted wedge `Sat`) that recorded subsumptions
    /// imply. Each is sound, since every recorded pair is. Diagnostic only: it neither
    /// sets nor clears `incomplete` (a cut also prunes the walk below it). 0 when the
    /// closure is off.
    pub(crate) matrix_closure_added_pairs: usize,
    /// #564: classify-wedge fixpoints the merge-churn guard stopped
    /// (`RUSTDL_WEDGE_MERGE_CHURN_GUARD`): a fixpoint that built nodes only to merge
    /// them away, with every blocker refused by the NN guard, returned no verdict, and
    /// its probe went to the main tableau. NOT `incomplete`: no pair was cut and no
    /// answer was guessed. A fall-through the tableau could not finish is reported by
    /// the fields that cover that. Diagnostic; 0 when the guard never fired.
    pub(crate) wedge_merge_churn_stops: u64,
    /// #571: classify's consistency wedge returned `Stalled` (no verdict). Since
    /// #571 the main tableau then runs in what is left of the wedge's budget, and
    /// `stalled_wedge_fallback` says what it answered. Diagnostic.
    pub(crate) consistency_wedge_stalled: bool,
    /// #571: that consistency wedge's fixpoints the merge-churn guard stopped: the
    /// cause of the `Stalled` when non-zero, its deadline or another cap when 0.
    /// Separate from `wedge_merge_churn_stops`, which counts the classify wedge.
    pub(crate) consistency_wedge_merge_churn_stops: u64,
    /// #571: what the main-tableau fallback after a `Stalled` consistency wedge
    /// answered: `"consistent"` (a model), `"inconsistent"` (a clash), `"gave_up"`
    /// (no verdict in budget; `consistency_undetermined` stays set), or `null` when
    /// it did not run (`RUSTDL_CLASSIFY_STALLED_WEDGE_FALLBACK=0`, no budget left,
    /// or no `Stalled` wedge).
    pub(crate) stalled_wedge_fallback: Option<&'static str>,
    /// #462/#475: probes (per-class unsat probes and pair probes) that ended in
    /// `NoVerdict` as an internal limit, likely a reasoner bug (#449): any probe with NO
    /// deadline (e.g. under `--pair-timeout-ms 0`), and any probe whose deterministic
    /// saturation stalled, budgeted or not. The command also prints a "likely a reasoner
    /// bug" note on stderr and exits 3. Each took its sound default and is also a
    /// timed-out pair, so `incomplete` is set; it is not counted as a decision-depth cut.
    /// It does NOT count every `NoVerdict`: on a budgeted probe that did not stall, a
    /// `NoVerdict` is the routine decision-depth cut (#288). 0 when there is none.
    pub(crate) no_verdict_pairs: usize,
    /// #574: of the cut pairs and probes, those cut by the tableau's decision-depth limit
    /// (`ClassificationStats::depth_capped_pairs`, #288), directly or through early
    /// abandon, rather than by a deadline or the node cap. A larger `--pair-timeout-ms`
    /// or node cap does not recover these; the deadline-free search
    /// (`--pair-timeout-ms 0`), whose limit is far larger, may. Each also sets
    /// `incomplete`; a #462 `NoVerdict` is counted in `no_verdict_pairs`, not here. 0 when
    /// there is none. Added without a `schema_version` bump.
    pub(crate) depth_capped_pairs: usize,
    pub(crate) unsatisfiable: Vec<String>,
    pub(crate) equivalent_groups: Vec<Vec<String>>,
    pub(crate) direct_subsumptions: Vec<[String; 2]>,
    pub(crate) dropped: BTreeMap<String, u64>,
    /// #538: n-ary axioms (`DisjointClasses`, `DifferentIndividuals`, `DisjointUnion`,
    /// `Disjoint`/`EquivalentObjectProperties`, …) that repeated an operand. Their operands
    /// are a SET (OWL 2 Structural Specification §2.1), so a repeat says nothing:
    /// `DisjointClasses(:C :C)` does not make `C` unsat, `DifferentIndividuals(:a :a :b)`
    /// says only `a ≠ b`. `vacuous` counts those left with one operand, which say nothing
    /// at all (a one-member `DisjointUnion(D C)` still says `D ≡ C` and is not counted).
    /// Not an under-approximation under the set reading, so a SEPARATE field from
    /// `dropped`, and it never affects `completeness_guaranteed`. Under a list reading
    /// these axioms could add answers (both Konclude and `HermiT` keep the repeats); this
    /// field is how a caller sees that. `{"axioms": 0, "vacuous": 0}` when none.
    pub(crate) collapsed_operands: CollapsedOperandsJson,
}

/// `collapsed_operands` in `classify --json` and `consistent --json` (#538).
#[derive(Serialize, Clone, Copy, Default)]
pub(crate) struct CollapsedOperandsJson {
    pub(crate) axioms: u64,
    pub(crate) vacuous: u64,
}

impl From<owl_dl_core::CollapsedOperands> for CollapsedOperandsJson {
    fn from(c: owl_dl_core::CollapsedOperands) -> Self {
        Self {
            axioms: c.axioms,
            vacuous: c.vacuous,
        }
    }
}

// `struct_excessive_bools`: each bool is a field of the published JSON schema.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
pub(crate) struct ConsistentJson {
    pub(crate) schema_version: u32,
    pub(crate) consistent: bool,
    /// #182: `true` when `consistent` is a GIVE-UP reported in its sound
    /// direction (fall-through deadline / node cap, or the #344 data-counting
    /// verification of a wedge `Sat` giving up), not a witnessed verdict.
    pub(crate) incomplete: bool,
    /// #392: `consistent: true` rests only on the consistency wedge's `Sat`, which
    /// no main-tableau run verified (the wedge is not proven complete). A trust
    /// decision, not a give-up, so it is SEPARATE from `incomplete`; broader than
    /// `wedge_clauses_refused`. The same flag as `sat-expr`/`subclass-expr`/`prove`
    /// `--json` (#401). Never set with `consistent: false`.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #357: clauses the consistency wedge cannot match, which therefore never fire,
    /// so a `consistent: true` from the wedge may miss a clash. A SEPARATE field from
    /// `incomplete` (a give-up), as in `classify --json`. Counted whenever the
    /// consistency wedge was consulted; 0 when it was not.
    pub(crate) wedge_clauses_refused: usize,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    pub(crate) dropped: BTreeMap<String, u64>,
    /// #538: as in `classify --json`. A repeated operand of `DifferentIndividuals` or
    /// `DisjointClasses` no longer makes the ontology inconsistent.
    pub(crate) collapsed_operands: CollapsedOperandsJson,
}

/// One entry of `VerifyElJson::violations`.
#[derive(Serialize)]
pub(crate) struct VerifyElViolationJson {
    pub(crate) axiom_index: usize,
    pub(crate) note: String,
}

/// `--json` output for `verify-el`. `verdict` is one of `"verified"` /
/// `"violated"` / `"unresolved"` (never inferred from the other fields, to
/// keep a machine reader from having to reconstruct it) — see
/// `main.rs::verify_el_verdict_str`. `unresolved` carries the `Debug`
/// rendering of every [`owl_dl_verify::UnresolvedReason`], SORTED before this
/// struct is built (never in whatever order the model builder happened to
/// discover them in) — the CLI-level half of the byte-identity guarantee
/// `--json` run twice must satisfy; see `main.rs::sorted_debug_strings`.
#[derive(Serialize)]
pub(crate) struct VerifyElJson {
    pub(crate) schema_version: u32,
    pub(crate) verdict: &'static str,
    pub(crate) domain_size: usize,
    pub(crate) axioms_checked: usize,
    pub(crate) violations: Vec<VerifyElViolationJson>,
    pub(crate) unresolved: Vec<String>,
    pub(crate) dropped: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub(crate) struct IndividualTypesJson {
    pub(crate) iri: String,
    pub(crate) types: Vec<String>,
    pub(crate) direct_types: Vec<String>,
}

// `struct_excessive_bools`: each bool is a field of the published JSON schema.
#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct RealizeJson {
    pub(crate) schema_version: u32,
    /// At least one instance probe was CUT rather than refuted, so a reported type
    /// set may be short. Mirrors `classify --json`'s field of the same name, which
    /// `realize` lacked entirely until 2026-08-18 — a consumer could not tell a
    /// complete type set from a truncated one. See `Realization::incomplete`.
    pub(crate) incomplete: bool,
    /// #574: at least one cut instance probe hit the tableau's decision-depth limit
    /// (`Realization::depth_capped`), where a larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or
    /// node cap does not help and the deadline-free probe (`=0`) may. Implies
    /// `incomplete`. It covers the instance probes only: a depth cut in realize's own
    /// classification step sets neither field.
    pub(crate) depth_capped: bool,
    /// The pseudo-model witness prune was ACTIVE, so some reported non-memberships were
    /// SKIPPED rather than refuted by a probe. Distinct from `incomplete` (which reports
    /// CUT probes) and NOT implied by it: the prune's loss is silent, leaving
    /// `incomplete: false`. See `Realization::witness_prune_active`.
    pub(crate) witness_prune_active: bool,
    /// The prune was active AND its witness may lack an entailed type (#387), so a
    /// type may be missing with `incomplete: false`. `false` with the prune active
    /// means no known reason for loss, NOT a guarantee that nothing was dropped. See
    /// `Realization::witness_prune_may_drop`.
    pub(crate) witness_prune_may_drop: bool,
    /// Why the witness may lack an entailed type, as stable names (`branched`,
    /// `deferred_clauses`, `refused_clauses`, `reseed_net_off`,
    /// `inverse_functional_max_off`); empty when the prune was not active or none
    /// applies. See `WitnessPruneReasons`.
    pub(crate) witness_prune_reasons: Vec<&'static str>,
    /// Some individual has two `direct_types` whose subsumption was left undecided
    /// (#388), so a `direct_types` list may hold a strict superclass of another entry;
    /// every entry is still a genuine type. Distinct from `incomplete` (a type may be
    /// missing) and from `witness_prune_active`, not implied by either, and does not
    /// cover them. See `Realization::direct_types_unverified`.
    pub(crate) direct_types_unverified: bool,
    /// #429: the classification under this realization took the ontology as
    /// consistent on a consistency-wedge `Sat` alone (the `consistent --json` /
    /// `classify --json` field). Not a give-up. See
    /// `Realization::relies_on_trusted_consistency`.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #493: a probe ended in `NoVerdict` as an internal limit, likely a reasoner bug
    /// (#449): a probe with no deadline (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`), or one
    /// whose deterministic saturation stalled, budgeted or not (#475). The command also
    /// prints a "likely a reasoner bug" note on stderr and exits 3, after this JSON. It
    /// covers the realize classification, every instance probe (anonymous individuals'
    /// too) and #388's direct-types checks. Each took its sound default, so `incomplete`
    /// is set too. A plain depth cut, deadline or node cap does not set it. Not covered:
    /// the pseudo-model witness build. (Realize runs no #369 lazy consistency check.)
    pub(crate) no_verdict: bool,
    /// #539: the run was `realize --saturation-only`, which reads types off the EL
    /// saturation closure alone and runs no tableau probe and no consistency check.
    /// A sound under-approximation: every listed type is entailed, but a type may
    /// be MISSING with every other flag clean, and an inconsistent ontology (where
    /// every type is entailed) may go undetected. `false` on a plain `realize`.
    pub(crate) saturation_only: bool,
    pub(crate) individuals: Vec<IndividualTypesJson>,
    pub(crate) dropped: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub(crate) struct DisjointJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) disjoint_classes: Vec<[String; 2]>,
    pub(crate) disjoint_object_properties: Vec<[String; 2]>,
    pub(crate) disjoint_data_properties: Vec<[String; 2]>,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    /// #369: the property half's answer is non-empty and the KB's consistency
    /// stayed undetermined (see `sat-expr`). The class half reports through
    /// classify's own inconsistency verdict.
    pub(crate) consistency_undetermined: bool,
    /// #389/#390: constructs present that the property half of this surface (the class half reports through `incomplete`) ignores and that
    /// can entail a relation it would report (see
    /// `owl_dl_reasoner::property_structural_gaps`). Empty means none of the known
    /// routes occurs, not that the answer is complete. Separate from `incomplete`,
    /// which means only that a deadline cut a probe.
    pub(crate) structural_gaps: Vec<String>,
}

// `struct_excessive_bools`: each bool is a field of the published JSON schema.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
pub(crate) struct SatExprJson {
    pub(crate) schema_version: u32,
    /// A probe gave up (node cap): the verdict is the sound direction of no
    /// verdict, and the command exits 3 (#363 A2). Give-up only, as on the other
    /// query JSONs (#395); an answer off the pure-EL fast path is reported by
    /// `completeness_guaranteed`. (Before package 2 this was also `true` whenever
    /// the ontology was not pure EL.)
    pub(crate) incomplete: bool,
    /// The verdict is guaranteed complete: answered on the pure-EL fast path,
    /// nothing gave up, and conversion dropped no axiom (#540): neither the
    /// input's nor the query expression's own. The stderr `warning:` names
    /// those drops (from the probed conversion, so query-side drops too). `false` means a "satisfiable" / "not entailed" is not
    /// proven complete, not that it is wrong (as `classify --json`).
    pub(crate) completeness_guaranteed: bool,
    /// #401: the verdict was reached with the `ABox` seed dropped (it cannot
    /// affect the answer: the ontology and the expression have no nominal), and
    /// the consistency that licensed dropping it rests on a trusted wedge `Sat`
    /// rather than a main-tableau model, as `consistent`'s own answer does. A
    /// wrong `Sat` there could only make the answer a miss, never a false
    /// positive. Separate from `incomplete`; not a give-up.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #369: a positive answer whose KB consistency stayed undetermined (the
    /// consistency wedge stalled or ran past `RUSTDL_QUERY_LAZY_CONSISTENCY_MS`;
    /// on a KB with no individual, #457's bounded main-tableau check ran past
    /// `RUSTDL_CONSISTENCY_FALLBACK_MS`), so it may be the vacuous answer of an
    /// inconsistent KB. Separate from `incomplete`; not a give-up. A KB found
    /// inconsistent is an error instead.
    pub(crate) consistency_undetermined: bool,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    pub(crate) satisfiable: bool,
}

// `struct_excessive_bools`: each bool is a field of the published JSON schema.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
pub(crate) struct SubclassExprJson {
    pub(crate) schema_version: u32,
    /// A probe gave up (node cap): the verdict is the sound direction of no
    /// verdict, and the command exits 3 (#363 A2). Give-up only, as on the other
    /// query JSONs (#395); an answer off the pure-EL fast path is reported by
    /// `completeness_guaranteed`. (Before package 2 this was also `true` whenever
    /// the ontology was not pure EL.)
    pub(crate) incomplete: bool,
    /// The verdict is guaranteed complete: answered on the pure-EL fast path,
    /// nothing gave up, and conversion dropped no axiom (#540): neither the
    /// input's nor the query expression's own. The stderr `warning:` names
    /// those drops (from the probed conversion, so query-side drops too). `false` means a "satisfiable" / "not entailed" is not
    /// proven complete, not that it is wrong (as `classify --json`).
    pub(crate) completeness_guaranteed: bool,
    /// #401: the verdict was reached with the `ABox` seed dropped (it cannot
    /// affect the answer: the ontology and the expression have no nominal), and
    /// the consistency that licensed dropping it rests on a trusted wedge `Sat`
    /// rather than a main-tableau model, as `consistent`'s own answer does. A
    /// wrong `Sat` there could only make the answer a miss, never a false
    /// positive. Separate from `incomplete`; not a give-up.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #369: a positive answer whose KB consistency stayed undetermined (the
    /// consistency wedge stalled or ran past `RUSTDL_QUERY_LAZY_CONSISTENCY_MS`;
    /// on a KB with no individual, #457's bounded main-tableau check ran past
    /// `RUSTDL_CONSISTENCY_FALLBACK_MS`), so it may be the vacuous answer of an
    /// inconsistent KB. Separate from `incomplete`; not a give-up. A KB found
    /// inconsistent is an error instead.
    pub(crate) consistency_undetermined: bool,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    pub(crate) entailed: bool,
}

// `struct_excessive_bools`: each bool is a field of the published JSON schema.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
pub(crate) struct InstancesExprJson {
    pub(crate) schema_version: u32,
    /// At least one instance probe was cut, so members may be missing; those
    /// listed are sound (#363 A2). Give-up only (#395); exit 0, as `instances`.
    /// (Before package 2 this was also `true` whenever the ontology was not pure
    /// EL.)
    pub(crate) incomplete: bool,
    /// #574: at least one cut instance probe hit the tableau's decision-depth limit
    /// (`CeInstances::depth_capped`), where a larger `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or
    /// node cap does not help and the deadline-free probe (`=0`) may. Implies
    /// `incomplete`.
    pub(crate) depth_capped: bool,
    /// The list is guaranteed complete: the instance query ran on the realize
    /// saturation fast path and conversion dropped no axiom (#540). Off it the tableau path's pseudo-model prune is not
    /// proven complete, so this is `false` even when nothing was cut.
    pub(crate) completeness_guaranteed: bool,
    /// #369: the answer was positive and the KB's consistency stayed
    /// undetermined (the consistency wedge stalled or was cut), so it may be the
    /// vacuous answer of an inconsistent KB. Separate from `incomplete`; not a
    /// give-up. A KB found inconsistent is an error instead.
    pub(crate) consistency_undetermined: bool,
    /// #493: an instance probe ended in `NoVerdict` as an internal limit, likely a
    /// reasoner bug (#449): a probe with no deadline (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`),
    /// or one whose deterministic saturation stalled, budgeted or not (#475). The command
    /// also prints a "likely a reasoner bug" note on stderr and exits 3, after this JSON.
    /// The probe took its sound default, so `incomplete` is set too. A plain depth cut,
    /// deadline or node cap does not set it. #493: a stall of the #369 lazy consistency
    /// check's own probe (after a positive answer) sets it with `consistency_undetermined`.
    pub(crate) no_verdict: bool,
    pub(crate) instances: Vec<String>,
}

#[must_use]
pub(crate) fn build_sat_expr_json(v: &owl_dl_reasoner::CeVerdict) -> SatExprJson {
    SatExprJson {
        schema_version: SCHEMA_VERSION,
        incomplete: v.incomplete(),
        completeness_guaranteed: v.completeness_guaranteed(),
        relies_on_trusted_consistency: v.relies_on_trusted_consistency(),
        consistency_undetermined: v.consistency_undetermined(),
        no_verdict: v.no_verdict(),
        satisfiable: v.holds(),
    }
}

#[must_use]
pub(crate) fn build_subclass_expr_json(v: &owl_dl_reasoner::CeVerdict) -> SubclassExprJson {
    SubclassExprJson {
        schema_version: SCHEMA_VERSION,
        incomplete: v.incomplete(),
        completeness_guaranteed: v.completeness_guaranteed(),
        relies_on_trusted_consistency: v.relies_on_trusted_consistency(),
        consistency_undetermined: v.consistency_undetermined(),
        no_verdict: v.no_verdict(),
        entailed: v.holds(),
    }
}

#[must_use]
pub(crate) fn build_instances_expr_json(r: &owl_dl_reasoner::CeInstances) -> InstancesExprJson {
    let mut instances = r.individuals().to_vec();
    instances.sort();
    InstancesExprJson {
        schema_version: SCHEMA_VERSION,
        incomplete: r.incomplete(),
        depth_capped: r.depth_capped(),
        completeness_guaranteed: r.completeness_guaranteed(),
        consistency_undetermined: r.consistency_undetermined(),
        no_verdict: r.no_verdict(),
        instances,
    }
}

#[must_use]
pub(crate) fn build_classify_json(
    h: &Classification,
    dropped: BTreeMap<String, u64>,
) -> ClassifyJson {
    let stats = h.stats();

    let mut unsatisfiable: Vec<String> = h
        .unsatisfiable_classes()
        .into_iter()
        .map(str::to_owned)
        .collect();
    unsatisfiable.sort();

    // Equivalence groups: for each class, its equivalence peers; canonicalise
    // by sorting each group and deduping groups (a group is emitted once).
    // Unsatisfiable classes are EXCLUDED: they are all mutually equivalent
    // (≡ ⊥), so `equivalent_classes` returns the whole unsat set for any of
    // them — a spurious peer group. They belong in `unsatisfiable` (the
    // bottom node), not in `equivalent_groups`.
    let unsat_set: std::collections::HashSet<&str> =
        unsatisfiable.iter().map(String::as_str).collect();
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in h.classes() {
        if seen.contains(c) || unsat_set.contains(c.as_str()) {
            continue;
        }
        let mut group: Vec<String> = h
            .equivalent_classes(c)
            .into_iter()
            .filter(|e| !unsat_set.contains(e))
            .map(str::to_owned)
            .collect();
        if !group.iter().any(|g| g == c) {
            group.push(c.clone());
        }
        group.sort();
        group.dedup();
        for g in &group {
            seen.insert(g.clone());
        }
        if group.len() > 1 {
            groups.push(group);
        }
    }
    groups.sort();

    // Unsatisfiable subjects are excluded here for the same reason they are
    // excluded from `equivalent_groups` just above: `≡ ⊥` makes their parent
    // set spurious, and they belong in `unsatisfiable`. See
    // `Classification::taxonomy_direct_subsumers`.
    let mut direct_subsumptions: Vec<[String; 2]> = Vec::new();
    for c in h.classes() {
        for sup in h.taxonomy_direct_subsumers(c) {
            direct_subsumptions.push([c.clone(), sup.to_owned()]);
        }
    }
    direct_subsumptions.sort();
    direct_subsumptions.dedup();

    ClassifyJson {
        schema_version: SCHEMA_VERSION,
        consistent: !stats.inconsistent,
        incomplete: deadline_flags(&stats).0,
        prep_timed_out: deadline_flags(&stats).1,
        conversion_timed_out: deadline_flags(&stats).2,
        parse_timed_out: false,
        consistency_undetermined: stats.consistency_undetermined && !stats.inconsistent,
        relies_on_trusted_consistency: stats.relies_on_trusted_consistency,
        trusted_sat_refutations: trusted_sat_risk(&stats),
        completeness_guaranteed: h.completeness_guaranteed(),
        wedge_hierarchy_blind: stats.wedge_hierarchy_blind,
        wedge_clauses_refused: stats.wedge_clauses_refused,
        derived_same_undecided: stats.derived_same_undecided,
        matrix_closure_added_pairs: stats.matrix_closure_added_pairs,
        wedge_merge_churn_stops: stats.wedge_merge_churn_stops,
        consistency_wedge_stalled: stats.consistency_wedge_stalled,
        consistency_wedge_merge_churn_stops: stats.consistency_wedge_merge_churn_stops,
        stalled_wedge_fallback: stats
            .stalled_wedge_fallback
            .map(owl_dl_reasoner::StalledWedgeFallback::as_str),
        no_verdict_pairs: stats.no_verdict_pairs,
        depth_capped_pairs: stats.depth_capped_pairs,
        unsatisfiable,
        equivalent_groups: groups,
        direct_subsumptions,
        dropped,
        collapsed_operands: stats.collapsed_operands.into(),
    }
}

/// `classify --json` when the global deadline ran out while parsing (#162,
/// `RUSTDL_PARSE_DEADLINE`). Nothing was read, so the answer is empty, which is
/// trivially sound, and every cut flag is set. `consistent: true` here means
/// "not checked", as `consistency_undetermined` says.
#[must_use]
pub(crate) fn parse_timed_out_classify_json() -> ClassifyJson {
    ClassifyJson {
        schema_version: SCHEMA_VERSION,
        consistent: true,
        incomplete: true,
        prep_timed_out: true,
        conversion_timed_out: true,
        parse_timed_out: true,
        consistency_undetermined: true,
        relies_on_trusted_consistency: false,
        trusted_sat_refutations: 0,
        completeness_guaranteed: false,
        wedge_hierarchy_blind: false,
        wedge_clauses_refused: 0,
        derived_same_undecided: 0,
        matrix_closure_added_pairs: 0,
        wedge_merge_churn_stops: 0,
        consistency_wedge_stalled: false,
        consistency_wedge_merge_churn_stops: 0,
        stalled_wedge_fallback: None,
        no_verdict_pairs: 0,
        depth_capped_pairs: 0,
        unsatisfiable: Vec::new(),
        equivalent_groups: Vec::new(),
        direct_subsumptions: Vec::new(),
        dropped: BTreeMap::new(),
        collapsed_operands: CollapsedOperandsJson::default(),
    }
}

/// `(incomplete, prep_timed_out, conversion_timed_out)`: something was cut anywhere
/// (a deadline, the node cap, the decision-depth limit, a `NoVerdict` or a tail skip:
/// see `ClassifyJson::incomplete`), a deadline fired during preparation, and one fired
/// during conversion. A cut preparation counts as one timed-out pair and a cut
/// conversion is also a cut preparation, so each flag implies the one before but not
/// the converse.
fn deadline_flags(stats: &owl_dl_reasoner::ClassificationStats) -> (bool, bool, bool) {
    (
        stats.timed_out_pairs > 0,
        stats.prep_timed_out,
        stats.conversion_timed_out,
    )
}

// `fn_params_excessive_bools`: one parameter per `ConsistentJson` schema field.
#[allow(clippy::fn_params_excessive_bools)]
#[must_use]
pub(crate) fn build_consistent_json(
    consistent: bool,
    incomplete: bool,
    relies_on_trusted_consistency: bool,
    wedge_clauses_refused: usize,
    no_verdict: bool,
    dropped: BTreeMap<String, u64>,
    collapsed_operands: CollapsedOperandsJson,
) -> ConsistentJson {
    ConsistentJson {
        schema_version: SCHEMA_VERSION,
        consistent,
        incomplete,
        relies_on_trusted_consistency,
        wedge_clauses_refused,
        no_verdict,
        dropped,
        collapsed_operands,
    }
}

/// Builds `verify-el --json`'s output from already-computed, already-SORTED
/// pieces — this function does no sorting of its own; that discipline lives
/// with the caller (`main.rs`), which is where the raw, possibly
/// non-deterministically-ordered `owl_dl_verify::Verdict` is unpacked. Taking
/// plain primitives here (rather than the `Verdict` type itself) keeps this
/// module from needing to know that type's shape or its ordering hazards.
#[must_use]
pub(crate) fn build_verify_el_json(
    verdict: &'static str,
    domain_size: usize,
    axioms_checked: usize,
    violations: Vec<VerifyElViolationJson>,
    unresolved: Vec<String>,
    dropped: BTreeMap<String, u64>,
) -> VerifyElJson {
    VerifyElJson {
        schema_version: SCHEMA_VERSION,
        verdict,
        domain_size,
        axioms_checked,
        violations,
        unresolved,
        dropped,
    }
}

#[must_use]
pub(crate) fn build_realize_json(
    r: &Realization,
    dropped: BTreeMap<String, u64>,
    saturation_only: bool,
) -> RealizeJson {
    let mut individuals: Vec<IndividualTypesJson> = r
        .individuals()
        .iter()
        .map(|ind| {
            let mut types: Vec<String> = r.entailed_types(ind).to_vec();
            types.sort();
            let mut direct_types: Vec<String> = r.most_specific_types(ind).to_vec();
            direct_types.sort();
            IndividualTypesJson {
                iri: ind.clone(),
                types,
                direct_types,
            }
        })
        .collect();
    individuals.sort_by(|a, b| a.iri.cmp(&b.iri));
    RealizeJson {
        schema_version: SCHEMA_VERSION,
        incomplete: r.incomplete(),
        depth_capped: r.depth_capped(),
        witness_prune_active: r.witness_prune_active(),
        witness_prune_may_drop: r.witness_prune_may_drop(),
        witness_prune_reasons: if r.witness_prune_active() {
            r.witness_prune_reasons().names()
        } else {
            Vec::new()
        },
        direct_types_unverified: r.direct_types_unverified(),
        relies_on_trusted_consistency: r.relies_on_trusted_consistency(),
        no_verdict: r.no_verdict(),
        saturation_only,
        individuals,
        dropped,
    }
}

#[derive(Serialize)]
pub(crate) struct PropHierSide {
    pub(crate) equivalent_groups: Vec<Vec<String>>,
    pub(crate) direct_subsumptions: Vec<[String; 2]>,
}

#[derive(Serialize)]
pub(crate) struct PropHierJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) object_properties: PropHierSide,
    pub(crate) data_properties: PropHierSide,
    /// #389/#390: constructs present that this STRUCTURAL surface ignores and that
    /// can entail a relation it would report (see
    /// `owl_dl_reasoner::property_structural_gaps`). Empty means none of the known
    /// routes occurs, not that the answer is complete. Separate from `incomplete`,
    /// which means only that a deadline cut a probe.
    pub(crate) structural_gaps: Vec<String>,
}

fn side(c: &PropertyClassification) -> PropHierSide {
    let mut ds: Vec<[String; 2]> = c
        .direct_subsumptions()
        .iter()
        .map(|(a, b)| [a.clone(), b.clone()])
        .collect();
    ds.sort();
    let mut eg: Vec<Vec<String>> = c.equivalent_groups().to_vec();
    eg.sort();
    PropHierSide {
        equivalent_groups: eg,
        direct_subsumptions: ds,
    }
}

#[must_use]
pub(crate) fn build_prophier_json(
    obj: &PropertyClassification,
    data: &PropertyClassification,
    structural_gaps: Vec<String>,
) -> PropHierJson {
    PropHierJson {
        schema_version: SCHEMA_VERSION,
        incomplete: false,
        object_properties: side(obj),
        data_properties: side(data),
        structural_gaps,
    }
}

#[must_use]
pub(crate) fn build_disjoint_json(
    classes: &Disjointness,
    obj: Vec<(String, String)>,
    data: Vec<(String, String)>,
    consistency_undetermined: bool,
    structural_gaps: Vec<String>,
) -> DisjointJson {
    let to_arr = |v: Vec<(String, String)>| {
        let mut a: Vec<[String; 2]> = v.into_iter().map(|(x, y)| [x, y]).collect();
        a.sort();
        a
    };
    let mut dc: Vec<[String; 2]> = classes
        .pairs()
        .iter()
        .map(|(x, y)| [x.clone(), y.clone()])
        .collect();
    dc.sort();
    DisjointJson {
        schema_version: SCHEMA_VERSION,
        incomplete: classes.incomplete(),
        disjoint_classes: dc,
        disjoint_object_properties: to_arr(obj),
        disjoint_data_properties: to_arr(data),
        no_verdict: classes.no_verdict(),
        consistency_undetermined,
        structural_gaps,
    }
}

#[derive(Serialize)]
pub(crate) struct IndividualsJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    /// #369: the answer was positive and the KB's consistency stayed
    /// undetermined (the consistency wedge stalled or was cut), so it may be the
    /// vacuous answer of an inconsistent KB. Separate from `incomplete`; not a
    /// give-up. A KB found inconsistent is an error instead.
    pub(crate) consistency_undetermined: bool,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    pub(crate) same_groups: Vec<Vec<String>>,
    pub(crate) different_pairs: Vec<[String; 2]>,
}

#[must_use]
pub(crate) fn build_individuals_json(
    same: &SameIndividuals,
    different: &DifferentIndividuals,
) -> IndividualsJson {
    let mut same_groups: Vec<Vec<String>> = same
        .groups()
        .iter()
        .map(|g| {
            let mut g = g.clone();
            g.sort();
            g
        })
        .collect();
    same_groups.sort();

    let mut different_pairs: Vec<[String; 2]> = different
        .pairs()
        .iter()
        .map(|(a, b)| {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            [lo.clone(), hi.clone()]
        })
        .collect();
    different_pairs.sort();

    IndividualsJson {
        schema_version: SCHEMA_VERSION,
        incomplete: same.incomplete() || different.incomplete(),
        consistency_undetermined: same.consistency_undetermined()
            || different.consistency_undetermined(),
        no_verdict: same.no_verdict() || different.no_verdict(),
        same_groups,
        different_pairs,
    }
}

#[derive(Serialize)]
pub(crate) struct PropertyValuesJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) object_property_values: Vec<[String; 3]>,
    pub(crate) data_property_values: Vec<[String; 5]>,
    /// #369: a non-empty answer whose KB consistency stayed undetermined (the
    /// consistency wedge stalled or ran past `RUSTDL_QUERY_LAZY_CONSISTENCY_MS`),
    /// so it may be the vacuous answer of an inconsistent KB. Separate from
    /// `incomplete`; not a give-up. A KB found inconsistent is an error instead.
    pub(crate) consistency_undetermined: bool,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    /// #389/#390: constructs present that the data half of this surface (the object half reports through `incomplete`) ignores and that
    /// can entail a relation it would report (see
    /// `owl_dl_reasoner::property_structural_gaps`). Empty means none of the known
    /// routes occurs, not that the answer is complete. Separate from `incomplete`,
    /// which means only that a deadline cut a probe.
    pub(crate) structural_gaps: Vec<String>,
}

#[must_use]
pub(crate) fn build_property_values_json(
    obj: &ObjectPropertyValues,
    data: &DataPropertyValues,
    structural_gaps: Vec<String>,
) -> PropertyValuesJson {
    let mut object_property_values: Vec<[String; 3]> = obj
        .triples()
        .iter()
        .map(|(s, p, o)| [s.clone(), p.clone(), o.clone()])
        .collect();
    object_property_values.sort();

    // 5 elements: the language tag is part of the literal's identity (issue
    // #72). Empty string for every non-`rdf:langString` value.
    let mut data_property_values: Vec<[String; 5]> = data
        .quints()
        .iter()
        .map(|(s, p, lex, dt, lang)| [s.clone(), p.clone(), lex.clone(), dt.clone(), lang.clone()])
        .collect();
    data_property_values.sort();

    PropertyValuesJson {
        schema_version: SCHEMA_VERSION,
        incomplete: obj.incomplete() || data.incomplete(),
        object_property_values,
        consistency_undetermined: obj.consistency_undetermined() || data.consistency_undetermined(),
        no_verdict: obj.no_verdict(),
        data_property_values,
        structural_gaps,
    }
}

// `struct_excessive_bools`: as for `ClassifyJson`, the bools are the wire format.
#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct JustifyJson {
    pub(crate) schema_version: u32,
    pub(crate) status: String, // "entailed" | "not-entailed" | "unknown"
    /// A probe gave up (#396): the top-level one (`status: "unknown"`), or one
    /// inside the search (the justifications still entail, but may be non-minimal
    /// or, under `--all`, incomplete). Give-up meaning, as in the other query JSONs.
    pub(crate) incomplete: bool,
    /// #574: a check gave up (`incomplete`) and at least one instance check hit the
    /// tableau's decision-depth limit (`JustifyOutcome::depth_capped`), where a larger
    /// `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` or node cap does not help and the deadline-free
    /// probe (`=0`) may. Implies `incomplete`, but not that the depth cut caused the
    /// give-up. Only instance checks set it; another check's depth limit is `no_verdict`.
    pub(crate) depth_capped: bool,
    pub(crate) enumeration_complete: bool,
    pub(crate) minimal: bool, // all justifications minimal_guaranteed
    pub(crate) laconic: bool,
    /// #429: an oracle call answered "not entailed" on a consistency-wedge `Sat`
    /// alone (the name `prove --json` and `consistent --json` use). With
    /// `status: "not-entailed"` that verdict rests on it; with `"entailed"` only
    /// minimality can. Not a give-up. See `JustifyOutcome::relies_on_trusted_consistency`.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    pub(crate) justifications: Vec<JustificationJson>,
}

#[derive(Serialize)]
pub(crate) struct JustificationJson {
    pub(crate) ofn: String, // self-contained OFN ontology document
}

/// Render a set of axioms as a self-contained OFN ontology document: a fresh
/// `SetOntology` holding exactly those axioms (no more, no fewer —
/// anti-fabrication), written with the SOURCE ontology's `PrefixMapping` so
/// prefixes round-trip on reparse. Shared by `justify --json`'s
/// per-justification rendering and `prove --json`'s per-node/fallback
/// rendering.
fn axioms_to_ofn_doc(axioms: &[Component<RcStr>], pm: &PrefixMapping) -> String {
    let mut so: SetOntology<RcStr> = SetOntology::new();
    for ax in axioms {
        so.insert(ax.clone());
    }
    let cmo: RcComponentMappedOntology = so.into();
    let mut buf: Vec<u8> = Vec::new();
    write_ofn(&mut buf, &cmo, Some(pm)).expect(
        "writing a justification's axioms (no OntologyID, single ontology) to an in-memory \
         buffer cannot fail",
    );
    String::from_utf8(buf).expect("horned-owl's OFN writer emits valid UTF-8")
}

/// Build the `justify --json` payload. `enumeration_complete` reflects
/// whether the outcome's justifications are the FULL set of minimal
/// justifications (always `true` for the default single-justification query;
/// for `--all`, `false` when the `--max` cap was hit or a probe gave up — see
/// the call site).
#[must_use]
pub(crate) fn build_justify_json(
    outcome: &JustifyOutcome<RcStr>,
    pm: &PrefixMapping,
    laconic: bool,
    enumeration_complete: bool,
) -> JustifyJson {
    let justs = &outcome.justifications;
    let minimal = justs.iter().all(|j| j.minimal_guaranteed);
    let justifications = justs
        .iter()
        .map(|j| JustificationJson {
            ofn: axioms_to_ofn_doc(&j.axioms, pm),
        })
        .collect();
    JustifyJson {
        schema_version: SCHEMA_VERSION,
        status: match outcome.status {
            Verdict::Entailed => "entailed",
            Verdict::NotEntailed => "not-entailed",
            Verdict::Unknown => "unknown",
        }
        .to_owned(),
        incomplete: outcome.incomplete,
        depth_capped: outcome.depth_capped,
        enumeration_complete,
        minimal,
        laconic,
        relies_on_trusted_consistency: outcome.relies_on_trusted_consistency,
        no_verdict: outcome.no_verdict,
        justifications,
    }
}

// ---------------------------------------------------------------------------
// `prove --json`
// ---------------------------------------------------------------------------

// Each bool is its own field of the `prove --json` contract (docs/json-schema.md),
// read by external consumers; folding them into an enum would change the schema.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
pub(crate) struct ProveJson {
    pub(crate) schema_version: u32,
    /// `true` iff the entailment holds. `false` is also the sound direction of a
    /// give-up: check `status` / `incomplete` (#402).
    pub(crate) entailed: bool,
    /// `"entailed" | "not-entailed" | "unknown"` (#402; justify's vocabulary).
    /// `"unknown"` (exit 3): the reasoner check gave up before deciding.
    pub(crate) status: String,
    /// A reasoner check gave up (#402): the top level (`status: "unknown"`), or
    /// the justification fallback (its justification may not be minimal, or
    /// none was found). Give-up meaning, as in the other query JSONs.
    pub(crate) incomplete: bool,
    /// #401: `status: "not-entailed"` was answered without the `ABox`, relying
    /// on a consistency verdict that rests on a trusted wedge `Sat` (not a
    /// main-tableau model). Not a give-up; separate from `incomplete`. Always
    /// `false` for the other statuses.
    pub(crate) relies_on_trusted_consistency: bool,
    /// #428: the ontology was found inconsistent, so the entailment holds
    /// vacuously and `justification_fallback` is the inconsistency core (#456),
    /// as `rustdl justify <file> inconsistent` searches for. `false` does not prove
    /// consistency.
    pub(crate) kb_inconsistent: bool,
    /// #462/#475: a probe ended in `NoVerdict` as an internal limit, likely a reasoner
    /// bug (#449): a probe with NO deadline, or one whose deterministic saturation
    /// stalled, budgeted or not. The command also prints a "likely a reasoner bug" note
    /// on stderr and exits 3. It does NOT mean "any `NoVerdict`": on a budgeted probe that
    /// did not stall, a `NoVerdict` is the routine decision-depth cut (#288) and sets
    /// only `incomplete`. A SEPARATE field from `incomplete` (#395), which is set too for
    /// a probe of the answer. #493: where the command runs the #369 lazy consistency
    /// check after a positive answer, a stall of that check's own probe sets this field
    /// with `consistency_undetermined` instead (the answer itself was decided); the
    /// light route's opt-in tail is not covered.
    pub(crate) no_verdict: bool,
    pub(crate) has_proof: bool,
    /// `true` when every proof step cites the axiom its rule applies (or needs none) and
    /// every premise has a sub-proof; `false` when some step cannot be completed from
    /// the cited axioms; `null` when there is no proof.
    pub(crate) proof_complete: Option<bool>,
    pub(crate) proof: Option<ProofNodeJson>,
    /// OFN ontology document (present iff `has_proof` is `false` and
    /// `entailed` is `true`).
    pub(crate) justification_fallback: Option<String>,
    /// Present iff `justification_fallback` is: `true` when that justification is
    /// guaranteed minimal. An EMPTY fallback with `justification_minimal: true`
    /// is a tautology (#427: `C ⊑ owl:Thing`, `owl:Nothing ⊑ C`), which holds
    /// with no axioms at all, unlike an empty fallback with `incomplete: true`,
    /// where the search gave up (#402).
    pub(crate) justification_minimal: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct ProofNodeJson {
    /// OFN ontology document containing the single derived axiom this node
    /// proves.
    pub(crate) conclusion: String,
    /// `ElRule` name (its `Display` impl — matches the text renderer).
    pub(crate) rule: String,
    /// Source axioms used at this step, each its own OFN ontology document. #586: each is
    /// a component of the source ontology as written (not conversion's normalised form),
    /// so a consumer can look it up literally.
    pub(crate) axioms: Vec<String>,
    /// #539: axioms used at this step that conversion added from the OWL semantics
    /// (e.g. `∃owl:bottomObjectProperty.⊤ ⊑ ⊥`), not taken from the source; each its
    /// own OFN ontology document. Kept apart from `axioms` so a consumer never reads
    /// them as the user's. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) builtin_axioms: Vec<String>,
    /// #586: axioms used at this step that a later conversion pass built (from one or
    /// several source axioms), so no source axiom is their written form;
    /// each rendered from rustdl's internal form, its own OFN ontology document. Never
    /// listed in `axioms`. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) derived_axioms: Vec<String>,
    /// The rule applies an axiom but none was recorded for this step.
    pub(crate) axiom_missing: bool,
    pub(crate) premises: Vec<ProofNodeJson>,
    /// Premises this step used that have no recorded derivation, each a one-axiom OFN
    /// document like `conclusion`.
    pub(crate) unproved_premises: Vec<String>,
}

/// Resolve a class id to a horned-owl `ClassExpression`: a named class if
/// `id` is within the source vocabulary, else its `SyntheticDef` expansion
/// (mirrors `owl_dl_saturation::proof::render_class_expanded`/
/// `render_synthetic_def`, but building real model objects instead of a
/// display string, so the result can be written as OFN).
///
/// # Errors
/// Returns an error if `id` is neither a named-vocabulary class nor has a
/// `SyntheticDef` record. This should never happen for a proof the saturator
/// itself produced (every synthetic `ClassId` it can mention is registered in
/// `synthetic_defs` at construction time), but if a future saturator change
/// ever broke that invariant, silently fabricating an opaque
/// `urn:rustdl-synthetic:<idx>` class into user-facing proof JSON would be
/// far worse than failing loudly — so this fails instead of fabricating.
fn class_id_to_class_expression(
    id: ClassId,
    vocab: &Vocabulary,
    defs: &HashMap<ClassId, SyntheticDef>,
    build: &Build<RcStr>,
) -> Result<ClassExpression<RcStr>> {
    let idx = id.index() as usize;
    if idx < vocab.num_classes() {
        return Ok(ClassExpression::Class(build.class(vocab.class_iri(id))));
    }
    if let Some(def) = defs.get(&id) {
        return synthetic_def_to_class_expression(def, vocab, defs, build);
    }
    bail!(
        "internal error rendering proof JSON: class id {idx} has no vocabulary entry and no \
         synthetic_defs record (proof-rendering invariant violated — every synthetic ClassId a \
         saturator proof can mention should be registered in synthetic_defs at construction \
         time; refusing to fabricate an opaque class into the proof output)"
    );
}

fn synthetic_def_to_class_expression(
    def: &SyntheticDef,
    vocab: &Vocabulary,
    defs: &HashMap<ClassId, SyntheticDef>,
    build: &Build<RcStr>,
) -> Result<ClassExpression<RcStr>> {
    Ok(match def {
        SyntheticDef::TseitinConj(bodies) => {
            let mut parts: Vec<ClassExpression<RcStr>> = bodies
                .iter()
                .map(|&b| class_id_to_class_expression(b, vocab, defs, build))
                .collect::<Result<Vec<_>>>()?;
            if parts.len() == 1 {
                parts.remove(0)
            } else {
                ClassExpression::ObjectIntersectionOf(parts)
            }
        }
        SyntheticDef::ExistMarkerOneWay { role, body }
        | SyntheticDef::ExistMarkerEquiv { role, body } => ClassExpression::ObjectSomeValuesFrom {
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(class_id_to_class_expression(*body, vocab, defs, build)?),
        },
        SyntheticDef::NominalKey(ind) => ClassExpression::ObjectOneOf(vec![Individual::Named(
            individual_id_to_named(*ind, vocab, build),
        )]),
        SyntheticDef::MaxKey { n, role } => ClassExpression::ObjectMaxCardinality {
            n: *n,
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(ClassExpression::ObjectIntersectionOf(vec![])), // unqualified: ⊤
        },
        SyntheticDef::MinKey { n, role, body } => ClassExpression::ObjectMinCardinality {
            n: *n,
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(class_id_to_class_expression(*body, vocab, defs, build)?),
        },
        SyntheticDef::ForallKey { role, members } => {
            let mems: Vec<Individual<RcStr>> = members
                .iter()
                .map(|&ind| Individual::Named(individual_id_to_named(ind, vocab, build)))
                .collect();
            ClassExpression::ObjectAllValuesFrom {
                ope: role_id_to_ope(*role, vocab, build),
                bce: Box::new(ClassExpression::ObjectOneOf(mems)),
            }
        }
        SyntheticDef::ForallAtomicKey { role, filler } => ClassExpression::ObjectAllValuesFrom {
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(class_id_to_class_expression(*filler, vocab, defs, build)?),
        },
        // #435: the ⊤-witness is the set of `R`-successors, `∃R⁻.⊤`.
        SyntheticDef::TopWitness { role } => ClassExpression::ObjectSomeValuesFrom {
            ope: horned_owl::model::ObjectPropertyExpression::InverseObjectProperty(
                build.object_property(vocab.role_iri(*role)),
            ),
            bce: Box::new(ClassExpression::Class(
                build.class(owl_dl_core::OWL_THING_IRI),
            )),
        },
        // #436: the trigger-position ⊤-marker `∃R.⊤`.
        SyntheticDef::TopMarker { role } => ClassExpression::ObjectSomeValuesFrom {
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(ClassExpression::Class(
                build.class(owl_dl_core::OWL_THING_IRI),
            )),
        },
        // #444/#445: the shared ⊤ key `K⊤` is the whole domain.
        SyntheticDef::TopKey => ClassExpression::Class(build.class(owl_dl_core::OWL_THING_IRI)),
        // Not reached in practice: `DKey` classes are interned as real named
        // vocabulary entries at conversion time (`owl_dl_core::convert`'s
        // `urn:rustdl-dkey:` classes), so they resolve via the `vocab` branch
        // above, never via `synthetic_defs`. Kept for match exhaustiveness.
        // Unlike the fallback in `class_id_to_class_expression`, this arm
        // renders GENUINE content if it ever fires: `urn:rustdl-dkey:<suffix>`
        // is the exact IRI convention `owl_dl_core::convert`'s real DKey
        // vocabulary entries use (Phase D6), not a fabricated marker — so it
        // is deliberately left as-is rather than hardened into an error.
        SyntheticDef::DKey(iri_suffix) => {
            ClassExpression::Class(build.class(format!("urn:rustdl-dkey:{iri_suffix}")))
        }
    })
}

fn role_id_to_ope(
    role: RoleId,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> horned_owl::model::ObjectPropertyExpression<RcStr> {
    horned_owl::model::ObjectPropertyExpression::ObjectProperty(
        build.object_property(vocab.role_iri(role)),
    )
}

fn individual_id_to_named(
    id: IndividualId,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> NamedIndividual<RcStr> {
    build.named_individual(vocab.individual_iri(id))
}

/// Render a `DerivedFact` (a saturator proof-node conclusion) as the
/// horned-owl axiom it faithfully represents — always a `SubClassOf`
/// (`Exist` is `SubClassOf(sub, ObjectSomeValuesFrom(role, target))`,
/// `Unsat` is `SubClassOf(class, owl:Nothing)`).
fn derived_fact_to_component(
    fact: &owl_dl_reasoner::DerivedFact,
    vocab: &Vocabulary,
    defs: &HashMap<ClassId, SyntheticDef>,
    build: &Build<RcStr>,
) -> Result<Component<RcStr>> {
    Ok(match fact {
        owl_dl_reasoner::DerivedFact::Sub(s, p) => Component::SubClassOf(SubClassOf {
            sub: class_id_to_class_expression(*s, vocab, defs, build)?,
            sup: class_id_to_class_expression(*p, vocab, defs, build)?,
        }),
        owl_dl_reasoner::DerivedFact::Exist(s, r, t) => Component::SubClassOf(SubClassOf {
            sub: class_id_to_class_expression(*s, vocab, defs, build)?,
            sup: ClassExpression::ObjectSomeValuesFrom {
                ope: role_id_to_ope(*r, vocab, build),
                bce: Box::new(class_id_to_class_expression(*t, vocab, defs, build)?),
            },
        }),
        owl_dl_reasoner::DerivedFact::Unsat(c) => Component::SubClassOf(SubClassOf {
            sub: class_id_to_class_expression(*c, vocab, defs, build)?,
            sup: ClassExpression::Class(build.class(OWL_NOTHING)),
        }),
    })
}

/// #586: where a proof's cited IR axioms came from in the source: see
/// [`owl_dl_core::provenance`].
pub(crate) struct Citations<'a> {
    literal:
        HashMap<owl_dl_core::ontology::Axiom, &'a horned_owl::model::AnnotatedComponent<RcStr>>,
}

impl<'a> Citations<'a> {
    pub(crate) fn new(src: &'a SetOntology<RcStr>, internal: &InternalOntology) -> Self {
        Self {
            literal: owl_dl_core::provenance::literal_sources(src, internal),
        }
    }

    /// The source component `ax` is the stored form of, if one is: directly, or, for a
    /// piece of a decomposed chain, through the chain it came from.
    fn source_of(
        &self,
        ax: &owl_dl_core::ontology::Axiom,
        internal: &InternalOntology,
    ) -> Option<&'a Component<RcStr>> {
        self.literal
            .get(ax)
            .or_else(|| {
                let chain = owl_dl_core::provenance::decomposed_chain_origin(internal, ax)?;
                self.literal.get(&chain)
            })
            .map(|ac| &ac.component)
    }
}

/// Recursively build a `ProofNodeJson` from a `ProofNode`: the conclusion
/// and each cited source axiom are each rendered as their own self-contained
/// OFN ontology document.
fn build_proof_node_json(
    node: &owl_dl_reasoner::ProofNode,
    internal: &InternalOntology,
    cites: &Citations<'_>,
    defs: &HashMap<ClassId, SyntheticDef>,
    pm: &PrefixMapping,
    build: &Build<RcStr>,
) -> Result<ProofNodeJson> {
    let vocab = &internal.vocabulary;
    let conclusion_component = derived_fact_to_component(&node.conclusion, vocab, defs, build)?;
    let conclusion = axioms_to_ofn_doc(std::slice::from_ref(&conclusion_component), pm);

    // `node.axiom_refs` index into `internal.axioms` (the same
    // `InternalOntology` `prove_entailment_rcstr` converted the source
    // ontology into) — resolved via `owl_dl_core::axiom_to_component`, the
    // same reverse-conversion `owl_dl_core::convert_back` uses to reverse a
    // whole `InternalOntology`. An out-of-range ref is silently skipped
    // (mirrors the text renderer's `if let Some(ax) = ...` at the CLI's
    // axiom-provenance printout); it should not occur for a proof the
    // saturator itself produced.
    let render = |ax: &owl_dl_core::ontology::Axiom| {
        let component = owl_dl_core::axiom_to_component(ax, internal, build);
        axioms_to_ofn_doc(std::slice::from_ref(&component), pm)
    };
    let (builtin_refs, source_refs): (Vec<_>, Vec<_>) = node
        .axiom_refs
        .iter()
        .filter_map(|r| internal.axioms.get(r.0))
        .partition(|ax| internal.builtin.contains(ax));
    // #586: a cited axiom is rendered as the source component it came from, as written,
    // never as conversion stored it (which Protégé's literal lookup rejects). One with no
    // single source (a later pass derived it) goes to `derived_axioms`.
    let mut axioms: Vec<String> = Vec::new();
    let mut derived_axioms: Vec<String> = Vec::new();
    for ax in source_refs {
        match cites.source_of(ax, internal) {
            Some(component) => {
                let doc = axioms_to_ofn_doc(std::slice::from_ref(component), pm);
                if !axioms.contains(&doc) {
                    axioms.push(doc);
                }
            }
            None => derived_axioms.push(render(ax)),
        }
    }
    let builtin_axioms: Vec<String> = builtin_refs.into_iter().map(render).collect();

    let premises = node
        .premises
        .iter()
        .map(|p| build_proof_node_json(p, internal, cites, defs, pm, build))
        .collect::<Result<Vec<_>>>()?;
    let unproved_premises = node
        .unproved_premises
        .iter()
        .map(|f| {
            let c = derived_fact_to_component(f, vocab, defs, build)?;
            Ok(axioms_to_ofn_doc(std::slice::from_ref(&c), pm))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ProofNodeJson {
        conclusion,
        rule: node.rule.to_string(),
        axiom_missing: node.rule.needs_axiom()
            && axioms.is_empty()
            && builtin_axioms.is_empty()
            && derived_axioms.is_empty(),
        axioms,
        builtin_axioms,
        derived_axioms,
        premises,
        unproved_premises,
    })
}

/// Build the `prove --json` payload. `internal` must be the same ontology
/// `result` was derived from (re-converted by the caller — see
/// `prove_entailment_rcstr`'s own internal conversion).
///
/// # Errors
/// Returns an error if rendering the proof tree hits the
/// should-never-happen `ClassId`-resolution invariant violation described on
/// `class_id_to_class_expression` — propagated here instead of silently
/// emitting fabricated content, so `prove --json` exits with a real error in
/// that case.
/// No node is missing an axiom or a premise's sub-proof. Read off the rendered JSON
/// so `proof_complete` agrees with the per-node fields by construction (an axiom ref
/// that fails to resolve counts as missing there too).
fn proof_is_complete(node: &ProofNodeJson) -> bool {
    !node.axiom_missing
        && node.unproved_premises.is_empty()
        && node.premises.iter().all(proof_is_complete)
}

pub(crate) fn build_prove_json(
    result: &ProveEntailmentResult,
    flags: owl_dl_reasoner::ProveFlags,
    src: &SetOntology<RcStr>,
    internal: &InternalOntology,
    pm: &PrefixMapping,
) -> Result<ProveJson> {
    let verdict = |entailed: bool, status: &str| ProveJson {
        schema_version: SCHEMA_VERSION,
        entailed,
        status: status.to_owned(),
        incomplete: flags.incomplete,
        relies_on_trusted_consistency: flags.relies_on_trusted_consistency,
        kb_inconsistent: flags.kb_inconsistent,
        no_verdict: flags.no_verdict,
        has_proof: false,
        proof_complete: None,
        proof: None,
        justification_fallback: None,
        justification_minimal: None,
    };
    Ok(match result {
        ProveEntailmentResult::SaturatorProof(data) => {
            let build: Build<RcStr> = Build::new_rc();
            let cites = Citations::new(src, internal);
            let proof = build_proof_node_json(
                &data.root,
                internal,
                &cites,
                &data.trace.synthetic_defs,
                pm,
                &build,
            )?;
            ProveJson {
                has_proof: true,
                proof_complete: Some(proof_is_complete(&proof)),
                proof: Some(proof),
                ..verdict(true, "entailed")
            }
        }
        ProveEntailmentResult::JustificationFallback(j) => ProveJson {
            justification_fallback: Some(axioms_to_ofn_doc(&j.axioms, pm)),
            justification_minimal: Some(j.minimal_guaranteed),
            ..verdict(true, "entailed")
        },
        ProveEntailmentResult::NotEntailed => verdict(false, "not-entailed"),
        ProveEntailmentResult::Unknown => verdict(false, "unknown"),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    /// A pair cut, a preparation cut and a conversion cut all set `incomplete`;
    /// only the last two set `prep_timed_out`, and only the last sets
    /// `conversion_timed_out`. (The end-to-end test in `json_output.rs` cannot
    /// make a pair time out deterministically, so it sees the flags co-vary.)
    #[test]
    fn a_pair_cut_is_incomplete_without_prep_timed_out() {
        let pair_cut = owl_dl_reasoner::ClassificationStats {
            timed_out_pairs: 3,
            ..Default::default()
        };
        assert_eq!(deadline_flags(&pair_cut), (true, false, false));
        let prep_cut = owl_dl_reasoner::ClassificationStats {
            timed_out_pairs: 1,
            prep_timed_out: true,
            ..Default::default()
        };
        assert_eq!(deadline_flags(&prep_cut), (true, true, false));
        let conversion_cut = owl_dl_reasoner::ClassificationStats {
            timed_out_pairs: 1,
            prep_timed_out: true,
            conversion_timed_out: true,
            ..Default::default()
        };
        assert_eq!(deadline_flags(&conversion_cut), (true, true, true));
        assert_eq!(
            deadline_flags(&owl_dl_reasoner::ClassificationStats::default()),
            (false, false, false)
        );
    }

    fn classify_ofn(src: &str) -> owl_dl_reasoner::Classification {
        let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
            &mut Cursor::new(src.to_owned()),
            ParserConfiguration::default(),
        )
        .unwrap();
        owl_dl_reasoner::classify(&onto).unwrap()
    }

    #[test]
    fn classify_json_is_sorted_and_carries_verdict() {
        // B ⊑ A, C ⊑ B ⇒ direct B⊑A, C⊑B; consistent; nothing unsat.
        let h = classify_ofn(
            r"Prefix(:=<http://ex/#>)
              Ontology(<http://ex/>
                Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
                SubClassOf(:B :A) SubClassOf(:C :B))",
        );
        let j = build_classify_json(&h, BTreeMap::new());
        assert_eq!(j.schema_version, 1);
        assert!(j.consistent);
        assert!(!j.incomplete);
        assert!(j.unsatisfiable.is_empty());
        // direct edges present, sorted by (sub, sup):
        assert!(
            j.direct_subsumptions
                .contains(&["http://ex/#B".to_owned(), "http://ex/#A".to_owned()])
        );
        assert!(
            j.direct_subsumptions
                .contains(&["http://ex/#C".to_owned(), "http://ex/#B".to_owned()])
        );
        // sorted invariant:
        let mut sorted = j.direct_subsumptions.clone();
        sorted.sort();
        assert_eq!(j.direct_subsumptions, sorted);
    }

    /// #539: a proof step that cites an axiom conversion added from the OWL semantics
    /// (`InternalOntology::builtin`) lists it under `builtin_axioms`, never under
    /// `axioms` (the source's), and the step does not read as `axiom_missing`. No
    /// saturator proof cites one today (it records no derivation for `∃r.X ⊑ ⊥` through
    /// the role hierarchy, with or without a user-written axiom), so the step is built
    /// by hand.
    #[test]
    fn a_proof_step_citing_a_builtin_axiom_keeps_it_out_of_the_source_axioms() {
        let src = "Prefix(:=<http://ex/#>)
            Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
            Ontology(<http://ex/>
              Declaration(Class(:C)) Declaration(Class(:D))
              SubClassOf(:C ObjectSomeValuesFrom(owl:bottomObjectProperty :D)))";
        let (onto, pm): (SetOntology<RcStr>, _) = read_ofn(
            &mut Cursor::new(src.to_owned()),
            ParserConfiguration::default(),
        )
        .unwrap();
        let internal = owl_dl_core::convert::convert_ontology(&onto).unwrap();
        assert_eq!(internal.builtin.len(), 1, "{:?}", internal.builtin);
        let builtin_idx = internal
            .axioms
            .iter()
            .position(|ax| internal.builtin.contains(ax))
            .unwrap();
        let user_idx = (0..internal.axioms.len())
            .find(|&i| {
                i != builtin_idx
                    && matches!(
                        internal.axioms[i],
                        owl_dl_core::ontology::Axiom::SubClassOf { .. }
                    )
            })
            .unwrap();
        let c = internal.vocabulary.class_id("http://ex/#C").unwrap();
        let node = owl_dl_reasoner::ProofNode {
            conclusion: owl_dl_reasoner::DerivedFact::Unsat(c),
            rule: owl_dl_reasoner::ElRule::ToldUnsat,
            axiom_refs: vec![
                owl_dl_reasoner::AxiomRef(builtin_idx),
                owl_dl_reasoner::AxiomRef(user_idx),
            ],
            premises: Vec::new(),
            unproved_premises: Vec::new(),
        };
        let build = Build::new();
        let cites = Citations::new(&onto, &internal);
        let j =
            build_proof_node_json(&node, &internal, &cites, &HashMap::new(), &pm, &build).unwrap();
        assert_eq!(j.axioms.len(), 1, "source axioms: {:?}", j.axioms);
        assert!(j.axioms[0].contains(":D"), "{}", j.axioms[0]);
        assert_eq!(j.builtin_axioms.len(), 1, "{:?}", j.builtin_axioms);
        // `axiom_to_component` writes ⊤ / ⊥ as `ObjectIntersectionOf()` /
        // `ObjectUnionOf()`; only the role is checked here.
        assert!(
            j.builtin_axioms[0].contains("ObjectSomeValuesFrom(owl:bottomObjectProperty"),
            "{}",
            j.builtin_axioms[0]
        );
        assert!(!j.axiom_missing);
        assert!(j.derived_axioms.is_empty(), "{:?}", j.derived_axioms);
        // Sabotage check of the partition itself: with the builtin record emptied, the
        // builtin axiom is no longer kept apart. #586: it still never reads as the
        // source's, since no source component converts to it: it is derived.
        let mut stripped = internal.clone();
        stripped.builtin.clear();
        let j2 =
            build_proof_node_json(&node, &stripped, &cites, &HashMap::new(), &pm, &build).unwrap();
        assert_eq!(j2.axioms.len(), 1, "{:?}", j2.axioms);
        assert!(j2.builtin_axioms.is_empty());
        assert_eq!(j2.derived_axioms.len(), 1, "{:?}", j2.derived_axioms);
    }
}
