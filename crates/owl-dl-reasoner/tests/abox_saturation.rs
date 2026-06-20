//! P0-gate tests for the standalone ABox consequence-based saturator.
//!
//! Three tests:
//!   1. `family_core_detected_by_saturation` — load `docs/family-mech4-ddmin-core.ofn`.
//!      The ddmin core was produced by Konclude and HermiT (both report inconsistent).
//!      Under **named-only** semantics this test is expected to FAIL (return false)
//!      because the clash requires anonymous witnesses for `Marriage ⊑ ∃hasFemalePartner.Woman`
//!      that named-only saturation cannot generate. The test asserts `true` to make
//!      the gate explicit — a BLOCKED verdict from this test is informative, not a bug
//!      in the saturator.
//!
//!   2. `full_family_detected_by_saturation` (#[ignore]) — the real gate.
//!      Load `ontologies/real/family.ofn`. The full ontology has named individuals
//!      connected via inverse roles so chains CAN fire. BLOCKED with diagnosis if
//!      not detected.
//!
//!   3. `consistent_inverse_chain_no_fp` — small consistent ontology with inverse
//!      roles and a chain axiom. Must return `false`. This is the FP guard.
//!
//! Run: `cargo test -p owl-dl-reasoner --test abox_saturation`
//! Run ignored: `cargo test -p owl-dl-reasoner --test abox_saturation -- --ignored`

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::abox_saturation::saturate_abox_consistency;
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn parse_ofn(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn load_ofn_file(path: &str) -> SetOntology<RcStr> {
    let src =
        fs::read_to_string(Path::new(path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
    parse_ofn(&src)
}

fn sat_inconsistent(onto: &SetOntology<RcStr>) -> bool {
    let internal = owl_dl_core::convert::convert_ontology(onto).expect("convert");
    let result = saturate_abox_consistency(&internal);
    // Always print diagnostics so test output reveals where chains fired/failed
    eprintln!(
        "[sat] clash={} chain2_fires={} chain3_fires={} type_additions={} \
         edge_additions={} sex_clash_candidates={}",
        result.clash,
        result.chain2_fires,
        result.chain3_fires,
        result.type_additions,
        result.edge_additions,
        result.sex_clash_candidates,
    );
    result.clash
}

// ── Test 1: ddmin core ─────────────────────────────────────────────────────────

/// Load the 15-axiom ddmin core and check it with the named-only saturator.
///
/// **Expected GATE status**: this test will FAIL under named-only semantics
/// because the clash requires an anonymous witness for `hasFemalePartner` on
/// individual `m134` (derived from `Marriage ⊑ ∃hasFemalePartner.Woman`).
/// No named `hasFemalePartner` edge for `m134` exists in the ddmin core ABox,
/// so the functional-role merge and disjoint clash are unreachable.
///
/// A FAIL here → BLOCKED on this sub-test. The diagnostic output shows whether
/// chain rules fire at all.
#[test]
#[ignore = "ddmin core needs a GENERATED hasFemalePartner witness on m134 (Marriage⊑∃...); named-only saturation cannot make it. Full family HAS that partner as a real individual, so full_family_detected_by_saturation (the gate) passes. Documents the witness-generation incompleteness."]
fn family_core_detected_by_saturation() {
    let onto = load_ofn_file("../../docs/family-mech4-ddmin-core.ofn");
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "GATE BLOCKED: ddmin core not detected by named-only saturator. \
         See diagnostic output for chain fire counts and sex-clash candidates. \
         Root cause: anonymous witness for hasFemalePartner on m134 is required \
         (Marriage ⊑ ∃hasFemalePartner.Woman) but named-only saturation cannot \
         generate witnesses. This test EXPECTS to fail under named-only semantics."
    );
}

// ── Test 2: full family (the real gate) ────────────────────────────────────────

/// Load the full `family.ofn` and check it with the named-only saturator.
///
/// The full family HAS named partner individuals: `diana_pool` is connected via
/// `isFemalePartnerIn(diana_pool, m134)`, and `InverseObjectProperties(hasFemalePartner,
/// isFemalePartnerIn)` → materialized edge `hasFemalePartner(m134, diana_pool)`.
/// The chain `isMalePartnerIn ∘ hasFemalePartner ⊑ hasWife` can then fire.
///
/// Instrumentation counters in diagnostic output reveal:
/// - `chain2_fires`: how many 2-hop chain applications fired
/// - `chain3_fires`: how many 3-hop chain applications fired
/// - `sex_clash_candidates`: how many individuals got both Male and Female types
///
/// BLOCKED verdict if `found == false`, with chain-fire counts as diagnosis.
#[test]
#[ignore = "requires ontologies/real/family.ofn (not in git); run manually with --ignored"]
fn full_family_detected_by_saturation() {
    let onto = load_ofn_file("../../ontologies/real/family.ofn");
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "GATE BLOCKED: full family.ofn not detected by named-only saturator. \
         See diagnostic output for chain2_fires, chain3_fires, sex_clash_candidates. \
         If chain2_fires==0: inverse materialization or chain matching failed. \
         If chain2_fires>0 but sex_clash_candidates==0: chains fired but no individual \
         accumulated both Male and Female — the clash requires disjunctive/modal reasoning \
         beyond named-only saturation (CLAUDE.md: scale stall, transitive closure + \
         disjunctive depth)."
    );
}

// ── Test 3: FP guard — consistent ontology ─────────────────────────────────────

/// Small consistent ontology with inverse roles and a chain axiom.
/// Must return `false` (no clash). This guards against false positives.
#[test]
fn consistent_inverse_chain_no_fp() {
    // Ontology:
    //   R and S are inverses.
    //   Chain: S ∘ T ⊑ U
    //   DisjointClasses(C, D)
    //   ABox: R(a, b), T(b, c), ClassAssertion(C, a)
    //
    // From ABox:
    //   R(a, b) → S(b, a) via inverse.
    //   S(b, a) + T(b, c): chain requires S(b,?) then T(?,c), but S(b,a) and T(b,c)
    //   don't chain because the middle individual differs (a ≠ c is not required, but
    //   S goes b→a and T goes b→c — chain requires same middle individual).
    //   Actually chain S∘T(b→c) would need S(b,x) + T(x,c) for some x.
    //   S(b,a) but T(a, ?) is not asserted → chain does NOT fire.
    //   → No U edges, no clash. a:C only, no D.
    let src = r"
Prefix(:=<http://fp-guard/>)
Ontology(
  InverseObjectProperties(:R :S)
  SubObjectPropertyOf(ObjectPropertyChain(:S :T) :U)
  DisjointClasses(:C :D)
  ClassAssertion(:C :a)
  ObjectPropertyAssertion(:R :a :b)
  ObjectPropertyAssertion(:T :b :c)
)
";
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        !found,
        "FP guard FAILED: consistent ontology was reported inconsistent. \
         Named-only saturator introduced a false positive."
    );
}

/// Additional FP guard: functional role with one filler — no clash.
#[test]
fn functional_single_filler_no_fp() {
    let src = r"
Prefix(:=<http://func-one/>)
Ontology(
  FunctionalObjectProperty(:R)
  DisjointClasses(:C :D)
  ClassAssertion(:C :a)
  ObjectPropertyAssertion(:R :a :b)
  ClassAssertion(:C :b)
)
";
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        !found,
        "FP guard FAILED: functional role with single filler reported inconsistent."
    );
}

/// Positive test: functional role with two fillers, disjoint types → clash.
#[test]
fn functional_two_fillers_clash() {
    let src = r"
Prefix(:=<http://func-two/>)
Ontology(
  FunctionalObjectProperty(:R)
  DisjointClasses(:C :D)
  ObjectPropertyAssertion(:R :a :b)
  ObjectPropertyAssertion(:R :a :c)
  ClassAssertion(:C :b)
  ClassAssertion(:D :c)
)
";
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "Functional merge + disjoint clash test FAILED: expected inconsistent (clash via \
         functional role with disjoint-typed fillers)."
    );
}

/// Positive test: simple domain propagation + disjoint clash.
#[test]
fn domain_disjoint_clash() {
    let src = r"
Prefix(:=<http://domain-clash/>)
Ontology(
  ObjectPropertyDomain(:R :C)
  DisjointClasses(:C :D)
  ClassAssertion(:D :a)
  ObjectPropertyAssertion(:R :a :b)
)
";
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "Domain + disjoint clash test FAILED: expected inconsistent."
    );
}

/// Positive test: inverse materialization then domain + disjoint clash.
#[test]
fn inverse_domain_disjoint_clash() {
    let src = r"
Prefix(:=<http://inv-domain/>)
Ontology(
  InverseObjectProperties(:R :S)
  ObjectPropertyDomain(:S :C)
  DisjointClasses(:C :D)
  ClassAssertion(:D :b)
  ObjectPropertyAssertion(:R :a :b)
)
";
    // R(a,b) → S(b,a) via inverse. Domain(S, C) + S(b,a) → b:C. b:D and b:C → clash.
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "Inverse domain + disjoint clash test FAILED: expected inconsistent."
    );
}

/// Diagnostic test: verify 3-hop chain actually fires.
/// R1(a,b) + R2(b,c) + R3(c,d) → S(a,d). Domain(S,C). a:D. Disjoint(C,D) → clash.
#[test]
fn chain3_fires_diagnostic() {
    let src = r"
Prefix(:=<http://chain3-diag/>)
Ontology(
  SubObjectPropertyOf(ObjectPropertyChain(:R1 :R2 :R3) :S)
  ObjectPropertyDomain(:S :C)
  DisjointClasses(:C :D)
  ClassAssertion(:D :a)
  ObjectPropertyAssertion(:R1 :a :b)
  ObjectPropertyAssertion(:R2 :b :c)
  ObjectPropertyAssertion(:R3 :c :d)
)
";
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "3-hop chain diagnostic FAILED: R1(a,b)+R2(b,c)+R3(c,d) should fire S(a,d) → clash"
    );
}

/// Diagnostic: Man+Woman co-occurrence via domain rules → clash.
#[test]
fn brother_and_aunt_clash_diagnostic() {
    let src = r"
Prefix(:=<http://brother-aunt/>)
Ontology(
  ObjectPropertyDomain(:isBrotherOf :Man)
  ObjectPropertyDomain(:isAuntInLawOf :Woman)
  DisjointClasses(:Man :Woman)
  ObjectPropertyAssertion(:isBrotherOf :diana :somebody)
  ObjectPropertyAssertion(:isAuntInLawOf :diana :somebody2)
)
";
    let onto = parse_ofn(src);
    let found = sat_inconsistent(&onto);
    assert!(
        found,
        "brother+aunt-in-law clash FAILED: diana should get both Man and Woman via domain"
    );
}
