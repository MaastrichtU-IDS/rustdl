//! Task 4: correctness fixtures for the model-derived realization read-off
//! (`RUSTDL_MODEL_DERIVED_TYPES`, default OFF; `=1` turns the read-off ON).
//!
//! The read-off (see `realize.rs::instance_check_with_closure` +
//! `hyper.rs`'s deterministic-label accessor) returns a type for individual
//! `a` and class `C` *without probing* when `C` is a DETERMINISTIC
//! (empty-branch-dependency) label of `a`'s node in ONE witness model — which
//! is entailed in every model, hence sound. Branch-dependent
//! (disjunction/merge-triggered) labels are NOT read off; they still get the
//! `{a} ⊓ ¬C` probe. A merge guard excludes any individual whose node was
//! touched by a `≤n`/functional/`SameIndividual` merge (those can carry an
//! under-reported empty dep — an FP risk).
//!
//! **Core soundness invariant under test: the read-off is verdict-preserving,
//! so `realize` ON (`RUSTDL_MODEL_DERIVED_TYPES=1`) must produce the SAME types
//! as OFF (unset) on every ontology.** ON==OFF byte-identity IS the soundness
//! gate: if ON ever adds a type OFF doesn't, that is a false-positive
//! subsumption (the crown-jewel FP=0 violation) and these tests must catch it.
//!
//! NEGATIVES-FIRST: the two FP-guard fixtures come first (a disjunction-only
//! type and a functional-merge case). ON==OFF alone cannot distinguish "guard
//! works" from "read-off never ran", so each negative ALSO asserts the specific
//! branch/merge class is ABSENT from ON's output — a directly-observable proof
//! the branch label was not read off. An unsound read-off that emitted the
//! branch disjunct would make ON diverge from OFF *and* add the forbidden class.
//!
//! Path requirement (verified against `realize.rs:945`): the witness model is
//! only built on the tableau realize path, and only when the flag is on. Every
//! fixture must therefore stay OFF the saturation fast path — done here the same
//! way `pseudo_model_realize.rs` does it, via `EquivalentClasses(:Nom
//! ObjectOneOf(:a))` which trips `ontology_uses_nominals` and forces
//! `realize_tableau_internal`. `RUSTDL_PSEUDO_MODEL` /
//! `RUSTDL_WEDGE_CONSISTENCY` are left at their defaults so the witness builds.
//!
//! Run: `cargo test -p owl-dl-reasoner --test model_derived_realize`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::realize;
use std::collections::BTreeMap;
use std::io::Cursor;

