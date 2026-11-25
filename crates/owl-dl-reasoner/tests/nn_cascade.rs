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

// ---------------------------------------------------------------------------
// #564: `classify` on the same family.
//
// The per-class label wedge decides `classify`'s unsat probe and `trust_sat` pair
// refutations. It had no `≤1 t⁻` for `InverseFunctional(t)` whenever conversion's
// `inv_func_merge_consumable` declined the GCI, so its `Sat` was a model of a weaker
// KB: `Q` read satisfiable (`RUSTDL_CLASSIFY_WEDGE_IF_BOUND`). And with the
// consistency wedge off, classify's pre-check ran only the `ABox` saturation, so the
// `ABox` form read consistent with every signal clean
// (`RUSTDL_CLASSIFY_MAIN_CONSISTENCY_FALLBACK`). Verdicts are HermiT's.
// ---------------------------------------------------------------------------

/// `Q` (or `class`) among `classify`'s unsatisfiable classes, on the unbounded
/// top-down driver, a per-pair-budgeted run, and the `n²` driver. The budget is a
/// generous 2 s, not the CLI's 5 ms: a positive under a 5 ms budget is a race.
fn classify_unsat(body: &str, class: &str) -> [bool; 3] {
    let o = onto(body);
    let iri = format!("{NS}{class}");
    let has =
        |c: &owl_dl_reasoner::Classification| c.unsatisfiable_classes().contains(&iri.as_str());
    [
        has(&owl_dl_reasoner::classify(&o).unwrap()),
        has(
            &owl_dl_reasoner::classify_with_timeout(&o, std::time::Duration::from_secs(2)).unwrap(),
        ),
        has(&owl_dl_reasoner::classify_n2(&o).unwrap()),
    ]
}

/// [`classify_unsat`] on a worker thread, `None` if it does not finish within 60 s,
/// so a shape that sends a wedge into a cycle fails instead of hanging the suite.
fn classify_unsat_bounded(body: &str, class: &str) -> Option<[bool; 3]> {
    let (body, class) = (body.to_owned(), class.to_owned());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(classify_unsat(&body, &class));
    });
    rx.recv_timeout(std::time::Duration::from_secs(60)).ok()
}

/// `classify`'s `consistent` on the `ABox` form (`Q(d)` added), which must be decided.
fn classify_consistent(body: &str, what: &str) -> bool {
    let c = owl_dl_reasoner::classify(&onto(&format!("{body}\nClassAssertion(:Q :d)"))).unwrap();
    let s = c.stats();
    assert!(
        !s.consistency_undetermined,
        "a decided verdict expected: {what}"
    );
    !s.inconsistent
}

/// The r-cycle shape. With the bound, the wedge's Horn fixpoint on it builds nodes
/// that its nominal merges fold straight back, and the NN guard refuses every
/// blocker, so it never settles; the merge-churn guard
/// (`RUSTDL_WEDGE_MERGE_CHURN_GUARD`) stops it with `Stalled` and the main tableau
/// decides. Before the guard the unbounded `classify` did not return on it.
const R_CYCLE: &str = "an r-cycle back from B to A";

#[test]
fn issue_564_classify_finds_q_unsat_on_every_cascade_shape() {
    let _l = lock();
    for (what, body) in inconsistent_shapes() {
        assert_eq!(
            classify_unsat_bounded(&body, "Q"),
            Some([true; 3]),
            "classify: Q unsat, within the bound: {what}"
        );
    }
}

/// The unbounded `classify` returns on the r-cycle shape, with `Q` unsatisfiable
/// (the merge-churn guard; `=0` brings the non-termination back).
#[test]
fn issue_564_r_cycle_terminates_in_classify() {
    let _l = lock();
    let (_, body) = inconsistent_shapes()
        .into_iter()
        .find(|(w, _)| *w == R_CYCLE)
        .unwrap();
    let b = body.clone();
    assert_eq!(
        Some(true),
        bounded(move || {
            let c = owl_dl_reasoner::classify(&onto(&b)).ok()?;
            Some(
                c.unsatisfiable_classes()
                    .contains(&format!("{NS}Q").as_str()),
            )
        }),
        "Q unsat within the bound"
    );
}

