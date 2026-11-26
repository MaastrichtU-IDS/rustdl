//! #586: `prove --json` cites each source axiom as written, so a consumer that looks it up
//! literally (the Protégé plugin's `RustdlOfn.verifiedAgainst`, OWLAPI `containsAxiom`
//! ignoring annotations) finds it.
//!
//! The proof's `axioms[]` used to be conversion's stored form reverse-rendered: `ObjectHasValue`
//! came back as `∃r.{a}`, `owl:Thing` as `ObjectIntersectionOf()`, an exact cardinality as
//! `≥ ⊓ ≤`, `InverseFunctional(r⁻)` as `Functional(r)`, a data-property axiom as its
//! object-role lowering, a long chain as pieces through `urn:rustdl-aux-role:` roles, and
//! the rewritten `X ⊑ ¬Y` and `(X ⊔ Y) ⊑ D` as their rewrites. Protégé rejected each such
//! proof as a possible fabrication. Each test below is the guard, done in Rust: every cited
//! axiom is a component of the source. An axiom a later conversion pass derived from several
//! (here a datatype seed edge) has no single source: it is listed under `derived_axioms`,
//! never `axioms`.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;
use std::process::Command;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;

const T: &str = "http://t/";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/prove_cites_source/{name}.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// The logical components of an OFN document, annotations dropped (the Protégé guard
/// ignores them too).
fn components(doc: &str) -> BTreeSet<Component<RcStr>> {
    let (o, _): (SetOntology<RcStr>, _) = read(
        &mut Cursor::new(doc.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap();
    o.iter()
        .map(|ac| ac.component.clone())
        .filter(|c| !matches!(c, Component::OntologyID(_) | Component::DocIRI(_)))
        .collect()
}

struct Cited {
    axioms: Vec<String>,
    derived: Vec<String>,
}

/// `prove --json FILE SUB SUP`: the proof's cited `axioms` (each the body of its one-axiom
/// document) and `derived_axioms`, over the whole tree. Asserts a step proof exists and
/// that every cited axiom is a component of the source.
fn cited(file: &str, sub: &str, sup: &str) -> Cited {
    let path = fixture(file);
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .env_remove("RUSTDL_HOME")
        .env_remove("RUSTDL_ORE_DIR")
        .args(["prove", "--json", &path, sub, sup])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{file}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["has_proof"], true, "{file}: expected a step proof: {v}");
    let source = components(&std::fs::read_to_string(&path).unwrap());
    let mut c = Cited {
        axioms: Vec::new(),
        derived: Vec::new(),
    };
    fn walk(node: &serde_json::Value, c: &mut Cited) {
        for a in node["axioms"].as_array().unwrap() {
            c.axioms.push(a.as_str().unwrap().to_owned());
        }
        for a in node["derived_axioms"].as_array().into_iter().flatten() {
            c.derived.push(a.as_str().unwrap().to_owned());
        }
        for p in node["premises"].as_array().unwrap() {
            walk(p, c);
        }
    }
    walk(&v["proof"], &mut c);
    for doc in &c.axioms {
        let cs = components(doc);
        assert_eq!(cs.len(), 1, "{file}: one axiom per document: {doc}");
        assert!(
            source.contains(cs.iter().next().unwrap()),
            "{file}: cited axiom not in the source (Protégé would reject the proof): {doc}"
        );
    }
    c
}

/// The body of each cited document (`Ontology(  <axiom>\n)` → `<axiom>`), sorted.
fn bodies(docs: &[String]) -> Vec<String> {
    let mut v: Vec<String> = docs
        .iter()
        .map(|d| {
            let b = d.split("Ontology(").nth(1).unwrap();
            b.trim().trim_end_matches(')').trim().to_owned()
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

fn x_d(file: &str) -> Cited {
    cited(file, &format!("{T}X"), &format!("{T}D"))
}

#[test]
fn an_inverse_functional_inverse_is_cited_as_written() {
    let c = x_d("inverse_functional_inverse");
    assert!(
        bodies(&c.axioms).contains(&"InverseFunctionalObjectProperty(ObjectInverseOf(:r))".into()),
        "{:?}",
        bodies(&c.axioms)
    );
}

#[test]
fn a_long_chain_is_cited_whole_not_as_its_pieces() {
    let c = x_d("chain3");
    assert!(
        bodies(&c.axioms).contains(&"SubObjectPropertyOf(ObjectPropertyChain(:p :q :s) :t)".into()),
        "{:?}",
        bodies(&c.axioms)
    );
    assert!(c.derived.is_empty(), "{:?}", c.derived);
    // A 4-chain (its middle piece has an aux role on both sides), next to another long
    // chain whose aux roles must not join its cascade.
    let c = x_d("chain4_and_another");
    assert!(
        bodies(&c.axioms)
            .contains(&"SubObjectPropertyOf(ObjectPropertyChain(:p :q :s :u) :t)".into()),
        "{:?}",
        bodies(&c.axioms)
    );
    assert!(c.derived.is_empty(), "{:?}", c.derived);
}

#[test]
fn normalised_class_expressions_are_cited_as_written() {
    for (file, want) in [
        ("has_value", "SubClassOf(:X ObjectHasValue(:r :a))"),
        ("thing", "SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))"),
        ("exact", "SubClassOf(:X ObjectExactCardinality(1 :r :A))"),
        ("union_lhs", "SubClassOf(ObjectUnionOf(:X :Y) :D)"),
    ] {
        let c = x_d(file);
        assert!(
            bodies(&c.axioms).contains(&want.into()),
            "{file}: {:?}",
            bodies(&c.axioms)
        );
    }
}

#[test]
fn a_negated_super_is_cited_as_written() {
    let c = cited("negated_super", &format!("{T}A"), NOTHING);
    assert!(
        bodies(&c.axioms).contains(&"SubClassOf(:X ObjectComplementOf(:Y))".into()),
        "{:?}",
        bodies(&c.axioms)
    );
}

#[test]
fn a_data_property_axiom_is_cited_as_written() {
    let c = x_d("data_property");
    assert!(
        bodies(&c.axioms).contains(&"SubDataPropertyOf(:dp :dq)".into()),
        "{:?}",
        bodies(&c.axioms)
    );
}

#[test]
fn an_annotated_or_repeated_operand_axiom_is_cited_as_the_source_has_it() {
    // The annotation is not part of the citation (as the guard ignores it).
    let c = x_d("annotated");
    assert!(bodies(&c.axioms).contains(&"SubClassOf(:X :Y)".into()));
    // The source's own operand list, not conversion's set-canonical one.
    let c = cited("repeated_operand", &format!("{T}A"), NOTHING);
    assert!(
        bodies(&c.axioms).contains(&"DisjointClasses(:C :C :D)".into()),
        "{:?}",
        bodies(&c.axioms)
    );
}

/// Control: an axiom a later pass derived (the datatype seed edge `5 ∈ [0, ∞)`) has no
/// single source, so it is listed apart, never as the source's.
#[test]
fn a_derived_axiom_is_listed_apart_from_the_source_axioms() {
    let c = x_d("dkey_edge");
    assert_eq!(c.derived.len(), 1, "{:?}", c.derived);
    assert!(
        c.derived[0].contains("urn:rustdl-dkey:"),
        "{}",
        c.derived[0]
    );
    assert!(
        bodies(&c.axioms).contains(&"SubClassOf(:X DataHasValue(:dp \"5\"^^xsd:integer))".into()),
        "{:?}",
        bodies(&c.axioms)
    );
}
