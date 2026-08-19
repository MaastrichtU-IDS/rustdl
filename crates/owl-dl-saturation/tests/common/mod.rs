//! Shared helpers for the `owl-dl-saturation` integration tests.
//!
//! `cargo test` runs integration tests with cwd == the crate manifest dir
//! (`crates/owl-dl-saturation`), NOT the workspace root, so every fixture path
//! is resolved off `CARGO_MANIFEST_DIR` — the same pattern as
//! `crates/owl-dl-cli/tests/incremental_fixpoint_identity.rs`.
#![allow(dead_code)]
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::path::{Path, PathBuf};

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::InternalOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_saturation::Subsumers;

/// Roots searched for a bare fixture file name, in order. All are relative to
/// the crate manifest dir. `tests/fixtures` holds crate-local fixtures;
/// `bench-corpus` is the tracked workspace corpus; `ontologies/real` is the
/// gitignored on-demand corpus (present on developer machines only).
const FIXTURE_ROOTS: &[&str] = &[
    "tests/fixtures",
    "../../bench-corpus",
    "../../ontologies/real",
];

/// Resolve a bare fixture file name (e.g. `"pizza.ofn"`) to an existing path.
///
/// # Panics
/// Panics if the fixture is not found under any known root.
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

/// Parse an `.ofn` fixture into an [`InternalOntology`].
///
/// # Panics
/// Panics if the fixture cannot be found, read, parsed, or converted.
pub(crate) fn load_fixture(name: &str) -> InternalOntology {
    let path = fixture_path(name);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse fixture {}: {e}", path.display()));
    convert_ontology(&onto).unwrap_or_else(|e| panic!("convert fixture {}: {e}", path.display()))
}

