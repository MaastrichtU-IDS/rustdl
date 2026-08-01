//! Canaries for `⊥` as an existential **filler** (`RUSTDL_EL_BOT_FILLER`, default OFF).
//!
//! THE BUG (D10 class — the gate certifies the closure complete while the engine
//! drops the axiom). `is_el_concept` (`classify.rs`) has a `Bot` arm and recurses
//! through `Some(role, body)` unrestricted, so `X ⊑ ∃r.⊥` is certified
//! `pure-EL — saturator alone is complete`. The saturator's existential-body
//! lowering had no `Bot` arm: it fell to `_ => return None` and the caller dropped
//! the whole axiom. rustdl reported `X` satisfiable with `incomplete: false`, while
//! **Konclude and `HermiT` both report `X ≡ owl:Nothing`** (verified 2026-08-01 on
//! every positive fixture below).
//!
//! NEGATIVES FIRST. Four of the ten tests are cases that must stay UNCHANGED — the
//! LHS occurrence `∃r.⊥ ⊑ Y` (vacuously true, entails nothing), a plain satisfiable
//! `∃r.A`, `≥0 r.⊥` (which is `⊤`, not `⊥`), and `∀r.⊥` (satisfied by having no
//! successor). Those are the arms `concept_is_provably_bot` must REFUSE; an
//! over-eager detector would make them unsat, which WOULD be a false positive.
//!
//! Run: `cargo test -p owl-dl-reasoner --test el_bot_filler`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Mirrors classify_defined_sweep.rs.
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

const DECLS: &str = "    Declaration(Class(:X))
    Declaration(Class(:A))
    Declaration(Class(:Y))
    Declaration(ObjectProperty(:r))
    Declaration(ObjectProperty(:s))
";

fn unsat_of(body: &str) -> Vec<String> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{DECLS}{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

/// Classify `body` with the lever ON.
fn unsat_on(body: &str) -> Vec<String> {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_EL_BOT_FILLER", "1");
    unsat_of(body)
}

/// Classify `body` with the lever explicitly OFF (the shipped default).
fn unsat_off(body: &str) -> Vec<String> {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_EL_BOT_FILLER", "0");
    unsat_of(body)
}

fn x() -> Vec<String> {
    vec!["http://t/X".to_string()]
}

// ---------------------------------------------------------------- negatives

/// NEGATIVE. `∃r.⊥ ⊑ Y` is vacuously true — nothing can have an `r`-edge to a
/// `⊥`-instance, so the axiom entails nothing. `X ⊑ ∃r.A` must stay satisfiable.
/// If the lever ever marks `X` (or `Y`) unsat here, it is emitting a FALSE
/// POSITIVE, not closing a miss.
#[test]
fn lhs_bot_existential_entails_nothing() {
    let body = "    SubClassOf(ObjectSomeValuesFrom(:r owl:Nothing) :Y)
    SubClassOf(:X ObjectSomeValuesFrom(:r :A))";
    assert!(unsat_on(body).is_empty(), "LHS ∃r.⊥ must entail nothing");
    assert_eq!(unsat_on(body), unsat_off(body));
}

/// NEGATIVE. A plain satisfiable existential must be untouched.
#[test]
fn plain_existential_stays_satisfiable() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:r :A))";
    assert!(unsat_on(body).is_empty());
    assert_eq!(unsat_on(body), unsat_off(body));
}

/// NEGATIVE — the `∀` boundary, **nested so it actually reaches the lowering**.
/// `∀s.⊥` is satisfied by having no `s`-successor, so `∃r.∀s.⊥` is satisfiable.
/// `concept_is_provably_bot` must refuse the `All` arm.
///
/// WHY NESTED. A top-level `X ⊑ ∀r.⊥` never reaches this code: `∀` leaves the EL
/// fragment, the ontology routes to the hybrid tableau, and the saturator has no
/// lowering for a bare `∀` RHS — so the flat spelling passes even with the
/// predicate deliberately broken (measured: sabotage S4 survived it). Under an
/// `∃` the filler DOES go through the chokepoint, and a wrong `true` here would
/// mark `X` unsat: a **false positive**, the one direction FP=0 forbids.
#[test]
fn forall_under_existential_is_not_bot() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:r ObjectAllValuesFrom(:s owl:Nothing)))";
    assert!(
        unsat_on(body).is_empty(),
        "∃r.∀s.⊥ is satisfiable — the r-successor simply has no s-successor"
    );
}

/// NEGATIVE — the `≤n` boundary, same nesting rationale. `≤2 s.⊥` is satisfied by
/// having no `s`-successor at all, so `∃r.≤2 s.⊥` is satisfiable. A `Max` arm in
/// the predicate would be a false positive.
///
/// (The dual `≥0 s.⊥` cannot be tested through the parser: `ConceptPool::min`
/// folds `≥0 r.C` to `⊤` at intern time, so `Min(0, ..)` never exists in the pool.
/// The `*n >= 1` guard in `concept_is_provably_bot` is therefore defensive, and is
/// pinned by the unit test in `owl-dl-saturation` instead of from here.)
#[test]
fn max_under_existential_is_not_bot() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:r ObjectMaxCardinality(2 :s owl:Nothing)))";
    assert!(
        unsat_on(body).is_empty(),
        "∃r.(≤2 s.⊥) is satisfiable — the r-successor has no s-successor"
    );
}

