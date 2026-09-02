//! `classify` now consults the ABox-seeded WEDGE consistency route (#89).
//!
//! Before this, `classify --json` reported `"consistent": true` on ontologies that
//! `rustdl consistent`, Konclude AND Kobayashi-MaRust all call inconsistent
//! (`ore_ont_16321`, `ore_ont_4198`). Two surfaces of one binary, opposite answers.
//!
//! ## Why the wedge, and not the two signals classify already had
//!
//! Traced on both reproducers the verdict reads `consistency: wedge Unsat`. The
//! saturator does not see it, and `abox_saturation` cannot: it propagates over
//! NAMED individuals with no witness generation, while the clash sits at a DATA
//! SUCCESSOR — a `DataPropertyRange` violated by an assertion. That is out of its
//! reach by construction, not by budget.
//!
//! #89 framed the fix as needing DKey-aware ABox saturation or a `decide(Top)`
//! probe. Neither was required. The design record separately calls a `decide(Top)`
//! probe here "a measured dead-end (hangs on consistent alehif/pizza)" — that is
//! about an UNBOUNDED probe; the wedge route is bounded and measures 2.34 ms on
//! pizza.
//!
//! ## Direction of risk
//!
//! Only a witnessed `Unsat` returns true. `Sat` and `Stalled` both mean "no clash
//! seen", the same sound under-approximation classify already made — so this can
//! only ever ADD detections, never invent one. The negative controls below are the
//! load-bearing half: a spurious inconsistency marks EVERY class unsatisfiable.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(\n{body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

fn classify_says_consistent(o: &SetOntology<RcStr>) -> bool {
    !owl_dl_reasoner::classify(o).unwrap().stats().inconsistent
}

/// #89's own two-axiom reproducer. `xsd:float` and `xsd:double` have disjoint
/// value spaces, so the asserted value cannot lie in the declared range.
const FLOAT_IN_DOUBLE_RANGE: &str = r#"Declaration(DataProperty(:p))
    Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:double)
    DataPropertyAssertion(:p :a "1.0"^^xsd:float)"#;

/// Detection, cross-surface agreement, and the flag's load-bearing-ness in ONE
/// test — deliberately.
///
/// These were three `#[test]`s. Two depend on the flag being ON and the third
/// mutates it, and cargo runs a binary's tests concurrently, so the process-global
/// write raced the readers: **15 of 15 runs failed** at default concurrency while
/// passing under `--test-threads=1`. That is the same defect #92/#94 documented in
/// `realize_incomplete_signal.rs`, caught here before it shipped rather than after.
///
/// The negative controls below stay separate because they are flag-INDEPENDENT: the
/// route only ever ADDS detections, so a consistent ontology stays consistent
/// whichever way the flag is set.
#[test]
fn wedge_route_detects_and_the_flag_is_load_bearing() {
    let o = onto(FLOAT_IN_DOUBLE_RANGE);

    // ── the fix: classify no longer reports consistent on a KB the wedge refutes
    assert!(
        !classify_says_consistent(&o),
        "classify must not report consistent on a KB the wedge refutes (#89)"
    );

    // ── the point of #89 was that the two surfaces DISAGREED; pin that they agree
    for body in [
        FLOAT_IN_DOUBLE_RANGE,
        // an ordinary ABox clash, which both surfaces already caught
        r"Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
          DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x)",
    ] {
        let each = onto(body);
        assert_eq!(
            classify_says_consistent(&each),
            owl_dl_reasoner::is_consistent(&each).unwrap(),
            "classify and is_consistent must agree on:\n{body}"
        );
    }

    // ── the flag is load-bearing: with it off, #89 reproduces. If this ever
    // fails, the defect closed by another route and the flag can retire.
    let _g = EnvGuard::set("0");
    assert!(
        classify_says_consistent(&o),
        "with RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY=0 the pre-existing #89 behaviour \
         must reappear; if it does not, this fix is not what is doing the work"
    );
}

// ── NEGATIVE CONTROLS: a spurious inconsistency marks EVERY class unsat ──────

/// An ABox-bearing but perfectly consistent ontology must stay consistent. This
/// is the shape the new route runs on, so it is where a false positive would land.
#[test]
fn a_consistent_abox_stays_consistent() {
    let o = onto(
        r#"Declaration(Class(:A)) Declaration(Class(:B))
        Declaration(DataProperty(:p)) Declaration(NamedIndividual(:x))
        SubClassOf(:A :B)
        ClassAssertion(:A :x)
        DataPropertyRange(:p xsd:double)
        DataPropertyAssertion(:p :x "1.0"^^xsd:double)"#,
    );
    assert!(
        classify_says_consistent(&o),
        "matching datatypes must not clash"
    );
}

/// The soundness subtlety this pre-check has always had to respect:
/// all-named-classes-unsatisfiable is NOT inconsistency. `{A ⊑ ⊥, B ⊑ ⊥}` empties
/// every named class yet has a non-empty model. Mirrors the canary in
/// `classify_inconsistency.rs`, re-asserted here because this change adds a new
/// route into the same verdict.
#[test]
fn all_classes_unsat_is_still_consistent() {
    let o = onto(
        r"Declaration(Class(:A)) Declaration(Class(:B))
        Declaration(NamedIndividual(:x)) Declaration(ObjectProperty(:r))
        SubClassOf(:A owl:Nothing)
        SubClassOf(:B owl:Nothing)
        ObjectPropertyAssertion(:r :x :x)",
    );
    assert!(
        classify_says_consistent(&o),
        "every named class empty is not an inconsistency — the test is that TOP is unsat"
    );
}

struct EnvGuard(Option<std::ffi::OsString>);
impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(v: &str) -> Self {
        let prior = std::env::var_os("RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY");
        // SAFETY: the only test in this binary that mutates the environment.
        unsafe { std::env::set_var("RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY", v) };
        Self(prior)
    }
}
impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match self.0.take() {
                Some(v) => std::env::set_var("RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY", v),
                None => std::env::remove_var("RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY"),
            }
        }
    }
}
