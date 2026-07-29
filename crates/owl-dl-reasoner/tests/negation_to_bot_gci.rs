//! End-to-end canaries for the `X ⊑ ¬Y` → `X ⊓ Y ⊑ ⊥` canonicalization.
//!
//! The rewrite is a logical equivalence, so these assert two things: the
//! entailments are unchanged, AND the ontology now reaches the saturation
//! fast path (which is the whole point — `ConceptExpr::Not` is rejected by
//! `is_el_concept` / `is_saturator_concept`).
//!
//! Run: `cargo test -p owl-dl-reasoner --test negation_to_bot_gci`

#![allow(clippy::unwrap_used, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Classification, FragmentClassification};
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

// All tests that call `classify` (directly or indirectly) must hold this lock
// for their entire body.  `flag_off_gives_identical_entailments` mutates
// `RUSTDL_NEG_TO_BOT_GCI`; without the lock the other four tests can race that
// window and observe the flag=0 state, causing spurious `OutOfFragment` failures.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard: sets `RUSTDL_NEG_TO_BOT_GCI` to `"0"` on construction and
/// restores the prior value (or removes the variable) on `Drop`.
///
/// SAFETY: `set_var`/`remove_var` is `unsafe` under edition 2024. All
/// mutations are serialised by `ENV_LOCK` and restored on `Drop` — including
/// on unwind.
struct NegGuard {
    prior: Option<std::ffi::OsString>,
}
impl NegGuard {
    fn off() -> Self {
        let prior = std::env::var_os("RUSTDL_NEG_TO_BOT_GCI");
        // SAFETY: serialised by ENV_LOCK (held by the caller); restored on Drop.
        unsafe { std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", "0") };
        Self { prior }
    }
}
impl Drop for NegGuard {
    fn drop(&mut self) {
        // SAFETY: see NegGuard::off.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var("RUSTDL_NEG_TO_BOT_GCI", v),
                None => std::env::remove_var("RUSTDL_NEG_TO_BOT_GCI"),
            }
        }
    }
}

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn classify(body: &str) -> Classification {
    owl_dl_reasoner::classify(&parse(body)).expect("classify")
}

fn unsat(c: &Classification) -> Vec<String> {
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    v.sort();
    v
}

/// `A ⊑ ¬B` + `C ⊑ A` + `C ⊑ B` ⟹ `C` unsat, AND the ontology reaches the
/// pure-EL fast path (before the rewrite the `Not` forced the hybrid path).
#[test]
fn atomic_negation_reaches_fast_path() {
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A ObjectComplementOf(:B))
    SubClassOf(:C :A)
    SubClassOf(:C :B)";
    let c = classify(body);
    assert_eq!(unsat(&c), vec!["http://t/C".to_string()]);
    assert_eq!(
        c.stats().fragment,
        FragmentClassification::PureEl,
        "atomic negation on a GCI RHS must no longer force the hybrid path"
    );
}

/// `X ⊑ ¬∃R.C` becomes `X ⊓ ∃R.C ⊑ ⊥` — in-fragment. Post-NNF it would have
/// become `X ⊑ ∀R.¬C`, which is out-of-fragment, so this test is what pins the
/// PRE-NNF placement of the pass.
#[test]
fn negated_existential_reaches_fast_path() {
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = "    Declaration(Class(:C))
    Declaration(Class(:X))
    Declaration(Class(:Y))
    Declaration(ObjectProperty(:R))
    SubClassOf(:X ObjectComplementOf(ObjectSomeValuesFrom(:R :C)))
    SubClassOf(:Y :X)
    SubClassOf(:Y ObjectSomeValuesFrom(:R :C))";
    let c = classify(body);
    assert_eq!(
        unsat(&c),
        vec!["http://t/Y".to_string()],
        "Y is both X (no R-successor in C) and has one"
    );
    assert_eq!(
        c.stats().fragment,
        FragmentClassification::PureEl,
        "¬∃R.C on a GCI RHS must lower to an EL-positive ⊥-GCI (pre-NNF placement)"
    );
}

/// `X ⊑ ¬(A ⊓ B)` becomes `X ⊓ A ⊓ B ⊑ ⊥`. Post-NNF it would be `¬A ⊔ ¬B`,
/// an `Or` — the second pin on the pre-NNF placement.
#[test]
fn negated_conjunction_reaches_fast_path() {
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:X))
    Declaration(Class(:Y))
    SubClassOf(:X ObjectComplementOf(ObjectIntersectionOf(:A :B)))
    SubClassOf(:Y :X)
    SubClassOf(:Y :A)
    SubClassOf(:Y :B)";
    let c = classify(body);
    assert_eq!(unsat(&c), vec!["http://t/Y".to_string()]);
    assert_eq!(c.stats().fragment, FragmentClassification::PureEl);
}

/// FP GUARD (negatives-first). The rewrite must not invent entailments: a class
/// carrying only ONE side of the negation stays satisfiable, and no spurious
/// subsumption appears.
#[test]
fn negation_rewrite_does_not_over_derive() {
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:D))
    SubClassOf(:A ObjectComplementOf(:B))
    SubClassOf(:D :A)";
    let c = classify(body);
    assert!(unsat(&c).is_empty(), "nothing is unsatisfiable here");
    assert!(
        !c.is_subclass("http://t/D", "http://t/B"),
        "D ⊑ B must NOT hold"
    );
}

/// FLAG IDENTITY. Entailments must be identical with the lever off — only the
/// engine that answers may differ. Serialised because it mutates the process env.
#[test]
fn flag_off_gives_identical_entailments() {
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let body = "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A ObjectComplementOf(:B))
    SubClassOf(:C :A)
    SubClassOf(:C :B)";

    let on = unsat(&classify(body));

    let _env = NegGuard::off();
    let off = unsat(&classify(body));
    // _env is dropped here, restoring the prior value before the lock is released.

    assert_eq!(on, off, "the rewrite is a logical equivalence");
}
