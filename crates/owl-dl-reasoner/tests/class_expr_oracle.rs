#![allow(clippy::doc_markdown)]
//! External completeness/soundness oracle for the complex class-expression
//! queries (issue #48): `class_expression_entailed_subclass` and
//! `class_expression_instances` are diffed against **HermiT**-inferred
//! axioms over the same fresh PROBE class the reasoner itself mints
//! (`EquivalentClasses(<urn:rustdl-ce-probe:q> ObjectUnionOf(:A :B))`). FP
//! (rustdl reports, HermiT does not) is the hard soundness guard and must
//! always be empty; MISSED (HermiT infers, rustdl omits) is allowed to be
//! non-empty only when rustdl reports its own `incomplete() == true`.
//!
//! The oracle is generated offline by `docker/robot/class-expr-oracle.sh`
//! (ROBOT + embedded HermiT) and committed as `ce-materialized.owx`, so this
//! test needs no docker at run time.
//!
//! Regenerate after changing the fixture:
//!   1. append the SAME probe axiom `class_expression_*` injects to a copy of
//!      `ce.ofn` (insert before the ontology's closing paren):
//!      `EquivalentClasses(<urn:rustdl-ce-probe:q> ObjectUnionOf(:A :B))`
//!   2. bash docker/robot/class-expr-oracle.sh /path/to/ce-with-probe.ofn
//!      crates/owl-dl-reasoner/tests/fixtures/class_expr/ce-materialized.owx
//!
//! `class-expr-oracle.sh` runs `robot reason --reasoner hermit
//! --axiom-generators "SubClass ClassAssertion" --include-indirect true` — the
//! `--include-indirect true` flag is required, otherwise ROBOT's inferred-axiom
//! generators only assert each individual's/class's MOST SPECIFIC type (e.g.
//! `x`'s direct type stays `:A`, never the strictly-more-general probe union),
//! silently dropping the very axioms this oracle needs.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::{
    Build, ClassAssertion, ClassExpression, Component, Individual, RcStr, SubClassOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{class_expression_entailed_subclass, class_expression_instances};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const PROBE_IRI: &str = "urn:rustdl-ce-probe:q";
const C_IRI: &str = "http://ex/ce#C";

fn fixture_dir() -> &'static Path {
    Path::new("tests/fixtures/class_expr")
}

fn load_fixture() -> SetOntology<RcStr> {
    let file = File::open(fixture_dir().join("ce.ofn")).expect("fixture ce.ofn");
    let mut reader = BufReader::new(file);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse ce.ofn")
        .0
}

fn load_oracle() -> SetOntology<RcStr> {
    let path = fixture_dir().join("ce-materialized.owx");
    let file = File::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    read_owx(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
        .0
}

/// Does the committed oracle contain `SubClassOf(<probe> :C)` — i.e. did
/// HermiT infer the probe class (`A ⊔ B`) is a subclass of `C`?
fn oracle_probe_subclass_of_c(oracle: &SetOntology<RcStr>) -> bool {
    oracle.iter().any(|ax| {
        if let Component::SubClassOf(SubClassOf { sub, sup }) = &ax.component {
            matches!(sub, ClassExpression::Class(c) if c.0.to_string() == PROBE_IRI)
                && matches!(sup, ClassExpression::Class(c) if c.0.to_string() == C_IRI)
        } else {
            false
        }
    })
}

/// Collect `ClassAssertion(<probe> <ind>)` inferred axioms from the committed
/// oracle — HermiT's instances of the probe class (named individuals only).
fn oracle_probe_instances(oracle: &SetOntology<RcStr>) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for ax in oracle {
        if let Component::ClassAssertion(ClassAssertion { ce, i }) = &ax.component
            && matches!(ce, ClassExpression::Class(c) if c.0.to_string() == PROBE_IRI)
            && let Individual::Named(ind) = i
        {
            set.insert(ind.0.to_string());
        }
    }
    set
}

/// `class_expression_entailed_subclass(A⊔B, C)` must equal HermiT's verdict on
/// the same probe class (`SubClassOf(<probe> :C)` present in the oracle).
/// Since HermiT is sound+complete here, rustdl must not claim entailment
/// HermiT doesn't (the FP-direction of issue #48's soundness guard); on this
/// simple/complete fixture the verdicts must match exactly.
#[test]
fn class_expr_entailed_subclass_matches_hermit() {
    let o = load_fixture();
    let oracle = load_oracle();
    let b = Build::<RcStr>::new();
    let a_or_b = ClassExpression::ObjectUnionOf(vec![
        ClassExpression::Class(b.class("http://ex/ce#A")),
        ClassExpression::Class(b.class("http://ex/ce#B")),
    ]);
    let c = ClassExpression::Class(b.class(C_IRI));

    let verdict = class_expression_entailed_subclass(&o, &a_or_b, &c).expect("entailed_subclass");
    let hermit_says = oracle_probe_subclass_of_c(&oracle);

    // FP=0 soundness guard (issue #48): rustdl must never claim an entailment
    // HermiT doesn't. Always asserted, regardless of completeness.
    assert!(
        !verdict.holds() || hermit_says,
        "FP — rustdl claims A⊔B ⊑ C entailed, HermiT does not"
    );
    // This fixture is simple/complete for both engines, so the verdicts should
    // match exactly (MISS would also be a bug here, not just a soft signal).
    // Gated on `!incomplete()` (mirrors the instances test's pattern below): if
    // a future fixture edit grows past the EL fragment and rustdl legitimately
    // returns a sound MISS, it self-reports `incomplete()` and that must not
    // fire as a hard failure indistinguishable from a real FP.
    if verdict.incomplete() {
        if verdict.holds() != hermit_says {
            eprintln!(
                "MISS (allowed, class_expression_entailed_subclass reported incomplete): \
                 rustdl={:?} hermit={hermit_says:?}",
                verdict.holds()
            );
        }
    } else {
        assert_eq!(
            verdict.holds(),
            hermit_says,
            "rustdl verdict {:?} != HermiT oracle {hermit_says:?} (incomplete={:?})",
            verdict.holds(),
            verdict.incomplete()
        );
    }
}

/// `class_expression_instances(A⊔B)` vs HermiT's instances of the same probe
/// class. FP-direction (rustdl ⊆ oracle) is UNCONDITIONAL — issue #48's
/// soundness guard for the CE reduction. MISSED (oracle − got) is gated on
/// `!incomplete()`.
#[test]
fn class_expr_instances_matches_hermit() {
    let o = load_fixture();
    let oracle = load_oracle();
    let b = Build::<RcStr>::new();
    let a_or_b = ClassExpression::ObjectUnionOf(vec![
        ClassExpression::Class(b.class("http://ex/ce#A")),
        ClassExpression::Class(b.class("http://ex/ce#B")),
    ]);

    let result = class_expression_instances(&o, &a_or_b).expect("instances");
    let got: BTreeSet<String> = result.individuals().iter().cloned().collect();
    let oracle_instances = oracle_probe_instances(&oracle);

    let fp: Vec<_> = got.difference(&oracle_instances).collect();
    assert!(
        fp.is_empty(),
        "FP — rustdl reports an instance of A⊔B HermiT does not: {fp:?}"
    );

    let missed: Vec<_> = oracle_instances.difference(&got).collect();
    if result.incomplete() {
        if !missed.is_empty() {
            eprintln!(
                "MISSED (allowed, class_expression_instances reported incomplete): {missed:?}"
            );
        }
    } else {
        assert!(
            missed.is_empty(),
            "MISSED — HermiT infers an instance of A⊔B rustdl omits (and rustdl did not report incomplete): {missed:?}"
        );
    }
}
