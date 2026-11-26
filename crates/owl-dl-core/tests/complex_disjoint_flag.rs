//! #600: with `RUSTDL_CLAUSIFY_COMPLEX_DISJOINT=0` the old deferrals come back
//! (attribution for the sweep), and on atomic members the flag changes nothing: the
//! clause set is identical either way.
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::clause::{clausify_with_stats, deferred_census};
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn internal(axiom: &str) -> owl_dl_core::InternalOntology {
    let src = format!(
        "Prefix(:=<http://x/>)\nOntology(\nDeclaration(Class(:A)) Declaration(Class(:B)) \
         Declaration(Class(:C)) Declaration(ObjectProperty(:r))\n{axiom}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    convert_ontology(&onto).unwrap()
}

/// Runs `f` with the flag set to `value`, restoring it after.
#[allow(unsafe_code)]
fn with_flag<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _l = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = "RUSTDL_CLAUSIFY_COMPLEX_DISJOINT";
    let prior = std::env::var_os(key);
    // SAFETY: set_var is unsafe under edition 2024; serialised via ENV_MUTEX and
    // restored below.
    unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    let out = f();
    // SAFETY: see above.
    unsafe {
        match prior {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    out
}

#[test]
fn flag_off_restores_the_deferrals() {
    for (axiom, kinds) in [
        (
            "DisjointUnion(:C :A ObjectSomeValuesFrom(:r :B))",
            vec!["disjoint-union-covering"],
        ),
        (
            "DisjointUnion(:C :A ObjectAllValuesFrom(:r :B))",
            vec!["disjoint-union-covering", "disjoint-union-member"],
        ),
        (
            "DisjointClasses(:A ObjectAllValuesFrom(:r :B))",
            vec!["disjoint-antecedent"],
        ),
    ] {
        let o = internal(axiom);
        let off: Vec<&str> = with_flag(Some("0"), || deferred_census(&o))
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(off, kinds, "{axiom}: flag off");
        assert!(
            with_flag(None, || deferred_census(&o)).is_empty(),
            "{axiom}: flag on"
        );
    }
}

#[test]
fn atomic_members_clausify_identically_either_way() {
    for axiom in ["DisjointUnion(:C :A :B)", "DisjointClasses(:A :B :C)"] {
        let o = internal(axiom);
        let on = with_flag(None, || clausify_with_stats(&o).0);
        let off = with_flag(Some("0"), || clausify_with_stats(&o).0);
        assert_eq!(on, off, "{axiom}");
    }
}
