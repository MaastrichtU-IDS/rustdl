//! Slack must be semantically invisible: the IRI-level closure with slack N
//! is identical to the closure with slack 0, for every N. Slack only moves
//! synthetic ids further up the id space.
#![allow(clippy::unwrap_used)]

use owl_dl_saturation::{SaturateConfig, saturate, saturate_with_config, saturate_with_slack};

mod common;
use common::{closure_as_iri_pairs, load_fixture}; // parses an .ofn fixture into InternalOntology

#[test]
fn slack_does_not_change_the_closure() {
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn"] {
        let internal = load_fixture(fixture);
        let base = closure_as_iri_pairs(&internal, &saturate(&internal));
        for slack in [1usize, 64, 1000] {
            let with = closure_as_iri_pairs(&internal, &saturate_with_slack(&internal, slack));
            assert_eq!(base, with, "fixture {fixture} diverged at slack {slack}");
        }
    }
}

#[test]
fn slack_zero_is_the_default_path() {
    let internal = load_fixture("pizza.ofn");
    let a = closure_as_iri_pairs(&internal, &saturate(&internal));
    let b = closure_as_iri_pairs(&internal, &saturate_with_slack(&internal, 0));
    assert_eq!(a, b);
}

/// Task 3's reviewer flagged that no test covered `record_proofs = true` x
/// `slack > 0` — the combination that hides a second `TseitinAllocator::new`
/// construction site (`mini_tseitin`) behind the proof-recording path. Both
/// allocators must be based at `num_classes + slack`, or the re-simulated
/// provenance describes different synthetic ids than the real run.
#[test]
fn proof_recording_with_slack_matches_the_plain_closure() {
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn"] {
        let internal = load_fixture(fixture);
        let base = closure_as_iri_pairs(&internal, &saturate(&internal));
        let cfg = SaturateConfig {
            record_proofs: true,
            slack: 64,
        };
        let (subs, trace) = saturate_with_config(&internal, &cfg);
        assert!(trace.is_some(), "record_proofs = true must yield a trace");
        assert_eq!(
            base,
            closure_as_iri_pairs(&internal, &subs),
            "fixture {fixture} diverged under record_proofs + slack"
        );
    }
}
