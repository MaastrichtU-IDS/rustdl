//! Sub-project 1 POC: first-class data-property lowering, end-to-end.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static DP_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct DpGuard {
    prior: Option<std::ffi::OsString>,
}
impl DpGuard {
    #[allow(unsafe_code)]
    fn on() -> Self {
        let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
        // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
        unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "1") };
        Self { prior }
    }
    #[allow(unsafe_code)]
    fn off() -> Self {
        let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
        // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
        unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "0") };
        Self { prior }
    }
}
impl Drop for DpGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see DpGuard::on.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var("RUSTDL_DATA_PROPERTIES", v),
                None => std::env::remove_var("RUSTDL_DATA_PROPERTIES"),
            }
        }
    }
}

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://t/o>\n{body}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    o
}

#[test]
fn poc_sub_data_property_forces_inconsistency() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(DataProperty(:dq))\n\
         Declaration(NamedIndividual(:a))\n\
         SubDataPropertyOf(:dp :dq)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)\n\
         NegativeDataPropertyAssertion(:dq :a \"5\"^^xsd:integer)",
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&o).unwrap(),
        "dp⊑dq + dp(a,5) + ¬dq(a,5) must be inconsistent"
    );
}

#[test]
fn poc_sub_data_property_consistent_when_values_differ() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(DataProperty(:dq))\n\
         Declaration(NamedIndividual(:a))\n\
         SubDataPropertyOf(:dp :dq)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)\n\
         NegativeDataPropertyAssertion(:dq :a \"6\"^^xsd:integer)",
    );
    assert!(
        owl_dl_reasoner::is_consistent(&o).unwrap(),
        "distinct value must stay consistent"
    );
}

#[test]
fn poc_functional_data_property_class_restriction_plus_assertion_inconsistent() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    // a ∈ C gives dp-value 5 (via the class DataHasValue, lowered even gate-OFF);
    // dp(a,6) is a data assertion (lowered only gate-ON); Functional(dp) (gate-ON)
    // merges the two dp value-nodes → DKey(5) ⊓ DKey(6) disjoint → inconsistent.
    // D4's functional-cardinality pre-check does NOT fire here (only one direct
    // assertion + a ≥1 class restriction), so the clash is approach B's alone.
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         FunctionalDataProperty(:dp)\n\
         ClassAssertion(:C :a)\n\
         SubClassOf(:C DataHasValue(:dp \"5\"^^xsd:integer))\n\
         DataPropertyAssertion(:dp :a \"6\"^^xsd:integer)",
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&o).unwrap(),
        "functional dp: class-restriction value 5 + asserted value 6 must clash (approach B)"
    );
}

#[test]
fn gate_off_classification_unchanged_on_data_fixture() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::off();
    // gate explicitly forced OFF ⇒ converter behaves as legacy.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ontologies/real/shoiq-knowledge.ofn");
    if !path.exists() {
        eprintln!("SKIP: corpus fixture absent");
        return;
    }
    let file = std::fs::File::open(&path).unwrap();
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::BufReader::new(file),
        ParserConfiguration::default(),
    )
    .unwrap();
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        !c.classes().is_empty(),
        "gate-OFF classify produces a hierarchy"
    );
}

#[test]
fn poc_unqualified_max_cardinality_merges_distinct_values() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataMaxCardinality(1 :dp))\n\
         SubClassOf(:C DataHasValue(:dp \"5\"^^xsd:integer))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"6\"^^xsd:integer)",
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&o).unwrap(),
        "≤1 unqualified dp with two distinct values must be inconsistent (gate ON)"
    );
}

#[test]
fn poc_unqualified_max_cardinality_consistent_gate_off() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::off();
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataMaxCardinality(1 :dp))\n\
         SubClassOf(:C DataHasValue(:dp \"5\"^^xsd:integer))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"6\"^^xsd:integer)",
    );
    assert!(
        owl_dl_reasoner::is_consistent(&o).unwrap(),
        "gate OFF: unqualified cardinality + dp assertion drop → consistent"
    );
}

#[test]
fn poc_data_all_values_from_abox_out_of_range_inconsistent() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    // a ∈ C ⇒ ∀dp.[≤3]; dp(a,5) ⇒ ∃dp.DKey(5); ∀ pushes the range onto the
    // value-node ⇒ 5 ∉ (-∞,3] → inconsistent.
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataAllValuesFrom(:dp DatatypeRestriction(xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)",
    );
    assert!(
        !owl_dl_reasoner::is_consistent(&o).unwrap(),
        "∀dp.[≤3] + dp(a,5) must be inconsistent (gate ON)"
    );
}

#[test]
fn poc_data_all_values_from_abox_in_range_consistent() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataAllValuesFrom(:dp DatatypeRestriction(xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"2\"^^xsd:integer)",
    );
    assert!(
        owl_dl_reasoner::is_consistent(&o).unwrap(),
        "∀dp.[≤3] + dp(a,2) must be consistent (in range)"
    );
}

#[test]
fn poc_data_leaf_nodes_terminate_under_object_cycle() {
    let _lock = DP_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    // Cyclic object role + data leaf must terminate and be consistent.
    let o = onto(
        "Declaration(ObjectProperty(:r)) Declaration(DataProperty(:dp))\n\
         Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C ObjectSomeValuesFrom(:r :C))\n\
         SubClassOf(:C DataSomeValuesFrom(:dp xsd:integer))\n\
         ClassAssertion(:C :a)",
    );
    assert!(
        owl_dl_reasoner::is_consistent(&o).unwrap(),
        "cyclic object role with data leaves must stay consistent and terminate"
    );
}
