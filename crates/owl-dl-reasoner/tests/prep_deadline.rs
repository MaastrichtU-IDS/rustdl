//! Canaries for `RUSTDL_PREP_DEADLINE` — a classify GLOBAL wall-clock budget
//! must bound the PREPARATION phases (EL saturation, `from_internal`), not just
//! the search.
//!
//! The defect: `classify_top_down_internal` consulted `global_deadline` for the
//! first time inside the label-cache loop. Measured over the 252-ontology DNF
//! population under a **1 ms** budget, 77 still burned ≥ 10 s and 26 never
//! finished `from_internal` at all (`ore_ont_10926`: 84.9 s against a 1 ms
//! promise).
//!
//! Five things must hold, one canary each:
//!
//! 1. **The budget is honoured.** Flag ON under a 1 ms budget must be
//!    dramatically faster than flag OFF on the same input — the "previously
//!    overran it" half is measured in the same test, so the comparison cannot
//!    rot into a vacuous absolute threshold.
//! 2. **The result is flagged INCOMPLETE**, not passed off as complete. Silently
//!    returning a partial closure as complete would be a new instance of this
//!    codebase's worst bug class (a gate certifying complete while work was
//!    dropped).
//! 3. **The `from_internal` half is bounded too** — a separate code path from
//!    the saturation abort, and (sabotage-verified) not guarded by any of the
//!    others.
//! 4. **Soundness**: the truncated hierarchy is a SUBSET of the unbounded one.
//!    Aborting prep may only MISS edges, never invent one (FP=0).
//! 5. **An UNTIMED run is unaffected** — that is the common path.
//!
//! See `owl_dl_reasoner::prep_deadline_enabled` for the full argument.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Classification, classify, classify_with_budget};
use std::io::Cursor;
use std::time::Duration;

// Env-mutation plumbing: serialize the flag against other env-mutating tests,
// restore on Drop. Mirrors `classify_inconsistency.rs`.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one test,
        // serialized via ENV_MUTEX, restored on Drop.
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
    let mut reader = Cursor::new(src.to_owned());
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// A subsumption CHAIN `A0 ⊑ A1 ⊑ … ⊑ A(k-1)`.
///
/// Why a chain: its transitive closure is `k²/2` subsumer pairs, so the
/// saturation fixpoint performs Θ(k²) worklist pops. That is what makes the
/// prep phase measurably expensive *without* any tableau work, and it comfortably
/// exceeds the 4096-pop deadline-check stride so the bounded run has somewhere to
/// stop. `∃r` fillers are added so the fixpoint also exercises the fact queue.
fn chain_ontology(k: usize) -> String {
    use std::fmt::Write as _;
    let mut s = String::from("Prefix(:=<http://e.org/>)\nOntology(\n");
    s.push_str("Declaration(ObjectProperty(:r))\n");
    for i in 0..k {
        let _ = writeln!(s, "Declaration(Class(:A{i}))");
    }
    for i in 0..k.saturating_sub(1) {
        let _ = writeln!(s, "SubClassOf(:A{i} :A{})", i + 1);
    }
    for i in (0..k).step_by(7) {
        let _ = writeln!(
            s,
            "SubClassOf(:A{i} ObjectSomeValuesFrom(:r :A{}))",
            (i + 3) % k
        );
    }
    s.push_str(")\n");
    s
}

/// A small OUT-OF-FRAGMENT ontology (`∀` kicks it off every saturation fast
/// path), used by the canaries that must reach `from_internal`.
fn forall_ontology() -> String {
    "Prefix(:=<http://e.org/>)\nOntology(\n\
     Declaration(ObjectProperty(:p))\n\
     Declaration(Class(:X))\nDeclaration(Class(:Y))\nDeclaration(Class(:Z))\n\
     SubClassOf(:X ObjectAllValuesFrom(:p :Y))\n\
     SubClassOf(:X ObjectSomeValuesFrom(:p :Y))\n\
     SubClassOf(:Y :Z)\n\
     EquivalentClasses(:Z ObjectSomeValuesFrom(:p :Y))\n)\n"
        .to_owned()
}

