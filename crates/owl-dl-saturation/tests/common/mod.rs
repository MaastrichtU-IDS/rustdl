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
