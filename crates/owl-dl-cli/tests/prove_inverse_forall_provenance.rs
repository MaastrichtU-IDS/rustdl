//! `prove` must cite every axiom an existential trigger rests on.
//!
//! Three gaps, each of which left a proof's axiom list short of a justification:
//!
//! * The inverse-`∀` triggers (#180's `∃S.Key(R,K) ⊑ K`, #274's nominal-set
//!   variant) come from a `∀` key plus the role axioms making `S ⊑ R⁻`, so no single
//!   axiom produced them and the proof cited none of those role axioms. The nominal
//!   variant's head also rests on class assertions and told subsumptions.
//! * A trigger firing on an edge of a SUB-role of its own role did not cite that
//!   role inclusion (any trigger, not only the inverse ones).
//! * #180's atomic `∀` key had no synthetic definition, so `prove --json` refused
//!   to render any proof mentioning it.
//!
//! Each test collects the axioms cited anywhere in the `prove --json` tree and
//! checks they are the whole justification.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/json/{name}.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// The functional-syntax body of every axiom cited anywhere in the proof tree,
/// sorted, from `prove --json FILE SUB SUP` (local names under `http://t/`).
fn cited_axioms(file: &str, sub: &str, sup: &str) -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args([
            "prove",
            "--json",
            &fixture(file),
            &format!("http://t/{sub}"),
            &format!("http://t/{sup}"),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "prove --json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["has_proof"], true, "expected a step proof");
    fn walk(node: &serde_json::Value, out: &mut Vec<String>) {
        for a in node["axioms"].as_array().unwrap() {
            // "Prefix(..)\nOntology(    <axiom>\n)\n" → "<axiom>"
            let s = a.as_str().unwrap();
            let body = s.split("Ontology(").nth(1).unwrap();
            let body = body.trim().trim_end_matches(')').trim().to_string();
            if !out.contains(&body) {
                out.push(body);
            }
        }
        for p in node["premises"].as_array().unwrap() {
            walk(p, out);
        }
    }
    let mut axioms = Vec::new();
    walk(&v["proof"], &mut axioms);
    axioms.sort();
    axioms
}

fn sorted(v: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = v.iter().map(ToString::to_string).collect();
    v.sort();
    v
}

/// #180: `A ⊑ ∀p.K`, `Inverse(p, q)`, `s ⊑ q`, `X ⊑ ∃s.A` ⟹ `X ⊑ K`. Also the case
/// `prove --json` used to refuse, for want of a definition of the `∀p.K` key.
#[test]
fn an_atomic_inverse_forall_cites_its_role_axioms() {
    assert_eq!(
        cited_axioms("prove_inverse_forall_atomic", "X", "K"),
        sorted(&[
            "SubClassOf(:A ObjectAllValuesFrom(:p :K))",
            "InverseObjectProperties(:p :q)",
            "SubObjectPropertyOf(:s :q)",
            "SubClassOf(:X ObjectSomeValuesFrom(:s :A))",
        ])
    );
}

/// #274: `A ⊑ ∀R.{a}`, `Inverse(S, R)`, `C ⊑ ∃S.A`, `a : D` ⟹ `C ⊑ D`.
#[test]
fn a_nominal_inverse_forall_cites_the_class_assertion() {
    assert_eq!(
        cited_axioms("prove_inverse_forall_nominal", "C", "D"),
        sorted(&[
            "SubClassOf(:A ObjectAllValuesFrom(:R ObjectOneOf(:a)))",
            "InverseObjectProperties(:S :R)",
            "SubClassOf(:C ObjectSomeValuesFrom(:S :A))",
            "ClassAssertion(:D :a)",
        ])
    );
}

/// #274, two members: `C ⊑ G` rests on both members' assertions and on the told
/// `D ⊑ G`, `E ⊑ G` — and not on `F`, which holds of `a` only.
#[test]
fn a_nominal_set_inverse_forall_cites_every_members_typing() {
    assert_eq!(
        cited_axioms("prove_inverse_forall_nominal_set", "C", "G"),
        sorted(&[
            "SubClassOf(:A ObjectAllValuesFrom(:R ObjectUnionOf(ObjectOneOf(:a) ObjectOneOf(:b))))",
            "InverseObjectProperties(:S :R)",
            "SubClassOf(:C ObjectSomeValuesFrom(:S :A))",
            "ClassAssertion(:D :a)",
            "ClassAssertion(:E :b)",
            "SubClassOf(:D :G)",
            "SubClassOf(:E :G)",
        ])
    );
}

/// Any trigger: `∃q.A ⊑ K` fires on `X ⊑ ∃s.A` through `s ⊑ q`.
#[test]
fn a_trigger_firing_on_a_sub_role_cites_the_inclusion() {
    assert_eq!(
        cited_axioms("prove_trigger_sub_role", "X", "K"),
        sorted(&[
            "SubClassOf(ObjectSomeValuesFrom(:q :A) :K)",
            "SubClassOf(:X ObjectSomeValuesFrom(:s :A))",
            "SubObjectPropertyOf(:s :q)",
        ])
    );
}

/// Two told paths from `a`'s class `Y` to `H`: a labelled one (`Y ⊑ M ⊑ N ⊑ H`) and
/// a shorter one through the enumeration rule's `Y ⊑ X ⊑ H` edge (`X ≡ {h1, h2}`),
/// which records no axiom. The citation must follow the labelled path; the shorter
/// one would leave the step unjustified.
#[test]
fn a_member_typing_path_prefers_recorded_axioms() {
    assert_eq!(
        cited_axioms("prove_trigger_told_path_decoy", "C", "H"),
        sorted(&[
            "SubClassOf(:A ObjectAllValuesFrom(:R ObjectOneOf(:a)))",
            "InverseObjectProperties(:S :R)",
            "SubClassOf(:C ObjectSomeValuesFrom(:S :A))",
            "ClassAssertion(:Y :a)",
            "SubClassOf(:Y :M)",
            "SubClassOf(:M :N)",
            "SubClassOf(:N :H)",
        ])
    );
}

/// The head is itself a `∀` key reached through a monotonicity edge
/// `∀p.K ⊑ ∀p.L`, which must cite the told `K ⊑ L` it was built from.
#[test]
fn a_forall_monotonicity_edge_cites_its_told_subsumption() {
    assert_eq!(
        cited_axioms("prove_trigger_forall_monotonicity", "C", "D"),
        sorted(&[
            "SubClassOf(:A ObjectAllValuesFrom(:R ObjectOneOf(:a)))",
            "InverseObjectProperties(:S :R)",
            "SubClassOf(:C ObjectSomeValuesFrom(:S :A))",
            "ClassAssertion(:Y :a)",
            "SubClassOf(:Y ObjectAllValuesFrom(:p :K))",
            "SubClassOf(:K :L)",
            "EquivalentClasses(:D ObjectIntersectionOf(:Z ObjectAllValuesFrom(:p :L)))",
            "SubClassOf(:C :Z)",
        ])
    );
}
