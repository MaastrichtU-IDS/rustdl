//! #369: the instance, individuals and `instances-expr` queries error on an
//! inconsistency the #313 pre-checks miss. After a probe answers POSITIVELY (an
//! instance, a merge, a difference), a lazy check settles the KB's consistency
//! with the #401 wedge route. A witnessed inconsistency is
//! `Err(Inconsistent)`; when the wedge leaves it undetermined, the answer is kept
//! and reported as `consistency_undetermined`, unless the opt-in main-tableau
//! tail (`RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL=1`) decides it.
//!
//! Fixtures (HermiT-style reasoning in each comment; Konclude agrees on the
//! corpus files these stand for):
//! - `T2`: the TBox-only nominal `{b} ⊑ ≥2 r.⊤` with `r` functional: `b`
//!   cannot exist, so the KB is inconsistent;
//! - `T3`: `c : ∃r.E` with `E ⊑ F ⊔ G` and `F`, `G` disjoint from `E`, so `E` is
//!   unsatisfiable only by case split;
//! - `CTL`: `T2` with `≥1`, consistent.
//!
//! `RUSTDL_WEDGE_CONSISTENCY=0` makes the wedge route undetermined, which is how
//! these tests reach the signal and the tail. The follow-up's surfaces
//! (`sat-expr`, `subclass-expr`, `materialize_*`, `property-values`, the property
//! disjointness queries) are pinned in `lazy_query_consistency_followup.rs`.
//! Every test locks the env mutex.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    ReasonError, class_expression_entailed_subclass, class_expression_instances,
    class_expression_satisfiable, different_individuals, instances_of_reporting,
    is_instance_of_reporting, same_individuals,
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

const T2: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/t2>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
  SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(2 :r owl:Thing))
  FunctionalObjectProperty(:r)
  ClassAssertion(:Y :a) ClassAssertion(:Y :c)
)";

const T3: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/t3>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:E)) Declaration(Class(:F))
  Declaration(Class(:G)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:c))
  SubClassOf(:E ObjectUnionOf(:F :G)) DisjointClasses(:F :E) DisjointClasses(:G :E)
  ClassAssertion(ObjectSomeValuesFrom(:r :E) :c) ClassAssertion(:Y :a)
)";

fn ctl() -> String {
    T2.replace("ObjectMinCardinality(2", "ObjectMinCardinality(1")
}

const X: &str = "http://ex/#X";
const Y: &str = "http://ex/#Y";
const A: &str = "http://ex/#a";

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn class(iri: &str) -> ClassExpression<RcStr> {
    ClassExpression::Class(Build::<RcStr>::new().class(iri))
}

fn is_inconsistent<T: std::fmt::Debug>(r: &Result<T, ReasonError>) -> bool {
    matches!(r, Err(ReasonError::Inconsistent))
}

#[test]
fn every_surface_errors_on_a_tableau_only_inconsistency() {
    let _g = lock();
    for (name, src) in [("t2", T2), ("t3", T3)] {
        let o = parse(src);
        let y = class(Y);
        let thing = class("http://www.w3.org/2002/07/owl#Thing");
        assert!(
            is_inconsistent(&is_instance_of_reporting(&o, X, A)),
            "{name} instance"
        );
        assert!(
            is_inconsistent(&instances_of_reporting(&o, X)),
            "{name} instances"
        );
        // The #369 follow-up wires these two (the light route).
        assert!(
            is_inconsistent(&class_expression_satisfiable(&o, &y)),
            "{name} sat-expr"
        );
        assert!(
            is_inconsistent(&class_expression_entailed_subclass(&o, &y, &thing)),
            "{name} subclass-expr"
        );
        assert!(
            is_inconsistent(&class_expression_instances(&o, &class(X))),
            "{name} instances-expr"
        );
        assert!(
            is_inconsistent(&different_individuals(&o, None)),
            "{name} different"
        );
    }
    // `same_individuals` needs two named individuals a probe can merge.
    assert!(
        is_inconsistent(&same_individuals(&parse(T2), None)),
        "t2 same"
    );
}

