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
    // Nothing is cut. The fixture's consistency rests on the wedge's model, which
    // #429 reports beside the cut flags; it is not a cut.
    assert_eq!(
        flags,
        ExistentialSuccessorFlags {
            relies_on_trusted_consistency: flags.relies_on_trusted_consistency,
            ..ExistentialSuccessorFlags::default()
        }
    );
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

/// #387: `SRC` is Horn, so its witness is the deterministic closure; adding
/// `C ⊑ D ⊔ E` makes the witness search choose, which `witness_prune_may_drop`
/// reports on `Realization` and on the successor rows' flags. The search
/// shortcuts that could settle a disjunction without a decision are pinned off.
#[test]
fn a_branched_witness_is_reported_and_a_horn_one_is_not() {
    let _l = lock();
    let _sb = SetEnv::new("RUSTDL_SEMANTIC_BRANCHING", "0");
    let _la = SetEnv::new("RUSTDL_SAT_LOOKAHEAD", "0");
    let horn = parse(SRC);
    let union = parse(&SRC.replace(
        "ClassAssertion(:C :a)",
        "SubClassOf(:C ObjectUnionOf(:D :E)) ClassAssertion(:C :a)",
    ));

    let r = owl_dl_reasoner::realize(&horn).unwrap();
    assert!(r.witness_prune_active());
    assert!(!r.witness_prune_may_drop());
    assert!(!r.witness_prune_reasons().any());
    let (_, flags) = materialize_existential_successors_reporting(&horn).unwrap();
    assert!(flags.witness_prune_active && !flags.witness_prune_may_drop);

    let r = owl_dl_reasoner::realize(&union).unwrap();
    assert!(r.witness_prune_may_drop());
    assert_eq!(r.witness_prune_reasons().names(), ["branched"]);
    assert!(
        !r.incomplete(),
        "no probe is cut; only the prune is flagged"
    );
    let (_, flags) = materialize_existential_successors_reporting(&union).unwrap();
    assert!(flags.witness_prune_may_drop);

    // With the prune off there is no witness to drop anything.
    let _off = SetEnv::new("RUSTDL_PSEUDO_MODEL", "0");
    let r = owl_dl_reasoner::realize(&union).unwrap();
    assert!(!r.witness_prune_active() && !r.witness_prune_may_drop());
    let (_, flags) = materialize_existential_successors_reporting(&union).unwrap();
    assert!(!flags.witness_prune_may_drop);
}
