//! Acceptance tests for the whole `owl-dl-verify` instrument, run against the
//! real, live D10 defects (issues #80, #81, #82) it was designed to catch.
//!
//! # Why these tests never assert `Violated`
//!
//! Every non-control fixture here demonstrates a genuine rustdl completeness
//! defect: the fragment gate certifies `pure-EL` / `trust_sat sound by
//! construction`, and the engine still drops an entailment. Those defects are
//! filed as GitHub issues #80/#81/#82 and are expected to be FIXED. A test
//! that asserted `Verdict::Violated` directly would then start failing
//! **because the codebase improved** — the mirror image of the
//! `#[ignore]`d-sentinel trap this repo already has a postmortem for
//! (`docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`: a test
//! whose green depends on a bug persisting).
//!
//! So every case here is phrased as the STABLE invariant instead:
//!
//! > On fixture `F` with committed oracle verdict `O`, the instrument must
//! > **not** return `Verified` whenever rustdl's own classification
//! > disagrees with `O`.
//!
//! That holds in both engine states. While a defect is live, `rustdl_agrees`
//! is `false` and the instrument must land on `Violated` (a genuine
//! detection) or `Unresolved` (honest, but weaker coverage — logged, not
//! silently counted as a detection). Once a defect is fixed, rustdl agrees
//! with the oracle, the antecedent `!rustdl_agrees` is false, and this test
//! passes WITHOUT modification — it does not need to be un-`#[ignore]`d or
//! rewritten the way a `Violated`-asserting sentinel would.
//!
//! `rustdl`'s own classification, for the purpose of "does rustdl agree with
//! the oracle", is read via `owl_dl_reasoner::classify_internal` — the actual
//! hybrid classifier a user gets, NOT this crate's `build_model`/`verify`
//! (which is the independent CHECK being tested, and must never be used to
//! grade its own patient). `owl-dl-reasoner` is a dev-dependency only: it is
//! not in `[dependencies]`, so this creates no cycle and `src/` still never
//! imports it (see `src/eval.rs`'s independence contract, guarded by
//! `tests/independence.rs`).
//!
//! Every fixture also carries a committed `<fixture>.oracle` file recording
//! the expected classification and its **provenance** — `konclude-v0.7.0-1138`,
//! `hermit`, `derivation-only` (issue #82's range half: BOTH peers miss it),
//! or `structural` for the uncontested healthy controls. A derivation-only
//! claim says so in the assertion failure message, so nobody who sees this
//! test fail later mistakes it for peer-confirmed.

use owl_dl_core::InternalOntology;
use owl_dl_verify::{Bounds, UnresolvedReason, Verdict};

mod common;
use common::load;

/// One committed expectation about rustdl's classification, read from a
/// `<fixture>.oracle` file — never about `verify_fixture`'s verdict, which is
/// what keeps this an invariant test rather than a `Violated` sentinel (see
/// the module doc above).
#[derive(Debug, PartialEq, Eq)]
enum Claim {
    Unsatisfiable(String),
    Subclass(String, String),
}

#[derive(Debug)]
struct Oracle {
    provenance: String,
    claims: Vec<Claim>,
}

/// Parses a `<fixture>.oracle` file: `#`-prefixed lines and `# ...` trailing
/// comments are stripped, `provenance:` sets the provenance tag, and
/// `unsatisfiable: NAME` / `subclass: SUB SUP` each add one [`Claim`]. `NAME`,
/// `SUB` and `SUP` are LOCAL names (e.g. `C`), resolved against the fixture's
/// own IRIs by [`resolve`] — the oracle files stay readable without hardcoding
/// a namespace per fixture.
fn parse_oracle(text: &str) -> Oracle {
    let mut provenance = String::new();
    let mut claims = Vec::new();
    for raw_line in text.lines() {
        let line = match raw_line.split_once('#') {
            Some((head, _comment)) => head.trim(),
            None => raw_line.trim(),
        };
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("provenance:") {
            provenance = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("unsatisfiable:") {
            claims.push(Claim::Unsatisfiable(rest.trim().to_string()));
        } else if let Some(rest) = line.strip_prefix("subclass:") {
            let mut parts = rest.split_whitespace();
            let sub = parts.next().expect("subclass: SUB SUP needs two names");
            let sup = parts.next().expect("subclass: SUB SUP needs two names");
            claims.push(Claim::Subclass(sub.to_string(), sup.to_string()));
        } else {
            panic!("unrecognized oracle line: {raw_line:?}");
        }
    }
    assert!(
        !provenance.is_empty(),
        "oracle file has no `provenance:` line — every claim here must say \
         where it came from (konclude-v0.7.0-1138 / hermit / derivation-only / structural)"
    );
    assert!(!claims.is_empty(), "oracle file has no claims to check");
    Oracle { provenance, claims }
}

fn load_oracle(fixture: &str) -> Oracle {
    let text = std::fs::read_to_string(format!("tests/fixtures/{fixture}.oracle"))
        .expect("oracle file present");
    parse_oracle(&text)
}

fn load_fixture(fixture: &str) -> InternalOntology {
    let ofn =
        std::fs::read_to_string(format!("tests/fixtures/{fixture}.ofn")).expect("fixture present");
    load(&ofn)
}

/// Resolves a short local name (e.g. `"C"`) to the one class IRI among
/// `classes` ending in `/NAME` — every fixture here uses a single namespace,
/// so this keeps the oracle files free of hardcoded prefixes.
fn resolve<'a>(classes: &'a [String], name: &str) -> &'a str {
    let suffix = format!("/{name}");
    classes
        .iter()
        .find(|iri| iri.ends_with(&suffix))
        .unwrap_or_else(|| panic!("oracle names {name:?}, not found among {classes:?}"))
}

