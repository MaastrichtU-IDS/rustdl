//! #392: `is_consistent_with_stats` reports a `consistent` whose only evidence is
//! the consistency wedge's `Sat` as `QueryStats::relies_on_trusted_consistency`.
//! That is a trust decision, not a give-up, so `incomplete` stays clear. The
//! wedge `Sat` has been wrong before (#290, #376).
//!
//! The flag is clear whenever something else decided: the main tableau (wedge
//! off), no wedge at all (no individual named), or a witnessed clash. The #344
//! data-counting verification is pinned in `consistency_data_counting.rs`.
//!
//! Every test locks the env mutex, since one test sets `RUSTDL_WEDGE_CONSISTENCY`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{QueryStats, is_consistent_with_stats};
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Off pure EL (the union), no nominal and no data counting: the wedge decides
/// it `Sat` (#401's fixture).
const TBOX: &str = "SubClassOf(:A ObjectUnionOf(:C :D)) SubClassOf(:C :B) SubClassOf(:D :B)";
const ABOX: &str = "ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b)";

fn consistent(body: &str) -> (bool, QueryStats) {
    let ofn = format!(
        "Prefix(:=<http://ex/#>)\n\
         Ontology(<http://ex/trustedsat>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(ObjectProperty(:r))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse ofn");
    is_consistent_with_stats(&onto).expect("consistent")
}

#[test]
fn a_wedge_sat_is_reported_as_trusted() {
    let _g = lock();
    let (verdict, stats) = consistent(&format!("{TBOX}\n{ABOX}"));
    assert!(verdict);
    assert!(stats.relies_on_trusted_consistency, "{stats:?}");
    assert!(!stats.incomplete, "a trust decision, not a give-up");
}

#[test]
fn a_main_tableau_model_is_not_trusted() {
    let _g = lock();
    let _off = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let (verdict, stats) = consistent(&format!("{TBOX}\n{ABOX}"));
    assert!(verdict);
    assert!(!stats.relies_on_trusted_consistency, "{stats:?}");
    assert!(!stats.incomplete);
}

#[test]
fn no_individual_means_no_wedge_and_no_trust() {
    let _g = lock();
    let (verdict, stats) = consistent(TBOX);
    assert!(verdict);
    assert!(!stats.relies_on_trusted_consistency, "{stats:?}");
}

#[test]
fn an_inconsistency_is_never_trusted() {
    let _g = lock();
    let (verdict, stats) = consistent(&format!(
        "{TBOX}\n{ABOX}\nClassAssertion(ObjectComplementOf(:B) :a)"
    ));
    assert!(!verdict);
    assert!(!stats.relies_on_trusted_consistency, "{stats:?}");
}
