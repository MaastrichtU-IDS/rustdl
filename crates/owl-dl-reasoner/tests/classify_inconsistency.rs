//! Canaries for `RUSTDL_CLASSIFY_INCONSISTENCY` — `classify` must not report
//! `"consistent": true` on a KB the sibling `is_consistent` calls inconsistent.
//!
//! Two assertions carry this file:
//!
//! * **Positive canary** — `family.ofn` is inconsistent (`HermiT`, Konclude and
//!   rustdl's own `is_consistent` all agree, sub-second). With the flag on,
//!   `classify` must agree AND mark every class unsatisfiable (Konclude's
//!   behaviour, and rustdl's own Phase A1 handling via `classify_inconsistent`).
//!
//! * **Negative control** — `{A ⊑ ⊥, B ⊑ ⊥}` empties every named class yet has a
//!   perfectly good non-empty model. *All-named-classes-unsat is NOT an
//!   inconsistency signal*; only `⊤` being unsatisfiable is. This is the
//!   assertion most likely to catch an implementation that infers inconsistency
//!   from an empty class list.
//!
//! Both are differential against `is_consistent` on the same input, so the two
//! surfaces cannot silently drift apart again.
//!
//! Spec: the `classify_inconsistency_precheck` doc comment in `lib.rs`.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Classification, classify, is_consistent};
use std::io::Cursor;
use std::path::Path;

// Env-mutation plumbing: serialize the flag against other env-mutating tests,
// restore on Drop. Mirrors the pattern in `adaptive_budget.rs`.
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

/// `classify`'s view of consistency — exactly what `classify --json` serialises
/// into its `"consistent"` field (`json_out.rs`: `consistent: !stats.inconsistent`).
fn classify_says_consistent(c: &Classification) -> bool {
    !c.stats().inconsistent
}

// ── Positive canary: family.ofn ────────────────────────────────────────────

/// The bug this flag fixes, verbatim: `classify --json ontologies/real/family.ofn`
/// reported `"consistent": true, "unsatisfiable": []` while `rustdl consistent`
/// on the same file reported `inconsistent`.
///
/// The corpus is gitignored (`./scripts/fetch-real-ontologies.sh`), so the test
/// skips loudly rather than failing when the fixture is absent — but it must NOT
/// skip silently, because a skipped canary is not a canary.
#[test]
fn family_classify_agrees_with_is_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = Path::new("../../ontologies/real/family.ofn");
    let Ok(src) = std::fs::read_to_string(path) else {
        eprintln!(
            "SKIP family_classify_agrees_with_is_consistent: {} absent \
             (run ./scripts/fetch-real-ontologies.sh)",
            path.display()
        );
        return;
    };
    let onto = parse(&src);

    let consistent_verdict = is_consistent(&onto).expect("is_consistent succeeds");
    assert!(
        !consistent_verdict,
        "precondition: family.ofn IS inconsistent (HermiT + Konclude agree). \
         If this fires, the ABox-saturation pre-check regressed, not classify."
    );

    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    // Pin the classify-path budget OFF (`0` = unbounded). `family.ofn` needs
    // ~2.0 s of ABox saturation in a RELEASE build (506 individuals, but a
    // 267k-edge role-chain closure) and several times that in the unoptimized
    // test profile, so under the shipped 3000 ms default this canary would be
    // measuring the host rather than the code. What it must pin is the
    // *signal*: that classify consults the ABox-saturation clash at all.
    // That the shipped DEFAULT budget suffices in a release build is gated
    // separately — at the CLI, and profile-independently by
    // `classify_inconsistency_budget::default_budget_still_detects_small_abox_inconsistency`.
    let _budget = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "0");
    let classification = classify(&onto).expect("classify succeeds");

    assert!(
        !classify_says_consistent(&classification),
        "classify reported consistent on family.ofn while is_consistent reported \
         inconsistent — the two CLI surfaces contradict each other"
    );
    // On an inconsistent KB every class is unsatisfiable (Konclude's behaviour,
    // and rustdl's own A1 handling). An empty list alongside `consistent: false`
    // would be a second wrong answer.
    let n_classes = classification.classes().len();
    assert!(n_classes > 0, "family.ofn should expose named classes");
    assert_eq!(
        classification.unsatisfiable_classes().len(),
        n_classes,
        "inconsistent KB: every one of the {n_classes} named classes must be \
         reported unsatisfiable"
    );
}

