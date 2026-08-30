//! The PER-PAIR false-positive gate.
//!
//! ## Why this file exists
//!
//! `konclude_closure_diff.rs` — the FP=0 net — diffs **`classify`** closures.
//! Issue #76 was a REAL false positive (the SROIQ `≤n` rule committed to the
//! first merge pair and never reconsidered, so a clash on that arbitrary choice
//! reported a satisfiable concept unsatisfiable). It lived on the
//! `subclass` / `explain` / `is_subclass_of` **per-pair** path, which `classify`
//! never consults while `trust_sat` is on. The net was therefore *structurally
//! blind* to it and it shipped for months, on `pizza` — a flagship fixture the
//! net checks on every run and reported FP=0 for throughout.
//!
//! So "FP=0 corpus-wide" was, and still is, a claim about `classify` alone.
//! This file gates the other surface.
//!
//! ## How it gates it
//!
//! `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1` withdraws the wedge `Sat`-trust on
//! out-of-fragment ontologies, so every refuted pair falls through to the same
//! tableau the per-pair surface uses. Running the oracle diff under that flag
//! therefore audits the per-pair path with the existing net's machinery, rather
//! than reimplementing closure diffing (which `oracle_diff` owns).
//!
//! The flag is gated on `OutOfFragment`, so it is INERT on `PureEl`/`Horn`
//! inputs — `bibtex` is Horn and is deliberately not a case here, because a
//! fixture where the flag cannot engage would pad the gate without testing it.
//!
//! ## Sabotage-validated — re-run this after touching either flag
//!
//! With `RUSTDL_MAX_TRIAL_MERGE=0` (which reverts the #76 fix) this test FAILS:
//! pizza closure **501 vs oracle 499, FP=2**, naming
//! `Margherita ⊑ InterestingPizza` and `QuattroFormaggi ⊑ InterestingPizza` —
//! exactly the two pairs the #76 investigation identified, and exactly the
//! numbers the held #66 branch recorded before #76 landed. A gate that cannot
//! be made to fail is not a gate.
//!
//! Fixtures live under the gitignored `ontologies/`, so this is `#[ignore]`d and
//! **panics** when they are absent rather than passing vacuously — the failure
//! mode `family-stripped` demonstrates elsewhere in the suite.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use owl_dl_reasoner::oracle_diff::{aligned_closures, read_owx_verdict};
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `set_var` guard: restores the prior value on drop. Mirrors the pattern in
/// `adaptive_inconsistency_budget.rs`.
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

fn load(path: &Path) -> SetOntology<RcStr> {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    onto
}

/// `(label, ontology, oracle)`. Every case is REQUIRED: a missing fixture fails
/// the test with the path, because a skipped case is indistinguishable from a
/// passing one in the log.
fn cases() -> Vec<(&'static str, PathBuf, PathBuf)> {
    let base = Path::new("../../ontologies/real");
    ["pizza", "ro", "sulo"]
        .iter()
        .map(|n| {
            (
                *n,
                base.join(format!("{n}.ofn")),
                base.join(format!("konclude-input/{n}-classified.owx")),
            )
        })
        .collect()
}

#[test]
#[ignore = "needs the gitignored ontologies/real/{pizza,ro,sulo}.ofn + konclude-input/*-classified.owx; \
            runs classify with the wedge Sat-trust withdrawn, so it is slower than the plain net"]
fn per_pair_path_has_no_false_positives() {
    // Withdraw the wedge `Sat`-trust so refuted pairs reach the tableau — the
    // code path `subclass`/`is_subclass_of` use, and the one #76 was wrong on.
    let _guard = SetEnvGuard::set("RUSTDL_CLASSIFY_VERIFY_REFUTATIONS", "1");

    let mut offenders: Vec<String> = Vec::new();
    let mut checked: Vec<&str> = Vec::new();

    for (label, input, truth) in cases() {
        assert!(
            input.exists() && truth.exists(),
            "[pp-fp] REQUIRED fixture `{label}` is missing ({} or {}). \
             Fetch with ./scripts/fetch-real-ontologies.sh and generate the oracle with \
             scripts/konclude-oracle.sh — do NOT silently skip: a vacuous pass here reads \
             identically to a real one.",
            input.display(),
            truth.display(),
        );

        let onto = load(&input);
        let c = classify_top_down_with_timeout(&onto, Duration::from_millis(200))
            .unwrap_or_else(|e| panic!("classify {label}: {e:?}"));
        let verdict = read_owx_verdict(&truth).expect("read oracle verdict");
        let (rustdl, oracle) = aligned_closures(&c, &verdict);
        let fp: BTreeSet<_> = rustdl.difference(&oracle).cloned().collect();

        eprintln!(
            "[pp-fp] {label}: rustdl={} oracle={} FP={}",
            rustdl.len(),
            oracle.len(),
            fp.len()
        );
        for (s, t) in fp.iter().take(5) {
            eprintln!("[pp-fp]   FP: {s} ⊑ {t}");
        }
        if !fp.is_empty() {
            offenders.push(format!("{label} (FP={})", fp.len()));
        }
        checked.push(label);
    }

    eprintln!("[pp-fp] checked: {}", checked.join(", "));
    assert!(
        offenders.is_empty(),
        "[pp-fp] the PER-PAIR path reports subsumptions the oracle rejects: {}. \
         This is a soundness regression on `subclass`/`is_subclass_of`, which the \
         classify-only FP=0 net cannot see (that is why #76 shipped for months).",
        offenders.join(", "),
    );
}
