//! Task 5 (issue #35 v4): realize-layer canaries for the 3-axiom
//! nominal+cardinality reproducer (see task-5 brief + `.superpowers/sdd/
//! task-5-report.md`): `SubClassOf(A, ObjectOneOf(x,y,z))` +
//! `EquivalentClasses(B, ObjectIntersectionOf(A, ObjectMinCardinality(2, r,
//! C)))` + `ObjectPropertyDomain(r, A)`.
//!
//! # HONESTY REQUIREMENT — the full 3-axiom reproducer does NOT terminate
//!
//! Per the tableau-layer finding in `crates/owl-dl-tableau/tests/
//! nominal_first_bounded.rs` (the load-bearing gate, currently `#[ignore]`d
//! because it hangs even with the fix ON and the cap disabled), the FULL
//! 3-axiom core does not terminate: the `ObjectPropertyDomain`-derived
//! universal residual disjunction repeatedly re-derives `A` on freshly
//! generated `≥2 r.C` witnesses, which — via the covering nominal
//! disjunction — forces those witnesses to merge into the very individual
//! that owns the cardinality constraint, destroying witness-distinctness
//! and forcing regeneration. This is NOT gated by `has_pending_nominal_
//! disjunction` (Task 2), so Tasks 1-4 do not bound it. Per the brief's
//! honesty requirement, this file does **not** include a `realize`/
//! `is_class_satisfiable` smoke test against the full 3-axiom core (it
//! would hang / require an enormous node budget even under the default
//! cap — confirmed empirically: wall time at `RUSTDL_MAX_NODES` 500/2000/
//! 8000 was 0.5s/19.4s/>30s, a steep combinatorial blow-up, not a slow
//! convergence). What follows instead are the tests from the brief that
//! CAN be built safely and that remain meaningful given that finding:
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
//!   `OnceLock`-cached env vars across tests sharing a binary).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{is_class_satisfiable, is_consistent, is_instance_of};
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
/// reproducer core. Per the HONESTY REQUIREMENT (module doc), this does
/// **not** currently pass — `realize` does not terminate on this input
/// even with the fix on and the cap disabled (confirmed: >30s wall at
/// `RUSTDL_MAX_NODES=8000`, cap OFF never returns). `#[ignore]`d so
/// `cargo test` never hangs; kept verbatim (rather than deleted) so a
/// future fix to the domain-residual/cardinality-merge gap (see
/// `nominal_first_bounded.rs`) can flip this back on as the acceptance
/// check the brief intended.
#[test]
#[ignore = "issue #35 v4: realize does NOT terminate on the full 3-axiom \
            reproducer even with the fix on (cap OFF) — see module doc + \
            task-5-report.md. Do not remove this ignore without first \
            fixing the domain-residual/cardinality-merge gap."]
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