// ---------------------------------------------------------------- positives
// Oracle for all four: Konclude v0.7.0 AND HermiT 1.4.3 agree the subject is
// equivalent to owl:Nothing (run 2026-08-01).

/// THE REPRODUCER (review R2 fixture `p1`). `X ⊑ ∃r.⊥` ⟹ `X ⊑ ⊥`.
#[test]
fn bot_filler_derives_unsat() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:r owl:Nothing))";
    assert_eq!(unsat_on(body), x(), "X ⊑ ∃r.⊥ entails X ⊑ ⊥");
    assert!(unsat_off(body).is_empty(), "flag OFF keeps the old miss");
}

/// Conjunctive filler (review R2 fixture `p4`): `X ⊑ ∃r.(A ⊓ ⊥)`.
///
/// NOTE this does NOT exercise the predicate's `And` arm — `ConceptPool::and`
/// short-circuits on the `⊥` annihilator, so this interns as plain `∃r.⊥`. It is
/// kept because it is the review's published reproducer and pins the *user-visible*
/// spelling; `and_with_bot_existential_operand_derives_unsat` is what guards the
/// `And` arm.
#[test]
fn bot_inside_conjunctive_filler_derives_unsat() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A owl:Nothing)))";
    assert_eq!(unsat_on(body), x());
    assert!(unsat_off(body).is_empty());
}

/// The REACHABLE `And` arm: `X ⊑ ∃q.(A ⊓ ∃r.⊥)`. The inner `∃r.⊥` is not the
/// literal `⊥` at intern time, so `ConceptPool::and` keeps it as an operand and
/// the predicate must find it by recursing into the conjunction.
#[test]
fn and_with_bot_existential_operand_derives_unsat() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:s ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r owl:Nothing))))";
    assert_eq!(unsat_on(body), x(), "A ⊓ ∃r.⊥ is empty, so ∃s.(…) is empty");
    assert!(unsat_off(body).is_empty());
}

/// The REACHABLE `Min` arm: `X ⊑ ∃s.(≥2 r.⊥)`. Two witnesses in `⊥` is still
/// impossible, so the filler is empty.
#[test]
fn min_bot_under_existential_derives_unsat() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:s ObjectMinCardinality(2 :r owl:Nothing)))";
    assert_eq!(unsat_on(body), x(), "≥2 r.⊥ is empty");
}

/// NESTED (`∃r.∃s.⊥`). This is the case a `Bot`-match-arm-only fix would MISS:
/// the inner existential otherwise lowers to a ONE-WAY marker, which does not
/// carry the emptiness. Guards against the half-fix.
#[test]
fn nested_bot_filler_derives_unsat() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Nothing)))";
    assert_eq!(unsat_on(body), x(), "∃r.∃s.⊥ ≡ ⊥");
    assert!(unsat_off(body).is_empty());
}

/// `EquivalentClasses` sufficient direction (review R2 fixture `q1`).
#[test]
fn equivalent_to_bot_filler_derives_unsat() {
    let body = "    EquivalentClasses(:X ObjectSomeValuesFrom(:r owl:Nothing))";
    assert_eq!(unsat_on(body), x());
    assert!(unsat_off(body).is_empty());
}

/// SPELLING DIFFERENTIAL — the direct gate. `X ⊑ ∃r.⊥` and the dynamic
/// equivalent `X ⊑ ∃r.C` + `C ⊑ ⊥` (which ALREADY worked before this lever)
/// must produce the same closure once the lever is on.
#[test]
fn syntactic_bot_matches_dynamic_bot_spelling() {
    let syntactic = "    SubClassOf(:X ObjectSomeValuesFrom(:r owl:Nothing))";
    let dynamic = "    Declaration(Class(:C))
    SubClassOf(:X ObjectSomeValuesFrom(:r :C))
    SubClassOf(:C owl:Nothing)";
    let mut dyn_unsat = unsat_on(dynamic);
    dyn_unsat.retain(|c| c != "http://t/C");
    assert_eq!(
        unsat_on(syntactic),
        dyn_unsat,
        "the two spellings of an empty filler must agree"
    );
}

/// The propagation must reach SUBCLASSES too (`Z ⊑ X ⊑ ∃r.⊥`).
#[test]
fn bot_filler_propagates_to_subclasses() {
    let body = "    Declaration(Class(:Z))
    SubClassOf(:X ObjectSomeValuesFrom(:r owl:Nothing))
    SubClassOf(:Z :X)";
    assert_eq!(
        unsat_on(body),
        vec!["http://t/X".to_string(), "http://t/Z".to_string()]
    );
}
