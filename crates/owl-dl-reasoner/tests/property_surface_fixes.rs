//! Structural fixes to the property surfaces (#389, #390). Each fix adds entailed
//! relations, so each has positive cases (checked against HermiT where robot can
//! generate the axiom type) and false-positive guards, and each flag's `=0`
//! restores the old answer.
//!
//! Run: `cargo test -p owl-dl-reasoner --test property_surface_fixes`

#![allow(clippy::unwrap_used)]

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

const NS: &str = "http://x.org/f#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://x.org/f>\n\
         Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(DataProperty(:d)) Declaration(DataProperty(:e))\n\
         Declaration(DataProperty(:f))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         {body}\n)"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    o
}

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

/// Named `(sub, sup)` object-property subsumptions, transitively closed.
fn sub_obj(body: &str) -> Vec<(String, String)> {
    owl_dl_reasoner::materialize_subobjectproperty_axioms(&onto(body)).unwrap()
}

fn holds(rows: &[(String, String)], a: &str, b: &str) -> bool {
    rows.contains(&(iri(a), iri(b)))
}

// ── F1: SymmetricObjectProperty ─────────────────────────────────────────────

/// HermiT: `Symmetric(p)` + `InverseObjectProperties(p q)` ⊨ `p ≡ q`.
#[test]
fn symmetric_with_a_declared_inverse_is_equivalent() {
    let _l = lock();
    let body = "SymmetricObjectProperty(:p)\nInverseObjectProperties(:p :q)";
    let rows = sub_obj(body);
    assert!(holds(&rows, "p", "q") && holds(&rows, "q", "p"), "{rows:?}");
    let h = owl_dl_reasoner::classify_object_property_hierarchy(&onto(body)).unwrap();
    assert!(h.equivalent_groups().contains(&vec![iri("p"), iri("q")]));
    // A symmetric inverse is a symmetric property.
    let rows =
        sub_obj("SymmetricObjectProperty(ObjectInverseOf(:p))\nInverseObjectProperties(:p :q)");
    assert!(holds(&rows, "p", "q") && holds(&rows, "q", "p"), "{rows:?}");
}

/// FP guards: symmetry does not reverse a told subsumption, and does not relate a
/// symmetric property to an unrelated one.
#[test]
fn symmetric_adds_nothing_else() {
    let _l = lock();
    let rows = sub_obj("SymmetricObjectProperty(:p)\nSubObjectPropertyOf(:q :p)");
    assert!(holds(&rows, "q", "p"));
    assert!(!holds(&rows, "p", "q"), "{rows:?}");
    let rows = sub_obj("SymmetricObjectProperty(:p)\nSymmetricObjectProperty(:q)");
    assert!(rows.is_empty(), "{rows:?}");
    // Two symmetric properties that are each other's inverse are equivalent, and
    // that is all: `r` stays unrelated.
    let rows = sub_obj(
        "SymmetricObjectProperty(:p)\nInverseObjectProperties(:p :q)\nSubObjectPropertyOf(:r :s)",
    );
    assert!(!holds(&rows, "p", "r") && !holds(&rows, "r", "p"));
}

#[test]
fn symmetric_flag_off_restores_the_old_answer() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_PROPERTY_SYMMETRIC", "0");
    let rows = sub_obj("SymmetricObjectProperty(:p)\nInverseObjectProperties(:p :q)");
    assert!(rows.is_empty(), "{rows:?}");
}

// ── F2: disjointness passes down the hierarchy ──────────────────────────────

fn dj_obj(body: &str) -> Vec<(String, String)> {
    owl_dl_reasoner::disjoint_object_properties(&onto(body)).unwrap()
}

fn dj_data(body: &str) -> Vec<(String, String)> {
    owl_dl_reasoner::disjoint_data_properties(&onto(body)).unwrap()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (iri(a), iri(b))
}

/// HermiT (`KB ∪ {x(a,b), y(a,b)}` inconsistent): `p ⊑ q`, `Disjoint(q, r)` ⊨
/// `Disjoint(p, r)`; `Disjoint(q, r⁻)` with `r⁻ ≡ s` ⊨ `Disjoint(q, s)`;
/// `Disjoint(q⁻, r⁻)` ⊨ `Disjoint(q, r)`.
#[test]
fn disjointness_reaches_sub_properties_and_inverses() {
    let _l = lock();
    let d = dj_obj("SubObjectPropertyOf(:p :q)\nDisjointObjectProperties(:q :r)");
    assert!(
        d.contains(&pair("p", "r")) && d.contains(&pair("q", "r")),
        "{d:?}"
    );
    let d =
        dj_obj("DisjointObjectProperties(:q ObjectInverseOf(:r))\nInverseObjectProperties(:r :s)");
    assert!(d.contains(&pair("q", "s")), "{d:?}");
    let d = dj_obj("DisjointObjectProperties(ObjectInverseOf(:q) ObjectInverseOf(:r))");
    assert_eq!(d, vec![pair("q", "r")]);
    let d = dj_data("SubDataPropertyOf(:d :e)\nDisjointDataProperties(:e :f)");
    assert!(
        d.contains(&pair("d", "f")) && d.contains(&pair("e", "f")),
        "{d:?}"
    );
}

