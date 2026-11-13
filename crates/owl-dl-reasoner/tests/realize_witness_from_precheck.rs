//! #430: `realize` takes its pseudo-model witness from the model classify's
//! inconsistency pre-check already built (route 2, the `ABox`-seeded wedge), and
//! builds its own under `RUSTDL_PSEUDO_MODEL_WITNESS_MS` only when there is none.
//!
//! The tests separate the two sources with `RUSTDL_PSEUDO_MODEL_WITNESS_MS=0`,
//! which gives the fallback build an expired deadline, so it never yields a model.
//! A prune that is active there can only rest on the pre-check's model.
//!
//! Every answer is compared with the no-prune arm (`RUSTDL_PSEUDO_MODEL=0`), the
//! ground truth here: the prune only skips probes, so it must never change a type
//! set.
//!
//! Run: `cargo test -p owl-dl-reasoner --test realize_witness_from_precheck`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Realization, realize};
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const FLAGS: [&str; 3] = [
    "RUSTDL_PSEUDO_MODEL",
    "RUSTDL_PSEUDO_MODEL_WITNESS_MS",
    "RUSTDL_REALIZE_WITNESS_FROM_PRECHECK",
];

/// Sets the three flags (`None` = unset) for one realize call, restoring them
/// afterwards. Callers hold `ENV_MUTEX`.
#[allow(unsafe_code)]
fn realize_with(onto: &SetOntology<RcStr>, values: [Option<&str>; 3]) -> Realization {
    let prior: Vec<_> = FLAGS.iter().map(std::env::var_os).collect();
    // SAFETY: set_var/remove_var are unsafe under edition 2024. Every test here
    // holds `ENV_MUTEX` around the call, and the prior values are restored below.
    unsafe {
        for (k, v) in FLAGS.iter().zip(values) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
    let r = realize(onto);
    // SAFETY: as above.
    unsafe {
        for (k, v) in FLAGS.iter().zip(prior) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
    r.expect("realization")
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The defaults: the pre-check's model first, the 1000 ms fallback second.
const DEFAULT: [Option<&str>; 3] = [None, None, None];
/// The fallback can never build, so a witness can only come from the pre-check.
const PRECHECK_ONLY: [Option<&str>; 3] = [None, Some("0"), None];
/// The kill switch, with the fallback still disabled: no witness at all.
const NEITHER: [Option<&str>; 3] = [None, Some("0"), Some("0")];
/// The kill switch alone: today's 1000 ms build.
const FALLBACK_ONLY: [Option<&str>; 3] = [None, None, Some("0")];
/// No prune at all: ground truth.
const NO_PRUNE: [Option<&str>; 3] = [Some("0"), None, None];

const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("{HEADER}Ontology(<http://rustdl.test/t>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    read(&mut reader, ParserConfiguration::default())
        .expect("fixture parses")
        .0
}

fn iri(local: &str) -> String {
    format!("http://rustdl.test/{local}")
}

fn sorted_types(r: &Realization, ind: &str) -> Vec<String> {
    let mut t = r.entailed_types(&iri(ind)).to_vec();
    t.sort();
    t
}

/// Three individuals with different types, entailed through an existential (not
/// the told closure, which answers before the prune), a merge and a nominal (to
/// stay off the saturation fast path).
///
/// - `a : ∃r.G` and `∃r.G ⊑ H`, so `a : H`;
/// - `b : D`, `D ⊑ E`, and `b` is the same as `e`, which is `K`; so `b, e : D, E, K`;
/// - `c : F`, functional `f` with `f(c, x)` and `f(c, y)`, `x : M`, so `y : M`;
/// - `c : U ⊔ V` with `U ⊑ W` and `V ⊑ W`, so `c : W`. Saturation cannot derive this
///   one, so only the witness (or a probe) answers it: without it, a witness that
///   lacks a type or is indexed wrongly would pass, since every other type above
///   is answered before the prune;
/// - `DisjointClasses(D, F)`.
const MIXED: &str = "\
Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))\n\
Declaration(Class(:G)) Declaration(Class(:H)) Declaration(Class(:K))\n\
Declaration(Class(:M)) Declaration(Class(:Nom))\n\
Declaration(Class(:U)) Declaration(Class(:V)) Declaration(Class(:W))\n\
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:f))\n\
Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:e))\n\
Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y))\n\
SubClassOf(ObjectSomeValuesFrom(:r :G) :H)\n\
SubClassOf(:D :E)\n\
DisjointClasses(:D :F)\n\
FunctionalObjectProperty(:f)\n\
EquivalentClasses(:Nom ObjectOneOf(:a))\n\
ClassAssertion(ObjectSomeValuesFrom(:r :G) :a)\n\
ClassAssertion(:D :b) ClassAssertion(:K :e) SameIndividual(:b :e)\n\
ClassAssertion(:F :c) ObjectPropertyAssertion(:f :c :x) ObjectPropertyAssertion(:f :c :y)\n\
ClassAssertion(:M :x)\n\
SubClassOf(:U :W) SubClassOf(:V :W)\n\
ClassAssertion(ObjectUnionOf(:U :V) :c)";

const MIXED_INDIVIDUALS: [&str; 6] = ["a", "b", "c", "e", "x", "y"];

/// With the fallback disabled, the prune still runs, on the pre-check's model, and
/// every type set equals the no-prune arm's.
#[test]
fn precheck_model_drives_the_prune_and_loses_no_type() {
    let _g = lock();
    let o = onto(MIXED);
    let pruned = realize_with(&o, PRECHECK_ONLY);
    let truth = realize_with(&o, NO_PRUNE);
    assert!(
        pruned.witness_prune_active(),
        "the pre-check's model must stand in for the disabled fallback"
    );
    assert!(!truth.witness_prune_active());
    for ind in MIXED_INDIVIDUALS {
        assert_eq!(
            sorted_types(&pruned, ind),
            sorted_types(&truth, ind),
            "types of {ind}"
        );
    }
    // The entailments the fixture is built around, so a vacuous truth arm fails.
    for (ind, class) in [
        ("a", "H"),
        ("b", "E"),
        ("b", "K"),
        ("e", "D"),
        ("e", "E"),
        ("y", "M"),
        ("c", "W"),
    ] {
        assert!(
            sorted_types(&pruned, ind).contains(&iri(class)),
            "{ind} : {class}, got {:?}",
            sorted_types(&pruned, ind)
        );
    }
    assert!(
        !sorted_types(&pruned, "c").contains(&iri("D")),
        "c : F and F ⊥ D"
    );
}

/// `RUSTDL_REALIZE_WITNESS_FROM_PRECHECK=0` restores today's behaviour: with the
/// fallback disabled too, there is no witness.
#[test]
fn kill_switch_takes_no_model_from_the_precheck() {
    let _g = lock();
    let o = onto(MIXED);
    let r = realize_with(&o, NEITHER);
    assert!(
        !r.witness_prune_active(),
        "the kill switch must leave only the fallback"
    );
    let fallback = realize_with(&o, FALLBACK_ONLY);
    assert!(
        fallback.witness_prune_active(),
        "today's own build still runs under the kill switch"
    );
    let default = realize_with(&o, DEFAULT);
    assert!(default.witness_prune_active());
    for ind in MIXED_INDIVIDUALS {
        assert_eq!(
            sorted_types(&fallback, ind),
            sorted_types(&default, ind),
            "{ind}"
        );
    }
}

/// `RUSTDL_PSEUDO_MODEL=0` turns the prune off whatever the pre-check found.
#[test]
fn pseudo_model_off_uses_no_precheck_model() {
    let _g = lock();
    let r = realize_with(&onto(MIXED), [Some("0"), Some("0"), None]);
    assert!(!r.witness_prune_active());
}

/// #344: where the ontology counts data values, the wedge's `Sat` skips that
/// count, so its model is not taken; with the fallback disabled there is then no
/// witness. The control below shows the same ontology without the count does
/// get one, so the test can fail.
#[test]
fn data_counting_keeps_the_precheck_model_out() {
    let _g = lock();
    let base = "\
Declaration(Class(:P)) Declaration(Class(:Nom)) Declaration(DataProperty(:p))\n\
Declaration(NamedIndividual(:a))\n\
EquivalentClasses(:Nom ObjectOneOf(:a))\n\
ClassAssertion(:P :a)\n\
DataPropertyAssertion(:p :a \"1\"^^xsd:integer)";
    let counting = format!(
        "{base}\nSubClassOf(:P DataMaxCardinality(2 :p DataOneOf(\"1\"^^xsd:integer \
         \"2\"^^xsd:integer \"3\"^^xsd:integer)))"
    );
    let control = realize_with(&onto(base), PRECHECK_ONLY);
    assert!(
        control.witness_prune_active(),
        "control: no count, model taken"
    );
    let r = realize_with(&onto(&counting), PRECHECK_ONLY);
    assert!(
        !r.witness_prune_active(),
        "a model that skipped a data count must not drive the prune"
    );
    let truth = realize_with(&onto(&counting), NO_PRUNE);
    assert_eq!(sorted_types(&r, "a"), sorted_types(&truth, "a"));
}

/// The committed nominal-`ABox` fixtures: the default (pre-check model) answers
/// exactly as the no-prune arm and as the fallback-only arm.
#[test]
fn nominal_abox_fixtures_match_with_and_without_the_precheck_model() {
    let _g = lock();
    for name in ["nominal_abox.ofn", "nominal_abox_scaled.ofn"] {
        let path = format!(
            "{}/tests/fixtures/pseudo_model/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let src = std::fs::read_to_string(&path).expect("fixture present");
        let mut reader = Cursor::new(src);
        let o: SetOntology<RcStr> = read(&mut reader, ParserConfiguration::default())
            .expect("fixture parses")
            .0;
        let default = realize_with(&o, DEFAULT);
        let precheck = realize_with(&o, PRECHECK_ONLY);
        let fallback = realize_with(&o, FALLBACK_ONLY);
        let truth = realize_with(&o, NO_PRUNE);
        assert!(
            precheck.witness_prune_active(),
            "{name}: pre-check model taken"
        );
        for ind in truth.individuals() {
            let t = |r: &Realization| {
                let mut v = r.entailed_types(ind).to_vec();
                v.sort();
                v
            };
            assert_eq!(t(&default), t(&truth), "{name} {ind}: default");
            assert_eq!(t(&precheck), t(&truth), "{name} {ind}: pre-check only");
            assert_eq!(t(&fallback), t(&truth), "{name} {ind}: fallback only");
        }
    }
}
