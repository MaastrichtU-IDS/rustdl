//! SP1: semantic branching + disjunct reordering verdict-preservation + canaries.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::HyperResult;
use std::io::Cursor;

// Disjunction where one branch is SAT and another self-clashes: A ≡ (B ⊔ C),
// A ⊑ ¬C  ⟹  A is satisfiable via B. Verdict must be identical flag-on/off.
const SAT_ONT: &str = "Prefix(:=<urn:s#>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  EquivalentClasses(:A ObjectUnionOf(:B :C))
  SubClassOf(:A ObjectComplementOf(:C))
)";

fn load(s: &str) -> SetOntology<RcStr> {
    let mut r = Cursor::new(s.as_bytes().to_vec());
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse").0
}

fn sat(ont: &SetOntology<RcStr>, c: &str) -> HyperResult {
    owl_dl_reasoner::sat_class_probe(ont, c, 256, None)
        .expect("probe")
        .expect("class")
        .0
}

struct EnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Tests are serialized
        // per-key; env state is restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }

    #[allow(unsafe_code)]
    fn remove(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: remove_var is unsafe under edition 2024. Restored on Drop.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, prior }
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see EnvGuard::set / EnvGuard::remove.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[test]
fn reorder_preserves_sat_verdict() {
    let ont = load(SAT_ONT);
    let off_guard = EnvGuard::remove("RUSTDL_WEDGE_SEMANTIC_BRANCHING");
    let off = sat(&ont, "urn:s#A");
    drop(off_guard);
    let _on_guard = EnvGuard::set("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1");
    let on = sat(&ont, "urn:s#A");
    assert_eq!(off, HyperResult::Sat, "A is satisfiable via the B disjunct");
    assert_eq!(on, off, "reordering must not change the verdict");
}

// Reordering must put an obviously-clashing disjunct last: A ⊑ ¬B, A ≡ (B ⊔ D).
// Verdict (Sat) is the gate.
#[test]
fn reorder_obvious_clash_last_still_sat() {
    let ont = load(
        "Prefix(:=<urn:r#>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))
  EquivalentClasses(:A ObjectUnionOf(:B :D))
  SubClassOf(:A ObjectComplementOf(:B))
)",
    );
    let _g = EnvGuard::set("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1");
    assert_eq!(sat(&ont, "urn:r#A"), HyperResult::Sat);
}

// FP regression (cross-var disjunct): a disjunct on a SUCCESSOR var must have its
// semantic-branching complement asserted on the SUCCESSOR, not the body node.
// C is satisfiable (the R-successor takes D2); a previous bug asserted ¬D1 on the
// body node n0 (which carries D1 via C ⊑ D1), producing a spurious Unsat.
// Flag-on MUST equal flag-off MUST equal Sat.
#[test]
fn semantic_branching_cross_var_no_fp() {
    let ont = load(
        "Prefix(:=<urn:x#>)
Ontology(
  Declaration(Class(:C)) Declaration(Class(:D1)) Declaration(Class(:D2))
  Declaration(Class(:E)) Declaration(Class(:G)) Declaration(Class(:S))
  Declaration(ObjectProperty(:R))
  EquivalentClasses(:E ObjectIntersectionOf(:D1 :G))
  SubClassOf(:C ObjectSomeValuesFrom(:R :S))
  SubClassOf(:C ObjectAllValuesFrom(:R ObjectUnionOf(:D1 :D2)))
  SubClassOf(:S ObjectComplementOf(:D1))
  SubClassOf(:C :D1)
)",
    );
    let off_guard = EnvGuard::remove("RUSTDL_WEDGE_SEMANTIC_BRANCHING");
    let off = sat(&ont, "urn:x#C");
    drop(off_guard);
    let _on_guard = EnvGuard::set("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1");
    let on = sat(&ont, "urn:x#C");
    assert_eq!(off, HyperResult::Sat, "C is satisfiable (successor takes D2)");
    assert_eq!(on, off, "semantic branching must NOT introduce a spurious Unsat (FP)");
}

// Semantic branching must NOT break disjunctive-Unsat proofs. A ≡ (B ⊔ C),
// A ⊑ ¬B, A ⊑ ¬C ⟹ A ⊑ ⊥ (unsat) — both disjuncts clash. Flag-on must still
// return Unsat (a dropped clash would be a MISSED subsumption).
#[test]
fn semantic_branching_preserves_unsat() {
    let ont = load(
        "Prefix(:=<urn:u#>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  EquivalentClasses(:A ObjectUnionOf(:B :C))
  SubClassOf(:A ObjectComplementOf(:B))
  SubClassOf(:A ObjectComplementOf(:C))
)",
    );
    let _g = EnvGuard::set("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1");
    assert_eq!(sat(&ont, "urn:u#A"), HyperResult::Unsat, "A must still be unsat");
}
