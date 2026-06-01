//! Phase 2c — pair_06 canary documenting the saturator's RECOVERY of
//! the canonical cluster-C subsumption via Phase 2d + 2c-redux.
//!
//! Loads the HermiT-verified GALEN extract `pair_06.ofn` (committed at
//! Phase 2b.0; see `crates/owl-dl-reasoner/tests/fixtures/phase2b/`)
//! and asserts that the EL saturator RECOVERS the canonical Phase 2c
//! cluster-C subsumption
//!
//!   <http://example.org/factkb#CongestiveCardiacFailure> ⊑
//!     <http://example.org/factkb#IntrinsicallyPathologicalBodyProcess>
//!
//! ## Recovery path (Phase 2d + Phase 2c-redux combined)
//!
//! The original Phase 2c rule (commit `b83fcd6`, reverted at `cc2019e`)
//! shipped sound + terminating but did NOT recover this pair. The
//! reason was empirical: IntrinsicallyCardiacFunction (ICF, the
//! bridge class between CCF and IPBP via the equivalence chain) had
//! only ONE directly-materialised existential fact in its
//! `facts_by_sub` row at saturation time. Phase 2c's rule keys on
//! `facts_by_sub[X]`, and the saturator propagates *subsumers* (not
//! facts) to subclasses — so even though `ICF ⊑ PathologicalBodyProcess`
//! is derived, the underlying `(PBP, hasPathologicalStatus, pathological)`
//! fact never landed on ICF itself.
//!
//! Phase 2d (commit `b78c5fd`) closed that gap by inheriting
//! existential facts onto subclasses at `process_subsumer` /
//! `push_fact` time. After Phase 2d:
//!
//! 1. `(ICF, hasIntrinsicPathologicalStatus, physiological)` is
//!    inherited from `NAMEDPhysiologicalProcess`.
//! 2. `(ICF, hasPathologicalStatus, pathological)` is inherited
//!    from `PathologicalBodyProcess`.
//! 3. Phase 2a's StatusAttribute-merge fires on ICF (both
//!    `hasIntrinsicPathologicalStatus` and `hasPathologicalStatus`
//!    share the functional super `StatusAttribute`), accumulating
//!    `{physiological, pathological}` into the merged atom set and
//!    emitting `(ICF, StatusAttribute, synthetic)` where `synthetic`
//!    is a class equivalent to `physiological ⊓ pathological`.
//! 4. Phase 2c-redux propagates the merged synthetic back to
//!    `hasIntrinsicPathologicalStatus`:
//!    `(ICF, hasIntrinsicPathologicalStatus, synthetic)`.
//! 5. The existential trigger for `IntrinsicallyPathologicalBodyProcess`
//!    (body roughly: `∃hasIntrinsicPathologicalStatus.pathological`)
//!    matches via target-subsumer propagation: `synthetic ⊑ pathological`
//!    by Tseitin / intersection semantics.
//! 6. `ICF ⊑ IPBP`; hence `CCF ⊑ IPBP` by transitivity through the
//!    equivalence chain.
//!
//! Soundness: Phase 2c's original witness-coincidence argument
//! (`docs/phase2c-fix-target.md` §"Rule design") extends because
//! inherited facts preserve the same model-theoretic witness as the
//! parent's fact (`docs/phase2d-design.md` §Soundness).
//!
//! See also:
//! - `docs/phase2d-design.md` for the propagation mechanism.
//! - `docs/phase2c-fix-target.md` §"Predicted walkthrough on pair_06
//!   (and what actually happened)" for the original empirical
//!   reckoning that motivated Phase 2d.
//! - `docs/hypertableau-dead-ends.md` §15 (resolved by Phase 2d +
//!   2c-redux).
//!
//! This test is kept as a permanent recovery-asserter so a future
//! regression that breaks the inheritance chain (or disables the
//! Phase 2c-redux rule) is caught mechanically.
//!
//! The canary loads the fixture, parses it via `horned-owl`, lowers it
//! to the internal IR via `owl_dl_core::convert::convert_ontology`,
//! runs `owl_dl_saturation::saturate` directly (NOT the full classifier
//! — the wedge/tableau would close this entailment, defeating the
//! point), and asserts the target pair IS PRESENT in the saturator's
//! subsumer closure.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const CCF_IRI: &str = "http://example.org/factkb#CongestiveCardiacFailure";
const IPBP_IRI: &str = "http://example.org/factkb#IntrinsicallyPathologicalBodyProcess";

#[test]
fn phase2c_pair_06_saturator_recovers_target_subsumption_via_phase2d() {
    let onto_path = "tests/fixtures/phase2b/pair_06.ofn";
    let src = std::fs::read_to_string(onto_path).expect("pair_06.ofn readable");
    let mut reader = Cursor::new(src);
    let (set_onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("pair_06 parses");
    let internal = convert_ontology(&set_onto).expect("pair_06 lowers to IR");

    let subsumers = owl_dl_saturation::saturate(&internal);

    let ccf = internal
        .vocabulary
        .class_id(CCF_IRI)
        .expect("CongestiveCardiacFailure declared in pair_06");
    let ipbp = internal
        .vocabulary
        .class_id(IPBP_IRI)
        .expect("IntrinsicallyPathologicalBodyProcess declared in pair_06");

    // RECOVERY-ASSERTING (Phase 2d + Phase 2c-redux combined): the
    // saturator now closes this pair via the chain described in this
    // file's module doc. If this assertion ever starts failing (the
    // saturator no longer recovers the pair), it means a regression
    // broke the inheritance chain (Phase 2d) or disabled the sub-role
    // witness-propagation rule (Phase 2c-redux). Investigate which
    // counter dropped: `phase2d_facts_inherited` (inheritance) or
    // `phase2c_sub_role_propagations` (the rule's emission).
    assert!(
        subsumers.contains(ccf, ipbp),
        "Phase 2d + 2c-redux canary regressed: saturator failed to \
         close CongestiveCardiacFailure ⊑ \
         IntrinsicallyPathologicalBodyProcess. Expected recovery via \
         (1) Phase 2d inheriting both \
         (ICF, hasIntrinsicPathologicalStatus, physiological) and \
         (ICF, hasPathologicalStatus, pathological); (2) Phase 2a's \
         StatusAttribute-merge emitting a merged synthetic; (3) Phase \
         2c-redux propagating that synthetic to \
         hasIntrinsicPathologicalStatus; (4) IPBP's existential \
         trigger matching via target-subsumer propagation. See this \
         file's module doc + docs/phase2d-design.md + \
         docs/phase2c-fix-target.md §'Rule design'. CCF subsumers: {:?}",
        subsumers.subsumers_of(ccf)
    );
}
