//! Behavioural guard on the DEFAULT of every public boolean `RUSTDL_*` flag.
//!
//! # Why this exists
//!
//! On 2026-08-17 `RUSTDL_CLASSIFY_LABELS_AMORTIZE` was found documented as
//! **"DEFAULT OFF"** in two places — its own doc header and `CLAUDE.md`, the
//! latter with a note that a corpus bake-off was "pending" — while the code had
//! used the default-**ON** idiom since 0.4.10. The comment *inside* the same
//! predicate said "DEFAULT ON since 0.4.10", so the file contradicted itself for
//! two weeks. Measured cost of that drift: `=0` is ~20× slower
//! (`ore_ont_12698` `label_cache_build` 2,028 ms → 104,237 ms), and the stale note
//! nearly caused a re-run of a bake-off whose flip had already shipped.
//!
//! Two sibling defects the same day: `label_cache_timeout_ms` was listed in
//! `CLAUDE.md`'s constant audit as **"dead code"** while being load-bearing at 18×,
//! and `RUSTDL_PREP_DEADLINE`'s stated rationale described behaviour a measurement
//! contradicted.
//!
//! # Why a behavioural table rather than doc parsing
//!
//! A doc-comment scanner was tried first. It does detect a real mismatch (verified
//! by injecting one), but it **false-positives on historical narrative**: the
//! `RUSTDL_SNAPSHOT_CAPTURE` header explains that the flag *used to be* default-ON
//! before the 2026-06-08 soundness flip, and a text scan reads that as the current
//! default. Calling the accessor cannot be fooled that way.
//!
//! # How to change a default
//!
//! Deliberately: flip the code, then flip the entry in `EXPECTED` below in the same
//! commit. That is the point — the table makes a default change a reviewable edit
//! instead of a silent one.
//!
//! Coverage is the 32 public `fn() -> bool` accessors across `owl-dl-core`,
//! `owl-dl-tableau` and `owl-dl-reasoner`. Numeric knobs
//! (`RUSTDL_*_MS`, `RUSTDL_MAX_NODES`, …) are out of scope here; they are pinned by
//! their own tests where they are load-bearing (e.g.
//! `adaptive_inconsistency_budget.rs`, `pair_timeout_default.rs`).

use std::sync::Mutex;

