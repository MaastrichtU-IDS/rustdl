//! Canaries for `cb_eli_eligible` — the Horn-ELHI fragment gate (milestone 1
//! of `docs/superpowers/specs/2026-08-16-cb-horn-eli-design.md`).
//!
//! The gate is DEAD CODE for now (nothing dispatches on it; its only caller is
//! the opt-in `RUSTDL_CB_ELI_PROBE` census line), so these tests carry the
//! entire correctness weight until milestone 2 wires an engine behind it.
//!
//! Structure: one NEGATIVE test per rejected construct, so a failure names the
//! construct; one positive test over the full allowlist; nesting tests proving
//! the concept predicate is genuinely recursive (a top-level `Bot` arm alone
//! does not handle `∃r.∃s.⊥` — the `RUSTDL_EL_BOT_FILLER` lesson).
//!
//! Run: `cargo test -p owl-dl-reasoner --test cb_eli_eligible`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::InternalOntology;
use owl_dl_reasoner::{cb_eli_blocker, cb_eli_eligible};
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://ex.org/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";
const DECLS: &str = "Declaration(Class(:A))
Declaration(Class(:B))
Declaration(Class(:C))
Declaration(ObjectProperty(:r))
Declaration(ObjectProperty(:s))
Declaration(ObjectProperty(:t))
";

fn internal_of(body: &str) -> InternalOntology {
    let src = format!("{PFX}Ontology(<http://ex.org/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    owl_dl_core::convert::convert_ontology(&onto).expect("convert")
}

/// Assert rejection AND that the diagnostic names the expected blocker, so a
/// test failure identifies the construct that leaked through.
fn assert_rejected(body: &str, expected_blocker: &str) {
    let i = internal_of(body);
    assert!(
        !cb_eli_eligible(&i),
        "must be rejected (expected blocker {expected_blocker})"
    );
    let blocker = cb_eli_blocker(&i).expect("rejected ⟹ blocker present");
    assert_eq!(blocker, expected_blocker, "wrong blocker reported");
}

// ── positive: the full Horn-ELHI allowlist in one ontology ──────────────────

#[test]
fn accepts_el_plus_inverse_transitive_chain() {
    let i = internal_of(
        "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :C)))
SubClassOf(ObjectSomeValuesFrom(ObjectInverseOf(:r) :A) :B)
EquivalentClasses(:C ObjectIntersectionOf(:A ObjectSomeValuesFrom(:s :B)))
SubObjectPropertyOf(:r :s)
SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)
InverseObjectProperties(:r :s)
TransitiveObjectProperty(:t)
ObjectPropertyDomain(:r :A)
ObjectPropertyRange(:r ObjectIntersectionOf(:B :C))
DisjointClasses(:A :B)
SubClassOf(:C owl:Nothing)",
    );
    assert!(
        cb_eli_eligible(&i),
        "EL + inverse + transitivity + 2-chain + domain/range + disjointness \
         + ⊥ is exactly Horn-ELHI and must be accepted (blocker: {:?})",
        cb_eli_blocker(&i)
    );
    assert_eq!(cb_eli_blocker(&i), None, "eligible ⟹ no blocker");
}

// ── negatives: one per construct, so a failure names the construct ──────────

#[test]
fn rejects_forall() {
    assert_rejected(
        "SubClassOf(:A ObjectAllValuesFrom(:r :B))",
        "SubClassOf[All]",
    );
}

#[test]
fn rejects_union() {
    assert_rejected("SubClassOf(:A ObjectUnionOf(:B :C))", "SubClassOf[Or]");
}

#[test]
fn rejects_complement() {
    // NOTE the shape: a top-level RHS `X ⊑ ¬Y` would be REWRITTEN AWAY by the
    // default-ON `RUSTDL_NEG_TO_BOT_GCI` pass (→ `X⊓Y⊑⊥`, which IS
    // in-fragment — logically equivalent, so acceptance there is correct).
    // A `¬` nested inside an ∃-filler survives conversion and must reject.
    assert_rejected(
        "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectComplementOf(:B)))",
        "SubClassOf[Not]",
    );
}

#[test]
fn rejects_min_cardinality() {
    assert_rejected(
        "SubClassOf(:A ObjectMinCardinality(2 :r :B))",
        "SubClassOf[Min]",
    );
}

#[test]
fn rejects_max_cardinality() {
    assert_rejected(
        "SubClassOf(:A ObjectMaxCardinality(1 :r :B))",
        "SubClassOf[Max]",
    );
}

