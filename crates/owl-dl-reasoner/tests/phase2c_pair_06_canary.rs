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
//! ## Status after Phase 2c T4 (rule shipped, pair_06 NOT recovered)
//!
//! Phase 2c shipped a sub-role witness-propagation rule (see
//! `crates/owl-dl-saturation/src/lib.rs`, Phase 2c block inside
//! `process_fact`). The rule is sound, terminates, and fires 3 times
//! during pair_06 saturation on other classes (ClassId 77, 79, 114) —
//! but it does NOT recover the target `CCF ⊑ IPBP` entailment.
//!
//! Why: IntrinsicallyCardiacFunction (ICF, the bridge class between
//! CCF and IPBP via the equivalence chain) has only ONE directly-
//! materialised existential fact in its `facts_by_sub` row at
//! saturation time (on a single `RoleId`, not on the two sub-roles
//! `hasIntrinsicPathologicalStatus` and `hasPathologicalStatus` that
//! T3's design walkthrough predicted). Phase 2c's rule is a
//! fact-time rule keyed on `facts_by_sub[X]`: the saturator
//! propagates *subsumers* (not facts) to subclasses, so even though
//! `ICF ⊑ PathologicalBodyProcess` is derived, the underlying
//! `(PBP, hasPathologicalStatus, pathological)` fact never lands on
//! ICF itself. The rule's precondition (X has two facts on sub-roles
//! sharing a functional super) is not met for ICF.
//!
//! See `docs/phase2c-fix-target.md`:
//! - §"Predicted walkthrough on pair_06 (and what actually happened)"
//!   for the empirical reckoning.
//! - §"What this design does NOT close" (first bullet, strengthened
//!   in T4) for the general statement of this limitation.
//!
//! This test is kept as a permanent gap-asserter so a future
//! regression (or a future fix that *does* reach ICF) is caught
//! mechanically. T5 measures whether ANY cluster-C pair benefits
//! from the rule.
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
fn phase2c_pair_06_saturator_still_misses_target_subsumption_known_limitation() {
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

    // GAP-ASSERTING (known limitation after Phase 2c T4): the
    // saturator still misses this pair. The Phase 2c sub-role
    // propagation rule shipped (and fires 3 times elsewhere in
    // pair_06's saturation), but ICF — the bridge class between CCF
    // and IPBP — has only one existential fact directly materialised
    // on it, so the rule's precondition is not met for the chain that
    // would reach IPBP. See this file's module doc and
    // `docs/phase2c-fix-target.md` §"Predicted walkthrough on pair_06
    // (and what actually happened)".
    //
    // If this assertion ever starts failing (the saturator DOES
    // recover the pair), it means a later phase closed the gap —
    // celebrate, then invert this assertion and rename to
    // `phase2c_pair_06_saturator_recovers_target_subsumption`.
    assert!(
        !subsumers.contains(ccf, ipbp),
        "Phase 2c canary unexpectedly closed CongestiveCardiacFailure ⊑ \
         IntrinsicallyPathologicalBodyProcess via the saturator alone. \
         A later phase has apparently propagated the missing existential \
         fact onto IntrinsicallyCardiacFunction (or otherwise covered \
         this entailment). Invert this assertion (drop the leading `!`) \
         and rename to \
         `phase2c_pair_06_saturator_recovers_target_subsumption`. \
         CCF subsumers: {:?}",
        subsumers.subsumers_of(ccf)
    );
}
