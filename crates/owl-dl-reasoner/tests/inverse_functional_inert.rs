//! `InverseFunctionalRole` is admitted by `saturator_complete_fragment` and NEVER
//! read by the saturator. That is sound and complete — but for a structural reason,
//! and this file pins the reason.
//!
//! # Why this test exists
//!
//! The gate arm previously carried a comment saying the admission was for "the
//! Phase-2 functional / inverse-functional witness-merge", i.e. that the engine
//! consumes the axiom. It does not: `grep Axiom::InverseFunctionalRole
//! crates/owl-dl-saturation` finds nothing. That reads exactly like the **D10 bug
//! class** — a fragment gate certifying COMPLETE while the engine silently drops an
//! admitted axiom, of which this repository has six recorded instances — and it cost
//! an investigation on 2026-08-18 to establish that it is not one.
//!
//! # Why dropping it IS complete here
//!
//! Inverse-functionality constrains PREDECESSORS: at most one `r`-predecessor per
//! node. The admitted fragment has no nominals, no `ABox` assertions and no inverse
//! role *use*, so the canonical model is a TREE — every `∃`-witness is created by
//! exactly one parent, hence has exactly one predecessor. The constraint holds by
//! construction and entails nothing.
//!
//! **The exclusions are load-bearing.** Two `r`-edges into one node require identity
//! forcing, which needs nominals or an `ABox`. **If the fragment is ever widened to
//! either, this becomes a real D10 defect** — at which point the saturator must
//! consume inverse-functionality, or the gate arm must go.
//!
//! **THESE TESTS DO NOT GUARD THAT WIDENING, AND THE ATTEMPT TO MAKE THEM IS
//! RECORDED HERE BECAUSE IT FAILED TWICE.** First, sabotage showed the three closure
//! tests are blind to it: adding
//! `Axiom::ClassAssertion { .. } | Axiom::ObjectPropertyAssertion { .. } => true` to
//! `is_saturator_axiom` left all three **passing**, because their fixtures contain no
//! `ABox` and so cannot notice a fragment widening. Second, a purpose-built tripwire
//! — an `ABox` forcing `x r z`, `y r z` with `r` inverse-functional, so `x = y` and the
//! tree-model argument lapses — **also failed**, but for an unrelated reason: that
//! fixture reaches the saturation fast path via `is_pure_el`, a DIFFERENT gate, and
//! the forced merge yields no CLASS subsumption, so there is no demonstrated defect
//! to assert. Asserting "must not reach the fast path" would have pinned a
//! requirement I could not justify.
//!
//! **Open question, deliberately not answered here:** whether inverse-functional +
//! `ABox` is complete on the `is_pure_el` path for *`realize`* (where individual
//! identity, unlike classification, is observable). `realize` has its own gate
//! (`realize_saturation_eligible`), so it needs its own investigation rather than a
//! test bolted onto this one.
//!
//! # What is asserted
//!
//! The three fixtures are the shapes where inverse-functionality could plausibly
//! bite inside the fragment. Expected closures were **adjudicated against Konclude**
//! (2026-08-18): identical in all three, modulo `owl:Thing` rows which rustdl does
//! not report as `direct`. The expectations below are those adjudicated closures, so
//! a change in either the gate or the saturator that alters them fails here.

use owl_dl_reasoner::classify;
use std::collections::BTreeSet;
use std::path::Path;

fn closure_of(fixture: &str) -> BTreeSet<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/inverse_functional")
        .join(fixture);
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut std::io::Cursor::new(src),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse fixture");
    let h = classify(&onto).expect("classify fixture");
    let strip = |s: &str| s.replace("http://t/", "");
    let mut out = BTreeSet::new();
    for c in h.classes() {
        for d in h.direct_subsumers(c) {
            out.insert((strip(c), strip(d)));
        }
    }
    out
}

fn expect(fixture: &str, want: &[(&str, &str)]) {
    let got = closure_of(fixture);
    let want: BTreeSet<(String, String)> = want
        .iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect();
    assert_eq!(
        got, want,
        "closure for {fixture} changed. These expectations are the Konclude-adjudicated \
         closures (2026-08-18). If the gate or the saturator changed, re-adjudicate against \
         Konclude before editing them — and if the fragment was widened to nominals or an \
         ABox, `InverseFunctionalRole` is no longer inert and the gate arm is now a D10 defect."
    );
}

