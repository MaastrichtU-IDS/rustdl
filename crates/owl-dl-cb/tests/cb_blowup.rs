//! Adversarial ∀-rich disjunctive ALCH blowup baseline.
//!
//! Characterises the `∏ᵢ|supports(pᵢ)|` cross-product blowup that the
//! un-tamed S1 (Sequoia ordered) engine exhibits on ALCH ontologies with:
//! - multiple `∀R.(Aᵢ ⊔ Bᵢ)` GCIs (pairwise distinct atoms, cross-disjoint)
//!   back-propagated into a single successor context via a `∃R.⊤` existential
//! - the resulting disjunctive heads `{Aᵢ, Bᵢ}` form a subset-antichain
//!   (each pair incomparable under ⊆), so S1's Elim rule cannot collapse them
//!   and they accumulate multiplicatively
//!
//! The generator `adversarial(k)` uses `2k` atoms split into `k` pairs
//! `(A0,B0), …, (Ak-1,Bk-1)`, pairwise disjoint within each pair and also
//! cross-pair, with `∀R.(Ai⊔Bi)` for each `i`. An existential `C ⊑ ∃R.⊤`
//! forces a successor context. The Succ rule spawns a context for the ⊤-filler;
//! the R∀ rule back-propagates each `∀R.(Ai⊔Bi)` into that context, giving `k`
//! independent binary disjunctions whose heads are pairwise incomparable. S1's
//! Hyper rule resolves the maximal disjunct of each, but the incomparability
//! means it must track all combinations → ≈ 2^k derived clauses.
//!
//! ## Acceptance (Task 2 of the CB-ALCH taming plan)
//! - `agreement_on_tiny` (non-ignored): both engines agree on `adversarial(2)`
//!   — confirms the generator produces valid in-fragment ALCH, not garbage.
//! - `s1_blows_up_on_adversarial` (ignored): S1 does NOT finish in 30 s on
//!   `adversarial(N_BLOWUP)`. Smallest confirmed `N_BLOWUP` = 13.
//!
//! Wall-time sweep (debug build, 32-core/251GB Linux, 2026-07-28):
//! ```text
//! n= 4 →   6 ms
//! n= 5 →  17 ms
//! n= 6 →  44 ms
//! n= 7 → 114 ms
//! n= 8 → 298 ms
//! n= 9 → 790 ms
//! n=10 →   2.1 s
//! n=11 →   5.6 s
//! n=12 →  15.4 s
//! n=13 → TIMEOUT (>35 s)  ← N_BLOWUP
//! ```
//! Doubling time ≈ 2.5–2.7× per step (consistent with 2^k antichain growth).
//!
//! Run the blowup baseline explicitly:
//! ```text
//! cargo test -p owl-dl-cb --test cb_blowup -- --ignored s1_blows_up_on_adversarial
//! ```

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_cb::{CbOutcome, classify_sequoia, classify_unordered};
use owl_dl_core::convert::convert_ontology;
use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

// ── Smallest confirmed N that causes S1 to blow up (>30 s) ─────────────────
// Measured 2026-07-28 on a 32-core/251GB Linux host (debug build).
// n=12 finishes in ~15 s; n=13 exceeds 35 s.
// Adjust upward if future hardware/engine changes make it terminate sooner.
const N_BLOWUP: usize = 13;

// ── Generator ────────────────────────────────────────────────────────────────