#[test]
fn issue_564_classify_controls_keep_q_satisfiable() {
    let _l = lock();
    for (what, body) in consistent_controls() {
        assert_eq!(
            classify_unsat(&body, "Q"),
            [false; 3],
            "classify control: {what}"
        );
        assert!(
            classify_consistent(&body, what),
            "classify control, ABox form: {what}"
        );
    }
}

#[test]
fn issue_564_classify_abox_form_is_inconsistent_with_and_without_the_wedge() {
    let _l = lock();
    for (what, body) in inconsistent_shapes() {
        assert!(!classify_consistent(&body, what), "default: {what}");
        let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        assert!(
            !classify_consistent(&body, what),
            "consistency wedge off: {what}"
        );
    }
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    for (what, body) in consistent_controls() {
        assert!(
            classify_consistent(&body, what),
            "consistency wedge off, control: {what}"
        );
    }
}

/// Each #564 flag is load-bearing on its own. With the bound off, the label wedge's
/// `Sat` is back and `classify` misses `Q`. With the bound off and the consistency
/// wedge off, only the main-tableau fallback finds the `ABox` form's clash (its
/// `Q` is no longer unsat to the label wedge, so no other route can), and turning
/// that off too brings back the issue's `consistent: true`.
#[test]
fn issue_564_flags_off_restore_the_old_answers() {
    let _l = lock();
    let _b = SetEnvGuard::set("RUSTDL_CLASSIFY_WEDGE_IF_BOUND", "0");
    // The `n²` driver finds `Q` without the bound (it does not prune on the label
    // wedge), so the miss shows on the other two.
    assert_eq!(
        classify_unsat(&repro(), "Q")[..2],
        [false; 2],
        "bound off: the #564 miss"
    );
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    assert!(
        !classify_consistent(&repro(), "repro"),
        "fallback on: decided by the main tableau"
    );
    let _f = SetEnvGuard::set("RUSTDL_CLASSIFY_MAIN_CONSISTENCY_FALLBACK", "0");
    let c =
        owl_dl_reasoner::classify(&onto(&format!("{}\nClassAssertion(:Q :d)", repro()))).unwrap();
    assert!(
        !c.stats().inconsistent,
        "both off: the #564 consistent: true"
    );
}

/// The same miss with no nominal at all: `t ⊑ u`, `Functional(u)` merges `C`'s
/// `t`-successor into `B`, which then has two `t`-predecessors, `X` and `C`, so
/// `IF(t)` makes them one (HermiT: `X` unsatisfiable with `X ⊓ C ⊑ ⊥`, and
/// `X ⊑ C`, so `X ⊑ Y` with `C ⊑ Y`). Conversion's census declines `IF(t)` here
/// too.
const X_T_B: &str = "SubClassOf(:X ObjectSomeValuesFrom(:t :B))";
const B_U_C: &str = "SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:u) :C))";
const C_T_D: &str = "SubClassOf(:C ObjectSomeValuesFrom(:t :D))";
const T_U: &str = "SubObjectPropertyOf(:t :u)";
const F_U: &str = "FunctionalObjectProperty(:u)";

fn x_merge(extra: &[&str]) -> String {
    let mut parts = vec![
        "Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:Y))",
        X_T_B,
        B_U_C,
        C_T_D,
        T_U,
        F_U,
        IF_T,
    ];
    parts.extend_from_slice(extra);
    join(&parts)
}

#[test]
fn issue_564_classify_merge_through_a_functional_super_role() {
    let _l = lock();
    let body = x_merge(&["DisjointClasses(:X :C)"]);
    assert_eq!(classify_unsat(&body, "X"), [true; 3], "X unsat");
    let body = x_merge(&["SubClassOf(:C :Y)"]);
    let o = onto(&body);
    let (x, y, c_) = (format!("{NS}X"), format!("{NS}Y"), format!("{NS}C"));
    for c in [
        owl_dl_reasoner::classify(&o).unwrap(),
        owl_dl_reasoner::classify_with_timeout(&o, std::time::Duration::from_millis(5)).unwrap(),
        owl_dl_reasoner::classify_n2(&o).unwrap(),
    ] {
        assert!(c.is_subclass(&x, &c_), "X ⊑ C");
        assert!(c.is_subclass(&x, &y), "X ⊑ Y");
    }
}

