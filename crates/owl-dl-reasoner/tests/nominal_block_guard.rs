//! #362 `RUSTDL_NOMINAL_BLOCK_GUARD` (default ON), main tableau: pair blocking never
//! blocks, and never blocks against, a node that is or stands for a nominal
//! individual, and a `≤` merge prunes the merged node's subtree (SHOIQ).
//!
//! A blocked node is replaced in the model by a copy of its blocker's subtree. When
//! that subtree has an edge into a nominal `a`, the copy gives `a` one more neighbour,
//! which can break a `≤n` at `a`: `Q ⊑ ≥2 r.A`, `A ⊑ ∃s⁻.{a}`, `s` functional; the
//! second `r`-witness was blocked by the first, so `a` saw one `s`-neighbour, not two.
//! Guarding those witnesses alone looped: each `≤` merge reparented a child that
//! nothing could block. Pruning the merged node's subtree, as SHOIQ does, ends it.
//!
//! Every verdict below is HermiT's. The controls are the false-positive side. The
//! wedge's side (#361) is #375's.
//!
//! This is a guard plus prune, not the SHOIQ NN-rule (no `≤m` guess, no new nominal
//! nodes). The `known_*` tests pin what it still misses; a fix should FLIP them.
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

/// A termination claim on the deadline-free `sat` path (anywhere blocking, where the
/// loop was): the verdict within 20 s, `None` if there is none. Run on a worker so
/// a hang fails the test instead of stalling the suite; `*_with_timeout` would take
/// the deadline path, which uses ancestor blocking and cannot see the loop.
fn sat_bounded(body: &str, class: &str) -> Option<bool> {
    let (body, iri) = (body.to_owned(), format!("{NS}{class}"));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(owl_dl_reasoner::is_class_satisfiable(&onto(&body), &iri).ok());
    });
    rx.recv_timeout(std::time::Duration::from_secs(20))
        .ok()
        .flatten()
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

/// The same shape on an individual, and #361's `p0289` reduction (the `≥3` on a
/// nominal), through the main tableau (`RUSTDL_WEDGE_CONSISTENCY=0`).
const ABOX_FORMS: [&str; 2] = [
    "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))\n\
     FunctionalObjectProperty(:s)\n\
     SubClassOf(:Q ObjectMinCardinality(2 :r :A))\n\
     ClassAssertion(:Q :q)",
    "SubClassOf(ObjectOneOf(:a) ObjectMinCardinality(3 :r :A))\n\
     SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b)))\n\
     FunctionalObjectProperty(:s)",
];

#[test]
fn abox_forms_are_inconsistent_on_the_main_tableau() {
    let _l = lock();
    let _wedge_off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    for body in ABOX_FORMS {
        assert!(!consistent(body), "should be inconsistent: {body}");
    }
}

/// The same forms on the default route, which trusts the wedge's `Sat`. The wedge
/// used to accept a blocker merely next to an individual under a `≤` that counts it
/// (#376, `RUSTDL_WEDGE_NN_GUARD`); this was the pinned miss until that was fixed.
#[test]
fn abox_forms_are_inconsistent_on_the_default_route() {
    let _l = lock();
    for body in ABOX_FORMS {
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
/// lifts, generation at a counted node waits, and the `≤` merges. The merged node's
/// subtree is pruned: reparenting it looped (each round left a child nothing could
/// block), with this file's extra declared individuals. Irreflexive + asymmetric `r`
/// leaves no merge.
#[test]
fn guarded_chain_terminates() {
    let _l = lock();
    let chain = "\
        SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
        ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a))))\n";
    assert_eq!(
        Some(false),
        sat_bounded(
            &format!(
                "{chain}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
             IrreflexiveObjectProperty(:r)\nAsymmetricObjectProperty(:r)"
            ),
            "A"
        )
    );
    assert_eq!(
        Some(true),
        sat_bounded(
            &format!("{chain}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))"),
            "A"
        )
    );
    assert_eq!(
        Some(true),
        sat_bounded(
            &format!(
                "{chain}SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(3 :s))\n\
             IrreflexiveObjectProperty(:r)"
            ),
            "A"
        )
    );
}

#[test]
fn flag_off_restores_the_old_answers() {
    let _l = lock();
    let _off = SetEnvGuard::set(FLAG, "0");
    assert!(sat(I362, "Q"), "flag off: the #362 miss");
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

/// #373's reproducer, fixed there: after the self-looping `≤2` merge is refuted the
/// other merge is tried. Under this guard it must still terminate.
#[test]
fn max_merge_self_loop_is_satisfiable() {
    let _l = lock();
    assert!(sat(
        "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
         ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a))))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))\n\
         IrreflexiveObjectProperty(:r)",
        "A"
    ));
}

/// The prune reaches past the merged node's children: each `A` also has a `t`-chain
/// of `B`s (grandchildren of a merged witness), and with `t` transitive the chain
/// has edges that skip a level. Both satisfiable.
#[test]
fn prune_takes_deep_subtrees() {
    let _l = lock();
    let deep = "SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :A) \
        ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)) ObjectSomeValuesFrom(:t :B)))\n\
        SubClassOf(:B ObjectSomeValuesFrom(:t :B))\n\
        SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))";
    assert_eq!(Some(true), sat_bounded(deep, "A"));
    assert_eq!(
        Some(true),
        sat_bounded(&format!("{deep}\nTransitiveObjectProperty(:t)"), "A")
    );
}
