//! #118 — `prove` must attribute a conjunctive `ObjectPropertyDomain` /
//! `ObjectPropertyRange`-derived subsumption to its axiom.
//!
//! `collect_el_rules_with_provenance` (owl-dl-saturation) carries two mirrors of
//! the real Pass 1 domain/range lowering, used purely to attribute derived rule
//! slots to their source axiom: the `domain_axiom_refs` collector, and a mini
//! Pass-1 simulation that rebuilds `effective_ranges`/`chain_ranges` so the
//! per-axiom rule-slot delta cursors in the mini re-run of `lower_sub_class_of`
//! stay aligned with the real run. Before this fix, both mirrors accepted only
//! an `Atomic` filler, so a conjunctive one (`ObjectIntersectionOf(:P :Q)`) was
//! invisible to them even though the real Pass 1 (#110/#119) now decomposes it:
//! `prove` on a conjunctive-domain pair produced the right ANSWER with NO axiom
//! reference (`(Domain(sub)) ⊢ X SubClassOf P` with no `[axiom[i]]`), and on a
//! conjunctive-range pair the cursor misalignment misattributed (or dropped)
//! provenance for every axiom processed after the affected one.
//!
//! These two canaries are deliberately SEPARATE — a single domain fixture
//! leaves the range mirror unguarded, which is how this gap survived once
//! already (see the domain-only fix in #110/PR #112).

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn rustdl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
}

fn prove_conjunctive_domain() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/prove_conjunctive_domain.ofn"
    )
}

fn prove_conjunctive_range() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/prove_conjunctive_range.ofn"
    )
}

/// Recursively collect `(rule, conclusion, axioms[])` from every node of a
/// `prove --json` proof tree.
fn collect_nodes<'a>(node: &'a serde_json::Value, out: &mut Vec<(&'a str, &'a str, Vec<&'a str>)>) {
    let rule = node["rule"].as_str().unwrap();
    let conclusion = node["conclusion"].as_str().unwrap();
    let axioms: Vec<&str> = node["axioms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    out.push((rule, conclusion, axioms));
    for premise in node["premises"].as_array().unwrap() {
        collect_nodes(premise, out);
    }
}

#[test]
fn prove_attributes_a_conjunctive_domain_derived_subsumption_to_its_axiom() {
    // ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q)) + X ⊑ ∃r.B ⟹ X ⊑ P.
    let out = rustdl()
        .args([
            "prove",
            "--json",
            prove_conjunctive_domain(),
            "http://ex/#X",
            "http://ex/#P",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["entailed"], true);
    assert_eq!(
        v["has_proof"], true,
        "conjunctive domain is EL-fragment; must have a step proof"
    );

    let mut nodes = Vec::new();
    collect_nodes(&v["proof"], &mut nodes);

    // The root step must be a Domain derivation citing the ObjectPropertyDomain
    // axiom — not an empty axiom_refs, which is exactly the #118 bug (the
    // engine derived the right ANSWER with no provenance).
    let root_rule = v["proof"]["rule"].as_str().unwrap();
    assert_eq!(root_rule, "Domain(sub)", "expected a Domain(sub) root step");
    let root_axioms: Vec<&str> = v["proof"]["axioms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(
        !root_axioms.is_empty(),
        "the conjunctive-domain-derived step must cite its source axiom, got none"
    );
    assert!(
        root_axioms.iter().any(|a| {
            a.contains("ObjectPropertyDomain") && a.contains(":P") && a.contains(":Q")
        }),
        "expected the cited axiom to be ObjectPropertyDomain(:r ObjectIntersectionOf(:P :Q)); \
         got {root_axioms:?}"
    );
    // Sanity: some node in the tree must still be the exists-fact leaf.
    assert!(
        nodes
            .iter()
            .any(|(rule, _, axioms)| *rule == "ToldFact" && axioms.iter().any(|a| a.contains(":B"))),
        "expected a ToldFact leaf citing the SubClassOf(:X ObjectSomeValuesFrom(:r :B)) axiom"
    );
}

#[test]
fn prove_attributes_a_conjunctive_range_derived_subsumption_to_its_axiom() {
    // ObjectPropertyRange(:r ObjectIntersectionOf(:P :Q))
    // + X ⊑ ∃r.B + ∃r.(B⊓P) ⊑ D  ⟹  X ⊑ D.
    //
    // The range mirror's bug does not show up at the ROOT step — that step's
    // axiom_refs come from `domain_axiom_refs`-independent machinery
    // (`ConjunctiveTrigger`/`ExistentialTrigger`, which already cite the
    // consuming axiom correctly either way). It shows up DEEPER: the
    // conjunctive range folds `Range(r) = P⊓Q` into the Tseitin synthetic
    // `F ≡ B⊓P⊓Q` that stands in for X's ∃r.B witness, and the mini
    // provenance simulation must re-derive that SAME synthetic (so its
    // per-axiom rule-slot cursors for every axiom after the affected one stay
    // aligned with the real run) to correctly attribute the leaf
    // `F ⊑ B` / `F ⊑ P` ToldSubsumer steps back to the EXISTS axiom
    // (`SubClassOf(:X ObjectSomeValuesFrom(:r :B))`). Before the fix these
    // leaves lost their axiom reference entirely.
    let out = rustdl()
        .args([
            "prove",
            "--json",
            prove_conjunctive_range(),
            "http://ex/#X",
            "http://ex/#D",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["entailed"], true);
    assert_eq!(
        v["has_proof"], true,
        "conjunctive range is EL-fragment; must have a step proof"
    );

    let mut nodes = Vec::new();
    collect_nodes(&v["proof"], &mut nodes);

    // Find the ToldSubsumer leaves for the range-widened synthetic
    // (ObjectIntersectionOf(:B :P :Q)) and require each to cite the EXISTS
    // axiom that created the fact the synthetic stands in for.
    let range_widened_leaves: Vec<&(&str, &str, Vec<&str>)> = nodes
        .iter()
        .filter(|(rule, conclusion, _)| {
            *rule == "ToldSubsumer" && conclusion.contains("ObjectIntersectionOf(:B :P :Q)")
        })
        .collect();
    assert!(
        !range_widened_leaves.is_empty(),
        "expected ToldSubsumer leaves for the range-widened synthetic \
         ObjectIntersectionOf(:B :P :Q); got nodes {nodes:?}"
    );
    for (rule, conclusion, axioms) in &range_widened_leaves {
        assert!(
            !axioms.is_empty(),
            "range-widened ToldSubsumer leaf ({rule}) {conclusion} lost its axiom \
             reference — this is the #118 range-mirror bug"
        );
        assert!(
            axioms.iter().any(|a| {
                a.contains(":X") && a.contains(":B") && a.contains("ObjectSomeValuesFrom")
            }),
            "expected the cited axiom to be SubClassOf(:X ObjectSomeValuesFrom(:r :B)); \
             got {axioms:?}"
        );
    }
}