/// Does rustdl's OWN classification (via `owl_dl_reasoner::classify_internal`
/// — the real hybrid classifier, not this crate's checker) satisfy every
/// claim in `oracle`? `false` on a `classify_internal` error too: if rustdl
/// cannot even classify the fixture, agreement cannot be confirmed.
fn classification_matches_oracle(internal: &InternalOntology, oracle: &Oracle) -> bool {
    let Ok(classification) = owl_dl_reasoner::classify_internal(internal) else {
        return false;
    };
    let classes = classification.classes();
    oracle.claims.iter().all(|claim| match claim {
        Claim::Unsatisfiable(name) => {
            let iri = resolve(classes, name);
            classification.unsatisfiable_classes().contains(&iri)
        }
        Claim::Subclass(sub, sup) => {
            let sub_iri = resolve(classes, sub);
            let sup_iri = resolve(classes, sup);
            classification.is_subclass(sub_iri, sup_iri)
        }
    })
}

/// Runs this crate's own instrument end to end: build, then check. A
/// `build_model` refusal (e.g. `ChainRangeOutOfProfile`) is folded into
/// `Verdict::Unresolved` — it satisfies the "never `Verified`" invariant
/// trivially, but `refused` distinguishes it in the acceptance log from a
/// verdict that came out of an actual completed check.
///
/// `build_model`'s own `Vec<UnresolvedReason>` (returned alongside a
/// successfully built model) is returned too, separately from `verdict` —
/// `verify`'s `Violated`/`Unresolved` variants only ever carry CHECK-time
/// unresolved rows (from `check_axiom` itself), never these BUILD-time ones,
/// so a caller who only looked at `verdict` could see a clean `Violated` and
/// never learn the model it was checked against had already flagged a gap
/// during construction (see `cascade.ofn` in the acceptance log below: `Violated`
/// there is not concealing an admitted build-time gap on the same node —
/// logging both channels together is what proves that, rather than asserting it).
fn verify_fixture(internal: &InternalOntology) -> (Verdict, bool, Vec<UnresolvedReason>) {
    match owl_dl_verify::build_model(internal, &Bounds::default()) {
        Ok((model, build_reasons)) => {
            let (verdict, _verified_model) = owl_dl_verify::verify(model, internal, None);
            (verdict, false, build_reasons)
        }
        Err(reason) => (
            Verdict::Unresolved {
                domain_size: 0,
                reasons: vec![reason.clone()],
            },
            true,
            vec![reason],
        ),
    }
}

