//! Phase 2c — pair_06 canary documenting the saturator-only gap.
//!
//! Loads the HermiT-verified GALEN extract `pair_06.ofn` (committed at
//! Phase 2b.0; see `crates/owl-dl-reasoner/tests/fixtures/phase2b/`)
//! and asserts that the EL saturator MISSES the canonical Phase 2c
//! cluster-C subsumption
//!
//!   <http://example.org/factkb#CongestiveCardiacFailure> ⊑
//!     <http://example.org/factkb#IntrinsicallyPathologicalBodyProcess>
//!
//! This is the gap the Phase 2c functional-role + covering EL+
//! approximation will close. Phase 2c T4 flips the assertion + renames
//! the test from `_misses_target_subsumption` to
//! `_recovers_target_subsumption`. See
//! `docs/superpowers/plans/2026-06-01-phase2c-functional-role-covering.md`
//! for the full plan and
//! `docs/phase2b-galen-pair-analysis.md` §"Pair 06" for the
//! per-pair HermiT trace that motivates the rule.
//!
//! The canary loads the fixture, parses it via `horned-owl`, lowers it
//! to the internal IR via `owl_dl_core::convert::convert_ontology`,
//! runs `owl_dl_saturation::saturate` directly (NOT the full classifier
//! — the wedge/tableau would close this entailment, defeating the
//! point), and asserts the target pair is absent from the saturator's
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
fn phase2c_pair_06_saturator_misses_target_subsumption() {
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

    // GAP-ASSERTING: passes while the saturator misses the entailment.
    // Phase 2c T4 inverts this assertion once the functional-role +
    // covering rule lands.
    assert!(
        !subsumers.contains(ccf, ipbp),
        "Phase 2c canary unexpectedly closed CongestiveCardiacFailure ⊑ \
         IntrinsicallyPathologicalBodyProcess via the saturator alone. Phase \
         2a/2b may have inadvertently covered pair_06 — invert this assertion \
         (drop the leading `!`) and rename to \
         `phase2c_pair_06_saturator_recovers_target_subsumption`. CCF subsumers: \
         {:?}",
        subsumers.subsumers_of(ccf)
    );
}