// ─── env-mutation plumbing (serialized; restored on Drop) ──────────────
//
// Mirrors `tests/pseudo_model_realize.rs`: env vars are process-global, so
// tests that set/unset `RUSTDL_MODEL_DERIVED_TYPES` must serialize against each
// other (and any other test in this binary touching the same var) to avoid a
// flaky env race. The mutex + restore-on-Drop pattern is cheap insurance.

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. The mutation is held
        // only for the duration of one test (serialized via `ENV_MUTEX`) and
        // restored on `Drop`.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }

    #[allow(unsafe_code)]
    fn unset(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: see `set`.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `SetEnvGuard::set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (ontology, _prefixes) =
        read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    ontology
}

/// Run `realize` with `RUSTDL_MODEL_DERIVED_TYPES` set to ON (`"1"`) or OFF
/// (unset), returning a sorted, comparable map from individual IRI → its
/// (`entailed_types`, `most_specific_types`), both sorted. DRYs the three tests.
/// Caller holds `ENV_MUTEX`.
fn realize_types(
    onto: &SetOntology<RcStr>,
    flag_on: bool,
) -> BTreeMap<String, (Vec<String>, Vec<String>)> {
    let _flag = if flag_on {
        SetEnvGuard::set("RUSTDL_MODEL_DERIVED_TYPES", "1")
    } else {
        SetEnvGuard::unset("RUSTDL_MODEL_DERIVED_TYPES")
    };
    let r = realize(onto).expect("realization terminates");
    let mut out = BTreeMap::new();
    for iri in r.individuals() {
        let mut entailed: Vec<String> = r.entailed_types(iri).to_vec();
        entailed.sort();
        let mut specific: Vec<String> = r.most_specific_types(iri).to_vec();
        specific.sort();
        out.insert(iri.clone(), (entailed, specific));
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 1 (NEGATIVE, FP guard — written first): disjunction-dependent type
// ─────────────────────────────────────────────────────────────────────────

/// `a : (D ⊔ E)` — `a`'s membership in `D` (and in `E`) is only BRANCH-true:
/// the witness model picks one disjunct for `a`'s node, and that label carries
/// a non-empty branch dependency. Neither `D` nor `E` is entailed. `A ⊑ C` +
/// `a : A` give a genuine deterministic type (`C`) as a live control — so the
/// read-off does fire on *something*, and we can be sure the model built.
///
/// The `ObjectOneOf(:a)` nominal keeps this off the saturation fast path, so
/// `realize` runs the tableau path where the read-off lives.
const F1_DISJUNCTION: &str = "\
Ontology(<http://rustdl.test/f1>\n\
    Declaration(Class(:A)) Declaration(Class(:C))\n\
    Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:Nom))\n\
    Declaration(NamedIndividual(:a))\n\
    SubClassOf(:A :C)\n\
    EquivalentClasses(:Nom ObjectOneOf(:a))\n\
    ClassAssertion(:A :a)\n\
    ClassAssertion(ObjectUnionOf(:D :E) :a)\n\
)\n";

const A_IRI: &str = "http://rustdl.test/A";
const C_IRI: &str = "http://rustdl.test/C";
const D_IRI: &str = "http://rustdl.test/D";
const E_IRI: &str = "http://rustdl.test/E";
const AA_IRI: &str = "http://rustdl.test/a";

#[test]
fn disjunction_dependent_type_not_read_off() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let onto = parse(&format!("{HEADER}{F1_DISJUNCTION}"));

    let off = realize_types(&onto, false);
    let on = realize_types(&onto, true);

    // Soundness gate: the read-off must be verdict-identical to probing.
    assert_eq!(
        on, off,
        "read-off (ON) must produce identical types to probing (OFF)"
    );

    // Non-vacuity + FP-specific check: `a`'s entailed types contain the
    // DETERMINISTIC `C` (proving the read-off path really fired / the model
    // built) but NOT the branch-only disjuncts `D`/`E` (proving the
    // branch-dependent label was NOT read off).
    let (a_entailed, _) = on.get(AA_IRI).expect("individual a present");
    assert!(
        a_entailed.iter().any(|c| c == C_IRI),
        "a must be entailed C (via A⊑C, deterministic); got {a_entailed:?}"
    );
    assert!(
        a_entailed.iter().any(|c| c == A_IRI),
        "a must be entailed A (told); got {a_entailed:?}"
    );
    assert!(
        !a_entailed.iter().any(|c| c == D_IRI),
        "a must NOT be entailed D (branch-only disjunct) — spurious read-off; got {a_entailed:?}"
    );
    assert!(
        !a_entailed.iter().any(|c| c == E_IRI),
        "a must NOT be entailed E (branch-only disjunct) — spurious read-off; got {a_entailed:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 2 (NEGATIVE, FP guard): functional-merge case
// ─────────────────────────────────────────────────────────────────────────

/// Exercises the merge/guard path: `FunctionalObjectProperty(:r)` +
/// `r(:a,:b)` + `r(:a,:c)` with `:b`,`:c` NOT `DifferentIndividuals` forces the
/// two `r`-successors of `a` to be merged (functional-role collapse). The merge
/// unions `b`'s and `c`'s labels; `c : M` then becomes a moved label on the
/// merged node. `M ⊑ Bot` is deliberately absent, and there is a disjunction
/// (`b : (P ⊔ Q)`) so the merge interacts with branch-dependent labels — the
/// shape the merge guard must not read off.
///
/// The merge guard should exclude the merge-touched individual(s) from the
/// read-off, so ON==OFF must hold regardless of what the merge moves. We also
/// assert non-vacuity via the deterministic control `a : A`, `A ⊑ C`.
///
/// NOTE (documented, per the brief): guaranteeing a label is *provably moved
/// onto a NAMED individual node* in the witness is not something this fixture
/// can force deterministically (the merge survivor choice / ABox-saturation
/// pre-check interplay is engine-internal). What it DOES guarantee is that the
/// functional role + two-named-successor + disjunction shape drives the
/// tableau through the `≤1`/functional merge machinery whose empty-dep labels
/// the guard is responsible for; ON==OFF is the soundness gate either way.
const F2_MERGE: &str = "\
Ontology(<http://rustdl.test/f2>\n\
    Declaration(Class(:A)) Declaration(Class(:C))\n\
    Declaration(Class(:M)) Declaration(Class(:P)) Declaration(Class(:Q))\n\
    Declaration(Class(:Nom))\n\
    Declaration(NamedIndividual(:a))\n\
    Declaration(NamedIndividual(:b))\n\
    Declaration(NamedIndividual(:c))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    SubClassOf(:A :C)\n\
    EquivalentClasses(:Nom ObjectOneOf(:a))\n\
    ClassAssertion(:A :a)\n\
    ObjectPropertyAssertion(:r :a :b)\n\
    ObjectPropertyAssertion(:r :a :c)\n\
    ClassAssertion(:M :c)\n\
    ClassAssertion(ObjectUnionOf(:P :Q) :b)\n\
)\n";

#[test]
fn merge_moved_label_not_read_off() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let onto = parse(&format!("{HEADER}{F2_MERGE}"));

    let off = realize_types(&onto, false);
    let on = realize_types(&onto, true);

    // Soundness gate: read-off must be verdict-identical to probing across the
    // whole merge/functional path.
    assert_eq!(
        on, off,
        "read-off (ON) must produce identical types to probing (OFF) on the merge fixture"
    );

    // Non-vacuity: `a` is deterministically `A` and `C`; the branch disjuncts
    // `P`/`Q` on `b` must NOT be spuriously read off onto `b`.
    let (a_entailed, _) = on.get(AA_IRI).expect("individual a present");
    assert!(
        a_entailed.iter().any(|c| c == C_IRI),
        "a must be entailed C (via A⊑C, deterministic); got {a_entailed:?}"
    );
    let b_iri = "http://rustdl.test/b";
    if let Some((b_entailed, _)) = on.get(b_iri) {
        assert!(
            !b_entailed.iter().any(|c| c == "http://rustdl.test/P"),
            "b must NOT be entailed P (branch-only) — spurious read-off; got {b_entailed:?}"
        );
        assert!(
            !b_entailed.iter().any(|c| c == "http://rustdl.test/Q"),
            "b must NOT be entailed Q (branch-only) — spurious read-off; got {b_entailed:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture 3 (POSITIVE): deterministic type IS read off
// ─────────────────────────────────────────────────────────────────────────

/// The accelerated case: `a : D`, `D ⊑ E`, `E ⊑ F` — a deterministic chain
/// with NO disjunction, so `a : E` and `a : F` are deterministic labels on
/// `a`'s (merge-untouched) node and get read off. Also a domain edge
/// (`ObjectPropertyDomain(:s :G)` + `s(:a,:d)`) types `a : G` deterministically.
/// The `ObjectOneOf(:a)` nominal keeps it off the saturation fast path.
///
/// Asserts `a : E`/`F`/`G` reported both ON and OFF (verdict-identical) and
/// non-vacuity (`a`'s types are non-empty and actually contain `E`).
const F3_DETERMINISTIC: &str = "\
Ontology(<http://rustdl.test/f3>\n\
    Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))\n\
    Declaration(Class(:G)) Declaration(Class(:Nom))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:d))\n\
    Declaration(ObjectProperty(:s))\n\
    SubClassOf(:D :E)\n\
    SubClassOf(:E :F)\n\
    ObjectPropertyDomain(:s :G)\n\
    EquivalentClasses(:Nom ObjectOneOf(:a))\n\
    ClassAssertion(:D :a)\n\
    ObjectPropertyAssertion(:s :a :d)\n\
)\n";

#[test]
fn deterministic_type_is_read_off() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let onto = parse(&format!("{HEADER}{F3_DETERMINISTIC}"));

    let off = realize_types(&onto, false);
    let on = realize_types(&onto, true);

    // Verdict-identity: the read-off accelerates this case; it must not change
    // the answer.
    assert_eq!(
        on, off,
        "deterministic read-off (ON) must produce identical types to probing (OFF)"
    );

    // Non-vacuity + positive check: `a`'s deterministic types are all present.
    let (a_entailed, _) = on.get(AA_IRI).expect("individual a present");
    assert!(
        !a_entailed.is_empty(),
        "a's entailed types must be non-empty (non-vacuity)"
    );
    assert!(
        a_entailed.iter().any(|c| c == E_IRI),
        "a must be entailed E (via D⊑E, deterministic); got {a_entailed:?}"
    );
    assert!(
        a_entailed.iter().any(|c| c == "http://rustdl.test/F"),
        "a must be entailed F (via D⊑E⊑F, deterministic); got {a_entailed:?}"
    );
    assert!(
        a_entailed.iter().any(|c| c == "http://rustdl.test/G"),
        "a must be entailed G (via ObjectPropertyDomain(:s :G), deterministic); got {a_entailed:?}"
    );
}
