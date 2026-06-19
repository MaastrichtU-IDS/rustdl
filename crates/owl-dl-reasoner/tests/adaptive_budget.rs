//! Verdict-preservation test for the `RUSTDL_ADAPTIVE_BUDGET` flag (Lever #1).
//!
//! The adaptive-budget predicate fires only on diverging searches (depth-saturated,
//! high restore ratio, and growing model). On non-diverging workloads it is a no-op.
//! This test verifies that flag-ON produces the SAME subsumption closure as flag-OFF
//! on a small non-diverging EL ontology (A ⊑ B ⊑ C ⊨ A ⊑ C).
//!
//! The *diverging-case* behavior (early-cut of stalling searches) is validated by
//! the corpus gate in Task 3 (ore-15672 + full closure-diff).
//!
//! Run: `cargo test -p owl-dl-reasoner --test adaptive_budget`

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
use std::io::Cursor;

// Env-mutation plumbing: serialize RUSTDL_ADAPTIVE_BUDGET against other
// env-mutating tests, restore on Drop. Mirrors the pattern in
// `classify_inverse_domain.rs`.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one
        // test, serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }

    #[allow(unsafe_code)]
    fn remove(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: remove_var is unsafe under edition 2024. Held only for one
        // test, serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Helper: returns true iff the classification records `sub_iri ⊑ sup_iri`.
fn has_sub(c: &Classification, sub_iri: &str, sup_iri: &str) -> bool {
    c.is_subclass(sub_iri, sup_iri)
}

/// Small transitivity ontology: A ⊑ B, B ⊑ C ⊨ A ⊑ C.
const TRANSITIVE_SRC: &str = r"Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:A :B) SubClassOf(:B :C)
)
";

/// Collect the set of all entailed non-reflexive subsumption pairs from
/// a Classification for comparison (returned as a sorted Vec of (sub,sup) pairs).
fn pair_set(c: &Classification) -> Vec<(String, String)> {
    let classes = c.classes();
    let mut pairs = Vec::new();
    for sub in classes {
        for sup in classes {
            if sub != sup && has_sub(c, sub, sup) {
                pairs.push((sub.clone(), sup.clone()));
            }
        }
    }
    pairs.sort();
    pairs
}

/// With `RUSTDL_ADAPTIVE_BUDGET=1`, classify of a small EL ontology must produce
/// the SAME subsumption closure as the default (flag-OFF) path.
///
/// This confirms that the adaptive-budget gate is correctly guarded (default OFF)
/// and that when ON it does not disturb verdict-stable (non-diverging) searches.
#[test]
fn adaptive_budget_preserves_verdicts_small() {
    // --- baseline: classify with flag absent (default OFF) ---
    let pairs_off = {
        let _serial = ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _flag = SetEnvGuard::remove("RUSTDL_ADAPTIVE_BUDGET");
        let mut r = Cursor::new(TRANSITIVE_SRC);
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
        let c = owl_dl_reasoner::classify(&onto).expect("classify off");
        pair_set(&c)
    };

    // --- flag-ON: must produce the identical closure ---
    let pairs_on = {
        let _serial = ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _flag = SetEnvGuard::set("RUSTDL_ADAPTIVE_BUDGET", "1");
        let mut r = Cursor::new(TRANSITIVE_SRC);
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
        let c = owl_dl_reasoner::classify(&onto).expect("classify on");
        pair_set(&c)
    };

    // The key entailment must be present in both.
    assert!(
        pairs_off.contains(&("http://e#A".to_owned(), "http://e#C".to_owned())),
        "flag-OFF: classify must report A ⊑ C (transitive chain)"
    );
    assert!(
        pairs_on.contains(&("http://e#A".to_owned(), "http://e#C".to_owned())),
        "flag-ON: classify must report A ⊑ C (transitive chain)"
    );

    // The full closures must match (flag-ON must not produce spurious or missing pairs).
    assert_eq!(
        pairs_off, pairs_on,
        "RUSTDL_ADAPTIVE_BUDGET=1 must not change the subsumption closure on a \
         non-diverging EL ontology (pairs differ)"
    );
}

/// Documents default-OFF semantics: with the flag absent, `adaptive_budget_enabled()`
/// returns false — no `with_adaptive_budget()` call is made, so behavior is identical
/// to the pre-Lever-#1 baseline. This test exercises the same small ontology without
/// any env mutation, confirming the default path is clean.
#[test]
fn adaptive_budget_default_off_classifies_correctly() {
    // No env guard: test runs in whatever ambient env the harness provides.
    // The flag defaults to OFF (`map_or(false, ...)`), so this is the standard path.
    let mut r = Cursor::new(TRANSITIVE_SRC);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        has_sub(&c, "http://e#A", "http://e#C"),
        "default classify must report A ⊑ C (transitive chain)"
    );
    assert!(
        has_sub(&c, "http://e#A", "http://e#B"),
        "default classify must report A ⊑ B"
    );
    assert!(
        has_sub(&c, "http://e#B", "http://e#C"),
        "default classify must report B ⊑ C"
    );
    // Sound: no spurious upward entailment (C is not a subclass of A or B).
    assert!(
        !has_sub(&c, "http://e#C", "http://e#A"),
        "C ⊑ A must not be reported (no converse entailment)"
    );
}
