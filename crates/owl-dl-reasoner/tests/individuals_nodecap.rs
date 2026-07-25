//! Regression test for the PR #50 review Fix 1 (Important), the
//! `pair_individuals_disjoint_with_deadline` sibling of
//! `tests/disjoint_nodecap.rs` (see that file's doc comment for the full
//! mechanism). With `deadline=None`, the pre-fix `None`-deadline branch
//! called `self.decide(...)`, whose `unwrap_or(true)` folded an inconclusive
//! `NodeCap` `None` into `Some(true)` ⟹ `Some(false)` ("not different"),
//! and `different_individuals`'s `None => incomplete = true` arm never fired.
//!
//! Pre-fix: `di.incomplete()` is `false` (BUG). Post-fix:
//! `pair_individuals_disjoint_with_deadline` uses `decide_raw`, so the
//! `NodeCap` `None` propagates and `incomplete()` is honestly `true`.
//!
//! `RUSTDL_MAX_NODES`'s process-wide `OnceLock` cache means this file must be
//! the ONLY test in its binary (see `tests/node_cap.rs` / `disjoint_nodecap.rs`).
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

// `a` and `b` are both `:C`-typed, and `:C` self-generates an `:r`-successor
// (`C ⊑ ∃r.C`) — so seeding `ClassAssertion(:C, a/b)` alone already forces the
// `{a} ⊓ {b}` probe's completion graph past 1 node before any verdict, which
// `RUSTDL_MAX_NODES=1` deterministically caps.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/individualsnodecap>
  Declaration(Class(:C)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  ClassAssertion(:C :a) ClassAssertion(:C :b)
)
";

#[test]
fn different_individuals_none_deadline_reports_incomplete_on_nodecap() {
    let _cap = SetEnvGuard::set("RUSTDL_MAX_NODES", "1");
    let onto = parse(SRC);
    // `pair_deadline = None` ⟹ `different_individuals` calls
    // `pair_individuals_disjoint_with_deadline(a, b, None)` internally.
    let di = owl_dl_reasoner::different_individuals(&onto, None).expect("different_individuals");
    assert!(
        di.incomplete(),
        "a NodeCap-capped {{a}}⊓{{b}} probe must be reported incomplete, got pairs={:?}",
        di.pairs()
    );
    // FP=0: a capped (inconclusive) probe must never be read as "different".
    assert!(
        !di.pairs()
            .iter()
            .any(|(a, b)| a == "http://ex/#a" && b == "http://ex/#b"),
        "a NodeCap None must not be read as different (FP)"
    );
}
