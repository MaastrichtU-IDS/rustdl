//! Regression test for the PR #50 review Fix 1 (Important): with an
//! unbounded per-pair deadline (`deadline=None`, i.e. `--pair-timeout-ms 0`),
//! `PreparedOntology::pair_disjoint_with_deadline`'s `None`-deadline branch
//! used to call `self.decide(build_test_concept)?`, and `Self::decide` folds
//! an inconclusive `SearchVerdict::NodeCap` (`RUSTDL_MAX_NODES`) `None` into
//! `Some(true)` via `unwrap_or(true)`. `pair_disjoint_with_deadline` then
//! negated that to `Some(false)` ("not disjoint") and `disjoint_classes`'s
//! `None => incomplete = true` arm never fired — a NodeCap-capped probe was
//! silently reported as a complete, negative result.
//!
//! Pre-fix: this test's assertion `d.incomplete()` is `false` (BUG).
//! Post-fix: `pair_disjoint_with_deadline`'s `None`-deadline branch calls the
//! raw Option-returning `decide_raw`, so the `NodeCap` `None` propagates and
//! `disjoint_classes` correctly reports `incomplete() == true`.
//!
//! `RUSTDL_MAX_NODES` is read through a process-wide `OnceLock` (cached after
//! first read), so this env var must be set before ANY tableau search runs in
//! this process — this file is deliberately the ONLY test in its binary
//! (mirrors `tests/node_cap.rs`'s established pattern).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. This is the only
        // test in this binary (single-threaded), restored on Drop.
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

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (onto, _prefixes) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("fixture parses");
    onto
}

// `C ⊓ D` is satisfiable (no `DisjointClasses`, no clash), but each class
// self-generates an `:r`-successor (`C ⊑ ∃r.C`, `D ⊑ ∃r.D`), so the `C ⊓ D`
// tableau probe must create at least one new node beyond the anonymous test
// root before it can decide `Sat` — with `RUSTDL_MAX_NODES=1` that trips the
// live-node cap deterministically, before any verdict is reached.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/disjointnodecap>
  Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
)
";

#[test]
fn disjoint_classes_none_deadline_reports_incomplete_on_nodecap() {
    let _cap = SetEnvGuard::set("RUSTDL_MAX_NODES", "1");
    let onto = parse(SRC);
    // `pair_deadline = None` ⟹ `disjoint_classes` calls
    // `pair_disjoint_with_deadline(ci, cj, None)` internally — the exact
    // unbounded path Fix 1 targets.
    let d = owl_dl_reasoner::disjoint_classes(&onto, None).expect("disjoint_classes");
    assert!(
        d.incomplete(),
        "a NodeCap-capped C⊓D probe must be reported incomplete, got pairs={:?}",
        d.pairs()
    );
    // FP=0: a capped (inconclusive) probe must never be read as "disjoint".
    assert!(
        !d.pairs()
            .iter()
            .any(|(a, b)| a == "http://ex/#C" && b == "http://ex/#D"),
        "a NodeCap None must not be read as disjoint (FP)"
    );
}
