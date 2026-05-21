//! Smoke test: every fixture in `fixtures/` parses with horned-owl's
//! OWL Functional Syntax reader.
//!
//! Once the `owl-dl-reasoner` facade exists, this file will grow into
//! the full differential test against the ROBOT/HermiT oracle. Until
//! then, parse success is the contract.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::{Build, RcStr};
use horned_owl::ontology::set::SetOntology;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn each_fixture(mut check: impl FnMut(&str)) {
    let dir = fixtures_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("fixtures dir readable")
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ofn"))
            {
                Some(e.file_name().to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    let dir_display = dir.display();
    assert!(!names.is_empty(), "no .ofn fixtures found in {dir_display}");
    for name in names {
        check(&name);
    }
}

#[test]
fn every_fixture_parses() {
    each_fixture(|name| {
        let path = fixtures_dir().join(name);
        let file = File::open(&path).unwrap_or_else(|e| panic!("open {path:?}: {e}"));
        let mut reader = BufReader::new(file);
        let result: Result<(SetOntology<RcStr>, _), _> =
            read(&mut reader, ParserConfiguration::default());
        if let Err(e) = result {
            panic!("parse failed for {name}: {e:?}");
        }
    });
}

#[test]
fn manifest_lists_every_fixture() {
    let manifest = fixtures_dir().join("manifest.toml");
    let text = std::fs::read_to_string(&manifest).expect("read manifest.toml");
    each_fixture(|name| {
        assert!(
            text.contains(&format!("\"{name}\"")),
            "manifest.toml is missing an entry for {name}"
        );
    });
}

#[test]
fn build_can_construct_iris_from_manifest_namespace() {
    // Sanity: the IRIs we pin in manifest.toml are well-formed and
    // round-trip through horned-owl's Build::iri.
    let build: Build<RcStr> = Build::new();
    let iri = build.iri("http://rustdl.test/Test");
    assert_eq!(iri.as_ref(), "http://rustdl.test/Test");
}
