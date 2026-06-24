//! THROWAWAY Phase-0 combination spike gate (does NOT merge). Run TWICE (OFF then ON):
//!   cargo test -p owl-dl-reasoner --test combo_spike_gate -- --ignored --nocapture
//!   RUSTDL_COMBO_SPIKE=1 cargo test -p owl-dl-reasoner --test combo_spike_gate -- --ignored --nocapture
#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const WINE: &str = "../../ontologies/real/wine.ofn";
const NS: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(WINE).unwrap_or_else(|e| panic!("read {WINE}: {e}"));
    read_ofn(&mut Cursor::new(src.into_bytes()), ParserConfiguration::default()).expect("parse").0
}

#[test]
#[ignore = "throwaway combo-spike gate; run with/without RUSTDL_COMBO_SPIKE"]
fn combo_spike_wine_collapse() {
    unsafe { std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0"); }
    let child = std::thread::Builder::new().stack_size(2 * 1024 * 1024 * 1024).spawn(|| {
        let ont = load();
        let on = std::env::var("RUSTDL_COMBO_SPIKE").as_deref() == Ok("1");
        let dl = Some(Duration::from_secs(60));
        println!("##### COMBO-SPIKE GATE combo_on={on} #####");
        // sat(SweetWine)
        if let Some((res, s, ms)) = owl_dl_reasoner::sat_class_probe(&ont, &format!("{NS}SweetWine"), 256, dl).expect("probe") {
            println!("sat(SweetWine)            verdict={res:?} wall_ms={ms:.0} branches={} restores={} disj={} merge={}",
                s.branches_taken, s.restores, s.disj_branches, s.merge_branches);
        }
        // sat(Alsatian ⊓ ¬American)
        if let Some((res, s, ms)) = owl_dl_reasoner::decide_pair_probe(&ont, &format!("{NS}AlsatianWine"), &format!("{NS}AmericanWine"), 256, dl).expect("probe") {
            println!("sat(Alsatian⊓¬American)   verdict={res:?} wall_ms={ms:.0} branches={} restores={} disj={} merge={}",
                s.branches_taken, s.restores, s.disj_branches, s.merge_branches);
        }
        println!("##### END combo_on={on} #####");
    }).expect("spawn");
    child.join().expect("thread");
}