/// FP guards: disjointness does not pass UP, and `q ∩ r⁻ = ∅` says nothing about
/// `q ∩ r` (HermiT: `KB ∪ {q(a,b), r(a,b)}` is consistent).
#[test]
fn disjointness_does_not_pass_up_or_across_polarity() {
    let _l = lock();
    let d = dj_obj("SubObjectPropertyOf(:p :q)\nDisjointObjectProperties(:p :r)");
    assert_eq!(d, vec![pair("p", "r")]);
    let d = dj_obj("DisjointObjectProperties(:q ObjectInverseOf(:r))");
    assert!(d.is_empty(), "{d:?}");
    let d = dj_data("SubDataPropertyOf(:d :e)\nDisjointDataProperties(:d :f)");
    assert_eq!(d, vec![pair("d", "f")]);
}

#[test]
fn disjoint_down_flag_off_restores_the_old_answer() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_PROPERTY_DISJOINT_DOWN", "0");
    let d = dj_obj("SubObjectPropertyOf(:p :q)\nDisjointObjectProperties(:q :r)");
    assert_eq!(d, vec![pair("q", "r")]);
    let d = dj_obj("DisjointObjectProperties(ObjectInverseOf(:q) ObjectInverseOf(:r))");
    assert!(d.is_empty(), "{d:?}");
}

// ── F3: AsymmetricObjectProperty ────────────────────────────────────────────

/// HermiT (`KB ∪ {x(a,b), y(a,b)}` inconsistent): `Asymmetric(p)` with `p⁻ ≡ q`
/// ⊨ `Disjoint(p, q)`, and with `s ⊑ p` also `Disjoint(s, q)`.
#[test]
fn asymmetric_is_disjoint_from_its_inverse() {
    let _l = lock();
    let body = "AsymmetricObjectProperty(:p)\nInverseObjectProperties(:p :q)";
    assert_eq!(dj_obj(body), vec![pair("p", "q")]);
    let d = dj_obj(&format!("{body}\nSubObjectPropertyOf(:s :p)"));
    assert!(
        d.contains(&pair("p", "q")) && d.contains(&pair("q", "s")),
        "{d:?}"
    );
    let d = dj_obj("AsymmetricObjectProperty(ObjectInverseOf(:p))\nInverseObjectProperties(:p :q)");
    assert_eq!(d, vec![pair("p", "q")]);
}

/// FP guards: an asymmetric `p` is not disjoint from a mere sub-property, nor from
/// an unrelated property (HermiT: both `KB ∪ {x(a,b), y(a,b)}` consistent).
#[test]
fn asymmetric_adds_nothing_else() {
    let _l = lock();
    let d = dj_obj(
        "AsymmetricObjectProperty(:p)\nSubObjectPropertyOf(:s :p)\nSubObjectPropertyOf(:r :s)",
    );
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn asymmetric_flag_off_restores_the_old_answer() {
    let _l = lock();
    let body = "AsymmetricObjectProperty(:p)\nInverseObjectProperties(:p :q)";
    for flag in [
        "RUSTDL_PROPERTY_ASYMMETRIC_DISJOINT",
        "RUSTDL_PROPERTY_DISJOINT_DOWN",
    ] {
        let _off = SetEnv::new(flag, "0");
        assert!(dj_obj(body).is_empty(), "{flag}=0");
    }
}

// ── owl:topObjectProperty / owl:bottomObjectProperty ────────────────────────

fn sub_data(body: &str) -> Vec<(String, String)> {
    owl_dl_reasoner::materialize_subdataproperty_axioms(&onto(body)).unwrap()
}

/// HermiT: `p ⊑ bottom` ⊨ `p ⊑ q` for every `q`; `top ⊑ p` ⊨ `q ⊑ p` for every
/// `q`. Neither top nor bottom appears in the rows.
#[test]
fn top_and_bottom_reach_every_property() {
    let _l = lock();
    let rows = sub_obj("SubObjectPropertyOf(:p owl:bottomObjectProperty)");
    for q in ["q", "r", "s"] {
        assert!(holds(&rows, "p", q), "p ⊑ {q}: {rows:?}");
    }
    assert!(!holds(&rows, "q", "p"));
    let rows = sub_obj("SubObjectPropertyOf(owl:topObjectProperty :p)");
    for q in ["q", "r", "s"] {
        assert!(holds(&rows, q, "p"), "{q} ⊑ p: {rows:?}");
    }
    assert!(!holds(&rows, "p", "q"));
    assert!(
        rows.iter()
            .all(|(a, b)| !a.contains("www.w3.org") && !b.contains("www.w3.org"))
    );
    let rows = sub_data("SubDataPropertyOf(:d owl:bottomDataProperty)");
    assert!(holds(&rows, "d", "e") && holds(&rows, "d", "f"), "{rows:?}");
    // `p ⊑ bottom` is empty, so it is disjoint from every property it sits below
    // (here `q` and `r`); bottom itself never shows.
    let d =
        dj_obj("SubObjectPropertyOf(:p owl:bottomObjectProperty)\nDisjointObjectProperties(:q :r)");
    assert_eq!(d, vec![pair("p", "q"), pair("p", "r"), pair("q", "r")]);
}

/// Mentioning neither leaves the closure as it was: no seeding, no new rows.
#[test]
fn an_ordinary_ontology_is_not_seeded() {
    let _l = lock();
    assert_eq!(sub_obj("SubObjectPropertyOf(:p :q)"), vec![pair("p", "q")]);
}

#[test]
fn top_bottom_flag_off_restores_the_old_answer() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_PROPERTY_TOP_BOTTOM", "0");
    assert!(sub_obj("SubObjectPropertyOf(:p owl:bottomObjectProperty)").is_empty());
}

