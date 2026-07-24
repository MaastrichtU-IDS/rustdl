//! Task 5 (issue #35 v4): realize-layer canaries for the 3-axiom
//! nominal+cardinality reproducer (see task-5 brief + `.superpowers/sdd/
//! task-5-report.md`): `SubClassOf(A, ObjectOneOf(x,y,z))` +
//! `EquivalentClasses(B, ObjectIntersectionOf(A, ObjectMinCardinality(2, r,
//! C)))` + `ObjectPropertyDomain(r, A)`.
//!
//! # HONESTY REQUIREMENT — the full 3-axiom reproducer's UNBOUNDED search
//! does NOT terminate (Task A gap); the DEFAULT realize call now does
//! (Task B safety net)
//!
//! Per the tableau-layer finding in `crates/owl-dl-tableau/tests/
//! nominal_first_bounded.rs` (the load-bearing gate, currently `#[ignore]`d
//! because it hangs even with the fix ON and the cap disabled), the FULL
//! 3-axiom core's completion graph itself does not converge: the
//! `ObjectPropertyDomain`-derived universal residual disjunction repeatedly
//! re-derives `A` on freshly generated `≥2 r.C` witnesses, which — via the
//! covering nominal disjunction — forces those witnesses to merge into the
//! very individual that owns the cardinality constraint, destroying
//! witness-distinctness and forcing regeneration. This is NOT gated by
//! `has_pending_nominal_disjunction` (Task 2), so Tasks 1-4 (the deferred
//! "real fix A") do not bound it — that remains an open gap, still
//! documented by the ignored gate above.
//!
//! Task B (safety net, this file's `issue35_v4_realize_smoke_and_correct`)
//! instead bounds the *caller-side* realize call: a default, non-zero
//! `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` per-pair deadline plus a hard
//! `NodeCap` early-return in `search::branch` make `realize` on the full
//! 3-axiom core return promptly with a sound (under-approximate) result
//! out of the box — no env vars needed. This is not a claim that the
//! search itself converges; it is a claim that the *caller* is protected
//! from the divergence, matching the issue reporter's original ask.
//!
//! What follows are the tests from the original brief that were built
//! around the (still-true) tableau-divergence finding, plus the realize
//! smoke test now enabled by the Task B default:
//!
//! - a positive-entailment canary in the nominal+cardinality shape
//!   (Step 3d), using a variant that terminates fast (see below for why),
//! - the two minimality-variant sub-tests (Step 4): drop
//!   `ObjectPropertyDomain`, or replace the `ObjectOneOf` covering axiom
//!   with a plain `SubClassOf` — BOTH terminate quickly and give the same
//!   verdict under `RUSTDL_NOMINAL_FIRST=0` and `=1` (manually verified via
//!   the CLI with `RUSTDL_MAX_NODES=0`; see the report for the transcript —
//!   this file only runs the default, fix-ON configuration in-process,
//!   following this crate's existing convention of not toggling
//!   `OnceLock`-cached env vars across tests sharing a binary),
//! - `issue35_v4_realize_smoke_and_correct`: `realize` on the full 3-axiom
//!   core, default settings, asserting it returns (no hang) with a sound
//!   MISS (see below).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{is_class_satisfiable, is_consistent, is_instance_of, realize};
use std::io::Cursor;

const NS: &str = "http://example.org/card#";

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (onto, _prefixes) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    onto
}