/// Two classes whose `∃r` witnesses share a filler, `r` inverse-functional.
/// Distinct witnesses in the tree model ⇒ no merge, no extra entailment.
#[test]
fn shared_filler_entails_nothing_extra() {
    expect(
        "shared-filler.ofn",
        &[("A", "E"), ("B", "E"), ("C", "D"), ("E", "F")],
    );
}

/// Two SUB-roles of one inverse-functional role, both into the same filler from the
/// same subject — the closest this fragment gets to two `r`-edges meeting.
/// Still distinct witnesses.
#[test]
fn two_subroles_into_one_filler_entails_nothing_extra() {
    expect("subrole-merge.ofn", &[("A", "B"), ("C", "D")]);
}

/// Inverse-functional + functional + transitive on one role along a chain, i.e. all
/// three role characteristics the gate admits interacting at once.
#[test]
fn inverse_functional_with_functional_and_transitive_entails_nothing_extra() {
    expect(
        "func-transitive-chain.ofn",
        &[("A", "E"), ("B", "E"), ("C", "D")],
    );
}

/// The fragment gate really does admit these ontologies — otherwise the tests above
/// would be vacuous, asserting the hybrid path's behaviour rather than the
/// saturator's. This is the check that makes the others mean what they claim.
#[test]
fn the_fixtures_are_actually_on_the_saturator_fast_path() {
    for f in [
        "shared-filler.ofn",
        "subrole-merge.ofn",
        "func-transitive-chain.ofn",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/inverse_functional")
            .join(f);
        let src = std::fs::read_to_string(&path).expect("read fixture");
        let (onto, _): (
            horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
            _,
        ) = horned_owl::io::ofn::reader::read(
            &mut std::io::Cursor::new(src),
            horned_owl::io::ParserConfiguration::default(),
        )
        .expect("parse fixture");
        let h = classify(&onto).expect("classify fixture");
        assert!(
            h.stats().pure_el_mode,
            "{f} must take the saturation fast path — if it falls to the hybrid path these \
             tests no longer exercise the gate/saturator gap they exist for"
        );
    }
}

/// The `RUSTDL_INVERSE_FUNC_MAX` gate arm is FLAG-GATED, and this pins why that is not
/// redundant.
///
/// With the flag off there is no DERIVED `≤1 r⁻` — but a **hand-written** one does not
/// care about the flag. An ungated arm would therefore newly admit this ontology to the
/// saturation fast path at the DEFAULT, changing behaviour on a path the flag is
/// supposed to leave alone. That escaped the corpus spot-check used to claim
/// "flag-off byte-identical" (pizza/ro/sio have no such shape — the check was inert),
/// and was caught only by reading the diff.
#[test]
fn handwritten_inverse_max_is_admitted_only_under_the_flag() {
    let src = "\
Prefix(:=<http://t/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(<http://t/hw>\n\
Declaration(Class(:A)) Declaration(Class(:B))\n\
Declaration(ObjectProperty(:r))\n\
InverseFunctionalObjectProperty(:r)\n\
SubClassOf(ObjectSomeValuesFrom(ObjectInverseOf(:r) owl:Thing) \
ObjectMaxCardinality(1 ObjectInverseOf(:r)))\n\
SubClassOf(:A :B)\n\
)";
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut std::io::Cursor::new(src.to_string()),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse");
    // No env mutation: this asserts the DEFAULT, so it reads the ambient environment
    // and needs no lock (and must not take one — see the deadlock note in
    // `realize_derived_same.rs`).
    assert!(
        !classify(&onto).expect("classify").stats().pure_el_mode,
        "at the DEFAULT (flag off) a hand-written ≤1 r⁻ must still be out-of-fragment, \
         exactly as before this change. If it is admitted, the gate arm lost its flag \
         guard and flag-off is no longer byte-identical to pre-change."
    );
}
