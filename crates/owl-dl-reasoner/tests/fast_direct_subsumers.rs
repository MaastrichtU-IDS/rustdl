//! Canaries for the hoisted Hasse reduction (`RUSTDL_FAST_DIRECT_SUBSUMERS`,
//! default OFF).
//!
//! The lever replaces `Classification::direct_subsumers`'s per-call `O(k²)`
//! transitive reduction (and its `O(n²)` rescan per UNSATISFIABLE subject) with a
//! `OnceLock` index built once. It is a pure re-association: the output must be
//! identical as an ORDERED sequence, for every declared class, on both the
//! satisfiable and the unsatisfiable arm.
//!
//! **Negatives first.** These tests are written so that they fail if the fast path
//! (a) returns a super-set (skips the Hasse prune), (b) returns a sub-set (over-prunes),
//! (c) mishandles equivalence (a `⊑`-cycle member is NOT a direct super), (d) mishandles
//! the elided unsatisfiable row, or (e) reorders the output. Each is checked as
//! `assert_eq!` on `Vec<&str>`, so ordering is load-bearing, not incidental.
//!
//! Sabotage record (2026-08-01) — each mutation applied to
//! `Classification::direct_subsumers_fast` / `build_direct_index`, suite run, reverted:
//! * skip the Hasse prune (return `S(i)` unfiltered): `hasse_prune_chain`,
//!   `diamond_lattice_prune`, `slow_and_fast_agree_on_every_class` FAIL.
//! * drop the `!self.entails(j, i)` strictness filter in `build_direct_index`:
//!   `equivalent_classes_are_not_direct_supers`, `slow_and_fast_agree_on_every_class` FAIL.
//! * `minimal_sat` → every satisfiable class (skip the minimality filter):
//!   `unsatisfiable_subject_gets_minimal_classes`, `slow_and_fast_agree_on_every_class` FAIL.
//! * emit `minimal_sat` / the filtered supers in DESCENDING order:
//!   `unsatisfiable_subject_gets_minimal_classes` and
//!   `slow_and_fast_agree_on_every_class` FAIL (ordered comparison).

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

/// Serialize the env mutation against other tests in this binary.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held for one test only,
        // serialized via ENV_MUTEX, restored on Drop.
        unsafe { std::env::set_var(key, value) };
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

