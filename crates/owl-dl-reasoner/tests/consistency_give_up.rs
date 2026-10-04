//! Canaries for #182: `is_consistent` reported an inconsistent ontology as
//! `consistent` once the `ABox` passed ~124 individuals, because the wedge
//! stalled and the bounded tableau fall-through EARLY-ABANDONED — and the
//! give-up was folded into `true` with no signal a caller could see.
//!
//! Two fixes, pinned separately:
//!
//! 1. **Full effort** (`RUSTDL_CONSISTENCY_FULL_EFFORT`, default ON): the
//!    fall-through probe suppresses early-abandon — the lever was calibrated
//!    for per-pair classify probes where a give-up costs one pair; on
//!    `is_consistent` it costs the verdict, and the
//!    `RUSTDL_CONSISTENCY_FALLBACK_MS` deadline already bounds the wall. The
//!    committed reproducer (the issue's 97-axiom delta-debugged file; `HermiT`,
//!    Konclude and `JFact` all say inconsistent) decides correctly in ~2.5 s
//!    without the abandon.
//! 2. **Observability**: when the engine still gives up (deadline or node
//!    cap), `QueryStats::incomplete` is `true`, so the CLI can print a third
//!    outcome instead of a definitive-looking `consistent`.
//!
//! The fixtures are the issue's own reproducer and its control (one padding
//! assertion fewer — below the abandon cliff), kept byte-for-byte so the
//! ~124-individual threshold stays live.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::path::{Path, PathBuf};

// Env mutation is process-wide; serialise and restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(&'static str, Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: every mutation here happens while ENV_MUTEX is held by the caller.
        unsafe { std::env::set_var(key, value) };
        Self(key, prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the lock is still held by the test body.
        unsafe {
            match self.1.take() {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue182")
        .join(name)
}

fn parse(name: &str) -> SetOntology<RcStr> {
    let text = std::fs::read_to_string(fixture(name)).expect("read fixture");
    let (onto, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// The issue's reproducer is decided INCONSISTENT at the default. Pre-fix it
/// read `consistent` — the early-abandon give-up printed as a verdict.
#[test]
fn issue182_reproducer_is_decided_inconsistent_at_the_default() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The release binary decides this in ~2.5 s; the DEBUG test profile is
    // an order of magnitude slower, so the default 10 s fall-through budget
    // fires before the probe finishes and the test would pin the BUDGET, not
    // the calculus. Raise it; the budget behaviour has its own test below.
    let _b = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "600000");
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&parse("reproducer.ofn")).expect("consistency");
    assert!(
        !verdict,
        "HermiT, Konclude and `JFact` all say inconsistent"
    );
    assert!(
        !stats.incomplete,
        "a witnessed clash is a verdict, not a give-up"
    );
}

/// The control (one padding assertion fewer, below the abandon cliff) keeps
/// its correct answer — the fix must not disturb the already-working side of
/// the threshold.
#[test]
fn issue182_control_stays_inconsistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Debug-profile wall, as above.
    let _b = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "600000");
    let (verdict, _) =
        owl_dl_reasoner::is_consistent_with_stats(&parse("control.ofn")).expect("consistency");
    assert!(!verdict);
}

/// With BOTH #182 safety nets off (`RUSTDL_CONSISTENCY_FULL_EFFORT=0` and
/// `RUSTDL_CONSISTENCY_COMPONENTS=0`) the wedge itself now refutes the
/// reproducer.
///
/// FLIPPED (#139). This test used to assert the opposite — that the nets off
/// reproduce the wrong-looking `consistent` — to show the full-effort flag was
/// load-bearing. The give-up it relied on was the #139 ghost-node loop: a `⊔`
/// whose body sat on a node a `≤1` merge had folded was read on the ghost and
/// never closed, so the wedge re-picked it until the depth cap and abandoned.
/// Pre-#139 `main` with `RUSTDL_INVERSE_FUNC_MERGE=0` (no merge, so no ghost)
/// also answers `inconsistent` here, which is what attributes the give-up to
/// the merge rather than to the calculus. The safety nets stay — they protect
/// give-ups this fix does not cure — but this fixture can no longer
/// demonstrate them, and a test asserting the wrong answer would now pin a
/// fixed defect.
#[test]
fn with_both_safety_nets_off_the_wedge_now_refutes_the_reproducer() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = EnvGuard::set("RUSTDL_CONSISTENCY_FULL_EFFORT", "0");
    let _c = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENTS", "0");
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&parse("reproducer.ofn")).expect("consistency");
    assert!(
        !verdict,
        "HermiT, Konclude and `JFact` all say inconsistent; with the #139 fix the \
         wedge reaches that without either safety net"
    );
    assert!(
        !stats.incomplete,
        "a witnessed clash is a verdict, not a give-up"
    );
}

