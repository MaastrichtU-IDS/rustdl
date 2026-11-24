//! #555 `RUSTDL_NN_CASCADE` (default ON), main tableau: the #362 nominal block guard
//! follows the SHOIQ NN-rule's cascade. A node the guard treats as a nominal's
//! `≤`-witness acts as a nominal for its tree parent, so a `≤` it holds that counts
//! the parent guards the parent too.
//!
//! The issue's shape: `Q ⊑ ≥2 r.A`, `A ⊑ ∃t.B`, `B ⊑ ∃s⁻.{a}`, `s` functional, `t`
//! inverse-functional. `Functional(s)` makes the two `B`s one node, the one
//! `s`-successor of `a`; `IF(t)` then merges its two `t`-predecessors, the two `A`s,
//! against `≥2 r.A`. The guard looked one edge away (at the `B`s), so the second `A`
//! was blocked by the first, `a` saw one `B`, and the main tableau accepted the
//! model: `consistent` with `RUSTDL_WEDGE_CONSISTENCY=0` said consistent with
//! `relies_on_trusted_consistency: false`, and `sat Q` said satisfiable.
//!
//! Every verdict here is HermiT's (Konclude calls the issue's ABox consistent, which
//! is wrong). Consistent / satisfiable controls first: they are the false-positive
//! side.
//!
//! Run: `cargo test -p owl-dl-reasoner --test nn_cascade`

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Every test locks.
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
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{NS}>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://x.org/f>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:t1)) Declaration(ObjectProperty(:u))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:d))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `consistent` on the main tableau: the consistency wedge, which decides these
/// since #553, is off.
fn consistent_main(body: &str) -> bool {
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    owl_dl_reasoner::is_consistent(&onto(&format!("{body}\nClassAssertion(:Q :d)"))).unwrap()
}

fn sat_q(body: &str) -> bool {
    owl_dl_reasoner::is_class_satisfiable(&onto(body), &format!("{NS}Q")).unwrap()
}

fn q_is_nothing(body: &str) -> bool {
    owl_dl_reasoner::is_subclass_of(&onto(body), &format!("{NS}Q"), NOTHING).unwrap()
}

const Q2: &str = "SubClassOf(:Q ObjectMinCardinality(2 :r :A))";
const A_T_B: &str = "SubClassOf(:A ObjectSomeValuesFrom(:t :B))";
const B_S_A: &str = "SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))";
const F_S: &str = "FunctionalObjectProperty(:s)";
const IF_T: &str = "InverseFunctionalObjectProperty(:t)";

fn join(parts: &[&str]) -> String {
    parts.join("\n")
}

/// The issue's reproducer (TBox part; `consistent_main` adds `Q(d)`).
fn repro() -> String {
    join(&[Q2, A_T_B, B_S_A, F_S, IF_T])
}

