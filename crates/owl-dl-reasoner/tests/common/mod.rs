//! Shared helpers for the incremental-session gates (Task 8).
//!
//! `cargo test` runs integration tests with cwd == the crate manifest dir, so
//! every fixture path is resolved off `CARGO_MANIFEST_DIR` — the same pattern
//! as `crates/owl-dl-saturation/tests/common/mod.rs`.
#![allow(dead_code)]
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Once;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{AnnotatedComponent, MutableOntology, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;

/// Roots searched for a bare fixture file name, in order, relative to the
/// crate manifest dir. `/ontologies/` is GITIGNORED and deliberately absent:
/// a gate that only runs on a developer machine is not a gate.
const FIXTURE_ROOTS: &[&str] = &[
    "tests/fixtures",
    "tests/fixtures/incremental",
    "../../bench-corpus",
    "../owl-dl-saturation/tests/fixtures",
];

// ---------------------------------------------------------------------------
// Budget-free mode
// ---------------------------------------------------------------------------

/// Every wall-clock budget that the default `classify` path consults, pinned
/// OFF.
///
/// Byte-identity is a claim about the *algorithm*, and it is only testable
/// where the algorithm is deterministic. With any of these on, a verdict
/// depends on how fast this host happens to be, so a from-scratch run is not
/// even reproducible against itself and the gate would flake by construction.
///
/// * `RUSTDL_LABEL_CACHE_TIMEOUT_MS=0` — unbounded per-class label-cache
///   build. This is the one that bites: `adaptive_label_cache_ms` (lib.rs)
///   returns the 30 s CEILING when `per_pair_timeout` is `None`, so even the
///   "unbounded" `classify()` entry point carries a wall-clock cut. `0` is the
///   documented "unbounded" value and always wins over the adaptive formula.
/// * `RUSTDL_ADAPTIVE_BUDGET=0` — no early-cut of a "diverging" wedge search.
/// * `RUSTDL_AGGREGATE_DEADLINE_MS=0` — no aggregate wall (opt-in anyway, but
///   an ambient value in the developer's shell would silently arm it).
/// * `RUSTDL_HYPER_TRUST_SAT_MIN_MS=0` — no wall-time-thresholded distrust of
///   a wedge `NotSubsumed`.
/// * `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0` — realize-only, unused here, but it
///   defaults to 750 ms and costs nothing to pin.
///
/// Deliberately NOT disabled: `RUSTDL_MAX_NODES` (default 50 000). It is a
/// *node-count* cap, not a clock, so it is deterministic — the same input cuts
/// at the same place on every host. Turning it off would only re-arm the
/// issue-#35 unbounded-generation hazard.
const BUDGET_FREE_ENV: &[(&str, &str)] = &[
    ("RUSTDL_LABEL_CACHE_TIMEOUT_MS", "0"),
    ("RUSTDL_ADAPTIVE_BUDGET", "0"),
    ("RUSTDL_AGGREGATE_DEADLINE_MS", "0"),
    ("RUSTDL_HYPER_TRUST_SAT_MIN_MS", "0"),
    ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0"),
];

static BUDGET_FREE: Once = Once::new();

/// Pin the process into budget-free mode. Call it as the FIRST statement of
/// every test in this suite.
///
/// `Once` is what makes the `set_var` sound: no test in this binary touches
/// the reasoner before it has passed through here, and by the time it does the
/// writes are complete and never repeated. (Rayon's workers are spawned lazily
/// by the first reasoner call, i.e. strictly after.)
#[allow(unsafe_code)]
pub(crate) fn budget_free() {
    BUDGET_FREE.call_once(|| {
        for (k, v) in BUDGET_FREE_ENV {
            // SAFETY: single-shot, before any reasoner call in this binary has
            // read the environment, and before rayon has spawned a worker.
            unsafe { std::env::set_var(k, v) };
        }
    });
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Resolve a bare fixture file name (e.g. `"pizza.ofn"`) to an existing path.
///
/// # Panics
/// Panics if the fixture is not found under any tracked root.
pub(crate) fn fixture_path(name: &str) -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    for root in FIXTURE_ROOTS {
        let candidate = manifest.join(root).join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("fixture {name} not found under any of {FIXTURE_ROOTS:?} (relative to the crate dir)");
}

/// Parse a tracked `.ofn` fixture into a `SetOntology`.
///
/// # Panics
/// Panics if the fixture cannot be found, read or parsed.
pub(crate) fn load_ofn(name: &str) -> SetOntology<RcStr> {
    let path = fixture_path(name);
    let mut r = std::io::BufReader::new(
        std::fs::File::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display())),
    );
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut r, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    onto
}

