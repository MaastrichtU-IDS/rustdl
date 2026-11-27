//! #603: a step proof that rests on an `A ⊑ D` the told-definition match derived
//! (#139/#180) cites that edge's source premises, not the derived edge. On the curated
//! corpus these were the only derived axioms step proofs cited (wine 47 of 150 sampled
//! proofs, pizza 2 of 113). Two more kinds are replayed the same way. On ORE ontologies
//! rich in data axioms (13,727 calls), they were the derived kinds step proofs cited,
//! DKey seed edges apart:
//! - the union common-subsumer `disjunction_existential` derives (`X ⊑ D₁ ⊔ … ⊔ Dₙ`,
//!   every `Dᵢ` told-below `C`, so `X ⊑ C`): 15 proofs (SWEET, `OLiA`), plus wine's
//!   `Fruit ⊑ EdibleThing`;
//! - `data_axioms`' domain inference (`C ⊑ =1 dp`, `domain(dp) = D`, so `C ⊑ D`):
//!   23 proofs (OAEI benchmarks, SWEET), replayed on the source components.
//!
//! Each test checks the citation three ways: nothing is left in `derived_axioms`; every
//! cited axiom is a component of the source; and the cited axioms ALONE entail the goal
//! with the told-definition match switched off (`RUSTDL_TOLD_DEFINITION_MATCH=0`), so the
//! pass cannot vouch for itself.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;
use std::process::Command;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;

const T: &str = "http://t/";

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/prove_cites_source/{name}.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn rustdl(env: &[(&str, &str)], args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(var);
        }
    }
    cmd.envs(env.iter().copied()).args(args).output().unwrap()
}

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

/// The body of a one-axiom document.
fn body(doc: &str) -> String {
    let b = doc.split("Ontology(").nth(1).unwrap();
    b.trim().trim_end_matches(')').trim().to_owned()
}

/// `prove --json FILE A D`: the cited axioms (bodies), after asserting the proof has no
/// derived or builtin axiom and every cited one is a source component.
fn cited(file: &str, sub: &str, sup: &str) -> Vec<String> {
    let path = fixture(file);
    let out = rustdl(&[], &["prove", "--json", &path, sub, sup]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["has_proof"], true, "{file}: {v}");
    assert_eq!(v["proof_uses_non_source_axioms"], false, "{file}: {v}");
    let source = components(&std::fs::read_to_string(&path).unwrap());
    let mut docs = Vec::new();
    fn walk(n: &serde_json::Value, docs: &mut Vec<String>) {
        for a in n["axioms"].as_array().unwrap() {
            docs.push(a.as_str().unwrap().to_owned());
        }
        for p in n["premises"].as_array().unwrap() {
            walk(p, docs);
        }
    }
    walk(&v["proof"], &mut docs);
    for d in &docs {
        let c = components(d);
        assert!(
            source.contains(c.iter().next().unwrap()),
            "{file}: not a source axiom: {d}"
        );
    }
    let mut bodies: Vec<String> = docs.iter().map(|d| body(d)).collect();
    bodies.sort();
    bodies.dedup();
    bodies
}

/// Whether `axioms` alone (with the fixture's declarations and prefixes) entail
/// `sub ⊑ sup`, with the told-definition match off.
fn entailed_by(file: &str, axioms: &[String], sub: &str, sup: &str) -> bool {
    let src = std::fs::read_to_string(fixture(file)).unwrap();
    let mut doc = String::new();
    for line in src.lines() {
        if line.starts_with("Prefix(") {
            doc.push_str(line);
            doc.push('\n');
        }
    }
    doc.push_str("Ontology(<http://cited/only>\n");
    for line in src.lines().filter(|l| l.starts_with("Declaration(")) {
        doc.push_str(line);
        doc.push('\n');
    }
    for a in axioms {
        doc.push_str(a);
        doc.push('\n');
    }
    doc.push_str(")\n");
    let path = std::env::temp_dir().join(format!(
        "rustdl-603-{}-{file}-{}.ofn",
        std::process::id(),
        axioms.len()
    ));
    std::fs::write(&path, doc).unwrap();
    let out = rustdl(
        &[("RUSTDL_TOLD_DEFINITION_MATCH", "0")],
        &["subclass", path.to_str().unwrap(), sub, sup],
    );
    let _ = std::fs::remove_file(&path);
    String::from_utf8_lossy(&out.stdout).trim() == "yes"
}

#[test]
fn a_matched_definition_cites_its_premises() {
    let (a, d) = (format!("{T}A"), format!("{T}D"));
    let c = cited("tdm_one_round", &a, &d);
    assert_eq!(c.len(), 3, "{c:?}");
    assert!(entailed_by("tdm_one_round", &c, &a, &d), "{c:?}");
    // Each cited axiom is needed: drop any one and the rest no longer entail it.
    for i in 0..c.len() {
        let mut fewer = c.clone();
        fewer.remove(i);
        assert!(!entailed_by("tdm_one_round", &fewer, &a, &d), "{fewer:?}");
    }
}