/// HermiT: inconsistent with `Q(d)`, `Q` unsatisfiable. Each is a different spelling
/// of the cascade.
fn inconsistent_shapes() -> Vec<(&'static str, String)> {
    vec![
        ("the issue's reproducer", repro()),
        (
            "IF(t) spelled Functional(t⁻)",
            join(&[
                Q2,
                A_T_B,
                B_S_A,
                F_S,
                "FunctionalObjectProperty(ObjectInverseOf(:t))",
            ]),
        ),
        (
            "F and IF swapped: B is A's t-predecessor, s points at a",
            join(&[
                Q2,
                "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:t) :B))",
                "FunctionalObjectProperty(:t)",
                "SubClassOf(:B ObjectSomeValuesFrom(:s ObjectOneOf(:a)))",
                "InverseFunctionalObjectProperty(:s)",
            ]),
        ),
        (
            "the nominal on the other side: B ⊑ ∃s.{a}, IF(s)",
            join(&[
                Q2,
                A_T_B,
                "SubClassOf(:B ObjectSomeValuesFrom(:s ObjectOneOf(:a)))",
                "InverseFunctionalObjectProperty(:s)",
                IF_T,
            ]),
        ),
        (
            "≥3 over two nominals",
            join(&[
                "SubClassOf(:Q ObjectMinCardinality(3 :r :A))",
                A_T_B,
                "SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a :b)))",
                F_S,
                IF_T,
            ]),
        ),
        (
            "the t-edge through a sub-role t1 ⊑ t",
            join(&[
                Q2,
                "SubClassOf(:A ObjectSomeValuesFrom(:t1 :B))",
                "SubObjectPropertyOf(:t1 :t)",
                B_S_A,
                F_S,
                IF_T,
            ]),
        ),
        (
            "three edges: A -t-> B -u-> C -s⁻-> a",
            join(&[
                Q2,
                A_T_B,
                "SubClassOf(:B ObjectSomeValuesFrom(:u :C))",
                "SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))",
                F_S,
                IF_T,
                "InverseFunctionalObjectProperty(:u)",
            ]),
        ),
        // Fable's review of #559 (each HermiT-checked).
        (
            "an r-cycle back from B to A",
            join(&[
                Q2,
                A_T_B,
                B_S_A,
                "SubClassOf(:B ObjectSomeValuesFrom(:r :A))",
                F_S,
                IF_T,
            ]),
        ),
        (
            "IF(t) as the qualified ≤1 t⁻.A on B",
            join(&[
                Q2,
                A_T_B,
                B_S_A,
                F_S,
                "SubClassOf(:B ObjectMaxCardinality(1 ObjectInverseOf(:t) :A))",
            ]),
        ),
        (
            "one more inverse-functional level above A",
            join(&[
                "SubClassOf(:Q ObjectMinCardinality(2 :r :C))",
                "SubClassOf(:C ObjectSomeValuesFrom(:u :A))",
                "InverseFunctionalObjectProperty(:u)",
                A_T_B,
                B_S_A,
                F_S,
                IF_T,
            ]),
        ),
        (
            "four edges: A -t-> B -u-> C -t1-> D -s⁻-> a",
            join(&[
                Q2,
                A_T_B,
                "SubClassOf(:B ObjectSomeValuesFrom(:u :C))",
                "SubClassOf(:C ObjectSomeValuesFrom(:t1 :D))",
                "SubClassOf(:D ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))",
                F_S,
                IF_T,
                "InverseFunctionalObjectProperty(:u)",
                "InverseFunctionalObjectProperty(:t1)",
            ]),
        ),
        (
            "a disjunctive nominal with ≥3",
            join(&[
                "SubClassOf(:Q ObjectMinCardinality(3 :r :A))",
                A_T_B,
                "SubClassOf(:B ObjectUnionOf(\
                 ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)) \
                 ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:b))))",
                F_S,
                IF_T,
            ]),
        ),
        (
            "{a} ⊑ ≤2 s with ≥3",
            join(&[
                "SubClassOf(:Q ObjectMinCardinality(3 :r :A))",
                A_T_B,
                B_S_A,
                "SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(2 :s))",
                IF_T,
            ]),
        ),
    ]
}

/// HermiT: consistent with `Q(d)`, `Q` satisfiable. Each drops one link of a chain
/// above.
fn consistent_controls() -> Vec<(&'static str, String)> {
    vec![
        ("no Functional(s)", join(&[Q2, A_T_B, B_S_A, IF_T])),
        ("no InverseFunctional(t)", join(&[Q2, A_T_B, B_S_A, F_S])),
        (
            "≥1 instead of ≥2",
            join(&[
                "SubClassOf(:Q ObjectMinCardinality(1 :r :A))",
                A_T_B,
                B_S_A,
                F_S,
                IF_T,
            ]),
        ),
        (
            "≥2 over two nominals",
            join(&[
                Q2,
                A_T_B,
                "SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a :b)))",
                F_S,
                IF_T,
            ]),
        ),
        (
            "three edges, the middle role u not inverse-functional",
            join(&[
                Q2,
                A_T_B,
                "SubClassOf(:B ObjectSomeValuesFrom(:u :C))",
                "SubClassOf(:C ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))",
                F_S,
                IF_T,
            ]),
        ),
        (
            "swapped, no Functional(t)",
            join(&[
                Q2,
                "SubClassOf(:A ObjectSomeValuesFrom(ObjectInverseOf(:t) :B))",
                "SubClassOf(:B ObjectSomeValuesFrom(:s ObjectOneOf(:a)))",
                "InverseFunctionalObjectProperty(:s)",
            ]),
        ),
    ]
}

#[test]
fn controls_stay_consistent_and_satisfiable() {
    let _l = lock();
    for (what, body) in consistent_controls() {
        assert!(consistent_main(&body), "should be consistent: {what}");
        assert!(sat_q(&body), "Q should be satisfiable: {what}");
        assert!(!q_is_nothing(&body), "Q ⊑ ⊥ should not hold: {what}");
    }
}

