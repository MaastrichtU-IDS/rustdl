//! #589: a data key that may need reasoning by cases voids the completeness guarantee.
//!
//! Data ranges lower to atomic `DKey` classes, which every engine treats as atoms, so an
//! entailment that needs the value-level split is missed while the pure-EL gate admits
//! the ontology. Each positive below read `completeness_guaranteed: true` with the
//! entailment missed before #589 (`HermiT` derives every one; the fixtures are the
//! #589 issue's and its plan review's probes, `~/rustdl/sweeps/wB/{556,589}/fable/`).
//! The flag never changes an answer, so only the signal is pinned, not the miss.
//! Negatives first: shapes that need no case split keep the guarantee.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn flagged(src: &str) -> (bool, bool) {
    let c = owl_dl_reasoner::classify(&onto(src)).unwrap();
    (c.stats().data_case_split_risk, c.completeness_guaranteed())
}

const PRE: &str = "Prefix(:=<http://ex.org/t#>)\n\
     Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
     Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
     Ontology(<http://ex.org/t>\n\
     Declaration(DataProperty(:d)) Declaration(DataProperty(:e))\n\
     Declaration(Class(:C)) Declaration(Class(:E))\n";

fn with_pre(body: &str) -> String {
    format!("{PRE}{body}\n)")
}

// ── Negatives: no case split needed, the guarantee stays. ──

/// Only points: `C ⊑ hasValue(d, 1)`, `hasValue(d, 1) ⊑ E`. `C ⊑ E` is found and the
/// closure is certified.
#[test]
fn points_only_keep_the_guarantee() {
    let src = with_pre(
        "SubClassOf(:C DataHasValue(:d \"1\"^^xsd:integer))\n\
         SubClassOf(DataHasValue(:d \"1\"^^xsd:integer) :E)",
    );
    assert_eq!(flagged(&src), (false, true));
    let c = owl_dl_reasoner::classify(&onto(&src)).unwrap();
    assert!(c.is_subclass("http://ex.org/t#C", "http://ex.org/t#E"));
}

/// One range and nothing it meets: no second key, nothing to split.
#[test]
fn a_lone_range_keeps_the_guarantee() {
    let src = with_pre(
        "SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))",
    );
    assert_eq!(flagged(&src), (false, true));
}

/// Disjoint ranges (`Range(d, [5,10])`, `C ⊑ ∃d.[1,3]`): the seeded disjointness makes
/// `C` unsatisfiable; no pair meets, so nothing is flagged.
#[test]
fn disjoint_ranges_keep_the_guarantee() {
    let src = with_pre(
        "DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive \
         \"5\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer))\n\
         SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))",
    );
    assert_eq!(flagged(&src), (false, true));
    let c = owl_dl_reasoner::classify(&onto(&src)).unwrap();
    assert!(c.unsatisfiable_classes().contains(&"http://ex.org/t#C"));
}

/// The one `SubDataPropertyOf` control (#632 fixed its false positive): only points,
/// so nothing is flagged and `C ⊑ E` is (rightly) not derived.
#[test]
fn a_sub_data_property_with_points_keeps_the_guarantee() {
    let src = with_pre(
        "SubDataPropertyOf(:d :e)\n\
         SubClassOf(:C DataHasValue(:d \"7\"^^xsd:integer))\n\
         SubClassOf(DataHasValue(:e \"1\"^^xsd:integer) :E)",
    );
    assert_eq!(flagged(&src), (false, true));
    let c = owl_dl_reasoner::classify(&onto(&src)).unwrap();
    assert!(!c.is_subclass("http://ex.org/t#C", "http://ex.org/t#E"));
}

/// Over-flagging, by design: the cover sits on another property (`t_otherprop`), so
/// nothing is entailed, but the role-blind detector flags it. Pinned so a refinement
/// that drops it is a deliberate change.
#[test]
fn a_cover_on_another_property_is_over_flagged() {
    let src = with_pre(
        "SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"1\"^^xsd:integer xsd:maxInclusive \"2\"^^xsd:integer)))\n\
         SubClassOf(DataHasValue(:e \"1\"^^xsd:integer) :E)\n\
         SubClassOf(DataHasValue(:e \"2\"^^xsd:integer) :E)",
    );
    assert_eq!(flagged(&src), (true, false));
}

// ── Positives: each read `completeness_guaranteed: true` with the entailment missed. ──

/// the issue: `∃d.{1,2}` with both points (`s_resid.ofn`).
#[test]
fn s_resid_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "s_resid");
}