/// HermiT: `X` satisfiable once any link is gone.
#[test]
fn issue_564_classify_merge_controls() {
    let _l = lock();
    for drop in [X_T_B, B_U_C, C_T_D, T_U, F_U, IF_T] {
        let body = x_merge(&["DisjointClasses(:X :C)"]).replace(drop, "");
        assert_eq!(
            classify_unsat(&body, "X"),
            [false; 3],
            "X satisfiable without {drop}"
        );
    }
}

// ---------------------------------------------------------------------------
// #564 polarity canaries for the injected bound (`RUSTDL_CLASSIFY_WEDGE_IF_BOUND`).
// `InverseFunctional(t)` bounds `t`-PREDECESSORS (`≤1 t⁻`), never successors, and a
// bound on `t` says nothing about a super-role. Conversion declines the GCI on these
// shapes, so the bound in the wedge is the injected one. Every surface that builds a
// `HyperCache` is checked: `classify` (three drivers) and the per-pair named query
// (`is_subclass_of`, behind `subclass` / `explain`). The controls show the surfaces
// do see a `≤1 t⁻` clash where there is one. Verdicts are HermiT's semantics: each
// FP canary has a model with `C` non-empty.
// ---------------------------------------------------------------------------

const DISJ_AB: &str = "DisjointClasses(:A :B)";

/// `C` satisfiable on `classify` and on the per-pair surface (`C ⋢ ⊥`, `C ⋢ Q`).
fn c_satisfiable_everywhere(body: &str) -> bool {
    let unsat = classify_unsat(body, "C");
    let o = onto(body);
    let c = format!("{NS}C");
    unsat == [false; 3]
        && !owl_dl_reasoner::is_subclass_of(&o, &c, NOTHING).unwrap()
        && !owl_dl_reasoner::is_subclass_of(&o, &c, &format!("{NS}Q")).unwrap()
}

/// Two `t`-SUCCESSORS in disjoint classes: `IF(t)` does not merge them.
#[test]
fn issue_564_if_bound_polarity_successors_are_not_bounded() {
    let _l = lock();
    let body = join(&[
        "SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:t :A) ObjectSomeValuesFrom(:t :B)))",
        DISJ_AB,
        IF_T,
    ]);
    assert!(c_satisfiable_everywhere(&body), "C satisfiable");
    // Control: two `t`-predecessors in disjoint classes do clash.
    let ctrl = join(&[
        "SubClassOf(:C ObjectIntersectionOf(\
         ObjectSomeValuesFrom(ObjectInverseOf(:t) :A) \
         ObjectSomeValuesFrom(ObjectInverseOf(:t) :B)))",
        DISJ_AB,
        IF_T,
    ]);
    assert_eq!(classify_unsat(&ctrl, "C"), [true; 3], "control: C unsat");
    assert!(
        owl_dl_reasoner::is_subclass_of(&onto(&ctrl), &format!("{NS}C"), NOTHING).unwrap(),
        "control, per pair: C ⊑ ⊥"
    );
}

/// `IF(t)` with `t ⊑ u`: two `u`-predecessors are not bounded.
#[test]
fn issue_564_if_bound_polarity_super_role_is_not_bounded() {
    let _l = lock();
    let body = join(&[
        "SubObjectPropertyOf(:t :u)",
        "SubClassOf(:C ObjectIntersectionOf(\
         ObjectSomeValuesFrom(ObjectInverseOf(:u) :A) \
         ObjectSomeValuesFrom(ObjectInverseOf(:u) :B)))",
        DISJ_AB,
        IF_T,
    ]);
    assert!(c_satisfiable_everywhere(&body), "C satisfiable");
    // And a bound on the super-role does not leak down as two `t`-predecessors'
    // worth: one `t`- and one `u`-predecessor in disjoint classes is fine too.
    let mixed = join(&[
        "SubObjectPropertyOf(:t :u)",
        "SubClassOf(:C ObjectIntersectionOf(\
         ObjectSomeValuesFrom(ObjectInverseOf(:t) :A) \
         ObjectSomeValuesFrom(ObjectInverseOf(:u) :B)))",
        DISJ_AB,
        IF_T,
    ]);
    assert!(c_satisfiable_everywhere(&mixed), "C satisfiable, mixed");
}