/// Build an adversarial ∀-rich disjunctive ALCH ontology parameterised by
/// `n_pairs`. Uses `2·n_pairs` atoms split into `n_pairs` pairs
/// `(A0,B0), …, (A{n-1},B{n-1})`, all pairwise disjoint (including cross-pair).
///
/// Pattern:
/// - All `2n` atoms declared.
/// - Pairwise disjointness for every distinct pair `(Xi, Xj)`:
///   `SubClassOf(ObjectIntersectionOf(:Xi :Xj) owl:Nothing)`
///   (the two-premise empty-head form the order's dead-tier reads).
/// - One named existential root: `SubClassOf(:C ObjectSomeValuesFrom(:R owl:Thing))`
///   This forces a successor context cored at `{⊤}` via the Succ rule.
/// - `n_pairs` universal axioms:
///   `SubClassOf(owl:Thing ObjectAllValuesFrom(:R ObjectUnionOf(:Ai :Bi)))`
///   R∀ back-propagates each pair's disjunction into the successor context.
///   The `k` resulting clauses `{Ai,Bi}` form a subset-antichain (every pair
///   incomparable under ⊆), so Elim cannot collapse them → ≈ 2^k combinations.
/// - `:C` declared to anchor the existential and make the ontology non-trivial.
pub(crate) fn adversarial(n_pairs: usize) -> owl_dl_core::ontology::InternalOntology {
    let mut decls = String::new();
    let mut disjointness = String::new();
    let mut universals = String::new();

    // Declarations: A0, B0, A1, B1, …, A{n-1}, B{n-1}, C, and the role R.
    for i in 0..n_pairs {
        let _ = writeln!(decls, "    Declaration(Class(:A{i}))");
        let _ = writeln!(decls, "    Declaration(Class(:B{i}))");
    }
    decls.push_str("    Declaration(Class(:C))\n");
    decls.push_str("    Declaration(ObjectProperty(:R))\n");

    // Collect all atom names for pairwise disjointness.
    let mut atoms: Vec<String> = Vec::new();
    for i in 0..n_pairs {
        atoms.push(format!(":A{i}"));
        atoms.push(format!(":B{i}"));
    }

    // Pairwise disjointness (all N*(N-1)/2 pairs): SubClassOf(A⊓B, ⊥).
    for i in 0..atoms.len() {
        for j in (i + 1)..atoms.len() {
            let _ = writeln!(
                disjointness,
                "    SubClassOf(ObjectIntersectionOf({} {}) owl:Nothing)",
                atoms[i], atoms[j]
            );
        }
    }

    // Existential: C ⊑ ∃R.⊤  (forces a successor context cored at {⊤}).
    let existential = "    SubClassOf(:C ObjectSomeValuesFrom(:R owl:Thing))\n";

    // Universal axioms: ⊤ ⊑ ∀R.(Ai⊔Bi) for each pair i.
    // owl:Thing on the LHS normalises to ⊤ in the clause form.
    for i in 0..n_pairs {
        let _ = writeln!(
            universals,
            "    SubClassOf(owl:Thing ObjectAllValuesFrom(:R ObjectUnionOf(:A{i} :B{i})))"
        );
    }

    let body = format!("{decls}{disjointness}{existential}{universals}");
    parse(&body)
}

/// Parse an OFN body string into an `InternalOntology`. Copied verbatim from
/// `cb_sequoia_diff.rs` (the established test-helper pattern for this crate).
fn parse(body: &str) -> owl_dl_core::ontology::InternalOntology {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("OFN parse error");
    convert_ontology(&onto).expect("convert_ontology error")
}

/// Spawn `f` on a background thread and wait up to `timeout` for it to finish.
/// Returns `Some(elapsed)` if it finished within the deadline, `None` if it
/// was still running. The worker thread is deliberately leaked on timeout so
/// the test suite process can exit cleanly.
///
/// Used by Task 2 (blowup baseline) and exposed for Task 3/5 (taming regression).
pub(crate) fn run_with_timeout(
    f: impl FnOnce() + Send + 'static,
    timeout: Duration,
) -> Option<Duration> {
    let (tx, rx) = std::sync::mpsc::channel();
    let t = Instant::now();
    std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    rx.recv_timeout(timeout).ok().map(|()| t.elapsed())
}

/// Run `classify_sequoia(&onto)` on a background thread with a wall-clock
/// deadline, returning `Some((elapsed, outcome))` if it finished in time and
/// `None` if it was still running (a leaked worker; process exits cleanly).
/// Unlike [`run_with_timeout`] this carries the `CbOutcome` back so the caller
/// can self-check the verdict (Gate 2 needs C-unsat, not just termination).
fn sequoia_with_timeout(
    onto: owl_dl_core::ontology::InternalOntology,
    timeout: Duration,
) -> Option<(Duration, CbOutcome)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let t = Instant::now();
    std::thread::spawn(move || {
        let out = classify_sequoia(&onto);
        let _ = tx.send(out);
    });
    rx.recv_timeout(timeout).ok().map(|out| (t.elapsed(), out))
}

// ── Sanity / agreement test (non-ignored — runs in CI) ────────────────────