/// the decimal form (`s_resid_dec.ofn`).
#[test]
fn s_resid_dec_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1.5"^^xsd:decimal "2.5"^^xsd:decimal)))
SubClassOf(DataHasValue(:d "1.5"^^xsd:decimal) :E)
SubClassOf(DataHasValue(:d "2.5"^^xsd:decimal) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "s_resid_dec");
}

/// `int[1,2]` with both points (`s_resid_range.ofn`).
#[test]
fn s_resid_range_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "s_resid_range");
}

/// `xsd:boolean` with true and false (`q_bool.ofn`).
#[test]
fn q_bool_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d xsd:boolean))
SubClassOf(DataHasValue(:d "true"^^xsd:boolean) :E)
SubClassOf(DataHasValue(:d "false"^^xsd:boolean) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_bool");
}

/// a string enumeration with its points (`q_str.ofn`).
#[test]
fn q_str_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("a" "b")))
SubClassOf(DataHasValue(:d "a") :E)
SubClassOf(DataHasValue(:d "b") :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_str");
}

/// `[1,10]` = `[1,5]` ∪ `[6,10]` (`q_r10.ofn`).
#[test]
fn q_r10_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "6"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_r10");
}

/// a range and a point cover `[1,10]` (`q_mix.ofn`).
#[test]
fn q_mix_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "2"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)) :E)
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_mix");
}

/// integer `[1,2]` by two decimal ranges (`q_intdec.ofn`).
#[test]
fn q_intdec_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "0.5"^^xsd:decimal xsd:maxInclusive "1.5"^^xsd:decimal)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "1.5"^^xsd:decimal xsd:maxInclusive "2.5"^^xsd:decimal)) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_intdec");
}

/// two overlapping decimal ranges (`q_decov.ofn`).
#[test]
fn q_decov_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "1.0"^^xsd:decimal xsd:maxInclusive "3.0"^^xsd:decimal)))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minInclusive "0.5"^^xsd:decimal xsd:maxInclusive "2.0"^^xsd:decimal)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minExclusive "1.5"^^xsd:decimal xsd:maxInclusive "3.5"^^xsd:decimal)) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_decov");
}

/// `xsd:integer` = `≤0` ∪ `≥1` (`q_unb.ofn`).
#[test]
fn q_unb_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d xsd:integer))
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:maxInclusive "0"^^xsd:integer)) :E)
SubClassOf(DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer)) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_unb");
}

/// a negated point (an EL ⊥-axiom) (`q_neg2.ofn`).
#[test]
fn q_neg2_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(:C ObjectComplementOf(DataHasValue(:d "1"^^xsd:integer)))
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "q_neg2");
}

/// a boolean range under an untyped filler (`r_range_bool.ofn`).
#[test]
fn r_range_bool_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
DataPropertyRange(:d xsd:boolean)
SubClassOf(:C DataSomeValuesFrom(:d rdfs:Literal))
SubClassOf(DataHasValue(:d "true"^^xsd:boolean) :E)
SubClassOf(DataHasValue(:d "false"^^xsd:boolean) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "r_range_bool");
}

/// a range narrows a filler to one point (`r_range_inter.ofn`).
#[test]
fn r_range_inter_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive "5"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
SubClassOf(DataHasValue(:d "5"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "r_range_inter");
}

/// a range narrows a filler to two points (`r_range_split.ofn`).
#[test]
fn r_range_split_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
DataPropertyRange(:d DatatypeRestriction(xsd:integer xsd:minInclusive "9"^^xsd:integer xsd:maxInclusive "20"^^xsd:integer))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "10"^^xsd:integer)))
SubClassOf(DataHasValue(:d "9"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "10"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "r_range_split");
}

/// `Range(nonNegativeInteger)` narrows `≤1` to 0 and 1 (`t_nni.ofn`).
#[test]
fn t_nni_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
DataPropertyRange(:d xsd:nonNegativeInteger)
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:maxInclusive "1"^^xsd:integer)))
SubClassOf(DataHasValue(:d "0"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "t_nni");
}

/// two adjacent float points (`t_float_adj.ofn`).
#[test]
fn t_float_adj_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:float xsd:minInclusive "1.0"^^xsd:float xsd:maxInclusive "1.0000001"^^xsd:float)))
SubClassOf(DataHasValue(:d "1.0"^^xsd:float) :E)
SubClassOf(DataHasValue(:d "1.0000001"^^xsd:float) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "t_float_adj");
}

