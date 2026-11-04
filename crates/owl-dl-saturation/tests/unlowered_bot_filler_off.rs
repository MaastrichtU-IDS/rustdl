//! `rhs-exists-dropped`'s `+`: with the D10 chokepoint off (`RUSTDL_EL_BOT_FILLER=0`),
//! `A ⊑ ∃r.⊥` has no lowering and no weakening (the `∃r.⊤` weakening refuses a provably
//! empty filler), so the whole fact is dropped. Its own binary: the only test here sets
//! the env, so nothing can race it.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

#[test]
#[allow(unsafe_code)]
fn rhs_exists_dropped_with_the_bot_filler_off() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has exactly one test,
    // so no other thread reads the environment concurrently.
    unsafe { std::env::set_var("RUSTDL_EL_BOT_FILLER", "0") };
    let src = "Prefix(:=<http://rustdl.test/>)\n\
               Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
               Ontology(<http://rustdl.test/o>\n\
               Declaration(Class(:A)) Declaration(ObjectProperty(:r))\n\
               SubClassOf(:A ObjectSomeValuesFrom(:r owl:Nothing))\n)";
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut Cursor::new(src), ParserConfiguration::default()).expect("ofn parses");
    let internal = convert_ontology(&onto).expect("conversion");
    let sites: Vec<&str> = owl_dl_saturation::unlowered_el_constructs(&internal)
        .into_iter()
        .map(|u| u.site)
        .collect();
    assert_eq!(sites, ["rhs-exists-dropped"]);
}
