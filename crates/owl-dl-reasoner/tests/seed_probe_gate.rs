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

fn probe(local: &str, mode: u8, depth: usize, t: Option<Duration>) {
    let ont = load();
    let iri = format!("{WINE}{local}");
    let name = match mode {
        0 => "none",
        1 => "real-subsumers",
        _ => "GARBAGE-control",
    };
    let (result, stats, wall_ms, n_seeded) =
        owl_dl_reasoner::seed_probe(&ont, &iri, mode, depth, t)
            .expect("probe ok")
            .expect("IRI resolves");
    println!(
        "  sat({local}) seed={name} (n_seeded={n_seeded}): result={result:?} wall_ms={wall_ms:.0} \
         branches={} (disj={} merge={}) restores={} max_depth={}",
        stats.branches_taken,
        stats.disj_branches,
        stats.merge_branches,
        stats.restores,
        stats.max_branch_depth
    );
}

/// Control gate: none vs real-subsumers vs GARBAGE (same count of non-subsumers).
/// If GARBAGE collapses too ⇒ the win is MRV-reorder / root-label-count, not
/// saturation knowledge. If only real-subsumers collapses ⇒ the seed is the lever.
/// Also run with RUSTDL_MRV_ORDERING=0 to confirm the collapse isn't MRV-dependent.
#[test]
#[ignore = "viability control; RUSTDL_ADAPTIVE_BUDGET=0 --ignored --nocapture"]
fn wine_seed_collapse_probe() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let t = Some(Duration::from_secs(60));
            let depth = 256usize;
            println!("\n===== SweetWine =====");
            probe("SweetWine", 0, depth, t);
            probe("SweetWine", 1, depth, t);
            probe("SweetWine", 2, depth, t);
            println!("\n===== Zinfandel (the 952k-branch wall) =====");
            probe("Zinfandel", 0, depth, t);
            probe("Zinfandel", 1, depth, t);
            probe("Zinfandel", 2, depth, t);
        })
        .expect("spawn");
    child.join().expect("probe thread");
}
