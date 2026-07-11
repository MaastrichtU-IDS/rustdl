//! Regression: functional (≤1) role merge across an inverse-induced edge, in a
//! cyclic model. Konclude derives A ⊑ Y (⊑ Z); rustdl missed it because the
//! wedge's ≤n-successor count ignored inverse-induced successors. See
//! docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md.
//!
//! Post-fix, the wedge itself derives `A ⊑ Y` (verified directly via
//! `distinct_role_succ`/`root_labels` and via `rustdl explain`). But this
//! minimal 5-class fixture's `Y` is a plain `SubClassOf`-only class (not an
//! `EquivalentClasses`-defined one), so `classify()`'s default top-down
//! walk — which places classes into tiers by EL-closure-subsumer count and
//! only recovers same/cross-tier engine-derived subsumptions for classes
//! that are either EL-closure-seeded or `EquivalentClasses`-defined (the
//! "defined-sup sweep") — never tests the `A ⊑ Y` pair: `A`'s (artificially
//! low, EL-blind) tier is processed before `Y`'s tier, so `Y` isn't yet a
//! placed candidate when `A`'s parents are searched. This is the same
//! same-tier/cross-tier blind spot documented and deliberately opt-in as
//! `RUSTDL_CLASSIFY_SAME_TIER` (see
//! `crates/owl-dl-reasoner/tests/classify_inverse_domain.rs`, whose
//! `default_classify_off_misses_inverse_domain_subsumption` pins the exact
//! same default-OFF behavior as *intended*, not a bug). So this test opts
//! in for its duration, exactly mirroring that precedent, to exercise the
//! wedge fix end-to-end through `classify()`.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

// Env-mutation plumbing: serialize RUSTDL_CLASSIFY_SAME_TIER against other
// env-mutating tests, restore on Drop. Mirrors the pattern in
// `classify_inverse_domain.rs` / `inverse_symmetric_domain.rs`.
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

const FUNCMERGE_CYCLIC: &str = r"Prefix(:=<http://t/#>)
Ontology(
Declaration(Class(:A))
Declaration(Class(:N))
Declaration(Class(:Y))
Declaration(Class(:Z))
Declaration(Class(:LFC))
Declaration(ObjectProperty(:f))
Declaration(ObjectProperty(:g))
Declaration(ObjectProperty(:h))
SubClassOf(:A ObjectSomeValuesFrom(:f :N))
InverseObjectProperties(:f :g)
FunctionalObjectProperty(:g)
EquivalentClasses(:N ObjectSomeValuesFrom(:g ObjectIntersectionOf(:Y ObjectSomeValuesFrom(:h :LFC))))
SubClassOf(:Y :Z)
EquivalentClasses(:LFC ObjectSomeValuesFrom(:g :A))
)
";

fn load(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut cur, ParserConfiguration::default()).expect("parse OFN");
    onto
}

#[test]
fn funcmerge_cyclic_derives_a_sub_y() {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_SAME_TIER", "1");
    let onto = load(FUNCMERGE_CYCLIC);
    let c = classify(&onto).expect("classify");
    // A ⊑ Y by the functional merge across the inverse edge; A ⊑ Z since Y ⊑ Z.
    assert!(
        c.is_subclass("http://t/#A", "http://t/#Y"),
        "expected A ⊑ Y (functional-merge-across-inverse)"
    );
    assert!(
        c.is_subclass("http://t/#A", "http://t/#Z"),
        "expected A ⊑ Z (A ⊑ Y ⊑ Z)"
    );
}
