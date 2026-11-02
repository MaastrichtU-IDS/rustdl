//! #369 follow-up: the lazy consistency check on the surfaces that hold no
//! prepared ontology of their own: `sat-expr` / `subclass-expr`, the
//! `materialize_*` property functions, `property-values` and the property
//! disjointness queries. Their answer is positive when non-empty (or
//! "unsatisfiable" / "subsumed"); a witnessed inconsistency is
//! `Err(Inconsistent)`, an undetermined one is kept and reported as
//! `consistency_undetermined`.
//!
//! Most of them take the light route (the consistency wedge built straight from
//! the converted ontology); a tableau-answered expression and `property-values`
//! reuse the prepared ontology they built. With no individual the wedge has no
//! seed and is not run, and nothing is flagged: that is also a soundness gate,
//! pinned here on a TBox-only KB whose first class is unsatisfiable.
//!
//! Fixtures:
//! - `P2`: #419's `T2` (`{b} ⊑ ≥2 r.⊤`, `r` functional: inconsistent) with
//!   property assertions, sub-properties and property disjointness;
//! - `ctl()`: the same with `≥1`, consistent;
//! - `TBOX_FIRST_UNSAT`: no individual; its first class `A` is unsatisfiable by
//!   case split; consistent.
//!
//! Every test locks the env mutex.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    ReasonError, class_expression_entailed_subclass, class_expression_satisfiable,
    disjoint_data_properties_reporting, disjoint_object_properties_reporting,
    inferred_data_property_values, inferred_object_property_values,
    materialize_data_property_assertions_reporting,
    materialize_object_property_assertions_reporting, materialize_subdataproperty_axioms_reporting,
    materialize_subobjectproperty_axioms_reporting,
};
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
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe { std::env::set_var(key, value) };
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

const P2: &str = r#"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/p2>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(ObjectProperty(:r))
  Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:s))
  Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(DataProperty(:f))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
  SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(MIN :r owl:Thing))
  FunctionalObjectProperty(:r)
  ClassAssertion(:Y :a) ClassAssertion(:Y :c)
  ObjectPropertyAssertion(:p :a :c) DataPropertyAssertion(:d :a "1")
  SubObjectPropertyOf(:p :s) SubDataPropertyOf(:d :f)
  DisjointObjectProperties(:p :q) DisjointDataProperties(:d :e)
)"#;

const TBOX_FIRST_UNSAT: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/tboxfirstunsat>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:F)) Declaration(Class(:G))
  Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:s))
  SubClassOf(:A ObjectUnionOf(:F :G)) DisjointClasses(:F :A) DisjointClasses(:G :A)
  SubObjectPropertyOf(:p :s)
)";

const Y: &str = "http://ex/#Y";
const B: &str = "http://ex/#B";
const THING: &str = "http://www.w3.org/2002/07/owl#Thing";

fn p2() -> SetOntology<RcStr> {
    parse(&P2.replace("MIN", "2"))
}

fn ctl() -> SetOntology<RcStr> {
    parse(&P2.replace("MIN", "1"))
}

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn class(iri: &str) -> ClassExpression<RcStr> {
    ClassExpression::Class(Build::<RcStr>::new().class(iri))
}

/// `(Y ⊔ Z) ⊓ ¬Y ⊓ ¬Z`, Z fresh: unsatisfiable only by case split, so the
/// tableau answers it and the check reuses its prepared ontology.
fn case_split() -> ClassExpression<RcStr> {
    let z = class("urn:rustdl-test:z");
    ClassExpression::ObjectIntersectionOf(vec![
        ClassExpression::ObjectUnionOf(vec![class(Y), z.clone()]),
        ClassExpression::ObjectComplementOf(Box::new(class(Y))),
        ClassExpression::ObjectComplementOf(Box::new(z)),
    ])
}

fn is_inconsistent<T: std::fmt::Debug>(r: &Result<T, ReasonError>) -> bool {
    matches!(r, Err(ReasonError::Inconsistent))
}

