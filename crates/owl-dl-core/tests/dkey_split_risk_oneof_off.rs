//! #589: the split-risk detector sees enumeration keys even with
//! `RUSTDL_DKEY_ONEOF_SEED=0`, which leaves the seeding buckets empty (Fable review). Its
//! own test binary, because it sets that variable for the process.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

#[test]
fn enumeration_keys_count_with_the_oneof_seed_off() {
    // SAFETY: the only test in this binary, so nothing reads the variable concurrently.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_DKEY_ONEOF_SEED", "0");
    }
    let src = "Prefix(:=<http://ex.org/t#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\nDeclaration(DataProperty(:d))\n\
         Declaration(Class(:C)) Declaration(Class(:E))\n\
         SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubClassOf(DataHasValue(:d \"1\"^^xsd:integer) :E)\n\
         SubClassOf(DataHasValue(:d \"2\"^^xsd:integer) :E)\n)\n";
    let o: SetOntology<RcStr> = read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0;
    let internal = owl_dl_core::convert::convert_ontology(&o).unwrap();
    assert!(owl_dl_core::convert::data_case_split_risk(&internal));
}
