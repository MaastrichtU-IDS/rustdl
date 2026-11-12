//! #510 (2b): the flags of the single instance query's deep retry after a depth cut, at
//! the API (the CLI's wording is pinned in `owl-dl-cli/tests/instance_depth_fallback_cli.rs`).
//!
//! The fixture is #288's 280-union (`a : Y` not entailed; `HermiT`: consistent with
//! `¬Y(a)`). Its bounded probe is depth-capped, as long as the per-pair deadline is far
//! beyond the 32 cap hits, so every test here sets a long one. Each test sets the
//! process environment, so they share one lock.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::InstanceDepthFallback;
use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn union() -> SetOntology<RcStr> {
    let mut body = String::new();
    for k in 0..280 {
        write!(body, " ObjectUnionOf(:A{k} :B{k})").unwrap();
    }
    let src = format!(
        "Prefix(:=<http://t/>)\nOntology(<http://t/x>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y))\n\
         SubClassOf(:X ObjectIntersectionOf({body}))\nClassAssertion(:X :a)\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// Runs the query with exactly these `RUSTDL_*` settings (the others cleared).
fn query(env: &[(&str, &str)]) -> (bool, owl_dl_reasoner::InstanceFlags) {
    let _lock = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let keys: Vec<std::ffi::OsString> = std::env::vars_os()
        .map(|(k, _)| k)
        .filter(|k| k.to_string_lossy().starts_with("RUSTDL_"))
        .collect();
    #[allow(unsafe_code)]
    // SAFETY: `ENV` serialises every test in this binary that touches the environment.
    unsafe {
        for k in keys {
            std::env::remove_var(k);
        }
        std::env::set_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "60000");
        for (k, v) in env {
            std::env::set_var(k, v);
        }
    }
    owl_dl_reasoner::is_instance_of_reporting(&union(), "http://t/Y", "http://t/a").unwrap()
}

#[test]
fn a_decided_retry_clears_every_cut_flag() {
    let (holds, f) = query(&[]);
    assert!(!holds);
    assert_eq!(f.depth_fallback, InstanceDepthFallback::Decided, "{f:?}");
    assert!(!f.incomplete && !f.depth_capped && !f.no_verdict, "{f:?}");
}

/// A cut retry is a budget cut: `incomplete`, and NOT `depth_capped` (whose advice is
/// that a larger budget does not help, which is false for the retry).
#[test]
fn a_cut_retry_is_incomplete_but_not_depth_capped() {
    let (holds, f) = query(&[("RUSTDL_INSTANCE_DEPTH_FALLBACK_MS", "1")]);
    assert!(!holds);
    assert_eq!(f.depth_fallback, InstanceDepthFallback::Cut, "{f:?}");
    assert!(f.incomplete && !f.depth_capped && !f.no_verdict, "{f:?}");
}

/// Off, the first probe's depth cut is reported as before (#514).
#[test]
fn with_the_retry_off_the_depth_cut_is_reported() {
    let (holds, f) = query(&[("RUSTDL_INSTANCE_DEPTH_FALLBACK_MS", "0")]);
    assert!(!holds);
    assert_eq!(f.depth_fallback, InstanceDepthFallback::NotRun, "{f:?}");
    assert!(f.incomplete && f.depth_capped && !f.no_verdict, "{f:?}");
}