/// Each surface's `(positive, consistency_undetermined)` on `o`.
#[allow(clippy::type_complexity)]
fn all(o: &SetOntology<RcStr>) -> Vec<(&'static str, Result<(bool, bool), ReasonError>)> {
    let pair =
        |r: Result<(Vec<(String, String)>, bool), ReasonError>| r.map(|(v, u)| (!v.is_empty(), u));
    vec![
        (
            "sat-expr",
            class_expression_satisfiable(o, &class(Y))
                .map(|v| (!v.holds(), v.consistency_undetermined())),
        ),
        (
            "subclass-expr",
            class_expression_entailed_subclass(o, &class(Y), &class(THING))
                .map(|v| (v.holds(), v.consistency_undetermined())),
        ),
        (
            "sat-expr case split",
            class_expression_satisfiable(o, &case_split())
                .map(|v| (!v.holds(), v.consistency_undetermined())),
        ),
        (
            "materialize object",
            materialize_object_property_assertions_reporting(o).map(|(v, u)| (!v.is_empty(), u)),
        ),
        (
            "materialize data",
            materialize_data_property_assertions_reporting(o).map(|(v, u)| (!v.is_empty(), u)),
        ),
        (
            "materialize subobjectproperty",
            pair(materialize_subobjectproperty_axioms_reporting(o)),
        ),
        (
            "materialize subdataproperty",
            pair(materialize_subdataproperty_axioms_reporting(o)),
        ),
        (
            "disjoint object properties",
            pair(disjoint_object_properties_reporting(o)),
        ),
        (
            "disjoint data properties",
            pair(disjoint_data_properties_reporting(o)),
        ),
        (
            "object property values",
            inferred_object_property_values(o, None)
                .map(|v| (!v.triples().is_empty(), v.consistency_undetermined())),
        ),
        (
            "data property values",
            inferred_data_property_values(o)
                .map(|v| (!v.quints().is_empty(), v.consistency_undetermined())),
        ),
    ]
}

#[test]
fn every_surface_errors_on_a_wedge_only_inconsistency() {
    let _g = lock();
    for (name, r) in all(&p2()) {
        assert!(is_inconsistent(&r), "{name}: {r:?}");
    }
}

#[test]
fn a_consistent_kb_keeps_its_answers_and_no_flag() {
    let _g = lock();
    for (name, r) in all(&ctl()) {
        let (positive, undetermined) = r.unwrap();
        // `Y` is satisfiable on the consistent KB: the one negative answer here.
        assert_eq!(positive, name != "sat-expr", "{name}");
        assert!(!undetermined, "{name}");
    }
}

#[test]
fn the_flag_reverts_to_the_vacuous_answers() {
    let _g = lock();
    let _off = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY", "0");
    for (name, r) in all(&p2()) {
        let (positive, undetermined) = r.unwrap();
        assert!(positive && !undetermined, "{name}");
    }
}

/// With the wedge turned off the verdict is undetermined: the answer is kept and
/// flagged on every surface; the opt-in tail decides it.
#[test]
fn an_undetermined_wedge_is_reported_and_the_tail_decides_it() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    for (name, r) in all(&p2()) {
        let (positive, undetermined) = r.unwrap();
        assert!(positive && undetermined, "{name}");
    }
    let _tail = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1");
    for (name, r) in all(&p2()) {
        assert!(is_inconsistent(&r), "{name} with the tail: {r:?}");
    }
}

/// The soundness gate: with no individual the wedge is not run. Run unseeded it
/// would test the first class, `A`, which is unsatisfiable, and turn a positive
/// answer on this consistent KB into a false `Inconsistent`.
#[test]
fn no_individual_runs_no_wedge_and_flags_nothing() {
    let _g = lock();
    let o = parse(TBOX_FIRST_UNSAT);
    let v = class_expression_entailed_subclass(&o, &class(B), &class(THING)).unwrap();
    assert!(v.holds() && !v.consistency_undetermined());
    let (pairs, undetermined) = materialize_subobjectproperty_axioms_reporting(&o).unwrap();
    assert!(!pairs.is_empty() && !undetermined);
    let v = class_expression_satisfiable(&o, &class("http://ex/#A")).unwrap();
    assert!(
        !v.holds() && !v.consistency_undetermined(),
        "A is unsatisfiable"
    );
}

/// The check runs only after a positive answer: with the wedge undetermined, a
/// negative answer would carry the flag if the check had run.
#[test]
fn a_negative_answer_never_runs_the_check() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let o = ctl();
    let v = class_expression_satisfiable(&o, &class(Y)).unwrap();
    assert!(v.holds() && !v.consistency_undetermined());
    let v = class_expression_entailed_subclass(&o, &class(THING), &class(Y)).unwrap();
    assert!(!v.holds() && !v.consistency_undetermined());
    let none = parse(
        r"Prefix(:=<http://ex/#>) Ontology(<http://ex/none>
          Declaration(Class(:Y)) Declaration(NamedIndividual(:a)) ClassAssertion(:Y :a))",
    );
    let (rows, undetermined) = materialize_object_property_assertions_reporting(&none).unwrap();
    assert!(rows.is_empty() && !undetermined);
}