/// Both B1 (unordered, directly complete) and S1 (ordered) must agree on a
/// tiny adversarial instance. This confirms:
/// 1. The generator produces valid in-fragment ALCH (not garbage / OOF).
/// 2. S1 is not unsound on this pattern at small scale.
/// 3. Both engines terminate fast on the tiny instance.
///
/// `n_pairs=2` → 4 atoms, 6 pairwise disjointness axioms, 2 universals.
#[test]
fn agreement_on_tiny() {
    // Default-mode S1 call: fence against flag-ON tests in this binary.
    let _serial = env_serial();
    let internal = adversarial(2);

    let b1_out = classify_unordered(&internal);
    let s1_out = classify_sequoia(&internal);

    let b1_hier = match b1_out {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(reason) => {
            panic!("B1 returned OutOfFragment on adversarial(2): {reason}");
        }
    };
    let s1_hier = match s1_out {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(reason) => {
            panic!("S1 returned OutOfFragment on adversarial(2): {reason}");
        }
    };

    let fp: Vec<_> = s1_hier
        .subsumptions
        .difference(&b1_hier.subsumptions)
        .collect();
    let missed: Vec<_> = b1_hier
        .subsumptions
        .difference(&s1_hier.subsumptions)
        .collect();
    let unsat_fp: Vec<_> = s1_hier.unsat.difference(&b1_hier.unsat).collect();
    let unsat_missed: Vec<_> = b1_hier.unsat.difference(&s1_hier.unsat).collect();

    assert!(
        fp.is_empty(),
        "adversarial(2): S1 has FALSE POSITIVES vs B1: {fp:?}"
    );
    assert!(
        missed.is_empty(),
        "adversarial(2): S1 MISSED subsumptions vs B1: {missed:?}"
    );
    assert!(
        unsat_fp.is_empty(),
        "adversarial(2): S1 unsat FP: {unsat_fp:?}"
    );
    assert!(
        unsat_missed.is_empty(),
        "adversarial(2): S1 unsat MISSED: {unsat_missed:?}"
    );
}

// ── Blowup baseline (ignored — must not hang CI) ───────────────────────────

/// Baseline: S1 (`classify_sequoia`) does NOT finish within 30 s on
/// `adversarial(N_BLOWUP)`.
///
/// Smallest confirmed `N_BLOWUP` = 13 (32-core/251GB Linux host, debug build,
/// 2026-07-28). Wall-time sweep shows ~2.5–2.7× growth per step (n=12 → 15 s,
/// n=13 → TIMEOUT at 35 s), consistent with the ≈ 2^k antichain clause
/// accumulation.
///
/// The pattern: `k=N_BLOWUP` independent binary disjunctions `{Ai,Bi}` in the
/// successor context form a subset-antichain, so Elim cannot collapse any pair.
/// S1's Hyper rule must resolve the maximal disjunct from each independently,
/// producing exponentially many derived clauses (≈ 2^k combinations).
///
/// Run:
/// ```text
/// cargo test -p owl-dl-cb --test cb_blowup -- --ignored s1_blows_up_on_adversarial
/// ```
#[test]
#[ignore = "baseline: S1 expected to hang; run explicitly to verify the blowup reproduces"]
fn s1_blows_up_on_adversarial() {
    // Default-mode baseline: fence against flag-ON tests (also ensures the flag
    // is in its default-unset state for this measurement).
    let _serial = env_serial();
    let o = adversarial(N_BLOWUP);
    let done = Arc::new(AtomicBool::new(false));
    let done2 = Arc::clone(&done);

    let finished = run_with_timeout(
        move || {
            let _ = classify_sequoia(&o);
            done2.store(true, Ordering::SeqCst);
        },
        Duration::from_secs(30),
    );

    assert!(
        finished.is_none(),
        "BASELINE BROKEN: S1 finished adversarial({N_BLOWUP}) in {:?}. \
        Either the blowup no longer reproduces on this hardware/engine, \
        or N_BLOWUP must be increased. Reassess before taming.",
        finished.unwrap_or_default()
    );
    // Worker thread is deliberately leaked; the test process exits cleanly.
}

// ── Candidate-1 taming regression (Task 3, Gate 2) ─────────────────────────

/// Serializes every test in this binary that reads OR writes
/// `RUSTDL_CB_SECOND_MAXIMAL` (cargo runs `#[test]` fns concurrently within one
/// process; `classify_sequoia` snapshots the flag via `var_os` at
/// `OrderBuilder::build`). Both flag-ON tests and the default-mode tests take
/// this lock so none observes another's env mutation. Mirrors the reasoner
/// crate's `ENV_MUTEX` prior art.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take `ENV_MUTEX` (no mutation) so a default-mode `classify_*` test cannot
/// run concurrently with a flag-ON test.
fn env_serial() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// RAII guard: set `RUSTDL_CB_SECOND_MAXIMAL=1` for its lifetime, restore on
/// drop (panic-safe). Holds `ENV_MUTEX` for its whole lifetime. Snapshotted per
/// `classify_*` call at `OrderBuilder::build`.
struct SecondMaximalGuard {
    prev: Option<std::ffi::OsString>,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl SecondMaximalGuard {
    #[allow(unsafe_code)]
    fn set() -> Self {
        let serial = env_serial();
        let prev = std::env::var_os("RUSTDL_CB_SECOND_MAXIMAL");
        // SAFETY: set_var is unsafe under edition 2024. Held only for one test,
        // serialized via ENV_MUTEX, restored on Drop.
        unsafe { std::env::set_var("RUSTDL_CB_SECOND_MAXIMAL", "1") };
        Self {
            prev,
            _serial: serial,
        }
    }
}

impl Drop for SecondMaximalGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var("RUSTDL_CB_SECOND_MAXIMAL", v),
                None => std::env::remove_var("RUSTDL_CB_SECOND_MAXIMAL"),
            }
        }
    }
}

