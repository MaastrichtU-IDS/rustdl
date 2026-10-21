//! Issue #288 — `sat PMD_0010100` stopped terminating on a 10-axiom PMD core
//! that v0.4.40 answered in 1.2 s. `HermiT` calls the class satisfiable.
//!
//! Two defects in `apply_max`, both needed to recover it:
//!
//! - **Merge direction** (`RUSTDL_MAX_MERGE_KEEP_OLDER`). `RO_0000052` is
//!   functional and reached through the inverse of `RO_0000086`, so the forced
//!   `≤1` merge pairs a node's predecessor with a fresh successor. Neighbour
//!   order lists successors first, and the predecessor was merged INTO the
//!   successor. Re-parenting then made a node a root, roots are never blocked,
//!   and the graph grew without end.
//! - **Deps of the `⊥` added when every trial merge is rejected**
//!   (`RUSTDL_MAX_PRECISE_BOT`). It carried every active decision, so no
//!   back-jump could pass it and the search thrashed (~22k clashes per 5 s).
//!
//! Which defect bites depends on rule order: at the default (`∀` first) only
//! the deps matter; with `RUSTDL_FORALL_FIRST=0` only the merge direction does.
//! So each test below guards one flag. Sabotage, run by hand:
//! `RUSTDL_MAX_PRECISE_BOT=0` fails `core_is_satisfiable`, and
//! `RUSTDL_MAX_MERGE_KEEP_OLDER=0` fails `core_is_satisfiable_without_forall_first`.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_class_satisfiable;

/// The fixed path answers in ~0.04 s; the broken one never does.
const BOUND: Duration = Duration::from_secs(60);

/// The 10-axiom core from the #288 investigation, with the original IRIs.
/// They matter: rule order follows class and role ids, and with IRIs shortened
/// to `:P10100` etc. the keep-older sabotage below no longer stalls.
const CORE: &str = r"Prefix(obo:=<http://purl.obolibrary.org/obo/>)
Prefix(pmd:=<https://w3id.org/pmd/co/>)
Ontology(
SubClassOf(pmd:PMD_0090002 pmd:PMD_0023004)
SubClassOf(pmd:PMD_0090000 ObjectSomeValuesFrom(obo:RO_0000086 pmd:PMD_0090002))
SubClassOf(pmd:PMD_0010100 ObjectSomeValuesFrom(obo:RO_0002351 ObjectIntersectionOf(pmd:PMD_0020140 ObjectSomeValuesFrom(obo:RO_0000091 pmd:PMD_0090001))))
EquivalentClasses(pmd:PMD_0023004 ObjectUnionOf(ObjectSomeValuesFrom(obo:RO_0000052 pmd:PMD_0000001) ObjectSomeValuesFrom(pmd:PMD_0025999 pmd:PMD_0000001)))
EquivalentClasses(pmd:PMD_0090000 ObjectSomeValuesFrom(obo:RO_0000091 pmd:PMD_0090001))
SubObjectPropertyOf(obo:RO_0000080 obo:RO_0000052)
SubObjectPropertyOf(obo:RO_0000092 obo:RO_0000052)
InverseObjectProperties(obo:RO_0000080 obo:RO_0000086)
InverseObjectProperties(obo:RO_0000091 obo:RO_0000092)
FunctionalObjectProperty(obo:RO_0000052)
)";

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prev = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Every test in this file
        // holds ENV_MUTEX for its whole run.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as in `set`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Fail rather than hang if `PMD_0010100`'s satisfiability is not decided in time.
fn pmd_0010100_satisfiable(what: &str) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let o: SetOntology<RcStr> = read_ofn(
            &mut Cursor::new(CORE.to_owned()),
            ParserConfiguration::default(),
        )
        .unwrap()
        .0;
        let _ = tx.send(is_class_satisfiable(&o, "https://w3id.org/pmd/co/PMD_0010100").unwrap());
    });
    rx.recv_timeout(BOUND).unwrap_or_else(|e| {
        panic!("{what} did not finish within {BOUND:?} ({e}) — issue #288 has regressed")
    })
}

/// Default rule order. Guards `RUSTDL_MAX_PRECISE_BOT`.
#[test]
fn core_is_satisfiable() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(pmd_0010100_satisfiable("sat PMD_0010100"));
}

/// `∀` not applied first. Guards `RUSTDL_MAX_MERGE_KEEP_OLDER`.
#[test]
fn core_is_satisfiable_without_forall_first() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ff = EnvGuard::set("RUSTDL_FORALL_FIRST", "0");
    assert!(pmd_0010100_satisfiable(
        "sat PMD_0010100 [RUSTDL_FORALL_FIRST=0]"
    ));
}
