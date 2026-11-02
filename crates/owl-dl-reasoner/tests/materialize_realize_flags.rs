//! #391: `materialize_existential_successors_reporting` carries the flags of the
//! `realize` run it reads. The capped case is in `materialize_realize_flags_cap.rs`
//! (`RUSTDL_MAX_NODES` is cached once per process); the flags here are read per
//! call, so the tests share one process under an env mutex.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    ExistentialSuccessorFlags, materialize_existential_successors,
    materialize_existential_successors_reporting,
};
use std::io::Cursor;

/// The same fixture as `materialize_realize_flags_cap.rs`.
/// #395's fixture: every class generates an `:r`-successor, so a probe needs a
/// node beyond the root and trips a cap of 1, and `∀s.E` keeps it off the
/// saturation fast path. `C ⊑ ∃r.C` is the told existential the rows join on.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/querygiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
";

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

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

#[test]
fn uncapped_nothing_is_cut_and_the_rows_match_the_bare_form() {
    let _l = lock();
    let _off = SetEnv::new("RUSTDL_PSEUDO_MODEL", "0");
    let o = parse(SRC);
    let (rows, flags) = materialize_existential_successors_reporting(&o).unwrap();
    assert_eq!(flags, ExistentialSuccessorFlags::default());
    assert_eq!(rows, materialize_existential_successors(&o).unwrap());
    assert_eq!(rows.len(), 1);
}

#[test]
fn the_prune_is_reported_when_it_runs() {
    let _l = lock();
    let o = parse(SRC);
    let (_, flags) = materialize_existential_successors_reporting(&o).unwrap();
    assert!(
        flags.witness_prune_active,
        "default prune, ABox present: {flags:?}"
    );
    assert!(!flags.incomplete);
    let _off = SetEnv::new("RUSTDL_PSEUDO_MODEL", "0");
    let (_, flags) = materialize_existential_successors_reporting(&o).unwrap();
    assert!(!flags.witness_prune_active);
}

/// No told `X ⊑ ∃R.C`: realize never runs and no row can exist, so no flag.
#[test]
fn no_told_existential_means_no_flags() {
    let _l = lock();
    let o = parse(
        &SRC.replace("SubClassOf(:C ObjectSomeValuesFrom(:r :C))", "")
            .replace("SubClassOf(:D ObjectSomeValuesFrom(:r :D))", ""),
    );
    let (rows, flags) = materialize_existential_successors_reporting(&o).unwrap();
    assert!(rows.is_empty());
    assert_eq!(flags, ExistentialSuccessorFlags::default());
}
