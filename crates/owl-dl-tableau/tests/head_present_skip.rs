//! #128 — the head-present skip must be an IDENTITY, and must actually fire.
//!
//! `fire_clause` skips the whole body match when the clause's single head atom is
//! `Class(c, X)` and the node already carries `c`. That is sound because
//! `HyperNode::add` is KEEP-FIRST: a repeat add returns false without widening the
//! dep-set and without re-enqueuing, so every binding the skipped match could have
//! produced would have resolved to `NoChange`.
//!
//! # Why the counter assertion is here
//!
//! An on/off comparison alone is worthless if the skip never fires on the fixture —
//! it would pass identically on a build where the feature was deleted. This repo has
//! been bitten by exactly that (`docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`,
//! and the vacuous FP guards in `chain_symmetric_leg.rs`). So the ON arm asserts
//! `head_present_skips > 0` — the non-vacuity signal — and the OFF arm asserts it is
//! 0, which together prove the comparison exercised the code under test.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::clausify_with_stats;
use owl_dl_core::convert::convert_ontology;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult, SearchStats};
use std::io::Cursor;

/// `A ⊑ B`, `A ⊑ D`, `D ⊑ B` — two independent derivations of `B` on the same node.
/// Whichever fires first puts `B` on the node, so the other is guaranteed to hit the
/// already-present head, in ANY worklist order. That order-independence is what makes
/// the `> 0` assertion below stable rather than a coin flip.
const FIXTURE_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Ontology(<http://rustdl.test/headskip>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:D))
    Declaration(Class(:E))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A :B)
    SubClassOf(:A :D)
    SubClassOf(:D :B)
    SubClassOf(:A ObjectSomeValuesFrom(:R :E))
    SubClassOf(:E :B)
)
";

/// UNSAT by a CHAIN of class-headed Horn clauses: `A ⊑ B`, `B ⊑ C`, and `A`/`C`
/// disjoint. Every link is exactly the clause shape the skip inspects, so an
/// over-eager skip never derives `C`, never clashes, and reports `Sat`.
///
/// This fixture exists because the Sat fixture above CANNOT catch that: skipping
/// derivations there changes no verdict, so `identical_verdict_with_skip_on_and_off`
/// passes under a mutation that skips unconditionally. Verified by mutation:
/// `if true || …` makes `unsound_over_skipping_would_flip_this_to_sat` FAIL and
/// leaves every other test in this file green.
const UNSAT_SRC: &str = "Prefix(:=<http://rustdl.test/>)
Ontology(<http://rustdl.test/headskip-unsat>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
    DisjointClasses(:A :C)
)
";

fn run_src(src: &str, root_iri: &str, skip: bool) -> (HyperResult, SearchStats) {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let root = internal
        .vocabulary
        .class_id(root_iri)
        .expect("root declared");
    let (clauses, _) = clausify_with_stats(&internal);
    let mut eng = HyperEngine::new(&clauses, root).with_head_present_skip(skip);
    let verdict = eng.decide(64);
    let stats = eng.stats();
    (verdict, stats)
}

fn run(skip: bool) -> (HyperResult, SearchStats) {
    let mut reader = Cursor::new(FIXTURE_SRC);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let root = internal
        .vocabulary
        .class_id("http://rustdl.test/A")
        .expect("A declared");
    let (clauses, _) = clausify_with_stats(&internal);
    let mut eng = HyperEngine::new(&clauses, root).with_head_present_skip(skip);
    let verdict = eng.decide(64);
    let stats = eng.stats();
    (verdict, stats)
}

#[test]
fn the_skip_actually_fires_on_this_fixture() {
    // NON-VACUITY GUARD. Without this, `identical_verdict_with_skip_on_and_off`
    // would pass on a build where the skip was removed entirely.
    let (_, on) = run(true);
    assert!(
        on.head_present_skips > 0,
        "the head-present skip never fired — the identity test below would be \
         vacuous. Fixture or trigger changed?"
    );
}

#[test]
fn the_counter_is_zero_when_the_skip_is_off() {
    // The other half of non-vacuity: proves the counter tracks the feature and is
    // not incremented on some unrelated path.
    let (_, off) = run(false);
    assert_eq!(
        off.head_present_skips, 0,
        "head_present_skips must be 0 with the skip disabled"
    );
}

#[test]
fn identical_verdict_with_skip_on_and_off() {
    let (v_off, _) = run(false);
    let (v_on, _) = run(true);
    assert_eq!(
        v_off, v_on,
        "the skip is an identity under keep-first `add`; a verdict difference \
         means that premise is broken"
    );
    assert_eq!(v_on, HyperResult::Sat, "fixture is satisfiable");
}

#[test]
fn unsound_over_skipping_would_flip_this_to_sat() {
    // THE SOUNDNESS GUARD, and the discriminator for this whole file. `A` is
    // unsatisfiable only if the `A ⊑ B ⊑ C` chain is actually derived; every link
    // is a single `Class(_, X)` head, so a skip that fired when the head was NOT
    // already present would lose the chain and report `Sat`.
    //
    // Without this test the file is mutation-blind: `if true || has(c)` (skip
    // everything, unsound) leaves the other three tests passing.
    for skip in [false, true] {
        let (verdict, _) = run_src(UNSAT_SRC, "http://rustdl.test/A", skip);
        assert_eq!(
            verdict,
            HyperResult::Unsat,
            "A ⊑ B ⊑ C with A/C disjoint is UNSAT; got {verdict:?} with skip={skip}. \
             A `Sat` here means the skip dropped a derivation it had no right to drop"
        );
    }
}
