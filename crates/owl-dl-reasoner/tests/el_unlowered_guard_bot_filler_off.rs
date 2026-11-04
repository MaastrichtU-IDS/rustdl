//! Layer 2 end to end: a certified drop IS reported, through the certification filter.
//!
//! Until #445 the live `⊤ ⊑ ∃r.B` drop pinned this. With it fixed no EL shape on main
//! drops anything, so the drop is made here: with the D10 chokepoint off
//! (`RUSTDL_EL_BOT_FILLER=0`), `A ⊑ ∃r.⊥` has no lowering, while the gate still
//! certifies it. Its own binary: the only test here sets the env, so nothing can race it.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

#[test]
#[allow(unsafe_code)]
fn a_certified_drop_is_reported() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has exactly one test,
    // so no other thread reads the environment concurrently.
    unsafe { std::env::set_var("RUSTDL_EL_BOT_FILLER", "0") };
    let src = "Prefix(:=<http://rustdl.test/>)\n\
               Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
               Ontology(<http://rustdl.test/o>\n\
               Declaration(Class(:A)) Declaration(ObjectProperty(:r))\n\
               SubClassOf(:A ObjectSomeValuesFrom(:r owl:Nothing))\n)";
    let onto: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("ofn parses")
        .0;
    let found = owl_dl_reasoner::certified_unlowered(&onto).expect("converts");
    assert!(found.classify_certified, "{found:?}");
    let sites: Vec<_> = found
        .classify
        .iter()
        .map(|u| (u.site, u.concept.as_str()))
        .collect();
    assert_eq!(sites, [("rhs-exists-dropped", "∃r.⊥")], "{found:?}");
}
