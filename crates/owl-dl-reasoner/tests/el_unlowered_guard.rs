//! Layer 2 of the pure-EL guard, over the repository's own OWL fixtures.
//!
//! Every `.ofn` under the roots below (CI's checkout holds only tracked files; a local
//! untracked one is checked too) is converted, under the process's `RUSTDL_*` flags, and wherever `classify`'s or
//! `realize`'s saturation fast path certifies it complete, the EL lowering must drop
//! nothing (`certified_unlowered`). No oracle is involved: a non-empty result means the
//! gate certified a construct the saturator never received as a rule, which is the D10
//! class (#435/#436 and their predecessors). `el_generated_guard.rs` runs the same check
//! on the frozen generated sample.
//!
//! Files that do not parse or convert are counted and listed, not failed: some fixtures
//! are deliberately malformed. A floor on the certified count keeps the test from going
//! quiet. A known live drop is listed in `ALLOWED` with a link to its issue draft, so the
//! guard is not blocked by the bug it found; an `ALLOWED` entry that no longer fires
//! fails the test, so the list cannot go stale.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const ROOTS: &[&str] = &["crates", "scripts", "tests", "docs", "bench-corpus"];
const SKIP_DIRS: &[&str] = &["target", "el_generated", ".git"];
const MAX_BYTES: u64 = 256 * 1024;
/// At 17445f03: 261 fixtures, 113 classify-certified, 100 realize-certified.
const CERTIFIED_FLOOR: usize = 100;
const REALIZE_CERTIFIED_FLOOR: usize = 90;

/// `(path relative to the workspace root, site, why)`.
const ALLOWED: &[(&str, &str, &str)] = &[];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name();
        if kind.is_dir() {
            if !SKIP_DIRS.iter().any(|s| name == *s) {
                collect(&path, out);
            }
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "ofn") {
            out.push(path);
        }
    }
}