/// The core invariant, checked over every fixture this project's D10 hunt
/// produced — the five straight detections and the two the checker refuses
/// outright — plus the three healthy controls
/// (included here because the invariant is vacuously true on them too:
/// `rustdl_agrees` is `true`, so nothing is asserted, and their `Verified`
/// requirement is pinned separately by `healthy_controls_are_verified`).
#[test]
fn instrument_never_verifies_a_classification_that_disagrees_with_the_oracle() {
    let fixtures = [
        "chainpoison",
        "chain-range-bot",
        "unsatnested",
        "nested-mono",
        "cascade",
        "chainrange",
        "chainrange_ctl",
        // Added when #84 made `chainrange` agree and emptied the detection set
        // (this test fired, as designed). Its own oracle records why it is the
        // right replacement: same family, still open, HermiT-confirmed.
        "chaincompose",
        "unsatconj",
        "flat-mono",
        "label-closure-range-sub",
    ];

    for fixture in fixtures {
        let internal = load_fixture(fixture);
        let oracle = load_oracle(fixture);
        let rustdl_agrees = classification_matches_oracle(&internal, &oracle);
        let (verdict, refused, build_reasons) = verify_fixture(&internal);

        if !rustdl_agrees {
            let provenance_caveat = if oracle.provenance == "derivation-only" {
                " (NOTE: this oracle is derivation-only — neither Konclude nor \
                  HermiT confirms it; the claim rests on hand-derivation alone, \
                  not peer agreement)"
            } else {
                ""
            };
            assert!(
                !matches!(verdict, Verdict::Verified { .. }),
                "{fixture}: rustdl disagrees with the {} oracle{provenance_caveat}, \
                 so the instrument must not report Verified. Got {verdict:?}",
                oracle.provenance,
            );
        }

        match (&verdict, refused) {
            (Verdict::Unresolved { .. }, true) => {
                eprintln!(
                    "[acceptance] {fixture}: REFUSED — build_model declined outright \
                     ({} oracle, rustdl_agrees={rustdl_agrees}); the invariant holds \
                     trivially, but this is neither a control nor a detection.",
                    oracle.provenance
                );
            }
            (Verdict::Unresolved { .. }, false) => {
                eprintln!(
                    "[acceptance] {fixture}: landed on Unresolved rather than Violated \
                     ({} oracle, rustdl_agrees={rustdl_agrees}) — the invariant holds, \
                     but this is WEAKER coverage than a detection.",
                    oracle.provenance
                );
            }
            (Verdict::Violated { .. }, _) => {
                // `build_reasons` is `verify`'s OWN build-time channel (see
                // `verify_fixture`'s doc) — a non-empty one here means the
                // model this `Violated` verdict was checked against had
                // ALREADY been flagged, during construction, as incompletely
                // closed somewhere. Logged rather than hidden: it does not
                // downgrade the verdict (the check is a separate, principled
                // computation over the model as actually built — see the
                // module doc), but it is exactly the causal story, not a
                // coincidence, on `cascade.ofn` (its 3 marker-targeted
                // `LabelNotClosed` name the very role/node whose missing
                // label entry is what the violation reports).
                eprintln!(
                    "[acceptance] {fixture}: DETECTED (Violated) against the {} oracle \
                     (rustdl_agrees={rustdl_agrees}); build-time unresolved rows: {}.",
                    oracle.provenance,
                    build_reasons.len()
                );
            }
            (Verdict::Verified { .. }, _) => {
                eprintln!(
                    "[acceptance] {fixture}: Verified ({} oracle, rustdl_agrees={rustdl_agrees}).",
                    oracle.provenance
                );
            }
        }
    }
}

/// Controls must come back `Verified` SPECIFICALLY — not merely "produced no
/// violations". An implementation that always returns `Unresolved` would pass
/// the invariant test above on every fixture (it would never say `Verified`),
/// so it needs a separate, positive assertion: on a fixture where rustdl and
/// the oracle genuinely agree, the instrument must reach a completed,
/// affirmative check.
#[test]
fn healthy_controls_are_verified() {
    let controls = ["unsatconj", "flat-mono", "label-closure-range-sub"];

    for fixture in controls {
        let internal = load_fixture(fixture);
        let oracle = load_oracle(fixture);
        assert!(
            classification_matches_oracle(&internal, &oracle),
            "{fixture} is claimed as a healthy control, but rustdl's own \
             classification does not even match its {} oracle — the control \
             assumption itself is wrong, not just the instrument",
            oracle.provenance
        );

        let (verdict, refused, _build_reasons) = verify_fixture(&internal);
        assert!(
            !refused,
            "{fixture} is a healthy control and must be BUILDABLE, not refused. \
             Got {verdict:?}"
        );
        assert!(
            matches!(verdict, Verdict::Verified { .. }),
            "{fixture} is a healthy control (rustdl agrees with its {} oracle), so \
             the instrument must report Verified — not just avoid Violated, which an \
             always-Unresolved implementation would also do. Got {verdict:?}",
            oracle.provenance
        );
    }
}

/// A `classification_matches_oracle` that wrongly returns `true` for every
/// claim shape would make the invariant test above pass VACUOUSLY on every
/// fixture — indistinguishable from a clean green run. `healthy_controls_
/// are_verified` already proves the function CAN return `true`; this proves
/// the `subclass:`/`unsatisfiable:` parsing itself is not a stub: parse one
/// real committed `.oracle` file and check the exact provenance and claims it
/// produces, so a gutted `strip_prefix`/`split_whitespace` arm breaks this
/// test rather than silently returning an empty or wrong `Oracle`.
#[test]
fn parse_oracle_reads_provenance_and_claims_not_just_the_file_existing() {
    let cascade = load_oracle("cascade");
    assert_eq!(cascade.provenance, "konclude-v0.7.0-1138");
    assert_eq!(
        cascade.claims,
        vec![Claim::Subclass("A".to_string(), "FINAL".to_string())]
    );

    let chain_range_bot = load_oracle("chain-range-bot");
    assert_eq!(chain_range_bot.provenance, "derivation-only");
    assert_eq!(
        chain_range_bot.claims,
        vec![Claim::Unsatisfiable("C".to_string())]
    );
}

