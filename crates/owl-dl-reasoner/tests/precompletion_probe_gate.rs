//! GATE: SP3 Phase-1 precompletion-graph viability probe.
//!
//! Does seeding the saturation's DERIVED ∃-facts (`Zinfandel ⊑ ∃hasColor.{Red}`)
//! collapse a hard non-collapsing wine class — beyond the SP2 named-seed ~7.5%
//! ceiling? modes: 0 none / 1 named-only (= SP2 seed) / 2 named+∃ / 3 garbage-∃.
//! GO iff mode-2 collapses order-of-mag below mode-1 AND verdict stays Sat AND
//! mode-3 (garbage) does NOT collapse-to-correct-Sat.
//!
//! Run: RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner --release \
//!        --test precompletion_probe_gate -- --ignored --nocapture

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
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse").0
}

fn probe(local: &str, mode: u8, depth: usize, t: Option<Duration>) {
    let ont = load();
    let iri = format!("{WINE}{local}");
    let name = match mode {
        0 => "none      ",
        1 => "named     ",
        2 => "named+EXIST",
        _ => "GARBAGE    ",
    };
    match owl_dl_reasoner::precompletion_probe(&ont, &iri, mode, depth, t).expect("probe ok") {
        None => println!("  sat({local}) [{name}]: IRI not a named class"),
        Some((result, stats, wall_ms, n_ex)) => println!(
            "  sat({local}) [{name}] n_exist={n_ex}: {result:?} wall={wall_ms:.0}ms \
             branches={} (disj={} merge={}) restores={} max_depth={}",
            stats.branches_taken,
            stats.disj_branches,
            stats.merge_branches,
            stats.restores,
            stats.max_branch_depth
        ),
    }
}

#[test]
#[ignore = "viability gate; RUSTDL_ADAPTIVE_BUDGET=0 --ignored --nocapture"]
fn precompletion_exists_collapse() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let t = Some(Duration::from_secs(60));
            let depth = 256usize;
            // CabernetFranc: the documented hard class (sat DNF'd at ~1.49M branches).
            // All four modes — the decisive comparison.
            println!("\n===== CabernetFranc (the documented hard wine class) =====");
            for mode in [0u8, 1, 2, 3] {
                probe("CabernetFranc", mode, depth, t);
            }
            // Breadth: a few other hard candidates, named vs named+∃.
            for cls in ["CabernetSauvignon", "Merlot", "Chardonnay", "WhiteWine"] {
                println!("\n===== {cls} (named vs named+∃) =====");
                probe(cls, 1, depth, t);
                probe(cls, 2, depth, t);
            }
        })
        .expect("spawn");
    child.join().expect("probe thread");
}
