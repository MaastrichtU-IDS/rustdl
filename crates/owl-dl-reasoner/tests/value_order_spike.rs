//! THROWAWAY value-ordering spike (does NOT merge). MRV is default-ON; this
//! measures whether adding sound det-look-ahead value-ordering (RUSTDL_VALUE_ORDER:
//! branch non-clashing disjuncts first, clashing last, NO DROP) collapses
//! SweetWine's residual (MRV-only baseline ~12366 branches). Run twice:
//!   cargo test -p owl-dl-reasoner --test value_order_spike -- --ignored --nocapture
//!   RUSTDL_VALUE_ORDER=1 cargo test -p owl-dl-reasoner --test value_order_spike -- --ignored --nocapture

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
    read_ofn(&mut Cursor::new(src.into_bytes()), ParserConfiguration::default())
        .expect("parse")
        .0
}

#[test]
#[ignore = "throwaway value-ordering spike; run with/without RUSTDL_VALUE_ORDER (MRV default-ON)"]
fn value_order_wine_residual() {
    unsafe {
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let ont = load();
            let vo = std::env::var("RUSTDL_VALUE_ORDER").is_ok();
            let dl = Some(Duration::from_secs(60));
            println!("##### VALUE-ORDER SPIKE value_order={vo} (MRV default-ON) #####");
            if let Some((res, s, ms)) =
                owl_dl_reasoner::sat_class_probe(&ont, &format!("{NS}SweetWine"), 256, dl).expect("p")
            {
                println!(
                    "sat(SweetWine)            verdict={res:?} wall_ms={ms:.0} branches={} restores={} disj={} merge={}",
                    s.branches_taken, s.restores, s.disj_branches, s.merge_branches
                );
            }
            if let Some((res, s, ms)) = owl_dl_reasoner::decide_pair_probe(
                &ont,
                &format!("{NS}AlsatianWine"),
                &format!("{NS}AmericanWine"),
                256,
                dl,
            )
            .expect("p")
            {
                println!(
                    "sat(Alsatian⊓¬American)   verdict={res:?} wall_ms={ms:.0} branches={} restores={} disj={} merge={}",
                    s.branches_taken, s.restores, s.disj_branches, s.merge_branches
                );
            }
            println!("##### END value_order={vo} #####");
        })
        .expect("spawn");
    child.join().expect("thread");
}
