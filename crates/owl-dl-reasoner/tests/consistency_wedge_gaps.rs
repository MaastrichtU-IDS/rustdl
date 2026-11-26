//! #558 review: each path that reports the trusted consistency wedge's weaker KB.
//!
//! A `DisjointUnion` with a complex member makes the clausifier defer its covering
//! half (`disjoint-union-covering`), so the consistency wedge's KB is weaker by one
//! deferral. The fixtures here are consistent, except the labelled pin of the #600
//! miss.
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialised via ENV_MUTEX,
        // restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Off pure EL (the union), with an `ABox`, no nominal: the consistency wedge decides
/// it and its `Sat` is trusted. The `DisjointUnion` adds one deferral.
fn onto(extra: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex/gaps>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:E)) Declaration(Class(:F)) Declaration(Class(:G)) Declaration(Class(:U))\n\
         Declaration(ObjectProperty(:r)) Declaration(DataProperty(:p))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         SubClassOf(:A ObjectUnionOf(:B :F))\n\
         ClassAssertion(:A :a)\n\
         DisjointUnion(:E :F ObjectSomeValuesFrom(:r :G))\n\
         ClassAssertion(:E :b)\n\
         {extra}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// classify's consistency PROBE (layer 2) records the gaps on its own: with the
/// pre-check's wedge route off, an unsatisfiable class admits the probe and its
/// wedge `Sat` is the one trusted.
#[test]
fn the_classify_probe_route_records_the_gaps() {
    let _l = lock();
    let _off = SetEnvGuard::set("RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY", "0");
    let c = owl_dl_reasoner::classify(&onto("SubClassOf(:U owl:Nothing)")).unwrap();
    let s = c.stats();
    assert!(s.consistency_probe_admitted, "precondition: the probe ran");
    assert!(
        s.relies_on_trusted_consistency,
        "precondition: its Sat is trusted"
    );
    assert_eq!(s.consistency_wedge_clauses_deferred, 1);
}

/// When a main-tableau run verifies consistency after the wedge's `Sat` was
/// trusted (here: the probe's layer 3, which a data-counting `Sat` admits), nothing
/// rests on the wedge any more, so its gaps are masked to 0.
#[test]
fn verified_consistency_masks_the_gaps() {
    let _l = lock();
    let c = owl_dl_reasoner::classify(&onto(
        "SubClassOf(:A DataMaxCardinality(2 :p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         DataPropertyAssertion(:p :a \"1\"^^xsd:integer)",
    ))
    .unwrap();
    let s = c.stats();
    assert!(s.consistency_probe_admitted, "precondition: the probe ran");
    assert!(
        !s.relies_on_trusted_consistency,
        "the main tableau verified it"
    );
    assert_eq!(s.consistency_wedge_clauses_deferred, 0, "masked");
    assert_eq!(s.consistency_wedge_clauses_refused, 0, "masked");
}

/// A single query that drops its `ABox` on the trusted wedge `Sat` (#401) reports the
/// wedge's deferrals, for satisfiability and subsumption alike.
#[test]
fn single_queries_report_the_deferral() {
    let _l = lock();
    // The seeded probe would answer first (#401's seeded-first budget); `0` goes
    // straight to the consistency step and the `ABox`-free probe.
    let _s = SetEnvGuard::set("RUSTDL_QUERY_SEEDED_FIRST_MS", "0");
    let o = onto("");
    let (_, s) = owl_dl_reasoner::is_class_satisfiable_with_stats(&o, "http://ex/#C").unwrap();
    assert!(
        s.relies_on_trusted_consistency,
        "precondition: the ABox was dropped"
    );
    assert_eq!(s.wedge_clauses_deferred, 1, "sat");
    let (_, s) =
        owl_dl_reasoner::is_subclass_of_with_stats(&o, "http://ex/#C", "http://ex/#B").unwrap();
    assert!(
        s.relies_on_trusted_consistency,
        "precondition: the ABox was dropped"
    );
    assert_eq!(s.wedge_clauses_deferred, 1, "subclass");
}

/// PINS A KNOWN WRONG ANSWER (#600): this KB is inconsistent (HermiT, Konclude, and
/// the main tableau with `RUSTDL_WEDGE_CONSISTENCY=0` agree), but the consistency
/// wedge never sees the deferred covering `E ⊑ F ⊔ ∃r.G` and its trusted `Sat` says
/// consistent. Part A's signal is the deferral. #600 must flip the first assertion
/// to `!consistent` (and may then drop the deferral to 0).
#[test]
fn issue_600_pin_the_deferred_covering_miss_and_its_signal() {
    let _l = lock();
    let o = onto(
        "ClassAssertion(ObjectComplementOf(:F) :b)\n\
         ClassAssertion(ObjectAllValuesFrom(:r ObjectComplementOf(:G)) :b)",
    );
    let (consistent, s) = owl_dl_reasoner::is_consistent_with_stats(&o).unwrap();
    assert!(
        consistent,
        "#600: today's wrong answer; flip when the clausifier is fixed"
    );
    assert!(s.relies_on_trusted_consistency);
    assert_eq!(s.wedge_clauses_deferred, 1, "the signal for the miss");
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let (consistent, _) = owl_dl_reasoner::is_consistent_with_stats(&o).unwrap();
    assert!(!consistent, "control: the main tableau finds the clash");
}
