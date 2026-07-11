//! Scaling guard for the default-on incremental functional/`≤1` merge
//! (`RUSTDL_INVERSE_FUNC_MERGE`, default ON since 2026-07-11; see
//! `crates/owl-dl-tableau/src/lib.rs::inverse_func_merge_enabled` and
//! `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs` for the minimal
//! single-node fixture this generalizes).
//!
//! Builds a *ring* of K copies of the funcmerge-across-inverse pattern —
//! `A_i ⊑ ∃f.N_i`, `f ≡ inv(g)`, `Functional(g)`, `N_i ≡ ∃g.(Y_i ⊓ ∃h.LFC_i)`,
//! `Y_i ⊑ Z`, `LFC_i ≡ ∃g.A_{(i+1)%K}` — where the last axiom closes the ring
//! by feeding each node's `LFC` filler back into the *next* node's `A` class,
//! so the merge cascade chains all the way around instead of staying
//! node-local. `f`/`g`/`h`/`Z` are shared across the whole ring (single
//! declarations); only `A`/`N`/`Y`/`LFC` are per-index.
//!
//! The incremental merge (as opposed to the old whole-graph re-fire) is what
//! makes this fast: each `≤1`/functional merge in `horn_fixpoint` only
//! re-processes the delta at the folded node, not the whole completion
//! graph. This test guards against an `O(K^3)`-style regression creeping
//! back in — wall time per K is printed, and K=5/10/20 must all terminate
//! well within the test harness's default timeout.
//!
//! Like `funcmerge_inverse.rs`'s single-node fixture, each `Y_i` is a plain
//! `SubClassOf`-only class, so the `A_i ⊑ Z` pair (via the engine-derived,
//! same-tier `A_i ⊑ Y_i`) falls into the same same-tier/cross-tier blind
//! spot in `classify()`'s default top-down walk (see that file's doc
//! comment for the full explanation). This test opts into
//! `RUSTDL_CLASSIFY_SAME_TIER` for its duration, exactly mirroring that
//! precedent — confirmed empirically: all three K values FAIL at bare
//! default (no env at all) and PASS with the flag set.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::fmt::Write as _;
use std::time::Instant;

// Env-mutation plumbing: serialize RUSTDL_CLASSIFY_SAME_TIER against other
// env-mutating tests, restore on Drop. Mirrors the pattern in
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

/// Build a K-node ring of the funcmerge-across-inverse pattern as OFN text.
fn build_ring(k: usize) -> String {
    assert!(
        k >= 2,
        "ring needs at least 2 nodes to have a non-trivial cycle"
    );
    let mut s = String::new();
    s.push_str("Prefix(:=<http://ring/#>)\nOntology(\n");
    s.push_str("Declaration(Class(:Z))\n");
    s.push_str("Declaration(ObjectProperty(:f))\n");
    s.push_str("Declaration(ObjectProperty(:g))\n");
    s.push_str("Declaration(ObjectProperty(:h))\n");
    s.push_str("InverseObjectProperties(:f :g)\n");
    s.push_str("FunctionalObjectProperty(:g)\n");
    for i in 0..k {
        let j = (i + 1) % k;
        let _ = write!(
            s,
            "Declaration(Class(:A{i}))\n\
             Declaration(Class(:N{i}))\n\
             Declaration(Class(:Y{i}))\n\
             Declaration(Class(:LFC{i}))\n\
             SubClassOf(:A{i} ObjectSomeValuesFrom(:f :N{i}))\n\
             EquivalentClasses(:N{i} ObjectSomeValuesFrom(:g ObjectIntersectionOf(:Y{i} ObjectSomeValuesFrom(:h :LFC{i}))))\n\
             SubClassOf(:Y{i} :Z)\n\
             EquivalentClasses(:LFC{i} ObjectSomeValuesFrom(:g :A{j}))\n"
        );
    }
    s.push_str(")\n");
    s
}

fn load(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut cur, ParserConfiguration::default()).expect("parse OFN");
    onto
}

fn run_ring(k: usize) {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_SAME_TIER", "1");
    let src = build_ring(k);
    let onto = load(&src);
    let start = Instant::now();
    let c = classify(&onto).expect("classify");
    let wall = start.elapsed();
    println!("funcmerge_scaling: K={k} wall={wall:?}");
    for i in 0..k {
        let a = format!("http://ring/#A{i}");
        assert!(
            c.is_subclass(&a, "http://ring/#Z"),
            "expected A{i} ⊑ Z (ring K={k}, functional-merge-across-inverse)"
        );
    }
}

#[test]
fn funcmerge_ring_k5_terminates_and_derives_a_sub_z() {
    run_ring(5);
}

#[test]
fn funcmerge_ring_k10_terminates_and_derives_a_sub_z() {
    run_ring(10);
}

#[test]
fn funcmerge_ring_k20_terminates_and_derives_a_sub_z() {
    run_ring(20);
}