/// The full risky shape set in ONE ontology, so `slow_and_fast_agree_on_every_class`
/// exercises all of it at once:
/// * `A ⊑ B ⊑ C` — the Hasse prune (`C` is indirect for `A`);
/// * `D ⊑ B`, `D ⊑ E`, `B ⊑ C`, `E ⊑ C` — a diamond (two direct supers for `D`);
/// * `E1 ≡ E2` — a `⊑`-cycle (neither is a *strict* super of the other);
/// * `U ⊑ ⊥` — an unsatisfiable class (its row is ELIDED, so `direct_subsumers(U)`
///   takes the degenerate `0..n` arm);
/// * `V ⊑ ⊥` — a SECOND unsatisfiable class, so the "compute `maximal_sat` once and
///   share it" claim is exercised rather than trivially true;
/// * `Z` — a top class with no supers at all, and `Y ⊑ Z` (so `Z` is NOT minimal).
///
/// Local names are chosen so that declaration order and lexical order differ from
/// the subsumption order — an ordering bug cannot hide behind an accidental match.
const FIXTURE: &str = "Prefix(:=<http://e#>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
Ontology(\n\
 Declaration(Class(:U)) Declaration(Class(:V))\n\
 Declaration(Class(:C)) Declaration(Class(:B)) Declaration(Class(:A))\n\
 Declaration(Class(:D)) Declaration(Class(:E))\n\
 Declaration(Class(:E1)) Declaration(Class(:E2))\n\
 Declaration(Class(:Y)) Declaration(Class(:Z))\n\
 SubClassOf(:U owl:Nothing) SubClassOf(:V owl:Nothing)\n\
 SubClassOf(:A :B) SubClassOf(:B :C)\n\
 SubClassOf(:D :B) SubClassOf(:D :E) SubClassOf(:E :C)\n\
 EquivalentClasses(:E1 :E2)\n\
 SubClassOf(:Y :Z)\n\
)\n";

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(src.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

fn iri(local: &str) -> String {
    format!("http://e#{local}")
}

/// `direct_subsumers(c)` under the flag value `on`, as owned strings.
fn directs(h: &owl_dl_reasoner::Classification, c: &str, on: bool) -> Vec<String> {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_FAST_DIRECT_SUBSUMERS", if on { "1" } else { "0" });
    h.direct_subsumers(&iri(c))
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// THE differential gate: for EVERY declared class, slow and fast must agree as an
/// ordered sequence. Both are read off the SAME `Classification`, so nothing but the
/// reduction algorithm can differ.
#[test]
fn slow_and_fast_agree_on_every_class() {
    let h = classify(FIXTURE);
    let mut saw_nonempty = 0usize;
    for c in h.classes().to_vec() {
        let slow = {
            let _lock = ENV_MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _g = SetEnvGuard::set("RUSTDL_FAST_DIRECT_SUBSUMERS", "0");
            h.direct_subsumers(&c)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let fast = {
            let _lock = ENV_MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _g = SetEnvGuard::set("RUSTDL_FAST_DIRECT_SUBSUMERS", "1");
            h.direct_subsumers(&c)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            slow, fast,
            "direct_subsumers({c}) diverged (ORDERED compare)"
        );
        if !slow.is_empty() {
            saw_nonempty += 1;
        }
    }
    // Guard against the whole fixture degenerating to empty answers, which would
    // make the identity above vacuous.
    assert!(
        saw_nonempty >= 6,
        "fixture produced only {saw_nonempty} non-empty direct-super sets — too weak to be evidence"
    );
}

/// Hasse prune: `A ⊑ B ⊑ C` ⟹ `direct(A) = [B]`, NOT `[B, C]`.
#[test]
fn hasse_prune_chain() {
    let h = classify(FIXTURE);
    for on in [false, true] {
        assert_eq!(directs(&h, "A", on), vec![iri("B")], "flag on={on}");
    }
}

/// Diamond: `D ⊑ B`, `D ⊑ E`, both `⊑ C` ⟹ `direct(D) = [B, E]` (ascending by
/// class index — `C` declared before `B` before `A` before `D` before `E`), and `C`
/// is pruned.
#[test]
fn diamond_lattice_prune() {
    let h = classify(FIXTURE);
    for on in [false, true] {
        let d = directs(&h, "D", on);
        assert!(
            !d.contains(&iri("C")),
            "C is indirect for D but was emitted (flag on={on})"
        );
        assert_eq!(d.len(), 2, "D has exactly two direct supers (flag on={on})");
        assert!(
            d.contains(&iri("B")) && d.contains(&iri("E")),
            "flag on={on}"
        );
    }
}

/// An equivalent class is NOT a strict super, so it must not appear.
#[test]
fn equivalent_classes_are_not_direct_supers() {
    let h = classify(FIXTURE);
    for on in [false, true] {
        assert!(
            !directs(&h, "E1", on).contains(&iri("E2")),
            "E2 ≡ E1 must not be reported as a direct SUPER of E1 (flag on={on})"
        );
    }
}

/// The elided-row arm. An unsatisfiable subject is subsumed by every satisfiable
/// class, and `⊥` sits at the BOTTOM of the hierarchy, so its Hasse-direct supers
/// are the MINIMAL satisfiable classes — here `A`, `D`, the `E1 ≡ E2` pair and `Y`.
/// A class with a strict subclass (`B`, `C`, `E`, `Z`) must be pruned. Both
/// unsatisfiable classes must get the identical answer, ascending.
#[test]
fn unsatisfiable_subject_gets_minimal_classes() {
    let h = classify(FIXTURE);
    assert!(
        h.unsatisfiable_classes().contains(&iri("U").as_str()),
        "fixture must actually make U unsatisfiable"
    );
    let want: Vec<String> = ["A", "D", "E1", "E2", "Y"].iter().map(|c| iri(c)).collect();
    for on in [false, true] {
        let u = directs(&h, "U", on);
        let v = directs(&h, "V", on);
        assert_eq!(u, v, "the two unsatisfiable classes must agree (on={on})");
        // Exact, ORDERED — pins membership (no over/under-pruning) and order at once.
        assert_eq!(u, want, "unsatisfiable subject's direct supers (on={on})");
    }
}

/// Unknown IRI ⟹ empty, on both paths (the early return must survive the split).
#[test]
fn undeclared_class_is_empty_on_both_paths() {
    let h = classify(FIXTURE);
    for on in [false, true] {
        assert!(directs(&h, "NoSuchClass", on).is_empty(), "flag on={on}");
    }
}