#[test]
fn issue_555_main_tableau_finds_the_inconsistency() {
    let _l = lock();
    assert!(
        !consistent_main(&repro()),
        "#555: the main tableau must see the Functional + IF cascade at the nominal"
    );
}

#[test]
fn issue_555_q_is_unsatisfiable() {
    let _l = lock();
    assert!(!sat_q(&repro()));
    assert!(q_is_nothing(&repro()));
}

#[test]
fn cascade_variants_are_inconsistent_and_unsatisfiable() {
    let _l = lock();
    for (what, body) in inconsistent_shapes() {
        assert!(!consistent_main(&body), "should be inconsistent: {what}");
        assert!(!sat_q(&body), "Q should be unsatisfiable: {what}");
        assert!(q_is_nothing(&body), "Q ⊑ ⊥ should hold: {what}");
    }
}

/// The flag is load-bearing: off, the main tableau's old answers come back (and
/// the controls are unchanged).
#[test]
fn flag_off_restores_the_old_answers() {
    let _l = lock();
    let _off = SetEnvGuard::set("RUSTDL_NN_CASCADE", "0");
    assert!(consistent_main(&repro()), "flag off: the #555 miss");
    assert!(sat_q(&repro()), "flag off: the #555 miss (sat)");
    assert!(
        !q_is_nothing(&repro()),
        "flag off: the #555 miss (subclass)"
    );
    for (what, body) in consistent_controls() {
        assert!(consistent_main(&body), "flag off, control: {what}");
    }
}

/// A verdict within 20 s on a worker thread, `None` if there is none, so a
/// non-terminating completion fails the test instead of stalling the suite.
fn bounded(f: impl FnOnce() -> Option<bool> + Send + 'static) -> Option<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(20))
        .ok()
        .flatten()
}

/// Termination: the cascade climbs only to a holder's tree parent, as the SHOIQ
/// NN-rule does. Followed downwards, the `≤1 u` at each guarded `A` would guard its
/// `u`-child, so the `u`-chain from the nominal would never be blocked. HermiT:
/// consistent, `A` satisfiable.
#[test]
fn cascade_does_not_climb_down_a_chain() {
    let _l = lock();
    let chain = "SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:u :A))\n\
                 SubClassOf(:A ObjectSomeValuesFrom(:u :A))\n\
                 FunctionalObjectProperty(:u)\n\
                 ClassAssertion(:A :d)";
    let body = chain.to_owned();
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    assert_eq!(
        Some(true),
        bounded(move || owl_dl_reasoner::is_consistent(&onto(&body)).ok()),
        "consistent, within the bound"
    );
    let body = chain.to_owned();
    assert_eq!(
        Some(true),
        bounded(move || {
            owl_dl_reasoner::is_class_satisfiable(&onto(&body), &format!("{NS}A")).ok()
        }),
        "A satisfiable, within the bound"
    );
}

/// Termination with the cascade active: cyclic TBoxes whose nodes a `≤` at a
/// nominal, or at a node it guards, keeps counting (Fable's review of #559). HermiT:
/// `Q` satisfiable, the ontology with `Q(d)` consistent.
#[test]
fn cyclic_tboxes_terminate_with_the_cascade() {
    let _l = lock();
    for body in [
        "SubClassOf(:Q ObjectSomeValuesFrom(:r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectOneOf(:a :b)))\n\
         InverseFunctionalObjectProperty(:s)\n\
         InverseFunctionalObjectProperty(:t)",
        "SubClassOf(:Q ObjectSomeValuesFrom(:r :A))\n\
         SubClassOf(:A ObjectUnionOf(ObjectSomeValuesFrom(:t :A) \
         ObjectSomeValuesFrom(:s ObjectOneOf(:a))))\n\
         InverseFunctionalObjectProperty(:s)\n\
         InverseFunctionalObjectProperty(:t)",
        "SubClassOf(:Q ObjectSomeValuesFrom(:r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(3 ObjectInverseOf(:s)))\n\
         InverseFunctionalObjectProperty(:t)",
        "SubClassOf(:Q ObjectSomeValuesFrom(:r :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:u :A))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectOneOf(:a)))\n\
         SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(3 ObjectInverseOf(:s)))\n\
         InverseFunctionalObjectProperty(:t)\n\
         InverseFunctionalObjectProperty(:u)",
    ] {
        let b = body.to_owned();
        assert_eq!(
            Some(true),
            bounded(
                move || owl_dl_reasoner::is_class_satisfiable(&onto(&b), &format!("{NS}Q")).ok()
            ),
            "Q satisfiable, within the bound: {body}"
        );
        let b = format!("{body}\nClassAssertion(:Q :d)");
        let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        assert_eq!(
            Some(true),
            bounded(move || owl_dl_reasoner::is_consistent(&onto(&b)).ok()),
            "consistent on the main tableau, within the bound: {body}"
        );
    }
}

