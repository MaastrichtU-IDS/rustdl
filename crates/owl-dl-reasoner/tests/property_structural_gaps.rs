//! `property_structural_gaps` (#389, #390): each kind fires on an ontology with
//! the construct it names (a route to a relation the structural surfaces miss)
//! and stays silent on a near miss that lacks one ingredient of that route.
//!
//! Run: `cargo test -p owl-dl-reasoner --test property_structural_gaps`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::PropertyStructuralGaps;
use std::io::Cursor;

// Kinds a fix covers are listed only with that fix's flag off, so tests flip
// env vars. Every test locks, including read-only ones, so none observes another
// test's setting.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for the guard's lifetime and restores it on drop.
struct SetEnv(&'static str, Option<std::ffi::OsString>);

impl SetEnv {
    #[allow(unsafe_code)]
    fn new(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: serialized by ENV_MUTEX, restored on drop.
        unsafe { std::env::set_var(key, value) };
        Self(key, prior)
    }
}

impl Drop for SetEnv {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `new`.
        unsafe {
            match &self.1 {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

fn gaps(body: &str) -> PropertyStructuralGaps {
    let src = format!(
        "Prefix(:=<http://x.org/g#>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://x.org/g>\n\
         Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:t))\n\
         Declaration(DataProperty(:d)) Declaration(DataProperty(:e))\n\
         Declaration(DataProperty(:f))\n\
         Declaration(Class(:A)) Declaration(Class(:B))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:o))\n\
         {body}\n)"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    owl_dl_reasoner::property_structural_gaps(&o).unwrap()
}

fn has(list: &[String], kind: &str) -> bool {
    list.iter().any(|k| k == kind)
}

// ── object hierarchy ────────────────────────────────────────────────────────

#[test]
fn symmetric() {
    let _l = lock();
    // Covered by F1 (`RUSTDL_PROPERTY_SYMMETRIC`, default ON): listed only with it off.
    assert!(!has(
        gaps("SymmetricObjectProperty(:p)").object_hierarchy(),
        "object-symmetric"
    ));
    let _off = SetEnv::new("RUSTDL_PROPERTY_SYMMETRIC", "0");
    assert!(has(
        gaps("SymmetricObjectProperty(:p)").object_hierarchy(),
        "object-symmetric"
    ));
    assert!(!has(
        gaps("InverseObjectProperties(:p :q)").object_hierarchy(),
        "object-symmetric"
    ));
}

#[test]
fn top_bottom() {
    let _l = lock();
    let g = gaps("SubObjectPropertyOf(:p owl:bottomObjectProperty)");
    assert!(has(g.object_hierarchy(), "object-top-bottom"));
    assert!(!has(
        gaps("SubObjectPropertyOf(:p :q)").object_hierarchy(),
        "object-top-bottom"
    ));
}

/// A one-leg chain cannot be written in OFN (the grammar wants two legs), but other
/// syntaxes and the API can build one.
#[test]
fn unit_chain() {
    let _l = lock();
    use horned_owl::model::MutableOntology as _;
    use horned_owl::model::{
        Build, Component, ObjectPropertyExpression as OPE, SubObjectPropertyExpression as SOPE,
        SubObjectPropertyOf,
    };
    let b: Build<RcStr> = Build::new_rc();
    let mut o: SetOntology<RcStr> = SetOntology::new();
    o.insert(Component::SubObjectPropertyOf(SubObjectPropertyOf {
        sub: SOPE::ObjectPropertyChain(vec![OPE::ObjectProperty(
            b.object_property("http://x.org/g#p"),
        )]),
        sup: OPE::ObjectProperty(b.object_property("http://x.org/g#s")),
    }));
    let g = owl_dl_reasoner::property_structural_gaps(&o).unwrap();
    assert!(has(g.object_hierarchy(), "object-unit-chain"));
    let g = gaps("SubObjectPropertyOf(ObjectPropertyChain(:p :q) :s)");
    assert!(!has(g.object_hierarchy(), "object-unit-chain"));
}

#[test]
fn chain_with_a_reflexive_leg() {
    let _l = lock();
    let chain = "SubObjectPropertyOf(ObjectPropertyChain(:p :q) :s)";
    let g = gaps(&format!("{chain}\nReflexiveObjectProperty(:q)"));
    assert!(has(g.object_hierarchy(), "object-chain-reflexive"));
    let g = gaps(&format!("{chain}\nSubClassOf(:A ObjectHasSelf(:q))"));
    assert!(has(g.object_hierarchy(), "object-chain-reflexive"));
    let g = gaps("SubObjectPropertyOf(ObjectPropertyChain(:p owl:topObjectProperty) :s)");
    assert!(has(g.object_hierarchy(), "object-chain-reflexive"));
    // Near misses: the chain alone, the reflexive leg alone.
    assert!(gaps(chain).object_hierarchy().is_empty());
    assert!(
        gaps("ReflexiveObjectProperty(:q)")
            .object_hierarchy()
            .is_empty()
    );
}

#[test]
fn nominal_bounded_range() {
    let _l = lock();
    let all = "SubClassOf(owl:Thing ObjectAllValuesFrom(:r ObjectOneOf(:o)))";
    let value = "SubClassOf(owl:Thing ObjectHasValue(:s :o))";
    let g = gaps(&format!("{all}\n{value}"));
    assert!(has(g.object_hierarchy(), "object-nominal-range"));
    let g = gaps(&format!("ObjectPropertyRange(:r ObjectOneOf(:o))\n{value}"));
    assert!(has(g.object_hierarchy(), "object-nominal-range"));
    assert!(has(
        gaps("SubClassOf(owl:Thing ObjectOneOf(:o))").object_hierarchy(),
        "object-nominal-range"
    ));
    assert!(!has(gaps(all).object_hierarchy(), "object-nominal-range"));
    assert!(!has(gaps(value).object_hierarchy(), "object-nominal-range"));
}

#[test]
fn counting_under_a_bound() {
    let _l = lock();
    let g = gaps("SubObjectPropertyOf(:r :t)\nFunctionalObjectProperty(:t)");
    assert!(has(g.object_hierarchy(), "object-counting"));
    let g = gaps("SubObjectPropertyOf(:r :t)\nSubClassOf(:A ObjectMaxCardinality(2 :t))");
    assert!(has(g.object_hierarchy(), "object-counting"));
    // InverseFunctional(t) bounds t⁻, whose strict sub-property is r⁻.
    let g = gaps("SubObjectPropertyOf(:r :t)\nInverseFunctionalObjectProperty(:t)");
    assert!(has(g.object_hierarchy(), "object-counting"));
    assert!(!has(
        gaps("FunctionalObjectProperty(:t)").object_hierarchy(),
        "object-counting"
    ));
    assert!(!has(
        gaps("SubObjectPropertyOf(:r :t)").object_hierarchy(),
        "object-counting"
    ));
    // An equivalent property is not a strict sub-property.
    let g = gaps("EquivalentObjectProperties(:r :t)\nFunctionalObjectProperty(:t)");
    assert!(!has(g.object_hierarchy(), "object-counting"));
}

#[test]
fn empty_property() {
    let _l = lock();
    let k = "object-empty-property";
    let g = gaps("ObjectPropertyDomain(:p :A)\nSubClassOf(:A owl:Nothing)");
    assert!(has(g.object_hierarchy(), k) && has(g.object_disjoint(), k));
    assert!(has(
        gaps("ObjectPropertyRange(:p owl:Nothing)").object_hierarchy(),
        k
    ));
    let g = gaps(
        "SubObjectPropertyOf(:p :q)\nSubObjectPropertyOf(:p :r)\nDisjointObjectProperties(:q :r)",
    );
    assert!(has(g.object_hierarchy(), k));
    let g = gaps("SymmetricObjectProperty(:p)\nAsymmetricObjectProperty(:p)");
    assert!(has(g.object_hierarchy(), k));
    // Near misses: a satisfiable domain; one disjoint parent only.
    assert!(!has(
        gaps("ObjectPropertyDomain(:p :A)").object_hierarchy(),
        k
    ));
    let g = gaps("SubObjectPropertyOf(:p :q)\nDisjointObjectProperties(:q :r)");
    assert!(!has(g.object_hierarchy(), k));
}

// ── object disjointness ─────────────────────────────────────────────────────

#[test]
fn disjointness_inherited_by_a_sub_property() {
    let _l = lock();
    let k = "object-disjoint-inheritance";
    let body = "SubObjectPropertyOf(:p :q)\nDisjointObjectProperties(:q :r)";
    // Covered by F2 (`RUSTDL_PROPERTY_DISJOINT_DOWN`, default ON).
    assert!(!has(gaps(body).object_disjoint(), k));
    let _off = SetEnv::new("RUSTDL_PROPERTY_DISJOINT_DOWN", "0");
    let g = gaps(body);
    assert!(has(g.object_disjoint(), k));
    assert!(!has(
        gaps("DisjointObjectProperties(:q :r)").object_disjoint(),
        k
    ));
}

#[test]
fn disjointness_with_an_inverse_member() {
    let _l = lock();
    let k = "object-disjoint-inverse-member";
    let body = "DisjointObjectProperties(:q ObjectInverseOf(:r))";
    assert!(!has(gaps(body).object_disjoint(), k));
    let _off = SetEnv::new("RUSTDL_PROPERTY_DISJOINT_DOWN", "0");
    let g = gaps(body);
    assert!(has(g.object_disjoint(), k));
    assert!(!has(
        gaps("DisjointObjectProperties(:q :r)").object_disjoint(),
        k
    ));
}

#[test]
fn asymmetric() {
    let _l = lock();
    assert!(has(
        gaps("AsymmetricObjectProperty(:p)").object_disjoint(),
        "object-asymmetric"
    ));
    assert!(!has(
        gaps("IrreflexiveObjectProperty(:p)").object_disjoint(),
        "object-asymmetric"
    ));
}

#[test]
fn class_constraints_with_a_disjointness_source() {
    let _l = lock();
    let k = "object-class-constraint";
    let doms = "ObjectPropertyDomain(:p :A)\nObjectPropertyDomain(:q :B)";
    assert!(has(
        gaps(&format!("{doms}\nDisjointClasses(:A :B)")).object_disjoint(),
        k
    ));
    assert!(!has(gaps(doms).object_disjoint(), k));
    let g = gaps("ObjectPropertyDomain(:p :A)\nDisjointClasses(:A :B)");
    assert!(
        !has(g.object_disjoint(), k),
        "one constrained property cannot be disjoint from another"
    );
}

#[test]
fn hierarchy_routes_reach_disjointness_only_with_a_told_disjointness() {
    let _l = lock();
    let base = "SubObjectPropertyOf(:r :t)\nFunctionalObjectProperty(:t)";
    let g = gaps(&format!("{base}\nDisjointObjectProperties(:p :q)"));
    assert!(has(g.object_disjoint(), "object-counting"));
    let g = gaps(base);
    assert!(!has(g.object_disjoint(), "object-counting"));
}

// ── data hierarchy and disjointness ─────────────────────────────────────────

#[test]
fn data_hierarchy_kinds() {
    let _l = lock();
    let g = gaps("SubDataPropertyOf(:d owl:bottomDataProperty)");
    assert!(has(g.data_hierarchy(), "data-top-bottom"));
    let g = gaps("SubDataPropertyOf(:d :e)\nFunctionalDataProperty(:e)");
    assert!(has(g.data_hierarchy(), "data-counting"));
    assert!(!has(
        gaps("FunctionalDataProperty(:e)").data_hierarchy(),
        "data-counting"
    ));
    let range = "DataPropertyRange(:d DataOneOf(\"1\"^^xsd:integer))";
    let g = gaps(&format!(
        "{range}\nSubClassOf(owl:Thing DataHasValue(:e \"1\"^^xsd:integer))"
    ));
    assert!(has(g.data_hierarchy(), "data-value-range"));
    assert!(!has(gaps(range).data_hierarchy(), "data-value-range"));
    let g = gaps("DataPropertyDomain(:d :A)\nSubClassOf(:A owl:Nothing)");
    assert!(has(g.data_hierarchy(), "data-empty-property"));
    assert!(!has(
        gaps("DataPropertyDomain(:d :A)").data_hierarchy(),
        "data-empty-property"
    ));
}

#[test]
fn data_disjointness_kinds() {
    let _l = lock();
    let body = "SubDataPropertyOf(:d :e)\nDisjointDataProperties(:e :f)";
    assert!(!has(
        gaps(body).data_disjoint(),
        "data-disjoint-inheritance"
    ));
    {
        let _off = SetEnv::new("RUSTDL_PROPERTY_DISJOINT_DOWN", "0");
        assert!(has(gaps(body).data_disjoint(), "data-disjoint-inheritance"));
    }
    assert!(!has(
        gaps("DisjointDataProperties(:e :f)").data_disjoint(),
        "data-disjoint-inheritance"
    ));
    let g = gaps("DataPropertyRange(:d xsd:integer)\nDataPropertyRange(:e xsd:string)");
    assert!(has(g.data_disjoint(), "data-class-constraint"));
    assert!(!has(
        gaps("DataPropertyRange(:d xsd:integer)").data_disjoint(),
        "data-class-constraint"
    ));
}

// ── data property values (#390) ─────────────────────────────────────────────

#[test]
fn a_data_value_from_a_class_axiom_needs_an_individual() {
    let _l = lock();
    let k = "data-value-class-axiom";
    let tbox = "SubClassOf(:A DataHasValue(:d \"1\"^^xsd:integer))";
    assert!(has(
        gaps(&format!("{tbox}\nClassAssertion(:A :a)")).data_values(),
        k
    ));
    assert!(!has(gaps(tbox).data_values(), k));
}

#[test]
fn derived_individual_equality() {
    let _l = lock();
    let k = "data-derived-equality";
    let dpa = "DataPropertyAssertion(:d :b \"1\"^^xsd:integer)";
    let two = "ObjectPropertyAssertion(:r :a :b)\nObjectPropertyAssertion(:r :a :c)";
    let g = gaps(&format!("FunctionalObjectProperty(:r)\n{two}\n{dpa}"));
    assert!(has(g.data_values(), k));
    // One successor: nothing to merge.
    let g = gaps(&format!(
        "FunctionalObjectProperty(:r)\nObjectPropertyAssertion(:r :a :b)\n{dpa}"
    ));
    assert!(!has(g.data_values(), k));
    // Two successors on an unbounded role.
    assert!(!has(gaps(&format!("{two}\n{dpa}")).data_values(), k));
    // A shared key value.
    let g = gaps(&format!(
        "HasKey(:A () (:d))\n{dpa}\nDataPropertyAssertion(:d :c \"1\"^^xsd:integer)"
    ));
    assert!(has(g.data_values(), k));
    // A singleton nominal type equates the individual with `o`.
    let g = gaps(&format!("ClassAssertion(ObjectOneOf(:o) :b)\n{dpa}"));
    assert!(has(g.data_values(), k));
    // No data assertion: nothing to fold.
    assert!(!has(
        gaps(&format!("FunctionalObjectProperty(:r)\n{two}")).data_values(),
        k
    ));
}

#[test]
fn anonymous_individual() {
    let _l = lock();
    let k = "data-anonymous-individual";
    let g = gaps("DataPropertyAssertion(:d _:x \"1\"^^xsd:integer)\nSameIndividual(_:x :a)");
    assert!(has(g.data_values(), k));
    assert!(!has(
        gaps("DataPropertyAssertion(:d :a \"1\"^^xsd:integer)").data_values(),
        k
    ));
}

#[test]
fn data_hierarchy_routes_reach_values_only_with_a_data_assertion() {
    let _l = lock();
    let base = "SubDataPropertyOf(:d :e)\nFunctionalDataProperty(:e)";
    let g = gaps(&format!(
        "{base}\nDataPropertyAssertion(:d :a \"1\"^^xsd:integer)"
    ));
    assert!(has(g.data_values(), "data-counting"));
    assert!(!has(gaps(base).data_values(), "data-counting"));
}

#[test]
fn a_plain_ontology_has_no_gaps() {
    let _l = lock();
    let g = gaps(
        "SubObjectPropertyOf(:p :q)\nInverseObjectProperties(:q :r)\n\
         SubDataPropertyOf(:d :e)\nDisjointObjectProperties(:s :t)\n\
         ObjectPropertyAssertion(:p :a :b)\nDataPropertyAssertion(:d :a \"1\"^^xsd:integer)",
    );
    assert_eq!(g, PropertyStructuralGaps::default());
}

/// The saturation behind the `empty-property` check also derives unsatisfiable
/// SYNTHETIC classes (here the marker for `∃p.B`), which have no IRI. Reading them
/// back as named classes panicked on 149 ORE ontologies.
#[test]
fn an_unsatisfiable_synthetic_class_is_not_read_as_a_named_one() {
    let _l = lock();
    let g = gaps(
        "SubClassOf(:A ObjectSomeValuesFrom(:p :B))\n\
         SubClassOf(ObjectSomeValuesFrom(:p :B) owl:Nothing)\nObjectPropertyDomain(:r :A)",
    );
    assert!(has(g.object_hierarchy(), "object-empty-property"));
}
