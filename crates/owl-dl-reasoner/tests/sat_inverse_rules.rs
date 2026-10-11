//! Canaries for the two #180 saturator rules over the SIGNED role hierarchy
//! (`RUSTDL_SAT_INVERSE_FORALL`, `RUSTDL_SAT_INVERSE_FUNC_MERGE`, both default ON).
//!
//! THE GAP. The saturator's role hierarchy was named-only, so it never read a
//! declared inverse or symmetry. The wedge reads them only with Layer A on, and
//! Layer A is off above its closure cap (`RUSTDL_CLASSIFY_ROLE_HIERARCHY_MAX_CLOSURE`).
//! On `ore_ont_9786` (SIO, above the cap) that left 13 entailments missed:
//!
//! * `A ⊑ ∀p.K` with `q ≡ p⁻` and `X ⊑ ∃s.A`, `s ⊑ q` entails `X ⊑ K` (1 pair).
//! * `X ⊑ ∃p.T`, `T ⊑ ∃s.B`, `s ≡ p⁻`, `s ⊑ f`, `Functional(f)` entails `X ⊑ B`:
//!   the `T`-witness has `x` and `b` as `f`-successors (12 pairs).
//!
//! Every test pins `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0` to reproduce the above-cap
//! condition; with Layer A on the wedge answers these small fixtures itself and
//! the saturator rules are not observable. Konclude agrees with every verdict
//! below (2026-10-11): it reports the three positives and stays silent on the two
//! FP guards, so its silence is a discriminating control.
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_inverse_rules`

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

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:A)) Declaration(Class(:K))
    Declaration(Class(:T)) Declaration(Class(:U)) Declaration(Class(:B))
    Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
    Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:f))
    Declaration(ObjectProperty(:g))
";

/// `X ⊑ ∃s.A`, `s ⊑ q`, `q ≡ p⁻`, `A ⊑ ∀p.K` ⟹ `X ⊑ K`. The `L ⊑ A ⊔ K` axiom
/// keeps the ontology off the pure-EL fast path, as on `ore_ont_9786`.
const FORALL: &str = "    SubClassOf(:A ObjectAllValuesFrom(:p :K))
    InverseObjectProperties(:p :q)
    SubObjectPropertyOf(:s :q)
    SubClassOf(:X ObjectSomeValuesFrom(:s :A))
    SubClassOf(:U ObjectUnionOf(:A :K))";

/// `X ⊑ ∃p.T`, `T ⊑ U ⊑ ∃s.B`, `s ≡ p⁻`, `s ⊑ f`, `Functional(f)` ⟹ `X ⊑ B`.
const MERGE: &str = "    InverseObjectProperties(:s :p)
    SubObjectPropertyOf(:s :f)
    FunctionalObjectProperty(:f)
    SubClassOf(:X ObjectSomeValuesFrom(:p :T))
    SubClassOf(:T :U)
    SubClassOf(:U ObjectSomeValuesFrom(:s :B))";

/// Classify with Layer A off and `extra` env settings, and report `X ⊑ sup`.
fn x_subsumed_by(body: &str, sup: &str, extra: &[(&'static str, &str)]) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _layer_a = SetEnvGuard::set("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0");
    let _flags: Vec<SetEnvGuard> = extra.iter().map(|(k, v)| SetEnvGuard::set(k, v)).collect();
    let src = format!("{PFX}Ontology(<http://t/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    c.is_subclass("http://t/X", &format!("http://t/{sup}"))
}

// ---------------------------------------------------------------- negatives

/// FP guard: a FORWARD `p`-edge into `∀p.K` says nothing about its source.
#[test]
fn forward_edge_into_forall_entails_nothing() {
    let body = "    SubClassOf(:A ObjectAllValuesFrom(:p :K))
    InverseObjectProperties(:p :q)
    SubClassOf(:X ObjectSomeValuesFrom(:p :A))
    SubClassOf(:U ObjectUnionOf(:A :K))";
    assert!(!x_subsumed_by(body, "K", &[]));
}

/// FP guard: two DIFFERENT functional roles force no equality.
#[test]
fn edges_on_two_functional_roles_do_not_merge() {
    let body = "    InverseObjectProperties(:p :f)
    FunctionalObjectProperty(:f)
    SubObjectPropertyOf(:s :g)
    FunctionalObjectProperty(:g)
    SubClassOf(:X ObjectSomeValuesFrom(:p :T))
    SubClassOf(:T ObjectSomeValuesFrom(:s :B))
    SubClassOf(:U ObjectUnionOf(:A :K))";
    assert!(!x_subsumed_by(body, "B", &[]));
}

// ---------------------------------------------------------------- positives

#[test]
fn forall_through_an_inverse_sub_role_is_derived() {
    assert!(x_subsumed_by(FORALL, "K", &[]));
}

#[test]
fn functional_merge_across_an_inverse_is_derived() {
    assert!(x_subsumed_by(MERGE, "B", &[]));
}

/// `InverseFunctional(g)`: `x g t` and `t s b` with `s ≡ g⁻` give `t` two
/// `g⁻`-successors.
#[test]
fn merge_through_inverse_functionality_is_derived() {
    let body = "    InverseObjectProperties(:s :g)
    InverseFunctionalObjectProperty(:g)
    SubClassOf(:X ObjectSomeValuesFrom(:g :T))
    SubClassOf(:T ObjectSomeValuesFrom(:s :B))
    SubClassOf(:U ObjectUnionOf(:A :K))";
    assert!(x_subsumed_by(body, "B", &[]));
}

// ------------------------------------------------- the flags are load-bearing

/// Each flag at `=0` loses exactly its own pair, so the positives above are
/// the rules' doing and not some other path's.
#[test]
fn each_flag_off_loses_its_own_entailment() {
    assert!(!x_subsumed_by(
        FORALL,
        "K",
        &[("RUSTDL_SAT_INVERSE_FORALL", "0")]
    ));
    assert!(x_subsumed_by(
        FORALL,
        "K",
        &[("RUSTDL_SAT_INVERSE_FUNC_MERGE", "0")]
    ));
    assert!(!x_subsumed_by(
        MERGE,
        "B",
        &[("RUSTDL_SAT_INVERSE_FUNC_MERGE", "0")]
    ));
    assert!(x_subsumed_by(
        MERGE,
        "B",
        &[("RUSTDL_SAT_INVERSE_FORALL", "0")]
    ));
}