/// an unrelated superset key (plan review blocker 1) (`t_superkey.ofn`).
#[test]
fn t_superkey_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
SubClassOf(:F DataSomeValuesFrom(:e DatatypeRestriction(xsd:integer xsd:minInclusive "0"^^xsd:integer xsd:maxInclusive "5"^^xsd:integer)))
)"#;
    assert_eq!(flagged(src), (true, false), "t_superkey");
}

/// The instance form (`a_real.ofn`): `instances-expr` and its guarantee read the flag.
#[test]
fn the_instance_query_reads_the_flag() {
    use horned_owl::model::{Build, ClassExpression};
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(Class(:C)) Declaration(Class(:E))
Declaration(NamedIndividual(:a))
ClassAssertion(:C :a)
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "1"^^xsd:integer xsd:maxInclusive "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#;
    let build: Build<RcStr> = Build::new();
    let e = ClassExpression::Class(build.class("http://ex.org/t#E"));
    let found = owl_dl_reasoner::class_expression_instances(&onto(src), &e).unwrap();
    assert!(found.data_case_split_risk());
    assert!(!found.completeness_guaranteed());
}

/// The subsumption query on the issue's repro: the flag and a voided guarantee.
#[test]
fn the_subsumption_queries_read_the_flag() {
    use horned_owl::model::{Build, ClassExpression};
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(NamedIndividual(:a))
Declaration(Class(:C)) Declaration(Class(:E))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
)"#;
    let o = onto(src);
    let build: Build<RcStr> = Build::new();
    let (c, e) = (
        ClassExpression::Class(build.class("http://ex.org/t#C")),
        ClassExpression::Class(build.class("http://ex.org/t#E")),
    );
    let v = owl_dl_reasoner::class_expression_entailed_subclass(&o, &c, &e).unwrap();
    assert!(v.data_case_split_risk());
    assert!(!v.completeness_guaranteed());
    let (_, stats) =
        owl_dl_reasoner::is_subclass_of_with_stats(&o, "http://ex.org/t#C", "http://ex.org/t#E")
            .unwrap();
    assert!(stats.data_case_split_risk && !stats.completeness_guaranteed());
}

/// an empty integer range (`int[5,1]`): `C` is unsatisfiable, and was certified satisfiable (`em_int.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn em_int_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer xsd:minInclusive "5"^^xsd:integer xsd:maxInclusive "1"^^xsd:integer)))
)"#;
    // Since #635 the empty integer range lowers to `⊥`: `C` is unsatisfiable, the right
    // answer, and the guarantee may stand. Before it, the flag had to void it.
    let c = owl_dl_reasoner::classify(&onto(src)).unwrap();
    let unsat = c.unsatisfiable_classes().contains(&"http://ex.org/t#C");
    assert!(
        unsat || flagged(src) == (true, false),
        "em_int: {:?}",
        c.stats()
    );
}

/// float bounds with no `f32` between them: an empty range (`em_float.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn em_float_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:float xsd:minExclusive "1.0"^^xsd:float xsd:maxExclusive "1.0000001"^^xsd:float)))
)"#;
    assert_eq!(flagged(src), (true, false), "em_float");
}

/// an empty decimal range (`(1.0, 1.0]`) (`dec_empty.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn dec_empty_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:decimal xsd:minExclusive "1.0"^^xsd:decimal xsd:maxInclusive "1.0"^^xsd:decimal)))
)"#;
    assert_eq!(flagged(src), (true, false), "dec_empty");
}

/// a plain string against bare `rdf:langString`, which the oracles read as holding it (`lg_top2.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn lg_top2_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataHasValue(:d "a"))
SubClassOf(DataSomeValuesFrom(:d rdf:langString) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "lg_top2");
}

/// a dateTime enumeration with its points (`dt_cover.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn dt_cover_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("2020-01-01T00:00:00"^^xsd:dateTime "2021-01-01T00:00:00"^^xsd:dateTime)))
SubClassOf(DataHasValue(:d "2020-01-01T00:00:00"^^xsd:dateTime) :E)
SubClassOf(DataHasValue(:d "2021-01-01T00:00:00"^^xsd:dateTime) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "dt_cover");
}

/// an anyURI enumeration with its points (`uri_cover.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn uri_cover_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("http://a"^^xsd:anyURI "http://b"^^xsd:anyURI)))
SubClassOf(DataHasValue(:d "http://a"^^xsd:anyURI) :E)
SubClassOf(DataHasValue(:d "http://b"^^xsd:anyURI) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "uri_cover");
}