/// The components of `o` in a CANONICAL order.
///
/// `SetOntology` is backed by a `HashSet` with a per-process random seed, so
/// its iteration order differs from run to run. Every edit script in this
/// suite is derived from this ordering, not from the raw iteration order, or
/// "seed 42" would name a different script on every invocation and a failure
/// would not be reproducible.
pub(crate) fn canonical_components(o: &SetOntology<RcStr>) -> Vec<AnnotatedComponent<RcStr>> {
    let mut v: Vec<AnnotatedComponent<RcStr>> = o.into_iter().cloned().collect();
    v.sort_by_cached_key(|ac| format!("{ac:?}"));
    v
}

/// Partition `o` into (kept, rest): a component at canonical index `i` goes to
/// `kept` iff `keep(i)` is true, and to `rest` otherwise.
pub(crate) fn split_axioms(
    o: &SetOntology<RcStr>,
    mut keep: impl FnMut(usize) -> bool,
) -> (SetOntology<RcStr>, Vec<AnnotatedComponent<RcStr>>) {
    let mut kept: SetOntology<RcStr> = SetOntology::new_rc();
    let mut rest = Vec::new();
    for (i, ac) in canonical_components(o).into_iter().enumerate() {
        if keep(i) {
            kept.insert(ac);
        } else {
            rest.push(ac);
        }
    }
    (kept, rest)
}

// ---------------------------------------------------------------------------
// Verdict projection — IRIs only, read through the public accessors
// ---------------------------------------------------------------------------

/// The full reported verdict of a classification, projected to IRIs.
///
/// Session ids and from-scratch ids differ BY CONSTRUCTION (`convert_ontology`
/// sorts the vocabulary, `convert_delta` appends), so nothing id-shaped may
/// appear here. Subsumption is read through [`Classification::is_subclass`],
/// which routes via the private `entails` choke-point — the ONLY place the
/// ELIDED rows of unsatisfiable classes are reintroduced as `⊥ ⊑ *`. Reading
/// raw `EntailmentMatrix` rows would silently drop exactly those.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub classes: Vec<String>,
    pub subsumptions: Vec<(String, String)>,
    pub unsatisfiable: Vec<String>,
}

/// The sorted `(sub, sup)` IRI pairs entailed by `c`, reflexive pairs omitted.
pub(crate) fn hierarchy_iris(c: &Classification) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for a in c.classes() {
        for b in c.classes() {
            if a != b && c.is_subclass(a, b) {
                v.push((a.clone(), b.clone()));
            }
        }
    }
    v.sort();
    v
}

/// Hierarchy + reported class set + unsatisfiable set, all IRI-keyed.
pub(crate) fn verdict(c: &Classification) -> Verdict {
    let mut unsatisfiable: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    unsatisfiable.sort();
    // Both sides are re-sorted, because the two reporting orders legitimately
    // differ: `classify()` emits in vocabulary-id order, which is IRI order
    // only when the input declares every class (a partially-declared ontology
    // — which every split in this gate is — interns some classes on first USE
    // instead), whereas a session always emits through `restricted_sorted`.
    // Report ORDER is therefore not this gate's business; it is pinned by
    // `incremental_session.rs::reported_classes_are_sorted_by_iri_not_by_session_id`.
    let mut classes = c.classes().to_vec();
    classes.sort();
    Verdict {
        classes,
        subsumptions: hierarchy_iris(c),
        unsatisfiable,
    }
}