#[test]
fn a_consistent_kb_keeps_its_answers_and_no_flag() {
    let _g = lock();
    let o = parse(&ctl());
    let (holds, flags) = is_instance_of_reporting(&o, Y, A).unwrap();
    assert!(holds);
    assert!(
        !flags.consistency_undetermined && !flags.incomplete,
        "{flags:?}"
    );
    let (members, flags) = instances_of_reporting(&o, Y).unwrap();
    assert_eq!(members, ["http://ex/#a", "http://ex/#c"]);
    assert!(!flags.consistency_undetermined, "{flags:?}");
    let r = class_expression_instances(&o, &class(Y)).unwrap();
    assert!(!r.individuals().is_empty() && !r.consistency_undetermined());
    let (holds, _) = is_instance_of_reporting(&o, X, A).unwrap();
    assert!(!holds);
}

#[test]
fn the_flag_reverts_to_the_vacuous_answers() {
    let _g = lock();
    let _off = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY", "0");
    let (holds, flags) = is_instance_of_reporting(&parse(T2), X, A).unwrap();
    assert!(
        holds,
        "the vacuous membership of an inconsistent KB, as before #369"
    );
    assert!(!flags.consistency_undetermined);
}

#[test]
fn report_mode_never_errors() {
    let _g = lock();
    let _rep = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY", "report");
    let (holds, _) = is_instance_of_reporting(&parse(T2), X, A).unwrap();
    assert!(holds);
}

/// The guard already in place reverts the lazy check too.
#[test]
fn the_query_inconsistency_guard_flag_reverts_it_too() {
    let _g = lock();
    let _off = SetEnvGuard::set("RUSTDL_QUERY_INCONSISTENCY_GUARD", "0");
    assert!(is_instance_of_reporting(&parse(T2), X, A).unwrap().0);
}

/// With the wedge undetermined and the tail off (the default), a positive answer
/// is kept and flagged; with the tail on, the main tableau decides it.
#[test]
fn an_undetermined_wedge_is_reported_and_the_tail_decides_it() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    for src in [T2, T3] {
        let o = parse(src);
        let (holds, flags) = is_instance_of_reporting(&o, X, A).unwrap();
        assert!(holds && flags.consistency_undetermined, "{flags:?}");
        assert!(!flags.incomplete, "not a give-up");
        let (_, flags) = instances_of_reporting(&o, X).unwrap();
        assert!(flags.consistency_undetermined);
        assert!(
            class_expression_instances(&o, &class(X))
                .unwrap()
                .consistency_undetermined()
        );
        assert!(
            different_individuals(&o, None)
                .unwrap()
                .consistency_undetermined()
        );
        {
            let _tail = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1");
            assert!(is_inconsistent(&is_instance_of_reporting(&o, X, A)));
        }
    }
    // The tail settles a consistent KB too, which clears the flag.
    let o = parse(&ctl());
    assert!(
        is_instance_of_reporting(&o, Y, A)
            .unwrap()
            .1
            .consistency_undetermined
    );
    let _tail = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1");
    assert!(
        !is_instance_of_reporting(&o, Y, A)
            .unwrap()
            .1
            .consistency_undetermined
    );
}

/// The check runs only after a positive answer: with the wedge undetermined, a
/// negative answer would carry the flag if the check had run.
#[test]
fn a_negative_answer_never_runs_the_check() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let o = parse(&ctl());
    let (holds, flags) = is_instance_of_reporting(&o, X, A).unwrap();
    assert!(!holds && !flags.consistency_undetermined, "{flags:?}");
    let (members, flags) = instances_of_reporting(&o, X).unwrap();
    assert!(members.is_empty() && !flags.consistency_undetermined);
    let r = class_expression_instances(&o, &class(X)).unwrap();
    assert!(r.individuals().is_empty() && !r.consistency_undetermined());
}

/// A cut tail is no verdict: the answer stays, flagged.
#[test]
fn a_cut_tail_keeps_the_answer() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let _tail = SetEnvGuard::set("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1");
    let _budget = SetEnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "0");
    let (holds, flags) = is_instance_of_reporting(&parse(T2), X, A).unwrap();
    assert!(holds && flags.consistency_undetermined, "{flags:?}");
}
