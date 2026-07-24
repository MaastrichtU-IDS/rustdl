//! Integration tests for `same_individuals` / `different_individuals`
//! (issue #46).
//!
//! Includes a hand-verified soundness/completeness oracle
//! (`{same,different}_individuals_soundness` below). Unlike
//! `disjoint_oracle.rs` (issue #47), which diffs against a genuine
//! HermiT-materialized `.owx`, this oracle has NO ROBOT/HermiT counterpart:
//! ROBOT (and the OWLAPI `InferredAxiomGenerator` set it wraps) has no
//! `SameIndividual` / `DifferentIndividuals` inferred-axiom generator —
//! confirmed empirically against `obolibrary/robot:v1.9.6`:
//!
//! ```text
//! $ docker run --rm obolibrary/robot:v1.9.6 robot reason --reasoner hermit \
//!     --axiom-generators "SameIndividual DifferentIndividuals" \
//!     --input in.ofn --output out.owx
//! reason#UNKNOWN AXIOM GENERATOR SameIndividual is not a valid inferred
//! axiom generator
//! ```
//!
//! So the ground truth here is instead `tests/fixtures/individuals/inds.ofn`,
//! a fixture deliberately kept small enough that every same/different
//! entailment is trivially hand-derivable, plus the `expected_*` constants
//! below (manually derived, treated as ground truth):
//!
//!   - `Functional(:hasParent)` + `:hasParent(:a,:b)` + `:hasParent(:a,:c)`
//!     ⇒ `:b = :c` (functional-role-forced same).
//!   - `DisjointClasses(:Dog :Cat)` + `:Dog(:x)` + `:Cat(:y)` ⇒ `:x ≠ :y`
//!     (disjoint-type-forced different).
//!   - `SameIndividual(:p :q)` + `SameIndividual(:q :s)` ⇒ `{:p, :q, :s}`
//!     (asserted same-as chain, transitively closed).
//!
//! No other same/different relationship is entailed among the fixture's 8
//! named individuals, so both directions (FP and MISSED) are exercised
//! cleanly against an exact expected set. FP (rustdl reports something the
//! hand-derived oracle does not) is the hard soundness guard (issue #46) and
//! is asserted unconditionally, regardless of completeness — never weaken
//! this. MISSED (hand-derived oracle expects, rustdl omits) is allowed only
//! when rustdl itself reports `incomplete() == true`.
//!
//! Note on `incomplete()`: `different_individuals` never times out here (no
//! `pair_deadline` is set, so every pairwise probe runs to completion), so
//! its `incomplete()` is `false` and MISSED is a hard assertion in practice.
//! `same_individuals::incomplete()` is documented as conservatively `true`
//! whenever ANY pairwise extension probe beyond the told/functional-merge
//! seed is consulted at all — unavoidable once a fixture has more named
//! individuals than the seed alone resolves — so it is `true` here, and
//! MISSED is checked against the hand-derived set but only hard-asserted
//! when `!incomplete()` (it still holds — the checked-but-not-hard-asserted
//! MISSED set is empty).
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::io::Cursor;
use std::path::Path;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

#[test]
fn same_from_functional_role() {
    // Functional(r); r(a,b); r(a,c) ⇒ b=c.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c)) Declaration(ObjectProperty(:r))
            FunctionalObjectProperty(:r)
            ObjectPropertyAssertion(:r :a :b)
            ObjectPropertyAssertion(:r :a :c))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(
        s.groups()
            .iter()
            .any(|g| g.contains(&"http://ex/#b".to_string())
                && g.contains(&"http://ex/#c".to_string())),
        "expected {{b,c}} group, got {:?}",
        s.groups()
    );
}

#[test]
fn different_from_disjoint_types() {
    // A,B disjoint; a:A, b:B ⇒ a≠b.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :b))",
    );
    let d = owl_dl_reasoner::different_individuals(&o, None).unwrap();
    assert!(
        d.pairs()
            .iter()
            .any(|(x, y)| (x == "http://ex/#a" && y == "http://ex/#b")
                || (x == "http://ex/#b" && y == "http://ex/#a")),
        "expected a≠b, got {:?}",
        d.pairs()
    );
}

#[test]
fn same_individuals_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        owl_dl_reasoner::same_individuals(&o, None),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

#[test]
fn different_individuals_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        owl_dl_reasoner::different_individuals(&o, None),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

#[test]
fn same_individuals_told_seed_only_is_complete() {
    // Only one asserted SameIndividual pair, no other named individuals to
    // probe against ⇒ the seed alone resolves everything ⇒ incomplete=false.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
            SameIndividual(:a :b))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(!s.incomplete(), "seed-only case must be complete");
    assert!(
        s.groups()
            .iter()
            .any(|g| g.contains(&"http://ex/#a".to_string())
                && g.contains(&"http://ex/#b".to_string())),
        "expected {{a,b}} group, got {:?}",
        s.groups()
    );
}