/// Serialises env mutation within this test binary.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// Restores a variable's prior value on drop.
struct EnvGuard(&'static str, Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn unset(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: all mutation in this file happens while ENV_MUTEX is held.
        unsafe { std::env::remove_var(key) };
        Self(key, prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the caller still holds ENV_MUTEX.
        unsafe {
            match self.1.take() {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

/// `(env var, accessor, expected value when the var is UNSET)`.
///
/// `true` = the feature is ON by default. Sorted by crate then name so a diff of
/// this table reads as a list of default changes.
type FlagRow = (&'static str, fn() -> bool, bool);

fn expected() -> Vec<FlagRow> {
    vec![
        // ── owl-dl-core ──────────────────────────────────────────────────
        (
            "RUSTDL_DOMAIN_ABSORPTION",
            owl_dl_core::absorb::domain_absorption_enabled as fn() -> bool,
            true,
        ),
        (
            "RUSTDL_NOMINAL_EXISTS_ABSORPTION",
            owl_dl_core::absorb::nominal_exists_absorption_enabled as fn() -> bool,
            true,
        ),
        (
            "RUSTDL_INVERSE_PAIR_FUNC",
            owl_dl_core::convert::inverse_pair_functionality_enabled,
            false,
        ),
        // ── owl-dl-tableau ───────────────────────────────────────────────
        (
            "RUSTDL_FIXPOINT_DEADLINE",
            owl_dl_tableau::hyper_fixpoint_deadline_enabled,
            true,
        ),
        (
            "RUSTDL_MAX_TRIAL_MERGE",
            owl_dl_tableau::max_trial_merge_enabled as fn() -> bool,
            true,
        ),
        (
            "RUSTDL_HYPER_MATCH_DEADLINE",
            owl_dl_tableau::hyper_match_deadline_enabled,
            true,
        ),
        (
            "RUSTDL_INVERSE_FUNC_MERGE",
            owl_dl_tableau::inverse_func_merge_enabled,
            true,
        ),
        // ON since 2026-08-22. It emits an entailed `∃r⁻.⊤ ⊑ ≤1 r⁻.⊤` GCI so realize stops
        // dropping inverse-functional-forced individual equality — but now only for roles
        // that occur in an `ObjectPropertyAssertion` (`inv_func_merge_consumable`), because
        // that is the only shape in which the merge it triggers can fire.
        //
        // The flip's two stated prerequisites are met. Two-arm ORE sweep over all 109
        // InverseFunctional-bearing ontologies: 99 IDENTICAL, 0 ok->DNF, 0 DNF->ok,
        // aggregate wall +0.5%; the single DIFFER (`ore_ont_12698`) adjudicated to
        // concurrency nondeterminism — four sequential runs byte-identical. ΔMISSED is
        // SUBSUMED rather than skipped: identical classify output cannot have changed
        // MISSED, and realize over the 73 ABox+InverseFunctional frame is 39 comparable
        // with 0 gains and 0 losses.
        //
        // Honest framing: a correctness fix with ZERO measured corpus benefit and zero
        // measured cost — the same basis on which its FUNCTIONAL sibling already ships
        // default-ON (recorded as firing on 0 of 64 qualifying ORE ontologies). Do not
        // cite it as a corpus win. The gate deliberately forgoes `ore_ont_13859`'s
        // classify gain, which had no ObjectPropertyAssertion.
        (
            "RUSTDL_INVERSE_FUNC_MAX",
            owl_dl_core::convert::inverse_functional_max_enabled,
            true,
        ),
        // OFF pending a full-corpus sweep. Measured on its addressable population (19
        // slow small-`n` completers): aggregate +1.5%, one 3.46x win (`ore_ont_5107`
        // 6.65s -> 1.92s), 0 losses <=0.8x, 0 row diffs; and on 20 fast completers
        // -2.3% (~50ms total), 0 at >=1.25x slower. Safe and net-positive, but it
        // changes the budget on every small-`n` ontology, and this repo's record has a
        // 12-ontology benchmark hiding four ok->DNF regressions.
        (
            "RUSTDL_LABEL_CACHE_PROBE",
            owl_dl_reasoner::label_cache_probe_enabled,
            false,
        ),
        // ── owl-dl-reasoner ──────────────────────────────────────────────
        (
            "RUSTDL_ABOX_CHECK",
            owl_dl_reasoner::abox_check_enabled,
            true,
        ),
        (
            "RUSTDL_ABOX_SATURATION",
            owl_dl_reasoner::abox_saturation_enabled,
            true,
        ),
        (
            "RUSTDL_ANYWHERE_BLOCKING",
            owl_dl_reasoner::anywhere_blocking_enabled,
            false,
        ),
        (
            "RUSTDL_CLASSIFY_CONSISTENCY_PROBE",
            owl_dl_reasoner::classify_consistency_probe_enabled,
            true,
        ),
        (
            "RUSTDL_CLASSIFY_TBOX_FRAGMENT",
            owl_dl_reasoner::classify_tbox_fragment_enabled,
            true,
        ),
        (
            "RUSTDL_CLASSIFY_TBOX_ONLY",
            owl_dl_reasoner::classify_tbox_only_enabled,
            true,
        ),
        (
            "RUSTDL_COUNTING_PAIR_VERIFY",
            owl_dl_reasoner::counting_pair_verify_enabled,
            true,
        ),
        (
            "RUSTDL_HORN_SHORTCIRCUIT",
            owl_dl_reasoner::horn_shortcircuit_enabled,
            true,
        ),
        (
            "RUSTDL_HYPER_DOUBLE_BLOCK",
            owl_dl_reasoner::hyper_double_block_enabled,
            true,
        ),
        (
            "RUSTDL_MRV_ORDERING",
            owl_dl_reasoner::hyper_mrv_ordering_enabled,
            true,
        ),
        (
            "RUSTDL_PRECISE_CARD_DEPS",
            owl_dl_reasoner::hyper_precise_card_deps_enabled,
            true,
        ),
        (
            "RUSTDL_SAT_LOOKAHEAD",
            owl_dl_reasoner::hyper_sat_lookahead_enabled,
            false,
        ),
        (
            "RUSTDL_SAT_SEED",
            owl_dl_reasoner::hyper_sat_seed_enabled,
            true,
        ),
        (
            "RUSTDL_SHADOW_DEP_PROBE",
            owl_dl_reasoner::hyper_shadow_dep_probe_enabled,
            false,
        ),
        (
            "RUSTDL_HYPERTABLEAU_TRUST_SAT",
            owl_dl_reasoner::hyper_trust_sat_enabled,
            true,
        ),
        (
            "RUSTDL_HYPERTABLEAU",
            owl_dl_reasoner::hyper_wedge_enabled,
            true,
        ),
        (
            "RUSTDL_LABEL_HEURISTIC",
            owl_dl_reasoner::label_heuristic_enabled,
            true,
        ),
        (
            "RUSTDL_LAZY_ABOX_SATURATION",
            owl_dl_reasoner::lazy_abox_saturation_enabled,
            false,
        ),
        // Flipped ON 2026-08-17 once `prep_bounding_active` removed the
        // blown-budget pathology; see that flag's doc comment.
        (
            "RUSTDL_PREP_DEADLINE",
            owl_dl_reasoner::prep_deadline_enabled,
            true,
        ),
        // OFF since 2026-06-08 — SOUNDNESS FIX (FP-unsound on non-Horn).
        // A default flip here would re-introduce false subsumptions.
        (
            "RUSTDL_SNAPSHOT_CAPTURE",
            owl_dl_reasoner::snapshot_capture_enabled,
            false,
        ),
        (
            "RUSTDL_SNAPSHOT_LAZY",
            owl_dl_reasoner::snapshot_lazy_enabled,
            true,
        ),
        (
            "RUSTDL_UNSAT_VIA_LABELS",
            owl_dl_reasoner::unsat_via_labels_enabled,
            true,
        ),
        (
            "RUSTDL_WEDGE_CONSISTENCY",
            owl_dl_reasoner::wedge_consistency_enabled,
            true,
        ),
    ]
}

/// Every public boolean flag has the default this table records.
///
/// One test rather than N so the failure message lists **all** drifted flags at
/// once — a default flip usually comes in groups, and fixing them one
/// test-run at a time is slow.
#[test]
fn public_bool_flag_defaults_are_pinned() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut drift = Vec::new();
    for (key, accessor, want) in expected() {
        let _g = EnvGuard::unset(key);
        let got = accessor();
        if got != want {
            drift.push(format!(
                "  {key}: expected default {want}, got {got}  (flip the code back, or \
                 update this table in the same commit)"
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "{} public flag default(s) drifted from the pinned table:\n{}",
        drift.len(),
        drift.join("\n")
    );
}

/// The two flags whose default is a SOUNDNESS decision, asserted separately so a
/// failure names the risk rather than appearing as one row in a list of 29.
///
/// `RUSTDL_SNAPSHOT_CAPTURE` was default-ON until 2026-06-08 and emitted spurious
/// subsumptions on disjunctive ontologies (ORE surfaced 30+ FPs each on
/// `ore_ont_13723` and others, silently, with no incompleteness signal). Turning it
/// back on by default would re-introduce that.
#[test]
fn soundness_critical_defaults_stay_off() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::unset("RUSTDL_SNAPSHOT_CAPTURE");
    assert!(
        !owl_dl_reasoner::snapshot_capture_enabled(),
        "RUSTDL_SNAPSHOT_CAPTURE must stay OFF by default: it is FP-UNSOUND on the \
         non-Horn fragment (replay trusts ONE satisfying model, and its BackPropRisk \
         gate does not exclude disjunction). Flipping this on ships false subsumptions."
    );
}

/// An explicit `=0` must disable a default-ON flag, and `=1` must enable a
/// default-OFF one. Guards the *other* half of the contract: a flag whose default
/// is right but which ignores its override is equally broken, and the two idioms
/// in use (`is_none_or` / `is_some_and`) are easy to transpose.
#[test]
fn overrides_are_honoured_in_both_directions() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    #[allow(unsafe_code)]
    fn with(key: &str, val: &str, f: impl Fn() -> bool) -> bool {
        let prior = std::env::var_os(key);
        // SAFETY: the caller holds ENV_MUTEX.
        unsafe { std::env::set_var(key, val) };
        let got = f();
        unsafe {
            match prior {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        got
    }
    assert!(
        !with(
            "RUSTDL_LABEL_HEURISTIC",
            "0",
            owl_dl_reasoner::label_heuristic_enabled
        ),
        "`=0` must disable a default-ON flag"
    );
    assert!(
        with(
            "RUSTDL_ANYWHERE_BLOCKING",
            "1",
            owl_dl_reasoner::anywhere_blocking_enabled
        ),
        "`=1` must enable a default-OFF flag"
    );
}
