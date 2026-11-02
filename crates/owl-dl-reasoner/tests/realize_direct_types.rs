//! #388: `realize`'s most-specific types are read off a classification that can
//! cut a pair or trust a wedge `Sat`, so a missed `X ⊑ Y` would leave `Y` listed
//! beside `X`. `Realization::direct_types_unverified` reports a co-listed pair whose
//! subsumption is undecided; on the tableau path the pair is first checked with the
//! main tableau (`RUSTDL_REALIZE_VERIFY_DIRECT`, default ON).
//!
//! The field is distinct from `incomplete` (a type may be missing): these tests
//! check it moves while `incomplete` does not.
//!
//! No natural ontology was found where the hierarchy misses a subsumption between
//! two types of one individual (28 small ORE ontologies checked by hand, and the
//! 142 ORE ontologies where the field fires before verification). The drop rule is
//! therefore pinned by unit tests in `realize.rs`; the cut path is pinned in
//! `realize_direct_types_cap.rs`, in its own process because the node cap is
//! cached once per process.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets an env var for the guard's lifetime and restores it on drop.
struct SetEnv(&'static str, Option<std::ffi::OsString>);

impl SetEnv {
    #[allow(unsafe_code)]
    fn new(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: serialized by ENV_MUTEX, restored on drop.
        unsafe { std::env::set_var(key, value) };
        Self(key, prior)
    }
}

impl Drop for SetEnv {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `new`.
        unsafe {
            match &self.1 {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Ontology(<http://ex.org/o>\n\
         Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a))\n\
         {body}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// `∀` keeps the `TBox` off the saturation fast path, so realize takes the tableau.
const OFF_FRAGMENT: &str = "SubClassOf(:C ObjectAllValuesFrom(:r :D))";
const TWO_TYPES: &str = "ClassAssertion(:X :a)\nClassAssertion(:Y :a)";

fn direct(r: &owl_dl_reasoner::Realization) -> Vec<String> {
    let mut v = r.most_specific_types("http://ex.org/a").to_vec();
    v.sort();
    v
}

#[test]
fn two_unrelated_types_are_checked_and_refuted() {
    let _l = lock();
    let r = owl_dl_reasoner::realize(&parse(&format!("{OFF_FRAGMENT}\n{TWO_TYPES}"))).unwrap();
    assert_eq!(direct(&r), ["http://ex.org/X", "http://ex.org/Y"]);
    assert!(
        !r.direct_types_unverified(),
        "X ⊑ Y and Y ⊑ X are both refuted, so the pair is decided"
    );
    assert!(!r.incomplete());
}

#[test]
fn with_the_check_off_an_undecided_pair_is_reported() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_REALIZE_VERIFY_DIRECT", "0");
    let r = owl_dl_reasoner::realize(&parse(&format!("{OFF_FRAGMENT}\n{TWO_TYPES}"))).unwrap();
    assert_eq!(direct(&r), ["http://ex.org/X", "http://ex.org/Y"]);
    assert!(
        r.direct_types_unverified(),
        "the hierarchy's `no` is not trusted off the EL fragment"
    );
    assert!(!r.incomplete(), "no instance probe was cut");
}

#[test]
fn a_told_subclass_is_not_most_specific_and_needs_no_check() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_REALIZE_VERIFY_DIRECT", "0");
    let r = owl_dl_reasoner::realize(&parse(&format!(
        "{OFF_FRAGMENT}\nSubClassOf(:X :Y)\n{TWO_TYPES}"
    )))
    .unwrap();
    assert_eq!(direct(&r), ["http://ex.org/X"]);
    assert!(!r.direct_types_unverified());
}

#[test]
fn equivalent_types_are_both_kept_and_decided() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_REALIZE_VERIFY_DIRECT", "0");
    let r = owl_dl_reasoner::realize(&parse(&format!(
        "{OFF_FRAGMENT}\nEquivalentClasses(:X :Y)\n{TWO_TYPES}"
    )))
    .unwrap();
    assert_eq!(direct(&r), ["http://ex.org/X", "http://ex.org/Y"]);
    assert!(
        !r.direct_types_unverified(),
        "a pair the hierarchy proves equivalent is decided"
    );
}

/// The saturation fast path takes types and hierarchy from one closure on a
/// fragment where it is complete: never reported.
#[test]
fn the_saturation_fast_path_never_reports() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_REALIZE_VERIFY_DIRECT", "0");
    let r = owl_dl_reasoner::realize(&parse(TWO_TYPES)).unwrap();
    assert_eq!(direct(&r), ["http://ex.org/X", "http://ex.org/Y"]);
    assert!(!r.direct_types_unverified());
}

/// `realize_saturation_only` runs no tableau by contract, so an undecided pair
/// stays undecided on any input, with the check on.
#[test]
fn saturation_only_reports_an_undecided_pair() {
    let _l = lock();
    let r =
        owl_dl_reasoner::realize_saturation_only(&parse(&format!("{OFF_FRAGMENT}\n{TWO_TYPES}")))
            .unwrap();
    assert_eq!(direct(&r), ["http://ex.org/X", "http://ex.org/Y"]);
    assert!(r.direct_types_unverified());
}