#[test]
fn same_individuals_probe_sets_incomplete() {
    // Two unrelated named individuals with nothing forcing them same or
    // different ⇒ a probe is consulted ⇒ incomplete=true (per this query's
    // conservative policy), and they must NOT be reported same.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(
        s.incomplete(),
        "an extension probe must have been consulted"
    );
    assert!(
        !s.groups()
            .iter()
            .any(|g| g.contains(&"http://ex/#a".to_string())),
        "unrelated individuals must not be merged, got {:?}",
        s.groups()
    );
}

#[test]
fn anonymous_individuals_are_skipped() {
    // A blank-node individual should never appear in same/different output.
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A))
            ClassAssertion(:A _:anon))",
    );
    let s = owl_dl_reasoner::same_individuals(&o, None).unwrap();
    assert!(
        s.groups()
            .iter()
            .all(|g| g.iter().all(|i| !i.contains("_:")))
    );
    let d = owl_dl_reasoner::different_individuals(&o, None).unwrap();
    assert!(d.pairs().is_empty());
}

/// Loads `tests/fixtures/individuals/inds.ofn` (see module doc for what it
/// hand-encodes).
fn load_inds_fixture() -> SetOntology<RcStr> {
    let path = Path::new("tests/fixtures/individuals/inds.ofn");
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
        .0
}

/// Hand-derived expected `different_individuals` pairs for the fixture (see
/// module doc): only the disjoint-type-forced `:x ≠ :y`.
fn expected_different_pairs() -> BTreeSet<(String, String)> {
    [("urn:inds#x".to_string(), "urn:inds#y".to_string())]
        .into_iter()
        .collect()
}

/// Hand-derived expected `same_individuals` groups for the fixture (see
/// module doc), each a sorted `BTreeSet<String>` so neither group-internal
/// nor outer ordering can cause a spurious mismatch: the functional-forced
/// `{b, c}` and the asserted same-as chain `{p, q, s}`.
fn expected_same_groups() -> BTreeSet<BTreeSet<String>> {
    [
        vec!["urn:inds#b", "urn:inds#c"],
        vec!["urn:inds#p", "urn:inds#q", "urn:inds#s"],
    ]
    .into_iter()
    .map(|g| g.into_iter().map(str::to_string).collect())
    .collect()
}

/// Hand-verified soundness/completeness oracle for `different_individuals`
/// (issue #46) — see the module doc for why this is hand-derived rather than
/// ROBOT/HermiT-generated.
#[test]
fn different_individuals_soundness() {
    let o = load_inds_fixture();
    let result = owl_dl_reasoner::different_individuals(&o, None).expect("different_individuals");
    let got: BTreeSet<(String, String)> = result.pairs().iter().cloned().collect();
    let expected = expected_different_pairs();

    let fp: Vec<_> = got.difference(&expected).collect();
    // Soundness guard (issue #46): rustdl must never report a pair the
    // hand-derived oracle doesn't. Always asserted, regardless of
    // completeness.
    assert!(
        fp.is_empty(),
        "FP — rustdl reports, hand-derived oracle does not: {fp:?}"
    );

    let missed: Vec<_> = expected.difference(&got).collect();
    if result.incomplete() {
        if !missed.is_empty() {
            eprintln!("MISSED (allowed, different_individuals reported incomplete): {missed:?}");
        }
    } else {
        assert!(
            missed.is_empty(),
            "MISSED — hand-derived oracle expects, rustdl omits (and rustdl did not report \
             incomplete): {missed:?}"
        );
    }
}

/// Hand-verified soundness/completeness oracle for `same_individuals`
/// (issue #46) — see the module doc for why this is hand-derived rather than
/// ROBOT/HermiT-generated. Groups are compared as sets of `BTreeSet<String>`
/// so group ordering (outer or inner) never causes a spurious mismatch.
#[test]
fn same_individuals_soundness() {
    let o = load_inds_fixture();
    let result = owl_dl_reasoner::same_individuals(&o, None).expect("same_individuals");
    let got: BTreeSet<BTreeSet<String>> = result
        .groups()
        .iter()
        .map(|g| g.iter().cloned().collect())
        .collect();
    let expected = expected_same_groups();

    let fp: Vec<_> = got.difference(&expected).collect();
    // Soundness guard (issue #46): every rustdl same-group must be genuinely
    // entailed per the hand-derived oracle. Always asserted, regardless of
    // completeness.
    assert!(
        fp.is_empty(),
        "FP — rustdl reports a same-group the hand-derived oracle does not: {fp:?}"
    );

    let missed: Vec<_> = expected.difference(&got).collect();
    if result.incomplete() {
        if !missed.is_empty() {
            eprintln!("MISSED (allowed, same_individuals reported incomplete): {missed:?}");
        }
    } else {
        assert!(
            missed.is_empty(),
            "MISSED — hand-derived oracle expects, rustdl omits (and rustdl did not report \
             incomplete): {missed:?}"
        );
    }
}
