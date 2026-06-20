//! Direct-call integration tests for `saturate_abox_for_consistency`.
//!
//! These tests call the function directly (bypassing `is_consistent` and the
//! A1 `abox_check` backstop), so they constitute genuine coverage of the new
//! ABox-saturation code path.  No env vars are mutated here; the function
//! reads `RUSTDL_TRACE` for diagnostic output but its return value is
//! env-var-independent.
//!
//! Run: `cargo test -p owl-dl-saturation --test abox_sat_consistency`

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_saturation::saturate_abox_for_consistency;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse_and_check(body: &str) -> bool {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let internal = convert_ontology(&onto).expect("convert");
    saturate_abox_for_consistency(&internal)
}

// ── Positive (clash detected) ──────────────────────────────────────────────

/// Functional(hasSex) + DisjointClasses(Male, Female) + ClassAssertion(∃hasSex.Male ⊓ ∃hasSex.Female, pat)
/// → clash via functional-marker rule.
///
/// This is the canonical test for the algorithm's Rule 7b path.
/// It CANNOT be explained by the pre-existing A1 P8 `abox_check` because we
/// call `saturate_abox_for_consistency` directly — there is no `abox_check` here.
#[test]
fn functional_marker_clash_detected_directly() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(ObjectProperty(:hasSex))
  Declaration(NamedIndividual(:pat))

  DisjointClasses(:Male :Female)
  FunctionalObjectProperty(:hasSex)

  SubClassOf(:Parent ObjectIntersectionOf(
    ObjectSomeValuesFrom(:hasSex :Male)
    ObjectSomeValuesFrom(:hasSex :Female)))

  ClassAssertion(:Parent :pat)
",
    );
    assert!(clash, "functional-marker clash (∃hasSex.Male ⊓ ∃hasSex.Female + Functional + Disjoint) must be detected");
}

/// Explicit ObjectPropertyAssertion witnesses that are already typed disjoint
/// → clash via Rule 8 (disjoint clash on individual types).
#[test]
fn direct_disjoint_type_clash_detected() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:Cat))
  Declaration(Class(:Dog))
  Declaration(NamedIndividual(:pet))

  DisjointClasses(:Cat :Dog)
  ClassAssertion(:Cat :pet)
  ClassAssertion(:Dog :pet)
",
    );
    assert!(clash, "direct disjoint-type clash on one individual must be detected");
}

/// Inverse materialization + disjoint: r(a,b) + InverseObjectProperties(r,s)
/// → s(b,a) derived, then range(s)=C + ClassAssertion(D,a) + Disjoint(C,D) → clash.
#[test]
fn inverse_materialization_triggers_clash() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:C))
  Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))

  DisjointClasses(:C :D)
  InverseObjectProperties(:r :s)
  ObjectPropertyRange(:s :C)

  ClassAssertion(:D :a)
  ObjectPropertyAssertion(:r :a :b)
",
    );
    // r(a,b) → inverse → s(b,a); range(s)=C → a:C; a:D already; Disjoint(C,D) → clash
    assert!(clash, "inverse materialization + range + disjoint must detect clash");
}

/// Role chain clash: r(a,b) + r(b,c) + SubPropertyChain(r o r → t) + domain(t)=T + a:X + Disjoint(T,X)
/// → t(a,c) derived + domain(t)=T → a:T → a has T and X → clash.
#[test]
fn role_chain_derives_clash() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:T))
  Declaration(Class(:X))
  Declaration(ObjectProperty(:r))
  Declaration(ObjectProperty(:t))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  Declaration(NamedIndividual(:c))

  DisjointClasses(:T :X)
  SubObjectPropertyOf(ObjectPropertyChain(:r :r) :t)
  ObjectPropertyDomain(:t :T)

  ClassAssertion(:X :a)
  ObjectPropertyAssertion(:r :a :b)
  ObjectPropertyAssertion(:r :b :c)
",
    );
    // r(a,b) + r(b,c) → chain → t(a,c); domain(t)=T → a:T; a also :X; Disjoint(T,X) → clash
    assert!(clash, "role chain + domain + disjoint must detect clash");
}

// ── Negative (no clash — soundness guards) ────────────────────────────────

/// A consistent ABox with inverses and chains must NOT be flagged as inconsistent.
/// No disjoint classes ⟹ rules 7b/8 cannot fire.
#[test]
fn consistent_inverse_chain_no_clash() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:A))
  Declaration(Class(:B))
  Declaration(ObjectProperty(:r))
  Declaration(ObjectProperty(:s))
  Declaration(ObjectProperty(:t))
  Declaration(NamedIndividual(:x))
  Declaration(NamedIndividual(:y))
  Declaration(NamedIndividual(:z))

  InverseObjectProperties(:r :s)
  SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)
  SubClassOf(:A :B)

  ClassAssertion(:A :x)
  ObjectPropertyAssertion(:r :x :y)
  ObjectPropertyAssertion(:r :y :z)
",
    );
    assert!(!clash, "consistent ABox (no disjoint) must NOT be flagged as inconsistent");
}

/// Consistent ontology WITH functional + inverse + disjoint but NO clash.
/// Guard: r(a,b) with Functional(r), DisjointClasses(C,D), a:C, but b is NOT typed D.
/// This is the critical soundness guard: functional + inverse + disjoint present,
/// but the witnesses don't actually clash.
#[test]
fn functional_inverse_disjoint_no_clash_when_witnesses_safe() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(ObjectProperty(:hasSex))
  Declaration(NamedIndividual(:pat))
  Declaration(NamedIndividual(:genderFiller))

  DisjointClasses(:Male :Female)
  FunctionalObjectProperty(:hasSex)

  ClassAssertion(:Male :genderFiller)
  ObjectPropertyAssertion(:hasSex :pat :genderFiller)
",
    );
    // pat has exactly ONE hasSex filler typed Male — Functional is satisfied,
    // no second filler with a disjoint type exists → no clash.
    assert!(!clash, "single functional filler (Male) with no disjoint witness must NOT clash");
}

/// Domain/range propagation with a consistent assignment must NOT clash.
#[test]
fn domain_range_no_clash() {
    let clash = parse_and_check(
        r"
  Declaration(Class(:Person))
  Declaration(Class(:Animal))
  Declaration(ObjectProperty(:owns))
  Declaration(NamedIndividual(:alice))
  Declaration(NamedIndividual(:dog))

  ObjectPropertyDomain(:owns :Person)
  ObjectPropertyRange(:owns :Animal)

  ObjectPropertyAssertion(:owns :alice :dog)
",
    );
    // alice:Person and dog:Animal — not disjoint → no clash.
    assert!(!clash, "domain/range propagation with compatible types must NOT clash");
}
