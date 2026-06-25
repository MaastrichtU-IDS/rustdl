//! GATE: marker-saturator ⊔ failed-literal look-ahead (SP-A).
//!
//! Measures whether `RUSTDL_SAT_LOOKAHEAD` collapses wine's hard-class model
//! builds. The OFF baseline is already MRV-on (MRV is default-ON), so this
//! measures look-ahead ON *against the MRV baseline*, NOT the 66k raw.
//!
//! Two hard wine probes:
//!   - `sat(SweetWine)`              (MRV baseline ≈ 12 366 branches)
//!   - `sat(AlsatianWine ⊓ ¬AmericanWine)` (MRV baseline ≈ 1227 branches)
//!
//! Run OFF then ON, ignore wall, read `branches` + the `lookahead_*` counters:
//!   RUSTDL_ADAPTIVE_BUDGET=0 cargo test -p owl-dl-reasoner --release \
//!     --test sat_lookahead_gate -- --ignored --nocapture
//!   RUSTDL_ADAPTIVE_BUDGET=0 RUSTDL_SAT_LOOKAHEAD=1 cargo test -p owl-dl-reasoner --release \
//!     --test sat_lookahead_gate -- --ignored --nocapture
//!
//! GO  = SweetWine → low-hundreds-or-fewer AND Alsatian → tens-or-fewer, both Sat.
//! FLOOR = SweetWine stays within ~2× of the MRV baseline.
//! Spurious Unsat is NOT a GO.

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const OFN_PATH: &str = "/data/dumontier/rustdl/ontologies/real/wine.ofn";
const WINE: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn iri(local: &str) -> String {
    format!("{WINE}{local}")
}

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(OFN_PATH).unwrap_or_else(|e| {
        panic!("read {OFN_PATH}: {e} (run ./scripts/fetch-real-ontologies.sh)")
    });
    let mut r = Cursor::new(src.into_bytes());
    let (ont, _) = read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    ont
}

fn flag_state() -> &'static str {
    match std::env::var("RUSTDL_SAT_LOOKAHEAD") {
        Ok(v) if v != "0" && !v.is_empty() => "ON",
        _ => "OFF",
    }
}

fn report_sat(label: &str, local: &str, depth: usize, t: Option<Duration>) {
    let ont = load();
    let (result, s, wall_ms) = owl_dl_reasoner::sat_class_probe(&ont, &iri(local), depth, t)
        .expect("probe ok")
        .expect("IRI resolves to a named class");
    println!("\n===== {label}  [look-ahead {}] =====", flag_state());
    println!("  result={result:?}  wall_ms={wall_ms:.0}");
    println!(
        "  branches={} (disj={} merge={})  restores={}  max_depth={}/{}",
        s.branches_taken, s.disj_branches, s.merge_branches, s.restores, s.max_branch_depth, depth
    );
    println!(
        "  lookahead: calls={} dropped={} forced_single={}",
        s.lookahead_calls, s.lookahead_dropped, s.lookahead_forced_single
    );
}

fn report_pair(label: &str, sub: &str, sup: &str, depth: usize, t: Option<Duration>) {
    let ont = load();
    let (result, s, wall_ms) =
        owl_dl_reasoner::decide_pair_probe(&ont, &iri(sub), &iri(sup), depth, t)
            .expect("probe ok")
            .expect("both IRIs resolve to named classes");
    println!("\n===== {label}  [look-ahead {}] =====", flag_state());
    println!("  result={result:?}  wall_ms={wall_ms:.0}");
    println!(
        "  branches={} (disj={} merge={})  restores={}  max_depth={}/{}",
        s.branches_taken, s.disj_branches, s.merge_branches, s.restores, s.max_branch_depth, depth
    );
    println!(
        "  lookahead: calls={} dropped={} forced_single={}",
        s.lookahead_calls, s.lookahead_dropped, s.lookahead_forced_single
    );
}

/// The gate. Adaptive budget OFF (set externally), big stack for deep recursion.
#[test]
#[ignore = "gate measurement; run with --ignored --nocapture, flag set externally"]
fn wine_lookahead_branch_collapse() {
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let t = Some(Duration::from_secs(60));
            let depth = 256usize;
            // sat(Alsatian ⊓ ¬American): correctly NOT a subsumption → Sat expected.
            report_pair(
                "sat(AlsatianWine ⊓ ¬AmericanWine)",
                "AlsatianWine",
                "AmericanWine",
                depth,
                t,
            );
            // sat(SweetWine): satisfiable → Sat expected.
            report_sat("sat(SweetWine)", "SweetWine", depth, t);
        })
        .expect("spawn big-stack thread");
    child.join().expect("gate thread");
}