/// Project a [`Subsumers`] closure down to IRI pairs over the *user* vocabulary.
///
/// Synthetics (Tseitin stand-ins, nominal keys, …) live above the user
/// vocabulary and carry no IRI, so they are skipped. The result is sorted, so
/// two closures computed with different synthetic id bases compare equal iff
/// they are semantically identical.
#[must_use]
pub(crate) fn closure_as_iri_pairs(
    internal: &InternalOntology,
    subs: &Subsumers,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for i in 0..internal.vocabulary.num_classes() {
        let c = owl_dl_core::ClassId::new(u32::try_from(i).unwrap());
        for s in subs.subsumers_of(c) {
            // Synthetics live above the user vocabulary and have no IRI - skip them.
            if (s.index() as usize) < internal.vocabulary.num_classes() {
                out.push((
                    internal.vocabulary.class_iri(c).to_string(),
                    internal.vocabulary.class_iri(s).to_string(),
                ));
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Incremental-addition fixtures (Task 6)
// ---------------------------------------------------------------------------

/// Read a fixture's raw text.
///
/// # Panics
/// Panics if the fixture cannot be found or read.
pub(crate) fn fixture_text(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

/// Parse an in-memory OFN document into an [`InternalOntology`].
///
/// # Panics
/// Panics if the document cannot be parsed or converted.
pub(crate) fn load_ofn_str(src: &str) -> InternalOntology {
    let mut reader = Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse inline ontology: {e}\n---\n{src}\n---"));
    convert_ontology(&onto).unwrap_or_else(|e| panic!("convert inline ontology: {e}"))
}

/// Parse an OFN document and lower it into `internal` through
/// [`owl_dl_core::delta::convert_delta`], returning the new axiom indices.
///
/// # Panics
/// Panics if the document cannot be parsed or lowered.
pub(crate) fn apply_delta_ofn(internal: &mut InternalOntology, src: &str) -> Vec<usize> {
    let mut reader = Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse delta document: {e}\n---\n{src}\n---"));
    let components: Vec<_> = onto.iter().cloned().collect();
    owl_dl_core::delta::convert_delta(internal, &onto, &components)
        .unwrap_or_else(|e| panic!("lower delta document: {e:?}"))
}

/// `(base, union, added_axiom_indices)` for one monotone addition.
///
/// `union` is built from `base` by lowering a delta document through
/// `convert_delta` — the path a live session takes — so both revisions share
/// ONE vocabulary and ONE concept pool and every `ClassId` means the same thing
/// in each. Re-parsing two whole `.ofn` files would not give that:
/// `convert_ontology` sorts components before interning, so a one-axiom
/// difference permutes the class ids and no incremental comparison is possible.
///
/// Fixture convention: `"<stem>-new-class.ofn"` is a *delta document* applied
/// on top of `"<stem>.ofn"`. Every other name is a base ontology whose delta is
/// synthesized from its own vocabulary — see [`synthesized_delta`].
///
/// # Panics
/// Panics if either fixture is missing, unparseable, or cannot be lowered.
pub(crate) fn load_fixture_pair(name: &str) -> (InternalOntology, InternalOntology, Vec<usize>) {
    let (base_name, delta) = match name.strip_suffix("-new-class.ofn") {
        Some(stem) => (format!("{stem}.ofn"), Some(fixture_text(name))),
        None => (name.to_string(), None),
    };
    let base = load_fixture(&base_name);
    let delta = delta.unwrap_or_else(|| synthesized_delta(&base));
    let mut union = base.clone();
    let added = apply_delta_ofn(&mut union, &delta);
    assert!(
        !added.is_empty(),
        "the delta for {name} lowered to no axioms"
    );
    (base, union, added)
}

/// Class/role IRIs that are part of OWL itself rather than the fixture's own
/// vocabulary. `⊤`/`⊥` behave specially in the saturator, so a synthesized
/// delta must not name them.
fn is_builtin(iri: &str) -> bool {
    iri.starts_with("http://www.w3.org/2002/07/owl#")
        || iri.starts_with("http://www.w3.org/2000/01/rdf-schema#")
        || iri.starts_with("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
}

/// Build a delta over `base`'s own vocabulary that is guaranteed to be
/// *non-vacuous* and to exercise every rule shape `apply_additions` splices.
///
/// The classes are chosen by scanning the base closure for the first pair that
/// is not already related in either direction, so the delta always adds
/// something the base did not already entail. Four distinct class shapes are
/// emitted deliberately:
///
/// * `A ⊑ B` — a new `atomic_subsumption`
/// * `A ⊑ ∃r.(C ⊓ D)` — a new Tseitin synthetic (`C⊓D`), its two `F ⊑ C` /
///   `F ⊑ D` clauses, the `{C,D} → F` conjunctive trigger, and a new
///   existential fact
/// * `∃r.C ⊑ D` — a new existential trigger
/// * `B ⊑ C ⊓ D` — RHS conjunction over existing classes
///
/// A delta of only `A ⊑ B` would leave four of the six spliced rule vectors
/// untested, and the identity assertion would pass without proving much.
fn synthesized_delta(base: &InternalOntology) -> String {
    let closure = owl_dl_saturation::saturate(base);
    let named: Vec<owl_dl_core::ClassId> = (0..base.vocabulary.num_classes())
        .map(|i| owl_dl_core::ClassId::new(u32::try_from(i).unwrap()))
        .filter(|c| !is_builtin(base.vocabulary.class_iri(*c)))
        .collect();
    assert!(named.len() >= 4, "fixture needs at least 4 named classes");

    // First unrelated ordered pair, ascending — deterministic across runs.
    let (sub, sup) = named
        .iter()
        .flat_map(|&x| named.iter().map(move |&y| (x, y)))
        .find(|&(x, y)| x != y && !closure.contains(x, y) && !closure.contains(y, x))
        .expect("fixture must contain two unrelated named classes");
    let mid = named[named.len() / 3];
    let last = named[named.len() - 1];

    let role = (0..base.vocabulary.num_roles())
        .map(|i| owl_dl_core::RoleId::new(u32::try_from(i).unwrap()))
        .find(|r| !is_builtin(base.vocabulary.role_iri(*r)))
        .expect("fixture must declare at least one non-builtin role");

    let iri = |x: owl_dl_core::ClassId| base.vocabulary.class_iri(x).to_string();
    let role_iri = base.vocabulary.role_iri(role).to_string();
    format!(
        "Ontology(<http://rustdl.test/delta>\n\
         SubClassOf(<{a}> <{b}>)\n\
         SubClassOf(<{a}> ObjectSomeValuesFrom(<{r}> ObjectIntersectionOf(<{c}> <{d}>)))\n\
         SubClassOf(ObjectSomeValuesFrom(<{r}> <{c}>) <{d}>)\n\
         SubClassOf(<{b}> ObjectIntersectionOf(<{c}> <{d}>))\n\
         )\n",
        a = iri(sub),
        b = iri(sup),
        c = iri(mid),
        d = iri(last),
        r = role_iri,
    )
}