/// Resolve `:C`'s `ClassId` and assert it is reported unsatisfiable in `h`.
/// (The complete oracle B1 reports C in `.unsat` on this pattern — validated
/// against `adversarial(3)` in [`ground_truth_c_unsat_small`].)
fn assert_c_unsat(internal: &owl_dl_core::ontology::InternalOntology, h: &owl_dl_cb::CbHierarchy) {
    let c = internal
        .vocabulary
        .class_id("http://t/C")
        .expect(":C must be interned");
    assert!(
        h.unsat.contains(&c),
        "C must be reported unsatisfiable (C ⊑ owl:Nothing); unsat set = {:?}",
        h.unsat
    );
}

/// GATE-2 SANITY: on a SMALL adversarial instance where B1 (the complete
/// oracle) terminates, confirm B1 reports C unsatisfiable — this validates the
/// "correct answer = C ⊑ owl:Nothing" claim used as the expected verdict at
/// N=13, AND that flag-ON tamed-S1 agrees with B1 there.
#[test]
fn ground_truth_c_unsat_small() {
    // Hold the guard (and thus ENV_MUTEX) for the whole test. B1 is
    // flag-insensitive (engine.rs never reads RUSTDL_CB_SECOND_MAXIMAL), so
    // running it under the flag is harmless; only the S1 call needs it ON.
    let _guard = SecondMaximalGuard::set();
    let internal = adversarial(3);

    let b1 = match classify_unordered(&internal) {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(r) => panic!("B1 OOF on adversarial(3): {r}"),
    };
    assert_c_unsat(&internal, &b1);

    let s1 = match classify_sequoia(&internal) {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(r) => panic!("tamed-S1 OOF on adversarial(3): {r}"),
    };
    assert_c_unsat(&internal, &s1);
}

/// GATE-2 (the crux) — Candidate 1 result: **UNDER-TAMES** (measured 2026-07-28).
///
/// The plan's intended acceptance: with `RUSTDL_CB_SECOND_MAXIMAL=1`, tamed-S1
/// (a) FINISHES fast (< 5 s) on `adversarial(N_BLOWUP)` AND (b) reports C
/// unsatisfiable. Part (b) holds (parity with B1 is preserved — see
/// [`ground_truth_c_unsat_small`] and `cb_sequoia_diff`), but part (a) does NOT:
/// second-maximal eligibility is a strict SUPERSET of the single-maximal Hyper
/// resolutions, so it does MORE work, and `add_clause`'s backward subsumption
/// cannot collapse the resulting incomparable disjunctive antichain. Flag-ON is
/// ~3× SLOWER than flag-OFF at every N (release sweep n=4..12: OFF n=12 ≈ 2.4 s,
/// ON n=12 TIMEOUT >20 s), i.e. Candidate 1 makes the blowup WORSE, not tamer.
///
/// This test is therefore `#[ignore]`d and DOCUMENTS the under-tame: it asserts
/// tamed-S1 does NOT finish in 5 s (the empirical Candidate-1 verdict). Under-
/// taming routes to Task 5 (KM's disjunct-count cap + splitting). Run:
/// `cargo test -p owl-dl-cb --test cb_blowup -- --ignored second_maximal_under_tames`
#[test]
#[ignore = "Candidate-1 result: under-tames (flag-ON is slower); documents the finding, routes to Task 5"]
fn second_maximal_under_tames() {
    let _guard = SecondMaximalGuard::set();
    let internal = adversarial(N_BLOWUP);

    let result = sequoia_with_timeout(internal.clone(), Duration::from_secs(5));

    assert!(
        result.is_none(),
        "UNEXPECTED: Candidate-1 (second-maximal) finished adversarial({N_BLOWUP}) in {:?}. \
         The under-tame no longer reproduces — re-assess Task 4/5.",
        result.map(|(d, _)| d).unwrap_or_default()
    );
    // NB: at small N (see `ground_truth_c_unsat_small`) tamed-S1 DOES report C
    // unsat and matches B1 — completeness (Gate 1) is preserved; only the
    // blowup-taming (Gate 2a) fails.
}
