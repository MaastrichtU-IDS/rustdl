//! #361/#362 `RUSTDL_NOMINAL_BLOCK_GUARD` (default ON): pair blocking never blocks,
//! and never blocks against, a node that is or stands for a nominal individual.
//!
//! A blocked node is replaced in the model by a copy of its blocker's subtree. When
//! that subtree has an edge into a nominal `a`, the copy gives `a` one more neighbour,
//! which can break a `≤n` at `a`. Two engines had the gap:
//! * main tableau (#362): `Q ⊑ ≥2 r.A`, `A ⊑ ∃s⁻.{a}`, `s` functional. The second
//!   `r`-witness was blocked by the first, so `a` saw one `s`-neighbour, not two.
//! * wedge (#361): `{a} : ≥3 s.{a,b}`. Two witnesses were blocked by the first, which
//!   then merged into `a` and stayed a blocker; the pigeonhole clash never formed.
//!
//! Every verdict below is HermiT's. The controls are the false-positive side.
//!
//! This is a blocking guard, not the SHOIQ NN-rule. The `known_*` tests pin what it
//! still misses; a fix should FLIP them.
//!
//! Run: `cargo test -p owl-dl-reasoner --test nominal_block_guard`

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

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

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const FLAG: &str = "RUSTDL_NOMINAL_BLOCK_GUARD";
const NS: &str = "http://x.org/f#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://x.org/f>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:s1)) Declaration(ObjectProperty(:t))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:q))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

fn sat(body: &str, class: &str) -> bool {
    owl_dl_reasoner::is_class_satisfiable(&onto(body), &format!("{NS}{class}")).unwrap()
}

fn consistent(body: &str) -> bool {
    owl_dl_reasoner::is_consistent(&onto(body)).unwrap()
}

// ---- #362: a blocked witness hides an edge into a nominal ----

/// The issue's reproducer.
const I362: &str = "\
    SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
    FunctionalObjectProperty(:s)\n\
    SubClassOf(:Q ObjectMinCardinality(2 :r :A))";

#[test]
fn issue_362_q_is_unsatisfiable() {
    let _l = lock();
    assert!(!sat(I362, "Q"));
}

#[test]
fn issue_362_variants_are_unsatisfiable() {
    let _l = lock();
    // Two nominals, three witnesses.
    assert!(!sat(
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a :b)))\n\
         FunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(3 :r :A))",
        "Q"
    ));
    // The same through a union of two existentials.
    assert!(!sat(
        "SubClassOf(:A ObjectUnionOf(ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)) \
         ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b))))\n\
         FunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(3 :r :A))",
        "Q"
    ));
    // `≤1 s` written on the nominal instead of `Functional(s)`.
    assert!(!sat(
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :s owl:Thing))\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))",
        "Q"
    ));
}

/// Role spellings: the edge is an inverse, or on a sub-role; the `≤` is qualified.
#[test]
fn role_and_qualifier_spellings_are_unsatisfiable() {
    let _l = lock();
    assert!(!sat(
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectOneOf(:a)))\n\
         InverseFunctionalObjectProperty(:s)",
        "Q"
    ));
    assert!(!sat(
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s1) ObjectOneOf(:a)))\n\
         SubObjectPropertyOf(:s1 :s)\n\
         FunctionalObjectProperty(:s)",
        "Q"
    ));
    assert!(!sat(
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :s :C))\n\
         SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:s :C))",
        "Q"
    ));
}

/// The same shape on an individual, through `consistent`.
#[test]
fn abox_forms_are_inconsistent() {
    let _l = lock();
    assert!(!consistent(&format!("{I362}\nClassAssertion(:Q :q)")));
    // #361's `p0289` reduction: the `≥3` sits on a nominal.
    assert!(!consistent(
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b)))\n\
         FunctionalObjectProperty(:s)"
    ));
}

// ---- #361: the wedge blocked against a node merged into a nominal ----

#[test]
fn issue_361_variants_are_inconsistent() {
    let _l = lock();
    for body in [
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :a)",
        "SubClassOf(ObjectOneOf(:a :b) ObjectMinCardinality(3 :s ObjectOneOf(:a :b)))",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectUnionOf(ObjectOneOf(:a) ObjectOneOf(:b))) :a)",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :c)",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :a)\nDifferentIndividuals(:a :b)",
        "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :s ObjectOneOf(:a :b)))",
    ] {
        assert!(!consistent(body), "should be inconsistent: {body}");
    }
}

