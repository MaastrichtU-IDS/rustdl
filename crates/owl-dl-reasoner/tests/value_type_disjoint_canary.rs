//! Negatives-first soundness canary for value-derived type-disjointness
//! (`RUSTDL_VALUE_TYPE_DISJOINT`). The mechanism seeds `T1⊓T2 ⊑ ⊥` for type
//! pairs forced to DIFFERENT `DifferentIndividuals`-distinct nominal values on
//! the SAME functional role. It is sound ONLY under both conditions — the
//! negative tests verify it does NOT produce a spurious clash (FP) when either
//! condition is absent. Runs as a dedicated test binary (process-isolated env)
//! so setting the flag cannot leak into other tests.
#![allow(unsafe_code, clippy::unwrap_used, clippy::default_trait_access)]

use std::sync::Mutex;

// Serializes env mutation across this file's tests (they run as threads in one
// process). All three want the same flag values, so we only ever set them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Build a tiny ontology: `T1 ≡ ∃r.{a}`, `T2 ≡ ∃r.{b}`, `C ≡ T1 ⊓ T2`, with the
/// functional / different toggles, then return whether `sat(C)` is Unsat with
/// `RUSTDL_VALUE_TYPE_DISJOINT=1`.
fn c_is_unsat(functional: bool, different: bool) -> bool {
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;

    let func_ax = if functional {
        "    FunctionalObjectProperty(:r)\n"
    } else {
        ""
    };
    let diff_ax = if different {
        "    DifferentIndividuals(:a :b)\n"
    } else {
        ""
    };
    let src = format!(
        "Prefix(:=<http://t/>)\n\
Ontology(<http://t/o>\n\
    Declaration(Class(:T1)) Declaration(Class(:T2)) Declaration(Class(:C))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
{func_ax}{diff_ax}\
    EquivalentClasses(:T1 ObjectHasValue(:r :a))\n\
    EquivalentClasses(:T2 ObjectHasValue(:r :b))\n\
    EquivalentClasses(:C ObjectIntersectionOf(:T1 :T2))\n\
)\n"
    );
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: serialized by ENV_LOCK; this is a dedicated test binary.
    unsafe {
        std::env::set_var("RUSTDL_VALUE_TYPE_DISJOINT", "1");
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let (ont, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::Cursor::new(src.into_bytes()),
        Default::default(),
    )
    .unwrap();
    let out = owl_dl_reasoner::sat_class_probe(
        &ont,
        "http://t/C",
        64,
        Some(std::time::Duration::from_secs(5)),
    )
    .expect("probe ok")
    .expect("C resolves");
    matches!(out.0, owl_dl_tableau::hyper::HyperResult::Unsat)
}

/// POSITIVE: functional `r` + `DifferentIndividuals(a,b)` ⟹ `T1⊓T2 ⊑ ⊥`, so
/// `C ≡ T1⊓T2` is unsatisfiable. Confirms the value-disjoint pair fires.
#[test]
fn value_disjoint_fires_when_functional_and_different() {
    assert!(
        c_is_unsat(true, true),
        "functional r + DifferentIndividuals(a,b): T1⊓T2 must be unsat (value-disjoint fires)"
    );
}

/// NEGATIVE (soundness): `r` NOT functional ⟹ a node may have both an `r`-succ
/// `a` and `b`, so `T1⊓T2` is SATISFIABLE. value-disjoint must NOT pair them.
#[test]
fn value_disjoint_silent_when_not_functional() {
    assert!(
        !c_is_unsat(false, true),
        "non-functional r: T1⊓T2 is SAT — value-disjoint must not produce a spurious clash (FP)"
    );
}

/// NEGATIVE (soundness): functional `r` but NO `DifferentIndividuals` ⟹ `a` and
/// `b` may denote the same individual (no UNA), so `T1⊓T2` is SATISFIABLE.
/// value-disjoint must NOT pair them.
#[test]
fn value_disjoint_silent_when_not_different() {
    assert!(
        !c_is_unsat(true, false),
        "functional r, no DifferentIndividuals: a,b may be equal ⇒ T1⊓T2 SAT — no spurious clash"
    );
}