/// #564, the fallback's give-up arm: with the consistency wedge off and a 1 ms
/// pre-check budget, the main-tableau fallback cannot finish on a few hundred
/// individuals, so `classify` must say `consistency_undetermined` rather than read
/// "consistent" off a check that did not run. (The `Some(true)` arm, which clears
/// the flag, is pinned by the wedge-off controls in
/// `issue_564_classify_abox_form_is_inconsistent_with_and_without_the_wedge`.)
#[test]
fn issue_564_fallback_give_up_is_undetermined() {
    let _l = lock();
    let mut body = String::from(
        "SubClassOf(:A ObjectUnionOf(:B :C))\n\
         SubClassOf(:B ObjectSomeValuesFrom(:r :A))\n\
         SubClassOf(:C ObjectSomeValuesFrom(:t :A))\n",
    );
    for i in 0..400 {
        use std::fmt::Write as _;
        let _ = writeln!(
            body,
            "Declaration(NamedIndividual(:i{i})) ClassAssertion(:A :i{i}) \
             ObjectPropertyAssertion(:u :i{i} :i{})",
            i + 1
        );
    }
    let _w = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let _b = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "1");
    let c = owl_dl_reasoner::classify(&onto(&body)).unwrap();
    assert!(!c.stats().inconsistent, "consistent KB");
    assert!(
        c.stats().consistency_undetermined,
        "a 1 ms fallback is no verdict"
    );
}

// ---------------------------------------------------------------------------
// #571: a `Stalled` consistency wedge in classify's pre-check.
//
// On the r-cycle shape the merge-churn guard stops the consistency wedge with
// `Stalled`. With `Q(d)` classify still ends inconsistent through `Q`'s unsat probe,
// but with `A(d)` or `B(d)` (HermiT: consistent, `Q ≡ ⊥`) nothing else decides,
// and the pre-check left `consistency_undetermined` set. The main-tableau fallback
// (`RUSTDL_CLASSIFY_STALLED_WEDGE_FALLBACK`) now decides it.
// ---------------------------------------------------------------------------

fn r_cycle() -> String {
    inconsistent_shapes()
        .into_iter()
        .find(|(w, _)| *w == R_CYCLE)
        .unwrap()
        .1
}

/// `(consistent, consistency_undetermined)` on the r-cycle with `assertion`, from
/// the top-down driver, a run with a per-pair timeout (30 s, generous so a loaded
/// runner cannot cut it), and the `n²` driver. Every one of them gives the
/// pre-check its own adaptive budget (`classify_inconsistency_budget_ms`); the
/// no-deadline cap is pinned separately, in `stalled_wedge_consistency_cli.rs`.
fn r_cycle_classify(assertion: &str) -> [(bool, bool); 3] {
    let o = onto(&format!("{}\n{assertion}", r_cycle()));
    let read = |c: owl_dl_reasoner::Classification| {
        let s = c.stats();
        (!s.inconsistent, s.consistency_undetermined)
    };
    [
        read(owl_dl_reasoner::classify(&o).unwrap()),
        read(
            owl_dl_reasoner::classify_with_timeout(&o, std::time::Duration::from_secs(30)).unwrap(),
        ),
        read(owl_dl_reasoner::classify_n2(&o).unwrap()),
    ]
}

#[test]
fn issue_571_stalled_wedge_r_cycle_abox_is_decided() {
    let _l = lock();
    for x in ["A", "B"] {
        assert_eq!(
            r_cycle_classify(&format!("ClassAssertion(:{x} :d)")),
            [(true, false); 3],
            "{x}(d): consistent, decided"
        );
    }
    assert_eq!(
        r_cycle_classify("ClassAssertion(:Q :d)"),
        [(false, false); 3],
        "Q(d): inconsistent"
    );
}

/// The fallback is what decides `A(d)`: either flag off brings back the undecided
/// pre-check, and `Q(d)` stays inconsistent either way.
#[test]
fn issue_571_flags_off_leave_the_stalled_wedge_undetermined() {
    let _l = lock();
    for flag in [
        "RUSTDL_CLASSIFY_STALLED_WEDGE_FALLBACK",
        "RUSTDL_CLASSIFY_MAIN_CONSISTENCY_FALLBACK",
    ] {
        let _f = SetEnvGuard::set(flag, "0");
        assert_eq!(
            r_cycle_classify("ClassAssertion(:A :d)"),
            [(true, true); 3],
            "{flag}=0: A(d) undetermined"
        );
        assert_eq!(
            r_cycle_classify("ClassAssertion(:Q :d)"),
            [(false, false); 3],
            "{flag}=0: Q(d) inconsistent"
        );
    }
}