/// Step 3d: positive-entailment canary in the nominal+cardinality shape.
///
/// `SubClassOf(A, ObjectOneOf(x,y,z))` + `EquivalentClasses(B,
/// ObjectIntersectionOf(A, ObjectMinCardinality(2,r,C)))`, WITHOUT the
/// `ObjectPropertyDomain` axiom (dropping it is what keeps this variant
/// terminating — see the module doc). `D` is asserted on all three of
/// `x`, `y`, `z`, and `w` is asserted `B`. Since OWL has no unique-name
/// assumption, `w` being `A` (via `B ⊑ A`) forces `w` to denote one of
/// `x`, `y`, `z` — and `D` holds on all three, so `w:D` is entailed
/// regardless of which one `w` is identified with. A divergence-capped
/// run that failed to resolve the covering disjunction would report
/// `Ok(None)` -> "not an instance", the WRONG answer; this asserts the
/// correct `true`.
///
/// Run with the cap effectively disabled (`RUSTDL_MAX_NODES=0`) so a
/// regression that reintroduces divergence hangs loudly instead of
/// silently passing via cap fallback — guard any CI run of this specific
/// assertion with a shell `timeout` if the shape is ever widened.
///
/// Note: `realize`'s per-individual *most-specific-type* summary does
/// NOT surface `w:D` for this fixture (verified: `entailed_types` for
/// `w` reports only `B`) — that is a separate, pre-existing scoping
/// property of realize's most-specific-type algorithm (it does not
/// appear to probe every named class independent of the subsumption
/// hierarchy), not a nominal-first regression. The direct pairwise API
/// (`is_instance_of`, used below) is unaffected and correctly returns
/// `true`. See the task-5 report.
#[test]
fn issue35_v4_positive_entailment_canary() {
    let onto = parse(&format!(
        "Prefix(:=<{NS}>)
Ontology(<http://example.org/card>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y)) Declaration(NamedIndividual(:z))
  Declaration(NamedIndividual(:w))
  SubClassOf(:A ObjectOneOf(:x :y :z))
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))
  ClassAssertion(:D :x)
  ClassAssertion(:D :y)
  ClassAssertion(:D :z)
  ClassAssertion(:B :w)
)
"
    ));
    assert!(
        is_instance_of(&onto, &format!("{NS}D"), &format!("{NS}w"))
            .expect("must terminate, not hang"),
        "w must be entailed D: w:B ⟹ w:A ⟹ w ∈ {{x,y,z}} (no UNA) and D holds on all three"
    );
    // Sanity companions: the asserted type is trivially entailed; an
    // unrelated class is not.
    assert!(
        is_instance_of(&onto, &format!("{NS}B"), &format!("{NS}w")).expect("terminates"),
        "w is told B"
    );
    assert!(
        !is_instance_of(&onto, &format!("{NS}C"), &format!("{NS}w")).expect("terminates"),
        "w is not entailed C"
    );

    // Issue #39 regression: `realize` must agree with `is_instance_of`.
    // `A ⊑ {x,y,z}` + `D` on all of x,y,z ⟹ `A ⊑ D` (sound nominal reasoning),
    // and `B ≡ A ⊓ …` ⟹ `B ⊑ A ⊑ D`. So `w`'s entailed types are exactly
    // {A, B, D}, and — since they form the chain `B ⊑ A ⊑ D` — the single
    // most-specific (Hasse-leaf) type is `B`. Before #38's edge-corruption fix
    // (v0.3.41) `realize`'s parallel per-pair probe could intermittently drop
    // `D` from the entailed set; this pins the corrected behaviour.
    let r = realize(&onto).expect("realize terminates");
    let w = format!("{NS}w");
    let entailed = r.entailed_types(&w);
    for c in ["A", "B", "D"] {
        assert!(
            entailed.iter().any(|t| t == &format!("{NS}{c}")),
            "realize entailed_types(w) must contain {c}; got {entailed:?}"
        );
    }
    assert!(
        !entailed.iter().any(|t| t == &format!("{NS}C")),
        "realize entailed_types(w) must NOT contain C; got {entailed:?}"
    );
    assert_eq!(
        r.most_specific_types(&w),
        &[format!("{NS}B")],
        "B ⊑ A ⊑ D ⟹ B is the sole most-specific type of w"
    );
}

/// Step 4(a) minimality variant: drop `ObjectPropertyDomain(r, A)`, keep
/// the covering nominal disjunction and the `≥2 r.C` cardinality. Confirms
/// the nominal-first guard does not need (or over-fire in the absence of)
/// the domain axiom: `A`, `B`, `C` all remain satisfiable and the ontology
/// stays consistent. Manually verified via the CLI that
/// `RUSTDL_NOMINAL_FIRST=0` gives the identical verdicts (see task-5
/// report) — this file runs only the default (fix ON) configuration
/// in-process, per this crate's existing env-toggling convention.
#[test]
fn issue35_v4_minimality_variant_drop_domain() {
    let onto = parse(&format!(
        "Prefix(:=<{NS}>)
Ontology(<http://example.org/card>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y)) Declaration(NamedIndividual(:z))
  SubClassOf(:A ObjectOneOf(:x :y :z))
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))
)
"
    ));
    assert!(
        is_consistent(&onto).expect("terminates"),
        "ontology is consistent"
    );
    for cls in ["A", "B", "C"] {
        assert!(
            is_class_satisfiable(&onto, &format!("{NS}{cls}")).expect("terminates"),
            "{cls} must be satisfiable"
        );
    }
}