// ── Negative control: every named class unsat, KB still consistent ─────────

/// `{A ⊑ ⊥, B ⊑ ⊥}` — every named class is empty, yet the KB has a non-empty
/// model and is CONSISTENT. The correct inconsistency test is that `⊤` is
/// unsatisfiable, never that the unsatisfiable-class list covers everything.
///
/// This is the soundness subtlety of the whole change; an implementation that
/// derives inconsistency from "all classes unsat" passes the family canary and
/// fails here.
const ALL_CLASSES_UNSAT: &str = r"Prefix(:=<http://example.org/allunsat#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://example.org/allunsat>
  Declaration(Class(:A))
  Declaration(Class(:B))
  SubClassOf(:A owl:Nothing)
  SubClassOf(:B owl:Nothing)
)
";

#[test]
fn all_classes_unsat_is_still_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(ALL_CLASSES_UNSAT);

    assert!(
        is_consistent(&onto).expect("is_consistent succeeds"),
        "precondition: {{A ⊑ ⊥, B ⊑ ⊥}} is CONSISTENT (non-empty domain, all \
         named classes merely empty)"
    );

    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    let classification = classify(&onto).expect("classify succeeds");

    // The load-bearing assertion: every class is unsatisfiable …
    assert_eq!(
        classification.unsatisfiable_classes().len(),
        classification.classes().len(),
        "both A and B should be unsatisfiable"
    );
    // … and the KB is nonetheless consistent.
    assert!(
        classify_says_consistent(&classification),
        "all-named-classes-unsat is NOT an inconsistency signal — classify must \
         still report consistent, matching is_consistent"
    );
}

/// `⊤ ⊑ ⊥` IS the real signal: same shape as above but genuinely inconsistent.
/// Pins the discriminator between the two cases.
const TOP_UNSAT: &str = r"Prefix(:=<http://example.org/topunsat#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://example.org/topunsat>
  Declaration(Class(:A))
  Declaration(Class(:B))
  SubClassOf(owl:Thing owl:Nothing)
)
";

#[test]
fn top_unsat_is_inconsistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(TOP_UNSAT);

    assert!(
        !is_consistent(&onto).expect("is_consistent succeeds"),
        "precondition: ⊤ ⊑ ⊥ is inconsistent"
    );

    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    let classification = classify(&onto).expect("classify succeeds");
    assert!(
        !classify_says_consistent(&classification),
        "⊤ unsatisfiable ⟹ classify must report inconsistent"
    );
}

// ── Documented residual ────────────────────────────────────────────────────

/// KNOWN RESIDUAL, recorded rather than hidden. `docs/family-mech4-ddmin-core.ofn`
/// is inconsistent, but the clash is derived by the **wedge consistency route**
/// (`consistency: wedge Unsat`), not by either pre-check `classify` can afford —
/// a bounded global `decide(Top)` probe on the classify path is a measured
/// dead-end (it hangs on consistent `alehif`/`pizza`; see the
/// `inconsistency-detection-gap` record).
///
/// So `classify` remains a sound UNDER-approximation of inconsistency: it can
/// still MISS a tableau-only inconsistency and report `consistent: true`. What
/// this change guarantees is that the two surfaces cannot disagree at the
/// *pre-check* tier, which is now literally shared code
/// (`abox_saturation_inconsistent`).
#[test]
#[ignore = "documents a known residual: wedge-tier inconsistency is not reachable from classify"]
fn ddmin_core_residual_divergence() {
    let path = Path::new("../../docs/family-mech4-ddmin-core.ofn");
    let src = std::fs::read_to_string(path).expect("committed fixture");
    let onto = parse(&src);
    assert!(!is_consistent(&onto).expect("is_consistent succeeds"));

    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    let classification = classify(&onto).expect("classify succeeds");
    assert!(
        !classify_says_consistent(&classification),
        "residual: classify's pre-checks do not reach this wedge-tier clash"
    );
}
