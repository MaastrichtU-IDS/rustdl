//! Differential test: every fixture in `fixtures/` is parsed, run
//! through rustdl's [`owl_dl_reasoner::is_class_satisfiable`], and
//! the verdict checked against the manifest's `expected` value.
//!
//! The ROBOT/HermiT oracle in `docker/robot/` already validates the
//! manifest values against an external reasoner. This test closes
//! the loop on the rustdl side: any drift between rustdl and
//! manifest fails the suite.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

use owl_dl_reasoner::is_class_satisfiable;

#[derive(Debug)]
struct ManifestEntry {
    file: String,
    test_class: String,
    expected: bool,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Tiny TOML-array reader tuned to the manifest's known layout. Avoids
/// pulling in a TOML crate just for this; the manifest is fixed-shape
/// and authored alongside this code.
fn read_manifest() -> Vec<ManifestEntry> {
    let text =
        std::fs::read_to_string(fixtures_dir().join("manifest.toml")).expect("read manifest");
    let mut entries = Vec::new();
    let mut current: Option<(Option<String>, Option<String>, Option<String>)> = None;
    let flush = |slot: &mut Option<(Option<String>, Option<String>, Option<String>)>,
                 out: &mut Vec<ManifestEntry>| {
        if let Some((file, test_class, expected)) = slot.take() {
            let file = file.expect("manifest entry missing file");
            let test_class = test_class.expect("manifest entry missing test_class");
            let expected = expected.expect("manifest entry missing expected");
            let expected = match expected.as_str() {
                "sat" => true,
                "unsat" => false,
                other => panic!("unknown verdict in manifest: {other}"),
            };
            out.push(ManifestEntry {
                file,
                test_class,
                expected,
            });
        }
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[fixture]]" {
            flush(&mut current, &mut entries);
            current = Some((None, None, None));
        } else if let Some((file, test_class, expected)) = current.as_mut() {
            if let Some(value) = strip_kv(line, "file") {
                *file = Some(value);
            } else if let Some(value) = strip_kv(line, "test_class") {
                *test_class = Some(value);
            } else if let Some(value) = strip_kv(line, "expected") {
                *expected = Some(value);
            }
        }
    }
    flush(&mut current, &mut entries);
    entries
}

/// Extract the value from a `key = "value"` line if `line` starts
/// with the given key.
fn strip_kv(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn parse_fixture(name: &str) -> SetOntology<RcStr> {
    let path = fixtures_dir().join(name);
    let path_display = path.display();
    let file = File::open(&path).unwrap_or_else(|e| panic!("open {path_display}: {e}"));
    let mut reader = BufReader::new(file);
    let (ontology, _prefixes) =
        read(&mut reader, ParserConfiguration::default()).expect("parse fixture");
    ontology
}

#[test]
fn rustdl_agrees_with_manifest() {
    let entries = read_manifest();
    assert!(!entries.is_empty(), "manifest is empty");
    let mut failures: Vec<String> = Vec::new();
    for entry in &entries {
        let onto = parse_fixture(&entry.file);
        match is_class_satisfiable(&onto, &entry.test_class) {
            Ok(actual) if actual == entry.expected => {}
            Ok(actual) => {
                failures.push(format!(
                    "{}: expected={} rustdl={}",
                    entry.file,
                    label(entry.expected),
                    label(actual)
                ));
            }
            Err(e) => {
                failures.push(format!("{}: rustdl errored: {e}", entry.file));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "rustdl disagreed with manifest on {} fixture(s):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

fn label(b: bool) -> &'static str {
    if b { "sat" } else { "unsat" }
}
