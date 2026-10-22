//! #315 `RUSTDL_DKEY_FAMILY_DISJOINT` (default ON): the numeric datatypes, `xsd:string`,
//! `rdf:langString` and `xsd:dateTime` have pairwise disjoint value spaces, so a value or
//! restriction of one family on `p` clashes with a range or `∀p` of another.
//!
//! `DataPropertyRange(p, xsd:integer)` with `C ⊑ ∃p.xsd:string` made `C` satisfiable,
//! reported with `completeness_guaranteed: true`; only IEEE-vs-exact numeric pairs were
//! ever seeded. `HermiT` agrees with every verdict below (2026-10-22).
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_family_disjointness`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Every test locks, including the
// read-only ones, so none observes another test's setting.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://example.org/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)\n\
         Ontology(<http://example.org/onto>\n    Declaration(DataProperty(:p)) Declaration(DataProperty(:q)) Declaration(Class(:C))\n\
         {axioms}\n)\n"
    );
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `C` satisfiable, with the flag set to `flag`.
fn c_sat(axioms: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_DKEY_FAMILY_DISJOINT", flag);
    owl_dl_reasoner::is_class_satisfiable(&parse(axioms), "http://example.org/C").expect("sat")
}

fn unsat_only_by_the_rule(axioms: &str) {
    assert!(!c_sat(axioms, "1"), "C must be unsatisfiable: {axioms}");
    assert!(
        c_sat(axioms, "0"),
        "control: without the rule it is missed: {axioms}"
    );
}

#[test]
fn an_integer_range_clashes_with_a_string_value() {
    unsat_only_by_the_rule(
        "    DataPropertyRange(:p xsd:integer) SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
    );
}

#[test]
fn a_string_range_clashes_with_an_integer_value() {
    unsat_only_by_the_rule(
        "    DataPropertyRange(:p xsd:string) SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))",
    );
}

#[test]
fn an_integer_range_clashes_with_a_datetime_value() {
    unsat_only_by_the_rule(
        "    DataPropertyRange(:p xsd:integer) SubClassOf(:C DataSomeValuesFrom(:p xsd:dateTime))",
    );
}

#[test]
fn a_universal_clashes_like_a_range() {
    unsat_only_by_the_rule(
        "    SubClassOf(:C DataAllValuesFrom(:p xsd:integer))
    SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
    );
}

#[test]
fn literal_values_and_enumerations_clash_with_another_family() {
    for axioms in [
        r#"    DataPropertyRange(:p xsd:string) SubClassOf(:C DataHasValue(:p "5"^^xsd:integer))"#,
        r#"    DataPropertyRange(:p xsd:string)
    SubClassOf(:C DataSomeValuesFrom(:p DataOneOf("5"^^xsd:integer "6"^^xsd:integer)))"#,
        r#"    DataPropertyRange(:p xsd:string) SubClassOf(:C DataHasValue(:p "hi"@en))"#,
        r#"    DataPropertyRange(:p xsd:string) SubClassOf(:C DataSomeValuesFrom(:p
        DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer)))"#,
    ] {
        unsat_only_by_the_rule(axioms);
    }
}

/// FP guards: same family, or `xsd:integer` inside `xsd:decimal`, or two values that
/// need not be the same one.
#[test]
fn values_of_a_compatible_datatype_do_not_clash() {
    for axioms in [
        "    DataPropertyRange(:p xsd:decimal) SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))",
        "    DataPropertyRange(:p xsd:string) SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
        r#"    DataPropertyRange(:p xsd:decimal) SubClassOf(:C DataHasValue(:p "5"^^xsd:integer))"#,
        "    DataPropertyRange(:p xsd:dateTime) SubClassOf(:C DataSomeValuesFrom(:p xsd:dateTime))",
        "    SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))
    SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
    ] {
        assert!(c_sat(axioms, "1"), "C must stay satisfiable: {axioms}");
    }
}

/// FP guards for family placement: a complement is not its datatype's family, and
/// values on two DIFFERENT properties never meet. All satisfiable per `HermiT`.
/// (`rdf:PlainLiteral` is not a guard: it is dropped as unsupported, and `HermiT` calls
/// a `PlainLiteral` range against an integer value unsatisfiable, so that is a miss.)
#[test]
fn complements_other_ranges_and_other_properties_do_not_clash() {
    for axioms in [
        "    DataPropertyRange(:p DataComplementOf(xsd:integer))
    SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
        "    DataPropertyRange(:p xsd:integer)
    SubClassOf(:C DataSomeValuesFrom(:p DataComplementOf(xsd:string)))",
        "    SubClassOf(:C DataAllValuesFrom(:p DataComplementOf(xsd:integer)))
    SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
        "    DataPropertyRange(:p xsd:integer) SubClassOf(:C DataSomeValuesFrom(:q xsd:string))",
    ] {
        assert!(c_sat(axioms, "1"), "C must stay satisfiable: {axioms}");
    }
}

/// CONTESTED, so kept satisfiable: with a bare `rdf:langString` range, `HermiT` and Konclude
/// both answer satisfiable for a plain string value (they read `rdf:langString`
/// OWL-2-style, as containing plain strings). The bare-range key is kept out of the
/// language-string family for that reason. The opposite direction (a string range
/// against a language-tagged value) is uncontested and clashes, above.
#[test]
fn a_langstring_range_with_a_plain_string_value_stays_satisfiable() {
    for axioms in [
        r#"    DataPropertyRange(:p rdf:langString) SubClassOf(:C DataHasValue(:p "x"))"#,
        "    DataPropertyRange(:p rdf:langString) SubClassOf(:C DataSomeValuesFrom(:p xsd:string))",
    ] {
        assert!(c_sat(axioms, "1"), "{axioms}");
    }
}