#[test]
fn the_told_path_to_the_asserting_class_is_cited() {
    // A reaches M by a told edge; M asserts `∀r.C` and, through its definition, `K`.
    let (a, d) = (format!("{T}A"), format!("{T}D"));
    let c = cited("tdm_path", &a, &d);
    assert!(c.contains(&"SubClassOf(:A :M)".to_owned()), "{c:?}");
    assert!(entailed_by("tdm_path", &c, &a, &d), "{c:?}");
}

#[test]
fn a_second_round_match_resolves_the_first_round_edge() {
    // `A ⊑ D2` needs `A ⊑ D1`, itself derived in the first round.
    let (a, d2) = (format!("{T}A"), format!("{T}D2"));
    let c = cited("tdm_two_rounds", &a, &d2);
    assert!(entailed_by("tdm_two_rounds", &c, &a, &d2), "{c:?}");
}

#[test]
fn a_union_member_edge_cites_the_union_definition() {
    // `D ≡ A ⊔ B` and `X ⊑ A`: another pass states `A ⊑ D` as an atomic axiom; its
    // source is the union definition (wine's `SweetFruit ⊑ Fruit`).
    let (x, d) = (format!("{T}X"), format!("{T}D"));
    let c = cited("union_member", &x, &d);
    assert!(
        c.contains(&"EquivalentClasses(:D ObjectUnionOf(:A :B))".to_owned()),
        "{c:?}"
    );
    assert!(entailed_by("union_member", &c, &x, &d), "{c:?}");
}

/// The union common-subsumer, as wine (`Fruit ≡ NonSweetFruit ⊔ SweetFruit`, both
/// `⊑ EdibleThing`) and SWEET (`Sorption ≡ Adsorption ⊔ Absorption`) assert it: the
/// definition and each member's edge, each needed.
#[test]
fn a_union_common_subsumer_cites_its_premises() {
    let (a, d) = (format!("{T}Fruit"), format!("{T}EdibleThing"));
    let c = cited("union_common_equiv", &a, &d);
    assert_eq!(c.len(), 3, "{c:?}");
    assert!(
        c.iter().any(|x| x.starts_with("EquivalentClasses(")),
        "{c:?}"
    );
    assert!(entailed_by("union_common_equiv", &c, &a, &d), "{c:?}");
    for i in 0..c.len() {
        let mut fewer = c.clone();
        fewer.remove(i);
        assert!(
            !entailed_by("union_common_equiv", &fewer, &a, &d),
            "{c:?} without {}",
            c[i]
        );
    }
}

/// As `OLiA` asserts it (`X ⊑ D₁ ⊔ D₂`, a subclass axiom, not a definition), with a member
/// reaching the common subsumer through a told path.
#[test]
fn a_union_super_with_a_told_path_cites_the_path() {
    let (a, d) = (format!("{T}X"), format!("{T}P"));
    let c = cited("union_common_sub_path", &a, &d);
    assert_eq!(c.len(), 4, "{c:?}");
    assert!(entailed_by("union_common_sub_path", &c, &a, &d), "{c:?}");
    for i in 0..c.len() {
        let mut fewer = c.clone();
        fewer.remove(i);
        assert!(
            !entailed_by("union_common_sub_path", &fewer, &a, &d),
            "{c:?} without {}",
            c[i]
        );
    }
}

/// The union as a conjunct of the super (`X ⊑ A ⊓ (D₁ ⊔ D₂)`).
#[test]
fn a_union_conjunct_cites_its_premises() {
    let (a, d) = (format!("{T}X"), format!("{T}C"));
    let c = cited("union_common_conjunct", &a, &d);
    assert_eq!(c.len(), 3, "{c:?}");
    assert!(entailed_by("union_common_conjunct", &c, &a, &d), "{c:?}");
}

/// `data_axioms`' domain inference, as OAEI benchmarks assert it: `Manual ⊑ =1 title`
/// and `domain(title) = Reference` give `Manual ⊑ Reference`. It derives from source
/// components, so the replay is on the source: the restriction and the domain, each
/// needed.
#[test]
fn a_data_property_domain_inference_cites_its_premises() {
    for (file, sub, sup) in [
        ("data_exact_domain", "Manual", "Reference"),
        // SWEET's shape: `Neutral ⊑ hasValue(hasCharge, 0)`.
        ("data_hasvalue_domain", "Neutral", "Chemical"),
    ] {
        let (a, d) = (format!("{T}{sub}"), format!("{T}{sup}"));
        let c = cited(file, &a, &d);
        assert_eq!(c.len(), 2, "{file}: {c:?}");
        assert!(
            c.iter().any(|x| x.starts_with("DataPropertyDomain(")),
            "{file}: {c:?}"
        );
        assert!(entailed_by(file, &c, &a, &d), "{file}: {c:?}");
        for i in 0..c.len() {
            let mut fewer = c.clone();
            fewer.remove(i);
            assert!(
                !entailed_by(file, &fewer, &a, &d),
                "{file}: {c:?} without {}",
                c[i]
            );
        }
    }
}
