//! #313 `RUSTDL_QUERY_INCONSISTENCY_GUARD` (default ON): the query surfaces error as
//! `Inconsistent` on a KB that `abox_check` or the EL saturator proves inconsistent.
//!
//! They checked only the `ABox` saturation, so `c : E`, `E ⊑ ⊥` returned answers from
//! `same_individuals`, `disjoint_classes`, the instance and class-expression queries,
//! while `consistent` said inconsistent. Inconsistencies only the `ABox`-seeded wedge finds
//! are still missed here, by design (see `query_kb_inconsistent`).
//!
//! Run: `cargo test -p owl-dl-reasoner --test query_inconsistency_guard`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Every test locks, including the
// read-only ones, so none observes another test's setting.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:E)) Declaration(Class(:F))
    Declaration(Class(:A)) Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:r))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:c))
    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
";

/// `abox_check` finds it (an individual asserted to an unsatisfiable class).
const ASSERTED_BOTTOM: &str = "    ClassAssertion(:E :c) SubClassOf(:E owl:Nothing)";
/// The EL saturator finds it (an unsatisfiable nominal stand-in).
const NOMINAL_BOTTOM: &str = "    SubClassOf(ObjectOneOf(:a) owl:Nothing)";
/// FP guard: an unsatisfiable class with no instance; consistent.
const CONSISTENT: &str = "    ClassAssertion(:F :c) SubClassOf(:E owl:Nothing)";

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://t/x>\n{DECLS}{axioms}\n)\n"
    );
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// Which of the query surfaces report `Inconsistent`, with the flag set to `flag`.
fn inconsistent_surfaces(axioms: &str, flag: &str) -> Vec<&'static str> {
    use owl_dl_reasoner::ReasonError::Inconsistent;
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_QUERY_INCONSISTENCY_GUARD", flag);
    let o = parse(axioms);
    let ce = horned_owl::model::Build::<RcStr>::new()
        .class("http://t/X")
        .into();
    let mut out = Vec::new();
    let mut note = |name, inconsistent: bool| {
        if inconsistent {
            out.push(name);
        }
    };
    note(
        "same",
        matches!(
            owl_dl_reasoner::same_individuals(&o, None),
            Err(Inconsistent)
        ),
    );
    note(
        "different",
        matches!(
            owl_dl_reasoner::different_individuals(&o, None),
            Err(Inconsistent)
        ),
    );
    note(
        "disjoint",
        matches!(
            owl_dl_reasoner::disjoint_classes(&o, None),
            Err(Inconsistent)
        ),
    );
    note(
        "materialize",
        matches!(
            owl_dl_reasoner::materialize_object_property_assertions(&o),
            Err(Inconsistent)
        ),
    );
    note(
        "instances",
        matches!(
            owl_dl_reasoner::instances_of(&o, "http://t/X"),
            Err(Inconsistent)
        ),
    );
    note(
        "instance",
        matches!(
            owl_dl_reasoner::is_instance_of(&o, "http://t/X", "http://t/a"),
            Err(Inconsistent)
        ),
    );
    note(
        "sat-expr",
        matches!(
            owl_dl_reasoner::class_expression_satisfiable(&o, &ce),
            Err(Inconsistent)
        ),
    );
    out
}

const ALL: [&str; 7] = [
    "same",
    "different",
    "disjoint",
    "materialize",
    "instances",
    "instance",
    "sat-expr",
];

#[test]
fn every_surface_errors_on_an_asserted_bottom() {
    assert_eq!(inconsistent_surfaces(ASSERTED_BOTTOM, "1"), ALL);
}

#[test]
fn every_surface_errors_on_an_unsatisfiable_nominal() {
    assert_eq!(inconsistent_surfaces(NOMINAL_BOTTOM, "1"), ALL);
}

/// With the flag off only the `ABox` saturation guards, which sees neither shape.
#[test]
fn without_the_guard_the_surfaces_answer() {
    assert!(inconsistent_surfaces(NOMINAL_BOTTOM, "0").is_empty());
}

#[test]
fn a_consistent_kb_with_an_unsatisfiable_class_still_answers() {
    assert!(inconsistent_surfaces(CONSISTENT, "1").is_empty());
}

/// The reflexive empty-clause check, as in `consistent`: a reflexive asymmetric role.
#[test]
fn a_reflexive_asymmetric_role_errors() {
    let axioms = "    ReflexiveObjectProperty(:r) AsymmetricObjectProperty(:r)";
    assert_eq!(inconsistent_surfaces(axioms, "1"), ALL);
}

/// `class_expression_instances` guards the PROBED ontology (`Q ≡ CE` added).
fn instances_expr(axioms: &str, ce: &horned_owl::model::ClassExpression<RcStr>) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    matches!(
        owl_dl_reasoner::class_expression_instances(&parse(axioms), ce),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    )
}

#[test]
fn instances_of_a_class_expression_error_on_an_inconsistent_kb() {
    let x: horned_owl::model::ClassExpression<RcStr> = horned_owl::model::Build::<RcStr>::new()
        .class("http://t/X")
        .into();
    assert!(instances_expr(ASSERTED_BOTTOM, &x));
}

/// FP guard: the probe `Q ≡ ¬A` on a KB with `a : A` is a definitional extension, so the
/// KB stays consistent and the query answers.
#[test]
fn a_negated_probe_on_a_consistent_kb_answers() {
    let b = horned_owl::model::Build::<RcStr>::new();
    let not_a = horned_owl::model::ClassExpression::ObjectComplementOf(Box::new(
        b.class("http://t/A").into(),
    ));
    assert!(!instances_expr("    ClassAssertion(:A :a)", &not_a));
}

/// `justify … instance` on an inconsistent KB returns the inconsistency core, like the
/// property-assertion queries, instead of erroring.
#[test]
fn justify_an_instance_on_an_inconsistent_kb_returns_the_core() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let q = owl_dl_reasoner::justify::parse_query(&[
        "instance".to_owned(),
        "http://t/a".to_owned(),
        "http://t/F".to_owned(),
    ])
    .expect("query");
    let j = owl_dl_reasoner::justify::find_one_justification(&parse(ASSERTED_BOTTOM), &q)
        .expect("justify must not error");
    assert!(
        j.is_some(),
        "an inconsistent KB entails every instance assertion"
    );
}
