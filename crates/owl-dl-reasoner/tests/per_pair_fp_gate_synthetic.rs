//! The live control for the PER-PAIR false-positive gate (`per_pair_fp_gate.rs`).
//!
//! `per_pair_fp_gate.rs` audits the per-pair tableau path by classifying under
//! `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1`. On its curated fixtures, no known
//! sabotage makes it fail (see its "Sabotage" note), so on its own it is
//! unlicensed: a gate that cannot fail proves nothing.
//!
//! This file supplies the control. It is a separate file, not a second test in the
//! gate, because the gate sets process environment and relies on being its
//! binary's only test. It runs the same mechanism on the five-axiom #76 synthetic,
//! `crates/owl-dl-cli/tests/fixtures/laws/maxmerge_fp.ofn`:
//! - `A` has three `r`-successors, typed `M`, `T` and `PT`, with `M` and `PT`
//!   disjoint;
//! - `I ≡ ≥3 r`.
//!
//! `A ⊑ I` is NOT entailed. `T` may merge with `M` (or `PT`), leaving two
//! successors; HermiT refutes it. #76's bug committed to the first `≤n` merge
//! pair and never reconsidered, so a clash on that one choice made `A ⊓ ¬I`
//! look unsatisfiable and reported the false subsumption.
//!
//! The fixture is in-repo, so this test is NOT `#[ignore]`d and runs in CI.
//!
//! ## Sabotage (verified 2026-11-06 on main 3d0fb8b0)
//!
//! `RUSTDL_MAX_TRIAL_MERGE=0 RUSTDL_MAX_CHOICE_POINT=0` restores the first-pair
//! commit and makes this test FAIL. Either flag alone does not: the #293 merge
//! choice point superseded #76's trial merge, and each of the two still recovers
//! from the bad first choice when the other is off.
//!
//! Without `VERIFY_REFUTATIONS`, the same sabotage does NOT surface in `classify`:
//! the wedge's trusted `Sat` refutes the pair before the tableau runs. That is the
//! blindness that let #76 ship. The per-query commands (`subclass A I`,
//! `explain A I`) do answer `yes` under that sabotage. The cross-command law
//! harness's R1 (`scripts/cross-command-laws.py --sabotage`) and the generated-net
//! per-query laws cover them.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{FragmentClassification, classify_top_down_with_timeout};
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

const FIXTURE: &str = "../owl-dl-cli/tests/fixtures/laws/maxmerge_fp.ofn";
const A: &str = "http://ex.org/p#A";
const I: &str = "http://ex.org/p#I";

/// `set_var` guard: restores the prior value on drop. Mirrors `per_pair_fp_gate.rs`.
struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: `set_var` is unsafe under edition 2024 because it races with
        // concurrent readers. This test binary contains exactly ONE test, so no
        // other thread is reading the environment; the value is restored on drop.
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match self.prior.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[test]
fn per_pair_path_refutes_the_76_synthetic() {
    let _guard = SetEnvGuard::set("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS", "1");

    let path = Path::new(FIXTURE);
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

    let c = classify_top_down_with_timeout(&onto, Duration::from_millis(200))
        .unwrap_or_else(|e| panic!("classify {}: {e:?}", path.display()));

    // `VERIFY_REFUTATIONS` engages only out of fragment. If the fixture ever
    // classified as PureEl or Horn, the flag would be inert and this test vacuous.
    assert_eq!(
        c.stats().fragment,
        FragmentClassification::OutOfFragment,
        "the #76 synthetic must be out of fragment, or VERIFY_REFUTATIONS is inert here"
    );
    assert!(
        !c.is_subclass(A, I),
        "the per-pair path reports A ⊑ I on the #76 synthetic, which HermiT refutes: \
         the ≤n merge commits to its first pair again (#76)"
    );
}
