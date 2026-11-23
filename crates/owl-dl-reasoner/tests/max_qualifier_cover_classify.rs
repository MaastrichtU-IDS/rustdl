//! #554 at the classify level: a `≤n r.C` with a compound qualifier `C` names `C`
//! by a fresh `Q`, and the clausifier gave `Q` only `Q ⊑ C`, so the wedge did not
//! count an `r`-successor satisfying `C` and its trusted `Sat` kept unsatisfiable
//! classes and entailed subsumptions out of the hierarchy. Expected answers are
//! `HermiT`'s classification of `K`:
//!
//! - `A`, `B` and `E` unsat; `C ⊑ D`;
//! - `R`, `F`, `C` and `D` satisfiable.
//!
//! Sabotage: `RUSTDL_MAX_QUALIFIER_COVER=0` loses `C ⊑ D`
//! (`flag_off_reverts`); an over-strong cover (`Q` on every successor) makes `R`
//! unsat, which the satisfiable controls catch.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const NS: &str = "http://ex/#";

/// `A`: `≤0 r.(Q ⊔ P)` and an `r`-successor in `P`. `B`: `≤0` over a tautology
/// and any `r`-successor. `C`: `(≤0 r.(Q ⊔ P)) ⊔ D` with an `r`-successor in `Q`,
/// so `C ⊑ D`. `E`: `≤1` over a tautology with a `Q` and a `¬Q` successor. `R`:
/// the `≤0 r.(Q ⊔ P)` successor is outside `Q ⊔ P` (satisfiable). `F`: the `≤1
/// r.(Q ⊔ P)` successors merge into one in `Q ⊓ P` (satisfiable).
const K: &str = "\
     SubClassOf(:A ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P))) SubClassOf(:A ObjectSomeValuesFrom(:r :P)) \
     SubClassOf(:B ObjectMaxCardinality(0 :r ObjectUnionOf(:Q ObjectComplementOf(:Q)))) \
     SubClassOf(:B ObjectSomeValuesFrom(:r :R)) \
     SubClassOf(:C ObjectUnionOf(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P)) :D)) \
     SubClassOf(:C ObjectSomeValuesFrom(:r :Q)) \
     SubClassOf(:E ObjectMaxCardinality(1 :r ObjectUnionOf(:Q ObjectComplementOf(:Q)))) \
     SubClassOf(:E ObjectSomeValuesFrom(:r :Q)) \
     SubClassOf(:E ObjectSomeValuesFrom(:r ObjectComplementOf(:Q))) \
     SubClassOf(:R ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P))) SubClassOf(:R ObjectSomeValuesFrom(:r \
     ObjectIntersectionOf(ObjectComplementOf(:Q) ObjectComplementOf(:P)))) \
     SubClassOf(:F ObjectMaxCardinality(1 :r ObjectUnionOf(:Q :P))) SubClassOf(:F ObjectSomeValuesFrom(:r :Q)) \
     SubClassOf(:F ObjectSomeValuesFrom(:r :P))";

fn classify(body: &str) -> owl_dl_reasoner::Classification {
    let ofn = format!(
        "Prefix(:=<{NS}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/k>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F)) \
         Declaration(Class(:P)) Declaration(Class(:Q)) Declaration(Class(:R)) \
         Declaration(ObjectProperty(:r))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse ofn");
    owl_dl_reasoner::classify(&onto).expect("classify")
}

fn unsat(c: &owl_dl_reasoner::Classification) -> Vec<String> {
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .iter()
        .map(|s| s.trim_start_matches(NS).to_string())
        .collect();
    v.sort();
    v
}

fn sub(c: &owl_dl_reasoner::Classification, a: &str, b: &str) -> bool {
    c.is_subclass(&format!("{NS}{a}"), &format!("{NS}{b}"))
}

#[test]
fn satisfiable_controls() {
    let _g = lock();
    let c = classify(K);
    for name in ["C", "D", "F", "P", "Q", "R"] {
        assert!(
            !unsat(&c).contains(&name.to_string()),
            "{name} is satisfiable (HermiT)"
        );
    }
    assert!(!sub(&c, "F", "D"), "HermiT: F ⋢ D");
}

#[test]
fn complex_qualifier_unsat_and_entailment() {
    let _g = lock();
    let c = classify(K);
    assert_eq!(unsat(&c), ["A", "B", "E"], "HermiT: A, B and E unsat");
    assert!(sub(&c, "C", "D"), "HermiT: C ⊑ D");
}

#[test]
fn flag_off_reverts() {
    let _g = lock();
    let _off = SetEnvGuard::set("RUSTDL_MAX_QUALIFIER_COVER", "0");
    let c = classify(K);
    // The main tableau still finds the unsat classes; the wedge's trusted `Sat`
    // on the `C ⊑ D` pair is what drops the subsumption.
    assert!(
        !sub(&c, "C", "D"),
        "flag off should reproduce the #554 miss"
    );
}

/// #91's shape (`≤1 r.(B ⊓ C)` with `≥2 r.(B ⊓ C)`) is unsat, and with the cover
/// the wedge sees it on its own: `≥2` names its qualifier `Q₁ ⊑ B ⊓ C`, `≤1` names
/// its `B ⊓ C ⊑ Q₂`, so the two successors carry `Q₂` and must merge. Before #554
/// only `RUSTDL_COMPLEX_QUALIFIER_VERIFY`'s main-tableau probe found it.
#[test]
fn cover_alone_finds_issue_91_unsat() {
    let _g = lock();
    let body = "SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C))) \
                SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))";
    let _verify_off = SetEnvGuard::set("RUSTDL_COMPLEX_QUALIFIER_VERIFY", "0");
    assert_eq!(unsat(&classify(body)), ["A"], "HermiT/Konclude: A unsat");
    let _cover_off = SetEnvGuard::set("RUSTDL_MAX_QUALIFIER_COVER", "0");
    assert!(
        unsat(&classify(body)).is_empty(),
        "both off should reproduce #91"
    );
}