// ── one-leg chains ──────────────────────────────────────────────────────────

/// `ObjectPropertyChain([p]) ⊑ s` (built through the API: OFN cannot write it) is
/// `p ⊑ s`.
fn unit_chain_onto() -> SetOntology<RcStr> {
    use horned_owl::model::MutableOntology as _;
    use horned_owl::model::{
        Build, Component, ObjectPropertyExpression as OPE, SubObjectPropertyExpression as SOPE,
        SubObjectPropertyOf,
    };
    let b: Build<RcStr> = Build::new_rc();
    let mut o = onto("");
    o.insert(Component::SubObjectPropertyOf(SubObjectPropertyOf {
        sub: SOPE::ObjectPropertyChain(vec![OPE::ObjectProperty(b.object_property(iri("p")))]),
        sup: OPE::ObjectProperty(b.object_property(iri("s"))),
    }));
    o
}

#[test]
fn a_one_leg_chain_is_a_plain_subsumption() {
    let _l = lock();
    let rows = owl_dl_reasoner::materialize_subobjectproperty_axioms(&unit_chain_onto()).unwrap();
    assert_eq!(rows, vec![pair("p", "s")]);
    let _off = SetEnv::new("RUSTDL_PROPERTY_UNIT_CHAIN", "0");
    let rows = owl_dl_reasoner::materialize_subobjectproperty_axioms(&unit_chain_onto()).unwrap();
    assert!(rows.is_empty(), "{rows:?}");
}

/// FP guard: a two-leg chain stays out of the simple closure.
#[test]
fn a_two_leg_chain_is_not_a_subsumption() {
    let _l = lock();
    assert!(sub_obj("SubObjectPropertyOf(ObjectPropertyChain(:p :q) :s)").is_empty());
}

// ── anonymous individuals in the data-value folding (#390) ──────────────────

fn values(body: &str) -> Vec<(String, String, String, String, String)> {
    owl_dl_reasoner::materialize_data_property_assertions(&onto(body)).unwrap()
}

/// `d(_:x, 1)` + `SameIndividual(_:x, a)` ⊨ `d(a, 1)` (`∃x. d(x,1) ∧ x = a`). OWL 2 DL
/// forbids an anonymous individual in `SameIndividual`, so HermiT rejects the input
/// and cannot adjudicate; rustdl reads it anyway. No row is reported
/// for the anonymous individual itself.
#[test]
fn a_value_on_an_anonymous_individual_reaches_its_named_equal() {
    let _l = lock();
    let body = "DataPropertyAssertion(:d _:x \"1\"^^xsd:integer)\nSameIndividual(_:x :a)";
    let rows = values(body);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1.as_str(), rows[0].2.as_str()),
        (iri("a").as_str(), iri("d").as_str(), "1")
    );
    // FP guard: without the equality nothing reaches `a`.
    assert!(values("DataPropertyAssertion(:d _:x \"1\"^^xsd:integer)").is_empty());
    let _off = SetEnv::new("RUSTDL_DATA_VALUES_ANONYMOUS", "0");
    assert!(values(body).is_empty());
}