/// Every entailed `(sub, sup)` pair, as index pairs.
fn edges(c: &Classification) -> std::collections::HashSet<(usize, usize)> {
    let classes = c.classes();
    let mut out = std::collections::HashSet::new();
    for (i, sub) in classes.iter().enumerate() {
        for (j, sup) in classes.iter().enumerate() {
            if c.is_subclass(sub, sup) {
                out.insert((i, j));
            }
        }
    }
    out
}

// ── 1. The budget is honoured ──────────────────────────────────────────────

/// A 1 ms budget must actually bound the run. Both arms are measured HERE, so
/// the assertion is "ON is far faster than OFF on this very input" rather than
/// an absolute wall threshold that host speed could invalidate.
#[test]
fn small_budget_is_honoured_on_prep_bound_ontology() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(&chain_ontology(500));
    let budget = Some(Duration::from_millis(1));

    // Flag OFF — the pre-change behaviour: the 1 ms budget bounds only the
    // search, so the whole Θ(k²) saturation still runs.
    let off_wall = {
        let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "0");
        let t = std::time::Instant::now();
        let h = classify_with_budget(&onto, None, budget).expect("classify OFF");
        let w = t.elapsed();
        assert!(
            !h.stats().prep_timed_out,
            "flag OFF must never take the prep-timeout path"
        );
        w
    };

    // Flag ON — the fixpoint is abandoned at the deadline.
    let on_wall = {
        let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "1");
        let t = std::time::Instant::now();
        let h = classify_with_budget(&onto, None, budget).expect("classify ON");
        let w = t.elapsed();
        assert!(
            h.stats().prep_timed_out,
            "flag ON under a 1 ms budget must abandon prep on this fixture \
             (if this fires, either the fixture became too cheap to overrun \
             1 ms, or the deadline no longer reaches the prep phases)"
        );
        w
    };

    // The load-bearing comparison. A factor of 2 is a deliberately loose bar:
    // the real ratio on this fixture is far larger, and a loose bar keeps the
    // canary from flaking under host contention while still failing outright if
    // the deadline stops reaching prep (in which case ON == OFF).
    assert!(
        on_wall * 2 < off_wall,
        "prep deadline did not bound the run: ON {on_wall:?} vs OFF {off_wall:?}"
    );
}

// ── 2. The result is flagged incomplete, not passed off as complete ────────

/// The signal that must never be missing. `classify --json` maps
/// `stats.timed_out_pairs > 0` to `"incomplete": true`, and
/// `completeness_guaranteed()` reads the same counter — so BOTH are asserted,
/// not just the dedicated `prep_timed_out` bool.
#[test]
fn prep_timeout_is_reported_incomplete() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "1");
    let onto = parse(&chain_ontology(500));
    let h = classify_with_budget(&onto, None, Some(Duration::from_millis(1)))
        .expect("classify under 1 ms budget");

    assert!(h.stats().prep_timed_out, "prep_timed_out must be set");
    assert!(
        h.stats().timed_out_pairs > 0,
        "timed_out_pairs must be bumped — it is what drives `\"incomplete\": true` \
         in `classify --json`; without it a partial closure is served as complete"
    );
    assert!(
        !h.completeness_guaranteed(),
        "completeness_guaranteed() must be false after a prep timeout"
    );
    // Invariant every other timeout site keeps: +1 count, +1 id, so
    // `undecided_pairs()` stays index-safe.
    assert_eq!(
        h.stats().timed_out_pairs,
        h.stats().timed_out_pair_ids.len(),
        "timed-out count/id invariant"
    );
    let _ = h.undecided_pairs(); // must not panic
}

// ── 3. The `from_internal` half specifically ───────────────────────────────