/// THE INVARIANT ABOVE CAN GO VACUOUS WITHOUT ANYTHING TURNING RED.
///
/// `instrument_never_verifies_a_classification_that_disagrees_with_the_oracle`
/// asserts nothing at all when `rustdl_agrees` — so every time an engine fix
/// closes a fixture's gap, that fixture silently stops testing the instrument
/// and the suite still reports green. Issue #81 did exactly that to
/// `cascade.ofn`: it went from a genuine DETECTION (rustdl emitted zero rows,
/// the instrument correctly refused to say `Verified`) to a fixture rustdl
/// classifies correctly, in one commit, with no test failing.
///
/// This is the stale-sentinel pattern pointed the other way: not "an
/// `#[ignore]`d test that starts passing", but "a live test whose premise
/// evaporates". The cheap guard is to require the detection set to be
/// NON-EMPTY, so the suite cannot quietly become an all-vacuous green.
#[test]
fn the_detection_set_has_not_silently_gone_vacuous() {
    let fixtures = [
        "chainpoison",
        "chain-range-bot",
        "unsatnested",
        "nested-mono",
        "cascade",
        "chainrange",
        "chainrange_ctl",
        // See the note on the same entry in the list above.
        "chaincompose",
        "unsatconj",
        "flat-mono",
        "label-closure-range-sub",
    ];

    let disagreeing: Vec<&str> = fixtures
        .iter()
        .copied()
        .filter(|f| {
            let internal = load_fixture(f);
            let oracle = load_oracle(f);
            !classification_matches_oracle(&internal, &oracle)
        })
        .collect();

    assert!(
        !disagreeing.is_empty(),
        "NO fixture in the acceptance set still disagrees with its oracle, so \
         `instrument_never_verifies_a_classification_that_disagrees_with_the_\
         oracle` now asserts NOTHING on any of them and passes vacuously. Either \
         the engine closed every gap this set was built from — in which case add \
         a new detection fixture rather than leaving a green suite that tests \
         nothing — or `classification_matches_oracle` has broken in the \
         always-true direction."
    );
}

/// `cascade.ofn` is the worked example of the paragraph above, pinned so its
/// role change is explicit rather than inferred from a passing suite.
///
/// BEFORE issue #81 rustdl emitted zero rows here and the instrument's
/// `Violated` was a true detection. The fix made rustdl derive `A ⊑ FINAL` —
/// Konclude's only non-trivial row, confirmed against Konclude v0.7.0-1138 —
/// so rustdl now AGREES with the oracle. The instrument still says `Violated`,
/// which makes it a **false positive** of the documented F1 mechanism
/// (`docs/known-limitations/verify-two-expansion-paths-split-a-witness.md`):
/// cascade's `SubClassOf(ObjectIntersectionOf(∃v.W, G), Z)` is a GCI over a
/// conjunction, and `materialise_exists` labels that witness by plain union
/// without closing it under the GCI.
///
/// Fails loudly if either half moves: if rustdl stops agreeing (a completeness
/// regression) or if the false `Violated` clears (F1 fixed — update this test
/// and the known-limitations page together).
#[test]
fn cascade_now_agrees_with_its_oracle_and_its_violated_is_a_known_f1_false_positive() {
    let internal = load_fixture("cascade");
    let oracle = load_oracle("cascade");
    assert!(
        classification_matches_oracle(&internal, &oracle),
        "issue #81 made rustdl derive cascade's A ⊑ FINAL; losing it again is a \
         completeness regression in the nested-existential range fold"
    );
    let (verdict, refused, _reasons) = verify_fixture(&internal);
    assert!(!refused, "cascade must still be buildable: {verdict:?}");
    assert!(
        matches!(verdict, Verdict::Violated { .. }),
        "cascade's Violated is a KNOWN F1 false positive (GCI over a conjunctive \
         ∃-body, labelled by plain union). If this is no longer Violated, F1 has \
         been fixed — update docs/known-limitations/\
         verify-two-expansion-paths-split-a-witness.md in the same change. Got \
         {verdict:?}"
    );
}
