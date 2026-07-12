//! Task 3 (label-cache back-fold): injecting the entailed
//! `LabelOracle::Sat::derived_sups` into the class hierarchy via
//! `RUSTDL_CLASSIFY_BACKFOLD`.
//!
//! Two minimal repros of the galen `TibialTuberosity ⊑
//! TibialInterCondylarEminence` residual (see
//! `docs/known-limitations/galen-defined-class-monotonicity-residual.md` and
//! `docs/superpowers/specs/2026-07-12-label-cache-backfold-design.md`):
//!
//! - **told-filler**: `TT ≡ E ⊓ ∃g.Sub`, `TICE ≡ E ⊓ ∃g.Sup`, `Sub ⊑ Sup`
//!   (asserted, an EL-closure fact).
//! - **merge-derived-filler**: same, but `Sub ⊑ Sup` is NOT asserted or
//!   EL-derivable; it only holds via the functional/`≤1` merge across a
//!   declared inverse (`Sub ⊑ ∃f.M`, `f⁻ = g2`, `Functional(g2)`,
//!   `M ≡ ∃g2.Sup`) — the exact shape of the galen residual's filler
//!   subsumption.
//!
//! **Measured finding (2026-07-12), not the design doc's prediction:** both
//! fixtures already derive `TT ⊑ TICE` via `classify()` with the flag OFF —
//! the ordinary label-cache/hierarchy machinery (the defined-sup sweep's
//! label pass-through to `subsumes_via_tableau`, which resolves fast at this
//! tiny scale) closes both pairs before back-fold ever runs. This matches
//! `galen-defined-class-monotonicity-residual.md`'s own "Follow-up
//! diagnosis" section: **there is no small OFN fixture that reproduces the
//! real galen-scale gap** — that gap is scale/order-dependent inside the
//! actual 2748-class ontology, not a structural property of this abstract
//! pattern. So these two tests are regression/control tests (flag ON must
//! not break either pair), NOT a RED→GREEN demonstration of back-fold
//! specifically. The injection code path itself (add-if-entailed-and-not-
//! already-known, with the dedup/closure guard) is unit-tested directly —
//! with a hand-built `label_cache` where `derived_sups` is the ONLY channel
//! carrying the edge — in `crates/owl-dl-reasoner/src/classify.rs`'s
//! `inject_backfold_derived_sups_*` tests.
//!
//! IMPORTANT: `is_subclass_of`/`subclass` HANG on the real galen instance of
//! this pattern (the `¬TICE` verify direction is disjunctive and does not
//! converge at that scale — see the design doc §0). Only the `classify()`
//! path (the label-cache/hierarchy walk) is exercised here, per the task
//! brief.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

// Env-mutation plumbing: serialize RUSTDL_CLASSIFY_BACKFOLD against other
// env-mutating tests in this binary, restore on Drop. Mirrors the pattern in
// `funcmerge_inverse.rs` / `classify_inverse_domain.rs`.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one
        // test, serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }

    #[allow(unsafe_code)]
    fn unset(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: see above.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const TOLD_FILLER: &str = r"Prefix(:=<http://t/#>)
Ontology(
Declaration(Class(:E))
Declaration(Class(:Sub))
Declaration(Class(:Sup))
Declaration(Class(:TT))
Declaration(Class(:TICE))
Declaration(ObjectProperty(:g))
EquivalentClasses(:TT ObjectIntersectionOf(:E ObjectSomeValuesFrom(:g :Sub)))
EquivalentClasses(:TICE ObjectIntersectionOf(:E ObjectSomeValuesFrom(:g :Sup)))
SubClassOf(:Sub :Sup)
)
";

const MERGE_DERIVED_FILLER: &str = r"Prefix(:=<http://t/#>)
Ontology(
Declaration(Class(:E))
Declaration(Class(:Sub))
Declaration(Class(:Sup))
Declaration(Class(:M))
Declaration(Class(:TT))
Declaration(Class(:TICE))
Declaration(ObjectProperty(:g))
Declaration(ObjectProperty(:f))
Declaration(ObjectProperty(:g2))
EquivalentClasses(:TT ObjectIntersectionOf(:E ObjectSomeValuesFrom(:g :Sub)))
EquivalentClasses(:TICE ObjectIntersectionOf(:E ObjectSomeValuesFrom(:g :Sup)))
SubClassOf(:Sub ObjectSomeValuesFrom(:f :M))
InverseObjectProperties(:f :g2)
FunctionalObjectProperty(:g2)
EquivalentClasses(:M ObjectSomeValuesFrom(:g2 :Sup))
)
";

const NORMAL_ONTOLOGY: &str = r"Prefix(:=<http://t/#>)
Ontology(
Declaration(Class(:A))
Declaration(Class(:B))
Declaration(Class(:C))
SubClassOf(:A :B)
SubClassOf(:B :C)
)
";

fn load(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut cur, ParserConfiguration::default()).expect("parse OFN");
    onto
}

#[test]
fn classify_flag_on_told_filler_subsumption_holds() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_BACKFOLD", "1");
    let onto = load(TOLD_FILLER);
    let c = classify(&onto).expect("classify");
    assert!(
        c.is_subclass("http://t/#TT", "http://t/#TICE"),
        "expected TT ⊑ TICE (told-filler: Sub ⊑ Sup is an EL fact)"
    );
}

#[test]
fn classify_flag_on_merge_derived_filler_subsumption_holds() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_BACKFOLD", "1");
    let onto = load(MERGE_DERIVED_FILLER);
    let c = classify(&onto).expect("classify");
    assert!(
        c.is_subclass("http://t/#TT", "http://t/#TICE"),
        "expected TT ⊑ TICE (merge-derived filler: Sub ⊑ Sup only holds on \
         sat(TT)'s graph via the functional/≤1 merge across the declared \
         inverse f⁻=g2 — the galen residual pattern)"
    );
}

/// Pins the measured finding above: this pair is ALREADY closed by the
/// ordinary `classify()` machinery even with the back-fold flag OFF, at this
/// (small) scale. If a future change to the label-cache/defined-sup sweep
/// ever regresses this back to a MISS, this test — not the flag-ON one —
/// is the one that will catch it, since the flag-ON test alone can't
/// distinguish "closed by back-fold" from "closed anyway".
#[test]
fn classify_flag_off_merge_derived_filler_subsumption_already_holds() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::unset("RUSTDL_CLASSIFY_BACKFOLD");
    let onto = load(MERGE_DERIVED_FILLER);
    let c = classify(&onto).expect("classify");
    assert!(
        c.is_subclass("http://t/#TT", "http://t/#TICE"),
        "measured finding: TT ⊑ TICE already holds via classify() at this \
         scale even with RUSTDL_CLASSIFY_BACKFOLD unset — see this file's \
         module doc comment"
    );
}

#[test]
fn backfold_flag_off_sanity_classify_still_succeeds() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::unset("RUSTDL_CLASSIFY_BACKFOLD");
    let onto = load(NORMAL_ONTOLOGY);
    let c = classify(&onto).expect("classify");
    assert!(c.is_subclass("http://t/#A", "http://t/#B"));
    assert!(c.is_subclass("http://t/#A", "http://t/#C"));
    assert!(c.is_subclass("http://t/#B", "http://t/#C"));
}