#[test]
fn rejects_exact_cardinality() {
    // ObjectExactCardinality(1 r B) converts to Min ⊓ Max; either conjunct
    // must reject. The blocker label is whichever the walk reaches first.
    let i = internal_of("SubClassOf(:A ObjectExactCardinality(1 :r :B))");
    assert!(
        !cb_eli_eligible(&i),
        "exact cardinality must be rejected (blocker: {:?})",
        cb_eli_blocker(&i)
    );
    let blocker = cb_eli_blocker(&i).expect("rejected ⟹ blocker present");
    assert!(
        blocker == "SubClassOf[Min]" || blocker == "SubClassOf[Max]",
        "exact cardinality should surface as Min or Max, got {blocker}"
    );
}

#[test]
fn rejects_oneof_nominal() {
    assert_rejected(
        "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectOneOf(:a)))",
        "SubClassOf[Nominal]",
    );
}

#[test]
fn rejects_data_property_axiom() {
    // A value-bearing data-property axiom lowers to `∃dp.DKey(range)`; the
    // gate must reject the synthetic DKey atomic (datatype semantics live in
    // told-subsumption seeding + the concrete-domain solver, neither of which
    // the CB engine will run).
    assert_rejected(
        "Declaration(DataProperty(:d))
SubClassOf(:A DataSomeValuesFrom(:d xsd:integer))",
        "SubClassOf[DKey]",
    );
}

#[test]
fn three_step_chain_is_decomposed_and_accepted() {
    // Written as a REJECTION canary first (sabotage planning found no test
    // pinned the chain-length arm), and it FAILED: `decompose_long_chains` in
    // convert.rs rewrites every >2-step chain into an EXACT-equivalent cascade
    // of 2-step chains over fresh aux roles, so the post-convert IR this gate
    // reads never contains a longer chain — acceptance is correct, same
    // rationale as the `X ⊑ ¬Y` rewrite in `rejects_complement`. The
    // `parts.len() == 2` arm is therefore belt-and-braces; this test pins the
    // conversion-level invariant it relies on.
    let i = internal_of("SubObjectPropertyOf(ObjectPropertyChain(:r :s :t) :t)");
    assert!(
        cb_eli_eligible(&i),
        "3-step chain must arrive decomposed into 2-step chains (blocker: {:?})",
        cb_eli_blocker(&i)
    );
}

#[test]
fn rejects_class_assertion_abox() {
    assert_rejected(
        "Declaration(NamedIndividual(:a))
ClassAssertion(:A :a)",
        "ClassAssertion",
    );
}

// ── recursion: the concept predicate must be TOTAL and recursive ────────────

#[test]
fn nested_bot_filler_is_accepted() {
    // `∃r.∃s.⊥` — a `Bot` MATCH ARM alone does not reach a ⊥ nested two
    // existentials deep; only a recursive predicate does.
    let i =
        internal_of("SubClassOf(:A ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Nothing)))");
    assert!(
        cb_eli_eligible(&i),
        "∃r.∃s.⊥ is Horn-ELHI and must be accepted (blocker: {:?})",
        cb_eli_blocker(&i)
    );
}

#[test]
fn nested_union_filler_is_rejected() {
    // `∃r.(A ⊔ B)` — the ⊔ is only visible by recursing into the filler.
    // CAUTION: `disjunction_existential.rs` may DERIVE an additional sound
    // `∃r.CommonSubsumer` axiom from this shape, but the original Or-bearing
    // axiom remains and must reject the ontology.
    assert_rejected(
        "SubClassOf(:C ObjectSomeValuesFrom(:r ObjectUnionOf(:A :B)))",
        "SubClassOf[Or]",
    );
}

// ── diagnostic/gate consistency ──────────────────────────────────────────────

#[test]
fn blocker_agrees_with_gate() {
    // The probe line derives eligibility from `cb_eli_blocker`; pin the
    // equivalence on one accepted and one rejected input so the two cannot
    // drift silently.
    let ok = internal_of("SubClassOf(:A ObjectSomeValuesFrom(:r :B))");
    assert_eq!(cb_eli_eligible(&ok), cb_eli_blocker(&ok).is_none());
    let bad = internal_of("SubClassOf(:A ObjectAllValuesFrom(:r :B))");
    assert_eq!(cb_eli_eligible(&bad), cb_eli_blocker(&bad).is_none());
    assert!(!cb_eli_eligible(&bad));
}
