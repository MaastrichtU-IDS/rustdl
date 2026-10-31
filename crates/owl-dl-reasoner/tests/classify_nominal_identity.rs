//! `RUSTDL_CLASSIFY_WEDGE_NOMINALS` (default ON, #383): the classify wedge is built
//! `with_nominals`, so two nodes carrying the same `{a}` merge.
//!
//! Before, `{a}` was an ordinary class in the classify wedge. `classify` trusts that
//! wedge's `Sat` in its unsat probe and in the label heuristic, so it missed every
//! entailment that needs two occurrences of `{a}` to be one individual, reporting
//! `incomplete: false`, while `sat` / `subclass` (the main tableau) were right.
//!
//! Verdicts are HermiT's. The controls are the false-positive side: merging nominal
//! nodes adds entailments, so a wrong merge would show up as a spurious one.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_nominal_identity`

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
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

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const NS: &str = "http://x.org/f#";

fn classify(body: &str) -> Classification {
    let src = format!(
        "Prefix(:=<{NS}>)\n\
         Ontology(<http://x.org/f>\n\
         Declaration(Class(:X)) Declaration(Class(:C)) Declaration(Class(:D))\n\
         Declaration(Class(:Y)) Declaration(Class(:A)) Declaration(Class(:Q))\n\
         Declaration(Class(:E)) Declaration(Class(:F))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:t))\n\
         Declaration(ObjectProperty(:s))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         {body}\n)"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    owl_dl_reasoner::classify(&o).unwrap()
}

fn unsat(c: &Classification, class: &str) -> bool {
    c.unsatisfiable_classes()
        .contains(&format!("{NS}{class}").as_str())
}

fn sub(c: &Classification, a: &str, b: &str) -> bool {
    c.is_subclass(&format!("{NS}{a}"), &format!("{NS}{b}"))
}

/// The witness `A` reaches `a` through `s⁻`.
const S_INV_A: &str = "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))";

/// The `TBox` form of #362/#376: both `r`-witnesses are `s`-successors of `a`, and
/// `s` is functional, so they are one element, contradicting `≥2`.
#[test]
fn counting_through_a_nominal_neighbour_is_unsat() {
    let _l = lock();
    let c = classify(&format!(
        "{S_INV_A}\nFunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))"
    ));
    assert!(unsat(&c, "Q"));
    // The same bound written at the nominal instead of as `Func(s)`.
    let c = classify(&format!(
        "{S_INV_A}\nSubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :s))\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))"
    ));
    assert!(unsat(&c, "Q"));
}

/// Control: one witness fits under `Func(s)`.
#[test]
fn one_witness_through_a_nominal_neighbour_stays_sat() {
    let _l = lock();
    let c = classify(&format!(
        "{S_INV_A}\nFunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(1 :r :A))"
    ));
    assert!(!unsat(&c, "Q"));
}

/// No counting: both fillers are `a`, so `a` is `C` and `D`, which are disjoint.
#[test]
fn two_fillers_that_are_one_individual_clash() {
    let _l = lock();
    let c = classify(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :C)))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:t ObjectIntersectionOf(ObjectOneOf(:a) :D)))\n\
         DisjointClasses(:C :D)",
    );
    assert!(unsat(&c, "X"));
}

/// Control: two different individuals do not merge.
#[test]
fn fillers_that_are_different_individuals_do_not_clash() {
    let _l = lock();
    let c = classify(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :C)))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:t ObjectIntersectionOf(ObjectOneOf(:b) :D)))\n\
         DisjointClasses(:C :D)\nDifferentIndividuals(:a :b)",
    );
    assert!(!unsat(&c, "X"));
}

/// A subsumption, pruned by the label heuristic before: the `t`-filler `a` is `C`
/// because the `r`-filler is, so `X ⊑ ∃t.C ⊑ Y`.
#[test]
fn a_label_learned_through_one_edge_reaches_the_other() {
    let _l = lock();
    let c = classify(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :C)))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:t ObjectOneOf(:a)))\n\
         SubClassOf(ObjectSomeValuesFrom(:t :C) :Y)",
    );
    assert!(sub(&c, "X", "Y"));
}

/// Control: the `t`-filler is `b`, which is not known to be `C`.
#[test]
fn a_label_does_not_reach_a_different_individual() {
    let _l = lock();
    let c = classify(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :C)))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:t ObjectOneOf(:b)))\n\
         SubClassOf(ObjectSomeValuesFrom(:t :C) :Y)",
    );
    assert!(!sub(&c, "X", "Y"));
}

/// The probe root itself becomes `a` (`C ⊑ {a}`), so the merge folds the root into
/// another node and the root's labels must be read through the merge.
#[test]
fn a_root_that_is_the_nominal_gets_its_labels() {
    let _l = lock();
    let c = classify(
        "SubClassOf(:C ObjectOneOf(:a))\n\
         SubClassOf(:C ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :E)))\n\
         SubClassOf(:E :F)",
    );
    assert!(sub(&c, "C", "E"));
    assert!(sub(&c, "C", "F"));
}

/// Control: a root merely next to `a` is not `a`.
#[test]
fn a_root_next_to_the_nominal_is_not_it() {
    let _l = lock();
    let c = classify(
        "SubClassOf(:C ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :E)))\n\
         SubClassOf(:C ObjectSomeValuesFrom(:s ObjectOneOf(:a)))\n\
         SubClassOf(:E :F)",
    );
    assert!(!sub(&c, "C", "E"));
    assert!(!sub(&c, "C", "F"));
}

/// `=0` restores the old behaviour, where every one of the shapes above was missed.
#[test]
fn flag_off_restores_the_old_answers() {
    let _l = lock();
    let _off = SetEnvGuard::set("RUSTDL_CLASSIFY_WEDGE_NOMINALS", "0");
    let c = classify(&format!(
        "{S_INV_A}\nFunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))"
    ));
    assert!(!unsat(&c, "Q"));
    let c = classify(
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(ObjectOneOf(:a) :C)))\n\
         SubClassOf(:X ObjectSomeValuesFrom(:t ObjectOneOf(:a)))\n\
         SubClassOf(ObjectSomeValuesFrom(:t :C) :Y)",
    );
    assert!(!sub(&c, "X", "Y"));
}
