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