/// `Cᵢ ⊑ ∃r.Cᵢ₊₁` for `i = 1..n`, `r` inverse-functional: every `Cᵢ₊₁` holds a
/// `≤1 r⁻` that counts its parent, so the cascade's walk from `C₁` runs the whole
/// chain. With `nominal`, the last class points at a nominal under a functional
/// role, so every level is guarded.
fn if_chain(n: usize, nominal: bool) -> String {
    use std::fmt::Write as _;
    let mut s = String::from("InverseFunctionalObjectProperty(:r)\n");
    for i in 1..=n {
        writeln!(s, "SubClassOf(:C{i} ObjectSomeValuesFrom(:r :C{}))", i + 1).unwrap();
    }
    if nominal {
        writeln!(
            s,
            "FunctionalObjectProperty(:s)\n\
             SubClassOf(:C{} ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))",
            n + 1
        )
        .unwrap();
    }
    s
}

/// #563 / #562: [`if_chain`] with `E` back-propagating up it (`Cₙ₊₁ ⊑ E`,
/// `E ⊑ ∀r⁻.E`) and an unrelated nominal (`Z ⊑ {a}`). The nominal puts a nominal
/// node in the completion, so the guard cannot skip the walk, and every `E` that
/// climbs a level is a new label.
fn back_chain(n: usize) -> String {
    use std::fmt::Write as _;
    let mut s = if_chain(n, false);
    writeln!(
        s,
        "SubClassOf(:C{} :E)\n\
         SubClassOf(:E ObjectAllValuesFrom(ObjectInverseOf(:r) :E))\n\
         SubClassOf(:Z ObjectOneOf(:a))",
        n + 1
    )
    .unwrap();
    s
}

/// #562: `E` climbs the chain against the saturation's node order, one pass per
/// level, so about 2,000 levels used up the 4,096 passes the deterministic
/// saturation allowed and `sat C₁` answered `NoVerdict`. A pass that only adds
/// labels and edges no longer counts.
#[test]
fn a_deep_chain_with_back_propagation_is_satisfiable_562() {
    let _l = lock();
    let o = onto(&back_chain(3000));
    assert!(owl_dl_reasoner::is_class_satisfiable(&o, &format!("{NS}C1")).unwrap());
}

/// CPU-independent cost pin (Fable's review of #559): `sat C₁` on a deep
/// inverse-functional chain costs about the same with the cascade as without. The
/// unmemoised cascade walked the chain below a node on every guard call, about
/// `n^2.7` (`n = 4000`: 22 CPU-s against 0.6 s, release); memoised per trail
/// generation it is linear. A wide margin against the flag-off arm of the same
/// process keeps this independent of the host's speed.
///
/// #563: [`back_chain`] adds a label per level after the chain is built, and the
/// memo keyed on the trail generation was rebuilt below each one (`n = 2000`: 1.0
/// CPU-s against 0.3 s, release). The tableau unit tests
/// `a_label_climbing_the_chain_reuses_the_walk_563` and
/// `no_walk_until_a_node_is_nominal_563` pin that cost by count.
#[test]
fn a_deep_inverse_functional_chain_costs_no_more_with_the_cascade() {
    let _l = lock();
    for (what, body) in [
        ("plain", if_chain(1200, false)),
        ("nominal", if_chain(1200, true)),
        (
            "back-propagation, unrelated nominal (#563)",
            back_chain(1200),
        ),
    ] {
        let o = onto(&body);
        let iri = format!("{NS}C1");
        let time = |o: &SetOntology<RcStr>| {
            let t = std::time::Instant::now();
            assert!(owl_dl_reasoner::is_class_satisfiable(o, &iri).unwrap());
            t.elapsed()
        };
        let off = {
            let _off = SetEnvGuard::set("RUSTDL_NN_CASCADE", "0");
            time(&o)
        };
        let on = time(&o);
        assert!(
            on <= off * 4 + std::time::Duration::from_secs(2),
            "{what}: cascade {on:?} against {off:?} without it"
        );
    }
}