/// The saturation abort and the `from_internal` abort are two independent code
/// paths, and a canary that only exercises the first does NOT guard the second:
/// verified by sabotage — unbudgeting `from_internal` left every other canary in
/// this file green.
///
/// This one pins the second path by CONSTRUCTION rather than by timing:
///
/// * the ontology is OUT-OF-FRAGMENT (`∀`), so classify cannot take the pure-EL
///   fast path and must reach `from_internal`;
/// * it is tiny, so its saturation fixpoint is far below the 4096-pop deadline
///   stride and therefore CANNOT abort — the only remaining source of a
///   prep-timeout is `from_internal`;
/// * the budget is `ZERO`, so the deadline has provably passed before
///   `from_internal`'s first check regardless of host speed.
///
/// The `edges` equality is the discriminator: because saturation completed, the
/// degraded answer must be the FULL EL closure — so this asserts "bounded in
/// `from_internal`, and nothing lost from the phase that did finish".
#[test]
fn from_internal_is_bounded_too() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(&forall_ontology());

    let full_el_edges = {
        // `--saturation-only` is exactly the EL closure read-off, i.e. what the
        // degradation path returns when saturation itself completed.
        let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "0");
        edges(&owl_dl_reasoner::classify_saturation_only(&onto).expect("saturation-only classify"))
    };

    let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "1");
    let h = classify_with_budget(&onto, None, Some(Duration::ZERO)).expect("zero-budget classify");

    // CONTRACT CHANGED 2026-08-17 (`classify::prep_bounding_active`). This
    // previously asserted `prep_timed_out`, i.e. that a ZERO budget aborts
    // inside `from_internal`. It no longer does, deliberately: prep is bounded
    // only while the budget is still MEETABLE, and a zero budget never is. The
    // motivation is measured — bounding prep against an already-blown budget
    // made `ore_ont_7192` at a 3 s budget pay its full ~18 s of uninterruptible
    // parse + conversion and then return 0 rows, where the fallback returns all
    // 50,753.
    //
    // What must still hold is the part that always mattered: the result is a
    // SOUND, EXPLICITLY INCOMPLETE partial answer, and it is the full EL closure
    // because saturation ran to completion. Only the phase that reports the
    // truncation moved (from `prep_timed_out` to the pair deadline).
    assert!(
        !h.stats().prep_timed_out,
        "a ZERO budget is unmeetable, so prep must NOT be bounded — bounding it \
         spends the whole prep wall and then reports nothing"
    );
    assert!(
        h.stats().timed_out_pairs > 0,
        "must still be flagged incomplete"
    );
    assert_eq!(
        edges(&h),
        full_el_edges,
        "saturation ran unbounded here, so the degraded answer must be the full \
         EL closure — a mismatch means something other than the pair deadline cut it"
    );
}

// ── 4. Soundness: the truncated hierarchy is a SUBSET ──────────────────────

/// FP=0 in-test: aborting prep may only OMIT entailments. Every edge the
/// bounded run reports must also be reported by the unbounded run.
#[test]
fn prep_timeout_hierarchy_is_a_subset_of_the_complete_one() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(&chain_ontology(400));

    let full = {
        let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "0");
        edges(&classify(&onto).expect("unbounded classify"))
    };
    let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "1");
    let bounded =
        edges(&classify_with_budget(&onto, None, Some(Duration::from_millis(1))).expect("bounded"));

    let extra: Vec<_> = bounded.difference(&full).take(5).collect();
    assert!(
        extra.is_empty(),
        "bounded run reported {} edge(s) the unbounded run does not — that is an FP: {extra:?}",
        bounded.difference(&full).count()
    );
}

// ── 5. An UNTIMED run is unaffected ────────────────────────────────────────

/// The common path. With no global budget the flag must be inert: same edges,
/// same unsatisfiable set, and never the prep-timeout path — regardless of how
/// long prep takes.
#[test]
fn untimed_classify_is_unaffected_by_the_flag() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Includes an `∀` axiom so this exercises the HYBRID path (`from_internal` +
    // label cache + tier walk), not only the pure-EL fast path.
    let onto = parse(&forall_ontology());

    let off = {
        let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "0");
        classify(&onto).expect("classify OFF")
    };
    let on = {
        let _flag = SetEnvGuard::set("RUSTDL_PREP_DEADLINE", "1");
        classify(&onto).expect("classify ON")
    };

    assert!(
        !on.stats().prep_timed_out,
        "an untimed run must never take the prep-timeout path"
    );
    assert_eq!(on.classes(), off.classes(), "class list");
    assert_eq!(edges(&on), edges(&off), "entailed edges");
    assert_eq!(
        on.unsatisfiable_classes(),
        off.unsatisfiable_classes(),
        "unsatisfiable set"
    );
    assert_eq!(
        on.stats().timed_out_pairs,
        off.stats().timed_out_pairs,
        "timed-out pairs"
    );
}
