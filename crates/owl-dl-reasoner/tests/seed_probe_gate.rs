//! VIABILITY PROBE: does seeding the wedge root with the complete all-model
//! saturated subsumer set collapse wine's disjunctive model-search branch count?
//!
//! The load-bearing assumption of the coupled-saturation/precompletion build.
//! The saturator is already closure-complete on wine (653, tableau=0), so every
//! per-pair wedge call is a REFUTATION (model search). If seeding all-model
//! subsumers into the root does NOT drop branches, the SP1→SP2 saturation chain
//! cannot collapse wine (the branches are genuine value-assignment choices, not
//! entailed facts) — killing the build before increment-3.
//!
//! Run:
//!   RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner --release \
//!     --test seed_probe_gate -- --ignored --nocapture

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const OFN_PATH: &str = "/data/dumontier/rustdl/ontologies/real/wine.ofn";
const WINE: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(OFN_PATH).unwrap();
    let mut r = Cursor::new(src.into_bytes());
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse")
        .0
}

fn probe(local: &str, seed: bool, depth: usize, t: Option<Duration>) {
    let ont = load();
    let iri = format!("{WINE}{local}");
    let (result, stats, wall_ms, n_seeded) =
        owl_dl_reasoner::seed_probe(&ont, &iri, seed, depth, t)
            .expect("probe ok")
            .expect("IRI resolves");
    println!(
        "  sat({local}) seed={seed} (n_seeded={n_seeded}): result={result:?} wall_ms={wall_ms:.0} \
         branches={} (disj={} merge={}) restores={} max_depth={}",
        stats.branches_taken,
        stats.disj_branches,
        stats.merge_branches,
        stats.restores,
        stats.max_branch_depth
    );
}

#[test]
#[ignore = "viability probe; RUSTDL_ADAPTIVE_BUDGET=0 --ignored --nocapture"]
fn wine_seed_collapse_probe() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let t = Some(Duration::from_secs(60));
            let depth = 256usize;
            // Fast contrast classes first.
            println!("\n===== SweetWine =====");
            probe("SweetWine", false, depth, t);
            probe("SweetWine", true, depth, t);
            // The wall.
            println!("\n===== Zinfandel (the 952k-branch wall) =====");
            probe("Zinfandel", false, depth, t);
            probe("Zinfandel", true, depth, t);
        })
        .expect("spawn");
    child.join().expect("probe thread");
}