#[test]
fn every_certified_fixture_lowers_completely() {
    let root = workspace();
    let mut files = Vec::new();
    for r in ROOTS {
        collect(&root.join(r), &mut files);
    }
    files.sort();
    let rel = |p: &Path| {
        p.strip_prefix(&root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };

    enum Outcome {
        Skipped(String),
        Checked {
            classify_certified: bool,
            realize_certified: bool,
            reports: Vec<(&'static str, owl_dl_reasoner::UnloweredReport)>,
        },
    }
    let outcomes: Vec<(String, Outcome)> = files
        .par_iter()
        .map(|path| {
            let name = rel(path);
            let skip = |why: &str| Outcome::Skipped(format!("{name} ({why})"));
            if std::fs::metadata(path).map_or(true, |m| m.len() > MAX_BYTES) {
                return (name.clone(), skip("over the size cap"));
            }
            let Ok(src) = std::fs::read_to_string(path) else {
                return (name.clone(), skip("not UTF-8"));
            };
            let parsed: Result<(SetOntology<RcStr>, _), _> =
                read_ofn(&mut Cursor::new(src), ParserConfiguration::default());
            let Ok((onto, _)) = parsed else {
                return (name.clone(), skip("does not parse"));
            };
            let Ok(found) = owl_dl_reasoner::certified_unlowered(&onto) else {
                return (name.clone(), skip("does not convert"));
            };
            let reports = found
                .classify
                .into_iter()
                .map(|u| ("classify", u))
                .chain(found.realize.into_iter().map(|u| ("realize", u)))
                .collect();
            let outcome = Outcome::Checked {
                classify_certified: found.classify_certified,
                realize_certified: found.realize_certified,
                reports,
            };
            (name, outcome)
        })
        .collect();

    let (mut skipped, mut certified, mut realize_certified) = (Vec::new(), 0usize, 0usize);
    let mut hits: BTreeSet<(String, &'static str)> = BTreeSet::new();
    let mut failures = Vec::new();
    for (name, outcome) in outcomes {
        match outcome {
            Outcome::Skipped(why) => skipped.push(why),
            Outcome::Checked {
                classify_certified,
                realize_certified: rc,
                reports,
            } => {
                certified += usize::from(classify_certified);
                realize_certified += usize::from(rc);
                for (surface, u) in reports {
                    if ALLOWED.iter().any(|(f, s, _)| *f == name && *s == u.site) {
                        hits.insert((name.clone(), u.site));
                    } else {
                        failures.push(format!(
                            "{name}: {surface} axiom {} {}: {}",
                            u.axiom, u.site, u.concept
                        ));
                    }
                }
            }
        }
    }

    let stale: Vec<_> = ALLOWED
        .iter()
        .filter(|(f, s, _)| !hits.contains(&((*f).to_owned(), *s)))
        .collect();
    let summary = format!(
        "{} fixtures, {certified} classify-certified, {realize_certified} realize-certified, \
         {} skipped: {skipped:?}",
        files.len(),
        skipped.len()
    );
    assert!(
        failures.is_empty(),
        "the EL lowering drops constructs the fast path certifies ({summary}):\n{}",
        failures.join("\n")
    );
    assert!(
        stale.is_empty(),
        "ALLOWED entries that no longer fire, remove them: {stale:?}"
    );
    assert!(
        certified >= CERTIFIED_FLOOR && realize_certified >= REALIZE_CERTIFIED_FLOOR,
        "only {certified} classify- / {realize_certified} realize-certified fixtures (floors \
         {CERTIFIED_FLOOR} / {REALIZE_CERTIFIED_FLOOR}): the guard has gone quiet. {summary}"
    );
    eprintln!("{summary}");
}

fn parse(axioms: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://rustdl.test/>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://rustdl.test/o>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) \
         Declaration(ObjectProperty(:r))\n{axioms}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("ofn parses")
        .0
}

/// The live drop the layer-2 survey found, `⊤ ⊑ ∃r.B` losing its `∃` (#445), is gone:
/// certified, and nothing dropped. That a certified drop IS reported end to end is
/// pinned in `el_unlowered_guard_bot_filler_off.rs`.
#[test]
fn the_top_lhs_existential_now_lowers() {
    let found = owl_dl_reasoner::certified_unlowered(&parse(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) \
         SubClassOf(ObjectSomeValuesFrom(:r :B) :X)",
    ))
    .expect("converts");
    assert!(found.classify_certified);
    assert!(found.classify.is_empty(), "{found:?}");
}

/// Not certified, so not reported: the same drop on an input outside the fast-path
/// fragment is the tableau's business.
#[test]
fn drops_on_uncertified_inputs_are_not_reported() {
    let found = owl_dl_reasoner::certified_unlowered(&parse(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) \
         SubClassOf(:A ObjectAllValuesFrom(:r :X))",
    ))
    .expect("converts");
    assert!(!found.classify_certified);
    assert!(found.classify.is_empty(), "{found:?}");
}

/// The conversion-derived `∃r.⊤ ⊑ ≤1 r` of a functional role is admitted by the gate and
/// deliberately not lowered (the functional bitset enforces it): no report.
#[test]
fn the_derived_functional_max_gci_is_exempt() {
    let found = owl_dl_reasoner::certified_unlowered(&parse(
        "FunctionalObjectProperty(:r) SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
    ))
    .expect("converts");
    assert!(found.classify_certified, "{found:?}");
    assert!(found.classify.is_empty(), "{found:?}");
    // Not vacuous: the raw lowering does record the derived GCI; the gate filter drops it.
    let internal = owl_dl_core::convert::convert_ontology(&parse(
        "FunctionalObjectProperty(:r) SubClassOf(:A ObjectSomeValuesFrom(:r :B))",
    ))
    .expect("converts");
    let raw: Vec<_> = owl_dl_saturation::unlowered_el_constructs(&internal)
        .into_iter()
        .map(|u| u.site)
        .collect();
    assert_eq!(raw, ["domain-heads-lossy"]);
}
