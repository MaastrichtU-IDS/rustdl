//! Corpus-wide wedge profile: per-fixture branch decisions, backjump rate (L1
//! headroom), and context-independent UNSAT fraction (sound per-label-cache
//! headroom). Run: cargo test -p owl-dl-reasoner --test corpus_profile -- --ignored --nocapture
#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_tableau::hyper::profile;
use std::io::BufReader;
use std::time::Duration;

const CORPUS: &[(&str, &str)] = &[
    ("galen", "../../ontologies/external/galen.ofn"),
    ("notgalen", "../../ontologies/external/notgalen.ofn"),
    ("ro", "../../ontologies/real/ro.ofn"),
    ("sulo", "../../ontologies/real/sulo.ofn"),
    ("bibtex", "../../ontologies/real/bibtex.ofn"),
    ("alehif", "../../ontologies/external/alehif-test-classified.owx"),
    ("pizza", "../../ontologies/real/pizza.ofn"),
    ("ore-10908", "../../ontologies/external/ore-10908-sroiq-classified.owx"),
    ("ore-15672", "../../ontologies/external/ore-15672-shoin-classified.owx"),
    ("sio", "../../ontologies/real/sio.ofn"),
    ("wine", "../../ontologies/real/wine.ofn"),
];

fn load(path: &str) -> Option<SetOntology<RcStr>> {
    let f = std::fs::File::open(path).ok()?;
    let mut r = BufReader::new(f);
    if path.ends_with(".owx") {
        Some(read_owx(&mut r, ParserConfiguration::default()).ok()?.0)
    } else {
        Some(read_ofn(&mut r, ParserConfiguration::default()).ok()?.0)
    }
}

#[test]
#[ignore = "corpus-wide wedge profile; needs fixtures"]
fn corpus_wedge_profile() {
    println!(
        "\n{:<11} {:>11} {:>10} {:>9} {:>11} {:>9} {:>10}",
        "fixture", "branches", "backjumps", "bj-rate", "unsat-exh", "CI-uns", "cache-pot"
    );
    for (name, path) in CORPUS {
        let Some(ont) = load(path) else {
            println!("{name:<11}  (missing)");
            continue;
        };
        profile::reset();
        // 1s/pair budget; we only care about the wedge counters, not completeness.
        let _ = owl_dl_reasoner::classify_with_timeout(&ont, Duration::from_secs(1));
        let (br, bj, ue, ci) = profile::snapshot();
        let bjr = if br > 0 { 100.0 * bj as f64 / br as f64 } else { 0.0 };
        let cachep = if ue > 0 { 100.0 * ci as f64 / ue as f64 } else { 0.0 };
        println!(
            "{name:<11} {br:>11} {bj:>10} {bjr:>8.2}% {ue:>11} {ci:>9} {cachep:>8.1}%"
        );
    }
}