// ---- controls: satisfiable, and must stay so ----

#[test]
fn controls_stay_satisfiable() {
    let _l = lock();
    for body in [
        // `s` not functional.
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))",
        // Two nominals for two witnesses.
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a :b)))\n\
         FunctionalObjectProperty(:s)\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))",
        // `≤2` at the nominal.
        "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s owl:Thing))\n\
         SubClassOf(:Q ObjectMinCardinality(2 :r :A))",
        // The qualified `≤1 s.C` does not count `A`s, which are `¬C`.
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :s :C))\n\
         SubClassOf(:A ObjectComplementOf(:C))",
        // The functional role is another one.
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         FunctionalObjectProperty(:t)\n\
         SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:t owl:Thing))",
        // The nominal two edges away, without the inverse-functional step.
        "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :B))\n\
         SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
         FunctionalObjectProperty(:s)",
    ] {
        assert!(sat(body, "Q"), "should be satisfiable: {body}");
    }
    for body in [
        "ClassAssertion(ObjectMinCardinality(2 :s ObjectOneOf(:a :b)) :a)",
        "ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b :c)) :a)",
    ] {
        assert!(consistent(body), "should be consistent: {body}");
    }
}

/// Termination: every `A` is an `s`-neighbour of `a`, and `A ⊑ ∃r.A` generates them
/// without end. While `a` is not over-full they are never blocked; past `≤2` the guard
/// lifts and blocking bounds the chain until the `≤` merges. Never blocking them
/// grew the graph to the node cap. Irreflexive + asymmetric `r` leaves no merge.
#[test]
fn guarded_chain_terminates() {
    let _l = lock();
    let chain = "\
        SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
        ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a))))\n";
    let start = std::time::Instant::now();
    assert!(!sat(
        &format!(
            "{chain}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
             IrreflexiveObjectProperty(:r)\nAsymmetricObjectProperty(:r)"
        ),
        "A"
    ));
    assert!(sat(
        &format!("{chain}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))"),
        "A"
    ));
    assert!(sat(
        &format!(
            "{chain}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(3 :s))\n\
             IrreflexiveObjectProperty(:r)"
        ),
        "A"
    ));
    assert!(start.elapsed() < std::time::Duration::from_secs(60));
}

#[test]
fn flag_off_restores_the_old_answers() {
    let _l = lock();
    let _off = SetEnvGuard::set(FLAG, "0");
    assert!(sat(I362, "Q"), "flag off: the #362 miss");
    assert!(
        consistent("ClassAssertion(ObjectMinCardinality(3 :s ObjectOneOf(:a :b)) :a)"),
        "flag off: the #361 miss"
    );
}

// ---- known limitations: a future NN-rule should FLIP these ----

/// The nominal is two edges below the blocker. `t` inverse-functional makes the
/// two `B`s' merge at `a` force the two `A`s together, against their `≠`. HermiT:
/// `Q` unsatisfiable. The SHOIQ NN-rule propagates nominal-ness up to the `A`s;
/// this guard looks one edge away, so anywhere blocking (`sat`) still misses it.
/// Ancestor blocking does not block siblings and gets it right.
#[test]
fn known_nominal_two_edges_away_is_missed() {
    let _l = lock();
    assert!(
        sat(
            "SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
             SubClassOf(:A ObjectSomeValuesFrom(:t :B))\n\
             SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
             FunctionalObjectProperty(:s)\n\
             InverseFunctionalObjectProperty(:t)",
            "Q"
        ),
        "FLIP: the nominal-two-edges-away miss is fixed"
    );
}

/// NOT this change's defect, pinned because the guard sends more merges through
/// it: `A` is satisfiable (`{a, y0, y1}`, `r = {(y0,y1),(y1,y0)}`,
/// `s = {(a,y0),(a,y1)}`), and rustdl says unsat on main too. The `≤2` merge makes
/// an `r` self-loop whose clash does not carry the merge decision, so the search
/// skips the other merge.
#[test]
fn known_max_merge_self_loop_false_positive() {
    let _l = lock();
    assert!(
        !sat(
            "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
             ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a))))\n\
             SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
             IrreflexiveObjectProperty(:r)",
            "A"
        ),
        "FLIP: the ≤-merge self-loop false positive is fixed"
    );
}