/// The OTHER give-up route (#182's trigger 2): a tiny fall-through budget
/// expires before any verdict, and the `consistent` that comes back is marked
/// INCOMPLETE. This is the observability half — pre-fix, this `true` was
/// indistinguishable from a real answer.
#[test]
fn deadline_give_up_is_marked_incomplete() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Keep full effort ON (the default) and starve the fall-through budget
    // instead, so the give-up is the deadline, not the abandon.
    let _g = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "1");
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&parse("reproducer.ofn")).expect("consistency");
    assert!(verdict, "a 1 ms budget cannot witness this clash");
    assert!(
        stats.incomplete,
        "a deadline give-up must be VISIBLE — the invisible form is the #182 bug class"
    );
}

/// FP-direction guard: a genuinely CONSISTENT ontology with an `ABox` (so the
/// wedge route is active) still answers `consistent` with `incomplete =
/// false` — full effort must not manufacture clashes or spurious
/// incompleteness on the easy side.
#[test]
fn consistent_abox_ontology_is_a_clean_verdict() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let text = "Prefix(:=<urn:cg#>)\nOntology(<urn:cg>\n\
                Declaration(Class(:A)) Declaration(Class(:B))\n\
                Declaration(ObjectProperty(:p))\n\
                Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y))\n\
                SubClassOf(:A :B)\n\
                ClassAssertion(:A :x)\n\
                ObjectPropertyAssertion(:p :x :y)\n)";
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    let (verdict, stats) = owl_dl_reasoner::is_consistent_with_stats(&onto).expect("consistency");
    assert!(verdict);
    assert!(!stats.incomplete);
}

// ---------------------------------------------------------------------------
// The component retry (`RUSTDL_CONSISTENCY_COMPONENTS`, default ON). The full
// ECO+GENEPIO merge still gives up after the full-effort fix; the clash lives in
// a small `ABox` component, so on a give-up the `ABox` is re-checked in batches
// of connected components. Sound only in the inconsistency direction (a subset
// with no model ⇒ the whole has none), which is the only direction used.
//
// To make the give-up deterministic, these tests starve the whole-ontology
// fall-through (`RUSTDL_CONSISTENCY_FALLBACK_MS=1`) and give the batches their
// own generous budget. In the reproducer the clash individual
// (`GENEPIO_0002195`) is a one-axiom component of its own.

const CLASH_ASSERTION: &str = "ClassAssertion(<http://purl.obolibrary.org/obo/GENEPIO_0002148> \
     <http://purl.obolibrary.org/obo/GENEPIO_0002195>)";

/// The reproducer with its one clash assertion removed. Konclude reports it
/// consistent (and the reproducer inconsistent), so the pair discriminates.
fn parse_without_clash() -> SetOntology<RcStr> {
    let text = std::fs::read_to_string(fixture("reproducer.ofn")).expect("read fixture");
    assert!(text.contains(CLASH_ASSERTION), "fixture drifted");
    let text = text.replace(CLASH_ASSERTION, "");
    let (onto, _) =
        read_ofn(&mut Cursor::new(text), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// A give-up on the whole ontology is turned into `inconsistent` by a component
/// batch that witnesses the clash, and the answer is a verdict, not `incomplete`.
#[test]
fn component_retry_decides_a_whole_ontology_give_up() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _f = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "1");
    let _b = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENT_BATCH_MS", "600000");
    let _t = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENTS_MS", "1200000");
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&parse("reproducer.ofn")).expect("consistency");
    assert!(!verdict, "a component batch must witness the clash");
    assert!(!stats.incomplete, "a witnessed clash is a verdict");
}

/// `RUSTDL_CONSISTENCY_COMPONENTS=0` keeps the give-up: the flag is what
/// decides the case above, not something else on the path.
#[test]
fn component_retry_off_keeps_the_give_up() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _off = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENTS", "0");
    let _f = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "1");
    let _b = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENT_BATCH_MS", "600000");
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&parse("reproducer.ofn")).expect("consistency");
    assert!(verdict);
    assert!(stats.incomplete);
}

/// FP guard: on the consistent variant every batch is consistent, so the retry
/// must leave the give-up as `consistent (incomplete)` and never invent a clash.
#[test]
fn component_retry_does_not_invent_a_clash() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _f = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "1");
    let _b = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENT_BATCH_MS", "600000");
    let _t = EnvGuard::set("RUSTDL_CONSISTENCY_COMPONENTS_MS", "1200000");
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&parse_without_clash()).expect("consistency");
    assert!(verdict, "Konclude: consistent");
    assert!(stats.incomplete, "the whole-ontology check still gave up");
}