/// a language-tagged enumeration with its points (`lang_cover.ofn`, Fable review of #589; `HermiT` derives the entailment).
#[test]
fn lang_cover_voids_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("a"@en "b"@en)))
SubClassOf(DataHasValue(:d "a"@en) :E)
SubClassOf(DataHasValue(:d "b"@en) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "lang_cover");
}

/// Empty ranges on every non-integer ordered bucket (from Fable's review of #635, which
/// lowers empty integer ranges to `⊥` only): each read satisfiable with the guarantee set,
/// `HermiT` says unsatisfiable. Each must now void the guarantee (or come out unsat).
#[test]
fn empty_ranges_on_every_ordered_bucket_void_the_guarantee() {
    let restriction = |dt: &str, lo_facet: &str, lo: &str, hi_facet: &str, hi: &str| {
        with_pre(&format!(
            "SubClassOf(:C DataSomeValuesFrom(:d DatatypeRestriction(xsd:{dt} \
             xsd:{lo_facet} \"{lo}\"^^xsd:{dt} xsd:{hi_facet} \"{hi}\"^^xsd:{dt})))"
        ))
    };
    for (what, src) in [
        (
            "decimal [3,2]",
            restriction("decimal", "minInclusive", "3", "maxInclusive", "2"),
        ),
        (
            "double [3,2]",
            restriction("double", "minInclusive", "3.0", "maxInclusive", "2.0"),
        ),
        (
            "float [3,2]",
            restriction("float", "minInclusive", "3.0", "maxInclusive", "2.0"),
        ),
        (
            "dateTime [2021,2020]",
            restriction(
                "dateTime",
                "minInclusive",
                "2021-01-01T00:00:00",
                "maxInclusive",
                "2020-01-01T00:00:00",
            ),
        ),
        (
            "decimal (3,3)",
            restriction("decimal", "minExclusive", "3", "maxExclusive", "3"),
        ),
    ] {
        let c = owl_dl_reasoner::classify(&onto(&src)).unwrap();
        let unsat = c.unsatisfiable_classes().contains(&"http://ex.org/t#C");
        assert!(
            unsat || !c.completeness_guaranteed(),
            "{what}: satisfiable and certified: {:?}",
            c.stats()
        );
        if !unsat {
            assert!(
                c.stats().data_case_split_risk || !c.stats().dropped.is_empty(),
                "{what}"
            );
        }
    }
}

/// Whole decimals past `i64` against `nonNegativeInteger` (`pBa.ofn`, Fable review of
/// #639): `HermiT` derives `C ⊑ E`; the detector's integer × decimal-enumeration arm read
/// the undecided values as disjoint. The twin (`1.0`, `2.0`) finds `C ⊑ E` outright.
#[test]
fn whole_decimals_past_i64_void_the_guarantee() {
    let src = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e))
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:M)) SubClassOf(:C :M)
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("100000000000000000000.0"^^xsd:decimal "200000000000000000000.0"^^xsd:decimal)))
SubClassOf(DataSomeValuesFrom(:d xsd:nonNegativeInteger) :E)
)"#;
    assert_eq!(flagged(src), (true, false), "pBa");
    let ctl = r#"Prefix(:=<http://ex.org/t#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/t>
Declaration(DataProperty(:d)) Declaration(DataProperty(:e))
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:M)) SubClassOf(:C :M)
SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1.0"^^xsd:decimal "2.0"^^xsd:decimal)))
SubClassOf(DataSomeValuesFrom(:d xsd:nonNegativeInteger) :E)
)"#;
    let c = owl_dl_reasoner::classify(&onto(ctl)).unwrap();
    assert!(
        c.is_subclass("http://ex.org/t#C", "http://ex.org/t#E"),
        "pBa_ctl"
    );
}

/// The consistency query stamps the flag too (#639 review: no test read it).
#[test]
fn the_consistency_query_reads_the_flag() {
    let src = with_pre(
        "Declaration(NamedIndividual(:a)) ClassAssertion(:C :a)\n\
         SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
         SubClassOf(DataHasValue(:d \"1\"^^xsd:integer) :E)\n\
         SubClassOf(DataHasValue(:d \"2\"^^xsd:integer) :E)",
    );
    let (consistent, stats) = owl_dl_reasoner::is_consistent_with_stats(&onto(&src)).unwrap();
    assert!(consistent);
    assert!(stats.data_case_split_risk, "{stats:?}");
}
