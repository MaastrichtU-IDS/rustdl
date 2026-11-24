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

/// CPU-independent cost pin (Fable's review of #559): `sat C₁` on a deep
/// inverse-functional chain costs about the same with the cascade as without. The
/// unmemoised cascade walked the chain below a node on every guard call, about
/// `n^2.7` (`n = 4000`: 22 CPU-s against 0.6 s, release); memoised per trail
/// generation it is linear. A wide margin against the flag-off arm of the same
/// process keeps this independent of the host's speed.
#[test]
fn a_deep_inverse_functional_chain_costs_no_more_with_the_cascade() {
    let _l = lock();
    for nominal in [false, true] {
        let body = if_chain(1200, nominal);
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
            "nominal={nominal}: cascade {on:?} against {off:?} without it"
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
/// top-down driver, the CLI's 5 ms per-pair budget, and the `n²` driver.
fn classify_unsat(body: &str, class: &str) -> [bool; 3] {
    let o = onto(body);
    let iri = format!("{NS}{class}");
    let has =
        |c: &owl_dl_reasoner::Classification| c.unsatisfiable_classes().contains(&iri.as_str());
    [
        has(&owl_dl_reasoner::classify(&o).unwrap()),
        has(
            &owl_dl_reasoner::classify_with_timeout(&o, std::time::Duration::from_millis(5))
                .unwrap(),
        ),
        has(&owl_dl_reasoner::classify_n2(&o).unwrap()),
    ]
}

/// `classify`'s `consistent` on the `ABox` form (`Q(d)` added). A proven
/// inconsistency is decided even when a pre-check route gave up on the way (the CLI
/// masks `consistency_undetermined` the same way); a `consistent` must be decided.
fn classify_consistent(body: &str, what: &str) -> bool {
    let c = owl_dl_reasoner::classify(&onto(&format!("{body}\nClassAssertion(:Q :d)"))).unwrap();
    let s = c.stats();
    assert!(
        s.inconsistent || !s.consistency_undetermined,
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
            classify_unsat(&body, "Q"),
            [true; 3],
            "classify: Q unsat: {what}"
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
