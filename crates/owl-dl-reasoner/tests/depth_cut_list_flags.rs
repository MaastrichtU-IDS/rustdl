//! #510 follow-up: `Realization::depth_capped`, `CeInstances::depth_capped` and
//! `ExistentialSuccessorFlags::depth_capped` say when a cut instance probe hit the
//! tableau's decision-depth limit, which a larger budget does not recover.
//!
//! The fixture is `X ⊑ ⊓_{k<280} (∀r_k.A ⊔ ∀s_k.A)` with `a : X`. Its named classes are
//! only {X, Y, A} (plus `Z` with a told `Z ⊑ ∃p.A`, so that
//! `materialize_existential_successors` runs its `realize`). The ∀-disjuncts make no
//! successors, so each union is one decision, and a probe `{a} ⊓ ¬C` for C ∈ {Y, A, Z}
//! needs 280 levels, past `MAX_SEARCH_DEPTH` (256). Early abandon gives up after 32
//! cap hits, well before a 60 s deadline. #514's fixture (280 unions of fresh named
//! classes) has 562 classes, and `realize` runs on it for minutes in debug.
//!
//! The per-pair deadline is read per call, so the tests share one process under an env
//! mutex.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, ClassExpression, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{class_expression_instances, materialize_existential_successors_reporting};
use std::fmt::Write as _;
use std::io::Cursor;

/// A per-pair deadline far beyond the probes' run time, so a cut cannot be the deadline
/// even on a loaded host (#514's review).
const LONG_DEADLINE_MS: &str = "60000";

const X: &str = "http://example.org/d#X";
const Y: &str = "http://example.org/d#Y";
const A: &str = "http://example.org/d#a";

fn fixture(told_exists: bool) -> SetOntology<RcStr> {
    let mut src = String::from(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:A))\n\
         SubClassOf(:X ObjectIntersectionOf(",
    );
    for k in 0..280 {
        write!(
            src,
            " ObjectUnionOf(ObjectAllValuesFrom(:r{k} :A) ObjectAllValuesFrom(:s{k} :A))"
        )
        .unwrap();
    }
    src.push_str("))\n");
    if told_exists {
        src.push_str("Declaration(Class(:Z)) SubClassOf(:Z ObjectSomeValuesFrom(:p :A))\n");
    }
    src.push_str("ClassAssertion(:X :a)\n)\n");
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

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

#[test]
fn realize_reports_a_depth_cut_and_the_deadline_free_probe_recovers_it() {
    let _l = lock();
    let o = fixture(false);
    let r = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS);
        owl_dl_reasoner::realize(&o).unwrap()
    };
    assert!(r.depth_capped(), "a depth cut, not a deadline: {r:?}");
    assert!(r.incomplete(), "depth_capped implies incomplete: {r:?}");
    assert!(!r.no_verdict(), "{r:?}");
    assert_eq!(r.entailed_types(A), [X], "the sound answer is unchanged");

    let r = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0");
        owl_dl_reasoner::realize(&o).unwrap()
    };
    assert!(!r.depth_capped() && !r.incomplete(), "{r:?}");
    assert_eq!(r.entailed_types(A), [X]);
}

#[test]
fn instances_expr_reports_a_depth_cut_and_the_deadline_free_probe_recovers_it() {
    let _l = lock();
    let o = fixture(false);
    let build: Build<RcStr> = Build::new();
    let ce = ClassExpression::Class(build.class(Y));
    let r = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS);
        class_expression_instances(&o, &ce).unwrap()
    };
    assert!(r.depth_capped() && r.incomplete(), "{r:?}");
    assert!(!r.no_verdict() && r.individuals().is_empty(), "{r:?}");

    let r = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0");
        class_expression_instances(&o, &ce).unwrap()
    };
    assert!(!r.depth_capped() && !r.incomplete(), "{r:?}");
    assert!(r.individuals().is_empty(), "{r:?}");
}

#[test]
fn existential_successors_carry_realize_s_depth_cut() {
    let _l = lock();
    let o = fixture(true);
    let (_, flags) = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS);
        materialize_existential_successors_reporting(&o).unwrap()
    };
    assert!(flags.depth_capped && flags.incomplete, "{flags:?}");
    assert!(!flags.no_verdict, "{flags:?}");

    let (_, flags) = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0");
        materialize_existential_successors_reporting(&o).unwrap()
    };
    assert!(!flags.depth_capped && !flags.incomplete, "{flags:?}");

    // Without a told existential no `realize` runs, so nothing is cut: the flags of
    // the depth-cut fixture above are not this early return's defaults.
    let (rows, flags) = {
        let _d = SetEnv::new("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS);
        materialize_existential_successors_reporting(&fixture(false)).unwrap()
    };
    assert!(
        rows.is_empty() && !flags.depth_capped && !flags.incomplete,
        "{flags:?}"
    );
}