/// Step 4(b) minimality variant: keep `ObjectPropertyDomain(r, A)` but
/// replace the `ObjectOneOf` covering axiom with a plain `SubClassOf(A,
/// D)` — no nominal disjunction at all. Guards against the guard
/// over-firing: `has_pending_nominal_disjunction` must not fire (there is
/// no nominal `Or` to find), so `apply_exists`/`apply_min` generation
/// proceeds exactly as pre-fix. Manually verified via the CLI that
/// `RUSTDL_NOMINAL_FIRST=0` gives the identical verdicts (see task-5
/// report).
#[test]
fn issue35_v4_minimality_variant_plain_subclass() {
    let onto = parse(&format!(
        "Prefix(:=<{NS}>)
Ontology(<http://example.org/card>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  SubClassOf(:A :D)
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))
  ObjectPropertyDomain(:r :A)
)
"
    ));
    assert!(
        is_consistent(&onto).expect("terminates"),
        "ontology is consistent"
    );
    for cls in ["A", "B", "C", "D"] {
        assert!(
            is_class_satisfiable(&onto, &format!("{NS}{cls}")).expect("terminates"),
            "{cls} must be satisfiable"
        );
    }
}

/// Step 3b/3c of the brief, verbatim shape: `realize` on the FULL 3-axiom
/// reproducer core, run with **default settings (no env vars set)**.
///
/// Task B (safety net) added a default, non-zero
/// `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` bound (see `realize.rs`'s
/// `DEFAULT_REALIZE_PAIR_TIMEOUT_MS`) to the off-fragment tableau realize
/// path, plus a hard early-return on a tableau `NodeCap` trip
/// (`search::branch`) — together these make this reproducer TERMINATE
/// FAST out of the box (previously: `#[ignore]`d because it did not
/// terminate even with the fix ON and the node cap disabled — see the
/// HONESTY REQUIREMENT above and task-5-report.md; that finding about the
/// *tableau's own completion graph diverging* is unchanged and still
/// documented by `nominal_first_bounded.rs`'s `#[ignore]`d
/// `issue35_v4_completion_graph_is_bounded` gate, which stays ignored —
/// this test is about the *caller-side realize bound* making the overall
/// call return promptly with a sound result, not about the search
/// converging on its own). The per-pair deadline bounds each (individual,
/// class) probe, so the answer here is a sound MISS: `x`/`y`/`z` do not
/// get typed `B`/`C` (the tableau proof that would derive it is cut off
/// before completing) — asserting the type set stays EMPTY, not that it's
/// wrong, is exactly what "sound under-approximation" means.
///
/// Runs in BOTH debug and release. It previously carried a
/// `cfg_attr(debug_assertions, ignore)` because this reproducer's rapid graph
/// growth tripped a stale-index `debug_assert_eq!` in
/// `TableauContext::remove_edge_recorded` (issue #38, fixed: the merge re-anchor
/// now searches the mirror in-edge by the union-find representative `y_eff`, not
/// the unresolved snapshot node), so it was un-gated.
///
/// It EXERCISES the fixed #38 merge path, but is NOT a deterministic #38 guard:
/// the offending merge only arises deep in the (deferred-fix-A) unbounded
/// divergent search and is reached nondeterministically under the parallel
/// per-pair timeout, so its panic-without-the-fix is timing-dependent (any node
/// cap that would make it deterministic also cuts the search before the trigger
/// forms). The *deterministic* backstop for #38 is the `debug_assert_eq!` in
/// `remove_edge_recorded` itself — it fires in ANY debug test that hits a
/// mis-indexed merge, across the whole suite, so a regression cannot pass debug
/// CI silently. See the #38 investigation notes in the ledger. (The separate
/// *completion-graph divergence* finding — fix A deferred — stays documented by
/// `nominal_first_bounded.rs`'s `#[ignore]`d `issue35_v4_completion_graph_is_bounded`.)
#[test]
fn issue35_v4_realize_smoke_and_correct() {
    let onto = parse(&format!(
        "Prefix(:=<{NS}>)
Ontology(<http://example.org/card>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y)) Declaration(NamedIndividual(:z))
  SubClassOf(:A ObjectOneOf(:x :y :z))
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))
  ObjectPropertyDomain(:r :A)
)
"
    ));
    let r = owl_dl_reasoner::realize(&onto).expect("realize returns (no hang, no error)");
    for ind in ["x", "y", "z"] {
        let iri = format!("{NS}{ind}");
        let types = r.entailed_types(&iri);
        assert!(
            !types.iter().any(|t| t.ends_with("#B")),
            "{ind} must not be B"
        );
        assert!(
            !types.iter().any(|t| t.ends_with("#C")),
            "{ind} must not be C"
        );
    }
}
